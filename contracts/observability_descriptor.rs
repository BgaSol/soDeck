//! Shared package boundary: deliberately no executable schemas or free text payloads.
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Descriptor {pub schema_version:u32,pub plugin_id:String,pub events:Vec<Definition>,pub actions:Vec<Definition>,pub snapshots:Vec<Definition>,pub metrics:Vec<Definition>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Definition {
    #[serde(rename="type")]pub name:String,pub type_version:u32,pub fields:BTreeMap<String,Field>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub kind:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub level:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub message_key:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub unit:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub metric_kind:Option<String>,
    #[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub labels:BTreeMap<String,Vec<String>>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]pub buckets:Vec<f64>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub stale_after_ms:Option<u64>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Field {
    #[serde(rename="type")]pub kind:String,pub classification:String,
    #[serde(default)]pub required:bool,
    #[serde(default,skip_serializing_if="Option::is_none")]pub max_length:Option<usize>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub minimum:Option<f64>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub maximum:Option<f64>,
    #[serde(rename="enum",default,skip_serializing_if="Vec::is_empty")]pub values:Vec<Value>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub items:Option<Box<Field>>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub max_items:Option<usize>,
    #[serde(default,skip_serializing_if="BTreeMap::is_empty")]pub properties:BTreeMap<String,Field>,
}
pub fn identifier(s:&str)->bool {!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b))}
pub const LEVELS:[&str;6]=["trace","debug","info","warn","error","fatal"];
impl Descriptor {
    pub fn parse(bytes:&[u8],plugin:&str)->Result<Self,String>{
        if bytes.len()>1024*1024{return Err("Descriptor exceeds 1 MiB".into());}
        let value:Self=serde_json::from_slice(bytes).map_err(|_|"Invalid observation descriptor")?;
        value.validate(plugin)?;Ok(value)
    }
    pub fn definitions(&self)->impl Iterator<Item=(&'static str,&Definition)> {[("events",&self.events),("actions",&self.actions),("snapshots",&self.snapshots),("metrics",&self.metrics)].into_iter().flat_map(|(k,v)|v.iter().map(move|d|(k,d)))}
    pub fn validate(&self,plugin:&str)->Result<(),String>{
        if self.schema_version!=1||self.plugin_id!=plugin||!identifier(plugin)||self.definitions().count()>128||self.metrics.len()>32{return Err("Invalid descriptor identity or capacity".into());}
        let mut keys=BTreeSet::new();
        for(category,d)in self.definitions(){
            if !identifier(&d.name)||d.type_version==0||d.type_version>65535||!keys.insert((category,&d.name,d.type_version))||serde_json::to_vec(d).map_err(|_|"Invalid definition")?.len()>8192{return Err("Invalid or duplicate observation type".into());}
            if matches!(category,"events"|"actions")&&!d.name.starts_with(&format!("plugin.{plugin}.")){return Err("Observation type has foreign namespace".into());}
            if d.kind.as_ref().is_some_and(|k|!matches!(k.as_str(),"action"|"domain"|"log"|"lifecycle"|"health"))||d.level.as_ref().is_some_and(|l|!LEVELS.contains(&l.as_str()))||d.message_key.as_ref().is_some_and(|s|!identifier(s)){return Err("Invalid kind, level or message key".into());}
            if category=="actions"&&d.kind.as_deref().is_some_and(|k|k!="action"){return Err("Action kind mismatch".into());}
            validate_fields(&d.fields,0)?;
            if d.stale_after_ms.is_some_and(|v|!(5000..=3600000).contains(&v)){return Err("Invalid snapshot lifetime".into());}
            if category=="metrics" {
                if !d.unit.as_deref().is_some_and(identifier)||!matches!(d.metric_kind.as_deref(),Some("counter"|"gauge"|"histogram"))||d.labels.len()>4||d.buckets.len()>32||d.buckets.iter().any(|v|!v.is_finite())||d.buckets.windows(2).any(|v|v[0]>=v[1]) {return Err("Invalid metric definition".into());}
                for(key,values)in &d.labels{if !identifier(key)||values.is_empty()||values.len()>32||values.iter().any(|v|!identifier(v)){return Err("Invalid metric labels".into());}}
            }
        }Ok(())
    }
}
fn validate_fields(fields:&BTreeMap<String,Field>,depth:usize)->Result<(),String>{
    if fields.len()>32||depth>3{return Err("Fields exceed structural limits".into());}
    for(key,field)in fields{if !identifier(key){return Err("Invalid field name".into());}field.validate(depth)?;}Ok(())
}
impl Field {
    fn validate(&self,depth:usize)->Result<(),String>{
        if depth>3||!matches!(self.classification.as_str(),"safe-id"|"enum"|"count"|"duration"|"state"|"structure")||self.values.len()>32{return Err("Invalid field classification".into());}
        if self.minimum.is_some_and(|v|!v.is_finite())||self.maximum.is_some_and(|v|!v.is_finite())||self.minimum.zip(self.maximum).is_some_and(|(a,b)|a>b){return Err("Invalid numeric bounds".into());}
        match self.kind.as_str(){
            "string"=>{if !self.max_length.is_some_and(|n|(1..=256).contains(&n))||!matches!(self.classification.as_str(),"safe-id"|"enum"|"state")||(self.classification!="safe-id"&&self.values.is_empty())||self.values.iter().any(|v|v.as_str().is_none_or(|s|s.len()>256)){return Err("Strings require bounded IDs or static enums".into());}},
            "integer"|"number"|"boolean"=>{},
            "array"=>{if !self.max_items.is_some_and(|n|(1..=32).contains(&n)){return Err("Array needs a bound".into());}self.items.as_ref().ok_or("Array needs items")?.validate(depth+1)?;},
            "object"=>validate_fields(&self.properties,depth+1)?,
            _=>return Err("Unsupported descriptor field".into()),
        }Ok(())
    }
    fn check(&self,value:&Value)->bool{
        if !self.values.is_empty()&&!self.values.contains(value){return false;}
        match self.kind.as_str(){
            "string"=>value.as_str().is_some_and(|s|s.chars().count()<=self.max_length.unwrap_or(0)&&if self.classification=="safe-id" {identifier(s)}else{true}),
            "boolean"=>value.is_boolean(),
            "integer"|"number"=>value.as_f64().is_some_and(|v|v.is_finite()&&(self.kind!="integer"||(v.fract()==0.&&v.abs()<=9007199254740991.))&&self.minimum.is_none_or(|n|v>=n)&&self.maximum.is_none_or(|n|v<=n)),
            "array"=>value.as_array().is_some_and(|a|a.len()<=self.max_items.unwrap_or(0)&&a.iter().all(|v|self.items.as_ref().is_some_and(|f|f.check(v)))),
            "object"=>check_fields(&self.properties,value).is_ok(),_=>false,
        }
    }
}
pub fn check_fields(fields:&BTreeMap<String,Field>,value:&Value)->Result<(),String>{
    let values=value.as_object().ok_or("Fields must be an object")?;
    if values.len()>32||fields.iter().any(|(k,f)|f.required&&!values.contains_key(k))||values.iter().any(|(k,v)|fields.get(k).is_none_or(|f|!f.check(v))){return Err("Unspecified or unsafe observation field".into());}Ok(())
}
