use super::UpdateService;
use crate::{official_plugins::OfficialCatalog, AppCore};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct OfficialPreparation {
    pub phase: String,
    pub catalog: Option<OfficialCatalog>,
    pub error: Option<String>,
}

/// Keeps catalog discovery and the gaps between individual installs in one admission scope.
pub struct OfficialInstall {
    core: Arc<AppCore>,
    preparation: bool,
    completed: bool,
}

impl OfficialInstall {
    fn finish(mut self, error: Option<String>) {
        self.complete(error);
    }
    fn complete(&mut self, error: Option<String>) {
        self.completed = true;
        let mut state = self.core.updates.lock().unwrap();
        state.official_installing = false;
        if self.preparation {
            if let Some(preparation) = &mut state.official_preparation {
                preparation.phase = "finished".into();
                preparation.error = error;
            }
        }
        drop(state);
        self.core.changed();
    }
}

impl Drop for OfficialInstall {
    fn drop(&mut self) {
        if !self.completed {
            self.complete(Some("官方插件安装已中断，请重试".into()));
        }
    }
}

impl AppCore {
    pub(crate) fn require_official_install_idle(&self) -> Result<(), String> {
        if self.updates.lock().unwrap().official_installing {
            Err("官方插件正在安装，请完成后修改插件登记".into())
        } else {
            Ok(())
        }
    }
}

impl UpdateService {
    pub fn reserve_default_installation(&self) -> Option<OfficialInstall> {
        let mutation = self.core.mutations.lock().unwrap();
        let mut state = self.core.updates.lock().unwrap();
        if !self.core.started_without_plugins
            || self.core.is_closing()
            || state.is_busy()
            || state.official_preparation.is_some()
        {
            return None;
        }
        let store = self.core.store.lock().unwrap();
        if store.errors[0].is_some() || !store.plugins.is_empty() {
            return None;
        }
        drop(store);
        state.official_installing = true;
        state.official_preparation = Some(OfficialPreparation {
            phase: "pending".into(),
            catalog: None,
            error: None,
        });
        drop(state);
        drop(mutation);
        self.core.changed();
        Some(OfficialInstall {
            core: self.core.clone(),
            preparation: true,
            completed: false,
        })
    }

    pub async fn prepare_official(&self, reservation: OfficialInstall) -> Result<(), String> {
        self.run_official_install(reservation, None, false)
            .await
            .map(|_| ())
    }

    pub async fn retry_official_preparation(&self) -> Result<(), String> {
        let reservation = {
            let mutation = self.core.mutations.lock().unwrap();
            let mut state = self.core.updates.lock().unwrap();
            if self.core.is_closing() || state.is_busy() {
                return Err("已有任务在处理中，请稍后重试".into());
            }
            let preparation = state
                .official_preparation
                .as_mut()
                .ok_or("本次启动没有自动安装任务")?;
            if preparation
                .catalog
                .as_ref()
                .is_some_and(|c| !c.plugins.is_empty())
            {
                return Err("请使用列表中的重试安装，只重试失败项".into());
            }
            preparation.phase = "catalog".into();
            preparation.error = None;
            state.official_installing = true;
            drop(state);
            drop(mutation);
            OfficialInstall {
                core: self.core.clone(),
                preparation: true,
                completed: false,
            }
        };
        self.run_official_install(reservation, None, true)
            .await
            .map(|_| ())
    }

    pub async fn install_official(&self, id: String) -> Result<String, String> {
        let reservation = {
            let mutation = self.core.mutations.lock().unwrap();
            let mut state = self.core.updates.lock().unwrap();
            if self.core.is_closing() || state.is_busy() {
                return Err("已有任务在处理中，请稍后重试".into());
            }
            let preparation = state.official_preparation.as_mut().is_some_and(|p| {
                let tracked = p
                    .catalog
                    .as_ref()
                    .is_some_and(|c| c.plugins.iter().any(|p| p.id == id));
                if tracked {
                    p.phase = "installing".into();
                }
                tracked
            });
            state.official_installing = true;
            drop(state);
            drop(mutation);
            OfficialInstall {
                core: self.core.clone(),
                preparation,
                completed: false,
            }
        };
        self.run_official_install(reservation, Some(id.clone()), false)
            .await?;
        Ok(id)
    }

    async fn run_official_install(
        &self,
        reservation: OfficialInstall,
        id: Option<String>,
        refresh: bool,
    ) -> Result<(), String> {
        let core = self.core.clone();
        let client = match self.client() {
            Ok(client) => client,
            Err(error) => {
                reservation.finish(Some(error.clone()));
                return Err(error);
            }
        };
        tauri::async_runtime::spawn_blocking(move || {
            if reservation.preparation {
                if let Some(p) = &mut core.updates.lock().unwrap().official_preparation {
                    p.phase = "catalog".into();
                }
            }
            core.changed();
            let result = (|| {
                if core.is_closing() {
                    return Err("应用正在退出或重启".into());
                }
                let catalog = core.official_plugins(&client, refresh)?;
                let targets = if let Some(id) = &id {
                    vec![catalog
                        .plugins
                        .iter()
                        .find(|p| &p.id == id)
                        .ok_or("该插件不在官方列表中，请刷新列表后重试")?
                        .clone()]
                } else {
                    catalog.plugins.clone()
                };
                if reservation.preparation {
                    let mut state = core.updates.lock().unwrap();
                    let p = state.official_preparation.as_mut().unwrap();
                    // Single-item retries preserve the original batch membership.
                    if id.is_none() {
                        p.catalog = Some(catalog);
                    }
                    p.phase = "installing".into();
                }
                core.changed();
                let mut single_error = None;
                for plugin in targets {
                    if core.is_closing() {
                        return Err("应用正在退出或重启".into());
                    }
                    if core
                        .snapshot()
                        .plugins
                        .iter()
                        .any(|p| p.manifest.id == plugin.id)
                    {
                        if id.is_some() {
                            return Err("该插件已安装，请在详情中管理".into());
                        }
                        continue;
                    }
                    let op = core.update_operation(&plugin.id)?;
                    if let Err(error) = core.install_official_reserved(&plugin, &client, op) {
                        single_error = Some(error);
                    }
                }
                if id.is_some() {
                    single_error.map_or(Ok(()), Err)
                } else {
                    Ok(())
                }
            })();
            reservation.finish(if id.is_none() {
                result.as_ref().err().cloned()
            } else {
                None
            });
            result
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
