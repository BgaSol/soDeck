//! Short-lived directory capabilities for the keyboard copy alternative.
//! A caller never receives permission to access another window's session.
use super::*;

#[derive(Default)]
pub(super) struct TargetState {
    registry: Mutex<Registry>,
}
#[derive(Default)]
struct Registry {
    views: HashMap<String, FileView>,
    revoked: HashMap<String, Instant>,
    grants: HashMap<String, TargetGrant>,
}
#[derive(Clone)]
struct FileView {
    owner: String,
    generation: u64,
    session: String,
    path: String,
    revision: u64,
    expires: Instant,
}
struct TargetGrant {
    caller: String,
    generation: u64,
    source_session: String,
    view_id: String,
    view: FileView,
    expires: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyTarget {
    token: String,
    server_name: String,
    path: String,
}

impl Registry {
    fn prune(&mut self) {
        let now = Instant::now();
        self.views.retain(|_, v| v.expires > now);
        self.revoked.retain(|_, e| *e > now);
        self.grants.retain(|_, g| g.expires > now);
    }
}

#[tauri::command]
pub fn register_file_view(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    view_id: String,
    path: String,
    revision: u64,
) -> AppResult<()> {
    let session_id = state.context_key(&context, window.label())?;
    moves::workspace_only(&window)?;
    state.workspace_context(&session_id, window.label())?;
    uuid::Uuid::parse_str(&view_id).map_err(|_| AppError::new("validation", "文件视图标识无效"))?;
    crate::remote_path::absolute(&path)?;
    let generation = state.owner(window.label()).generation;
    let mut registry = state.drags.targets.registry.lock().unwrap();
    registry.prune();
    if registry.revoked.contains_key(&view_id) {
        return Ok(());
    }
    if let Some(old) = registry.views.get(&view_id) {
        if old.owner != window.label() || old.generation != generation || old.session != session_id
        {
            return Err(AppError::new("permission", "文件视图归属已变化"));
        }
        if old.revision >= revision {
            return Ok(());
        }
    }
    registry.views.insert(
        view_id,
        FileView {
            owner: window.label().into(),
            generation,
            session: session_id,
            path,
            revision,
            expires: Instant::now() + Duration::from_secs(45),
        },
    );
    Ok(())
}
#[tauri::command]
pub fn unregister_file_view(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    view_id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let mut registry = state.drags.targets.registry.lock().unwrap();
    if registry
        .views
        .get(&view_id)
        .is_some_and(|v| v.owner != window.label())
    {
        return Err(AppError::new("permission", "此窗口无权移除文件视图"));
    }
    registry.views.remove(&view_id);
    registry
        .revoked
        .insert(view_id, Instant::now() + Duration::from_secs(120));
    Ok(())
}

#[tauri::command]
pub fn list_copy_targets(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
) -> AppResult<Vec<CopyTarget>> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    moves::workspace_only(&window)?;
    let source = state.workspace_context(&session_id, window.label())?;
    let mut registry = state.drags.targets.registry.lock().unwrap();
    registry.prune();
    registry
        .grants
        .retain(|_, g| g.caller != window.label() || g.source_session != session_id);
    let views: Vec<_> = registry
        .views
        .iter()
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let mut targets = Vec::new();
    for (view_id, view) in views {
        if state.owner(&view.owner).generation != view.generation
            || state.closed_windows.lock().unwrap().contains(&view.owner)
        {
            continue;
        }
        let Ok(target) = state.workspace_context(&view.session, &view.owner) else {
            continue;
        };
        if target.target().id == source.target().id || target.identity() == source.identity() {
            continue;
        }
        let token = uuid::Uuid::new_v4().to_string();
        targets.push(CopyTarget {
            token: token.clone(),
            server_name: target.target().name.clone(),
            path: view.path.clone(),
        });
        registry.grants.insert(
            token,
            TargetGrant {
                caller: window.label().into(),
                generation: state.owner(window.label()).generation,
                source_session: session_id.clone(),
                view_id,
                view,
                expires: Instant::now() + Duration::from_secs(120),
            },
        );
    }
    targets.sort_by(|a, b| (&a.server_name, &a.path).cmp(&(&b.server_name, &b.path)));
    Ok(targets)
}

fn validate_grant(
    state: &State,
    registry: &Registry,
    grant: &TargetGrant,
    window: &str,
    session: &str,
) -> AppResult<()> {
    let current = registry.views.get(&grant.view_id);
    if grant.caller != window
        || grant.source_session != session
        || grant.generation != state.owner(window).generation
        || grant.expires <= Instant::now()
        || state.owner(&grant.view.owner).generation != grant.view.generation
        || state
            .closed_windows
            .lock()
            .unwrap()
            .contains(&grant.view.owner)
        || current.is_none_or(|v| {
            v.owner != grant.view.owner
                || v.generation != grant.view.generation
                || v.session != grant.view.session
                || v.path != grant.view.path
                || v.expires <= Instant::now()
        })
    {
        return Err(AppError::new(
            "permission",
            "目标目录或窗口已变化，请刷新目标列表",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn prepare_server_copy(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    paths: Vec<String>,
    target_token: String,
) -> AppResult<DraftPreview> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    moves::workspace_only(&window)?;
    let source = state.workspace_context(&session_id, window.label())?;
    let paths = transfers::normalize_selection(&paths)?;
    let mut registry = state.drags.targets.registry.lock().unwrap();
    let grant = registry
        .grants
        .get(&target_token)
        .ok_or_else(|| AppError::new("permission", "目标授权已使用或过期，请刷新列表"))?;
    validate_grant(&state, &registry, grant, window.label(), &session_id)?;
    let target = state.workspace_context(&grant.view.session, &grant.view.owner)?;
    let path = grant.view.path.clone();
    let result = draft(
        &state,
        &window,
        Source::Remote {
            context: source,
            paths,
        },
        target,
        path,
    )?;
    registry.grants.remove(&target_token);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_capability_binds_caller_source_generation_and_directory() {
        let state = crate::connection::tests::state();
        let view = FileView {
            owner: "workspace-b".into(),
            generation: 0,
            session: "b".into(),
            path: "/target".into(),
            revision: 1,
            expires: Instant::now() + Duration::from_secs(45),
        };
        let grant = TargetGrant {
            caller: "main".into(),
            generation: 0,
            source_session: "a".into(),
            view_id: "view".into(),
            view: view.clone(),
            expires: view.expires,
        };
        let mut registry = Registry::default();
        registry.views.insert("view".into(), view);
        assert!(validate_grant(&state, &registry, &grant, "main", "a").is_ok());
        assert!(validate_grant(&state, &registry, &grant, "workspace-x", "a").is_err());
        assert!(validate_grant(&state, &registry, &grant, "main", "other").is_err());
        registry.views.get_mut("view").unwrap().path = "/changed".into();
        assert!(validate_grant(&state, &registry, &grant, "main", "a").is_err());
        registry.views.get_mut("view").unwrap().path = "/target".into();
        state
            .window_generations
            .lock()
            .unwrap()
            .insert("workspace-b".into(), 1);
        assert!(validate_grant(&state, &registry, &grant, "main", "a").is_err());
    }
}
