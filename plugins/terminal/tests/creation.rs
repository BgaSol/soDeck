//! Exercise the actual plugin process against a deterministic core RPC peer.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};
struct Plugin {
    child: Child,
    input: ChildStdin,
    frames: Receiver<Value>,
}
impl Plugin {
    fn new(launch: bool, open: bool) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sodeck-terminal"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, frames) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut plugin = Self {
            child,
            input,
            frames,
        };
        let mut caps = vec!["terminal.v1", "viewData.v1", "terminal.history.v1"];
        if launch {
            caps.push("terminal.launch.v1");
        }
        if open {
            caps.push("ui.openView.v1");
        }
        plugin.request("init", "initialize", json!({"capabilities":caps}));
        let initialized = plugin.next();
        assert_eq!(initialized["result"]["uiActionsApiVersion"], "1");
        assert_eq!(initialized["result"]["uiApiVersion"], "2");
        plugin
    }
    fn send(&mut self, frame: Value) {
        writeln!(self.input, "{frame}").unwrap();
        self.input.flush().unwrap();
    }
    fn request(&mut self, id: &str, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    }
    fn reply(&mut self, request: &Value, value: Value) {
        self.send(json!({"jsonrpc":"2.0","id":request["id"],"result":value}));
    }
    fn next(&self) -> Value {
        self.frames
            .recv_timeout(Duration::from_secs(3))
            .expect("plugin response")
    }
}
impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn config() -> Value {
    json!({"version":1,"title":"container shell","launch":{"kind":"command","command":"container exec --interactive shell"},"directoryIntegration":"off"})
}
#[test]
fn commands_register_pages_and_reopen_preserves_ticket_across_lost_receipts() {
    let mut plugin = Plugin::new(true, true);
    plugin.request("commands-init","initialize",json!({"capabilities":["commands.v1","terminal.v1","terminal.launch.v1","viewData.v1","terminal.history.v1","ui.openView.v1"]}));
    let init = plugin.next();
    assert_eq!(init["result"]["commandsApiVersion"], "1");
    assert_eq!(
        init["result"]["commandHandlers"],
        json!(["terminal.open.v1"])
    );
    let context = json!({"workspaceId":"w","serverId":"s","connectionId":"c","generation":1});
    plugin.request("command","commands.handle",json!({"commandId":"command-1","context":context,"name":"terminal.open.v1","payload":config()}));
    let open = plugin.next();
    assert_eq!(open["method"], "ui.openView");
    assert_eq!(open["params"]["commandId"], "command-1");
    assert_eq!(open["params"]["requestId"], "open:command-1");
    assert_eq!(open["params"]["params"], config());
    plugin.reply(&open, json!({"viewId":"page"}));
    assert_eq!(plugin.next()["result"]["viewId"], "page");
    plugin.request("rejected","commands.handle",json!({"commandId":"command-2","context":context,"name":"terminal.open.v1","payload":config()}));
    let rejected = plugin.next();
    plugin.send(json!({"jsonrpc":"2.0","id":rejected["id"],"error":{"code":-32000,"message":"目标正在交接","data":{"code":"targetBusy"}}}));
    let rejected = plugin.next();
    assert_eq!(rejected["error"]["data"]["code"], "targetBusy");
    assert_eq!(rejected["error"]["message"], "目标正在交接");
    let event = json!({"viewId":"source","context":context,"data":{"action":"createTerminal","requestId":"intent-1","params":config()}});
    plugin.request("reopen", "ui.event", event.clone());
    let submit = plugin.next();
    assert_eq!(submit["method"], "commands.submit");
    assert_eq!(submit["params"]["sourceViewId"], "source");
    plugin
        .send(json!({"jsonrpc":"2.0","id":submit["id"],"error":{"code":-32003,"message":"lost"}}));
    assert_eq!(plugin.next()["result"]["data"]["status"], "pending");
    plugin.request(
        "check",
        "ui.event",
        json!({"viewId":"source","context":context,"data":{"action":"terminalCommandStatus"}}),
    );
    let get = plugin.next();
    assert_eq!(get["method"], "commands.get");
    assert_eq!(get["params"]["requestId"], "intent-1");
    plugin.reply(&get, json!({"status":"completed","viewId":"next"}));
    assert_eq!(plugin.next()["result"]["data"]["viewId"], "next");
    plugin.request("same", "ui.event", event);
    assert_eq!(plugin.next()["result"]["data"]["viewId"], "next");
    assert!(plugin
        .frames
        .recv_timeout(Duration::from_millis(100))
        .is_err());
}
fn mount(view: &str, params: Value) -> Value {
    json!({"viewId":view,"params":params,"context":{"workspaceId":"w","serverId":"s","connectionId":"c","generation":1}})
}

#[test]
fn a_core_without_large_history_support_is_rejected_before_creating_a_pty() {
    let mut plugin = Plugin::new(false, false);
    plugin.request(
        "legacy",
        "initialize",
        json!({"capabilities":["terminal.v1","viewData.v1"]}),
    );
    let response = plugin.next();
    assert!(response["error"]
        .to_string()
        .contains("terminal.history.v1"));
    assert!(plugin
        .frames
        .recv_timeout(Duration::from_millis(100))
        .is_err());
}

#[test]
fn action_and_reopen_create_pages_before_pty_and_preserve_config() {
    let mut p = Plugin::new(true, true);
    let context = mount("v", Value::Null)["context"].clone();
    p.request(
        "action",
        "ui.action",
        json!({"context":context,"requestId":"r","actionId":"new-terminal","data":config()}),
    );
    let open = p.next();
    assert_eq!(open["method"], "ui.openView");
    assert_eq!(open["params"]["params"], config());
    assert_eq!(open["params"]["expectedContext"], context);
    p.reply(&open, json!({"viewId":"v"}));
    assert_eq!(p.next()["result"]["viewId"], "v");
    p.request("uncreated", "ui.getView", mount("v", config()));
    assert!(p.next().get("error").is_some());
    assert!(p.frames.recv_timeout(Duration::from_millis(100)).is_err());
    p.request("create", "ui.createView", mount("v", config()));
    let launch = p.next();
    assert_eq!(launch["method"], "terminal.open");
    assert_eq!(launch["params"]["command"], config()["launch"]["command"]);
    p.reply(&launch, json!({"id":"pty"}));
    let mounted = p.next();
    assert_eq!(mounted["result"]["data"]["createParams"], config());
    assert_eq!(mounted["result"]["data"]["integration"]["mode"], "off");
    p.request(
        "remount",
        "ui.getView",
        mount("v", json!({"launch":{"kind":"invalid"}})),
    );
    assert_eq!(p.next()["result"]["data"]["streamId"], "pty");
    p.request("reopen","ui.event",json!({"viewId":"v","context":context,"data":{"action":"createTerminal","requestId":"r2","params":config()}}));
    let reopen = p.next();
    assert_eq!(reopen["method"], "ui.openView");
    assert_eq!(reopen["params"]["params"], config());
    p.reply(&reopen, json!({"viewId":"v2"}));
    assert_eq!(p.next()["result"]["data"]["viewId"], "v2");
}
#[test]
fn concurrent_creates_share_one_initialization_and_reads_do_not_create() {
    let mut p = Plugin::new(true, true);
    p.request("first", "ui.createView", mount("one", config()));
    let first = p.next();
    assert_eq!(first["method"], "terminal.open");
    p.request("candidate", "ui.getView", mount("one", Value::Null));
    let candidate = p.next();
    assert_eq!(candidate["id"], "candidate");
    assert!(candidate.get("error").is_some());
    p.request("same", "ui.createView", mount("one", config()));
    p.request("different", "ui.createView", mount("two", config()));
    let second = p.next();
    assert_eq!(second["method"], "terminal.open");
    assert_eq!(second["params"]["viewId"], "two");
    p.reply(&second, json!({"id":"pty-two"}));
    assert_eq!(p.next()["id"], "different");
    p.reply(&first, json!({"id":"pty-one"}));
    for _ in 0..2 {
        assert_eq!(p.next()["result"]["data"]["streamId"], "pty-one");
    }
    assert!(p.frames.recv_timeout(Duration::from_millis(100)).is_err());
}
#[test]
fn disposal_during_initialization_closes_late_pty_and_failure_is_cached() {
    let mut p = Plugin::new(true, true);
    p.request("create", "ui.createView", mount("closing", config()));
    let launch = p.next();
    p.request("dispose", "ui.disposeView", json!({"viewId":"closing"}));
    assert_eq!(p.next()["id"], "dispose");
    p.reply(&launch, json!({"id":"late-pty"}));
    let close = p.next();
    assert_eq!(close["method"], "terminal.close");
    assert_eq!(close["params"]["id"], "late-pty");
    p.reply(&close, json!({}));
    assert!(p.next().get("error").is_some());
    p.request("failure", "ui.createView", mount("failed", config()));
    let launch = p.next();
    p.send(
        json!({"jsonrpc":"2.0","id":launch["id"],"error":{"code":-32000,"message":"PTY failure"}}),
    );
    assert!(p.next().get("error").is_some());
    p.request("repeat", "ui.createView", mount("failed", config()));
    let repeated = p.next();
    assert_eq!(repeated["id"], "repeat");
    assert!(repeated.get("error").is_some());
}
#[test]
fn missing_optional_launch_capability_rejects_command_and_opens_plain_shell() {
    let mut p = Plugin::new(false, false);
    p.request("command", "ui.createView", mount("command", config()));
    assert!(p.next().get("error").is_some());
    p.request("host", "ui.createView", mount("host", Value::Null));
    let settings = p.next();
    assert_eq!(settings["method"], "settings.get");
    p.reply(&settings, json!({"values":{"directoryMode":"auto"}}));
    let launch = p.next();
    assert_eq!(launch["method"], "terminal.open");
    assert!(launch["params"].get("command").is_none());
    p.reply(&launch, json!({"id":"host-pty"}));
    assert_eq!(
        p.next()["result"]["data"]["integration"]["reasonCode"],
        "old-core"
    );
}
