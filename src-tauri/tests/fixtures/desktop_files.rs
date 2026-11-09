//! Separate clipboard owner for explicit, isolated native package tests.
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

fn ready() {
    println!("ready");
    io::stdout().flush().unwrap();
}

#[cfg(windows)]
fn set(paths: &[PathBuf], files: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HGLOBAL},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
            Ole::{CF_HDROP, CF_UNICODETEXT},
        },
        UI::{
            Shell::DROPFILES,
            WindowsAndMessaging::{CreateWindowExW, DestroyWindow},
        },
    };
    struct Allocation(HGLOBAL);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
    let window = unsafe {
        CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            windows_sys::w!("SoDeck clipboard fixture"),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err("Cannot create clipboard owner window".into());
    }
    struct Window(windows_sys::Win32::Foundation::HWND);
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    let _window = Window(window);
    let mut opened = false;
    for _ in 0..50 {
        if unsafe { OpenClipboard(window) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !opened {
        return Err("Cannot open test clipboard".into());
    }
    struct Board;
    impl Drop for Board {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    let _board = Board;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("Cannot clear test clipboard".into());
    }
    let mut wide = Vec::new();
    if files {
        for path in paths {
            wide.extend(path.as_os_str().encode_wide());
            wide.push(0);
        }
    } else {
        wide.extend("plain text file:///this-is-not-a-file".encode_utf16());
    }
    wide.push(0);
    let header = if files {
        std::mem::size_of::<DROPFILES>()
    } else {
        0
    };
    let memory =
        Allocation(unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, header + wide.len() * 2) });
    if memory.0.is_null() {
        return Err("Cannot allocate test clipboard".into());
    }
    let bytes = unsafe { GlobalLock(memory.0) }.cast::<u8>();
    if bytes.is_null() {
        return Err("Cannot lock test clipboard".into());
    }
    unsafe {
        if files {
            let drop = bytes.cast::<DROPFILES>();
            (*drop).pFiles = header as u32;
            (*drop).fWide = 1;
        }
        std::ptr::copy_nonoverlapping(
            wide.as_ptr().cast::<u8>(),
            bytes.add(header),
            wide.len() * 2,
        );
        GlobalUnlock(memory.0);
    }
    if unsafe {
        SetClipboardData(
            if files {
                CF_HDROP.into()
            } else {
                CF_UNICODETEXT.into()
            },
            memory.0,
        )
    }
    .is_null()
    {
        return Err("Cannot publish test clipboard".into());
    }
    std::mem::forget(memory); // Ownership transferred to the system clipboard.
    Ok(())
}

#[cfg(target_os = "macos")]
fn set(paths: &[PathBuf], files: bool) -> Result<(), String> {
    use objc2::{rc::autoreleasepool, runtime::ProtocolObject};
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL, NSPasteboardTypeString,
    };
    use objc2_foundation::{NSArray, NSString};
    autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        if files {
            let mut items = Vec::new();
            for path in paths {
                let url = reqwest::Url::from_file_path(path).map_err(|_| "Invalid test path")?;
                let item = NSPasteboardItem::new();
                if !item.setString_forType(&NSString::from_str(url.as_str()), unsafe {
                    NSPasteboardTypeFileURL
                }) {
                    return Err("Cannot write test file URL".into());
                }
                items.push(ProtocolObject::from_retained(item));
            }
            if !board.writeObjects(&NSArray::from_retained_slice(&items)) {
                return Err("Cannot publish test file list".into());
            }
        } else if !board
            .setString_forType(&NSString::from_str("file:///this-is-not-a-file"), unsafe {
                NSPasteboardTypeString
            })
        {
            return Err("Cannot publish test text".into());
        }
        Ok(())
    })
}

#[cfg(target_os = "linux")]
fn run(paths: Vec<PathBuf>) -> Result<(), String> {
    gtk::init().map_err(|e| e.to_string())?;
    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
    let set_files = move |clipboard: &gtk::Clipboard, gnome: bool| -> Result<(), String> {
        let target = if gnome {
            "x-special/gnome-copied-files"
        } else {
            "text/uri-list"
        };
        let mut data = if gnome {
            "copy\n".to_string()
        } else {
            String::new()
        };
        for path in &paths {
            data.push_str(
                reqwest::Url::from_file_path(path)
                    .map_err(|_| "Invalid test path")?
                    .as_str(),
            );
            data.push('\n');
        }
        if !clipboard.set_with_data(
            &[gtk::TargetEntry::new(target, gtk::TargetFlags::empty(), 0)],
            move |_, selection, _| {
                selection.set(&gdk::Atom::intern(target), 8, data.as_bytes());
            },
        ) {
            return Err("Cannot publish test clipboard".into());
        }
        Ok(())
    };
    set_files(&clipboard, false)?;
    ready();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if let Ok(line) = line {
                let _ = sender.send(line);
            }
        }
        let _ = sender.send("quit".into());
    });
    glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        while let Ok(command) = receiver.try_recv() {
            if command == "quit" {
                clipboard.clear();
                gtk::main_quit();
                return glib::ControlFlow::Break;
            }
            if command == "text" {
                clipboard.set_text("file:///this-is-not-a-file");
            } else if command == "files" || command == "gnome" {
                if let Err(error) = set_files(&clipboard, command == "gnome") {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            ready();
        }
        glib::ControlFlow::Continue
    });
    gtk::main();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn run(paths: Vec<PathBuf>) -> Result<(), String> {
    set(&paths, true)?;
    ready();
    for command in io::stdin().lock().lines() {
        let command = command.map_err(|e| e.to_string())?;
        if command == "quit" {
            break;
        }
        set(&paths, command == "files")?;
        ready();
    }
    Ok(())
}
fn main() {
    let result = (|| {
        let file = std::env::args_os()
            .nth(1)
            .ok_or("Provide test paths JSON")?;
        let paths: Vec<PathBuf> =
            serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if paths.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
            return Err("Use nonempty absolute test paths".into());
        }
        run(paths)
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
