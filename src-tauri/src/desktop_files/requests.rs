//! Linux OS requests keep their slot after the business receiver has gone away.
use super::Clipboard;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::sync::oneshot;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub(super) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub(super) struct Request {
    pub revision: String,
    pub sender: Option<oneshot::Sender<Result<Clipboard, String>>>,
}
impl Request {
    pub fn active(&self) -> bool {
        self.sender.as_ref().is_some_and(|s| !s.is_closed())
    }
    pub fn finish(mut self, result: Result<Clipboard, String>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
    }
}

#[derive(Default)]
pub(super) struct Pending(HashMap<u64, Request>);
impl Pending {
    pub fn insert(&mut self, id: u64, request: Request) -> Result<(), Request> {
        if self.0.len() >= 4 {
            return Err(request);
        }
        self.0.insert(id, request);
        Ok(())
    }
    pub fn cancel(&mut self, id: u64) {
        if let Some(request) = self.0.get_mut(&id) {
            request.sender.take();
        }
    }
    pub fn take(&mut self, id: u64) -> Option<Request> {
        self.0.remove(&id)
    }
    pub fn shutdown(&mut self) {
        for (_, request) in self.0.drain() {
            request.finish(Err("文件剪贴板读取已结束".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> (Request, oneshot::Receiver<Result<Clipboard, String>>) {
        let (sender, receiver) = oneshot::channel();
        (
            Request {
                revision: "1".into(),
                sender: Some(sender),
            },
            receiver,
        )
    }
    #[test]
    fn cancelled_and_timed_out_os_requests_still_bound_new_reads() {
        let mut pending = Pending::default();
        for id in 0..4 {
            let (request, receiver) = request();
            assert!(pending.insert(id, request).is_ok());
            if id % 2 == 0 {
                pending.cancel(id);
            }
            drop(receiver);
        }
        assert!(pending.insert(5, request().0).is_err());
        let expired = pending.take(0).unwrap();
        assert_eq!(expired.revision, "1");
        assert!(!expired.active());
        assert!(pending.insert(5, request().0).is_ok());
        assert!(pending.take(0).is_none());
    }
    #[test]
    fn shutdown_finishes_waiters_and_old_ids_cannot_deliver_again() {
        let mut pending = Pending::default();
        let (request, mut receiver) = request();
        let id = next_id();
        assert!(pending.insert(id, request).is_ok());
        pending.shutdown();
        pending.shutdown();
        assert!(receiver.try_recv().unwrap().is_err());
        assert!(pending.take(id).is_none());
        assert!(next_id() > id);
    }
}
