use serde_json::Value;
pub fn check(schema: &Value, s: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(reference) = s["$ref"].as_str() {
        return check(
            schema,
            &schema["$defs"][reference.rsplit('/').next().unwrap()],
            value,
            path,
        );
    }
    if let Some(parts) = s["allOf"].as_array() {
        for part in parts {
            check(schema, part, value, path)?;
        }
    }
    if let Some(condition) = s.get("if") {
        if check(schema, condition, value, path).is_ok() {
            if let Some(then) = s.get("then") {
                check(schema, then, value, path)?;
            }
        }
    }
    let invalid = || format!("{path} 不符合工作台契约");
    if s["anyOf"].as_array().is_some_and(|parts| {
        !parts
            .iter()
            .any(|part| check(schema, part, value, path).is_ok())
    }) {
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
                    check(schema, field, item, &format!("{path}.{key}"))?;
                } else if s["additionalProperties"] == false {
                    return Err(invalid());
                }
            }
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(invalid)?;
            if items.len() < s["minItems"].as_u64().unwrap_or(0) as usize
                || items.len() > s["maxItems"].as_u64().unwrap_or(u64::MAX) as usize
            {
                return Err(invalid());
            }
            for item in items {
                check(schema, &s["items"], item, path)?;
            }
        }
        Some("string") => {
            if s["format"]=="uint64" && value.as_str().is_none_or(|v|v.parse::<u64>().is_err()) {return Err(invalid());}
            if let Some(pattern) = s["pattern"].as_str() {
                if !regex::Regex::new(pattern)
                    .map_err(|_| invalid())?
                    .is_match(value.as_str().ok_or_else(invalid)?)
                {
                    return Err(invalid());
                }
            }
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
