use super::*;
pub(super) struct CopyRequest {
    pub capture: bool,
    pub content_only: bool,
}
impl CopyRequest {
    pub fn parse(p: &Value) -> Result<Self, OperationError> {
        let capture = p["destinationGuard"]["kind"] == "capture";
        if !["absent", "match", "capture"]
            .contains(&p["destinationGuard"]["kind"].as_str().unwrap_or(""))
            || capture && p["destination"]["side"] != "remote"
        {
            return Err(failure("invalid", "复制目标检查无效"));
        }
        let content_only = match p.get("metadataPolicy") {
            None => false,
            Some(v) if v == "contentOnly" => true,
            _ => return Err(failure("invalid", "复制属性策略无效")),
        };
        for field in ["source", "destination"] {
            let e = &p[field];
            if e["side"] == "localPath"
                && (field != "source"
                    || p["destination"]["side"] != "remote"
                    || e.as_object()
                        .is_none_or(|v| v.keys().any(|k| !["side", "path"].contains(&k.as_str()))))
            {
                return Err(failure("invalid", "本机路径只能用作远端复制来源"));
            }
        }
        Ok(Self {
            capture,
            content_only,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_path_is_read_only_and_capture_is_remote_only() {
        let mut p = json!({"source":{"side":"localPath","path":"/a"},"destination":{"side":"remote","path":"/b"},"destinationGuard":{"kind":"capture"},"metadataPolicy":"contentOnly"});
        assert!(CopyRequest::parse(&p).unwrap().content_only);
        p["destination"]["side"] = json!("localPath");
        assert!(CopyRequest::parse(&p).is_err());
        p["source"]["side"] = json!("remote");
        p["destinationGuard"]["kind"] = json!("absent");
        assert!(CopyRequest::parse(&p).is_err());
    }
}
