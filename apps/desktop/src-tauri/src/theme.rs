//! Theme data and preference persistence; renderers never invent fallback colors.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};

pub const DEFAULT: &str = include_str!("../../../../themes/dopamine.json");
const DARK: &str = include_str!("../../../../themes/soft-dark.json");
const CATALOG: &str = include_str!("../../../../themes/catalog.json");
const LIGHT: &str = include_str!("../../../../themes/soft-light.json");
const LEGACY: &str = include_str!("../../../../themes/graphite.json");
const V1: &str = include_str!("../../../../contracts/presentation-v1.schema.json");
const V2: &str = include_str!("../../../../contracts/presentation-v2.schema.json");
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preference { schema_version: u32, theme_id: String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct LegacyPreference { schema_version:u32, mode:String }
pub struct Appearance { pub config_revision:u64, pub storage_generation:String, pub read_only:bool, pub base: Value, pub effective: Value, pub revision: u64, pub mode: String, pub selected_id:String, pub test_override: bool, pub preference_invalid: bool }

pub fn validate(value: &Value) -> Result<(), String> {
    let source = match value["schemaVersion"].as_u64() { Some(1)=>V1,Some(2)=>V2,_=>return Err("主题版本无效".into()) };
    let schema: Value=serde_json::from_str(source).unwrap();
    sodeck_core::contract::check(&schema,&schema["$defs"]["Theme"],value,"theme")
}
pub fn catalog()->Value {serde_json::from_str(CATALOG).expect("validated built-in theme catalog")}
pub fn default_id()->String {catalog()["defaultThemeId"].as_str().unwrap().into()}
pub fn by_id(id:&str)->Result<Value,String>{
    let entries=catalog();let entry=entries["themes"].as_array().unwrap().iter().find(|v|v["id"]==id).ok_or("未知主题")?;
    let source=match entry["file"].as_str(){Some("soft-light.json")=>LIGHT,Some("soft-dark.json")=>DARK,Some("dopamine.json")=>DEFAULT,_=>return Err("主题资源缺失".into())};
    let value:Value=serde_json::from_str(source).map_err(|e|e.to_string())?;validate(&value)?;
    if value["id"]!=entry["id"]||value["mode"]!=entry["mode"]{return Err("主题目录与资源不一致".into());}Ok(value)
}
pub fn builtin(mode:&str)->Value {by_id(if mode=="light"{"soft-light"}else{"soft-dark"}).unwrap()}
/// Legacy decoration fallbacks are defined here, never in a component.
pub fn upgrade(value: Value) -> Value {
    if value["schemaVersion"]==2 {return value;}
    let mut result=builtin(value["mode"].as_str().unwrap_or("dark"));
    result["id"]=value["id"].clone();result["terminal"]=value["terminal"].clone();
    for (key,color) in value["colors"].as_object().unwrap(){result["colors"][key]=color.clone();} result
}
pub fn legacy(value: &Value) -> Value {
    let mut result:Value=serde_json::from_str(LEGACY).unwrap();
    result["id"]=value["id"].clone();result["mode"]=value["mode"].clone();result["terminal"]=value["terminal"].clone();
    for (key,color) in result["colors"].as_object_mut().unwrap(){*color=value["colors"][key].clone();}
    result["revision"]=json!(value["revision"].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(1).min(9_007_199_254_740_991));result
}
impl Appearance {
    pub fn new(base:Value, mode:String, test_override:bool)->Self {
        let selected_id=base["id"].as_str().unwrap().to_owned();let mut value=Self{config_revision:0,storage_generation:uuid::Uuid::new_v4().to_string(),read_only:false,base,effective:Value::Null,revision:1,selected_id,mode,test_override,preference_invalid:false};value.effective=value.render();value
    }
    pub fn render(&self)->Value {
        let mut v=self.base.clone();let high=apply_system_preferences(&mut v);
        v["revision"]=json!(self.revision.to_string());v["highContrast"]=json!(high);v["testOverride"]=json!(self.test_override);v
    }
    pub fn chosen(&self,id:&str)->Result<Self,String>{
        let base=by_id(id)?;let mode=base["mode"].as_str().unwrap().to_owned();
        let mut next=Self::new(base,mode,false);
        next.config_revision=self.config_revision.checked_add(1).ok_or("外观配置修订溢出")?;next.storage_generation=self.storage_generation.clone();
        next.revision=self.revision.checked_add(1).ok_or("主题修订已超出范围")?;next.effective=next.render();Ok(next)
    }

}
fn read_file(path:&Path, limit:u64)->Result<Vec<u8>,String>{
    let meta=fs::symlink_metadata(path).map_err(|e|e.to_string())?;
    if !meta.file_type().is_file() || meta.len()>limit{return Err("外观文件不是普通文件或超出大小限制".into());}
    fs::read(path).map_err(|e|e.to_string())
}
fn preference(bytes:&[u8])->Result<(String,bool),String>{
    let version:Value=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;
    match version["schemaVersion"].as_u64(){
        Some(1)=>{let p:LegacyPreference=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;if p.schema_version!=1||!matches!(p.mode.as_str(),"light"|"dark"){return Err("外观模式无效".into());}Ok((if p.mode=="light"{"soft-light"}else{"soft-dark"}.into(),true))},
        Some(2)=>{let p:Preference=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;by_id(&p.theme_id)?;Ok((p.theme_id,false))},
        Some(3)=>{let p:PreferenceV3=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;by_id(&p.theme_id)?;Ok((p.theme_id,false))},
        _=>Err("外观偏好版本无效".into())
    }
}
pub fn load(data:&Path)->(Appearance,Option<String>){
    let mut id=default_id();let mut warning=None;let path=data.join("appearance-preferences.json");
    if path.exists(){match read_file(&path,128*1024).and_then(|bytes|Ok((preference(&bytes)?,bytes))) {
        Ok(((selected,_),_))=>{id=selected;},
        Err(e)=>warning=Some(format!("外观偏好无法读取，已使用多巴胺；重新选择外观可修复：{e}")),
    }}
    let mut base=by_id(&id).unwrap();let mode=base["mode"].as_str().unwrap().to_owned();let mut overridden=false;
    if let Some(path)=&crate::startup_config::debug().theme_file{
        match read_file(Path::new(&path),128*1024).and_then(|b|serde_json::from_slice(&b).map_err(|e|e.to_string())).and_then(|v|{validate(&v)?;Ok(v)}){
            Ok(v)=>{base=upgrade(v);overridden=true;},
            Err(e)=>warning=Some(format!("测试主题未加载，已使用所选完整主题：{e}")),
        }
    }
    let mut value=Appearance::new(base,mode,overridden);value.selected_id=id;value.preference_invalid=warning.is_some();
    if let Ok(bytes)=read_file(&path,128*1024) {if let Ok(v)=serde_json::from_slice::<Value>(&bytes) {
        value.read_only=v["schemaVersion"].as_u64().is_some_and(|n|n>3);
        if let Ok(p)=serde_json::from_value::<PreferenceV3>(v){if p.schema_version==3{value.config_revision=p.settings_revision;value.storage_generation=p.storage_generation;}}
    }}(value,warning)
}
/// Initial native backdrop and DOM palette share the same validated source.
pub fn background(value:&Value)->(u8,u8,u8,u8){
    let color=value["colors"]["canvas"].as_str().unwrap();
    (u8::from_str_radix(&color[1..3],16).unwrap(),u8::from_str_radix(&color[3..5],16).unwrap(),u8::from_str_radix(&color[5..7],16).unwrap(),255)
}
pub fn bootstrap(value:&Value)->String{bootstrap_surface(value,false)}
pub fn bootstrap_surface(value:&Value,transparent:bool)->String{
    format!(r#"(() => {{const theme={value};const apply=()=>{{const root=document.documentElement;if(!root)return false;root.style.colorScheme=theme.mode;root.dataset.theme=theme.id;root.dataset.themeRevision=String(theme.revision);root.dataset.highContrast=String(Boolean(theme.highContrast));for(const [k,v] of Object.entries(theme.colors))root.style.setProperty('--'+k,v);root.style.setProperty('--terminal-background',theme.terminal.background);root.style.background={transparent}?theme.colors.transparent:theme.colors.canvas;return true;}};if(!apply()){{const observer=new MutationObserver(()=>{{if(apply())observer.disconnect();}});observer.observe(document,{{childList:true,subtree:true}});}}}})();"#)
}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct PreferenceV3 {schema_version:u32,theme_id:String,settings_revision:u64,storage_generation:String,receipts:Vec<Value>}
fn backup(data:&Path,bytes:&[u8])->Result<(),String>{
    let path=data.join("appearance-preferences.pre-v3-backup.json");
    if path.exists(){if read_file(&path,128*1024)?==bytes{return Ok(());}return Err("外观备份已存在且内容不同，原文件未覆盖".into());}
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
    f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|e|e.to_string())
}
pub fn operation(data:&Path,id:&str)->Result<Option<Value>,String>{
    let path=data.join("appearance-preferences.json");if !path.exists(){return Ok(None);}
    let bytes=read_file(&path,128*1024)?;
    let Ok(v)=serde_json::from_slice::<Value>(&bytes)else{return Ok(None)};
    if v["schemaVersion"]!=3{return Ok(None);}
    let p:PreferenceV3=serde_json::from_value(v).map_err(|e|e.to_string())?;Ok(p.receipts.into_iter().find(|r|r["operationId"]==id))
}
pub fn save_selection(data:&Path,current:&Appearance,id:&str,operation:&str,digest:&str)->Result<Value,String>{
    if current.read_only{return Err("外观偏好版本较新，本版只读".into());}by_id(id)?;
    let target=data.join("appearance-preferences.json");let mut receipts=vec![];
    if target.exists(){let bytes=read_file(&target,128*1024)?;let version=serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|v["schemaVersion"].as_u64());
        if version.is_some_and(|v|v>3){return Err("外观偏好版本较新，本版只读".into());}
        if version==Some(3){let previous:PreferenceV3=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
            if previous.settings_revision!=current.config_revision||previous.storage_generation!=current.storage_generation{return Err("外观配置修订冲突".into());}receipts=previous.receipts;
        }else{backup(data,&bytes)?;}
    }
    let revision=current.config_revision.checked_add(1).ok_or("外观修订溢出")?;
    let result=json!({"operationId":operation,"persistedRevision":revision.to_string(),"persistence":"saved","synchronization":"pending"});
    receipts.push(json!({"operationId":operation,"digest":digest,"result":result}));if receipts.len()>64{receipts.drain(..receipts.len()-64);}
    let value=PreferenceV3{schema_version:3,theme_id:id.into(),settings_revision:revision,storage_generation:current.storage_generation.clone(),receipts};
    crate::atomic_file::write(&target,&serde_json::to_vec(&value).map_err(|e|e.to_string())?)?;Ok(result)
}
pub fn save_id(data:&Path,id:&str)->Result<(),String>{let (current,_)=load(data);save_selection(data,&current,id,&uuid::Uuid::new_v4().to_string(),"legacy").map(|_|())}
#[cfg(not(windows))] fn apply_system_preferences(_: &mut Value)->bool{false}
#[cfg(windows)] fn apply_system_preferences(value:&mut Value)->bool{
    use windows::Win32::{UI::{Accessibility::*,WindowsAndMessaging::*},Graphics::Gdi::*};
    let mut high=HIGHCONTRASTW{cbSize:std::mem::size_of::<HIGHCONTRASTW>() as u32,..Default::default()};
    if unsafe{SystemParametersInfoW(SPI_GETHIGHCONTRAST,high.cbSize,Some((&mut high as *mut HIGHCONTRASTW).cast()),SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))}.is_err() || high.dwFlags.0&HCF_HIGHCONTRASTON.0==0{return false;}
    fn color(index:SYS_COLOR_INDEX)->String{let c=unsafe{GetSysColor(index)};format!("#{:02x}{:02x}{:02x}",c&255,(c>>8)&255,(c>>16)&255)}
    let bg=color(COLOR_WINDOW);let fg=color(COLOR_WINDOWTEXT);let selected=color(COLOR_HIGHLIGHT);let selected_text=color(COLOR_HIGHLIGHTTEXT);
    for key in ["canvas","surface","elevated","field","glass"]{value["colors"][key]=json!(bg);}
    for key in ["text","secondary","muted","border","controlBorder","glassBorder","success","warning","error","scrollbar"]{value["colors"][key]=json!(fg);}
    for key in ["accent","focus","hover","pressed","selected","dangerHover","dragBackground","selection","primary","primaryHover","primaryPressed"]{value["colors"][key]=json!(selected);}
    for key in ["selectedText","dangerText","primaryText"]{value["colors"][key]=json!(selected_text);}
    value["colors"]["disabled"]=json!(color(COLOR_GRAYTEXT));
    for key in ["foreground","cursor","scrollbarSliderBackground","scrollbarSliderHoverBackground","scrollbarSliderActiveBackground","overviewRulerBorder"]{value["terminal"][key]=json!(fg);}
    for key in ["background","cursorAccent"]{value["terminal"][key]=json!(bg);}
    for key in ["selectionBackground","selectionInactiveBackground"]{value["terminal"][key]=json!(selected);}
    value["terminal"]["selectionForeground"]=json!(selected_text);true
}
#[cfg(test)] mod tests{
    use super::*;
    #[test]fn legacy_reads_are_pure_and_first_write_keeps_exact_backup(){
        for old in [br#"{"schemaVersion":1,"mode":"dark"}"#.as_slice(),br#"{"schemaVersion":2,"themeId":"soft-dark"}"#.as_slice()] {
            let dir=tempfile::tempdir().unwrap();let file=dir.path().join("appearance-preferences.json");fs::write(&file,old).unwrap();
            let (theme,warning)=load(dir.path());assert!(warning.is_none());assert_eq!(theme.selected_id,"soft-dark");assert_eq!(theme.config_revision,0);assert_eq!(fs::read(&file).unwrap(),old);assert!(!dir.path().join("appearance-preferences.pre-v3-backup.json").exists());
            save_selection(dir.path(),&theme,"dopamine","operation","digest").unwrap();assert_eq!(fs::read(dir.path().join("appearance-preferences.pre-v3-backup.json")).unwrap(),old);
            let (next,warning)=load(dir.path());assert!(warning.is_none());assert_eq!(next.selected_id,"dopamine");assert_eq!(next.config_revision,1);assert_eq!(operation(dir.path(),"operation").unwrap().unwrap()["digest"],"digest");
        }
    }
    #[test]fn backup_conflict_and_future_format_are_read_only(){
        let dir=tempfile::tempdir().unwrap();let file=dir.path().join("appearance-preferences.json");let old=br#"{"schemaVersion":1,"mode":"dark"}"#;fs::write(&file,old).unwrap();fs::write(dir.path().join("appearance-preferences.pre-v3-backup.json"),b"different").unwrap();assert!(save_id(dir.path(),"dopamine").is_err());assert_eq!(fs::read(&file).unwrap(),old);
        let future=br#"{"schemaVersion":4,"themeId":"soft-light"}"#;fs::write(&file,future).unwrap();assert!(load(dir.path()).0.read_only);assert!(save_id(dir.path(),"dopamine").is_err());assert_eq!(fs::read(&file).unwrap(),future);
    }
    #[test]fn both_palettes_and_legacy_are_strict(){for source in [DEFAULT,LIGHT,DARK,LEGACY]{let value:Value=serde_json::from_str(source).unwrap();assert!(validate(&value).is_ok());let mut bad=value.clone();bad["colors"]["accent"]=json!("red");assert!(validate(&bad).is_err());bad=value;bad["colors"]["unknown"]=json!("#ffffff");assert!(validate(&bad).is_err());}}
    #[test]fn legacy_projection_preserves_exact_fields(){let next=Appearance::new(builtin("light"),"light".into(),false);let mut projected=legacy(&next.effective);assert_eq!(projected["schemaVersion"],1);projected.as_object_mut().unwrap().remove("revision");assert!(validate(&projected).is_ok());assert_eq!(upgrade(projected)["schemaVersion"],2);}
    #[test]fn selection_revision_is_independent_of_system_render_revision(){let dir=tempfile::tempdir().unwrap();let (mut theme,_)=load(dir.path());theme.revision+=100;assert_eq!(theme.config_revision,0);let next=theme.chosen("soft-dark").unwrap();assert_eq!(next.config_revision,1);assert_eq!(next.revision,102);}

}
