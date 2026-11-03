//! Controlador de tiras LED Bluetooth, con un Bluetooth y una tira simulados (no hace falta hardware).

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use stream_control_lib::lights::{
    Adapter, Backend, LightDevice, Lights, Link,
    protocol::{self, Packet},
};

struct FakeLink {
    writes: Mutex<Vec<(Instant, Packet)>>,
    broken: AtomicBool,
    alive: AtomicBool,
}

#[async_trait]
impl Link for FakeLink {
    async fn write(&self, packet: &Packet) -> Result<(), String> {
        if self.broken.load(Ordering::SeqCst) {
            return Err("GATT desconectado".into());
        }
        self.writes.lock().unwrap().push((Instant::now(), *packet));
        Ok(())
    }
    async fn connected(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    async fn disconnect(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

struct FakeBackend {
    devices: Mutex<Vec<LightDevice>>,
    adapter: Adapter,
    scan_error: Option<String>,
    connect_error: Option<String>,
    link: Arc<FakeLink>,
}

impl FakeBackend {
    fn new(devices: Vec<LightDevice>) -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::new(devices),
            adapter: Adapter::Ready,
            scan_error: None,
            connect_error: None,
            link: Arc::new(FakeLink { writes: Mutex::new(vec![]), broken: AtomicBool::new(false), alive: AtomicBool::new(true) }),
        })
    }
}

#[async_trait]
impl Backend for FakeBackend {
    async fn adapter_state(&self) -> Adapter {
        self.adapter
    }
    async fn scan(&self, _seconds: u64) -> Result<Vec<LightDevice>, String> {
        match &self.scan_error {
            Some(e) => Err(e.clone()),
            None => Ok(self.devices.lock().unwrap().clone()),
        }
    }
    async fn connect(&self, id: &str) -> Result<Arc<dyn Link>, String> {
        if let Some(e) = &self.connect_error {
            return Err(e.clone());
        }
        if !self.devices.lock().unwrap().iter().any(|d| d.id == id) {
            return Err("Ese dispositivo ya no está a la vista".into());
        }
        self.link.alive.store(true, Ordering::SeqCst);
        Ok(self.link.clone() as Arc<dyn Link>)
    }
}

fn dev(id: &str, name: &str, rssi: Option<i16>, likely: bool) -> LightDevice {
    LightDevice { id: id.into(), name: name.into(), rssi, likely }
}

fn strip() -> LightDevice {
    dev("AA:BB", "MELK-OA10   5F", Some(-60), true)
}

async fn connected() -> (Arc<FakeBackend>, Arc<Lights>) {
    let backend = FakeBackend::new(vec![strip()]);
    let lights = Lights::new(backend.clone());
    lights.scan().await.unwrap();
    lights.connect("AA:BB").await.unwrap();
    (backend, lights)
}

fn written(b: &FakeBackend) -> Vec<Packet> {
    b.link.writes.lock().unwrap().iter().map(|(_, p)| *p).collect()
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(450)).await;
}

#[tokio::test]
async fn la_busqueda_pone_primero_las_tiras_probables_y_dentro_de_cada_grupo_las_mas_cercanas() {
    let backend = FakeBackend::new(vec![
        dev("1", "Auriculares", Some(-30), false),
        dev("2", "MELK-OA10   5F", Some(-80), true),
        dev("3", "MELK-OA10   77", Some(-50), true),
        dev("4", "Reloj", None, false),
    ]);
    let lights = Lights::new(backend);
    let found = lights.scan().await.unwrap();
    let ids: Vec<_> = found.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["3", "2", "1", "4"]);
    let status = lights.status();
    assert_eq!(status.devices.len(), 4);
    assert!(!status.scanning);
    assert_eq!(status.adapter, Adapter::Ready);
}

#[tokio::test]
async fn un_bluetooth_apagado_o_ausente_se_explica_en_vez_de_fallar_en_silencio() {
    let mut backend = Arc::try_unwrap(FakeBackend::new(vec![])).ok().unwrap();
    backend.adapter = Adapter::Off;
    backend.scan_error = Some("El Bluetooth está apagado. Enciéndelo".into());
    let lights = Lights::new(Arc::new(backend));
    assert!(lights.scan().await.unwrap_err().contains("apagado"));
    let status = lights.status();
    assert_eq!(status.adapter, Adapter::Off);
    assert!(status.error.unwrap().contains("apagado"));
    assert!(!status.scanning);
}

#[tokio::test]
async fn sin_tira_conectada_las_ordenes_fallan_con_un_mensaje_claro() {
    let lights = Lights::new(FakeBackend::new(vec![strip()]));
    for r in [lights.set_color(1, 2, 3), lights.set_brightness(50), lights.set_speed(5), lights.set_mode(3), lights.power(true)] {
        assert!(r.unwrap_err().contains("No hay ninguna tira conectada"));
    }
}

#[tokio::test]
async fn conecta_y_manda_cada_orden_con_los_bytes_del_protocolo() {
    let (backend, lights) = connected().await;
    assert_eq!(lights.status().connected.unwrap().name, "MELK-OA10   5F");

    lights.power(true).unwrap();
    lights.set_brightness(75).unwrap();
    lights.set_speed(40).unwrap();
    lights.set_mode(143).unwrap();
    lights.set_color(255, 100, 0).unwrap();
    settle().await;

    assert_eq!(
        written(&backend),
        [protocol::power(true), protocol::brightness(75), protocol::speed(40), protocol::mode(143), protocol::rgb(255, 100, 0)],
        "en el mismo orden en que se pidieron"
    );
    let s = lights.status();
    assert_eq!((s.power, s.brightness, s.speed), (Some(true), Some(75), Some(40)));
    assert_eq!((s.color, s.mode), (Some([255, 100, 0]), None), "un color fijo cancela la animación");
    lights.set_mode(5).unwrap();
    assert_eq!(lights.status().mode, Some(5));
    assert!(lights.set_mode(213).unwrap_err().contains("no existe"));
}

#[tokio::test]
async fn muchas_ordenes_de_color_seguidas_se_juntan_y_se_espacian_100_ms() {
    let (backend, lights) = connected().await;
    // Como haría una sincronización con la imagen: 50 colores en una ráfaga.
    for i in 0..50u8 {
        lights.set_color(i, 255 - i, 0).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(600)).await;

    let writes = backend.link.writes.lock().unwrap().clone();
    assert!(!writes.is_empty() && writes.len() <= 3, "la ráfaga se junta: {} escrituras", writes.len());
    assert_eq!(writes.last().unwrap().1, protocol::rgb(49, 206, 0), "gana el último color");
    for pair in writes.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(gap >= Duration::from_millis(90), "la tira no admite órdenes con menos de ~100 ms: {gap:?}");
    }
}

#[tokio::test]
async fn un_deslizador_de_brillo_no_inunda_la_tira() {
    let (backend, lights) = connected().await;
    for level in 0..=100u8 {
        lights.set_brightness(level).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let writes = written(&backend);
    assert!(writes.len() <= 2, "{} escrituras", writes.len());
    assert_eq!(*writes.last().unwrap(), protocol::brightness(100));
}

#[tokio::test]
async fn el_orden_se_respeta_entre_colores_y_otras_ordenes() {
    let (backend, lights) = connected().await;
    lights.set_color(10, 0, 0).unwrap();
    lights.set_color(20, 0, 0).unwrap();
    lights.power(false).unwrap();
    lights.set_color(30, 0, 0).unwrap();
    settle().await;
    settle().await;
    // Los dos primeros colores se juntan (queda el segundo); el apagado no se adelanta ni se pierde.
    assert_eq!(written(&backend), [protocol::rgb(20, 0, 0), protocol::power(false), protocol::rgb(30, 0, 0)]);
}

#[tokio::test]
async fn si_se_pierde_la_conexion_lo_dice_y_las_ordenes_siguientes_fallan() {
    let (backend, lights) = connected().await;
    lights.set_color(1, 1, 1).unwrap();
    settle().await;
    backend.link.broken.store(true, Ordering::SeqCst);
    lights.set_color(2, 2, 2).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let s = lights.status();
    assert!(s.connected.is_none());
    assert!(s.error.unwrap().contains("Se perdió la conexión"));
    assert!(lights.set_color(3, 3, 3).unwrap_err().contains("No hay ninguna tira conectada"));
}

#[tokio::test]
async fn un_fallo_al_conectar_da_el_motivo_y_no_deja_la_app_a_medias() {
    let mut backend = Arc::try_unwrap(FakeBackend::new(vec![strip()])).ok().unwrap();
    backend.connect_error = Some("No se pudo conectar. La tira solo admite una conexión a la vez".into());
    let lights = Lights::new(Arc::new(backend));
    lights.scan().await.unwrap();
    let err = lights.connect("AA:BB").await.unwrap_err();
    assert!(err.contains("una conexión"));
    let s = lights.status();
    assert!(!s.connecting && s.connected.is_none());
    assert_eq!(s.error.as_deref(), Some(err.as_str()));
}

#[tokio::test]
async fn al_desconectar_suelta_la_tira_y_se_puede_volver_a_conectar() {
    let (backend, lights) = connected().await;
    lights.disconnect().await;
    assert!(lights.status().connected.is_none());
    assert!(!backend.link.alive.load(Ordering::SeqCst));
    assert!(lights.set_color(1, 2, 3).is_err());
    lights.connect("AA:BB").await.unwrap();
    assert!(lights.set_color(1, 2, 3).is_ok());
}

#[tokio::test]
async fn al_abrir_la_app_se_reconecta_solo_si_la_tira_guardada_esta_a_la_vista() {
    let backend = FakeBackend::new(vec![strip()]);
    let lights = Lights::new(backend.clone());
    assert!(!lights.connect_saved("OTRA").await, "no está a la vista: no se conecta a otra");
    assert!(lights.status().connected.is_none());
    assert!(lights.connect_saved("AA:BB").await);
    assert_eq!(lights.status().connected.unwrap().id, "AA:BB");
}
