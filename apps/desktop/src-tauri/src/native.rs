//! Native plugin surfaces. The owning UI thread keeps Wry/COM objects; workers
//! post bounded tasks and never call WebView construction from an IPC callback.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

pub type NativeResult<T> = Result<T, String>;
pub type Ipc = Arc<dyn Fn(String, String) + Send + Sync>;
pub type Failure = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct Assets(Arc<HashMap<String, Vec<u8>>>);
impl Assets {
    pub fn load(root: &Path) -> NativeResult<Self> {
        fn collect(
            root: &Path,
            at: &Path,
            files: &mut HashMap<String, Vec<u8>>,
            total: &mut usize,
        ) -> NativeResult<()> {
            for entry in std::fs::read_dir(at).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_symlink() {
                    return Err("插件资源包含符号链接".into());
                }
                if kind.is_dir() {
                    collect(root, &entry.path(), files, total)?;
                } else if kind.is_file() {
                    let data = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
                    *total += data.len();
                    if *total > 128 * 1024 * 1024 || files.len() >= 2000 {
                        return Err("插件资源超出容量限制".into());
                    }
                    files.insert(
                        entry
                            .path()
                            .strip_prefix(root)
                            .map_err(|e| e.to_string())?
                            .to_string_lossy()
                            .replace('\\', "/"),
                        data,
                    );
                }
            }
            Ok(())
        }
        let mut files = HashMap::new();
        collect(root, root, &mut files, &mut 0)?;
        Ok(Self(Arc::new(files)))
    }
    fn get(&self, path: &str) -> Option<&[u8]> {
        self.0.get(path).map(Vec::as_slice)
    }
}

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 100_000.)
            && self.width >= 0.
            && self.height >= 0.
    }
}

struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Task {
    run: Box<dyn FnOnce() + Send>,
    _permit: Permit,
}
impl Task {
    fn execute(self) {
        (self.run)();
        drop(self._permit);
    }
}

#[derive(Clone)]
pub struct NativeViews {
    send: mpsc::SyncSender<Task>,
    pending: Arc<AtomicUsize>,
    app: tauri::AppHandle,
}
impl NativeViews {
    pub fn new(app: tauri::AppHandle) -> NativeResult<Self> {
        let (send, recv) = mpsc::sync_channel::<Task>(128);
        let dispatcher = app.clone();
        std::thread::Builder::new()
            .name("sodeck-native-dispatch".into())
            .spawn(move || {
                while let Ok(task) = recv.recv() {
                    let _ = dispatcher.run_on_main_thread(move || task.execute());
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            send,
            pending: Arc::new(AtomicUsize::new(0)),
            app,
        })
    }
    pub fn call<T: Send + 'static>(
        &self,
        run: impl FnOnce() -> NativeResult<T> + Send + 'static,
    ) -> NativeResult<T> {
        let previous = self.pending.fetch_add(1, Ordering::SeqCst);
        let permit = Permit(self.pending.clone());
        if previous >= 128 {
            return Err("原生界面队列已满".into());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let deadline = Instant::now() + Duration::from_secs(10);
        self.send
            .try_send(Task {
                run: Box::new(move || {
                    if Instant::now() >= deadline {
                        let _ = tx.send(Err("原生操作已超时".into()));
                        return;
                    }
                    let _ = tx.send(run());
                }),
                _permit: permit,
            })
            .map_err(|_| "原生界面调度不可用".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "原生界面未及时回应".to_string())?
    }
    pub fn create(
        &self,
        id: String,
        window: String,
        entry: String,
        context: Value,
        assets: Assets,
        ipc: Ipc,
        failed: Failure,
        profile: std::path::PathBuf,
    ) -> NativeResult<()> {
        let app = self.app.clone();
        let cleanup_id = id.clone();
        let result = self.call(move || {
            platform::create(
                &app, id, window, entry, context, assets, ipc, failed, profile,
            )
        });
        if result.is_err() {
            // This closure has no expiry: it must run even if construction was
            // already executing when the caller's deadline elapsed.
            let _ = self.app.run_on_main_thread(move || {
                let _ = platform::close(&cleanup_id);
            });
        }
        result
    }
    pub fn present(&self, id: String, bounds: Bounds, visible: bool) -> NativeResult<()> {
        if !bounds.valid() {
            return Err("视图尺寸无效".into());
        }
        self.call(move || platform::present(&id, bounds, visible))
    }
    pub fn eval(&self, id: String, script: String) -> NativeResult<()> {
        self.call(move || platform::eval(&id, &script))
    }
    pub fn post_eval(&self, id: String, script: String) -> NativeResult<()> {
        let previous = self.pending.fetch_add(1, Ordering::SeqCst);
        let permit = Permit(self.pending.clone());
        if previous >= 128 {
            return Err("原生界面队列已满".into());
        }
        self.send
            .try_send(Task {
                run: Box::new(move || {
                    let _ = platform::eval(&id, &script);
                }),
                _permit: permit,
            })
            .map_err(|_| "原生界面调度不可用".into())
    }
    pub fn notify(&self, id: String, kind: &str, payload: Value) -> NativeResult<()> {
        let notification = serde_json::json!({"type":kind,"payload":payload});
        self.eval(id, format!("window.__sodeckNotify?.({notification});"))
    }
    pub fn close(&self, id: String) -> NativeResult<()> {
        self.call(move || platform::close(&id))
    }
    pub fn focus(&self, id: String) -> NativeResult<()> {
        self.call(move || platform::focus(&id))
    }
    pub fn modal(&self, window: String, open: bool) -> NativeResult<()> {
        self.call(move || platform::modal(&window, open))
    }
    pub fn backgrounds(&self, ids:Vec<String>,color:(u8,u8,u8,u8)) {
        let _=self.app.run_on_main_thread(move||{for id in ids{let _=platform::background(&id,color);}});
    }
    pub fn diagnostics(&self) -> NativeResult<Value> {
        let mut counts = self.call(|| Ok(platform::diagnostics()))?;
        counts["pendingTasks"] = self.pending.load(Ordering::SeqCst).into();
        Ok(counts)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
        ProcessFailedEventHandler,
    };
    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
    use wry::raw_window_handle::{
        HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
    };
    use wry::{WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};
    struct Container(HWND);
    impl HasWindowHandle for Container {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let handle = Win32WindowHandle::new(
                std::num::NonZeroIsize::new(self.0 .0 as isize).ok_or(HandleError::Unavailable)?,
            );
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
        }
    }
    impl Drop for Container {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    // A child Wry view tears down its parent's Wry subclass on drop. Giving
    // each surface a private parent keeps it away from the shell's callbacks.
    // Field order ensures WebView/COM teardown runs before Container::drop.
    struct Surface {
        webview: WebView,
        _context: WebContext,
        container: Container,
        window: tauri::WebviewWindow,
        bounds: Cell<Bounds>,
        visible: Cell<bool>,
        shown: Cell<bool>,
    }
    thread_local! {
        static SURFACES: RefCell<HashMap<String,Rc<Surface>>> = RefCell::new(HashMap::new());
        static MODALS: RefCell<HashMap<String,bool>> = RefCell::new(HashMap::new());
    }
    fn surface(id: &str) -> NativeResult<Rc<Surface>> {
        SURFACES
            .with(|s| s.borrow().get(id).cloned())
            .ok_or_else(|| "原生表面不存在".into())
    }
    pub fn create(
        app: &tauri::AppHandle,
        id: String,
        label: String,
        entry: String,
        context: Value,
        assets: Assets,
        ipc: Ipc,
        failed: Failure,
        profile: std::path::PathBuf,
    ) -> NativeResult<()> {
        if SURFACES.with(|s| s.borrow().contains_key(&id)) {
            return Err("原生表面已存在".into());
        }
        let window = app.get_webview_window(&label).ok_or("目标窗口不存在")?;
        let parent = window.hwnd().map_err(|e| e.to_string())?;
        let container = Container(
            unsafe {
                CreateWindowExW(
                    WS_EX_NOPARENTNOTIFY,
                    windows::core::w!("STATIC"),
                    windows::core::w!("SoDeck plugin surface"),
                    WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                    0,
                    0,
                    1,
                    1,
                    Some(parent),
                    None,
                    None,
                    None,
                )
            }
            .map_err(|e| e.to_string())?,
        );
        std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let mut web_context = WebContext::new(Some(profile));
        let prefix = format!("/{id}/");
        let url = format!("http://sodeck-plugin.localhost/{id}/{entry}");
        let allowed_url = url.clone();
        let source = id.clone();
        let dialog=context.get("dialog").is_some();
        let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_transparent(dialog)
            .with_background_color(if dialog{(0,0,0,0)}else{crate::theme::background(&context["theme"])})
            .with_initialization_script(&crate::theme::bootstrap_surface(&context["theme"],dialog))
            .with_initialization_script(&format!("Object.defineProperty(window,'__SODECK_CONTEXT__',{{value:Object.freeze({context}),writable:false}});"))
            .with_initialization_script(r#"(() => {
                let sequence=0;
                const focus = event => {
                    if (window.__SODECK_CONTEXT__.dialog || !event.isTrusted || event.type==='keydown' && event.key!=='Tab') return;
                    window.ipc.postMessage(JSON.stringify({apiVersion:1,requestId:'host-focus-'+(++sequence),method:'views.requestFocus',params:{viewId:window.__SODECK_CONTEXT__.viewId,userGesture:true}}));
                };
                document.addEventListener('pointerdown',focus,true);
                document.addEventListener('keydown',focus,true);
            })();"#)
            .with_custom_protocol("sodeck-plugin".into(), move |_, request| {
                let decoded = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8().ok();
                let relative = decoded.as_deref().and_then(|path| path.strip_prefix(&prefix));
                let data = relative.filter(|p| !p.contains('\\') && p.split('/').all(|s| s != ".." && s != "." && !s.is_empty())).and_then(|path| assets.get(path));
                let mime = relative.map(|p| mime_guess::from_path(p).first_or_octet_stream().to_string()).unwrap_or_else(|| "text/plain".into());
                wry::http::Response::builder().status(if data.is_some() {200} else {404})
                    .header("Content-Type", mime).header("Cache-Control","no-store").header("X-Content-Type-Options","nosniff")
                    .header("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; object-src 'none'")
                    .body(std::borrow::Cow::Owned(data.unwrap_or(b"Resource unavailable").to_vec())).unwrap()
            })
            .with_navigation_handler(move |next| next == allowed_url)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_ipc_handler(move |request| ipc(source.clone(), request.body().clone()))
            .with_focused(false).with_visible(false)
            .with_bounds(wry::Rect { position: wry::dpi::LogicalPosition::new(0.,0.).into(), size: wry::dpi::LogicalSize::new(1.,1.).into() });
        let debug=app.state::<crate::startup_config::DebugConfig>();
        if let Some(args)=&debug.webview_args {builder=builder.with_additional_browser_args(args.clone());}
        if let Some(port)=debug.plugin_ports.get(context["pluginId"].as_str().unwrap_or("")) {builder=builder.with_additional_browser_args(format!("--remote-debugging-port={port}"));}
        let webview = builder
            .build_as_child(&container)
            .map_err(|e| e.to_string())?;
        let fail_id = id.clone();
        unsafe {
            let core = webview.webview();
            let mut token = 0;
            core.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, _| {
                    failed(fail_id.clone());
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| e.to_string())?;
        }
        let native = Rc::new(Surface {
            webview,
            _context: web_context,
            container,
            window,
            bounds: Cell::new(Bounds::default()),
            visible: Cell::new(false),
            shown: Cell::new(false),
        });
        SURFACES.with(|s| s.borrow_mut().insert(id.clone(), native.clone()));
        if let Err(error) = native.webview.load_url(&url) {
            let _ = close(&id);
            return Err(error.to_string());
        }
        Ok(())
    }
    fn apply(surface: &Surface) -> NativeResult<()> {
        let bounds = surface.bounds.get();
        let blocked = MODALS.with(|m| {
            m.borrow()
                .get(surface.window.label())
                .copied()
                .unwrap_or(false)
        });
        let visible =
            surface.visible.get() && !blocked && bounds.width >= 1. && bounds.height >= 1.;
        surface.shown.set(visible);
        if !visible {
            surface
                .webview
                .set_visible(false)
                .map_err(|e| e.to_string())?;
            unsafe {
                let _ = ShowWindow(surface.container.0, SW_HIDE);
            }
        }
        if bounds.width >= 1. && bounds.height >= 1. {
            let scale = surface.window.scale_factor().map_err(|e| e.to_string())?;
            unsafe {
                SetWindowPos(
                    surface.container.0,
                    Some(HWND_TOP),
                    (bounds.x * scale).round() as i32,
                    (bounds.y * scale).round() as i32,
                    (bounds.width * scale).round() as i32,
                    (bounds.height * scale).round() as i32,
                    SWP_NOACTIVATE,
                )
            }
            .map_err(|e| e.to_string())?;
            surface
                .webview
                .set_bounds(wry::Rect {
                    position: wry::dpi::LogicalPosition::new(0., 0.).into(),
                    size: wry::dpi::LogicalSize::new(bounds.width, bounds.height).into(),
                })
                .map_err(|e| e.to_string())?;
        }
        if visible {
            unsafe {
                let _ = ShowWindow(surface.container.0, SW_SHOWNOACTIVATE);
            }
            surface
                .webview
                .set_visible(true)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn present(id: &str, bounds: Bounds, visible: bool) -> NativeResult<()> {
        let s = surface(id)?;
        s.bounds.set(bounds);
        s.visible.set(visible);
        apply(&s)
    }
    pub fn background(id:&str,color:(u8,u8,u8,u8))->NativeResult<()>{surface(id)?.webview.set_background_color(color).map_err(|e|e.to_string())}
    pub fn eval(id: &str, script: &str) -> NativeResult<()> {
        surface(id)?
            .webview
            .evaluate_script(script)
            .map_err(|e| e.to_string())
    }
    pub fn close(id: &str) -> NativeResult<()> {
        let native = SURFACES.with(|s| s.borrow_mut().remove(id));
        if let Some(native) = native {
            unsafe {
                native
                    .webview
                    .controller()
                    .Close()
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub fn focus(id: &str) -> NativeResult<()> {
        let s = surface(id)?;
        if !s.visible.get() || !s.window.is_focused().unwrap_or(false) || MODALS.with(|m|m.borrow().get(s.window.label()).copied().unwrap_or(false)) {
            return Ok(());
        }
        unsafe {
            s.webview
                .controller()
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
                .map_err(|e| e.to_string())
        }
    }
    pub fn modal(label: &str, open: bool) -> NativeResult<()> {
        MODALS.with(|m| {
            if open {
                m.borrow_mut().insert(label.to_string(), true);
            } else {
                m.borrow_mut().remove(label);
            }
        });
        let surfaces = SURFACES.with(|s| {
            s.borrow()
                .values()
                .filter(|s| s.window.label() == label)
                .cloned()
                .collect::<Vec<_>>()
        });
        for s in surfaces {
            apply(&s)?;
        }
        Ok(())
    }
    pub fn diagnostics() -> Value {
        serde_json::json!({"surfaces":SURFACES.with(|s|s.borrow().len()),"modalWindows":MODALS.with(|m|m.borrow().len())})
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn create(
        _: &tauri::AppHandle,
        _: String,
        _: String,
        _: String,
        _: Value,
        _: Assets,
        _: Ipc,
        _: Failure,
        _: std::path::PathBuf,
    ) -> NativeResult<()> {
        Err("本批原生插件视图仅验证 Windows".into())
    }
    pub fn present(_: &str, _: Bounds, _: bool) -> NativeResult<()> {
        Err("原生视图不可用".into())
    }
    pub fn background(_:&str,_:(u8,u8,u8,u8))->NativeResult<()>{Ok(())}
    pub fn eval(_: &str, _: &str) -> NativeResult<()> {
        Err("原生视图不可用".into())
    }
    pub fn close(_: &str) -> NativeResult<()> {
        Ok(())
    }
    pub fn focus(_: &str) -> NativeResult<()> {
        Err("原生视图不可用".into())
    }
    pub fn modal(_: &str, _: bool) -> NativeResult<()> {
        Ok(())
    }
    pub fn diagnostics() -> Value {
        serde_json::json!({"surfaces":0,"modalWindows":0})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nonfinite_or_negative_geometry() {
        assert!(!Bounds {
            width: f64::NAN,
            ..Bounds::default()
        }
        .valid());
        assert!(!Bounds {
            height: -1.,
            ..Bounds::default()
        }
        .valid());
    }
    #[test]
    fn assets_are_immutable_after_loading() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "first").unwrap();
        let a = Assets::load(dir.path()).unwrap();
        std::fs::write(dir.path().join("index.html"), "second").unwrap();
        assert_eq!(a.get("index.html"), Some(b"first".as_slice()));
    }
}
