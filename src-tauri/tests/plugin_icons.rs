use serde_json::json;
use sodeck::{AppCore, TARGET};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
#[test]
fn static_icons_use_the_installed_package_without_process_or_catalog() {
    let root = std::env::temp_dir().join(format!(
        "sodeck-icons-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("plugin/assets")).unwrap();
    fs::write(root.join("plugin/entry"), "not executed").unwrap();
    let definition = |version: &str| json!({"id":"arbitrary.tool","name":"tool","version":version,"apiVersion":"2.0","target":TARGET,"entry":"entry","icon":"custom","icons":{"custom":"assets/shape.svg"}});
    fs::write(
        root.join("plugin/plugin.json"),
        definition("1.0.0").to_string(),
    )
    .unwrap();
    fs::write(root.join("plugin/assets/shape.svg"), "first").unwrap();
    let core = AppCore::open(root.join("data"), "test".into()).unwrap();
    core.add_plugin(&root.join("plugin")).unwrap();
    assert!(core.snapshot().plugins[0].pid.is_none());
    assert_eq!(
        core.icon_resource("arbitrary.tool", "1.0.0", "custom")
            .unwrap()
            .0,
        b"first"
    );
    assert!(core
        .icon_resource("arbitrary.tool", "2.0.0", "custom")
        .is_err());
    assert!(core
        .icon_resource("arbitrary.tool", "1.0.0", "../custom")
        .is_err());
    fs::write(root.join("plugin/assets/shape.svg"), vec![0; 32769]).unwrap();
    assert!(core
        .icon_resource("arbitrary.tool", "1.0.0", "custom")
        .is_err());
    fs::remove_file(root.join("plugin/assets/shape.svg")).unwrap();
    assert!(core
        .icon_resource("arbitrary.tool", "1.0.0", "custom")
        .is_err());
    assert!(
        core.snapshot().plugins[0].error.is_none(),
        "asset failure does not fail the plugin"
    );
    drop(core);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn icon_mapping_rejects_external_paths_and_keeps_old_manifests_compatible() {
    use sodeck::{
        model::Manifest,
        plugin_icons::{url, validate},
    };
    let legacy:Manifest=serde_json::from_value(json!({"id":"test","name":"test","version":"1.0.0","apiVersion":"2.0","target":TARGET,"entry":"entry"})).unwrap();
    assert!(url(&legacy, "bot").is_none());
    for path in [
        "../a.svg",
        "/a.svg",
        "C:/a.svg",
        "a.svg#x",
        "a.png",
        "a/%2e.svg",
    ] {
        assert!(validate(&[("bot".into(), path.into())].into()).is_err());
    }
}
