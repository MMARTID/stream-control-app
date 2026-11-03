//! Ajustes propios de la app (RF-30, RF-39): conexión, micro, escenas rápidas y cámaras.
//! Se guardan en un JSON local. La contraseña de OBS no está aquí: va al almacén seguro.
//! La app nunca escribe los archivos de colección de OBS.

pub mod secrets;

use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
}

impl Default for ObsSettings {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 4455 }
    }
}

/// Referencia por nombre a una escena que ya existe en OBS (RF-38). No la duplica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickScene {
    /// Nombre exacto de la escena en OBS.
    pub scene: String,
    /// Nombre corto que se ve en el botón.
    pub label: String,
    pub color: String,
    pub icon: String,
    /// RF-37: silenciar el micro al entrar y reactivarlo al salir.
    pub mute_mic: bool,
}

impl Default for QuickScene {
    fn default() -> Self {
        Self {
            scene: String::new(),
            label: String::new(),
            color: "#3b82f6".into(),
            icon: "▶".into(),
            mute_mic: false,
        }
    }
}

/// Una cámara es una fuente de OBS dentro de la escena, con el nombre que le da el usuario (RF-5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Camera {
    pub id: String,
    pub name: String,
    /// Nombre de la fuente en OBS.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub obs: ObsSettings,
    /// Entrada de audio de OBS que se silencia con el botón del micro.
    pub mic_input: Option<String>,
    pub quick_scenes: Vec<QuickScene>,
    pub cameras: Vec<Camera>,
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Carga los ajustes. Si el archivo no existe o está dañado, devuelve los de fábrica
    /// (un archivo dañado se aparta como `.corrupto` para no perderlo).
    pub fn load(&self) -> Settings {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
                let _ = fs::rename(&self.path, self.path.with_extension("json.corrupto"));
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    /// Guarda de forma atómica: escribe a un temporal y lo renombra.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
        fs::rename(tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stream-control-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("settings.json")
    }

    #[test]
    fn sin_archivo_devuelve_los_valores_de_fabrica() {
        let store = SettingsStore::new(temp_path("vacio"));
        assert_eq!(store.load(), Settings::default());
        assert_eq!(store.load().obs.port, 4455);
    }

    #[test]
    fn guarda_y_carga_sin_perder_datos() {
        let store = SettingsStore::new(temp_path("ida-vuelta"));
        let mut s = Settings::default();
        s.mic_input = Some("Micro".into());
        s.quick_scenes.push(QuickScene { scene: "Pausa / baño".into(), label: "Pausa".into(), mute_mic: true, ..Default::default() });
        s.cameras.push(Camera { id: "movil".into(), name: "Móvil".into(), source: "OpenCam".into() });
        store.save(&s).unwrap();
        assert_eq!(store.load(), s);
    }

    #[test]
    fn el_json_no_contiene_la_contrasena() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.to_lowercase().contains("password"));
    }

    #[test]
    fn un_archivo_danado_se_aparta_y_se_usan_los_valores_de_fabrica() {
        let path = temp_path("danado");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ esto no es json").unwrap();
        let store = SettingsStore::new(path.clone());
        assert_eq!(store.load(), Settings::default());
        assert!(path.with_extension("json.corrupto").exists());
    }
}
