//! `functions/datetime.ts`: DATE, TIME, DATEVALUE, TIMEVALUE, NOW, TODAY, YEAR…SECOND, WEEKDAY, WEEKNUM, ISOWEEKNUM,
//! EDATE, EOMONTH, NETWORKDAYS(.INTL), WORKDAY(.INTL), DATEDIF, DAYS, DAYS360, YEARFRAC (rules measured by the TS in
//! Excel 365 pt-BR, listed in its header). The calendar is [`crate::dates`] (A1); YEARFRAC is
//! [`super::daycount::year_frac`]. NOW/TODAY read the host clock ([`FnContext::now`], decision C1); their volatility
//! is the registry's (A3).

use uo_jsnum::conv::js_index;
use uo_jsnum::math::{abs, floor, min, rem, trunc};

use super::daycount::year_frac;
use super::{Steps, lift_atp, type_error};
use crate::coerce::parse_date_time_parts;
use crate::dates::{days_in_month, excel_date, max_serial, serial_to_ymd, split_serial, weekday, ymd_to_serial};
use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::functions::shared::tdl::{AtpArg, atp_num, num as tnum, txt};
use crate::functions::shared::{R, Stop, bodies, err, num, scalar};
use crate::strings::JsStr;
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `datetime.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "DATE" => date,
    "TIME" => time,
    "DATEVALUE" => datevalue,
    "TIMEVALUE" => timevalue,
    "NOW" => now,
    "TODAY" => today,
    "YEAR" => year,
    "MONTH" => month,
    "DAY" => day,
    "HOUR" => hour,
    "MINUTE" => minute,
    "SECOND" => second,
    "WEEKDAY" => weekday_fn,
    "WEEKNUM" => weeknum,
    "ISOWEEKNUM" => isoweeknum,
    "EDATE" => edate,
    "EOMONTH" => eomonth,
    "NETWORKDAYS" => networkdays,
    "NETWORKDAYS.INTL" => networkdays_intl,
    "WORKDAY" => workday_fn,
    "WORKDAY.INTL" => workday_intl,
    "DATEDIF" => datedif_fn,
    "DAYS" => days,
    "DAYS360" => days360_fn,
    "YEARFRAC" => yearfrac,
];

/// An error as a body result.
#[inline]
fn e(c: ErrorCode) -> R<Value> {
    Ok(err(c))
}

/// A `number | CalcError` as a body result (no `fin`: these bodies return NaN as the TS does).
#[inline]
fn nv(r: Result<f64, ErrorCode>) -> R<Value> {
    Ok(crate::functions::shared::num_or_err(r))
}

/// The scalar of a lifted `value` argument (`args[i]`; a missing one reads as empty — the callers check presence).
#[inline]
fn sc(a: &CallArgs<'_>, i: usize) -> Scalar {
    scalar(a, i).unwrap_or(Scalar::Empty)
}

/// `split(v, ctx)`: the serial rounded to the second → (day, second) or `#NUM!`.
fn split(v: Scalar, ctx: &dyn FnContext) -> Result<(f64, f64), ErrorCode> {
    let n = tnum(ctx, v)?;
    split_serial(n, ctx.date1904()).ok_or(ErrorCode::Num)
}

/// `dateArg(v, ctx)`: the date (integer part) of an argument, 0 ≤ serial ≤ 31/12/9999 (NaN passes, as the TS).
fn date_arg(v: Scalar, ctx: &dyn FnContext) -> Result<f64, ErrorCode> {
    let d = floor(tnum(ctx, v)?);
    if d < 0.0 || d > max_serial(ctx.date1904()) { Err(ErrorCode::Num) } else { Ok(d) }
}

/// `atpDate(v, ctx)`: the date of an ATP argument (boolean → `#VALUE!`, omitted → `#N/A`, out of range → `#NUM!`).
fn atp_date(v: Option<&AtpArg>, ctx: &dyn FnContext) -> Result<f64, ErrorCode> {
    let d = floor(atp_num(ctx, v, None)?);
    if d < 0.0 || d > max_serial(ctx.date1904()) { Err(ErrorCode::Num) } else { Ok(d) }
}

/// `wd(s, ctx)`: day of the week (0 = Sunday).
#[inline]
fn wd(s: f64, ctx: &dyn FnContext) -> f64 {
    weekday(s, ctx.date1904())
}

/// `serial(y, m, d, ctx)`.
#[inline]
fn serial(y: f64, m: f64, d: f64, ctx: &dyn FnContext) -> f64 {
    ymd_to_serial(y, m, d, ctx.date1904())
}

// ---------------------------------------------------------------- weekends and holidays

/// A weekend mask (index 0 = Sunday … 6 = Saturday).
type Mask = [bool; 7];

/// Saturday and Sunday.
const SAT_SUN: Mask = [true, false, false, false, false, false, true];

/// `mask[w]` (a day that is not 0..6 — `undefined` in the TS — is not a weekend).
#[inline]
fn is_weekend(mask: &Mask, w: f64) -> bool {
    js_index(w, 7).is_some_and(|i| mask[i])
}

/// `weekendMask(v, ctx)` for a present weekend argument (`atpWeekend` handled the omitted, error and boolean ones).
fn weekend_mask(v: Scalar, ctx: &dyn FnContext) -> Result<Mask, ErrorCode> {
    if let Scalar::Str(r) = v {
        // `/^[01]{7}$/`: "Monday…Sunday"
        let t = ctx.text(r);
        if t.len() != 7 || !t.units().all(|u| u == u16::from(b'0') || u == u16::from(b'1')) {
            return Err(ErrorCode::Value);
        }
        let on = |i: usize| t.unit(i) == Some(u16::from(b'1'));
        return Ok([on(6), on(0), on(1), on(2), on(3), on(4), on(5)]);
    }
    let k = trunc(tnum(ctx, v)?);
    let mut out = [false; 7];
    // 1 = Sat+Sun, 2 = Sun+Mon, …, 7 = Fri+Sat; 11 = Sunday only, …, 17 = Saturday only
    if (1.0..=7.0).contains(&k) {
        let a = rem(k + 5.0, 7.0); // 1 → Saturday (6)
        if let (Some(i), Some(j)) = (js_index(a, 7), js_index(rem(a + 1.0, 7.0), 7)) {
            out[i] = true;
            out[j] = true;
        }
        return Ok(out);
    }
    if (11.0..=17.0).contains(&k) {
        if let Some(i) = js_index(rem(k - 11.0, 7.0), 7) {
            out[i] = true;
        }
        return Ok(out);
    }
    Err(ErrorCode::Num)
}

/// `atpWeekend(v, ctx)`: omitted/empty → Saturday+Sunday; an error or a boolean → `#VALUE!` (measured); else
/// [`weekend_mask`].
fn atp_weekend(v: Option<&AtpArg>, ctx: &dyn FnContext) -> Result<Mask, ErrorCode> {
    match v.and_then(AtpArg::scalar) {
        None | Some(Scalar::Empty) => Ok(SAT_SUN),
        Some(Scalar::Err(_) | Scalar::Bool(_)) => Err(ErrorCode::Value),
        Some(s) => weekend_mask(s, ctx),
    }
}

/// `holidaySet(v, ctx)`: the holidays as a `Set` of whole dates — here sorted and deduplicated (`-0` = `+0`, as the
/// `Set`), a NaN member kept as a flag (it counts in the size and matches only NaN).
/// The longest array a JS spread (`push(...xs)`) accepts in Node 24: V8 throws a `RangeError` past about this many
/// values (the same bound as `uo_dml_render::metafile::SPREAD_LIMIT`; declared divergence between it and V8's exact,
/// stack-dependent limit).
const SPREAD_LIMIT: usize = 120_000;

#[derive(Default)]
struct Holidays {
    days: Vec<f64>,
    nan: bool,
}

impl Holidays {
    /// `hol.size === 0`.
    fn is_empty(&self) -> bool {
        self.days.is_empty() && !self.nan
    }
    /// `hol.has(s)`.
    fn has(&self, s: f64) -> bool {
        if s.is_nan() {
            return self.nan;
        }
        self.days.binary_search_by(|x| x.total_cmp(&(s + 0.0))).is_ok()
    }
}

fn holiday_set(v: Option<&AtpArg>, ctx: &mut dyn FnContext) -> R<Holidays> {
    let mut h = Holidays::default();
    let v = match v {
        None | Some(AtpArg::Omitted) => return Ok(h),
        Some(AtpArg::S(s)) => Value::Scalar(*s),
        Some(AtpArg::Raw(v)) => v.clone(),
    };
    let add = |x: Scalar, ctx: &dyn FnContext, h: &mut Holidays| -> R<()> {
        match x {
            Scalar::Empty => Ok(()),
            Scalar::Err(c) => Err(Stop::Err(c)),
            Scalar::Bool(_) => Err(Stop::Err(ErrorCode::Value)),
            x => {
                let n = tnum(ctx, x)?;
                if n < 0.0 {
                    return Err(Stop::Err(ErrorCode::Num));
                }
                let d = floor(n);
                if d.is_nan() {
                    h.nan = true;
                } else {
                    h.days.push(d + 0.0);
                }
                Ok(())
            }
        }
    };
    match &v {
        Value::Scalar(Scalar::Empty) => return Ok(h),
        Value::Scalar(s) => add(*s, ctx, &mut h)?,
        Value::Array(a) => {
            for &x in a.data.iter() {
                add(x, ctx, &mut h)?;
            }
        }
        Value::Ref(r) => {
            // the TS collects every area first: a later area's `NeedCell` comes before an earlier error
            let mut all = Vec::new();
            for k in 0..r.areas().len() {
                let x = ctx.values(r, k)?;
                // `xs.push(...data)`: V8 throws a `RangeError` (stack) for a spread past ≈ 120,000 values
                if x.data.len() > SPREAD_LIMIT {
                    return Err(Stop::Throw(crate::fnreg::Throw::Range));
                }
                all.push(x);
            }
            for a in &all {
                for &x in a.data.iter() {
                    add(x, ctx, &mut h)?;
                }
            }
        }
        // a LAMBDA reaches `toNumber` as an object (the TS `TypeError`)
        Value::Lambda(_) => return Err(type_error()),
    }
    h.days.sort_by(f64::total_cmp);
    h.days.dedup();
    Ok(h)
}

/// `networkDays(start, end, mask, hol)` (NaN dates give NaN, as the TS).
fn network_days(start: f64, end: f64, mask: &Mask, hol: &Holidays, ctx: &dyn FnContext) -> f64 {
    let sign = if end >= start { 1.0 } else { -1.0 };
    let (a, b) = if sign > 0.0 { (start, end) } else { (end, start) };
    let work_per_week = mask.iter().filter(|&&x| !x).count() as f64;
    let total = b - a + 1.0;
    let weeks = floor(total / 7.0);
    let mut count = weeks * work_per_week;
    let mut s = a + weeks * 7.0;
    // at most 7 days (dates are validated ≤ 31/12/9999)
    while s <= b {
        if !is_weekend(mask, wd(s, ctx)) {
            count += 1.0;
        }
        s += 1.0;
    }
    // a NaN holiday is never between the dates
    for &h in &hol.days {
        if h >= a && h <= b && !is_weekend(mask, wd(h, ctx)) {
            count -= 1.0;
        }
    }
    sign * count
}

/// The most single days [`workday`] steps (a valid start leaves the date range within ≈ 3M steps; past the cap the
/// start is NaN and the TS loop runs `days` times — up to forever).
const WORKDAY_CAP: u32 = 1 << 23;

/// `workday(start, days, mask, hol)`.
fn workday(start: f64, days: f64, mask: &Mask, hol: &Holidays, ctx: &dyn FnContext) -> Result<f64, ErrorCode> {
    if mask.iter().all(|&x| x) {
        return Err(ErrorCode::Value);
    }
    let mut s = start;
    let step = if days >= 0.0 { 1.0 } else { -1.0 };
    let mut left = abs(days);
    let work_per_week = mask.iter().filter(|&&x| !x).count() as f64;
    // whole weeks are skipped when there is no holiday on the way
    let max = max_serial(ctx.date1904());
    let mut steps = Steps::new(WORKDAY_CAP);
    while left > 0.0 {
        steps.tick()?;
        if left > work_per_week * 2.0 && hol.is_empty() {
            let weeks = floor(left / work_per_week) - 1.0;
            s += step * weeks * 7.0;
            left -= weeks * work_per_week;
            continue;
        }
        s += step;
        if s < 0.0 || s > max {
            return Err(ErrorCode::Num);
        }
        if !is_weekend(mask, wd(s, ctx)) && !hol.has(s) {
            left -= 1.0;
        }
    }
    if s < 0.0 || s > max {
        return Err(ErrorCode::Num);
    }
    Ok(s)
}

// ---------------------------------------------------------------- weeks

/// `isoWeek(s, ctx)`: the Thursday of the same ISO week decides the year.
fn iso_week(s: f64, ctx: &dyn FnContext) -> f64 {
    let dow = rem(wd(s, ctx) + 6.0, 7.0); // 0 = Monday
    let thu = s - dow + 3.0;
    let y = serial_to_ymd(thu, ctx.date1904()).y;
    let jan1 = serial(y, 1.0, 1.0, ctx);
    floor((thu - jan1) / 7.0) + 1.0
}

/// `WEEKNUM_START[type]`: the day the week starts on (0 = Sunday).
fn weeknum_start(t: f64) -> Option<f64> {
    Some(match t {
        1.0 | 17.0 => 0.0,
        2.0 | 11.0 => 1.0,
        12.0 => 2.0,
        13.0 => 3.0,
        14.0 => 4.0,
        15.0 => 5.0,
        16.0 => 6.0,
        _ => return None,
    })
}

// ---------------------------------------------------------------- DATEDIF / DAYS360

/// The units of DATEDIF (`unit.toUpperCase()` compared with them: only ASCII letters map onto these — no other
/// code unit upper-cases to `D`, `M` or `Y`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    D,
    M,
    Y,
    Ym,
    Yd,
    Md,
}

fn unit_of(t: JsStr<'_>) -> Option<Unit> {
    let up = |i: usize| t.unit(i).and_then(|u| u8::try_from(u).ok()).map(|b| b.to_ascii_uppercase());
    match (t.len(), up(0), up(1)) {
        (1, Some(b'D'), _) => Some(Unit::D),
        (1, Some(b'M'), _) => Some(Unit::M),
        (1, Some(b'Y'), _) => Some(Unit::Y),
        (2, Some(b'Y'), Some(b'M')) => Some(Unit::Ym),
        (2, Some(b'Y'), Some(b'D')) => Some(Unit::Yd),
        (2, Some(b'M'), Some(b'D')) => Some(Unit::Md),
        _ => None,
    }
}

/// `datedif(a, b, unit, ctx)`.
fn datedif(a: f64, b: f64, unit: Option<Unit>, ctx: &dyn FnContext) -> Result<f64, ErrorCode> {
    if a > b {
        return Err(ErrorCode::Num);
    }
    let d1904 = ctx.date1904();
    let s = serial_to_ymd(a, d1904);
    let en = serial_to_ymd(b, d1904);
    Ok(match unit {
        Some(Unit::D) => b - a,
        Some(Unit::M) => {
            let mut m = (en.y - s.y) * 12.0 + en.m - s.m;
            if en.d < s.d {
                m -= 1.0;
            }
            m
        }
        Some(Unit::Y) => {
            let mut y = en.y - s.y;
            if en.m < s.m || (en.m == s.m && en.d < s.d) {
                y -= 1.0;
            }
            y
        }
        Some(Unit::Ym) => {
            let mut m = en.m - s.m;
            if en.d < s.d {
                m -= 1.0;
            }
            rem(rem(m, 12.0) + 12.0, 12.0)
        }
        Some(Unit::Yd) => {
            // measured by the TS (daily grid 2022–2028): Excel adds the "YM" months to the start IN THE START'S
            // YEAR, then the "MD" days
            let ym = datedif(a, b, Some(Unit::Ym), ctx)?;
            let md = datedif(a, b, Some(Unit::Md), ctx)?;
            serial(s.y, s.m + ym, s.d, ctx) - a + md
        }
        Some(Unit::Md) => {
            // days as if in the same month (the one before the end's when the day goes back)
            if en.d >= s.d {
                return Ok(en.d - s.d);
            }
            let pm = if en.m == 1.0 { 12.0 } else { en.m - 1.0 };
            let py = if en.m == 1.0 { en.y - 1.0 } else { en.y };
            days_in_month(py, pm, d1904) - s.d + en.d
        }
        None => return Err(ErrorCode::Num),
    })
}

/// `days360(a, b, european, ctx)`.
fn days360(a: f64, b: f64, european: bool, ctx: &dyn FnContext) -> f64 {
    let d1904 = ctx.date1904();
    let s = serial_to_ymd(a, d1904);
    let en = serial_to_ymd(b, d1904);
    let (mut sd, mut ed) = (s.d, en.d);
    if european {
        if sd == 31.0 {
            sd = 30.0;
        }
        if ed == 31.0 {
            ed = 30.0;
        }
    } else {
        // `lastOfFeb(s)`
        if s.m == 2.0 && s.d == days_in_month(s.y, 2.0, d1904) {
            sd = 30.0;
        }
        if sd == 31.0 {
            sd = 30.0;
        }
        if ed == 31.0 {
            if sd < 30.0 {
                return (en.y - s.y) * 360.0 + (en.m - s.m) * 30.0 + 30.0 + 1.0 - sd;
            }
            ed = 30.0;
        }
    }
    (en.y - s.y) * 360.0 + (en.m - s.m) * 30.0 + ed - sd
}

/// `addMonths(d, k, eom, ctx)` (EDATE/EOMONTH) in the REAL calendar: 29/02/1900 (serial 60) becomes 28/02/1900; a year
/// < 1900 → `#NUM!` even in the 1904 system.
fn add_months(d: f64, k: f64, eom: bool, ctx: &dyn FnContext) -> Result<f64, ErrorCode> {
    let p = serial_to_ymd(d, ctx.date1904());
    let mm = p.m + k;
    let y = p.y + floor((mm - 1.0) / 12.0);
    let mo = rem(rem(mm - 1.0, 12.0) + 12.0, 12.0) + 1.0;
    if y < 1900.0 || y > 9999.0 {
        return Err(ErrorCode::Num);
    }
    let dim = days_in_month(y, mo, true);
    let s = serial(y, mo, if eom { dim } else { min(p.d, dim) }, ctx);
    if s > max_serial(ctx.date1904()) { Err(ErrorCode::Num) } else { Ok(s) }
}

// ---------------------------------------------------------------- bodies

fn date(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    let y = match tnum(ctx, sc(a, 0)) {
        Ok(x) => x,
        Err(c) => return e(c),
    };
    let m = match tnum(ctx, sc(a, 1)) {
        Ok(x) => x,
        Err(c) => return e(c),
    };
    let d = match tnum(ctx, sc(a, 2)) {
        Ok(x) => x,
        Err(c) => return e(c),
    };
    nv(excel_date(y, m, d, ctx.date1904()).ok_or(ErrorCode::Num))
}

fn time(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut xs = [0.0; 3];
    for (i, x) in xs.iter_mut().enumerate() {
        let n = match tnum(ctx, sc(a, i)) {
            Ok(n) => n,
            Err(c) => return e(c),
        };
        let mut k = floor(n);
        if k > 32767.0 {
            return e(ErrorCode::Num);
        }
        if k < -32768.0 {
            // 16 bits (measured: -40000 min = 25536)
            k = rem(rem(k + 32768.0, 65536.0) + 65536.0, 65536.0) - 32768.0;
        }
        *x = k;
    }
    let total = xs[0] * 3600.0 + xs[1] * 60.0 + xs[2];
    if total < 0.0 {
        return e(ErrorCode::Num);
    }
    let t = total / 86400.0;
    Ok(num(t - floor(t)))
}

/// `(p.date ?? 0) + (p.time ?? 0)` of a text argument (`None`: not a text, or neither a date nor a time).
fn date_time_of(v: Scalar, ctx: &dyn FnContext) -> Option<f64> {
    let Scalar::Str(r) = v else { return None };
    let p = parse_date_time_parts(ctx.text(r), ctx.locale(), ctx.date1904(), ctx.year())?;
    Some(p.date.unwrap_or(0.0) + p.time.unwrap_or(0.0))
}

fn datevalue(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // only text (a number → #VALUE!); "25:00" = 1, "1/2/2024 25:00" = the next day
    match date_time_of(sc(a, 0), ctx) {
        Some(t) => Ok(num(floor(t))),
        None => e(ErrorCode::Value),
    }
}

fn timevalue(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    match date_time_of(sc(a, 0), ctx) {
        Some(t) => Ok(num(t - floor(t))),
        None => e(ErrorCode::Value),
    }
}

fn now(_: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    Ok(num(ctx.now()))
}

fn today(_: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    Ok(num(floor(ctx.now())))
}

/// YEAR/MONTH/DAY: a part of the date of the serial rounded to the second.
fn ymd_part(a: &CallArgs<'_>, ctx: &dyn FnContext, part: fn(crate::dates::Ymd) -> f64) -> R<Value> {
    match split(sc(a, 0), ctx) {
        Ok((day, _)) => Ok(num(part(serial_to_ymd(day, ctx.date1904())))),
        Err(c) => e(c),
    }
}

fn year(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ymd_part(a, ctx, |p| p.y)
}
fn month(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ymd_part(a, ctx, |p| p.m)
}
fn day(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    ymd_part(a, ctx, |p| p.d)
}

/// HOUR/MINUTE/SECOND: a part of the second of the day.
fn hms_part(a: &CallArgs<'_>, ctx: &dyn FnContext, part: fn(f64) -> f64) -> R<Value> {
    match split(sc(a, 0), ctx) {
        Ok((_, sec)) => Ok(num(part(sec))),
        Err(c) => e(c),
    }
}

fn hour(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    hms_part(a, ctx, |s| floor(s / 3600.0))
}
fn minute(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    hms_part(a, ctx, |s| rem(floor(s / 60.0), 60.0))
}
fn second(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    hms_part(a, ctx, |s| rem(s, 60.0))
}

fn weekday_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    // the type is validated before the date (WEEKDAY(-1;"x") = #VALUE!); omitted with ";" = 0 → #NUM!
    let mut ty = 1.0;
    if let Some(t) = scalar(a, 1) {
        match tnum(ctx, t) {
            Ok(n) => ty = trunc(n),
            Err(c) => return e(c),
        }
    }
    let day = match split(sc(a, 0), ctx) {
        Ok((d, _)) => d,
        Err(c) => return e(c),
    };
    let w = wd(day, ctx); // 0 = Sunday
    Ok(num(if ty == 1.0 {
        w + 1.0
    } else if ty == 2.0 {
        rem(w + 6.0, 7.0) + 1.0
    } else if ty == 3.0 {
        rem(w + 6.0, 7.0)
    } else if (11.0..=17.0).contains(&ty) {
        let start = rem(ty - 10.0, 7.0);
        rem(w - start + 7.0, 7.0) + 1.0
    } else {
        return e(ErrorCode::Num);
    }))
}

fn weeknum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift_atp(a, ctx, None, |xs, ctx| {
        let d = match atp_date(xs.first(), ctx) {
            Ok(d) => d,
            Err(c) => return e(c),
        };
        let ty = match atp_num(ctx, xs.get(1), Some(1.0)) {
            Ok(n) => trunc(n),
            Err(c) => return e(c),
        };
        if ty == 21.0 {
            return Ok(num(iso_week(d, ctx)));
        }
        let Some(start) = weeknum_start(ty) else { return e(ErrorCode::Num) };
        let y = serial_to_ymd(d, ctx.date1904()).y;
        let jan1 = serial(y, 1.0, 1.0, ctx);
        let off = rem(wd(jan1, ctx) - start + 7.0, 7.0);
        Ok(num(floor((d - jan1 + off) / 7.0) + 1.0))
    })
}

fn isoweeknum(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    match date_arg(sc(a, 0), ctx) {
        Ok(d) => Ok(num(iso_week(d, ctx))),
        Err(c) => e(c),
    }
}

/// EDATE/EOMONTH.
fn months_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext, eom: bool) -> R<Value> {
    lift_atp(a, ctx, None, |xs, ctx| {
        let d = match atp_date(xs.first(), ctx) {
            Ok(d) => d,
            Err(c) => return e(c),
        };
        let k = match atp_num(ctx, xs.get(1), None) {
            Ok(k) => k,
            Err(c) => return e(c),
        };
        nv(add_months(d, trunc(k), eom, ctx))
    })
}

fn edate(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    months_fn(a, ctx, false)
}

fn eomonth(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    months_fn(a, ctx, true)
}

/// A body step that fails with an Excel error (as a value) or a host throw.
macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(c) => return e(c),
        }
    };
}

/// The holidays of a body: an Excel error becomes the element's value, a host throw goes on.
macro_rules! hol {
    ($e:expr) => {
        match $e {
            Ok(h) => h,
            Err(Stop::Err(c)) => return e(c),
            Err(t) => return Err(t),
        }
    };
}

fn networkdays(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift_atp(a, ctx, Some(2), |xs, ctx| {
        let s = tri!(atp_date(xs.first(), ctx));
        let en = tri!(atp_date(xs.get(1), ctx));
        let hol = hol!(holiday_set(xs.get(2), ctx));
        Ok(num(network_days(s, en, &SAT_SUN, &hol, ctx)))
    })
}

fn networkdays_intl(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // the weekend is validated BEFORE the dates (measured: ("x";1;0) = #NUM!)
    lift_atp(a, ctx, Some(3), |xs, ctx| {
        let mask = tri!(atp_weekend(xs.get(2), ctx));
        let s = tri!(atp_date(xs.first(), ctx));
        let en = tri!(atp_date(xs.get(1), ctx));
        let hol = hol!(holiday_set(xs.get(3), ctx));
        Ok(num(network_days(s, en, &mask, &hol, ctx)))
    })
}

fn workday_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift_atp(a, ctx, Some(2), |xs, ctx| {
        let s = tri!(atp_date(xs.first(), ctx));
        let k = tri!(atp_num(ctx, xs.get(1), None));
        let hol = hol!(holiday_set(xs.get(2), ctx));
        nv(workday(s, floor(k), &SAT_SUN, &hol, ctx))
    })
}

fn workday_intl(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // days = 0 returns the start without looking at the weekend; start+days out of range → #NUM! before validating the
    // weekend (measured)
    lift_atp(a, ctx, Some(3), |xs, ctx| {
        let s = tri!(atp_date(xs.first(), ctx));
        let k = tri!(atp_num(ctx, xs.get(1), None));
        let days = floor(k);
        if days == 0.0 {
            return Ok(num(s));
        }
        if s + days < 0.0 || s + days > max_serial(ctx.date1904()) {
            return e(ErrorCode::Num);
        }
        let mask = tri!(atp_weekend(xs.get(2), ctx));
        let hol = hol!(holiday_set(xs.get(3), ctx));
        nv(workday(s, days, &mask, &hol, ctx))
    })
}

fn datedif_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    let s = tri!(date_arg(sc(a, 0), ctx));
    let en = tri!(date_arg(sc(a, 1), ctx));
    let unit = match txt(ctx, sc(a, 2)) {
        Ok(t) => unit_of(t.as_js()),
        Err(c) => return e(c),
    };
    nv(datedif(s, en, unit, ctx))
}

fn days(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    let en = tri!(date_arg(sc(a, 0), ctx));
    let s = tri!(date_arg(sc(a, 1), ctx));
    Ok(num(en - s))
}

fn days360_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let ctx = &*ctx;
    let s = tri!(date_arg(sc(a, 0), ctx));
    let en = tri!(date_arg(sc(a, 1), ctx));
    let mut eu = false;
    match scalar(a, 2) {
        None | Some(Scalar::Empty) => {}
        Some(Scalar::Str(_)) => return e(ErrorCode::Value),
        Some(m) => eu = tri!(tnum(ctx, m)) != 0.0,
    }
    Ok(num(days360(s, en, eu, ctx)))
}

fn yearfrac(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    lift_atp(a, ctx, None, |xs, ctx| {
        let s = tri!(atp_date(xs.first(), ctx));
        let en = tri!(atp_date(xs.get(1), ctx));
        let basis = trunc(tri!(atp_num(ctx, xs.get(2), Some(0.0))));
        if basis < 0.0 || basis > 4.0 {
            return e(ErrorCode::Num);
        }
        Ok(num(year_frac(s, en, basis, ctx.date1904())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        let t = |s: &str| unit_of(JsStr::Latin1(s.as_bytes()));
        assert!(t("d") == Some(Unit::D) && t("yM") == Some(Unit::Ym) && t("md") == Some(Unit::Md));
        assert!(t("x").is_none() && t("ymd").is_none() && t("").is_none());
        // no non-ASCII unit upper-cases to a unit
        assert!(unit_of(JsStr::Utf16(&[0x131])).is_none());
    }
}
