//! One state lock for workspace identities, view placement and lifecycle reservations.
//! Network workers, plugin RPC and display buffers remain outside this state.
use crate::{component_ui, workspaces, AppCore};
use std::{
    ops::{Deref, DerefMut},
    path::Path,
    sync::MutexGuard,
};

pub struct State {
    pub workspaces: workspaces::Runtime,
    pub ui: component_ui::Runtime,
}
impl State {
    pub fn open(root: &Path) -> Result<Self, String> {
        Ok(Self {
            workspaces: workspaces::Runtime::default(),
            ui: component_ui::Runtime::open(root)?,
        })
    }
}

/// A projection retains the same state lock; it never copies a writable registry.
pub struct PartGuard<'a, T> {
    state: MutexGuard<'a, State>,
    read: fn(&State) -> &T,
    write: fn(&mut State) -> &mut T,
}
impl<T> Deref for PartGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        (self.read)(&self.state)
    }
}
impl<T> DerefMut for PartGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        (self.write)(&mut self.state)
    }
}
impl AppCore {
    pub fn workbench_state(&self) -> MutexGuard<'_, State> {
        self.workbench.lock().unwrap()
    }
    pub fn workspace_state(&self) -> PartGuard<'_, workspaces::Runtime> {
        PartGuard {
            state: self.workbench_state(),
            read: |s| &s.workspaces,
            write: |s| &mut s.workspaces,
        }
    }
    pub fn ui_state(&self) -> PartGuard<'_, component_ui::Runtime> {
        PartGuard {
            state: self.workbench_state(),
            read: |s| &s.ui,
            write: |s| &mut s.ui,
        }
    }
}
