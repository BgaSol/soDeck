#![cfg_attr(windows, windows_subsystem = "windows")]
use std::{fs::OpenOptions, io::Write};

fn main() {
    let mut file = OpenOptions::new().create(true).append(true)
        .open(env!("SODECK_OPEN_HANDLER_LOG")).unwrap();
    for arg in std::env::args_os().skip(1) {
        let hex: String = arg.as_encoded_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
        writeln!(file, "{hex}").unwrap();
    }
}
