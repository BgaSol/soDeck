//! Transient native menus: the host owns presentation, the caller owns commands.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::{Arc, Mutex}};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item { pub id: String, pub label: String, pub enabled: bool, #[serde(default)] pub shortcut: Option<String>, #[serde(default)] pub separator: bool }
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultState { pub menu_id: String, pub status: String, pub selected_id: Option<String> }
struct Entry { source: String, window: String, result: ResultState }
#[derive(Default)]
pub struct Menus { entries: Mutex<HashMap<String, Entry>> }
impl Menus {
    pub fn begin(&self, id: &str, source: &str, window: &str) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        if entries.contains_key(id) { return Err("菜单标识已存在".into()); }
        if entries.len() >= 256 { entries.retain(|_, v| v.result.status == "open"); }
        if entries.len() >= 256 { return Err("菜单数量超出限制".into()); }
        entries.insert(id.into(), Entry { source: source.into(), window: window.into(), result: ResultState { menu_id: id.into(), status: "open".into(), selected_id: None } });
        Ok(())
    }
    pub fn get(&self, id: &str, source: &str) -> Result<ResultState, String> {
        self.entries.lock().unwrap().get(id).filter(|e| e.source == source).map(|e| e.result.clone()).ok_or("菜单来源失效".into())
    }
    pub fn finish(&self, id: &str, selected: Option<String>) -> Option<ResultState> {
        let mut entries = self.entries.lock().unwrap(); let e = entries.get_mut(id)?;
        if e.result.status != "open" { return None; }
        e.result.status = "closed".into(); e.result.selected_id = selected; Some(e.result.clone())
    }
    pub fn cancel_window(&self, window: &str) -> Vec<(String, ResultState)> {
        let mut entries = self.entries.lock().unwrap(); let mut closed = vec![];
        for e in entries.values_mut().filter(|e| e.window == window && e.result.status == "open") {
            e.result.status = "closed".into(); closed.push((e.source.clone(), e.result.clone()));
        }
        closed
    }
}
pub type Completed = Arc<dyn Fn(Option<String>) + Send + Sync>;

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{cell::RefCell, sync::atomic::{AtomicUsize, Ordering}};
    use windows::{Win32::{Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::{Controls::*, WindowsAndMessaging::*}}};
    static OWNER: AtomicUsize = AtomicUsize::new(0);
    static TICKET: AtomicUsize = AtomicUsize::new(0);
    static SERIAL: Mutex<()> = Mutex::new(());
    struct Paint { items: Vec<Item>, theme: Value, dpi: f64 }
    thread_local! { static PAINT: RefCell<Option<Paint>> = const { RefCell::new(None) }; }
    fn color(theme: &Value, key: &str) -> COLORREF {
        let value = theme["colors"][key].as_str().unwrap();
        let n = u32::from_str_radix(&value[1..7], 16).unwrap();
        COLORREF(((n >> 16) & 255) | (n & 0xff00) | ((n & 255) << 16))
    }
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_MEASUREITEM || msg == WM_DRAWITEM {
            let handled = PAINT.with(|p| { let p = p.borrow(); let Some(p) = p.as_ref() else { return false; };
                if msg == WM_MEASUREITEM {
                    let m = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
                    if m.CtlType != ODT_MENU || m.itemData == 0 { return false; }
                    let Some(item) = p.items.get(m.itemData - 1) else { return false; };
                    let len = item.label.chars().map(|c| if c.is_ascii() { 7 } else { 14 }).sum::<u32>() + item.shortcut.as_ref().map(|s| s.len() as u32 * 7 + 40).unwrap_or(0);
                    m.itemWidth = ((len.max(180).min(700) + 32) as f64 * p.dpi) as u32;
                    m.itemHeight = ((if item.separator { 40.0 } else { 32.0 }) * p.dpi) as u32; true
                } else {
                    let d = &*(lp.0 as *const DRAWITEMSTRUCT);
                    if d.CtlType != ODT_MENU || d.itemData == 0 { return false; }
                    let Some(item) = p.items.get(d.itemData - 1) else { return false; };
                    let selected = d.itemState.0 & ODS_SELECTED.0 != 0;
                    let brush = CreateSolidBrush(color(&p.theme, if selected { "selected" } else { "elevated" }));
                    FillRect(d.hDC, &d.rcItem, brush); let _ = DeleteObject(brush.into());
                    SetBkMode(d.hDC, TRANSPARENT);
                    SetTextColor(d.hDC, color(&p.theme, if !item.enabled { "disabled" } else if selected { "selectedText" } else { "text" }));
                    let font = CreateFontW(-(13.0*p.dpi) as i32, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, windows::core::w!("Segoe UI"));
                    let previous = SelectObject(d.hDC, font.into());
                    let pad = (12.0*p.dpi) as i32;
                    let mut rect = d.rcItem; rect.left += pad; rect.right -= pad;
                    if item.separator { rect.top += (6.0*p.dpi) as i32; }
                    let mut text: Vec<u16> = item.label.encode_utf16().collect();
                    DrawTextW(d.hDC, &mut text, &mut rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                    if let Some(shortcut) = &item.shortcut {
                        SetTextColor(d.hDC, color(&p.theme, if !item.enabled { "disabled" } else if selected { "selectedText" } else { "secondary" }));
                        let mut text: Vec<u16> = shortcut.encode_utf16().collect();
                        DrawTextW(d.hDC, &mut text, &mut rect, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                    }
                    SelectObject(d.hDC, previous); let _ = DeleteObject(font.into()); true
                }
            });
            if handled { return LRESULT(1); }
        }
        DefWindowProcW(hwnd,msg,wp,lp)
    }
    pub fn cancel() {
        TICKET.fetch_add(1,Ordering::SeqCst);
        let hwnd=OWNER.load(Ordering::SeqCst);
        if hwnd!=0 { unsafe {let _=PostMessageW(Some(HWND(hwnd as *mut _)),WM_CANCELMODE,WPARAM(0),LPARAM(0));} }
    }
    pub fn owns_focus() -> bool { let hwnd=OWNER.load(Ordering::SeqCst);hwnd!=0 && unsafe{GetForegroundWindow().0 as usize==hwnd} }
    pub fn show(hwnd: usize, point: (i32,i32), items: Vec<Item>, theme: Value, dpi: f64, completed: Completed) {
        // A separate native message loop must not block Tao's user-event dispatcher.
        let ticket=TICKET.fetch_add(1,Ordering::SeqCst)+1;
        let worker_completed=completed.clone();
        let started=std::thread::Builder::new().name("sodeck-native-menu".into()).spawn(move||{
        let _serial=SERIAL.lock().unwrap();
        if TICKET.load(Ordering::SeqCst)!=ticket {worker_completed(None);return;}
        let result = unsafe { (|| -> windows::core::Result<Option<String>> {
            let parent = HWND(hwnd as *mut _);
            let instance=GetModuleHandleW(None)?;
            let class=windows::core::w!("SoDeckTransientMenuOwner");
            let wc=WNDCLASSW{lpfnWndProc:Some(procedure),hInstance:instance.into(),lpszClassName:class,..Default::default()};
            let _=RegisterClassW(&wc);
            let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW,class,windows::core::w!(""),WS_POPUP,0,0,0,0,Some(parent),None,Some(instance.into()),None)?;
            struct OwnerGuard(HWND);
            impl Drop for OwnerGuard {fn drop(&mut self){OWNER.store(0,Ordering::SeqCst);unsafe{let _=DestroyWindow(self.0);}}}
            let _owner=OwnerGuard(hwnd);
            OWNER.store(hwnd.0 as usize,Ordering::SeqCst);
            if TICKET.load(Ordering::SeqCst)!=ticket {return Ok(None);}
            let menu = CreatePopupMenu()?;
            let brush = CreateSolidBrush(color(&theme,"elevated"));
            let info = MENUINFO { cbSize: std::mem::size_of::<MENUINFO>() as u32, fMask: MIM_BACKGROUND, hbrBack: brush, ..Default::default() };
            let _ = SetMenuInfo(menu,&info);
            for (i,item) in items.iter().enumerate() {
                let mut label: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                let info = MENUITEMINFOW { cbSize:std::mem::size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_ID|MIIM_FTYPE|MIIM_STATE|MIIM_DATA|MIIM_STRING, fType:MFT_OWNERDRAW, fState:if item.enabled { MFS_ENABLED } else { MFS_DISABLED }, wID:(i+1) as u32, dwItemData:i+1, dwTypeData:windows::core::PWSTR(label.as_mut_ptr()),cch:(label.len()-1) as u32, ..Default::default() };
                if let Err(e)=InsertMenuItemW(menu,i as u32,true,&info) { let _=DestroyMenu(menu);let _=DeleteObject(brush.into());return Err(e); }
            }
            PAINT.with(|p| *p.borrow_mut()=Some(Paint{items:items.clone(),theme,dpi}));
            let _ = SetForegroundWindow(hwnd);
            let choice = TrackPopupMenuEx(menu, (TPM_RETURNCMD|TPM_NONOTIFY|TPM_RIGHTBUTTON).0,point.0,point.1,hwnd,None).0 as usize;
            if GetForegroundWindow()==hwnd { let _=SetForegroundWindow(parent); }
            PAINT.with(|p| *p.borrow_mut()=None);
            let _ = DestroyMenu(menu); let _ = DeleteObject(brush.into());
            let _ = PostMessageW(Some(hwnd),WM_NULL,WPARAM(0),LPARAM(0));
            Ok(choice.checked_sub(1).and_then(|i|items.get(i)).filter(|i|i.enabled).map(|i|i.id.clone()))
        })() };
        worker_completed(result.ok().flatten());
        });
        if started.is_err(){completed(None);}
    }
}
#[cfg(windows)] pub use platform::{show,cancel,owns_focus};
#[cfg(not(windows))] pub fn cancel() {}
#[cfg(not(windows))] pub fn owns_focus()->bool{false}
#[cfg(not(windows))] pub fn show(_:usize,_:(i32,i32),_:Vec<Item>,_:Value,_:f64,completed:Completed){completed(None);}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn menu_source_and_single_completion() {
        let m=Menus::default();m.begin("one","surface-a","main").unwrap();
        assert!(m.get("one","surface-b").is_err());assert!(m.begin("one","surface-a","main").is_err());
        assert_eq!(m.cancel_window("main").len(),1);assert!(m.finish("one",Some("late".into())).is_none());
        assert!(m.get("one","surface-a").unwrap().selected_id.is_none());
    }
}
