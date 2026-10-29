use crate::{CoreError, Result};
use serde_json::Value;
use std::{collections::VecDeque, io::{Read, Write}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

pub const HELLO: u8 = 1;
pub const CONFIG: u8 = 2;
pub const READY: u8 = 3;
pub const BEGIN: u8 = 4;
pub const DATA: u8 = 5;
pub const ACK: u8 = 6;
pub const END: u8 = 7;
pub const RESULT: u8 = 8;
pub const CANCEL: u8 = 9;
pub const MAX_PAYLOAD: usize = 65536;

pub struct Frame { pub kind: u8, pub phase: u8, pub payload: Vec<u8> }
impl Frame {
    pub fn json(&self) -> Result<Value> {
        serde_json::from_slice(&self.payload).map_err(|_| protocol_error())
    }
}
pub struct Wire<'a, T> {
    stream: &'a mut T,
    canceled: &'a AtomicBool,
    deadline: Instant,
    tx: u32,
    rx: u32,
    control: usize,
    incoming: VecDeque<u8>,
}

pub fn protocol_error() -> CoreError { CoreError::new("PROTOCOL_ERROR", "测速协议或数据顺序无效") }

impl<'a, T: Read + Write> Wire<'a, T> {
    pub fn new(stream: &'a mut T, canceled: &'a AtomicBool) -> Self {
        Self { stream, canceled, deadline: Instant::now() + Duration::from_secs(60), tx: 0, rx: 0, control: 0, incoming:VecDeque::new() }
    }
    fn check(&self) -> Result<()> {
        if self.canceled.load(Ordering::Relaxed) { return Err(CoreError::new("CANCELED", "测速已取消")); }
        if Instant::now() >= self.deadline { return Err(CoreError::new("TIMEOUT", "测速达到时间上限")); }
        Ok(())
    }
    fn control(&mut self, bytes: usize) -> Result<()> {
        self.control = self.control.checked_add(bytes).ok_or_else(protocol_error)?;
        if self.control > 65536 { return Err(CoreError::new("CAPACITY", "测速控制数据超过上限")); }
        Ok(())
    }
    fn write(&mut self, mut bytes: &[u8], on_bytes: &mut impl FnMut(usize)) -> Result<()> {
        while !bytes.is_empty() {
            self.check()?;
            // SSH windows and remote progress receipts must advance while the
            // upload is writing, not only after PHASE_END. Buffer controls with
            // a hard ceiling; the phase parser still validates every frame.
            let mut inbound=[0u8;4096];
            match self.stream.read(&mut inbound) {
                Ok(n) => {
                    if self.incoming.len()+n>65536 {return Err(protocol_error());}
                    self.incoming.extend(&inbound[..n]);
                },
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{},
                Err(_)=>return Err(CoreError::new("RESULT_PARTIAL","测速连接接收中断")),
            }
            match self.stream.write(bytes) {
                Ok(0) => return Err(CoreError::new("RESULT_PARTIAL", "测速连接在回执前关闭")),
                Ok(n) => { on_bytes(n); bytes = &bytes[n..]; }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => return Err(CoreError::new("RESULT_PARTIAL", "测速数据发送未完成")),
            }
        }
        Ok(())
    }
    fn read(&mut self, mut bytes: &mut [u8], on_bytes: &mut impl FnMut(usize)) -> Result<()> {
        while !bytes.is_empty() {
            self.check()?;
            if !self.incoming.is_empty() {
                let count=bytes.len().min(self.incoming.len());
                for target in &mut bytes[..count] {*target=self.incoming.pop_front().unwrap();}
                on_bytes(count);bytes=&mut bytes[count..];continue;
            }
            match self.stream.read(bytes) {
                Ok(0) => return Err(CoreError::new("RESULT_PARTIAL", "测速连接缺少完整回执")),
                Ok(n) => { on_bytes(n); bytes = &mut bytes[n..]; }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => return Err(CoreError::new("RESULT_PARTIAL", "测速数据接收未完成")),
            }
        }
        Ok(())
    }
    pub fn bootstrap(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 16384 { return Err(protocol_error()); }
        self.control(bytes.len())?;
        self.write(bytes, &mut |_| {})
    }
    pub fn send_json(&mut self, kind: u8, phase: u8, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(|_| protocol_error())?;
        self.send(kind, phase, &bytes, &mut |_| {})
    }
    pub fn send(&mut self, kind: u8, phase: u8, bytes: &[u8], on_bytes: &mut impl FnMut(usize)) -> Result<()> {
        if !(HELLO..=CANCEL).contains(&kind) || bytes.len() > MAX_PAYLOAD || (kind != DATA && bytes.len() > 4096) {
            return Err(protocol_error());
        }
        if kind != DATA { self.control(bytes.len())?; }
        let mut header = [0; 16];
        header[..4].copy_from_slice(b"SDS1");
        header[4] = 1; header[5] = kind; header[6] = phase;
        header[8..12].copy_from_slice(&self.tx.to_be_bytes());
        header[12..].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        self.tx = self.tx.checked_add(1).ok_or_else(protocol_error)?;
        self.write(&header, &mut |_| {})?;
        self.write(bytes, on_bytes)
    }
    pub fn receive(&mut self, on_data: &mut impl FnMut(usize)) -> Result<Frame> {
        let mut header = [0; 16];
        self.read(&mut header, &mut |_| {})?;
        let kind = header[5];
        let length = u32::from_be_bytes(header[12..16].try_into().unwrap()) as usize;
        if &header[..4] != b"SDS1" || header[4] != 1 || header[7] != 0 ||
            u32::from_be_bytes(header[8..12].try_into().unwrap()) != self.rx ||
            !(HELLO..=CANCEL).contains(&kind) || length > MAX_PAYLOAD || (kind != DATA && length > 4096) {
            return Err(protocol_error());
        }
        self.rx = self.rx.checked_add(1).ok_or_else(protocol_error)?;
        if kind != DATA { self.control(length)?; }
        let mut payload = vec![0; length];
        if kind == DATA { self.read(&mut payload, on_data)?; }
        else { self.read(&mut payload, &mut |_| {})?; }
        Ok(Frame { kind, phase: header[6], payload })
    }
}
