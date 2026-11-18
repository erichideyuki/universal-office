//! Date and time fields (`<a:fld type="datetime…">`) as PowerPoint draws them (Slides wave 5, `!SlidesRules::ts_date_locale`):
//! PowerPoint replaces the cached text of a `datetime`, `datetime1`…`datetime13` or `datetimeFigureOut` field with the
//! CURRENT date and time in the format of that number in the field's LANGUAGE (`a:rPr lang`; none: the system's), the lists
//! of its Date and Time dialog. The TS draws the cached text.
//!
//! Measured in PowerPoint 365 (pt-BR Windows) over 16 languages × 14 types (decks `d2-*`, record `uo-dml-render.md`
//! "Date and time fields"): the tables below are those outputs written as patterns; the month and day names are Windows'
//! (NLS) except where PowerPoint writes others (Russian stand-alone months in lower case). Some East Asian entries are the
//! SYSTEM's short or long date (`ja-JP` 1–2, `zh-CN` 1 and 9, `ko-KR` 1, 8, 9) and the `ja`/`zh`/`ko` `d-MMM-yy` uses the
//! system's month abbreviation: here the system is [`FieldClock::default_lang`]. A language outside the tables formats in
//! the default language (PowerPoint has more lists than were measured).
//!
//! The clock is an input ([`FieldClock`], the local time of the host): without one the cached text stays, so a test or a
//! server run is deterministic unless it sets a clock.

/// The local date and time the fields show, and the language of a field without one (PowerPoint: the system's).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldClock {
    pub year: i32,
    /// 1–12
    pub month: u32,
    /// 1–31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// BCP 47 tag of the fields without a language (the host's / system's), e.g. `pt-BR`.
    pub default_lang: Box<str>,
}

/// Is `kind` a date/time field type that PowerPoint updates?
#[must_use]
pub fn is_datetime_field(kind: &str) -> bool {
    kind == "datetime" || kind == "datetimeFigureOut" || datetime_index(kind).is_some()
}

fn datetime_index(kind: &str) -> Option<usize> {
    let n: usize = kind.strip_prefix("datetime")?.parse().ok()?;
    (1..=13).contains(&n).then_some(n)
}

/// The text PowerPoint shows for a date/time field of type `kind` in language `lang` at `clock` (`None` for a type that
/// is not a date/time field).
#[must_use]
pub fn format_datetime_field(kind: &str, lang: Option<&str>, clock: &FieldClock) -> Option<String> {
    let n = match kind {
        "datetime" | "datetimeFigureOut" => 1,
        k => datetime_index(k)?,
    };
    let sys = locale(&clock.default_lang).unwrap_or(&EN_US);
    let loc = lang.filter(|l| !l.is_empty()).and_then(locale).unwrap_or(sys);
    let pat = *loc.patterns.get(n - 1)?;
    Some(render(pat, loc, sys, clock))
}

struct Locale {
    tags: &'static [&'static str],
    months: [&'static str; 12],
    /// the genitive (in-date) month names (`MMMMg`)
    months_gen: [&'static str; 12],
    abbr: [&'static str; 12],
    days: [&'static str; 7],
    am_pm: [&'static str; 2],
    /// `datetime1`…`datetime13`
    patterns: [&'static str; 13],
}

/// Pattern tokens: `d dd dddd M MM MMM MMMM MMMMg yy yyyy H HH h hh m mm s ss tt`, `'…'` literal, `\x01` the system's
/// short date (its `datetime1`), `\x02` the system's long date (its `datetime2`), `SMMM` the system's month abbreviation,
/// `ggg` the Japanese era name, `e` its year, `K…` the kanji form of the next number token, `D` the Korean Dangi year.
const EN_US: Locale = Locale {
    tags: &["en-us", "en"],
    months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    months_gen: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    abbr: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    days: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
    am_pm: ["AM", "PM"],
    patterns: [
        "M/d/yyyy",
        "dddd, MMMM d, yyyy",
        "d MMMM yyyy",
        "MMMM d, yyyy",
        "d-MMM-yy",
        "MMMM yy",
        "MMM-yy",
        "M/d/yyyy h:mm tt",
        "M/d/yyyy h:mm:ss tt",
        "HH:mm",
        "HH:mm:ss",
        "h:mm tt",
        "h:mm:ss tt",
    ],
};

const EN_GB: Locale = Locale {
    tags: &["en-gb"],
    patterns: [
        "dd/MM/yyyy",
        "dddd, d MMMM yyyy",
        "d MMMM, yyyy",
        "d MMMM yyyy",
        "d-MMM-yy",
        "MMMM yy",
        "MMM-yy",
        "dd/MM/yyyy HH:mm",
        "dd/MM/yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "h:mm tt",
        "h:mm:ss tt",
    ],
    ..EN_US
};

const PT_MONTHS: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
const PT_BR: Locale = Locale {
    tags: &["pt-br", "pt", "pt-pt"],
    months: PT_MONTHS,
    months_gen: PT_MONTHS,
    abbr: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
    days: ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"],
    am_pm: ["", ""],
    patterns: [
        "dd/MM/yyyy",
        "dddd, d' de 'MMMM' de 'yyyy",
        "dd.MM.yy",
        "d' de 'MMMM' de 'yyyy",
        "dd-MMM-yy",
        "MMMM' de 'yy",
        "MMM-yy",
        "dd/MM/yyyy HH:mm",
        "dd/MM/yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const DE_MONTHS: [&str; 12] =
    ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const DE_DE: Locale = Locale {
    tags: &["de-de", "de"],
    months: DE_MONTHS,
    months_gen: DE_MONTHS,
    abbr: ["Jan", "Feb", "Mrz", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
    days: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
    am_pm: ["", ""],
    patterns: [
        "dd.MM.yyyy",
        "dddd, d. MMMM yyyy",
        "dd/MM/yy",
        "d. MMMM yyyy",
        "yy-MM-dd",
        "MMMM yy",
        "MMM-yy",
        "dd.MM.yyyy HH:mm",
        "dd.MM.yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const FR_MONTHS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const FR_FR: Locale = Locale {
    tags: &["fr-fr", "fr"],
    months: FR_MONTHS,
    months_gen: FR_MONTHS,
    abbr: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
    days: ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
    am_pm: ["", ""],
    patterns: [
        "dd/MM/yyyy",
        "dddd d MMMM yyyy",
        "dd.MM.yy",
        "d MMMM yyyy",
        "dd-MMM-yy",
        "MMMM yy",
        "MMM-yy",
        "dd/MM/yyyy HH:mm",
        "dd/MM/yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const ES_MONTHS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const ES_ES: Locale = Locale {
    tags: &["es-es", "es"],
    months: ES_MONTHS,
    months_gen: ES_MONTHS,
    abbr: ["ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sep.", "oct.", "nov.", "dic."],
    days: ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"],
    am_pm: ["", ""],
    patterns: [
        "dd/MM/yyyy",
        "dddd, d' de 'MMMM' de 'yyyy",
        "dd.MM.yy",
        "d' de 'MMMM' de 'yyyy",
        "dd-MMM-yy",
        "MMMM' de 'yyyy",
        "MMM-yy",
        "dd/MM/yyyy H:mm",
        "dd/MM/yyyy H:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const ES_MX: Locale = Locale {
    tags: &["es-mx", "es-419", "es-us"],
    am_pm: ["a. m.", "p. m."],
    patterns: [
        "dd/MM/yyyy",
        "dddd, d' de 'MMMM' de 'yyyy",
        "dd.MM.yy",
        "d' de 'MMMM' de 'yyyy",
        "dd-MMM-yy",
        "MMMM' de 'yyyy",
        "MMM-yy",
        "dd/MM/yyyy hh:mm tt",
        "dd/MM/yyyy hh:mm:ss tt",
        "HH:mm",
        "HH:mm:ss",
        "h:mm tt",
        "h:mm:ss tt",
    ],
    ..ES_ES
};

const IT_MONTHS: [&str; 12] =
    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
const IT_IT: Locale = Locale {
    tags: &["it-it", "it"],
    months: IT_MONTHS,
    months_gen: IT_MONTHS,
    abbr: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
    days: ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"],
    am_pm: ["", ""],
    patterns: [
        "dd/MM/yyyy",
        "dddd d MMMM yyyy",
        "MMM'. \u{2019}'yy",
        "d MMMM yyyy",
        "dd-MM'.-'yy",
        "MMMM' \u{2019}'yy",
        "MMM-yy",
        "dd/MM/yyyy HH:mm",
        "dd/MM/yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const NL_MONTHS: [&str; 12] =
    ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"];
const NL_NL: Locale = Locale {
    tags: &["nl-nl", "nl"],
    months: NL_MONTHS,
    months_gen: NL_MONTHS,
    abbr: ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"],
    days: ["zondag", "maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag"],
    am_pm: ["", ""],
    patterns: [
        "d-M-yyyy",
        "dddd d MMMM yyyy",
        "dd/MM/yy",
        "d MMMM yyyy",
        "d-MMM-yy",
        "MMMM' \u{2019}'yy",
        "MMM-yy",
        "d-M-yyyy HH:mm",
        "d-M-yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const JA_JP: Locale = Locale {
    tags: &["ja-jp", "ja"],
    months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    months_gen: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    abbr: ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"],
    days: ["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"],
    am_pm: ["午前", "午後"],
    patterns: [
        "\u{1}",
        "\u{2}",
        "ggge'年'M'月'd'日'",
        "yyyy'年'M'月'd'日'",
        "dd-SMMM-yy",
        "ggge'年'M'月'",
        "gggKe'年'KM'月'Kd'日'",
        "yy/MM/dd H'時'm'分'",
        "yy/MM/dd H'時'm'分's'秒'",
        "HH:mm",
        "HH:mm:ss",
        "H'時'm'分'",
        "H'時'm'分's'秒'",
    ],
};

const ZH_CN: Locale = Locale {
    tags: &["zh-cn", "zh", "zh-sg", "zh-hans"],
    months: ["一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"],
    months_gen: ["一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"],
    abbr: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
    days: ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"],
    am_pm: ["上午", "下午"],
    patterns: [
        "\u{1}",
        "yyyy'年'M'月'd'日'",
        "dd.MM.yy",
        "yyyy'年'M'月'd'日'dddd",
        "dd-SMMM-yy",
        "yyyy'年'M'月'",
        "SMMM-yy",
        "yyyy'年'M'月'd'日'H'时'm'分'",
        "\u{1} HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "tth'时'm'分'",
        "tth'时'm'分's'秒'",
    ],
};

const KO_KR: Locale = Locale {
    tags: &["ko-kr", "ko"],
    months: ["1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월", "12월"],
    months_gen: ["1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월", "12월"],
    abbr: ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"],
    days: ["일요일", "월요일", "화요일", "수요일", "목요일", "금요일", "토요일"],
    am_pm: ["오전", "오후"],
    patterns: [
        "\u{1}",
        "yyyy'년 'M'월 'd'일 'dddd",
        "'단기 'D'년 'M'월 'd'일'",
        "yyyy'년 'M'월 'd'일'",
        "dd-SMMM-yy",
        "yyyy'년 'M'월'",
        "'단기 'D'년 'M'월'",
        "\u{1} HH:mm",
        "\u{1} HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "tt h'시 'm'분'",
        "tt h'시 'm'분 's'초'",
    ],
};

const RU_RU: Locale = Locale {
    tags: &["ru-ru", "ru"],
    // PowerPoint writes the stand-alone month in lower case (`ноябрь 26`; Windows: `Ноябрь`)
    months: ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь"],
    months_gen: ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"],
    abbr: ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"],
    days: ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"],
    am_pm: ["", ""],
    patterns: [
        "dd.MM.yyyy",
        "dddd, d MMMMg yyyy' г.'",
        "dd/MM/yy",
        "d MMMMg yyyy' г.'",
        "dd-MMM-yy",
        "MMMM yy",
        "MMM-yy",
        "dd.MM.yyyy H:mm",
        "dd.MM.yyyy H:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "H:mm",
        "H:mm:ss",
    ],
};

const PL_PL: Locale = Locale {
    tags: &["pl-pl", "pl"],
    months: [
        "styczeń",
        "luty",
        "marzec",
        "kwiecień",
        "maj",
        "czerwiec",
        "lipiec",
        "sierpień",
        "wrzesień",
        "październik",
        "listopad",
        "grudzień",
    ],
    months_gen: [
        "stycznia",
        "lutego",
        "marca",
        "kwietnia",
        "maja",
        "czerwca",
        "lipca",
        "sierpnia",
        "września",
        "października",
        "listopada",
        "grudnia",
    ],
    abbr: ["sty", "lut", "mar", "kwi", "maj", "cze", "lip", "sie", "wrz", "paź", "lis", "gru"],
    days: ["niedziela", "poniedziałek", "wtorek", "środa", "czwartek", "piątek", "sobota"],
    am_pm: ["AM", "PM"],
    patterns: [
        "d.MM.yyyy",
        "dddd, d MMMMg yyyy",
        "dd/MM/yy",
        "d MMMMg yyyy",
        "dd-MMM-yy",
        "MMMM yy",
        "MMM-yy",
        "d.MM.yyyy HH:mm",
        "d.MM.yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
        "h:mm tt",
        "h:mm:ss tt",
    ],
};

const SV_MONTHS: [&str; 12] =
    ["januari", "februari", "mars", "april", "maj", "juni", "juli", "augusti", "september", "oktober", "november", "december"];
const SV_SE: Locale = Locale {
    tags: &["sv-se", "sv"],
    months: SV_MONTHS,
    months_gen: SV_MONTHS,
    abbr: ["jan", "feb", "mar", "apr", "maj", "jun", "jul", "aug", "sep", "okt", "nov", "dec"],
    days: ["söndag", "måndag", "tisdag", "onsdag", "torsdag", "fredag", "lördag"],
    am_pm: ["", ""],
    patterns: [
        "yyyy-MM-dd",
        "dddd' den 'd MMMM yyyy",
        "yy-MM-dd",
        "d MMMM yyyy",
        "dd MMM -yy",
        "MMMM yyyy",
        "'den 'd MMMM yyyy",
        "yyyy-MM-dd HH:mm",
        "yy-MM-dd HH:mm",
        "HH:mm",
        "HH:mm:ss",
        "H.mm",
        "H.mm",
    ],
};

const LOCALES: [&Locale; 15] =
    [&EN_US, &EN_GB, &PT_BR, &DE_DE, &FR_FR, &ES_ES, &ES_MX, &IT_IT, &NL_NL, &JA_JP, &ZH_CN, &KO_KR, &RU_RU, &PL_PL, &SV_SE];

/// The table of a language: the exact tag, then its primary subtag (`de-AT` → `de`).
fn locale(tag: &str) -> Option<&'static Locale> {
    let low = tag.to_ascii_lowercase().replace('_', "-");
    if let Some(l) = LOCALES.iter().find(|l| l.tags.contains(&low.as_str())) {
        return Some(l);
    }
    let primary = low.split('-').next().unwrap_or("");
    LOCALES.iter().find(|l| l.tags.contains(&primary)).copied()
}

/// Day of the week, 0 = Sunday (proleptic Gregorian).
fn weekday(y: i32, m: u32, d: u32) -> usize {
    let (y, m) = if m < 3 { (i64::from(y) - 1, i64::from(m) + 12) } else { (i64::from(y), i64::from(m)) };
    let k = y.rem_euclid(100);
    let j = y.div_euclid(100);
    // Zeller: 0 = Saturday
    let h = (i64::from(d) + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    usize::try_from((h + 6).rem_euclid(7)).unwrap_or(0)
}

/// The Japanese era of a date: name and year.
fn japanese_era(y: i32, m: u32, d: u32) -> (&'static str, i32) {
    let ymd = (y, m, d);
    if ymd >= (2019, 5, 1) {
        ("令和", y - 2018)
    } else if ymd >= (1989, 1, 8) {
        ("平成", y - 1988)
    } else if ymd >= (1926, 12, 25) {
        ("昭和", y - 1925)
    } else {
        ("大正", y - 1911)
    }
}

/// A number in kanji (`十一`, `二十八`, `二千二十六`).
fn kanji(n: i64) -> String {
    const D: [&str; 10] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if n <= 0 {
        return D[0].to_owned();
    }
    let mut out = String::new();
    let mut rest = n;
    for (unit, name) in [(1000, "千"), (100, "百"), (10, "十")] {
        let q = rest / unit;
        if q > 0 {
            if q > 1 {
                out.push_str(D.get(usize::try_from(q % 10).unwrap_or(0)).copied().unwrap_or(""));
            }
            out.push_str(name);
        }
        rest %= unit;
    }
    if rest > 0 {
        out.push_str(D.get(usize::try_from(rest).unwrap_or(0)).copied().unwrap_or(""));
    }
    out
}

fn render(pat: &str, loc: &Locale, sys: &Locale, c: &FieldClock) -> String {
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut kanji_next = false;
    let month_ix = usize::try_from(c.month.clamp(1, 12) - 1).unwrap_or(0);
    let h12 = if c.hour.is_multiple_of(12) { 12 } else { c.hour % 12 };
    let num = |out: &mut String, v: i64, width: usize, kj: bool| {
        if kj {
            out.push_str(&kanji(v));
        } else {
            out.push_str(&format!("{v:0width$}"));
        }
    };
    while let Some(&ch) = chars.get(i) {
        let run = chars.get(i..).map_or(0, |r| r.iter().take_while(|&&x| x == ch).count());
        match ch {
            '\'' => {
                let end = chars.get(i + 1..).and_then(|r| r.iter().position(|&x| x == '\'')).map_or(chars.len(), |p| i + 1 + p);
                out.extend(chars.get(i + 1..end).unwrap_or(&[]));
                i = end + 1;
                continue;
            }
            '\u{1}' => out.push_str(&render(sys.patterns[0], sys, sys, c)),
            '\u{2}' => out.push_str(&render(sys.patterns[1], sys, sys, c)),
            'K' => {
                kanji_next = true;
                i += 1;
                continue;
            }
            'S' if chars.get(i + 1..i + 4) == Some(&['M', 'M', 'M'][..]) => {
                out.push_str(sys.abbr[month_ix]);
                i += 4;
                continue;
            }
            'd' => {
                match run {
                    1 => num(&mut out, i64::from(c.day), 1, kanji_next),
                    2 => num(&mut out, i64::from(c.day), 2, kanji_next),
                    _ => out.push_str(loc.days[weekday(c.year, c.month, c.day)]),
                }
                kanji_next = false;
            }
            'M' => {
                match run {
                    1 => num(&mut out, i64::from(c.month), 1, kanji_next),
                    2 => num(&mut out, i64::from(c.month), 2, kanji_next),
                    3 => out.push_str(loc.abbr[month_ix]),
                    _ => {
                        if chars.get(i + run) == Some(&'g') {
                            out.push_str(loc.months_gen[month_ix]);
                            i += run + 1;
                            kanji_next = false;
                            continue;
                        }
                        out.push_str(loc.months[month_ix]);
                    }
                }
                kanji_next = false;
            }
            'y' => {
                if run <= 2 {
                    num(&mut out, i64::from(c.year).rem_euclid(100), 2, false);
                } else {
                    num(&mut out, i64::from(c.year), 4, false);
                }
            }
            'H' => num(&mut out, i64::from(c.hour), run.min(2), false),
            'h' => num(&mut out, i64::from(h12), run.min(2), false),
            'm' => num(&mut out, i64::from(c.minute), run.min(2), false),
            's' => num(&mut out, i64::from(c.second), run.min(2), false),
            't' => out.push_str(loc.am_pm[usize::from(c.hour >= 12)]),
            'g' => out.push_str(japanese_era(c.year, c.month, c.day).0),
            'e' => {
                let y = japanese_era(c.year, c.month, c.day).1;
                num(&mut out, i64::from(y), 1, kanji_next);
                kanji_next = false;
            }
            'D' => num(&mut out, i64::from(c.year) + 2333, 1, false),
            other => {
                for _ in 0..run {
                    out.push(other);
                }
            }
        }
        i += run;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanji_numbers() {
        assert_eq!(kanji(8), "八");
        assert_eq!(kanji(11), "十一");
        assert_eq!(kanji(18), "十八");
        assert_eq!(kanji(20), "二十");
        assert_eq!(kanji(2026), "二千二十六");
    }

    #[test]
    fn weekdays() {
        assert_eq!(weekday(2026, 11, 18), 3);
        assert_eq!(weekday(2000, 1, 1), 6);
        assert_eq!(weekday(2024, 2, 29), 4);
    }
}
