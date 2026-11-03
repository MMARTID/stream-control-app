//! OBS simulado para probar la app a mano sin OBS:
//!
//!     cargo run --example mock_obs -- [puerto] [contraseña]
//!
//! Habla obs-websocket v5 con tres escenas («Juego», «Charla», «Pausa / baño»), dos micros
//! («Micro», «Escritorio») y dos cámaras («Cámara móvil», «Webcam»). No sustituye a OBS real.
//! Imprime cada orden de escritura que recibe de la app.

#[path = "../tests/common/mod.rs"]
mod common;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port = args.next().map(|p| p.parse().expect("el puerto debe ser un número")).unwrap_or(4455);
    let password = args.next();
    let obs = common::MockObs::start(Some(port), password.as_deref()).await;
    println!("OBS simulado escuchando en 127.0.0.1:{}", obs.port);

    let mut seen = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let log = obs.log();
        for (kind, data) in &log[seen..] {
            if kind.starts_with("Set") || kind.starts_with("Start") || kind.starts_with("Stop") {
                println!("{kind} {data}");
            }
        }
        seen = log.len();
    }
}
