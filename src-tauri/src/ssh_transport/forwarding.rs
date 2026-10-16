//! One owner drives all channels of an SSH session. No UI, rules, or reconnect policy.
use super::{Socket, SshConnection};
use crate::model::{AppError, AppResult};
use polling::{Event, Events, Poller};
use ssh2::{BlockDirections, Channel, ErrorCode};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Shutdown, TcpListener},
    time::{Duration, Instant},
};

const OPEN_TIMEOUT: Duration = Duration::from_secs(12);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const BUFFER: usize = 32768;

#[derive(Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
}
pub enum Update {
    Opened(u64),
    Failed(u64, AppError),
    Finished(u64),
    Bytes { sent: u64, received: u64 },
}
struct Buffer {
    bytes: Box<[u8; BUFFER]>,
    pos: usize,
    len: usize,
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            bytes: Box::new([0; BUFFER]),
            pos: 0,
            len: 0,
        }
    }
}
impl Buffer {
    fn empty(&self) -> bool {
        self.pos == self.len
    }
}
struct Flow {
    id: u64,
    socket: Option<Socket>,
    target: Target,
    origin: (String, u16),
    channel: Option<Channel>,
    outgoing: Buffer,
    incoming: Buffer,
    local_eof: bool,
    remote_eof: bool,
    sent_eof: bool,
    shutdown: bool,
    closing: Option<Instant>,
    close_sent: bool,
    abandoned: bool,
}
impl Flow {
    fn new(id: u64, socket: Option<Socket>, target: Target, origin: (String, u16)) -> Self {
        Self {
            id,
            socket,
            target,
            origin,
            channel: None,
            outgoing: Buffer::default(),
            incoming: Buffer::default(),
            local_eof: false,
            remote_eof: false,
            sent_eof: false,
            shutdown: false,
            closing: None,
            close_sent: false,
            abandoned: false,
        }
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        if let Some(s) = &self.socket {
            s.shutdown(Shutdown::Both);
        }
    }
}
#[derive(Clone, Copy)]
enum Operation {
    Open,
    Read,
    Write,
    Eof,
    Close,
    WaitClose,
}
struct PendingWrite {
    id: u64,
    operation: Operation,
    since: Instant,
}

pub struct ForwardCore {
    connection: SshConnection,
    queue: VecDeque<Flow>,
    opening: Option<(Flow, Instant)>,
    flows: Vec<Flow>,
    pending: Option<PendingWrite>,
    outbound: bool,
    keepalive: Instant,
    pub updates: Vec<Update>,
}
impl ForwardCore {
    pub fn new(connection: SshConnection) -> Self {
        connection.session().set_blocking(false);
        connection.session().set_keepalive(false, 20);
        Self {
            connection,
            queue: VecDeque::new(),
            opening: None,
            flows: Vec::new(),
            pending: None,
            outbound: false,
            keepalive: Instant::now(),
            updates: Vec::new(),
        }
    }
    pub fn enqueue(
        &mut self,
        id: u64,
        socket: Option<Socket>,
        target: Target,
        origin: (String, u16),
    ) -> AppResult<()> {
        if self.len() >= 64 || self.queue.len() + usize::from(self.opening.is_some()) >= 8 {
            return Err(AppError::new("limit", "此转发的连接或等待队列已满"));
        }
        if let Some(s) = &socket {
            s.nonblocking()?;
        }
        self.queue.push_back(Flow::new(id, socket, target, origin));
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.flows.len() + self.queue.len() + usize::from(self.opening.is_some())
    }
    pub fn cancel(&mut self, id: u64) {
        self.queue.retain(|f| f.id != id);
        if let Some((f, _)) = self.opening.as_mut().filter(|(f, _)| f.id == id) {
            f.abandoned = true;
        }
        for f in self.flows.iter_mut().filter(|f| f.id == id) {
            f.abandoned = true;
        }
    }
    fn blocked(&mut self, id: u64, op: Operation) {
        let direction = self.connection.session().block_directions();
        if matches!(direction, BlockDirections::Outbound | BlockDirections::Both) {
            self.outbound = true;
            if self.pending.is_none() {
                self.pending = Some(PendingWrite {
                    id,
                    operation: op,
                    since: Instant::now(),
                });
            }
        }
    }
    fn open(&mut self) -> AppResult<bool> {
        let Some((f, began)) = &mut self.opening else {
            return Ok(false);
        };
        if began.elapsed() > OPEN_TIMEOUT {
            return Err(AppError::new(
                "network",
                "打开转发通道超时；正在结束此 SSH 路径",
            ));
        }
        let id = f.id;
        match self.connection.session().channel_direct_tcpip(
            &f.target.host,
            f.target.port,
            Some((&f.origin.0, f.origin.1)),
        ) {
            Ok(channel) => {
                let (mut f, _) = self.opening.take().unwrap();
                f.channel = Some(channel);
                if f.socket.is_none() || f.abandoned {
                    f.closing = Some(Instant::now());
                }
                if !f.abandoned {
                    self.updates.push(Update::Opened(id));
                }
                self.flows.push(f);
                Ok(true)
            }
            Err(e) if e.code() == ErrorCode::Session(-37) => {
                self.blocked(id, Operation::Open);
                Ok(false)
            }
            Err(e) => {
                let e: AppError = e.into();
                if matches!(e.kind.as_str(), "network" | "timeout") {
                    return Err(e);
                }
                self.opening.take();
                self.updates
                    .push(Update::Failed(id, AppError::new("target", e.message)));
                Ok(true)
            }
        }
    }
    fn ssh_op(&mut self, index: usize, op: Operation) -> AppResult<bool> {
        let f = &mut self.flows[index];
        let id = f.id;
        let channel = f.channel.as_mut().unwrap();
        let result: Result<usize, io::Error> = match op {
            Operation::Read => channel.read(&mut f.incoming.bytes[..]),
            Operation::Write => channel.write(&f.outgoing.bytes[f.outgoing.pos..f.outgoing.len]),
            Operation::Eof => channel.send_eof().map(|_| 1).map_err(Into::into),
            Operation::Close => channel.close().map(|_| 1).map_err(Into::into),
            Operation::WaitClose => channel.wait_close().map(|_| 1).map_err(Into::into),
            Operation::Open => unreachable!(),
        };
        match result {
            Ok(n) => {
                match op {
                    Operation::Read => {
                        if n == 0 {
                            f.remote_eof = channel.eof();
                        } else {
                            f.incoming.pos = 0;
                            f.incoming.len = n;
                        }
                    }
                    Operation::Write => {
                        f.outgoing.pos += n;
                        if n > 0 {
                            self.updates.push(Update::Bytes {
                                sent: n as u64,
                                received: 0,
                            });
                        }
                    }
                    Operation::Eof => f.sent_eof = true,
                    Operation::Close => f.close_sent = true,
                    Operation::WaitClose => {
                        self.updates.push(Update::Finished(id));
                        self.flows.remove(index);
                    }
                    _ => {}
                }
                Ok(n > 0)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.blocked(id, op);
                Ok(false)
            }
            Err(e) => {
                // ssh2's io::Error conversion erases ErrorCode. Read it now,
                // before another operation can replace the session's last error.
                let code =
                    ssh2::Error::last_session_error(self.connection.session()).map(|e| e.code());
                if matches!(code, Some(ErrorCode::Session(-26 | -27))) {
                    if f.closing.is_none() {
                        self.updates.push(Update::Failed(
                            id,
                            AppError::new("target", format!("目标连接已关闭：{e}")),
                        ));
                        f.closing = Some(Instant::now());
                    }
                    if matches!(op, Operation::Close | Operation::WaitClose) {
                        self.updates.push(Update::Finished(id));
                        self.flows.remove(index);
                    }
                    Ok(true)
                } else {
                    Err(AppError::new("network", format!("SSH 转发中断：{e}")))
                }
            }
        }
    }
    pub fn drive(&mut self) -> AppResult<bool> {
        if let Some(e) = self.connection.transport_error() {
            return Err(e);
        }
        self.connection.socket.check_alive()?;
        self.outbound = false;
        let mut progress = false;
        if let Some(pending) = self.pending.take() {
            if pending.since.elapsed() > WRITE_TIMEOUT {
                return Err(AppError::new("network", "SSH 协议发送超时"));
            }
            let id = pending.id;
            progress |= match pending.operation {
                Operation::Open => self.open()?,
                op => {
                    let index = self
                        .flows
                        .iter()
                        .position(|f| f.id == id)
                        .ok_or_else(|| AppError::new("internal", "转发挂起操作已失去归属"))?;
                    self.ssh_op(index, op)?
                }
            };
            if let Some(next) = &mut self.pending {
                next.since = pending.since;
                return Ok(progress);
            }
        }
        if self.opening.is_none() {
            self.opening = self.queue.pop_front().map(|f| (f, Instant::now()));
        }
        progress |= self.open()?;
        if self.pending.is_some() {
            return Ok(progress);
        }
        // One bounded pass; every client gets an opportunity each turn.
        let ids: Vec<_> = self.flows.iter().map(|f| f.id).collect();
        for id in ids {
            let Some(i) = self.flows.iter().position(|f| f.id == id) else {
                continue;
            };
            {
                let f = &mut self.flows[i];
                if f.abandoned && f.closing.is_none() {
                    f.closing = Some(Instant::now());
                }
                if f.closing.is_none() {
                    if let Some(socket) = &mut f.socket {
                        if f.outgoing.empty() && !f.local_eof {
                            match socket.read(&mut f.outgoing.bytes[..]) {
                                Ok(0) => {
                                    f.local_eof = true;
                                    progress = true;
                                }
                                Ok(n) => {
                                    f.outgoing.pos = 0;
                                    f.outgoing.len = n;
                                    progress = true;
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                                Err(_) => f.closing = Some(Instant::now()),
                            }
                        }
                        if !f.incoming.empty() {
                            match socket.write(&f.incoming.bytes[f.incoming.pos..f.incoming.len]) {
                                Ok(0) => f.closing = Some(Instant::now()),
                                Ok(n) => {
                                    f.incoming.pos += n;
                                    self.updates.push(Update::Bytes {
                                        sent: 0,
                                        received: n as u64,
                                    });
                                    progress = true;
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                                Err(_) => f.closing = Some(Instant::now()),
                            }
                        }
                        if f.remote_eof && f.incoming.empty() && !f.shutdown {
                            socket.shutdown(Shutdown::Write);
                            f.shutdown = true;
                            progress = true;
                        }
                    }
                    if f.sent_eof && f.shutdown {
                        f.closing = Some(Instant::now());
                    }
                }
            }
            if let Some(start) = self.flows[i].closing {
                if start.elapsed() > CLOSE_TIMEOUT {
                    return Err(AppError::new(
                        "network",
                        "转发通道关闭超时；需要重新建立路径",
                    ));
                }
                let op = if self.flows[i].close_sent {
                    Operation::WaitClose
                } else {
                    Operation::Close
                };
                progress |= self.ssh_op(i, op)?;
                if self.pending.is_some() {
                    return Ok(progress);
                }
                continue;
            }
            if !self.flows[i].outgoing.empty() {
                progress |= self.ssh_op(i, Operation::Write)?;
                if self.pending.is_some() {
                    return Ok(progress);
                }
            }
            if self.flows[i].local_eof && self.flows[i].outgoing.empty() && !self.flows[i].sent_eof
            {
                progress |= self.ssh_op(i, Operation::Eof)?;
                if self.pending.is_some() {
                    return Ok(progress);
                }
            }
            if self.flows[i].incoming.empty() && !self.flows[i].remote_eof {
                progress |= self.ssh_op(i, Operation::Read)?;
                if self.pending.is_some() {
                    return Ok(progress);
                }
            }
        }
        if self.keepalive.elapsed() >= Duration::from_secs(20) {
            // Run only when no outbound operation is pending. libssh2's public
            // API swallows EAGAIN and uses a stack-local packet address, so a
            // partial keepalive send cannot safely be resumed. Retire that path
            // immediately rather than reuse a poisoned pending packet.
            self.connection.session().keepalive_send()?;
            if matches!(
                self.connection.session().block_directions(),
                BlockDirections::Outbound | BlockDirections::Both
            ) {
                return Err(AppError::new("network", "SSH 保活发送未完成，重新建立路径"));
            }
            self.keepalive = Instant::now();
        }
        Ok(progress)
    }
    /// Registrations borrow sources for this wait only; every path unregisters
    /// before returning, including errors. Socket ownership cannot move meanwhile.
    pub fn wait(
        &self,
        poller: &Poller,
        listener: Option<&TcpListener>,
        timeout: Duration,
    ) -> AppResult<()> {
        let mut sockets: Vec<&Socket> = Vec::new();
        let mut listener_added = false;
        let result = (|| {
            if let Some(listener) = listener {
                unsafe {
                    poller.add(listener, Event::readable(0))?;
                }
                listener_added = true;
            }
            if self.len() > 0 {
                let socket = self.connection.socket.as_ref();
                unsafe {
                    poller.add(socket, Event::new(1, true, self.outbound))?;
                }
                sockets.push(socket);
            }
            for (i, f) in self.flows.iter().enumerate() {
                if let Some(s) = &f.socket {
                    if f.closing.is_some() {
                        continue;
                    }
                    let read = f.outgoing.empty() && !f.local_eof;
                    let write = !f.incoming.empty();
                    if read || write {
                        unsafe {
                            poller.add(s, Event::new(i + 2, read, write))?;
                        }
                        sockets.push(s);
                    }
                }
            }
            poller.wait(&mut Events::new(), Some(timeout))?;
            Ok::<_, io::Error>(())
        })();
        for s in sockets {
            let _ = poller.delete(s);
        }
        if listener_added {
            let _ = poller.delete(listener.unwrap());
        }
        result.map_err(Into::into)
    }
}
impl Drop for ForwardCore {
    fn drop(&mut self) {
        self.connection.socket.shutdown(Shutdown::Both);
        self.pending = None;
        self.opening = None;
        self.queue.clear();
        self.flows.clear();
    }
}

#[cfg(test)]
#[path = "forwarding_tests.rs"]
pub(crate) mod tests;
