//! A passive terminal emulator. Its responses and OSC callbacks are never connected to a PTY.
//! Records are immutable screen/text versions; transport packet boundaries do not terminate lines.
use super::model::{Event, EventKind, Row};
use unicode_width::UnicodeWidthChar;

#[derive(Default)]
enum Action {
    #[default]
    None,
    Print(char),
    LineFeed,
    CarriageReturn,
    Move,
    Erase,
    Clear,
    Reset,
    Alternate,
}
#[derive(Default)]
struct Observer {
    action: Action,
}
impl vte::Perform for Observer {
    fn print(&mut self, c: char) {
        self.action = Action::Print(c);
    }
    fn execute(&mut self, b: u8) {
        self.action = match b {
            10..=12 => Action::LineFeed,
            13 => Action::CarriageReturn,
            8 | 9 => Action::Move,
            _ => Action::None,
        };
    }
    fn esc_dispatch(&mut self, intermediates: &[u8], _: bool, b: u8) {
        if intermediates.is_empty() {
            self.action = match b {
                b'c' => Action::Reset,
                b'8' | b'M' | b'D' | b'E' => Action::Move,
                _ => Action::None,
            };
        }
    }
    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _: bool, c: char) {
        if intermediates == b"?"
            && matches!(c, 'h' | 'l')
            && params.iter().any(|p| matches!(p[0], 47 | 1047 | 1049))
        {
            self.action = Action::Alternate;
        } else if intermediates.is_empty() {
            self.action = match c {
                'J' if params.iter().next().is_some_and(|p| matches!(p[0], 2 | 3)) => Action::Clear,
                'J' | 'K' | 'X' | 'P' | '@' | 'L' | 'M' | 'S' | 'T' => Action::Erase,
                'A'..='I' | 'd' | 'f' | 'r' | 'u' => Action::Move,
                _ => Action::None,
            };
        }
    }
}

pub struct Projection {
    terminal: vt100::Parser,
    observer_parser: vte::Parser,
    observer: Observer,
    session: String,
    segment: u64,
    id: i64,
    dirty: bool,
    revision: bool,
    cleared: bool,
    pending: String,
    pending_ansi: String,
    current: (i64, i64, usize),
    last_applied: (i64, i64, usize),
    rows: Vec<Row>,
}
impl Projection {
    pub fn new(session: &str) -> Self {
        Self {
            terminal: vt100::Parser::new(30, 100, 0),
            observer_parser: vte::Parser::new(),
            observer: Observer::default(),
            session: session.into(),
            segment: 0,
            id: 0,
            dirty: false,
            revision: false,
            cleared: false,
            pending: String::new(),
            pending_ansi: String::new(),
            current: (0, 0, 0),
            last_applied: (0, 0, 0),
            rows: Vec::new(),
        }
    }
    fn emit(&mut self, kind: &str, text: String, ansi: String) {
        self.id += 1;
        self.rows.push(Row {
            id: self.id.to_string(),
            session_id: self.session.clone(),
            seq: self.current.0.to_string(),
            offset: self.current.2,
            time: self.current.1,
            segment: self.segment,
            kind: kind.into(),
            text,
            ansi,
        });
    }
    fn capture(&mut self, continuation: bool) {
        if !self.dirty && self.pending.is_empty() {
            return;
        }
        let screen = self.terminal.screen();
        let row = screen.cursor_position().0 as usize;
        let cols = screen.size().1;
        let text = screen.rows(0, cols).nth(row).unwrap_or_default();
        let ansi =
            String::from_utf8_lossy(&screen.rows_formatted(0, cols).nth(row).unwrap_or_default())
                .into_owned();
        self.pending.push_str(&text);
        self.pending_ansi.push_str(&ansi);
        self.dirty = false;
        if continuation && self.pending.len() < 64 * 1024 {
            return;
        }
        let kind = if screen.alternate_screen() {
            "screen"
        } else if self.revision {
            "revision"
        } else {
            "text"
        };
        let text = std::mem::take(&mut self.pending);
        let ansi = std::mem::take(&mut self.pending_ansi);
        let current = self.current;
        self.current = self.last_applied;
        self.emit(kind, text, ansi);
        self.current = current;
        self.revision = true;
    }
    fn marker(&mut self, text: &str) {
        self.emit("marker", text.into(), String::new());
    }
    pub fn apply(&mut self, event: &Event) -> Vec<Row> {
        self.current = (event.seq, event.time, 0);
        match &event.kind {
            EventKind::Start { cols, rows } => {
                self.terminal = vt100::Parser::new(*rows, *cols, 0);
                self.marker("会话开始 · 此前会话的记录保留在历史中");
            }
            EventKind::Output { bytes } => {
                for (offset, byte) in bytes.iter().enumerate() {
                    self.current.2 = offset;
                    self.observer.action = Action::None;
                    self.observer_parser.advance(&mut self.observer, &[*byte]);
                    let action = std::mem::take(&mut self.observer.action);
                    let was_alt = self.terminal.screen().alternate_screen();
                    match action {
                        Action::Print(c) => {
                            let (_, col) = self.terminal.screen().cursor_position();
                            if col.saturating_add(c.width().unwrap_or(0) as u16)
                                > self.terminal.screen().size().1
                            {
                                self.capture(true);
                            }
                            self.dirty = true;
                            self.cleared = false;
                        }
                        Action::LineFeed => {
                            if !self.dirty && self.pending.is_empty() && !self.revision {
                                self.emit(
                                    if was_alt { "screen" } else { "text" },
                                    String::new(),
                                    String::new(),
                                );
                            } else {
                                self.capture(false);
                            }
                        }
                        Action::CarriageReturn
                        | Action::Move
                        | Action::Erase
                        | Action::Alternate
                        | Action::Clear
                        | Action::Reset => self.capture(false),
                        Action::None => {}
                    }
                    if matches!(action, Action::Clear | Action::Reset) && !was_alt && !self.cleared
                    {
                        self.segment += 1;
                        self.marker("屏幕已清除 · 以上为清屏前记录");
                        self.cleared = true;
                    }
                    self.terminal.process(&[*byte]);
                    self.last_applied = self.current;
                    if matches!(action, Action::LineFeed | Action::Clear | Action::Reset) {
                        self.revision = false;
                    }
                    if matches!(action, Action::Alternate)
                        && was_alt != self.terminal.screen().alternate_screen()
                    {
                        self.marker(if was_alt {
                            "屏幕交互结束"
                        } else {
                            "进入屏幕交互 · 可展开或回放"
                        });
                        self.revision = false;
                    }
                }
            }
            EventKind::Resize { cols, rows } => {
                self.capture(false);
                self.terminal.screen_mut().set_size(*rows, *cols);
            }
            EventKind::Clear => {
                self.capture(false);
                if self.terminal.screen().alternate_screen() {
                    self.marker("屏幕交互中，清理显示已跳过");
                    return std::mem::take(&mut self.rows);
                }
                // xterm.clear retains the cursor line and column, moving that line to row zero.
                let screen = self.terminal.screen();
                let (row, col) = screen.cursor_position();
                let cols = screen.size().1;
                let line = screen
                    .rows_formatted(0, cols)
                    .nth(row as usize)
                    .unwrap_or_default();
                let attributes = screen.attributes_formatted();
                self.terminal.process(b"\x1b[H\x1b[2J");
                self.terminal.process(&line);
                self.terminal
                    .process(format!("\x1b[1;{}H", col.saturating_add(1)).as_bytes());
                self.terminal.process(&attributes);
                self.segment += 1;
                self.marker("已清理显示 · 以上为历史记录");
            }
            EventKind::Checkpoint => self.capture(false),
            EventKind::Gap { reason } => {
                self.capture(false);
                self.segment += 1;
                self.marker(&format!("记录缺口：{reason}；后续屏幕状态可能不完整"));
                let (rows, cols) = self.terminal.screen().size();
                self.terminal = vt100::Parser::new(rows, cols, 0);
                self.observer_parser = vte::Parser::new();
            }
            EventKind::End { reason } => {
                self.capture(false);
                self.marker(&format!("会话结束 · {reason}"));
            }
            EventKind::Cwd { .. } => {}
        }
        std::mem::take(&mut self.rows)
    }
    pub fn last_id(&self) -> i64 {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(seq: i64, bytes: &[u8]) -> Event {
        Event {
            seq,
            time: 0,
            elapsed: 0,
            kind: EventKind::Output {
                bytes: bytes.into(),
            },
        }
    }
    #[test]
    fn split_utf8_escape_clear_and_real_repetitions() {
        let data = "中文\r\n重复\r\n重复\r\n\x1b[H\x1b[2J\x1b[3J以后\r\n".as_bytes();
        let mut whole = Projection::new("a");
        let rows = whole.apply(&output(1, data));
        let texts: Vec<_> = rows.iter().map(|r| (&r.kind, &r.text)).collect();
        for split in 0..=data.len() {
            let mut p = Projection::new("a");
            let mut actual = p.apply(&output(1, &data[..split]));
            actual.extend(p.apply(&output(2, &data[split..])));
            assert_eq!(
                texts,
                actual
                    .iter()
                    .map(|r| (&r.kind, &r.text))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(rows.iter().filter(|r| r.text == "重复").count(), 2);
        assert_eq!(rows.iter().filter(|r| r.kind == "marker").count(), 1);
    }
    #[test]
    fn overwritten_text_and_alternate_screen_survive() {
        let mut p = Projection::new("a");
        let rows = p.apply(&output(
            1,
            b"old\rnew\r\n\x1b[?1049hhidden\x1b[2J\x1b[?1049l",
        ));
        assert!(rows.iter().any(|r| r.text == "old"));
        assert!(rows.iter().any(|r| r.text == "new"));
        assert!(rows
            .iter()
            .any(|r| r.kind == "screen" && r.text == "hidden"));
        assert!(!rows.iter().any(|r| r.text.starts_with("屏幕已清除")));
    }
    #[test]
    fn soft_wrap_is_one_searchable_logical_line() {
        let mut p = Projection::new("a");
        p.apply(&Event {
            seq: 1,
            time: 0,
            elapsed: 0,
            kind: EventKind::Resize { rows: 3, cols: 4 },
        });
        let rows = p.apply(&output(2, b"abcdefghij\r\n"));
        assert_eq!(rows[0].text, "abcdefghij");
    }
    #[test]
    fn cursor_overwrite_combining_text_and_local_clear() {
        let mut p = Projection::new("a");
        let rows = p.apply(&output(
            1,
            "\x1b[31m中文e\u{301}\x1b[0m\r\n\r\nold\x1b[2DXX\r\n$ ".as_bytes(),
        ));
        assert!(rows
            .iter()
            .any(|r| r.text == "中文e\u{301}" && r.ansi.contains("31")));
        assert!(rows.iter().any(|r| r.text.is_empty()));
        assert!(rows.iter().any(|r| r.text == "old"));
        assert!(rows.iter().any(|r| r.text == "oXX"));
        p.apply(&Event {
            seq: 2,
            time: 0,
            elapsed: 0,
            kind: EventKind::Clear,
        });
        let rows = p.apply(&output(3, b"echo test\r\n"));
        assert!(rows.iter().any(|r| r.text == "$ echo test"));
    }
}
