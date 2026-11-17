//! `FormatLocale`, `PT_BR`, the built-in formats and the per-LCID month/day names of `numfmt.ts`.

use std::sync::OnceLock;

/// Locale of the formatter (`FormatLocale` in the TS). The month and day tables have their fixed
/// lengths in the type (the TS indexes arrays a host could have shortened: `undefined` or a
/// `TypeError`; here a host cannot build such a locale).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatLocale {
    /// Decimal separator (pt-BR `,`).
    pub decimal: String,
    /// Thousands separator (pt-BR `.`).
    pub group: String,
    pub months_short: [String; 12],
    pub months_long: [String; 12],
    pub days_short: [String; 7],
    pub days_long: [String; 7],
    pub ampm: [String; 2],
    /// Overrides of the locale-dependent built-ins (ids 14–22 in Excel), consulted BEFORE
    /// [`builtin_format`]: `(numFmtId, formatCode)` pairs (the TS `Record<number, string>`).
    pub builtins: Vec<(u32, String)>,
    /// Long system date (`[$-F800]`) and system time (`[$-F400]`).
    pub sys_date: Option<String>,
    pub sys_time: Option<String>,
}

/// The names of a language (`LANG_NAMES` entries other than pt).
pub(crate) struct LangNames {
    pub months_short: [String; 12],
    pub months_long: [String; 12],
    pub days_short: [String; 7],
    pub days_long: [String; 7],
    pub ampm: Option<[String; 2]>,
}

fn strs<const N: usize>(a: [&str; N]) -> [String; N] {
    a.map(str::to_owned)
}

fn build_pt_br() -> FormatLocale {
    let b = |id: u32, s: &str| (id, s.to_owned());
    let date = "dd/mm/yyyy";
    FormatLocale {
        decimal: ",".into(),
        group: ".".into(),
        months_short: strs(["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"]),
        months_long: strs([
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
        days_short: strs(["dom", "seg", "ter", "qua", "qui", "sex", "sáb"]),
        days_long: strs(["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"]),
        ampm: strs(["AM", "PM"]),
        // Measured in Excel 365 pt-BR (NumberFormat + Range.Text of every id 0..81): see numfmt.ts
        builtins: vec![
            b(5, "\"R$ \"#,##0;\\-\"R$ \"#,##0"),
            b(6, "\"R$ \"#,##0;[Red]\\-\"R$ \"#,##0"),
            b(7, "\"R$ \"#,##0.00;\\-\"R$ \"#,##0.00"),
            b(8, "\"R$ \"#,##0.00;[Red]\\-\"R$ \"#,##0.00"),
            b(14, date),
            b(15, "dd/mmm/yy"),
            b(16, "dd/mmm"),
            b(17, "mmm/yy"),
            b(20, "hh:mm"),
            b(21, "hh:mm:ss"),
            b(22, "dd/mm/yyyy hh:mm"),
            b(23, "General"),
            b(24, "General"),
            b(25, "General"),
            b(26, "General"),
            b(27, date),
            b(28, date),
            b(29, date),
            b(30, date),
            b(31, date),
            b(32, "hh:mm:ss"),
            b(33, "hh:mm:ss"),
            b(34, "hh:mm:ss"),
            b(35, "hh:mm:ss"),
            b(36, date),
            b(37, "#,##0;\\-#,##0"),
            b(38, "#,##0;[Red]\\-#,##0"),
            b(39, "#,##0.00;\\-#,##0.00"),
            b(40, "#,##0.00;[Red]\\-#,##0.00"),
            b(41, "_-* #,##0_-;\\-* #,##0_-;_-* \"-\"_-;_-@_-"),
            b(42, "_-\"R$ \"* #,##0_-;\\-\"R$ \"* #,##0_-;_-\"R$ \"* \"-\"_-;_-@_-"),
            b(43, "_-* #,##0.00_-;\\-* #,##0.00_-;_-* \"-\"??_-;_-@_-"),
            b(44, "_-\"R$ \"* #,##0.00_-;\\-\"R$ \"* #,##0.00_-;_-\"R$ \"* \"-\"??_-;_-@_-"),
            b(50, date),
            b(51, date),
            b(52, date),
            b(53, date),
            b(54, date),
            b(55, date),
            b(56, date),
            b(57, date),
            b(58, date),
            b(59, "0"),
            b(60, "0.00"),
            b(61, "#,##0"),
            b(62, "#,##0.00"),
            b(63, "\"R$ \"#,##0;\\-\"R$ \"#,##0"),
            b(64, "\"R$ \"#,##0;[Red]\\-\"R$ \"#,##0"),
            b(65, "\"R$ \"#,##0.00;\\-\"R$ \"#,##0.00"),
            b(66, "\"R$ \"#,##0.00;[Red]\\-\"R$ \"#,##0.00"),
            b(67, "0%"),
            b(68, "0.00%"),
            b(69, "# ?/?"),
            b(70, "# ??/??"),
            b(71, date),
            b(72, date),
            b(73, "dd/mmm/yy"),
            b(74, "dd/mmm"),
            b(75, "mmm/yy"),
            b(76, "hh:mm"),
            b(77, "hh:mm:ss"),
            b(78, "dd/mm/yyyy hh:mm"),
            b(79, "mm:ss"),
            b(80, "[h]:mm:ss"),
            b(81, "mm:ss.0"),
        ],
        sys_date: Some("dddd, d\" de \"mmmm\" de \"yyyy".into()),
        sys_time: Some("hh:mm:ss".into()),
    }
}

/// The default locale `PT_BR` (Excel 16.0 pt-BR, lower-case names). One instance per process: the
/// TS compares locales by identity (`names === loc` in `localeFor`), so `ptr::eq` against this
/// reference is what "the PT_BR object" means here.
pub fn pt_br() -> &'static FormatLocale {
    static PT_BR: OnceLock<FormatLocale> = OnceLock::new();
    PT_BR.get_or_init(build_pt_br)
}

/// `LANG_NAMES` for the primary language id of an LCID, other than `0x16` (pt, which is `PT_BR`
/// itself in the TS and is handled by identity in `date::locale_for`).
pub(crate) fn lang_names(id: u32) -> Option<&'static LangNames> {
    static NAMES: OnceLock<Vec<(u32, LangNames)>> = OnceLock::new();
    let all = NAMES.get_or_init(|| {
        let ja_months: [String; 12] = std::array::from_fn(|i| format!("{}月", i + 1));
        vec![
            (
                0x09,
                LangNames {
                    months_short: strs(["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]),
                    months_long: strs([
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
                    days_short: strs(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]),
                    days_long: strs(["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]),
                    ampm: None,
                },
            ),
            (
                0x0c,
                LangNames {
                    months_short: strs(["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."]),
                    months_long: strs([
                        "janvier",
                        "février",
                        "mars",
                        "avril",
                        "mai",
                        "juin",
                        "juillet",
                        "août",
                        "septembre",
                        "octobre",
                        "novembre",
                        "décembre",
                    ]),
                    days_short: strs(["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."]),
                    days_long: strs(["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"]),
                    ampm: None,
                },
            ),
            (
                0x07,
                LangNames {
                    months_short: strs(["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"]),
                    months_long: strs([
                        "Januar",
                        "Februar",
                        "März",
                        "April",
                        "Mai",
                        "Juni",
                        "Juli",
                        "August",
                        "September",
                        "Oktober",
                        "November",
                        "Dezember",
                    ]),
                    days_short: strs(["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]),
                    days_long: strs(["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"]),
                    ampm: None,
                },
            ),
            (
                0x0a,
                LangNames {
                    months_short: strs(["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"]),
                    months_long: strs([
                        "enero",
                        "febrero",
                        "marzo",
                        "abril",
                        "mayo",
                        "junio",
                        "julio",
                        "agosto",
                        "septiembre",
                        "octubre",
                        "noviembre",
                        "diciembre",
                    ]),
                    days_short: strs(["dom", "lun", "mar", "mié", "jue", "vie", "sáb"]),
                    days_long: strs(["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"]),
                    ampm: None,
                },
            ),
            (
                0x10,
                LangNames {
                    months_short: strs(["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"]),
                    months_long: strs([
                        "gennaio",
                        "febbraio",
                        "marzo",
                        "aprile",
                        "maggio",
                        "giugno",
                        "luglio",
                        "agosto",
                        "settembre",
                        "ottobre",
                        "novembre",
                        "dicembre",
                    ]),
                    days_short: strs(["dom", "lun", "mar", "mer", "gio", "ven", "sab"]),
                    days_long: strs(["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"]),
                    ampm: None,
                },
            ),
            (
                0x11,
                LangNames {
                    months_short: ja_months.clone(),
                    months_long: ja_months,
                    days_short: strs(["日", "月", "火", "水", "木", "金", "土"]),
                    days_long: strs(["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"]),
                    ampm: Some(strs(["午前", "午後"])),
                },
            ),
        ]
    });
    all.iter().find(|(k, _)| *k == id).map(|(_, v)| v)
}

/// `BUILTIN_FORMATS`: numFmtId → formatCode present in every styles.xml.
#[must_use]
pub fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "\"$\"#,##0_);(\"$\"#,##0)",
        6 => "\"$\"#,##0_);[Red](\"$\"#,##0)",
        7 => "\"$\"#,##0.00_);(\"$\"#,##0.00)",
        8 => "\"$\"#,##0.00_);[Red](\"$\"#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// `builtinFormatCode(id, locale)`: the code of a built-in numFmtId honouring the locale (ids 14–22
/// are locale dependent). The TS default locale is `PT_BR`: pass [`pt_br()`]. The id is a `u32`
/// (the TS takes a number: a fraction or a negative number finds nothing, as here).
#[must_use]
pub fn builtin_format_code(id: u32, locale: &FormatLocale) -> Option<&str> {
    // the last pair of an id wins, as a later key of a JS object literal does
    locale.builtins.iter().rev().find(|(k, _)| *k == id).map(|(_, v)| v.as_str()).or_else(|| builtin_format(id))
}
