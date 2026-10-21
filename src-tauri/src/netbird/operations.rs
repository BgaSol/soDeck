use super::{
    manager::{Draft, NetBirdManager},
    model::*,
    query, runner, safety,
    store::Receipt,
};
use crate::{
    connection::{self, WindowOwner},
    model::{AppError, AppResult},
    traffic::{now, TrafficPurpose},
    State,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub(super) fn preview_matches(
    draft: &Draft,
    owner: &WindowOwner,
    identity: &str,
    request: &OperationRequest,
) -> bool {
    draft.owner.label == owner.label
        && draft.owner.generation == owner.generation
        && draft.identity == identity
        && draft.preview.allowed
        && draft.preview.expires_at > now()
        && draft.preview.binding_token == request.binding_token
        && draft.preview.action == request.action
}
pub(super) fn achieved(r: &Receipt, snapshot: &Snapshot) -> bool {
    match r.operation.action {
        Action::Up => snapshot.connection == "connected",
        Action::Down => snapshot.connection == "disconnected",
        Action::ServiceStart => snapshot.service == "running",
        Action::ServiceRestart => {
            r.returned
                && snapshot.service == "running"
                && snapshot.binding.as_ref().is_some_and(|b| {
                    !b.service.invocation.is_empty() && b.service.invocation != r.invocation
                })
        }
    }
}
impl NetBirdManager {
    pub fn prepare(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        token: &str,
        action: Action,
    ) -> AppResult<Preview> {
        let (preview, signature, identity) = self.read(
            state,
            session,
            owner,
            token,
            |ssh, spec, snapshot, check| {
                let binding = snapshot.binding.as_ref().unwrap();
                let path = safety::assess(ssh, spec, snapshot, &|| check())?;
                let capable = query::allowed(snapshot, &action);
                let allowed = capable && (action == Action::Up || path.result == "independent");
                let mut consequences = vec![match action {
                    Action::Up => "连接可能改变远端路由和 DNS；若需要重新登录，请在终端处理。",
                    Action::Down => "将断开此客户端的 NetBird 隧道，经过它的访问会中断。",
                    Action::ServiceStart => {
                        "将启动已确认的 netbird.service，可能恢复隧道、路由和 DNS。"
                    }
                    Action::ServiceRestart => {
                        "将重启已确认的 netbird.service，现有 NetBird 连接会短暂中断。"
                    }
                }
                .into()];
                if !capable {
                    consequences.push(
                        binding
                            .capabilities
                            .reason
                            .clone()
                            .unwrap_or_else(|| "当前状态不支持此操作".into()),
                    );
                }
                if action == Action::Up && path.result != "independent" {
                    consequences.push("无法确认连接后的 SSH 路径，操作可能使会话断开。".into());
                }
                Ok((
                    Preview {
                        id: uuid::Uuid::new_v4().to_string(),
                        binding_token: token.into(),
                        action: action.clone(),
                        expires_at: now() + 30_000,
                        allowed,
                        path,
                        consequences,
                    },
                    binding.signature.clone(),
                    spec.identity().to_string(),
                ))
            },
        )?;
        let mut drafts = self.drafts.lock().unwrap();
        drafts.retain(|_, d| d.preview.expires_at > now());
        if drafts.len() >= 128 {
            return Err(AppError::new("busy", "待确认操作过多"));
        }
        drafts.insert(
            preview.id.clone(),
            Draft {
                owner: owner.clone(),
                identity,
                preview: preview.clone(),
                signature,
            },
        );
        Ok(preview)
    }
    pub fn submit(
        &self,
        state: &Arc<State>,
        session: &str,
        owner: &WindowOwner,
        request: OperationRequest,
    ) -> AppResult<String> {
        if uuid::Uuid::parse_str(&request.request_id).is_err() {
            return Err(AppError::new("validation", "无效的请求 ID"));
        }
        let _gate = state.configuration.lock().unwrap();
        let spec = state.spec(session, &owner.label)?;
        self.check_owner(state, session, owner, spec.identity())?;
        if let Some(prior) = self.with_store(|s| s.request(spec.identity(), &request.request_id))? {
            if prior.operation.action != request.action
                || prior.binding_token != request.binding_token
                || prior.preview_id != request.preview_id
            {
                return Err(AppError::new("conflict", "请求 ID 已用于另一个操作"));
            }
            return Ok(prior.operation.id);
        }
        let (entry, snapshot) = self.bound(&spec, &request.binding_token)?;
        let mut drafts = self.drafts.lock().unwrap();
        let draft = drafts
            .get(&request.preview_id)
            .ok_or_else(|| AppError::new("conflict", "操作预检已失效，请重新预检"))?;
        if !preview_matches(draft, owner, spec.identity(), &request)
            || snapshot.binding.as_ref().unwrap().signature != draft.signature
        {
            return Err(AppError::new("conflict", "操作预检已过期或客户端已变化"));
        }
        let target =
            super::parser::hash(&format!("{}|{}", spec.target_public_key(), query::DAEMON));
        let mut locks = self.locks.lock().unwrap();
        if locks.contains_key(&target) {
            return Err(AppError::new("busy", "此客户端已有操作正在进行"));
        }
        let binding = snapshot.binding.as_ref().unwrap();
        let r = Receipt {
            identity: spec.identity().into(),
            target: target.clone(),
            servers: spec.server_ids().map(str::to_string).collect(),
            signature: binding.signature.clone(),
            binding_token: request.binding_token.clone(),
            preview_id: request.preview_id.clone(),
            invocation: binding.service.invocation.clone(),
            returned: false,
            operation: Operation {
                id: uuid::Uuid::new_v4().to_string(),
                request_id: request.request_id,
                action: request.action,
                phase: "queued".into(),
                message: "已接收，正在重新检查客户端和 SSH 路径".into(),
                created_at: now(),
                finished_at: None,
                acknowledged: false,
            },
        };
        self.with_store(|s| s.save(&r))?;
        locks.insert(target.clone(), r.servers.clone());
        drafts.remove(&request.preview_id);
        drop(locks);
        drop(drafts);
        {
            let mut c = entry.core.lock().unwrap();
            c.busy = true;
            c.generation += 1;
            c.operations.insert(0, r.operation.clone());
            c.operations.truncate(100);
        }
        if let Some(c) = entry.poll.lock().unwrap().as_ref() {
            c.cancel();
        }
        entry.publish();
        let id = r.operation.id.clone();
        let state = state.clone();
        std::thread::spawn(move || {
            let mut r = r;
            let attempted = AtomicBool::new(false);
            let outcome = (|| {
                let job = state.netbird.job()?;
                let until = Instant::now() + Duration::from_secs(55);
                let check = || {
                    job.cancel.check()?;
                    state.check_context(&spec)?;
                    if Instant::now() >= until {
                        Err(AppError::new("timeout", "操作核对超时"))
                    } else {
                        Ok(())
                    }
                };
                check()?;
                let transport = connection::connect_verified(
                    &spec,
                    &job.cancel,
                    TrafficPurpose::NetBirdAction,
                )?;
                let path = state.netbird.with_store(|s| s.path(&spec.target().id))?;
                let fresh = query::detect(transport.session(), path.as_deref(), &snapshot, &check)?;
                if fresh
                    .binding
                    .as_ref()
                    .is_none_or(|b| b.signature != r.signature)
                    || !query::allowed(&fresh, &r.operation.action)
                {
                    return Err(AppError::new("conflict", "客户端身份或操作能力已变化"));
                }
                let assessment = safety::assess(transport.session(), &spec, &fresh, &check)?;
                if r.operation.action != Action::Up && assessment.result != "independent" {
                    return Err(AppError::new("conflict", "SSH 路径未通过复检，未执行操作"));
                }
                r.operation.phase = "running".into();
                r.operation.message = format!("正在{}", r.operation.action.label());
                state.netbird.save_receipt(&r)?;
                let command =
                    query::operation_command(fresh.binding.as_ref().unwrap(), &r.operation.action);
                let receipt = runner::run(
                    transport.session(),
                    &command,
                    25,
                    256 * 1024,
                    &attempted,
                    &check,
                )?;
                r.returned = true;
                if receipt.code != 0 {
                    return Err(AppError::new(
                        if [124, 137].contains(&receipt.code) {
                            "uncertain"
                        } else {
                            "remote"
                        },
                        format!(
                            "远端命令返回 {}；请在终端检查权限、认证或服务状态",
                            receipt.code
                        ),
                    ));
                }
                r.operation.phase = "verifying".into();
                r.operation.message = "命令已返回，正在核对实际状态".into();
                state.netbird.save_receipt(&r)?;
                let mut last_error = None;
                for delay in [0, 500, 1000, 2000, 4000] {
                    std::thread::sleep(Duration::from_millis(delay));
                    check()?;
                    match query::detect(transport.session(), path.as_deref(), &fresh, &check) {
                        Ok(current) => {
                            if current
                                .binding
                                .as_ref()
                                .is_none_or(|b| b.signature != r.signature)
                            {
                                last_error =
                                    Some(AppError::new("uncertain", "操作后客户端身份无法对应"));
                                continue;
                            }
                            entry.core.lock().unwrap().snapshot = current.clone();
                            entry.publish();
                            if achieved(&r, &current) {
                                return Ok(());
                            }
                            last_error = Some(AppError::new(
                                if current.collection_status == "ready" {
                                    "state"
                                } else {
                                    "uncertain"
                                },
                                "命令已结束，但尚未达到目标状态",
                            ));
                        }
                        Err(e) => last_error = Some(AppError::new("uncertain", e.message)),
                    }
                }
                Err(last_error.unwrap_or_else(|| AppError::new("uncertain", "无法核对操作结果")))
            })();
            match outcome {
                Ok(()) => {
                    r.operation.phase = "succeeded".into();
                    r.operation.message = "已核对远端状态，操作完成".into();
                }
                Err(e) => {
                    r.operation.phase = if (attempted.load(Ordering::SeqCst) && !r.returned)
                        || matches!(e.kind.as_str(), "uncertain" | "timeout" | "cancelled")
                            && attempted.load(Ordering::SeqCst)
                    {
                        "uncertain"
                    } else {
                        "failed"
                    }
                    .into();
                    r.operation.message = if r.operation.phase == "uncertain" {
                        format!("结果待核对：{}。不会自动重试", e.message)
                    } else {
                        e.message
                    };
                }
            }
            r.operation.finished_at = Some(now());
            if let Err(e) = state.netbird.save_receipt(&r) {
                r.operation.phase = "uncertain".into();
                r.operation.message = format!("回执保存失败：{}；请检查当前状态", e.message);
                state.netbird.publish_receipt(&r);
            }
            {
                let mut c = entry.core.lock().unwrap();
                c.busy = false;
                c.next = Instant::now();
            }
            state.netbird.locks.lock().unwrap().remove(&target);
            entry.publish();
        });
        Ok(id)
    }
    fn save_receipt(&self, r: &Receipt) -> AppResult<()> {
        self.with_store(|s| s.save(r))?;
        self.publish_receipt(r);
        Ok(())
    }
    fn publish_receipt(&self, r: &Receipt) {
        crate::diagnostics::state_event("netbird", &r.operation.id, &r.operation.phase);
        if let Ok(e) = self.entry(&r.identity) {
            {
                let mut c = e.core.lock().unwrap();
                if let Some(op) = c.operations.iter_mut().find(|o| o.id == r.operation.id) {
                    *op = r.operation.clone();
                } else {
                    c.operations.insert(0, r.operation.clone());
                }
            }
            e.publish();
        }
    }
    pub fn reconcile(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        id: &str,
    ) -> AppResult<()> {
        let spec = state.spec(session, &owner.label)?;
        let mut r = self.with_store(|s| s.get(id))?;
        if r.identity != spec.identity() {
            return Err(AppError::new("permission", "操作不属于当前 SSH 身份"));
        }
        if r.operation.phase != "uncertain" {
            return Err(AppError::new("conflict", "仅待核对的操作需要检查"));
        }
        let (entry, token) = {
            let e = self.entry(spec.identity())?;
            let token = e
                .core
                .lock()
                .unwrap()
                .snapshot
                .binding
                .as_ref()
                .map(|b| b.token.clone())
                .ok_or_else(|| AppError::new("unavailable", "请先刷新客户端状态"))?;
            (e, token)
        };
        let fresh = self.read(state, session, owner, &token, |_, _, s, _| Ok(s.clone()))?;
        if fresh
            .binding
            .as_ref()
            .is_none_or(|b| b.signature != r.signature)
        {
            return Err(AppError::new(
                "conflict",
                "客户端身份已变化，无法核对原操作",
            ));
        }
        if achieved(&r, &fresh) {
            r.operation.phase = "succeeded".into();
            r.operation.message = "重新检查确认已达到目标状态".into();
            r.operation.finished_at = Some(now());
        } else {
            r.operation.message = "已检查当前状态，仍不足以确认原操作结果；未重复执行命令".into();
        }
        self.save_receipt(&r)?;
        entry.core.lock().unwrap().snapshot = fresh;
        entry.publish();
        Ok(())
    }
    pub fn acknowledge(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        id: &str,
    ) -> AppResult<()> {
        let spec = state.spec(session, &owner.label)?;
        let mut r = self.with_store(|s| s.get(id))?;
        if r.identity != spec.identity() || r.operation.active() {
            return Err(AppError::new("conflict", "操作尚未结束或不属于当前会话"));
        }
        r.operation.acknowledged = true;
        self.save_receipt(&r)
    }
}
