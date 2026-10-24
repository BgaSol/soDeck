#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod native;
mod workbench;
mod view_operations;

fn main() {
    if let Some(code) = sodeck_plugin_host::run_internal_host_if_requested() { std::process::exit(code); }
    if let Err(error) = app::run() { eprintln!("SoDeck: {error}"); std::process::exit(1); }
}
