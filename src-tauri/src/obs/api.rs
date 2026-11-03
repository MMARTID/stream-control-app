//! Peticiones concretas de obs-websocket v5 que usa la app.
//!
//! Nombres de petición y campos comprobados contra docs/generated/protocol.md de
//! obsproject/obs-websocket: GetVersion, GetSceneList, SetCurrentProgramScene,
//! GetInputList, GetInputMute, SetInputMute, GetSceneItemList, SetSceneItemEnabled,
//! GetStreamStatus, StartStream y StopStream.

use serde_json::{Value, json};

use super::client::{ObsClient, ObsError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneList {
    pub current: Option<String>,
    pub scenes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneItem {
    pub id: i64,
    pub source_name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamStatus {
    pub active: bool,
    pub reconnecting: bool,
}

fn data(response: Option<Value>) -> Result<Value, ObsError> {
    response.ok_or_else(|| ObsError::Protocol("la respuesta no trae datos".into()))
}

fn str_field(v: &Value, key: &str) -> Result<String, ObsError> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ObsError::Protocol(format!("falta el campo `{key}`")))
}

fn bool_field(v: &Value, key: &str) -> Result<bool, ObsError> {
    v.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| ObsError::Protocol(format!("falta el campo `{key}`")))
}

fn array_field<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, ObsError> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ObsError::Protocol(format!("falta la lista `{key}`")))
}

impl ObsClient {
    pub async fn get_version(&self) -> Result<String, ObsError> {
        let v = data(self.request("GetVersion", None).await?)?;
        str_field(&v, "obsWebSocketVersion")
    }

    /// Escenas en el orden en que las devuelve OBS.
    pub async fn get_scene_list(&self) -> Result<SceneList, ObsError> {
        let v = data(self.request("GetSceneList", None).await?)?;
        let scenes = array_field(&v, "scenes")?
            .iter()
            .map(|s| str_field(s, "sceneName"))
            .collect::<Result<Vec<_>, _>>()?;
        let current = v
            .get("currentProgramSceneName")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(SceneList { current, scenes })
    }

    pub async fn set_current_scene(&self, scene: &str) -> Result<(), ObsError> {
        self.request("SetCurrentProgramScene", Some(json!({ "sceneName": scene }))).await?;
        Ok(())
    }

    pub async fn get_input_list(&self) -> Result<Vec<Input>, ObsError> {
        let v = data(self.request("GetInputList", None).await?)?;
        array_field(&v, "inputs")?
            .iter()
            .map(|i| Ok(Input { name: str_field(i, "inputName")?, kind: str_field(i, "inputKind")? }))
            .collect()
    }

    pub async fn get_input_muted(&self, input: &str) -> Result<bool, ObsError> {
        let v = data(self.request("GetInputMute", Some(json!({ "inputName": input }))).await?)?;
        bool_field(&v, "inputMuted")
    }

    pub async fn set_input_muted(&self, input: &str, muted: bool) -> Result<(), ObsError> {
        self.request("SetInputMute", Some(json!({ "inputName": input, "inputMuted": muted })))
            .await?;
        Ok(())
    }

    pub async fn get_scene_items(&self, scene: &str) -> Result<Vec<SceneItem>, ObsError> {
        let v = data(self.request("GetSceneItemList", Some(json!({ "sceneName": scene }))).await?)?;
        array_field(&v, "sceneItems")?
            .iter()
            .map(|i| {
                Ok(SceneItem {
                    id: i
                        .get("sceneItemId")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| ObsError::Protocol("falta `sceneItemId`".into()))?,
                    source_name: str_field(i, "sourceName")?,
                    enabled: bool_field(i, "sceneItemEnabled")?,
                })
            })
            .collect()
    }

    pub async fn set_scene_item_enabled(&self, scene: &str, item_id: i64, enabled: bool) -> Result<(), ObsError> {
        self.request(
            "SetSceneItemEnabled",
            Some(json!({ "sceneName": scene, "sceneItemId": item_id, "sceneItemEnabled": enabled })),
        )
        .await?;
        Ok(())
    }

    pub async fn get_stream_status(&self) -> Result<StreamStatus, ObsError> {
        let v = data(self.request("GetStreamStatus", None).await?)?;
        Ok(StreamStatus {
            active: bool_field(&v, "outputActive")?,
            reconnecting: bool_field(&v, "outputReconnecting")?,
        })
    }

    pub async fn start_stream(&self) -> Result<(), ObsError> {
        self.request("StartStream", None).await?;
        Ok(())
    }

    pub async fn stop_stream(&self) -> Result<(), ObsError> {
        self.request("StopStream", None).await?;
        Ok(())
    }
}
