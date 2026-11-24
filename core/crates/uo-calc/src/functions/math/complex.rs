//! `functions/complex.ts`: complex numbers as text (COMPLEX, IM*). Text "a+bi"/"a+bj" in the locale's decimal,
//! spaces ignored, "+-" becomes "-"; output with 15 significant digits, fixed while it fits in 21 characters, else
//! scientific with a 2-digit exponent; the polar functions use Excel's sine/cosine (the x87 π).

use uo_jsnum::math::{abs, atan2, cosh, exp, hypot, log, sign, sinh, sqrt};

use super::super::shared::mathu::{Raw, atp_raw, lift_scalars};
use super::super::shared::{EMPTY, R, Stop, arg, bodies, err, len, num, text};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::math_util::{TRIG_LIMIT, cos_ext, excel_power, fin, sin_ext, tan_ext};
use crate::strings::JsStr;
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `complex.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "COMPLEX" => complex,
    "IMABS" => imabs,
    "IMAGINARY" => imaginary,
    "IMREAL" => imreal,
    "IMARGUMENT" => imargument,
    "IMCONJUGATE" => imconjugate,
    "IMDIV" => imdiv,
    "IMSUB" => imsub,
    "IMSUM" => imsum,
    "IMPRODUCT" => improduct,
    "IMEXP" => imexp,
    "IMLN" => imln,
    "IMLOG10" => imlog10,
    "IMLOG2" => imlog2,
    "IMPOWER" => impower,
    "IMSQRT" => imsqrt,
    "IMSIN" => imsin,
    "IMCOS" => imcos,
    "IMTAN" => imtan,
    "IMCOT" => imcot,
    "IMSEC" => imsec,
    "IMCSC" => imcsc,
    "IMSINH" => imsinh,
    "IMCOSH" => imcosh,
    "IMSECH" => imsech,
    "IMCSCH" => imcsch,
];

/// A complex number with its suffix (`i`/`j`, none when read from a real number).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
    /// `b'i'`, `b'j'` or 0.
    pub sfx: u8,
}

const fn cx(re: f64, im: f64) -> Complex {
    Complex { re, im, sfx: 0 }
}

/// `s.slice(a, b)` with the JS negative-index rule, on bytes.
fn js_slice(s: &[u8], a: i64, b: i64) -> &[u8] {
    let n = s.len() as i64;
    let fix = |i: i64| if i < 0 { (n + i).max(0) } else { i.min(n) } as usize;
    let (a, b) = (fix(a), fix(b));
    if a < b { &s[a..b] } else { &[] }
}

/// `formatImNumber(x, locale)`: 15 digits, fixed up to 21 characters, else scientific (`E±dd`). Non-finite values
/// follow the TS string surgery on `"NaN"`/`"Infinity"` (`COMPLEX(NaN;1)` = "0,N+i").
pub fn format_im_number(out: &mut Vec<u16>, x: f64, dec: &[u16]) {
    if x == 0.0 {
        out.push(u16::from(b'0'));
        return;
    }
    let neg = x < 0.0;
    let s = uo_jsnum::format::to_exponential(abs(x), 14).unwrap_or_default();
    let s = s.as_bytes();
    let ei = s.iter().position(|&b| b == b'e').map_or(-1, |p| p as i64);
    let mut digits: Vec<u8> = Vec::with_capacity(16);
    digits.extend_from_slice(js_slice(s, 0, 1));
    digits.extend_from_slice(js_slice(s, 2, ei));
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    let e = uo_jsnum::parse::parse_int(core::str::from_utf8(js_slice(s, ei + 1, s.len() as i64)).unwrap_or(""), 10);
    let mut fixed: Vec<u16> = Vec::with_capacity(32);
    let w = |v: &mut Vec<u16>, b: &[u8]| v.extend(b.iter().map(|&c| u16::from(c)));
    if e >= 0.0 {
        let need = e + 1.0;
        if (digits.len() as f64) <= need {
            w(&mut fixed, &digits);
            let zeros = crate::cast::trunc_usize(need - digits.len() as f64);
            fixed.extend(core::iter::repeat_n(u16::from(b'0'), zeros));
        } else {
            let k = crate::cast::trunc_usize(need);
            w(&mut fixed, &digits[..k]);
            fixed.extend_from_slice(dec);
            w(&mut fixed, &digits[k..]);
        }
    } else {
        fixed.push(u16::from(b'0'));
        fixed.extend_from_slice(dec);
        // `'0'.repeat(-e - 1)`: NaN repeats 0 times
        let z = -e - 1.0;
        if z >= 1.0 {
            fixed.extend(core::iter::repeat_n(u16::from(b'0'), crate::cast::trunc_usize(z)));
        }
        w(&mut fixed, &digits);
    }
    if neg {
        out.push(u16::from(b'-'));
    }
    if fixed.len() <= 21 {
        out.extend_from_slice(&fixed);
        return;
    }
    if digits.len() > 1 {
        w(out, &digits[..1]);
        out.extend_from_slice(dec);
        w(out, &digits[1..]);
    } else {
        w(out, &digits);
    }
    out.push(u16::from(b'E'));
    out.push(u16::from(if e < 0.0 { b'-' } else { b'+' }));
    let ae = uo_jsnum::format::to_js_string(abs(e));
    if ae.len() < 2 {
        out.push(u16::from(b'0'));
    }
    w(out, ae.as_bytes());
}

/// `formatComplex(c, locale)`.
pub fn format_complex(out: &mut Vec<u16>, c: Complex, dec: &[u16]) {
    let sfx = if c.sfx == 0 { b'i' } else { c.sfx };
    let re = if c.re == 0.0 { 0.0 } else { c.re };
    let im = if c.im == 0.0 { 0.0 } else { c.im };
    if im == 0.0 {
        format_im_number(out, re, dec);
        return;
    }
    let mut mag = Vec::new();
    format_im_number(&mut mag, abs(im), dec);
    let one = mag.len() == 1 && mag[0] == u16::from(b'1');
    let push_im = |out: &mut Vec<u16>| {
        if !one {
            out.extend_from_slice(&mag);
        }
        out.push(u16::from(sfx));
    };
    if re == 0.0 {
        if im < 0.0 {
            out.push(u16::from(b'-'));
        }
        push_im(out);
        return;
    }
    format_im_number(out, re, dec);
    out.push(u16::from(if im < 0.0 { b'-' } else { b'+' }));
    push_im(out);
}

fn digit_run(s: &[u16], i: usize) -> usize {
    s.get(i..).map_or(0, |r| r.iter().take_while(|&&u| (0x30..=0x39).contains(&u)).count())
}

fn starts(s: &[u16], i: usize, p: &[u16]) -> bool {
    s.get(i..).is_some_and(|r| r.starts_with(p))
}

/// The number pattern of `parseComplex` at `i`: `(?:\d+(?:D\d*)?|D\d+)(?:[eE][+-]?\d+)?`, greedy. Its matches are
/// deterministic (each optional part is taken exactly when it can be: giving one back always leaves a character the
/// rest of every pattern rejects), so this scanner returns the end the regular expression's first match has.
fn num_at(s: &[u16], i: usize, dec: &[u16]) -> Option<usize> {
    let mut j = i;
    let d = digit_run(s, j);
    if d > 0 {
        j += d;
        if starts(s, j, dec) {
            j += dec.len();
            j += digit_run(s, j);
        }
    } else if starts(s, j, dec) && digit_run(s, j + dec.len()) > 0 {
        j += dec.len();
        j += digit_run(s, j);
    } else {
        return None;
    }
    if matches!(s.get(j), Some(&0x65 | &0x45)) {
        let mut k = j + 1;
        if matches!(s.get(k), Some(&0x2b | &0x2d)) {
            k += 1;
        }
        let r = digit_run(s, k);
        if r > 0 {
            j = k + r;
        }
    }
    Some(j)
}

/// `Number(t.replace(locale.decimal, '.'))` of a matched number (only the first occurrence is replaced; an empty
/// decimal inserts the point before the text, as `''.replace` does).
fn to_num_text(t: &[u16], dec: &[u16]) -> f64 {
    let mut units: Vec<u16> = Vec::with_capacity(t.len() + 1);
    match (!dec.is_empty()).then(|| t.windows(dec.len()).position(|w| w == dec)).flatten() {
        Some(p) => {
            units.extend_from_slice(&t[..p]);
            units.push(u16::from(b'.'));
            units.extend_from_slice(&t[p + dec.len()..]);
        }
        None if dec.is_empty() => {
            units.push(u16::from(b'.'));
            units.extend_from_slice(t);
        }
        None => units.extend_from_slice(t),
    }
    if units.iter().any(|&u| u > 0x7f) {
        return f64::NAN;
    }
    let b: String = units.iter().map(|&u| char::from(u as u8)).collect();
    uo_jsnum::parse::to_number(&b)
}

fn is_ij(u: Option<&u16>) -> Option<u8> {
    match u {
        Some(&0x69) => Some(b'i'),
        Some(&0x6a) => Some(b'j'),
        _ => None,
    }
}

/// `parseComplex(text, locale)` (`None` = `#NUM!`).
#[must_use]
pub fn parse_complex(text: JsStr<'_>, dec: &[u16]) -> Option<Complex> {
    let s: Vec<u16> = text.units().filter(|&u| u != 0x20).collect();
    if s.is_empty() {
        return None;
    }
    let n = s.len();
    let (sg, p) = match s[0] {
        0x2b => (1.0, 1),
        0x2d => (-1.0, 1),
        _ => (1.0, 0),
    };
    // ^([+-]?)(num)$
    if let Some(e) = num_at(&s, p, dec)
        && e == n
    {
        return Some(cx(sg * to_num_text(&s[p..e], dec), 0.0));
    }
    // ^([+-]?)(num)?([ij])$
    match num_at(&s, p, dec) {
        Some(e) => {
            if e + 1 == n
                && let Some(x) = is_ij(s.get(e))
            {
                return Some(Complex { re: 0.0, im: sg * to_num_text(&s[p..e], dec), sfx: x });
            }
        }
        None => {
            if p + 1 == n
                && let Some(x) = is_ij(s.get(p))
            {
                return Some(Complex { re: 0.0, im: sg, sfx: x });
            }
        }
    }
    // ^([+-]?)(num)([+-]{1,2})(num)?([ij])$
    let e1 = num_at(&s, p, dec)?;
    let mut k = e1;
    let mut sg2 = 1.0;
    let mut signs = 0;
    while signs < 2 && matches!(s.get(k), Some(&0x2b | &0x2d)) {
        if s[k] == 0x2d {
            sg2 = -sg2;
        }
        k += 1;
        signs += 1;
    }
    if signs == 0 {
        return None;
    }
    let re = sg * to_num_text(&s[p..e1], dec);
    match num_at(&s, k, dec) {
        Some(e2) => {
            let x = is_ij(s.get(e2))?;
            (e2 + 1 == n).then(|| Complex { re, im: sg2 * to_num_text(&s[k..e2], dec), sfx: x })
        }
        None => {
            let x = is_ij(s.get(k))?;
            (k + 1 == n).then_some(Complex { re, im: sg2, sfx: x })
        }
    }
}

/// `toComplex(v, ctx)` (ATP rules).
fn to_complex(ctx: &dyn FnContext, v: Scalar, dec: &[u16]) -> Result<Complex, ErrorCode> {
    match v {
        Scalar::Err(e) => Err(e),
        Scalar::Bool(_) => Err(ErrorCode::Value),
        Scalar::Empty => Ok(cx(0.0, 0.0)),
        Scalar::Num(x) => Ok(cx(x, 0.0)),
        Scalar::Str(r) => parse_complex(ctx.text(r), dec).ok_or(ErrorCode::Num),
    }
}

/// `joinSfx(cs)`: the common suffix (`Err` when `i` and `j` mix).
fn join_sfx(cs: &[Complex]) -> Result<u8, ()> {
    let mut s = 0;
    for c in cs {
        if c.sfx == 0 {
            continue;
        }
        if s != 0 && s != c.sfx {
            return Err(());
        }
        s = c.sfx;
    }
    Ok(s)
}

/// A body's result: a complex, a number or an error.
enum Cx {
    C(Complex),
    N(f64),
    E(ErrorCode),
}

impl From<Complex> for Cx {
    fn from(c: Complex) -> Self {
        Self::C(c)
    }
}

impl From<Result<Complex, ErrorCode>> for Cx {
    fn from(r: Result<Complex, ErrorCode>) -> Self {
        match r {
            Ok(c) => Self::C(c),
            Err(e) => Self::E(e),
        }
    }
}

/// The locale's decimal as units.
fn decimal_of(ctx: &dyn FnContext) -> Vec<u16> {
    ctx.locale().decimal.as_js().to_utf16()
}

/// The text of a complex result (subnormals → 0, the suffix applied).
fn complex_text(ctx: &mut dyn FnContext, z: Complex, sfx: u8, dec: &[u16]) -> R<Value> {
    let mut out = Vec::with_capacity(32);
    format_complex(&mut out, flush(z, sfx), dec);
    text(ctx, JsStr::Utf16(&out))
}

/// `cxFn(name, min, max, body, {nums})`: complex arguments (numbers at the positions `nums`), lifted over arrays.
fn cx_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, nums: &[usize], body: fn(&[Complex], &[f64]) -> Cx) -> R<Value> {
    let mut raw = Vec::with_capacity(len(a));
    for i in 0..len(a) {
        raw.push(atp_raw(Some(arg(a, i).unwrap_or(&EMPTY)), ctx)?);
    }
    let dec = decimal_of(ctx);
    lift_scalars(&raw, ctx, |s, ctx| {
        let mut cs = [cx(0.0, 0.0); 4];
        let mut ncs = 0;
        let mut ns = [0.0; 4];
        let mut nns = 0;
        for (i, &v) in s.iter().enumerate() {
            if nums.contains(&i) {
                if let Scalar::Err(e) = v {
                    return Ok(err(e));
                }
                if let Scalar::Bool(_) = v {
                    return Ok(err(ErrorCode::Value));
                }
                let c = match to_complex(ctx, v, &dec) {
                    Ok(c) => c,
                    Err(e) => return Ok(err(if matches!(v, Scalar::Str(_)) { ErrorCode::Value } else { e })),
                };
                if c.im != 0.0 {
                    return Ok(err(ErrorCode::Value));
                }
                if nns < ns.len() {
                    ns[nns] = c.re;
                    nns += 1;
                }
                continue;
            }
            match to_complex(ctx, v, &dec) {
                Ok(c) => {
                    if ncs < cs.len() {
                        cs[ncs] = c;
                        ncs += 1;
                    }
                }
                Err(e) => return Ok(err(e)),
            }
        }
        // measured: IMDIV("3+4i";"1+j") = #NUM! (IMSUM gives #VALUE!)
        let Ok(sfx) = join_sfx(&cs[..ncs]) else { return Ok(err(ErrorCode::Num)) };
        match body(&cs[..ncs], &ns[..nns]) {
            Cx::E(e) => Ok(err(e)),
            Cx::N(x) => Ok(match fin(x) {
                Ok(v) => num(v),
                Err(e) => err(e),
            }),
            Cx::C(z) => {
                if !z.re.is_finite() || !z.im.is_finite() {
                    return Ok(err(ErrorCode::Num));
                }
                complex_text(ctx, z, sfx, &dec)
            }
        }
    })
}

/// `cxList(args, ctx)` (IMSUM/IMPRODUCT: ranges and arrays; an empty cell = 0, an area with missing cells adds one
/// 0; an omitted argument = `#N/A` except the last).
fn cx_list(a: &CallArgs<'_>, ctx: &mut dyn FnContext, dec: &[u16]) -> R<Vec<Complex>> {
    let mut out = Vec::new();
    let n = len(a);
    let mut cells: Vec<Scalar> = Vec::new();
    for i in 0..n {
        match arg(a, i) {
            None | Some(Value::Scalar(Scalar::Empty)) => {
                if i == n - 1 && i > 0 {
                    continue;
                }
                return Err(Stop::Err(ErrorCode::NA));
            }
            Some(Value::Ref(r)) => {
                for &area in r.areas() {
                    cells.clear();
                    ctx.for_each_cell(area, &mut |v, _, _| cells.push(v))?;
                    for &v in &cells {
                        out.push(to_complex(ctx, v, dec)?);
                    }
                    if (cells.len() as u64) < crate::stats::area_size(&area) {
                        out.push(cx(0.0, 0.0));
                    }
                }
            }
            Some(Value::Array(x)) => {
                for &v in x.data.iter() {
                    out.push(to_complex(ctx, v, dec)?);
                }
            }
            Some(Value::Lambda(_)) => return Err(Stop::Err(ErrorCode::Value)),
            Some(Value::Scalar(v)) => out.push(to_complex(ctx, *v, dec)?),
        }
    }
    Ok(out)
}

/// `cxListFn(name, f, overflow)`.
fn cx_list_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, g: fn(&[Complex]) -> Option<Complex>, overflow: ErrorCode) -> R<Value> {
    let dec = decimal_of(ctx);
    let cs = cx_list(a, ctx, &dec)?;
    let Ok(sfx) = join_sfx(&cs) else { return Ok(err(ErrorCode::Value)) };
    // IMPRODUCT of no value: the TS reads `.re` of `undefined` (a TypeError)
    let Some(r) = g(&cs) else { return Err(Stop::Throw(Throw::Internal("TypeError: cannot read properties of undefined"))) };
    if !r.re.is_finite() || !r.im.is_finite() {
        return Ok(err(overflow));
    }
    complex_text(ctx, r, sfx, &dec)
}

// ---------------------------------------------------------------- arithmetic

/// Subnormals become 0 (as Excel) and the suffix is applied.
fn flush(z: Complex, sfx: u8) -> Complex {
    let f = |v: f64| if abs(v) < 2.225_073_858_507_201_4e-308 { 0.0 } else { v };
    Complex { re: f(z.re), im: f(z.im), sfx }
}

fn mul(a: Complex, b: Complex) -> Complex {
    cx(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

/// Smith's division (no overflow in the denominator).
fn smith(a: Complex, b: Complex) -> Result<Complex, ErrorCode> {
    if b.re == 0.0 && b.im == 0.0 {
        return Err(ErrorCode::Num);
    }
    // measured: IMSECH("1E+20+1E-20i") = 0 (infinite divisor)
    if b.re.is_nan() || b.im.is_nan() {
        return Err(ErrorCode::Num);
    }
    if !b.re.is_finite() || !b.im.is_finite() {
        return if a.re.is_finite() && a.im.is_finite() { Ok(cx(0.0, 0.0)) } else { Err(ErrorCode::Num) };
    }
    if abs(b.re) >= abs(b.im) {
        let r = b.im / b.re;
        let den = b.re + b.im * r;
        return Ok(cx((a.re + a.im * r) / den, (a.im - a.re * r) / den));
    }
    let r = b.re / b.im;
    let den = b.re * r + b.im;
    Ok(cx((a.re * r + a.im) / den, (a.im * r - a.re) / den))
}

/// The direct division a·conj(b)/|b|² (IMDIV's); Smith when |b|² is out of range.
fn div(a: Complex, b: Complex) -> Result<Complex, ErrorCode> {
    let d = b.re * b.re + b.im * b.im;
    if b.re == 0.0 && b.im == 0.0 {
        return Err(ErrorCode::Num);
    }
    if !(d > 2.225_073_858_507_201_4e-308 && d < f64::INFINITY) {
        return smith(a, b);
    }
    Ok(cx((a.re * b.re + a.im * b.im) / d, (a.im * b.re - a.re * b.im) / d))
}

fn cabs(z: Complex) -> f64 {
    hypot(z.re, z.im)
}

fn carg(z: Complex) -> f64 {
    atan2(z.im, z.re)
}

fn polar(r: f64, t: f64) -> Complex {
    cx(r * cos_ext(t), r * sin_ext(t))
}

fn cln(z: Complex) -> Result<Complex, ErrorCode> {
    if z.re == 0.0 && z.im == 0.0 {
        return Err(ErrorCode::Num);
    }
    Ok(cx(log(cabs(z)), carg(z)))
}

const DBL_MAX: f64 = 1.797_693_134_862_315_7e308;

/// `e^x` as the ATP: saturates at `DBL_MAX` until x passes the extended range (x > 11356.5).
fn exp_c(x: f64) -> f64 {
    let e = exp(x);
    if e.is_finite() {
        e
    } else if x < 11356.5 {
        DBL_MAX
    } else {
        f64::INFINITY
    }
}

fn cexp(z: Complex) -> Complex {
    polar(exp_c(z.re), z.im)
}

/// Excel's sine/cosine (|x| ≥ 2^27 → NaN).
fn s_(x: f64) -> f64 {
    if abs(x) >= TRIG_LIMIT { f64::NAN } else { sin_ext(x) }
}

fn c_(x: f64) -> f64 {
    if abs(x) >= TRIG_LIMIT { f64::NAN } else { cos_ext(x) }
}

fn sh_n(y: f64) -> f64 {
    (exp_c(y) - exp_c(-y)) / 2.0
}

fn ch_n(y: f64) -> f64 {
    (exp_c(y) + exp_c(-y)) / 2.0
}

fn csin(z: Complex) -> Complex {
    cx(s_(z.re) * ch_n(z.im), c_(z.re) * sh_n(z.im))
}

fn ccos(z: Complex) -> Complex {
    cx(c_(z.re) * ch_n(z.im), -s_(z.re) * sh_n(z.im))
}

fn csinh(z: Complex) -> Complex {
    cx(sinh(z.re) * c_(z.im), cosh(z.re) * s_(z.im))
}

fn ccosh(z: Complex) -> Complex {
    cx(cosh(z.re) * c_(z.im), sinh(z.re) * s_(z.im))
}

/// tan z = (sin 2x + i sinh 2y)/(cos 2x + cosh 2y) (measured); a real z is `TAN`.
fn ctan(z: Complex) -> Complex {
    if z.im == 0.0 {
        return cx(if abs(z.re) >= TRIG_LIMIT { f64::NAN } else { tan_ext(z.re) }, 0.0);
    }
    let ch = cosh(2.0 * z.im);
    if !ch.is_finite() {
        return cx(0.0 * s_(2.0 * z.re), sign(z.im));
    }
    let d = c_(2.0 * z.re) + ch;
    cx(s_(2.0 * z.re) / d, sinh(2.0 * z.im) / d)
}

/// cot z = (sin 2x − i sinh 2y)/(cosh 2y − cos 2x) (measured, also the cancellation near 0).
fn ccot(z: Complex) -> Result<Complex, ErrorCode> {
    if z.im == 0.0 {
        if abs(z.re) >= TRIG_LIMIT {
            return Ok(cx(0.0, 0.0)); // measured: IMCOT("1E+300") = 0
        }
        return div(cx(c_(z.re), 0.0), cx(s_(z.re), 0.0));
    }
    let ch = ch_n(2.0 * z.im);
    if !ch.is_finite() {
        return Ok(cx(0.0 * s_(2.0 * z.re), -sign(z.im)));
    }
    let d = ch - c_(2.0 * z.re);
    Ok(cx(s_(2.0 * z.re) / d, -sh_n(2.0 * z.im) / d))
}

const ONE: Complex = cx(1.0, 0.0);

/// `hypCx(z)`: (sinh z, cosh z) by the exponential formulas.
fn hyp_cx(z: Complex) -> (Complex, Complex) {
    if z.im == 0.0 {
        return (cx(sinh(z.re), 0.0), cx(ch_n(z.re), 0.0));
    }
    (cx(sh_n(z.re) * c_(z.im), ch_n(z.re) * s_(z.im)), cx(ch_n(z.re) * c_(z.im), sh_n(z.re) * s_(z.im)))
}

fn complex(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut raw = Vec::with_capacity(3);
    for i in 0..len(a) {
        let v = arg(a, i).unwrap_or(&EMPTY);
        if i == 2 && matches!(v, Value::Scalar(Scalar::Empty)) {
            raw.push(Raw::S(Scalar::Str(ctx.new_text(JsStr::Latin1(b"i"))?)));
        } else {
            raw.push(atp_raw(Some(v), ctx)?);
        }
    }
    let dec = decimal_of(ctx);
    lift_scalars(&raw, ctx, |s, ctx| {
        let mut xs = [0.0; 2];
        for (k, slot) in xs.iter_mut().enumerate() {
            let v = s.get(k).copied().unwrap_or(Scalar::Empty);
            if let Scalar::Err(e) = v {
                return Ok(err(e));
            }
            if let Scalar::Bool(_) = v {
                return Ok(err(ErrorCode::Value));
            }
            match to_complex(ctx, v, &dec) {
                Ok(c) if c.im == 0.0 => *slot = c.re,
                _ => return Ok(err(ErrorCode::Value)),
            }
        }
        let mut sfx = b'i';
        if let Some(&v) = s.get(2) {
            match v {
                Scalar::Err(e) => return Ok(err(e)),
                Scalar::Str(r) if ctx.text(r).eq_str("j") => sfx = b'j',
                // measured: only "i", "j" or empty
                Scalar::Str(r) if ctx.text(r).eq_str("i") || ctx.text(r).is_empty() => {}
                // `s === 0` (an empty cell read as 0 by `atpRaw`; -0 too)
                Scalar::Num(x) => {
                    if x != 0.0 {
                        return Ok(err(ErrorCode::Value));
                    }
                }
                _ => return Ok(err(ErrorCode::Value)),
            }
        }
        let mut out = Vec::with_capacity(32);
        format_complex(&mut out, Complex { re: xs[0], im: xs[1], sfx }, &dec);
        text(ctx, JsStr::Utf16(&out))
    })
}

fn imabs(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::N(cabs(c[0])))
}

fn imaginary(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::N(c[0].im))
}

fn imreal(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::N(c[0].re))
}

fn imargument(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| if c[0].re == 0.0 && c[0].im == 0.0 { Cx::E(ErrorCode::Div0) } else { Cx::N(carg(c[0])) })
}

fn imconjugate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(cx(c[0].re, -c[0].im)))
}

fn imdiv(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| div(c[0], c[1]).into())
}

fn imsub(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(cx(c[0].re - c[1].re, c[0].im - c[1].im)))
}

fn imsum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_list_fn(
        a,
        ctx,
        |cs| {
            let mut s = cx(0.0, 0.0);
            for c in cs {
                s = cx(s.re + c.re, s.im + c.im);
            }
            Some(s)
        },
        ErrorCode::Num,
    )
}

fn improduct(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_list_fn(
        a,
        ctx,
        |cs| {
            let (&first, rest) = cs.split_first()?;
            let mut p = first;
            for &c in rest {
                p = mul(p, c);
            }
            Some(p)
        },
        ErrorCode::Value,
    )
}

fn imexp(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(cexp(c[0])))
}

fn imln(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| cln(c[0]).into())
}

fn imlog10(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| cln(c[0]).map(|l| cx(l.re / core::f64::consts::LN_10, l.im / core::f64::consts::LN_10)).into())
}

fn imlog2(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| cln(c[0]).map(|l| cx(l.re / core::f64::consts::LN_2, l.im / core::f64::consts::LN_2)).into())
}

fn impower(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[1], |c, ns| {
        let (z, k) = (c[0], ns.first().copied().unwrap_or(f64::NAN));
        if z.re == 0.0 && z.im == 0.0 {
            return if k > 0.0 { Cx::C(cx(0.0, 0.0)) } else { Cx::E(ErrorCode::Num) };
        }
        match excel_power(cabs(z), k) {
            Ok(r) => Cx::C(polar(r, k * carg(z))),
            Err(e) => Cx::E(e),
        }
    })
}

fn imsqrt(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // measured: IMSQRT uses |z| = √(x²+y²) directly (IMSQRT("1E-300") = 0; "1E+300" = #NUM!)
    cx_fn(a, ctx, &[], |c, _| {
        let z = c[0];
        Cx::C(polar(sqrt(sqrt(z.re * z.re + z.im * z.im)), carg(z) / 2.0))
    })
}

fn imsin(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(csin(c[0])))
}

fn imcos(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(ccos(c[0])))
}

fn imtan(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(ctan(c[0])))
}

fn imcot(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| ccot(c[0]).into())
}

fn imsec(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| smith(ONE, ccos(c[0])).into())
}

fn imcsc(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| smith(ONE, csin(c[0])).into())
}

fn imsinh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(csinh(c[0])))
}

fn imcosh(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| Cx::C(ccosh(c[0])))
}

fn imsech(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| div(ONE, hyp_cx(c[0]).1).into())
}

fn imcsch(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cx_fn(a, ctx, &[], |c, _| div(ONE, hyp_cx(c[0]).0).into())
}
