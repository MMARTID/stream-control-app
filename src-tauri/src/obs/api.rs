//! Peticiones concretas de obs-websocket v5 que usa la app.
//!
//! Nombres de petición y campos comprobados contra docs/generated/protocol.md de
//! obsproject/obs-websocket: GetVersion, GetSceneList, SetCurrentProgramScene,
//! GetInputList, GetInputMute, SetInputMute, GetSceneItemList, SetSceneItemEnabled,
//! GetStreamStatus, StartStream, StopStream, GetStats, GetSourceScreenshot, CreateInput, CreateScene,
//! GetVideoSettings, CreateSceneItem, GetSceneItemTransform y SetSceneItemTransform.

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
    /// Posición en la pila de la escena (0 = el de más abajo), si OBS la envía.
    pub index: Option<i64>,
    /// `inputKind` de OBS (p. ej. `dshow_input`). Vacío en escenas anidadas.
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StreamStatus {
    pub active: bool,
    pub reconnecting: bool,
    pub duration_ms: u64,
    pub bytes: u64,
    pub skipped_frames: u64,
    pub total_frames: u64,
    pub congestion: f64,
}

/// `GetStats`: carga de OBS.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObsStats {
    pub cpu_usage: f64,
    pub active_fps: f64,
    pub render_skipped_frames: u64,
    pub render_total_frames: u64,
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
                    index: i.get("sceneItemIndex").and_then(Value::as_i64),
                    kind: i.get("inputKind").and_then(Value::as_str).map(str::to_owned),
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

    pub async fn create_scene(&self, name: &str) -> Result<(), ObsError> {
        self.request("CreateScene", Some(json!({ "sceneName": name }))).await?;
        Ok(())
    }

    /// Tamaño del lienzo base de OBS (`baseWidth` × `baseHeight` de `GetVideoSettings`).
    pub async fn get_canvas_size(&self) -> Result<(u32, u32), ObsError> {
        let v = data(self.request("GetVideoSettings", None).await?)?;
        let n = |key: &str| {
            v.get(key)
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .filter(|n| *n > 0)
                .ok_or_else(|| ObsError::Protocol(format!("falta `{key}`")))
        };
        Ok((n("baseWidth")?, n("baseHeight")?))
    }

    /// Añade a una escena una fuente que ya existe en OBS.
    pub async fn create_scene_item(&self, scene: &str, source: &str, enabled: bool) -> Result<i64, ObsError> {
        let v = data(
            self.request(
                "CreateSceneItem",
                Some(json!({ "sceneName": scene, "sourceName": source, "sceneItemEnabled": enabled })),
            )
            .await?,
        )?;
        v.get("sceneItemId").and_then(Value::as_i64).ok_or_else(|| ObsError::Protocol("falta `sceneItemId`".into()))
    }

    pub async fn get_scene_item_transform(&self, scene: &str, item_id: i64) -> Result<Value, ObsError> {
        let v = data(
            self.request("GetSceneItemTransform", Some(json!({ "sceneName": scene, "sceneItemId": item_id }))).await?,
        )?;
        v.get("sceneItemTransform").cloned().ok_or_else(|| ObsError::Protocol("falta `sceneItemTransform`".into()))
    }

    pub async fn set_scene_item_transform(&self, scene: &str, item_id: i64, transform: Value) -> Result<(), ObsError> {
        self.request(
            "SetSceneItemTransform",
            Some(json!({ "sceneName": scene, "sceneItemId": item_id, "sceneItemTransform": transform })),
        )
        .await?;
        Ok(())
    }

    /// Crea una entrada (p. ej. una fuente de navegador) en una escena. Devuelve el id del elemento de escena.
    pub async fn create_input(
        &self,
        scene: &str,
        input_name: &str,
        input_kind: &str,
        settings: Value,
    ) -> Result<i64, ObsError> {
        let v = data(
            self.request(
                "CreateInput",
                Some(json!({
                    "sceneName": scene,
                    "inputName": input_name,
                    "inputKind": input_kind,
                    "inputSettings": settings,
                    "sceneItemEnabled": true,
                })),
            )
            .await?,
        )?;
        v.get("sceneItemId").and_then(Value::as_i64).ok_or_else(|| ObsError::Protocol("falta `sceneItemId`".into()))
    }

    pub async fn get_stream_status(&self) -> Result<StreamStatus, ObsError> {
        let v = data(self.request("GetStreamStatus", None).await?)?;
        let num = |key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
        Ok(StreamStatus {
            active: bool_field(&v, "outputActive")?,
            reconnecting: bool_field(&v, "outputReconnecting")?,
            duration_ms: num("outputDuration"),
            bytes: num("outputBytes"),
            skipped_frames: num("outputSkippedFrames"),
            total_frames: num("outputTotalFrames"),
            congestion: v.get("outputCongestion").and_then(Value::as_f64).unwrap_or(0.0),
        })
    }

    pub async fn get_stats(&self) -> Result<ObsStats, ObsError> {
        let v = data(self.request("GetStats", None).await?)?;
        let num = |key: &str| v.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        Ok(ObsStats {
            cpu_usage: num("cpuUsage"),
            active_fps: num("activeFps"),
            render_skipped_frames: num("renderSkippedFrames") as u64,
            render_total_frames: num("renderTotalFrames") as u64,
        })
    }

    /// Formatos de imagen que admite `GetSourceScreenshot` (campo `supportedImageFormats` de `GetVersion`).
    pub async fn get_image_formats(&self) -> Result<Vec<String>, ObsError> {
        let v = data(self.request("GetVersion", None).await?)?;
        Ok(v.get("supportedImageFormats")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default())
    }

    /// Captura de una fuente o escena, escalada a `width` px de ancho. Devuelve un «data URI»
    /// listo para usar como `src` de una imagen.
    pub async fn get_source_screenshot(
        &self,
        source: &str,
        format: &str,
        width: u32,
        quality: i32,
    ) -> Result<String, ObsError> {
        let v = data(
            self.request(
                "GetSourceScreenshot",
                Some(json!({
                    "sourceName": source,
                    "imageFormat": format,
                    "imageWidth": width,
                    "imageCompressionQuality": quality,
                })),
            )
            .await?,
        )?;
        let image = str_field(&v, "imageData")?;
        // La documentación dice «Base64-encoded screenshot»; OBS lo devuelve ya como data URI.
        // Se acepta también base64 a secas.
        Ok(if image.starts_with("data:") {
            image
        } else {
            let mime = if format == "jpg" { "jpeg" } else { format };
            format!("data:image/{mime};base64,{image}")
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
