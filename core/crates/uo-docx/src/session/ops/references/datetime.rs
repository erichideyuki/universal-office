//! The Date and Time formats of the Insert menu (`hf-templates.ts` `DATE_TIME_FORMATS`) and the
//! clock of the session as a `Date` (`formatWordDate(new Date(), pattern, uiLocale(lang))`).
//!
//! The session's clock has no seconds (`DateParts`): `ss` shows 00 (declared, F10-D9). The TS
//! `formatWordDate` takes the PT tables for every non-English locale, so Spanish dates carry
//! Portuguese month and day names (the quirk is the TS's and is kept: F10-D10).

use crate::session::env::LocalNow;
use crate::session::ops::styles::UiLang;
use crate::session::ops::tables::sort::days_from_civil;
use crate::word::datefmt::{LocalDateTime, format_word_date};

/// `DATE_TIME_FORMATS[lang]`.
#[must_use]
pub fn date_time_formats(lang: UiLang) -> &'static [&'static str] {
    const PT: [&str; 15] = [
        "dd/MM/yyyy",
        "dddd, d' de 'MMMM' de 'yyyy",
        "d' de 'MMMM' de 'yyyy",
        "dd/MM/yy",
        "yyyy-MM-dd",
        "d-MMM-yy",
        "dd.MM.yyyy",
        "d MMM. yy",
        "d' de 'MMMM' de 'yy",
        "MMMM' de 'yy",
        "MMM-yy",
        "dd/MM/yyyy HH:mm",
        "dd/MM/yyyy HH:mm:ss",
        "HH:mm",
        "HH:mm:ss",
    ];
    const EN: [&str; 17] = [
        "M/d/yyyy",
        "dddd, MMMM d, yyyy",
        "MMMM d, yyyy",
        "M/d/yy",
        "yyyy-MM-dd",
        "d-MMM-yy",
        "M.d.yyyy",
        "MMM. d, yy",
        "d MMMM yyyy",
        "MMMM yy",
        "MMM-yy",
        "M/d/yyyy h:mm AM/PM",
        "M/d/yyyy h:mm:ss AM/PM",
        "h:mm AM/PM",
        "h:mm:ss AM/PM",
        "HH:mm",
        "HH:mm:ss",
    ];
    match lang {
        // the Spanish list is the Portuguese one
        UiLang::Pt | UiLang::Es => &PT,
        UiLang::En => &EN,
    }
}

/// `uiLocale(lang)`: `{ en: 'en-US', pt: 'pt-BR', es: 'es-ES' }`.
#[must_use]
pub fn ui_locale(lang: UiLang) -> &'static str {
    match lang {
        UiLang::En => "en-US",
        UiLang::Pt => "pt-BR",
        UiLang::Es => "es-ES",
    }
}

/// The session's local "now" as the `Date` getters answer it (seconds 0).
#[must_use]
pub fn local_now(now: &LocalNow) -> LocalDateTime {
    let p = &now.local;
    let days = days_from_civil(i64::from(p.year), i64::from(p.month), i64::from(p.day));
    let ms = days as f64 * 86_400_000.0 + f64::from(p.hour) * 3_600_000.0 + f64::from(p.minute) * 60_000.0;
    LocalDateTime::from_local_time(ms)
}

/// `formatWordDate(now, pattern, uiLocale(lang))` as UTF-16.
#[must_use]
pub fn format_now(now: &LocalNow, pattern: &[u16], lang: UiLang) -> Vec<u16> {
    format_word_date(&local_now(now), pattern, ui_locale(lang))
}

/// `dateTimeFormats(lang)`: the patterns with their sample for `now`.
#[must_use]
pub fn date_time_samples(lang: UiLang, now: &LocalNow) -> Vec<(&'static str, Vec<u16>)> {
    date_time_formats(lang)
        .iter()
        .map(|p| {
            let pic: Vec<u16> = p.encode_utf16().collect();
            (*p, format_now(now, &pic, lang))
        })
        .collect()
}
