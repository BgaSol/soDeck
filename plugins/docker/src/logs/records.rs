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
        let seconds = clock_seconds(value)?;
        let fraction = if value.as_bytes().get(19) == Some(&b'.') {
            value[20..].split(['Z', '+', '-']).next().unwrap()
        } else {
            ""
        };
        let nanos = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>().unwrap() * 10u128.pow(9 - fraction.len() as u32)
        };
        Ok(Self(seconds as u128 * NS + nanos))
    }
    pub fn text(self) -> String {
        let seconds = self.0 / NS;
        let days = seconds / 86400;
        let clock = seconds % 86400;
        let z = days as i64 + 719468;
        let era = z / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let mut year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        year += if month <= 2 { 1 } else { 0 };
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",
            clock / 3600,
            clock / 60 % 60,
            clock % 60,
            self.0 % NS
        )
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
}
pub struct Parser {
    channels: [Fragment; 2],
    sequence: Option<u64>,
    combined: bool,
    pub raw_bytes: u64,
    skip: [bool; 2],
}
impl Parser {
    pub fn new(combined: bool) -> Self {
        Self {
            channels: Default::default(),
            sequence: None,
            combined,
            raw_bytes: 0,
            skip: [false; 2],
        }
    }
    pub fn gap(&mut self) {
        self.channels = Default::default();
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
                fragment.incomplete |= part.len() > available;
                if part.last() == Some(&b'\n') {
                    let fragment = std::mem::take(fragment);
                    if !self.skip[channel] {
                        rows.push(decode(
                            fragment,
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
fn decode(fragment: Fragment, received_at: u64, source: &'static str) -> Draft {
    let raw = fragment.bytes;
    let decoded = String::from_utf8_lossy(&raw);
    let line = decoded.trim_end_matches(['\r', '\n']);
    let prefix = line.split_once(' ');
    let time = prefix.and_then(|(time, _)| Stamp::parse(time).ok());
    let text = plain(if time.is_some() {
        prefix.unwrap().1
    } else {
        line
    });
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
fn plain(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c.is_control() && c != '\t' {
            output.push('�');
        } else {
            output.push(c);
        }
    }
    output
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

fn clock_seconds(value: &str) -> Result<u64, String> {
    let invalid = || "日志源时间无效".to_owned();
    if !value.is_ascii()
        || value.len() < 20
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || value.as_bytes()[10] != b'T'
        || value.as_bytes()[13] != b':'
        || value.as_bytes()[16] != b':'
    {
        return Err(invalid());
    }
    let number = |a, b| {
        if !value[a..b].bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        value[a..b].parse::<i64>().map_err(|_| invalid())
    };
    let (mut year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=days[(month - 1) as usize]).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
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
    let offset = if zone == "Z" {
        0
    } else {
        if zone.len() != 6
            || !zone.starts_with(['+', '-'])
            || &zone[3..4] != ":"
            || !zone[1..3]
                .bytes()
                .chain(zone[4..6].bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(invalid());
        }
        let h = zone[1..3].parse::<i64>().map_err(|_| invalid())?;
        let m = zone[4..6].parse::<i64>().map_err(|_| invalid())?;
        if h > 23 || m > 59 {
            return Err(invalid());
        }
        (h * 3600 + m * 60) * if zone.starts_with('+') { 1 } else { -1 }
    };
    year -= if month <= 2 { 1 } else { 0 };
    let era = year / 400;
    let y = year - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let d = (153 * m + 2) / 5 + day - 1;
    u64::try_from(
        (era * 146097 + y * 365 + y / 4 - y / 100 + d - 719468) * 86400
            + hour * 3600
            + minute * 60
            + second
            - offset,
    )
    .map_err(|_| invalid())
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
            "2026-02-29T00:00:00Z",
            "2026-11-24T00:00:00.helloZ",
            "中文中文中文中文中文中文中文中文",
            "2026-11-24T00:00:00--1:00",
        ] {
            assert!(Stamp::parse(s).is_err());
        }
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
    fn explicit_levels_and_utf16_highlights() {
        assert_eq!(level("failed to connect"), "");
        assert_eq!(level("{\"severity\":\"warning\"}"), "WARN");
        assert_eq!(level("[ERROR] message"), "ERROR");
        assert_eq!(matches("😀中文 INFO İ K", "info"), vec![[5, 9]]);
        assert_eq!(matches("😀中文 INFO İ K", "k"), vec![[12, 13]]);
        assert_eq!(matches("a.b aXb", "a.b"), vec![[0, 3]]);
    }
}
