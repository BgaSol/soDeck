use super::validate_target;
use crate::{
    model::{AppError, AppResult},
    ssh_transport::forwarding::Target,
};
use http::{header, HeaderMap, HeaderName, Uri};

pub fn target(authority: &str, explicit_port: bool) -> AppResult<Target> {
    let a: http::uri::Authority = authority.parse().map_err(|_| invalid())?;
    if a.as_str().contains('@') || (explicit_port && a.port_u16().is_none()) {
        return Err(invalid());
    }
    validate_target(a.host(), a.port_u16().unwrap_or(80))
}
pub fn origin(uri: &Uri, websocket: bool) -> AppResult<(Target, Uri, String)> {
    if uri.to_string().contains('#') {
        return Err(invalid());
    }
    match uri.scheme_str() {
        Some("http") => {}
        Some("ws") if websocket => {}
        _ => return Err(invalid()),
    }
    let authority = uri.authority().ok_or_else(invalid)?;
    let target = target(authority.as_str(), false)?;
    let path: Uri = uri
        .path_and_query()
        .map(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .unwrap_or("/")
        .parse()
        .map_err(|_| invalid())?;
    Ok((target, path, authority.as_str().into()))
}
pub fn websocket(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"websocket"))
        && headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|s| s.split(','))
            .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
}
pub fn filter(headers: &mut HeaderMap, upgrade: bool) {
    let named: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(','))
        .filter_map(|s| HeaderName::from_bytes(s.trim().as_bytes()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in [
        "connection",
        "proxy-connection",
        "keep-alive",
        "proxy-authorization",
        "proxy-authenticate",
        "transfer-encoding",
        "trailer",
        "te",
        "upgrade",
    ] {
        headers.remove(name);
    }
    if upgrade {
        headers.insert(
            header::CONNECTION,
            http::HeaderValue::from_static("upgrade"),
        );
        headers.insert(header::UPGRADE, http::HeaderValue::from_static("websocket"));
    }
}
pub fn trailers(headers: &mut HeaderMap) {
    filter(headers, false);
    for name in [
        "host",
        "content-length",
        "content-type",
        "content-encoding",
        "authorization",
        "cookie",
        "set-cookie",
        "expect",
    ] {
        headers.remove(name);
    }
}
fn invalid() -> AppError {
    AppError::new("validation", "代理需要包含主机和端口的有效目标地址")
}
