//! Almacén seguro del sistema (RF-31): Credential Manager en Windows, Llavero en macOS.
//! Aquí van la contraseña de OBS y, más adelante, los tokens de chat. Nunca van al JSON de ajustes.

use std::{collections::HashMap, sync::Mutex};

pub const OBS_PASSWORD_KEY: &str = "obs-password";
/// Hash (Argon2) de la contraseña del panel web.
pub const PANEL_PASSWORD_KEY: &str = "panel-password-hash";
/// Clave privada del certificado HTTPS del panel.
pub const PANEL_TLS_KEY: &str = "panel-tls-key";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_owned() }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, key)
            .map_err(|e| format!("No se pudo abrir el almacén seguro del sistema: {e}"))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("No se pudo leer del almacén seguro: {e}")),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| format!("No se pudo guardar en el almacén seguro: {e}"))
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("No se pudo borrar del almacén seguro: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Usa el almacén REAL del sistema (Credential Manager en Windows, Llavero en macOS),
    /// por eso está ignorada por defecto. Ejecutar a mano en cada plataforma:
    /// `cargo test keyring_real -- --ignored`
    #[test]
    #[ignore = "toca el almacén seguro real del sistema"]
    fn keyring_real_guarda_lee_y_borra() {
        let store = KeyringStore::new("app.streamcontrol.desktop.test");
        store.delete("prueba").unwrap();
        assert_eq!(store.get("prueba").unwrap(), None);
        store.set("prueba", "valor-de-prueba").unwrap();
        assert_eq!(store.get("prueba").unwrap().as_deref(), Some("valor-de-prueba"));
        store.delete("prueba").unwrap();
        assert_eq!(store.get("prueba").unwrap(), None);
    }
}

/// Almacén en memoria para pruebas. No se usa en la app.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
