//! Resolve startup inputs once. Installed/portable runs never fall back to source files.
use serde::Deserialize;
use std::{collections::BTreeMap,fs,io,path::{Component,Path,PathBuf}};

#[derive(Clone,Default)]
pub struct DebugConfig {
    pub shell_port:Option<u16>,
    pub webview_args:Option<String>,
    pub plugin_ports:BTreeMap<String,u16>,
    pub theme_file:Option<PathBuf>,pub brand_file:Option<PathBuf>,pub fail_application_menu:bool,
}
static DEBUG:std::sync::OnceLock<DebugConfig>=std::sync::OnceLock::new();
pub fn debug()->&'static DebugConfig{DEBUG.get_or_init(DebugConfig::default)}
pub struct Bundle { pub plugin_id:Option<String>,pub path:PathBuf }
pub struct Resources { pub keys:PathBuf,pub packages:Vec<Bundle>,pub warnings:Vec<String> }
pub struct StartupConfig { pub data:PathBuf,pub resources:Result<Resources,String>,pub debug:DebugConfig }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Portable { schema_version:u32 }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Index { schema_version:u32,public_keys:String,plugins:Vec<IndexedPlugin> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct IndexedPlugin {plugin_id:String,path:String}

fn json<T:serde::de::DeserializeOwned>(path:&Path)->Result<T,String>{
    let file=fs::File::open(path).map_err(|e|format!("无法读取 {}：{e}",path.display()))?;
    if file.metadata().map_err(|e|e.to_string())?.len()>65536{return Err(format!("配置文件过大：{}",path.display()));}
    serde_json::from_reader(file).map_err(|e|format!("配置无效 {}：{e}",path.display()))
}
fn resource(root:&Path,name:&str)->Result<PathBuf,String>{
    let path=Path::new(name);
    if path.components().any(|c|!matches!(c,Component::Normal(_)))||path.as_os_str().is_empty(){return Err("资源索引路径必须位于资源目录内".into());}
    let resolved=fs::canonicalize(root.join(path)).map_err(|e|format!("资源缺失 {name}：{e}"))?;
    if !resolved.starts_with(root)||!resolved.is_file(){return Err(format!("资源路径越界或不是文件：{name}"));}
    Ok(resolved)
}
fn indexed_resources(exe_dir:&Path,framework:&Path)->Result<Resources,String>{
    let beside=exe_dir.join("resources");
    let root=if beside.exists(){beside}else if framework.join("resources").exists(){framework.join("resources")}else{framework.to_path_buf()};
    let root=fs::canonicalize(&root).map_err(|e|format!("资源目录不可用 {}：{e}",root.display()))?;
    let index:Index=json(&root.join("bundled-plugins.json"))?;
    if index.schema_version!=1{return Err("随附插件索引版本不支持".into());}
    let mut expected=std::collections::BTreeSet::from(["sodeck.terminal","sodeck.files","sodeck.network-proxy"]);
    let keys=resource(&root,&index.public_keys)?;
    let mut packages=Vec::new();
    let mut warnings=Vec::new();
    for entry in index.plugins {
        if !expected.remove(entry.plugin_id.as_str()){return Err("随附插件索引包含未知或重复项".into());}
        match resource(&root,&entry.path) {
            Ok(path)=>packages.push(Bundle{plugin_id:Some(entry.plugin_id),path}),
            Err(error)=>warnings.push(error),
        }
    }
    if !expected.is_empty(){return Err(format!("随附插件索引缺少：{}",expected.into_iter().collect::<Vec<_>>().join("、")));}
    Ok(Resources{keys,packages,warnings})
}
impl StartupConfig {
    pub fn resolve(exe:&Path,system_data:&Path,framework:&Path)->io::Result<Self>{
        let vars=std::env::vars_os().filter_map(|(k,v)|k.into_string().ok().map(|k|(k,v))).collect();
        let config=Self::from_inputs(exe,system_data,framework,cfg!(debug_assertions),&vars)?;
        let _=DEBUG.set(config.debug.clone());Ok(config)
    }
    fn from_inputs(exe:&Path,system_data:&Path,framework:&Path,debug_build:bool,vars:&BTreeMap<String,std::ffi::OsString>)->io::Result<Self>{
        let dir=exe.parent().ok_or_else(||io::Error::other("程序路径无父目录"))?;
        let developer=debug_build&&vars.get("SODECK_DEV_MODE").is_some_and(|v|v=="1");
        let portable=dir.join("portable.json");
        let mut data=if portable.exists(){
            let marker:Portable=json(&portable).map_err(io::Error::other)?;
            if marker.schema_version!=1{return Err(io::Error::other("portable.json 版本不支持"));}
            dir.join("data")
        }else{system_data.to_path_buf()};
        let mut debug=DebugConfig::default();
        if developer {
            debug.theme_file=vars.get("SODECK_THEME_FILE").map(PathBuf::from);
            debug.brand_file=vars.get("SODECK_TEST_BRAND_FILE").map(PathBuf::from);
            debug.fail_application_menu=vars.contains_key("SODECK_TEST_APPLICATION_MENU_FAILURE");
            if let Some(path)=vars.get("SODECK_DATA_DIR"){data=PathBuf::from(path);}
            debug.shell_port=vars.get("SODECK_SHELL_DEBUG_PORT").and_then(|p|p.to_str()).and_then(|p|p.parse::<u16>().ok()).filter(|p|*p>1024);
            debug.webview_args=vars.get("SODECK_WEBVIEW_ARGS").and_then(|p|p.to_str()).map(str::to_owned);
            debug.plugin_ports=vars.get("SODECK_PLUGIN_DEBUG_PORTS").and_then(|v|v.to_str()).and_then(|s|serde_json::from_str::<BTreeMap<String,u16>>(s).ok()).unwrap_or_default();
            debug.plugin_ports.retain(|_,p|*p>1024);
        }
        let overrides=developer&&(vars.contains_key("SODECK_PLUGIN_PACKAGES")||vars.contains_key("SODECK_PLUGIN_PACKAGE"));
        let resources=if overrides {
            let paths=vars.get("SODECK_PLUGIN_PACKAGES").map(|v|std::env::split_paths(v).collect::<Vec<_>>()).unwrap_or_else(||vec![PathBuf::from(&vars["SODECK_PLUGIN_PACKAGE"])]);
            match vars.get("SODECK_PLUGIN_KEYS") {
                Some(keys) if !paths.is_empty()=>Ok(Resources{keys:PathBuf::from(keys),packages:paths.into_iter().map(|path|Bundle{plugin_id:None,path}).collect(),warnings:vec![]}),
                _=>Err("开发覆盖必须同时指定插件包和签名公钥".into()),
            }
        }else{indexed_resources(dir,framework)};
        Ok(Self{data,resources,debug})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->tempfile::TempDir{
        let temp=tempfile::tempdir().unwrap();let r=temp.path().join("resources");fs::create_dir(&r).unwrap();
        fs::write(r.join("keys.json"),"{}").unwrap();
        let plugins=["terminal","files","network-proxy"].map(|name|{fs::write(r.join(format!("{name}.pkg")),"fixture").unwrap();serde_json::json!({"pluginId":format!("sodeck.{name}"),"path":format!("{name}.pkg")})});
        fs::write(r.join("bundled-plugins.json"),serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"publicKeys":"keys.json","plugins":plugins})).unwrap()).unwrap();temp
    }
    #[test]fn ordinary_and_release_ignore_development_overrides(){
        let t=fixture();fs::write(t.path().join("portable.json"),r#"{"schemaVersion":1}"#).unwrap();
        let mut vars=BTreeMap::from([("SODECK_DATA_DIR".into(),"unrelated-data".into()),("SODECK_SHELL_DEBUG_PORT".into(),"9555".into()),("SODECK_PLUGIN_PACKAGE".into(),"missing.pkg".into()),("SODECK_PLUGIN_KEYS".into(),"missing.keys".into())]);
        for (build,mode) in [(true,"0"),(false,"1")] {vars.insert("SODECK_DEV_MODE".into(),mode.into());let c=StartupConfig::from_inputs(&t.path().join("app.exe"),Path::new("system"),Path::new("missing"),build,&vars).unwrap();assert_eq!(c.data,t.path().join("data"));assert!(c.debug.shell_port.is_none());assert_eq!(c.resources.unwrap().packages.len(),3);}
        vars.insert("SODECK_DEV_MODE".into(),"1".into());let c=StartupConfig::from_inputs(&t.path().join("app.exe"),Path::new("system"),Path::new("missing"),true,&vars).unwrap();assert_eq!(c.data,Path::new("unrelated-data"));assert_eq!(c.debug.shell_port,Some(9555));
    }
    #[test]fn index_cannot_escape_or_silently_skip_a_plugin(){
        let t=fixture();let root=fs::canonicalize(t.path().join("resources")).unwrap();assert!(resource(&root,"../keys.json").is_err());
        fs::write(root.join("bundled-plugins.json"),r#"{"schemaVersion":1,"publicKeys":"keys.json","plugins":[]}"#).unwrap();assert!(indexed_resources(t.path(),t.path()).is_err());
        fs::write(t.path().join("portable.json"),r#"{"schemaVersion":2}"#).unwrap();assert!(StartupConfig::from_inputs(&t.path().join("app.exe"),t.path(),t.path(),false,&BTreeMap::new()).is_err());
    }
    #[test]fn missing_bundle_keeps_keys_for_verifying_active_versions(){
        let t=fixture();fs::remove_file(t.path().join("resources/files.pkg")).unwrap();
        let resources=indexed_resources(t.path(),t.path()).unwrap();
        assert!(resources.keys.is_file());assert_eq!(resources.packages.len(),2);assert_eq!(resources.warnings.len(),1);assert!(resources.warnings[0].contains("files.pkg"));
    }
}
