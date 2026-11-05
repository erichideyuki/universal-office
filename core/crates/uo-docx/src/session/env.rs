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
