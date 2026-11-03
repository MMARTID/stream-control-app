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
    settings::{AvatarSettings, Camera, ObsSettings, Settings, SettingsStore, secrets::MemoryStore},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

async fn start(name: &str, avatar: AvatarSettings) -> (MockObs, Arc<Control>, Arc<Avatar>) {
    start_with(name, avatar, vec![]).await
}

async fn start_with(name: &str, avatar: AvatarSettings, cameras: Vec<Camera>) -> (MockObs, Arc<Control>, Arc<Avatar>) {
    let obs = MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-av-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store
        .save(&Settings { obs: ObsSettings { host: "127.0.0.1".into(), port: obs.port }, avatar, cameras, ..Default::default() })
        .unwrap();
    let control = Control::start(store, Arc::new(MemoryStore::default()));
    let avatar = Avatar::start(Arc::clone(&control));
    let mut rx = control.subscribe();
    wait_snapshot(&mut rx, |s| s.connection.state == ConnectionState::Connected && !s.scenes.is_empty()).await;
    (obs, control, avatar)
}

fn enabled() -> AvatarSettings {
    AvatarSettings { enabled: true, port: 0, character: "jason".into(), ..Default::default() }
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
    let (_obs, control, avatar) = start("onoff", AvatarSettings { enabled: false, port: 0, character: "cal".into(), ..Default::default() }).await;
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
    let (_obs, _control, avatar) = start("ocupado", AvatarSettings { enabled: true, port, character: "jason".into(), ..Default::default() }).await;
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

fn cameras() -> Vec<Camera> {
    vec![
        Camera { id: "movil".into(), name: "Móvil".into(), source: "Cámara móvil".into() },
        Camera { id: "webcam".into(), name: "Webcam".into(), source: "Webcam".into() },
    ]
}

fn disabled() -> AvatarSettings {
    // Desactivado: solo se prueba la conversación con OBS, sin abrir ningún puerto.
    AvatarSettings { enabled: false, port: 4488, ..Default::default() }
}

/// Escena, orden de las fuentes y transformaciones tal como las dejó OBS (simulado).
fn items_of(obs: &MockObs, scene: &str) -> Vec<(String, Value)> {
    obs.state.lock().unwrap().items[scene].iter().map(|i| (i.source.clone(), i.transform.clone())).collect()
}

#[tokio::test]
async fn crea_la_fuente_del_avatar_en_la_escena_actual_abajo_a_la_izquierda() {
    let (obs, control, _avatar) = start("fuente", disabled()).await;

    let scene = control.create_avatar_source().await.unwrap();
    assert_eq!(scene, "Juego");
    let log = obs.log();
    let (_, data) = log.iter().find(|(k, _)| k == "CreateInput").expect("debería haber pedido CreateInput");
    assert_eq!(data["sceneName"], "Juego");
    assert_eq!(data["inputKind"], "browser_source");
    assert_eq!(data["inputName"], "Avatar (Stream Control)");
    assert_eq!(data["inputSettings"]["url"], "http://127.0.0.1:4488/avatar");
    assert_eq!((data["inputSettings"]["width"].as_i64(), data["inputSettings"]["height"].as_i64()), (Some(800), Some(880)));

    // Abajo a la izquierda: lienzo de 1920×1080, alto del 34 % (367 px) y margen del 2 % (22 px).
    let (_, transform) = items_of(&obs, "Juego").into_iter().find(|(name, _)| name == "Avatar (Stream Control)").unwrap();
    let scale = 367.0 / 880.0;
    assert_eq!(transform["positionX"], 22.0);
    assert_eq!(transform["positionY"], 1080.0 - 22.0 - 367.0);
    assert!((transform["scaleX"].as_f64().unwrap() - scale).abs() < 1e-9);
    assert_eq!(transform["scaleX"], transform["scaleY"]);

    // No se duplica en la misma escena.
    let err = control.create_avatar_source().await.unwrap_err();
    assert!(err.contains("ya tiene el avatar"), "{err}");
}

#[tokio::test]
async fn crea_la_escena_avatar_con_el_avatar_y_copia_las_fuentes_menos_las_camaras() {
    let (obs, control, _avatar) = start_with("escena", disabled(), cameras()).await;

    let report = control.create_avatar_scene(true, true).await.unwrap();
    assert_eq!(report.scene, "Avatar");
    assert_eq!(report.copied, 2, "Marco y Micro; las dos cámaras no se copian");
    assert!(report.failed.is_empty());

    let items = items_of(&obs, "Avatar");
    let names: Vec<_> = items.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Marco", "Micro", "Avatar (Stream Control)"], "mismo orden de apilado y el avatar encima");
    assert!(!names.contains(&"Cámara móvil") && !names.contains(&"Webcam"));

    // «Marco» conserva su posición y tamaño de la escena de origen.
    let (_, marco) = &items[0];
    assert_eq!((marco["positionX"].as_f64(), marco["positionY"].as_f64(), marco["scaleX"].as_f64()), (Some(100.0), Some(50.0), Some(0.5)));
    // Solo se escribieron campos escribibles (el simulador rechaza `sourceWidth`, `width`…).
    // El avatar queda abajo a la izquierda.
    let (_, avatar) = &items[2];
    assert_eq!((avatar["positionX"].as_f64(), avatar["positionY"].as_f64()), (Some(22.0), Some(691.0)));

    // Se añade a las escenas rápidas para cambiar de un toque.
    let quick = control.snapshot().settings.quick_scenes;
    assert!(quick.iter().any(|q| q.scene == "Avatar" && q.label == "Avatar" && !q.mute_mic));

    // La escena de origen no se ha tocado.
    let origin: Vec<_> = items_of(&obs, "Juego").into_iter().map(|(n, _)| n).collect();
    assert_eq!(origin, ["Cámara móvil", "Webcam", "Marco", "Micro"]);
}

#[tokio::test]
async fn sin_copiar_fuentes_la_escena_solo_lleva_el_avatar_y_no_se_duplica_la_escena_rapida() {
    let (obs, control, _avatar) = start_with("solo", disabled(), cameras()).await;
    let report = control.create_avatar_scene(false, false).await.unwrap();
    assert_eq!(report.copied, 0);
    let names: Vec<_> = items_of(&obs, "Avatar").into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Avatar (Stream Control)"]);
    assert!(control.snapshot().settings.quick_scenes.is_empty(), "sin pedirlo no se toca la lista de escenas rápidas");
}

#[tokio::test]
async fn no_toca_una_escena_que_ya_existe_y_reutiliza_la_fuente_del_avatar_si_ya_estaba_en_obs() {
    let (obs, control, _avatar) = start_with("existe", disabled(), cameras()).await;

    // El avatar ya existe como entrada de OBS (p. ej. de una escena borrada): se reutiliza, no se duplica.
    control.create_avatar_source().await.unwrap();
    let report = control.create_avatar_scene(false, false).await.unwrap();
    assert_eq!(report.scene, "Avatar");
    assert!(obs.log().iter().any(|(k, d)| k == "CreateSceneItem" && d["sourceName"] == "Avatar (Stream Control)" && d["sceneName"] == "Avatar"));
    let inputs = obs.state.lock().unwrap().items.values().flatten().filter(|i| i.kind == "browser_source").count();
    assert_eq!(inputs, 2, "una en «Juego» y una en «Avatar», sobre la misma entrada (el simulador cuenta elementos)");

    // La escena ya existe: error claro y nada que modificar.
    let before = obs.log().len();
    let err = control.create_avatar_scene(true, true).await.unwrap_err();
    assert!(err.contains("Ya existe una escena «Avatar»"), "{err}");
    assert!(obs.log()[before..].iter().all(|(k, _)| !k.starts_with("Create") && !k.starts_with("Set")), "no debe pedir cambios a OBS");
}

#[tokio::test]
async fn el_nombre_de_la_escena_del_avatar_se_valida_y_se_puede_cambiar() {
    let (obs, control, _avatar) = start_with("nombre", disabled(), vec![]).await;
    let mut settings = control.snapshot().settings;
    settings.avatar.scene = "  ".into();
    assert!(control.save_settings(settings.clone()).await.unwrap_err().contains("escena del avatar"));

    settings.avatar.scene = "Cinemática".into();
    control.save_settings(settings).await.unwrap();
    let report = control.create_avatar_scene(false, true).await.unwrap();
    assert_eq!(report.scene, "Cinemática");
    assert!(obs.state.lock().unwrap().scenes.contains(&"Cinemática".to_owned()));
}
