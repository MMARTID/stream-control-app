//! Avatar animado con la boca (y la cara) del streamer.
//!
//! La ventana de la app sigue la cara o la voz (cámara o micrófono, en local) y envía unos pocos
//! números por fotograma (`AvatarFrame`). Este módulo los reparte por WebSocket a la página
//! `/avatar`, que dibuja el personaje y que OBS carga como **fuente de navegador**.
//!
//! - El servidor escucha **solo en 127.0.0.1** y comprueba `Host` y `Origin`.
//! - Los datos solo entran por la app (comando de Tauri): el WebSocket del avatar es de solo lectura.
//! - Los personajes son archivos SVG (`web/avatars/`), así que se pueden sustituir por otros.

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{
        Path, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

use crate::control::Control;

/// Valores de un fotograma de seguimiento, ya normalizados.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AvatarFrame {
    /// `false` si no se está siguiendo a nadie: el avatar se anima solo (parpadeo, respiración).
    pub tracking: bool,
    /// Apertura de la boca, 0 (cerrada) a 1 (muy abierta).
    pub mouth: f32,
    /// -1 (mueca) a 1 (sonrisa).
    pub smile: f32,
    /// Ojo izquierdo de la pantalla: 0 abierto, 1 cerrado.
    pub blink_left: f32,
    pub blink_right: f32,
    /// -1 (ceño) a 1 (cejas arriba).
    pub brow: f32,
    /// Giro de la cabeza, -1 (izquierda de la pantalla) a 1.
    pub yaw: f32,
    /// -1 (abajo) a 1 (arriba).
    pub pitch: f32,
    /// Inclinación en grados, -35 a 35.
    pub roll: f32,
}

impl AvatarFrame {
    /// Acota todos los valores y sustituye los no numéricos por 0, venga lo que venga de la ventana.
    pub fn clamped(self) -> Self {
        let c = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
        Self {
            tracking: self.tracking,
            mouth: c(self.mouth, 0.0, 1.0),
            smile: c(self.smile, -1.0, 1.0),
            blink_left: c(self.blink_left, 0.0, 1.0),
            blink_right: c(self.blink_right, 0.0, 1.0),
            brow: c(self.brow, -1.0, 1.0),
            yaw: c(self.yaw, -1.0, 1.0),
            pitch: c(self.pitch, -1.0, 1.0),
            roll: c(self.roll, -35.0, 35.0),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    /// Dirección que hay que poner en la fuente de navegador de OBS.
    pub url: String,
    pub error: Option<String>,
    /// Páginas del avatar abiertas ahora (OBS y la vista previa de la app).
    pub clients: usize,
}

struct Running {
    port: u16,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

pub struct Avatar {
    control: Arc<Control>,
    frame_tx: watch::Sender<AvatarFrame>,
    character_tx: watch::Sender<String>,
    clients: Arc<AtomicUsize>,
    clients_changed: watch::Sender<usize>,
    server: tokio::sync::Mutex<Option<Running>>,
    error: Mutex<Option<String>>,
    status_tx: watch::Sender<AvatarStatus>,
}

impl Avatar {
    /// Arranca el servidor cuando el avatar está activado en los ajustes. Dentro de un runtime de tokio.
    pub fn start(control: Arc<Control>) -> Arc<Self> {
        let character = control.snapshot().settings.avatar.character;
        let avatar = Arc::new(Self {
            control,
            frame_tx: watch::channel(AvatarFrame::default()).0,
            character_tx: watch::channel(character).0,
            clients: Arc::new(AtomicUsize::new(0)),
            clients_changed: watch::channel(0).0,
            server: tokio::sync::Mutex::new(None),
            error: Mutex::new(None),
            status_tx: watch::channel(AvatarStatus::default()).0,
        });
        tokio::spawn(Arc::clone(&avatar).watch_settings());
        tokio::spawn(Arc::clone(&avatar).watch_clients());
        avatar
    }

    pub fn status(&self) -> AvatarStatus {
        self.status_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<AvatarStatus> {
        self.status_tx.subscribe()
    }

    /// Recibe un fotograma de seguimiento de la ventana de la app.
    pub fn push(&self, frame: AvatarFrame) {
        self.frame_tx.send_replace(frame.clamped());
    }

    async fn watch_settings(self: Arc<Self>) {
        let mut rx = self.control.subscribe();
        let mut applied: Option<(bool, u16)> = None;
        loop {
            let cfg = rx.borrow_and_update().settings.avatar.clone();
            self.character_tx.send_if_modified(|c| {
                let changed = *c != cfg.character;
                if changed {
                    *c = cfg.character.clone();
                }
                changed
            });
            if applied != Some((cfg.enabled, cfg.port)) {
                applied = Some((cfg.enabled, cfg.port));
                self.reconcile(cfg.enabled, cfg.port).await;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    async fn watch_clients(self: Arc<Self>) {
        let mut rx = self.clients_changed.subscribe();
        while rx.changed().await.is_ok() {
            self.rebuild_status().await;
        }
    }

    async fn reconcile(&self, enabled: bool, port: u16) {
        let mut server = self.server.lock().await;
        if let Some(running) = server.take() {
            let _ = running.shutdown.send(());
            let _ = running.task.await;
        }
        *self.error.lock().unwrap() = None;
        if enabled {
            match self.launch(port).await {
                Ok(running) => *server = Some(running),
                Err(e) => *self.error.lock().unwrap() = Some(e),
            }
        }
        drop(server);
        self.rebuild_status().await;
    }

    async fn launch(&self, port: u16) -> Result<Running, String> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("El puerto {port} ya está en uso. Elige otro para el avatar")
            } else {
                format!("No se pudo abrir el puerto {port}: {e}")
            }
        })?;
        let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = watch::channel(false);
        let state = Arc::new(Shared {
            port: actual,
            frames: self.frame_tx.subscribe(),
            character: self.character_tx.subscribe(),
            clients: Arc::clone(&self.clients),
            clients_changed: self.clients_changed.clone(),
            stop: stop_rx,
        });
        let app = router(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                    // Avisa a las páginas abiertas para que cierren su WebSocket y el servidor pueda terminar.
                    let _ = stop_tx.send(true);
                })
                .await;
        });
        Ok(Running { port: actual, shutdown, task })
    }

    async fn rebuild_status(&self) {
        let cfg = self.control.snapshot().settings.avatar;
        let running = self.server.lock().await.as_ref().map(|r| r.port);
        let port = running.unwrap_or(cfg.port);
        let status = AvatarStatus {
            enabled: cfg.enabled,
            running: running.is_some(),
            port,
            url: format!("http://127.0.0.1:{port}/avatar"),
            error: self.error.lock().unwrap().clone(),
            clients: self.clients.load(Ordering::Relaxed),
        };
        self.status_tx.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }
}

// ---- Servidor ----

struct Shared {
    port: u16,
    frames: watch::Receiver<AvatarFrame>,
    character: watch::Receiver<String>,
    clients: Arc<AtomicUsize>,
    clients_changed: watch::Sender<usize>,
    stop: watch::Receiver<bool>,
}

fn router(state: Arc<Shared>) -> Router {
    Router::new()
        .route("/avatar", get(|| asset("avatar.html")))
        .route("/avatar.js", get(|| asset("avatar.js")))
        .route("/avatars/{name}", get(|Path(name): Path<String>| async move { character_svg(&name) }))
        .route("/avatar/ws", get(ws_upgrade))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), guard))
        .with_state(state)
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, message.to_owned()).into_response()
}

async fn guard(State(state): State<Arc<Shared>>, req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_lowercase);
    let allowed = [format!("127.0.0.1:{}", state.port), format!("localhost:{}", state.port)];
    match &host {
        Some(h) if allowed.contains(h) => {}
        _ => return error(StatusCode::MISDIRECTED_REQUEST, "Dirección no admitida"),
    }
    // Una página de otro sitio no puede abrir el WebSocket del avatar.
    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        let same = origin.strip_prefix("http://").is_some_and(|o| allowed.contains(&o.to_lowercase()));
        // La vista previa de la app (WebView) y OBS (CEF) cargan la página de `http://127.0.0.1`; si llega
        // otro origen se rechaza, salvo el origen nulo que mandan algunos CEF.
        if !same && origin != "null" {
            return error(StatusCode::FORBIDDEN, "Origen no admitido");
        }
    }
    let mut res = next.run(req).await;
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

async fn asset(name: &'static str) -> Response {
    let (body, mime) = match name {
        "avatar.html" => (include_str!("../web/avatar.html"), "text/html; charset=utf-8"),
        "avatar.js" => (include_str!("../web/avatar.js"), "text/javascript; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Response::builder().header(header::CONTENT_TYPE, mime).body(Body::from(body)).unwrap()
}

fn character_svg(name: &str) -> Response {
    let body = match name {
        "jason.svg" => include_str!("../web/avatars/jason.svg"),
        "cal.svg" => include_str!("../web/avatars/cal.svg"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Response::builder().header(header::CONTENT_TYPE, "image/svg+xml").body(Body::from(body)).unwrap()
}

async fn ws_upgrade(State(state): State<Arc<Shared>>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(1024).on_upgrade(move |socket| session(state, socket))
}

async fn session(state: Arc<Shared>, mut socket: WebSocket) {
    let n = state.clients.fetch_add(1, Ordering::Relaxed) + 1;
    state.clients_changed.send_replace(n);

    let mut frames = state.frames.clone();
    let mut character = state.character.clone();
    let mut stop = state.stop.clone();
    let config = |c: &str| Message::Text(json!({ "type": "config", "character": c }).to_string().into());
    let frame = |f: &AvatarFrame| {
        let mut v = serde_json::to_value(f).unwrap();
        v["type"] = "frame".into();
        Message::Text(v.to_string().into())
    };

    let first = character.borrow_and_update().clone();
    let mut alive = socket.send(config(&first)).await.is_ok();
    let current = *frames.borrow_and_update();
    alive = alive && socket.send(frame(&current)).await.is_ok();

    while alive {
        tokio::select! {
            changed = character.changed() => {
                if changed.is_err() { break; }
                let c = character.borrow_and_update().clone();
                alive = socket.send(config(&c)).await.is_ok();
            }
            changed = frames.changed() => {
                if changed.is_err() { break; }
                let f = *frames.borrow_and_update();
                alive = socket.send(frame(&f)).await.is_ok();
                // Como mucho ~40 mensajes por segundo; lo que llegue entre medias se junta en el siguiente.
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => alive = false,
                Some(Ok(_)) => {}
            },
            _ = stop.changed() => break,
        }
    }

    let n = state.clients.fetch_sub(1, Ordering::Relaxed) - 1;
    state.clients_changed.send_replace(n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acota_los_valores_y_neutraliza_los_no_numericos() {
        let f = AvatarFrame {
            tracking: true,
            mouth: 5.0,
            smile: -9.0,
            blink_left: f32::NAN,
            blink_right: -1.0,
            brow: f32::INFINITY,
            yaw: 2.0,
            pitch: -2.0,
            roll: 90.0,
        }
        .clamped();
        assert_eq!(f.mouth, 1.0);
        assert_eq!(f.smile, -1.0);
        assert_eq!(f.blink_left, 0.0);
        assert_eq!(f.blink_right, 0.0);
        assert_eq!(f.brow, 0.0);
        assert_eq!((f.yaw, f.pitch, f.roll), (1.0, -1.0, 35.0));
        assert!(f.tracking);
    }

    #[test]
    fn un_fotograma_incompleto_se_rellena_con_ceros() {
        let f: AvatarFrame = serde_json::from_str(r#"{"tracking":true,"mouth":0.4}"#).unwrap();
        assert_eq!((f.mouth, f.smile, f.roll), (0.4, 0.0, 0.0));
    }
}
