// Evita la ventana de consola extra en Windows en release. NO QUITAR.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    stream_control_lib::run()
}
