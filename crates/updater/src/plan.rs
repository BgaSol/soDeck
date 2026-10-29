use crate::{catalog::VerifiedCatalog,digest,error,model::*,Result};
use serde::{Deserialize,Serialize};
use std::collections::BTreeSet;

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="camelCase",deny_unknown_fields)]
pub enum Target {
    Core { release:CoreRelease, installer:CoreInstaller },
    Plugin { #[serde(rename="pluginId")] plugin_id:String, release:PluginRelease, #[serde(rename="requiresCore")] requires_core:Option<String> },
}
impl Target {
    pub fn asset(&self)->&Asset{match self{Self::Core{installer,..}=>&installer.asset,Self::Plugin{release,..}=>&release.asset}}
    pub fn version(&self)->&str{match self{Self::Core{release,..}=>&release.app_version,Self::Plugin{release,..}=>&release.version}}
    pub fn id(&self)->&str{match self{Self::Core{..}=>"core",Self::Plugin{plugin_id,..}=>plugin_id}}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ComponentPlan {
    pub component_id:String,
    pub current_version:Option<String>,
    pub available_version:Option<String>,
    pub selected:bool,
    pub target:Option<Target>,
    pub blocked_reasons:Vec<String>,
    pub required_component_ids:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct UpdatePlan {
    pub plan_revision:String,
    pub catalog_revision:String,
    pub components:Vec<ComponentPlan>,
    pub component_ids:Vec<String>,
    pub download_bytes:u64,
    pub blocked_reasons:Vec<String>,
}
fn newer(candidate:&str,installed:&str)->bool {
    semver::Version::parse(candidate).ok().zip(semver::Version::parse(installed).ok()).is_some_and(|(mut a,mut b)|{a.build=semver::BuildMetadata::EMPTY;b.build=semver::BuildMetadata::EMPTY;a>b})
}
fn best<'a>(versions:&'a Versions<PluginRelease>,installed:&InstalledPlugin,context:&Compatibility)->Option<&'a PluginRelease>{
    std::iter::once(&versions.current).chain(&versions.history).filter(|p| {
        p.requires.matches(context)&&installed.next().is_some_and(|v|newer(&p.version,&v.version))
    }).max_by_key(|p|semver::Version::parse(&p.version).ok())
}
pub fn make(catalog:&VerifiedCatalog,snapshot:&InstalledSnapshot,component_ids:&[String])->Result<UpdatePlan>{
    let mut ids=component_ids.to_vec();ids.sort();
    if ids.len()>64||ids.windows(2).any(|v|v[0]==v[1]){return Err(error("INVALID_PARAMS","更新组件选择重复或超出容量"));}
    let selected=|id:&str|ids.iter().any(|x|x==id);
    let c=catalog.catalog();let runtime=&snapshot.runtime;
    if c.channel!=runtime.channel{return Err(error("UPDATE_SOURCE_INVALID","更新渠道与当前应用不符"));}
    let available_core=&c.core.current;
    let core_new=newer(&available_core.app_version,&runtime.app_version);
    let mut core=ComponentPlan{component_id:"core".into(),current_version:Some(runtime.app_version.clone()),available_version:core_new.then(||available_core.app_version.clone()),selected:selected("core"),target:None,blocked_reasons:vec![],required_component_ids:vec![]};
    if core_new {
        if let Some(installer)=available_core.platforms.get(&runtime.compatibility.platform){core.target=Some(Target::Core{release:available_core.clone(),installer:installer.clone()});}
        else {core.blocked_reasons.push("新版本没有当前平台的安装包".into());}
        if runtime.distribution!="installed"{core.blocked_reasons.push("当前为目录运行，请手动安装新版本".into());}
        if runtime.data_formats!=available_core.data_formats{core.blocked_reasons.push("新版本需要处理数据格式变化，请先查看人工升级说明".into());}
    }
    let next_context=if selected("core")&&core_new{available_core.context(&runtime.compatibility.platform)}else{runtime.compatibility.clone()};
    let future_context=available_core.context(&runtime.compatibility.platform);
    let mut components=Vec::new();let mut seen=BTreeSet::new();
    for installed in &snapshot.plugins {
        if !seen.insert(&installed.plugin_id){return Err(error("CONFLICT","插件快照包含重复组件"));}
        let mut row=ComponentPlan{component_id:installed.plugin_id.clone(),current_version:installed.next().map(|p|p.version.clone()),available_version:None,selected:selected(&installed.plugin_id),target:None,blocked_reasons:vec![],required_component_ids:vec![]};
        if let Some(versions)=c.plugins.get(&installed.plugin_id){
            let target=best(versions,installed,&next_context);
            let future=if target.is_none()&&core_new{best(versions,installed,&future_context)}else{None};
            if let Some(p)=target.or(future){
                row.available_version=Some(p.version.clone());
                let requires_core=(!p.requires.matches(&runtime.compatibility)).then(||available_core.app_version.clone());
                if requires_core.is_some(){row.required_component_ids.push("core".into());}
                row.target=Some(Target::Plugin{plugin_id:installed.plugin_id.clone(),release:p.clone(),requires_core});
                if installed.selected.as_ref().is_some_and(|s|s.data_schema_version!=p.data_schema_version){row.blocked_reasons.push("插件数据格式变化，尚无可用迁移流程".into());}
            }
        }
        if !installed.follow_official{row.target=None;row.blocked_reasons.push("此插件由本地包管理".into());}
        if installed.pending.is_some(){row.target=None;row.blocked_reasons.push("已有待重启候选，请先处理现有候选".into());}
        components.push(row);
    }
    // Check the saved next-boot selection, including a locally staged package.
    if selected("core")&&core_new {
        for installed in snapshot.plugins.iter().filter(|p|p.enabled){
            let replacement=components.iter().find(|r|r.component_id==installed.plugin_id&&r.selected&&r.blocked_reasons.is_empty()).and_then(|r|r.target.as_ref());
            let compatible=match replacement {
                Some(Target::Plugin{release,..})=>release.requires.matches(&next_context),
                _=>installed.next().is_some_and(|p|p.requires.matches(&next_context)),
            };
            if !compatible {
                let candidate=components.iter().find(|r|r.component_id==installed.plugin_id&&r.target.is_some()&&r.blocked_reasons.is_empty());
                if candidate.is_some(){core.required_component_ids.push(installed.plugin_id.clone());}
                core.blocked_reasons.push(format!("插件 {} 与目标核心不兼容，请选择兼容更新或先停用",installed.plugin_id));
            }
        }
    }
    components.insert(0,core);
    if ids.iter().any(|id|!components.iter().any(|p|&p.component_id==id)){return Err(error("INVALID_PARAMS","选择了未安装或未知组件"));}
    let mut blocked_reasons=Vec::new();let mut download_bytes=0;
    for row in components.iter().filter(|r|r.selected){
        blocked_reasons.extend(row.blocked_reasons.iter().map(|r|format!("{}：{r}",row.component_id)));
        if let Some(t)=&row.target{download_bytes+=t.asset().size;}else{blocked_reasons.push(format!("{} 没有可用更新",row.component_id));}
    }
    let plan_revision=digest(&(catalog.digest(),snapshot,&ids));
    Ok(UpdatePlan{plan_revision,catalog_revision:catalog.digest().into(),components,component_ids:ids,download_bytes,blocked_reasons})
}

#[cfg(test)]
mod tests {
    #[test] fn semver_build_is_not_an_upgrade(){assert!(!super::newer("1.2.3+b","1.2.3+a"));assert!(super::newer("1.2.4","1.2.3"));assert!(!super::newer("1.2.4-beta.1","1.2.4"));}
}
