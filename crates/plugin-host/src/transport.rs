use serde_json::Value;
use std::io::{self, Read, Write};
pub const MAX_FRAME: usize = 1024 * 1024;
pub enum Wire { Json(Value), Binary(sodeck_communication::binary::Frame) }

pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Value>> {
    match read_wire(reader)? {
        Some(Wire::Json(value)) => Ok(Some(value)), None => Ok(None),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected binary frame")),
    }
}
pub fn read_wire(reader: &mut impl Read) -> io::Result<Option<Wire>> {
    let mut length = [0u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Plugin frame exceeds 1 MiB",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    if bytes[0] == 0 { return sodeck_communication::binary::Frame::decode(&bytes).map(|f|Some(Wire::Binary(f))); }
    serde_json::from_slice(&bytes)
        .map(|v|Some(Wire::Json(v)))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub fn write_binary(writer: &mut impl Write, frame: &sodeck_communication::binary::Frame) -> io::Result<()> {
    let bytes = frame.encode()?;
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn write_frame(writer: &mut impl Write, frame: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(frame)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Plugin frame exceeds 1 MiB",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_limit_and_truncated_header() {
        assert!(read_frame(&mut &b"\0\0"[..]).is_err());
        assert!(read_frame(&mut &(MAX_FRAME as u32 + 1).to_be_bytes()[..]).is_err());
        let mut encoded = Vec::new();
        let value = serde_json::json!({"text":"中文"});
        write_frame(&mut encoded, &value).unwrap();
        assert_eq!(read_frame(&mut encoded.as_slice()).unwrap(), Some(value));
    }
}
