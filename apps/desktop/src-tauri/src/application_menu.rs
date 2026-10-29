//! Host-owned transparent input shield and menu. Never hides plugin surfaces.
use super::*;
#[derive(Clone)]
pub(super) struct ApplicationMenu { pub owner:String,pub source:String,pub label:String,pub menu_id:String,pub token:String,pub actions:Vec<String>,pub shown:Arc<AtomicBool>,pub opened:Instant }

impl State {
    fn application_menu_owner_valid(&self,owner:&str)->bool {
        if self.settings_registered(owner){return true;}
        if self.monitor_registered(owner){return true;}
        let workbench=self.workbench.lock().unwrap();
        workbench.windows.contains_key(owner)||owner.strip_prefix("detached-").is_some_and(|id|workbench.views.iter().any(|v|v.view_id==id&&v.placement=="detached"))
    }

    pub(super) fn expire_application_menus(&self){
        let entries=self.application_menus.lock().unwrap().values().filter(|e|!e.shown.load(Ordering::SeqCst)&&e.opened.elapsed()>Duration::from_secs(8)).cloned().collect::<Vec<_>>();
        for entry in entries{self.finish_application_menu(&entry,None,"timeout");}
    }
    pub(super) fn close_application_menu(&self,owner:&str){
        let entry={let mut menus=self.application_menus.lock().unwrap();menus.remove(owner)};
        if let Some(entry)=entry {if let Some(popup)=self.app.get_webview_window(&entry.label){let _=popup.destroy();}}
    }
    pub(super) fn application_menu_has_focus(&self,owner:&str)->bool{
        let label=self.application_menus.lock().unwrap().get(owner).map(|entry|entry.label.clone());
        label.and_then(|label|self.app.get_webview_window(&label)).is_some_and(|w|w.is_focused().unwrap_or(false))
    }
    pub(super) fn finish_application_menu(&self,entry:&ApplicationMenu,selected:Option<String>,reason:&str){
        // Claim completion under one lock. A late blur cannot remove a newer session.
        let claimed={let mut menus=self.application_menus.lock().unwrap();
            if menus.get(&entry.owner).is_some_and(|v|v.token==entry.token){menus.remove(&entry.owner);true}else{false}};
        if !claimed{return;}
        let focused=self.app.get_webview_window(&entry.label).is_some_and(|w|w.is_focused().unwrap_or(false));
        if let Some(popup)=self.app.get_webview_window(&entry.label){let _=popup.destroy();}
        if focused {if let Some(owner)=self.app.get_webview_window(&entry.owner){let _=owner.set_focus();}}
        if let Some(result)=self.menus.finish(&entry.menu_id,selected){
            if let Some(owner)=self.app.get_webview_window(&entry.owner){let _=owner.eval(&format!("window.__sodeckApplicationMenuReason={};",json!(reason)));}
            self.menu_notification(&entry.source,&result);
        }
    }
}

#[tauri::command]
pub(super) async fn application_menu_open(window:tauri::WebviewWindow,state:tauri::State<'_,Arc<State>>,menu_id:String,anchor:Value,keyboard:bool,font_size:f64)->std::result::Result<(),String>{
    let state=state.inner().clone();let owner=window.label().to_string();
    if !state.application_menu_owner_valid(&owner){return Err("菜单来源窗口失效".into());}
    let source=if owner==monitor_window::LABEL {state.monitor_source().ok_or("监控窗口已关闭")?}else{format!("@window:{owner}")};
    if Uuid::parse_str(&menu_id).is_err(){return Err("菜单标识无效".into());}
    let x=anchor["x"].as_f64().filter(|v|v.is_finite()&&v.abs()<100000.).ok_or("菜单位置无效")?;
    if !font_size.is_finite()||!(8.0..=64.0).contains(&font_size){return Err("文字尺寸无效".into());}
    let y=anchor["y"].as_f64().filter(|v|v.is_finite()&&v.abs()<100000.).ok_or("菜单位置无效")?;
    tauri::async_runtime::spawn_blocking(move||{
        let _opening=state.menu_opening.lock().unwrap();
        for id in state.app.webview_windows().keys(){state.cancel_menus(id);}
        state.endpoint_window(&source,true).map_err(|e|e.message)?;
        state.menus.begin(&menu_id,&source,&owner)?;
        let theme=state.theme_snapshot(true);let theme_label=if theme["highContrast"]==true{"高对比".to_string()}else if theme["testOverride"]==true{"测试主题".into()}else{crate::theme::catalog()["themes"].as_array().unwrap().iter().find(|v|v["id"]==theme["id"]).and_then(|v|v["label"].as_str()).unwrap_or("外观").to_string()};
        let mut items=vec![json!({"id":"appearance","label":"外观","badge":theme_label}),json!({"id":"plugins","label":"管理插件"}),json!({"id":"communication","label":"通信监控","separator":true}),json!({"id":"resources","label":"运行资源"}),json!({"id":"calls","label":"调用记录"}),json!({"id":"recall","label":"找回所有窗口","separator":true})];
        if owner.starts_with("workspace-"){items.push(json!({"id":"merge","label":"全部合并回主窗口"}));}
        items.extend([json!({"id":"layoutRetry","label":"重试保存","detail":"偏好与清理记录"}),json!({"id":"exit","label":"退出 SoDeck","danger":true,"separator":true})]);
        let entry=ApplicationMenu{owner:owner.clone(),source,label:format!("application-menu-{}",Uuid::new_v4()),menu_id,token:Uuid::new_v4().to_string(),opened:Instant::now(),shown:Arc::new(AtomicBool::new(false)),actions:items.iter().map(|v|v["id"].as_str().unwrap().into()).collect()};
        state.application_menus.lock().unwrap().insert(owner.clone(),entry.clone());
        let result=(||->std::result::Result<(),String>{
            #[cfg(debug_assertions)]
            if crate::startup_config::debug().fail_application_menu{return Err("测试注入：应用菜单创建失败".into());}
            let size=window.inner_size().map_err(|e|e.to_string())?;let position=window.inner_position().map_err(|e|e.to_string())?;
            let dpi=window.scale_factor().map_err(|e|e.to_string())?;
            let monitor=window.current_monitor().map_err(|e|e.to_string())?;
            let limits=monitor.map(|m|{let area=m.work_area();json!({"left":((area.position.x-position.x) as f64/dpi).max(0.),"top":((area.position.y-position.y) as f64/dpi).max(0.),"right":(((area.position.x as i64+area.size.width as i64)-position.x as i64) as f64/dpi).min(size.width as f64/dpi),"bottom":(((area.position.y as i64+area.size.height as i64)-position.y as i64) as f64/dpi).min(size.height as f64/dpi)})});
            let data=json!({"fontSize":font_size,"limits":limits,"token":entry.token,"items":items,"anchor":{"x":x,"y":y},"keyboard":keyboard});
            let mut builder=WebviewWindowBuilder::new(&state.app,&entry.label,WebviewUrl::App("index.html?application-menu".into()))
                .title("SoDeck 应用菜单").decorations(false).transparent(true).shadow(false).skip_taskbar(true).resizable(false).visible(false)
                .initialization_script(crate::theme::bootstrap(&theme))
                .initialization_script(format!("window.__SODECK_APPLICATION_MENU__={data};"))
                .data_directory(state.data.join("profiles/shell"));
            #[cfg(windows)]{builder=builder.owner(&window).map_err(|e|e.to_string())?;}
            if let Some(port)=state.app.state::<crate::startup_config::DebugConfig>().shell_port{builder=builder.additional_browser_args(&format!("--remote-debugging-port={port}"));}
            let popup=builder.build().map_err(|e|e.to_string())?;
            popup.set_size(size).map_err(|e|e.to_string())?;popup.set_position(position).map_err(|e|e.to_string())?;
            let weak=Arc::downgrade(&state);let opened=entry.clone();
            popup.on_window_event(move|event|{if opened.shown.load(Ordering::SeqCst)&&matches!(event,tauri::WindowEvent::Focused(false)|tauri::WindowEvent::CloseRequested{..}){if let Some(state)=weak.upgrade(){let opened=opened.clone();std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(80));if !state.app.get_webview_window(&opened.label).is_some_and(|w|w.is_focused().unwrap_or(false)){state.finish_application_menu(&opened,None,"blur");}});}}});
            // A canceled request must never show a late-created invisible shield.
            if !state.application_menus.lock().unwrap().get(&owner).is_some_and(|v|v.token==entry.token){let _=popup.destroy();return Err("菜单已取消".into());}
            Ok(())
        })();
        if result.is_err(){state.finish_application_menu(&entry,None,"error");}result
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub(super) async fn application_menu_reply(window:tauri::WebviewWindow,state:tauri::State<'_,Arc<State>>,token:String,action:Option<String>,reason:String)->std::result::Result<(),String>{
    let state=state.inner().clone();
    let entry=state.application_menus.lock().unwrap().values().find(|v|v.label==window.label()&&v.token==token).cloned().ok_or("菜单会话失效")?;
    if !state.application_menu_owner_valid(&entry.owner)||state.endpoint_window(&entry.source,true).is_err(){state.finish_application_menu(&entry,None,"owner-closed");return Err("来源窗口已关闭".into());}
    if reason=="ready"{
        let ready=(||{window.show().map_err(|e|e.to_string())?;window.set_focus().map_err(|e|e.to_string())?;
            configure_popup_window(&window)?;
            entry.shown.store(true,Ordering::SeqCst);
            if !state.application_menus.lock().unwrap().get(&entry.owner).is_some_and(|v|v.token==entry.token){let _=window.destroy();return Err("菜单已取消".into());}Ok(())})();
        if ready.is_err(){state.finish_application_menu(&entry,None,"error");}return ready;
    }
    if !matches!(reason.as_str(),"select"|"escape"|"tab"|"shift-tab"|"outside"){return Err("菜单响应无效".into());}
    if action.as_ref().is_some_and(|id|!entry.actions.contains(id))||reason!="select"&&action.is_some(){return Err("菜单动作无效".into());}
    state.finish_application_menu(&entry,action,&reason);Ok(())
}

// Tao reapplies its window styles when showing. Set tool-window flags after those
// queued mutations, on the same UI thread, so the popup stays out of Alt+Tab.
fn configure_popup_window(window:&tauri::WebviewWindow)->std::result::Result<(),String>{
    #[cfg(windows)] {
        let hwnd=window.hwnd().map_err(|e|e.to_string())?.0 as usize;
        let (send,receive)=std::sync::mpsc::sync_channel(1);
        window.run_on_main_thread(move||unsafe{
            use windows::Win32::{Foundation::HWND,UI::WindowsAndMessaging::*};
            let hwnd=HWND(hwnd as _);let style=GetWindowLongPtrW(hwnd,GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd,GWL_EXSTYLE,(style&!(WS_EX_APPWINDOW.0 as isize))|WS_EX_TOOLWINDOW.0 as isize);
            let result=SetWindowPos(hwnd,None,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE|SWP_FRAMECHANGED).map_err(|e|e.to_string());
            let _=send.send(result);
        }).map_err(|e|e.to_string())?;
        receive.recv().map_err(|e|e.to_string())??;
    }
    Ok(())
}
