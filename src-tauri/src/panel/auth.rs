//! Acceso al panel web (RF-43): contraseña + código de emparejamiento + una credencial por dispositivo.
//!
//! Flujo:
//! 1. El móvil envía la contraseña del panel y un nombre. Si es correcta, el servidor crea un
//!    emparejamiento con un código de 6 dígitos que **solo se ve en la app de escritorio**.
//! 2. El usuario escribe ese código en el móvil. Con él, el servidor entrega una credencial
//!    (token) propia de ese dispositivo.
//! 3. A partir de ahí el móvil usa el token. Del token solo se guarda su hash.
//! 4. Revocar un dispositivo borra su registro y corta sus conexiones abiertas al instante.
//!
//! Los intentos fallidos se frenan por dirección IP con un bloqueo creciente.

use std::{
    collections::HashMap,
    fs,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, watch};

use crate::settings::secrets::{PANEL_PASSWORD_KEY, SecretStore};

const PAIRING_TTL: Duration = Duration::from_secs(120);
const PAIRING_MAX_ATTEMPTS: u8 = 5;
const MAX_PENDING_PAIRINGS: usize = 5;
const FAILS_BEFORE_LOCK: u32 = 5;
const LOCK_BASE: Duration = Duration::from_secs(60);
const LOCK_MAX: Duration = Duration::from_secs(15 * 60);
pub const MIN_PASSWORD_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("Demasiados intentos fallidos. Espera {0} s e inténtalo de nuevo")]
    Locked(u64),
    #[error("Contraseña incorrecta")]
    BadPassword,
    #[error("El panel aún no tiene contraseña: créala en la app de escritorio")]
    NoPassword,
    #[error("Hay demasiados emparejamientos pendientes. Espera un par de minutos")]
    TooManyPairings,
    #[error("El emparejamiento ha caducado. Vuelve a empezar")]
    PairingExpired,
    #[error("Código incorrecto")]
    BadCode,
    #[error("{0}")]
    Invalid(String),
}

/// Dispositivo emparejado. Solo se guarda el hash del token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub created_at: u64,
    pub last_seen: u64,
    pub last_ip: String,
}

struct Pairing {
    name: String,
    code: String,
    ip: IpAddr,
    expires: Instant,
    expires_at: u64,
    attempts: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    pub id: String,
    pub name: String,
    pub code: String,
    pub ip: String,
    pub expires_at: u64,
}

#[derive(Default)]
struct Fails {
    count: u32,
    locks: u32,
    until: Option<Instant>,
}

pub struct Auth {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    devices: Mutex<Vec<Device>>,
    pairings: Mutex<HashMap<String, Pairing>>,
    fails: Mutex<HashMap<IpAddr, Fails>>,
    revoked: broadcast::Sender<String>,
    changed: watch::Sender<u64>,
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut out = String::new();
    while out.len() < bytes * 2 {
        out.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    out.truncate(bytes * 2);
    out
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hash_password(password: &str, salt: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| format!("No se pudo calcular el hash de la contraseña: {e}"))?;
    Ok(out)
}

fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() { "Dispositivo sin nombre".into() } else { cleaned }
}

impl Auth {
    pub fn new(dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        let path = dir.join("panel-devices.json");
        let devices = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            secrets,
            devices: Mutex::new(devices),
            pairings: Mutex::new(HashMap::new()),
            fails: Mutex::new(HashMap::new()),
            revoked: broadcast::channel(16).0,
            changed: watch::channel(0).0,
        }
    }

    /// Avisa de que cambió algo visible en la app de escritorio (dispositivos, emparejamientos).
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Para quien necesite avisar de un cambio visible (p. ej. un móvil que se conecta).
    pub fn notify(&self) {
        self.bump();
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    /// Identificadores de los dispositivos que se acaban de revocar.
    pub fn revocations(&self) -> broadcast::Receiver<String> {
        self.revoked.subscribe()
    }

    fn save(&self, devices: &[Device]) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(devices) {
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(tmp, &self.path);
            }
        }
    }

    // ---- Contraseña del panel ----

    pub fn has_password(&self) -> bool {
        self.secrets.get(PANEL_PASSWORD_KEY).ok().flatten().is_some()
    }

    /// Guarda el hash Argon2id (sal aleatoria) en el almacén seguro del sistema.
    pub fn set_password(&self, password: &str) -> Result<(), AuthError> {
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(AuthError::Invalid(format!("La contraseña del panel necesita al menos {MIN_PASSWORD_LEN} caracteres")));
        }
        let salt = uuid::Uuid::new_v4().into_bytes();
        let hash = hash_password(password, &salt).map_err(AuthError::Invalid)?;
        let stored = format!("argon2id${}${}", STANDARD_NO_PAD.encode(salt), STANDARD_NO_PAD.encode(hash));
        self.secrets.set(PANEL_PASSWORD_KEY, &stored).map_err(AuthError::Invalid)?;
        self.bump();
        Ok(())
    }

    pub fn clear_password(&self) -> Result<(), AuthError> {
        self.secrets.delete(PANEL_PASSWORD_KEY).map_err(AuthError::Invalid)?;
        self.bump();
        Ok(())
    }

    fn verify_password(&self, password: &str) -> Result<bool, AuthError> {
        let stored = self.secrets.get(PANEL_PASSWORD_KEY).map_err(AuthError::Invalid)?.ok_or(AuthError::NoPassword)?;
        let mut parts = stored.split('$');
        let (Some("argon2id"), Some(salt), Some(hash)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(AuthError::Invalid("El hash guardado de la contraseña no es válido".into()));
        };
        let (Ok(salt), Ok(expected)) = (STANDARD_NO_PAD.decode(salt), STANDARD_NO_PAD.decode(hash)) else {
            return Err(AuthError::Invalid("El hash guardado de la contraseña no es válido".into()));
        };
        let got = hash_password(password, &salt).map_err(AuthError::Invalid)?;
        Ok(constant_time_eq(&got, &expected))
    }

    // ---- Freno de intentos ----

    fn check_locked(&self, ip: IpAddr) -> Result<(), AuthError> {
        let fails = self.fails.lock().unwrap();
        if let Some(until) = fails.get(&ip).and_then(|f| f.until) {
            if until > Instant::now() {
                return Err(AuthError::Locked((until - Instant::now()).as_secs() + 1));
            }
        }
        Ok(())
    }

    fn register_failure(&self, ip: IpAddr) {
        let mut fails = self.fails.lock().unwrap();
        let entry = fails.entry(ip).or_default();
        entry.count += 1;
        if entry.count >= FAILS_BEFORE_LOCK {
            entry.count = 0;
            let lock = (LOCK_BASE * 2u32.saturating_pow(entry.locks)).min(LOCK_MAX);
            entry.locks += 1;
            entry.until = Some(Instant::now() + lock);
        }
    }

    fn register_success(&self, ip: IpAddr) {
        self.fails.lock().unwrap().remove(&ip);
    }

    // ---- Emparejamiento ----

    /// Paso 1: comprueba la contraseña y crea un emparejamiento pendiente. El código no se devuelve.
    pub fn start_pairing(&self, ip: IpAddr, password: &str, name: &str) -> Result<String, AuthError> {
        self.check_locked(ip)?;
        if !self.verify_password(password)? {
            self.register_failure(ip);
            return Err(AuthError::BadPassword);
        }
        self.register_success(ip);

        let mut pairings = self.pairings.lock().unwrap();
        pairings.retain(|_, p| p.expires > Instant::now());
        if pairings.len() >= MAX_PENDING_PAIRINGS {
            return Err(AuthError::TooManyPairings);
        }
        let id = random_hex(16);
        let code = format!("{:06}", u32::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 1_000_000);
        pairings.insert(
            id.clone(),
            Pairing {
                name: clean_name(name),
                code,
                ip,
                expires: Instant::now() + PAIRING_TTL,
                expires_at: now_secs() + PAIRING_TTL.as_secs(),
                attempts: 0,
            },
        );
        drop(pairings);
        self.bump();
        Ok(id)
    }

    /// Paso 2: con el código visto en el escritorio se entrega el token del dispositivo.
    /// Devuelve `(token, dispositivo)`.
    pub fn finish_pairing(&self, ip: IpAddr, pairing_id: &str, code: &str) -> Result<(String, Device), AuthError> {
        self.check_locked(ip)?;
        let mut pairings = self.pairings.lock().unwrap();
        pairings.retain(|_, p| p.expires > Instant::now());
        let Some(pairing) = pairings.get_mut(pairing_id) else {
            return Err(AuthError::PairingExpired);
        };
        // Un emparejamiento solo lo puede cerrar el mismo dispositivo que lo abrió.
        if pairing.ip != ip {
            return Err(AuthError::PairingExpired);
        }
        if !constant_time_eq(pairing.code.as_bytes(), code.trim().as_bytes()) {
            pairing.attempts += 1;
            if pairing.attempts >= PAIRING_MAX_ATTEMPTS {
                pairings.remove(pairing_id);
            }
            drop(pairings);
            self.register_failure(ip);
            self.bump();
            return Err(AuthError::BadCode);
        }
        let pairing = pairings.remove(pairing_id).expect("existe");
        drop(pairings);
        self.register_success(ip);

        let token = format!("scd_{}", random_hex(32));
        let now = now_secs();
        let device = Device {
            id: random_hex(8),
            name: pairing.name,
            token_hash: sha256_hex(&token),
            created_at: now,
            last_seen: now,
            last_ip: ip.to_string(),
        };
        let mut devices = self.devices.lock().unwrap();
        devices.push(device.clone());
        self.save(&devices);
        drop(devices);
        self.bump();
        Ok((token, device))
    }

    // ---- Dispositivos ----

    /// Identifica un dispositivo por su token. Los dispositivos revocados dejan de existir aquí.
    pub fn authenticate(&self, token: &str, ip: IpAddr) -> Option<Device> {
        let hash = sha256_hex(token);
        let mut devices = self.devices.lock().unwrap();
        let device = devices.iter_mut().find(|d| constant_time_eq(d.token_hash.as_bytes(), hash.as_bytes()))?;
        let now = now_secs();
        let ip = ip.to_string();
        // Se guarda la última actividad como mucho una vez por minuto.
        if now.saturating_sub(device.last_seen) >= 60 || device.last_ip != ip {
            device.last_seen = now;
            device.last_ip = ip;
            let device = device.clone();
            self.save(&devices);
            return Some(device);
        }
        Some(device.clone())
    }

    pub fn is_valid(&self, device_id: &str) -> bool {
        self.devices.lock().unwrap().iter().any(|d| d.id == device_id)
    }

    pub fn revoke(&self, device_id: &str) -> bool {
        let mut devices = self.devices.lock().unwrap();
        let before = devices.len();
        devices.retain(|d| d.id != device_id);
        let removed = devices.len() != before;
        if removed {
            self.save(&devices);
        }
        drop(devices);
        if removed {
            let _ = self.revoked.send(device_id.to_owned());
            self.bump();
        }
        removed
    }

    pub fn devices(&self) -> Vec<Device> {
        self.devices.lock().unwrap().clone()
    }

    pub fn pending(&self) -> Vec<PairingView> {
        let mut pairings = self.pairings.lock().unwrap();
        pairings.retain(|_, p| p.expires > Instant::now());
        let mut views: Vec<_> = pairings
            .iter()
            .map(|(id, p)| PairingView {
                id: id.clone(),
                name: p.name.clone(),
                code: p.code.clone(),
                ip: p.ip.to_string(),
                expires_at: p.expires_at,
            })
            .collect();
        views.sort_by_key(|v| v.expires_at);
        views
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::secrets::MemoryStore;

    fn auth(name: &str) -> Auth {
        let dir = std::env::temp_dir().join(format!("stream-control-auth-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let auth = Auth::new(dir, Arc::new(MemoryStore::default()));
        auth.set_password("contraseña-larga").unwrap();
        auth
    }

    const IP: &str = "192.168.1.50";

    fn ip() -> IpAddr {
        IP.parse().unwrap()
    }

    fn code_of(auth: &Auth, id: &str) -> String {
        auth.pending().into_iter().find(|p| p.id == id).unwrap().code
    }

    #[test]
    fn la_contrasena_se_guarda_como_hash_y_no_en_claro() {
        let secrets = Arc::new(MemoryStore::default());
        let auth = Auth::new(std::env::temp_dir().join("sc-auth-hash"), secrets.clone());
        auth.set_password("contraseña-larga").unwrap();
        let stored = secrets.get(PANEL_PASSWORD_KEY).unwrap().unwrap();
        assert!(stored.starts_with("argon2id$") && !stored.contains("contraseña"));
        assert!(auth.verify_password("contraseña-larga").unwrap());
        assert!(!auth.verify_password("otra-contraseña").unwrap());
    }

    #[test]
    fn rechaza_contrasenas_cortas() {
        let auth = Auth::new(std::env::temp_dir().join("sc-auth-short"), Arc::new(MemoryStore::default()));
        assert!(matches!(auth.set_password("corta"), Err(AuthError::Invalid(_))));
    }

    #[test]
    fn sin_contrasena_configurada_no_se_puede_emparejar() {
        let auth = Auth::new(std::env::temp_dir().join("sc-auth-none"), Arc::new(MemoryStore::default()));
        assert_eq!(auth.start_pairing(ip(), "x", "Móvil").unwrap_err(), AuthError::NoPassword);
    }

    #[test]
    fn el_emparejamiento_completo_entrega_un_token_que_funciona_y_se_puede_revocar() {
        let auth = auth("flujo");
        let id = auth.start_pairing(ip(), "contraseña-larga", "Samsung de Miguel").unwrap();
        // El código no sale del servidor salvo por la vista del escritorio.
        let code = code_of(&auth, &id);
        let (token, device) = auth.finish_pairing(ip(), &id, &code).unwrap();

        assert!(token.starts_with("scd_") && token.len() == 68);
        assert_eq!(device.name, "Samsung de Miguel");
        assert_eq!(auth.authenticate(&token, ip()).unwrap().id, device.id);
        assert!(auth.authenticate("scd_inventado", ip()).is_none());

        // En disco solo queda el hash.
        let saved = fs::read_to_string(&auth.path).unwrap();
        assert!(!saved.contains(&token) && saved.contains(&sha256_hex(&token)));

        let mut revoked = auth.revocations();
        assert!(auth.revoke(&device.id));
        assert_eq!(revoked.try_recv().unwrap(), device.id);
        assert!(auth.authenticate(&token, ip()).is_none(), "un dispositivo revocado deja de entrar al instante");
    }

    #[test]
    fn un_codigo_incorrecto_no_da_token_y_tras_cinco_fallos_se_anula_el_emparejamiento() {
        let auth = auth("codigo");
        let id = auth.start_pairing(ip(), "contraseña-larga", "Móvil").unwrap();
        let wrong = if code_of(&auth, &id) == "000000" { "111111" } else { "000000" };
        for _ in 0..4 {
            assert_eq!(auth.finish_pairing(ip(), &id, wrong).unwrap_err(), AuthError::BadCode);
        }
        assert_eq!(auth.finish_pairing(ip(), &id, wrong).unwrap_err(), AuthError::BadCode);
        assert!(auth.pending().is_empty(), "tras el quinto fallo el emparejamiento se borra");
    }

    #[test]
    fn otro_dispositivo_no_puede_cerrar_un_emparejamiento_ajeno() {
        let auth = auth("ajeno");
        let id = auth.start_pairing(ip(), "contraseña-larga", "Móvil").unwrap();
        let code = code_of(&auth, &id);
        let other: IpAddr = "192.168.1.99".parse().unwrap();
        assert_eq!(auth.finish_pairing(other, &id, &code).unwrap_err(), AuthError::PairingExpired);
    }

    #[test]
    fn tras_cinco_contrasenas_malas_se_bloquea_la_direccion() {
        let auth = auth("bloqueo");
        for _ in 0..5 {
            assert_eq!(auth.start_pairing(ip(), "mal", "x").unwrap_err(), AuthError::BadPassword);
        }
        assert!(matches!(auth.start_pairing(ip(), "contraseña-larga", "x"), Err(AuthError::Locked(s)) if s > 0 && s <= 61));
        // Otra dirección no queda afectada.
        let other: IpAddr = "192.168.1.60".parse().unwrap();
        assert!(auth.start_pairing(other, "contraseña-larga", "x").is_ok());
    }

    #[test]
    fn los_dispositivos_sobreviven_a_un_reinicio() {
        let dir = std::env::temp_dir().join(format!("stream-control-auth-{}-reinicio", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let secrets = Arc::new(MemoryStore::default());
        let first = Auth::new(dir.clone(), secrets.clone());
        first.set_password("contraseña-larga").unwrap();
        let id = first.start_pairing(ip(), "contraseña-larga", "Móvil").unwrap();
        let code = code_of(&first, &id);
        let (token, _) = first.finish_pairing(ip(), &id, &code).unwrap();

        let second = Auth::new(dir, secrets);
        assert!(second.authenticate(&token, ip()).is_some());
    }
}
