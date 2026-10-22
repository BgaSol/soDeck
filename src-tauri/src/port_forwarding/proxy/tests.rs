use super::*;
#[test]
fn authorities_and_hop_headers_are_strict() {
    for host in [
        "user@host",
        "[[::1]]",
        "host/path",
        "host\r\nsecret",
        "[::1",
        "name%zone",
    ] {
        assert!(validate_target(host, 80).is_err(), "{host}");
    }
    assert_eq!(headers::target("[::1]:4310", true).unwrap().host, "::1");
    assert!(headers::target("site", true).is_err());
    assert!(headers::origin(&"https://site/path".parse().unwrap(), false).is_err());
    let (_, uri, host) =
        headers::origin(&"http://site:4310/a?q=1".parse().unwrap(), false).unwrap();
    assert_eq!(uri.to_string(), "/a?q=1");
    assert_eq!(host, "site:4310");
    let mut h = ::http::HeaderMap::new();
    h.insert("connection", "x-private, keep-alive".parse().unwrap());
    h.insert("x-private", "secret".parse().unwrap());
    h.insert("proxy-authorization", "secret".parse().unwrap());
    h.insert("cookie", "business".parse().unwrap());
    headers::filter(&mut h, false);
    assert_eq!(h.len(), 1);
    assert!(h.contains_key("cookie"));
}
