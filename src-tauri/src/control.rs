//! Núcleo de la app: refleja el estado de OBS y ejecuta las acciones.
//!
//! OBS es la fuente de verdad (README, §8): aquí solo hay una copia de lo que OBS cuenta,
//! que se rehace al conectar y se mantiene con sus eventos. Las interfaces (la ventana de
//! Tauri hoy, el panel web en la v1) leen `Snapshot` y llaman a los métodos de `Control`.

use std::{collections::HashSet, sync::Arc, sync::Mutex, time::Duration};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::{
    sync::{broadcast, watch},
    time::{Instant, interval},
};

use crate::{
    obs::{ConnectionInfo, ConnectionState, ObsClient, ObsConfig, ObsError, api::SceneItem, protocol::ObsEvent},
    preview::{Interest, PreviewHub},
    settings::{
        AVATAR_CHARACTERS, QuickScene, Settings, SettingsStore,
        secrets::{OBS_PASSWORD_KEY, SecretStore},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct StreamView {
    pub active: bool,
    pub reconnecting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraView {
    pub id: String,
    pub name: String,
    pub source: String,
    /// La fuente existe en la escena en programa.
    pub in_scene: bool,
    /// La fuente está visible en la escena en programa.
    pub active: bool,
}

/// Un elemento de la escena en programa (RF-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// Tiene imagen que mostrar (no es solo audio).
    pub visual: bool,
    pub kind: Option<String>,
}

/// Salud del directo (RF-24). Se mide cada 2 s mientras haya conexión.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub cpu: f64,
    pub fps: f64,
    /// `None` si no se emite o aún no hay dos muestras.
    pub bitrate_kbps: Option<u32>,
    pub dropped_frames: u64,
    pub total_frames: u64,
    pub render_skipped_frames: u64,
    pub render_total_frames: u64,
    pub congestion: f64,
    pub duration_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputView {
    pub name: String,
    pub kind: String,
}

/// Todo lo que la interfaz necesita para pintarse. Nunca incluye la contraseña de OBS.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub connection: ConnectionInfo,
    pub scenes: Vec<String>,
    pub current_scene: Option<String>,
    pub previous_scene: Option<String>,
    pub stream: StreamView,
    /// `None` si no hay micro elegido o aún no se sabe.
    pub mic_muted: Option<bool>,
    pub cameras: Vec<CameraView>,
    /// Todos los elementos de la escena en programa.
    pub sources: Vec<SourceView>,
    pub health: Option<Health>,
    pub settings: Settings,
    pub has_password: bool,
}

/// Escena rápida en curso. Al salir de ella se devuelve el micro a como estaba.
struct QuickRun {
    scene: String,
    /// Entrada que silenciamos nosotros y hay que reactivar al salir.
    unmute_on_exit: Option<String>,
}

struct State {
    settings: Settings,
    quick_run: Option<QuickRun>,
}

pub struct Control {
    obs: ObsClient,
    store: SettingsStore,
    secrets: Arc<dyn SecretStore>,
    state: Mutex<State>,
    snapshot_tx: watch::Sender<Snapshot>,
    preview: Arc<PreviewHub>,
}

/// Nombre de la fuente de navegador que crea la app en OBS.
pub const AVATAR_SOURCE_NAME: &str = "Avatar (Stream Control)";
/// La fuente de navegador mide 800 × 880 (la proporción de los dibujos, 400 × 440), con fondo transparente.
const AVATAR_SOURCE_W: u32 = 800;
const AVATAR_SOURCE_H: u32 = 880;
/// El avatar ocupa esta fracción del alto del lienzo y queda a este margen del borde inferior e izquierdo.
const AVATAR_HEIGHT_FRACTION: f64 = 0.34;
const AVATAR_MARGIN_FRACTION: f64 = 0.02;

/// Resultado de crear la escena del avatar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarSceneReport {
    pub scene: String,
    /// Fuentes copiadas desde la escena de partida.
    pub copied: usize,
    /// Fuentes que no se pudieron copiar.
    pub failed: Vec<String>,
}

/// Posición y escala para dejar el avatar abajo a la izquierda. Los elementos nuevos de OBS nacen con el
/// ancla arriba a la izquierda, así que la posición es la de su esquina superior izquierda.
fn avatar_transform(canvas_h: u32) -> Value {
    let h = f64::from(canvas_h);
    let shown_h = (h * AVATAR_HEIGHT_FRACTION).round();
    let margin = (h * AVATAR_MARGIN_FRACTION).round();
    let scale = shown_h / f64::from(AVATAR_SOURCE_H);
    json!({ "positionX": margin, "positionY": h - margin - shown_h, "scaleX": scale, "scaleY": scale })
}

/// De la transformación que devuelve OBS, solo los campos que se pueden escribir (no el tamaño calculado).
fn writable_transform(t: &Value) -> Value {
    const KEYS: [&str; 14] = [
        "positionX", "positionY", "rotation", "scaleX", "scaleY", "alignment", "boundsType", "boundsAlignment",
        "boundsWidth", "boundsHeight", "cropLeft", "cropRight", "cropTop", "cropBottom",
    ];
    let mut out = serde_json::Map::new();
    for key in KEYS {
        if let Some(v) = t.get(key) {
            out.insert(key.to_owned(), v.clone());
        }
    }
    Value::Object(out)
}

fn msg(e: ObsError) -> String {
    e.to_string()
}

impl Control {
    /// Carga los ajustes, arranca la conexión con OBS y las tareas que mantienen el estado.
    /// Debe llamarse dentro de un runtime de tokio.
    pub fn start(store: SettingsStore, secrets: Arc<dyn SecretStore>) -> Arc<Self> {
        let settings = store.load();
        let password = secrets.get(OBS_PASSWORD_KEY).ok().flatten();
        let obs = ObsClient::spawn(Some(config_from(&settings, password.clone())));
        let snapshot = Snapshot {
            connection: obs.status().borrow().clone(),
            settings: settings.clone(),
            has_password: password.is_some(),
            ..Default::default()
        };
        let (snapshot_tx, snapshot_rx) = watch::channel(snapshot);
        let preview = PreviewHub::start(obs.clone(), move || snapshot_rx.borrow().current_scene.clone());
        let control = Arc::new(Self {
            obs,
            store,
            secrets,
            state: Mutex::new(State { settings, quick_run: None }),
            snapshot_tx,
            preview,
        });

        // Los eventos se escuchan antes de pedir el estado para no perder ninguno.
        let events = control.obs.subscribe_events();
        tokio::spawn(Arc::clone(&control).watch_events(events));
        tokio::spawn(Arc::clone(&control).watch_connection());
        tokio::spawn(Arc::clone(&control).watch_health());
        control
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.snapshot_tx.subscribe()
    }

    pub fn preview(&self) -> &Arc<PreviewHub> {
        &self.preview
    }

    /// Dice qué capturas quiere ver `consumer`. Solo se admiten fuentes de la escena en programa.
    pub fn watch_previews(&self, consumer: &str, mut interest: Interest) {
        let known: HashSet<String> = {
            let s = self.snapshot_tx.borrow();
            s.sources.iter().filter(|x| x.visual).map(|x| x.name.clone()).collect()
        };
        interest.sources.retain(|name| known.contains(name));
        interest.sources.dedup();
        self.preview.set_interest(consumer, interest);
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        self.snapshot_tx.send_if_modified(|snapshot| {
            let before = snapshot.clone();
            f(snapshot);
            *snapshot != before
        });
    }

    fn settings(&self) -> Settings {
        self.state.lock().unwrap().settings.clone()
    }

    fn current_scene(&self) -> Option<String> {
        self.snapshot_tx.borrow().current_scene.clone()
    }

    // ---- Estado: sincronización con OBS ----

    async fn watch_connection(self: Arc<Self>) {
        let mut status = self.obs.status();
        loop {
            let info = status.borrow_and_update().clone();
            let connected = info.state == ConnectionState::Connected;
            self.update(|s| s.connection = info);
            if connected {
                self.refresh_all().await;
            }
            if status.changed().await.is_err() {
                return;
            }
        }
    }

    async fn watch_events(self: Arc<Self>, mut events: broadcast::Receiver<ObsEvent>) {
        loop {
            match events.recv().await {
                Ok(event) => self.on_event(event).await,
                Err(broadcast::error::RecvError::Lagged(_)) => self.refresh_all().await,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    async fn refresh_all(&self) {
        self.refresh_scenes().await;
        if let Ok(status) = self.obs.get_stream_status().await {
            self.update(|s| s.stream = StreamView { active: status.active, reconnecting: status.reconnecting });
        }
        self.refresh_mic().await;
        self.refresh_scene_items().await;
    }

    async fn refresh_scenes(&self) {
        if let Ok(list) = self.obs.get_scene_list().await {
            self.update(|s| {
                if s.current_scene != list.current {
                    if list.current.is_some() && s.current_scene.is_some() {
                        s.previous_scene = s.current_scene.take();
                    }
                    s.current_scene = list.current;
                }
                s.scenes = list.scenes;
            });
        }
    }

    async fn refresh_mic(&self) {
        let Some(input) = self.settings().mic_input else {
            self.update(|s| s.mic_muted = None);
            return;
        };
        let muted = self.obs.get_input_muted(&input).await.ok();
        self.update(|s| s.mic_muted = muted);
    }

    /// Relee los elementos de la escena en programa: alimenta las cámaras y la lista de fuentes.
    async fn refresh_scene_items(&self) {
        let cameras = self.settings().cameras;
        let items = match self.current_scene() {
            Some(scene) => self.obs.get_scene_items(&scene).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let sources = items
            .iter()
            .map(|i| SourceView {
                id: i.id,
                name: i.source_name.clone(),
                enabled: i.enabled,
                visual: is_visual(i.kind.as_deref()),
                kind: i.kind.clone(),
            })
            .collect();
        let views = cameras
            .into_iter()
            .map(|c| {
                let item = items.iter().find(|i| i.source_name == c.source);
                CameraView {
                    in_scene: item.is_some(),
                    active: item.is_some_and(|i| i.enabled),
                    id: c.id,
                    name: c.name,
                    source: c.source,
                }
            })
            .collect();
        self.update(|s| {
            s.cameras = views;
            s.sources = sources;
        });
    }

    async fn on_event(&self, event: ObsEvent) {
        let data = event.event_data.unwrap_or(Value::Null);
        let text = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_owned);
        match event.event_type.as_str() {
            "CurrentProgramSceneChanged" => {
                let Some(scene) = text("sceneName") else { return };
                self.update(|s| {
                    if s.current_scene.as_deref() != Some(scene.as_str()) {
                        s.previous_scene = s.current_scene.take();
                        s.current_scene = Some(scene.clone());
                    }
                });
                self.leave_quick_scene_if_needed(&scene).await;
                self.refresh_scene_items().await;
            }
            "SceneListChanged" | "SceneCreated" | "SceneRemoved" | "SceneNameChanged" => {
                self.refresh_scenes().await;
            }
            "StreamStateChanged" => {
                let active = data.get("outputActive").and_then(Value::as_bool).unwrap_or(false);
                let reconnecting = text("outputState").as_deref() == Some("OBS_WEBSOCKET_OUTPUT_RECONNECTING");
                self.update(|s| s.stream = StreamView { active, reconnecting });
            }
            "InputMuteStateChanged" => {
                if text("inputName") == self.settings().mic_input {
                    let muted = data.get("inputMuted").and_then(Value::as_bool);
                    self.update(|s| s.mic_muted = muted);
                }
            }
            "SceneItemEnableStateChanged" | "SceneItemCreated" | "SceneItemRemoved" | "SceneItemListReindexed" => {
                if text("sceneName") == self.current_scene() {
                    self.refresh_scene_items().await;
                }
            }
            _ => {}
        }
    }

    /// RF-37: al salir de una escena rápida con acción de micro, se reactiva el micro
    /// (solo si lo silenciamos nosotros al entrar).
    async fn leave_quick_scene_if_needed(&self, new_scene: &str) {
        let run = {
            let mut state = self.state.lock().unwrap();
            match &state.quick_run {
                Some(run) if run.scene != new_scene => state.quick_run.take(),
                _ => None,
            }
        };
        if let Some(input) = run.and_then(|r| r.unmute_on_exit) {
            let _ = self.obs.set_input_muted(&input, false).await;
        }
    }

    /// RF-24: carga de OBS, FPS, bitrate y frames perdidos.
    async fn watch_health(self: Arc<Self>) {
        let mut tick = interval(Duration::from_secs(2));
        let mut last_bytes: Option<(u64, Instant)> = None;
        loop {
            tick.tick().await;
            if self.snapshot_tx.borrow().connection.state != ConnectionState::Connected {
                last_bytes = None;
                self.update(|s| s.health = None);
                continue;
            }
            let (stats, stream) = tokio::join!(self.obs.get_stats(), self.obs.get_stream_status());
            let (Ok(stats), Ok(stream)) = (stats, stream) else { continue };
            let now = Instant::now();
            let bitrate_kbps = match last_bytes {
                Some((bytes, at)) if stream.active && stream.bytes >= bytes => {
                    let secs = now.duration_since(at).as_secs_f64();
                    (secs > 0.2).then(|| ((stream.bytes - bytes) as f64 * 8.0 / 1000.0 / secs).round() as u32)
                }
                _ => None,
            };
            last_bytes = Some((stream.bytes, now));
            let round1 = |x: f64| (x * 10.0).round() / 10.0;
            let health = Health {
                cpu: round1(stats.cpu_usage),
                fps: round1(stats.active_fps),
                bitrate_kbps,
                dropped_frames: stream.skipped_frames,
                total_frames: stream.total_frames,
                render_skipped_frames: stats.render_skipped_frames,
                render_total_frames: stats.render_total_frames,
                congestion: (stream.congestion * 100.0).round() / 100.0,
                duration_secs: if stream.active { stream.duration_ms / 1000 } else { 0 },
            };
            self.update(|s| s.health = Some(health));
        }
    }

    // ---- Acciones ----

    /// Lanza una escena de OBS (RF-11). Si es una escena rápida con acciones, las aplica (RF-37).
    pub async fn launch_scene(&self, scene: &str) -> Result<(), String> {
        let settings = self.settings();
        let quick = settings.quick_scenes.iter().find(|q| q.scene == scene);
        let wants_mute = quick.is_some_and(|q| q.mute_mic).then(|| settings.mic_input.clone()).flatten();
        if quick.is_some_and(|q| q.mute_mic) && wants_mute.is_none() {
            return Err("Esta escena silencia el micro, pero no hay micro elegido en Ajustes".into());
        }

        // Lo que ya silenciamos al entrar en una escena rápida anterior; se saca de `quick_run`
        // para que el evento de cambio de escena no lo reactive a mitad de camino.
        let carried = self.state.lock().unwrap().quick_run.take().and_then(|r| r.unmute_on_exit);

        let mut unmute_on_exit = None;
        if let Some(input) = &wants_mute {
            if carried.as_deref() == Some(input.as_str()) {
                unmute_on_exit = carried.clone();
            } else if !self.obs.get_input_muted(input).await.map_err(msg)? {
                self.obs.set_input_muted(input, true).await.map_err(msg)?;
                unmute_on_exit = Some(input.clone());
            }
        }
        if let Some(input) = &carried {
            if unmute_on_exit.as_ref() != Some(input) {
                self.obs.set_input_muted(input, false).await.map_err(msg)?;
            }
        }

        if let Err(e) = self.obs.set_current_scene(scene).await {
            if let Some(input) = &unmute_on_exit {
                let _ = self.obs.set_input_muted(input, false).await;
            }
            return Err(msg(e));
        }
        if quick.is_some() {
            self.state.lock().unwrap().quick_run = Some(QuickRun { scene: scene.to_owned(), unmute_on_exit });
        }
        Ok(())
    }

    /// RF-15: vuelve a la escena anterior. El micro se reactiva solo, al salir de la escena rápida.
    pub async fn go_back(&self) -> Result<(), String> {
        let previous = self
            .snapshot_tx
            .borrow()
            .previous_scene
            .clone()
            .ok_or("Todavía no hay una escena anterior")?;
        self.obs.set_current_scene(&previous).await.map_err(msg)
    }

    /// RF-6: muestra primero la cámara elegida y después oculta las demás, para que
    /// nunca haya un instante sin cámara visible.
    pub async fn switch_camera(&self, camera_id: &str) -> Result<(), String> {
        let settings = self.settings();
        let target = settings
            .cameras
            .iter()
            .find(|c| c.id == camera_id)
            .ok_or("Esa cámara ya no existe en los ajustes")?;
        let scene = self.current_scene().ok_or("No hay escena en programa")?;
        let items = self.obs.get_scene_items(&scene).await.map_err(msg)?;

        let target_item = items
            .iter()
            .find(|i| i.source_name == target.source)
            .ok_or_else(|| format!("La fuente «{}» no está en la escena «{scene}»", target.source))?;
        if !target_item.enabled {
            self.obs.set_scene_item_enabled(&scene, target_item.id, true).await.map_err(msg)?;
        }

        let others: HashSet<&str> =
            settings.cameras.iter().filter(|c| c.id != target.id).map(|c| c.source.as_str()).collect();
        for item in items.iter().filter(|i| i.enabled && i.id != target_item.id && others.contains(i.source_name.as_str())) {
            self.obs.set_scene_item_enabled(&scene, item.id, false).await.map_err(msg)?;
        }
        Ok(())
    }

    /// RF-12: muestra u oculta un elemento de la escena en programa.
    pub async fn set_source_enabled(&self, item_id: i64, enabled: bool) -> Result<(), String> {
        let scene = self.current_scene().ok_or("No hay escena en programa")?;
        let known = self.snapshot_tx.borrow().sources.iter().any(|s| s.id == item_id);
        if !known {
            return Err("Esa fuente ya no está en la escena".into());
        }
        self.obs.set_scene_item_enabled(&scene, item_id, enabled).await.map_err(msg)
    }

    pub async fn set_mic_muted(&self, muted: bool) -> Result<(), String> {
        let input = self.settings().mic_input.ok_or("Elige primero el micro en Ajustes")?;
        self.obs.set_input_muted(&input, muted).await.map_err(msg)
    }

    pub async fn toggle_mic(&self) -> Result<(), String> {
        let input = self.settings().mic_input.ok_or("Elige primero el micro en Ajustes")?;
        let muted = self.obs.get_input_muted(&input).await.map_err(msg)?;
        self.obs.set_input_muted(&input, !muted).await.map_err(msg)
    }

    pub async fn start_stream(&self) -> Result<(), String> {
        self.obs.start_stream().await.map_err(msg)
    }

    pub async fn stop_stream(&self) -> Result<(), String> {
        self.obs.stop_stream().await.map_err(msg)
    }

    pub async fn list_inputs(&self) -> Result<Vec<InputView>, String> {
        let inputs = self.obs.get_input_list().await.map_err(msg)?;
        Ok(inputs.into_iter().map(|i| InputView { name: i.name, kind: i.kind }).collect())
    }

    /// Añade el avatar a la escena en programa como fuente de navegador de OBS, abajo a la izquierda.
    /// Solo se hace cuando el usuario pulsa el botón, nunca por su cuenta.
    pub async fn create_avatar_source(&self) -> Result<String, String> {
        let scene = self.current_scene().ok_or("No hay escena en programa")?;
        let items = self.obs.get_scene_items(&scene).await.map_err(msg)?;
        if items.iter().any(|i| i.source_name == AVATAR_SOURCE_NAME) {
            return Err(format!("La escena «{scene}» ya tiene el avatar"));
        }
        let (_, canvas_h) = self.obs.get_canvas_size().await.map_err(msg)?;
        self.add_avatar_item(&scene, canvas_h).await?;
        Ok(scene)
    }

    /// Crea la escena del avatar (nombre en los ajustes, «Avatar» por defecto): el avatar abajo a la izquierda
    /// en lugar de la cámara. Con `copy_sources`, copia las fuentes de la escena en programa **menos las cámaras**
    /// (juego, marcos, textos…) con su posición y tamaño, para que sea la misma pantalla sin tu cara.
    /// No toca ninguna escena que ya exista.
    pub async fn create_avatar_scene(&self, copy_sources: bool, add_quick: bool) -> Result<AvatarSceneReport, String> {
        let settings = self.settings();
        let name = settings.avatar.scene.trim().to_owned();
        if name.is_empty() {
            return Err("Falta el nombre de la escena del avatar".into());
        }
        let exists = self.snapshot_tx.borrow().scenes.contains(&name);
        if exists {
            return Err(format!("Ya existe una escena «{name}» en OBS. Cámbiale el nombre o bórrala y vuelve a intentarlo"));
        }
        let origin = self.current_scene();
        let (_, canvas_h) = self.obs.get_canvas_size().await.map_err(msg)?;

        match self.obs.create_scene(&name).await {
            Ok(()) => {}
            Err(ObsError::Request { code: 601, .. }) => {
                return Err(format!("Ya existe una escena «{name}» en OBS. Cámbiale el nombre o bórrala y vuelve a intentarlo"));
            }
            Err(e) => return Err(msg(e)),
        }

        let mut copied = 0;
        let mut failed = Vec::new();
        if let (true, Some(origin)) = (copy_sources, origin) {
            let cameras: HashSet<&str> = settings.cameras.iter().map(|c| c.source.as_str()).collect();
            let mut items = self.obs.get_scene_items(&origin).await.map_err(msg)?;
            // De abajo arriba, para conservar el orden de apilado.
            items.sort_by_key(|i| i.index.unwrap_or(i64::MAX));
            for item in items.iter().filter(|i| !cameras.contains(i.source_name.as_str()) && i.source_name != AVATAR_SOURCE_NAME) {
                match self.copy_item(&origin, item, &name).await {
                    Ok(()) => copied += 1,
                    Err(_) => failed.push(item.source_name.clone()),
                }
            }
        }

        self.add_avatar_item(&name, canvas_h).await?;

        if add_quick && !settings.quick_scenes.iter().any(|q| q.scene == name) {
            let mut next = self.settings();
            next.quick_scenes.push(QuickScene {
                scene: name.clone(),
                label: "Avatar".into(),
                color: "#a855f7".into(),
                icon: "🎭".into(),
                mute_mic: false,
            });
            self.save_settings(next).await?;
        }
        Ok(AvatarSceneReport { scene: name, copied, failed })
    }

    /// Copia un elemento a otra escena con su posición, tamaño y recorte.
    async fn copy_item(&self, from: &str, item: &SceneItem, to: &str) -> Result<(), ObsError> {
        let new_id = self.obs.create_scene_item(to, &item.source_name, item.enabled).await?;
        let transform = self.obs.get_scene_item_transform(from, item.id).await?;
        self.obs.set_scene_item_transform(to, new_id, writable_transform(&transform)).await
    }

    /// Pone la fuente de navegador del avatar en una escena, abajo a la izquierda. Si la entrada ya existe
    /// en OBS (de una escena que se borró, por ejemplo), la reutiliza.
    async fn add_avatar_item(&self, scene: &str, canvas_h: u32) -> Result<(), String> {
        let port = self.settings().avatar.port;
        let url = format!("http://127.0.0.1:{port}/avatar");
        let settings = json!({ "url": url, "width": AVATAR_SOURCE_W, "height": AVATAR_SOURCE_H, "fps": 30 });
        let item = match self.obs.create_input(scene, AVATAR_SOURCE_NAME, "browser_source", settings).await {
            Ok(id) => id,
            // 601 = ResourceAlreadyExists
            Err(ObsError::Request { code: 601, .. }) => {
                self.obs.create_scene_item(scene, AVATAR_SOURCE_NAME, true).await.map_err(msg)?
            }
            Err(e) => return Err(msg(e)),
        };
        self.obs.set_scene_item_transform(scene, item, avatar_transform(canvas_h)).await.map_err(msg)
    }

    // ---- Ajustes ----

    pub async fn save_settings(&self, mut new: Settings) -> Result<(), String> {
        validate(&mut new)?;
        self.store.save(&new).map_err(|e| format!("No se pudieron guardar los ajustes: {e}"))?;
        let obs_changed = {
            let mut state = self.state.lock().unwrap();
            let changed = state.settings.obs != new.obs;
            state.settings = new.clone();
            changed
        };
        self.update(|s| s.settings = new);
        if obs_changed {
            self.obs.set_config(Some(self.obs_config()));
        }
        self.refresh_mic().await;
        self.refresh_scene_items().await;
        Ok(())
    }

    /// Guarda (o borra, si va vacía) la contraseña de OBS en el almacén seguro del sistema.
    pub fn set_obs_password(&self, password: Option<String>) -> Result<(), String> {
        let password = password.filter(|p| !p.is_empty());
        match &password {
            Some(p) => self.secrets.set(OBS_PASSWORD_KEY, p)?,
            None => self.secrets.delete(OBS_PASSWORD_KEY)?,
        }
        self.update(|s| s.has_password = password.is_some());
        self.obs.set_config(Some(self.obs_config()));
        Ok(())
    }

    fn obs_config(&self) -> ObsConfig {
        config_from(&self.settings(), self.secrets.get(OBS_PASSWORD_KEY).ok().flatten())
    }
}

fn config_from(settings: &Settings, password: Option<String>) -> ObsConfig {
    ObsConfig { host: settings.obs.host.trim().to_owned(), port: settings.obs.port, password }
}

fn validate(settings: &mut Settings) -> Result<(), String> {
    if settings.obs.host.trim().is_empty() {
        return Err("Falta la dirección de OBS".into());
    }
    if settings.obs.port == 0 {
        return Err("El puerto de OBS no es válido".into());
    }
    if settings.panel.port != 0 && settings.panel.port < 1024 {
        return Err("El puerto del panel web debe ser 1024 o mayor".into());
    }
    if settings.panel.enabled && settings.panel.port == settings.obs.port {
        return Err("El panel web y OBS no pueden usar el mismo puerto".into());
    }
    if settings.avatar.port != 0 && settings.avatar.port < 1024 {
        return Err("El puerto del avatar debe ser 1024 o mayor".into());
    }
    if settings.avatar.enabled
        && (settings.avatar.port == settings.obs.port || (settings.panel.enabled && settings.avatar.port == settings.panel.port))
    {
        return Err("El avatar necesita un puerto distinto al de OBS y al del panel web".into());
    }
    if settings.avatar.scene.trim().is_empty() || settings.avatar.scene.chars().count() > 100 {
        return Err("El nombre de la escena del avatar no es válido (1 a 100 caracteres)".into());
    }
    if !AVATAR_CHARACTERS.contains(&settings.avatar.character.as_str()) {
        return Err("Ese personaje de avatar no existe".into());
    }
    let mut scenes = HashSet::new();
    for q in &mut settings.quick_scenes {
        if q.scene.trim().is_empty() {
            return Err("Una escena rápida no tiene escena de OBS".into());
        }
        if !scenes.insert(q.scene.clone()) {
            return Err(format!("La escena «{}» está duplicada en las escenas rápidas", q.scene));
        }
        if q.label.trim().is_empty() {
            q.label = q.scene.clone();
        }
    }
    let mut ids = HashSet::new();
    for c in &mut settings.cameras {
        if c.source.trim().is_empty() {
            return Err("Una cámara no tiene fuente de OBS".into());
        }
        if c.name.trim().is_empty() {
            c.name = c.source.clone();
        }
        if c.id.is_empty() {
            c.id = uuid::Uuid::new_v4().to_string();
        }
        if !ids.insert(c.id.clone()) {
            return Err("Hay dos cámaras con el mismo identificador".into());
        }
    }
    Ok(())
}

/// Los elementos solo de audio no tienen imagen que capturar.
fn is_visual(kind: Option<&str>) -> bool {
    let Some(kind) = kind else { return true };
    !["wasapi_", "coreaudio", "pulse_", "alsa_", "jack_", "sndio", "oss_", "audio"]
        .iter()
        .any(|audio| kind.contains(audio))
}
