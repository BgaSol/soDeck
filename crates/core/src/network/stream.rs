use crate::{CoreError, Result};
use sodeck_communication::binary::{Frame, Kind, MAX_OFFSET, WINDOW};
use std::io::{self, Read, Write};

/// Fixed storage permits bulk copies and stable libssh2 retry pointers.
struct Buffer {data:Box<[u8]>,start:usize,len:usize}
impl Buffer {
    fn new()->Self {Self{data:vec![0;WINDOW].into_boxed_slice(),start:0,len:0}}
    fn len(&self)->usize {self.len}
    fn is_empty(&self)->bool {self.len==0}
    fn clear(&mut self){self.len=0;self.start=0;}
    fn as_slices(&self)->(&[u8],&[u8]) {let first=self.len.min(WINDOW-self.start);(&self.data[self.start..self.start+first],&self.data[..self.len-first])}
    fn append(&mut self,data:&[u8]) {assert!(data.len()<=WINDOW-self.len);let end=(self.start+self.len)%WINDOW;let first=data.len().min(WINDOW-end);self.data[end..end+first].copy_from_slice(&data[..first]);self.data[..data.len()-first].copy_from_slice(&data[first..]);self.len+=data.len();}
    fn consume(&mut self,n:usize){assert!(n<=self.len);self.start=(self.start+n)%WINDOW;self.len-=n;}
    fn range(&self,offset:usize,count:usize)->Vec<u8>{assert!(offset+count<=self.len);let start=(self.start+offset)%WINDOW;let first=count.min(WINDOW-start);let mut result=vec![0;count];result[..first].copy_from_slice(&self.data[start..start+first]);result[first..].copy_from_slice(&self.data[..count-first]);result}
}

#[derive(Clone, Debug)]
pub(crate) struct Attachment {
    pub slot: u32,
    pub epoch: u32,
    delivered: u64,
    read_credit: u64,
    credit_sent: u64,
    ack_sent: u64,
    fin_sent: bool,
    reset_sent: bool,
    frozen: Option<(u64, u64)>,
}

/// The retained read window owns every byte until consume or native splice.
/// An attachment is a cursor, never an independent reader of the socket.
pub(crate) struct Stream {
    read: Buffer,
    write: Buffer,
    pending_write: Option<usize>,
    io_thread:Option<std::thread::Thread>,
    relay_thread:Option<std::thread::Thread>,
    pub consumed: u64,
    pub received: u64,
    pub accepted: u64,
    pub written: u64,
    pub read_eof: bool,
    pub write_fin: Option<u64>,
    pub write_eof: bool,
    pub closed: bool,
    pub error: Option<CoreError>,
    pub attachment: Option<Attachment>,
    pub spliced: bool,
}
impl Default for Stream {
    fn default() -> Self {
        Self { read: Buffer::new(), write: Buffer::new(), pending_write:None,io_thread:None,relay_thread:None, consumed: 0, received: 0,
            accepted: 0, written: 0, read_eof: false, write_fin: None, write_eof: false,
            closed: false, error: None, attachment: None, spliced: false }
    }
}
fn protocol(message: &str) -> CoreError { CoreError::new("PROTOCOL_ERROR", message) }
impl Stream {
    pub fn attach(&mut self, slot: u32, epoch: u32) -> Result<serde_json::Value> {
        if self.closed || self.spliced || self.attachment.is_some() { return Err(CoreError::new("STALE_HANDLE", "字节流已关闭或已有读取者")); }
        let credit = (self.written + WINDOW as u64).min(MAX_OFFSET);
        self.attachment = Some(Attachment { slot, epoch, delivered: self.consumed,
            read_credit: (self.consumed + WINDOW as u64).min(MAX_OFFSET), credit_sent: credit,
            ack_sent: self.written, fin_sent: false, reset_sent: false, frozen: None });
        Ok(serde_json::json!({"slot":slot,"epoch":epoch,"window":WINDOW,"readOffset":self.consumed,"writeOffset":self.accepted,"writeCredit":credit}))
    }
    pub fn input(&mut self, frame: &Frame) -> Result<()> {
        frame.validate().map_err(|_| protocol("二进制帧无效"))?;
        let a = self.attachment.as_ref().ok_or_else(|| CoreError::new("STALE_HANDLE", "字节会话已经结束"))?;
        if a.slot != frame.slot || a.epoch != frame.epoch { return Err(CoreError::new("STALE_HANDLE", "字节会话代次已失效")); }
        if frame.kind == Kind::Reset { self.close(); return Ok(()); }
        if self.closed { return Err(CoreError::new("STALE_HANDLE", "字节流已关闭")); }
        match frame.kind {
            Kind::Data => {
                let end = frame.offset + frame.payload.len() as u64;
                if frame.offset != self.accepted || end > a.credit_sent
                    || self.write_fin.is_some_and(|n| end > n)
                    || a.frozen.is_some_and(|(_, n)| end > n)
                    || self.write.len() + frame.payload.len() > WINDOW
                { return Err(protocol("写入偏移或额度无效")); }
                self.write.append(&frame.payload); self.accepted = end;
            }
            Kind::Ack => {
                if frame.offset > a.delivered || a.frozen.is_some_and(|(n, _)| frame.offset > n) { return Err(protocol("不能消费未投递的字节")); }
                // Replayed cumulative acknowledgements cannot replenish credit twice.
                if frame.offset > self.consumed {
                    self.read.consume((frame.offset - self.consumed) as usize);
                    self.consumed = frame.offset;
                }
            }
            Kind::Credit => {
                if frame.offset > (self.consumed + WINDOW as u64).min(MAX_OFFSET) { return Err(protocol("读取额度超出消费窗口")); }
                let a = self.attachment.as_mut().unwrap();
                a.read_credit = a.read_credit.max(frame.offset);
            }
            Kind::Fin => {
                if frame.offset < self.accepted || frame.offset > a.credit_sent
                    || self.write_fin.is_some_and(|n| n != frame.offset)
                { return Err(protocol("写入 EOF 偏移无效")); }
                // Control frames may overtake queued DATA. EOF waits for its exact offset.
                self.write_fin = Some(frame.offset);
            }
            Kind::Reset => unreachable!(),
        }
        if let Some(thread)=&self.io_thread{thread.unpark();}Ok(())
    }
    /// Caller accepts a frame into a bounded transport queue before its cursor advances.
    pub fn output_controls(&mut self, mut deliver: impl FnMut(Frame) -> bool) {
        let Some(a) = &mut self.attachment else { return };
        let control = |kind, offset| Frame::control(kind, a.slot, a.epoch, offset);
        if self.closed {
            if !a.reset_sent && deliver(control(Kind::Reset, 0)) { a.reset_sent = true; }
            return;
        }
        if self.accepted > a.ack_sent {if !deliver(control(Kind::Ack, self.accepted)){return;} a.ack_sent = self.accepted; }
        let credit = (self.written + WINDOW as u64).min(MAX_OFFSET);
        if credit > a.credit_sent && deliver(control(Kind::Credit, credit)) { a.credit_sent = credit; }
    }
    pub fn output(&mut self, mut deliver: impl FnMut(Frame) -> bool) {
        self.output_controls(&mut deliver);
        if self.closed{return;}
        let Some(a)=&mut self.attachment else{return};
        if a.frozen.is_some() { return; }
        let count = (self.received.min(a.read_credit) - a.delivered).min(WINDOW as u64) as usize;
        if count > 0 {
            let start = (a.delivered - self.consumed) as usize;
            let frame = Frame { kind: Kind::Data, slot: a.slot, epoch: a.epoch, offset: a.delivered,
                payload: self.read.range(start,count) };
            if deliver(frame) { a.delivered += count as u64; }
        }
        if self.read_eof && a.delivered == self.received && !a.fin_sent
            && deliver(Frame::control(Kind::Fin, a.slot, a.epoch, self.received)) { a.fin_sent = true; }
    }
    pub fn freeze(&mut self, epoch: u32, consumed: u64, written: u64) -> Result<()> {
        if self.closed || self.spliced { return Err(CoreError::new("STALE_HANDLE", "流不能移交")); }
        if let Some(a) = &mut self.attachment {
            if epoch != a.epoch || consumed < self.consumed || consumed > a.delivered
                || written < self.accepted || written > a.credit_sent
                || a.frozen.is_some_and(|v| v != (consumed, written)) { return Err(protocol("流移交游标无效")); }
            // The splice request itself acknowledges consumption; late ACKs are idempotent.
            self.read.consume((consumed - self.consumed) as usize);
            self.consumed = consumed; a.frozen = Some((consumed, written));
        } else if epoch != 0 || consumed != self.consumed || written != self.accepted {
            return Err(protocol("无会话流的移交游标无效"));
        }
        Ok(())
    }
    pub fn ready_to_splice(&self) -> bool {
        !self.closed && self.write.is_empty() && self.attachment.as_ref().is_none_or(|a|
            a.frozen.is_some_and(|(_, end)| self.accepted == end && self.written == end))
    }
    pub fn transfer(&mut self) { self.attachment = None; self.spliced = true; }
    pub fn half_close(&mut self) { self.write_fin.get_or_insert(self.accepted); }
    pub fn close(&mut self) { self.closed = true; self.read.clear(); self.write.clear(); }
    pub fn fail(&mut self, error: CoreError) { self.error = Some(error); self.close(); }
    pub fn buffered(&self) -> usize { self.read.len() + self.write.len() }
    pub fn reset_delivered(&self) -> bool { self.attachment.as_ref().is_none_or(|a| a.reset_sent) }
    pub fn wants_eof(&self) -> bool { !self.closed && !self.write_eof && self.write_fin == Some(self.written) }
    pub fn finished(&self) -> bool { self.closed || (self.read_eof && self.read.is_empty() && self.write_eof) }
    pub fn read_from(&mut self, reader: &mut impl Read) {
      self.io_thread=Some(std::thread::current());
      // Drain short SSH packets within the same fixed window. Sleeping after
      // every packet otherwise caps a hot flow at one packet per actor tick.
      for _ in 0..8 {
        if self.closed || self.read_eof || self.read.len() == WINDOW { return; }
        let mut data = [0; WINDOW];
        let count = (WINDOW - self.read.len()).min((MAX_OFFSET - self.received) as usize);
        if count == 0 { self.fail(protocol("字节序号已达到上限")); return; }
        match reader.read(&mut data[..count]) {
            Ok(0) => self.read_eof = true,
            Ok(n) => { self.read.append(&data[..n]); self.received += n as u64;if let Some(thread)=&self.relay_thread{thread.unpark();} }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => return,
            Err(_) => self.fail(CoreError::new("TRANSPORT_LOST", "字节通道读取失败")),
        }
      }
    }
    pub fn write_to(&mut self, writer: &mut impl Write) {
      self.io_thread=Some(std::thread::current());
      for _ in 0..8 {
        if self.closed || self.write.is_empty() { return; }
        let (first, _) = self.write.as_slices();
        // libssh2 requires identical bytes, length and address when retrying a
        // blocked write. Appending newly spliced bytes must not alter that call.
        let count=self.pending_write.unwrap_or(first.len());
        match writer.write(&first[..count]) {
            Ok(0) => self.fail(CoreError::new("TRANSPORT_LOST", "字节通道停止接收")),
            Ok(n) => { self.pending_write=None;self.write.consume(n); self.written += n as u64;if let Some(thread)=&self.relay_thread{thread.unpark();} }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {self.pending_write=Some(count);return;},
            Err(_) => self.fail(CoreError::new("TRANSPORT_LOST", "字节通道写入失败，结果可能不完整")),
        }
      }
    }
    pub fn splice_into(&mut self, target: &mut Self) {
        self.relay_thread=Some(std::thread::current());target.relay_thread=Some(std::thread::current());
        if self.closed || target.closed { self.close(); target.close(); return; }
        let n = self.read.len().min(WINDOW - target.write.len());
        if n as u64 > MAX_OFFSET - target.accepted { self.fail(protocol("字节序号已达到上限")); target.close(); return; }
        let (first,second)=self.read.as_slices();let count=first.len().min(n);target.write.append(&first[..count]);target.write.append(&second[..n-count]);self.read.consume(n);
        self.consumed += n as u64; target.accepted += n as u64;
        if n>0{if let Some(thread)=&self.io_thread{thread.unpark();}if let Some(thread)=&target.io_thread{thread.unpark();}}
        if self.read_eof && self.read.is_empty() { target.half_close(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_write_keeps_its_address_and_length_when_more_bytes_arrive() {
        struct Writer{first:Option<(usize,usize)>,blocked:bool,bytes:Vec<u8>}
        impl Write for Writer{fn flush(&mut self)->io::Result<()>{Ok(())}fn write(&mut self,data:&[u8])->io::Result<usize>{if !self.blocked{self.first=Some((data.as_ptr() as usize,data.len()));self.blocked=true;return Err(io::ErrorKind::WouldBlock.into());}if let Some(first)=self.first.take(){assert_eq!(first,(data.as_ptr() as usize,data.len()));}self.bytes.extend_from_slice(data);Ok(data.len())}}
        let mut s=Stream::default();s.attach(1,1).unwrap();s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:0,payload:b"first".to_vec()}).unwrap();
        let mut writer=Writer{first:None,blocked:false,bytes:Vec::new()};s.write_to(&mut writer);s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:5,payload:vec![3;WINDOW-5]}).unwrap();s.write_to(&mut writer);assert_eq!(writer.bytes.len(),WINDOW);assert_eq!(&writer.bytes[..5],b"first");
    }
    #[test]
    fn failed_ack_delivery_does_not_grant_unsent_write_credit() {
        let mut s=Stream::default();s.attach(1,1).unwrap();
        s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:0,payload:vec![1;WINDOW]}).unwrap();s.write_to(&mut Vec::new());
        let mut controls=Vec::new();s.output_controls(|f|{controls.push(f.kind);false});assert_eq!(controls,vec![Kind::Ack]);
        assert!(s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:WINDOW as u64,payload:vec![1]}).is_err());
        s.output_controls(|_|true);s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:WINDOW as u64,payload:vec![1]}).unwrap();
    }
    #[test]
    fn splice_preserves_handshake_tail_and_half_close() {
        let mut a = Stream::default(); let mut b = Stream::default();
        a.read_from(&mut &b"helloBODY"[..]); a.attach(1, 1).unwrap();
        a.output(|_| true);
        a.input(&Frame::control(Kind::Ack, 1, 1, 5)).unwrap();
        a.freeze(1, 5, 0).unwrap(); b.freeze(0, 0, 0).unwrap();
        assert!(a.ready_to_splice() && b.ready_to_splice());
        a.transfer(); b.transfer(); a.read_eof = true; a.splice_into(&mut b);
        let mut output = Vec::new(); b.write_to(&mut output);
        assert_eq!(output, b"BODY"); assert!(b.wants_eof()); assert!(!b.closed);
        b.read_from(&mut &b"reply"[..]); b.splice_into(&mut a);
        output.clear(); a.write_to(&mut output); assert_eq!(output, b"reply");
    }
    #[test]
    fn delivery_does_not_consume_or_inflate_window() {
        let mut s = Stream::default(); s.attach(1, 2).unwrap();
        s.read_from(&mut vec![3; WINDOW].as_slice()); s.output(|_| true);
        assert_eq!(s.buffered(), WINDOW);
        assert!(s.input(&Frame::control(Kind::Credit, 1, 2, 2 * WINDOW as u64)).is_err());
        s.input(&Frame::control(Kind::Ack, 1, 2, 100)).unwrap();
        s.input(&Frame::control(Kind::Ack, 1, 2, 100)).unwrap();
        assert_eq!(s.buffered(), WINDOW - 100);
        assert!(s.input(&Frame::control(Kind::Ack, 1, 1, 200)).is_err());
        assert_eq!(s.consumed, 100);
    }
    #[test]
    fn fin_can_overtake_data_but_cannot_truncate_it() {
        let mut s = Stream::default(); s.attach(1, 1).unwrap();
        s.input(&Frame::control(Kind::Fin, 1, 1, 3)).unwrap(); assert!(!s.wants_eof());
        s.input(&Frame { kind: Kind::Data, slot: 1, epoch: 1, offset: 0, payload: b"abc".to_vec() }).unwrap();
        s.write_to(&mut Vec::new()); assert!(s.wants_eof());
        assert!(s.input(&Frame { kind: Kind::Data, slot: 1, epoch: 1, offset: 3, payload: vec![4] }).is_err());
    }
    #[test]
    fn splice_waits_for_pipe_writes_and_native_drain() {
        let mut s = Stream::default(); s.attach(1, 1).unwrap(); s.freeze(1, 0, 3).unwrap();
        assert!(!s.ready_to_splice());
        s.input(&Frame { kind: Kind::Data, slot: 1, epoch: 1, offset: 0, payload: b"200".to_vec() }).unwrap();
        assert!(!s.ready_to_splice()); s.write_to(&mut Vec::new()); assert!(s.ready_to_splice());
    }
}
