//! Capturas en vivo de la salida (programa) y de las fuentes de la escena.
//!
//! OBS no emite vídeo por obs-websocket: lo que se ve son capturas JPEG pedidas con
//! `GetSourceScreenshot`, unas pocas por segundo. Por eso:
//!
//! - Solo se captura mientras alguien mira (la ventana visible o un móvil con el panel abierto).
//!   Con nadie mirando no se manda ninguna petición a OBS (RNF-1, README §11 «Miniaturas»).
//! - Las capturas van de una en una, nunca en paralelo, y se espacian: el programa a ~3 por
//!   segundo y cada fuente a 1 por segundo. Si OBS tarda, el ritmo baja solo.
//! - Una captura que falla (p. ej. cámara oculta o desconectada) se reintenta cada pocos
//!   segundos en vez de insistir.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Deserialize;
use tokio::{
    sync::{Notify, broadcast},
    time::{Instant, sleep_until},
};

use crate::obs::{ObsClient, ObsError};

/// Clave de la captura de la salida en programa (la escena activa).
pub const PROGRAM_KEY: &str = "@program";

const MAX_SOURCES: usize = 16;
const FAILURE_BACKOFF: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
struct Profile {
    width: u32,
    quality: i32,
    every: Duration,
}

const PROGRAM: Profile = Profile { width: 640, quality: 60, every: Duration::from_millis(333) };
const SOURCE: Profile = Profile { width: 320, quality: 55, every: Duration::from_millis(1000) };

/// Lo que quiere ver un consumidor (la ventana de escritorio o un móvil).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Interest {
    pub program: bool,
    pub sources: Vec<String>,
}

impl Interest {
    pub fn is_empty(&self) -> bool {
        !self.program && self.sources.is_empty()
    }
}

#[derive(Debug)]
pub struct Frame {
    /// `PROGRAM_KEY` o el nombre de la fuente.
    pub key: String,
    pub seq: u64,
    /// Data URI listo para un `<img src>`.
    pub image: String,
}

pub struct PreviewHub {
    obs: ObsClient,
    program_scene: Box<dyn Fn() -> Option<String> + Send + Sync>,
    interests: Mutex<HashMap<String, Interest>>,
    latest: Mutex<HashMap<String, Arc<Frame>>>,
    frames: broadcast::Sender<Arc<Frame>>,
    wake: Notify,
}

impl PreviewHub {
    /// Arranca la tarea de captura. `program_scene` dice qué escena está en programa.
    /// Debe llamarse dentro de un runtime de tokio.
    pub fn start(obs: ObsClient, program_scene: impl Fn() -> Option<String> + Send + Sync + 'static) -> Arc<Self> {
        let (frames, _) = broadcast::channel(32);
        let hub = Arc::new(Self {
            obs,
            program_scene: Box::new(program_scene),
            interests: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
            frames,
            wake: Notify::new(),
        });
        tokio::spawn(Arc::clone(&hub).run());
        hub
    }

    /// Fija lo que quiere ver `consumer`. Un interés vacío lo da de baja.
    pub fn set_interest(&self, consumer: &str, interest: Interest) {
        {
            let mut interests = self.interests.lock().unwrap();
            if interest.is_empty() {
                interests.remove(consumer);
            } else {
                interests.insert(consumer.to_owned(), interest);
            }
        }
        let wanted: HashSet<String> = self.targets().into_iter().map(|(key, _)| key).collect();
        self.latest.lock().unwrap().retain(|key, _| wanted.contains(key));
        self.wake.notify_one();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Frame>> {
        self.frames.subscribe()
    }

    /// Últimas capturas de lo que pide `interest`, para quien acaba de conectarse.
    pub fn latest_for(&self, interest: &Interest) -> Vec<Arc<Frame>> {
        let latest = self.latest.lock().unwrap();
        let program = interest.program.then(|| latest.get(PROGRAM_KEY)).flatten();
        let sources = interest.sources.iter().filter_map(|s| latest.get(s));
        program.into_iter().chain(sources).cloned().collect()
    }

    fn targets(&self) -> Vec<(String, Profile)> {
        let interests = self.interests.lock().unwrap();
        let mut targets = Vec::new();
        if interests.values().any(|i| i.program) {
            targets.push((PROGRAM_KEY.to_owned(), PROGRAM));
        }
        let mut seen = HashSet::new();
        for source in interests.values().flat_map(|i| &i.sources) {
            if targets.len() > MAX_SOURCES {
                break;
            }
            if seen.insert(source.clone()) {
                targets.push((source.clone(), SOURCE));
            }
        }
        targets
    }

    async fn run(self: Arc<Self>) {
        let mut next_due: HashMap<String, Instant> = HashMap::new();
        let mut format: Option<&'static str> = None;
        let mut seq = 0u64;
        loop {
            let targets = self.targets();
            if targets.is_empty() {
                next_due.clear();
                self.wake.notified().await;
                continue;
            }
            let now = Instant::now();
            next_due.retain(|key, _| targets.iter().any(|(t, _)| t == key));
            let (key, profile, due) = targets
                .iter()
                .map(|(key, profile)| (key.clone(), *profile, *next_due.get(key).unwrap_or(&now)))
                .min_by_key(|(_, _, due)| *due)
                .expect("hay al menos un objetivo");
            if due > now {
                tokio::select! {
                    _ = sleep_until(due) => {}
                    _ = self.wake.notified() => {}
                }
                continue;
            }

            let started = Instant::now();
            match self.capture(&key, profile, &mut format).await {
                Ok(image) => {
                    seq += 1;
                    let frame = Arc::new(Frame { key: key.clone(), seq, image });
                    self.latest.lock().unwrap().insert(key.clone(), Arc::clone(&frame));
                    let _ = self.frames.send(frame);
                    next_due.insert(key, started + profile.every);
                }
                Err(_) => {
                    format = None;
                    next_due.insert(key, Instant::now() + FAILURE_BACKOFF);
                }
            }
        }
    }

    async fn capture(
        &self,
        key: &str,
        profile: Profile,
        format: &mut Option<&'static str>,
    ) -> Result<String, ObsError> {
        let name = if key == PROGRAM_KEY {
            (self.program_scene)().ok_or_else(|| ObsError::Protocol("no hay escena en programa".into()))?
        } else {
            key.to_owned()
        };
        let format = match *format {
            Some(f) => f,
            None => {
                let formats = self.obs.get_image_formats().await?;
                let chosen = if formats.iter().any(|f| f == "jpg") { "jpg" } else { "png" };
                *format = Some(chosen);
                chosen
            }
        };
        self.obs.get_source_screenshot(&name, format, profile.width, profile.quality).await
    }
}
