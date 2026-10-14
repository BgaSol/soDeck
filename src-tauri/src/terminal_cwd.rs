//! Incrementally removes this session's directory reports from the PTY stream.
//! Other OSCs and ordinary output remain byte-for-byte intact.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CwdUpdate {
    pub path: String,
    pub home: Option<String>,
    pub sequence: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParsedOutput {
    Data(Vec<u8>),
    Cwd(CwdUpdate),
}

pub struct CwdParser {
    prefix: Vec<u8>,
    pending: Vec<u8>,
    last: Option<(String, Option<String>)>,
    sequence: u64,
}

impl CwdParser {
    pub fn new(token: &str) -> Self {
        Self {
            prefix: format!("\x1b]777;ssh-workspace;{token};").into_bytes(),
            pending: Vec::new(),
            last: None,
            sequence: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<ParsedOutput> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        loop {
            let start = self
                .pending
                .windows(self.prefix.len())
                .position(|window| window == self.prefix);
            let Some(start) = start else {
                // Keep only a suffix which could become our prefix next read.
                let keep = (1..self.prefix.len().min(self.pending.len() + 1))
                    .rev()
                    .find(|&size| self.pending.ends_with(&self.prefix[..size]))
                    .unwrap_or(0);
                let ready = self.pending.len() - keep;
                emit_data(&mut output, self.pending.drain(..ready).collect());
                break;
            };
            if start > 0 {
                emit_data(&mut output, self.pending.drain(..start).collect());
            }
            let payload_start = self.prefix.len();
            let end = (payload_start..self.pending.len()).find_map(|index| {
                if self.pending[index] == 7 {
                    Some((index, 1))
                } else if self.pending[index] == 27 && self.pending.get(index + 1) == Some(&b'\\') {
                    Some((index, 2))
                } else {
                    None
                }
            });
            let Some((end, terminator_size)) = end else {
                // A malformed OSC must never stall output or grow indefinitely.
                if self.pending.len() > 128 * 1024 {
                    emit_data(&mut output, std::mem::take(&mut self.pending));
                }
                break;
            };
            let payload = &self.pending[payload_start..end];
            if let Some((path, home)) = parse_payload(payload) {
                if self.last.as_ref() != Some(&(path.clone(), home.clone())) {
                    self.sequence += 1;
                    self.last = Some((path.clone(), home.clone()));
                    output.push(ParsedOutput::Cwd(CwdUpdate {
                        path,
                        home,
                        sequence: self.sequence,
                    }));
                }
                self.pending.drain(..end + terminator_size);
            } else {
                emit_data(
                    &mut output,
                    self.pending.drain(..end + terminator_size).collect(),
                );
            }
        }
        output
    }

    pub fn finish(&mut self) -> Option<Vec<u8>> {
        (!self.pending.is_empty()).then(|| std::mem::take(&mut self.pending))
    }
}

fn emit_data(output: &mut Vec<ParsedOutput>, bytes: Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    if let Some(ParsedOutput::Data(previous)) = output.last_mut() {
        previous.extend(bytes);
    } else {
        output.push(ParsedOutput::Data(bytes));
    }
}

fn parse_payload(payload: &[u8]) -> Option<(String, Option<String>)> {
    let mut parts = payload.split(|byte| *byte == b';');
    let path = decode_path(parts.next()?)?;
    let home_bytes = parts.next()?;
    let home = if home_bytes.is_empty() {
        None
    } else {
        Some(decode_path(home_bytes)?)
    };
    if parts.next().is_some() {
        return None;
    }
    Some((path, home))
}

fn decode_path(bytes: &[u8]) -> Option<String> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    (path.starts_with('/') && !path.contains('\0')).then_some(path)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(path: &str) -> Vec<u8> {
        format!("\x1b]777;ssh-workspace;own;{path};/home/ops\x07").into_bytes()
    }

    #[test]
    fn every_network_split_preserves_output_and_decodes_paths() {
        let osc = report("/home/ops/中文 空格%25%3B%0A");
        for split in 0..=osc.len() {
            let mut parser = CwdParser::new("own");
            let mut events = parser.push(b"before\x1b]0;title\x07");
            events.extend(parser.push(&osc[..split]));
            events.extend(parser.push(&osc[split..]));
            events.extend(parser.push(b"after"));
            let data: Vec<u8> = events
                .iter()
                .filter_map(|event| match event {
                    ParsedOutput::Data(data) => Some(data.as_slice()),
                    _ => None,
                })
                .flatten()
                .copied()
                .collect();
            assert_eq!(data, b"before\x1b]0;title\x07after");
            let changes: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    ParsedOutput::Cwd(cwd) => Some(cwd),
                    _ => None,
                })
                .collect();
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].path, "/home/ops/中文 空格%;\n");
            assert_eq!(changes[0].sequence, 1);
        }
    }

    #[test]
    fn bytewise_reports_deduplicate_and_order_changes() {
        let mut parser = CwdParser::new("own");
        let bytes = [report("/home/ops"), report("/home/ops"), report("/tmp")].concat();
        let events: Vec<_> = bytes
            .iter()
            .flat_map(|byte| parser.push(&[*byte]))
            .collect();
        assert_eq!(
            events,
            vec![
                ParsedOutput::Cwd(CwdUpdate {
                    path: "/home/ops".into(),
                    home: Some("/home/ops".into()),
                    sequence: 1
                }),
                ParsedOutput::Cwd(CwdUpdate {
                    path: "/tmp".into(),
                    home: Some("/home/ops".into()),
                    sequence: 2
                }),
            ]
        );
    }

    #[test]
    fn foreign_tokens_invalid_paths_and_incomplete_output_are_preserved() {
        let mut parser = CwdParser::new("own");
        let foreign = b"\x1b]777;ssh-workspace;other;/tmp;/home/ops\x07";
        assert_eq!(
            parser.push(foreign),
            vec![ParsedOutput::Data(foreign.to_vec())]
        );
        let invalid = report("/bad%ZZ");
        assert_eq!(parser.push(&invalid), vec![ParsedOutput::Data(invalid)]);
        let partial = b"tail\x1b]777;ssh-workspace;own;/unfinished";
        assert_eq!(
            parser.push(partial),
            vec![ParsedOutput::Data(b"tail".to_vec())]
        );
        assert_eq!(parser.finish().unwrap(), &partial[4..]);
    }

    #[test]
    fn accepts_st_terminator_and_bounds_malformed_reports() {
        let mut parser = CwdParser::new("own");
        let message = b"\x1b]777;ssh-workspace;own;/tmp;\x1b\\";
        assert_eq!(
            parser.push(message),
            vec![ParsedOutput::Cwd(CwdUpdate {
                path: "/tmp".into(),
                home: None,
                sequence: 1
            })]
        );
        let large = [
            b"\x1b]777;ssh-workspace;own;/".as_slice(),
            &vec![b'x'; 129 * 1024],
        ]
        .concat();
        assert_eq!(parser.push(&large), vec![ParsedOutput::Data(large)]);
        assert!(parser.finish().is_none());
    }
}
