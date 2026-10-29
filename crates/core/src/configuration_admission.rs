//! Bound unique configuration writes before waiting for the configuration lock.
use super::*;
#[derive(Default)]
pub(crate) struct Admission(Mutex<HashMap<String, usize>>);
pub(crate) struct Permit<'a> { admission: &'a Admission, key: String }
impl Admission {
    pub(crate) fn enter(&self, caller: &Caller, method: &str, p: &Value) -> Result<Option<Permit<'_>>> {
        if !matches!(method, "servers.catalog.save" | "serverGroups.create" | "serverGroups.update" | "serverGroups.delete" | "serverGroups.assign" | "proxies.catalog.save" | "proxies.catalog.remove" | "credentials.configuration.set") { return Ok(None); }
        let key = format!("{}:{}:{}:{}", caller.source, caller.plugin_id.as_deref().unwrap_or("@shell"), method, field(p, "operationId")?);
        let mut entries = self.0.lock().unwrap();
        if !entries.contains_key(&key) && entries.len() >= 8 { return Err(CoreError::new("BUSY", "配置保存繁忙，请稍后重试")); }
        *entries.entry(key.clone()).or_default() += 1;
        Ok(Some(Permit { admission: self, key }))
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut entries = self.admission.0.lock().unwrap();
        let count = entries.get_mut(&self.key).unwrap();
        *count -= 1;
        if *count == 0 { entries.remove(&self.key); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_unique_writes_and_reuses_the_original_slot() {
        let admission = Admission::default(); let caller = Caller::shell();
        let mut permits = Vec::new();
        for i in 0..8 { permits.push(admission.enter(&caller, "servers.catalog.save", &json!({"operationId":i.to_string()})).unwrap()); }
        let same = admission.enter(&caller, "servers.catalog.save", &json!({"operationId":"0"})).unwrap();
        assert_eq!(admission.enter(&caller, "proxies.catalog.save", &json!({"operationId":"9"})).err().unwrap().code, "BUSY");
        drop(same); permits.pop();
        assert!(admission.enter(&caller, "proxies.catalog.save", &json!({"operationId":"9"})).is_ok());
    }
}
