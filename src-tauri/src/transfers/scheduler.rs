use crate::model::AppResult;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct PathLocks {
    active: Mutex<Vec<(String, String, String, String)>>,
    wake: Condvar,
}
pub struct PathGuard<'a> {
    locks: &'a PathLocks,
    token: String,
}
pub fn overlaps(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    a == b
        || a.strip_prefix(b).is_some_and(|s| s.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|s| s.starts_with('/'))
}
impl PathLocks {
    pub fn acquire(
        &self,
        key: &str,
        path: &str,
        task: &str,
        mut check: impl FnMut(Option<(&str, &str)>) -> AppResult<()>,
    ) -> AppResult<PathGuard<'_>> {
        let mut active = self.active.lock().unwrap();
        loop {
            let conflict = active
                .iter()
                .find(|(_, k, p, _)| k == key && overlaps(p, path));
            check(conflict.map(|(_, _, p, t)| (p.as_str(), t.as_str())))?;
            if conflict.is_none() {
                break;
            }
            active = self
                .wake
                .wait_timeout(active, Duration::from_millis(100))
                .unwrap()
                .0;
        }
        let token = uuid::Uuid::new_v4().to_string();
        active.push((token.clone(), key.into(), path.into(), task.into()));
        Ok(PathGuard { locks: self, token })
    }
}
impl Drop for PathGuard<'_> {
    fn drop(&mut self) {
        self.locks
            .active
            .lock()
            .unwrap()
            .retain(|(t, _, _, _)| t != &self.token);
        self.locks.wake.notify_all();
    }
}
