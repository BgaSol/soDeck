//! Bounded, display-only terminal parsing. Raw records never pass through this layer.
#[derive(Default)]
pub struct Plain(vte::Parser);

struct Text<'a>(&'a mut String);
impl vte::Perform for Text<'_> {
    fn print(&mut self, c: char) {
        self.0.push(if c.is_control() { '�' } else { c });
    }
    fn execute(&mut self, byte: u8) {
        match byte {
            b'\t' => self.0.push('\t'),
            b'\r' | b'\n' => (),
            _ => self.0.push('�'),
        }
    }
}
impl Plain {
    pub fn text(&mut self, value: &str) -> String {
        let mut text = String::with_capacity(value.len());
        self.0.advance(&mut Text(&mut text), value.as_bytes());
        text
    }
    pub fn discard(&mut self, bytes: &[u8]) {
        struct Ignore;
        impl vte::Perform for Ignore {}
        self.0.advance(&mut Ignore, bytes);
    }
}
