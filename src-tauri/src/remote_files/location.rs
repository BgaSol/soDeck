use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn location_uses_remote_representation_and_rejects_unknown_views() {
        assert_eq!(normalize(&json!({"path":"C:\\目录\\文件","pathStyle":"windows","filesystemView":"session-default"})).unwrap(),"/C:/目录/文件");
        assert_eq!(
            normalize(
                &json!({"path":"/目录","pathStyle":"posix","filesystemView":"session-default"})
            )
            .unwrap(),
            "/目录"
        );
        for path in ["C:relative", "\\\\server\\share", "/bad\0path"] {
            assert!(normalize(
                &json!({"path":path,"pathStyle":"windows","filesystemView":"session-default"})
            )
            .is_err());
        }
        assert!(
            normalize(&json!({"path":"/","pathStyle":"posix","filesystemView":"container"}))
                .is_err()
        );
    }
}
pub(super) fn normalize(p: &Value) -> Result<String, String> {
    if p["filesystemView"] != "session-default" {
        return Err("unsupportedLocation: 文件系统视角无法映射".into());
    }
    let value = p["path"]
        .as_str()
        .filter(|p| p.len() <= 4096 && !p.chars().any(char::is_control))
        .ok_or("unsupportedLocation: 路径无效")?;
    match p["pathStyle"].as_str() {
        Some("posix") if value.starts_with('/') => Ok(value.into()),
        Some("windows")
            if value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && value.as_bytes().get(1) == Some(&b':')
                && matches!(value.as_bytes().get(2), Some(b'\\' | b'/')) =>
        {
            Ok(format!("/{}", value.replace('\\', "/")))
        }
        _ => Err("unsupportedLocation: 路径表示暂不支持".into()),
    }
}
