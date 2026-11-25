use super::contract::RECORD_BYTES;
use serde_json::Value;
use sodeck_plugin_client::{encoding::unbase64, protocol::replies::OutputChunk};

pub const NS: u128 = 1_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub u128);
impl Stamp {
    pub fn seconds(n: u64) -> Self {
        Self(n as u128 * NS)
    }
    pub fn add(self, seconds: u64) -> Self {
        Self(self.0 + seconds as u128 * NS)
    }
    pub fn before(self, seconds: u64) -> Self {
        Self(self.0.saturating_sub(seconds as u128 * NS))
    }
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || "日志源时间无效".to_owned();
        // Keep the Docker source contract stricter than chrono's RFC3339 parser.
        if !value.is_ascii()
            || value.len() < 20
            || value.as_bytes()[10] != b'T'
            || &value[0..4] < "1970"
            || &value[0..4] > "9999"
            || &value[17..19] > "59"
        {
            return Err(invalid());
        }
        let suffix = &value[19..];
        let zone = if let Some(fraction) = suffix.strip_prefix('.') {
            let at = fraction.find(['Z', '+', '-']).ok_or_else(invalid)?;
            if at == 0 || at > 9 || !fraction[..at].bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            &fraction[at..]
        } else {
            suffix
        };
        if zone != "Z"
            && (zone.len() != 6 || !zone.starts_with(['+', '-']) || zone.as_bytes()[3] != b':')
        {
            return Err(invalid());
        }
        let parsed = chrono::DateTime::parse_from_rfc3339(value).map_err(|_| invalid())?;
        let seconds = u128::try_from(parsed.timestamp()).map_err(|_| invalid())?;
        Ok(Self(seconds * NS + parsed.timestamp_subsec_nanos() as u128))
    }
    pub fn text(self) -> String {
        let seconds = i64::try_from(self.0 / NS).expect("bounded Docker timestamp");
        chrono::DateTime::from_timestamp(seconds, (self.0 % NS) as u32)
            .expect("bounded Docker calendar")
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
    }
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub raw: Vec<u8>,
    pub text: String,
    pub time: Option<Stamp>,
    pub received_at: u64,
    pub source: &'static str,
    pub level: String,
    pub incomplete: bool,
}
#[derive(Default)]
struct Fragment {
    bytes: Vec<u8>,
    incomplete: bool,
    preview: Option<(Option<Stamp>, String)>,
}
pub struct Parser {
    channels: [Fragment; 2],
    plain: [super::plain::Plain; 2],
    sequence: Option<u64>,
    combined: bool,
    pub raw_bytes: u64,
    skip: [bool; 2],
}
impl Parser {
    pub fn new(combined: bool) -> Self {
        Self {
            channels: Default::default(),
            plain: Default::default(),
            sequence: None,
            combined,
            raw_bytes: 0,
            skip: [false; 2],
        }
    }
    pub fn gap(&mut self) {
        self.channels = Default::default();
        self.plain = Default::default();
        self.sequence = None;
        self.skip = [true; 2];
    }
    pub fn append(&mut self, chunks: &[OutputChunk]) -> Result<Vec<Draft>, String> {
        let mut rows = Vec::new();
        for chunk in chunks {
            let sequence = chunk
                .sequence
                .parse::<u64>()
                .map_err(|_| "日志输出序号无效")?;
            if chunk.v != 1 || self.sequence.is_some_and(|old| sequence != old + 1) {
                return Err("日志输出版本或序号不连续".into());
            }
            let channel = match chunk.source.as_str() {
                "stdout" => 0,
                "stderr" => 1,
                _ => return Err("日志输出来源无效".into()),
            };
            self.sequence = Some(sequence);
            let bytes = unbase64(&chunk.data_base64)?;
            self.raw_bytes += bytes.len() as u64;
            for part in bytes.split_inclusive(|b| *b == b'\n') {
                let fragment = &mut self.channels[channel];
                let available = RECORD_BYTES.saturating_sub(fragment.bytes.len());
                fragment
                    .bytes
                    .extend_from_slice(&part[..part.len().min(available)]);
                if part.len() > available {
                    if !fragment.incomplete {
                        fragment.preview = Some(display(&fragment.bytes, &mut self.plain[channel]));
                    }
                    // Feed discarded bytes too: a truncated OSC/DCS must not poison the next row.
                    self.plain[channel].discard(&part[available..]);
                    fragment.incomplete = true;
                }
                if part.last() == Some(&b'\n') {
                    let fragment = std::mem::take(fragment);
                    if !self.skip[channel] {
                        rows.push(decode(
                            fragment,
                            &mut self.plain[channel],
                            chunk.received_at,
                            if self.combined {
                                "combined"
                            } else if channel == 0 {
                                "stdout"
                            } else {
                                "stderr"
                            },
                        ));
                    }
                    self.skip[channel] = false;
                }
            }
        }
        Ok(rows)
    }
    pub fn finish(&mut self, received_at: u64) -> Vec<Draft> {
        (0..2)
            .filter_map(|i| {
                let f = std::mem::take(&mut self.channels[i]);
                if f.bytes.is_empty() || self.skip[i] {
                    None
                } else {
                    Some(decode(
                        f,
                        &mut self.plain[i],
                        received_at,
                        if self.combined {
                            "combined"
                        } else if i == 0 {
                            "stdout"
                        } else {
                            "stderr"
                        },
                    ))
                }
            })
            .collect()
    }
}
fn display(raw: &[u8], plain: &mut super::plain::Plain) -> (Option<Stamp>, String) {
    let decoded = String::from_utf8_lossy(raw);
    let line = decoded.trim_end_matches(['\r', '\n']);
    let prefix = line.split_once(' ');
    let time = prefix.and_then(|(time, _)| Stamp::parse(time).ok());
    let text = plain.text(if time.is_some() {
        prefix.unwrap().1
    } else {
        line
    });
    (time, text)
}
fn decode(
    fragment: Fragment,
    plain: &mut super::plain::Plain,
    received_at: u64,
    source: &'static str,
) -> Draft {
    let raw = fragment.bytes;
    let (time, text) = fragment.preview.unwrap_or_else(|| display(&raw, plain));
    let level = level(&text);
    Draft {
        raw,
        text,
        time,
        received_at,
        source,
        level,
        incomplete: fragment.incomplete,
    }
}
pub fn level(text: &str) -> String {
    let mut text = text.trim_start();
    if text.starts_with('[') && text.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        if let Some(end) = text.find(']') {
            text = text[end + 1..].trim_start();
        }
    }
    let token = if let Some(text) = text.strip_prefix('[') {
        text.split(']').next().unwrap_or("")
    } else {
        text.split(|c: char| !c.is_ascii_alphabetic())
            .next()
            .unwrap_or("")
    };
    let value = if text.starts_with('{') {
        serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| {
                v.get("level")
                    .or_else(|| v.get("severity"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    } else {
        token.into()
    };
    match value.to_ascii_uppercase().as_str() {
        "TRACE" => "TRACE",
        "DEBUG" => "DEBUG",
        "INFO" | "INFORMATION" => "INFO",
        "WARN" | "WARNING" => "WARN",
        "ERROR" | "ERR" => "ERROR",
        "FATAL" | "CRITICAL" => "FATAL",
        _ => "",
    }
    .into()
}
pub fn matches(text: &str, query: &str) -> Vec<[usize; 2]> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut folded = String::new();
    let mut mapping = Vec::new();
    let mut utf16 = 0;
    for c in text.chars() {
        let end = utf16 + c.len_utf16();
        for lower in c.to_lowercase() {
            let start = folded.len();
            folded.push(lower);
            mapping.push((start, folded.len(), utf16, end));
        }
        utf16 = end;
    }
    let query = query.to_lowercase();
    folded
        .match_indices(&query)
        .map(|(at, found)| {
            let start = mapping.partition_point(|(_, b, _, _)| *b <= at);
            let end = mapping
                .partition_point(|(a, _, _, _)| *a < at + found.len())
                .saturating_sub(1);
            [mapping[start].2, mapping[end].3]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sodeck_plugin_client::encoding::base64;
    fn chunk(n: u64, source: &str, bytes: &[u8]) -> OutputChunk {
        OutputChunk {
            v: 1,
            sequence: n.to_string(),
            source: source.into(),
            received_at: 123,
            data_base64: base64(bytes),
        }
    }
    #[test]
    fn source_time_keeps_nanoseconds_calendar_and_zones() {
        let time = Stamp::parse("2026-11-24T08:00:00.123456789+08:00").unwrap();
        assert_eq!(time.text(), "2026-11-24T00:00:00.123456789Z");
        assert_eq!(Stamp::seconds(0).text(), "1970-01-01T00:00:00.000000000Z");
        for s in [
            "2016-12-31T23:59:60Z",
            "2026-11-24T00:00:00.1234567891Z",
            "1969-12-31T23:59:59Z",
            "1970-01-01T00:00:00+00:01",
            "2026-11-24t00:00:00z",
            "2026-02-29T00:00:00Z",
            "2026-11-24T00:00:00.helloZ",
            "中文中文中文中文中文中文中文中文",
            "2026-11-24T00:00:00--1:00",
        ] {
            assert!(Stamp::parse(s).is_err());
        }
        let maximum = "9999-12-31T23:59:59.999999999Z";
        assert_eq!(Stamp::parse(maximum).unwrap().text(), maximum);
        assert_eq!(
            Stamp::parse("2000-02-29T00:00:00.1Z").unwrap().text(),
            "2000-02-29T00:00:00.100000000Z"
        );
    }
    #[test]
    fn terminal_sequences_cross_records_without_leaking_payload_or_sources() {
        let mut p = Parser::new(false);
        let rows = p
            .append(&[
                chunk(1, "stdout", b"2026-11-24T00:00:00Z INFO\t\x1bPqsecret\n"),
                chunk(2, "stderr", b"2026-11-24T00:00:00Z WARN independent\n"),
                chunk(
                    3,
                    "stdout",
                    b"2026-11-24T00:00:01Z more\x1b\\\x1b(0lqqk\x1b(B\n",
                ),
            ])
            .unwrap();
        assert_eq!(rows[0].text, "INFO\t");
        assert_eq!(rows[1].text, "WARN independent");
        assert_eq!(rows[2].text, "lqqk");
        assert!(rows[0].raw.windows(6).any(|v| v == b"secret"));
    }
    #[test]
    fn split_utf8_interleaved_sources_and_repeats_preserve_original_bytes() {
        let mut p = Parser::new(false);
        let text = "2026-11-24T00:00:00.000000001Z INFO 中文\n";
        let at = text.len() - 3;
        assert!(p
            .append(&[chunk(1, "stdout", &text.as_bytes()[..at])])
            .unwrap()
            .is_empty());
        let rows = p
            .append(&[
                chunk(2, "stderr", b"WARN elsewhere\n"),
                chunk(3, "stdout", &text.as_bytes()[at..]),
                chunk(4, "stdout", text.as_bytes()),
            ])
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].raw, text.as_bytes());
        assert_eq!(rows[1].text, "INFO 中文");
        assert_eq!(rows[1].time, rows[2].time);
    }
    #[test]
    fn gaps_do_not_join_records_and_oversized_lines_are_explicit() {
        let mut p = Parser::new(false);
        p.append(&[chunk(1, "stdout", b"broken")]).unwrap();
        p.gap();
        let rows = p
            .append(&[chunk(4, "stdout", b"fragment\nINFO valid\n")])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "INFO valid");
        let mut p = Parser::new(false);
        let bytes = vec![b'x'; RECORD_BYTES];
        for (i, b) in bytes.chunks(8192).enumerate() {
            p.append(&[chunk(i as u64 + 1, "stdout", b)]).unwrap();
        }
        let rows = p.append(&[chunk(129, "stdout", b"overflow\n")]).unwrap();
        assert!(rows[0].incomplete);
        assert_eq!(rows[0].raw.len(), RECORD_BYTES);
    }
    #[test]
    fn invalid_utf8_ansi_and_tty_keep_safe_text_and_raw() {
        let raw = b"\x1b[31mINFO \xff\x1b[0m\n";
        let rows = Parser::new(true)
            .append(&[chunk(1, "stderr", raw)])
            .unwrap();
        assert_eq!(rows[0].source, "combined");
        assert_eq!(rows[0].text, "INFO �");
        assert_eq!(rows[0].raw, raw);
    }
    #[test]
    fn fragmented_osc_truncation_and_gaps_keep_bounded_parser_state() {
        let mut p = Parser::new(false);
        assert!(p
            .append(&[chunk(1, "stdout", b"\x1b]0;hidden")])
            .unwrap()
            .is_empty());
        let rows = p
            .append(&[chunk(2, "stdout", b"title\x07INFO ok\n")])
            .unwrap();
        assert_eq!(rows[0].text, "INFO ok");
        let mut oversized = b"\x1b]0;".to_vec();
        oversized.extend(vec![b'x'; RECORD_BYTES]);
        oversized.extend_from_slice(b"\x07\nINFO next\n");
        let rows = p.append(&[chunk(3, "stdout", &oversized)]).unwrap();
        assert!(rows[0].incomplete);
        assert_eq!(rows[0].text, "");
        assert_eq!(rows[1].text, "INFO next");
        p.append(&[chunk(4, "stdout", b"\x1bPqhidden\n")]).unwrap();
        p.gap();
        let rows = p
            .append(&[chunk(6, "stdout", b"unknown fragment\nWARN recovered\n")])
            .unwrap();
        assert_eq!(rows[0].text, "WARN recovered");
    }

    #[test]
    fn explicit_levels_and_utf16_highlights() {
        assert_eq!(level("failed to connect"), "");
        assert_eq!(level("{\"severity\":\"warning\"}"), "WARN");
        assert_eq!(level("[ERROR] message"), "ERROR");
        assert_eq!(matches("😀中文 INFO İ K", "info"), vec![[5, 9]]);
        assert_eq!(matches("😀中文 INFO İ K", "k"), vec![[12, 13]]);
        assert_eq!(matches("a.b aXb", "a.b"), vec![[0, 3]]);
    }
}
