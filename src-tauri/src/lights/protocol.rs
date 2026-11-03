//! Protocolo Bluetooth LE de las tiras LED «Lotus Lamp» (app Lotus Lamp X, dispositivos `MELK-…`).
//!
//! El formato de los comandos está documentado por el proyecto de código abierto
//! <https://github.com/wporter82/lotus-lamp-python> (licencia MIT, `docs/PROTOCOL.md`), que lo obtuvo
//! de la app oficial y lo probó con una tira `MELK-OA10 5F`. Aquí está reescrito en Rust: no se usa Python.
//!
//! Todos los comandos miden 9 bytes y se escriben **sin respuesta** en la característica `FFF3` del servicio
//! `FFF0`. Los bytes de parámetro que no se usan valen `FF` (no `00`). La tira es de una sola zona: todos los
//! LED tienen el mismo color. Admite unas 10 órdenes por segundo.

use std::sync::OnceLock;

use serde::Serialize;
use uuid::Uuid;

pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x0000fff0_0000_1000_8000_00805f9b34fb);
pub const WRITE_CHAR_UUID: Uuid = Uuid::from_u128(0x0000fff3_0000_1000_8000_00805f9b34fb);

/// Un comando completo.
pub type Packet = [u8; 9];

/// El modo más alto que conoce la app de la tira (hay 213: 0 a 212).
pub const MAX_MODE: u8 = 212;

/// Color fijo. `7E 07 05 03 RR GG BB 10 EF`.
pub fn rgb(r: u8, g: u8, b: u8) -> Packet {
    [0x7E, 0x07, 0x05, 0x03, r, g, b, 0x10, 0xEF]
}

/// Brillo de 0 a 100. No cambia el modo.
pub fn brightness(level: u8) -> Packet {
    [0x7E, 0x07, 0x01, level.min(100), 0xFF, 0xFF, 0xFF, 0x00, 0xEF]
}

/// Velocidad de las animaciones, de 0 a 100. No afecta a un color fijo. Ojo: la longitud de este comando es 04.
pub fn speed(level: u8) -> Packet {
    [0x7E, 0x04, 0x02, level.min(100), 0xFF, 0xFF, 0xFF, 0x00, 0xEF]
}

/// Animación integrada, de 0 a 212.
pub fn mode(mode: u8) -> Packet {
    [0x7E, 0x07, 0x03, mode.min(MAX_MODE), 0xFF, 0xFF, 0xFF, 0x00, 0xEF]
}

/// Encender o apagar.
pub fn power(on: bool) -> Packet {
    [0x7E, 0x07, 0x04, u8::from(on), 0x00, 0xFF, 0xFF, 0x00, 0xEF]
}

/// ¿Parece una tira de esta familia por su nombre anunciado? (`MELK-OA10   5F`, con espacios.)
pub fn is_lotus_name(name: &str) -> bool {
    name.to_uppercase().contains("MELK")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModeInfo {
    pub id: u8,
    pub name: String,
    pub category: String,
}

/// Las 213 animaciones con su nombre en la app oficial y su categoría, ordenadas por categoría y número.
pub fn modes() -> &'static [ModeInfo] {
    static MODES: OnceLock<Vec<ModeInfo>> = OnceLock::new();
    MODES.get_or_init(|| {
        let names: std::collections::BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../data/lotus_mode_names.json")).expect("nombres de modos");
        let categories: std::collections::BTreeMap<String, Vec<u8>> =
            serde_json::from_str(include_str!("../../data/lotus_mode_categories.json")).expect("categorías de modos");
        // Orden de las categorías como en la app.
        let order = ["basic", "trans", "tail", "water", "curtain", "run", "runback", "flow"];
        let mut out = Vec::new();
        for category in order {
            let mut ids = categories.get(category).cloned().unwrap_or_default();
            ids.sort_unstable();
            for id in ids {
                let name = names.get(&id.to_string()).cloned().unwrap_or_else(|| format!("Modo {id}"));
                out.push(ModeInfo { id, name, category: category.to_owned() });
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(p: Packet) -> String {
        p.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn los_comandos_son_exactamente_los_de_la_documentacion_del_protocolo() {
        assert_eq!(hex(rgb(255, 0, 0)), "7E 07 05 03 FF 00 00 10 EF");
        assert_eq!(hex(rgb(255, 165, 0)), "7E 07 05 03 FF A5 00 10 EF");
        assert_eq!(hex(rgb(0, 0, 0)), "7E 07 05 03 00 00 00 10 EF");
        assert_eq!(hex(brightness(100)), "7E 07 01 64 FF FF FF 00 EF");
        assert_eq!(hex(brightness(75)), "7E 07 01 4B FF FF FF 00 EF");
        assert_eq!(hex(speed(50)), "7E 04 02 32 FF FF FF 00 EF");
        assert_eq!(hex(mode(0)), "7E 07 03 00 FF FF FF 00 EF");
        assert_eq!(hex(mode(138)), "7E 07 03 8A FF FF FF 00 EF");
        assert_eq!(hex(mode(143)), "7E 07 03 8F FF FF FF 00 EF");
        assert_eq!(hex(power(true)), "7E 07 04 01 00 FF FF 00 EF");
        assert_eq!(hex(power(false)), "7E 07 04 00 00 FF FF 00 EF");
    }

    #[test]
    fn los_valores_se_acotan_para_no_enviar_nada_invalido() {
        assert_eq!(brightness(250)[3], 100);
        assert_eq!(speed(101)[3], 100);
        assert_eq!(mode(255)[3], MAX_MODE);
        // Siempre 9 bytes con cabecera 7E y pie EF.
        for p in [rgb(1, 2, 3), brightness(0), speed(0), mode(0), power(true)] {
            assert_eq!((p[0], p[8], p.len()), (0x7E, 0xEF, 9));
        }
    }

    #[test]
    fn se_reconocen_las_tiras_por_el_nombre_con_espacios() {
        assert!(is_lotus_name("MELK-OA10   5F"));
        assert!(is_lotus_name("melk-oa10"));
        assert!(!is_lotus_name("Mi Reloj"));
        assert!(!is_lotus_name(""));
    }

    #[test]
    fn las_213_animaciones_tienen_nombre_y_una_sola_categoria() {
        let all = modes();
        assert_eq!(all.len(), 213);
        let mut ids: Vec<u8> = all.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids, (0..=212).collect::<Vec<u8>>(), "cada número del 0 al 212 aparece una vez");
        let flow = all.iter().find(|m| m.id == 143).unwrap();
        assert_eq!((flow.name.as_str(), flow.category.as_str()), ("W-R-W Flow", "flow"));
        assert_eq!(all[0].id, 0);
        assert!(all.iter().all(|m| !m.name.starts_with("Modo ")), "todos tienen nombre oficial");
    }
}
