use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const PAGE_BYTES: usize = 24 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub order: u32,
    pub view_types: Vec<String>,
    pub groups: Vec<Group>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub fields: Vec<Field>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub default: Value,
    pub apply: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
    pub max_length: Option<usize>,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default)]
    pub presets: Vec<Font>,
    pub visible_when: Option<Condition>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Choice {
    pub value: Value,
    pub title: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Font {
    pub id: String,
    pub title: String,
    pub regular: String,
    pub bold: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Condition {
    pub key: String,
    pub equals: Value,
}

pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn title(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256
}
fn asset(s: &str) -> bool {
    !s.is_empty()
        && !s.contains(['\\', ':', '%'])
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
        && s.ends_with(".woff2")
}
impl Field {
    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind.as_str() {
            "boolean" => value.is_boolean(),
            "number" => value.as_f64().is_some_and(|n| {
                let min = self.min.unwrap_or(0.0);
                let max = self.max.unwrap_or(0.0);
                let step = self.step.unwrap_or(1.0);
                let steps = (n - min) / step;
                n.is_finite() && n >= min && n <= max && (steps - steps.round()).abs() <= 1e-7
            }),
            "string" => value
                .as_str()
                .is_some_and(|s| s.len() <= self.max_length.unwrap_or(1024)),
            "select" => self.options.iter().any(|c| c.value == *value),
            "font" => value.as_object().is_some_and(|o| {
                if o.len() != 2 {
                    return false;
                }
                match value["source"].as_str() {
                    Some("preset") => value["id"]
                        .as_str()
                        .is_some_and(|id| self.presets.iter().any(|p| p.id == id)),
                    Some("local") => value["family"].as_str().is_some_and(|s| {
                        !s.trim().is_empty()
                            && s.chars().count() <= 128
                            && s.len() <= 512
                            && !s.chars().any(char::is_control)
                    }),
                    _ => false,
                }
            }),
            _ => false,
        }
    }
}
impl Page {
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.groups.iter().flat_map(|g| &g.fields)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !identifier(&self.id)
            || !title(&self.title)
            || self.icon.len() > 80
            || self.groups.is_empty()
            || self.groups.len() > 8
            || self.view_types.is_empty()
            || self.view_types.len() > 64
            || self.view_types.iter().any(|v| !identifier(v))
        {
            return Err("设置页面声明无效".into());
        }
        let mut groups = HashSet::new();
        let mut keys = HashSet::new();
        for g in &self.groups {
            if !identifier(&g.id)
                || !groups.insert(&g.id)
                || !title(&g.title)
                || g.fields.is_empty()
            {
                return Err("设置分组无效或重复".into());
            }
            for f in &g.fields {
                if !identifier(&f.key)
                    || !keys.insert(&f.key)
                    || !title(&f.title)
                    || f.description.len() > 1024
                    || !["live", "newInstance"].contains(&f.apply.as_str())
                    || !["boolean", "number", "string", "select", "font"].contains(&f.kind.as_str())
                {
                    return Err(format!("字段 {} 的类型或声明不受支持", f.key));
                }
                if f.kind == "number"
                    && !(matches!((f.min, f.max, f.step), (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() && a <= b && c > 0.0))
                {
                    return Err(format!("字段 {} 数字范围无效", f.key));
                }
                if f.max_length.is_some_and(|n| n > 1024)
                    || f.unit.as_ref().is_some_and(|s| s.len() > 32)
                {
                    return Err("字段长度限制无效".into());
                }
                if f.kind == "select" {
                    let mut values = HashSet::new();
                    if f.options.is_empty()
                        || f.options.len() > 32
                        || f.options.iter().any(|c| {
                            !(c.value.is_string() || c.value.is_boolean() || c.value.is_number())
                                || !title(&c.title)
                                || !values.insert(c.value.to_string())
                        })
                    {
                        return Err("设置选项无效或重复".into());
                    }
                }
                if f.kind == "font" {
                    let mut ids = HashSet::new();
                    if f.presets.is_empty()
                        || f.presets.len() > 32
                        || f.presets.iter().any(|p| {
                            !identifier(&p.id)
                                || !ids.insert(&p.id)
                                || !title(&p.title)
                                || !asset(&p.regular)
                                || !asset(&p.bold)
                        })
                    {
                        return Err("预置字体声明无效".into());
                    }
                }
                if !f.accepts(&f.default) {
                    return Err(format!("字段 {} 默认值无效", f.key));
                }
            }
        }
        if keys.len() > 64 {
            return Err("设置页超过 64 个字段".into());
        }
        for f in self.fields() {
            if let Some(c) = &f.visible_when {
                if c.key == f.key
                    || !self.fields().any(|other| {
                        other.key == c.key
                            && ["boolean", "select"].contains(&other.kind.as_str())
                            && other.visible_when.is_none()
                            && other.accepts(&c.equals)
                    })
                {
                    return Err("字段显示条件无效或循环".into());
                }
            }
        }
        self.validate_values(&self.defaults())?;
        Ok(())
    }
    pub fn defaults(&self) -> Map<String, Value> {
        self.fields()
            .map(|f| (f.key.clone(), f.default.clone()))
            .collect()
    }
    pub fn structure(&self) -> Value {
        Value::Object(
            self.fields()
                .map(|f| {
                    (
                        f.key.clone(),
                        serde_json::json!({"type":f.kind,"unit":f.unit}),
                    )
                })
                .collect(),
        )
    }
    pub fn validate_values(&self, values: &Map<String, Value>) -> Result<(), String> {
        if serde_json::to_vec(values).map_err(|e| e.to_string())?.len() > PAGE_BYTES {
            return Err("设置值超过 24 KiB".into());
        }
        if values.len() != self.fields().count() {
            return Err("设置字段不完整或包含未知字段".into());
        }
        for f in self.fields() {
            if !values.get(&f.key).is_some_and(|v| f.accepts(v)) {
                return Err(format!("{}：值不符合字段约束", f.title));
            }
        }
        Ok(())
    }
    pub fn effective(&self, saved: &Map<String, Value>) -> (Map<String, Value>, Vec<String>) {
        let mut warnings = Vec::new();
        let values = self
            .fields()
            .map(|f| {
                let value = match saved.get(&f.key) {
                    Some(v) if f.accepts(v) => v.clone(),
                    Some(_) => {
                        warnings.push(format!("{}：旧值已保留，当前使用默认值", f.title));
                        f.default.clone()
                    }
                    None => f.default.clone(),
                };
                (f.key.clone(), value)
            })
            .collect();
        (values, warnings)
    }
    pub fn token(&self, plugin: &str, version: &str, schema: u64) -> String {
        let bytes =
            serde_json::to_vec(&(plugin, version, schema, self)).expect("serializable page");
        format!("{:x}", Sha256::digest(bytes))
    }
}
