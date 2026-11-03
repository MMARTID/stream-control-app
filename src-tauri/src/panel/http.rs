//! Servidor del panel web: página, emparejamiento, comandos y WebSocket de estado y capturas.
//!
//! Reglas (README §5.10):
//! - Solo atiende direcciones de la red local (RF-44) y solo si la cabecera `Host` es una IP,
//!   `localhost` o el nombre de este equipo (frena el «DNS rebinding»).
//! - Todo lo que sale hacia el móvil pasa por `WebState` (RF-45).
//! - Iniciar o parar el stream exige dos pasos en el servidor y el permiso activado en la app (RF-46, RF-49).
//! - Cada acción queda en el registro (RF-47).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{
        ConnectInfo, DefaultBodyLimit, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{interval, timeout};

use super::{
    audit::Audit,
    auth::{Auth, AuthError, Device},
    tls::{hostname, is_lan},
    web_state::WebState,
};
use crate::{control::Control, preview::Interest};

const CONFIRM_TTL: Duration = Duration::from_secs(10);
const WS_IDLE_LIMIT: Duration = Duration::from_secs(70);

struct Confirm {
    device_id: String,
    action: StreamAction,
    expires: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamAction {
    Start,
    Stop,
}

impl StreamAction {
    fn verb(self) -> &'static str {
        match self {
            Self::Start => "iniciar",
            Self::Stop => "parar",
        }
    }
}

/// Estado compartido por todas las peticiones.
pub struct Shared {
    pub control: Arc<Control>,
    pub auth: Arc<Auth>,
    pub audit: Arc<Audit>,
    /// Conexiones WebSocket abiertas por dispositivo (para mostrar «conectado» en el escritorio).
    live: Mutex<HashMap<String, usize>>,
    confirms: Mutex<HashMap<String, Confirm>>,
    connections: AtomicU64,
    host: Option<String>,
}

impl Shared {
    pub fn new(control: Arc<Control>, auth: Arc<Auth>, audit: Arc<Audit>) -> Arc<Self> {
        Arc::new(Self {
            control,
            auth,
            audit,
            live: Mutex::new(HashMap::new()),
            confirms: Mutex::new(HashMap::new()),
            connections: AtomicU64::new(0),
            host: hostname().map(|h| h.to_lowercase()),
        })
    }

    pub fn online_devices(&self) -> Vec<String> {
        self.live.lock().unwrap().iter().filter(|(_, n)| **n > 0).map(|(id, _)| id.clone()).collect()
    }

    fn set_live(&self, device_id: &str, delta: isize) {
        let mut live = self.live.lock().unwrap();
        let count = live.entry(device_id.to_owned()).or_insert(0);
        *count = count.saturating_add_signed(delta);
        if *count == 0 {
            live.remove(device_id);
        }
        drop(live);
        self.auth.notify();
    }
}

pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/", get(|| asset("index.html")))
        .route("/app.js", get(|| asset("app.js")))
        .route("/style.css", get(|| asset("style.css")))
        .route("/manifest.webmanifest", get(|| asset("manifest.webmanifest")))
        .route("/icon.svg", get(|| asset("icon.svg")))
        .route("/api/pair/start", post(pair_start))
        .route("/api/pair/finish", post(pair_finish))
        .route("/api/me", get(me))
        .route("/api/cmd", post(command))
        .route("/ws", get(ws_upgrade))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(Arc::clone(&shared), guard))
        .with_state(shared)
}

// ---- Protección común ----

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn host_allowed(headers: &HeaderMap, own_host: Option<&str>) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return false };
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map_or(host, |(h, _)| h)
    }
    .to_lowercase();
    if name.parse::<IpAddr>().is_ok() || name == "localhost" {
        return true;
    }
    own_host.is_some_and(|own| name == own || name == format!("{own}.local"))
}

/// Si el navegador manda `Origin`, debe ser la misma página que sirve este servidor.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else { return true };
    let (Ok(origin), Some(host)) = (origin.to_str(), headers.get(header::HOST).and_then(|h| h.to_str().ok())) else {
        return false;
    };
    origin.split_once("://").is_some_and(|(_, rest)| rest.eq_ignore_ascii_case(host))
}

async fn guard(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !is_lan(addr.ip()) {
        return error(StatusCode::FORBIDDEN, "El panel solo está disponible en la red local");
    }
    if !host_allowed(req.headers(), shared.host.as_deref()) {
        return error(StatusCode::MISDIRECTED_REQUEST, "Dirección no admitida: usa la IP del equipo");
    }
    let is_api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self' ws: wss:; \
             frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

// ---- Página ----

async fn asset(name: &'static str) -> Response {
    let (body, mime) = match name {
        "index.html" => (include_str!("../../web/index.html"), "text/html; charset=utf-8"),
        "app.js" => (include_str!("../../web/app.js"), "text/javascript; charset=utf-8"),
        "style.css" => (include_str!("../../web/style.css"), "text/css; charset=utf-8"),
        "manifest.webmanifest" => (include_str!("../../web/manifest.webmanifest"), "application/manifest+json"),
        "icon.svg" => (include_str!("../../web/icon.svg"), "image/svg+xml"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap()
}

// ---- Emparejamiento ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairStart {
    password: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairFinish {
    pairing_id: String,
    code: String,
}

fn auth_status(e: &AuthError) -> StatusCode {
    match e {
        AuthError::Locked(_) | AuthError::TooManyPairings => StatusCode::TOO_MANY_REQUESTS,
        AuthError::BadPassword | AuthError::BadCode | AuthError::PairingExpired => StatusCode::UNAUTHORIZED,
        AuthError::NoPassword | AuthError::Invalid(_) => StatusCode::BAD_REQUEST,
    }
}

async fn pair_start(
    State(sh): State<Arc<Shared>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairStart>,
) -> Response {
    if !origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origen no admitido");
    }
    let ip = addr.ip();
    // Argon2 es lento a propósito: se hace fuera del hilo del servidor.
    let auth = Arc::clone(&sh.auth);
    let name = body.name.clone();
    let result = tokio::task::spawn_blocking(move || auth.start_pairing(ip, &body.password, &body.name)).await.unwrap();
    match result {
        Ok(id) => {
            sh.audit.record(&name, &ip.to_string(), "Emparejamiento", "Contraseña correcta; falta el código", true);
            Json(json!({ "pairingId": id })).into_response()
        }
        Err(e) => {
            sh.audit.record(&name, &ip.to_string(), "Emparejamiento", &e.to_string(), false);
            error(auth_status(&e), e.to_string())
        }
    }
}

async fn pair_finish(
    State(sh): State<Arc<Shared>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairFinish>,
) -> Response {
    if !origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origen no admitido");
    }
    let ip = addr.ip();
    match sh.auth.finish_pairing(ip, &body.pairing_id, &body.code) {
        Ok((token, device)) => {
            sh.audit.record(&device.name, &ip.to_string(), "Emparejamiento", "Dispositivo añadido", true);
            Json(json!({ "token": token, "name": device.name })).into_response()
        }
        Err(e) => {
            sh.audit.record("—", &ip.to_string(), "Emparejamiento", &e.to_string(), false);
            error(auth_status(&e), e.to_string())
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

fn authenticate(sh: &Shared, headers: &HeaderMap, ip: IpAddr) -> Result<Device, Response> {
    bearer(headers)
        .and_then(|token| sh.auth.authenticate(token, ip))
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "Dispositivo no autorizado: vuelve a emparejarlo"))
}

async fn me(State(sh): State<Arc<Shared>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    match authenticate(&sh, &headers, addr.ip()) {
        Ok(device) => Json(json!({ "name": device.name })).into_response(),
        Err(res) => res,
    }
}

// ---- Comandos ----

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    LaunchScene { scene: String },
    Back,
    SwitchCamera { id: String },
    Mic { muted: bool },
    ToggleMic,
    Source { id: i64, enabled: bool },
    StreamArm { action: StreamAction },
    StreamConfirm { confirm: String },
}

async fn command(
    State(sh): State<Arc<Shared>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(cmd): Json<Command>,
) -> Response {
    if !origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origen no admitido");
    }
    let ip = addr.ip();
    let device = match authenticate(&sh, &headers, ip) {
        Ok(d) => d,
        Err(res) => return res,
    };
    let (action, detail) = describe(&sh, &cmd);
    match execute(&sh, &device, cmd).await {
        Ok(body) => {
            sh.audit.record(&device.name, &ip.to_string(), action, &detail, true);
            Json(body).into_response()
        }
        Err(message) => {
            sh.audit.record(&device.name, &ip.to_string(), action, &format!("{detail} — {message}"), false);
            error(StatusCode::BAD_REQUEST, message)
        }
    }
}

fn describe(sh: &Shared, cmd: &Command) -> (&'static str, String) {
    match cmd {
        Command::LaunchScene { scene } => ("Escena", scene.clone()),
        Command::Back => ("Escena", "volver a la anterior".into()),
        Command::SwitchCamera { id } => {
            let name = sh.control.snapshot().cameras.iter().find(|c| &c.id == id).map(|c| c.name.clone());
            ("Cámara", name.unwrap_or_else(|| id.clone()))
        }
        Command::Mic { muted } => ("Micro", if *muted { "silenciar" } else { "activar" }.into()),
        Command::ToggleMic => ("Micro", "alternar".into()),
        Command::Source { id, enabled } => {
            let name = sh.control.snapshot().sources.iter().find(|s| s.id == *id).map(|s| s.name.clone());
            ("Fuente", format!("{} → {}", name.unwrap_or_else(|| id.to_string()), if *enabled { "visible" } else { "oculta" }))
        }
        Command::StreamArm { action } => ("Stream (pedido)", format!("{} el directo", action.verb())),
        Command::StreamConfirm { .. } => ("Stream", "confirmación".into()),
    }
}

async fn execute(sh: &Shared, device: &Device, cmd: Command) -> Result<Value, String> {
    let control = &sh.control;
    match cmd {
        Command::LaunchScene { scene } => {
            let snapshot = control.snapshot();
            if !snapshot.scenes.contains(&scene) {
                return Err("Esa escena no existe en OBS".into());
            }
            let quick = snapshot.settings.quick_scenes.iter().any(|q| q.scene == scene);
            if snapshot.settings.panel.only_quick_scenes && !quick {
                return Err("El panel solo puede lanzar escenas rápidas".into());
            }
            control.launch_scene(&scene).await?;
        }
        Command::Back => control.go_back().await?,
        Command::SwitchCamera { id } => control.switch_camera(&id).await?,
        Command::Mic { muted } => control.set_mic_muted(muted).await?,
        Command::ToggleMic => control.toggle_mic().await?,
        Command::Source { id, enabled } => control.set_source_enabled(id, enabled).await?,
        Command::StreamArm { action } => {
            check_stream_allowed(sh)?;
            let streaming = control.snapshot().stream.active;
            match action {
                StreamAction::Start if streaming => return Err("Ya estás en directo".into()),
                StreamAction::Stop if !streaming => return Err("No estás en directo".into()),
                _ => {}
            }
            let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
            let mut confirms = sh.confirms.lock().unwrap();
            confirms.retain(|_, c| c.expires > Instant::now());
            confirms.insert(
                token.clone(),
                Confirm { device_id: device.id.clone(), action, expires: Instant::now() + CONFIRM_TTL },
            );
            return Ok(json!({ "confirm": token, "expiresIn": CONFIRM_TTL.as_secs() }));
        }
        Command::StreamConfirm { confirm } => {
            check_stream_allowed(sh)?;
            // Solo se consume si es de este dispositivo: otro móvil no puede gastar mi confirmación.
            let pending = {
                let mut confirms = sh.confirms.lock().unwrap();
                match confirms.get(&confirm) {
                    Some(c) if c.device_id == device.id && c.expires > Instant::now() => confirms.remove(&confirm),
                    _ => None,
                }
            };
            let Some(pending) = pending else {
                return Err("La confirmación ha caducado. Vuelve a pulsar el botón".into());
            };
            match pending.action {
                StreamAction::Start => control.start_stream().await?,
                StreamAction::Stop => control.stop_stream().await?,
            }
        }
    }
    Ok(json!({ "ok": true }))
}

fn check_stream_allowed(sh: &Shared) -> Result<(), String> {
    if sh.control.snapshot().settings.panel.allow_stream_control {
        Ok(())
    } else {
        Err("Iniciar y parar el directo desde el panel está desactivado en la app de escritorio".into())
    }
}

// ---- WebSocket ----

async fn ws_upgrade(
    State(sh): State<Arc<Shared>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origen no admitido");
    }
    ws.max_message_size(8 * 1024).on_upgrade(move |socket| session(sh, addr.ip(), socket))
}

fn text(value: Value) -> Message {
    Message::Text(value.to_string().into())
}

fn state_message(sh: &Shared) -> Message {
    text(json!({ "type": "state", "state": WebState::from(&sh.control.snapshot()) }))
}

async fn session(sh: Arc<Shared>, ip: IpAddr, mut socket: WebSocket) {
    // El primer mensaje debe traer el token. Los navegadores no pueden poner cabeceras en un WebSocket.
    let hello = timeout(Duration::from_secs(5), socket.recv()).await;
    let token = match hello {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<Value>(t.as_str())
            .ok()
            .filter(|v| v["type"] == "hello")
            .and_then(|v| v["token"].as_str().map(str::to_owned)),
        _ => None,
    };
    let Some(device) = token.and_then(|t| sh.auth.authenticate(&t, ip)) else {
        let _ = socket.send(text(json!({ "type": "denied" }))).await;
        return;
    };

    let id = sh.connections.fetch_add(1, Ordering::Relaxed);
    let consumer = format!("web:{}:{id}", device.id);
    sh.set_live(&device.id, 1);

    let mut snapshot_rx = sh.control.subscribe();
    let mut frames = sh.control.preview().subscribe();
    let mut revoked = sh.auth.revocations();
    let mut heartbeat = interval(Duration::from_secs(25));
    let mut last_rx = Instant::now();
    let mut wanted = Interest::default();

    let ready = text(json!({ "type": "ready", "name": device.name }));
    let mut alive = socket.send(ready).await.is_ok() && socket.send(state_message(&sh)).await.is_ok();
    snapshot_rx.borrow_and_update();

    while alive {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    last_rx = Instant::now();
                    let Ok(msg) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                    if msg["type"] == "watch" {
                        wanted = Interest {
                            program: msg["program"].as_bool().unwrap_or(false),
                            sources: msg["sources"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).take(16).collect())
                                .unwrap_or_default(),
                        };
                        sh.control.watch_previews(&consumer, wanted.clone());
                        for frame in sh.control.preview().latest_for(&wanted) {
                            let msg = text(json!({ "type": "frame", "key": frame.key, "image": frame.image }));
                            if socket.send(msg).await.is_err() { alive = false; break; }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => alive = false,
                Some(Ok(_)) => last_rx = Instant::now(),
            },
            changed = snapshot_rx.changed() => {
                if changed.is_err() { break; }
                snapshot_rx.borrow_and_update();
                // Si cambian las fuentes de la escena, se vuelve a aplicar lo que el móvil pidió ver.
                if !wanted.sources.is_empty() { sh.control.watch_previews(&consumer, wanted.clone()); }
                alive = socket.send(state_message(&sh)).await.is_ok();
            }
            frame = frames.recv() => match frame {
                Ok(frame) if sh.control.preview().wants(&consumer, &frame.key) => {
                    let msg = text(json!({ "type": "frame", "key": frame.key, "image": frame.image }));
                    alive = socket.send(msg).await.is_ok();
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            gone = revoked.recv() => {
                if gone.as_deref() == Ok(device.id.as_str()) || !sh.auth.is_valid(&device.id) {
                    let _ = socket.send(text(json!({ "type": "revoked" }))).await;
                    break;
                }
            }
            _ = heartbeat.tick() => {
                if last_rx.elapsed() > WS_IDLE_LIMIT { break; }
                alive = socket.send(Message::Ping(Vec::new().into())).await.is_ok();
            }
        }
    }

    sh.control.watch_previews(&consumer, Interest::default());
    sh.set_live(&device.id, -1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        h
    }

    #[test]
    fn solo_se_aceptan_hosts_de_ip_localhost_o_del_propio_equipo() {
        let own = Some("mi-pc");
        for ok in ["192.168.1.20:8443", "localhost:8443", "[::1]:8443", "mi-pc:8443", "MI-PC.local", "10.0.0.2"] {
            assert!(host_allowed(&headers(ok), own), "{ok}");
        }
        for bad in ["evil.example.com", "evil.example.com:8443", "mi-pc.evil.com"] {
            assert!(!host_allowed(&headers(bad), own), "{bad}");
        }
        assert!(!host_allowed(&HeaderMap::new(), own));
    }

    #[test]
    fn el_origen_debe_coincidir_con_el_host() {
        let mut h = headers("192.168.1.20:8443");
        assert!(origin_allowed(&h), "sin Origin se admite");
        h.insert(header::ORIGIN, HeaderValue::from_static("https://192.168.1.20:8443"));
        assert!(origin_allowed(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("https://evil.example.com"));
        assert!(!origin_allowed(&h));
    }
}
