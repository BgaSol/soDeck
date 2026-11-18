//! Sidebar composition; identity is local and does not depend on a successful observation.
use crate::{
    query, response,
    targets::{self, Target},
    task::CapturedTask,
    tasks::{failure, App},
};
use serde_json::{json, Value};
use std::sync::Arc;

fn current(app: &App, view: &str, target: &Arc<Target>) -> Result<(), Value> {
    if !Arc::ptr_eq(&app.view_target(view)?, target) {
        return Err(failure("staleContext", "读取期间连接代次已改变"));
    }
    Ok(())
}
pub fn snapshot(app: &App, view: &str) -> Result<Value, Value> {
    let target = app.view_target(view)?;
    let observation = match app.observe_target(&target) {
        Ok(s) => {
            json!({"descriptor":s.target,"targetSource":s.target_source,"availability":s.availability(),"capabilities":s.capabilities,"error":null})
        }
        Err(e) => {
            json!({"descriptor":null,"targetSource":"unknown","availability":targets::error_availability(&e),"capabilities":null,"error":response::brief_error(&e)})
        }
    };
    let mut tasks: Vec<_> = app
        .records()
        .into_iter()
        .filter(|t| t.target.id == target.id)
        .map(|t| (t.order, response::ui_row(&CapturedTask::new(t))))
        .collect();
    tasks.sort_by_key(|(order, v)| (v["done"] == true, std::cmp::Reverse(*order)));
    let count = tasks.len();
    let (mut rows, mut bytes) = (Vec::new(), 0);
    for (_, row) in tasks.into_iter().take(50) {
        let size = serde_json::to_vec(&row).unwrap().len();
        if bytes + size > 40 * 1024 {
            break;
        }
        bytes += size;
        rows.push(row);
    }
    current(app, view, &target)?;
    Ok(
        json!({"runId":app.run,"targetId":target.id,"target":observation,
        "endpoint":format!("http://127.0.0.1:{}/mcp",app.port),"service":*app.http_state.lock().unwrap(),
        "tasks":rows,"recordCount":count,"evidenceDirectory":app.root.join("evidence")}),
    )
}
pub fn task(app: &App, view: &str, p: &Value) -> Result<Value, Value> {
    let target = app.view_target(view)?;
    let task = app.resolve(p, Some(&target.id))?;
    let options = query::parse_ui_read(p)?;
    let read = query::ui_read(app, task, options).map_err(response::ui_read_error)?;
    current(app, view, &target)?;
    let value = response::ui_task(&read);
    if serde_json::to_vec(&value).unwrap().len() > 56 * 1024 {
        return Err(failure(
            "resourceLimit",
            "详情超过界面传输大小，请使用本地证据或 MCP 读取",
        ));
    }
    Ok(value)
}
