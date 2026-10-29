//! Declarative settings: data only; shared by package verification and the host.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor { pub descriptor_version: u32, pub plugin_id: String, pub pages: Vec<Page> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page { pub id: String, pub title: String, pub order: u32, pub schema_version: u32, pub sections: Vec<Section> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section { pub id: String, pub title: String, pub fields: Vec<Field> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub key: String, pub label: String, #[serde(rename="type")] pub kind: String,
    pub default: Value, pub effect: String,
    #[serde(default, skip_serializing_if="Option::is_none")] pub description: Option<String>,
    #[serde(default, skip_serializing_if="Vec::is_empty")] pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub minimum: Option<i64>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub maximum: Option<i64>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub max_length: Option<usize>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub options: Option<Vec<Choice>>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub visible_when: Option<Condition>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub enabled_when: Option<Condition>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice { pub value: String, pub label: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition { pub key: String, pub equals: Value }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry { pub key: String, pub value: Value }

pub fn identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn text(s: &str, max: usize) -> bool { !s.trim().is_empty() && s.chars().count() <= max }
impl Field {
    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind.as_str() {
            "boolean" => value.is_boolean(),
            "integer" => value.as_i64().is_some_and(|n| n.unsigned_abs() <= 9007199254740991
                && self.minimum.is_none_or(|min| n >= min) && self.maximum.is_none_or(|max| n <= max)),
            "text" => value.as_str().is_some_and(|s| s.chars().count() <= self.max_length.unwrap_or(4096)),
            "enum" => self.options.as_ref().is_some_and(|opts| opts.iter().any(|o| value.as_str() == Some(&o.value))),
            _ => false,
        }
    }
    fn validate(&self) -> Result<(), String> {
        if !identifier(&self.key) || !text(&self.label,80) || !matches!(self.effect.as_str(),"immediate"|"newSession"|"restart")
            || self.description.as_ref().is_some_and(|s|s.chars().count()>512) || self.keywords.len()>16 || self.keywords.iter().any(|s|!text(s,40))
            || self.minimum.zip(self.maximum).is_some_and(|(a,b)|a>b)
            || (self.kind!="integer" && (self.minimum.is_some()||self.maximum.is_some()))
            || (self.kind!="text" && self.max_length.is_some()) || self.max_length.is_some_and(|n| !(1..=4096).contains(&n))
            || (self.kind!="enum" && self.options.is_some()) || !self.accepts(&self.default) {
            return Err(format!("Invalid field {}",self.key));
        }
        if let Some(opts)=&self.options {
            let mut seen=BTreeSet::new();
            if opts.is_empty() || opts.len()>64 || opts.iter().any(|o|!text(&o.value,80)||!text(&o.label,80)||!seen.insert(&o.value)) {return Err("Invalid enum choices".into());}
        }
        Ok(())
    }
}
impl Page {
    pub fn fields(&self) -> impl Iterator<Item=&Field> { self.sections.iter().flat_map(|s|&s.fields) }
    pub fn validate_changes(&self, entries: &[Entry]) -> Result<(),String> {
        let mut seen=BTreeSet::new();
        if entries.len()>64 { return Err("Too many settings".into()); }
        for e in entries {
            if !seen.insert(&e.key) || !self.fields().any(|f| f.key==e.key && f.accepts(&e.value)) {return Err(format!("Invalid setting {}",e.key));}
        } Ok(())
    }
    pub fn validate(&self) -> Result<(),String> {
        if serde_json::to_vec(self).unwrap().len()>48*1024{return Err("Settings page exceeds 48 KiB".into());}
        let defaults=self.fields().map(|f|serde_json::json!({"key":f.key,"value":f.default})).collect::<Vec<_>>();
        if serde_json::to_vec(&defaults).unwrap().len()>16*1024{return Err("Settings defaults exceed 16 KiB".into());}
        if !identifier(&self.id)||!text(&self.title,80)||self.order>100000||self.schema_version==0||self.schema_version>2147483647
            ||self.sections.is_empty()||self.sections.len()>8||self.fields().count()>64 {return Err("Invalid settings page".into());}
        let mut sections=BTreeSet::new();let mut fields=BTreeMap::new();
        for section in &self.sections {
            if !identifier(&section.id)||!text(&section.title,80)||!sections.insert(&section.id)||section.fields.is_empty(){return Err("Invalid section".into());}
            for f in &section.fields { f.validate()?;if fields.insert(f.key.as_str(),f).is_some(){return Err("Duplicate setting key".into());} }
        }
        fn visit<'a>(key:&'a str,fields:&BTreeMap<&'a str,&'a Field>,active:&mut BTreeSet<&'a str>,done:&mut BTreeSet<&'a str>)->Result<(),String>{
            if done.contains(key){return Ok(());} if !active.insert(key){return Err("Cyclic setting conditions".into());}
            let f=fields[key];
            for c in [&f.visible_when,&f.enabled_when].into_iter().flatten() {
                let target=fields.get(c.key.as_str()).ok_or("Unknown condition field")?;
                if !target.accepts(&c.equals){return Err("Invalid condition value".into());}
                visit(&c.key,fields,active,done)?;
            }
            active.remove(key);done.insert(key);Ok(())
        }
        let mut done=BTreeSet::new();
        for key in fields.keys(){visit(key,&fields,&mut BTreeSet::new(),&mut done)?;}
        Ok(())
    }
}
impl Descriptor {
    pub fn parse(bytes:&[u8],plugin:&str)->Result<Self,String>{
        if bytes.len()>128*1024{return Err("Settings descriptor exceeds 128 KiB".into());}
        let raw:Value=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;
        for field in raw["pages"].as_array().into_iter().flatten().flat_map(|p|p["sections"].as_array().into_iter().flatten()).flat_map(|s|s["fields"].as_array().into_iter().flatten()){
            if ["description","keywords","minimum","maximum","maxLength","options","visibleWhen","enabledWhen"].iter().any(|key|field.get(key).is_some_and(Value::is_null)){return Err("Optional settings properties cannot be null".into());}
        }
        let value:Self=serde_json::from_slice(bytes).map_err(|e|format!("Invalid settings descriptor: {e}"))?;
        if value.descriptor_version!=1||value.plugin_id!=plugin||plugin.is_empty()||plugin.len()>128||value.pages.is_empty()||value.pages.len()>4{return Err("Invalid settings identity".into());}
        let mut pages=BTreeSet::new();let mut keys=BTreeSet::new();
        for page in &value.pages {
            page.validate()?;
            if !pages.insert(&page.id)||page.fields().any(|f|!keys.insert(&f.key)){return Err("Overlapping settings groups".into());}
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sample()->Value { json!({"descriptorVersion":1,"pluginId":"sample","pages":[{"id":"general","title":"设置","order":1,"schemaVersion":1,"sections":[{"id":"display","title":"显示","fields":[{"key":"size","label":"字号","type":"integer","default":14,"minimum":10,"maximum":24,"effect":"immediate"}]}]}]}) }
    fn parse(v:&Value)->Result<Descriptor,String>{Descriptor::parse(&serde_json::to_vec(v).unwrap(),"sample")}
    #[test] fn validates_defaults_types_and_conditions(){
        assert!(parse(&sample()).is_ok());
        for invalid in [json!("14"),json!(25),json!(null),json!(true)]{let mut v=sample();v["pages"][0]["sections"][0]["fields"][0]["default"]=invalid;assert!(parse(&v).is_err());}
        let mut v=sample();v["pages"][0]["sections"][0]["fields"][0]["visibleWhen"]=json!({"key":"size","equals":14});assert!(parse(&v).is_err());
        v["pages"][0]["sections"][0]["fields"][0]["visibleWhen"]=json!({"key":"missing","equals":14});assert!(parse(&v).is_err());
    }
    #[test] fn rejects_cross_group_overlap_and_unknown_properties(){
        let mut v=sample();let mut page=v["pages"][0].clone();page["id"]=json!("second");v["pages"].as_array_mut().unwrap().push(page);assert!(parse(&v).is_err());
        let mut v=sample();v["script"]=json!("alert(1)");assert!(parse(&v).is_err());
    }
}
