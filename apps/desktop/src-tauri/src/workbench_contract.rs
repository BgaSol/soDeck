use serde_json::Value;
use std::sync::OnceLock;
#[path = "workbench_contract.generated.rs"]
pub mod generated;
fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../contracts/workbench-v3.schema.json"
        ))
        .expect("checked contract schema")
    })
}
pub fn validate(method: &str, value: &Value) -> Result<(), String> {
    validate_kind("x-methods", method, value)
}
pub fn validate_kind(kind: &str, method: &str, value: &Value) -> Result<(), String> {
    if let Some(name) = schema()[kind][method].as_str() {
        check(&schema()["$defs"][name], value, method)
    } else {
        Ok(())
    }
}
fn check(s: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(reference) = s["$ref"].as_str() {
        return check(
            &schema()["$defs"][reference.rsplit('/').next().unwrap()],
            value,
            path,
        );
    }
    if let Some(parts) = s["allOf"].as_array() {
        for part in parts {
            check(part, value, path)?;
        }
    }
    if let Some(condition) = s.get("if") {
        if check(condition, value, path).is_ok() {
            if let Some(then) = s.get("then") {
                check(then, value, path)?;
            }
        }
    }
    let invalid = || format!("{path} 不符合工作台契约");
    if s["anyOf"]
        .as_array()
        .is_some_and(|parts| !parts.iter().any(|part| check(part, value, path).is_ok()))
    {
        return Err(invalid());
    }
    if s["enum"]
        .as_array()
        .is_some_and(|items| !items.contains(value))
    {
        return Err(invalid());
    }
    match s["type"].as_str() {
        Some("null") => {
            if !value.is_null() {
                return Err(invalid());
            }
        }
        Some("object") => {
            let data = value.as_object().ok_or_else(invalid)?;
            if let Some(required) = s["required"].as_array() {
                for key in required {
                    if !data.contains_key(key.as_str().unwrap()) {
                        return Err(invalid());
                    }
                }
            }
            for (key, item) in data {
                if let Some(field) = s["properties"].get(key) {
                    check(field, item, &format!("{path}.{key}"))?;
                } else if s["additionalProperties"] == false {
                    return Err(invalid());
                }
            }
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(invalid)?;
            if items.len() < s["minItems"].as_u64().unwrap_or(0) as usize || items.len() > s["maxItems"].as_u64().unwrap_or(u64::MAX) as usize {
                return Err(invalid());
            }
            for item in items {
                check(&s["items"], item, path)?;
            }
        }
        Some("string") => {
            let len = value.as_str().ok_or_else(invalid)?.chars().count() as u64;
            if len < s["minLength"].as_u64().unwrap_or(0)
                || len > s["maxLength"].as_u64().unwrap_or(u64::MAX)
            {
                return Err(invalid());
            }
        }
        Some("number" | "integer") => {
            let n = value.as_f64().ok_or_else(invalid)?;
            if !n.is_finite()
                || n < s["minimum"].as_f64().unwrap_or(f64::NEG_INFINITY)
                || n > s["maximum"].as_f64().unwrap_or(f64::INFINITY)
                || (s["type"] == "integer" && (n.fract() != 0. || n.abs() > 9007199254740991.))
            {
                return Err(invalid());
            }
        }
        Some("boolean") => {
            if !value.is_boolean() {
                return Err(invalid());
            }
        }
        _ => {}
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn shared_contract_examples() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/workbench-v3.examples.json"
        ))
        .unwrap();
        for c in cases.as_array().unwrap() {
            let kind = match c["kind"].as_str() {
                Some("result") => "x-results",
                Some("event") => "x-events",
                _ => "x-methods",
            };
            assert_eq!(
                super::validate_kind(kind, c["method"].as_str().unwrap(), &c["params"]).is_ok(),
                c["valid"].as_bool().unwrap(),
                "{c}"
            );
        }
    }
}
