//! `functions/regional.ts`: the Thai and CJK functions (THAIDIGIT, THAIYEAR, THAIDAYOFWEEK, THAIMONTHOFYEAR,
//! THAINUMSOUND, THAINUMSTRING, THAISTRINGLENGTH, ISTHAIDIGIT, NUMBERSTRING, DATESTRING). Outside their languages
//! the THAI* answer `#VALUE!` and NUMBERSTRING/DATESTRING `#N/A` (measured in Excel pt-BR); inside them the TS
//! follows the documentation.

use super::js::{Buf, chars};
use crate::dates::{serial_to_ymd, split_serial, weekday};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::functions::shared::tdl::{num, txt};
use crate::functions::shared::{R, Stop, bodies, boolean, scalar};
use crate::jstext::with_u16;
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `regional.ts`.
pub(super) const REGIONAL: &[(&str, FnBody)] = bodies![
    "THAIDIGIT" => thaidigit,
    "THAIYEAR" => thaiyear,
    "THAIDAYOFWEEK" => thaidayofweek,
    "THAIMONTHOFYEAR" => thaimonthofyear,
    "THAINUMSOUND" => thainum,
    "THAINUMSTRING" => thainum,
    "THAISTRINGLENGTH" => thaistringlength,
    "ISTHAIDIGIT" => isthaidigit,
    "NUMBERSTRING" => numberstring,
    "DATESTRING" => datestring,
];

/// `/^<lang>\b/i.test(id)` for an ASCII language code (`\b`: the next character is not `[A-Za-z0-9_]`).
pub(super) fn lang_is(id: &str, lang: &str) -> bool {
    id.len() >= lang.len()
        && id.as_bytes()[..lang.len()].eq_ignore_ascii_case(lang.as_bytes())
        && !id.as_bytes().get(lang.len()).is_some_and(|&b| uo_jsnum::text::is_js_word(b))
}

fn is_thai(ctx: &dyn FnContext) -> bool {
    lang_is(&ctx.locale().id, "th")
}

fn is_cjk(ctx: &dyn FnContext) -> bool {
    let id = &ctx.locale().id;
    lang_is(id, "zh") || lang_is(id, "ja")
}

const TH_DIGITS: [char; 10] = ['๐', '๑', '๒', '๓', '๔', '๕', '๖', '๗', '๘', '๙'];
const TH_DAYS: [&str; 7] = ["อาทิตย์", "จันทร์", "อังคาร", "พุธ", "พฤหัสบดี", "ศุกร์", "เสาร์"];
const TH_MONTHS: [&str; 12] =
    ["มกราคม", "กุมภาพันธ์", "มีนาคม", "เมษายน", "พฤษภาคม", "มิถุนายน", "กรกฎาคม", "สิงหาคม", "กันยายน", "ตุลาคม", "พฤศจิกายน", "ธันวาคม"];
const TH_NUM: [&str; 10] = ["ศูนย์", "หนึ่ง", "สอง", "สาม", "สี่", "ห้า", "หก", "เจ็ด", "แปด", "เก้า"];
const TH_UNIT: [&str; 6] = ["", "สิบ", "ร้อย", "พัน", "หมื่น", "แสน"];

/// The decimal digits of a non-negative integer below 10^6 (`String(x).split('').map(Number)`).
fn digits6(x: f64) -> Vec<usize> {
    uo_jsnum::format::to_js_string(x).bytes().map(|b| usize::from(b.wrapping_sub(b'0')).min(9)).collect()
}

/// The groups of 10^6 of a non-negative integer, most significant first (`unshift(x % 1e6); x = floor(x / 1e6)`).
/// The TS loops for ever on an infinite value (`∞ % 1e6` is NaN, `∞ / 1e6` stays ∞): bounded here (A4d-3).
pub(super) fn million_groups(mut x: f64) -> Result<Vec<f64>, Throw> {
    let mut groups = Vec::new();
    while x > 0.0 {
        if !x.is_finite() {
            return Err(Throw::Range);
        }
        groups.push(uo_jsnum::math::rem(x, 1_000_000.0));
        x = uo_jsnum::math::floor(x / 1_000_000.0);
    }
    groups.reverse();
    Ok(groups)
}

/// The Thai words of an integer below 10^6 (`below(x, higher)` / `thaiBelowMillion`), with `zero` the digit table.
pub(super) fn thai_below(out: &mut String, x: f64, higher: bool, digit: &[&str; 10]) {
    let ds = digits6(x);
    for (i, &d) in ds.iter().enumerate() {
        let pos = ds.len() - 1 - i;
        if d == 0 {
            continue;
        }
        if pos == 1 {
            match d {
                1 => out.push_str("สิบ"),
                2 => out.push_str("ยี่สิบ"),
                _ => {
                    out.push_str(digit[d]);
                    out.push_str("สิบ");
                }
            }
        } else if pos == 0 {
            out.push_str(if d == 1 && (ds.len() > 1 || higher) { "เอ็ด" } else { digit[d] });
        } else {
            out.push_str(digit[d]);
            out.push_str(TH_UNIT.get(pos).copied().unwrap_or(""));
        }
    }
}

/// `thaiWords(n)` (n ≥ 0).
fn thai_words(n: f64) -> Result<String, Throw> {
    if n == 0.0 {
        return Ok(TH_NUM[0].to_owned());
    }
    let groups = million_groups(uo_jsnum::math::floor(n))?;
    let mut out = String::new();
    for (i, &g) in groups.iter().enumerate() {
        thai_below(&mut out, g, i > 0, &TH_NUM);
        if i + 1 < groups.len() {
            out.push_str("ล้าน");
        }
    }
    Ok(out)
}

fn put(ctx: &mut dyn FnContext, s: &str) -> R<Value> {
    let u: Vec<u16> = s.encode_utf16().collect();
    crate::functions::shared::text(ctx, crate::strings::JsStr::Utf16(&u))
}

/// `dateOf(v, ctx)`: (y, m, d, weekday) of the rounded serial; `#NUM!` out of range.
fn date_of(ctx: &dyn FnContext, v: Scalar) -> Result<(f64, f64, f64, f64), ErrorCode> {
    let n = num(ctx, v)?;
    let (day, _) = split_serial(n, ctx.date1904()).ok_or(ErrorCode::Num)?;
    let ymd = serial_to_ymd(day, ctx.date1904());
    Ok((ymd.y, ymd.m, ymd.d, weekday(day, ctx.date1904())))
}

fn arg0(a: &CallArgs<'_>) -> Scalar {
    scalar(a, 0).unwrap_or(Scalar::Empty)
}

fn thaidigit(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let mut out = Buf::new();
    {
        let t = txt(ctx, arg0(a))?;
        with_u16(t.as_js(), |s| {
            for &c in s {
                if (0x30..=0x39).contains(&c) {
                    out.push(TH_DIGITS[usize::from(c - 0x30)] as u16);
                } else {
                    out.push(c);
                }
            }
        });
    }
    crate::functions::shared::text(ctx, crate::strings::JsStr::Utf16(out.as_slice()))
}

fn thaiyear(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let (y, ..) = date_of(ctx, arg0(a))?;
    Ok(crate::functions::shared::num(y + 543.0))
}

fn thaidayofweek(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let (.., wd) = date_of(ctx, arg0(a))?;
    put(ctx, TH_DAYS[crate::cast::trunc_usize(wd).min(6)])
}

fn thaimonthofyear(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let (_, m, ..) = date_of(ctx, arg0(a))?;
    put(ctx, TH_MONTHS[crate::cast::trunc_usize(m - 1.0).min(11)])
}

fn thainum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let n = num(ctx, arg0(a))?;
    let mut s = String::new();
    if n < 0.0 {
        s.push_str("ลบ");
    }
    s.push_str(&thai_words(n.abs())?);
    put(ctx, &s)
}

/// `/[ัิ-ฺ็-๎]/`: the Thai combining marks THAISTRINGLENGTH does not count.
fn thai_mark(c: u32) -> bool {
    c == 0x0E31 || (0x0E34..=0x0E3A).contains(&c) || (0x0E47..=0x0E4E).contains(&c)
}

fn thaistringlength(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let t = txt(ctx, arg0(a))?;
    let n = with_u16(t.as_js(), |s| chars(s).filter(|&(cp, ..)| !thai_mark(cp)).count());
    Ok(crate::functions::shared::num(n as f64))
}

fn isthaidigit(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if !is_thai(ctx) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    let t = txt(ctx, arg0(a))?;
    let r = with_u16(t.as_js(), |s| !s.is_empty() && chars(s).all(|(cp, ..)| (0x0E50..=0x0E59).contains(&cp)));
    Ok(boolean(r))
}

const CJK_DIGITS: [&str; 10] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
const CJK_FIN: [&str; 10] = ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"];
const CJK_PLAIN: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

/// `cjkNumber(n, type)`.
fn cjk_number(n: f64, ty: f64) -> String {
    let s = uo_jsnum::format::to_js_string(uo_jsnum::math::trunc(n.abs()));
    let digit = |c: u8| usize::from(c.wrapping_sub(b'0'));
    if ty == 3.0 {
        // `CJK_DIGITS[+d]` of each character of `String(…)` ("Infinity", "1e+21": `undefined` → "")
        return s.bytes().map(|b| CJK_DIGITS.get(digit(b)).copied().unwrap_or("")).collect();
    }
    let digits = if ty == 2.0 { &CJK_FIN } else { &CJK_PLAIN };
    let units: [&str; 4] = if ty == 2.0 { ["", "拾", "佰", "仟"] } else { ["", "十", "百", "千"] };
    let big = ["", "万", "亿", "兆"];
    let b = s.as_bytes();
    let mut groups: Vec<&[u8]> = Vec::new();
    let mut i = b.len();
    while i > 0 {
        groups.insert(0, &b[i.saturating_sub(4)..i]);
        i = i.saturating_sub(4);
    }
    let mut out = String::new();
    let ng = groups.len();
    for (gi, g) in groups.iter().enumerate() {
        let mut part = String::new();
        let mut zero = false;
        for (k, &d) in g.iter().enumerate() {
            let u = g.len() - 1 - k;
            if d == b'0' {
                zero = !part.is_empty();
                continue;
            }
            if zero {
                part.push_str(digits[0]);
                zero = false;
            }
            // `digits[+d]` / `units[u]`: a non-digit (`Infinity`, `e+21`) reads `undefined`
            match digits.get(digit(d)) {
                Some(x) => part.push_str(x),
                None => part.push_str("undefined"),
            }
            part.push_str(units[u]);
        }
        if !part.is_empty() {
            out.push_str(&part);
            out.push_str(big.get(ng - 1 - gi).copied().unwrap_or("undefined"));
        }
    }
    let mut r = String::new();
    if n < 0.0 {
        r.push('-');
    }
    r.push_str(if out.is_empty() { digits[0] } else { &out });
    r
}

fn numberstring(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let n = num(ctx, arg0(a))?;
    let k = num(ctx, scalar(a, 1).unwrap_or(Scalar::Empty))?;
    if !is_cjk(ctx) {
        return Err(Stop::Err(ErrorCode::NA));
    }
    let ty = uo_jsnum::math::trunc(k);
    if !(1.0..=3.0).contains(&ty) {
        return Err(Stop::Err(ErrorCode::Value));
    }
    put(ctx, &cjk_number(n, ty))
}

fn datestring(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let (y, m, d, _) = date_of(ctx, arg0(a))?;
    if !is_cjk(ctx) {
        return Err(Stop::Err(ErrorCode::NA));
    }
    let p = |x: f64| format!("{:0>2}", uo_jsnum::format::to_js_string(x));
    let s = format!("{}年{}月{}日", p(uo_jsnum::math::rem(y, 100.0)), p(m), p(d));
    put(ctx, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tests_follow_the_regex() {
        assert!(lang_is("th-TH", "th"));
        assert!(lang_is("TH", "th"));
        assert!(!lang_is("thx", "th"));
        assert!(!lang_is("th_TH", "th"));
        assert!(lang_is("ja", "ja"));
    }

    #[test]
    fn numbers_in_words() {
        assert_eq!(cjk_number(10_203.0, 1.0), "一万二百零三");
        assert_eq!(cjk_number(0.0, 2.0), "零");
        assert_eq!(cjk_number(-12.0, 3.0), "一二");
        assert_eq!(thai_words(21.0).unwrap(), "ยี่สิบเอ็ด");
        assert!(thai_words(f64::INFINITY).is_err());
    }
}
