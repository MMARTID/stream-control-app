//! Prueba de concepto del cliente obs-websocket v5 contra un OBS simulado:
//! conectar, listar escenas, cambiar de escena, silenciar un micro, iniciar y parar el stream,
//! y la reconexión automática (RF-1 a RF-4).

mod common;

use std::time::Duration;

use common::MockObs;
use serde_json::json;
use stream_control_lib::obs::{ConnectionInfo, ConnectionState, ObsClient, ObsConfig, ObsError};
use tokio::{sync::watch, time::timeout};

fn config(port: u16, password: Option<&str>) -> ObsConfig {
    ObsConfig { host: "127.0.0.1".into(), port, password: password.map(str::to_owned) }
}

async fn wait_for_state(rx: &mut watch::Receiver<ConnectionInfo>, state: ConnectionState) -> ConnectionInfo {
    timeout(Duration::from_secs(15), async {
        loop {
            let info = rx.borrow_and_update().clone();
            if info.state == state {
                return info;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no se llegó al estado {state:?}; último: {:?}", rx.borrow().clone()))
}

#[tokio::test]
async fn conecta_lista_escenas_y_cambia_de_escena() {
    let obs = MockObs::start(None, None).await;
    let client = ObsClient::spawn(Some(config(obs.port, None)));
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;

    assert_eq!(client.get_version().await.unwrap(), "5.5.2");

    let list = client.get_scene_list().await.unwrap();
    assert_eq!(list.scenes, ["Juego", "Charla", "Pausa / baño"]);
    assert_eq!(list.current.as_deref(), Some("Juego"));

    client.set_current_scene("Charla").await.unwrap();
    assert_eq!(obs.current_scene(), "Charla");
}

#[tokio::test]
async fn silencia_un_micro_e_inicia_y_para_el_stream() {
    let obs = MockObs::start(None, None).await;
    let client = ObsClient::spawn(Some(config(obs.port, None)));
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;

    assert!(!client.get_input_muted("Micro").await.unwrap());
    client.set_input_muted("Micro", true).await.unwrap();
    assert!(client.get_input_muted("Micro").await.unwrap());
    assert!(obs.is_muted("Micro"));

    assert!(!client.get_stream_status().await.unwrap().active);
    client.start_stream().await.unwrap();
    assert!(client.get_stream_status().await.unwrap().active);
    client.stop_stream().await.unwrap();
    assert!(!client.get_stream_status().await.unwrap().active);
}

#[tokio::test]
async fn un_error_de_obs_llega_como_error_con_su_codigo() {
    let obs = MockObs::start(None, None).await;
    let client = ObsClient::spawn(Some(config(obs.port, None)));
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;

    match client.set_current_scene("No existe").await {
        Err(ObsError::Request { code, .. }) => assert_eq!(code, 600),
        other => panic!("se esperaba un error de OBS, llegó {other:?}"),
    }
    // La conexión sigue sana tras el error.
    assert_eq!(client.get_scene_list().await.unwrap().scenes.len(), 3);
}

#[tokio::test]
async fn los_eventos_de_obs_llegan_a_los_suscriptores() {
    let obs = MockObs::start(None, None).await;
    let client = ObsClient::spawn(Some(config(obs.port, None)));
    let mut events = client.subscribe_events();
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;

    client.set_current_scene("Charla").await.unwrap();
    let event = timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
    assert_eq!(event.event_type, "CurrentProgramSceneChanged");
    assert_eq!(event.event_data.unwrap()["sceneName"], json!("Charla"));
}

#[tokio::test]
async fn se_autentica_con_la_contrasena_de_obs() {
    let obs = MockObs::start(None, Some("secreto")).await;
    let client = ObsClient::spawn(Some(config(obs.port, Some("secreto"))));
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;
    assert_eq!(client.get_scene_list().await.unwrap().scenes.len(), 3);
}

#[tokio::test]
async fn con_la_contrasena_incorrecta_se_queda_desconectado_y_lo_dice() {
    let obs = MockObs::start(None, Some("secreto")).await;
    let client = ObsClient::spawn(Some(config(obs.port, Some("otra"))));
    let info = wait_for_state(&mut client.status(), ConnectionState::Disconnected).await;
    assert!(info.error.unwrap().contains("contraseña"));

    // No reintenta en bucle con la misma contraseña, pero al corregirla conecta.
    client.set_config(Some(config(obs.port, Some("secreto"))));
    wait_for_state(&mut client.status(), ConnectionState::Connected).await;
}

#[tokio::test]
async fn sin_conexion_las_peticiones_fallan_rapido_sin_bloquear() {
    let client = ObsClient::spawn(None);
    let started = std::time::Instant::now();
    assert!(matches!(client.get_scene_list().await, Err(ObsError::NotConnected)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn espera_a_que_obs_aparezca_y_se_reconecta_sola_si_se_reinicia() {
    // Puerto libre en el que todavía no hay nadie escuchando: OBS aún no está abierto (RF-3).
    let port = {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        probe.local_addr().unwrap().port()
    };
    let client = ObsClient::spawn(Some(config(port, None)));
    let mut status = client.status();
    wait_for_state(&mut status, ConnectionState::Reconnecting).await;
    assert!(matches!(client.get_scene_list().await, Err(ObsError::NotConnected)));

    // OBS se abre: la app conecta sin intervención.
    let obs = MockObs::start(Some(port), None).await;
    wait_for_state(&mut status, ConnectionState::Connected).await;
    assert_eq!(client.get_scene_list().await.unwrap().scenes.len(), 3);

    // OBS se cierra: la app lo nota y pasa a reconectando.
    obs.stop().await;
    wait_for_state(&mut status, ConnectionState::Reconnecting).await;

    // OBS se vuelve a abrir (RF-2): vuelve a conectar y las peticiones funcionan.
    let _obs = MockObs::start(Some(port), None).await;
    wait_for_state(&mut status, ConnectionState::Connected).await;
    assert_eq!(client.get_scene_list().await.unwrap().current.as_deref(), Some("Juego"));
}
