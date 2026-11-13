//! The session's environment (G8): the clock with local date parts, the random source of the TS
//! `Math.random` sites and the locale, injected so that tests and parity captures are
//! deterministic (the captures replace `Math.random` by the testkit xorshift32 and fix `Date`).

/// Calendar parts of one instant (the TS `getFullYear()`, `getMonth() + 1`, `getDate()`,
/// `getHours()`, `getMinutes()`, or their `getUTC*` twins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateParts {
    pub year: i32,
    /// 1–12
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl DateParts {
    /// `${y}-${pad(m)}-${pad(d)}T${pad(h)}:${pad(min)}:00Z` (`newRevision`: Word's `w:date`, seconds
    /// zeroed, a `Z` suffix even on local time).
    #[must_use]
    pub fn word_date(&self) -> String {
        format!("{}-{:02}-{:02}T{:02}:{:02}:00Z", self.year, self.month, self.day, self.hour, self.minute)
    }

    /// `new Date().toLocaleDateString('pt-BR')` (`dd/mm/yyyy`; the DATE fields at install).
    #[must_use]
    pub fn pt_br_date(&self) -> String {
        format!("{:02}/{:02}/{}", self.day, self.month, self.year)
    }
}

/// "Now" in local time and in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalNow {
    pub local: DateParts,
    pub utc: DateParts,
}

impl DateParts {
    /// Wave 7 (B, decision "SetClock validation"): a real calendar instant as a JS `Date` gives it — year inside the
    /// `Date` range (±275,760; the session's date arithmetic and `w:date` assume four digits, so 0–9999 here), month
    /// 1–12, day 1 to the month's length (leap years by the Gregorian rule), hour 0–23, minute 0–59. A host clock is
    /// always valid; a hostile clock (`SetClock` or the open message's; month 15, day 40…) is refused instead of reaching the DATE-field arithmetic,
    /// whose overflow differs from JS (WS-D14).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let leap = (self.year % 4 == 0 && self.year % 100 != 0) || self.year % 400 == 0;
        let days = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return false,
        };
        (0..=9999).contains(&self.year) && (1..=days).contains(&self.day) && self.hour < 24 && self.minute < 60
    }
}

impl LocalNow {
    /// Both instants valid ([`DateParts::is_valid`]); `Err` names the first bad one.
    ///
    /// # Errors
    /// `"local"` or `"utc"`: that instant is not a calendar instant.
    pub fn validated(self) -> Result<LocalNow, &'static str> {
        if !self.local.is_valid() {
            return Err("clock: the local instant is not a calendar date");
        }
        if !self.utc.is_valid() {
            return Err("clock: the UTC instant is not a calendar date");
        }
        Ok(self)
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    const fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateParts {
        DateParts { year, month, day, hour, minute }
    }

    #[test]
    fn hostile_clocks_are_refused() {
        let ok = at(2026, 2, 28, 23, 59);
        assert!(LocalNow { local: ok, utc: ok }.validated().is_ok());
        assert!(at(2024, 2, 29, 0, 0).is_valid());
        for bad in [
            at(2026, 2, 29, 0, 0),
            at(2026, 15, 1, 0, 0),
            at(2026, 0, 1, 0, 0),
            at(2026, 4, 31, 0, 0),
            at(2026, 1, 0, 0, 0),
            at(2026, 1, 40, 0, 0),
            at(2026, 1, 1, 24, 0),
            at(2026, 1, 1, 0, 60),
            at(-1, 1, 1, 0, 0),
            at(10_000, 1, 1, 0, 0),
            at(1900, 2, 29, 0, 0),
        ] {
            assert!(!bad.is_valid(), "{bad:?}");
            assert!(LocalNow { local: bad, utc: ok }.validated().is_err());
            assert!(LocalNow { local: ok, utc: bad }.validated().is_err());
        }
        assert!(LocalNow::default().validated().is_err(), "the zero date (month 0) is not a calendar date");
    }
}

/// The environment of a session.
pub trait Env {
    /// The current instant.
    fn now(&self) -> LocalNow;
    /// The next value of the TS `Math.random()` sites, as the testkit xorshift32 gives it
    /// (`next / 2^32` on the TS side of the captures).
    fn random_u32(&mut self) -> u32;
}

/// A fixed clock and the testkit xorshift32 (tests, parity, hosts without a clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedEnv {
    pub now: LocalNow,
    pub state: u32,
}

impl FixedEnv {
    #[must_use]
    pub fn new(now: LocalNow, seed: u32) -> FixedEnv {
        FixedEnv { now, state: if seed == 0 { 0x9e37_79b9 } else { seed } }
    }
}

impl Default for FixedEnv {
    fn default() -> Self {
        let t = DateParts { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
        FixedEnv::new(LocalNow { local: t, utc: t }, 1)
    }
}

impl Env for FixedEnv {
    fn now(&self) -> LocalNow {
        self.now
    }

    fn random_u32(&mut self) -> u32 {
        // uo-testkit `Rng` (xorshift32)
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }
}
