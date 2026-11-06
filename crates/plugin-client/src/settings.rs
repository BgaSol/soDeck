//! Optional settings.v1 cache. This handler never opens a remote resource.
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
#[derive(Default)]
pub struct SettingsCache(Mutex<HashMap<String, (u64, String, Value)>>);
impl SettingsCache {
    pub fn changed(&self, input: &Value, pages: &[&str]) -> Result<Value, String> {
        let page = input["pageId"]
            .as_str()
            .filter(|p| pages.contains(p))
            .ok_or("未知设置页")?;
        let revision = input["revision"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("设置修订无效")?;
        let token = input["definitionToken"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("设置定义标识无效")?;
        if !input["values"].is_object()
            || serde_json::to_vec(&input["values"])
                .map_err(|e| e.to_string())?
                .len()
                > 24 * 1024
        {
            return Err("设置值无效或超过 24 KiB".into());
        }
        let mut cache = self.0.lock().unwrap();
        if cache.get(page).is_none_or(|(r, _, _)| *r < revision) {
            cache.insert(
                page.into(),
                (revision, token.into(), input["values"].clone()),
            );
        }
        Ok(json!({"accepted":true,"revision":revision.to_string()}))
    }
}
