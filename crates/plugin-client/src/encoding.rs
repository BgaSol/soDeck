//! Bounded control-channel binary encoding, without a runtime dependency.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn base64(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for block in bytes.chunks(3) {
        let n = ((block[0] as u32) << 16)
            | ((block.get(1).copied().unwrap_or(0) as u32) << 8)
            | block.get(2).copied().unwrap_or(0) as u32;
        for i in 0..4 {
            result.push(if i > block.len() {
                '='
            } else {
                ALPHABET[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    result
}
pub fn unbase64(input: &str) -> Result<Vec<u8>, String> {
    if !input.len().is_multiple_of(4) || input.len() > 2 * 1024 * 1024 {
        return Err("Base64 长度无效".into());
    }
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    for (i, block) in input.as_bytes().chunks(4).enumerate() {
        let padding = block.iter().rev().take_while(|&&c| c == b'=').count();
        if padding > 2 || (padding > 0 && (i + 1) * 4 != input.len()) {
            return Err("Base64 填充无效".into());
        }
        let mut n = 0u32;
        for (j, &c) in block.iter().enumerate() {
            let value = if j >= 4 - padding {
                0
            } else {
                ALPHABET
                    .iter()
                    .position(|&v| v == c)
                    .ok_or("Base64 字符无效")? as u32
            };
            n = (n << 6) | value;
        }
        if (padding == 1 && n & 0xff != 0) || (padding == 2 && n & 0xffff != 0) {
            return Err("Base64 填充位无效".into());
        }
        for j in 0..3 - padding {
            output.push((n >> (16 - j * 8)) as u8);
        }
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_reject_malformed() {
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), encoded);
            assert_eq!(unbase64(encoded).unwrap(), raw.as_bytes());
        }
        assert!(unbase64("Zg=a").is_err());
        assert!(unbase64("Zh==").is_err());
    }
}
