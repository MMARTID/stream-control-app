//! Registro de acciones del panel web (RF-47): cuándo, desde qué dispositivo y qué se hizo.
//! Se guarda en `panel-audit.jsonl` (una línea por acción) y en memoria se conservan las últimas 500.

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::auth::now_secs;

const KEEP: usize = 500;
/// Cuando el archivo pasa de este tamaño se reescribe con solo las últimas entradas.
const MAX_FILE_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Segundos desde 1970.
    pub at: u64,
    pub device: String,
    pub ip: String,
    pub action: String,
    pub detail: String,
    pub ok: bool,
}

pub struct Audit {
    path: PathBuf,
    entries: Mutex<VecDeque<Entry>>,
    changed: watch::Sender<u64>,
}

impl Audit {
    pub fn new(dir: PathBuf) -> Self {
        let path = dir.join("panel-audit.jsonl");
        let mut entries: VecDeque<Entry> = fs::read_to_string(&path)
            .map(|text| text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
            .unwrap_or_default();
        while entries.len() > KEEP {
            entries.pop_front();
        }
        Self { path, entries: Mutex::new(entries), changed: watch::channel(0).0 }
    }

    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub fn record(&self, device: &str, ip: &str, action: &str, detail: &str, ok: bool) {
        let entry = Entry {
            at: now_secs(),
            device: device.to_owned(),
            ip: ip.to_owned(),
            action: action.to_owned(),
            detail: detail.chars().take(200).collect(),
            ok,
        };
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(line) = serde_json::to_string(&entry) {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.path) {
                let _ = writeln!(file, "{line}");
            }
        }
        let mut entries = self.entries.lock().unwrap();
        entries.push_back(entry);
        while entries.len() > KEEP {
            entries.pop_front();
        }
        if fs::metadata(&self.path).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
            let text: String = entries.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
            let _ = fs::write(&self.path, text);
        }
        drop(entries);
        self.changed.send_modify(|n| *n += 1);
    }

    /// Las últimas `n` entradas, de la más reciente a la más antigua.
    pub fn recent(&self, n: usize) -> Vec<Entry> {
        self.entries.lock().unwrap().iter().rev().take(n).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_en_disco_y_recupera_las_ultimas_en_orden_inverso() {
        let dir = std::env::temp_dir().join(format!("stream-control-audit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let audit = Audit::new(dir.clone());
        audit.record("Samsung", "192.168.1.50", "Escena", "Pausa / baño", true);
        audit.record("Samsung", "192.168.1.50", "Stream", "iniciar", false);

        let recent = audit.recent(10);
        assert_eq!(recent[0].action, "Stream");
        assert!(!recent[0].ok);
        assert_eq!(recent[1].detail, "Pausa / baño");

        let reopened = Audit::new(dir);
        assert_eq!(reopened.recent(10), recent);
    }
}
