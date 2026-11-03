//! An indenting text buffer for the emitters.

pub struct Out {
    pub s: String,
    indent: usize,
    unit: &'static str,
}

impl Out {
    pub fn new(unit: &'static str) -> Self {
        Self { s: String::new(), indent: 0, unit }
    }

    pub fn line(&mut self, text: &str) {
        if !text.is_empty() {
            for _ in 0..self.indent {
                self.s.push_str(self.unit);
            }
            self.s.push_str(text);
        }
        self.s.push('\n');
    }

    /// The text with no blank line at the end (one final newline).
    pub fn finish(mut self) -> String {
        self.s.truncate(self.s.trim_end().len());
        self.s.push('\n');
        self.s
    }

    pub fn blank(&mut self) {
        self.s.push('\n');
    }

    /// A line that opens a block.
    pub fn open(&mut self, text: &str) {
        self.line(text);
        self.indent += 1;
    }

    /// A line that closes a block.
    pub fn close(&mut self, text: &str) {
        self.indent = self.indent.saturating_sub(1);
        self.line(text);
    }

    /// A line that closes a block and opens the next (`} else {`).
    pub fn mid(&mut self, text: &str) {
        self.indent = self.indent.saturating_sub(1);
        self.line(text);
        self.indent += 1;
    }

    pub fn docs(&mut self, prefix: &str, docs: &[String]) {
        for d in docs {
            if d.is_empty() {
                self.line(prefix.trim_end());
            } else {
                self.line(&format!("{prefix}{d}"));
            }
        }
    }
}

/// `FlipH` → `FLIP_H`, `bytes_ref` → `BYTES_REF`.
pub fn screaming(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
    }
    out
}

/// `Page` → `page`, `RectItem` → `rect_item`.
pub fn snake(s: &str) -> String {
    screaming(s).to_ascii_lowercase()
}
