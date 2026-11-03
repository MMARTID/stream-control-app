//! Pegamento con Tauri: comandos que llama la interfaz y evento `state` con el estado completo.

use std::sync::Arc;

use tauri::{Emitter, Manager, State};

use crate::{
    control::{Control, InputView, Snapshot},
    settings::{
        Settings, SettingsStore,
        secrets::KeyringStore,
    },
};

type Ctl<'a> = State<'a, Arc<Control>>;

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
async fn list_inputs(control: Ctl<'_>) -> Result<Vec<InputView>, String> {
    control.list_inputs().await
}

#[tauri::command]
async fn save_settings(control: Ctl<'_>, settings: Settings) -> Result<(), String> {
    control.save_settings(settings).await
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
            let control = tauri::async_runtime::block_on(async { Control::start(store, secrets) });

            let mut rx = control.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while rx.changed().await.is_ok() {
                    let snapshot = rx.borrow_and_update().clone();
                    let _ = handle.emit("state", snapshot);
                }
            });

            app.manage(control);
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
            save_settings,
            set_obs_password,
        ])
        .run(tauri::generate_context!())
        .expect("error al arrancar Stream Control");
}
