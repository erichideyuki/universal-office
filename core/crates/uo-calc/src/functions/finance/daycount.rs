//! `functions/daycount.ts`: the day counts of the financial functions (bases 0–4) and the coupon dates — the
//! Analysis ToolPak logic (`ScaDate` of LibreOffice's `analysishelper.cxx`), checked against Excel by the TS.
//!
//! Bases: 0 = US 30/360 (NASD), 1 = actual/actual, 2 = actual/360, 3 = actual/365, 4 = European 30/360. Every value
//! is an `f64` as in the TS (a basis that is NaN takes the `default` branches; a date outside the JS `Date` range
//! has NaN fields). The coupon walks are bounded ([`COUP_CAP`], decision A4c-2): the TS walk of `coupncdDate` never
//! ends once a field is NaN.

use uo_jsnum::math::{min, rem, round, trunc};

use crate::dates::{serial_to_ymd, ymd_to_serial};

/// `isLeap(y)` (Gregorian, no fictitious 1900).
#[inline]
#[must_use]
pub fn is_leap(y: f64) -> bool {
    (rem(y, 4.0) == 0.0 && rem(y, 100.0) != 0.0) || rem(y, 400.0) == 0.0
}

/// `daysInMonth(m, y)` of `daycount.ts` (Gregorian; any month other than 2, 4, 6, 9, 11 has 31 days — NaN too).
#[inline]
#[must_use]
pub fn dim(m: f64, y: f64) -> f64 {
    if m == 2.0 {
        if is_leap(y) { 29.0 } else { 28.0 }
    } else if m == 4.0 || m == 6.0 || m == 9.0 || m == 11.0 {
        30.0
    } else {
        31.0
    }
}

/// The most steps a coupon walk (`couppcdDate`/`coupncdDate`) takes: a walk over finite fields stays within two
/// years of the settlement (≤ 9 steps); past the cap a field is NaN and the TS loop never ends (`#NUM!` here).
pub const COUP_CAP: u32 = 1000;

/// A coupon date with the ATP "last day of month" mode (`CDate`, LibreOffice's `ScaDate`).
#[derive(Clone, Copy, Debug)]
pub struct CDate {
    /// `origDay`.
    pub orig_day: f64,
    /// `day`.
    pub day: f64,
    /// `month`.
    pub month: f64,
    /// `year`.
    pub year: f64,
    /// `lastDayMode`.
    pub last_day_mode: bool,
    /// `lastDay`.
    pub last_day: bool,
    /// `days30`.
    pub days30: bool,
    /// `usMode`.
    pub us_mode: bool,
}

impl CDate {
    /// `CDate.of(serial, basis, date1904)`.
    #[must_use]
    pub fn of(serial: f64, basis: f64, d1904: bool) -> Self {
        let p = serial_to_ymd(serial, d1904);
        let mut c = Self {
            orig_day: p.d,
            day: 1.0,
            month: p.m,
            year: p.y,
            last_day_mode: basis != 5.0,
            last_day: p.d >= dim(p.m, p.y),
            days30: basis == 0.0 || basis == 4.0,
            us_mode: basis == 0.0,
        };
        c.set_day();
        c
    }

    /// `setDay()`.
    pub fn set_day(&mut self) {
        if self.days30 {
            self.day = min(self.orig_day, 30.0);
            if self.last_day || self.day >= dim(self.month, self.year) {
                self.day = 30.0;
            }
        } else {
            let last = dim(self.month, self.year);
            self.day = if self.last_day { last } else { min(self.orig_day, last) };
        }
    }

    /// `daysInMonthOf(m)`.
    #[must_use]
    pub fn days_in_month_of(&self, m: f64) -> f64 {
        if self.days30 { 30.0 } else { dim(m, self.year) }
    }

    /// `daysInMonthRange(from, to)`.
    #[must_use]
    pub fn days_in_month_range(&self, from: f64, to: f64) -> f64 {
        if from > to {
            return 0.0;
        }
        if self.days30 {
            return (to - from + 1.0) * 30.0;
        }
        let mut n = 0.0;
        let mut m = from;
        // months: at most 12 steps (`from` is a month 1..12 or NaN)
        while m <= to {
            n += dim(m, self.year);
            m += 1.0;
        }
        n
    }

    /// `daysInYearRange(from, to)` (years of the JS `Date` range: at most ≈ 550,000 steps).
    #[must_use]
    pub fn days_in_year_range(&self, from: f64, to: f64) -> f64 {
        if from > to {
            return 0.0;
        }
        if self.days30 {
            return (to - from + 1.0) * 360.0;
        }
        let mut n = 0.0;
        let mut y = from;
        while y <= to {
            n += if is_leap(y) { 366.0 } else { 365.0 };
            let ny = y + 1.0;
            if ny == y {
                // a year past 2^53 (unreachable: years come from `Date`, |y| < 300,000)
                break;
            }
            y = ny;
        }
        n
    }

    /// `addMonths(count)`.
    pub fn add_months(&mut self, count: f64) {
        let mut nm = count + self.month;
        if nm > 12.0 {
            nm -= 1.0;
            self.year += trunc(nm / 12.0);
            self.month = rem(nm, 12.0) + 1.0;
        } else if nm < 1.0 {
            self.year += trunc(nm / 12.0) - 1.0;
            self.month = rem(nm, 12.0) + 12.0;
        } else {
            self.month = nm;
        }
        self.set_day();
    }

    /// `addYears(count)`.
    pub fn add_years(&mut self, count: f64) {
        self.year += count;
        self.set_day();
    }

    /// `setYear(y)`.
    pub fn set_year(&mut self, y: f64) {
        self.year = y;
        self.set_day();
    }

    /// `serial(date1904)`: the serial (the last day of the month when the original was the last).
    #[must_use]
    pub fn serial(&self, d1904: bool) -> f64 {
        let last = dim(self.month, self.year);
        let d = if self.last_day_mode && self.last_day { last } else { min(last, self.orig_day) };
        ymd_to_serial(self.year, self.month, d, d1904)
    }

    /// `lt(o)`.
    #[must_use]
    pub fn lt(&self, o: &Self) -> bool {
        if self.year != o.year {
            return self.year < o.year;
        }
        if self.month != o.month {
            return self.month < o.month;
        }
        if self.day != o.day {
            return self.day < o.day;
        }
        if self.last_day || o.last_day {
            return !self.last_day && o.last_day;
        }
        self.orig_day < o.orig_day
    }

    /// `gt(o)`.
    #[must_use]
    pub fn gt(&self, o: &Self) -> bool {
        o.lt(self)
    }

    /// `CDate.diff(from, to)`: days between two dates in their basis (`ScaDate::getDiff`).
    #[must_use]
    pub fn diff(from: &Self, to: &Self) -> f64 {
        if from.gt(to) {
            // `to.gt(from)` cannot hold too (`lt` is a strict order, false on NaN): one swap at most
            return Self::diff(to, from);
        }
        let mut n = 0.0;
        let mut a = *from;
        let mut b = *to;
        if to.days30 {
            if to.us_mode {
                if (from.month == 2.0 || from.day < 30.0) && b.orig_day == 31.0 {
                    b.day = 31.0;
                } else if b.month == 2.0 && b.last_day {
                    b.day = dim(2.0, b.year);
                }
            } else {
                if a.month == 2.0 && a.day == 30.0 {
                    a.day = dim(2.0, a.year);
                }
                if b.month == 2.0 && b.day == 30.0 {
                    b.day = dim(2.0, b.year);
                }
            }
        }
        if a.year < b.year || (a.year == b.year && a.month < b.month) {
            n = a.days_in_month_of(a.month) - a.day + 1.0;
            a.day = 1.0;
            a.orig_day = 1.0;
            a.last_day = false;
            a.add_months(1.0);
            if a.year < b.year {
                n += a.days_in_month_range(a.month, 12.0);
                a.add_months(13.0 - a.month);
                n += a.days_in_year_range(a.year, b.year - 1.0);
                a.add_years(b.year - a.year);
            }
            n += a.days_in_month_range(a.month, b.month - 1.0);
            a.add_months(b.month - a.month);
        }
        n += b.day - a.day;
        uo_jsnum::math::max(n, 0.0)
    }
}

/// `couppcdDate(settle, mat, freq)`: the coupon on or before the settlement (`None`: the walk passed [`COUP_CAP`]).
#[must_use]
pub fn couppcd_date(settle: &CDate, mat: &CDate, freq: f64) -> Option<CDate> {
    let mut d = *mat;
    d.set_year(settle.year);
    if d.lt(settle) {
        d.add_years(1.0);
    }
    let mut steps = 0;
    while d.gt(settle) {
        steps += 1;
        if steps > COUP_CAP {
            return None;
        }
        d.add_months(-12.0 / freq);
    }
    Some(d)
}

/// `coupncdDate(settle, mat, freq)`: the coupon after the settlement (`None`: the TS walk never ends — a NaN field).
#[must_use]
pub fn coupncd_date(settle: &CDate, mat: &CDate, freq: f64) -> Option<CDate> {
    let mut d = *mat;
    d.set_year(settle.year);
    if d.gt(settle) {
        d.add_years(-1.0);
    }
    let mut steps = 0;
    while !d.gt(settle) {
        steps += 1;
        if steps > COUP_CAP {
            return None;
        }
        d.add_months(12.0 / freq);
    }
    Some(d)
}

/// `Bond`: validated settlement, maturity, frequency, basis and date system.
#[derive(Clone, Copy, Debug)]
pub struct Bond {
    /// `settle`.
    pub settle: f64,
    /// `mat`.
    pub mat: f64,
    /// `freq`.
    pub freq: f64,
    /// `basis` (0–4 or NaN).
    pub basis: f64,
    /// `d1904`.
    pub d1904: bool,
}

impl Bond {
    fn s(&self) -> CDate {
        CDate::of(self.settle, self.basis, self.d1904)
    }
    fn m(&self) -> CDate {
        CDate::of(self.mat, self.basis, self.d1904)
    }
}

/// `coupPcd(b)`.
#[must_use]
pub fn coup_pcd(b: &Bond) -> Option<f64> {
    Some(couppcd_date(&b.s(), &b.m(), b.freq)?.serial(b.d1904))
}

/// `coupNcd(b)`.
#[must_use]
pub fn coup_ncd(b: &Bond) -> Option<f64> {
    Some(coupncd_date(&b.s(), &b.m(), b.freq)?.serial(b.d1904))
}

/// `coupDayBs(b)`.
#[must_use]
pub fn coup_day_bs(b: &Bond) -> Option<f64> {
    let s = b.s();
    Some(CDate::diff(&couppcd_date(&s, &b.m(), b.freq)?, &s))
}

/// `coupDays(b)` (basis 1 measured by the TS: from the previous coupon to it + 12/f months without the end-of-month
/// rule).
#[must_use]
pub fn coup_days(b: &Bond) -> Option<f64> {
    if b.basis == 1.0 {
        let pcd = coup_pcd(b)?;
        return Some(edate(pcd, 12.0 / b.freq, b.d1904) - pcd);
    }
    Some((if b.basis == 3.0 { 365.0 } else { 360.0 }) / b.freq)
}

/// `edate(serial, months, d1904)` of `daycount.ts`: adds months and caps the day at the end of the month.
#[must_use]
pub fn edate(serial: f64, months: f64, d1904: bool) -> f64 {
    let p = serial_to_ymd(serial, d1904);
    let mut mm = p.m + months;
    let mut yy = p.y;
    // `months` is 12/f (≤ 12) and `p.m` a month or NaN: at most two steps
    while mm < 1.0 {
        mm += 12.0;
        yy -= 1.0;
    }
    while mm > 12.0 {
        mm -= 12.0;
        yy += 1.0;
    }
    ymd_to_serial(yy, mm, min(p.d, dim(mm, yy)), d1904)
}

/// `days360eu(s, e, d1904)`: European DAYS360 (31 → 30 at both ends).
#[must_use]
pub fn days360eu(s: f64, e: f64, d1904: bool) -> f64 {
    let a = serial_to_ymd(s, d1904);
    let b = serial_to_ymd(e, d1904);
    let d1 = min(a.d, 30.0);
    let d2 = min(b.d, 30.0);
    (b.y - a.y) * 360.0 + (b.m - a.m) * 30.0 + (d2 - d1)
}

/// `coupDaysNc(b)`.
#[must_use]
pub fn coup_days_nc(b: &Bond) -> Option<f64> {
    if b.basis != 0.0 && b.basis != 4.0 {
        let s = b.s();
        return Some(CDate::diff(&s, &coupncd_date(&s, &b.m(), b.freq)?));
    }
    // measured by the TS: in basis 4 it is the European DAYS360 to the next coupon
    if b.basis == 4.0 {
        return Some(days360eu(b.settle, coup_ncd(b)?, b.d1904));
    }
    Some(coup_days(b)? - coup_day_bs(b)?)
}

/// `coupNum(b)`.
#[must_use]
pub fn coup_num(b: &Bond) -> Option<f64> {
    let mat = b.m();
    let d = couppcd_date(&b.s(), &mat, b.freq)?;
    let months = (mat.year - d.year) * 12.0 + mat.month - d.month;
    Some(trunc((months * b.freq) / 12.0))
}

/// `diffDate(s, e, basis, d1904)` (`GetDiffDate` of the ATP): `(days, days in the year)`.
#[must_use]
pub fn diff_date(s: f64, e: f64, basis: f64, d1904: bool) -> (f64, f64) {
    let neg = s > e;
    let (s, e) = if neg { (e, s) } else { (s, e) };
    let (mut days, year);
    if basis == 0.0 || basis == 4.0 {
        let a = serial_to_ymd(s, d1904);
        let b = serial_to_ymd(e, d1904);
        days = (b.m - a.m + (b.y - a.y) * 12.0) * 30.0 + (b.d - a.d);
        if basis == 0.0 && a.m == 2.0 && b.m != 2.0 && a.y == b.y {
            days -= if is_leap(a.y) { 1.0 } else { 2.0 };
        }
        year = 360.0;
    } else {
        days = e - s;
        year = if basis == 1.0 {
            if is_leap(serial_to_ymd(s, d1904).y) { 366.0 } else { 365.0 }
        } else if basis == 2.0 {
            360.0
        } else {
            365.0
        };
    }
    (if neg { -days } else { days }, year)
}

/// `yearFracParts(s, e, basis, d1904)`: the days and the days in the year of `YEARFRAC` (the fraction is days/year).
#[must_use]
pub fn year_frac_parts(s: f64, e: f64, basis: f64, d1904: bool) -> (f64, f64) {
    let yf = year_frac(s, e, basis, d1904);
    let (s, e) = if s > e { (e, s) } else { (s, e) };
    let days = if basis == 0.0 || basis == 4.0 { round(yf * 360.0) } else { e - s };
    let year = if basis == 0.0 || basis == 2.0 || basis == 4.0 {
        360.0
    } else if basis == 3.0 || yf == 0.0 {
        365.0
    } else {
        days / yf
    };
    (days, year)
}

/// `daysInYear(serial, basis, d1904)` (`GetDaysInYear` of the ATP).
#[must_use]
pub fn days_in_year(serial: f64, basis: f64, d1904: bool) -> f64 {
    if basis == 1.0 {
        return if is_leap(serial_to_ymd(serial, d1904).y) { 366.0 } else { 365.0 };
    }
    if basis == 3.0 { 365.0 } else { 360.0 }
}

/// `yearFrac(s, e, basis, d1904)` (`GetYearFrac` of the ATP) = `YEARFRAC`.
#[must_use]
pub fn year_frac(s: f64, e: f64, basis: f64, d1904: bool) -> f64 {
    if s == e {
        return 0.0;
    }
    let (s, e) = if s > e { (e, s) } else { (s, e) };
    let a = serial_to_ymd(s, d1904);
    let b = serial_to_ymd(e, d1904);
    let (mut d1, m1, y1) = (a.d, a.m, a.y);
    let (mut d2, m2, y2) = (b.d, b.m, b.y);
    let day_diff = if basis == 0.0 {
        if d1 == 31.0 {
            d1 -= 1.0;
        }
        if d1 == 30.0 && d2 == 31.0 {
            d2 -= 1.0;
        } else if m1 == 2.0 && d1 == (if is_leap(y1) { 29.0 } else { 28.0 }) {
            d1 = 30.0;
            if m2 == 2.0 && d2 == (if is_leap(y2) { 29.0 } else { 28.0 }) {
                d2 = 30.0;
            }
        }
        (y2 - y1) * 360.0 + (m2 - m1) * 30.0 + (d2 - d1)
    } else if basis == 4.0 {
        if d1 == 31.0 {
            d1 -= 1.0;
        }
        if d2 == 31.0 {
            d2 -= 1.0;
        }
        (y2 - y1) * 360.0 + (m2 - m1) * 30.0 + (d2 - d1)
    } else {
        e - s
    };
    let days_in_year = if basis == 0.0 || basis == 2.0 || basis == 4.0 {
        360.0
    } else if basis == 3.0 {
        365.0
    } else {
        let different_year = y1 != y2;
        if different_year && (y2 != y1 + 1.0 || m1 < m2 || (m1 == m2 && d1 < d2)) {
            let mut count = 0.0;
            let mut y = y1;
            // years of the JS `Date` range: at most ≈ 550,000 steps
            while y <= y2 {
                count += if is_leap(y) { 366.0 } else { 365.0 };
                y += 1.0;
            }
            count / (y2 - y1 + 1.0)
        } else {
            let leap = if different_year {
                // within one year: leap if 29/02 lies between the dates
                (is_leap(y1) && (m1 < 2.0 || (m1 == 2.0 && d1 <= 29.0))) || (is_leap(y2) && (m2 > 2.0 || (m2 == 2.0 && d2 == 29.0)))
            } else {
                is_leap(y1)
            };
            if leap { 366.0 } else { 365.0 }
        }
    };
    day_diff / days_in_year
}
