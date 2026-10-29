#![cfg_attr(windows, windows_subsystem = "windows")]
mod startup_config;
mod startup_diagnostics;
mod app;
mod brand;
mod native;
mod view_operations;
mod window_registry;
mod window_preferences;
mod window_chrome;
mod workbench;
mod workbench_contract;
mod theme;
mod atomic_file;
mod presentation;

fn main() {
    if let Some(code) = sodeck_plugin_host::run_internal_host_if_requested() {
        std::process::exit(code);
    }
    if let Err(error) = app::run() {
        startup_diagnostics::fatal(&error);
        std::process::exit(1);
    }
}
