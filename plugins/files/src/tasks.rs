//! File business orchestration. The core executes only individual filesystem primitives.
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Tasks {
    inner: Arc<Inner>,
}
struct Inner {
    client: Client,
    directory: PathBuf,
    sequence: AtomicU64,
    queue: Mutex<Queue>,
    wake: Condvar,
}
#[derive(Default)]
struct Queue {
    batches: HashMap<String, Arc<Batch>>,
    waiting: VecDeque<String>,
    active: HashSet<String>,
}
struct Batch {
    id: String,
    view: String,
    file: String,
    context: Value,
    parameters: Value,
    cancel: AtomicBool,
    state: Mutex<Value>,
    decision: Mutex<Option<Value>>,
    decision_wake: Condvar,
    policy: Mutex<Value>,
    journal: PathBuf,
    confirm: AtomicBool,
    receipts: Mutex<Vec<String>>,
}
#[derive(Clone)]
struct Item {
    source: Value,
    destination: Value,
    name: String,
    stamp: Value,
    delete_directory: bool,
}
#[derive(Debug)]
struct TaskError {
    code: String,
    message: String,
}
impl TaskError {
    fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
        }
    }
    fn retryable(&self) -> bool {
        ![
            "unknown",
            "invalid",
            "unsupported",
            "expired",
            "cancelled",
            "contextChanged",
        ]
        .contains(&self.code.as_str())
    }
}
impl From<String> for TaskError {
    fn from(message: String) -> Self {
        Self::new("io", message)
    }
}
impl From<&str> for TaskError {
    fn from(message: &str) -> Self {
        Self::new("invalid", message)
    }
}
impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

struct Scratch(Vec<PathBuf>);
impl Drop for Scratch {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
impl Tasks {
    pub fn new(client: Client, directory: PathBuf) -> Result<Self, String> {
        // Only this plugin's flat, generated run folders are eligible for stale journal cleanup.
        if let Ok(entries) = fs::read_dir(&directory) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let Some(rest) = name.strip_prefix("file-tasks-") else {
                    continue;
                };
                let Some((pid, nonce)) = rest.split_once('-') else {
                    continue;
                };
                if pid.parse::<u32>().is_err()
                    || nonce.parse::<u64>().is_err()
                    || !entry
                        .file_type()
                        .is_ok_and(|t| t.is_dir() && !t.is_symlink())
                {
                    continue;
                }
                if let Ok(files) = fs::read_dir(entry.path()) {
                    for file in files.flatten() {
                        if file
                            .file_type()
                            .is_ok_and(|t| t.is_file() && !t.is_symlink())
                        {
                            let _ = fs::remove_file(file.path());
                        }
                    }
                }
                let _ = fs::remove_dir(entry.path());
            }
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let directory = directory.join(format!("file-tasks-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let inner = Arc::new(Inner {
            client,
            directory,
            sequence: AtomicU64::new(nonce),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        for index in 0..2 {
            let inner = inner.clone();
            thread::Builder::new()
                .name(format!("files-worker-{index}"))
                .spawn(move || inner.worker())
                .map_err(|e| e.to_string())?;
        }
        Ok(Self { inner })
    }
    pub fn start(
        &self,
        view: &str,
        file: &str,
        context: Value,
        parameters: Value,
    ) -> Result<Value, String> {
        let kind = parameters["kind"]
            .as_str()
            .ok_or("缺少文件任务类型")?
            .to_string();
        if ![
            "upload",
            "download",
            "copy",
            "move",
            "delete",
            "mkdir",
            "createEmpty",
            "rename",
        ]
        .contains(&kind.as_str())
        {
            return Err("未知文件任务".into());
        }
        if ["move", "rename"].contains(&kind.as_str()) {
            return Err("当前后端不支持原子无覆盖移动/改名；可另行复制，源不会自动删除".into());
        }
        if let Some(roots) = parameters["roots"].as_array() {
            if roots.is_empty() || roots.len() > 4096 {
                return Err("请选择 1 至 4096 个条目".into());
            }
        }
        let mut queue = self.inner.queue.lock().unwrap();
        if queue.batches.values().any(|b| {
            b.context["workspaceId"] == context["workspaceId"]
                && b.state.lock().unwrap()["state"] == "needsReview"
        }) {
            return Err("此工作区有待确认文件操作，请先核对结果".into());
        }
        if queue.waiting.len() >= 64 {
            return Err("文件任务队列已满，请先处理现有任务".into());
        }
        if queue.batches.len() >= 128 {
            let old = queue
                .batches
                .iter()
                .find(|(_, b)| terminal(&b.state.lock().unwrap()))
                .map(|(id, _)| id.clone())
                .ok_or("文件任务记录已满")?;
            self.inner.remove_batch(&mut queue, &old);
        }
        if let Some(input) = parameters["inputId"].as_str() {
            self.inner.client.call(
                "files.request",
                json!({"action":"input.claim","id":file,"inputId":input,"renew":parameters["renewInput"]==true}),
            )?;
        }
        let id = self.inner.next().map_err(|e| e.message)?.to_string();
        self.inner.client.call(
            "files.request",
            json!({"action":"batch.begin","id":file,"batchId":id,"context":context}),
        )?;
        let batch = Arc::new(Batch {
            id: id.clone(),
            view: view.into(),
            file: file.into(),
            context,
            parameters: parameters.clone(),
            cancel: AtomicBool::new(false),
            state: Mutex::new(
                json!({"id":id,"fileId":file,"inputId":parameters["inputId"],"kind":kind,"state":"queued","completed":"0","failed":"0","skipped":"0","cancelledItems":"0","bytes":"0","currentBytes":"0","currentTotal":null}),
            ),
            decision: Mutex::new(None),
            decision_wake: Condvar::new(),
            policy: Mutex::new(json!({})),
            journal: self.inner.directory.join(format!("{id}.results.jsonl")),
            confirm: AtomicBool::new(false),
            receipts: Mutex::new(Vec::new()),
        });
        queue.batches.insert(id.clone(), batch);
        queue.waiting.push_back(id.clone());
        self.inner.wake.notify_all();
        Ok(json!({"batchId":id}))
    }
    pub fn snapshot(&self, context: &Value) -> Value {
        self.snapshot_page(context, 0)
    }
    pub fn snapshot_page(&self, context: &Value, offset: usize) -> Value {
        let queue = self.inner.queue.lock().unwrap();
        let mut tasks: Vec<Value> = queue
            .batches
            .values()
            .filter(|b| b.context["workspaceId"] == context["workspaceId"])
            .map(|b| b.state.lock().unwrap().clone())
            .collect();
        tasks.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
        let total = tasks.len();
        let mut page = Vec::new();
        let mut bytes = 0;
        for mut task in tasks.into_iter().skip(offset).take(24) {
            for key in ["error", "lastError", "warning"] {
                if let Some(text) = task[key].as_str() {
                    task[key] = json!(text.chars().take(256).collect::<String>());
                }
            }
            if let Some(name) = task["conflict"]["name"].as_str() {
                task["conflict"]["name"] = json!(name.chars().take(256).collect::<String>());
            }
            if let Some(current) = task["current"].as_str() {
                task["current"] = json!(current.chars().take(512).collect::<String>());
            }
            let size = task.to_string().len();
            if bytes + size > 14000 {
                break;
            }
            bytes += size;
            page.push(task);
        }
        let next = offset + page.len();
        json!({"tasks":page,"next":if next<total{Some(next)}else{None}})
    }
    pub fn busy(&self, view: &str) -> bool {
        self.inner
            .queue
            .lock()
            .unwrap()
            .batches
            .values()
            .any(|b| b.view == view && !terminal(&b.state.lock().unwrap()))
    }
    pub fn succeeded(&self, id: &str) -> bool {
        self.inner
            .queue
            .lock()
            .unwrap()
            .batches
            .get(id)
            .is_some_and(|b| {
                let s = b.state.lock().unwrap();
                s["state"] == "completed" && count(&s, "skipped") == 0
            })
    }
    pub fn waiting_conflict(&self, view: &str) -> bool {
        self.inner
            .queue
            .lock()
            .unwrap()
            .batches
            .values()
            .any(|b| b.view == view && b.state.lock().unwrap()["state"] == "awaitingConflict")
    }
    pub fn blocks(&self, context: &Value, path: &str) -> bool {
        self.inner.queue.lock().unwrap().batches.values().any(|b| {
            b.context["workspaceId"] == context["workspaceId"]
                && !terminal(&b.state.lock().unwrap())
                && affected_paths(&b.parameters)
                    .iter()
                    .any(|root| within(path, root))
        })
    }
    pub fn cancel_view(&self, view: &str) {
        for batch in self
            .inner
            .queue
            .lock()
            .unwrap()
            .batches
            .values()
            .filter(|b| b.view == view)
        {
            batch.cancel.store(true, Ordering::Release);
            batch.decision_wake.notify_all();
        }
        self.inner.wake.notify_all();
    }
    pub fn command(&self, view: &str, parameters: &Value) -> Result<Value, String> {
        let id = parameters["batchId"].as_str().ok_or("缺少任务 ID")?;
        let batch = self
            .inner
            .queue
            .lock()
            .unwrap()
            .batches
            .get(id)
            .filter(|b| b.context["workspaceId"] == parameters["context"]["workspaceId"])
            .cloned()
            .ok_or("任务不属于此视图或已清理")?;
        match parameters["action"].as_str() {
            Some("cancelBatch") => {
                if terminal(&batch.state.lock().unwrap()) {
                    return Ok(json!({}));
                }
                batch.state.lock().unwrap()["state"] = json!("cancelling");
                batch.cancel.store(true, Ordering::Release);
                batch.decision_wake.notify_all();
                self.inner.wake.notify_all();
                Ok(json!({}))
            }
            Some("confirmBatch") => {
                if batch.state.lock().unwrap()["awaitingReceipt"] != true {
                    return Err("没有可查询的未决操作，请核对服务器现状".into());
                }
                batch.confirm.store(true, Ordering::Release);
                batch.decision_wake.notify_all();
                Ok(json!({}))
            }
            Some("resolveConflict") => {
                let state = batch.state.lock().unwrap();
                if state["state"] != "awaitingConflict"
                    || (!parameters["conflictId"].is_null()
                        && parameters["conflictId"] != state["conflict"]["itemId"])
                {
                    return Err("冲突已处理或任务已变化".into());
                }
                if !state["conflict"]["allowedActions"]
                    .as_array()
                    .is_some_and(|a| a.contains(&parameters["choice"]))
                {
                    return Err("此冲突不支持该操作".into());
                }
                drop(state);
                *batch.decision.lock().unwrap() = Some(parameters.clone());
                batch.decision_wake.notify_all();
                Ok(json!({}))
            }
            Some("readItems") => {
                let mut reader =
                    BufReader::new(File::open(&batch.journal).map_err(|e| e.to_string())?);
                reader
                    .seek(SeekFrom::Start(parameters["offset"].as_u64().unwrap_or(0)))
                    .map_err(|e| e.to_string())?;
                let mut entries = Vec::new();
                let mut bytes = 0;
                loop {
                    let position = reader.stream_position().map_err(|e| e.to_string())?;
                    let mut line = String::new();
                    if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                        break;
                    }
                    let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                    let value = json!({"state":value["state"],"error":value["error"].as_str().map(|e|e.chars().take(512).collect::<String>()),"item":{"name":value["item"]["name"].as_str().unwrap_or("").chars().take(256).collect::<String>(),"source":endpoint_label(&value["item"]["source"]),"destination":endpoint_label(&value["item"]["destination"])}});
                    let size = value.to_string().len();
                    if bytes + size > 14000 {
                        reader
                            .seek(SeekFrom::Start(position))
                            .map_err(|e| e.to_string())?;
                        break;
                    }
                    bytes += size;
                    entries.push(value);
                    if entries.len() >= 100 {
                        break;
                    }
                }
                Ok(
                    json!({"entries":entries,"offset":reader.stream_position().map_err(|e|e.to_string())?}),
                )
            }
            Some("retryFailed") => {
                if batch.file != parameters["fileId"].as_str().unwrap_or("") {
                    return Err("请在发起任务的文件面板重试；该面板已关闭时需重新选择来源".into());
                }
                if batch.state.lock().unwrap()["state"] == "needsReview" {
                    return Err("提交结果未知，请先刷新核对，不能直接重试".into());
                }
                if !terminal(&batch.state.lock().unwrap()) {
                    return Err("请先等待任务结束".into());
                }
                let mut p = batch.parameters.clone();
                p["renewInput"] = json!(batch.context != parameters["context"]);
                p["retryJournal"] = json!(batch.journal.to_str().ok_or("任务记录路径无效")?);
                drop(batch);
                let context = parameters["context"].clone();
                self.start(
                    view,
                    parameters["fileId"].as_str().ok_or("缺少文件资源")?,
                    context,
                    p,
                )
            }
            Some("clearBatch") => {
                if !terminal(&batch.state.lock().unwrap()) {
                    return Err("任务仍在运行".into());
                }
                self.inner
                    .remove_batch(&mut self.inner.queue.lock().unwrap(), id);
                Ok(json!({}))
            }
            _ => Err("未知文件任务操作".into()),
        }
    }
}
fn terminal(state: &Value) -> bool {
    if state["awaitingReceipt"] == true {
        return false;
    }
    ["completed", "partial", "failed", "cancelled", "needsReview"]
        .contains(&state["state"].as_str().unwrap_or(""))
}
pub fn within(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{}/", root.trim_end_matches('/')))
}
pub fn affected_paths(parameters: &Value) -> Vec<&str> {
    let kind = parameters["kind"].as_str().unwrap_or("");
    if kind == "download" {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if ["delete", "move", "rename"].contains(&kind) {
        if let Some(roots) = parameters["roots"].as_array() {
            paths.extend(roots.iter().filter_map(Value::as_str));
        }
    }
    for key in ["path", "destination", "targetPath"] {
        if let Some(path) = parameters[key].as_str() {
            paths.push(path);
        }
    }
    paths
}
fn workspace(batch: &Batch) -> String {
    batch.context["workspaceId"].as_str().unwrap_or("").into()
}
impl Inner {
    fn remove_batch(&self, queue: &mut Queue, id: &str) {
        if let Some(batch) = queue.batches.remove(id) {
            let _ = fs::remove_file(&batch.journal);
            if let Some(input) = batch.parameters["inputId"].as_str() {
                if !queue
                    .batches
                    .values()
                    .any(|b| b.file == batch.file && b.parameters["inputId"] == input)
                {
                    let _ = self.client.call(
                        "files.request",
                        json!({"action":"input.release","id":batch.file,"inputId":input}),
                    );
                }
            }
        }
    }
    fn next(&self) -> Result<u64, TaskError> {
        self.sequence
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_add(1))
            .map_err(|_| TaskError::new("expired", "操作序号已用尽，请重启插件"))
    }
    fn worker(self: Arc<Self>) {
        loop {
            let batch = {
                let mut queue = self.queue.lock().unwrap();
                loop {
                    if self.client.is_closed() {
                        return;
                    }
                    let selected = queue.waiting.iter().position(|id| {
                        queue
                            .batches
                            .get(id)
                            .is_some_and(|b| !queue.active.contains(&workspace(b)))
                    });
                    if let Some(index) = selected {
                        let id = queue.waiting.remove(index).unwrap();
                        let batch = queue.batches[&id].clone();
                        queue.active.insert(workspace(&batch));
                        break batch;
                    }
                    queue = self
                        .wake
                        .wait_timeout(queue, Duration::from_millis(300))
                        .unwrap()
                        .0;
                }
            };
            batch.state.lock().unwrap()["state"] = json!("scanning");
            let result = self.run(&batch);
            {
                let mut state = batch.state.lock().unwrap();
                if let Err(error) = result {
                    state["error"] = json!(error.message.chars().take(2048).collect::<String>());
                    state["state"] = json!(if error.code == "unknown" {
                        "needsReview"
                    } else if batch.cancel.load(Ordering::Acquire) || error.code == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    });
                } else {
                    let failed = count(&state, "failed");
                    state["state"] = json!(if batch.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else if failed > 0 {
                        "partial"
                    } else {
                        "completed"
                    });
                }
                state["conflict"] = Value::Null;
            }
            let state = batch.state.lock().unwrap().clone();
            if state["state"] == "completed" && state["kind"] != "download" {
                if let Some(input) = batch.parameters["inputId"].as_str() {
                    let _ = self.client.call(
                        "files.request",
                        json!({"action":"input.release","id":batch.file,"inputId":input}),
                    );
                }
            }
            let _ = self.client.call(
                "files.request",
                json!({"action":"batch.end","id":batch.file,"batchId":batch.id}),
            );
            let _=self.client.call("messages.publish",json!({"title":if state["state"]=="completed"{"文件任务完成"}else{"文件任务结束"},"body":format!("成功 {}，失败 {}，跳过 {}{}",state["completed"],state["failed"],state["skipped"],state["error"].as_str().map(|e|format!("；{e}")).unwrap_or_default()),"level":if state["state"]=="completed"{"success"}else{"warning"},"dedupeKey":batch.id}));
            self.queue.lock().unwrap().active.remove(&workspace(&batch));
            self.wake.notify_all();
        }
    }
    fn op(&self, batch: &Batch, operation: Value) -> Result<Value, TaskError> {
        if batch.cancel.load(Ordering::Acquire) {
            return Err(TaskError::new("cancelled", "文件任务已取消"));
        }
        let sequence = self.next()?.to_string();
        let request = json!({"action":"start","id":batch.file,"batchId":batch.id,"requestId":format!("{}/{}",batch.id,sequence),"sequence":sequence,"expectedContext":batch.context,"operation":operation});
        let readonly = ["inspect", "list", "next", "closeCursor"]
            .contains(&operation["kind"].as_str().unwrap_or(""));
        let pending_path = batch.journal.with_extension("pending.json");
        if !readonly {
            let mut f = File::create(&pending_path).map_err(|e| e.to_string())?;
            append(&mut f, &request)?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        let mut response = self
            .client
            .call_detailed("files.operations", request.clone());
        let mut id = None::<String>;
        let mut cancelled = false;
        loop {
            match response {
                Ok(status) => {
                    if let Some(value) = status["id"].as_str() {
                        id = Some(value.into());
                    }
                    if status["state"] == "completed" {
                        let id = id
                            .as_ref()
                            .ok_or_else(|| TaskError::new("unknown", "核心回执缺少操作身份"))?;
                        if readonly {
                            self.release_receipt(batch, id)?;
                        } else {
                            batch.receipts.lock().unwrap().push(id.clone());
                        }
                        batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
                        batch.state.lock().unwrap()["state"] = json!("running");
                        return Ok(status["result"].clone());
                    }
                    if ["failed", "cancelled", "unknown"]
                        .contains(&status["state"].as_str().unwrap_or(""))
                    {
                        batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
                        if let Some(id) = &id {
                            batch.receipts.lock().unwrap().push(id.clone());
                        }
                        let code = if status["state"] == "unknown"
                            || status["error"]["effect"] == "unknown"
                        {
                            "unknown"
                        } else {
                            status["error"]["code"].as_str().unwrap_or("io")
                        };
                        return Err(TaskError::new(
                            code,
                            status["error"]["message"]
                                .as_str()
                                .unwrap_or("文件操作失败"),
                        ));
                    }
                    if !["running", "committing"].contains(&status["state"].as_str().unwrap_or(""))
                    {
                        return Err(TaskError::new("unknown", "核心操作状态无效"));
                    }
                    if operation["kind"] == "copyFile" {
                        let mut s = batch.state.lock().unwrap();
                        s["currentBytes"] = status["received"].clone();
                        s["currentTotal"] = status["total"].clone();
                    }
                }
                Err(error) => {
                    if id.is_none()
                        && error.data["effect"] == "none"
                        && !["expired", "unknown"]
                            .contains(&error.data["code"].as_str().unwrap_or(""))
                    {
                        return Err(TaskError::new(
                            error.data["code"].as_str().unwrap_or("io"),
                            error.message,
                        ));
                    }
                    {
                        let mut s = batch.state.lock().unwrap();
                        s["state"] = json!("needsReview");
                        s["awaitingReceipt"] = json!(true);
                        s["error"] = json!(format!("操作结果待确认：{}", error.message));
                    }
                    let mut wait = batch.decision.lock().unwrap();
                    while !batch.confirm.swap(false, Ordering::AcqRel)
                        && !batch.cancel.load(Ordering::Acquire)
                        && !self.client.is_closed()
                    {
                        wait = batch
                            .decision_wake
                            .wait_timeout(wait, Duration::from_millis(300))
                            .unwrap()
                            .0;
                    }
                    drop(wait);
                    if self.client.is_closed() {
                        return Err(TaskError::new("unknown", "核心通信已结束，操作未确认"));
                    }
                }
            }
            if batch.cancel.load(Ordering::Acquire) && !cancelled {
                if let Err(error) = self.client.call_detailed(
                    "files.request",
                    json!({"action":"batch.cancel","id":batch.file,"batchId":batch.id}),
                ) {
                    response = Err(error);
                    thread::sleep(Duration::from_millis(300));
                    continue;
                }
                cancelled = true;
                if let Some(id) = &id {
                    let _ = self.client.call(
                        "files.operations",
                        json!({"action":"cancel","id":batch.file,"operationId":id}),
                    );
                }
            }
            thread::sleep(Duration::from_millis(60));
            response = self.client.call_detailed(
                "files.operations",
                if let Some(id) = &id {
                    json!({"action":"poll","id":batch.file,"operationId":id})
                } else {
                    json!({"action":"lookup","id":batch.file,"batchId":batch.id,"requestId":request["requestId"]})
                },
            );
        }
    }
    fn inspect(&self, batch: &Batch, endpoint: &Value) -> Result<Option<Value>, TaskError> {
        let value = self.op(batch, json!({"kind":"inspect","endpoint":endpoint}))?;
        Ok(if value["attrs"].is_null() {
            None
        } else {
            Some(value["attrs"].clone())
        })
    }
    fn release_receipt(&self, batch: &Batch, id: &str) -> Result<(), TaskError> {
        loop {
            let reply = self.client.call_detailed(
                "files.operations",
                json!({"action":"release","id":batch.file,"operationId":id}),
            );
            if reply
                .as_ref()
                .map_or_else(|error| error.data["code"] == "expired", |_| true)
            {
                batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
                return Ok(());
            }
            {
                let mut state = batch.state.lock().unwrap();
                state["state"] = json!("needsReview");
                state["awaitingReceipt"] = json!(true);
                state["error"] = json!(format!(
                    "结果已确认，回执释放待确认：{}",
                    reply.unwrap_err().message
                ));
            }
            let mut wait = batch.decision.lock().unwrap();
            while !batch.confirm.swap(false, Ordering::AcqRel)
                && !batch.cancel.load(Ordering::Acquire)
                && !self.client.is_closed()
            {
                wait = batch
                    .decision_wake
                    .wait_timeout(wait, Duration::from_millis(300))
                    .unwrap()
                    .0;
            }
            if batch.cancel.load(Ordering::Acquire) || self.client.is_closed() {
                batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
                return Err(TaskError::new(
                    "unknown",
                    "操作已结束，但核心回执释放未确认；请核对后关闭页面清理",
                ));
            }
        }
    }
    fn conflict(
        &self,
        batch: &Batch,
        item: &Item,
        target: &Value,
        type_conflict: bool,
    ) -> Result<Value, TaskError> {
        let group = if type_conflict { "type" } else { "file" };
        let allowed = if type_conflict {
            json!(["keepBoth", "skip", "cancel", "rename"])
        } else {
            json!(["replace", "keepBoth", "skip", "cancel", "rename"])
        };
        if let Some(policy) = batch.policy.lock().unwrap().get(group).cloned() {
            return Ok(policy);
        }
        *batch.decision.lock().unwrap() = None;
        {
            let mut state = batch.state.lock().unwrap();
            state["state"] = json!("awaitingConflict");
            state["conflict"] = json!({"allowedActions":allowed,"name":item.name,"typeConflict":type_conflict,"invalidName":target["kind"]=="invalidName","target":target,"itemId":self.next()?.to_string()});
        }
        let _=self.client.call("messages.publish",json!({"title":"文件任务需要处理同名条目","body":item.name.chars().take(256).collect::<String>(),"level":"warning","dedupeKey":format!("{}/conflict",batch.id)}));
        let mut decision = batch.decision.lock().unwrap();
        while decision.is_none() {
            if batch.cancel.load(Ordering::Acquire) || self.client.is_closed() {
                return Err(TaskError::new("cancelled", "文件任务已取消"));
            }
            decision = batch
                .decision_wake
                .wait_timeout(decision, Duration::from_millis(300))
                .unwrap()
                .0;
        }
        let value = decision.take().unwrap();
        drop(decision);
        {
            let mut state = batch.state.lock().unwrap();
            state["state"] = json!("running");
            state["conflict"] = Value::Null;
        }
        if value["applyAll"] == true && value["choice"] != "rename" {
            batch.policy.lock().unwrap()[group] = value.clone();
        }
        Ok(value)
    }
    fn run(&self, batch: &Batch) -> Result<(), TaskError> {
        let mut journal = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&batch.journal)
            .map_err(|e| e.to_string())?;
        journal.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        let work_path = self.directory.join(format!("{}.work.jsonl", batch.id));
        let dirs_path = self
            .directory
            .join(format!("{}.directories.jsonl", batch.id));
        let offsets_path = self.directory.join(format!("{}.offsets", batch.id));
        let children_path = self.directory.join(format!("{}.children.jsonl", batch.id));
        let _scratch = Scratch(vec![
            work_path.clone(),
            dirs_path.clone(),
            offsets_path.clone(),
            children_path.clone(),
        ]);
        let mut work = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&work_path)
            .map_err(|e| e.to_string())?;
        let kind = batch.parameters["kind"].as_str().unwrap_or("");
        if kind == "mkdir" || kind == "createEmpty" {
            let endpoint = json!({"side":"remote","path":batch.parameters["path"]});
            let result = self.op(
                batch,
                json!({"kind":kind,"endpoint":endpoint,"guard":{"kind":"absent"}}),
            );
            self.record(
                batch,
                &mut journal,
                &Item {
                    source: endpoint,
                    destination: Value::Null,
                    name: batch.parameters["path"].as_str().unwrap_or("").into(),
                    stamp: Value::Null,
                    delete_directory: false,
                },
                result.map(|_| 0),
            )?;
            return Ok(());
        }
        if let Some(retry) = batch.parameters["retryJournal"].as_str() {
            // The path comes only from a task owned by this process, never directly from the UI.
            for line in BufReader::new(File::open(retry).map_err(|e| e.to_string())?).lines() {
                let value: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                if value["state"] == "failed" && value["retryable"] == true {
                    append(&mut work, &value["item"])?;
                }
            }
        } else if kind == "upload" {
            let input = batch.parameters["inputId"].as_str().ok_or("缺少本地来源")?;
            let mut offset = 0;
            loop {
                let page = self.client.call(
                    "files.request",
                    json!({"action":"input.list","id":batch.file,"inputId":input,"offset":offset}),
                )?;
                for entry in page["entries"].as_array().ok_or("本地来源结果无效")? {
                    let name = entry["name"].as_str().ok_or("本地文件名无效")?;
                    let source = json!({"side":"local","inputId":input,"rootId":entry["rootId"],"relative":[]});
                    let target = json!({"side":"remote","path":batch.parameters["targetPath"]});
                    append(
                        &mut work,
                        &item_value(&Item {
                            source,
                            destination: child(&target, name),
                            name: name.into(),
                            stamp: Value::Null,
                            delete_directory: false,
                        }),
                    )?;
                }
                if let Some(next) = page["next"].as_u64() {
                    offset = next
                } else {
                    break;
                }
            }
        } else {
            let roots = batch.parameters["roots"].as_array().ok_or("缺少选中条目")?;
            let paths: Vec<&str> = roots.iter().filter_map(Value::as_str).collect();
            let mut seen = HashSet::new();
            for path in &paths {
                if !seen.insert(*path) {
                    continue;
                }
                if paths.iter().any(|other| {
                    other != path && path.starts_with(&format!("{}/", other.trim_end_matches('/')))
                }) {
                    continue;
                }
                let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                if name.is_empty() {
                    return Err("不能直接对服务器根目录执行此操作".into());
                }
                let source = json!({"side":"remote","path":path});
                let destination = if kind == "download" {
                    let base = json!({"side":"local","inputId":batch.parameters["inputId"],"rootId":"0","relative":[]});
                    if batch.parameters["single"] == true {
                        base
                    } else {
                        child(&base, name)
                    }
                } else if kind == "delete" {
                    Value::Null
                } else {
                    child(
                        &json!({"side":"remote","path":batch.parameters["targetPath"]}),
                        name,
                    )
                };
                if destination["side"] == "remote" {
                    let target = destination["path"].as_str().unwrap_or("");
                    if target == *path && kind == "move" {
                        return Err("不能移动到原位置".into());
                    }
                    if target.starts_with(&format!("{}/", path.trim_end_matches('/'))) {
                        return Err("不能将目录复制或移动到自身子目录".into());
                    }
                }
                append(
                    &mut work,
                    &item_value(&Item {
                        source,
                        destination,
                        name: name.into(),
                        stamp: Value::Null,
                        delete_directory: false,
                    }),
                )?;
            }
        }
        let mut reader = BufReader::new(File::open(&work_path).map_err(|e| e.to_string())?);
        let mut dirs = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&dirs_path)
            .map_err(|e| e.to_string())?;
        let mut offsets = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&offsets_path)
            .map_err(|e| e.to_string())?;
        let result = (|| {
            loop {
                if batch.cancel.load(Ordering::Acquire) {
                    return Err(TaskError::new("cancelled", "文件任务已取消"));
                }
                let mut line = String::new();
                if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                    break;
                }
                let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                let mut item = parse_item(&value)?;
                {
                    let mut state = batch.state.lock().unwrap();
                    state["state"] = json!("running");
                    state["current"] = json!(item.name);
                    state["currentBytes"] = json!("0");
                    state["currentTotal"] = Value::Null;
                }
                let result =
                    self.process(batch, &mut item, &mut work, &mut dirs, &mut offsets, kind);
                self.record(batch, &mut journal, &item, result)?;
            }
            if kind == "delete" {
                let mut directory_reader =
                    BufReader::new(File::open(&dirs_path).map_err(|e| e.to_string())?);
                let mut position = offsets.metadata().map_err(|e| e.to_string())?.len();
                while position >= 8 {
                    position -= 8;
                    offsets
                        .seek(SeekFrom::Start(position))
                        .map_err(|e| e.to_string())?;
                    let mut bytes = [0u8; 8];
                    offsets.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                    let offset = u64::from_le_bytes(bytes);
                    directory_reader
                        .seek(SeekFrom::Start(offset))
                        .map_err(|e| e.to_string())?;
                    let mut line = String::new();
                    directory_reader
                        .read_line(&mut line)
                        .map_err(|e| e.to_string())?;
                    let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                    let item = parse_item(&value)?;
                    let result = self.op(
                        batch,
                        json!({"kind":"removeDirectory","endpoint":item.source}),
                    );
                    self.record(batch, &mut journal, &item, result.map(|_| 0))?;
                }
            }
            Ok(())
        })();
        drop(reader);
        drop(work);
        drop(dirs);
        let _ = fs::remove_file(work_path);
        let _ = fs::remove_file(dirs_path);
        result
    }
    fn process(
        &self,
        batch: &Batch,
        item: &mut Item,
        work: &mut File,
        dirs: &mut File,
        offsets: &mut File,
        kind: &str,
    ) -> Result<u64, TaskError> {
        let attrs = self
            .inspect(batch, &item.source)?
            .ok_or_else(|| TaskError::new("missing", "源条目已不存在"))?;
        if item.delete_directory {
            if kind != "delete" || attrs["kind"] != "directory" {
                return Err(TaskError::new("invalid", "非删除任务不能清理源"));
            }
            self.op(
                batch,
                json!({"kind":"removeDirectory","endpoint":item.source}),
            )?;
            return Ok(0);
        }
        if !item.stamp.is_null() && attrs["kind"] != "directory" && item.stamp != attrs["stamp"] {
            return Err(TaskError::new(
                "sourceChanged",
                "枚举后源条目已变化，未修改源或目标",
            ));
        }
        item.stamp = attrs["stamp"].clone();
        if attrs["kind"] == "directory" && item.destination["side"] == "remote" {
            let path = item.destination["path"].as_str().ok_or("缺少目标路径")?;
            let parent = path
                .rsplit_once('/')
                .map(|p| if p.0.is_empty() { "/" } else { p.0 })
                .ok_or("目标需要绝对路径")?;
            if let Some(parent) = self.inspect(batch, &json!({"side":"remote","path":parent}))? {
                if let (Some(source), Some(target)) =
                    (attrs["resolved"].as_str(), parent["resolved"].as_str())
                {
                    if within(target, source) {
                        return Err(TaskError::new(
                            "invalid",
                            "不能将目录复制或移动到自身子目录（包括目录别名）",
                        ));
                    }
                }
            }
        }
        if kind == "delete" && attrs["kind"] != "directory" {
            self.op(
                batch,
                json!({"kind":"removeFile","endpoint":item.source,"sourceStamp":attrs["stamp"]}),
            )?;
            return Ok(0);
        }
        if attrs["kind"] == "link" || attrs["kind"] == "other" {
            return Err(TaskError::new("skipped", "符号链接与特殊文件不跟随传输"));
        }
        let directory = attrs["kind"] == "directory";
        let mut selected_guard = json!({"kind":"absent"});
        if kind != "delete" {
            for _ in 0..100 {
                let target = match self.inspect(batch, &item.destination) {
                    Err(error) if error.code == "invalidName" => {
                        Some(json!({"kind":"invalidName"}))
                    }
                    value => value?,
                };
                if directory
                    && item.source != item.destination
                    && target.as_ref().is_some_and(|a| a["kind"] == "directory")
                {
                    break;
                }
                if target.is_none() {
                    break;
                }
                let type_conflict = target.as_ref().unwrap()["kind"] != attrs["kind"];
                let decision =
                    self.conflict(batch, item, target.as_ref().unwrap(), type_conflict)?;
                match decision["choice"].as_str() {
                    Some("skip") => return Err(TaskError::new("skipped", "用户选择跳过")),
                    Some("cancel") => {
                        batch.cancel.store(true, Ordering::Release);
                        return Err(TaskError::new("cancelled", "文件任务已取消"));
                    }
                    Some("replace") if !type_conflict && !directory => {
                        selected_guard =
                            json!({"kind":"match","stamp":target.as_ref().unwrap()["stamp"]});
                        break;
                    }
                    Some("keepBoth") => {
                        let mut found = false;
                        for index in 1..10000 {
                            let name = duplicate_name(&item.name, index);
                            let candidate = sibling(&item.destination, &name);
                            if self.inspect(batch, &candidate)?.is_none() {
                                item.destination = candidate;
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            return Err("无法生成未占用的文件名".into());
                        }
                    }
                    Some("rename") => {
                        let name = decision["name"].as_str().ok_or("缺少新文件名")?;
                        if name.is_empty()
                            || name == "."
                            || name == ".."
                            || name.contains(['\0', '/'])
                        {
                            return Err("新文件名无效".into());
                        }
                        item.destination = sibling(&item.destination, name);
                    }
                    _ => return Err("无效冲突选择".into()),
                }
            }
        }
        if directory {
            if kind != "delete" && self.inspect(batch, &item.destination)?.is_none() {
                self.op(
                    batch,
                    json!({"kind":"mkdir","endpoint":item.destination,"guard":{"kind":"absent"}}),
                )?;
            }
            // Commit the children to the work queue only after enumeration completes.
            // A failed directory retry cannot replay children that already completed.
            let children_path = self.directory.join(format!("{}.children.jsonl", batch.id));
            let mut children = File::create(&children_path).map_err(|e| e.to_string())?;
            let mut cursor = Value::Null;
            let listed = (|| {
                loop {
                    let page = self.op(
                        batch,
                        if cursor.is_null() {
                            json!({"kind":"list","endpoint":item.source})
                        } else {
                            json!({"kind":"next","cursor":cursor})
                        },
                    )?;
                    cursor = page["cursor"].clone();
                    for entry in page["entries"].as_array().ok_or("目录页无效")? {
                        let name = entry["name"].as_str().ok_or("文件名无效")?;
                        append(
                            &mut children,
                            &item_value(&Item {
                                source: child(&item.source, name),
                                destination: if kind == "delete" {
                                    Value::Null
                                } else {
                                    child(&item.destination, name)
                                },
                                name: name.into(),
                                stamp: entry["attrs"]["stamp"].clone(),
                                delete_directory: false,
                            }),
                        )?;
                    }
                    if page["more"] != true {
                        break;
                    }
                }
                Ok::<_, TaskError>(())
            })();
            if !cursor.is_null() {
                let _ = self.op(batch, json!({"kind":"closeCursor","cursor":cursor}));
            }
            listed?;
            drop(children);
            std::io::copy(
                &mut File::open(&children_path).map_err(|e| e.to_string())?,
                work,
            )
            .map_err(|e| e.to_string())?;
            work.flush().map_err(|e| e.to_string())?;
            if kind == "delete" {
                let offset = dirs.metadata().map_err(|e| e.to_string())?.len();
                offsets
                    .write_all(&offset.to_le_bytes())
                    .and_then(|_| offsets.flush())
                    .map_err(|e| e.to_string())?;
                let mut cleanup = item.clone();
                cleanup.delete_directory = true;
                append(dirs, &item_value(&cleanup))?;
            }
            return if kind == "delete" {
                Err(TaskError::new("scanned", "目录已枚举"))
            } else {
                Ok(0)
            };
        }
        let copied=self.op(batch,json!({"kind":"copyFile","source":item.source,"destination":item.destination,"sourceStamp":attrs["stamp"],"destinationGuard":selected_guard}))?;
        if copied["cleanupWarning"].is_string() {
            batch.state.lock().unwrap()["warning"] = copied["cleanupWarning"].clone();
        }
        Ok(copied["size"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }
    fn record(
        &self,
        batch: &Batch,
        journal: &mut File,
        item: &Item,
        result: Result<u64, TaskError>,
    ) -> Result<(), TaskError> {
        if result.as_ref().is_err_and(|e| e.code == "scanned") {
            return Ok(());
        }
        let (status, error, bytes) = match result {
            Ok(bytes) => ("completed", None, bytes),
            Err(error) => {
                let status = if error.code == "skipped" {
                    "skipped"
                } else if error.code == "cancelled" {
                    "cancelled"
                } else if error.code == "unknown" {
                    "unknown"
                } else {
                    "failed"
                };
                (status, Some(error), 0)
            }
        };
        let offset = journal
            .metadata()
            .map_err(|e| TaskError::new("unknown", e))?
            .len();
        let record = json!({"item":item_value(item),"state":status,"retryable":error.as_ref().is_some_and(TaskError::retryable),"errorCode":error.as_ref().map(|e|&e.code),"error":error.as_ref().map(|e|e.message.chars().take(2048).collect::<String>())});
        loop {
            let persisted = (|| -> Result<(), String> {
                journal.set_len(offset).map_err(|e| e.to_string())?;
                journal
                    .seek(SeekFrom::Start(offset))
                    .map_err(|e| e.to_string())?;
                append(journal, &record)?;
                journal.sync_all().map_err(|e| e.to_string())
            })();
            if persisted.is_ok() {
                break;
            }
            {
                let mut state = batch.state.lock().unwrap();
                state["state"] = json!("needsReview");
                state["awaitingReceipt"] = json!(true);
                state["error"] = json!(format!(
                    "结果记录未持久化，保留核心回执：{}",
                    persisted.unwrap_err()
                ));
            }
            let mut wait = batch.decision.lock().unwrap();
            while !batch.confirm.swap(false, Ordering::AcqRel)
                && !batch.cancel.load(Ordering::Acquire)
                && !self.client.is_closed()
            {
                wait = batch
                    .decision_wake
                    .wait_timeout(wait, Duration::from_millis(300))
                    .unwrap()
                    .0;
            }
            if batch.cancel.load(Ordering::Acquire) || self.client.is_closed() {
                batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
                return Err(TaskError::new(
                    "unknown",
                    "结果记录未持久化；请保留并核对当前记录",
                ));
            }
        }
        batch.state.lock().unwrap()["awaitingReceipt"] = json!(false);
        {
            let mut state = batch.state.lock().unwrap();
            let key = if status == "cancelled" {
                "cancelledItems"
            } else if status == "unknown" {
                "failed"
            } else {
                status
            };
            let n = count(&state, key) + 1;
            state[key] = json!(n.to_string());
            if error.as_ref().is_some_and(TaskError::retryable) {
                state["retryable"] = json!((count(&state, "retryable") + 1).to_string());
            }
            let n = count(&state, "bytes") + bytes;
            state["bytes"] = json!(n.to_string());
            if let Some(error) = &error {
                state["lastError"] = json!(error.message.chars().take(1024).collect::<String>());
            }
        }
        loop {
            let id = batch.receipts.lock().unwrap().last().cloned();
            let Some(id) = id else { break };
            self.release_receipt(batch, &id)?;
            batch.receipts.lock().unwrap().pop();
        }
        let _ = fs::remove_file(batch.journal.with_extension("pending.json"));
        if let Some(error) = error {
            if error.code == "unknown"
                || error.code == "contextChanged"
                || error.code == "cancelled"
            {
                return Err(error);
            }
        }
        Ok(())
    }
}
fn count(value: &Value, key: &str) -> u64 {
    value[key]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
fn endpoint_label(value: &Value) -> String {
    let text = if let Some(path) = value["path"].as_str() {
        path.to_string()
    } else {
        value["relative"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default()
    };
    text.chars().take(512).collect()
}
fn append(file: &mut File, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    line.push(b'\n');
    file.write_all(&line)
        .and_then(|_| file.flush())
        .map_err(|e| e.to_string())
}
fn item_value(item: &Item) -> Value {
    json!({"source":item.source,"destination":item.destination,"name":item.name,"stamp":item.stamp,"deleteDirectory":item.delete_directory})
}
fn parse_item(value: &Value) -> Result<Item, String> {
    if value["cleanup"] == true || !value["targetStamp"].is_null() {
        return Err("旧移动清理记录不再接纳".into());
    }
    Ok(Item {
        source: value["source"].clone(),
        destination: value["destination"].clone(),
        name: value["name"].as_str().ok_or("任务条目无效")?.into(),
        stamp: value["stamp"].clone(),
        delete_directory: value["deleteDirectory"] == true,
    })
}
fn child(base: &Value, name: &str) -> Value {
    let mut value = base.clone();
    if base["side"] == "remote" {
        value["path"] = json!(format!(
            "{}/{}",
            base["path"].as_str().unwrap_or("").trim_end_matches('/'),
            name
        ));
    } else {
        value["relative"].as_array_mut().unwrap().push(json!(name));
    }
    value
}
fn sibling(base: &Value, name: &str) -> Value {
    let mut value = base.clone();
    if base["side"] == "remote" {
        let parent = base["path"]
            .as_str()
            .unwrap_or("")
            .rsplit_once('/')
            .map(|v| v.0)
            .unwrap_or("");
        value["path"] = json!(format!("{parent}/{name}"));
    } else {
        let parts = value["relative"].as_array_mut().unwrap();
        if let Some(last) = parts.last_mut() {
            *last = json!(name);
        } else {
            value["rootName"] = json!(name);
        }
    }
    value
}
fn duplicate_name(name: &str, index: usize) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if !stem.is_empty() {
            return format!("{stem} ({index}).{ext}");
        }
    }
    format!("{name} ({index})")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_names_keep_extensions_and_dotfiles() {
        assert_eq!(duplicate_name("file.txt", 2), "file (2).txt");
        assert_eq!(duplicate_name(".env", 1), ".env (1)");
    }
    #[test]
    fn child_paths_keep_remote_and_local_rules_separate() {
        assert_eq!(
            child(&json!({"side":"remote","path":"/"}), "中文")["path"],
            "/中文"
        );
        assert_eq!(
            child(
                &json!({"side":"local","inputId":"x","rootId":"0","relative":[]}),
                "中文"
            )["relative"],
            json!(["中文"])
        );
    }
    #[test]
    fn cleanup_retry_and_single_download_rename_keep_their_scope() {
        let item = Item {
            source: json!({"side":"remote","path":"/a"}),
            destination: Value::Null,
            name: "a".into(),
            stamp: json!("stamp"),
            delete_directory: true,
        };
        assert!(parse_item(&item_value(&item)).unwrap().delete_directory);
        assert_eq!(
            sibling(
                &json!({"side":"local","inputId":"x","rootId":"0","relative":[]}),
                "copy.txt"
            )["rootName"],
            "copy.txt"
        );
        assert!(within("/app/file", "/app"));
        assert!(!within("/app2/file", "/app"));
        assert_eq!(
            affected_paths(&json!({"kind":"download","roots":["/a"]})),
            Vec::<&str>::new()
        );
    }
}
