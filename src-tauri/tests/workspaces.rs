use serde_json::json;
use sodeck::{
    workspaces::{ConnectedSession, Layout},
    AppCore,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    path: PathBuf,
    core: Arc<AppCore>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sodeck-workspaces-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let core = AppCore::open(path.clone(), "test".into()).unwrap();
        core.workspace_state().ready("main").unwrap();
        Self { path, core }
    }
    fn open(&self, name: &str) -> sodeck::workspaces::Workspace {
        let c = self.core.change_catalog(self.core.server_catalog()["revision"].as_u64().unwrap(), serde_json::from_value(json!({"op":"saveServer","draft":{"name":name,"host":"same.invalid","port":22,"username":"ops","authentication":{"type":"password","secret":{"action":"clear"}},"route":[]}})).unwrap()).unwrap();
        self.core
            .accept_connected(ConnectedSession {
                server_id: c["selectedId"].as_str().unwrap().into(),
                connection_id: name.into(),
                endpoint: "ops@same.invalid:22".into(),
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn default_return_host_follows_host_creation_even_when_reusing_an_older_window() {
    let f = Fixture::new();
    let w = f.open("host-order");
    let id = &w.context.workspace_id;
    let mut state = f.core.workspace_state();
    state.register("earlier-window").unwrap();
    state.register("later-window").unwrap();
    state.add_host("later-window", id, 0).unwrap();
    state.add_host("earlier-window", id, 0).unwrap();
    state.remove_host("main", id);
    assert_eq!(state.owner(id).as_deref(), Some("later-window"));
    state.close_window("later-window").unwrap();
    assert_eq!(state.owner(id).as_deref(), Some("earlier-window"));
}
#[test]
fn unique_record_binding_and_context_never_follow_active_tab() {
    let f = Fixture::new();
    let a = f.open("A");
    let b = f.open("B");
    let again = f
        .core
        .accept_connected(ConnectedSession {
            server_id: a.context.server_id.clone(),
            connection_id: "ignored".into(),
            endpoint: "ignored".into(),
        })
        .unwrap();
    assert_eq!(a.context, again.context);
    let mut state = f.core.workspace_state();
    assert_eq!(state.projection("main").unwrap().workspaces.len(), 2);
    state
        .activate("main", Some(&b.context.workspace_id))
        .unwrap();
    state.validate_context(&a.context).unwrap();
    let mut wrong = a.context.clone();
    wrong.server_id = b.context.server_id;
    assert!(state.validate_context(&wrong).is_err());
}

#[test]
fn resizing_preserves_each_other_side_and_commits_preferences() {
    let f = Fixture::new();
    let a = f.open("A");
    let aid = &a.context.workspace_id;
    f.core.workspace_resize("main", aid, "right", 360).unwrap();
    let b = f.open("B");
    f.core
        .workspace_resize("main", &b.context.workspace_id, "right", 400)
        .unwrap();
    let result = f.core.workspace_resize("main", aid, "left", 340).unwrap();
    assert_eq!(result.layout.left_width, 340);
    assert_eq!(result.layout.right_width, 360);
    assert_eq!(
        result.revision,
        f.core.workspace_projection("main").unwrap().revision
    );
    let defaults: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.path.join("ui-layout.json")).unwrap()).unwrap();
    assert_eq!(defaults["leftWidth"], 340);
    assert_eq!(defaults["rightWidth"], 400);
    let c = f.open("C");
    assert_eq!(c.layout.left_width, 340);
    assert_eq!(c.layout.right_width, 400);
}

#[test]
fn rejected_or_failed_resize_changes_neither_memory_nor_saved_defaults() {
    let f = Fixture::new();
    let a = f.open("A");
    let id = &a.context.workspace_id;
    f.core.workspace_resize("main", id, "left", 300).unwrap();
    let saved = std::fs::read(f.path.join("ui-layout.json")).unwrap();
    let before = f.core.workspace_projection("main").unwrap();
    std::fs::create_dir(f.path.join("ui-layout.json.tmp")).unwrap();
    assert!(f.core.workspace_resize("main", id, "left", 350).is_err());
    assert_eq!(std::fs::read(f.path.join("ui-layout.json")).unwrap(), saved);
    let after = f.core.workspace_projection("main").unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.workspaces[0].layout, before.workspaces[0].layout);
    assert!(f.core.workspace_resize("main", id, "middle", 350).is_err());
    assert!(f.core.workspace_resize("main", id, "left", 159).is_err());
    let movement = f
        .core
        .prepare_workspace_move("main", id, "secondary", 0, "resize-move".into(), true)
        .unwrap();
    assert!(f
        .core
        .workspace_resize("main", id, "left", 350)
        .unwrap_err()
        .contains("移动"));
    assert_eq!(std::fs::read(f.path.join("ui-layout.json")).unwrap(), saved);
    f.core.workspace_state().cancel(&movement);
    f.core.register_close_host("main").unwrap();
    f.core
        .close_prepare(
            "main",
            "resize-close",
            sodeck::lifecycle::Target::Workspace { id: id.clone() },
        )
        .unwrap();
    assert!(f
        .core
        .workspace_resize("main", id, "left", 350)
        .unwrap_err()
        .contains("关闭"));
    assert_eq!(std::fs::read(f.path.join("ui-layout.json")).unwrap(), saved);
}

#[test]
fn move_commit_is_atomic_and_preserves_context_and_layout() {
    let f = Fixture::new();
    let a = f.open("A");
    let b = f.open("B");
    let mut state = f.core.workspace_state();
    let layout = Layout {
        left_open: false,
        right_open: true,
        left_width: 300,
        right_width: 260,
    };
    state
        .layout("main", &a.context.workspace_id, layout.clone())
        .unwrap();
    state.register("workspace-test").unwrap();
    let m = state
        .prepare(
            "main",
            &a.context.workspace_id,
            "workspace-test",
            0,
            "move1".into(),
        )
        .unwrap();
    assert!(state.commit(&m).is_err());
    assert_eq!(state.projection("main").unwrap().workspaces.len(), 2);
    assert!(state
        .projection("workspace-test")
        .unwrap()
        .incoming
        .is_some());
    assert!(state
        .prepare("main", &a.context.workspace_id, "main", 0, "repeat".into())
        .is_err());
    assert!(state.close("main", &a.context.workspace_id).is_err());
    state.ready("workspace-test").unwrap();
    state.commit(&m).unwrap();
    let moved = &state.projection("workspace-test").unwrap().workspaces[0];
    assert_eq!(moved.context, a.context);
    assert_eq!(moved.layout, layout);
    assert_eq!(moved.view_ids, a.view_ids);
    assert_eq!(
        state
            .projection("main")
            .unwrap()
            .window
            .active_workspace
            .as_deref(),
        Some(b.context.workspace_id.as_str())
    );
    assert!(state
        .layout("main", &a.context.workspace_id, Layout::default())
        .is_err());
    let back = state
        .prepare(
            "workspace-test",
            &a.context.workspace_id,
            "main",
            0,
            "back".into(),
        )
        .unwrap();
    state.commit(&back).unwrap();
    assert!(state.empty_secondary("workspace-test"));
    state.close_window("workspace-test").unwrap();
    assert_eq!(
        state.projection("main").unwrap().window.workspace_ids[0],
        a.context.workspace_id
    );
}

#[test]
fn cancelled_failed_or_lost_target_does_not_remove_source() {
    let f = Fixture::new();
    let a = f.open("A");
    let mut state = f.core.workspace_state();
    state.register("workspace-fail").unwrap();
    let m = state
        .prepare(
            "main",
            &a.context.workspace_id,
            "workspace-fail",
            0,
            "one".into(),
        )
        .unwrap();
    state.cancel(&m);
    assert!(state.commit(&m).is_err());
    let m = state
        .prepare(
            "main",
            &a.context.workspace_id,
            "workspace-fail",
            0,
            "two".into(),
        )
        .unwrap();
    state.lost_window("workspace-fail");
    assert!(state.commit(&m).is_err());
    assert_eq!(
        state.projection("main").unwrap().workspaces[0].context,
        a.context
    );
    assert!(state
        .prepare(
            "main",
            &a.context.workspace_id,
            "missing",
            0,
            "three".into()
        )
        .is_err());
}

#[test]
fn reorder_close_window_and_catalog_lifetimes_are_separate() {
    let f = Fixture::new();
    let a = f.open("A");
    let b = f.open("B");
    let c = f.open("C");
    let mut state = f.core.workspace_state();
    let m = state
        .prepare("main", &a.context.workspace_id, "main", 2, "reorder".into())
        .unwrap();
    state.commit(&m).unwrap();
    assert_eq!(
        state.projection("main").unwrap().window.workspace_ids,
        vec![
            b.context.workspace_id.clone(),
            c.context.workspace_id,
            a.context.workspace_id.clone()
        ]
    );
    state.register("workspace-close").unwrap();
    state.ready("workspace-close").unwrap();
    let m = state
        .prepare(
            "main",
            &a.context.workspace_id,
            "workspace-close",
            0,
            "out".into(),
        )
        .unwrap();
    state.commit(&m).unwrap();
    state.close_window("workspace-close").unwrap();
    assert!(state.validate_context(&a.context).is_err());
    state.validate_context(&b.context).unwrap();
    assert!(state.close_window("main").is_err());
    drop(state);
    assert_eq!(
        f.core.server_catalog()["servers"].as_array().unwrap().len(),
        3
    );
}

#[test]
fn old_session_results_rejected_after_disconnect_reconnect_and_close() {
    let f = Fixture::new();
    let a = f.open("A");
    let mut state = f.core.workspace_state();
    state.disconnected(&a.context).unwrap();
    assert_eq!(state.projection("main").unwrap().workspaces.len(), 1);
    assert!(state.validate_context(&a.context).is_err());
    let next = state
        .reconnect(
            &a.context,
            "session-next".into(),
            "ops@new.invalid:22".into(),
        )
        .unwrap();
    assert_eq!(next.generation, 2);
    assert!(state.validate_context(&a.context).is_err());
    state.validate_context(&next).unwrap();
    state.close("main", &next.workspace_id).unwrap();
    assert!(state.validate_context(&next).is_err());
    assert!(state
        .projection("main")
        .unwrap()
        .window
        .active_workspace
        .is_none());
}

#[test]
fn catalog_edit_renames_without_rerouting_and_blocks_delete() {
    let f = Fixture::new();
    let a = f.open("A");
    let change = |v| {
        f.core.change_catalog(
            f.core.server_catalog()["revision"].as_u64().unwrap(),
            serde_json::from_value(v).unwrap(),
        )
    };
    assert!(
        change(json!({"op":"deleteServer","id":a.context.server_id}))
            .unwrap_err()
            .contains("工作区")
    );
    change(json!({"op":"saveServer","draft":{"id":a.context.server_id,"name":"Renamed","host":"next.invalid","port":2222,"username":"next","authentication":{"type":"password","secret":{"action":"clear"}},"route":[]}})).unwrap();
    let p = f.core.workspace_projection("main").unwrap();
    assert_eq!(p.workspaces[0].name, "Renamed");
    assert_eq!(p.workspaces[0].endpoint, a.endpoint);
    assert_eq!(p.workspaces[0].context, a.context);
    f.core
        .workspace_state()
        .close("main", &a.context.workspace_id)
        .unwrap();
    change(json!({"op":"deleteServer","id":a.context.server_id})).unwrap();
    assert!(!std::fs::read_to_string(f.path.join("servers.json"))
        .unwrap()
        .contains("connectionId"));
}

#[test]
fn native_hit_testing_uses_strip_bounds_including_invalid_and_scaled_points() {
    use sodeck::windows::{hit_surface, DropTarget, Surface, TabRect};
    let s = Surface {
        x: 150.,
        y: 0.,
        width: 500.,
        height: 44.,
        groups: vec![],
        docks: vec![],
        tabs: vec![
            TabRect {
                id: "A".into(),
                center: 200.,
            },
            TabRect {
                id: "B".into(),
                center: 350.,
            },
        ],
        blocked: false,
    };
    for scale in [1., 1.25, 1.5, 2.] {
        // Negative physical monitor origin, converted once into this webview's logical units.
        let physical = -1920. + 400. * scale;
        let local = (physical - -1920.) / scale;
        assert_eq!(
            hit_surface("main", &s, local, 20., "A"),
            DropTarget::Strip {
                window_id: "main".into(),
                index: 1
            }
        );
    }
    for (x, y) in [(100., 20.), (300., 44.), (700., 10.)] {
        assert_eq!(hit_surface("main", &s, x, y, "A"), DropTarget::Invalid);
    }
    let blocked = Surface { blocked: true, ..s };
    assert_eq!(
        hit_surface("main", &blocked, 300., 20., "A"),
        DropTarget::Invalid
    );
}

#[test]
fn tool_drops_match_only_the_same_workspace_and_valid_sidebar() {
    use sodeck::windows::{hit_dock, DockRect, DropTarget};
    let docks = vec![DockRect {
        workspace_id: "A".into(),
        side: "left".into(),
        x: 0.,
        y: 44.,
        width: 262.,
        height: 600.,
        tools: vec![sodeck::windows::ToolRect {
            key: "test:later".into(),
            center: 250.,
        }],
    }];
    assert_eq!(
        hit_dock("child", &docks, 200., 200., "A"),
        DropTarget::Dock {
            window_id: "child".into(),
            workspace_id: "A".into(),
            side: "left".into(),
            before_tool_key: Some("test:later".into()),
        }
    );
    for (x, y, workspace) in [
        (200., 200., "B"),
        (262., 200., "A"),
        (0., 43., "A"),
        (f64::NAN, 200., "A"),
    ] {
        assert_eq!(
            hit_dock("child", &docks, x, y, workspace),
            DropTarget::Invalid
        );
    }
}

#[test]
fn scrolling_group_drop_excludes_controls_and_uses_current_centers() {
    use sodeck::windows::{hit_group, DropTarget, GroupRect};
    let mut group: GroupRect = serde_json::from_value(serde_json::json!({
        "workspaceId":"w", "groupId":"g", "x":20., "y":50., "width":280., "height":400.,
        "tabs":[{"id":"a","center":30.},{"id":"b","center":130.},{"id":"c","center":230.}],
        "tabViewport":{"x":25.,"y":50.,"width":160.,"height":36.}
    }))
    .unwrap();
    for scale in [1., 1.25, 1.5, 2.] {
        let physical = -1920. + 180. * scale;
        let x = (physical + 1920.) / scale;
        assert_eq!(
            hit_group("target", &[group.clone()], x, 70., "w", Some("a")),
            DropTarget::Group {
                window_id: "target".into(),
                group_id: "g".into(),
                edge: "tab".into(),
                index: 1
            }
        );
    }
    for x in [20., 185., 220., 290.] {
        assert_eq!(
            hit_group("target", &[group.clone()], x, 70., "w", None),
            DropTarget::Invalid
        );
    }
    group.tabs.iter_mut().for_each(|t| t.center -= 100.);
    assert_eq!(
        hit_group("target", &[group.clone()], 180., 70., "w", Some("a")),
        DropTarget::Group {
            window_id: "target".into(),
            group_id: "g".into(),
            edge: "tab".into(),
            index: 2
        }
    );
    group.tab_viewport = None;
    assert!(
        matches!(
            hit_group("target", &[group], 250., 70., "w", None),
            DropTarget::Group { .. }
        ),
        "Legacy fixtures retain the original hit area"
    );
}
