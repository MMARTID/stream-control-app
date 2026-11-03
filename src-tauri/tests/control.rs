//! Núcleo (escenas rápidas, cámaras, micro, stream) contra un OBS simulado.

mod common;

use std::{fs, sync::Arc, time::Duration};

use common::MockObs;
use stream_control_lib::{
    control::{Control, Snapshot},
    obs::ConnectionState,
    settings::{
        Camera, ObsSettings, QuickScene, Settings, SettingsStore,
        secrets::{MemoryStore, OBS_PASSWORD_KEY, SecretStore},
    },
};
use tokio::{sync::watch, time::timeout};

fn settings_for(port: u16) -> Settings {
    Settings {
        obs: ObsSettings { host: "127.0.0.1".into(), port },
        panel: Default::default(),
        avatar: Default::default(),
        mic_input: Some("Micro".into()),
        quick_scenes: vec![
            QuickScene { scene: "Pausa / baño".into(), label: "Pausa / baño".into(), mute_mic: true, ..Default::default() },
            QuickScene { scene: "Charla".into(), label: "Charla".into(), ..Default::default() },
        ],
        cameras: vec![
            Camera { id: "movil".into(), name: "Móvil".into(), source: "Cámara móvil".into() },
            Camera { id: "webcam".into(), name: "Webcam".into(), source: "Webcam".into() },
        ],
    }
}

struct Fixture {
    obs: MockObs,
    control: Arc<Control>,
    secrets: Arc<MemoryStore>,
    dir: std::path::PathBuf,
}

async fn fixture(name: &str) -> Fixture {
    let obs = MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store.save(&settings_for(obs.port)).unwrap();
    let secrets = Arc::new(MemoryStore::default());
    let control = Control::start(store, secrets.clone());
    let mut rx = control.subscribe();
    wait(&mut rx, |s| s.connection.state == ConnectionState::Connected && !s.scenes.is_empty() && s.mic_muted.is_some() && !s.cameras.is_empty() && s.cameras.iter().any(|c| c.in_scene)).await;
    Fixture { obs, control, secrets, dir }
}

async fn wait(rx: &mut watch::Receiver<Snapshot>, pred: impl Fn(&Snapshot) -> bool) -> Snapshot {
    timeout(Duration::from_secs(10), async {
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

#[tokio::test]
async fn al_conectar_refleja_escenas_micro_y_camaras_de_obs() {
    let f = fixture("refleja").await;
    let s = f.control.snapshot();
    assert_eq!(s.scenes, ["Juego", "Charla", "Pausa / baño"]);
    assert_eq!(s.current_scene.as_deref(), Some("Juego"));
    assert_eq!(s.mic_muted, Some(false));
    assert!(!s.stream.active);
    let movil = s.cameras.iter().find(|c| c.id == "movil").unwrap();
    let webcam = s.cameras.iter().find(|c| c.id == "webcam").unwrap();
    assert!(movil.in_scene && movil.active);
    assert!(webcam.in_scene && !webcam.active);
}

#[tokio::test]
async fn escena_rapida_silencia_el_micro_y_al_volver_lo_reactiva() {
    let f = fixture("rapida").await;
    let mut rx = f.control.subscribe();

    f.control.launch_scene("Pausa / baño").await.unwrap();
    let s = wait(&mut rx, |s| s.current_scene.as_deref() == Some("Pausa / baño") && s.mic_muted == Some(true)).await;
    assert_eq!(s.previous_scene.as_deref(), Some("Juego"));
    assert!(f.obs.is_muted("Micro"));

    f.control.go_back().await.unwrap();
    let s = wait(&mut rx, |s| s.current_scene.as_deref() == Some("Juego") && s.mic_muted == Some(false)).await;
    assert_eq!(s.previous_scene.as_deref(), Some("Pausa / baño"));
    assert!(!f.obs.is_muted("Micro"));
}

#[tokio::test]
async fn si_el_micro_ya_estaba_silenciado_se_queda_asi_al_salir() {
    let f = fixture("ya-mute").await;
    let mut rx = f.control.subscribe();
    f.control.set_mic_muted(true).await.unwrap();
    wait(&mut rx, |s| s.mic_muted == Some(true)).await;

    f.control.launch_scene("Pausa / baño").await.unwrap();
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Pausa / baño")).await;
    f.control.go_back().await.unwrap();
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Juego")).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(f.obs.is_muted("Micro"), "no debe reactivarse un micro que no silenció la app");
}

#[tokio::test]
async fn cualquier_escena_se_puede_lanzar_sin_tocar_el_micro() {
    let f = fixture("busqueda").await;
    let mut rx = f.control.subscribe();
    // «Juego» no es escena rápida: se lanza igual (RF-36) y no toca el micro.
    f.control.launch_scene("Charla").await.unwrap();
    f.control.launch_scene("Juego").await.unwrap();
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Juego")).await;
    assert!(!f.obs.is_muted("Micro"));
    assert!(f.obs.log().iter().all(|(kind, _)| kind != "SetInputMute"));
}

#[tokio::test]
async fn cambiar_de_camara_muestra_la_nueva_antes_de_ocultar_la_anterior() {
    let f = fixture("camara").await;
    let mut rx = f.control.subscribe();
    let before = f.obs.log().len();

    f.control.switch_camera("webcam").await.unwrap();
    let s = wait(&mut rx, |s| {
        s.cameras.iter().any(|c| c.id == "webcam" && c.active) && s.cameras.iter().any(|c| c.id == "movil" && !c.active)
    })
    .await;
    assert_eq!(s.cameras.iter().filter(|c| c.active).count(), 1);

    let writes: Vec<_> = f.obs.log()[before..]
        .iter()
        .filter(|(kind, _)| kind == "SetSceneItemEnabled")
        .map(|(_, d)| (d["sceneItemId"].as_i64().unwrap(), d["sceneItemEnabled"].as_bool().unwrap()))
        .collect();
    assert_eq!(writes, [(2, true), (1, false)], "primero se enciende la webcam y después se apaga el móvil");

    // Y de vuelta al móvil.
    f.control.switch_camera("movil").await.unwrap();
    wait(&mut rx, |s| s.cameras.iter().any(|c| c.id == "movil" && c.active) && s.cameras.iter().any(|c| c.id == "webcam" && !c.active)).await;
}

#[tokio::test]
async fn camara_que_no_esta_en_la_escena_da_un_error_claro() {
    let f = fixture("sin-camara").await;
    let mut rx = f.control.subscribe();
    f.control.launch_scene("Charla").await.unwrap();
    f.control.launch_scene("Pausa / baño").await.unwrap();
    wait(&mut rx, |s| s.current_scene.as_deref() == Some("Pausa / baño")).await;
    let err = f.control.switch_camera("webcam").await.unwrap_err();
    assert!(err.contains("no está en la escena"), "{err}");
}

#[tokio::test]
async fn inicia_y_para_el_stream_y_la_interfaz_lo_refleja() {
    let f = fixture("stream").await;
    let mut rx = f.control.subscribe();
    f.control.start_stream().await.unwrap();
    wait(&mut rx, |s| s.stream.active).await;
    f.control.stop_stream().await.unwrap();
    wait(&mut rx, |s| !s.stream.active).await;
}

#[tokio::test]
async fn la_contrasena_va_al_almacen_seguro_y_nunca_al_json_ni_a_la_interfaz() {
    let f = fixture("secreto").await;
    f.control.set_obs_password(Some("clave-super-secreta".into())).unwrap();

    assert_eq!(f.secrets.get(OBS_PASSWORD_KEY).unwrap().as_deref(), Some("clave-super-secreta"));
    let snapshot = f.control.snapshot();
    assert!(snapshot.has_password);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("clave-super-secreta"));
    let file = fs::read_to_string(f.dir.join("settings.json")).unwrap();
    assert!(!file.contains("clave-super-secreta"));

    f.control.set_obs_password(None).unwrap();
    assert!(!f.control.snapshot().has_password);
    assert_eq!(f.secrets.get(OBS_PASSWORD_KEY).unwrap(), None);
}

#[tokio::test]
async fn guardar_ajustes_los_persiste_y_rellena_lo_que_falta() {
    let f = fixture("ajustes").await;
    let mut new = f.control.snapshot().settings;
    new.cameras.push(Camera { id: String::new(), name: String::new(), source: "Integrada".into() });
    f.control.save_settings(new).await.unwrap();

    let saved = SettingsStore::new(f.dir.join("settings.json")).load();
    let added = saved.cameras.last().unwrap();
    assert_eq!(added.name, "Integrada");
    assert!(!added.id.is_empty());

    let mut bad = saved.clone();
    bad.quick_scenes.push(bad.quick_scenes[0].clone());
    assert!(f.control.save_settings(bad).await.unwrap_err().contains("duplicada"));
}
