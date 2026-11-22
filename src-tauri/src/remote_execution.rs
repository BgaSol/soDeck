//! One remote channel runner shared by commands, command capture and terminals.
use crate::{execution_output::OutputTarget, resource_owner::Owner, resource_tasks::Task, AppCore};
use russh::{client::Msg, ChannelReadHalf, ChannelWriteHalf};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, Weak,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Notify};
use tokio_util::sync::CancellationToken;

pub(crate) use sodeck_plugin_protocol::session::ExecutionIo;
pub(crate) enum Launch {
    Command(String),
    LoginShell,
}
pub(crate) struct Options {
    pub launch: Launch,
    pub io: ExecutionIo,
    pub started: Instant,
    pub deadline: Option<Instant>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputOperation {
    Write(Arc<[u8]>),
    Eof,
    Resize(u32, u32),
    Barrier,
}
impl InputOperation {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::Eof => "eof",
            Self::Resize(..) => "resize",
            Self::Barrier => "barrier",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) enum InputOutcome {
    Sent(Option<u64>),
    NotSent(String),
    Unconfirmed(String),
}
pub(crate) struct QueuedInput {
    pub operation: InputOperation,
    pub completion: oneshot::Sender<InputOutcome>,
}
#[derive(Clone, Default)]
struct State {
    requested: bool,
    queued: bool,
    accepted: bool,
    ending: bool,
    finished: bool,
    stdin_closed: bool,
    exit_code: Option<u32>,
    exit_signal: Option<String>,
    error: Option<String>,
    elapsed_ms: u64,
}
pub(crate) struct Execution {
    pub owner: Owner,
    state: Mutex<State>,
    input: mpsc::Sender<QueuedInput>,
    pub cancel: CancellationToken,
    stop: CancellationToken,
    input_stop: CancellationToken,
    ready: CancellationToken,
    pub changed: Notify,
    pub worker_done: CancellationToken,
    pub sent: AtomicU64,
    pub received: AtomicU64,
}
impl Execution {
    pub fn create(owner: Owner) -> (Arc<Self>, mpsc::Receiver<QueuedInput>) {
        let (input, rx) = mpsc::channel(32);
        (
            Arc::new(Self {
                owner,
                state: Mutex::new(State::default()),
                input,
                cancel: CancellationToken::new(),
                stop: CancellationToken::new(),
                input_stop: CancellationToken::new(),
                ready: CancellationToken::new(),
                changed: Notify::new(),
                worker_done: CancellationToken::new(),
                sent: AtomicU64::new(0),
                received: AtomicU64::new(0),
            }),
            rx,
        )
    }
    pub fn submit(
        &self,
        operation: InputOperation,
    ) -> Result<oneshot::Receiver<InputOutcome>, String> {
        if matches!(&operation,InputOperation::Write(data) if data.len()>crate::streams::BLOCK) {
            return Err("invalidParams:输入超过 16 KiB".into());
        }
        let state = self.state.lock().unwrap();
        if state.ending
            || state.finished
            || self.cancel.is_cancelled()
            || self.input_stop.is_cancelled()
        {
            return Err("inputClosed:执行已停止接受输入".into());
        }
        let (tx, rx) = oneshot::channel();
        self.input
            .try_send(QueuedInput {
                operation,
                completion: tx,
            })
            .map_err(|_| "resourceBusy:输入队列已满".to_string())?;
        Ok(rx)
    }
    pub fn snapshot(&self) -> Value {
        let s = self.state.lock().unwrap();
        let phase = if s.finished {
            "finished"
        } else if s.ending {
            "finishing"
        } else if s.accepted {
            "running"
        } else {
            "opening"
        };
        let code = s
            .error
            .as_ref()
            .map(|e| e.split(':').next().unwrap_or("commandFailed"));
        let state = if !s.finished {
            if s.accepted {
                "running"
            } else {
                "opening"
            }
        } else if s.error.is_none() {
            "completed"
        } else if code == Some("cancelled") {
            "cancelled"
        } else {
            "failed"
        };
        json!({"state":state,"phase":phase,"execSent":s.requested,"launchQueued":s.queued,"launchAccepted":s.accepted,"remoteExitConfirmed":s.exit_code.is_some()||s.exit_signal.is_some(),"exitCode":s.exit_code,"exitSignal":s.exit_signal,"error":s.error.as_ref().map(|e|json!({"code":code.unwrap(),"message":e})),"finished":s.finished,"workerFinished":self.worker_done.is_cancelled(),"outputComplete":s.finished&&s.error.is_none(),"elapsedMs":s.elapsed_ms,"inputClosed":s.stdin_closed||s.ending})
    }
    fn end_input(&self) {
        self.state.lock().unwrap().ending = true;
        self.input_stop.cancel();
        self.changed.notify_waiters();
    }
    fn stop(&self, error: Option<String>) {
        {
            let mut s = self.state.lock().unwrap();
            s.ending = true;
            if s.error.is_none() {
                s.error = error;
            }
        }
        self.input_stop.cancel();
        self.stop.cancel();
        self.changed.notify_waiters();
    }
    fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }
}

pub(crate) async fn run(
    core: Weak<AppCore>,
    execution: Arc<Execution>,
    mut input: mpsc::Receiver<QueuedInput>,
    session: Arc<crate::connections::Session>,
    options: Options,
    output: OutputTarget,
    guard: Task,
) {
    let parent = session.cancellation();
    let opening_deadline =
        tokio::time::Instant::from_std(options.started + Duration::from_secs(30));
    let deadline = async {
        match options.deadline {
            Some(at) => tokio::time::sleep_until(at.into()).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(deadline);
    let opening = tokio::select! {biased;
        _=execution.cancel.cancelled()=>Err("cancelled:执行已取消".to_string()),
        _=parent.cancelled()=>Err("sessionLost:服务器连接已断开".to_string()),
        _=&mut deadline=>Err("deadlineExceeded:执行期限已到".to_string()),
        r=tokio::time::timeout_at(opening_deadline,session.open_channel())=>r.map_err(|_|"deadlineExceeded:打开通道超时".to_string()).and_then(|r|r),
    };
    let mut close = None;
    match opening {
        Err(error) => execution.stop(Some(error)),
        Ok(channel) => {
            let (reader, writer) = channel.split();
            let writer = Arc::new(writer);
            close = Some(writer.clone());
            let (replies, reply_rx) = mpsc::channel(2);
            let reader_loop = read_loop(
                core.clone(),
                execution.clone(),
                reader,
                replies,
                &output,
                &options.io,
            );
            let writer_loop = write_loop(
                core.clone(),
                execution.clone(),
                writer,
                &mut input,
                reply_rx,
                &options,
            );
            let monitor = async {
                let opening_timeout = async {
                    tokio::select! {_=execution.ready.cancelled()=>std::future::pending::<()>().await,_=tokio::time::sleep_until(opening_deadline)=>{}}
                };
                tokio::select! {biased;
                    _=execution.stop.cancelled()=>{},
                    _=execution.cancel.cancelled()=>execution.stop(Some("cancelled:执行已取消".into())),
                    _=parent.cancelled()=>execution.stop(Some("sessionLost:服务器连接已断开".into())),
                    _=&mut deadline=>execution.stop(Some("deadlineExceeded:执行期限已到".into())),
                    _=opening_timeout=>execution.stop(Some("deadlineExceeded:启动请求超时".into())),
                }
            };
            // Joining rather than dropping the reader preserves real in-flight disk work.
            tokio::join!(reader_loop, writer_loop, monitor);
        }
    }
    execution.end_input();
    input.close();
    while let Ok(item) = input.try_recv() {
        let _ = item
            .completion
            .send(InputOutcome::NotSent("inputClosed:执行已结束".into()));
    }
    let missing_exit = {
        let state = execution.state.lock().unwrap();
        state.exit_code.is_none() && state.exit_signal.is_none()
    };
    if execution.error().is_none() && missing_exit {
        execution.stop(Some("missingExitStatus:远端未返回退出状态".into()));
    }
    if let Some(writer) = close {
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            if execution.error().is_some() {
                let _ = writer.signal(russh::Sig::TERM).await;
            }
            let _ = writer.close().await;
        })
        .await;
    }
    let terminal_result = match execution.error() {
        Some(e) => json!({"error":e}),
        None => json!({"exitCode":execution.state.lock().unwrap().exit_code}),
    };
    if let Err(e) = output.finish(terminal_result) {
        if execution.error().is_none() {
            execution.stop(Some(e));
        }
    }
    {
        let mut s = execution.state.lock().unwrap();
        s.finished = true;
        s.elapsed_ms = options.started.elapsed().as_millis() as u64;
    }
    drop(guard);
    execution.worker_done.cancel();
    execution.changed.notify_waiters();
}

async fn read_loop(
    core: Weak<AppCore>,
    ex: Arc<Execution>,
    mut reader: ChannelReadHalf,
    replies: mpsc::Sender<Result<(), String>>,
    output: &OutputTarget,
    io: &ExecutionIo,
) {
    let mut startup_deadline = None;
    loop {
        let changed = ex.changed.notified();
        if ex.ready.is_cancelled() && !output.ready() && startup_deadline.is_none() {
            startup_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(30));
        }
        let startup_timeout = async {
            match startup_deadline.filter(|_| !output.ready()) {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending::<()>().await,
            }
        };
        let message = tokio::select! {biased;_=ex.stop.cancelled()=>break,_=startup_timeout=>{ex.stop(Some("deadlineExceeded:程序启动结果超时".into()));break;},_=changed=>continue,m=reader.wait()=>m};
        match message {
            Some(russh::ChannelMsg::Success) => {
                if replies.try_send(Ok(())).is_err() {
                    ex.stop(Some("execRejected:多余的启动回复".into()));
                    break;
                }
            }
            Some(russh::ChannelMsg::Failure) => {
                let _ = replies.try_send(Err("execRejected:远端拒绝启动请求".into()));
                ex.stop(Some("execRejected:远端拒绝启动请求".into()));
                break;
            }
            Some(russh::ChannelMsg::Data { data }) => {
                ex.received.fetch_add(data.len() as u64, Ordering::Relaxed);
                if let Some(c) = core.upgrade() {
                    c.record_business(&ex.owner, 0, data.len() as u64);
                }
                if let Err(e) = output.append(false, &data, &ex.stop).await {
                    ex.stop(Some(e));
                    break;
                }
                ex.changed.notify_waiters();
            }
            Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 || io.terminal() => {
                ex.received.fetch_add(data.len() as u64, Ordering::Relaxed);
                if let Some(c) = core.upgrade() {
                    c.record_business(&ex.owner, 0, data.len() as u64);
                }
                if let Err(e) = output.append(true, &data, &ex.stop).await {
                    ex.stop(Some(e));
                    break;
                }
                ex.changed.notify_waiters();
            }
            Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                ex.state.lock().unwrap().exit_code = Some(exit_status);
                ex.end_input();
            }
            Some(russh::ChannelMsg::ExitSignal { signal_name, .. }) => {
                ex.state.lock().unwrap().exit_signal = Some(format!("{signal_name:?}"));
                ex.end_input();
            }
            Some(russh::ChannelMsg::Close) | None => {
                ex.stop(None);
                break;
            }
            _ => {}
        }
    }
}
async fn write_loop(
    core: Weak<AppCore>,
    ex: Arc<Execution>,
    writer: Arc<ChannelWriteHalf<Msg>>,
    input: &mut mpsc::Receiver<QueuedInput>,
    mut replies: mpsc::Receiver<Result<(), String>>,
    options: &Options,
) {
    let startup = async {
        if let ExecutionIo::Pty { cols, rows } = options.io {
            writer
                .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
                .await
                .map_err(|e| e.to_string())?;
            replies
                .recv()
                .await
                .ok_or("execRejected:终端启动回复丢失")??;
        }
        ex.state.lock().unwrap().requested = true;
        ex.changed.notify_waiters();
        match &options.launch {
            Launch::Command(command) => writer.exec(true, command.as_str()).await,
            Launch::LoginShell => writer.request_shell(true).await,
        }
        .map_err(|e| e.to_string())?;
        ex.state.lock().unwrap().queued = true;
        if let (Some(c), Launch::Command(command)) = (core.upgrade(), &options.launch) {
            c.record_business(&ex.owner, command.len() as u64, 0);
        }
        replies
            .recv()
            .await
            .ok_or("execRejected:程序启动回复丢失")??;
        ex.state.lock().unwrap().accepted = true;
        ex.ready.cancel();
        ex.changed.notify_waiters();
        if !options.io.interactive() {
            writer.eof().await.map_err(|e| e.to_string())?;
            ex.state.lock().unwrap().stdin_closed = true;
        }
        Ok::<_, String>(())
    };
    let result = tokio::select! {biased;_=ex.stop.cancelled()=>return,r=startup=>r};
    if let Err(e) = result {
        ex.stop(Some(e));
        return;
    }
    if !options.io.interactive() {
        return;
    }
    loop {
        let item =
            tokio::select! {biased;_=ex.input_stop.cancelled()=>break,item=input.recv()=>item};
        let Some(item) = item else {
            break;
        };
        let bytes = match &item.operation {
            InputOperation::Write(data) => Some(data.len() as u64),
            _ => None,
        };
        let sending = AtomicBool::new(false);
        let send = async {
            sending.store(true, Ordering::Release);
            match &item.operation {
                InputOperation::Write(data) => writer.data(data.as_ref()).await,
                InputOperation::Eof => writer.eof().await,
                InputOperation::Resize(cols, rows) => {
                    writer.window_change(*cols, *rows, 0, 0).await
                }
                InputOperation::Barrier => Ok(()),
            }
            .map_err(|e| e.to_string())
        };
        let result = tokio::select! {biased;_=ex.input_stop.cancelled()=>Err("inputClosed:输入传输中执行结束".to_string()),r=send=>r};
        match result {
            Ok(()) => {
                if let Some(n) = bytes {
                    ex.sent.fetch_add(n, Ordering::Relaxed);
                    if let Some(c) = core.upgrade() {
                        c.record_business(&ex.owner, n, 0);
                    }
                }
                let eof = matches!(item.operation, InputOperation::Eof);
                let _ = item.completion.send(InputOutcome::Sent(bytes));
                ex.changed.notify_waiters();
                if eof {
                    ex.state.lock().unwrap().stdin_closed = true;
                    break;
                }
            }
            Err(e) => {
                let outcome = if sending.load(Ordering::Acquire) {
                    InputOutcome::Unconfirmed(e.clone())
                } else {
                    InputOutcome::NotSent(e.clone())
                };
                let _ = item.completion.send(outcome);
                if !ex.input_stop.is_cancelled() {
                    ex.stop(Some(format!("inputFailed:{e}")));
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> Owner {
        Owner {
            plugin: "test".into(),
            instance: "instance".into(),
            view: "view".into(),
            context: crate::workspaces::Context {
                workspace_id: "workspace".into(),
                server_id: "server".into(),
                connection_id: "connection".into(),
                generation: 1,
            },
        }
    }
    #[tokio::test]
    async fn input_queue_is_bounded_preserves_order_and_stops_at_cancellation() {
        let (ex, mut rx) = Execution::create(owner());
        let mut receipts = Vec::new();
        for byte in 0..32 {
            receipts.push(ex.submit(InputOperation::Write(Arc::from([byte]))).unwrap());
        }
        assert!(ex
            .submit(InputOperation::Eof)
            .unwrap_err()
            .starts_with("resourceBusy:"));
        for (byte, receipt) in receipts.into_iter().enumerate() {
            let queued = rx.recv().await.unwrap();
            assert_eq!(
                queued.operation,
                InputOperation::Write(Arc::from([byte as u8]))
            );
            queued.completion.send(InputOutcome::Sent(Some(1))).unwrap();
            assert!(matches!(
                receipt.await.unwrap(),
                InputOutcome::Sent(Some(1))
            ));
        }
        ex.cancel.cancel();
        assert!(ex
            .submit(InputOperation::Eof)
            .unwrap_err()
            .starts_with("inputClosed:"));
    }
}
