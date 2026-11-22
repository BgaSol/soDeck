//! Native release verification driver. Never included in a core installer.
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sodeck::settings::declaration::Page;
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
        ("sodeck.mcp", "connection"),
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
fn preserved_values(before: &Value, defaults: Map<String, Value>) -> Value {
    Value::Object(
        defaults
            .into_iter()
            .map(|(key, default)| {
                let value = before.get(&key).cloned().unwrap_or(default);
                (key, value)
            })
            .collect(),
    )
}
fn seed(input: &Input) -> Result<(), String> {
    // Seed the installed on-disk records, including retired protocols. Never execute
    // an old plugin through the current core just to prepare an upgrade test.
    let mut records = Vec::new();
    let mut pages = Map::new();
    let mut baseline_settings = Map::new();
    for plugin in &input.plugins {
        let manifest: Value = serde_json::from_slice(
            &fs::read(plugin.directory.join("plugin.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if manifest["id"] != plugin.id
            || manifest["version"] != plugin.current
            || manifest["target"] != sodeck::TARGET
        {
            return Err("Unexpected old package identity".into());
        }
        records.push(json!({"directory":plugin.directory,"manifest":manifest,"enabled":true}));
        for raw in manifest["settings"]["pages"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let page: Page = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            page.validate()?;
            let key = format!("{}/{}", plugin.id, page.id);
            let mut values = page.defaults();
            for (id, name, field, value) in [
                ("sodeck.terminal", "appearance", "fontSize", json!(16)),
                ("sodeck.files", "browser", "fontSize", json!(13)),
                ("sodeck.files", "editor", "wordWrap", json!(true)),
                ("sodeck.network-proxy", "defaults", "fixedPort", json!(2080)),
                ("sodeck.docker", "display", "refreshSeconds", json!("30")),
            ] {
                if plugin.id == id && page.id == name {
                    values.insert(field.into(), value);
                }
            }
            page.validate_values(&values)?;
            baseline_settings.insert(key.clone(), json!(values));
            pages.insert(key, json!({"token":page.token(&plugin.id,&plugin.current,manifest["settings"]["schemaVersion"].as_u64().ok_or("Missing settings schema")?),"revision":1,"structure":page.structure(),"values":values,"history":[]}));
        }
        let data = input.root.join("plugin-data").join(&plugin.id);
        fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        fs::write(data.join("upgrade-marker.txt"), b"keep plugin user data")
            .map_err(|e| e.to_string())?;
    }
    write(&input.root, "plugins.json", &json!(records))?;
    write(
        &input.root,
        "component-settings.json",
        &json!({"pages":pages}),
    )?;
    let core = AppCore::open(input.root.clone(), input.current.clone())?;
    core.set_automatic_updates(false)?;
    core.start_enabled();
    check_plugins(&core, input, false)?;
    write(
        &input.root,
        "upgrade-baseline.json",
        &json!({"catalog":core.server_catalog(),"settings":baseline_settings,"snapshot":core.snapshot()}),
    )?;
    core.shutdown()
}
fn check_plugins(core: &Arc<AppCore>, input: &Input, updated: bool) -> Result<(), String> {
    let snapshot = core.snapshot();
    for expected in &input.plugins {
        if fs::read(
            input
                .root
                .join("plugin-data")
                .join(&expected.id)
                .join("upgrade-marker.txt"),
        )
        .map_err(|e| e.to_string())?
            != b"keep plugin user data"
        {
            return Err("Plugin data changed during upgrade".into());
        }
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
        let executable = updated || sodeck_plugin_protocol::supports(&actual.manifest.api_version);
        let expected_status = if executable {
            PluginStatus::Running
        } else {
            PluginStatus::Failed
        };
        if actual.manifest.version != *version
            || actual.status != expected_status
            || (!executable && (actual.pid.is_some() || actual.error.is_none()))
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
    for (key, before) in baseline["settings"]
        .as_object()
        .ok_or("Missing baseline settings")?
    {
        let (plugin, page_id) = key
            .rsplit_once('/')
            .ok_or("Invalid baseline settings page")?;
        let current = core.settings_page(plugin, page_id)?;
        let declaration: Page =
            serde_json::from_value(current["declaration"].clone()).map_err(|e| e.to_string())?;
        // Compare surviving fields and declared defaults, rather than an obsolete schema.
        if current["values"] != preserved_values(before, declaration.defaults()) {
            return Err(format!("Plugin settings changed during upgrade: {key}"));
        }
    }
    if baseline["catalog"] != core.server_catalog() || core.snapshot().preferences.automatic_updates
    {
        return Err("Server catalog or update preference changed during upgrade".into());
    }
    write(
        &input.root,
        "upgrade-activated.json",
        &json!({"completed":true,"snapshot":core.snapshot(),"settings":settings(&core, input)?,"catalog":core.server_catalog()}),
    )?;
    core.shutdown()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_seed_keeps_retired_installation_and_settings_without_starting_it() {
        let root = std::env::temp_dir().join(format!(
            "sodeck-upgrade-seed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let package = root.join("old-package");
        fs::create_dir_all(&package).unwrap();
        write(&package, "plugin.json", &json!({
            "id":"sodeck.terminal","name":"Old terminal","version":"0.10.2","apiVersion":"1.0","target":sodeck::TARGET,
            "entry":"must-not-execute","ui":{"apiVersion":1,"obsolete":true},
            "settings":{"schemaVersion":1,"apiVersion":1,"pages":[{
                "id":"appearance","title":"Appearance","viewTypes":["terminal"],"groups":[{"id":"text","title":"Text","fields":[{
                    "key":"fontSize","title":"Font size","type":"number","default":12,"min":8,"max":32,"step":1,"apply":"newInstance"
                }]}]
            }]}
        })).unwrap();
        let input = Input {
            root: root.join("data"),
            executable: root.join("unused"),
            current: "2.0.0".into(),
            version: "2.0.0".into(),
            plugins: vec![Plugin {
                directory: package,
                id: "sodeck.terminal".into(),
                current: "0.10.2".into(),
                version: "0.11.0".into(),
            }],
            core_package: None,
            manifests: vec![],
        };
        seed(&input).unwrap();
        let baseline: Value =
            serde_json::from_slice(&fs::read(input.root.join("upgrade-baseline.json")).unwrap())
                .unwrap();
        assert_eq!(
            baseline["settings"]["sodeck.terminal/appearance"]["fontSize"],
            16
        );
        assert_eq!(baseline["snapshot"]["plugins"][0]["status"], "failed");
        assert_eq!(baseline["snapshot"]["plugins"][0]["enabled"], true);
        assert!(baseline["snapshot"]["plugins"][0]["pid"].is_null());
        let saved: Value =
            serde_json::from_slice(&fs::read(input.root.join("component-settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved["pages"]["sodeck.terminal/appearance"]["values"]["fontSize"],
            16
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upgrade_preserves_existing_values_and_initializes_only_new_fields() {
        let before = json!({"refreshSeconds":"30", "logTail":"500"});
        let defaults = json!({"refreshSeconds":"5", "showLogMinimap":true});
        let expected = preserved_values(&before, defaults.as_object().unwrap().clone());
        assert_eq!(
            expected,
            json!({"refreshSeconds":"30", "showLogMinimap":true})
        );
        assert_ne!(
            expected, defaults,
            "Resetting the retained setting must fail"
        );
        assert_ne!(
            expected,
            json!({"refreshSeconds":"30", "showLogMinimap":false}),
            "A new field must have its declared default"
        );
        assert_eq!(before["logTail"], "500");
    }

    #[test]
    fn unchanged_schema_requires_every_previous_value() {
        let before = json!({"fontSize":16, "wordWrap":true});
        let defaults = json!({"fontSize":12, "wordWrap":false});
        assert_eq!(
            preserved_values(&before, defaults.as_object().unwrap().clone()),
            before
        );
    }
}
fn release_client() -> Result<ReleaseClient, String> {
    match std::env::var("GH_TOKEN") {
        Ok(token) => ReleaseClient::test_github(token),
        Err(_) => ReleaseClient::github(),
    }
}
fn discover(input: &Input) -> Result<(), String> {
    let client = release_client()?;
    let mut discovered = Vec::new();
    for expected in &input.manifests {
        let current = if expected.id == CORE_ID {
            &input.current
        } else {
            &input
                .plugins
                .iter()
                .find(|p| p.id == expected.id)
                .ok_or("Missing current plugin version")?
                .current
        };
        let manifest = client
            .discover(
                expected.source.as_ref().ok_or("Missing published source")?,
                &expected.id,
                current,
            )?
            .ok_or("No published update discovered")?;
        if serde_json::to_value(&manifest).map_err(|e| e.to_string())?
            != serde_json::to_value(expected).map_err(|e| e.to_string())?
        {
            return Err(format!(
                "Discovered latest manifest differs from the selected Release: {}",
                expected.id
            ));
        }
        discovered.push(manifest);
    }
    write(
        &input.root,
        "upgrade-discovered.json",
        &json!({"completed":true,"target":sodeck::TARGET,"manifests":discovered}),
    )
}
fn prepare_install(handle: tauri::AppHandle, input: Input) -> Result<(), String> {
    let core = AppCore::open(input.root.clone(), input.current.clone())?;
    core.start_enabled();
    check_plugins(&core, &input, false)?;
    let client = release_client()?.test_pin(input.manifests.clone());
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
        let update = updater
            .restore_update(serde_json::to_value(&manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .ok_or("Updater rejected published version")?;
        if update.body.as_deref() != Some(manifest.notes.as_str()) {
            return Err("Updater did not preserve published release notes".into());
        }
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
        "discover" => discover(&input).unwrap(),
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
        _ => panic!("Expected seed, discover, install or activate"),
    }
}
