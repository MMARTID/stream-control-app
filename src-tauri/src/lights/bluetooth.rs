//! Bluetooth real con `btleplug` (WinRT en Windows, CoreBluetooth en macOS, BlueZ en Linux).

use std::{collections::HashMap, sync::Mutex, time::Duration};

use async_trait::async_trait;
use btleplug::{
    api::{Central, CentralState, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter as BtAdapter, Manager, Peripheral},
};
use std::sync::Arc;
use tokio::time::{sleep, timeout};

use super::{
    Adapter, Backend, LightDevice, Link,
    protocol::{self, Packet},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Default)]
pub struct BtleBackend {
    adapter: tokio::sync::Mutex<Option<BtAdapter>>,
    found: Mutex<HashMap<String, Peripheral>>,
}

impl BtleBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// El primer adaptador Bluetooth del equipo. Se pide una vez y se reutiliza.
    async fn adapter(&self) -> Result<BtAdapter, String> {
        let mut slot = self.adapter.lock().await;
        if let Some(a) = slot.as_ref() {
            return Ok(a.clone());
        }
        let manager = Manager::new().await.map_err(|e| format!("No se pudo usar el Bluetooth del equipo: {e}"))?;
        let adapter = manager
            .adapters()
            .await
            .map_err(|e| format!("No se pudo usar el Bluetooth del equipo: {e}"))?
            .into_iter()
            .next()
            .ok_or("No se encuentra ningún adaptador Bluetooth en este equipo")?;
        *slot = Some(adapter.clone());
        Ok(adapter)
    }
}

#[async_trait]
impl Backend for BtleBackend {
    async fn adapter_state(&self) -> Adapter {
        match self.adapter().await {
            Err(_) => Adapter::Missing,
            Ok(a) => match a.adapter_state().await {
                Ok(CentralState::PoweredOn) => Adapter::Ready,
                Ok(CentralState::PoweredOff) => Adapter::Off,
                _ => Adapter::Unknown,
            },
        }
    }

    async fn scan(&self, seconds: u64) -> Result<Vec<LightDevice>, String> {
        let adapter = self.adapter().await?;
        if matches!(adapter.adapter_state().await, Ok(CentralState::PoweredOff)) {
            return Err("El Bluetooth está apagado. Enciéndelo en Windows (o en macOS) y vuelve a buscar".into());
        }
        adapter.start_scan(ScanFilter::default()).await.map_err(|e| format!("No se pudo buscar dispositivos: {e}"))?;
        sleep(Duration::from_secs(seconds)).await;
        let peripherals = adapter.peripherals().await.map_err(|e| format!("No se pudo leer la lista de dispositivos: {e}"));
        let _ = adapter.stop_scan().await;

        let mut devices = Vec::new();
        let mut found = HashMap::new();
        for p in peripherals? {
            let Ok(Some(props)) = p.properties().await else { continue };
            let name = props.local_name.or(props.advertisement_name).unwrap_or_default();
            let name = name.trim().to_owned();
            // Los dispositivos sin nombre (la mayoría de los anuncios) no se pueden reconocer.
            if name.is_empty() {
                continue;
            }
            let id = p.id().to_string();
            let likely = protocol::is_lotus_name(&name) || props.services.contains(&protocol::SERVICE_UUID);
            devices.push(LightDevice { id: id.clone(), name, rssi: props.rssi, likely });
            found.insert(id, p);
        }
        *self.found.lock().unwrap() = found;
        Ok(devices)
    }

    async fn connect(&self, id: &str) -> Result<Arc<dyn Link>, String> {
        let peripheral = self
            .found
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("Ese dispositivo ya no está a la vista. Pulsa «Buscar tiras» otra vez")?;
        timeout(CONNECT_TIMEOUT, peripheral.connect())
            .await
            .map_err(|_| "La tira no responde. Acércala al PC y cierra la app Lotus Lamp X del móvil (solo admite una conexión)")?
            .map_err(|e| format!("No se pudo conectar: {e}. Cierra la app Lotus Lamp X del móvil: la tira solo admite una conexión a la vez"))?;
        peripheral.discover_services().await.map_err(|e| format!("No se pudieron leer los servicios de la tira: {e}"))?;
        let Some(characteristic) = peripheral.characteristics().into_iter().find(|c| c.uuid == protocol::WRITE_CHAR_UUID) else {
            let _ = peripheral.disconnect().await;
            return Err("Este dispositivo no parece una tira Lotus Lamp (no tiene el canal de control FFF3)".into());
        };
        Ok(Arc::new(BtleLink { peripheral, characteristic }))
    }
}

struct BtleLink {
    peripheral: Peripheral,
    characteristic: btleplug::api::Characteristic,
}

#[async_trait]
impl Link for BtleLink {
    async fn write(&self, packet: &Packet) -> Result<(), String> {
        self.peripheral
            .write(&self.characteristic, packet, WriteType::WithoutResponse)
            .await
            .map_err(|e| e.to_string())
    }

    async fn connected(&self) -> bool {
        self.peripheral.is_connected().await.unwrap_or(false)
    }

    async fn disconnect(&self) {
        let _ = self.peripheral.disconnect().await;
    }
}
