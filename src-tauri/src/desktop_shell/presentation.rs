//! Application identity and native presentation share the UI thread and one intent owner.
use super::*;
use std::sync::atomic::AtomicU8;
use std::time::Duration;

pub(super) fn on_main<T: Send + 'static>(
    handle: &tauri::AppHandle,
    action: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = std::sync::mpsc::channel();
    let execution = Arc::new(AtomicU8::new(0));
    let task = execution.clone();
    let native = handle.clone();
    // Tauri runs this inline when already on the UI thread.
    handle
        .run_on_main_thread(move || {
            if task
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let result = action(&native);
                DesktopShell::changed(&native);
                let _ = send.send(result);
            }
        })
        .map_err(|e| e.to_string())?;
    receive.recv_timeout(Duration::from_secs(10)).map_err(|_| {
        if execution
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            "原生呈现未开始，已取消，请重试".to_owned()
        } else {
            "原生呈现结果尚未确认，请检查窗口状态".to_owned()
        }
    })?
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Policy {
    Regular,
    #[cfg(target_os = "macos")]
    Accessory,
}

fn set_policy(handle: &tauri::AppHandle, policy: Policy) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if current_policy()? != policy {
            handle
                .set_activation_policy(match policy {
                    Policy::Regular => tauri::ActivationPolicy::Regular,
                    Policy::Accessory => tauri::ActivationPolicy::Accessory,
                })
                .map_err(|e| e.to_string())?;
        }
        if current_policy()? != policy {
            return Err("macOS 未接受应用显示策略".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (handle, policy);
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_policy() -> Result<Policy, String> {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    let marker = objc2::MainThreadMarker::new().ok_or("应用策略只能在主线程读取")?;
    match NSApplication::sharedApplication(marker).activationPolicy() {
        NSApplicationActivationPolicy::Regular => Ok(Policy::Regular),
        NSApplicationActivationPolicy::Accessory => Ok(Policy::Accessory),
        _ => Err("macOS 应用处于不可呈现状态".into()),
    }
}

fn show(handle: &tauri::AppHandle, label: &str, focus: bool) -> Result<(), String> {
    set_policy(handle, Policy::Regular)?;
    #[cfg(target_os = "macos")]
    handle.show().map_err(|e| e.to_string())?;
    let window = handle.get_webview_window(label).ok_or("目标窗口不存在")?;
    window.show().map_err(|e| e.to_string())?;
    if window.is_minimized().map_err(|e| e.to_string())? {
        window.unminimize().map_err(|e| e.to_string())?;
    }
    if focus {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Local display receipt, not a second workspace migration operation.
pub(crate) struct StagedWindow {
    id: u64,
    label: String,
    visible: bool,
    minimized: bool,
    main_backgrounded: bool,
}

impl DesktopShell {
    pub(super) fn background_on_main(&self, handle: &tauri::AppHandle) -> Result<(), String> {
        let window = handle.get_webview_window("main").ok_or("主窗口不存在")?;
        let result: Result<(), String> = (|| {
            if cfg!(target_os = "linux") {
                if !window.is_visible().map_err(|e| e.to_string())? {
                    window.show().map_err(|e| e.to_string())?;
                }
                window.minimize().map_err(|e| e.to_string())?;
            } else if self.tray_available() {
                window.hide().map_err(|e| e.to_string())?;
            } else {
                return Err("系统托盘不可用，窗口保持可见".into());
            }
            self.apply_background_policy(handle)?;
            self.state.lock().unwrap().main_backgrounded = true;
            Ok(())
        })();
        if let Err(error) = result {
            let recovery = self.present_on_main(handle, "main", false);
            return Err(format!("收起未完成：{error}；窗口恢复：{recovery:?}"));
        }
        Self::changed(handle);
        Ok(())
    }

    pub fn present(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
        focus: bool,
    ) -> Result<(), String> {
        let label = label.to_owned();
        on_main(handle, move |handle| {
            let shell = handle.state::<Self>();
            shell.present_on_main(handle, &label, focus)
        })
    }

    fn present_on_main(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
        focus: bool,
    ) -> Result<(), String> {
        if self.handed_off() {
            return Err("应用已交接退出".into());
        }
        self.state.lock().unwrap().begin_presentation();
        if label == "main" {
            handle
                .state::<DesktopWindows>()
                .ensure_main_display(handle)?;
        }
        let result = show(handle, label, focus);
        if label == "main"
            && handle
                .get_webview_window(label)
                .is_some_and(|w| w.is_visible().unwrap_or(false))
        {
            self.state.lock().unwrap().main_backgrounded = false;
        }
        result
    }

    pub fn present_modal(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
    ) -> Result<ModalGuard, String> {
        let label = label.to_owned();
        on_main(handle, move |handle| {
            let shell = handle.state::<Self>();
            shell.present_on_main(handle, &label, true)?;
            Ok(shell.modal(handle, &label))
        })
    }

    pub(crate) fn show_for_commit(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
    ) -> Result<StagedWindow, String> {
        let label = label.to_owned();
        on_main(handle, move |handle| {
            let shell = handle.state::<Self>();
            if shell.handed_off() || shell.core.is_closing() {
                return Err("应用正在退出".into());
            }
            let window = handle.get_webview_window(&label).ok_or("目标窗口不存在")?;
            let visible = window.is_visible().map_err(|e| e.to_string())?;
            let minimized = window.is_minimized().map_err(|e| e.to_string())?;
            let (id, main_backgrounded) = {
                let mut state = shell.state.lock().unwrap();
                (state.begin_presentation(), state.main_backgrounded)
            };
            let receipt = StagedWindow {
                id,
                label,
                visible,
                minimized,
                main_backgrounded,
            };
            if let Err(error) = show(handle, &receipt.label, false) {
                shell.restore_staged_on_main(handle, &receipt)?;
                return Err(error);
            }
            // The receipt retains the previous intent until ownership commits or rolls back.
            if receipt.label == "main" {
                shell.state.lock().unwrap().main_backgrounded = false;
            }
            Ok(receipt)
        })
    }

    pub(crate) fn finish_staged(
        &self,
        handle: &tauri::AppHandle,
        receipt: StagedWindow,
        committed: bool,
    ) {
        let result = on_main(handle, move |handle| {
            let shell = handle.state::<Self>();
            if shell.handed_off()
                || shell.core.is_closing()
                || shell.state.lock().unwrap().presentation != receipt.id
            {
                return Ok(());
            }
            if committed {
                // Ownership is already committed: a focus failure must not roll it back.
                if receipt.label == "main" {
                    shell.state.lock().unwrap().main_backgrounded = false;
                }
                show(handle, &receipt.label, true)
            } else {
                shell.restore_staged_on_main(handle, &receipt)
            }
        });
        if let Err(error) = result {
            self.report(handle, error);
        }
    }

    fn restore_staged_on_main(
        &self,
        handle: &tauri::AppHandle,
        receipt: &StagedWindow,
    ) -> Result<(), String> {
        if self.state.lock().unwrap().presentation != receipt.id {
            return Ok(());
        }
        if let Some(window) = handle.get_webview_window(&receipt.label) {
            if !receipt.visible {
                window.hide().map_err(|e| e.to_string())?;
            } else if receipt.minimized {
                window.minimize().map_err(|e| e.to_string())?;
            }
            if receipt.label == "main" {
                self.state.lock().unwrap().main_backgrounded = receipt.main_backgrounded;
            }
        }
        self.reconcile_on_main(handle, None)
    }

    pub fn recover_main(&self, handle: &tauri::AppHandle) -> Result<(), String> {
        on_main(handle, |handle| {
            let shell = handle.state::<Self>();
            if shell.handed_off() || shell.core.is_closing() {
                return Ok(());
            }
            handle
                .state::<DesktopWindows>()
                .ensure_main_display(handle)?;
            let backgrounded = shell.state.lock().unwrap().main_backgrounded;
            if backgrounded {
                #[cfg(target_os = "linux")]
                {
                    show(handle, "main", false)?;
                    handle
                        .get_webview_window("main")
                        .ok_or("主窗口不存在")?
                        .minimize()
                        .map_err(|e| e.to_string())?;
                }
                shell.reconcile_on_main(handle, None)
            } else {
                shell.present_on_main(handle, "main", false)
            }
        })
    }

    pub(super) fn apply_background_policy(&self, handle: &tauri::AppHandle) -> Result<(), String> {
        self.reconcile_on_main(handle, Some(true))
    }

    pub fn reconcile(&self, handle: &tauri::AppHandle) {
        #[cfg(target_os = "macos")]
        if let Err(error) = on_main(handle, |handle| {
            handle.state::<Self>().reconcile_on_main(handle, None)
        }) {
            self.report(handle, error);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = handle;
    }

    fn reconcile_on_main(
        &self,
        handle: &tauri::AppHandle,
        backgrounded: Option<bool>,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            if self.handed_off() {
                return Ok(());
            }
            let (backgrounded, modal) = {
                let state = self.state.lock().unwrap();
                (
                    backgrounded.unwrap_or(state.main_backgrounded),
                    !state.modals.is_empty(),
                )
            };
            let (working, transferring) = {
                let state = self.core.workbench_state();
                (
                    state.workspaces.desktop_entry_windows(),
                    state.ui.restoring_layout
                        || !state.ui.transfers.is_empty()
                        || state.workspaces.has_pending_moves(),
                )
            };
            let windows = handle.webview_windows();
            let mut keep_window = false;
            for (label, window) in &windows {
                if working.contains(label)
                    || window.is_visible().map_err(|e| e.to_string())?
                    || (label != "main" && window.is_minimized().map_err(|e| e.to_string())?)
                {
                    keep_window = true;
                }
            }
            let accessory = backgrounded
                && !modal
                && !transferring
                && !keep_window
                && !self.core.is_closing()
                && self.tray.lock().unwrap().is_some();
            set_policy(
                handle,
                if accessory {
                    Policy::Accessory
                } else {
                    Policy::Regular
                },
            )?;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (handle, backgrounded);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(super) fn verify_native(handle: &tauri::AppHandle) -> Result<(), String> {
    fn expect(policy: Policy) -> Result<(), String> {
        if current_policy()? != policy {
            return Err(format!("实际 macOS 应用策略不符：{policy:?}"));
        }
        Ok(())
    }
    fn background(handle: &tauri::AppHandle) -> Result<(), String> {
        let shell = handle.state::<DesktopShell>();
        shell.close_requested(handle, "main");
        let pending = shell
            .snapshot(handle, "main")
            .background
            .ok_or("没有生成收起请求")?;
        shell.finish_background(handle, pending.generation, pending.id, true)
    }
    on_main(handle, |handle| {
        expect(Policy::Regular)?;
        let shell = handle.state::<DesktopShell>();
        background(handle)?;
        expect(Policy::Accessory)?;
        let staged = shell.show_for_commit(handle, "main")?;
        expect(Policy::Regular)?;
        shell.finish_staged(handle, staged, false);
        expect(Policy::Accessory)?;
        if shell.snapshot(handle, "main").visible {
            return Err("迁移回滚未恢复收起状态".into());
        }
        let obsolete = shell.show_for_commit(handle, "main")?;
        shell.present(handle, "main", true)?;
        shell.finish_staged(handle, obsolete, false);
        if !shell.snapshot(handle, "main").visible {
            return Err("旧迁移回执隐藏了新恢复窗口".into());
        }
        let guard = shell.present_modal(handle, "main")?;
        shell.close_requested(handle, "main");
        if !shell.snapshot(handle, "main").visible {
            return Err("模态交互被错误收起".into());
        }
        drop(guard);
        for _ in 0..12 {
            background(handle)?;
            expect(Policy::Accessory)?;
            shell.present(handle, "main", true)?;
            expect(Policy::Regular)?;
        }
        let secondary = tauri::WebviewWindowBuilder::new(
            handle,
            "desktop-policy-secondary",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
        shell.core.workspace_state().register(secondary.label())?;
        shell.present(handle, secondary.label(), false)?;
        background(handle)?;
        expect(Policy::Regular)?;
        secondary.minimize().map_err(|e| e.to_string())?;
        shell.reconcile(handle);
        expect(Policy::Regular)?;
        secondary.destroy().map_err(|e| e.to_string())?;
        Ok(())
    })?;
    // Native destruction is acknowledged by the event loop, never awaited on it.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while handle
        .get_webview_window("desktop-policy-secondary")
        .is_some()
    {
        if std::time::Instant::now() >= deadline {
            return Err("验收窗口未确认销毁".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    on_main(handle, |handle| {
        let shell = handle.state::<DesktopShell>();
        shell.reconcile_on_main(handle, None)?;
        expect(Policy::Accessory)?;
        shell.present(handle, "main", true)?;
        expect(Policy::Regular)?;
        Ok(())
    })
}
