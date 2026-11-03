//! Servidor del avatar: páginas, WebSocket de fotogramas, cambio de personaje, protección y fuente de OBS.

mod common;

use std::{fs, sync::Arc, time::Duration};

use common::MockObs;
use futures_util::StreamExt;
use serde_json::{Value, json};
use stream_control_lib::{
    avatar::{Avatar, AvatarFrame, AvatarStatus},
    control::{Control, Snapshot},
    obs::ConnectionState,
    settings::{AvatarSettings, ObsSettings, Settings, SettingsStore, secrets::MemoryStore},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

async fn start(name: &str, avatar: AvatarSettings) -> (MockObs, Arc<Control>, Arc<Avatar>) {
    let obs = MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-av-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store
        .save(&Settings { obs: ObsSettings { host: "127.0.0.1".into(), port: obs.port }, avatar, ..Default::default() })
        .unwrap();
    let control = Control::start(store, Arc::new(MemoryStore::default()));
    let avatar = Avatar::start(Arc::clone(&control));
    let mut rx = control.subscribe();
    wait_snapshot(&mut rx, |s| s.connection.state == ConnectionState::Connected && !s.scenes.is_empty()).await;
    (obs, control, avatar)
}

fn enabled() -> AvatarSettings {
    AvatarSettings { enabled: true, port: 0, character: "jason".into() }
}

async fn wait_snapshot(rx: &mut watch::Receiver<Snapshot>, pred: impl Fn(&Snapshot) -> bool) {
    timeout(Duration::from_secs(10), async {
        loop {
            if pred(&rx.borrow_and_update()) {
                return;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("la condición no se cumplió");
}

async fn wait_status(avatar: &Avatar, pred: impl Fn(&AvatarStatus) -> bool) -> AvatarStatus {
    let mut rx = avatar.subscribe();
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
    .unwrap_or_else(|_| panic!("el estado del avatar no llegó; último: {:?}", avatar.status()))
}

async fn get(port: u16, path: &str, host: Option<&str>) -> (u16, String, String) {
    let host = host.map(str::to_owned).unwrap_or_else(|| format!("127.0.0.1:{port}"));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_lowercase(), body.to_owned())
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn connect(port: u16) -> Ws {
    tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/avatar/ws")).await.unwrap().0
}

async fn next(ws: &mut Ws) -> Value {
    loop {
        match timeout(Duration::from_secs(5), ws.next()).await.expect("el avatar no envió nada").unwrap().unwrap() {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn sirve_la_pagina_el_script_y_los_dos_personajes_solo_en_loopback() {
    let (_obs, _control, avatar) = start("paginas", enabled()).await;
    let status = wait_status(&avatar, |s| s.running).await;
    assert!(status.url.starts_with("http://127.0.0.1:") && status.url.ends_with("/avatar"));

    let (code, _, html) = get(status.port, "/avatar", None).await;
    assert_eq!(code, 200);
    assert!(html.contains("avatar.js"));
    assert_eq!(get(status.port, "/avatar.js", None).await.0, 200);

    for name in ["jason", "cal"] {
        let (code, headers, svg) = get(status.port, &format!("/avatars/{name}.svg"), None).await;
        assert_eq!(code, 200, "{name}");
        assert!(headers.contains("image/svg+xml"));
        // Los personajes tienen las piezas que la página anima.
        for id in ["avatar-body", "avatar-head", "avatar-face", "eye-l", "eye-r", "brow-l", "brow-r", "mouth-cavity", "mouth-clip-path", "mouth-line", "mouth-teeth", "mouth-tongue"] {
            assert!(svg.contains(&format!("id=\"{id}\"")), "{name} no tiene #{id}");
        }
    }
    assert_eq!(get(status.port, "/avatars/otro.svg", None).await.0, 404);
    assert_eq!(get(status.port, "/avatars/..%2f..%2fCargo.toml", None).await.0, 404);

    // Solo escucha en 127.0.0.1: desde otra dirección del equipo no se puede entrar.
    if let Some(lan) = stream_control_lib::panel::tls::local_ips().first() {
        assert!(TcpStream::connect((*lan, status.port)).await.is_err(), "el avatar no debe escucharse fuera de loopback");
    }
}

#[tokio::test]
async fn rechaza_hosts_y_origenes_ajenos() {
    let (_obs, _control, avatar) = start("proteccion", enabled()).await;
    let port = wait_status(&avatar, |s| s.running).await.port;

    assert_eq!(get(port, "/avatar", Some("evil.example.com")).await.0, 421);
    assert_eq!(get(port, "/avatar", Some("127.0.0.1:1")).await.0, 421, "puerto distinto");
    assert_eq!(get(port, "/avatar", Some(&format!("localhost:{port}"))).await.0, 200);

    // Una página de otro sitio no puede abrir el WebSocket.
    let mut req = format!("ws://127.0.0.1:{port}/avatar/ws").into_client_request().unwrap();
    req.headers_mut().insert("Origin", "https://evil.example.com".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    let mut ok = format!("ws://127.0.0.1:{port}/avatar/ws").into_client_request().unwrap();
    ok.headers_mut().insert("Origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    assert!(tokio_tungstenite::connect_async(ok).await.is_ok());
}

#[tokio::test]
async fn el_websocket_envia_el_personaje_y_los_fotogramas_acotados() {
    let (_obs, _control, avatar) = start("ws", enabled()).await;
    let port = wait_status(&avatar, |s| s.running).await.port;
    let mut ws = connect(port).await;

    assert_eq!(next(&mut ws).await, json!({ "type": "config", "character": "jason" }));
    let first = next(&mut ws).await;
    assert_eq!(first["type"], "frame");
    assert_eq!(first["tracking"], false);

    avatar.push(AvatarFrame { tracking: true, mouth: 0.6, smile: 0.2, yaw: -0.5, roll: 10.0, ..Default::default() });
    let f = next(&mut ws).await;
    assert_eq!(f["tracking"], true);
    assert!((f["mouth"].as_f64().unwrap() - 0.6).abs() < 1e-6);
    assert!((f["yaw"].as_f64().unwrap() + 0.5).abs() < 1e-6);
    assert_eq!(f["roll"], 10.0);

    // Valores fuera de rango o no numéricos nunca llegan a la página.
    avatar.push(AvatarFrame { tracking: true, mouth: 7.0, blink_left: f32::NAN, roll: 300.0, ..Default::default() });
    let f = next(&mut ws).await;
    assert_eq!((f["mouth"].as_f64().unwrap(), f["blinkLeft"].as_f64().unwrap(), f["roll"].as_f64().unwrap()), (1.0, 0.0, 35.0));
}

#[tokio::test]
async fn al_cambiar_de_personaje_en_los_ajustes_la_pagina_lo_recibe_al_instante() {
    let (_obs, control, avatar) = start("personaje", enabled()).await;
    let port = wait_status(&avatar, |s| s.running).await.port;
    let mut ws = connect(port).await;
    assert_eq!(next(&mut ws).await["character"], "jason");
    next(&mut ws).await; // fotograma inicial

    let mut settings = control.snapshot().settings;
    settings.avatar.character = "cal".into();
    control.save_settings(settings.clone()).await.unwrap();
    let msg = loop {
        let m = next(&mut ws).await;
        if m["type"] == "config" {
            break m;
        }
    };
    assert_eq!(msg["character"], "cal");

    settings.avatar.character = "no-existe".into();
    assert!(control.save_settings(settings).await.unwrap_err().contains("personaje"));
}

#[tokio::test]
async fn cuenta_las_paginas_conectadas_para_decir_si_obs_lo_esta_viendo() {
    let (_obs, _control, avatar) = start("clientes", enabled()).await;
    wait_status(&avatar, |s| s.running && s.clients == 0).await;
    let mut a = connect(avatar.status().port).await;
    next(&mut a).await;
    wait_status(&avatar, |s| s.clients == 1).await;
    let mut b = connect(avatar.status().port).await;
    next(&mut b).await;
    wait_status(&avatar, |s| s.clients == 2).await;
    drop(a);
    wait_status(&avatar, |s| s.clients == 1).await;
    drop(b);
    wait_status(&avatar, |s| s.clients == 0).await;
}

#[tokio::test]
async fn se_enciende_y_se_apaga_desde_los_ajustes_y_libera_el_puerto() {
    let (_obs, control, avatar) = start("onoff", AvatarSettings { enabled: false, port: 0, character: "cal".into() }).await;
    let off = wait_status(&avatar, |s| !s.enabled).await;
    assert!(!off.running);

    let mut settings = control.snapshot().settings;
    settings.avatar.enabled = true;
    control.save_settings(settings.clone()).await.unwrap();
    let on = wait_status(&avatar, |s| s.running).await;
    let mut ws = connect(on.port).await;
    assert_eq!(next(&mut ws).await["character"], "cal");

    settings.avatar.enabled = false;
    control.save_settings(settings).await.unwrap();
    wait_status(&avatar, |s| !s.running).await;
    // Con una página abierta, el servidor se cierra igualmente y el puerto queda libre.
    assert!(TcpStream::connect(("127.0.0.1", on.port)).await.is_err());
}

#[tokio::test]
async fn si_el_puerto_esta_ocupado_lo_dice() {
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port();
    let (_obs, _control, avatar) = start("ocupado", AvatarSettings { enabled: true, port, character: "jason".into() }).await;
    let s = wait_status(&avatar, |s| s.error.is_some()).await;
    assert!(!s.running);
    assert!(s.error.unwrap().contains("ya está en uso"));
}

#[tokio::test]
async fn los_ajustes_del_avatar_se_validan() {
    let (_obs, control, _avatar) = start("validar", enabled()).await;
    let base = control.snapshot().settings;

    let mut s = base.clone();
    s.avatar.port = 80;
    assert!(control.save_settings(s).await.unwrap_err().contains("1024"));

    let mut s = base.clone();
    s.avatar.port = s.obs.port;
    assert!(control.save_settings(s).await.unwrap_err().contains("puerto distinto"));

    let mut s = base;
    s.panel.enabled = true;
    s.panel.port = 5000;
    s.avatar.port = 5000;
    assert!(control.save_settings(s).await.unwrap_err().contains("puerto distinto"));
}

#[tokio::test]
async fn crea_la_fuente_de_navegador_del_avatar_en_la_escena_en_programa() {
    // Desactivado: solo se prueba la petición a OBS, sin abrir ningún puerto.
    let (obs, control, _avatar) = start("crear", AvatarSettings { enabled: false, port: 4488, character: "jason".into() }).await;
    let scene = control.create_avatar_source().await.unwrap();
    assert_eq!(scene, "Juego");
    let log = obs.log();
    let (_, data) = log.iter().find(|(k, _)| k == "CreateInput").expect("debería haber pedido CreateInput");
    assert_eq!(data["sceneName"], "Juego");
    assert_eq!(data["inputKind"], "browser_source");
    assert_eq!(data["inputName"], "Avatar (Stream Control)");
    assert_eq!(data["inputSettings"]["url"], "http://127.0.0.1:4488/avatar");
    assert_eq!((data["inputSettings"]["width"].as_i64(), data["inputSettings"]["height"].as_i64()), (Some(800), Some(800)));

    // Si ya existe, se explica en vez de duplicarlo.
    let err = control.create_avatar_source().await.unwrap_err();
    assert!(err.contains("Ya existe"), "{err}");
}
