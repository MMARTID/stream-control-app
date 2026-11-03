//! Avatar con un OBS simulado y movimiento de prueba, para verlo en un navegador:
//!
//!     cargo run --example avatar_dev -- [puerto]
//!
//! Abre http://127.0.0.1:<puerto>/avatar?preview=1 (por defecto el 4488). Sin argumentos hace
//! hablar al avatar con valores inventados; con `quieto` lo deja sin seguimiento (anima solo).

#[path = "../tests/common/mod.rs"]
mod common;

use std::{sync::Arc, time::Duration};

use stream_control_lib::{
    avatar::{Avatar, AvatarFrame},
    control::Control,
    settings::{AvatarSettings, ObsSettings, Settings, SettingsStore, secrets::MemoryStore},
};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port: u16 = args.iter().find_map(|a| a.parse().ok()).unwrap_or(4488);
    let still = args.iter().any(|a| a == "quieto");

    let obs = common::MockObs::start(None, None).await;
    let dir = std::env::temp_dir().join(format!("stream-control-avatar-dev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = SettingsStore::new(dir.join("settings.json"));
    store
        .save(&Settings {
            obs: ObsSettings { host: "127.0.0.1".into(), port: obs.port },
            avatar: AvatarSettings { enabled: true, port, character: "jason".into() },
            ..Default::default()
        })
        .unwrap();
    let control = Control::start(store, Arc::new(MemoryStore::default()));
    let avatar = Avatar::start(Arc::clone(&control));
    let mut status = avatar.subscribe();
    loop {
        if status.borrow_and_update().running {
            break;
        }
        status.changed().await.unwrap();
    }
    println!("Avatar en {}?preview=1", status.borrow().url);

    let started = std::time::Instant::now();
    let mut toggled = false;
    loop {
        tokio::time::sleep(Duration::from_millis(33)).await;
        // Cambia de personaje cada 20 s para ver los dos.
        let secs = started.elapsed().as_secs_f32();
        if (secs as u64 / 20) % 2 == 1 && !toggled || (secs as u64 / 20) % 2 == 0 && toggled {
            toggled = !toggled;
            let mut s = control.snapshot().settings;
            s.avatar.character = if toggled { "cal".into() } else { "jason".into() };
            let _ = control.save_settings(s).await;
        }
        if still {
            avatar.push(AvatarFrame::default());
            continue;
        }
        let talk = ((secs * 7.0).sin() * 0.5 + 0.5) * (((secs * 1.3).sin() > -0.2) as u8 as f32);
        avatar.push(AvatarFrame {
            tracking: true,
            mouth: talk * 0.9,
            smile: (secs * 0.5).sin() * 0.5,
            blink_left: if (secs % 4.0) < 0.15 { 1.0 } else { 0.0 },
            blink_right: if (secs % 4.0) < 0.15 { 1.0 } else { 0.0 },
            brow: (secs * 0.7).sin() * 0.6,
            yaw: (secs * 0.4).sin() * 0.7,
            pitch: (secs * 0.3).cos() * 0.4,
            roll: (secs * 0.35).sin() * 12.0,
        });
    }
}
