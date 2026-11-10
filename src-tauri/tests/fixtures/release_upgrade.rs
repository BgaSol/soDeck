//! Native release verification driver. Never included in a core installer.
use serde::Deserialize;
use serde_json::{json, Value};
use sodeck::{model::*, updates::*, AppCore};
use std::{fs, path::PathBuf, sync::Arc};
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    root: PathBuf,
    executable: PathBuf,
    current: String,
    version: String,
    plugins: Vec<Plugin>,
    core_package: Option<PathBuf>,
    manifests: Vec<UpdateManifest>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plugin {
    directory: PathBuf,
    id: String,
    current: String,
    version: String,
}
fn write(root: &std::path::Path, name: &str, value: &Value) -> Result<(), String> {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).map_err(|e| e.to_string())
}
fn settings(core: &Arc<AppCore>, input: &Input) -> Result<Value, String> {
    let mut result = serde_json::Map::new();
    for (id, page) in [
        ("sodeck.terminal", "appearance"),
        ("sodeck.files", "browser"),
        ("sodeck.files", "editor"),
        ("sodeck.network-proxy", "defaults"),
        ("sodeck.system-status", "sampling"),
        ("sodeck.docker", "display"),
    ] {
        if !input.plugins.iter().any(|p| p.id == id) {
            continue;
        }
        result.insert(
            format!("{id}/{page}"),
            core.settings_page(id, page)?["values"].clone(),
        );
    }
    Ok(Value::Object(result))
}
fn seed(input: &Input) -> Result<(), String> {
    let core = AppCore::open(input.root.clone(), input.current.clone())?;
    core.set_automatic_updates(false)?;
    for plugin in &input.plugins {
        if core.add_plugin(&plugin.directory)? != plugin.id {
            return Err("Unexpected old plugin ID".into());
        }
    }
    for (id, page, key, value) in [
        ("sodeck.terminal", "appearance", "fontSize", json!(16)),
        ("sodeck.files", "browser", "fontSize", json!(13)),
        ("sodeck.files", "editor", "wordWrap", json!(true)),
        ("sodeck.network-proxy", "defaults", "fixedPort", json!(2080)),
    ] {
        if !input.plugins.iter().any(|p| p.id == id) {
            continue;
        }
        let previous = core.settings_page(id, page)?;
        let mut values = previous["values"].clone();
        values[key] = value;
        core.save_settings(id, page, json!({"definitionToken":previous["definitionToken"],"baseRevision":previous["revision"],"requestId":format!("upgrade-{id}-{page}"),"values":values}))?;
    }
    core.start_enabled();
    check_plugins(&core, input, false)?;
    write(
        &input.root,
        "upgrade-baseline.json",
        &json!({"catalog":core.server_catalog(),"settings":settings(&core, input)?,"snapshot":core.snapshot()}),
    )?;
    core.shutdown()
}
fn check_plugins(core: &Arc<AppCore>, input: &Input, updated: bool) -> Result<(), String> {
    let snapshot = core.snapshot();
    for expected in &input.plugins {
        let actual = snapshot
            .plugins
            .iter()
            .find(|p| p.manifest.id == expected.id)
            .ok_or("Missing plugin")?;
        let version = if updated {
            &expected.version
        } else {
            &expected.current
        };
        if actual.manifest.version != *version
            || actual.status != PluginStatus::Running
            || !actual.enabled
        {
            return Err(format!(
                "Unexpected plugin version/status: {} {} {:?}",
                expected.id, actual.manifest.version, actual.status
            ));
        }
    }
    Ok(())
}
fn activate(input: &Input) -> Result<(), String> {
    let core = AppCore::open(input.root.clone(), input.version.clone())?;
    core.start_enabled();
    check_plugins(&core, input, true)?;
    let baseline: Value = serde_json::from_slice(
        &fs::read(input.root.join("upgrade-baseline.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if baseline["catalog"] != core.server_catalog()
        || baseline["settings"] != settings(&core, input)?
        || core.snapshot().preferences.automatic_updates
    {
        return Err(
            "Server catalog, plugin settings or update preference changed during upgrade".into(),
        );
    }
    write(
        &input.root,
        "upgrade-activated.json",
        &json!({"completed":true,"snapshot":core.snapshot(),"settings":settings(&core, input)?,"catalog":core.server_catalog()}),
    )?;
    core.shutdown()
}
fn prepare_install(handle: tauri::AppHandle, input: Input) -> Result<(), String> {
    let core = AppCore::open(input.root.clone(), input.current.clone())?;
    core.start_enabled();
    check_plugins(&core, &input, false)?;
    let client = match std::env::var("GH_TOKEN") {
        Ok(token) => ReleaseClient::test_github(token)?,
        Err(_) => ReleaseClient::github()?,
    }
    .test_pin(input.manifests.clone());
    let core_update = if let Some(package) = &input.core_package {
        let manifest = client
            .discover(
                &UpdateSource {
                    repository: REPOSITORY.into(),
                    tag_prefix: "v".into(),
                },
                CORE_ID,
                &input.current,
            )?
            .ok_or("No published core update")?;
        if manifest.version != input.version {
            return Err("Unexpected published core version".into());
        }
        let asset = manifest
            .platforms
            .get(sodeck::TARGET)
            .ok_or("Core target missing")?;
        let builder = handle
            .updater_builder()
            .target(sodeck::TARGET)
            .executable_path(&input.executable)
            .endpoints(vec!["https://github.com/BgaSol/soDeck".parse().unwrap()])
            .map_err(|e| e.to_string())?
            .restart_after_install(false);
        #[cfg(windows)]
        let builder = builder.installer_args([
            "/S".to_string(),
            format!(
                "/D={}",
                input
                    .executable
                    .parent()
                    .ok_or("Missing install directory")?
                    .display()
            ),
        ]);
        let updater = builder.build().map_err(|e| e.to_string())?;
        let update = updater.restore_update(json!({"version":manifest.version,"platforms":{sodeck::TARGET:{"url":asset.url,"signature":asset.signature}}})).map_err(|e| e.to_string())?.ok_or("Updater rejected published version")?;
        let bytes = fs::read(package).map_err(|e| e.to_string())?;
        update.verify(&bytes).map_err(|e| e.to_string())?;
        fs::write(input.root.join("core-download.bin"), &bytes).map_err(|e| e.to_string())?;
        Some((update, bytes))
    } else {
        None
    };
    for plugin in &input.plugins {
        let candidate = core
            .check_component(&plugin.id, &client)?
            .ok_or("No published plugin update")?;
        if candidate.version != plugin.version {
            return Err(format!("Unexpected update for {}", plugin.id));
        }
        if !core.download_plugin(&plugin.id, &client)? {
            return Err("Enabled plugin did not request restart".into());
        }
        check_plugins(&core, &input, false)?;
    }
    write(
        &input.root,
        "upgrade-prepared.json",
        &json!({"coreVersion":input.version,"snapshot":core.snapshot(),"updates":core.update_views()}),
    )?;
    let close = core.close_fixture_prepare(
        "main",
        sodeck::lifecycle::Target::Application {
            action: sodeck::lifecycle::Action::InstallAndRestart,
        },
    )?;
    core.ui_close_finish(
        close["token"].as_str().ok_or("Missing close token")?,
        serde_json::from_value(close["views"].clone()).map_err(|e| e.to_string())?,
        "discard",
    )?;
    drop(core);
    // The official native installer may exit this driver on Windows.
    if let Some((update, bytes)) = core_update {
        update.install(bytes).map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input: Input = serde_json::from_slice(&fs::read(&args[2]).unwrap()).unwrap();
    match args[1].as_str() {
        "seed" => seed(&input).unwrap(),
        "activate" => activate(&input).unwrap(),
        "install" => {
            let mut context = tauri::generate_context!();
            context.config_mut().app.windows.clear();
            context.package_info_mut().version = semver::Version::parse(&input.current).unwrap();
            let root = input.root.clone();
            tauri::Builder::default()
                .plugin(tauri_plugin_updater::Builder::new().build())
                .setup(move |app| {
                    let handle = app.handle().clone();
                    let finished = handle.clone();
                    std::thread::spawn(move || match prepare_install(handle, input) {
                        Ok(()) => finished.exit(0),
                        Err(error) => {
                            eprintln!("Release upgrade failed: {error}");
                            let _ = fs::write(root.join("upgrade-error.txt"), error);
                            finished.exit(1);
                        }
                    });
                    Ok(())
                })
                .run(context)
                .unwrap();
        }
        _ => panic!("Expected seed, install or activate"),
    }
}
