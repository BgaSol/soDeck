use super::*;
use crate::{
    connection::tests::{mock_context, state},
    model::ServerConfig,
};
use std::sync::{Arc, Barrier};

fn server(id: &str) -> ServerConfig {
    ServerConfig {
        id: id.into(),
        name: "同名服务器".into(),
        host: "localhost".into(),
        port: 22,
        username: "ops".into(),
        tags: vec![],
        auth_type: "password".into(),
        private_key: String::new(),
        password: String::new(),
        jump_server_id: None,
        direct_host: None,
        direct_port: 22,
    }
}
fn owner() -> WindowOwner {
    WindowOwner {
        label: "main".into(),
        generation: 1,
    }
}
#[test]
fn invalid_open_requests_do_not_leave_a_server_reservation() {
    let mut registry = Registry::default();
    for request in [String::new(), "x".repeat(129)] {
        assert!(registry
            .open(server("a").view(), owner(), false, &request)
            .is_err());
        assert!(registry.entries.is_empty());
        assert!(registry.by_server.is_empty());
        assert_eq!(registry.revision, 0);
    }
    let opened = registry
        .open(server("a").view(), owner(), false, "valid")
        .unwrap();
    assert_eq!(opened.terminals.len(), 1);
    assert_eq!(opened.terminals[0].number, 1);
}
#[test]
fn concurrent_opens_and_requests_have_one_authoritative_membership() {
    let runtime = Arc::new(WorkspaceState::default());
    let barrier = Arc::new(Barrier::new(16));
    let workers: Vec<_> = (0..16)
        .map(|_| {
            let runtime = runtime.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                runtime
                    .registry
                    .lock()
                    .unwrap()
                    .open(server("a").view(), owner(), true, "same-request")
                    .unwrap()
                    .id
            })
        })
        .collect();
    let ids: Vec<_> = workers.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(ids.iter().all(|id| id == &ids[0]));
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.workspaces.len(), 1);
    assert_eq!(snapshot.workspaces[0].terminals.len(), 1);
    let mut r = runtime.registry.lock().unwrap();
    let second = r.open(server("a").view(), owner(), true, "next").unwrap();
    assert_eq!(second.terminals.len(), 2);
    assert_eq!(second.terminals[1].number, 2);
    assert_ne!(second.terminals[0].id, second.terminals[1].id);
    let other = r.open(server("b").view(), owner(), false, "other").unwrap();
    assert_ne!(other.id, second.id);
}
#[test]
fn empty_workspace_retains_uniqueness_context_and_blocks_deletion() {
    let state = state();
    let context = mock_context(server("a"));
    let mut r = state.workspaces.registry.lock().unwrap();
    let view = r
        .open(server("a").view(), state.owner("main"), false, "open")
        .unwrap();
    let w = r.entries.get_mut(&view.id).unwrap();
    install_context(w, context.clone());
    let capability = w.view.tools.context.clone().unwrap();
    w.view.terminals.clear();
    w.view.active_terminal_id = None;
    drop(r);
    assert!(Arc::ptr_eq(
        &state.workspace_context(&capability.id, "main").unwrap(),
        &context
    ));
    assert!(state.terminals.lock().unwrap().is_empty());
    assert!(state.delete_server("a").is_err());
    assert_eq!(
        state
            .workspaces
            .registry
            .lock()
            .unwrap()
            .open(server("a").view(), state.owner("main"), false, "again")
            .unwrap()
            .id,
        view.id
    );
    close_window(&state, "main");
    assert!(state.workspace_context(&capability.id, "main").is_err());
    let reopened = state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .open(server("a").view(), state.owner("main"), false, "again")
        .unwrap();
    assert_ne!(reopened.id, view.id);
}
#[test]
fn capabilities_reject_terminal_ids_foreign_windows_and_stale_generations() {
    let state = state();
    let mut r = state.workspaces.registry.lock().unwrap();
    let view = r
        .open(server("a").view(), state.owner("main"), false, "open")
        .unwrap();
    let w = r.entries.get_mut(&view.id).unwrap();
    install_context(w, mock_context(server("a")));
    let first = w.view.tools.context.clone().unwrap();
    drop(r);
    assert!(state
        .workspace_context(&view.terminals[0].id, "main")
        .is_err());
    assert!(state
        .workspace_context(&first.id, "workspace-foreign")
        .is_err());
    assert!(state
        .context_key(
            &ToolContextRef {
                id: first.id.clone(),
                epoch: first.epoch + 1
            },
            "main"
        )
        .is_err());
    assert!(state.spec(&first.id, "main").is_err());
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.entries.get_mut(&view.id).unwrap();
    install_context(w, mock_context(server("a")));
    let second = w.view.tools.context.clone().unwrap();
    drop(r);
    assert_eq!(second.epoch, first.epoch + 1);
    assert!(state.context_key(&first, "main").is_err());
    assert!(state.context_key(&second, "main").is_ok());
    state
        .window_generations
        .lock()
        .unwrap()
        .insert("main".into(), 1);
    assert!(state.context_key(&second, "main").is_err());
}
#[test]
fn old_owner_and_moving_workspaces_cannot_change_membership() {
    let mut registry = Registry::default();
    let view = registry
        .open(server("a").view(), owner(), false, "open")
        .unwrap();
    let mut stale_owner = owner();
    stale_owner.generation += 1;
    assert!(registry.owned_mut(&view.id, &stale_owner, 0).is_err());
    assert!(registry.owned_mut(&view.id, &owner(), 1).is_err());
    registry.entries.get_mut(&view.id).unwrap().view.lifecycle = "moving".into();
    assert!(registry.owned_mut(&view.id, &owner(), 0).is_err());
    assert!(registry
        .open(server("a").view(), owner(), true, "new")
        .is_err());
    assert_eq!(
        registry
            .open(server("a").view(), owner(), false, "enter")
            .unwrap()
            .id,
        view.id
    );
    assert_eq!(registry.entries[&view.id].view.terminals.len(), 1);
}

#[test]
fn operation_lease_blocks_lifecycle_and_old_capabilities_cannot_acquire() {
    let state = state();
    let owner = state.owner("main");
    let view = state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .open(server("a").view(), owner.clone(), false, "open")
        .unwrap();
    let context = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let w = r.entries.get_mut(&view.id).unwrap();
        install_context(w, mock_context(server("a")));
        w.view.tools.context.clone().unwrap()
    };
    let lease = state.workspaces.lease(&context, &owner).unwrap();
    assert!(state.workspaces.registry.lock().unwrap().entries[&view.id]
        .ensure_idle()
        .is_err());
    drop(lease);
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.entries.get_mut(&view.id).unwrap();
    w.ensure_idle().unwrap();
    w.view.lifecycle = "moving".into();
    drop(r);
    assert!(state.workspaces.lease(&context, &owner).is_err());
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.entries.get_mut(&view.id).unwrap();
    w.view.lifecycle = "active".into();
    install_context(w, mock_context(server("a")));
    drop(r);
    assert!(state.workspaces.lease(&context, &owner).is_err());
}
