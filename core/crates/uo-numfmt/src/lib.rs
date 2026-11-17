//! Excel number/date format engine of the UniversalOffice Rust core: a `numFmt` formatCode
//! (`#,##0.00`, `dd/mm/yyyy`, `[Red]0%;[Blue]-0%`, `# ?/?`, `[h]:mm:ss`, `@`...) applied to a number
//! or a text, giving the text Excel shows (and, for whoever knows the column width, the PARTS of it).
//! The 1:1 port of `shared/charts/src/numfmt.ts`: same algorithm, same floating-point operation
//! order, same results (record: `core/docs/modules/uo-numfmt.md`).
//!
//! - [`format_value`] / [`format_value_parts`]: the entry points (`formatValue`, `formatValueParts`).
//! - [`format_general`], [`decimal_digits`]: the General format and the 15-significant-digit rounding.
//! - [`FormatLocale`], [`pt_br`], [`builtin_format_code`], [`builtin_format`]: locales and numFmtIds.
//! - [`serial_to_date_ms`], [`utc_fields`]: the OLE serial as a `Date` (UTC).
//!
//! Strings are UTF-16 code units (`&[u16]`, [`FmtPart::Text`]): the TS indexes by code unit, and a
//! lone surrogate (for instance the high half after `_` or `*`) is a value it carries. A parsed
//! code is cached per thread ([`clear_caches`], capped as documented on the constants of
//! `parse.rs`); nothing else is stateful. No panics, no `unsafe`.
#![forbid(unsafe_code)]

mod date;
mod digits;
mod emit;
mod locale;
mod parse;

pub use date::{UtcFields, serial_to_date_ms, utc_fields};
pub use digits::{DEFAULT_GENERAL_BUDGET, MAX_GENERAL_BUDGET, decimal_digits, format_general};
pub use emit::FmtPart;
pub use locale::{FormatLocale, builtin_format, builtin_format_code, pt_br};
pub use parse::{cache_len, clear_caches};

use parse::{CondOp, Kind, Parsed, U, is_general, parse, u};
use uo_jsnum::math;

/// A value to format: a number (numeric or date) or a text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<'a> {
    Num(f64),
    Str(&'a [u16]),
}

/// `FormatOptions`.
#[derive(Clone, Copy, Debug)]
pub struct FormatOptions<'a> {
    /// `None` = [`pt_br()`].
    pub locale: Option<&'a FormatLocale>,
    /// Column width in characters: `*x` repeats x to fill it (without it, once). `None`, 0 and NaN are "unknown".
    pub col_width_chars: Option<f64>,
    /// 1904 date system (legacy Mac): epoch 1904-01-01 (serial + 1462).
    pub date1904: bool,
    /// `<sheetView showZeros="0">` is `show_zeros: false`: a zero without its own zero section comes out empty.
    pub show_zeros: bool,
}

impl Default for FormatOptions<'_> {
    fn default() -> Self {
        FormatOptions { locale: None, col_width_chars: None, date1904: false, show_zeros: true }
    }
}

/// `FormatParts`.
#[derive(Clone, Debug, PartialEq)]
pub struct FormatParts {
    pub parts: Vec<FmtPart>,
    /// Colour asked by `[Red]`/`[Color12]`... as `#RRGGBB`.
    pub color: Option<String>,
    /// No applicable section / date out of range: Excel shows "####".
    pub overflow: bool,
}

/// `FormatResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct FormatResult {
    /// The displayed text, UTF-16 as the TS string (a lone surrogate stays).
    pub text: Vec<u16>,
    pub color: Option<String>,
}

impl FormatResult {
    /// The text as a Rust string (a lone surrogate becomes U+FFFD).
    #[must_use]
    pub fn text_lossy(&self) -> String {
        String::from_utf16_lossy(&self.text)
    }
}

/// Most `*x` repetitions `format_value` writes. The TS `repeat(count)` throws a `RangeError` past
/// the V8 string limit (about 5e8 units; also for an infinite width); a column is at most 255
/// characters in Excel.
pub const MAX_FILL_REPS: usize = 65_536;

fn cond_test(op: CondOp, v: f64, x: f64) -> bool {
    match op {
        CondOp::Lt => x < v,
        CondOp::Gt => x > v,
        CondOp::Eq => x == v,
        CondOp::Le => x <= v,
        CondOp::Ge => x >= v,
        CondOp::Ne => x != v,
    }
}

/// A condition that only accepts negatives: Excel omits the "-" ([<0], [<-1], [<=-1], [=-5]; not [<=0]).
fn neg_only(op: CondOp, v: f64) -> bool {
    (op == CondOp::Lt && v <= 0.0) || (op == CondOp::Le && v < 0.0) || (op == CondOp::Eq && v < 0.0)
}

struct Pick {
    sec: Option<usize>,
    abs: bool,
    overflow: bool,
    general: Option<f64>,
}

/// Chooses the section and whether the value comes out unsigned (a "negative" section). Measured in Excel pt-BR.
fn pick_section(p: &Parsed, x: f64) -> Pick {
    let n = &p.nums;
    let sec_of = |k: usize| n.get(k).copied();
    let none = |abs: bool, general: Option<f64>, overflow: bool| Pick { sec: None, abs, overflow, general };
    let some = |k: usize, abs: bool| Pick { sec: sec_of(k), abs, overflow: false, general: None };
    if n.is_empty() {
        return none(false, Some(x), false); // only a text section: the number in General
    }
    if !p.has_cond {
        if n.len() == 1 {
            return some(0, false);
        }
        if n.len() == 2 {
            return if x < 0.0 { some(1, true) } else { some(0, false) };
        }
        return if x > 0.0 {
            some(0, false)
        } else if x < 0.0 {
            some(1, true)
        } else {
            some(2, false)
        };
    }
    let cond_of = |k: usize| sec_of(k).and_then(|i| p.secs.get(i)).and_then(|s| s.cond);
    // the 1st section without a condition (only the 2nd has one) accepts the positives
    let (c0op, c0v) = cond_of(0).map_or((CondOp::Gt, 0.0), |c| (c.op, c.v));
    if cond_test(c0op, c0v, x) {
        return some(0, cond_of(0).is_some() && neg_only(c0op, c0v));
    }
    // implicit "else" after a 1st condition: shows the sign after [=t] and after [>t]/[>=t] with t > 0
    let else_sign = cond_of(0).is_some() && (c0op == CondOp::Eq || ((c0op == CondOp::Gt || c0op == CondOp::Ge) && c0v > 0.0));
    if n.len() == 1 {
        return none(!else_sign, Some(if else_sign { x } else { x.abs() }), false);
    }
    let c1 = cond_of(1);
    let take_second = match c1 {
        Some(c) => cond_test(c.op, c.v, x),
        None => n.len() == 2 || x < 0.0,
    };
    if take_second {
        let abs = match c1 {
            Some(c) => neg_only(c.op, c.v),
            None => n.len() >= 3 || !else_sign,
        };
        return some(1, abs);
    }
    if n.len() >= 3 {
        return some(2, false);
    }
    none(false, None, true)
}

/// Formats in PARTS (text, `_x`, `*x`, General), for whoever knows the column width (Sheet).
/// `code` `None` or empty is General.
#[must_use]
pub fn format_value_parts(value: Value<'_>, code: Option<&[u16]>, opts: &FormatOptions<'_>) -> FormatParts {
    let loc = opts.locale.unwrap_or_else(|| pt_br());
    let code = code.filter(|c| !c.is_empty());
    let plain = |parts| FormatParts { parts, color: None, overflow: false };
    let overflow = |color: Option<String>| FormatParts { parts: Vec::new(), color, overflow: true };
    let s = match value {
        Value::Str(s) => {
            let one = || plain(vec![FmtPart::Text(s.to_vec())]);
            let Some(code) = code else { return one() };
            if is_general(code) {
                return one();
            }
            let p = parse(code);
            let Some(t) = p.text.and_then(|i| p.secs.get(i)) else { return one() };
            return FormatParts { parts: emit::emit_text(t, s), color: t.color.clone(), overflow: false };
        }
        Value::Num(x) => x,
    };
    let x = s;
    if !x.is_finite() {
        return overflow(None);
    }
    let Some(code) = code.filter(|c| !is_general(c)) else {
        if x == 0.0 && !opts.show_zeros {
            return plain(Vec::new());
        }
        return plain(vec![FmtPart::General(x)]);
    };
    let p = parse(code);
    let pick = pick_section(&p, x);
    if !opts.show_zeros && x == 0.0 && !(p.nums.len() >= 3 && pick.sec.is_some() && pick.sec == p.nums.get(2).copied()) {
        return plain(Vec::new());
    }
    let Some(sec) = pick.sec.and_then(|i| p.secs.get(i)) else {
        if pick.overflow {
            return overflow(None);
        }
        return plain(vec![FmtPart::General(pick.general.unwrap_or(x))]);
    };
    let v = if pick.abs { x.abs() } else { x };
    let neg = v < 0.0;
    let mut out: Vec<FmtPart> = Vec::new();
    let mut over = false;
    let mut nonzero = false;
    match sec.kind {
        Kind::Empty => {}
        Kind::Text => out.push(FmtPart::General(v)), // a number with a text format: General
        Kind::Gen => emit::emit_general_section(sec, v, &mut out),
        Kind::Lit => emit::emit_literals(sec, &mut out),
        Kind::Date => match date::emit_date(sec, v.abs(), loc, opts.date1904, neg) {
            Some(r) => out.extend(r),
            None => over = true,
        },
        Kind::Num => nonzero = emit::emit_number(sec, v.abs(), loc, &mut out),
    }
    if over {
        return overflow(sec.color.clone());
    }
    // "-" at the start of the whole section (before the literals); gone when the displayed value rounded to zero
    if neg && sec.kind == Kind::Num && nonzero {
        out.insert(0, FmtPart::Text(u("-")));
    }
    // a single section of only literals also gets the "-"
    if neg && (sec.kind == Kind::Lit || sec.kind == Kind::Empty) {
        out.insert(0, FmtPart::Text(u("-")));
    }
    FormatParts { parts: emit::merge_parts(emit::last_fill_only(out)), color: sec.color.clone(), overflow: false }
}

/// Formats (text + colour). `*x` repeats x up to `col_width_chars` (without it, once) and `_x`
/// becomes one space, like the Range.Text of a column of unknown width.
#[must_use]
pub fn format_value(value: Value<'_>, code: Option<&[u16]>, opts: &FormatOptions<'_>) -> FormatResult {
    let loc = opts.locale.unwrap_or_else(|| pt_br());
    let r = format_value_parts(value, code, opts);
    if r.overflow {
        return FormatResult { text: u("#"), color: r.color };
    }
    let mut out: U = Vec::new();
    let mut fill: Option<(usize, u16)> = None;
    for p in &r.parts {
        match p {
            FmtPart::Text(s) => out.extend_from_slice(s),
            FmtPart::Skip(_) => out.push(u16::from(b' ')),
            FmtPart::General(v) => out.extend(u(&format_general(*v, loc, DEFAULT_GENERAL_BUDGET))),
            FmtPart::Fill(c) => fill = Some((out.len(), *c)),
        }
    }
    if let Some((at, ch)) = fill {
        let reps = match opts.col_width_chars {
            Some(w) if uo_jsnum::conv::truthy(w) => math::max(0.0, w - out.len() as f64),
            _ => 1.0,
        };
        let reps = uo_jsnum::conv::js_index(math::trunc(math::min(reps, MAX_FILL_REPS as f64)), MAX_FILL_REPS + 1).unwrap_or(0);
        let tail = out.split_off(at.min(out.len()));
        out.extend(std::iter::repeat_n(ch, reps));
        out.extend(tail);
    }
    FormatResult { text: out, color: r.color }
}
