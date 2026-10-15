use crate::model::AppResult;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct PathLocks {
    active: Mutex<Vec<(String, Claim, String)>>,
    wake: Condvar,
}
#[derive(Clone)]
pub struct Claim {
    pub key: String,
    pub path: String,
    pub write: bool,
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
        check: impl FnMut(Option<(&str, &str)>) -> AppResult<()>,
    ) -> AppResult<PathGuard<'_>> {
        self.acquire_many(
            &[Claim {
                key: key.into(),
                path: path.into(),
                write: true,
            }],
            task,
            check,
        )
    }
    // All claims enter together. Never hold one host/path while waiting for another.
    // Nested publish claims from the same task are reentrant.
    pub fn acquire_many(
        &self,
        claims: &[Claim],
        task: &str,
        mut check: impl FnMut(Option<(&str, &str)>) -> AppResult<()>,
    ) -> AppResult<PathGuard<'_>> {
        let mut active = self.active.lock().unwrap();
        loop {
            let conflict = active.iter().find(|(_, a, t)| {
                t != task
                    && claims.iter().any(|c| {
                        c.key == a.key && (a.write || c.write) && overlaps(&a.path, &c.path)
                    })
            });
            check(conflict.map(|(_, a, t)| (a.path.as_str(), t.as_str())))?;
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
        active.extend(
            claims
                .iter()
                .cloned()
                .map(|c| (token.clone(), c, task.into())),
        );
        Ok(PathGuard { locks: self, token })
    }
}
impl Drop for PathGuard<'_> {
    fn drop(&mut self) {
        self.locks
            .active
            .lock()
            .unwrap()
            .retain(|(t, _, _)| t != &self.token);
        self.locks.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_claims_share_and_opposite_moves_acquire_atomically() {
        let locks = std::sync::Arc::new(PathLocks::default());
        let claim = |path: &str, write| Claim {
            key: "server".into(),
            path: path.into(),
            write,
        };
        let a = locks
            .acquire_many(&[claim("/a", false)], "reader1", |_| Ok(()))
            .unwrap();
        let b = locks
            .acquire_many(&[claim("/a/file", false)], "reader2", |_| Ok(()))
            .unwrap();
        let result = locks.acquire_many(
            &[claim("/a", true), claim("/b", true)],
            "move",
            |conflict| {
                if conflict.is_some() {
                    Err(crate::model::AppError::new("busy", "expected"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        drop(a);
        drop(b);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = vec![];
        for (id, first, second) in [("move1", "/a", "/b"), ("move2", "/b", "/a")] {
            let locks = locks.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                let _guard = locks
                    .acquire_many(&[claim(first, true), claim(second, true)], id, |_| {
                        if std::time::Instant::now() > deadline {
                            Err(crate::model::AppError::new("timeout", "deadlock"))
                        } else {
                            Ok(())
                        }
                    })
                    .unwrap();
                let _nested = locks.acquire("server", first, id, |_| Ok(())).unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
