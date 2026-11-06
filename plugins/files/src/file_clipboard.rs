use serde_json::{json, Value};

pub fn status(
    copied: &Value,
    context: &Value,
    revision: &Value,
    target: &str,
    consumed: bool,
) -> Value {
    if copied.is_null() || copied["revision"] != *revision || consumed {
        return json!({"remote":false,"reason":"没有可粘贴的文件"});
    }
    if copied["roots"].as_array().is_none_or(Vec::is_empty) {
        return json!({"remote":false,"reason":"没有可粘贴的文件"});
    }
    if copied["context"] != *context {
        return json!({"remote":false,"reason":"复制来源属于其他连接"});
    }
    let target = target.trim_end_matches('/');
    if copied["roots"].as_array().is_some_and(|roots| {
        roots.iter().filter_map(Value::as_str).any(|root| {
            let root = root.trim_end_matches('/');
            target == root
                || target
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }) {
        return json!({"remote":false,"reason":"不能粘贴到来源目录内部"});
    }
    json!({"remote":true,"clipboardId":copied["id"],"kind":if copied["cut"]==true {"move"} else {"copy"}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_clipboard_requires_current_source_and_valid_destination() {
        let context = json!({"connectionId":"one","generation":1});
        let copied = json!({"id":"copy-one","roots":["/home/source"],"context":context,"revision":"7","cut":true});
        let valid = status(&copied, &context, &json!("7"), "/home/target", false);
        assert_eq!(valid["remote"], true);
        assert_eq!(valid["kind"], "move");
        assert_eq!(valid["clipboardId"], "copy-one");
        for target in ["/home/source", "/home/source/", "/home/source/child"] {
            assert_eq!(
                status(&copied, &context, &json!("7"), target, false)["remote"],
                false
            );
        }
        assert_eq!(
            status(&copied, &context, &json!("7"), "/home/source-other", false)["remote"],
            true
        );
        assert_eq!(
            status(&copied, &context, &json!("8"), "/home/target", false)["remote"],
            false
        );
        assert_eq!(
            status(
                &copied,
                &json!({"connectionId":"two","generation":1}),
                &json!("7"),
                "/home/target",
                false
            )["remote"],
            false
        );
        assert_eq!(
            status(&copied, &context, &json!("7"), "/home/target", true)["remote"],
            false
        );
        assert_eq!(
            status(&Value::Null, &context, &json!("7"), "/home/target", false)["remote"],
            false
        );
        assert_eq!(copied["roots"][0], "/home/source");
    }
}
