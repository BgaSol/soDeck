//! One validated color snapshot for every renderer. No renderer owns fallback colors.
use serde_json::{json, Value};
use std::path::Path;

pub const DEFAULT: &str = include_str!("../../../../themes/graphite.json");

pub fn validate(value: &Value) -> Result<(), String> {
    let template: Value = serde_json::from_str(DEFAULT).expect("bundled theme");
    if !value.as_object().is_some_and(|o| o.len()==5 && ["schemaVersion","id","mode","colors","terminal"].iter().all(|k|o.contains_key(*k))) {return Err("主题结构无效".into());}
    if value["schemaVersion"] != 1 || !matches!(value["mode"].as_str(), Some("dark" | "light"))
        || !value["id"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128) {
        return Err("主题版本或标识无效".into());
    }
    fn color(v: &Value) -> bool {
        v.as_str().is_some_and(|s| (s.len() == 7 || s.len() == 9) && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
    }
    for section in ["colors", "terminal"] {
        let actual = value[section].as_object().ok_or("主题颜色表缺失")?;
        let expected = template[section].as_object().unwrap();
        if actual.len() != expected.len() { return Err("主题颜色字段不完整或未知".into()); }
        for key in expected.keys() {
            let valid = if key == "extendedAnsi" {
                actual.get(key).and_then(Value::as_array).is_some_and(|a| a.len() == 240 && a.iter().all(color))
            } else { actual.get(key).is_some_and(color) };
            if !valid { return Err(format!("主题颜色无效：{section}.{key}")); }
        }
    }
    Ok(())
}

pub fn load(resource: &Path) -> (Value, Option<String>) {
    let default: Value = serde_json::from_str(DEFAULT).expect("bundled theme");
    let override_path = std::env::var_os("SODECK_THEME_FILE").map(std::path::PathBuf::from);
    let path = override_path.as_deref().unwrap_or(resource);
    let selected = if path.exists() || override_path.is_some() {
        std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| {
            if b.len() > 128 * 1024 { return Err("主题文件过大".into()); }
            let v = serde_json::from_slice(&b).map_err(|e| e.to_string())?;
            validate(&v)?; Ok(v)
        })
    } else { Ok(default.clone()) };
    let (mut value, warning) = match selected { Ok(v) => (v, None), Err(e) => (default, Some(format!("主题未加载，已恢复默认主题：{e}"))) };
    apply_system_preferences(&mut value);
    value["revision"] = json!(1);
    (value, warning)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn complete_theme_only() {
        let v: Value = serde_json::from_str(DEFAULT).unwrap(); assert!(validate(&v).is_ok());
        let mut bad = v.clone(); bad["colors"]["hover"] = json!("red"); assert!(validate(&bad).is_err());
        let mut bad = v.clone(); bad["terminal"].as_object_mut().unwrap().remove("cursor"); assert!(validate(&bad).is_err());
        let mut bad = v; bad["terminal"]["extendedAnsi"] = json!([]); assert!(validate(&bad).is_err());
    }
}

#[cfg(not(windows))] fn apply_system_preferences(_: &mut Value) {}
#[cfg(windows)] fn apply_system_preferences(value: &mut Value) {
    use windows::Win32::{UI::{Accessibility::*,WindowsAndMessaging::*},Graphics::Gdi::*};
    let mut high=HIGHCONTRASTW{cbSize:std::mem::size_of::<HIGHCONTRASTW>() as u32,..Default::default()};
    if unsafe{SystemParametersInfoW(SPI_GETHIGHCONTRAST,high.cbSize,Some((&mut high as *mut HIGHCONTRASTW).cast()),SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))}.is_err() || high.dwFlags.0&HCF_HIGHCONTRASTON.0==0 {return;}
    fn color(index:SYS_COLOR_INDEX)->String {let c=unsafe{GetSysColor(index)};format!("#{:02x}{:02x}{:02x}",c&255,(c>>8)&255,(c>>16)&255)}
    let bg=color(COLOR_WINDOW);let fg=color(COLOR_WINDOWTEXT);let selected=color(COLOR_HIGHLIGHT);let selected_text=color(COLOR_HIGHLIGHTTEXT);let disabled=color(COLOR_GRAYTEXT);
    for key in ["canvas","surface","elevated","field"]{value["colors"][key]=json!(bg);}
    for key in ["text","secondary","muted","border","success","warning","error"]{value["colors"][key]=json!(fg);}
    for key in ["accent","focus","hover","pressed","selected","dangerHover","dragBackground","selection"]{value["colors"][key]=json!(selected);}
    for key in ["selectedText","dangerText"]{value["colors"][key]=json!(selected_text);}
    value["colors"]["disabled"]=json!(disabled);value["colors"]["scrollbar"]=json!(fg);
    for key in ["foreground","cursor","scrollbarSliderBackground","scrollbarSliderHoverBackground","scrollbarSliderActiveBackground","overviewRulerBorder"]{value["terminal"][key]=json!(fg);}
    for key in ["background","cursorAccent"]{value["terminal"][key]=json!(bg);}
    for key in ["selectionBackground","selectionInactiveBackground"]{value["terminal"][key]=json!(selected);}
    value["terminal"]["selectionForeground"]=json!(selected_text);
}
