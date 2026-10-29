#![cfg_attr(windows, windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;

fn record(data: &Path, value: Value) {
    fs::write(data.join("result.json"), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let data = handle.path().app_data_dir()?;
            fs::create_dir_all(&data)?;
            let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(data.join("probe.lock"))?;
            fs2::FileExt::try_lock_exclusive(&lock)?;
            // Keep the data lock through updater's native process exit.
            app.manage(Arc::new(lock));
            let version = handle.package_info().version.to_string();
            record(&data, json!({"stage":"started","version":version,"pid":std::process::id()}));
            std::thread::spawn(move || {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let request: Value = serde_json::from_slice(&fs::read(data.join("request.json"))?)?;
                    if request["fromVersion"] != version {
                        record(&data, json!({"stage":"reopened","version":version,"targetVersion":request["metadata"]["version"],"lockAcquired":true}));
                        return Ok(());
                    }
                    let hook_data = data.clone();
                    let updater = handle.updater_builder()
                        .pubkey(request["publicKey"].as_str().ok_or("missing public key")?)
                        .endpoints(vec!["https://github.com/BgaSol/soDeck/releases".parse()?])?
                        .on_before_exit(move || { fs::write(hook_data.join("handoff.json"), b"{\"beforeExit\":true}").unwrap(); })
                        .build()?;
                    let update = updater.restore_update(request["metadata"].clone())?.ok_or("no newer version")?;
                    let bytes = fs::read(request["installer"].as_str().ok_or("missing installer")?)?;
                    update.verify(&bytes)?;
                    record(&data, json!({"stage":"verified","version":version}));
                    if request["action"] == "install" {
                        record(&data, json!({"stage":"handoffAttempted","version":version}));
                        update.install(&bytes)?;
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => handle.exit(0),
                    Err(error) => { record(&data, json!({"stage":"failed","version":version,"error":error.to_string()})); handle.exit(1); }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("native updater probe");
}
