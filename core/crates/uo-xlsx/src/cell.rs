//! Cell-level encoding: column names, date serials, the ST_Xstring escape and numbers.

use crate::error::{Error, Result};

/// The letters of a 1-based column (`1` → `A`, `16384` → `XFD`); empty for 0.
pub fn column_name(col: u32) -> String {
    let mut s = String::new();
    push_column(&mut s, col);
    s
}

pub(crate) fn push_column(out: &mut String, col: u32) {
    let mut buf = [0u8; 8];
    let mut i = buf.len();
    let mut n = col;
    while n > 0 && i > 0 {
        let r = (n - 1) % 26;
        i -= 1;
        buf[i] = b'A' + r as u8;
        n = (n - 1) / 26;
    }
    for &b in &buf[i..] {
        out.push(char::from(b));
    }
}

pub(crate) fn push_u32(out: &mut String, mut n: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 || i == 0 {
            break;
        }
    }
    for &b in &buf[i..] {
        out.push(char::from(b));
    }
}

/// `A1`-style reference of a 1-based (row, column).
pub(crate) fn push_ref(out: &mut String, row: u32, col: u32) {
    push_column(out, col);
    push_u32(out, row);
}

/// A local wall-clock date and time (no time zone: the serial is the clock reading).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DateTime {
    pub year: i32,
    /// 1-12
    pub month: u8,
    /// 1-31
    pub day: u8,
    /// 0-23
    pub hour: u8,
    /// 0-59
    pub minute: u8,
    /// 0-59
    pub second: u8,
    /// 0-999
    pub millisecond: u16,
}

impl DateTime {
    /// A date at midnight.
    pub fn date(year: i32, month: u8, day: u8) -> Self {
        DateTime { year, month, day, ..DateTime::default() }
    }

    /// Whether the time of day is midnight.
    pub fn is_midnight(&self) -> bool {
        self.hour == 0 && self.minute == 0 && self.second == 0 && self.millisecond == 0
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let days_in_month = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap(self.year) => 29,
            2 => 28,
            _ => return Err(Error::InvalidDate),
        };
        if self.day == 0 || self.day > days_in_month || self.hour > 23 || self.minute > 59 || self.second > 59 || self.millisecond > 999 {
            return Err(Error::InvalidDate);
        }
        Ok(())
    }
}

fn leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Days from 1970-01-01 of a proleptic Gregorian date (Hinnant's `days_from_civil`).
fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Excel serial (1900 date system) of a wall-clock date: 1900-01-01 is 1, the fictitious
/// 1900-02-29 is kept (1900-03-01 is 61), the time of day is the fraction. `None` for dates
/// Excel cannot show (before 1900-01-01 or after 9999-12-31) or invalid fields.
pub fn date_serial(d: &DateTime) -> Option<f64> {
    d.validate().ok()?;
    if !(1900..=9999).contains(&d.year) {
        return None;
    }
    // 1899-12-30 is serial 0 for every date from 1900-03-01; earlier dates are one less
    let mut days = days_from_civil(d.year, d.month, d.day) - days_from_civil(1899, 12, 30);
    if days < 61 {
        days -= 1;
    }
    let ms = ((u32::from(d.hour) * 60 + u32::from(d.minute)) * 60 + u32::from(d.second)) * 1000 + u32::from(d.millisecond);
    Some(days as f64 + f64::from(ms) / 86_400_000.0)
}

/// The text of a date Excel cannot store as a serial: ISO 8601 `YYYY-MM-DD`, with
/// ` HH:MM:SS` (and `.mmm`) when the time is not midnight.
pub(crate) fn date_text(d: &DateTime) -> String {
    let mut s = String::new();
    if d.year < 0 {
        s.push('-');
    }
    let y = d.year.unsigned_abs();
    if y < 1000 {
        for _ in 0..(4 - digits(y)) {
            s.push('0');
        }
    }
    push_u32(&mut s, y);
    let two = |s: &mut String, sep: char, v: u8| {
        s.push(sep);
        if v < 10 {
            s.push('0');
        }
        push_u32(s, u32::from(v));
    };
    two(&mut s, '-', d.month);
    two(&mut s, '-', d.day);
    if !d.is_midnight() {
        two(&mut s, ' ', d.hour);
        two(&mut s, ':', d.minute);
        two(&mut s, ':', d.second);
        if d.millisecond > 0 {
            s.push('.');
            let ms = d.millisecond;
            if ms < 100 {
                s.push('0');
            }
            if ms < 10 {
                s.push('0');
            }
            push_u32(&mut s, u32::from(ms));
        }
    }
    s
}

fn digits(mut n: u32) -> u32 {
    let mut k = 1;
    while n >= 10 {
        n /= 10;
        k += 1;
    }
    k
}

/// The `<v>` text of a finite number: the shortest round-trip form (`String(x)` of JS: `1e+21`,
/// `1.5e-7`), which `xsd:double` and Excel read back to the same double. `-0` is written `0`.
pub(crate) fn push_number(out: &mut String, x: f64) {
    if x == 0.0 {
        out.push('0');
    } else {
        out.push_str(&uo_jsnum::format::to_js_string(x));
    }
}

/// Text cut to [`crate::MAX_CELL_TEXT`] UTF-16 units, never inside a surrogate pair.
pub(crate) fn cut_text(s: &str, max_units: usize) -> &str {
    if s.len() <= max_units {
        return s; // UTF-8 bytes >= UTF-16 units
    }
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max_units {
            return &s[..i];
        }
    }
    s
}

/// Appends `s` as ST_Xstring element text: a literal `_xHHHH_` gets the `_x005F_` escape, a
/// character XML 1.0 cannot carry becomes `_xHHHH_` (sheet-core `encodeXstring`, as Excel
/// writes), then `& < >` are escaped.
pub(crate) fn push_xstring(out: &mut String, s: &str) {
    let b = s.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let rep: Option<&str> = match c {
            b'&' => Some("&amp;"),
            b'<' => Some("&lt;"),
            b'>' => Some("&gt;"),
            _ => None,
        };
        if let Some(r) = rep {
            out.push_str(&s[last..i]);
            out.push_str(r);
            i += 1;
            last = i;
            continue;
        }
        if c == b'_' && is_x_escape(&b[i..]) {
            out.push_str(&s[last..i]);
            out.push_str("_x005F");
            last = i;
            i += 1;
            continue;
        }
        if c < 0x20 && !matches!(c, b'\t' | b'\n' | b'\r') {
            out.push_str(&s[last..i]);
            push_x_hex(out, u32::from(c));
            i += 1;
            last = i;
            continue;
        }
        // U+FFFE / U+FFFF: EF BF BE / EF BF BF
        if c == 0xEF && b.get(i + 1) == Some(&0xBF) && matches!(b.get(i + 2), Some(0xBE | 0xBF)) {
            out.push_str(&s[last..i]);
            push_x_hex(out, if b[i + 2] == 0xBE { 0xFFFE } else { 0xFFFF });
            i += 3;
            last = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&s[last..]);
}

/// `_xHHHH_` at the start of `b`.
fn is_x_escape(b: &[u8]) -> bool {
    b.len() >= 7 && b[1] == b'x' && b[2..6].iter().all(u8::is_ascii_hexdigit) && b[6] == b'_'
}

fn push_x_hex(out: &mut String, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push_str("_x");
    for shift in [12, 8, 4, 0] {
        out.push(char::from(HEX[((v >> shift) & 0xF) as usize]));
    }
    out.push('_');
}

/// Whether text needs `xml:space="preserve"` (white space at either end is trimmed otherwise).
pub(crate) fn needs_preserve(s: &str) -> bool {
    let edge = |c: Option<char>| matches!(c, Some(' ' | '\t' | '\n' | '\r'));
    edge(s.chars().next()) || edge(s.chars().next_back())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns() {
        for (n, s) in [(1, "A"), (26, "Z"), (27, "AA"), (52, "AZ"), (53, "BA"), (702, "ZZ"), (703, "AAA"), (16_384, "XFD")] {
            assert_eq!(column_name(n), s);
        }
        assert_eq!(column_name(0), "");
    }

    #[test]
    fn numbers() {
        let t = |x: f64| {
            let mut s = String::new();
            push_number(&mut s, x);
            s
        };
        assert_eq!(t(0.0), "0");
        assert_eq!(t(-0.0), "0");
        assert_eq!(t(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(t(1e21), "1e+21");
        assert_eq!(t(-1.5e-7), "-1.5e-7");
        assert_eq!(t(123_456_789.0), "123456789");
    }

    #[test]
    fn xstring() {
        let t = |x: &str| {
            let mut s = String::new();
            push_xstring(&mut s, x);
            s
        };
        assert_eq!(t("a&b<c>d"), "a&amp;b&lt;c&gt;d");
        assert_eq!(t("x\u{1}y\u{1f}"), "x_x0001_y_x001F_");
        assert_eq!(t("_x0041_"), "_x005F_x0041_");
        assert_eq!(t("_x00G1_ _x12"), "_x00G1_ _x12");
        assert_eq!(t("\t\n\r"), "\t\n\r");
        assert_eq!(t("a\u{FFFE}b\u{FFFF}"), "a_xFFFE_b_xFFFF_");
        assert_eq!(t("ção 😀"), "ção 😀");
    }

    #[test]
    fn cut() {
        assert_eq!(cut_text("abc", 2), "ab");
        assert_eq!(cut_text("a😀b", 2), "a");
        assert_eq!(cut_text("a😀b", 3), "a😀");
        assert_eq!(cut_text("ção", 2), "çã");
    }
}
