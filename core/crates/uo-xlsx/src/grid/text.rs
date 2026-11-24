//! The text a cell DISPLAYS for the width of its column, as Excel computes it (what `Range.Text` returns), and the
//! horizontal layout of the pieces drawn: 1:1 port of `sheet/src/cell-text.ts` (rules measured in Excel 365 pt-BR with
//! synthetic workbooks; the header of the TS file lists them) plus the wrapped-line breaker of `cell-paint.ts`
//! (`wrapLines`), which the automatic row height needs.
//!
//! The viewport path allocates nothing per cell once the buffers are warm: [`layout_cell_text`] fills a reusable
//! [`CellText`]. The one allocation left per formatted cell is `uo_numfmt::format_value_parts`' own parts vector
//! (that crate is not part of this frontier; recorded as an open item).

use uo_jsnum::math;
use uo_numfmt::{FmtPart, FormatLocale, FormatOptions, Value, decimal_digits, format_general, format_value_parts};
use uo_text::utf16::{code_points, decode_at};

/// Characters Excel keeps in a cell.
pub const MAX_CELL_CHARS: usize = 32_767;

/// `ERRORS_PT_BR`: errors as Excel pt-BR shows them.
static ERRORS_PT_BR: &[(&str, &str)] = &[
    ("#NULL!", "#NULO!"),
    ("#DIV/0!", "#DIV/0!"),
    ("#VALUE!", "#VALOR!"),
    ("#REF!", "#REF!"),
    ("#NAME?", "#NOME?"),
    ("#NUM!", "#NÚM!"),
    ("#N/A", "#N/D"),
    ("#GETTING_DATA", "#OBTENDO_DADOS"),
    ("#SPILL!", "#DESPEJAR!"),
    ("#CALC!", "#CALC!"),
    ("#FIELD!", "#CAMPO!"),
    ("#BLOCKED!", "#BLOQUEADO!"),
    ("#UNKNOWN!", "#DESCONHECIDO!"),
    ("#CONNECT!", "#CONECTAR!"),
    ("#BUSY!", "#OCUPADO!"),
    ("#PYTHON!", "#PYTHON!"),
];

/// `localizeError`: a stored error value (always English in the file) → the pt-BR text shown. An unknown text
/// comes back unchanged.
#[must_use]
pub fn localize_error(v: &[u16]) -> Vec<u16> {
    let t = uo_text::space::trim(v);
    let up = uo_text::case::to_uppercase(t);
    for (k, pt) in ERRORS_PT_BR {
        if up.iter().copied().eq(k.encode_utf16()) {
            return pt.encode_utf16().collect();
        }
    }
    v.to_vec()
}

/// `localizeBool`: a boolean as pt-BR shows it.
#[must_use]
pub fn localize_bool(b: bool) -> &'static str {
    if b { "VERDADEIRO" } else { "FALSO" }
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

fn is_space(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

/// `looksLikeNumberPtBR`: the text Excel pt-BR flags as "number stored as text" (green triangle). Measured in lab
/// e30 (see the TS header). The TS regex is matched by hand with the same backtracking outcome.
#[must_use]
pub fn looks_like_number_pt_br(s: &[u16]) -> bool {
    let mut t: &[u16] = {
        let a = s.iter().take_while(|&&u| u == 32).count();
        let rest = s.get(a..).unwrap_or(&[]);
        let b = rest.iter().rev().take_while(|&&u| u == 32).count();
        rest.get(..rest.len() - b).unwrap_or(&[])
    };
    if t.len() > 2 && t.first() == Some(&40) && t.last() == Some(&41) {
        t = uo_text::space::trim(t.get(1..t.len() - 1).unwrap_or(&[]));
    }
    num_pt_br(t)
}

fn num_pt_br(t: &[u16]) -> bool {
    let n = t.len();
    let at = |i: usize| t.get(i).copied();
    let skip_sp = |mut i: usize| {
        while at(i).is_some_and(is_space) {
            i += 1;
        }
        i
    };
    let sign = |i: usize| if matches!(at(i), Some(43 | 45)) { i + 1 } else { i };
    // `R$` is two units, `€` one
    let cur = |i: usize| -> usize {
        if at(i) == Some(82) && at(i + 1) == Some(36) {
            i + 2
        } else if at(i) == Some(0x20AC) {
            i + 1
        } else {
            i
        }
    };
    let mut i = skip_sp(sign(0));
    i = skip_sp(cur(i));
    i = skip_sp(sign(i));
    // group 1: digits, or 1-3 digits and `.ddd` groups
    let run = |from: usize| t.get(from..).unwrap_or(&[]).iter().take_while(|&&u| is_digit(u)).count();
    let tail = |j: usize| -> Option<(usize, bool)> {
        // `(,\d*)?(?:[eE][+-]?\d+)?\s*%?\s*(?:R$|€)?$`; returns (end, group 2 has a digit)
        let mut j = j;
        let mut dec = false;
        if at(j) == Some(44) {
            j += 1;
            let r = run(j);
            dec = r > 0;
            j += r;
        }
        if matches!(at(j), Some(69 | 101)) {
            let k = sign(j + 1);
            let r = run(k);
            if r > 0 {
                j = k + r;
            }
        }
        j = skip_sp(j);
        if at(j) == Some(37) {
            j += 1;
        }
        j = skip_sp(j);
        j = cur(j);
        (j == n).then_some((j, dec))
    };
    let r = run(i);
    let mut candidates: [Option<usize>; 3] = [None; 3];
    candidates[0] = (r > 0).then_some(i + r);
    if (1..=3).contains(&r) && at(i + r) == Some(46) {
        let mut j = i + r;
        let mut groups = 0;
        while at(j) == Some(46) && run(j + 1) >= 3 && at(j + 4).is_none_or(|u| !is_digit(u)) {
            j += 4;
            groups += 1;
        }
        if groups > 0 {
            candidates[1] = Some(j);
        }
    }
    candidates[2] = Some(i);
    for (k, c) in candidates.iter().enumerate() {
        let Some(j) = *c else { continue };
        if let Some((_, dec)) = tail(j) {
            let has_g1 = k < 2;
            return has_g1 || dec;
        }
    }
    false
}

/// `isoToSerial`: a `t="d"` cell (ISO 8601: `2023-01-15`, `2023-01-15T10:30:00`, `10:30`) → the Excel serial
/// (`undefined` = not a date).
#[must_use]
pub fn iso_to_serial(v: Option<&[u16]>, date1904: bool) -> Option<f64> {
    let v = v.filter(|v| !v.is_empty())?;
    let t = uo_text::space::trim(v);
    let at = |i: usize| t.get(i).copied();
    let digits = |i: usize, n: usize| -> Option<f64> {
        let mut x = 0.0;
        for k in 0..n {
            let u = at(i + k)?;
            if !is_digit(u) {
                return None;
            }
            x = x * 10.0 + f64::from(u - 48);
        }
        Some(x)
    };
    let mut i = 0;
    let mut date: Option<(f64, f64, f64)> = None;
    if let (Some(y), Some(45), Some(m), Some(45), Some(d)) = (digits(0, 4), at(4), digits(5, 2), at(7), digits(8, 2)) {
        date = Some((y, m, d));
        i = 10;
    }
    // time: `T?(\d{2}):(\d{2})(?::(\d{2}(?:\.\d+)?))?`
    let mut time: Option<(f64, f64, f64)> = None;
    {
        let j = if at(i) == Some(84) { i + 1 } else { i };
        if let (Some(h), Some(58), Some(mi)) = (digits(j, 2), at(j + 2), digits(j + 3, 2)) {
            let mut sec = 0.0;
            if at(j + 5) == Some(58)
                && let Some(s0) = digits(j + 6, 2)
            {
                sec = s0;
                if at(j + 8) == Some(46) {
                    let mut k = j + 9;
                    while at(k).is_some_and(is_digit) {
                        k += 1;
                    }
                    if k > j + 9 {
                        let txt = String::from_utf16_lossy(t.get(j + 6..k).unwrap_or(&[]));
                        sec = uo_jsnum::parse::parse_float(&txt);
                    }
                }
            }
            time = Some((h, mi, sec));
        }
    }
    if date.is_none() && time.is_none() {
        return None;
    }
    let mut days = 0.0;
    if let Some((y, m, d)) = date {
        let ms = date_utc(y, m - 1.0, d);
        let base = if date1904 { date_utc(1904.0, 0.0, 1.0) } else { date_utc(1899.0, 11.0, 30.0) };
        days = (ms - base) / 86_400_000.0;
        if !date1904 && days < 61.0 {
            days -= 1.0;
        }
    }
    let secs = time.map_or(0.0, |(h, m, s)| h * 3600.0 + m * 60.0 + s);
    Some(days + secs / 86400.0)
}

/// Days from 1970-01-01 of a proleptic Gregorian date (month 1..=12).
fn days_from_civil(y: f64, m: f64, d: f64) -> f64 {
    let y = if m <= 2.0 { y - 1.0 } else { y };
    let era = math::floor(y / 400.0);
    let yoe = y - era * 400.0;
    let mp = if m > 2.0 { m - 3.0 } else { m + 9.0 };
    let doy = math::floor((153.0 * mp + 2.0) / 5.0) + d - 1.0;
    let doe = yoe * 365.0 + math::floor(yoe / 4.0) - math::floor(yoe / 100.0) + doy;
    era * 146_097.0 + doe - 719_468.0
}

/// `Date.UTC(y, m0, d)` in ms for the values of this module (years 0..99 are 1900+y, months and days overflow).
fn date_utc(y: f64, m0: f64, d: f64) -> f64 {
    let y = if (0.0..=99.0).contains(&y) { 1900.0 + y } else { y };
    let y2 = y + math::floor(m0 / 12.0);
    let m = m0 - math::floor(m0 / 12.0) * 12.0;
    (days_from_civil(y2, m + 1.0, 1.0) + (d - 1.0)) * 86_400_000.0
}

/// The metrics of ONE font as Excel measures it: integer px per character at the ppem (`FontMetrics`).
pub trait FontMetrics {
    /// Advance (integer px) of a code point (a lone surrogate is its own code point).
    fn adv(&self, cp: u32) -> i32;
    /// The font's ppem.
    fn ppem(&self) -> i32;
    /// Width of a string; stops summing once it passes `limit` (the partial sum is returned). Scripts that need
    /// shaping are measured as a whole by the font (`CellFont.width`); the default sums the advances.
    fn width(&self, s: &[u16], limit: f64) -> f64 {
        let mut w = 0.0;
        for cp in code_points(s) {
            w += f64::from(self.adv(cp));
            if w > limit {
                return w;
            }
        }
        w
    }
}

/// `cellMarginPx`: the margin (px) Excel takes off the column to fit/fill: `2·m + 1` with `m = max(2, ⌈ppem/8⌉)`.
#[must_use]
pub fn cell_margin_px(ppem: i32) -> f64 {
    2.0 * math::max(2.0, math::ceil(f64::from(ppem) / 8.0)) + 1.0
}

/// `autofitWrapWidthPx`: width (px) at which the AUTOMATIC row height wraps the text.
#[must_use]
pub fn autofit_wrap_width_px(col_px: f64, ppem: i32, mdw: i32) -> f64 {
    let k = cell_margin_px(ppem);
    let m = f64::from(mdw);
    math::min(col_px - k - 2.0, ((col_px - k + 1.0) * m) / (m + 0.25))
}

/// `textWidth`.
#[must_use]
pub fn text_width<M: FontMetrics + ?Sized>(m: &M, s: &[u16], limit: f64) -> f64 {
    m.width(s, limit)
}

/// Kind of the value (`CellKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    /// A number (or date).
    Number,
    /// Text.
    Text,
    /// A boolean already localized.
    Bool,
    /// An error already localized.
    Error,
}

/// `CellTextInput`.
#[derive(Clone, Copy, Debug)]
pub struct CellTextInput<'a> {
    /// Kind.
    pub kind: CellKind,
    /// Numeric value (kind `Number`).
    pub num: f64,
    /// Text (kind `Text`), or the localized boolean/error.
    pub str: &'a [u16],
    /// The cell's formatCode (`None` = General).
    pub code: Option<&'a [u16]>,
    /// 1904 date system.
    pub date1904: bool,
    /// `<sheetView showZeros="0">` is `false`.
    pub show_zeros: bool,
    /// Format locale (`None` = pt-BR).
    pub locale: Option<&'a FormatLocale>,
    /// Total indent (px): takes useful width from fit/fill, not from the count of `#`.
    pub indent_px: f64,
    /// px less everywhere (an explicit "right": 1, measured).
    pub edge_px: f64,
}

impl Default for CellTextInput<'_> {
    fn default() -> Self {
        CellTextInput {
            kind: CellKind::Text,
            num: 0.0,
            str: &[],
            code: None,
            date1904: false,
            show_zeros: true,
            locale: None,
            indent_px: 0.0,
            edge_px: 0.0,
        }
    }
}

/// A piece to draw: `len` units of [`CellText::buf`] from `start`, at `x` px from the start of the content.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextPiece {
    /// Start in the buffer.
    pub start: u32,
    /// Length in units.
    pub len: u32,
    /// x (px).
    pub x: f64,
}

/// `CellText`: the displayed text for a width plus the pieces to draw. Reused across cells (`clear`).
#[derive(Clone, Debug, Default)]
pub struct CellText {
    /// The text displayed for the width (= `Range.Text`).
    pub text: Vec<u16>,
    /// Colour asked by the format (`[Red]` …).
    pub color: Option<String>,
    /// Pieces to draw.
    pub pieces: Vec<TextPiece>,
    /// Text of the pieces.
    pub buf: Vec<u16>,
    /// Total drawn width (`-1` = not computed: plain text that may overflow).
    pub width: f64,
    /// Did not fit: `####`.
    pub hashes: bool,
    /// Has a `*x` fill: takes the whole column (alignment does not apply).
    pub filled: bool,
    /// Text that may overflow into empty neighbours.
    pub overflows: bool,
}

impl CellText {
    /// Empties it (capacity kept).
    pub fn clear(&mut self) {
        self.text.clear();
        self.color = None;
        self.pieces.clear();
        self.buf.clear();
        self.width = 0.0;
        self.hashes = false;
        self.filled = false;
        self.overflows = false;
    }

    /// The text of piece `i`.
    #[must_use]
    pub fn piece_text(&self, i: usize) -> &[u16] {
        self.pieces.get(i).and_then(|p| self.buf.get(p.start as usize..(p.start + p.len) as usize)).unwrap_or(&[])
    }

    fn push_piece(&mut self, s: &[u16], x: f64) {
        let start = u32::try_from(self.buf.len()).unwrap_or(u32::MAX);
        self.buf.extend_from_slice(s);
        self.pieces.push(TextPiece { start, len: u32::try_from(s.len()).unwrap_or(u32::MAX), x });
    }

    fn push_repeat(&mut self, ch: u16, n: usize, x: f64) {
        let start = u32::try_from(self.buf.len()).unwrap_or(u32::MAX);
        self.buf.resize(self.buf.len() + n, ch);
        self.pieces.push(TextPiece { start, len: u32::try_from(n).unwrap_or(u32::MAX), x });
    }
}

fn is_general_ci(code: &[u16]) -> bool {
    code.len() == 7 && code.iter().zip(b"general").all(|(&u, &g)| u == u16::from(g) || u == u16::from(g.to_ascii_uppercase()))
}

/// Layout of the displayed text and its horizontal pieces for a cell of `col_px` px (column or merge width), with
/// the widths of font `m` (`layoutCellText`). `out` is cleared first.
pub fn layout_cell_text<M: FontMetrics + ?Sized>(input: &CellTextInput<'_>, m: &M, col_px: f64, out: &mut CellText) {
    out.clear();
    // useful width: the count of `#` uses the whole column; fit/fill take the indent off
    let full = col_px - cell_margin_px(m.ppem()) - input.edge_px;
    let avail = full - input.indent_px;
    match input.kind {
        CellKind::Bool | CellKind::Error => {
            let s = input.str;
            if s.is_empty() {
                return;
            }
            let w = text_width(m, s, avail);
            if w > avail {
                hashes(m, full, false, out);
                return;
            }
            out.text.extend_from_slice(s);
            out.push_piece(s, 0.0);
            out.width = w;
        }
        CellKind::Text => {
            let s = input.str.get(..MAX_CELL_CHARS).unwrap_or(input.str);
            let general = input.code.is_none_or(|c| c.is_empty() || is_general_ci(c));
            if general {
                plain(s, m, None, None, out);
                return;
            }
            let opts = FormatOptions { locale: input.locale, col_width_chars: None, date1904: false, show_zeros: true };
            let r = format_value_parts(Value::Str(s), input.code, &opts);
            // text with `*x`: fits by the whole column (the indent only shrinks the fill; measured, e13)
            if r.parts.iter().any(|p| matches!(p, FmtPart::Fill(_))) {
                fit(&r.parts, r.color, m, avail, full, false, full, out);
                return;
            }
            if let [FmtPart::Text(t)] = r.parts.as_slice()
                && t.as_slice() == s
            {
                plain(s, m, r.color, None, out);
                return;
            }
            let mut joined: Vec<u16> = Vec::new();
            for p in &r.parts {
                match p {
                    FmtPart::Text(t) => joined.extend_from_slice(t),
                    FmtPart::Skip(_) => joined.push(32),
                    _ => {}
                }
            }
            plain(&joined, m, r.color, Some(&r.parts), out);
        }
        CellKind::Number => {
            let v = input.num;
            // General (no code, empty or `General`): the parts are one `General` number, so skip the format engine
            // (its parts vector); a whole number of up to 11 digits that fits is its digits (`formatGeneral`)
            if input.code.is_none_or(|c| c.is_empty() || is_general_ci(c)) && v.is_finite() {
                if v == 0.0 && !input.show_zeros {
                    return;
                }
                if general_integer(v, m, avail, out) {
                    return;
                }
            }
            let opts =
                FormatOptions { locale: input.locale, col_width_chars: None, date1904: input.date1904, show_zeros: input.show_zeros };
            let r = format_value_parts(Value::Num(v), input.code, &opts);
            let loc = input.locale.unwrap_or_else(|| uo_numfmt::pt_br());
            if r.overflow {
                hashes(m, full, false, out);
                out.color = r.color;
                return;
            }
            if r.parts.is_empty() {
                out.color = r.color;
                return;
            }
            if let [FmtPart::General(gv)] = r.parts.as_slice() {
                match general_fit(*gv, m, avail, loc) {
                    GeneralFit::Hashes => {
                        hashes(m, full, true, out);
                        out.color = r.color;
                    }
                    GeneralFit::Fallback(s) => {
                        // no slot at all: Excel still shows the rounded digit (without checking the width)
                        let w = text_width(m, &s, f64::INFINITY);
                        out.color = r.color;
                        out.text.extend_from_slice(&s);
                        out.push_piece(&s, 0.0);
                        out.width = w;
                    }
                    GeneralFit::Text(s) => {
                        let parts = [FmtPart::Text(s)];
                        fit(&parts, r.color, m, avail, full, true, avail, out);
                    }
                }
                return;
            }
            let parts: Vec<FmtPart> = r
                .parts
                .into_iter()
                .map(|p| match p {
                    FmtPart::General(x) => FmtPart::Text(general_text(x, loc, None)),
                    other => other,
                })
                .collect();
            fit(&parts, r.color, m, avail, full, false, avail, out);
        }
    }
}

/// The General text of a whole number of at most 11 digits when it fits the column: `out` gets the digits as the
/// only piece. `false` = not this case (sign/width/overflow decisions stay with the general path).
fn general_integer<M: FontMetrics + ?Sized>(v: f64, m: &M, avail: f64, out: &mut CellText) -> bool {
    let a = math::abs(v);
    if a.fract() != 0.0 || a > 99_999_999_999.0 {
        return false;
    }
    let n = integer_digits_value(a);
    let mut digits = [0u16; 12];
    let mut len = 0usize;
    let mut x = n;
    loop {
        digits[11 - len] = 48 + (x % 10) as u16;
        len += 1;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    let neg = v < 0.0;
    let sign_w = if neg { f64::from(m.adv(45)) } else { 0.0 };
    let zero_w = math::max(1.0, f64::from(m.adv(48)));
    let slots = math::floor((avail - sign_w) / zero_w);
    if (len as f64) > slots {
        return false;
    }
    let body = &digits[12 - len..];
    let mut w = 0.0;
    if neg {
        w += sign_w;
    }
    for &d in body {
        w += f64::from(m.adv(u32::from(d)));
    }
    if w > avail {
        return false;
    }
    if neg {
        out.text.push(45);
    }
    out.text.extend_from_slice(body);
    let start = out.buf.len() as u32;
    if neg {
        out.buf.push(45);
    }
    out.buf.extend_from_slice(body);
    out.pieces.push(TextPiece { start, len: out.buf.len() as u32 - start, x: 0.0 });
    out.width = w;
    true
}

/// The value of a whole number below 2^53 (the digits loop wants an integer).
fn integer_digits_value(a: f64) -> u64 {
    // the scaled halves keep every step inside i32 (to_int32 wraps; the parts are below 2^31)
    let hi = math::floor(a / 4_294_967_296.0);
    let lo = a - hi * 4_294_967_296.0;
    (uo_jsnum::conv::to_uint32(hi) as u64) << 32 | u64::from(uo_jsnum::conv::to_uint32(lo))
}

fn general_text(v: f64, loc: &FormatLocale, budget: Option<i32>) -> Vec<u16> {
    format_general(v, loc, budget.unwrap_or(uo_numfmt::DEFAULT_GENERAL_BUDGET)).encode_utf16().collect()
}

/// Plain text (overflows): one piece, `_x` drawn with the width of x.
fn plain<M: FontMetrics + ?Sized>(s: &[u16], m: &M, color: Option<String>, parts: Option<&[FmtPart]>, out: &mut CellText) {
    if s.is_empty() {
        out.color = color;
        return;
    }
    out.color = color;
    out.text.extend_from_slice(s);
    let Some(parts) = parts else {
        out.push_piece(s, 0.0);
        out.width = -1.0;
        out.overflows = true;
        return;
    };
    let mut x = 0.0;
    for p in parts {
        match p {
            FmtPart::Text(t) => {
                out.push_piece(t, x);
                x += text_width(m, t, f64::INFINITY);
            }
            FmtPart::Skip(ch) => x += f64::from(m.adv(u32::from(*ch))),
            _ => {}
        }
    }
    out.width = x;
    out.overflows = true;
}

/// The `####` that fit in the column (General: at least 1).
fn hashes<M: FontMetrics + ?Sized>(m: &M, avail: f64, general: bool, out: &mut CellText) {
    let a = m.adv(35);
    let hw = if a == 0 { 1.0 } else { f64::from(a) };
    let mut n = math::floor(avail / hw);
    if n <= 0.0 || n.is_nan() {
        n = if general { 1.0 } else { 0.0 };
    }
    // a column cannot hold more than a few hundred `#`; the cap keeps a hostile width from allocating
    let count = if n.is_nan() { 0 } else { uo_jsnum::conv::to_int32(math::min(n, 65_536.0)).max(0) as usize };
    out.text.clear();
    out.text.resize(count, 35);
    out.pieces.clear();
    out.buf.clear();
    if count > 0 {
        out.push_repeat(35, count, 0.0);
    }
    out.width = n * hw;
    out.hashes = true;
    out.filled = false;
    out.overflows = false;
}

/// A number/text with parts: checks that it fits (`_x` = a space) and expands the `*x`.
#[allow(clippy::too_many_arguments)]
fn fit<M: FontMetrics + ?Sized>(
    parts: &[FmtPart],
    color: Option<String>,
    m: &M,
    avail: f64,
    full: f64,
    general: bool,
    fit_avail: f64,
    out: &mut CellText,
) {
    let mut range_w = 0.0;
    let mut paint_w = 0.0;
    let mut fill_ch: Option<u16> = None;
    for p in parts {
        match p {
            FmtPart::Text(t) => {
                let w = text_width(m, t, f64::INFINITY);
                range_w += w;
                paint_w += w;
            }
            FmtPart::Skip(ch) => {
                range_w += f64::from(m.adv(32));
                paint_w += f64::from(m.adv(u32::from(*ch)));
            }
            FmtPart::Fill(ch) => fill_ch = Some(*ch),
            FmtPart::General(_) => {}
        }
    }
    if range_w > fit_avail {
        hashes(m, full, general, out);
        out.color = color;
        return;
    }
    let fw = fill_ch.map_or(1.0, |c| math::max(1.0, f64::from(m.adv(u32::from(c)))));
    // the formatted text of Excel has at most 255 characters (measured: `@*.` in a 1,500 px column stops at 255)
    let mut fixed_len = 0usize;
    for p in parts {
        fixed_len += match p {
            FmtPart::Text(t) => t.len(),
            FmtPart::Skip(_) => 1,
            _ => 0,
        };
    }
    let cap = 255usize.saturating_sub(fixed_len) as f64;
    let n_of = |used: f64| -> usize {
        if fill_ch.is_none() {
            return 0;
        }
        let n = math::min(cap, math::max(0.0, math::floor((avail - used) / fw)));
        uo_jsnum::conv::to_int32(n).max(0) as usize
    };
    let n_text = n_of(range_w);
    let n_paint = n_of(paint_w);
    let mut x = 0.0;
    for p in parts {
        match p {
            FmtPart::Text(t) => {
                out.text.extend_from_slice(t);
                out.push_piece(t, x);
                x += text_width(m, t, f64::INFINITY);
            }
            FmtPart::Skip(ch) => {
                out.text.push(32);
                x += f64::from(m.adv(u32::from(*ch)));
            }
            FmtPart::Fill(ch) => {
                out.text.resize(out.text.len() + n_text, *ch);
                if n_paint > 0 {
                    out.push_repeat(*ch, n_paint, x);
                    x += n_paint as f64 * fw;
                }
            }
            FmtPart::General(_) => {}
        }
    }
    out.color = color;
    out.width = x;
    out.hashes = false;
    out.filled = fill_ch.is_some();
    out.overflows = false;
}

enum GeneralFit {
    Hashes,
    Fallback(Vec<u16>),
    Text(Vec<u16>),
}

/// General in a column of `avail` px.
fn general_fit<M: FontMetrics + ?Sized>(v: f64, m: &M, avail: f64, loc: &FormatLocale) -> GeneralFit {
    let sign_w = if v < 0.0 { f64::from(m.adv(45)) } else { 0.0 };
    let zero_w = math::max(1.0, f64::from(m.adv(48)));
    let slots = math::floor((avail - sign_w) / zero_w);
    let full: Vec<u16> = general_text(v, loc, None);
    let body_len = if v < 0.0 { full.len().saturating_sub(1) } else { full.len() };
    let s: Vec<u16>;
    if (body_len as f64) <= slots {
        s = full;
    } else if slots < 1.0 || slots.is_nan() {
        // no slot: a positive ≥ 0.0001 that rounds to one digit ("7", "1", "0")
        if v >= 1e-4 {
            let (i, _) = decimal_digits(v, 0);
            let digits = if i.is_empty() { "0".to_owned() } else { i };
            if digits.len() == 1 {
                return GeneralFit::Fallback(digits.encode_utf16().collect());
            }
        }
        return GeneralFit::Hashes;
    } else {
        s = general_text(v, loc, Some(uo_jsnum::conv::to_int32(slots)));
        if s.is_empty() {
            return GeneralFit::Hashes;
        }
    }
    if text_width(m, &s, avail) > avail {
        return GeneralFit::Hashes;
    }
    GeneralFit::Text(s)
}

/// Tokens of `para.match(/[^\s-]*-+|[^\s-]+|\s+/g)`: `(start, end)` unit ranges.
fn tokens(para: &[u16], mut f: impl FnMut(usize, usize)) {
    let n = para.len();
    let mut i = 0;
    while i < n {
        let u = para[i];
        if is_space(u) {
            let mut j = i + 1;
            while j < n && is_space(para[j]) {
                j += 1;
            }
            f(i, j);
            i = j;
            continue;
        }
        let mut j = i;
        while j < n && !is_space(para[j]) && para[j] != 45 {
            j += 1;
        }
        if j < n && para[j] == 45 {
            while j < n && para[j] == 45 {
                j += 1;
            }
        }
        f(i, j);
        i = j;
    }
}

/// `wrapLines`: Excel's wrapping — at spaces (and after `-`); a word longer than the line breaks by character; `\n`
/// forces. Lines are `(start, end)` unit ranges of `text` (each line of the TS is a contiguous slice, trailing
/// white space trimmed). `out` is cleared first; stops at `max_lines`.
pub fn wrap_lines<M: FontMetrics + ?Sized>(f: &M, text: &[u16], max_w: f64, max_lines: usize, out: &mut Vec<(u32, u32)>) {
    out.clear();
    let to_u32 = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
    let trim_end = |s: usize, e: usize| -> usize {
        let mut e = e;
        while e > s && text.get(e - 1).copied().is_some_and(is_space) {
            e -= 1;
        }
        e
    };
    let mut pos = 0usize;
    loop {
        // the paragraph [pos, nl)
        let nl = text.get(pos..).and_then(|t| t.iter().position(|&u| u == 10)).map_or(text.len(), |k| pos + k);
        if out.len() >= max_lines {
            break;
        }
        let para = text.get(pos..nl).unwrap_or(&[]);
        if para.is_empty() {
            out.push((to_u32(pos), to_u32(pos)));
        } else {
            let mut line_start = pos;
            let mut line_end = pos; // exclusive; == line_start means ''
            let mut line_w = 0.0;
            let mut stop = false;
            tokens(para, |a, b| {
                if stop {
                    return;
                }
                let (ta, tb) = (pos + a, pos + b);
                let tok = text.get(ta..tb).unwrap_or(&[]);
                let tw = f.width(tok, f64::INFINITY);
                if tok.first().copied().is_some_and(is_space) {
                    if line_end == line_start {
                        line_start = ta;
                    }
                    line_end = tb;
                    line_w += tw;
                    return;
                }
                if line_end > line_start && line_w + tw > max_w {
                    out.push((to_u32(line_start), to_u32(trim_end(line_start, line_end))));
                    if out.len() >= max_lines {
                        stop = true;
                        return;
                    }
                    line_start = ta;
                    line_end = ta;
                    line_w = 0.0;
                }
                if tw > max_w {
                    // a word longer than the line: break by character
                    let mut k = ta;
                    while let Some((cp, w)) = decode_at(text.get(..tb).unwrap_or(&[]), k) {
                        if k >= tb {
                            break;
                        }
                        let cw = f64::from(f.adv(cp));
                        if line_end > line_start && line_w + cw > max_w {
                            out.push((to_u32(line_start), to_u32(line_end)));
                            if out.len() >= max_lines {
                                stop = true;
                                return;
                            }
                            line_start = k;
                            line_end = k;
                            line_w = 0.0;
                        }
                        if line_end == line_start {
                            line_start = k;
                        }
                        line_end = k + w;
                        line_w += cw;
                        k += w;
                    }
                    return;
                }
                if line_end == line_start {
                    line_start = ta;
                }
                line_end = tb;
                line_w += tw;
            });
            if stop {
                break;
            }
            out.push((to_u32(line_start), to_u32(trim_end(line_start, line_end))));
        }
        if nl >= text.len() {
            break;
        }
        pos = nl + 1;
    }
    if out.is_empty() {
        out.push((0, 0));
    }
}
