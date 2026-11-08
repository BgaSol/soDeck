//! Admission and completion for workers that own native resources.
//! Closing seals admission; an acknowledgement never stands in for worker completion.
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct Tasks(Mutex<State>);
#[derive(Default)]
struct State {
    closing: bool,
    active: usize,
}
pub(crate) struct Task(Arc<Tasks>);
impl Tasks {
    pub(crate) fn enter(self: &Arc<Self>) -> Result<Task, String> {
        let mut state = self.0.lock().unwrap();
        if state.closing {
            return Err("资源正在关闭".into());
        }
        state.active += 1;
        Ok(Task(self.clone()))
    }
    pub(crate) fn close(&self) {
        self.0.lock().unwrap().closing = true;
    }
    pub(crate) fn finished(&self) -> bool {
        let state = self.0.lock().unwrap();
        state.closing && state.active == 0
    }
    pub(crate) fn active(&self) -> usize {
        self.0.lock().unwrap().active
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.0 .0.lock().unwrap().active -= 1;
    }
}

/// Observes the transport owned by a library's detached reader/writer tasks.
pub(crate) struct TrackedStream<S> {
    pub(crate) stream: S,
    pub(crate) _task: Task,
}
impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for TrackedStream<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for TrackedStream<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_does_not_hide_running_workers_or_accept_late_children() {
        let tasks = Arc::new(Tasks::default());
        let parent = tasks.enter().unwrap();
        let child = tasks.enter().unwrap();
        tasks.close();
        assert!(tasks.enter().is_err());
        assert!(!tasks.finished());
        drop(parent);
        assert!(!tasks.finished());
        drop(child);
        assert!(tasks.finished());
    }
}
