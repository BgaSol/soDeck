//! Only explicit window preferences survive an application run.
use crate::window_registry::WindowBounds;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    schema_version: u32,
    main: Option<WindowBounds>,
}

fn valid(bounds: &WindowBounds) -> bool {
    (760..=10000).contains(&bounds.width) && (480..=10000).contains(&bounds.height)
        && bounds.x.abs_diff(0) <= 1_000_000 && bounds.y.abs_diff(0) <= 1_000_000
}

pub fn save(data: &Path, main: Option<WindowBounds>) -> Result<(), String> {
    let value = Preferences { schema_version: 1, main: main.filter(valid) };
    let pending = data.join("window-preferences.pending");
    let path = data.join("window-preferences.json");
    // Never follow a link while replacing app-owned preference files.
    for path in [&pending, &path] {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.file_type().is_file() { return Err("窗口偏好路径不是普通文件".into()); }
        }
    }
    let mut file = fs::File::create(&pending).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?)
        .and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    drop(file);
    crate::workbench::replace(&pending, &path)
}

pub fn retire(data: &Path) -> Result<(), String> {
    let mut errors = vec![];
    for entry in fs::read_dir(data).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let legacy = name == "workbench.json" || name == "workbench.pending"
            || ((name.starts_with("workbench.pre-v3-") || name.starts_with("workbench.damaged-")) && name.ends_with(".json"));
        if !legacy { continue; }
        let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            errors.push(format!("{name} 不是普通文件"));
        } else if let Err(e) = fs::remove_file(entry.path()) { errors.push(e.to_string()); }
    }
    if errors.is_empty() { Ok(()) } else {
        Err(format!("旧工作记录未能清理，下次启动仍不会恢复：{}", errors.join("；")))
    }
}

pub fn load(data: &Path) -> (Option<WindowBounds>, Vec<String>) {
    let mut warnings = vec![];
    let path = data.join("window-preferences.json");
    let main = if path.exists() {
        match fs::symlink_metadata(&path).ok().filter(|m| m.file_type().is_file())
            .and_then(|_| fs::read(&path).ok())
            .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
            .filter(|p| p.schema_version == 1) {
            Some(p) => p.main.filter(valid),
            None => { warnings.push("窗口偏好无法读取，已使用默认窗口；可重试保存偏好".into()); None }
        }
    } else {
        let old = data.join("workbench.json");
        fs::symlink_metadata(&old).ok().filter(|m| m.file_type().is_file() && m.len() <= 16 * 1024 * 1024)
            .and_then(|_| fs::read(old).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .filter(|v| v.get("schemaVersion").is_none() || [serde_json::json!(2), serde_json::json!(3)].contains(&v["schemaVersion"]))
            .and_then(|v| serde_json::from_value::<WindowBounds>(v["windows"]["main"]["bounds"].clone()).ok())
            .filter(valid)
    };
    if !path.exists() {
        if let Err(e) = save(data, main.clone()) { warnings.push(format!("窗口偏好未保存：{e}")); }
    }
    if let Err(e) = retire(data) { warnings.push(e); }
    (main, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn imports_only_geometry_and_retires_snapshots_idempotently() {
        let dir = tempfile::tempdir().unwrap(); let data = dir.path();
        fs::write(data.join("workbench.json"), json!({"schemaVersion":3,"views":[{"title":"old"}],"windows":{"main":{"bounds":{"x":20,"y":30,"width":1280,"height":820,"maximized":true}}}}).to_string()).unwrap();
        fs::write(data.join("workbench.pre-v3-test.json"), "old").unwrap();
        fs::write(data.join("settings.json"), "keep").unwrap();
        let (bounds, warnings) = load(data); assert!(warnings.is_empty());
        assert!(bounds.unwrap().maximized);
        assert!(!data.join("workbench.json").exists());
        let saved = fs::read_to_string(data.join("window-preferences.json")).unwrap();
        assert!(!saved.contains("views") && !saved.contains("old"));
        assert_eq!(fs::read_to_string(data.join("settings.json")).unwrap(), "keep");
        assert!(load(data).1.is_empty());
    }
    #[test]
    fn broken_history_and_cleanup_failure_never_restore_work() {
        let dir = tempfile::tempdir().unwrap(); let data = dir.path();
        fs::write(data.join("workbench.json"), "broken").unwrap();
        fs::create_dir(data.join("workbench.pending")).unwrap();
        let (bounds, warnings) = load(data); assert!(bounds.is_none()); assert_eq!(warnings.len(),1);
        assert!(data.join("workbench.pending").is_dir());
        fs::remove_dir(data.join("workbench.pending")).unwrap(); assert!(retire(data).is_ok());
    }
    #[test]
    fn only_known_legacy_versions_can_supply_bounds() {
        for version in [serde_json::Value::Null, json!(2), json!(3), json!(99)] {
            let dir = tempfile::tempdir().unwrap();
            let mut legacy = json!({"windows":{"main":{"bounds":{"x":10,"y":20,"width":1280,"height":820}}},"views":["never restore"]});
            if !version.is_null() { legacy["schemaVersion"] = version.clone(); }
            fs::write(dir.path().join("workbench.json"), legacy.to_string()).unwrap();
            fs::write(dir.path().join("workbench.damaged-fixture.json"), "broken").unwrap();
            let (bounds,warnings)=load(dir.path());
            assert!(warnings.is_empty());assert_eq!(bounds.is_some(),version!=json!(99));
            assert!(!dir.path().join("workbench.json").exists());
            assert!(!dir.path().join("workbench.damaged-fixture.json").exists());
        }
    }
    #[test]
    fn failed_preference_write_preserves_previous_file() {
        let dir = tempfile::tempdir().unwrap(); save(dir.path(), None).unwrap();
        let before = fs::read(dir.path().join("window-preferences.json")).unwrap();
        fs::create_dir(dir.path().join("window-preferences.pending")).unwrap();
        assert!(save(dir.path(), None).is_err());
        assert_eq!(fs::read(dir.path().join("window-preferences.json")).unwrap(),before);
    }
}
