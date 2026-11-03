//! Panel web con un OBS simulado, para probarlo en un navegador sin OBS ni la ventana de escritorio:
//!
//!     cargo run --example panel_dev -- [puerto] [--plain]
//!
//! Contraseña del panel: `contraseña-demo`. Los códigos de emparejamiento se imprimen aquí
//! (en la app real salen en la ventana de escritorio). Con `--plain` el panel se sirve por HTTP
//! en 127.0.0.1, solo para poder inspeccionarlo desde herramientas que rechazan certificados
//! autofirmados; la app real nunca sirve el panel sin HTTPS.

#[path = "../tests/common/mod.rs"]
mod common;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use stream_control_lib::{
    control::Control,
    panel::{
        Panel,
        audit::Audit,
        auth::Auth,
        http::{Shared, router},
    },
    settings::{
        Camera, ObsSettings, PanelSettings, QuickScene, Settings, SettingsStore,
        secrets::MemoryStore,
    },
};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let plain = args.iter().any(|a| a == "--plain");
    let port: u16 = args.iter().find_map(|a| a.parse().ok()).unwrap_or(8443);

    let obs = common::MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-panel-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store
        .save(&Settings {
            obs: ObsSettings { host: "127.0.0.1".into(), port: obs.port },
            panel: PanelSettings { enabled: !plain, port, allow_stream_control: true, only_quick_scenes: false },
            mic_input: Some("Micro".into()),
            quick_scenes: vec![
                QuickScene { scene: "Pausa / baño".into(), label: "Pausa / baño".into(), color: "#f59e0b".into(), icon: "☕".into(), mute_mic: true },
                QuickScene { scene: "Charla".into(), label: "Cambio de tema".into(), color: "#8b5cf6".into(), icon: "💬".into(), mute_mic: false },
            ],
            cameras: vec![
                Camera { id: "movil".into(), name: "Móvil".into(), source: "Cámara móvil".into() },
                Camera { id: "webcam".into(), name: "Webcam".into(), source: "Webcam".into() },
            ],
        })
        .unwrap();
    let secrets = Arc::new(MemoryStore::default());
    let control = Control::start(store, secrets.clone());

    if plain {
        let auth = Arc::new(Auth::new(dir.clone(), secrets));
        auth.set_password("contraseña-demo").unwrap();
        let shared = Shared::new(Arc::clone(&control), Arc::clone(&auth), Arc::new(Audit::new(dir)));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        println!("Panel (HTTP, solo pruebas) en http://127.0.0.1:{port}");
        tokio::spawn(async move {
            axum::serve(listener, router(shared).into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
        });
        let mut shown = String::new();
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            for p in auth.pending() {
                if p.id != shown {
                    println!("CÓDIGO de «{}»: {}", p.name, p.code);
                    shown = p.id;
                }
            }
        }
    } else {
        let panel = Panel::start(Arc::clone(&control), dir, secrets);
        panel.set_password("contraseña-demo").unwrap();
        let mut rx = panel.subscribe();
        let mut shown = String::new();
        let mut announced = false;
        loop {
            let s = rx.borrow_and_update().clone();
            if s.running && !announced {
                announced = true;
                println!("Panel (HTTPS) en: {:?}", s.urls);
            }
            if let Some(e) = &s.error {
                println!("ERROR: {e}");
            }
            for p in &s.pairings {
                if p.id != shown {
                    println!("CÓDIGO de «{}»: {}", p.name, p.code);
                    shown = p.id.clone();
                }
            }
            if rx.changed().await.is_err() {
                break;
            }
        }
    }
}
