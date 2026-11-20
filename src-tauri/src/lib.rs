pub mod commands;
pub mod component_ui;
pub mod connections;
mod content_export;
pub mod core;
pub mod desktop_behavior;
pub mod desktop_files;
mod desktop_open;
pub mod desktop_updates;
mod execution_output;
pub mod lifecycle;
mod local_files;
pub mod model;
pub mod network;
pub mod network_metrics;
pub mod official_plugins;
mod platform;
pub mod plugin_icons;
mod plugins;
mod remote_execution;
pub mod remote_files;
mod remote_program;
mod resource_owner;
mod resource_tasks;
pub mod servers;
mod session_exec;
mod session_process;
mod session_stream;
mod session_target;
pub mod settings;
pub mod shared_state;
mod storage;
pub mod streams;
pub mod terminal;
pub mod update_network;
pub mod updates;
pub mod validation;
pub mod windows;
pub mod workbench;
pub mod workbench_layout;
pub mod workbench_memory;
pub mod workbench_move;
pub mod workspaces;

pub use core::AppCore;

pub const API_VERSION: &str = "1";
pub const TARGET: &str = env!("SODECK_TARGET");

pub fn app_version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../tauri.conf.json"))
        .expect("valid application configuration")["version"]
        .as_str()
        .expect("application version")
        .to_owned()
}
