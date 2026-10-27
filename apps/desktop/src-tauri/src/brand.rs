//! Frozen brand exports; corrupt optional development assets fall back locally.
use tauri::image::Image;
const WINDOW:&[u8]=include_bytes!("../../../../assets/brand/pocket-deck-256.png");
const TRAY:&[u8]=include_bytes!("../../../../assets/brand/pocket-deck-32.png");
const FALLBACK:&[u8]=include_bytes!("../../../../assets/brand/pocket-deck-16.png");
fn decode(bytes:&[u8])->Image<'static>{Image::from_bytes(bytes).unwrap_or_else(|error|{eprintln!("SoDeck brand: invalid asset ({error}); using embedded command icon");Image::from_bytes(FALLBACK).expect("validated embedded brand fallback")})}
pub fn icon(tray:bool)->Image<'static>{
    if let Some(path)=&crate::startup_config::debug().brand_file{return decode(&std::fs::read(path).unwrap_or_default());}
    decode(if tray{TRAY}else{WINDOW})
}
#[cfg(test)]mod tests{use super::*;#[test]fn corrupt_asset_retains_visible_fallback(){let icon=decode(b"invalid png");assert_eq!((icon.width(),icon.height()),(16,16));assert!(icon.rgba().chunks_exact(4).any(|p|p[3]>0));}#[test]fn exports_decode(){assert_eq!(decode(WINDOW).width(),256);assert_eq!(decode(TRAY).width(),32);}}
