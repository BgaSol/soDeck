use super::*;
use crate::window_registry::WindowRecord;

impl State {
    pub(super) fn move_workspace_batch(
        self: &Arc<Self>,
        ids: Vec<String>,
        target: &str,
        index: usize,
        active: Option<&str>,
        operation: Option<&str>,
        request: Value,
    ) -> Result<Value> {
        let op_id = operation
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let fingerprint =
            json!({"workspaceIds":ids,"targetWindowId":target,"index":index,"active":active});
        let op = {
            let mut operations = self.layout_operations.lock().unwrap();
            operations.retain(|_, o| {
                o.created.elapsed() < Duration::from_secs(600)
                    || o.result.lock().unwrap()["phase"] == "preparing"
            });
            if let Some(existing) = operations.get(&op_id) {
                let value = existing.result.lock().unwrap();
                if value["request"] != fingerprint {
                    return Err(err("REQUEST_CONFLICT", "操作标识已经用于其他移动"));
                }
                return Ok(value.clone());
            }
            if operations.len() >= 10000 {
                return Err(err("CAPACITY", "操作记录已满"));
            }
            let op = Arc::new(services::LayoutOperation {
                request,
                result: Mutex::new(
                    json!({"operationId":op_id,"phase":"preparing","committed":false,"request":fingerprint}),
                ),
                canceled: AtomicBool::new(false),
                created: Instant::now(),
            });
            operations.insert(op_id.clone(), op.clone());
            op
        };
        let result = self.transfer_workspaces(&ids, target, index, active, &op_id, &op);
        let mut value = match &result {
            Ok(v) => v.clone(),
            Err(error) => {
                json!({"operationId":op_id,"phase":"rolledBack","committed":false,"error":error})
            }
        };
        value["request"] = fingerprint;
        *op.result.lock().unwrap() = value.clone();
        result.map(|_| value)
    }
    fn transfer_workspaces(
        self: &Arc<Self>,
        ids: &[String],
        target: &str,
        index: usize,
        active: Option<&str>,
        op_id: &str,
        operation: &services::LayoutOperation,
    ) -> Result<Value> {
        let _guard = self.workspace_guard(ids)?;
        let fresh = !self.workbench.lock().unwrap().windows.contains_key(target);
        if fresh && !target.starts_with("workspace-") {
            return Err(err("WINDOW_CLOSED", "目标窗口不存在"));
        }
        let moving = {
            let w = self.workbench.lock().unwrap();
            for id in ids {
                w.owner(id).map_err(native_error)?;
            }
            w.views
                .iter()
                .filter(|v| ids.contains(&v.workspace_id) && v.placement != "detached")
                .cloned()
                .collect::<Vec<_>>()
        };
        let target_created = AtomicBool::new(!fresh);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut prepared: Vec<(View, Arc<Binding>, Option<Arc<Binding>>, String)> = vec![];
        let mut total_bytes = 0usize;
        let mut source_snapshots = Vec::new();
        let check = || {
            if operation.canceled.load(Ordering::SeqCst) {
                Err(err("CANCELED", "移动已取消"))
            } else if self.stopping.load(Ordering::SeqCst) {
                Err(err("APP_STOPPING", "应用正在退出"))
            } else if Instant::now() >= deadline {
                Err(err("MOVE_TIMEOUT", "整批迁移超过 30 秒"))
            } else if target_created.load(Ordering::SeqCst)
                && self.app.get_webview_window(target).is_none()
            {
                Err(err("WINDOW_CLOSED", "目标窗口已经关闭"))
            } else {
                Ok(())
            }
        };
        let result = (|| {
            for view in &moving {
                check()?;
                let Some(source) = self.view_binding(&view.view_id) else {
                    continue;
                };
                if source.window == target {
                    continue;
                }
                // A surface may have been created by a just-arrived geometry
                // report while its JS is still loading. Do not lose prepareMove.
                {
                    let mut ready = source.ready.lock().unwrap();
                    while !*ready {
                        check()?;
                        if source.phase.load(Ordering::SeqCst) == 3 {
                            return Err(err("MOVE_CANCELLED", "源表面已失效"));
                        }
                        let (next, _) = source
                            .changed
                            .wait_timeout(ready, Duration::from_millis(50))
                            .unwrap();
                        ready = next;
                    }
                }
                if self.view_operations.busy(&view.view_id) {
                    return Err(err("MOVE_IN_PROGRESS", "视图正在移动或关闭"));
                }
                let transfer = Uuid::new_v4().to_string();
                let (tx, rx) = mpsc::sync_channel(1);
                self.snapshots.lock().unwrap().insert(
                    transfer.clone(),
                    SnapshotTransfer {
                        source: source.id.clone(),
                        sender: tx,
                        data: String::new(),
                        total: 0,
                        next: 0,
                    },
                );
                prepared.push((view.clone(), source.clone(), None, transfer.clone()));
                self.native
                    .notify(
                        source.id.clone(),
                        "view.prepareMove",
                        json!({"operationId":transfer,"batchId":op_id}),
                    )
                    .map_err(native_error)?;
                let snapshot = loop {
                    check()?;
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(result) => break result?,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Err(err("MOVE_CANCELLED", "状态交接被中止")),
                    }
                };
                total_bytes += serde_json::to_vec(&snapshot).unwrap_or_default().len();
                if total_bytes > 32 * 1024 * 1024 {
                    return Err(err("CAPACITY", "整批快照超过 32 MiB，源工作区保持原位"));
                }
                source_snapshots.push(snapshot);
            }
            check()?;
            // Preparing every source first preserves IME/focus on later views:
            // even hidden native window creation can disturb composition.
            if fresh {
                self.create_workspace_window(target)?;
                self.workbench
                    .lock()
                    .unwrap()
                    .windows
                    .insert(target.into(), WindowRecord::new(target));
                target_created.store(true, Ordering::SeqCst);
            }
            for ((view, _, candidate_slot, _), snapshot) in
                prepared.iter_mut().zip(source_snapshots)
            {
                check()?;
                let mut candidate = view.clone();
                candidate.mount_generation += 1;
                let binding = self.create_binding(&candidate, target, Some(snapshot), true)?;
                *candidate_slot = Some(binding.clone());
                let mut ready = binding.ready.lock().unwrap();
                while !*ready {
                    check()?;
                    if binding.phase.load(Ordering::SeqCst) == 3 {
                        return Err(err("MOVE_CANCELLED", "候选表面已失效"));
                    }
                    let (next, _) = binding
                        .changed
                        .wait_timeout(ready, Duration::from_millis(50))
                        .unwrap();
                    ready = next;
                }
            }
            check()?;
            // No window is allowed to submit geometry or input in this commit.
            let _mount = self.mounting.lock().unwrap();
            let mut w = self.workbench.lock().unwrap();
            let mut next = w.clone();
            next.move_workspaces(ids, target, index, active)
                .map_err(native_error)?;
            let mut entries = vec![];
            for (view, _, candidate, _) in &prepared {
                let binding = candidate
                    .as_ref()
                    .ok_or_else(|| err("MOVE_CANCELLED", "候选表面未就绪"))?;
                if binding.phase.load(Ordering::SeqCst) != 0 {
                    return Err(err("MOVE_CANCELLED", "候选表面已失效"));
                }
                let v = next
                    .views
                    .iter_mut()
                    .find(|v| v.view_id == view.view_id)
                    .ok_or_else(|| err("VIEW_CLOSED", "迁移视图已关闭"))?;
                if v.mount_generation != view.mount_generation {
                    return Err(err("STALE_MOUNT", "迁移期间视图挂载已变化"));
                }
                v.mount_generation = binding.generation;
                v.revision += 1;
                entries.push(json!({"viewId":v.view_id,"workspaceId":v.workspace_id,"mountGeneration":v.mount_generation,"resources":w.resources.get(&v.view_id).cloned().unwrap_or_default()}));
            }
            let mut outcome = operation.result.lock().unwrap();
            check()?;
            if !entries.is_empty() {
                let cursors = self
                    .core
                    .bind_view_resources(&self.loaded()?.instance, &entries)?;
                for (view, _, candidate, _) in &prepared {
                    *candidate.as_ref().unwrap().resume.lock().unwrap() =
                        cursors[&view.view_id].clone();
                }
            }
            *w = next;
            for (_, source, candidate, _) in &prepared {
                source.phase.store(3, Ordering::SeqCst);
                let target = candidate.as_ref().unwrap();
                target.phase.store(1, Ordering::SeqCst);
                target.changed.notify_all();
            }
            outcome["phase"] = json!("committed");
            outcome["committed"] = json!(true);
            outcome["targetWindowId"] = json!(target);
            drop(outcome);
            drop(w);
            drop(_mount);
            for (_, source, _, transfer) in &prepared {
                let _ = self.native.notify(
                    source.id.clone(),
                    "view.moveCommitted",
                    json!({"operationId":transfer,"batchId":op_id}),
                );
                let _ = self.native.close(source.id.clone());
                self.bindings.lock().unwrap().remove(&source.id);
            }
            self.show_window(target);
            self.reap_empty_windows();
            self.update_window_titles();
            Ok(self.save_committed(json!({"operationId":op_id,"phase":"committed","workspaceIds":ids,"targetWindowId":target}),"工作区已移动，但布局未保存"))
        })();
        for (_, _, _, transfer) in &prepared {
            self.snapshots.lock().unwrap().remove(transfer);
        }
        if result.is_err() {
            for (_, source, candidate, transfer) in &prepared {
                if let Some(candidate) = candidate {
                    candidate.phase.store(3, Ordering::SeqCst);
                    candidate.changed.notify_all();
                    let _ = self.native.close(candidate.id.clone());
                    self.bindings.lock().unwrap().remove(&candidate.id);
                }
                if source.phase.load(Ordering::SeqCst) != 3 {
                    source.phase.store(1, Ordering::SeqCst);
                    let _ = self.native.notify(
                        source.id.clone(),
                        "view.moveCancelled",
                        json!({"operationId":transfer,"batchId":op_id}),
                    );
                }
            }
            if fresh {
                if let Some(win) = self.app.get_webview_window(target) {
                    let _ = win.destroy();
                }
                self.workbench.lock().unwrap().windows.remove(target);
            }
        }
        result
    }
}
