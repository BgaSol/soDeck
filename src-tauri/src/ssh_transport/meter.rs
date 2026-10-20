//! The stream wrapper owns registration lifetime; only libssh2 callbacks count bytes.
use super::Socket;
use crate::traffic::ConnectionTraffic;
use libssh2_sys::{libssh2_socket_t, LIBSSH2_SESSION};
use std::{
    collections::HashMap,
    ffi::{c_int, c_void},
    sync::{Arc, OnceLock, RwLock},
};

type SendFn =
    unsafe extern "C" fn(libssh2_socket_t, *const c_void, usize, c_int, *mut *mut c_void) -> isize;
type RecvFn =
    unsafe extern "C" fn(libssh2_socket_t, *mut c_void, usize, c_int, *mut *mut c_void) -> isize;
extern "C" {
    fn sodeck_install_io(
        session: *mut LIBSSH2_SESSION,
        send: SendFn,
        recv: RecvFn,
        old_send: *mut Option<SendFn>,
        old_recv: *mut Option<RecvFn>,
    );
}
struct Hook {
    send: SendFn,
    recv: RecvFn,
    counter: Arc<ConnectionTraffic>,
}
fn hooks() -> &'static RwLock<HashMap<usize, Arc<Hook>>> {
    static HOOKS: OnceLock<RwLock<HashMap<usize, Arc<Hook>>>> = OnceLock::new();
    HOOKS.get_or_init(Default::default)
}
fn hook(abstract_slot: *mut *mut c_void) -> Option<Arc<Hook>> {
    // Key by the stable slot ADDRESS. ssh2 temporarily replaces its VALUE during authentication.
    hooks()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(abstract_slot as usize))
        .cloned()
}
unsafe extern "C" fn send(
    sock: libssh2_socket_t,
    buf: *const c_void,
    len: usize,
    flags: c_int,
    slot: *mut *mut c_void,
) -> isize {
    let Some(h) = hook(slot) else {
        return -(libc::EIO as isize);
    };
    let n = (h.send)(sock, buf, len, flags, slot);
    h.counter.record(true, n);
    n
}
unsafe extern "C" fn recv(
    sock: libssh2_socket_t,
    buf: *mut c_void,
    len: usize,
    flags: c_int,
    slot: *mut *mut c_void,
) -> isize {
    let Some(h) = hook(slot) else {
        return -(libc::EIO as isize);
    };
    let n = (h.recv)(sock, buf, len, flags, slot);
    h.counter.record(false, n);
    n
}
pub struct MeteredSocket {
    socket: Socket,
    key: usize,
    counter: Arc<ConnectionTraffic>,
}
impl MeteredSocket {
    pub fn install(
        session: &ssh2::Session,
        socket: Socket,
        counter: Arc<ConnectionTraffic>,
    ) -> Self {
        let (mut old_send, mut old_recv) = (None, None);
        let mut raw = session.raw();
        let raw = &mut *raw as *mut LIBSSH2_SESSION;
        // The session is not published and no I/O can run until registration is complete.
        let key = unsafe {
            let key = libssh2_sys::libssh2_session_abstract(raw) as usize;
            sodeck_install_io(raw, send, recv, &mut old_send, &mut old_recv);
            key
        };
        let h = Hook {
            send: old_send.expect("libssh2 default send"),
            recv: old_recv.expect("libssh2 default recv"),
            counter: counter.clone(),
        };
        hooks()
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, Arc::new(h));
        Self {
            socket,
            key,
            counter,
        }
    }
}
impl Drop for MeteredSocket {
    fn drop(&mut self) {
        // ssh2 frees the C session before dropping its owned TCP stream, including when
        // Channel/SFTP clones retain SessionInner beyond the SshConnection wrapper.
        let mut registrations = hooks().write().unwrap_or_else(|e| e.into_inner());
        // libssh2 frees the session just before this guard drops. Another thread may
        // already have allocated a new session at the same address in that interval.
        if registrations
            .get(&self.key)
            .is_some_and(|h| Arc::ptr_eq(&h.counter, &self.counter))
        {
            registrations.remove(&self.key);
        }
        self.counter.close();
    }
}
#[cfg(unix)]
impl std::os::fd::AsRawFd for MeteredSocket {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.socket.as_raw_fd()
    }
}
#[cfg(windows)]
impl std::os::windows::io::AsRawSocket for MeteredSocket {
    fn as_raw_socket(&self) -> std::os::windows::io::RawSocket {
        self.socket.as_raw_socket()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
    };
    #[test]
    fn old_guard_cannot_remove_a_reused_session_address() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let _peer = listener.accept().unwrap();
        let mut session = ssh2::Session::new().unwrap();
        let old = Arc::new(ConnectionTraffic::default());
        let socket = MeteredSocket::install(&session, Socket::Tcp(stream), old);
        let key = socket.key;
        session.set_tcp_stream(socket);
        let before = hook(key as *mut *mut c_void).unwrap();
        let replacement = Arc::new(Hook {
            send: before.send,
            recv: before.recv,
            counter: Arc::new(ConnectionTraffic::default()),
        });
        hooks().write().unwrap().insert(key, replacement.clone());
        drop(session);
        assert!(Arc::ptr_eq(
            &hook(key as *mut *mut c_void).unwrap(),
            &replacement
        ));
        hooks().write().unwrap().remove(&key);
    }
    #[test]
    fn callbacks_preserve_abstract_count_io_and_follow_session_clones() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        socket2::SockRef::from(&stream)
            .set_send_buffer_size(4096)
            .unwrap();
        stream.set_nonblocking(true).unwrap();
        #[cfg(unix)]
        let fd = std::os::fd::AsRawFd::as_raw_fd(&stream);
        #[cfg(windows)]
        let fd = std::os::windows::io::AsRawSocket::as_raw_socket(&stream) as libssh2_socket_t;
        let mut session = ssh2::Session::new().unwrap();
        let counter = Arc::new(ConnectionTraffic::default());
        let wrapped = MeteredSocket::install(&session, Socket::Tcp(stream), counter.clone());
        let key = wrapped.key;
        session.set_tcp_stream(wrapped);
        let slot = key as *mut *mut c_void;
        let mut authentication_context = 17u64;
        unsafe {
            *slot = (&mut authentication_context as *mut u64).cast();
        }
        let data = b"abc";
        assert_eq!(
            unsafe { send(fd, data.as_ptr().cast(), data.len(), 0, slot) },
            3
        );
        let mut received = [0; 3];
        peer.read_exact(&mut received).unwrap();
        assert_eq!(&received, data);
        let mut buf = [0; 4];
        assert!(unsafe { recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0, slot) } < 0);
        assert_eq!(counter.counts(), [3, 0]);
        peer.write_all(b"test").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if unsafe { recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0, slot) } > 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
        }
        assert_eq!(&buf, b"test");
        // Kernels may accept a full/partial write or return WouldBlock. Count
        // exactly the bytes accepted, and do not count failed nonblocking sends.
        let chunk = vec![0u8; 64 * 1024];
        let mut sent = 0u64;
        let mut blocked = false;
        for _ in 0..1024 {
            let written = unsafe { send(fd, chunk.as_ptr().cast(), chunk.len(), 0, slot) };
            if written < 0 {
                blocked = true;
                assert_eq!(counter.counts(), [3 + sent, 4]);
                break;
            }
            assert!(written > 0 && written as usize <= chunk.len());
            sent += written as u64;
            assert_eq!(counter.counts(), [3 + sent, 4]);
        }
        assert!(
            blocked,
            "non-reading peer should eventually cause backpressure"
        );
        unsafe {
            assert_eq!(*slot, (&mut authentication_context as *mut u64).cast());
            *slot = std::ptr::null_mut();
        }
        let retained = session.clone();
        drop(session);
        assert!(hook(slot).is_some());
        drop(retained);
        assert!(hook(slot).is_none());
    }
}
