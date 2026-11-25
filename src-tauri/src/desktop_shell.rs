//! Native presentation and application intents; business lifetime remains in AppCore.
use crate::{
    desktop_updates::UpdateService,
    lifecycle::{Action, Operation, Target},
    model::CloseBehavior,
    windows::DesktopWindows,
    AppCore,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigation {
    pub id: u64,
    pub target: Destination,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Destination {
    Messages,
    Settings,
}
#[derive(Clone, Serialize)]
pub struct Background {
    pub id: u64,
    pub generation: u64,
    preparing: bool,
}
#[derive(Default)]
struct State {
    generation: u64,
    ready: bool,
    sequence: u64,
    navigation: Option<Navigation>,
    background: Option<Background>,
    modals: HashMap<u64, String>,
}
impl State {
    fn next(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    fn reset_document(&mut self) {
        self.generation += 1;
        self.ready = false;
        self.background = None;
    }
    fn acknowledge_navigation(&mut self, generation: u64, id: u64) {
        if self.generation == generation && self.navigation.as_ref().is_some_and(|n| n.id == id) {
            self.navigation = None;
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    generation: u64,
    visible: bool,
    navigation: Option<Navigation>,
    background: Option<Background>,
    blocked: bool,
    tray_available: bool,
}
pub struct DesktopShell {
    core: Arc<AppCore>,
    updates: Arc<UpdateService>,
    state: Mutex<State>,
    tray: Mutex<Option<TrayIcon>>,
    messages: Mutex<Option<(MenuItem<tauri::Wry>, usize)>>,
    exit_handoff: AtomicBool,
}
impl DesktopShell {
    pub fn new(core: Arc<AppCore>, updates: Arc<UpdateService>) -> Self {
        Self {
            core,
            updates,
            state: Mutex::new(State {
                generation: 1,
                ..State::default()
            }),
            tray: Mutex::new(None),
            messages: Mutex::new(None),
            exit_handoff: AtomicBool::new(false),
        }
    }
    pub fn handed_off(&self) -> bool {
        self.exit_handoff.load(Ordering::Acquire)
    }
    pub fn handoff(&self) {
        self.exit_handoff.store(true, Ordering::Release);
    }
    pub fn cancel_pending_background(&self, label: &str) {
        if label == "main" {
            self.state.lock().unwrap().background = None;
        }
    }
    pub fn page_started(&self, label: &str) {
        if label == "main" {
            self.state.lock().unwrap().reset_document();
        }
    }
    pub fn changed(handle: &tauri::AppHandle) {
        let _ = handle.emit("desktop-state-changed", ());
    }
    pub fn snapshot(&self, handle: &tauri::AppHandle, label: &str) -> Snapshot {
        let blocked = !self.core.close_operations("main").is_empty();
        let tray_available = self.tray.lock().unwrap().is_some();
        let visible = handle
            .get_webview_window(label)
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(true));
        let state = self.state.lock().unwrap();
        Snapshot {
            blocked,
            tray_available,
            generation: state.generation,
            visible,
            navigation: (label == "main")
                .then(|| state.navigation.clone())
                .flatten(),
            background: (label == "main")
                .then(|| state.background.clone().filter(|b| b.preparing))
                .flatten(),
        }
    }
    pub fn ready(&self, generation: u64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.generation != generation {
            return Err("桌面文档已变化".into());
        }
        state.ready = true;
        Ok(())
    }
    pub fn report(&self, handle: &tauri::AppHandle, error: impl ToString) {
        let error = error.to_string();
        self.core.log(
            None,
            "error",
            "桌面操作未完成",
            Some(json!({"error":error})),
        );
        let _ = handle.emit("core-desktop-error", error);
    }
    pub fn install(&self, handle: &tauri::AppHandle) -> Result<(), String> {
        let show = MenuItem::with_id(handle, "desktop.show", "显示主窗口", true, None::<&str>)
            .map_err(|e| e.to_string())?;
        let messages = MenuItem::with_id(
            handle,
            "desktop.messages",
            message_label(self.core.unread_count()),
            true,
            None::<&str>,
        )
        .map_err(|e| e.to_string())?;
        let settings = MenuItem::with_id(handle, "desktop.settings", "设置", true, None::<&str>)
            .map_err(|e| e.to_string())?;
        let quit = MenuItem::with_id(handle, "desktop.exit", "退出 SoDeck", true, None::<&str>)
            .map_err(|e| e.to_string())?;
        let separator = PredefinedMenuItem::separator(handle).map_err(|e| e.to_string())?;
        let menu = Menu::with_items(handle, &[&show, &messages, &settings, &separator, &quit])
            .map_err(|e| e.to_string())?;
        let mut builder = TrayIconBuilder::with_id("sodeck")
            .menu(&menu)
            .tooltip("SoDeck")
            .show_menu_on_left_click(!cfg!(windows));
        if let Some(icon) = handle.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        #[cfg(target_os = "macos")]
        {
            // Transparent terminal outline stays legible in either menu-bar appearance.
            let rows = [
                "00000000000000000000",
                "00000000000000000000",
                "00111111111111111100",
                "01100000000000000110",
                "01000000000000000010",
                "01001000000000000010",
                "01000100000000000010",
                "01000010000000000010",
                "01000100000000000010",
                "01001000011111000010",
                "01000000000000000010",
                "01000000000000000010",
                "01100000000000000110",
                "00111111111111111100",
                "00000000100100000000",
                "00000000100100000000",
                "00000111111111000000",
                "00000000000000000000",
                "00000000000000000000",
                "00000000000000000000",
            ];
            let rgba = rows
                .iter()
                .flat_map(|row| {
                    row.bytes()
                        .flat_map(|pixel| [0, 0, 0, if pixel == b'1' { 255 } else { 0 }])
                })
                .collect();
            builder = builder
                .icon(tauri::image::Image::new_owned(rgba, 20, 20))
                .icon_as_template(true);
        }
        let tray = builder
            .on_menu_event(|handle, event| Self::menu(handle, event.id.as_ref()))
            .on_tray_icon_event(|tray, event| {
                if cfg!(windows)
                    && matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    )
                {
                    Self::menu(tray.app_handle(), "desktop.show");
                }
            })
            .build(handle)
            .map_err(|e| e.to_string())?;
        *self.tray.lock().unwrap() = Some(tray);
        *self.messages.lock().unwrap() = Some((messages, usize::MAX));
        self.refresh_messages();
        Ok(())
    }
    pub fn menu(handle: &tauri::AppHandle, id: &str) {
        let shell = handle.state::<Self>();
        let result = match id {
            "desktop.show" => shell.present(handle, "main", true),
            "desktop.messages" => shell.navigate(handle, Destination::Messages),
            "desktop.settings" => shell.navigate(handle, Destination::Settings),
            "desktop.exit" => {
                Self::request_exit(handle);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            shell.report(handle, error);
        }
    }
    pub fn present(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
        focus: bool,
    ) -> Result<(), String> {
        if self.handed_off() {
            return Err("应用已交接退出".into());
        }
        if label == "main" {
            self.cancel_pending_background(label);
            handle
                .state::<DesktopWindows>()
                .restore_main_display(handle)?;
        }
        handle
            .state::<DesktopWindows>()
            .present_window(handle, label, focus)?;
        Self::changed(handle);
        Ok(())
    }
    fn navigate(&self, handle: &tauri::AppHandle, target: Destination) -> Result<(), String> {
        {
            let mut state = self.state.lock().unwrap();
            let id = state.next();
            state.navigation = Some(Navigation { id, target });
        }
        self.present(handle, "main", true)
    }
    fn refresh_messages(&self) {
        let count = self.core.unread_count();
        if let Some((item, last)) = self.messages.lock().unwrap().as_mut() {
            if *last != count && item.set_text(message_label(count)).is_ok() {
                *last = count;
            }
        }
    }
    pub fn on_core_event(handle: &tauri::AppHandle, name: &str, value: &Value) {
        if !matches!(
            name,
            "messages-changed" | "core-close-changed" | "connection-input-required"
        ) {
            return;
        }
        let handle2 = handle.clone();
        let name = name.to_owned();
        let value = value.clone();
        let _ = handle.run_on_main_thread(move || {
            let shell = handle2.state::<Self>();
            if shell.handed_off() {
                return;
            }
            if name == "messages-changed" {
                shell.refresh_messages();
            } else if name == "core-close-changed" {
                if let (Some(label), Some(id)) = (value["window"].as_str(), value["id"].as_str()) {
                    if shell.core.close_status(label, id).is_ok_and(|op| {
                        !matches!(
                            op.phase,
                            crate::lifecycle::Phase::Completed | crate::lifecycle::Phase::Aborted
                        )
                    }) {
                        if let Err(error) = shell.present(&handle2, label, false) {
                            shell.report(&handle2, error);
                        }
                    }
                }
                Self::changed(&handle2);
            } else if let (Some(window), Some(attempt), Some(question)) = (
                value["windowId"].as_str(),
                value["attemptId"].as_str(),
                value["questionId"].as_str(),
            ) {
                if shell
                    .core
                    .connection_question_current(window, attempt, question)
                {
                    if let Err(error) = shell.present(&handle2, window, true) {
                        shell.report(&handle2, error);
                    }
                }
            }
        });
    }
    pub async fn prepare_close(
        &self,
        handle: &tauri::AppHandle,
        origin: &str,
        request: &str,
        target: Target,
    ) -> Result<Operation, String> {
        let owner = if matches!(target, Target::Application { .. }) {
            if !self.core.close_host_registered(origin) {
                return Err("关闭来源窗口未就绪".into());
            }
            self.present(handle, "main", true)?;
            "main"
        } else {
            origin
        };
        if matches!(
            target,
            Target::Application {
                action: Action::InstallAndRestart
            }
        ) {
            let core = self.core.clone();
            let updates = self.updates.clone();
            tauri::async_runtime::spawn_blocking(move || updates.updater.verify_for_close(&core))
                .await
                .map_err(|e| e.to_string())??;
        }
        self.core.close_prepare(owner, request, target)
    }
    pub fn request_exit(handle: &tauri::AppHandle) {
        let shell = handle.state::<Self>();
        let _ = shell.present(handle, "main", true);
        match shell.core.close_native_prepare(
            "main",
            Target::Application {
                action: Action::Exit,
            },
        ) {
            Ok(op) => {
                shell.core.close_unhosted(&op.id);
            }
            Err(error) => shell.report(handle, error),
        }
    }
    pub fn close_requested(&self, handle: &tauri::AppHandle, label: &str) {
        if label != "main" {
            if let Err(error) = handle
                .state::<DesktopWindows>()
                .close_secondary(handle, label)
            {
                self.report(handle, error);
            }
            return;
        }
        if self.state.lock().unwrap().background.is_some() {
            return;
        }
        if self.core.desktop_preferences().0 == CloseBehavior::Exit
            || (!cfg!(target_os = "linux") && self.tray.lock().unwrap().is_none())
        {
            Self::request_exit(handle);
            return;
        }
        if !self.core.close_operations("main").is_empty() || self.modal_active("main") {
            if let Err(error) = self.present(handle, "main", true) {
                self.report(handle, error);
            }
            return;
        }
        let explained = self.core.desktop_preferences().1;
        let background = {
            let mut state = self.state.lock().unwrap();
            if state.background.is_some() {
                return;
            }
            if !state.ready {
                drop(state);
                self.report(handle, "窗口尚未就绪，请稍后收起");
                return;
            }
            let id = state.next();
            let b = Background {
                id,
                generation: state.generation,
                preparing: explained,
            };
            state.background = Some(b.clone());
            b
        };
        if explained {
            self.prepare_background(handle, background.id);
            return;
        }
        let Some(window) = handle.get_webview_window("main") else {
            return;
        };
        let guard = self.modal(handle, "main");
        // The first explanation is a user decision, so it has no machine timeout.
        let detail = if cfg!(target_os = "linux") {
            "关闭主窗口后将最小化并继续运行。可通过任务栏、窗口切换或托盘菜单返回。"
        } else {
            "关闭主窗口后将隐藏到托盘，已有连接和任务继续运行。可从托盘打开主窗口，选择“退出 SoDeck”结束应用。"
        };
        let native = handle.clone();
        handle
            .dialog()
            .message(detail)
            .title("SoDeck 继续在后台运行")
            .parent(&window)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "继续在后台运行".into(),
                "取消".into(),
            ))
            .show(move |accept| {
                drop(guard);
                let shell = native.state::<Self>();
                if accept {
                    shell.prepare_background(&native, background.id);
                } else {
                    shell.cancel_background(background.id);
                }
            });
    }
    fn prepare_background(&self, handle: &tauri::AppHandle, id: u64) {
        {
            let mut state = self.state.lock().unwrap();
            let Some(background) = state.background.as_mut().filter(|b| b.id == id) else {
                return;
            };
            background.preparing = true;
        }
        Self::changed(handle);
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(12)).await;
            if handle.state::<Self>().cancel_background(id) {
                handle
                    .state::<Self>()
                    .report(&handle, "收起窗口未得到界面确认，请重试");
                Self::changed(&handle);
            }
        });
    }
    fn cancel_background(&self, id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.background.as_ref().is_some_and(|b| b.id == id) {
            state.background = None;
            true
        } else {
            false
        }
    }
    pub fn finish_background(
        &self,
        handle: &tauri::AppHandle,
        generation: u64,
        id: u64,
        allow: bool,
    ) -> Result<(), String> {
        {
            let state = self.state.lock().unwrap();
            if state.generation != generation
                || !state
                    .background
                    .as_ref()
                    .is_some_and(|b| b.id == id && b.preparing)
            {
                return Err("收起请求已失效".into());
            }
        }
        self.cancel_background(id);
        if !allow {
            Self::changed(handle);
            return Ok(());
        }
        if self.core.desktop_preferences().0 != CloseBehavior::Background
            || !self.core.close_operations("main").is_empty()
            || self
                .core
                .connection_views()
                .iter()
                .any(|v| v.window_id == "main" && v.state == "connecting" && v.question.is_some())
            || self.modal_active("main")
            || self.handed_off()
        {
            Self::changed(handle);
            return Err("窗口当前需要处理交互，已取消收起".into());
        }
        let window = handle.get_webview_window("main").ok_or("主窗口不存在")?;
        let result = if cfg!(target_os = "linux") {
            window.minimize()
        } else if self.tray.lock().unwrap().is_some() {
            window.hide()
        } else {
            return Err("系统托盘不可用，窗口保持可见".into());
        };
        if let Err(error) = result {
            Self::changed(handle);
            return Err(error.to_string());
        }
        Self::changed(handle);
        if !self.core.desktop_preferences().1 {
            if let Err(error) = self.core.set_desktop_preferences(None, true) {
                self.report(handle, error);
            }
        }
        Ok(())
    }
    pub fn modal(&self, handle: &tauri::AppHandle, label: &str) -> ModalGuard {
        let mut state = self.state.lock().unwrap();
        let id = state.next();
        state.modals.insert(id, label.into());
        ModalGuard {
            handle: handle.clone(),
            id,
        }
    }
    /// Runs in the packaged process, using the same tray/menu and background paths.
    pub fn verify_native(handle: &tauri::AppHandle) -> Result<(), String> {
        let (reply, result) = std::sync::mpsc::channel();
        let native = handle.clone();
        handle
            .run_on_main_thread(move || {
                let _ = reply.send(Self::verify_on_main(&native));
            })
            .map_err(|e| e.to_string())?;
        result
            .recv_timeout(std::time::Duration::from_secs(8))
            .map_err(|e| e.to_string())?
    }
    fn verify_on_main(handle: &tauri::AppHandle) -> Result<(), String> {
        let shell = handle.state::<Self>();
        if shell.tray.lock().unwrap().is_none() {
            return Err("原生托盘未建立".into());
        }
        let first = shell.snapshot(handle, "main");
        shell.ready(first.generation)?;
        shell
            .core
            .set_desktop_preferences(Some(CloseBehavior::Background), true)?;
        shell.close_requested(handle, "main");
        let pending = shell
            .snapshot(handle, "main")
            .background
            .ok_or("未生成原生收起请求")?;
        shell.finish_background(handle, pending.generation, pending.id, true)?;
        let window = handle.get_webview_window("main").ok_or("主窗口丢失")?;
        if cfg!(target_os = "linux") {
            // A headless X server has no WM to acknowledge minimize; it still exercises the native call.
            if std::env::var_os("XDG_CURRENT_DESKTOP").is_some()
                && !window.is_minimized().map_err(|e| e.to_string())?
            {
                return Err("主窗口未最小化".into());
            }
        } else if window.is_visible().map_err(|e| e.to_string())? {
            return Err("主窗口未隐藏".into());
        }
        Self::menu(handle, "desktop.show");
        if !shell.snapshot(handle, "main").visible {
            return Err("主窗口恢复失败".into());
        }
        Self::menu(handle, "desktop.messages");
        if !matches!(
            shell.snapshot(handle, "main").navigation.map(|n| n.target),
            Some(Destination::Messages)
        ) {
            return Err("托盘消息分发失败".into());
        }
        let count = shell.core.unread_count();
        shell.refresh_messages();
        if shell.messages.lock().unwrap().as_ref().map(|(_, n)| *n) != Some(count) {
            return Err("托盘未读计数失效".into());
        }
        shell.state.lock().unwrap().navigation = None;
        Ok(())
    }
    fn modal_active(&self, label: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .modals
            .values()
            .any(|w| w == label)
    }
    pub fn attach_finalizer(handle: &tauri::AppHandle, core: &Arc<AppCore>) {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(
            Target,
            std::sync::mpsc::Sender<Result<(), String>>,
        )>(4);
        core.set_finalizer(Arc::new(move |target| {
            let (reply, result) = std::sync::mpsc::channel();
            tx.send((target, reply)).map_err(|_| "原生收尾通道已关闭")?;
            result.recv().map_err(|_| "原生收尾结果未确认")?
        }));
        let native = handle.clone();
        std::thread::spawn(move || {
            while let Ok((target, reply)) = rx.recv() {
                let global = matches!(target, Target::Application { .. });
                let result = (|| match target {
                    Target::Window { id } => {
                        native.state::<DesktopWindows>().finish_window(&native, &id)
                    }
                    Target::Application { action } => {
                        let shell = native.state::<Self>();
                        if action == Action::InstallAndRestart {
                            if let Err(error) = shell.updates.updater.install() {
                                shell.core.failed_core_install(error.clone());
                                return Err(error);
                            }
                        }
                        crate::desktop_files::shutdown(&native)?;
                        shell.updates.stop();
                        shell.handoff();
                        if action == Action::Exit {
                            native.exit(0);
                        } else {
                            native.request_restart();
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                })();
                let ended = global && result.is_ok();
                let _ = reply.send(result);
                if ended {
                    break;
                }
            }
        });
    }
}
pub struct ModalGuard {
    handle: tauri::AppHandle,
    id: u64,
}
impl Drop for ModalGuard {
    fn drop(&mut self) {
        if let Some(shell) = self.handle.try_state::<DesktopShell>() {
            shell.state.lock().unwrap().modals.remove(&self.id);
        }
    }
}
fn message_label(count: usize) -> String {
    if count == 0 {
        "消息".into()
    } else {
        format!("消息（{count} 条未读）")
    }
}

#[tauri::command]
pub fn desktop_state(handle: tauri::AppHandle, window: tauri::WebviewWindow) -> Snapshot {
    handle
        .state::<DesktopShell>()
        .snapshot(&handle, window.label())
}
#[tauri::command]
pub fn desktop_ready(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
) -> Result<Snapshot, String> {
    if window.label() == "main" {
        handle.state::<DesktopShell>().ready(generation)?;
    }
    Ok(handle
        .state::<DesktopShell>()
        .snapshot(&handle, window.label()))
}
#[tauri::command]
pub fn desktop_navigation_done(
    shell: tauri::State<DesktopShell>,
    window: tauri::WebviewWindow,
    generation: u64,
    id: u64,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("导航只属于主窗口".into());
    }
    let mut state = shell.state.lock().unwrap();
    state.acknowledge_navigation(generation, id);
    Ok(())
}
#[tauri::command]
pub fn desktop_background_done(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
    id: u64,
    allow: bool,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("只能收起主窗口".into());
    }
    handle
        .state::<DesktopShell>()
        .finish_background(&handle, generation, id, allow)
}
#[tauri::command]
pub fn set_close_behavior(handle: tauri::AppHandle, behavior: CloseBehavior) -> Result<(), String> {
    let shell = handle.state::<DesktopShell>();
    shell.state.lock().unwrap().background = None;
    shell.core.set_desktop_preferences(Some(behavior), false)?;
    DesktopShell::changed(&handle);
    Ok(())
}
#[tauri::command]
pub fn desktop_present(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    attempt_id: Option<String>,
    question_id: Option<String>,
) -> Result<(), String> {
    let shell = handle.state::<DesktopShell>();
    if let (Some(attempt), Some(question)) = (&attempt_id, &question_id) {
        if !shell
            .core
            .connection_question_current(window.label(), attempt, question)
        {
            return Err("连接问题已失效".into());
        }
    } else if attempt_id.is_some() || question_id.is_some() {
        return Err("连接问题身份不完整".into());
    }
    shell.present(&handle, window.label(), true)
}
#[tauri::command]
pub async fn choose_local_path(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    kind: String,
) -> Result<Option<String>, String> {
    if !matches!(kind.as_str(), "privateKey" | "pluginDirectory") {
        return Err("未知文件选择用途".into());
    }
    let shell = handle.state::<DesktopShell>();
    shell.present(&handle, window.label(), true)?;
    let guard = shell.modal(&handle, window.label());
    let dialog = handle.dialog().file().set_parent(&window);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let result = if kind == "pluginDirectory" {
            dialog
                .set_title("选择已解压的插件目录")
                .blocking_pick_folder()
        } else {
            dialog.set_title("选择私钥文件").blocking_pick_file()
        };
        result
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_reset_invalidates_background_and_readiness_but_keeps_navigation() {
        let mut state = State::default();
        state.ready = true;
        state.background = Some(Background {
            id: 1,
            generation: 0,
            preparing: true,
        });
        state.navigation = Some(Navigation {
            id: 2,
            target: Destination::Messages,
        });
        state.reset_document();
        assert!(!state.ready);
        assert!(state.background.is_none());
        assert_eq!(state.generation, 1);
        assert_eq!(state.navigation.unwrap().id, 2);
    }
    #[test]
    fn message_labels_include_only_unread_counts() {
        assert_eq!(message_label(0), "消息");
        assert_eq!(message_label(3), "消息（3 条未读）");
    }
    #[test]
    fn old_navigation_acknowledgements_cannot_remove_a_new_choice() {
        let mut state = State::default();
        state.generation = 4;
        state.navigation = Some(Navigation {
            id: 8,
            target: Destination::Settings,
        });
        state.acknowledge_navigation(3, 8);
        assert!(state.navigation.is_some());
        state.acknowledge_navigation(4, 7);
        assert!(state.navigation.is_some());
        state.acknowledge_navigation(4, 8);
        assert!(state.navigation.is_none());
    }
}
