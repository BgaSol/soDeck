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
pub struct Appearance { pub base: Value, pub effective: Value, pub revision: u64, pub mode: String, pub selected_id:String, pub test_override: bool, pub preference_invalid: bool }

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
        let selected_id=base["id"].as_str().unwrap().to_owned();let mut value=Self{base,effective:Value::Null,revision:1,selected_id,mode,test_override,preference_invalid:false};value.effective=value.render();value
    }
    pub fn render(&self)->Value {
        let mut v=self.base.clone();let high=apply_system_preferences(&mut v);
        v["revision"]=json!(self.revision.to_string());v["highContrast"]=json!(high);v["testOverride"]=json!(self.test_override);v
    }
    pub fn chosen(&self,id:&str)->Result<Self,String>{
        let base=by_id(id)?;let mode=base["mode"].as_str().unwrap().to_owned();
        let mut next=Self::new(base,mode,false);
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
        _=>Err("外观偏好版本无效".into())
    }
}
pub fn load(data:&Path)->(Appearance,Option<String>){
    let mut id=default_id();let mut warning=None;let path=data.join("appearance-preferences.json");
    if path.exists(){match read_file(&path,16*1024).and_then(|bytes|Ok((preference(&bytes)?,bytes))) {
        Ok(((selected,migrate),bytes))=>{id=selected;if migrate{if let Err(e)=backup_legacy(data,&bytes).and_then(|_|save_id(data,&id)){warning=Some(format!("已保留原外观，但偏好迁移尚未保存：{e}"));}}},
        Err(e)=>warning=Some(format!("外观偏好无法读取，已使用多巴胺；重新选择外观可修复：{e}")),
    }}
    let mut base=by_id(&id).unwrap();let mode=base["mode"].as_str().unwrap().to_owned();let mut overridden=false;
    if let Some(path)=&crate::startup_config::debug().theme_file{
        match read_file(Path::new(&path),128*1024).and_then(|b|serde_json::from_slice(&b).map_err(|e|e.to_string())).and_then(|v|{validate(&v)?;Ok(v)}){
            Ok(v)=>{base=upgrade(v);overridden=true;},
            Err(e)=>warning=Some(format!("测试主题未加载，已使用所选完整主题：{e}")),
        }
    }
    let mut value=Appearance::new(base,mode,overridden);value.selected_id=id;value.preference_invalid=warning.is_some();(value,warning)
}
fn backup_legacy(data:&Path,bytes:&[u8])->Result<(),String>{
    let path=data.join("appearance-preferences.v1-backup.json");
    if path.exists(){if read_file(&path,16*1024)?==bytes{return Ok(());}return Err("旧偏好备份已存在且内容不同，原偏好未覆盖".into());}
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
    f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|e|e.to_string())
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
pub fn save_id(data:&Path,id:&str)->Result<(),String>{
    by_id(id)?;
    let pending=data.join("appearance-preferences.pending");let target=data.join("appearance-preferences.json");
    for path in [&pending,&target]{if let Ok(meta)=fs::symlink_metadata(path){if !meta.file_type().is_file(){return Err("外观偏好路径不是普通文件".into());}}}
    if target.exists(){let bytes=read_file(&target,16*1024)?;if preference(&bytes).is_ok_and(|(_,legacy)|legacy){backup_legacy(data,&bytes)?;}}
    let mut file=fs::File::create(&pending).map_err(|e|e.to_string())?;
    file.write_all(&serde_json::to_vec(&Preference{schema_version:2,theme_id:id.into()}).unwrap()).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    drop(file);crate::workbench::replace(&pending,&target)
}
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
    #[test]fn third_theme_default_and_both_legacy_modes_migrate_without_overwrite(){
        let dir=tempfile::tempdir().unwrap();let (fresh,warning)=load(dir.path());assert_eq!(fresh.selected_id,"dopamine");assert!(warning.is_none());
        for (mode,id) in [("light","soft-light"),("dark","soft-dark")]{let dir=tempfile::tempdir().unwrap();let old=serde_json::to_vec(&json!({"schemaVersion":1,"mode":mode})).unwrap();fs::write(dir.path().join("appearance-preferences.json"),&old).unwrap();let (theme,warning)=load(dir.path());assert!(warning.is_none());assert_eq!(theme.selected_id,id);assert_eq!(fs::read(dir.path().join("appearance-preferences.v1-backup.json")).unwrap(),old);let (again,warning)=load(dir.path());assert!(warning.is_none());assert_eq!(again.selected_id,id);save_id(dir.path(),"dopamine").unwrap();assert_eq!(load(dir.path()).0.selected_id,"dopamine");}
    }
    #[test]fn failed_migration_keeps_valid_legacy_selection_and_original_file(){let dir=tempfile::tempdir().unwrap();let old=br#"{"schemaVersion":1,"mode":"dark"}"#;fs::write(dir.path().join("appearance-preferences.json"),old).unwrap();fs::create_dir(dir.path().join("appearance-preferences.pending")).unwrap();let (theme,warning)=load(dir.path());assert_eq!(theme.selected_id,"soft-dark");assert!(warning.is_some());assert_eq!(fs::read(dir.path().join("appearance-preferences.json")).unwrap(),old);}
    #[test]fn both_palettes_and_legacy_are_strict(){for source in [DEFAULT,LIGHT,DARK,LEGACY]{let value:Value=serde_json::from_str(source).unwrap();assert!(validate(&value).is_ok());let mut bad=value.clone();bad["colors"]["accent"]=json!("red");assert!(validate(&bad).is_err());bad=value;bad["colors"]["unknown"]=json!("#ffffff");assert!(validate(&bad).is_err());}}
    #[test]fn legacy_projection_preserves_exact_fields(){let next=Appearance::new(builtin("light"),"light".into(),false);let mut projected=legacy(&next.effective);assert_eq!(projected["schemaVersion"],1);projected.as_object_mut().unwrap().remove("revision");assert!(validate(&projected).is_ok());assert_eq!(upgrade(projected)["schemaVersion"],2);}
    #[test]fn persistence_is_atomic_and_independent(){let dir=tempfile::tempdir().unwrap();save_id(dir.path(),"soft-light").unwrap();let before=fs::read(dir.path().join("appearance-preferences.json")).unwrap();fs::create_dir(dir.path().join("appearance-preferences.pending")).unwrap();assert!(save_id(dir.path(),"soft-dark").is_err());assert_eq!(before,fs::read(dir.path().join("appearance-preferences.json")).unwrap());assert!(save_id(dir.path(),"invalid").is_err());}
}
