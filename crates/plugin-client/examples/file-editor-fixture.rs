//! Test-only consumer: deliberately has no dependency on the files plugin.
use serde_json::json;
use sodeck_plugin_client::file_editor::EditorHost;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let editors = EditorHost::new("configFile");
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            editors.check_capabilities(&p)?;
            return Ok(json!({"apiVersion":"1","uiApiVersion":"2"}));
        }
        if method == "ui.event"
            && p["data"]["action"]
                .as_str()
                .is_some_and(|v| v.starts_with("fileEditor."))
        {
            return Ok(json!({"data":editors.source_event(client,&p)?}));
        }
        if p["viewType"] == "configFile" || editors.contains(p["viewId"].as_str().unwrap_or("")) {
            return editors.handle(client, method, &p, |_, _| Ok(()));
        }
        if method == "ui.disposeView" {
            editors.dispose_source(client, p["viewId"].as_str().ok_or("缺少视图")?)?;
        }
        Ok(json!({"data":{}}))
    })
}
