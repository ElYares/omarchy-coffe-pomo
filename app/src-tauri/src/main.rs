// Sin consola en Windows. Aquí no aplica, pero es lo que genera Tauri y no
// cuesta nada dejarlo correcto.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    coffe_app_lib::run()
}
