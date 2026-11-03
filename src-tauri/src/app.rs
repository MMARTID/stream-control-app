//! Pegamento con Tauri: comandos que llama la interfaz y evento `state` con el estado completo.

use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::{
    avatar::{Avatar, AvatarFrame, AvatarStatus},
    control::{Control, InputView, Snapshot},
    panel::{Panel, PanelStatus},
    preview::Interest,
    settings::{
        Settings, SettingsStore,
        secrets::KeyringStore,
    },
};

type Ctl<'a> = State<'a, Arc<Control>>;

/// Identificador de la ventana de escritorio como espectadora de capturas.
const DESKTOP: &str = "desktop";

#[derive(Clone, Serialize)]
struct FramePayload {
    key: String,
    image: String,
}

#[tauri::command]
fn get_snapshot(control: Ctl<'_>) -> Snapshot {
    control.snapshot()
}

#[tauri::command]
async fn launch_scene(control: Ctl<'_>, scene: String) -> Result<(), String> {
    control.launch_scene(&scene).await
}

#[tauri::command]
async fn go_back(control: Ctl<'_>) -> Result<(), String> {
    control.go_back().await
}

#[tauri::command]
async fn switch_camera(control: Ctl<'_>, camera_id: String) -> Result<(), String> {
    control.switch_camera(&camera_id).await
}

#[tauri::command]
async fn toggle_mic(control: Ctl<'_>) -> Result<(), String> {
    control.toggle_mic().await
}

#[tauri::command]
async fn start_stream(control: Ctl<'_>) -> Result<(), String> {
    control.start_stream().await
}

#[tauri::command]
async fn stop_stream(control: Ctl<'_>) -> Result<(), String> {
    control.stop_stream().await
}

#[tauri::command]
fn watch_previews(control: Ctl<'_>, interest: Interest) {
    control.watch_previews(DESKTOP, interest);
}

#[tauri::command]
async fn set_source_enabled(control: Ctl<'_>, id: i64, enabled: bool) -> Result<(), String> {
    control.set_source_enabled(id, enabled).await
}

#[tauri::command]
async fn list_inputs(control: Ctl<'_>) -> Result<Vec<InputView>, String> {
    control.list_inputs().await
}

#[tauri::command]
async fn save_settings(control: Ctl<'_>, settings: Settings) -> Result<(), String> {
    control.save_settings(settings).await
}

#[tauri::command]
fn get_avatar_status(avatar: State<'_, Arc<Avatar>>) -> AvatarStatus {
    avatar.status()
}

/// Fotograma de seguimiento de la ventana (cara o voz) hacia el avatar. Se llama hasta ~30 veces por segundo.
#[tauri::command]
fn avatar_push(avatar: State<'_, Arc<Avatar>>, frame: AvatarFrame) {
    avatar.push(frame);
}

/// Añade el avatar a la escena en programa como fuente de navegador de OBS.
#[tauri::command]
async fn create_avatar_source(control: Ctl<'_>) -> Result<String, String> {
    control.create_avatar_source().await
}

#[tauri::command]
fn get_panel_status(panel: State<'_, Arc<Panel>>) -> PanelStatus {
    panel.status()
}

/// Guarda la contraseña del panel web (nunca vuelve a la interfaz) o la borra si va vacía.
#[tauri::command]
fn set_panel_password(panel: State<'_, Arc<Panel>>, password: Option<String>) -> Result<(), String> {
    match password.filter(|p| !p.is_empty()) {
        Some(p) => panel.set_password(&p),
        None => panel.clear_password(),
    }
}

#[tauri::command]
fn revoke_device(panel: State<'_, Arc<Panel>>, id: String) -> Result<(), String> {
    panel.revoke_device(&id)
}

#[tauri::command]
fn set_obs_password(control: Ctl<'_>, password: Option<String>) -> Result<(), String> {
    control.set_obs_password(password)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let store = SettingsStore::new(dir.join("settings.json"));
            let secrets = Arc::new(KeyringStore::new("app.streamcontrol.desktop"));
            let control = tauri::async_runtime::block_on(async { Control::start(store, secrets.clone()) });
            let panel = tauri::async_runtime::block_on(async {
                Panel::start(Arc::clone(&control), dir.clone(), secrets.clone())
            });

            let avatar = tauri::async_runtime::block_on(async { Avatar::start(Arc::clone(&control)) });
            let mut avatar_rx = avatar.subscribe();
            let avatar_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while avatar_rx.changed().await.is_ok() {
                    let status = avatar_rx.borrow_and_update().clone();
                    let _ = avatar_handle.emit("avatar", status);
                }
            });

            let mut panel_rx = panel.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while panel_rx.changed().await.is_ok() {
                    let status = panel_rx.borrow_and_update().clone();
                    let _ = handle.emit("panel", status);
                }
            });

            let mut rx = control.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while rx.changed().await.is_ok() {
                    let snapshot = rx.borrow_and_update().clone();
                    let _ = handle.emit("state", snapshot);
                }
            });

            let mut frames = control.preview().subscribe();
            let hub = Arc::clone(control.preview());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match frames.recv().await {
                        Ok(frame) if hub.wants(DESKTOP, &frame.key) => {
                            let _ = handle.emit("frame", FramePayload { key: frame.key.clone(), image: frame.image.clone() });
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            app.manage(control);
            app.manage(panel);
            app.manage(avatar);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            launch_scene,
            go_back,
            switch_camera,
            toggle_mic,
            start_stream,
            stop_stream,
            list_inputs,
            watch_previews,
            set_source_enabled,
            save_settings,
            set_obs_password,
            get_avatar_status,
            avatar_push,
            create_avatar_source,
            get_panel_status,
            set_panel_password,
            revoke_device,
        ])
        .run(tauri::generate_context!())
        .expect("error al arrancar Stream Control");
}
