//! Panel web: emparejamiento, permisos, comandos, WebSocket, revocación y HTTPS,
//! contra un OBS simulado. Las rutas se prueban contra un servidor real en el puerto de pruebas.

mod common;

use std::{fs, net::SocketAddr, sync::Arc, time::Duration};

use common::MockObs;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use stream_control_lib::{
    control::{Control, Snapshot},
    obs::ConnectionState,
    panel::{
        Panel,
        audit::Audit,
        auth::Auth,
        http::{Shared, router},
    },
    settings::{
        Camera, ObsSettings, PanelSettings, QuickScene, Settings, SettingsStore,
        secrets::{MemoryStore, SecretStore},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::tungstenite::Message;

const PANEL_PASSWORD: &str = "contraseña-del-panel";

struct Env {
    obs: MockObs,
    control: Arc<Control>,
    auth: Arc<Auth>,
    audit: Arc<Audit>,
    addr: SocketAddr,
}

fn settings(obs_port: u16, panel: PanelSettings) -> Settings {
    Settings {
        obs: ObsSettings { host: "127.0.0.1".into(), port: obs_port },
        panel,
        mic_input: Some("Micro".into()),
        quick_scenes: vec![QuickScene { scene: "Pausa / baño".into(), label: "Pausa".into(), mute_mic: true, ..Default::default() }],
        cameras: vec![
            Camera { id: "movil".into(), name: "Móvil".into(), source: "Cámara móvil".into() },
            Camera { id: "webcam".into(), name: "Webcam".into(), source: "Webcam".into() },
        ],
    }
}

async fn wait(rx: &mut watch::Receiver<Snapshot>, pred: impl Fn(&Snapshot) -> bool) -> Snapshot {
    timeout(Duration::from_secs(10), async {
        loop {
            let s = rx.borrow_and_update().clone();
            if pred(&s) {
                return s;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("la condición no se cumplió")
}

async fn control_for(name: &str, panel: PanelSettings) -> (MockObs, Arc<Control>, std::path::PathBuf, Arc<MemoryStore>) {
    let obs = MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-panel-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store.save(&settings(obs.port, panel)).unwrap();
    let secrets = Arc::new(MemoryStore::default());
    let control = Control::start(store, secrets.clone());
    let mut rx = control.subscribe();
    wait(&mut rx, |s| s.connection.state == ConnectionState::Connected && !s.cameras.is_empty() && s.mic_muted.is_some()).await;
    (obs, control, dir, secrets)
}

async fn env(name: &str, allow_stream: bool, only_quick: bool) -> Env {
    let panel = PanelSettings { allow_stream_control: allow_stream, only_quick_scenes: only_quick, ..Default::default() };
    let (obs, control, dir, secrets) = control_for(name, panel).await;
    let auth = Arc::new(Auth::new(dir.clone(), secrets));
    auth.set_password(PANEL_PASSWORD).unwrap();
    let audit = Arc::new(Audit::new(dir));
    let shared = Shared::new(Arc::clone(&control), Arc::clone(&auth), Arc::clone(&audit));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(shared).into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    Env { obs, control, auth, audit, addr }
}

struct Resp {
    status: u16,
    headers: String,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("no es JSON: {}", self.body))
    }
}

/// Cliente HTTP mínimo (una petición por conexión) para no depender de más librerías.
async fn http(addr: SocketAddr, method: &str, path: &str, host: Option<&str>, token: Option<&str>, body: Option<Value>) -> Resp {
    let host = host.map(str::to_owned).unwrap_or_else(|| addr.to_string());
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(t) = token {
        req += &format!("Authorization: Bearer {t}\r\n");
    }
    if !body.is_empty() {
        req += &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len());
    }
    req += &format!("\r\n{body}");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    parse(&raw)
}

fn parse(raw: &[u8]) -> Resp {
    let text = String::from_utf8_lossy(raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("respuesta HTTP incompleta");
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    Resp { status, headers: head.to_lowercase(), body: body.to_owned() }
}

async fn post(e: &Env, path: &str, token: Option<&str>, body: Value) -> Resp {
    http(e.addr, "POST", path, None, token, Some(body)).await
}

/// Empareja un dispositivo completo y devuelve su token.
async fn pair(e: &Env, name: &str) -> String {
    let start = post(e, "/api/pair/start", None, json!({ "password": PANEL_PASSWORD, "name": name })).await;
    assert_eq!(start.status, 200, "{}", start.body);
    let id = start.json()["pairingId"].as_str().unwrap().to_owned();
    let code = e.auth.pending().into_iter().find(|p| p.id == id).unwrap().code;
    let done = post(e, "/api/pair/finish", None, json!({ "pairingId": id, "code": code })).await;
    assert_eq!(done.status, 200, "{}", done.body);
    done.json()["token"].as_str().unwrap().to_owned()
}

async fn cmd(e: &Env, token: &str, body: Value) -> Resp {
    post(e, "/api/cmd", Some(token), body).await
}

async fn ws_connect(e: &Env, token: &str) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", e.addr)).await.unwrap();
    ws.send(Message::text(json!({ "type": "hello", "token": token }).to_string())).await.unwrap();
    ws
}

async fn next_json(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>) -> Option<Value> {
    loop {
        match timeout(Duration::from_secs(8), ws.next()).await.ok()?? {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn sirve_la_pagina_con_cabeceras_de_seguridad() {
    let e = env("pagina", false, false).await;
    for (path, needle) in [("/", "Stream Control"), ("/app.js", "wss"), ("/style.css", "--bg"), ("/manifest.webmanifest", "standalone")] {
        let r = http(e.addr, "GET", path, None, None, None).await;
        assert_eq!(r.status, 200, "{path}");
        assert!(r.body.contains(needle), "{path} debería contener {needle}");
        assert!(r.headers.contains("content-security-policy") && r.headers.contains("frame-ancestors 'none'"), "{path}");
        assert!(r.headers.contains("x-content-type-options: nosniff"), "{path}");
    }
    assert_eq!(http(e.addr, "GET", "/api/me", None, None, None).await.status, 401);
    assert_eq!(http(e.addr, "GET", "/nada", None, None, None).await.status, 404);
}

#[tokio::test]
async fn rechaza_nombres_de_host_ajenos_contra_el_dns_rebinding() {
    let e = env("host", false, false).await;
    let r = http(e.addr, "GET", "/", Some("evil.example.com"), None, None).await;
    assert_eq!(r.status, 421);
    assert!(!r.body.contains("<html"));
}

#[tokio::test]
async fn emparejamiento_en_dos_pasos_y_registro() {
    let e = env("pairing", false, false).await;

    let bad = post(&e, "/api/pair/start", None, json!({ "password": "no-es", "name": "Samsung" })).await;
    assert_eq!(bad.status, 401);
    assert!(bad.json()["error"].as_str().unwrap().contains("Contraseña"));

    let start = post(&e, "/api/pair/start", None, json!({ "password": PANEL_PASSWORD, "name": "Samsung de Miguel" })).await;
    assert_eq!(start.status, 200);
    assert!(!start.body.contains("code"), "el código nunca viaja al móvil: {}", start.body);
    let id = start.json()["pairingId"].as_str().unwrap().to_owned();
    let pending = e.auth.pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "Samsung de Miguel");

    let wrong = if pending[0].code == "000000" { "111111" } else { "000000" };
    assert_eq!(post(&e, "/api/pair/finish", None, json!({ "pairingId": id, "code": wrong })).await.status, 401);
    let done = post(&e, "/api/pair/finish", None, json!({ "pairingId": id, "code": pending[0].code })).await;
    assert_eq!(done.status, 200);
    let token = done.json()["token"].as_str().unwrap().to_owned();

    let me = http(e.addr, "GET", "/api/me", None, Some(&token), None).await;
    assert_eq!(me.json()["name"], "Samsung de Miguel");
    assert_eq!(http(e.addr, "GET", "/api/me", None, Some("scd_falso"), None).await.status, 401);

    let log = e.audit.recent(10);
    assert!(log.iter().any(|x| x.action == "Emparejamiento" && !x.ok && x.device == "Samsung"));
    assert!(log.iter().any(|x| x.action == "Emparejamiento" && x.ok && x.detail.contains("añadido")));
}

#[tokio::test]
async fn tras_cinco_contrasenas_malas_el_panel_frena_con_429() {
    let e = env("limite", false, false).await;
    for _ in 0..5 {
        assert_eq!(post(&e, "/api/pair/start", None, json!({ "password": "x", "name": "a" })).await.status, 401);
    }
    let blocked = post(&e, "/api/pair/start", None, json!({ "password": PANEL_PASSWORD, "name": "a" })).await;
    assert_eq!(blocked.status, 429);
}

#[tokio::test]
async fn los_comandos_actuan_sobre_obs_y_quedan_en_el_registro() {
    let e = env("comandos", false, false).await;
    let token = pair(&e, "Samsung").await;
    let mut rx = e.control.subscribe();

    // Escena rápida con silencio de micro.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "launch_scene", "scene": "Pausa / baño" })).await.status, 200);
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Pausa / baño") && s.mic_muted == Some(true)).await;
    assert!(e.obs.is_muted("Micro"));

    // Volver a la anterior reactiva el micro.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "back" })).await.status, 200);
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Juego") && s.mic_muted == Some(false)).await;

    // Cámara y fuente.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "switch_camera", "id": "webcam" })).await.status, 200);
    wait(&mut rx, |s| s.cameras.iter().any(|c| c.id == "webcam" && c.active)).await;
    let marco = e.control.snapshot().sources.iter().find(|s| s.name == "Marco").unwrap().id;
    assert_eq!(cmd(&e, &token, json!({ "cmd": "source", "id": marco, "enabled": false })).await.status, 200);
    wait(&mut rx, |s| s.sources.iter().any(|x| x.name == "Marco" && !x.enabled)).await;

    // Micro.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "mic", "muted": true })).await.status, 200);
    wait(&mut rx, |s| s.mic_muted == Some(true)).await;
    assert_eq!(cmd(&e, &token, json!({ "cmd": "toggle_mic" })).await.status, 200);
    wait(&mut rx, |s| s.mic_muted == Some(false)).await;

    let log = e.audit.recent(20);
    for (action, detail) in [("Escena", "Pausa / baño"), ("Escena", "volver"), ("Cámara", "Webcam"), ("Fuente", "Marco"), ("Micro", "silenciar")] {
        assert!(log.iter().any(|x| x.action == action && x.detail.contains(detail) && x.device == "Samsung" && x.ok), "{action} {detail}: {log:#?}");
    }
}

#[tokio::test]
async fn los_errores_de_los_comandos_se_devuelven_y_se_registran() {
    let e = env("errores", false, false).await;
    let token = pair(&e, "Samsung").await;

    let r = cmd(&e, &token, json!({ "cmd": "launch_scene", "scene": "No existe" })).await;
    assert_eq!(r.status, 400);
    assert!(r.json()["error"].as_str().unwrap().contains("no existe"));
    assert_eq!(cmd(&e, &token, json!({ "cmd": "borrar_todo" })).await.status, 422);
    assert_eq!(cmd(&e, "scd_falso", json!({ "cmd": "back" })).await.status, 401);
    assert!(e.audit.recent(5).iter().any(|x| x.action == "Escena" && !x.ok));
}

#[tokio::test]
async fn con_solo_escenas_rapidas_activado_el_panel_no_lanza_otras() {
    let e = env("soloquick", false, true).await;
    let token = pair(&e, "Samsung").await;
    let r = cmd(&e, &token, json!({ "cmd": "launch_scene", "scene": "Charla" })).await;
    assert_eq!(r.status, 400);
    assert!(r.json()["error"].as_str().unwrap().contains("escenas rápidas"));
    assert_eq!(cmd(&e, &token, json!({ "cmd": "launch_scene", "scene": "Pausa / baño" })).await.status, 200);
}

#[tokio::test]
async fn iniciar_o_parar_el_stream_esta_desactivado_por_defecto() {
    let e = env("sinstream", false, false).await;
    let token = pair(&e, "Samsung").await;
    for body in [json!({ "cmd": "stream_arm", "action": "start" }), json!({ "cmd": "stream_confirm", "confirm": "x" })] {
        let r = cmd(&e, &token, body).await;
        assert_eq!(r.status, 400);
        assert!(r.json()["error"].as_str().unwrap().contains("desactivado"));
    }
    assert!(!e.control.snapshot().stream.active);
}

#[tokio::test]
async fn iniciar_y_parar_el_stream_exige_dos_pasos_con_confirmacion_propia() {
    let e = env("dospasos", true, false).await;
    let token = pair(&e, "Samsung").await;
    let other = pair(&e, "Tablet").await;
    let mut rx = e.control.subscribe();

    // Una confirmación inventada o sin pedirla antes no hace nada.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "stream_confirm", "confirm": "inventada" })).await.status, 400);
    assert!(!e.obs.state.lock().unwrap().streaming);

    // Primer paso: se pide. Todavía no pasa nada en OBS.
    let arm = cmd(&e, &token, json!({ "cmd": "stream_arm", "action": "start" })).await;
    assert_eq!(arm.status, 200);
    let confirm = arm.json()["confirm"].as_str().unwrap().to_owned();
    assert!(!e.obs.state.lock().unwrap().streaming, "armar no inicia el directo");

    // Otro dispositivo no puede usar mi confirmación.
    assert_eq!(cmd(&e, &other, json!({ "cmd": "stream_confirm", "confirm": confirm })).await.status, 400);

    // Segundo paso, desde el mismo dispositivo.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "stream_confirm", "confirm": confirm })).await.status, 200);
    wait(&mut rx, |s| s.stream.active).await;
    assert!(e.obs.state.lock().unwrap().streaming);

    // Una confirmación vale una sola vez.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "stream_confirm", "confirm": confirm })).await.status, 400);

    // Ya en directo no se puede «iniciar» otra vez, y parar también pide dos pasos.
    assert_eq!(cmd(&e, &token, json!({ "cmd": "stream_arm", "action": "start" })).await.status, 400);
    let arm = cmd(&e, &token, json!({ "cmd": "stream_arm", "action": "stop" })).await;
    let confirm = arm.json()["confirm"].as_str().unwrap().to_owned();
    assert!(e.obs.state.lock().unwrap().streaming);
    assert_eq!(cmd(&e, &token, json!({ "cmd": "stream_confirm", "confirm": confirm })).await.status, 200);
    wait(&mut rx, |s| !s.stream.active).await;

    let log = e.audit.recent(20);
    assert!(log.iter().any(|x| x.action == "Stream" && x.ok && x.device == "Samsung"));
}

#[tokio::test]
async fn el_websocket_envia_estado_saneado_y_capturas_solo_de_lo_pedido() {
    let e = env("ws", true, false).await;
    e.control.set_obs_password(Some("clave-de-obs".into())).unwrap();
    let token = pair(&e, "Samsung").await;
    let mut ws = ws_connect(&e, &token).await;

    let ready = next_json(&mut ws).await.unwrap();
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["name"], "Samsung");
    let state_msg = next_json(&mut ws).await.unwrap();
    assert_eq!(state_msg["type"], "state");
    let state = &state_msg["state"];
    assert_eq!(state["currentScene"], "Juego");
    assert_eq!(state["obsConnected"], true);
    assert_eq!(state["cameras"].as_array().unwrap().len(), 2);
    assert_eq!(state["quickScenes"][0]["scene"], "Pausa / baño");
    assert_eq!(state["allowStreamControl"], true);

    // RF-45: nada de credenciales ni datos internos en lo que ve el móvil.
    let raw = state_msg.to_string().to_lowercase();
    for forbidden in ["password", "clave-de-obs", "\"host\"", "\"port\"", "127.0.0.1", "token", "pairing", "\"code\"", "micinput", "settings"] {
        assert!(!raw.contains(forbidden), "el estado del panel contiene «{forbidden}»: {raw}");
    }

    // Sin pedir nada no llegan capturas; al pedirlas, solo las pedidas.
    ws.send(Message::text(json!({ "type": "watch", "program": true, "sources": ["Cámara móvil", "Micro", "No existe"] }).to_string())).await.unwrap();
    let mut keys = std::collections::HashSet::new();
    timeout(Duration::from_secs(10), async {
        while keys.len() < 2 {
            let msg = next_json(&mut ws).await.expect("el socket se cerró");
            if msg["type"] == "frame" {
                assert!(msg["image"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
                keys.insert(msg["key"].as_str().unwrap().to_owned());
            }
        }
    })
    .await
    .expect("deberían llegar el programa y la cámara");
    assert!(keys.contains("@program") && keys.contains("Cámara móvil"));
    assert!(!keys.contains("Micro") && !keys.contains("No existe"));

    // Al dejar de mirar, se detienen las capturas.
    ws.send(Message::text(json!({ "type": "watch", "program": false, "sources": [] }).to_string())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let count = |e: &Env| e.obs.log().iter().filter(|(k, _)| k == "GetSourceScreenshot").count();
    let before = count(&e);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(count(&e), before);
}

#[tokio::test]
async fn el_websocket_rechaza_un_token_falso() {
    let e = env("wsfalso", false, false).await;
    let mut ws = ws_connect(&e, "scd_falso").await;
    let msg = next_json(&mut ws).await.unwrap();
    assert_eq!(msg["type"], "denied");
}

#[tokio::test]
async fn revocar_un_dispositivo_corta_su_conexion_y_sus_comandos_al_instante() {
    let e = env("revocar", false, false).await;
    let token = pair(&e, "Samsung").await;
    let mut ws = ws_connect(&e, &token).await;
    assert_eq!(next_json(&mut ws).await.unwrap()["type"], "ready");

    let id = e.auth.devices()[0].id.clone();
    assert!(e.auth.revoke(&id));

    let mut revoked = false;
    while let Some(msg) = next_json(&mut ws).await {
        if msg["type"] == "revoked" {
            revoked = true;
            break;
        }
    }
    assert!(revoked, "el móvil debería recibir el aviso de revocación");
    assert_eq!(cmd(&e, &token, json!({ "cmd": "back" })).await.status, 401);
    assert_eq!(http(e.addr, "GET", "/api/me", None, Some(&token), None).await.status, 401);
}

// ---- HTTPS de verdad, con la aplicación completa del panel ----

#[derive(Debug)]
struct AcceptAnyCert(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, _: &[u8], _: &rustls::pki_types::CertificateDer<'_>, _: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(&self, _: &[u8], _: &rustls::pki_types::CertificateDer<'_>, _: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[tokio::test]
async fn el_panel_completo_arranca_con_https_se_apaga_y_cambia_de_puerto() {
    let panel_cfg = PanelSettings { enabled: true, port: 0, ..Default::default() };
    let (_obs, control, dir, secrets) = control_for("https", panel_cfg).await;
    let panel = Panel::start(Arc::clone(&control), dir.clone(), secrets.clone());
    let mut status = panel.subscribe();
    let st = timeout(Duration::from_secs(20), async {
        loop {
            let s = status.borrow_and_update().clone();
            if s.running {
                return s;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .expect("el panel debería arrancar");
    assert!(st.port > 0 && st.error.is_none());
    assert!(!st.has_password);

    // Clave privada en el almacén seguro, no en archivos.
    assert!(secrets.get("panel-tls-key").unwrap().unwrap().contains("PRIVATE KEY"));
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(!fs::read_to_string(&path).unwrap_or_default().contains("PRIVATE KEY"), "{path:?}");
        }
    }

    // Habla TLS y sirve la página; en texto plano no sirve nada útil.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
        .with_no_client_auth();
    let addr: SocketAddr = ([127, 0, 0, 1], st.port).into();
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .expect("el apretón de manos TLS debería funcionar");
    tls.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", st.port).as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    let _ = tls.read_to_end(&mut raw).await;
    let page = parse(&raw);
    assert_eq!(page.status, 200);
    assert!(page.body.contains("Stream Control"));

    let plain = timeout(Duration::from_secs(3), async {
        let mut tcp = TcpStream::connect(addr).await.unwrap();
        tcp.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut raw = Vec::new();
        let _ = tcp.read_to_end(&mut raw).await;
        String::from_utf8_lossy(&raw).into_owned()
    })
    .await
    .unwrap_or_default();
    assert!(!plain.contains("Stream Control"), "el panel no debe servirse sin cifrar");

    // Contraseña: se guarda como hash en el almacén seguro.
    panel.set_password("otra-contraseña-larga").unwrap();
    assert!(secrets.get("panel-password-hash").unwrap().unwrap().starts_with("argon2id$"));
    assert!(panel.set_password("corta").is_err());

    // Desactivarlo desde los ajustes cierra el servidor.
    let mut new = control.snapshot().settings;
    new.panel.enabled = false;
    control.save_settings(new).await.unwrap();
    timeout(Duration::from_secs(10), async {
        loop {
            if !status.borrow_and_update().running {
                break;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .expect("debería apagarse");
    assert!(TcpStream::connect(addr).await.is_err(), "el puerto debería estar cerrado");
}

#[tokio::test]
async fn si_el_puerto_esta_ocupado_el_panel_lo_dice_en_vez_de_fallar_en_silencio() {
    let busy = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let port = busy.local_addr().unwrap().port();
    let panel_cfg = PanelSettings { enabled: true, port, ..Default::default() };
    let (_obs, control, dir, secrets) = control_for("ocupado", panel_cfg).await;
    let panel = Panel::start(control, dir, secrets);
    let mut status = panel.subscribe();
    let st = timeout(Duration::from_secs(20), async {
        loop {
            let s = status.borrow_and_update().clone();
            if s.error.is_some() {
                return s;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .expect("debería informar del error");
    assert!(!st.running);
    assert!(st.error.unwrap().contains("ya está en uso"));
}
