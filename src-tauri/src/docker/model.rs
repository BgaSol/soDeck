use crate::model::{AppError, AppResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineBinding {
    pub token: String,
    pub generation: String,
    pub engine_generation: u64,
    pub endpoint: String,
    pub daemon_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Remove,
    Pause,
    Unpause,
    Kill,
    Rename {
        name: String,
    },
    RestartPolicy {
        policy: String,
        maximum_retry_count: u32,
    },
    Compose {
        mode: super::compose::Mode,
        association_id: String,
        revision: u32,
    },
    Create {
        draft_id: String,
    },
    VolumeCreate,
    VolumeRemove,
    NetworkCreate,
    NetworkRemove,
    NetworkConnect {
        container_id: String,
    },
    NetworkDisconnect {
        container_id: String,
    },
    CachePrune,
    Batch {
        entries: Vec<super::batch::Entry>,
    },
    Transfer {
        draft_id: String,
        kind: super::transfer::Kind,
    },
    ImagePull,
    ImageTag {
        reference: String,
    },
    ImageRemove {
        reference: Option<String>,
    },
}
impl Action {
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Start
                | Self::Stop
                | Self::Restart
                | Self::Remove
                | Self::Pause
                | Self::Unpause
                | Self::Kill
                | Self::Rename { .. }
                | Self::RestartPolicy { .. }
        )
    }
    pub fn validate_target(&self, id: &str) -> AppResult<()> {
        match self {
            Self::Transfer { kind, .. } => match kind {
                super::transfer::Kind::ImageLoad if id == "image-import" => Ok(()),
                super::transfer::Kind::ImageLoad => {
                    Err(AppError::new("validation", "无效的导入目标"))
                }
                super::transfer::Kind::ImageSave => super::images::validate_id(id),
                _ => validate_id(id),
            },
            Self::Batch { .. } if id == "selection" => Ok(()),
            Self::Batch { .. } => Err(AppError::new("validation", "无效的批量目标")),
            Self::VolumeCreate | Self::VolumeRemove | Self::NetworkCreate => {
                super::resources::name(id)
            }
            Self::NetworkRemove | Self::NetworkConnect { .. } | Self::NetworkDisconnect { .. } => {
                validate_id(id)
            }
            Self::CachePrune if id == "builder-cache" => Ok(()),
            Self::CachePrune => Err(AppError::new("validation", "无效的缓存目标")),
            Self::Create { .. } => Self::Rename { name: id.into() }.validate(),
            Self::Compose { .. } => super::compose::project_name(id),
            Self::ImagePull => super::images::validate_reference(id),
            Self::ImageTag { .. } | Self::ImageRemove { .. } => super::images::validate_id(id),
            _ => validate_id(id),
        }
    }
    pub fn requires_preview(&self) -> bool {
        matches!(self, Self::Remove | Self::Kill)
            || !self.is_container() && !matches!(self, Self::ImagePull)
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Start => "启动",
            Self::Stop => "停止",
            Self::Restart => "重启",
            Self::Remove => "删除",
            Self::Pause => "暂停",
            Self::Unpause => "恢复",
            Self::Kill => "强制停止",
            Self::Rename { .. } => "重命名",
            Self::RestartPolicy { .. } => "修改重启策略",
            Self::Transfer { kind, .. } => kind.label(),
            Self::Batch { .. } => "批量维护",
            Self::VolumeCreate => "创建数据卷",
            Self::VolumeRemove => "删除数据卷",
            Self::NetworkCreate => "创建网络",
            Self::NetworkRemove => "删除网络",
            Self::NetworkConnect { .. } => "连接网络",
            Self::NetworkDisconnect { .. } => "断开网络",
            Self::CachePrune => "清理构建缓存",
            Self::Create { .. } => "创建容器",
            Self::Compose { mode, .. } => mode.label(),
            Self::ImagePull => "拉取镜像",
            Self::ImageTag { .. } => "添加 Tag",
            Self::ImageRemove { .. } => "删除镜像或引用",
        }
    }
    pub fn command(&self, endpoint: &str, id: &str) -> AppResult<String> {
        self.validate()?;
        self.validate_target(id)?;
        let prefix = super::query::docker(endpoint);
        if !self.is_container() {
            return super::images::command(self, &prefix, id);
        }
        let id = crate::ssh::quote(id);
        let command = match self {
            Self::Start => format!("start {id}"),
            Self::Stop => format!("stop {id}"),
            Self::Restart => format!("restart {id}"),
            Self::Remove => format!("rm {id}"),
            Self::Pause => format!("pause {id}"),
            Self::Unpause => format!("unpause {id}"),
            Self::Kill => format!("kill {id}"),
            Self::Rename { name } => format!("rename {id} {}", crate::ssh::quote(name)),
            Self::RestartPolicy {
                policy,
                maximum_retry_count,
            } => {
                let value = if policy == "on-failure" && *maximum_retry_count > 0 {
                    format!("{policy}:{maximum_retry_count}")
                } else {
                    policy.clone()
                };
                format!("update --restart {} {id}", crate::ssh::quote(&value))
            }
            _ => unreachable!(),
        };
        Ok(format!("{prefix} container {command}"))
    }
    pub fn validate(&self) -> AppResult<()> {
        match self {
            Self::Batch { entries } => super::batch::validate(entries),
            Self::NetworkConnect { container_id } | Self::NetworkDisconnect { container_id } => {
                validate_id(container_id)
            }
            Self::Compose {
                association_id,
                revision,
                ..
            } if association_id.is_empty() || association_id.len() > 128 || *revision == 0 => {
                Err(AppError::new("validation", "请先关联有效的项目配置"))
            }
            Self::ImageTag { reference } => super::images::validate_tag(reference),
            Self::ImageRemove {
                reference: Some(reference),
            } => super::images::validate_reference(reference),
            Self::Rename { name }
                if name.is_empty()
                    || name.len() > 255
                    || !name.as_bytes()[0].is_ascii_alphanumeric()
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) =>
            {
                Err(AppError::new(
                    "validation",
                    "名称须以字母或数字开头，只能包含字母、数字、下划线、点和连字符",
                ))
            }
            Self::RestartPolicy {
                policy,
                maximum_retry_count,
            } if !["no", "always", "unless-stopped", "on-failure"].contains(&policy.as_str())
                || (*maximum_retry_count > 0 && policy != "on-failure")
                || *maximum_retry_count > 100000 =>
            {
                Err(AppError::new("validation", "无效的重启策略或重试次数"))
            }
            _ => Ok(()),
        }
    }
    pub fn needs_confirmation(&self, detail: &ContainerDetail) -> bool {
        matches!(self, Self::Remove | Self::Kill)
            || (detail.auto_remove && matches!(self, Self::Stop | Self::Restart))
    }
    /// Returns false when the requested state has already been reached.
    pub fn check(&self, detail: &ContainerDetail) -> AppResult<bool> {
        let state = detail.state.as_str();
        self.validate()?;
        match self {
            Self::Pause if state == "running" => Ok(true),
            Self::Unpause if state == "paused" => Ok(true),
            Self::Kill if matches!(state, "running" | "paused" | "restarting") => Ok(true),
            Self::Rename { name } if !matches!(state, "removing" | "dead") => {
                Ok(*name != detail.name)
            }
            Self::RestartPolicy {
                policy,
                maximum_retry_count,
            } if !detail.auto_remove && !matches!(state, "removing" | "dead") => Ok(*policy
                != detail.restart_policy
                || u64::from(*maximum_retry_count) != detail.restart_maximum),
            Self::Start if state == "running" => Ok(false),
            Self::Start if matches!(state, "created" | "exited") => Ok(true),
            Self::Stop if matches!(state, "created" | "exited") => Ok(false),
            Self::Stop if matches!(state, "running" | "restarting") => Ok(true),
            Self::Restart if state == "running" => Ok(true),
            Self::Remove if matches!(state, "created" | "exited" | "dead") => Ok(true),
            _ => Err(AppError::new(
                "conflict",
                format!("容器当前状态为 {state}，无法{}；请刷新后重试", self.label()),
            )),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub request_id: String,
    pub engine_token: String,
    pub target_id: String,
    pub action: Action,
    pub preview_id: Option<String>,
}
impl OperationRequest {
    pub fn validate(&self) -> AppResult<()> {
        self.action.validate_target(&self.target_id)?;
        self.action.validate()?;
        if self.request_id.is_empty()
            || self.request_id.len() > 128
            || self.engine_token.len() > 256
        {
            return Err(AppError::new("validation", "无效的操作标识"));
        }
        Ok(())
    }
}
pub fn validate_id(id: &str) -> AppResult<()> {
    if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(AppError::new("validation", "请使用完整的容器 ID"))
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    pub container_port: String,
    pub host_ip: Option<String>,
    pub host_port: Option<String>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub kind: String,
    pub name: Option<String>,
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub name: String,
    pub id: String,
    pub address: String,
    pub ipv6: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDetail {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub health: Option<String>,
    pub failing_streak: Option<u64>,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub error: String,
    pub restart_count: u64,
    pub restart_policy: String,
    pub restart_maximum: u64,
    pub auto_remove: bool,
    pub stop_timeout: Option<i64>,
    pub created: String,
    pub started_at: String,
    pub finished_at: String,
    pub ports: Vec<Port>,
    pub mounts: Vec<Mount>,
    pub networks: Vec<Network>,
    pub updated_at: u64,
    pub stats: Option<ContainerStats>,
    pub stats_loading: bool,
    pub stats_error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStats {
    pub cpu_percent: Option<f64>,
    pub memory: String,
    pub memory_percent: Option<f64>,
    pub network_io: String,
    pub block_io: String,
    pub processes: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub engine: EngineBinding,
    pub target_id: String,
    pub target_name: String,
    pub action: Action,
    pub consequences: Vec<String>,
    pub expires_at: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub request_id: String,
    pub engine: EngineBinding,
    pub target_id: String,
    pub target_name: String,
    pub action: Action,
    pub phase: String,
    pub command_result: String,
    pub message: String,
    pub observed_state: Option<String>,
    pub observed_health: Option<String>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub reconciling: bool,
    pub created_container_id: Option<String>,
    pub items: Vec<super::batch::Item>,
    pub transfer: Option<super::transfer::Progress>,
}
impl Operation {
    pub fn active(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "accepted" | "checking" | "running" | "verifying"
        ) || self.reconciling
    }
    pub fn blocks(&self) -> bool {
        self.active() || self.phase == "unknown"
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailState {
    pub container_id: String,
    pub value: Option<ContainerDetail>,
    pub loading: bool,
    pub stale: bool,
    pub error_kind: Option<String>,
    pub missing: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub subscription_id: String,
    pub revision: u64,
    pub engine: Option<EngineBinding>,
    pub operations: Vec<Operation>,
    pub detail: Option<DetailState>,
    pub error: Option<String>,
    pub catalog_revision: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub operations: Vec<Operation>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Default, Serialize)]
pub struct Summary {
    pub active: usize,
    pub attention: usize,
}
