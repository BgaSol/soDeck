//! Read AppKit button frames only during the explicit packaged-application smoke test.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSWindow, NSWindowButton};
use std::{sync::mpsc, time::Duration};

pub(super) fn verify(window: &tauri::WebviewWindow) -> Result<(), String> {
    let label = window.label().to_owned();
    let (send, receive) = mpsc::channel();
    window
        .with_webview(move |webview| {
            let result = (|| {
                let _main = MainThreadMarker::new().ok_or("AppKit 验收必须在主线程执行")?;
                // Tauri owns this NSWindow; borrow it only for this UI-thread callback.
                let native = unsafe { webview.ns_window().cast::<NSWindow>().as_ref() }
                    .ok_or("原生窗口不存在")?;
                let height = native
                    .contentView()
                    .ok_or("原生窗口内容不存在")?
                    .bounds()
                    .size
                    .height;
                for (kind, button) in [
                    NSWindowButton::CloseButton,
                    NSWindowButton::MiniaturizeButton,
                    NSWindowButton::ZoomButton,
                ]
                .into_iter()
                .enumerate()
                {
                    let control = native
                        .standardWindowButton(button)
                        .ok_or_else(|| format!("{label}: macOS 原生窗口按钮 {kind} 不存在"))?;
                    let rect = control.convertRect_toView(control.bounds(), None);
                    let center = height - rect.origin.y - rect.size.height / 2.;
                    if !center.is_finite() || (center - 22.).abs() > 1. {
                        return Err(format!(
                            "{label}: macOS 按钮 {kind} 中心距顶部 {center:.2}pt，应为 22pt"
                        ));
                    }
                    if rect.origin.x < 0. || rect.origin.x + rect.size.width > 86. {
                        return Err(format!("{label}: macOS 按钮 {kind} 超出标题栏左侧留位"));
                    }
                    println!(
                        "{label}: macOS button {kind} center={center:.2}pt, x={:.2}pt",
                        rect.origin.x
                    );
                }
                Ok(())
            })();
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    receive
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("无法验证 macOS 原生窗口按钮：{e}"))?
}
