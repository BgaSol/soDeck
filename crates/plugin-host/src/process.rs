use crate::{transport, RpcError, INTERNAL_MODE};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub type CoreCallback = Arc<dyn Fn(&str, Value) -> Result<Value, RpcError> + Send + Sync>;
pub type CoreContextCallback = Arc<dyn Fn(&str, Value, Option<Value>) -> Result<Value, RpcError> + Send + Sync>;
pub type BinarySink = dyn FnMut(sodeck_communication::binary::Frame) -> bool;
#[derive(Clone)]
pub struct BinaryCallbacks {
    pub receive: Arc<dyn Fn(sodeck_communication::binary::Frame) -> Result<(), RpcError> + Send + Sync>,
    pub produce: Arc<dyn Fn(&mut BinarySink) + Send + Sync>,
}
type Reply = mpsc::SyncSender<Result<Value, RpcError>>;
const QUEUE_BYTES: usize = 4 * 1024 * 1024;

struct QueuePermit {
    bytes: usize,
    total: Arc<AtomicUsize>,
}
impl Drop for QueuePermit {
    fn drop(&mut self) {
        self.total.fetch_sub(self.bytes, Ordering::SeqCst);
    }
}
struct WriteJob {
    bytes: Vec<u8>,
    // 0 queued, 1 sending, 2 sent, 3 cancelled before any write.
    state: Arc<AtomicU8>,
    _permit: QueuePermit,
    _retention:Option<Arc<dyn Send+Sync>>,
}

#[derive(Clone)]
struct Writer {
    wake: Arc<Mutex<Option<std::thread::Thread>>>,
    binary: Arc<Mutex<sodeck_communication::binary::Queue>>,
    send: mpsc::SyncSender<WriteJob>,
    control: mpsc::SyncSender<WriteJob>,
    total: Arc<AtomicUsize>,
    control_bytes: Arc<AtomicUsize>,
}
impl Writer {
    fn wake(&self) { if let Some(thread)=self.wake.lock().unwrap().as_ref(){thread.unpark();} }
    fn binary(&self, frame:sodeck_communication::binary::Frame)->bool {
        let accepted=self.binary.lock().unwrap().push(frame).unwrap_or(false);
        if accepted {self.wake();} accepted
    }
    fn enqueue(&self, frame: &Value) -> Result<Arc<AtomicU8>, RpcError> {
        self.enqueue_lane(frame, false)
    }
    fn enqueue_lane(&self, frame: &Value, control: bool) -> Result<Arc<AtomicU8>, RpcError> {
        let bytes = serde_json::to_vec(frame)
            .map_err(|_| RpcError::new("PROTOCOL", "Frame encoding failed"))?;
        self.enqueue_bytes(bytes,control,None)
    }
    fn enqueue_bytes(&self,bytes:Vec<u8>,control:bool,retention:Option<Arc<dyn Send+Sync>>)->Result<Arc<AtomicU8>,RpcError>{
        if bytes.is_empty() || bytes.len() > transport::MAX_FRAME {
            return Err(not_sent("CAPACITY", "Plugin frame exceeds 1 MiB"));
        }
        let total = if control {&self.control_bytes} else {&self.total};
        total
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                used.checked_add(bytes.len())
                    .filter(|next| *next <= if control {32 * 1024} else {QUEUE_BYTES - 32 * 1024})
            })
            .map_err(|_| not_sent("CAPACITY", "Plugin write queue exceeds 4 MiB"))?;
        let state = Arc::new(AtomicU8::new(0));
        let permit = QueuePermit {
            bytes: bytes.len(),
            total: total.clone(),
        };
        (if control {&self.control} else {&self.send})
            .try_send(WriteJob {
                bytes,
                state: state.clone(),
                _permit: permit,
                _retention:retention,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => not_sent("CAPACITY", "Plugin write queue is full"),
                mpsc::TrySendError::Disconnected(_) => {
                    not_sent("PLUGIN_CRASHED", "Plugin writer stopped")
                }
            })?;
        self.wake();
        Ok(state)
    }
}

pub struct PluginProcess {
    child: Arc<Mutex<Child>>,
    writer: Writer,
    pending: Arc<Mutex<HashMap<String, Reply>>>,
    sequence: AtomicU64,
    alive: Arc<AtomicBool>,
    code: String,
    binary_enabled: bool,
    binary_ready: Arc<AtomicBool>,
    diagnostic_ready: AtomicBool,
}

impl PluginProcess {
    pub fn spawn(
        executable: impl AsRef<Path>,
        worker_path: impl AsRef<Path>,
        callback: CoreCallback,
    ) -> Result<Self, RpcError> {
        Self::spawn_with_binary(executable,worker_path,callback,None)
    }
    pub fn spawn_with_binary(
        executable: impl AsRef<Path>, worker_path: impl AsRef<Path>,
        callback: CoreCallback, binary: Option<BinaryCallbacks>,
    ) -> Result<Self, RpcError> {
        Self::spawn_with_context(executable, worker_path, Arc::new(move |method, params, _| callback(method, params)), binary)
    }
    pub fn spawn_with_context(
        executable: impl AsRef<Path>, worker_path: impl AsRef<Path>,
        callback: CoreContextCallback, binary: Option<BinaryCallbacks>,
    ) -> Result<Self, RpcError> {
        if fs::metadata(worker_path.as_ref()).map_err(io_error)?.len() > 768 * 1024 {
            return Err(not_sent("CAPACITY", "Worker exceeds startup frame budget"));
        }
        let code = fs::read_to_string(worker_path).map_err(io_error)?;
        Self::spawn_source_with_context(executable, code, callback, binary)
    }
    /// Production callers supply the exact source from a verified package snapshot.
    pub fn spawn_source_with_context(executable: impl AsRef<Path>, code: String, callback: CoreContextCallback, binary: Option<BinaryCallbacks>) -> Result<Self,RpcError> {
        if code.len()>768*1024 {return Err(not_sent("CAPACITY","Worker exceeds startup frame budget"));}
        let mut command = Command::new(executable.as_ref());
        command
            .arg(INTERNAL_MODE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(io_error)?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| RpcError::new("INTERNAL", "Missing child input"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| RpcError::new("INTERNAL", "Missing child output"))?;
        let child = Arc::new(Mutex::new(child));
        let pending: Arc<Mutex<HashMap<String, Reply>>> = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (send, receive) = mpsc::sync_channel::<WriteJob>(32);
        let (control, control_receive) = mpsc::sync_channel::<WriteJob>(32);
        let writer = Writer {
            wake: Arc::new(Mutex::new(None)),
            binary: Arc::new(Mutex::new(sodeck_communication::binary::Queue::default())),
            send,
            control,
            total: Arc::new(AtomicUsize::new(0)),
            control_bytes: Arc::new(AtomicUsize::new(0)),
        };
        let write_alive = alive.clone();
        let write_pending = pending.clone();
        let write_child = child.clone();
        let write_binary = writer.binary.clone();
        let write_wake = writer.wake.clone();
        std::thread::spawn(move || {
            *write_wake.lock().unwrap()=Some(std::thread::current());
            let mut control_batch=0;
            loop {
                let priority=if control_batch<8 {control_receive.try_recv().ok()} else {None};
                let job=if let Some(job)=priority {control_batch+=1;job} else {
                    control_batch=0;
                    match receive.try_recv() {
                        Ok(job)=>job,
                        Err(mpsc::TryRecvError::Empty)=>{
                            if !write_alive.load(Ordering::SeqCst){break;}
                            let frame=write_binary.lock().unwrap().pop();
                            if let Some(frame)=frame {
                                if transport::write_binary(&mut input,&frame).is_err(){
                                    fail_process(&write_alive,&write_pending,&write_child,RpcError::new("PLUGIN_IO","Binary pipe write failed"));break;
                                }
                            } else {std::thread::park_timeout(Duration::from_millis(5));}
                            continue;
                        },
                        Err(mpsc::TryRecvError::Disconnected)=>break,
                    }
                };
                if !write_alive.load(Ordering::SeqCst) {
                    break;
                }
                if job
                    .state
                    .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    continue;
                }
                let result = input
                    .write_all(&(job.bytes.len() as u32).to_be_bytes())
                    .and_then(|_| input.write_all(&job.bytes))
                    .and_then(|_| input.flush());
                if result.is_err() {
                    fail_process(
                        &write_alive,
                        &write_pending,
                        &write_child,
                        RpcError::new(
                            "PLUGIN_IO",
                            "Plugin pipe write failed; operation outcome may be unknown",
                        ),
                    );
                    break;
                }
                job.state.store(2, Ordering::SeqCst);
            }
        });
        let replies = pending.clone();
        let reader_writer = writer.clone();
        let reader_alive = alive.clone();
        let reader_child = child.clone();
        let active_calls = Arc::new(AtomicUsize::new(0));
        let binary_ready=Arc::new(AtomicBool::new(false));
        let binary_enabled=binary.is_some();
        let pump_wake=Arc::new(Mutex::new(None::<std::thread::Thread>));
        if let Some(callbacks)=binary.clone() {
            let ready=binary_ready.clone();let active=alive.clone();let binary_writer=writer.clone();
            let wake=pump_wake.clone();
            std::thread::Builder::new().name("plugin-binary-pump".into()).spawn(move||{
                *wake.lock().unwrap()=Some(std::thread::current());
                while active.load(Ordering::SeqCst) {
                    if ready.load(Ordering::SeqCst) {
                        let writer=binary_writer.clone();
                        (callbacks.produce)(&mut move |frame|writer.binary(frame));
                    }
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            }).map_err(io_error)?;
        }
        let read_binary_ready=binary_ready.clone();
        std::thread::spawn(move || {
            while let Ok(Some(wire)) = transport::read_wire(&mut output) {
                let frame=match wire {
                    transport::Wire::Json(value)=>value,
                    transport::Wire::Binary(frame)=>{
                        let Some(callback)=&binary else {break};
                        if !read_binary_ready.load(Ordering::SeqCst){break;}
                        if let Err(error)=(callback.receive)(frame) {
                            if !matches!(error.code.as_str(),"STALE_HANDLE"|"PROTOCOL_ERROR"){break;}
                        }
                        if let Some(thread)=pump_wake.lock().unwrap().as_ref(){thread.unpark();}
                        continue;
                    }
                };
                match frame["kind"].as_str() {
                    Some("result") => {
                        let Some(id) = frame["id"]
                            .as_str()
                            .filter(|id| !id.is_empty() && id.len() <= 128)
                        else {
                            break;
                        };
                        if let Some(reply) = replies.lock().unwrap().remove(id) {
                            let result = if frame.get("error").is_some() {
                                serde_json::from_value::<RpcError>(frame["error"].clone())
                                    .map_or_else(
                                        |_| {
                                            Err(RpcError::new("PROTOCOL", "Malformed plugin error"))
                                        },
                                        Err,
                                    )
                            } else {
                                Ok(frame["result"].clone())
                            };
                            let _ = reply.send(result);
                        }
                    }
                    Some("core") => {
                        let (Some(id), Some(method)) =
                            (frame["id"].as_str(), frame["method"].as_str())
                        else {
                            break;
                        };
                        if id.is_empty()
                            || method.is_empty()
                            || id.len() > 128
                            || method.len() > 128
                        {
                            break;
                        }
                        let id = id.to_owned();
                        let method = method.to_owned();
                        let params = frame["params"].clone();
                        let request_bytes=frame.to_string().len();
                        let metadata = frame.get("metadata").cloned();
                        if metadata.as_ref().is_some_and(|m| m.to_string().len() > 1024) {
                            if reader_writer.enqueue_lane(&json!({"kind":"result","id":id,"error":RpcError::new("INVALID_REQUEST","Diagnostic metadata exceeds limit")}),true).is_err(){break;}
                            continue;
                        }
                        if active_calls.fetch_add(1, Ordering::SeqCst) >= 32 {
                            active_calls.fetch_sub(1, Ordering::SeqCst);
                            if reader_writer.enqueue(&json!({"kind":"result","id":id,"error":RpcError::new("CAPACITY", "Too many plugin core calls")})).is_err() { break; }
                            continue;
                        }
                        let callback = callback.clone();
                        let writer = reader_writer.clone();
                        let calls = active_calls.clone();
                        let callback_alive = reader_alive.clone();
                        let callback_pending = replies.clone();
                        let callback_child = reader_child.clone();
                        let dispatch_method=method.clone();
                        let rejected_id=id.clone();
                        let job=Box::new(move || {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if callback_alive.load(Ordering::SeqCst) {callback(&method, params, metadata)} else {Err(RpcError::new("PLUGIN_STOPPING","Plugin instance ended before execution"))}
                                }))
                                .unwrap_or_else(|_| {
                                    Err(RpcError::new("INTERNAL", "Core callback failed"))
                                });
                            let response = match result {
                                Ok(result) => json!({"kind":"result","id":id,"result":result}),
                                Err(error) => json!({"kind":"result","id":id,"error":error}),
                            };
                            if callback_alive.load(Ordering::SeqCst) {
                                let control=crate::dispatcher::control(&method);
                                if writer.enqueue_lane(&response,control).is_err() {
                                    // A completed core call may not fit on the wire. Tell the
                                    // worker its result is unknown rather than dropping it.
                                    let mut unavailable = RpcError::new("RESULT_UNKNOWN", "Core response cannot be delivered within plugin wire limits");
                                    unavailable.operation_id = Some(id.clone());
                                    unavailable.phase = Some("sent".into());
                                    if writer
                                        .enqueue(
                                            &json!({"kind":"result","id":id,"error":unavailable}),
                                        )
                                        .is_err()
                                    {
                                        fail_process(
                                            &callback_alive,
                                            &callback_pending,
                                            &callback_child,
                                            RpcError::new("PLUGIN_IO", "Core reply queue is unavailable; in-flight operation outcomes may be unknown"),
                                        );
                                    }
                                }
                            }
                            calls.fetch_sub(1, Ordering::SeqCst);
                        });
                        if crate::dispatcher::short(&dispatch_method) {
                            if !crate::dispatcher::submit_sized(&dispatch_method,request_bytes,job) {
                                active_calls.fetch_sub(1,Ordering::SeqCst);
                                if reader_writer.enqueue_lane(&json!({"kind":"result","id":rejected_id,"error":RpcError::new("CAPACITY","Short core call queue is full")}),true).is_err(){break;}
                            }
                        } else {std::thread::spawn(job);}
                    }
                    _ => break,
                }
            }
            fail_process(
                &reader_alive,
                &replies,
                &reader_child,
                RpcError::new(
                    "PLUGIN_CRASHED",
                    "Plugin process stopped; in-flight operation outcomes may be unknown",
                ),
            );
        });
        Ok(Self {
            child,
            writer,
            pending,
            sequence: AtomicU64::new(0),
            alive,
            code,
            binary_enabled,
            binary_ready,
            diagnostic_ready: AtomicBool::new(false),
        })
    }

    pub fn initialize(&self, context: Value) -> Result<Value, RpcError> {
        let result=self.request(
            json!({"kind":"initialize", "code":self.code, "context":context,"diagnosticContextVersion":1,"binaryVersion":if self.binary_enabled {1}else{0}}),
            Duration::from_secs(15),
        )?;
        if self.binary_enabled {
            if result["binaryVersion"]!=1 {self.terminate();return Err(RpcError::new("PROTOCOL_ERROR","Binary worker version mismatch"));}
            self.binary_ready.store(true,Ordering::SeqCst);
        }
        self.diagnostic_ready.store(result["diagnosticContextVersion"]==1,Ordering::SeqCst);
        Ok(result)
    }
    pub fn invoke(&self, command: &str, params: Value) -> Result<Value, RpcError> {
        self.invoke_with_timeout(command, params, Duration::from_secs(180))
    }
    pub fn invoke_with_timeout(
        &self,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcError> {
        self.invoke_with_context(command,params,timeout,None)
    }
    pub fn invoke_with_context(&self,command:&str,params:Value,timeout:Duration,metadata:Option<Value>)->Result<Value,RpcError>{
        let mut frame=json!({"kind":"invoke", "command":command, "params":params});
        if self.diagnostic_ready.load(Ordering::SeqCst){if let Some(metadata)=metadata{frame["metadata"]=metadata;}}
        self.request(frame,timeout)
    }
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    pub fn notify(&self, event: &str, payload: Value) -> Result<(), RpcError> {
        if !self.is_alive(){return Err(RpcError::new("PLUGIN_CRASHED","Plugin process is unavailable"));}
        self.writer.enqueue_lane(&json!({"kind":"notification","type":event,"payload":payload}),
            matches!(event,"state.closed"|"state.resyncRequired"|"workspace.closed"|"events.v2.gap"|"events.v2.closed")).map(|_|())
    }
    /// Host-authored serialized history: credit follows this writer's queued frame.
    pub fn notify_serialized(&self,event:&str,payload:Arc<str>,retention:Arc<dyn Send+Sync>)->Result<(),RpcError>{
        if !self.is_alive(){return Err(RpcError::new("PLUGIN_CRASHED","Plugin process is unavailable"));}
        let prefix=format!("{{\"kind\":\"notification\",\"type\":{},\"payload\":",serde_json::to_string(event).unwrap());
        let mut bytes=Vec::with_capacity(prefix.len()+payload.len()+1);bytes.extend_from_slice(prefix.as_bytes());bytes.extend_from_slice(payload.as_bytes());bytes.push(b'}');
        self.writer.enqueue_bytes(bytes,matches!(event,"events.v2.gap"|"events.v2.closed"),Some(retention)).map(|_|())
    }
    pub fn process_id(&self) -> u32 {
        self.child.lock().unwrap().id()
    }
    pub fn stop(&self) -> Result<Value, RpcError> {self.stop_with_timeout(Duration::from_secs(5))}
    pub fn stop_with_timeout(&self,timeout:Duration) -> Result<Value, RpcError> {
        // Admission and waiting share the deadline; neither requires ownership
        // of ChildStdin. Killing the child also releases any blocked writer.
        let result = self.request(json!({"kind":"stop"}), timeout);
        self.terminate();
        result
    }
    pub fn terminate(&self) {
        fail_process(
            &self.alive,
            &self.pending,
            &self.child,
            RpcError::new("PLUGIN_STOPPING", "Plugin stopped"),
        );
    }
    fn request(&self, mut frame: Value, timeout: Duration) -> Result<Value, RpcError> {
        let started = Instant::now();
        if !self.alive.load(Ordering::SeqCst) {
            return Err(not_sent("PLUGIN_CRASHED", "Plugin process is unavailable"));
        }
        let id = format!("host-{}", self.sequence.fetch_add(1, Ordering::SeqCst));
        frame["id"] = Value::String(id.clone());
        let (send, receive) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().unwrap();
            // Pair with fail_process setting alive before taking this lock:
            // no request can be inserted after the final pending drain.
            if !self.alive.load(Ordering::SeqCst) {
                return Err(not_sent("PLUGIN_CRASHED", "Plugin process is unavailable"));
            }
            if pending.len() >= 64 {
                return Err(not_sent("CAPACITY", "Too many plugin commands"));
            }
            pending.insert(id.clone(), send);
        }
        let write_state = match self.writer.enqueue(&frame) {
            Ok(state) => state,
            Err(error) => {
                self.pending.lock().unwrap().remove(&id);
                return Err(error);
            }
        };
        match receive.recv_timeout(timeout.saturating_sub(started.elapsed())) {
            Ok(result) => result,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                if write_state
                    .compare_exchange(0, 3, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let mut error =
                        not_sent("TIMEOUT", "Plugin command expired before transmission");
                    error.operation_id = Some(id);
                    Err(error)
                } else {
                    let mut error = RpcError::new(
                        "RESULT_UNKNOWN",
                        "Plugin command deadline elapsed; do not automatically retry",
                    );
                    error.operation_id = Some(id);
                    error.phase = Some("sent".into());
                    Err(error)
                }
            }
        }
    }
}
impl Drop for PluginProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}
fn not_sent(code: &str, message: impl Into<String>) -> RpcError {
    let mut error = RpcError::new(code, message);
    error.phase = Some("notSent".into());
    error
}
fn io_error(error: std::io::Error) -> RpcError {
    not_sent("PLUGIN_IO", error.to_string())
}
fn fail_process(
    alive: &AtomicBool,
    pending: &Mutex<HashMap<String, Reply>>,
    child: &Mutex<Child>,
    error: RpcError,
) {
    alive.store(false, Ordering::SeqCst);
    {
        let mut child = child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
    for (_, reply) in pending.lock().unwrap().drain() {
        let _ = reply.send(Err(error.clone()));
    }
}
