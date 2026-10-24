use std::{collections::HashSet, sync::{Condvar, Mutex}};

#[derive(Default)]
struct Pending { active: HashSet<String>, closes: HashSet<String> }

/// Moves and closes share one gate per logical view. A native close reserves
/// the next turn, so repeated clicks cannot race or be silently discarded.
#[derive(Default)]
pub struct ViewOperations { pending: Mutex<Pending>, changed: Condvar }

pub struct Operation<'a> { owner: &'a ViewOperations, id: String, queued_close: bool }
impl ViewOperations {
    pub fn busy(&self, id: &str) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.active.contains(id) || pending.closes.contains(id)
    }
    pub fn begin(&self, id: &str) -> Option<Operation<'_>> {
        let mut pending = self.pending.lock().unwrap();
        if pending.closes.contains(id) || !pending.active.insert(id.into()) { return None; }
        Some(Operation { owner: self, id: id.into(), queued_close: false })
    }
    pub fn queue_close(&self, id: &str) -> bool { self.pending.lock().unwrap().closes.insert(id.into()) }
    pub fn wait_close(&self, id: &str) -> Operation<'_> {
        let pending = self.pending.lock().unwrap();
        let mut pending = self.changed.wait_while(pending, |pending| pending.active.contains(id)).unwrap();
        pending.active.insert(id.into());
        Operation { owner: self, id: id.into(), queued_close: true }
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        let mut pending = self.owner.pending.lock().unwrap();
        pending.active.remove(&self.id);
        if self.queued_close { pending.closes.remove(&self.id); }
        self.owner.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::{Arc, mpsc}, time::Duration};
    #[test]
    fn native_close_waits_for_move_and_reserves_next_turn_once() {
        let operations = Arc::new(ViewOperations::default());
        let moving = operations.begin("view").unwrap();
        assert!(operations.queue_close("view"));
        assert!(!operations.queue_close("view"));
        assert!(operations.begin("view").is_none());
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker = operations.clone();
        let closing = std::thread::spawn(move || {
            let _operation = worker.wait_close("view");
            started_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
        });
        assert!(started_rx.recv_timeout(Duration::from_millis(30)).is_err());
        drop(moving);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(operations.begin("view").is_none());
        assert!(operations.begin("peer").is_some());
        finish_tx.send(()).unwrap();
        closing.join().unwrap();
        assert!(!operations.busy("view"));
        assert!(operations.begin("view").is_some());
    }
    #[test]
    fn queued_close_blocks_new_move_even_before_close_thread_starts() {
        let operations = ViewOperations::default();
        assert!(operations.queue_close("view"));
        assert!(operations.begin("view").is_none());
        drop(operations.wait_close("view"));
        assert!(operations.begin("view").is_some());
    }
}
