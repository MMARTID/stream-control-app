//! Panel web en la red local (v1, RF-41 a RF-50), servido por la app de escritorio.
//!
//! Diseño y límites: `docs/panel-web.md`.

pub mod audit;
pub mod auth;
pub mod http;
pub mod tls;
pub mod web_state;

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum_server::{Handle, tls_rustls::RustlsConfig};
use serde::Serialize;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{control::Control, settings::secrets::SecretStore};
use audit::{Audit, Entry};
use auth::{Auth, AuthError, PairingView};
use http::Shared;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub last_seen: u64,
    pub last_ip: String,
    /// Tiene el panel abierto ahora mismo.
    pub online: bool,
}

/// Lo que muestra el escritorio sobre el panel. Aquí sí van los códigos de emparejamiento.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub urls: Vec<String>,
    pub error: Option<String>,
    pub has_password: bool,
    /// Código QR (SVG) de la primera dirección, para abrir el panel desde el móvil.
    pub qr_svg: Option<String>,
    pub devices: Vec<DeviceView>,
    pub pairings: Vec<PairingView>,
    pub audit: Vec<Entry>,
}

struct Running {
    configured_port: u16,
    actual_port: u16,
    handle: Handle<SocketAddr>,
    task: JoinHandle<()>,
}

pub struct Panel {
    control: Arc<Control>,
    shared: Arc<Shared>,
    dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    server: Mutex<Option<Running>>,
    start_error: std::sync::Mutex<Option<String>>,
    status_tx: watch::Sender<PanelStatus>,
}

impl Panel {
    /// Arranca el panel según los ajustes y se mantiene al día cuando cambian.
    /// Debe llamarse dentro de un runtime de tokio.
    pub fn start(control: Arc<Control>, dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Arc<Self> {
        let auth = Arc::new(Auth::new(dir.clone(), Arc::clone(&secrets)));
        let audit = Arc::new(Audit::new(dir.clone()));
        let shared = Shared::new(Arc::clone(&control), auth, audit);
        let panel = Arc::new(Self {
            control,
            shared,
            dir,
            secrets,
            server: Mutex::new(None),
            start_error: std::sync::Mutex::new(None),
            status_tx: watch::channel(PanelStatus::default()).0,
        });
        tokio::spawn(Arc::clone(&panel).watch_settings());
        tokio::spawn(Arc::clone(&panel).watch_changes());
        panel
    }

    pub fn status(&self) -> PanelStatus {
        self.status_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<PanelStatus> {
        self.status_tx.subscribe()
    }

    pub fn set_password(&self, password: &str) -> Result<(), String> {
        self.shared.auth.set_password(password).map_err(|e: AuthError| e.to_string())
    }

    pub fn clear_password(&self) -> Result<(), String> {
        self.shared.auth.clear_password().map_err(|e| e.to_string())
    }

    /// RF-43: el dispositivo deja de poder enviar comandos al instante y sus conexiones se cortan.
    pub fn revoke_device(&self, device_id: &str) -> Result<(), String> {
        let name = self.shared.auth.devices().into_iter().find(|d| d.id == device_id).map(|d| d.name);
        if !self.shared.auth.revoke(device_id) {
            return Err("Ese dispositivo ya no está emparejado".into());
        }
        self.shared.audit.record("Escritorio", "—", "Dispositivo revocado", &name.unwrap_or_default(), true);
        Ok(())
    }

    // ---- Ciclo de vida del servidor ----

    async fn watch_settings(self: Arc<Self>) {
        let mut rx = self.control.subscribe();
        let mut applied: Option<(bool, u16)> = None;
        loop {
            let panel = rx.borrow_and_update().settings.panel.clone();
            let wanted = (panel.enabled, panel.port);
            if applied != Some(wanted) {
                applied = Some(wanted);
                self.reconcile().await;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    async fn reconcile(&self) {
        let cfg = self.control.snapshot().settings.panel;
        let mut server = self.server.lock().await;
        if let Some(running) = server.take() {
            running.handle.graceful_shutdown(Some(Duration::from_millis(500)));
            let _ = running.task.await;
        }
        *self.start_error.lock().unwrap() = None;
        if cfg.enabled {
            match self.launch(cfg.port).await {
                Ok(running) => *server = Some(running),
                Err(e) => *self.start_error.lock().unwrap() = Some(e),
            }
        }
        drop(server);
        self.rebuild_status().await;
    }

    async fn launch(&self, port: u16) -> Result<Running, String> {
        let ips = tls::local_ips();
        let identity = tls::ensure_identity(&self.dir, self.secrets.as_ref(), &ips, tls::hostname().as_deref())?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = RustlsConfig::from_pem(identity.cert_pem.into_bytes(), identity.key_pem.into_bytes())
            .await
            .map_err(|e| format!("Certificado del panel no válido: {e}"))?;

        let listener = std::net::TcpListener::bind(("0.0.0.0", port)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("El puerto {port} ya está en uso. Elige otro en Ajustes")
            } else {
                format!("No se pudo abrir el puerto {port}: {e}")
            }
        })?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

        let handle = Handle::new();
        let app = http::router(Arc::clone(&self.shared)).into_make_service_with_connect_info::<SocketAddr>();
        let server = axum_server::from_tcp_rustls(listener, config).map_err(|e| e.to_string())?;
        let task = tokio::spawn({
            let handle = handle.clone();
            async move {
                let _ = server.handle(handle).serve(app).await;
            }
        });
        Ok(Running { configured_port: port, actual_port, handle, task })
    }

    // ---- Estado para el escritorio ----

    async fn watch_changes(self: Arc<Self>) {
        let mut auth = self.shared.auth.changes();
        let mut audit = self.shared.audit.changes();
        // Los emparejamientos caducan solos: se refresca de vez en cuando.
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                r = auth.changed() => if r.is_err() { return },
                r = audit.changed() => if r.is_err() { return },
                _ = tick.tick() => {}
            }
            self.rebuild_status().await;
        }
    }

    async fn rebuild_status(&self) {
        let cfg = self.control.snapshot().settings.panel;
        let running = self.server.lock().await.as_ref().map(|r| (r.configured_port, r.actual_port));
        let port = running.map_or(cfg.port, |(_, actual)| actual);
        let urls: Vec<String> = if running.is_some() {
            tls::local_ips().iter().map(|ip| format!("https://{ip}:{port}")).collect()
        } else {
            Vec::new()
        };
        let qr_svg = urls.first().and_then(|url| {
            qrcode::QrCode::new(url.as_bytes())
                .ok()
                .map(|code| code.render::<qrcode::render::svg::Color>().min_dimensions(180, 180).quiet_zone(true).build())
        });
        let online = self.shared.online_devices();
        let devices = self
            .shared
            .auth
            .devices()
            .into_iter()
            .map(|d| DeviceView {
                online: online.contains(&d.id),
                id: d.id,
                name: d.name,
                created_at: d.created_at,
                last_seen: d.last_seen,
                last_ip: d.last_ip,
            })
            .collect();
        let status = PanelStatus {
            enabled: cfg.enabled,
            running: running.is_some(),
            port,
            urls,
            error: self.start_error.lock().unwrap().clone(),
            has_password: self.shared.auth.has_password(),
            qr_svg,
            devices,
            pairings: self.shared.auth.pending(),
            audit: self.shared.audit.recent(100),
        };
        self.status_tx.send_if_modified(|current| {
            let before = serde_json::to_string(current).ok();
            *current = status;
            before != serde_json::to_string(current).ok()
        });
    }
}
