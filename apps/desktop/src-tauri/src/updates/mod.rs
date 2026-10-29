use super::*;
use sodeck_updater::{self as domain, catalog::VerifiedCatalog, model::*, plan::{self, Target, UpdatePlan}, store::{Store, Operation, Stage, ComponentResult}};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub mod identity;
mod plugin;
mod service;
mod installer;
mod work;
#[cfg(test)]
mod tests;

pub(super) struct Registry {
    store: Store,
    catalog: Option<VerifiedCatalog>,
    cancellations: BTreeMap<String, CancellationToken>,
    reservations: BTreeMap<String, u64>,
    asset_locks: BTreeMap<String, Arc<tokio::sync::Mutex<()>>>,
    reviews: BTreeMap<String, UpdatePlan>,
}
pub(super) struct Updates {
    storage: Mutex<domain::Result<Registry>>,
    pub build: identity::BuildIdentity,
    pub runtime: RuntimeIdentity,
    downloads: tokio::sync::Semaphore,
    check_lock: tokio::sync::Mutex<()>,
    cancel: CancellationToken,
    install_lock: Mutex<()>,
    work: Arc<work::Work>,
}
fn fault(e: domain::Error) -> CoreError { err(&e.code, e.message) }
fn manager_fault(e:sodeck_plugin_manager::Error)->domain::Error {let e=plugin_management::manager_error(e);domain::error(&e.code,e.message)}
pub(super) fn check(kind:&str,method:&str,value:&Value)->Result<()> {
    if serde_json::to_vec(value).unwrap().len()>64*1024 { return Err(err("CAPACITY","更新数据超出容量")); }
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(domain::contract::SCHEMA).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||err("UNSUPPORTED_API","未知更新方法"))?;
    sodeck_core::contract::check(schema,&schema["$defs"][name],value,method).map_err(|e|err(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},e))
}
impl Updates {
    pub fn stop(&self) {
        self.cancel.cancel();
        self.work.close_and_wait();
    }
    pub fn open(data:&Path,mode:&str,exe:&Path)->Self {
        let build=identity::BuildIdentity::compiled();let runtime=build.runtime(mode,exe);
        let storage=Store::open(data.join("updates")).map(|mut store| {
            let catalog=match store.catalog(&build.keys(),runtime.channel) {Ok(v)=>v,Err(e)=>{store.warnings.push(e.message);None}};
            if let Err(e)=store.reconcile_handoff(&runtime){store.warnings.push(e.message);}
            Registry{store,catalog,cancellations:BTreeMap::new(),reservations:BTreeMap::new(),asset_locks:BTreeMap::new(),reviews:BTreeMap::new()}
        });
        Self{storage:Mutex::new(storage),build,runtime,downloads:tokio::sync::Semaphore::new(2),check_lock:tokio::sync::Mutex::new(()),cancel:CancellationToken::new(),install_lock:Mutex::new(()),work:Arc::new(work::Work::default())}
    }
    fn with<T>(&self,f:impl FnOnce(&mut Registry)->domain::Result<T>)->domain::Result<T> {
        let mut storage=self.storage.lock().unwrap();match storage.as_mut(){Ok(r)=>f(r),Err(e)=>Err(e.clone())}
    }
    fn configured(&self)->bool {self.build.channel!="development"&&!self.build.keys().is_empty()&&self.build.updater_key.is_some()}
    fn policy(r:&Registry)->Value {json!({"generation":r.store.policy.generation,"revision":r.store.policy.revision.to_string(),"autoCheck":r.store.policy.auto_check})}
    fn operation(op:&Operation)->Value {
        json!({"operationId":op.operation_id,"kind":op.kind,"stage":op.stage,"running":op.running,"components":op.components.iter().map(|(id,c)|json!({"componentId":id,"stage":c.stage,"received":c.received,"total":c.total,"error":c.error})).collect::<Vec<_>>(),"error":op.error,"updatedAt":op.updated_at})
    }
    fn result(&self,id:&str)->domain::Result<Value> {self.with(|r|Ok(r.store.operation(id).map(Self::operation).unwrap_or_else(||json!({"operationId":id,"kind":"unknown","stage":"unknown","running":false,"components":[],"error":null,"updatedAt":0}))))}
    fn finish(&self,id:&str,result:domain::Result<Stage>) {
        let _=self.with(|r|{r.cancellations.remove(id);r.reviews.remove(id);r.asset_locks.retain(|_,lock|Arc::strong_count(lock)>1);r.store.update_operation(id,|o|{
            o.running=false;match result {Ok(stage)=>o.stage=stage,Err(e)=>{o.stage=if e.code=="CANCELLED"{Stage::Cancelled}else{Stage::Failed};o.error=Some(e);}}
        })?;Ok(())});
    }
    fn snapshot(&self,manager:&sodeck_plugin_manager::SelectionSnapshot)->domain::Result<InstalledSnapshot> {
        self.with(|r|Ok(InstalledSnapshot {runtime:self.runtime.clone(),selection_revision:manager.revision.clone(),enablement_revision:manager.enablement_revision.clone(),source_revision:domain::digest(&r.store.sources),plugins:manager.plugins.iter().map(|p| {
            let selection=|m:&sodeck_plugin_manager::SelectedPackageMetadata|PackageSelection {version:m.manifest.version.clone(),digest:m.digest.clone(),requires:Requirements{core:m.manifest.requires.core.clone(),plugin_api:m.manifest.requires.plugin_api.clone(),capabilities:m.manifest.requires.capabilities.clone(),platforms:m.manifest.requires.platforms.clone()},data_schema_version:m.manifest.data_schema_version};
            let (origin,follow)=p.pending.as_ref().or(p.selected.as_ref()).map(|m|r.store.source_for(&p.plugin_id,&m.digest)).unwrap_or((Origin::Local,false));
            domain::InstalledPlugin {plugin_id:p.plugin_id.clone(),selected:p.selected.as_ref().map(selection),pending:p.pending.as_ref().map(selection),enabled:p.enabled,origin,follow_official:follow,failure:p.error.clone()}
        }).collect()}))
    }
    fn plan(&self,manager:&sodeck_plugin_manager::SelectionSnapshot,ids:&[String])->domain::Result<UpdatePlan> {
        let snapshot=self.snapshot(manager)?;
        self.with(|r|plan::make(r.catalog.as_ref().ok_or_else(||domain::error("UPDATE_NOT_PUBLISHED","请先检查可用更新"))?,&snapshot,ids))
    }
    fn rows(&self,plan:&UpdatePlan,snapshot:&InstalledSnapshot)->domain::Result<Vec<Value>> {
        self.with(|r|Ok(plan.components.iter().map(|c| {
            let latest=r.store.state.operations.iter().rev().filter_map(|o|o.components.get(&c.component_id)).find(|o|c.target.as_ref().is_some_and(|t|o.target_digest.as_deref()==Some(t.asset().archive_sha256.as_str())));
            let installed=snapshot.plugins.iter().find(|p|p.plugin_id==c.component_id);
            let intent=r.store.state.installs.get(&c.component_id).filter(|i|c.target.as_ref().is_some_and(|t|t.asset().archive_sha256==i.target.asset().archive_sha256)||matches!(&i.target,Target::Plugin{release,..} if installed.and_then(|p|p.pending.as_ref()).is_some_and(|p|p.digest==release.package_digest)));
            let prepared=intent.filter(|i|matches!(i.stage,Stage::Ready|Stage::WaitingForCore));
            let stage=intent.filter(|i|matches!(i.stage,Stage::Ready|Stage::WaitingForCore)).map(|i|serde_json::to_value(&i.stage).unwrap()).or_else(||latest.map(|o|serde_json::to_value(&o.stage).unwrap())).unwrap_or_else(||json!(if !c.blocked_reasons.is_empty(){"blocked"}else{"available"}));
            json!({"componentId":c.component_id,"currentVersion":if prepared.is_some(){installed.and_then(|p|p.selected.as_ref()).map(|p|p.version.as_str())}else{c.current_version.as_deref()},"availableVersion":prepared.map(|i|i.target.version()).or(c.available_version.as_deref()),"selected":c.selected,"stage":stage,"received":latest.map_or(0,|o|o.received),"total":prepared.map_or_else(||c.target.as_ref().map_or(0,|t|t.asset().size),|i|i.target.asset().size),"blockedReasons":c.blocked_reasons,"requiredComponentIds":c.required_component_ids,"origin":installed.map(|p|serde_json::to_value(&p.origin).unwrap()).unwrap_or(json!("core")),"followOfficial":installed.is_none_or(|p|p.follow_official),"error":latest.and_then(|o|o.error.as_ref())})
        }).collect()))
    }
}
