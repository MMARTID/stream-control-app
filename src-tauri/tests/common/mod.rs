//! Servidor obs-websocket v5 simulado para las pruebas.
//!
//! Habla el protocolo real (Hello → Identify → Identified, peticiones y eventos) con un
//! OBS en memoria. Sirve para probar el cliente y el núcleo sin tener OBS instalado, pero
//! NO sustituye a una prueba contra OBS real: los nombres de campos están copiados de la
//! documentación, no observados en un OBS en marcha.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use stream_control_lib::obs::auth::auth_string;
use tokio::{net::TcpListener, sync::broadcast, task::JoinHandle};
use tokio_tungstenite::tungstenite::{
    Message,
    protocol::{CloseFrame, frame::coding::CloseCode},
};

pub struct Item {
    pub id: i64,
    pub source: String,
    pub enabled: bool,
}

pub struct MockState {
    pub password: Option<String>,
    pub scenes: Vec<String>,
    pub current: String,
    pub muted: HashMap<String, bool>,
    pub streaming: bool,
    pub items: HashMap<String, Vec<Item>>,
    /// Peticiones recibidas, en orden: (tipo, datos).
    pub log: Vec<(String, Value)>,
}

impl MockState {
    fn new(password: Option<String>) -> Self {
        let cameras = || {
            vec![
                Item { id: 1, source: "Cámara móvil".into(), enabled: true },
                Item { id: 2, source: "Webcam".into(), enabled: false },
            ]
        };
        let mut items = HashMap::new();
        items.insert("Juego".to_owned(), cameras());
        items.insert("Charla".to_owned(), cameras());
        items.insert("Pausa / baño".to_owned(), vec![]);
        Self {
            password,
            scenes: vec!["Juego".into(), "Charla".into(), "Pausa / baño".into()],
            current: "Juego".into(),
            muted: HashMap::from([("Micro".to_owned(), false), ("Escritorio".to_owned(), false)]),
            streaming: false,
            items,
            log: vec![],
        }
    }
}

pub struct MockObs {
    pub port: u16,
    pub state: Arc<Mutex<MockState>>,
    accept: JoinHandle<()>,
}

impl MockObs {
    /// Arranca el servidor. Con `port = None` elige uno libre.
    pub async fn start(port: Option<u16>, password: Option<&str>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(MockState::new(password.map(str::to_owned))));
        let (events, _) = broadcast::channel::<String>(64);
        let accept = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                // Al abortar esta tarea se sueltan el listener y todas las conexiones (como al cerrar OBS).
                let mut connections = tokio::task::JoinSet::new();
                while let Ok((stream, _)) = listener.accept().await {
                    connections.spawn(connection(stream, Arc::clone(&state), events.clone()));
                }
            })
        };
        Self { port, state, accept }
    }

    /// Simula que OBS se cierra: corta las conexiones y libera el puerto.
    pub async fn stop(self) {
        self.accept.abort();
        let _ = self.accept.await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    pub fn log(&self) -> Vec<(String, Value)> {
        self.state.lock().unwrap().log.clone()
    }

    pub fn is_muted(&self, input: &str) -> bool {
        self.state.lock().unwrap().muted[input]
    }

    pub fn current_scene(&self) -> String {
        self.state.lock().unwrap().current.clone()
    }
}

async fn connection(stream: tokio::net::TcpStream, state: Arc<Mutex<MockState>>, events: broadcast::Sender<String>) {
    let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else { return };
    let mut events_rx = events.subscribe();

    let password = state.lock().unwrap().password.clone();
    let (challenge, salt) = ("Y2hhbGxlbmdlLXBhcmEtbG9zLXRlc3Rz", "c2FsLXBhcmEtbG9zLXRlc3Rz");
    let mut hello = json!({ "obsWebSocketVersion": "5.5.2", "rpcVersion": 1 });
    if password.is_some() {
        hello["authentication"] = json!({ "challenge": challenge, "salt": salt });
    }
    if ws.send(Message::text(json!({ "op": 0, "d": hello }).to_string())).await.is_err() {
        return;
    }

    // Identify
    let Some(Ok(Message::Text(text))) = ws.next().await else { return };
    let identify: Value = serde_json::from_str(text.as_str()).unwrap();
    assert_eq!(identify["op"], 1);
    if let Some(password) = password {
        let expected = auth_string(&password, salt, challenge);
        if identify["d"]["authentication"].as_str() != Some(expected.as_str()) {
            let _ = ws
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(4009),
                    reason: "Authentication failed.".into(),
                })))
                .await;
            return;
        }
    }
    let _ = ws.send(Message::text(json!({ "op": 2, "d": { "negotiatedRpcVersion": 1 } }).to_string())).await;

    loop {
        tokio::select! {
            incoming = ws.next() => {
                let Some(Ok(Message::Text(text))) = incoming else { return };
                let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                if frame["op"] != 6 { continue; }
                let d = &frame["d"];
                let kind = d["requestType"].as_str().unwrap().to_owned();
                let data = d.get("requestData").cloned().unwrap_or(Value::Null);
                let (result, emitted) = handle(&mut state.lock().unwrap(), &kind, &data);
                let status = match &result {
                    Ok(_) => json!({ "result": true, "code": 100 }),
                    Err((code, comment)) => json!({ "result": false, "code": code, "comment": comment }),
                };
                let mut response = json!({
                    "requestType": kind,
                    "requestId": d["requestId"],
                    "requestStatus": status,
                });
                if let Ok(Some(body)) = result {
                    response["responseData"] = body;
                }
                // La respuesta va antes que los eventos, como en OBS.
                if ws.send(Message::text(json!({ "op": 7, "d": response }).to_string())).await.is_err() { return; }
                for event in emitted {
                    let _ = events.send(event);
                }
            }
            Ok(event) = events_rx.recv() => {
                if ws.send(Message::text(event)).await.is_err() { return; }
            }
        }
    }
}

fn event(kind: &str, data: Value) -> String {
    json!({ "op": 5, "d": { "eventType": kind, "eventIntent": 1, "eventData": data } }).to_string()
}

type Handled = (Result<Option<Value>, (i64, String)>, Vec<String>);

fn handle(s: &mut MockState, kind: &str, data: &Value) -> Handled {
    s.log.push((kind.to_owned(), data.clone()));
    let not_found = |what: &str| Err((600, format!("No existe {what}")));
    match kind {
        "GetVersion" => (Ok(Some(json!({ "obsVersion": "31.0.0", "obsWebSocketVersion": "5.5.2", "rpcVersion": 1 }))), vec![]),
        "GetSceneList" => {
            let scenes: Vec<Value> = s
                .scenes
                .iter()
                .enumerate()
                .map(|(i, name)| json!({ "sceneName": name, "sceneUuid": format!("uuid-{i}"), "sceneIndex": i }))
                .collect();
            (
                Ok(Some(json!({
                    "currentProgramSceneName": s.current,
                    "currentPreviewSceneName": null,
                    "scenes": scenes,
                }))),
                vec![],
            )
        }
        "SetCurrentProgramScene" => {
            let name = data["sceneName"].as_str().unwrap_or_default().to_owned();
            if !s.scenes.contains(&name) {
                return (not_found("la escena"), vec![]);
            }
            s.current = name.clone();
            (Ok(None), vec![event("CurrentProgramSceneChanged", json!({ "sceneName": name, "sceneUuid": "x" }))])
        }
        "GetInputList" => {
            let mut inputs: Vec<&String> = s.muted.keys().collect();
            inputs.sort();
            let inputs: Vec<Value> = inputs
                .into_iter()
                .map(|n| json!({ "inputName": n, "inputKind": "wasapi_input_capture", "unversionedInputKind": "wasapi_input_capture" }))
                .collect();
            (Ok(Some(json!({ "inputs": inputs }))), vec![])
        }
        "GetInputMute" => match s.muted.get(data["inputName"].as_str().unwrap_or_default()) {
            Some(muted) => (Ok(Some(json!({ "inputMuted": muted }))), vec![]),
            None => (not_found("la entrada"), vec![]),
        },
        "SetInputMute" => {
            let name = data["inputName"].as_str().unwrap_or_default().to_owned();
            let muted = data["inputMuted"].as_bool().unwrap();
            match s.muted.get_mut(&name) {
                Some(slot) => {
                    *slot = muted;
                    (Ok(None), vec![event("InputMuteStateChanged", json!({ "inputName": name, "inputUuid": "x", "inputMuted": muted }))])
                }
                None => (not_found("la entrada"), vec![]),
            }
        }
        "GetSceneItemList" => match s.items.get(data["sceneName"].as_str().unwrap_or_default()) {
            Some(items) => {
                let items: Vec<Value> = items
                    .iter()
                    .map(|i| json!({ "sceneItemId": i.id, "sourceName": i.source, "sceneItemEnabled": i.enabled, "sceneItemIndex": i.id }))
                    .collect();
                (Ok(Some(json!({ "sceneItems": items }))), vec![])
            }
            None => (not_found("la escena"), vec![]),
        },
        "SetSceneItemEnabled" => {
            let scene = data["sceneName"].as_str().unwrap_or_default().to_owned();
            let id = data["sceneItemId"].as_i64().unwrap();
            let enabled = data["sceneItemEnabled"].as_bool().unwrap();
            match s.items.get_mut(&scene).and_then(|items| items.iter_mut().find(|i| i.id == id)) {
                Some(item) => {
                    item.enabled = enabled;
                    (
                        Ok(None),
                        vec![event(
                            "SceneItemEnableStateChanged",
                            json!({ "sceneName": scene, "sceneUuid": "x", "sceneItemId": id, "sceneItemEnabled": enabled }),
                        )],
                    )
                }
                None => (not_found("el elemento"), vec![]),
            }
        }
        "GetStreamStatus" => (
            Ok(Some(json!({ "outputActive": s.streaming, "outputReconnecting": false, "outputTimecode": "00:00:00.000", "outputDuration": 0 }))),
            vec![],
        ),
        "StartStream" | "StopStream" => {
            let start = kind == "StartStream";
            if s.streaming == start {
                return (Err((if start { 500 } else { 501 }, "Ya estaba en ese estado".into())), vec![]);
            }
            s.streaming = start;
            let state = if start { "OBS_WEBSOCKET_OUTPUT_STARTED" } else { "OBS_WEBSOCKET_OUTPUT_STOPPED" };
            (Ok(None), vec![event("StreamStateChanged", json!({ "outputActive": start, "outputState": state }))])
        }
        other => (Err((204, format!("Petición desconocida: {other}"))), vec![]),
    }
}
