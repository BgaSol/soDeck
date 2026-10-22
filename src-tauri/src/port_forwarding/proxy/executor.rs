use crate::model::AppResult;
use std::{sync::Mutex, thread};
use tokio::{runtime::Handle, sync::oneshot};

pub struct Executor {
    pub handle: Handle,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Executor {
    pub fn new() -> AppResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let handle = runtime.handle().clone();
        let (send, receive) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("network-proxy".into())
            .spawn(move || {
                runtime.block_on(async {
                    let _ = receive.await;
                });
                runtime.shutdown_timeout(std::time::Duration::from_secs(1));
            })?;
        Ok(Self {
            handle,
            shutdown: Mutex::new(Some(send)),
            thread: Mutex::new(Some(thread)),
        })
    }
    pub fn shutdown(&self) {
        if let Some(send) = self.shutdown.lock().unwrap().take() {
            let _ = send.send(());
        }
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Executor {
    fn drop(&mut self) {
        self.shutdown();
    }
}
