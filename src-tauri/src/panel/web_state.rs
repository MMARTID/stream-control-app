//! Lo que ve el panel web (RF-45). Es un tipo aparte de `Snapshot` a propósito: se copia campo a
//! campo, de modo que un dato nuevo del escritorio (contraseñas, direcciones de OBS, códigos de
//! emparejamiento…) nunca llega al móvil por descuido.

use serde::Serialize;

use crate::{
    control::{Health, Snapshot},
    obs::ConnectionState,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebState {
    pub obs_connected: bool,
    pub scenes: Vec<String>,
    pub current_scene: Option<String>,
    pub previous_scene: Option<String>,
    pub streaming: bool,
    pub stream_reconnecting: bool,
    pub mic_configured: bool,
    pub mic_muted: Option<bool>,
    pub cameras: Vec<WebCamera>,
    pub sources: Vec<WebSource>,
    pub quick_scenes: Vec<WebQuickScene>,
    pub health: Option<Health>,
    pub allow_stream_control: bool,
    pub only_quick_scenes: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebCamera {
    pub id: String,
    pub name: String,
    /// Clave de la captura de esta cámara.
    pub preview: String,
    pub in_scene: bool,
    pub active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSource {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub visual: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebQuickScene {
    pub scene: String,
    pub label: String,
    pub color: String,
    pub icon: String,
    pub mute_mic: bool,
}

impl From<&Snapshot> for WebState {
    fn from(s: &Snapshot) -> Self {
        Self {
            obs_connected: s.connection.state == ConnectionState::Connected,
            scenes: s.scenes.clone(),
            current_scene: s.current_scene.clone(),
            previous_scene: s.previous_scene.clone(),
            streaming: s.stream.active,
            stream_reconnecting: s.stream.reconnecting,
            mic_configured: s.settings.mic_input.is_some(),
            mic_muted: s.mic_muted,
            cameras: s
                .cameras
                .iter()
                .map(|c| WebCamera {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    preview: c.source.clone(),
                    in_scene: c.in_scene,
                    active: c.active,
                })
                .collect(),
            sources: s
                .sources
                .iter()
                .map(|x| WebSource { id: x.id, name: x.name.clone(), enabled: x.enabled, visual: x.visual })
                .collect(),
            quick_scenes: s
                .settings
                .quick_scenes
                .iter()
                .map(|q| WebQuickScene {
                    scene: q.scene.clone(),
                    label: q.label.clone(),
                    color: q.color.clone(),
                    icon: q.icon.clone(),
                    mute_mic: q.mute_mic,
                })
                .collect(),
            health: s.health.clone(),
            allow_stream_control: s.settings.panel.allow_stream_control,
            only_quick_scenes: s.settings.panel.only_quick_scenes,
        }
    }
}
