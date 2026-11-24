use super::{
    contract::Range,
    records::{Draft, Parser, Stamp},
};
use crate::{cli, connection::Snapshot, runtime};
use serde_json::{json, Value};
use sodeck_plugin_client::{
    protocol::{requests as api, session::ProcessStart},
    Client,
};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Metadata {
    pub binding: Snapshot,
    pub container: String,
    pub name: String,
    pub status: String,
    pub tty: bool,
    pub now: Stamp,
    pub started: Option<Stamp>,
    pub finished: Option<Stamp>,
    pub engine: Value,
}
impl Metadata {
    pub fn read(app: &runtime::Shared, client: &Client, view: &str) -> Result<Self, String> {
        let expected = app.lock().unwrap().snapshot(view)?;
        let binding = runtime::bound_target(app, view, &expected)?;
        let target = runtime::log_target(app, client, view, &expected)?;
        Ok(Self {
            binding,
            container: target["containerId"]
                .as_str()
                .ok_or("日志容器身份无效")?
                .into(),
            name: crate::commands::text(target["name"].as_str().unwrap_or("容器"), 256),
            status: target["status"].as_str().unwrap_or("unknown").into(),
            tty: target["tty"] == true,
            now: Stamp::parse(
                target["engine"]["systemTime"]
                    .as_str()
                    .ok_or("Engine未提供源时间")?,
            )?,
            started: target["startedAt"]
                .as_str()
                .and_then(|s| Stamp::parse(s).ok())
                .filter(|s| s.0 > 0),
            finished: target["finishedAt"]
                .as_str()
                .and_then(|s| Stamp::parse(s).ok())
                .filter(|s| s.0 > 0),
            engine: json!({"id":target["engine"]["id"],"name":target["engine"]["name"],"version":target["engine"]["version"],"systemTime":target["engine"]["systemTime"]}),
        })
    }
    pub fn value(&self) -> Value {
        json!({"name":self.name,"containerId":self.container,"status":self.status,"tty":self.tty,"engine":self.engine,"endpoint":self.binding.endpoint.uri(),"context":self.binding.context,"startedAt":self.started.map(Stamp::text),"finishedAt":self.finished.map(Stamp::text)})
    }
    pub fn selection(&self, range: &Range) -> Result<Selection, String> {
        let mut selected = Selection {
            from: None,
            end: self.now,
            lower: None,
            upper: self.now,
            live: false,
            focus: None,
        };
        match range {
            Range::Latest => selected.live = self.status == "running",
            Range::Recent { seconds } => {
                if ![900, 3600, 86400].contains(seconds) {
                    return Err("日志时段无效".into());
                }
                selected.from = Some(self.now.before(*seconds));
                selected.lower = selected.from;
            }
            Range::Startup => {
                selected.from = Some(self.started.ok_or("Engine未提供可靠的最近启动时间")?);
                selected.lower = selected.from;
                if self.status != "running" {
                    selected.end = self.finished.ok_or("Engine未提供可靠的最近退出时间")?;
                    selected.upper = selected.end;
                }
            }
            Range::Custom { from, to } => {
                selected.from = Some(Stamp::seconds(*from));
                selected.end = Stamp::seconds(*to);
                selected.lower = selected.from;
                selected.upper = selected.end;
            }
            Range::At { at } => {
                let at = Stamp::seconds(*at);
                if at > self.now {
                    return Err("定位时间超过Engine当前时间".into());
                }
                selected.from = Some(at.before(300));
                selected.end = at.add(300).min(self.now);
                selected.focus = Some(at);
            }
        }
        if selected.end > self.now || selected.from.is_some_and(|start| start >= selected.end) {
            return Err("日志时间范围无效或超过Engine当前时间".into());
        }
        Ok(selected)
    }
}
#[derive(Clone)]
pub struct Selection {
    pub from: Option<Stamp>,
    pub end: Stamp,
    pub lower: Option<Stamp>,
    pub upper: Stamp,
    pub live: bool,
    pub focus: Option<Stamp>,
}
impl Selection {
    pub fn value(&self) -> Value {
        json!({"from":self.from.map(Stamp::text),"to":self.end.text(),"focus":self.focus.map(Stamp::text)})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Preview,
    Follow,
    BoundaryEarlier,
    BoundaryLater,
    Earlier,
    Later,
    Search,
}
pub struct Page {
    pub rows: Vec<Draft>,
    pub finished: bool,
    pub complete: bool,
    pub waiting: bool,
    pub status: Value,
}
pub struct Reader {
    pub purpose: Purpose,
    pub from: Option<Stamp>,
    pub end: Option<Stamp>,
    pub tail: Option<usize>,
    pub rows: usize,
    pub clipped: bool,
    pub parser: Parser,
    pub native: Value,
    pub offset: u64,
    pub deadline: Instant,
    ticket: ProcessStart,
    id: Option<String>,
    cancelled: bool,
}
impl Reader {
    pub fn open(
        app: &runtime::Shared,
        client: &Client,
        view: &str,
        meta: &Metadata,
        purpose: Purpose,
        from: Option<Stamp>,
        end: Option<Stamp>,
        tail: Option<usize>,
    ) -> Result<Self, String> {
        let container = app
            .lock()
            .unwrap()
            .views
            .get(view)
            .ok_or("日志页已关闭")?
            .container
            .clone();
        let mut args = vec!["container".into(), "logs".into(), "--timestamps".into()];
        if let Some(from) = from {
            args.extend(["--since".into(), from.text()]);
        }
        if let Some(end) = end {
            args.extend(["--until".into(), end.text()]);
        } else {
            args.push("--follow".into());
        }
        if let Some(tail) = tail {
            args.extend(["--tail".into(), tail.to_string()]);
        }
        args.push(container.clone());
        let prepared = cli::prepare(
            client,
            view,
            &meta.binding,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            Some(&container),
            false,
        )
        .map_err(|e| e.message)?;
        let (params, response) = runtime::open_stream(
            app,
            client,
            view,
            &meta.binding,
            "logs",
            prepared,
            16 * 1024 * 1024,
            false,
        )?;
        let ticket = serde_json::from_value(params).map_err(|_| "日志启动票据无效")?;
        let id = response
            .as_ref()
            .and_then(|r| r["processId"].as_str())
            .map(str::to_owned);
        Ok(Self {
            purpose,
            from,
            end,
            tail,
            rows: 0,
            clipped: false,
            parser: Parser::new(meta.tty),
            native: json!({"state":"opening"}),
            offset: 0,
            deadline: Instant::now()
                + Duration::from_secs(if purpose == Purpose::Preview { 30 } else { 120 }),
            ticket,
            id,
            cancelled: false,
        })
    }
    fn resolve(&mut self, client: &Client) -> Result<bool, String> {
        if self.id.is_some() {
            return Ok(true);
        }
        let found = client
            .request(api::call::ProcessLookup(self.ticket.clone()))
            .map_err(|e| e.message)?;
        self.id = found.process_id;
        Ok(self.id.is_some())
    }
    pub fn poll(&mut self, client: &Client) -> Result<Page, String> {
        if !self.resolve(client)? {
            return Ok(Page {
                rows: vec![],
                finished: false,
                complete: false,
                waiting: true,
                status: json!({"state":"opening","message":"原日志启动回执待确认"}),
            });
        }
        let id = self.id.as_ref().unwrap().clone();
        if self.end.is_some() && Instant::now() > self.deadline && !self.cancelled {
            self.cancel(client);
        }
        let page = match client.request(api::call::ProcessRead(api::ProcessRead {
            process_id: id.clone(),
            offset: self.offset.to_string(),
        })) {
            Ok(page) => page,
            Err(error) => {
                if error.message.contains("bufferGap") || error.message.contains("invalidOffset") {
                    if let Ok(status) =
                        client.request(api::call::ProcessStatus(api::Process { process_id: id }))
                    {
                        self.offset = status
                            .earliest_available_offset
                            .parse()
                            .map_err(|_| "日志输出水位无效")?;
                        self.parser.gap();
                    }
                    return Err("日志运输缓冲出现缺口，当前读取不能证明完整".into());
                }
                return Err(format!("日志状态暂时不可用：{}", error.message));
            }
        };
        let next = page
            .next_offset
            .parse::<u64>()
            .map_err(|_| "日志输出偏移无效")?;
        if next < self.offset {
            return Err("日志输出偏移倒退".into());
        }
        self.offset = next;
        let mut rows = self.parser.append(&page.chunks)?;
        let finished = page.status.worker_finished && page.next_offset == page.watermark;
        if finished {
            rows.extend(self.parser.finish(crate::runtime::now()));
        }
        let outside = |r: &Draft| {
            r.time.is_some_and(|time| {
                self.from.is_some_and(|start| time < start)
                    || self.end.is_some_and(|end| time >= end)
            })
        };
        rows.retain(|r| !outside(r));
        self.rows += rows.len();
        let complete = finished
            && page.status.output_complete
            && page.status.remote_exit_confirmed
            && page.status.exit_code == Some(0)
            && !self.cancelled;
        self.native = serde_json::to_value(&page.status).unwrap();
        Ok(Page {
            waiting: page.chunks.is_empty() && !finished,
            rows,
            finished,
            complete,
            status: self.native.clone(),
        })
    }
    pub fn cancel(&mut self, client: &Client) {
        self.cancelled = true;
        if let Some(id) = &self.id {
            let _ = client.request(api::call::ProcessCancel(api::Process {
                process_id: id.clone(),
            }));
        }
    }
    pub fn release(&mut self, client: &Client) -> bool {
        if !self.resolve(client).unwrap_or(false) {
            return false;
        }
        match client.request(api::call::ProcessRelease(api::Process {
            process_id: self.id.as_ref().unwrap().clone(),
        })) {
            Ok(r) => r.resource_released,
            Err(e) => e.message.contains("原进程资源已释放") || e.data["code"] == "processNotFound",
        }
    }
    pub fn raw_bytes(&self) -> u64 {
        self.parser.raw_bytes
    }
}
