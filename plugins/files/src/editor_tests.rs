use super::*;

#[test]
fn saved_snapshot_cannot_clear_a_new_unstaged_edit() {
    let mut view = View {
        id: "file".into(),
        title: "file".into(),
        path: "/file".into(),
        dirty: false,
        busy: false,
        revision: 3,
        saved: 3,
        edit_revision: 4,
        staged_edit_revision: 3,
        editor_state: serde_json::Value::Null,
        cursor: 0,
        scroll: 0,
        navigation: serde_json::Value::Null,
        context: serde_json::Value::Null,
        editor: true,
    };
    view.update_dirty();
    assert!(view.dirty);
    assert!(view.require_staged().is_err());
    view.revision = 4;
    view.staged_edit_revision = 4;
    view.update_dirty();
    assert!(view.dirty);
    view.saved = 4;
    view.update_dirty();
    assert!(!view.dirty);
}

#[test]
fn editor_state_is_bounded_and_validated_before_restore() {
    let mut state = json!({"version":1,"anchor":0,"active":12,"topLine":2,"topOffset":4.5,"left":0,"language":"auto","search":{"open":true,"query":"配置","matchCase":false}});
    validate_editor_state(&state).unwrap();
    state["search"]["query"] = json!("🙂".repeat(513));
    assert!(validate_editor_state(&state).is_err());
    state["search"]["query"] = json!("line\nnext");
    assert!(validate_editor_state(&state).is_err());
    state["search"]["query"] = json!("valid");
    state["topOffset"] = json!("bad");
    assert!(validate_editor_state(&state).is_err());
}
