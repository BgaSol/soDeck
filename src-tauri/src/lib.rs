pub mod component_ui;
pub mod connections;
pub mod core;
pub mod desktop_updates;
pub mod model;
mod platform;
mod plugins;
pub mod servers;
mod storage;
pub mod updates;
pub mod validation;
pub mod windows;
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
