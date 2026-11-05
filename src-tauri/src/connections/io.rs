//! Cancellation is enforced at the physical socket, including during SSH handshake.
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;

pub trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}
pub type Stream = Box<dyn Transport>;

#[derive(Default)]
pub struct Counters {
    pub sent: AtomicU64,
    pub received: AtomicU64,
}
impl Counters {
    pub fn totals(&self) -> (String, String) {
        (
            self.sent.load(Ordering::Relaxed).to_string(),
            self.received.load(Ordering::Relaxed).to_string(),
        )
    }
}
pub struct Socket<S> {
    inner: S,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
    counters: Arc<Counters>,
}
impl<S> Socket<S> {
    pub fn new(inner: S, cancel: CancellationToken, counters: Arc<Counters>) -> Self {
        Self {
            inner,
            cancelled: Box::pin(cancel.cancelled_owned()),
            counters,
        }
    }
    fn check(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        if self.cancelled.as_mut().poll(cx).is_ready() {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Socket<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.check(cx)?;
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        // Only count bytes actually read at the first physical socket.
        self.counters
            .received
            .fetch_add((buf.filled().len() - before) as u64, Ordering::Relaxed);
        result
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Socket<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.check(cx)?;
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &result {
            self.counters.sent.fetch_add(*n as u64, Ordering::Relaxed);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.check(cx)?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn counts_actual_transferred_bytes_and_cancellation_wakes_a_blocked_read() {
        let (client, mut peer) = tokio::io::duplex(64);
        let token = CancellationToken::new();
        let counters = Arc::new(Counters::default());
        let mut client = Socket::new(client, token.clone(), counters.clone());
        client.write_all(b"upload").await.unwrap();
        let mut bytes = [0; 6];
        peer.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"upload");
        peer.write_all(b"download").await.unwrap();
        let mut bytes = [0; 8];
        client.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"download");
        assert_eq!(counters.totals(), ("6".into(), "8".into()));
        let waiting = tokio::spawn(async move { client.read_u8().await });
        tokio::task::yield_now().await;
        token.cancel();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert_eq!(counters.totals(), ("6".into(), "8".into()));
    }
}
