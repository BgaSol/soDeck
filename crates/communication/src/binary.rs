//! Private worker byte transport. No business protocol or caller identity lives here.
use std::io;

pub const VERSION: u8 = 1;
pub const HEADER: usize = 24;
pub const WINDOW: usize = 64 * 1024;
pub const MAX_OFFSET: u64 = 9_007_199_254_740_991;
pub const QUEUE_BYTES: usize = 4 * 1024 * 1024;
pub const CONTROL_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind { Data = 1, Ack = 2, Credit = 3, Fin = 4, Reset = 5 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: Kind,
    pub slot: u32,
    pub epoch: u32,
    pub offset: u64,
    pub payload: Vec<u8>,
}

fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "Invalid binary stream frame") }

impl Frame {
    pub fn control(kind: Kind, slot: u32, epoch: u32, offset: u64) -> Self {
        Self { kind, slot, epoch, offset, payload: Vec::new() }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.slot == 0 || self.epoch == 0 || self.offset > MAX_OFFSET
            || self.payload.len() > WINDOW
            || self.offset.checked_add(self.payload.len() as u64).is_none_or(|n| n > MAX_OFFSET)
            || (self.kind == Kind::Data && self.payload.is_empty())
            || (self.kind != Kind::Data && !self.payload.is_empty())
            || (self.kind == Kind::Reset && self.offset != 0)
        { return Err(invalid()); }
        Ok(())
    }
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(HEADER + self.payload.len());
        bytes.extend_from_slice(&[0, VERSION, self.kind as u8, 0]);
        bytes.extend_from_slice(&self.slot.to_be_bytes());
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.offset.to_be_bytes());
        bytes.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < HEADER || bytes[0] != 0 || bytes[1] != VERSION || bytes[3] != 0 { return Err(invalid()); }
        let kind = match bytes[2] { 1 => Kind::Data, 2 => Kind::Ack, 3 => Kind::Credit, 4 => Kind::Fin, 5 => Kind::Reset, _ => return Err(invalid()) };
        let size = u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as usize;
        if size > WINDOW || bytes.len() != HEADER + size { return Err(invalid()); }
        let frame = Self {
            kind,
            slot: u32::from_be_bytes(bytes[4..8].try_into().unwrap()),
            epoch: u32::from_be_bytes(bytes[8..12].try_into().unwrap()),
            offset: u64::from_be_bytes(bytes[12..20].try_into().unwrap()),
            payload: bytes[HEADER..].to_vec(),
        };
        frame.validate()?;
        Ok(frame)
    }
    pub fn size(&self) -> usize { HEADER + self.payload.len() }
    pub fn is_control(&self) -> bool { self.kind != Kind::Data }
}

/// Controls keep their own allowance; a full data lane cannot prevent FIN/reset.
#[derive(Default)]
pub struct Queue {
    control: std::collections::VecDeque<Frame>,
    data: std::collections::VecDeque<Frame>,
    control_bytes: usize,
    data_bytes: usize,
    control_run: usize,
}
impl Queue {
    pub fn push(&mut self, frame: Frame) -> io::Result<bool> {
        frame.validate()?;
        let size = frame.size();
        if frame.is_control() {
            if self.control_bytes + size > CONTROL_BYTES { return Ok(false); }
            self.control_bytes += size; self.control.push_back(frame);
        } else {
            if self.data_bytes + size > QUEUE_BYTES { return Ok(false); }
            self.data_bytes += size; self.data.push_back(frame);
        }
        Ok(true)
    }
    pub fn pop(&mut self) -> Option<Frame> {
        if !self.control.is_empty() && (self.control_run < 8 || self.data.is_empty()) {
            let frame = self.control.pop_front()?;
            self.control_bytes -= frame.size(); self.control_run += 1; Some(frame)
        } else {
            let frame = self.data.pop_front()?;
            self.data_bytes -= frame.size(); self.control_run = 0; Some(frame)
        }
    }
    pub fn bytes(&self) -> usize { self.control_bytes + self.data_bytes }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_frames_and_safe_offsets() {
        let frame = Frame { kind: Kind::Data, slot: 17, epoch: 3, offset: 4, payload: vec![0, 255, 5] };
        assert_eq!(Frame::decode(&frame.encode().unwrap()).unwrap(), frame);
        let encoded = frame.encode().unwrap();
        for end in 0..encoded.len() { assert!(Frame::decode(&encoded[..end]).is_err()); }
        for index in [0, 1, 2, 3, 20] {
            let mut corrupt = encoded.clone(); corrupt[index] = 255;
            assert!(Frame::decode(&corrupt).is_err());
        }
        assert!(Frame { offset: MAX_OFFSET, ..frame.clone() }.encode().is_err());
        assert!(Frame { kind: Kind::Ack, ..frame }.encode().is_err());
    }
    #[test]
    fn saturated_data_keeps_control_capacity_and_fairness() {
        let mut queue = Queue::default();
        let data = Frame { kind: Kind::Data, slot: 1, epoch: 1, offset: 0, payload: vec![7; WINDOW] };
        while queue.push(data.clone()).unwrap() {}
        assert!(queue.bytes() <= QUEUE_BYTES);
        for _ in 0..10 { assert!(queue.push(Frame::control(Kind::Reset, 1, 1, 0)).unwrap()); }
        for _ in 0..8 { assert_eq!(queue.pop().unwrap().kind, Kind::Reset); }
        assert_eq!(queue.pop().unwrap().kind, Kind::Data);
    }
}
