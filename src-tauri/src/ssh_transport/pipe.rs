//! Bounded bridge between the session owner and asynchronous protocol tasks.
use polling::Poller;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const CAPACITY: usize = 32 * 1024;
#[derive(Default)]
struct State {
    to_worker: VecDeque<u8>,
    to_async: VecDeque<u8>,
    async_eof: bool,
    worker_eof: bool,
    aborted: bool,
    finished: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
}
struct Shared {
    state: Mutex<State>,
    wake: Arc<Poller>,
}
pub struct WorkerPipe(Arc<Shared>);
pub struct ProxyStream(Arc<Shared>);
pub fn pair(wake: Arc<Poller>) -> (WorkerPipe, ProxyStream) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State::default()),
        wake,
    });
    (WorkerPipe(shared.clone()), ProxyStream(shared))
}
fn aborted() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "SSH route closed")
}
impl WorkerPipe {
    pub fn shutdown_write(&self) {
        let wake = {
            let mut s = self.0.state.lock().unwrap();
            s.worker_eof = true;
            s.reader.take()
        };
        if let Some(w) = wake {
            w.wake();
        }
    }
    pub fn finish(&self) {
        let mut s = self.0.state.lock().unwrap();
        s.finished = true;
        s.worker_eof = true;
        let (r, w) = (s.reader.take(), s.writer.take());
        drop(s);
        if let Some(w) = r {
            w.wake();
        }
        if let Some(w) = w {
            w.wake();
        }
    }
}
fn terminate(shared: &Shared) {
    let (r, w) = {
        let mut s = shared.state.lock().unwrap();
        s.aborted = true;
        (s.reader.take(), s.writer.take())
    };
    if let Some(w) = r {
        w.wake();
    }
    if let Some(w) = w {
        w.wake();
    }
    let _ = shared.wake.notify();
}
impl Drop for WorkerPipe {
    fn drop(&mut self) {
        let finished = self.0.state.lock().unwrap().finished;
        if !finished {
            terminate(&self.0);
        }
    }
}
impl Drop for ProxyStream {
    fn drop(&mut self) {
        terminate(&self.0);
    }
}
impl Read for WorkerPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (n, wake) = {
            let mut s = self.0.state.lock().unwrap();
            if s.aborted {
                return Err(aborted());
            }
            if s.to_worker.is_empty() {
                return if s.async_eof {
                    Ok(0)
                } else {
                    Err(io::ErrorKind::WouldBlock.into())
                };
            }
            let n = buf.len().min(s.to_worker.len());
            for byte in &mut buf[..n] {
                *byte = s.to_worker.pop_front().unwrap();
            }
            (n, s.writer.take())
        };
        if let Some(w) = wake {
            w.wake();
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn bounded_backpressure_half_close_and_graceful_drain() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (mut worker, mut stream) = pair(Arc::new(Poller::new().unwrap()));
            stream.write_all(&vec![7; CAPACITY]).await.unwrap();
            assert!(tokio::time::timeout(
                std::time::Duration::from_millis(20),
                stream.write_all(&[8])
            )
            .await
            .is_err());
            let mut bytes = vec![0; CAPACITY];
            assert_eq!(worker.read(&mut bytes).unwrap(), CAPACITY);
            assert!(bytes.iter().all(|b| *b == 7));
            stream.write_all(&[8]).await.unwrap();
            assert_eq!(worker.read(&mut bytes).unwrap(), 1);
            stream.shutdown().await.unwrap();
            assert_eq!(worker.read(&mut bytes).unwrap(), 0);
            worker.write_all(b"reply after EOF").unwrap();
            worker.finish();
            drop(worker);
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            assert_eq!(response, b"reply after EOF");
        });
    }
    #[test]
    fn dropping_worker_wakes_async_reader() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (worker, mut stream) = pair(Arc::new(Poller::new().unwrap()));
            let reader = tokio::spawn(async move { stream.read_u8().await });
            tokio::task::yield_now().await;
            drop(worker);
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), reader)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
        });
    }
}
impl Write for WorkerPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (n, wake) = {
            let mut s = self.0.state.lock().unwrap();
            if s.aborted || s.worker_eof {
                return Err(aborted());
            }
            let n = buf.len().min(CAPACITY - s.to_async.len());
            if n == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            s.to_async.extend(&buf[..n]);
            (n, s.reader.take())
        };
        if let Some(w) = wake {
            w.wake();
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl AsyncRead for ProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        {
            let mut s = self.0.state.lock().unwrap();
            if s.aborted {
                return Poll::Ready(Err(aborted()));
            }
            if s.to_async.is_empty() {
                if s.worker_eof {
                    return Poll::Ready(Ok(()));
                }
                s.reader = Some(cx.waker().clone());
                return Poll::Pending;
            }
            let n = buf.remaining().min(s.to_async.len());
            for _ in 0..n {
                buf.put_slice(&[s.to_async.pop_front().unwrap()]);
            }
        }
        let _ = self.0.wake.notify();
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for ProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = {
            let mut s = self.0.state.lock().unwrap();
            if s.aborted || s.finished || s.async_eof {
                return Poll::Ready(Err(aborted()));
            }
            let n = buf.len().min(CAPACITY - s.to_worker.len());
            if n == 0 {
                s.writer = Some(cx.waker().clone());
                return Poll::Pending;
            }
            s.to_worker.extend(&buf[..n]);
            n
        };
        let _ = self.0.wake.notify();
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut s = self.0.state.lock().unwrap();
        if s.aborted {
            return Poll::Ready(Err(aborted()));
        }
        if s.to_worker.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            s.writer = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                self.0.state.lock().unwrap().async_eof = true;
                let _ = self.0.wake.notify();
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
