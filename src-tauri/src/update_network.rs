//! GitHub update transport. Release identity and package verification remain with callers.
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_METADATA: usize = 2 * 1024 * 1024;
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const METADATA_BUDGET: Duration = Duration::from_secs(45);
const METADATA_TIMEOUT: Duration = Duration::from_secs(8);
const HEADER_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinMirror {
    pub id: &'static str,
    pub prefix: &'static str,
    pub supports_api: bool,
}

pub const BUILTIN_MIRRORS: &[BuiltinMirror] = &[
    BuiltinMirror {
        id: "axisnow",
        prefix: "https://axisnow.gh-proxy.org/",
        supports_api: true,
    },
    BuiltinMirror {
        id: "v4",
        prefix: "https://v4.gh-proxy.org/",
        supports_api: true,
    },
    BuiltinMirror {
        id: "v6",
        prefix: "https://v6.gh-proxy.org/",
        supports_api: true,
    },
    BuiltinMirror {
        id: "gh-proxy-org",
        prefix: "https://gh-proxy.org/",
        supports_api: true,
    },
    BuiltinMirror {
        id: "gh-proxy-com",
        prefix: "https://gh-proxy.com/",
        supports_api: true,
    },
    BuiltinMirror {
        id: "ghfast",
        prefix: "https://ghfast.top/",
        supports_api: false,
    },
    BuiltinMirror {
        id: "ghproxy",
        prefix: "https://ghproxy.net/",
        supports_api: false,
    },
];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMirror {
    #[serde(default)]
    pub name: String,
    pub prefix: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

fn enabled_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub revision: u64,
    pub fallback_enabled: bool,
    pub disabled_builtins: Vec<String>,
    pub custom_mirrors: Vec<UpdateMirror>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            revision: 0,
            fallback_enabled: true,
            disabled_builtins: Vec::new(),
            custom_mirrors: Vec::new(),
        }
    }
}

pub fn normalize_prefix(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() > 2048 {
        return Err("加速地址不能超过 2048 字符".into());
    }
    let mut url = Url::parse(value).map_err(|_| "请输入完整的 HTTPS 加速地址".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("加速地址必须为 HTTPS，不能包含凭据、查询参数或片段".into());
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url.into())
}

impl Preferences {
    pub fn validated(mut self) -> Result<Self, String> {
        if self.custom_mirrors.len() > 10 {
            return Err("最多添加 10 个自定义加速地址".into());
        }
        let mut prefixes: HashSet<String> =
            BUILTIN_MIRRORS.iter().map(|m| m.prefix.into()).collect();
        for mirror in &mut self.custom_mirrors {
            mirror.name = mirror.name.trim().into();
            if mirror.name.chars().count() > 64 {
                return Err("地址名称不能超过 64 字符".into());
            }
            mirror.prefix = normalize_prefix(&mirror.prefix)?;
            if !prefixes.insert(mirror.prefix.clone()) {
                return Err("加速地址已存在，请勿重复添加".into());
            }
        }
        if self.disabled_builtins.len() > 32
            || self.disabled_builtins.iter().any(|id| id.len() > 64)
        {
            return Err("停用地址配置无效".into());
        }
        let mut seen = HashSet::new();
        self.disabled_builtins.retain(|id| seen.insert(id.clone()));
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum RequestKind {
    Api,
    File,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attempt {
    pub endpoint: String,
    pub attempt: usize,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub name: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

pub(crate) fn diagnostic<T>(
    results: &mut Vec<Diagnostic>,
    name: &str,
    run: impl FnOnce() -> Result<T, String>,
) -> Option<T> {
    let start = Instant::now();
    let result = run();
    results.push(Diagnostic {
        name: name.into(),
        ok: result.is_ok(),
        elapsed_ms: start.elapsed().as_millis() as u64,
        error: result.as_ref().err().cloned(),
    });
    result.ok()
}

#[derive(Default)]
pub struct State {
    inner: Mutex<Health>,
    epoch: AtomicU64,
}
#[derive(Default)]
struct Health {
    cooldown: HashMap<(RequestKind, String), Instant>,
    preferred: HashMap<RequestKind, String>,
}
impl State {
    pub fn clear(&self) {
        let mut health = self.inner.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::AcqRel);
        *health = Health::default();
    }
}

#[derive(Debug)]
pub enum Error {
    Remote(String, Duration),
    Local(String),
    Rejected(String),
    Cancelled,
}
impl Error {
    pub fn remote(message: impl Into<String>) -> Self {
        Self::Remote(message.into(), Duration::from_secs(300))
    }
    pub fn message(&self) -> String {
        match self {
            Self::Remote(message, _) | Self::Local(message) | Self::Rejected(message) => {
                message.clone()
            }
            Self::Cancelled => "更新任务已停止".into(),
        }
    }
}

type Observer = Arc<dyn Fn(Attempt) + Send + Sync>;
type Cancelled = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone)]
pub struct Transport {
    client: Client,
    preferences: Preferences,
    state: Arc<State>,
    epoch: u64,
    selected: Option<Route>,
    observer: Observer,
    cancelled: Cancelled,
    test_http: bool,
    #[cfg(feature = "test-fixtures")]
    test_routes: Option<Vec<(String, bool)>>,
    #[cfg(feature = "test-fixtures")]
    token: Option<String>,
}

#[derive(Clone)]
struct Route {
    prefix: Option<String>,
    key: String,
}
impl Route {
    fn endpoint(&self) -> String {
        self.prefix.clone().unwrap_or_else(|| "原始地址".into())
    }
    fn url(&self, original: &Url) -> Result<Url, Error> {
        match &self.prefix {
            Some(prefix) => Url::parse(&format!("{prefix}{original}"))
                .map_err(|e| Error::Rejected(e.to_string())),
            None => Ok(original.clone()),
        }
    }
}

fn github_url(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some(
            "api.github.com"
                | "github.com"
                | "raw.githubusercontent.com"
                | "release-assets.githubusercontent.com"
                | "objects.githubusercontent.com"
        )
    )
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use super::*;
    #[test]
    fn authorization_is_limited_to_direct_github_api_even_after_redirects() {
        let t = Transport::new(Preferences::default(), Arc::new(State::default()))
            .unwrap()
            .test_token("test-only-secret".into());
        for (value, prefix, expected) in [
            ("https://api.github.com/repos/test/repo", None, true),
            ("https://github.com/test/repo/releases", None, false),
            ("https://other.example/api", None, false),
            (
                "https://proxy.example/https://api.github.com/repos/test/repo",
                Some("https://proxy.example/"),
                false,
            ),
            (
                "https://api.github.com/repos/test/repo",
                Some("https://proxy.example/"),
                false,
            ),
        ] {
            let url = Url::parse(value).unwrap();
            let route = Route {
                prefix: prefix.map(str::to_owned),
                key: "test".into(),
            };
            let request = t
                .authorize(t.client.get(url.clone()), &route, &url)
                .build()
                .unwrap();
            assert_eq!(
                request
                    .headers()
                    .contains_key(reqwest::header::AUTHORIZATION),
                expected,
                "{value}"
            );
        }
    }
    #[test]
    fn health_respects_settings_snapshots_and_direct_recovery() {
        let state = Arc::new(State::default());
        let t = Transport::new(Preferences::default(), state.clone()).unwrap();
        let original = Url::parse("https://api.github.com/repos/test/repo").unwrap();
        let routes = t.routes(&original, RequestKind::Api);
        let direct = routes[0].clone();
        let alternate = routes[2].clone();
        t.failure(&direct, RequestKind::Api, 1, &Error::remote("offline"));
        t.success(&alternate, RequestKind::Api);
        assert_eq!(t.routes(&original, RequestKind::Api)[0].key, alternate.key);
        state
            .inner
            .lock()
            .unwrap()
            .cooldown
            .insert((RequestKind::Api, direct.key.clone()), Instant::now());
        assert!(t.routes(&original, RequestKind::Api)[0].prefix.is_none());
        state.clear();
        t.failure(&direct, RequestKind::Api, 1, &Error::remote("stale task"));
        t.success(&alternate, RequestKind::Api);
        assert!(state.inner.lock().unwrap().cooldown.is_empty());
        assert!(state.inner.lock().unwrap().preferred.is_empty());
        assert!(t.routes(&original, RequestKind::Api)[0].prefix.is_none());
        let disabled = Transport::new(
            Preferences {
                fallback_enabled: false,
                ..Default::default()
            },
            state,
        )
        .unwrap();
        assert_eq!(disabled.routes(&original, RequestKind::Api).len(), 1);
    }
}

impl Transport {
    pub fn new(preferences: Preferences, state: Arc<State>) -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .user_agent("SoDeck-updates")
            .connect_timeout(Duration::from_secs(3))
            .read_timeout(READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            preferences: preferences.validated()?,
            epoch: state.epoch.load(Ordering::Acquire),
            state,
            selected: None,
            observer: Arc::new(|_| {}),
            cancelled: Arc::new(|| false),
            test_http: false,
            #[cfg(feature = "test-fixtures")]
            test_routes: None,
            #[cfg(feature = "test-fixtures")]
            token: None,
        })
    }

    pub fn observed(mut self, observer: Observer, cancelled: Cancelled) -> Self {
        self.observer = observer;
        self.cancelled = cancelled;
        self
    }

    pub fn only_endpoint(mut self, prefix: Option<&str>) -> Result<Self, String> {
        let prefix = prefix
            .map(|value| {
                #[cfg(feature = "test-fixtures")]
                if self.test_http {
                    let url = Url::parse(value).map_err(|e| e.to_string())?;
                    if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") {
                        return Ok(format!("{}/", value.trim_end_matches('/')));
                    }
                }
                normalize_prefix(value)
            })
            .transpose()?;
        self.selected = Some(Route {
            key: "diagnostic".into(),
            prefix,
        });
        self.state = Arc::new(State::default());
        self.epoch = 0;
        self.observer = Arc::new(|_| {});
        Ok(self)
    }

    #[cfg(feature = "test-fixtures")]
    pub fn test_server(base: &str, mirrors: Vec<(String, bool)>, state: Arc<State>) -> Self {
        let mut transport = Self::new(Preferences::default(), state).unwrap();
        assert_eq!(Url::parse(base).unwrap().host_str(), Some("127.0.0.1"));
        for (prefix, _) in &mirrors {
            assert_eq!(Url::parse(prefix).unwrap().host_str(), Some("127.0.0.1"));
        }
        transport.client = Client::builder()
            .user_agent("SoDeck-updates")
            .no_proxy()
            .connect_timeout(Duration::from_secs(1))
            .read_timeout(READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        transport.test_http = true;
        transport.test_routes = Some(mirrors);
        transport
    }

    #[cfg(feature = "test-fixtures")]
    pub fn test_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    pub fn validate_url(&self, value: &str) -> Result<Url, String> {
        let url = Url::parse(value).map_err(|_| "更新下载地址无效".to_string())?;
        if (url.scheme() != "https"
            && !(self.test_http && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("更新下载必须使用不含凭据的 HTTPS 地址".into());
        }
        Ok(url)
    }

    fn routes(&self, original: &Url, kind: RequestKind) -> Vec<Route> {
        if let Some(route) = &self.selected {
            return vec![route.clone()];
        }
        let direct = Route {
            prefix: None,
            key: format!("direct:{}", original.host_str().unwrap_or_default()),
        };
        let eligible = github_url(original) || self.test_http;
        if !self.preferences.fallback_enabled || !eligible {
            return vec![direct];
        }
        let mut routes = vec![direct];
        #[cfg(feature = "test-fixtures")]
        if let Some(mirrors) = &self.test_routes {
            for (prefix, api) in mirrors {
                if kind != RequestKind::Api || *api {
                    routes.push(Route {
                        prefix: Some(prefix.clone()),
                        key: prefix.clone(),
                    });
                }
            }
        } else {
            self.add_mirrors(&mut routes, kind);
        }
        #[cfg(not(feature = "test-fixtures"))]
        self.add_mirrors(&mut routes, kind);
        let health = self.state.inner.lock().unwrap();
        if self.epoch != self.state.epoch.load(Ordering::Acquire) {
            return routes;
        }
        let now = Instant::now();
        let available: Vec<_> = routes
            .iter()
            .filter(|route| {
                health
                    .cooldown
                    .get(&(kind, route.key.clone()))
                    .is_none_or(|until| *until <= now)
            })
            .cloned()
            .collect();
        let mut cooling: Vec<_> = routes
            .into_iter()
            .filter(|route| {
                health
                    .cooldown
                    .get(&(kind, route.key.clone()))
                    .is_some_and(|until| *until > now)
            })
            .collect();
        // A manifest failure must not exclude a node's package when every fresh route also fails.
        cooling.sort_by_key(|route| {
            (
                route.prefix.is_none(),
                health.cooldown.get(&(kind, route.key.clone())).copied(),
            )
        });
        let mut available = available;
        if let Some(preferred) = health.preferred.get(&kind) {
            if let Some(position) = available
                .iter()
                .position(|route| &route.key == preferred && route.prefix.is_some())
            {
                let first = usize::from(available[0].prefix.is_none());
                let route = available.remove(position);
                available.insert(first, route);
            }
        }
        available.extend(cooling);
        available
    }

    fn add_mirrors(&self, routes: &mut Vec<Route>, kind: RequestKind) {
        for mirror in self.preferences.custom_mirrors.iter().filter(|m| m.enabled) {
            routes.push(Route {
                prefix: Some(mirror.prefix.clone()),
                key: mirror.prefix.clone(),
            });
        }
        for mirror in BUILTIN_MIRRORS {
            if !self
                .preferences
                .disabled_builtins
                .iter()
                .any(|id| id == mirror.id)
                && (kind != RequestKind::Api || mirror.supports_api)
            {
                routes.push(Route {
                    prefix: Some(mirror.prefix.into()),
                    key: mirror.prefix.into(),
                });
            }
        }
    }

    fn success(&self, route: &Route, kind: RequestKind) {
        let mut health = self.state.inner.lock().unwrap();
        if self.epoch != self.state.epoch.load(Ordering::Acquire) {
            return;
        }
        health.cooldown.remove(&(kind, route.key.clone()));
        health.preferred.insert(kind, route.key.clone());
    }
    fn failure(&self, route: &Route, kind: RequestKind, attempt: usize, error: &Error) {
        if let Error::Remote(_, cooldown) = error {
            let mut health = self.state.inner.lock().unwrap();
            if self.epoch == self.state.epoch.load(Ordering::Acquire) {
                health
                    .cooldown
                    .insert((kind, route.key.clone()), Instant::now() + *cooldown);
            }
        }
        (self.observer)(Attempt {
            endpoint: route.endpoint(),
            attempt,
            error: Some(error.message()),
        });
    }
    fn check_cancelled(&self) -> Result<(), Error> {
        if (self.cancelled)() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "test-fixtures")]
    fn authorize(
        &self,
        request: reqwest::RequestBuilder,
        route: &Route,
        url: &Url,
    ) -> reqwest::RequestBuilder {
        match (&self.token, &route.prefix, url.host_str()) {
            (Some(token), None, Some("api.github.com")) => request.bearer_auth(token),
            _ => request,
        }
    }

    async fn response(
        &self,
        route: &Route,
        original: &Url,
        accept: &str,
        deadline: Instant,
        sample: bool,
    ) -> Result<reqwest::Response, Error> {
        let mut url = route.url(original)?;
        for _ in 0..=5 {
            self.check_cancelled()?;
            self.validate_url(url.as_str()).map_err(Error::Rejected)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::remote("请求达到时限"));
            }
            let request = self
                .client
                .get(url.clone())
                .header(reqwest::header::ACCEPT, accept)
                .timeout(remaining);
            let request = if sample {
                request.header(reqwest::header::RANGE, "bytes=0-65535")
            } else {
                request
            };
            #[cfg(feature = "test-fixtures")]
            let request = self.authorize(request, route, &url);
            let response = tokio::time::timeout(remaining.min(HEADER_TIMEOUT), request.send())
                .await
                .map_err(|_| Error::remote("连接或响应首包超时"))?
                .map_err(|e| Error::remote(format!("无法连接：{e}")))?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| Error::remote("重定向缺少有效地址"))?;
                let target = url
                    .join(location)
                    .map_err(|_| Error::remote("重定向地址无效"))?;
                self.validate_url(target.as_str())
                    .map_err(|message| Error::remote(format!("重定向被拒绝：{message}")))?;
                url = if route.prefix.is_some()
                    && (github_url(&target)
                        || (self.test_http
                            && target.host_str() == original.host_str()
                            && target.port() == original.port()))
                {
                    route.url(&target)?
                } else {
                    target
                };
                continue;
            }
            if !response.status().is_success() {
                let status = response.status().as_u16();
                let retry = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(300)
                    .clamp(1, 900);
                let mut response = response;
                let message = match tokio::time::timeout(
                    Duration::from_millis(500),
                    response.chunk(),
                )
                .await
                {
                    Ok(Ok(Some(chunk))) => {
                        serde_json::from_slice::<serde_json::Value>(&chunk[..chunk.len().min(4096)])
                            .ok()
                            .and_then(|value| {
                                value["message"]
                                    .as_str()
                                    .map(|message| message.chars().take(256).collect::<String>())
                            })
                    }
                    _ => None,
                };
                return Err(Error::Remote(
                    format!(
                        "服务器返回 HTTP {status}{}",
                        message.map(|text| format!("：{text}")).unwrap_or_default()
                    ),
                    Duration::from_secs(retry),
                ));
            }
            return Ok(response);
        }
        Err(Error::remote("地址重定向次数过多"))
    }

    pub fn json<T: serde::de::DeserializeOwned>(
        &self,
        original: &str,
        kind: RequestKind,
        accept: &str,
    ) -> Result<T, String> {
        self.read(original, kind, accept, |bytes| {
            serde_json::from_slice(bytes)
                .map_err(|e| Error::remote(format!("发行信息格式错误：{e}")))
        })
    }

    pub fn read<T>(
        &self,
        original: &str,
        kind: RequestKind,
        accept: &str,
        decode: impl Fn(&[u8]) -> Result<T, Error>,
    ) -> Result<T, String> {
        tauri::async_runtime::block_on(async {
            let original = self.validate_url(original).map_err(Error::Rejected)?;
            let deadline = Instant::now() + METADATA_BUDGET;
            let mut failures = Vec::new();
            for (index, route) in self.routes(&original, kind).into_iter().enumerate() {
                self.check_cancelled()?;
                if Instant::now() >= deadline {
                    failures.push("达到请求时限".into());
                    break;
                }
                (self.observer)(Attempt {
                    endpoint: route.endpoint(),
                    attempt: index + 1,
                    error: None,
                });
                let result = async {
                    let mut response = self
                        .response(
                            &route,
                            &original,
                            accept,
                            deadline.min(Instant::now() + METADATA_TIMEOUT),
                            false,
                        )
                        .await?;
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|e| Error::remote(format!("读取中断：{e}")))?
                    {
                        self.check_cancelled()?;
                        if bytes.len() + chunk.len() > MAX_METADATA {
                            return Err(Error::Rejected("发行信息过大".into()));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    self.check_cancelled()?;
                    decode(&bytes)
                }
                .await;
                match result {
                    Ok(value) => {
                        self.success(&route, kind);
                        return Ok(value);
                    }
                    Err(error @ Error::Remote(_, _)) => {
                        self.failure(&route, kind, index + 1, &error);
                        failures.push(format!("{}：{}", route.endpoint(), error.message()));
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(Error::Rejected(format!(
                "无法读取更新资源，已尝试 {} 个地址：{}",
                failures
                    .iter()
                    .filter(|m| m.as_str() != "达到请求时限")
                    .count(),
                failures.join("；")
            )))
        })
        .map_err(|e| e.message())
    }

    pub fn download(
        &self,
        original: &str,
        path: &Path,
        mut progress: impl FnMut(u64, Option<u64>),
        verify: impl Fn(&Path, &str) -> Result<(), Error>,
    ) -> Result<(), String> {
        tauri::async_runtime::block_on(async {
            let original = self.validate_url(original).map_err(Error::Rejected)?;
            let mut failures = Vec::new();
            for (index, route) in self
                .routes(&original, RequestKind::File)
                .into_iter()
                .enumerate()
            {
                self.check_cancelled()?;
                (self.observer)(Attempt {
                    endpoint: route.endpoint(),
                    attempt: index + 1,
                    error: None,
                });
                progress(0, None);
                let mut owns_file = false;
                let result = async {
                    let mut response = self
                        .response(
                            &route,
                            &original,
                            "application/octet-stream",
                            Instant::now() + DOWNLOAD_TIMEOUT,
                            false,
                        )
                        .await?;
                    let total = response.content_length();
                    if total.is_some_and(|size| size > MAX_PACKAGE) {
                        return Err(Error::Rejected("更新包超过 512 MiB".into()));
                    }
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|e| Error::Local(format!("无法创建下载文件：{e}")))?;
                    owns_file = true;
                    let mut count = 0;
                    let mut hash = Sha256::new();
                    let mut last = Instant::now();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|e| Error::remote(format!("下载中断：{e}")))?
                    {
                        self.check_cancelled()?;
                        count += chunk.len() as u64;
                        if count > MAX_PACKAGE {
                            return Err(Error::Rejected("更新包超过 512 MiB".into()));
                        }
                        file.write_all(&chunk)
                            .map_err(|e| Error::Local(format!("无法写入下载文件：{e}")))?;
                        hash.update(&chunk);
                        if last.elapsed() >= Duration::from_millis(200) {
                            progress(count, total);
                            last = Instant::now();
                        }
                    }
                    file.sync_all()
                        .map_err(|e| Error::Local(format!("无法保存下载文件：{e}")))?;
                    drop(file);
                    if total.is_some_and(|size| size != count) {
                        return Err(Error::remote("下载包不完整"));
                    }
                    self.check_cancelled()?;
                    verify(path, &format!("{:x}", hash.finalize()))?;
                    self.check_cancelled()?;
                    progress(count, Some(count));
                    Ok(())
                }
                .await;
                match result {
                    Ok(()) => {
                        self.success(&route, RequestKind::File);
                        return Ok(());
                    }
                    Err(error) => {
                        if owns_file {
                            std::fs::remove_file(path)
                                .map_err(|e| Error::Local(format!("无法清理未完成下载：{e}")))?;
                        }
                        if matches!(error, Error::Remote(_, _)) {
                            self.failure(&route, RequestKind::File, index + 1, &error);
                            failures.push(format!("{}：{}", route.endpoint(), error.message()));
                        } else {
                            return Err(error);
                        }
                    }
                }
            }
            Err(Error::Rejected(format!(
                "下载失败，已尝试 {} 个地址：{}",
                failures.len(),
                failures.join("；")
            )))
        })
        .map_err(|e| e.message())
    }

    pub fn sample_package(&self, original: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(async {
            let original = self.validate_url(original).map_err(Error::Rejected)?;
            let route = self
                .selected
                .as_ref()
                .ok_or_else(|| Error::Rejected("诊断必须指定单一地址".into()))?;
            let mut response = self
                .response(
                    route,
                    &original,
                    "application/octet-stream",
                    Instant::now() + METADATA_TIMEOUT,
                    true,
                )
                .await?;
            let mut bytes = Vec::new();
            while bytes.len() < 65536 {
                self.check_cancelled()?;
                let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|e| Error::remote(format!("读取首段失败：{e}")))?
                else {
                    break;
                };
                bytes.extend_from_slice(&chunk[..chunk.len().min(65536 - bytes.len())]);
            }
            self.check_cancelled()?;
            if !bytes.starts_with(b"PK\x03\x04") && !bytes.starts_with(&[0x1f, 0x8b]) {
                return Err(Error::remote("返回内容不是 ZIP 或 GZIP 更新包"));
            }
            Ok(())
        })
        .map_err(|e| e.message())
    }
}
