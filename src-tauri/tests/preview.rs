//! Capturas en vivo, lista de fuentes y salud del directo contra el OBS simulado.

mod common;

use std::{fs, sync::Arc, time::Duration};

use common::MockObs;
use stream_control_lib::{
    control::{Control, Snapshot},
    obs::ConnectionState,
    preview::{Interest, PROGRAM_KEY},
    settings::{Camera, ObsSettings, Settings, SettingsStore, secrets::MemoryStore},
};
use tokio::{sync::watch, time::timeout};

async fn start(name: &str) -> (MockObs, Arc<Control>) {
    let obs = MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-pv-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store
        .save(&Settings {
            obs: ObsSettings { host: "127.0.0.1".into(), port: obs.port },
            mic_input: Some("Micro".into()),
            cameras: vec![
                Camera { id: "movil".into(), name: "Móvil".into(), source: "Cámara móvil".into() },
                Camera { id: "webcam".into(), name: "Webcam".into(), source: "Webcam".into() },
            ],
            ..Default::default()
        })
        .unwrap();
    let control = Control::start(store, Arc::new(MemoryStore::default()));
    let mut rx = control.subscribe();
    wait(&mut rx, |s| s.connection.state == ConnectionState::Connected && !s.sources.is_empty()).await;
    (obs, control)
}

async fn wait(rx: &mut watch::Receiver<Snapshot>, pred: impl Fn(&Snapshot) -> bool) -> Snapshot {
    timeout(Duration::from_secs(12), async {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            if pred(&snapshot) {
                return snapshot;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| panic!("la condición no se cumplió; último estado: {:#?}", rx.borrow().clone()))
}

fn screenshot_requests(obs: &MockObs) -> usize {
    obs.log().iter().filter(|(kind, _)| kind == "GetSourceScreenshot").count()
}

#[tokio::test]
async fn lista_los_elementos_de_la_escena_y_distingue_los_de_solo_audio() {
    let (_obs, control) = start("fuentes").await;
    let sources = control.snapshot().sources;
    let names: Vec<_> = sources.iter().map(|s| (s.name.as_str(), s.visual, s.enabled)).collect();
    assert_eq!(
        names,
        [("Cámara móvil", true, true), ("Webcam", true, false), ("Marco", true, true), ("Micro", false, true)]
    );
}

#[tokio::test]
async fn muestra_y_oculta_una_fuente_de_la_escena() {
    let (_obs, control) = start("toggle").await;
    let mut rx = control.subscribe();
    let marco = control.snapshot().sources.iter().find(|s| s.name == "Marco").unwrap().id;

    control.set_source_enabled(marco, false).await.unwrap();
    wait(&mut rx, |s| s.sources.iter().any(|x| x.name == "Marco" && !x.enabled)).await;
    control.set_source_enabled(marco, true).await.unwrap();
    wait(&mut rx, |s| s.sources.iter().any(|x| x.name == "Marco" && x.enabled)).await;

    assert!(control.set_source_enabled(999, true).await.unwrap_err().contains("ya no está"));
}

#[tokio::test]
async fn la_salud_del_directo_llega_con_cpu_fps_frames_y_bitrate() {
    let (_obs, control) = start("salud").await;
    let mut rx = control.subscribe();
    let health = wait(&mut rx, |s| s.health.is_some()).await.health.unwrap();
    assert_eq!(health.cpu, 7.3);
    assert_eq!(health.fps, 59.9);
    assert_eq!(health.dropped_frames, 3);
    assert_eq!(health.bitrate_kbps, None, "sin stream no hay bitrate");

    control.start_stream().await.unwrap();
    let live = wait(&mut rx, |s| s.health.as_ref().is_some_and(|h| h.bitrate_kbps.is_some())).await.health.unwrap();
    assert!(live.bitrate_kbps.unwrap() > 0);
    assert_eq!(live.duration_secs, 5);
}

#[tokio::test]
async fn solo_captura_mientras_alguien_mira_y_solo_lo_que_pide() {
    let (obs, control) = start("capturas").await;
    let mut frames = control.preview().subscribe();

    // Nadie mira: ninguna petición de captura a OBS.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(screenshot_requests(&obs), 0);

    // Se piden el programa, una cámara visible, una oculta y una fuente que no existe.
    control.watch_previews(
        "prueba",
        Interest {
            program: true,
            sources: vec!["Cámara móvil".into(), "Webcam".into(), "No existe".into(), "Micro".into()],
        },
    );
    let mut keys = std::collections::HashSet::new();
    timeout(Duration::from_secs(8), async {
        while keys.len() < 2 {
            let frame = frames.recv().await.unwrap();
            assert!(frame.image.starts_with("data:image/jpeg;base64,"), "{}", frame.image);
            keys.insert(frame.key.clone());
        }
    })
    .await
    .expect("deberían llegar capturas del programa y de la cámara visible");
    assert!(keys.contains(PROGRAM_KEY) && keys.contains("Cámara móvil"));

    // Lo que no es una fuente visual de la escena ni se pide a OBS.
    let asked: Vec<String> = obs
        .log()
        .iter()
        .filter(|(kind, _)| kind == "GetSourceScreenshot")
        .map(|(_, d)| d["sourceName"].as_str().unwrap().to_owned())
        .collect();
    assert!(!asked.iter().any(|n| n == "No existe" || n == "Micro"), "{asked:?}");
    assert!(asked.iter().all(|n| n == "Juego" || n == "Cámara móvil" || n == "Webcam"), "{asked:?}");

    // Al dejar de mirar, se detienen.
    control.watch_previews("prueba", Interest::default());
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = screenshot_requests(&obs);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(screenshot_requests(&obs), before, "no debe capturar sin espectadores");
}

#[tokio::test]
async fn el_programa_se_captura_con_el_nombre_de_la_escena_activa() {
    let (obs, control) = start("programa").await;
    let mut frames = control.preview().subscribe();
    control.watch_previews("prueba", Interest { program: true, sources: vec![] });
    timeout(Duration::from_secs(5), frames.recv()).await.unwrap().unwrap();

    let mut rx = control.subscribe();
    control.launch_scene("Charla").await.unwrap();
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Charla")).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let last = obs.log().iter().rev().find(|(k, _)| k == "GetSourceScreenshot").unwrap().1.clone();
    assert_eq!(last["sourceName"], "Charla");
    assert_eq!(last["imageFormat"], "jpg");
    assert_eq!(last["imageWidth"], 640);
}
