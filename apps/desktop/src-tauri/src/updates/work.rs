use super::domain;
use std::sync::{Arc, Condvar, Mutex};

// Register before spawning. Closing rejects new producers, including tasks
// waiting for a download permit, so a free semaphore alone is not completion.
#[derive(Default)]
pub(super) struct Work {
    state: Mutex<(bool, usize)>,
    completed: Condvar,
}
pub(super) struct Guard(Arc<Work>);
impl Work {
    pub fn enter(self: &Arc<Self>) -> domain::Result<Guard> {
        let mut state = self.state.lock().unwrap();
        if state.0 { return Err(domain::error("SHUTTING_DOWN", "更新服务正在退出")); }
        state.1 += 1;
        Ok(Guard(self.clone()))
    }
    pub fn close_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        while state.1 != 0 { state = self.completed.wait(state).unwrap(); }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.1 -= 1;
        if state.1 == 0 { self.0.completed.notify_all(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_waits_for_queued_producers_and_rejects_new_work() {
        let work = Arc::new(Work::default());
        let first = work.enter().unwrap();
        let queued = work.enter().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let closing = work.clone();
        let thread = std::thread::spawn(move || { closing.close_and_wait(); tx.send(()).unwrap(); });
        while !work.state.lock().unwrap().0 { std::thread::yield_now(); }
        assert!(work.enter().is_err());
        drop(first);
        assert!(rx.try_recv().is_err());
        drop(queued);
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
    }
}
