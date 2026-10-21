//! Bounded system resolver workers. Cancelling a request never creates another
//! resolver thread; expired queued requests are discarded before entering DNS.
use super::Cancellation;
use crate::model::{AppError, AppResult};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

struct Request {
    host: String,
    port: u16,
    deadline: Instant,
    cancel: Cancellation,
    reply: mpsc::SyncSender<std::io::Result<Vec<SocketAddr>>>,
}
fn pool() -> &'static mpsc::SyncSender<Request> {
    static POOL: OnceLock<mpsc::SyncSender<Request>> = OnceLock::new();
    POOL.get_or_init(|| {
        let (send, receive) = mpsc::sync_channel::<Request>(16);
        let receive = Arc::new(Mutex::new(receive));
        for _ in 0..4 {
            let receive = receive.clone();
            thread::spawn(move || loop {
                let request = receive.lock().unwrap().recv();
                let Ok(request) = request else {
                    break;
                };
                if request.cancel.is_cancelled() || Instant::now() >= request.deadline {
                    continue;
                }
                let result = (request.host.as_str(), request.port)
                    .to_socket_addrs()
                    .map(|v| v.take(8).collect());
                let _ = request.reply.send(result);
            });
        }
        send
    })
}
pub(super) fn resolve(host: &str, port: u16, cancel: &Cancellation) -> AppResult<Vec<SocketAddr>> {
    cancel.check()?;
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let (reply, receive) = mpsc::sync_channel(1);
    let deadline = Instant::now() + Duration::from_secs(12);
    pool()
        .try_send(Request {
            host: host.into(),
            port,
            deadline,
            cancel: cancel.clone(),
            reply,
        })
        .map_err(|_| AppError::new("network", "地址解析队列已满，请稍后重试"))?;
    loop {
        cancel.check()?;
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result.map_err(|e| AppError::io("network", e)),
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            _ => return Err(AppError::new("network", "服务器地址解析失败或超时")),
        }
    }
}
