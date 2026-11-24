//! The C1 host hook (decision C1, HANDOFF 2026-11-24): the shell derives the value conventions from `Intl`
//! exactly as the TS does (`i18n/value-locale.ts:24-56`, kept as shell code) and passes them in; Rust never
//! calls `Intl`. The server/.NET path builds them from the measured tables (pt-BR, en-US).

use crate::strings::JsString;

/// Date order of a locale (`fn.ts:82`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateOrder {
    /// Month, day, year.
    Mdy,
    /// Day, month, year.
    Dmy,
    /// Year, month, day.
    Ymd,
}

/// Format-code letters in the calculating locale (`fn.ts:98`): `TEXT(x;"dd/mm/aaaa")` in pt-BR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatLetters {
    /// Year letter.
    pub year: JsString,
    /// Month letter.
    pub month: JsString,
    /// Day letter.
    pub day: JsString,
    /// Hour letter.
    pub hour: JsString,
    /// Minute letter.
    pub minute: JsString,
    /// Second letter.
    pub second: JsString,
}

/// R1C1 row/column letters (`fn.ts:103`; pt-BR `L`/`C`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct R1c1Letters {
    /// Row letter.
    pub row: JsString,
    /// Column letter.
    pub col: JsString,
}

/// Value conventions of the calculating locale: the TS `ValueLocale` (`fn.ts:74-104`) field for field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueLocale {
    /// BCP 47 id (`"pt-BR"`); `pt-BR`/`en-US` select the measured tables.
    pub id: Box<str>,
    /// Decimal separator.
    pub decimal: JsString,
    /// Group separator (may be empty).
    pub group: JsString,
    /// `=""&TRUE` text.
    pub true_text: JsString,
    /// `=""&FALSE` text.
    pub false_text: JsString,
    /// Date order.
    pub date_order: DateOrder,
    /// Date separator.
    pub date_sep: JsString,
    /// Time separator.
    pub time_sep: JsString,
    /// AM designator.
    pub am: JsString,
    /// PM designator.
    pub pm: JsString,
    /// Month names.
    pub month_names: [JsString; 12],
    /// Month abbreviations.
    pub month_abbr: [JsString; 12],
    /// Weekday names, Sunday first.
    pub day_names: [JsString; 7],
    /// Weekday abbreviations, Sunday first.
    pub day_abbr: [JsString; 7],
    /// Currency symbol of the region.
    pub currency: JsString,
    /// Name of the General format (`TEXT(x;"Geral")`).
    pub general_name: Option<JsString>,
    /// Format-code letters.
    pub format_letters: Option<FormatLetters>,
    /// R1C1 letters.
    pub r1c1: Option<R1c1Letters>,
}

/// Everything the engine takes from its host instead of reading the environment.
#[derive(Clone, Debug)]
pub struct HostHooks {
    /// Value conventions (C1).
    pub values: ValueLocale,
    /// The decimal of the formula SYNTAX for a language outside the Excel tables: the shell's
    /// `Intl.NumberFormat(id).formatToParts(1.5)` decimal (`translate.ts:126`, `formula-locale.ts:32`).
    pub regional_decimal: Option<JsString>,
}

/// The clock of one recalc, passed by the host with every call that can evaluate: "now" as an Excel serial
/// in the host's LOCAL time (`nowSerial`, `engine.ts:186-189`, read once per run as `engine.ts:502` caches
/// it; also the current year of `coerce.ts:197` and `NOW`/`TODAY` of conditional formats,
/// `cf-formula.ts:405`, `cond-format.ts:526`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Local now as a serial (1900 system).
    pub now_serial: f64,
}

/// Source of `RAND`/`RANDBETWEEN`/`RANDARRAY` (`engine.ts:503`, `Math.random` by default): captures replay
/// the recorded sequence; the product uses a seeded generator (declared: values differ from V8's).
#[derive(Clone, Debug)]
pub enum RandomSource {
    /// xorshift128+ state.
    Seeded([u64; 2]),
    /// A recorded sequence, consumed in order (parity).
    Sequence(Vec<f64>, usize),
}

impl RandomSource {
    /// A seeded generator (xorshift128+; a zero seed is replaced, the state must not be all zero).
    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        let a = seed ^ 0x9e37_79b9_7f4a_7c15;
        let b = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9) | 1;
        Self::Seeded([a, b])
    }

    /// A recorded sequence (parity replays).
    #[must_use]
    pub fn sequence(values: Vec<f64>) -> Self {
        Self::Sequence(values, 0)
    }

    /// The next value in `[0, 1)` (`Math.random()`). An exhausted sequence keeps returning 0.
    pub fn next_f64(&mut self) -> f64 {
        match self {
            Self::Seeded(s) => {
                let (mut s1, s0) = (s[0], s[1]);
                s[0] = s0;
                s1 ^= s1 << 23;
                s1 ^= s1 >> 17;
                s1 ^= s0;
                s1 ^= s0 >> 26;
                s[1] = s1;
                // the top 53 bits of the sum, as V8 builds a double in [0, 1)
                crate::cast::u53_to_f64(s[1].wrapping_add(s[0]) >> 11) / 9_007_199_254_740_992.0
            }
            Self::Sequence(v, i) => {
                let x = v.get(*i).copied().unwrap_or(0.0);
                *i += 1;
                x
            }
        }
    }

    /// Values consumed so far (a sequence) or 0.
    #[must_use]
    pub fn consumed(&self) -> usize {
        match self {
            Self::Sequence(_, i) => *i,
            Self::Seeded(_) => 0,
        }
    }
}

fn js(s: &str) -> JsString {
    JsString::from_str_js(s)
}

fn js_arr<const N: usize>(v: [&str; N]) -> [JsString; N] {
    v.map(js)
}

impl ValueLocale {
    /// `EN_US` (`fn.ts:107-116`): the default of the TS tests and of the file.
    #[must_use]
    pub fn en_us() -> Self {
        Self {
            id: "en-US".into(),
            decimal: js("."),
            group: js(","),
            true_text: js("TRUE"),
            false_text: js("FALSE"),
            date_order: DateOrder::Mdy,
            date_sep: js("/"),
            time_sep: js(":"),
            am: js("AM"),
            pm: js("PM"),
            month_names: js_arr([
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ]),
            month_abbr: js_arr(["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]),
            day_names: js_arr(["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]),
            day_abbr: js_arr(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]),
            currency: js("$"),
            general_name: Some(js("General")),
            format_letters: Some(FormatLetters {
                year: js("y"),
                month: js("m"),
                day: js("d"),
                hour: js("h"),
                minute: js("m"),
                second: js("s"),
            }),
            r1c1: None,
        }
    }

    /// `PT_BR` (`fn.ts:122-131`): the Excel of the oracle (decimal `,`, `VERDADEIRO`).
    #[must_use]
    pub fn pt_br() -> Self {
        Self {
            id: "pt-BR".into(),
            decimal: js(","),
            group: js("."),
            true_text: js("VERDADEIRO"),
            false_text: js("FALSO"),
            date_order: DateOrder::Dmy,
            date_sep: js("/"),
            time_sep: js(":"),
            am: js("AM"),
            pm: js("PM"),
            month_names: js_arr([
                "janeiro",
                "fevereiro",
                "março",
                "abril",
                "maio",
                "junho",
                "julho",
                "agosto",
                "setembro",
                "outubro",
                "novembro",
                "dezembro",
            ]),
            month_abbr: js_arr(["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"]),
            day_names: js_arr(["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"]),
            day_abbr: js_arr(["dom", "seg", "ter", "qua", "qui", "sex", "sáb"]),
            currency: js("R$"),
            general_name: Some(js("Geral")),
            format_letters: Some(FormatLetters {
                year: js("a"),
                month: js("m"),
                day: js("d"),
                hour: js("h"),
                minute: js("m"),
                second: js("s"),
            }),
            r1c1: None,
        }
    }
}

impl Clock {
    /// The current year of the host's local clock (`new Date().getFullYear()`, `coerce.ts:197`): what a date typed
    /// without a year (`"1/2"`, `"13-jan"`) takes.
    #[must_use]
    pub fn current_year(&self) -> f64 {
        crate::dates::year_of_serial(self.now_serial)
    }
}
