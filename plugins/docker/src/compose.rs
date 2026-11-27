use crate::browser::Index;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn label(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
}
pub fn directory(index: &Index, project: &str, environment: &Value) -> Result<Value, String> {
    if !index.complete || index.partial || index.phase != "ready" {
        return Err("请先刷新完整项目信息".into());
    }
    let rows: Vec<_> = index
        .rows
        .iter()
        .filter(|r| r["project"] == project)
        .collect();
    if rows.is_empty() {
        return Err("项目已不在当前索引中".into());
    }
    let mut paths = BTreeMap::new();
    for row in rows {
        let path = label(&row["composeDirectory"]).ok_or("项目未提供完整工作目录")?;
        paths.insert(path, ());
    }
    if paths.len() != 1 {
        return Err("项目工作目录不一致".into());
    }
    let path = *paths.keys().next().unwrap();
    let style = match environment["kind"].as_str() {
        Some("posix") if path.starts_with('/') => "posix",
        Some("windows")
            if path.len() >= 3
                && path.as_bytes()[0].is_ascii_alphabetic()
                && path.as_bytes()[1] == b':'
                && matches!(path.as_bytes()[2], b'/' | b'\\') =>
        {
            "windows"
        }
        _ => return Err("项目目录无法对应当前服务器的路径表示".into()),
    };
    Ok(json!({"path":path,"pathStyle":style,"filesystemView":"session-default"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index(rows: Value) -> Index {
        Index {
            rows: rows.as_array().unwrap().clone(),
            phase: "ready".into(),
            complete: true,
            ..Index::default()
        }
    }
    #[test]
    fn projects_use_all_members_and_never_guess_missing_or_conflicting_paths() {
        let mut source = index(
            json!([{"project":"shop","composeDirectory":"/srv/商店"},{"project":"shop","composeDirectory":"/srv/商店"}]),
        );
        assert_eq!(
            directory(&source, "shop", &json!({"kind":"posix"})).unwrap()["path"],
            "/srv/商店"
        );
        source.rows[1]["composeDirectory"] = json!("/another");
        assert!(directory(&source, "shop", &json!({"kind":"posix"}))
            .unwrap_err()
            .contains("不一致"));
        source.rows[1]["composeDirectory"] = Value::Null;
        assert!(directory(&source, "shop", &json!({"kind":"posix"})).is_err());
        source.partial = true;
        assert!(directory(&source, "shop", &json!({"kind":"posix"})).is_err());
    }
    #[test]
    fn paths_follow_the_connection_environment_not_the_engine_or_client() {
        let source = index(json!([{"project":"shop","composeDirectory":"C:\\项目"}]));
        assert_eq!(
            directory(&source, "shop", &json!({"kind":"windows"})).unwrap()["pathStyle"],
            "windows"
        );
        assert!(directory(&source, "shop", &json!({"kind":"posix"})).is_err());
        for path in ["relative", "\\\\host\\share", "/bad\npath"] {
            assert!(directory(
                &index(json!([{"project":"x","composeDirectory":path}])),
                "x",
                &json!({"kind":"windows"})
            )
            .is_err());
        }
        assert!(label(&json!("x".repeat(4097))).is_none());
    }
}
