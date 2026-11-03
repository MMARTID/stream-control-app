//! Mensajes del protocolo obs-websocket v5 (docs/generated/protocol.md del repositorio obsproject/obs-websocket).

use serde::Deserialize;
use serde_json::{Value, json};

/// Versión RPC que pide esta app. Se incrementa en obs-websocket con cada cambio incompatible.
pub const RPC_VERSION: u32 = 1;

/// Código de cierre `WebSocketCloseCode::AuthenticationFailed`.
pub const CLOSE_AUTHENTICATION_FAILED: u16 = 4009;

/// `EventSubscription`: General | Scenes | Inputs | Outputs | SceneItems.
/// No se piden eventos de alto volumen (medidores de volumen, transformaciones).
pub const EVENT_SUBSCRIPTIONS: u32 = (1 << 0) | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 7);

pub mod op {
    pub const HELLO: u8 = 0;
    pub const IDENTIFY: u8 = 1;
    pub const IDENTIFIED: u8 = 2;
    pub const EVENT: u8 = 5;
    pub const REQUEST: u8 = 6;
    pub const REQUEST_RESPONSE: u8 = 7;
}

#[derive(Debug, Deserialize)]
pub struct Frame {
    pub op: u8,
    pub d: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub obs_web_socket_version: String,
    pub rpc_version: u32,
    pub authentication: Option<AuthChallenge>,
}

#[derive(Debug, Deserialize)]
pub struct AuthChallenge {
    pub challenge: String,
    pub salt: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestStatus {
    pub result: bool,
    pub code: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestResponse {
    pub request_id: String,
    pub request_status: RequestStatus,
    pub response_data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObsEvent {
    pub event_type: String,
    pub event_data: Option<Value>,
}

pub fn identify_message(authentication: Option<String>) -> String {
    let mut d = json!({
        "rpcVersion": RPC_VERSION,
        "eventSubscriptions": EVENT_SUBSCRIPTIONS,
    });
    if let Some(auth) = authentication {
        d["authentication"] = Value::String(auth);
    }
    json!({ "op": op::IDENTIFY, "d": d }).to_string()
}

pub fn request_message(request_id: &str, request_type: &str, data: Option<&Value>) -> String {
    let mut d = json!({ "requestType": request_type, "requestId": request_id });
    if let Some(data) = data {
        d["requestData"] = data.clone();
    }
    json!({ "op": op::REQUEST, "d": d }).to_string()
}
