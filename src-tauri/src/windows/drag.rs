use super::{DropTarget, LocalPoint};
use serde::Serialize;

#[derive(Clone)]
pub(super) struct Drag {
    pub token: String,
    pub source: String,
    pub workspace: String,
    pub offset: (f64, f64),
    pub scope: crate::workbench_move::Scope,
    pub revision: u64,
    pub context: crate::workspaces::Context,
    pub origin: (i32, i32),
    pub scale: f64,
    pub released: Option<Released>,
}
#[derive(Clone)]
pub(super) struct Released {
    pub position: Position,
    pub target: DropTarget,
    pub end: DragEvent,
}
#[derive(Clone)]
pub(super) enum Position {
    Global {
        x: f64,
        y: f64,
    },
    #[cfg(target_os = "linux")]
    Local(LocalPoint),
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DragEvent {
    Update {
        revision: u64,
        token: String,
        target: DropTarget,
        point: Option<LocalPoint>,
    },
    End {
        revision: u64,
        token: String,
    },
}
#[derive(Default)]
pub(super) struct DragState {
    current: Option<Drag>,
    revision: u64,
}
impl DragState {
    pub fn start(&mut self, drag: Drag) -> Result<(), String> {
        if self.current.is_some() {
            return Err("已有 Tab 正在拖动".into());
        }
        self.current = Some(drag);
        Ok(())
    }
    pub fn get(&self, token: &str, source: &str) -> Option<&Drag> {
        self.current
            .as_ref()
            .filter(|d| d.token == token && d.source == source)
    }
    pub fn update(
        &mut self,
        token: &str,
        source: &str,
        target: DropTarget,
        point: Option<LocalPoint>,
    ) -> Option<DragEvent> {
        if self.get(token, source)?.released.is_some() {
            return None;
        }
        self.revision += 1;
        Some(DragEvent::Update {
            revision: self.revision,
            token: token.into(),
            target,
            point,
        })
    }
    pub fn release(
        &mut self,
        token: &str,
        source: &str,
        position: Position,
        target: DropTarget,
    ) -> Result<DragEvent, String> {
        let drag = self.get(token, source).ok_or("拖动已取消")?;
        if let Some(released) = &drag.released {
            return Ok(released.end.clone());
        }
        self.revision += 1;
        let end = DragEvent::End {
            revision: self.revision,
            token: token.into(),
        };
        self.current.as_mut().unwrap().released = Some(Released {
            position,
            target,
            end: end.clone(),
        });
        Ok(end)
    }
    pub fn take(
        &mut self,
        token: &str,
        source: &str,
        cancel: bool,
    ) -> Result<(Option<Drag>, Option<DragEvent>), String> {
        let Some(drag) = self.get(token, source) else {
            return if cancel {
                Ok((None, None))
            } else {
                Err("拖动已取消".into())
            };
        };
        if !cancel && drag.released.is_none() {
            return Err("尚未释放拖动".into());
        }
        let drag = self.current.take().unwrap();
        let end = if let Some(released) = &drag.released {
            Some(released.end.clone())
        } else {
            self.revision += 1;
            Some(DragEvent::End {
                revision: self.revision,
                token: token.into(),
            })
        };
        Ok((Some(drag), end))
    }
    pub fn cancel_source(&mut self, source: &str) -> Option<DragEvent> {
        let token = self
            .current
            .as_ref()
            .filter(|d| d.source == source)?
            .token
            .clone();
        self.take(&token, source, true).ok()?.1
    }
}

pub(super) fn same_destination(before: &DropTarget, after: &DropTarget) -> bool {
    match (before, after) {
        (DropTarget::Strip { window_id: a, .. }, DropTarget::Strip { window_id: b, .. }) => a == b,
        (
            DropTarget::Group {
                window_id: a,
                group_id: ag,
                edge: ae,
                ..
            },
            DropTarget::Group {
                window_id: b,
                group_id: bg,
                edge: be,
                ..
            },
        ) => a == b && ag == bg && ae == be,
        (
            DropTarget::Dock {
                window_id: a,
                workspace_id: aw,
                side: aside,
                ..
            },
            DropTarget::Dock {
                window_id: b,
                workspace_id: bw,
                side: bside,
                ..
            },
        ) => a == b && aw == bw && aside == bside,
        (DropTarget::Outside { .. }, DropTarget::Outside { .. })
        | (DropTarget::Invalid, DropTarget::Invalid) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drag(token: &str) -> Drag {
        Drag {
            token: token.into(),
            source: "main".into(),
            workspace: "w".into(),
            offset: (0., 0.),
            scope: crate::workbench_move::Scope::Host,
            revision: 1,
            context: crate::workspaces::Context {
                workspace_id: "w".into(),
                server_id: "s".into(),
                connection_id: "c".into(),
                generation: 1,
            },
            origin: (0, 0),
            scale: 1.,
            released: None,
        }
    }
    fn revision(event: &DragEvent) -> u64 {
        match event {
            DragEvent::Update { revision, .. } | DragEvent::End { revision, .. } => *revision,
        }
    }
    #[test]
    fn release_freezes_once_and_late_samples_cannot_follow_end() {
        let mut state = DragState::default();
        state.start(drag("a")).unwrap();
        let update = state
            .update("a", "main", DropTarget::Invalid, None)
            .unwrap();
        let end = state
            .release(
                "a",
                "main",
                Position::Global { x: 10., y: 20. },
                DropTarget::Invalid,
            )
            .unwrap();
        assert!(revision(&end) > revision(&update));
        assert!(state
            .update("a", "main", DropTarget::Invalid, None)
            .is_none());
        let again = state
            .release(
                "a",
                "main",
                Position::Global { x: 100., y: 200. },
                DropTarget::Invalid,
            )
            .unwrap();
        assert_eq!(revision(&end), revision(&again));
        assert!(matches!(
            state
                .get("a", "main")
                .unwrap()
                .released
                .as_ref()
                .unwrap()
                .position,
            Position::Global { x: 10., y: 20. }
        ));
    }
    #[test]
    fn failure_cleanup_and_old_tokens_do_not_block_or_cancel_the_next_drag() {
        let mut state = DragState::default();
        state.start(drag("a")).unwrap();
        assert!(state.take("a", "main", false).is_err());
        assert!(state.cancel_source("other").is_none());
        let end = state.cancel_source("main").unwrap();
        state.start(drag("b")).unwrap();
        assert!(state.take("a", "main", true).unwrap().0.is_none());
        assert!(state.take("b", "other", true).unwrap().0.is_none());
        assert!(
            revision(
                &state
                    .update("b", "main", DropTarget::Invalid, None)
                    .unwrap()
            ) > revision(&end)
        );
        assert!(state.take("b", "main", true).unwrap().0.is_some());
        assert!(state.take("b", "main", false).is_err());
    }
    #[test]
    fn released_prepare_failure_retires_the_same_revision_and_accepts_a_new_drag() {
        let mut state = DragState::default();
        state.start(drag("a")).unwrap();
        let end = state
            .release(
                "a",
                "main",
                Position::Global { x: 1., y: 2. },
                DropTarget::Invalid,
            )
            .unwrap();
        let (_, cancelled) = state.take("a", "main", true).unwrap();
        assert_eq!(revision(&end), revision(&cancelled.unwrap()));
        state.start(drag("b")).unwrap();
        assert!(state
            .update("a", "main", DropTarget::Invalid, None)
            .is_none());
        assert!(state.get("b", "main").is_some());
    }
    #[test]
    fn settling_can_reorder_inside_the_destination_but_cannot_switch_or_detach() {
        let a = DropTarget::Strip {
            window_id: "main".into(),
            index: 0,
        };
        let b = DropTarget::Strip {
            window_id: "main".into(),
            index: 3,
        };
        assert!(same_destination(&a, &b));
        assert!(!same_destination(&a, &DropTarget::Outside { x: 1., y: 1. }));
        assert!(!same_destination(
            &a,
            &DropTarget::Strip {
                window_id: "other".into(),
                index: 0
            }
        ));
    }
}
