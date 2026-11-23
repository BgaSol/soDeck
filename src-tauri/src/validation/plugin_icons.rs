//! Native package check with inert, disabled plugin packages; no business plugin dependency.
use crate::{AppCore, TARGET};
use serde_json::json;
use std::{fs, sync::Arc, time::Duration};
use tauri::{Listener, Manager};

pub fn verify(handle: &tauri::AppHandle, core: &Arc<AppCore>) -> Result<(), String> {
    let root = core.snapshot().data_directory.join("icon-fixtures");
    for (id,svg) in [("fixture.icon.good","<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\" r=\"8\"/></svg>"),("fixture.icon.bad","broken svg")] {
        let directory=root.join(id);fs::create_dir_all(directory.join("assets")).map_err(|e|e.to_string())?;
        fs::write(directory.join("entry"),"disabled; never executed").map_err(|e|e.to_string())?;
        fs::write(directory.join("assets/shape.svg"),svg).map_err(|e|e.to_string())?;
        fs::write(directory.join("plugin.json"),json!({"id":id,"name":id,"version":"1.0.0","apiVersion":sodeck_plugin_protocol::API_VERSION,"target":TARGET,"entry":"entry","icon":"shape","icons":{"shape":"assets/shape.svg"}}).to_string()).map_err(|e|e.to_string())?;
        if !core.snapshot().plugins.iter().any(|p|p.manifest.id==id) { core.add_plugin(&directory)?; }
        core.set_enabled(id,false)?;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = handle.listen("plugin-icons-native-result", move |event| {
        let _ = tx.send(event.payload().to_string());
    });
    let script = r#"(async()=>{
      const delay=()=>new Promise(r=>setTimeout(r,40)),deadline=Date.now()+10000;
      const wait=async read=>{while(Date.now()<deadline){const value=read();if(value)return value;await delay();}throw Error('native icon UI timeout');};
      try {
        await wait(()=>document.querySelector('__STARTUP_PAGE__[aria-current="page"]'));
        if(__STARTUP_EMPTY__) await wait(()=>document.querySelector('#plugins-official[aria-pressed="true"]'));
        (await wait(()=>document.querySelector('[data-console-page="plugins"]'))).click();
        (await wait(()=>document.querySelector('#plugins-installed'))).click();
        const good=await wait(()=>document.querySelector('[data-plugin-id="fixture.icon.good"] .plugin-icon[data-ready="true"]'));
        const bad=await wait(()=>document.querySelector('[data-plugin-id="fixture.icon.bad"] .plugin-icon'));
        await delay();
        if(bad.dataset.ready==='true'||getComputedStyle(bad.querySelector('.plugin-icon-fallback')).display==='none')throw Error('broken icon did not preserve fallback');
        if(getComputedStyle(good.querySelector('.plugin-icon-image')).opacity!=='1'||good.getBoundingClientRect().width<=0)throw Error('native icon image invisible');
        const original=document.documentElement.dataset.theme,colors=[];
        for(const theme of ['light','dark','dopamine']){document.documentElement.dataset.theme=theme;await delay();const fill=getComputedStyle(good.querySelector('.plugin-icon-image')).fill;colors.push(fill);if(fill!==getComputedStyle(good).color)throw Error('native icon lost theme color');}
        document.documentElement.dataset.theme=original;
        await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'plugin-icons-native-result',payload:{ok:true,colors}});
      }catch(error){await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'plugin-icons-native-result',payload:{ok:false,error:String(error)}});}
    })()"#;
    let script = script
        .replace(
            "__STARTUP_PAGE__",
            if core.snapshot().started_without_plugins {
                "[data-console-page=plugins]"
            } else {
                "[data-console-page=connections]"
            },
        )
        .replace(
            "__STARTUP_EMPTY__",
            if core.snapshot().started_without_plugins {
                "true"
            } else {
                "false"
            },
        );
    let evaluated = handle
        .get_webview_window("main")
        .ok_or("原生图标验收主窗口不存在")?
        .eval(&script)
        .map_err(|e| e.to_string());
    let result = evaluated.and_then(|_| {
        rx.recv_timeout(Duration::from_secs(12))
            .map_err(|_| "原生图标验收回执超时".to_string())
    });
    handle.unlisten(listener);
    let value: serde_json::Value = serde_json::from_str(&result?).map_err(|e| e.to_string())?;
    if value["ok"] != true {
        return Err(format!("原生图标验收失败：{}", value["error"]));
    }
    Ok(())
}
