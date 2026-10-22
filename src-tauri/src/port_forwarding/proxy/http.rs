use super::{
    connector::{Connector, Lease},
    headers, Client,
};
use bytes::Bytes;
use http::{header, Request, Response, StatusCode};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Empty, Full};
use hyper::{
    body::{Body, Frame, Incoming, SizeHint},
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    future::Future,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::OwnedMutexGuard,
    task::JoinHandle,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type HttpBody = UnsyncBoxBody<Bytes, Error>;
fn empty() -> HttpBody {
    Empty::new()
        .map_err(|e: Infallible| -> Error { match e {} })
        .boxed_unsync()
}
fn error(status: StatusCode, message: &'static str) -> Response<HttpBody> {
    Response::builder()
        .status(status)
        .header(header::CONNECTION, "close")
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(
            Full::new(Bytes::from_static(message.as_bytes()))
                .map_err(|e: Infallible| -> Error { match e {} })
                .boxed_unsync(),
        )
        .unwrap()
}
fn target_error(e: crate::model::AppError) -> Response<HttpBody> {
    let code = if e.kind == "busy" || e.kind == "limit" {
        StatusCode::SERVICE_UNAVAILABLE
    } else if e.kind == "timeout" {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    error(
        code,
        "无法通过当前 SSH 路径连接目标，请检查目标地址和转发权限。",
    )
}
pub async fn serve<T>(io: T, client: Arc<Client>, connector: Connector) -> io::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .max_headers(100)
        .max_header_size(16 * 1024)
        .max_buf_size(32 * 1024)
        .writev(false)
        .pipeline_flush(false);
    let result = builder
        .serve_connection(
            TokioIo::new(io),
            service_fn(move |req| {
                let client = client.clone();
                let connector = connector.clone();
                async move { Ok::<_, Infallible>(handle(req, client, connector).await) }
            }),
        )
        .with_upgrades()
        .await;
    result.map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
}
async fn handle(
    mut req: Request<Incoming>,
    client: Arc<Client>,
    connector: Connector,
) -> Response<HttpBody> {
    let gate = client.gate.clone().lock_owned().await;
    client.count_received.store(false, Ordering::Relaxed);
    client.negotiated();
    if req.method() == http::Method::CONNECT {
        if req.headers().contains_key(header::CONTENT_LENGTH)
            || req.headers().contains_key(header::TRANSFER_ENCODING)
            || req.uri().scheme().is_some()
            || req.uri().path_and_query().is_some()
        {
            return error(
                StatusCode::BAD_REQUEST,
                "CONNECT 仅接受 host:port，不接受请求体。",
            );
        }
        let target = match headers::target(&req.uri().to_string(), true) {
            Ok(t) => t,
            Err(_) => return error(StatusCode::BAD_REQUEST, "无效的 CONNECT 目标。"),
        };
        let pending = match connector.open(client.clone(), target, "httpConnect").await {
            Ok(p) => p,
            Err(e) => return target_error(e),
        };
        let upgrade = hyper::upgrade::on(&mut req);
        let route = client.route.clone();
        route.spawn(async move {
            let _gate = gate;
            if let Ok(upgraded) = upgrade.await {
                if let Ok(mut tunnel) = pending.activate().await {
                    client.count_received.store(true, Ordering::Relaxed);
                    let mut io = CountedIo::new(TokioIo::new(upgraded), client.clone());
                    let _ = tokio::io::copy_bidirectional(&mut io, &mut tunnel).await;
                }
            }
        });
        return Response::new(empty());
    }
    let websocket = headers::websocket(req.headers());
    if req.headers().contains_key(header::UPGRADE) && !websocket {
        return error(StatusCode::BAD_REQUEST, "此代理只支持 WebSocket 升级。");
    }
    let (target, path, host) = match headers::origin(req.uri(), websocket) {
        Ok(t) => t,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "HTTP 代理请求需要绝对 http:// 地址；HTTPS 请使用 CONNECT。",
            )
        }
    };
    if req
        .headers()
        .get_all(header::EXPECT)
        .iter()
        .any(|v| !v.as_bytes().eq_ignore_ascii_case(b"100-continue"))
    {
        return error(StatusCode::EXPECTATION_FAILED, "不支持此 Expect 条件。");
    }
    let pending = match connector.open(client.clone(), target, "http").await {
        Ok(p) => p,
        Err(e) => return target_error(e),
    };
    let tunnel = match pending.activate().await {
        Ok(t) => t,
        Err(e) => return target_error(e),
    };
    let lease = tunnel.lease.clone();
    let downstream_upgrade = websocket.then(|| hyper::upgrade::on(&mut req));
    *req.uri_mut() = path;
    let content_length = req.headers().get(header::CONTENT_LENGTH).cloned();
    headers::filter(req.headers_mut(), websocket);
    req.headers_mut().remove(header::EXPECT);
    req.headers_mut().insert(
        header::HOST,
        match host.parse() {
            Ok(h) => h,
            Err(_) => return error(StatusCode::BAD_REQUEST, "无效的 Host。"),
        },
    );
    if let Some(length) = content_length {
        req.headers_mut().insert(header::CONTENT_LENGTH, length);
    }
    if !websocket {
        req.headers_mut()
            .insert(header::CONNECTION, http::HeaderValue::from_static("close"));
    }
    let request_done = Arc::new(AtomicBool::new(req.body().is_end_stream()));
    let mut upstream = hyper::client::conn::http1::Builder::new();
    upstream
        .max_headers(100)
        .max_header_size(16 * 1024)
        .max_buf_size(32 * 1024)
        .writev(false);
    let (mut sender, connection) = match upstream.handshake(TokioIo::new(tunnel)).await {
        Ok(c) => c,
        Err(_) => return error(StatusCode::BAD_GATEWAY, "目标 HTTP 连接无法建立。"),
    };
    let driver = client.route.spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    let mut driver = Driver(Some(driver));
    let done = request_done.clone();
    let request = req.map(move |body| FilteredBody { body, done }.boxed_unsync());
    let mut response = match sender.send_request(request).await {
        Ok(r) => r,
        Err(_) => return error(StatusCode::BAD_GATEWAY, "目标未返回有效的 HTTP 响应。"),
    };
    drop(sender);
    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if !websocket || !headers::websocket(response.headers()) {
            return error(StatusCode::BAD_GATEWAY, "目标返回了无效的 WebSocket 升级。");
        }
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        headers::filter(response.headers_mut(), true);
        let (parts, _) = response.into_parts();
        let route = client.route.clone();
        route.spawn(async move {
            let _gate = gate;
            let _driver = driver;
            if let (Ok(local), Ok(remote)) =
                tokio::join!(downstream_upgrade.unwrap(), upstream_upgrade)
            {
                client.count_received.store(true, Ordering::Relaxed);
                let _ = tokio::io::copy_bidirectional(
                    &mut CountedIo::new(TokioIo::new(local), client.clone()),
                    &mut TokioIo::new(remote),
                )
                .await;
            }
        });
        return Response::from_parts(parts, empty());
    }
    headers::filter(response.headers_mut(), false);
    if !request_done.load(Ordering::Relaxed) {
        response
            .headers_mut()
            .insert(header::CONNECTION, http::HeaderValue::from_static("close"));
    }
    client.count_received.store(true, Ordering::Relaxed);
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        ResponseBody {
            body,
            _gate: gate,
            driver: Driver(driver.0.take()),
            lease,
            finish: None,
        }
        .boxed_unsync(),
    )
}

struct CountedIo<T> {
    io: T,
    client: Arc<Client>,
}
impl<T> CountedIo<T> {
    fn new(io: T, client: Arc<Client>) -> Self {
        Self { io, client }
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for CountedIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for CountedIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.io).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            if n > 0 && !self.client.probe && self.client.count_received.load(Ordering::Relaxed) {
                self.client
                    .route
                    .received
                    .fetch_add(n as u64, Ordering::Relaxed);
            }
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

struct Driver(Option<JoinHandle<()>>);
impl Drop for Driver {
    fn drop(&mut self) {
        if let Some(t) = self.0.take() {
            t.abort();
        }
    }
}
struct FilteredBody {
    body: Incoming,
    done: Arc<AtomicBool>,
}
impl Body for FilteredBody {
    type Data = Bytes;
    type Error = Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        match Pin::new(&mut self.body).poll_frame(cx) {
            Poll::Ready(Some(Ok(mut frame))) => {
                if let Some(t) = frame.trailers_mut() {
                    headers::trailers(t);
                }
                if self.body.is_end_stream() {
                    self.done.store(true, Ordering::Relaxed);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(Box::new(e)))),
            Poll::Ready(None) => {
                self.done.store(true, Ordering::Relaxed);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}
struct ResponseBody {
    body: Incoming,
    _gate: OwnedMutexGuard<()>,
    driver: Driver,
    lease: Arc<Lease>,
    finish: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}
impl Body for ResponseBody {
    type Data = Bytes;
    type Error = Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        if self.finish.is_none() {
            match Pin::new(&mut self.body).poll_frame(cx) {
                Poll::Ready(None) => {
                    self.driver.0.take().map(|t| t.abort());
                    self.lease.cancel();
                    let lease = self.lease.clone();
                    self.finish = Some(Box::pin(async move {
                        lease.finished().await;
                    }));
                }
                Poll::Ready(Some(Ok(mut frame))) => {
                    if let Some(t) = frame.trailers_mut() {
                        headers::trailers(t);
                    }
                    return Poll::Ready(Some(Ok(frame)));
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(Box::new(e)))),
                Poll::Pending => return Poll::Pending,
            }
        }
        self.finish
            .as_mut()
            .unwrap()
            .as_mut()
            .poll(cx)
            .map(|_| None)
    }
    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}
impl Drop for ResponseBody {
    fn drop(&mut self) {
        self.lease.cancel();
    }
}
