//! Cliente obs-websocket v5 con reconexión automática (RF-1 a RF-4).
//!
//! Una tarea en segundo plano mantiene la conexión: espera a que OBS aparezca, se
//! identifica, reenvía los eventos y atiende las peticiones. Si la conexión se pierde,
//! reintenta con espera creciente sin bloquear a quien llama.

use std::{collections::HashMap, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot, watch},
    time::{Instant, interval, sleep, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use super::{
    auth::auth_string,
    protocol::{
        CLOSE_AUTHENTICATION_FAILED, Frame, Hello, ObsEvent, RequestResponse, identify_message, op,
        request_message,
    },
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
const PING_EVERY: Duration = Duration::from_secs(10);
const DEAD_AFTER: Duration = Duration::from_secs(30);

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
}

impl ObsConfig {
    fn url(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("ws://[{}]:{}", self.host, self.port)
        } else {
            format!("ws://{}:{}", self.host, self.port)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Sin configuración o con un error que exige cambiar los ajustes (p. ej. contraseña incorrecta).
    #[default]
    Disconnected,
    /// Primer intento con la configuración actual.
    Connecting,
    Connected,
    /// OBS no está disponible o se perdió la conexión; se reintenta en segundo plano.
    Reconnecting,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ConnectionInfo {
    pub state: ConnectionState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Error)]
pub enum ObsError {
    #[error("OBS no está conectado")]
    NotConnected,
    #[error("Se perdió la conexión con OBS")]
    Closed,
    #[error("OBS no respondió a tiempo")]
    Timeout,
    #[error("OBS rechazó la petición ({code}){}", .comment.as_deref().map(|c| format!(": {c}")).unwrap_or_default())]
    Request { code: i64, comment: Option<String> },
    #[error("Respuesta inesperada de OBS: {0}")]
    Protocol(String),
}

type Reply = oneshot::Sender<Result<Option<Value>, ObsError>>;

struct Cmd {
    kind: String,
    data: Option<Value>,
    reply: Reply,
}

#[derive(Clone)]
pub struct ObsClient {
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    config_tx: Arc<watch::Sender<Option<ObsConfig>>>,
    status_rx: watch::Receiver<ConnectionInfo>,
    events_tx: broadcast::Sender<ObsEvent>,
}

impl ObsClient {
    /// Arranca la tarea de conexión. Debe llamarse dentro de un runtime de tokio.
    pub fn spawn(config: Option<ObsConfig>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let initial = ConnectionInfo {
            state: if config.is_some() { ConnectionState::Connecting } else { ConnectionState::Disconnected },
            error: None,
        };
        let (status_tx, status_rx) = watch::channel(initial);
        let (config_tx, config_rx) = watch::channel(config);
        let (events_tx, _) = broadcast::channel(512);
        tokio::spawn(run(config_rx, cmd_rx, status_tx, events_tx.clone()));
        Self { cmd_tx, config_tx: Arc::new(config_tx), status_rx, events_tx }
    }

    /// Cambia la configuración. Si es distinta, reconecta de inmediato.
    pub fn set_config(&self, config: Option<ObsConfig>) {
        self.config_tx.send_if_modified(|current| {
            if *current == config {
                false
            } else {
                *current = config;
                true
            }
        });
    }

    pub fn status(&self) -> watch::Receiver<ConnectionInfo> {
        self.status_rx.clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ObsEvent> {
        self.events_tx.subscribe()
    }

    /// Envía una petición y espera su respuesta (10 s como máximo).
    pub async fn request(&self, kind: &str, data: Option<Value>) -> Result<Option<Value>, ObsError> {
        if self.status_rx.borrow().state != ConnectionState::Connected {
            return Err(ObsError::NotConnected);
        }
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd { kind: kind.to_owned(), data, reply })
            .map_err(|_| ObsError::Closed)?;
        match timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ObsError::Closed),
            Err(_) => Err(ObsError::Timeout),
        }
    }
}

fn set_status(tx: &watch::Sender<ConnectionInfo>, state: ConnectionState, error: Option<String>) {
    tx.send_if_modified(|current| {
        let next = ConnectionInfo { state, error };
        if *current == next {
            false
        } else {
            *current = next;
            true
        }
    });
}

enum SessionEnd {
    Shutdown,
    ConfigChanged,
    AuthFailed(String),
    Lost { was_connected: bool, error: String },
}

async fn run(
    mut config_rx: watch::Receiver<Option<ObsConfig>>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    status_tx: watch::Sender<ConnectionInfo>,
    events_tx: broadcast::Sender<ObsEvent>,
) {
    let mut backoff = BACKOFF_MIN;
    let mut first_attempt = true;
    loop {
        let Some(config) = config_rx.borrow_and_update().clone() else {
            set_status(&status_tx, ConnectionState::Disconnected, None);
            if !idle(&mut config_rx, &mut cmd_rx, None).await {
                return;
            }
            first_attempt = true;
            continue;
        };

        let state = if first_attempt { ConnectionState::Connecting } else { ConnectionState::Reconnecting };
        let previous_error = status_tx.borrow().error.clone();
        set_status(&status_tx, state, previous_error);
        first_attempt = false;

        match session(&config, &mut config_rx, &mut cmd_rx, &status_tx, &events_tx).await {
            SessionEnd::Shutdown => return,
            SessionEnd::ConfigChanged => {
                backoff = BACKOFF_MIN;
                first_attempt = true;
            }
            SessionEnd::AuthFailed(msg) => {
                // Reintentar con la misma contraseña no sirve: se espera a que cambien los ajustes.
                set_status(&status_tx, ConnectionState::Disconnected, Some(msg));
                if !idle(&mut config_rx, &mut cmd_rx, None).await {
                    return;
                }
                backoff = BACKOFF_MIN;
                first_attempt = true;
            }
            SessionEnd::Lost { was_connected, error } => {
                if was_connected {
                    backoff = BACKOFF_MIN;
                }
                set_status(&status_tx, ConnectionState::Reconnecting, Some(error));
                if !idle(&mut config_rx, &mut cmd_rx, Some(backoff)).await {
                    return;
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

/// Espera `dur` (o indefinidamente) respondiendo «no conectado» a las peticiones que lleguen.
/// Devuelve `false` si el cliente se ha cerrado y la tarea debe terminar.
async fn idle(
    config_rx: &mut watch::Receiver<Option<ObsConfig>>,
    cmd_rx: &mut mpsc::UnboundedReceiver<Cmd>,
    dur: Option<Duration>,
) -> bool {
    let sleeper = async {
        match dur {
            Some(d) => sleep(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(sleeper);
    loop {
        tokio::select! {
            _ = &mut sleeper => return true,
            changed = config_rx.changed() => return changed.is_ok(),
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => { let _ = cmd.reply.send(Err(ObsError::NotConnected)); }
                None => return false,
            },
        }
    }
}

enum Fail {
    Auth(String),
    Lost(String),
}

async fn establish(config: &ObsConfig) -> Result<Ws, Fail> {
    let url = config.url();
    let (mut ws, _) = match timeout(CONNECT_TIMEOUT, connect_async(url.as_str())).await {
        Ok(Ok(ok)) => ok,
        Ok(Err(e)) => {
            return Err(Fail::Lost(format!(
                "No se pudo conectar con OBS en {}:{} ({e})",
                config.host, config.port
            )));
        }
        Err(_) => {
            return Err(Fail::Lost(format!(
                "OBS no respondió en {}:{}",
                config.host, config.port
            )));
        }
    };
    timeout(HANDSHAKE_TIMEOUT, identify(&mut ws, config))
        .await
        .map_err(|_| Fail::Lost("OBS no completó la identificación a tiempo".into()))??;
    Ok(ws)
}

async fn next_text(ws: &mut Ws) -> Result<Frame, Fail> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(text.as_str())
                    .map_err(|e| Fail::Lost(format!("Mensaje inválido de OBS: {e}")));
            }
            Some(Ok(Message::Close(frame))) => {
                let code = frame.as_ref().map(|f| u16::from(f.code));
                return Err(if code == Some(CLOSE_AUTHENTICATION_FAILED) {
                    Fail::Auth("La contraseña de OBS es incorrecta".into())
                } else {
                    Fail::Lost("OBS cerró la conexión".into())
                });
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Fail::Lost(format!("Error de conexión con OBS: {e}"))),
            None => return Err(Fail::Lost("OBS cerró la conexión".into())),
        }
    }
}

async fn identify(ws: &mut Ws, config: &ObsConfig) -> Result<(), Fail> {
    let hello = next_text(ws).await?;
    if hello.op != op::HELLO {
        return Err(Fail::Lost("OBS no envió el saludo esperado".into()));
    }
    let hello: Hello = serde_json::from_value(hello.d)
        .map_err(|e| Fail::Lost(format!("Saludo de OBS inválido: {e}")))?;
    let authentication = match (&hello.authentication, config.password.as_deref()) {
        (Some(challenge), Some(password)) => {
            Some(auth_string(password, &challenge.salt, &challenge.challenge))
        }
        (Some(_), None) => return Err(Fail::Auth("OBS pide contraseña y no hay ninguna guardada".into())),
        (None, _) => None,
    };
    ws.send(Message::text(identify_message(authentication)))
        .await
        .map_err(|e| Fail::Lost(format!("No se pudo identificar con OBS: {e}")))?;
    let identified = next_text(ws).await?;
    if identified.op != op::IDENTIFIED {
        return Err(Fail::Lost("OBS no confirmó la identificación".into()));
    }
    Ok(())
}

async fn session(
    config: &ObsConfig,
    config_rx: &mut watch::Receiver<Option<ObsConfig>>,
    cmd_rx: &mut mpsc::UnboundedReceiver<Cmd>,
    status_tx: &watch::Sender<ConnectionInfo>,
    events_tx: &broadcast::Sender<ObsEvent>,
) -> SessionEnd {
    let mut ws = tokio::select! {
        established = establish(config) => match established {
            Ok(ws) => ws,
            Err(Fail::Auth(msg)) => return SessionEnd::AuthFailed(msg),
            Err(Fail::Lost(error)) => return SessionEnd::Lost { was_connected: false, error },
        },
        changed = config_rx.changed() => {
            return if changed.is_ok() { SessionEnd::ConfigChanged } else { SessionEnd::Shutdown };
        }
    };

    set_status(status_tx, ConnectionState::Connected, None);

    let mut pending: HashMap<String, Reply> = HashMap::new();
    let mut ping = interval(PING_EVERY);
    ping.tick().await;
    let mut last_rx = Instant::now();

    let end = loop {
        tokio::select! {
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    last_rx = Instant::now();
                    handle_text(text.as_str(), &mut pending, events_tx);
                }
                Some(Ok(Message::Close(_))) | None => {
                    break SessionEnd::Lost { was_connected: true, error: "OBS cerró la conexión".into() };
                }
                Some(Ok(_)) => last_rx = Instant::now(),
                Some(Err(e)) => {
                    break SessionEnd::Lost { was_connected: true, error: format!("Error de conexión con OBS: {e}") };
                }
            },
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => {
                    pending.retain(|_, reply| !reply.is_closed());
                    let id = uuid::Uuid::new_v4().to_string();
                    let frame = request_message(&id, &cmd.kind, cmd.data.as_ref());
                    match ws.send(Message::text(frame)).await {
                        Ok(()) => { pending.insert(id, cmd.reply); }
                        Err(e) => {
                            let _ = cmd.reply.send(Err(ObsError::Closed));
                            break SessionEnd::Lost { was_connected: true, error: format!("Error de conexión con OBS: {e}") };
                        }
                    }
                }
                None => break SessionEnd::Shutdown,
            },
            changed = config_rx.changed() => {
                break if changed.is_ok() { SessionEnd::ConfigChanged } else { SessionEnd::Shutdown };
            }
            _ = ping.tick() => {
                if last_rx.elapsed() > DEAD_AFTER {
                    break SessionEnd::Lost { was_connected: true, error: "OBS dejó de responder".into() };
                }
                if ws.send(Message::Ping(Default::default())).await.is_err() {
                    break SessionEnd::Lost { was_connected: true, error: "OBS cerró la conexión".into() };
                }
            }
        }
    };

    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(ObsError::Closed));
    }
    end
}

fn handle_text(text: &str, pending: &mut HashMap<String, Reply>, events_tx: &broadcast::Sender<ObsEvent>) {
    let Ok(frame) = serde_json::from_str::<Frame>(text) else { return };
    match frame.op {
        op::EVENT => {
            if let Ok(event) = serde_json::from_value::<ObsEvent>(frame.d) {
                let _ = events_tx.send(event);
            }
        }
        op::REQUEST_RESPONSE => {
            let Ok(resp) = serde_json::from_value::<RequestResponse>(frame.d) else { return };
            if let Some(reply) = pending.remove(&resp.request_id) {
                let result = if resp.request_status.result {
                    Ok(resp.response_data)
                } else {
                    Err(ObsError::Request {
                        code: resp.request_status.code,
                        comment: resp.request_status.comment,
                    })
                };
                let _ = reply.send(result);
            }
        }
        _ => {}
    }
}
