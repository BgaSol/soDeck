//! Bounded standard Base64 for control-channel binary data.
use ::base64::{engine::general_purpose::STANDARD, Engine};
pub fn base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}
pub fn unbase64(input: &str) -> Result<Vec<u8>, String> {
    if !input.len().is_multiple_of(4) || input.len() > 2 * 1024 * 1024 {
        return Err("Base64 长度无效".into());
    }
    STANDARD.decode(input).map_err(|_| "Base64 编码无效".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_vectors_and_binary_page_boundaries() {
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
        for size in [256usize, 8191, 8192, 8193] {
            let raw: Vec<_> = (0..size).map(|i| i as u8).collect();
            let encoded = base64(&raw);
            assert_eq!(encoded.len(), size.div_ceil(3) * 4);
            assert_eq!(unbase64(&encoded).unwrap(), raw);
        }
    }

    #[test]
    fn rejects_noncanonical_encoding() {
        for input in [
            "Zg=a",
            "Zh==",
            "Zm9=",
            "====",
            "Zg==AAAA",
            "Zg===garbage",
            "Zg",
            "Zg==!",
            "Zm9v=",
            "Zg==\n",
            " Zg==",
            "____",
            "----",
        ] {
            assert!(unbase64(input).is_err(), "accepted {input:?}");
        }
        assert_eq!(unbase64("Zg").unwrap_err(), "Base64 长度无效");
        assert_eq!(unbase64("Zh==").unwrap_err(), "Base64 编码无效");
    }

    #[test]
    fn limit_applies_to_encoded_text() {
        let encoded = "AAAA".repeat(2 * 1024 * 1024 / 4);
        assert_eq!(
            unbase64(&encoded).unwrap(),
            vec![0; 2 * 1024 * 1024 / 4 * 3]
        );
        assert_eq!(
            unbase64(&(encoded + "AAAA")).unwrap_err(),
            "Base64 长度无效"
        );
    }
}
