//! Tiras LED Bluetooth (RF extra del usuario): detectar las tiras cercanas, conectarse y controlarlas.
//!
//! - `protocol`: los comandos de 9 bytes de las tiras «Lotus Lamp» (ver ese módulo).
//! - `bluetooth`: la parte que habla con el Bluetooth del equipo (`btleplug`).
//! - Este archivo: el controlador. Detrás de dos *traits* (`Backend` y `Link`) para poder probarlo sin tira.
//!
//! La tira acepta unas 10 órdenes por segundo y una sola conexión a la vez. El controlador **espacia las
//! órdenes** (100 ms) y **junta los cambios de color seguidos** en el último, para que otras partes de la app
//! (p. ej. una futura sincronización con una fuente de OBS) puedan pedir colores sin preocuparse del ritmo.

pub mod bluetooth;
pub mod protocol;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use serde::Serialize;
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
    time::{interval, sleep},
};

use protocol::Packet;

/// Separación mínima entre dos órdenes (la tira admite ~10 por segundo).
const MIN_GAP: Duration = Duration::from_millis(100);
const SCAN_SECONDS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightDevice {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
    /// Parece una tira de esta familia (nombre «MELK…» o servicio FFF0).
    pub likely: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Adapter {
    #[default]
    Unknown,
    Ready,
    /// Hay Bluetooth, pero está apagado.
    Off,
    /// El equipo no tiene Bluetooth (o no se puede usar).
    Missing,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightsStatus {
    pub adapter: Adapter,
    pub scanning: bool,
    pub connecting: bool,
    pub devices: Vec<LightDevice>,
    pub connected: Option<LightDevice>,
    pub error: Option<String>,
    // Lo último que se le ha pedido a la tira (ella no lo cuenta de vuelta).
    pub power: Option<bool>,
    pub color: Option<[u8; 3]>,
    pub brightness: Option<u8>,
    pub speed: Option<u8>,
    /// Animación en curso; `None` si hay un color fijo.
    pub mode: Option<u8>,
}

/// Lo que hace falta del Bluetooth del equipo.
#[async_trait]
pub trait Backend: Send + Sync {
    async fn adapter_state(&self) -> Adapter;
    async fn scan(&self, seconds: u64) -> Result<Vec<LightDevice>, String>;
    async fn connect(&self, id: &str) -> Result<Arc<dyn Link>, String>;
}

/// Una conexión abierta con una tira.
#[async_trait]
pub trait Link: Send + Sync {
    async fn write(&self, packet: &Packet) -> Result<(), String>;
    async fn connected(&self) -> bool;
    async fn disconnect(&self);
}

#[derive(Default)]
struct Shared {
    queue: Mutex<VecDeque<Packet>>,
    wake: Notify,
}

struct Connection {
    link: Arc<dyn Link>,
    shared: Arc<Shared>,
    writer: JoinHandle<()>,
}

pub struct Lights {
    backend: Arc<dyn Backend>,
    connection: Mutex<Option<Connection>>,
    status_tx: watch::Sender<LightsStatus>,
}

const NOT_CONNECTED: &str = "No hay ninguna tira conectada. Búscala y conéctala primero";

impl Lights {
    pub fn new(backend: Arc<dyn Backend>) -> Arc<Self> {
        Arc::new(Self { backend, connection: Mutex::new(None), status_tx: watch::channel(LightsStatus::default()).0 })
    }

    pub fn status(&self) -> LightsStatus {
        self.status_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<LightsStatus> {
        self.status_tx.subscribe()
    }

    fn update(&self, f: impl FnOnce(&mut LightsStatus)) {
        self.status_tx.send_if_modified(|s| {
            let before = s.clone();
            f(s);
            *s != before
        });
    }

    /// Busca dispositivos Bluetooth durante unos segundos. Las tiras probables salen primero.
    pub async fn scan(&self) -> Result<Vec<LightDevice>, String> {
        self.update(|s| {
            s.scanning = true;
            s.error = None;
        });
        let adapter = self.backend.adapter_state().await;
        let result = self.backend.scan(SCAN_SECONDS).await;
        let result = result.map(|mut devices| {
            // Primero las que parecen tiras y, dentro de cada grupo, las más cercanas (más señal).
            devices.sort_by(|a, b| b.likely.cmp(&a.likely).then(b.rssi.unwrap_or(i16::MIN).cmp(&a.rssi.unwrap_or(i16::MIN))));
            devices
        });
        self.update(|s| {
            s.scanning = false;
            s.adapter = adapter;
            match &result {
                Ok(devices) => s.devices = devices.clone(),
                Err(e) => s.error = Some(e.clone()),
            }
        });
        result
    }

    /// Se conecta a una de las tiras vistas en la última búsqueda.
    pub async fn connect(self: &Arc<Self>, id: &str) -> Result<(), String> {
        self.disconnect().await;
        self.update(|s| {
            s.connecting = true;
            s.error = None;
        });
        match self.backend.connect(id).await {
            Err(e) => {
                self.update(|s| {
                    s.connecting = false;
                    s.error = Some(e.clone());
                });
                Err(e)
            }
            Ok(link) => {
                let device = self.status().devices.into_iter().find(|d| d.id == id).unwrap_or_else(|| LightDevice {
                    id: id.to_owned(),
                    name: "Tira LED".into(),
                    rssi: None,
                    likely: true,
                });
                let shared = Arc::new(Shared::default());
                let writer = tokio::spawn(Arc::clone(self).write_loop(Arc::clone(&link), Arc::clone(&shared)));
                *self.connection.lock().unwrap() = Some(Connection { link, shared, writer });
                self.update(|s| {
                    s.connecting = false;
                    s.connected = Some(device);
                    s.power = None;
                    s.color = None;
                    s.brightness = None;
                    s.speed = None;
                    s.mode = None;
                });
                Ok(())
            }
        }
    }

    /// Al abrir la app: si la tira guardada está a la vista, se conecta a ella. Devuelve si lo consiguió.
    pub async fn connect_saved(self: &Arc<Self>, id: &str) -> bool {
        match self.scan().await {
            Ok(devices) if devices.iter().any(|d| d.id == id) => self.connect(id).await.is_ok(),
            _ => false,
        }
    }

    pub async fn disconnect(&self) {
        let connection = self.connection.lock().unwrap().take();
        if let Some(c) = connection {
            c.writer.abort();
            c.link.disconnect().await;
            self.update(|s| s.connected = None);
        }
    }

    /// Escribe las órdenes de una en una, separadas 100 ms, y detecta cuándo se pierde la conexión.
    async fn write_loop(self: Arc<Self>, link: Arc<dyn Link>, shared: Arc<Shared>) {
        let mut check = interval(Duration::from_secs(3));
        loop {
            let next = shared.queue.lock().unwrap().pop_front();
            match next {
                Some(packet) => {
                    if let Err(e) = link.write(&packet).await {
                        return self.lost(&format!("Se perdió la conexión con la tira ({e}). Vuelve a conectarla"));
                    }
                    sleep(MIN_GAP).await;
                }
                None => tokio::select! {
                    _ = shared.wake.notified() => {}
                    _ = check.tick() => {
                        if !link.connected().await {
                            return self.lost("Se perdió la conexión con la tira. Vuelve a conectarla");
                        }
                    }
                },
            }
        }
    }

    fn lost(&self, message: &str) {
        *self.connection.lock().unwrap() = None;
        self.update(|s| {
            s.connected = None;
            s.error = Some(message.to_owned());
        });
    }

    fn send(&self, packet: Packet) -> Result<(), String> {
        let connection = self.connection.lock().unwrap();
        let c = connection.as_ref().ok_or(NOT_CONNECTED)?;
        {
            let mut queue = c.shared.queue.lock().unwrap();
            // Dos órdenes seguidas del mismo tipo (color, brillo, velocidad, animación, encendido): solo importa
            // la última. La tira no puede seguir el ritmo de un deslizador o de una sincronización con la imagen.
            if queue.back().is_some_and(|last| last[2] == packet[2]) {
                *queue.back_mut().unwrap() = packet;
            } else {
                queue.push_back(packet);
            }
        }
        c.shared.wake.notify_one();
        Ok(())
    }

    // ---- Órdenes ----

    /// Color fijo. Se puede llamar con mucha frecuencia: las órdenes seguidas se juntan.
    pub fn set_color(&self, r: u8, g: u8, b: u8) -> Result<(), String> {
        self.send(protocol::rgb(r, g, b))?;
        self.update(|s| {
            s.color = Some([r, g, b]);
            s.mode = None;
            s.power = Some(true);
        });
        Ok(())
    }

    pub fn set_brightness(&self, level: u8) -> Result<(), String> {
        self.send(protocol::brightness(level))?;
        self.update(|s| s.brightness = Some(level.min(100)));
        Ok(())
    }

    pub fn set_speed(&self, level: u8) -> Result<(), String> {
        self.send(protocol::speed(level))?;
        self.update(|s| s.speed = Some(level.min(100)));
        Ok(())
    }

    pub fn set_mode(&self, mode: u8) -> Result<(), String> {
        if mode > protocol::MAX_MODE {
            return Err(format!("La animación {mode} no existe (hay de la 0 a la {})", protocol::MAX_MODE));
        }
        self.send(protocol::mode(mode))?;
        self.update(|s| {
            s.mode = Some(mode);
            s.power = Some(true);
        });
        Ok(())
    }

    pub fn power(&self, on: bool) -> Result<(), String> {
        self.send(protocol::power(on))?;
        self.update(|s| s.power = Some(on));
        Ok(())
    }
}
