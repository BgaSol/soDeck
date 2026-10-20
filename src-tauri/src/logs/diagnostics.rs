//! CLI errors lack Docker's timestamp prefix. Only known diagnostic prefixes
//! are separated; ambiguous bytes remain intact in the application stream.
#[derive(Default)]
pub struct Diagnostics {
    pending: Vec<u8>,
    passthrough: bool,
    diagnostic: bool,
    pub details: Vec<u8>,
    original: Vec<u8>,
}
const PREFIXES: &[&[u8]] = &[
    b"Error response from daemon:",
    b"Error: No such container:",
    b"configured logging driver does not support",
    b"error during connect:",
    b"Cannot connect to the Docker daemon",
    b"permission denied while trying to connect",
];
impl Diagnostics {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        for &b in bytes {
            if self.diagnostic {
                if self.details.len() < 8192 {
                    self.details.push(b);
                }
                self.original.push(b);
                if self.original.len() > 8192 {
                    output.append(&mut self.original);
                    self.diagnostic = false;
                    self.passthrough = b != b'\n';
                }
                if b == b'\n' {
                    self.diagnostic = false;
                }
            } else if self.passthrough {
                output.push(b);
                if b == b'\n' {
                    self.passthrough = false;
                }
            } else {
                self.pending.push(b);
                if PREFIXES.iter().any(|p| self.pending.starts_with(p)) {
                    self.diagnostic = true;
                    self.original.extend_from_slice(&self.pending);
                    let remaining = 8192usize.saturating_sub(self.details.len());
                    self.details.extend(self.pending.drain(..).take(remaining));
                    self.pending.clear();
                } else if b == b'\n' || !PREFIXES.iter().any(|p| p.starts_with(&self.pending)) {
                    output.append(&mut self.pending);
                    self.passthrough = b != b'\n';
                }
            }
        }
        output
    }
    pub fn success(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.original)
    }
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_split_diagnostics_without_losing_timestamped_stderr() {
        let mut d = Diagnostics::default();
        assert!(d.push(b"configured logging dri").is_empty());
        assert!(d.push(b"ver does not support reading\n").is_empty());
        assert!(!d.details.is_empty());
        let app = b"2026-10-20T01:02:03Z Error response from daemon: user text\n\xff\xfe\n";
        assert_eq!(d.push(app), app);
        assert!(d.finish().is_empty());
    }
    #[test]
    fn ambiguous_partial_prefix_is_preserved() {
        let mut d = Diagnostics::default();
        assert!(d.push(b"Error").is_empty());
        assert_eq!(d.finish(), b"Error");
    }
}
