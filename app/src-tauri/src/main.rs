// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = app_lib::qt5_compat_entry() {
        std::process::exit(code);
    }
    if let Some(code) = app_lib::audio_supervisor_entry() {
        std::process::exit(code);
    }
    if let Some(code) = app_lib::audio_api_entry() {
        std::process::exit(code);
    }
    app_lib::run()
}
