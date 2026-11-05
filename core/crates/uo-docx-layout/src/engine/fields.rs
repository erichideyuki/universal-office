//! Dynamic fields: the answer of `ParaEnv.field` per `FieldSource` (`fieldValue(ps, block)`,
//! engine.ts:1099, the TS closure, LA3). F7a answers PAGE (with the chapter number of
//! `chapterOf`), NUMPAGES (`numPagesShown`), SECTIONPAGES, SECTION, DATE/TIME and
//! SAVEDATE/CREATEDATE/PRINTDATE; SEQ (`seqValue`), REF, NOTEREF, STYLEREF, PAGEREF and the
//! endnote's own mark are F7c (`Unported`). Field texts are capped by `max_field_text` (F2-4).

use crate::error::{Frontier, LResult, LayoutError};
use crate::keys::PageIx;
use crate::word;

use super::{Cx, Engine};

/// One heading of the chapter index: the page where it starts and its number text.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterMark {
    pub page: f64,
    pub text: Box<[u16]>,
}

fn utf16(s: &str) -> Box<[u16]> {
    s.encode_utf16().collect()
}

/// `new Date(iso).getTime()` for the ISO forms of the ECMAScript date-time string format
/// (`YYYY`, `YYYY-MM`, `YYYY-MM-DD`, optionally `THH:mm`, `:ss`, `.fff…`, then `Z` or `±HH:mm`):
/// a date-only form is UTC, a date-time without offset is local time. `None` for any other string
/// (V8's legacy grammar: not ported, F7a-7).
fn parse_iso_date(s: &str, tz: &crate::options::TimeZoneRule) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let digits = |i: &mut usize, n: usize| -> Option<f64> {
        let part = b.get(*i..*i + n)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        *i += n;
        Some(part.iter().fold(0.0, |a, d| a * 10.0 + f64::from(d - b'0')))
    };
    let year = match b.first() {
        Some(b'+' | b'-') => {
            let neg = b.first() == Some(&b'-');
            i = 1;
            let y = digits(&mut i, 6)?;
            if neg && y == 0.0 {
                return None;
            }
            if neg { -y } else { y }
        }
        _ => digits(&mut i, 4)?,
    };
    let (mut month, mut day) = (1.0, 1.0);
    if b.get(i) == Some(&b'-') {
        i += 1;
        month = digits(&mut i, 2)?;
        if b.get(i) == Some(&b'-') {
            i += 1;
            day = digits(&mut i, 2)?;
        }
    }
    let (mut h, mut mi, mut sec, mut ms) = (0.0, 0.0, 0.0, 0.0);
    let mut has_time = false;
    let mut offset: Option<f64> = None;
    if b.get(i) == Some(&b'T') {
        has_time = true;
        i += 1;
        h = digits(&mut i, 2)?;
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        mi = digits(&mut i, 2)?;
        if b.get(i) == Some(&b':') {
            i += 1;
            sec = digits(&mut i, 2)?;
            if b.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                // milliseconds: the first three digits (the rest truncated, as V8)
                let f = b.get(start..i)?;
                let mut v = 0.0;
                for k in 0..3 {
                    v = v * 10.0 + f.get(k).map_or(0.0, |d| f64::from(d - b'0'));
                }
                ms = v;
            }
        }
        match b.get(i) {
            Some(b'Z') => {
                i += 1;
                offset = Some(0.0);
            }
            Some(c @ (b'+' | b'-')) => {
                let sign = if *c == b'-' { -1.0 } else { 1.0 };
                i += 1;
                let oh = digits(&mut i, 2)?;
                if b.get(i) != Some(&b':') {
                    return None;
                }
                i += 1;
                let om = digits(&mut i, 2)?;
                offset = Some(sign * (oh * 60.0 + om));
            }
            _ => {}
        }
    }
    if i != b.len() || !(1.0..=12.0).contains(&month) || h > 24.0 || mi > 59.0 || sec > 59.0 {
        return None;
    }
    let dim = [31.0, if leap(year) { 29.0 } else { 28.0 }, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
    let max_day = dim.get(uo_jsnum::conv::to_uint32(month - 1.0) as usize).copied().unwrap_or(31.0);
    if day < 1.0 || day > max_day || (h == 24.0 && (mi > 0.0 || sec > 0.0 || ms > 0.0)) {
        return None;
    }
    let t = days_from_civil(year, month, day) * 86_400_000.0 + ((h * 60.0 + mi) * 60.0 + sec) * 1000.0 + ms;
    let t = match (offset, has_time) {
        (Some(o), _) => t - o * 60_000.0,
        (None, false) => t,
        // a local date-time: UTC = local − the zone offset at that instant
        (None, true) => t - f64::from(tz.offset_min_at(t - f64::from(tz.offset_min_at(t)) * 60_000.0)) * 60_000.0,
    };
    let t = word::time_clip(t);
    (!t.is_nan()).then_some(t)
}

fn leap(y: f64) -> bool {
    uo_jsnum::math::rem(y, 4.0) == 0.0 && (uo_jsnum::math::rem(y, 100.0) != 0.0 || uo_jsnum::math::rem(y, 400.0) == 0.0)
}

/// Days from 1970-01-01 to the civil date (proleptic Gregorian), exact in `f64`.
fn days_from_civil(y: f64, m: f64, d: f64) -> f64 {
    let y = if m <= 2.0 { y - 1.0 } else { y };
    let era = uo_jsnum::math::floor(y / 400.0);
    let yoe = y - era * 400.0;
    let mp = if m > 2.0 { m - 3.0 } else { m + 9.0 };
    let doy = uo_jsnum::math::floor((153.0 * mp + 2.0) / 5.0) + d - 1.0;
    let doe = yoe * 365.0 + uo_jsnum::math::floor(yoe / 4.0) - uo_jsnum::math::floor(yoe / 100.0) + doy;
    era * 146_097.0 + doe - 719_468.0
}

impl Engine {
    /// `chapterOf(level, page)` (engine.ts:612): the number of the LAST heading of the level that
    /// starts on the page or before; before the first heading, the first one's.
    fn chapter_of(&mut self, cx: &Cx<'_, '_>, level: f64, page: f64) -> Option<Box<[u16]>> {
        self.found.used_chapter = true;
        let key =
            if level == uo_jsnum::math::trunc(level) && level.abs() < 9.0e15 { uo_jsnum::conv::to_int32(level).into() } else { i64::MIN };
        let list = cx.input.chapters_guess.get(&key)?;
        let mut out = list.first()?.text.clone();
        for h in list {
            if h.page > page {
                break;
            }
            out = h.text.clone();
        }
        Some(out)
    }

    /// `fieldValue(ps, block)(name, instr, at)` (engine.ts:1099).
    pub(crate) fn field_value(
        &mut self,
        cx: &mut Cx<'_, '_>,
        ps: PageIx,
        _block: Option<i64>,
        name: &[u16],
        instr: &[u16],
        _at: f64,
    ) -> LResult<Option<Box<[u16]>>> {
        let name = String::from_utf16_lossy(name);
        let lang = cx.opts.lang.clone();
        let mut cap = word::text_cap(&cx.opts.limits);
        let out = match name.as_str() {
            "PAGEREF" | "SEQ" | "REF" | "NOTEREF" | "STYLEREF" => {
                return Err(LayoutError::Unported(Frontier::F7c, "SEQ/REF/NOTEREF/STYLEREF/PAGEREF fields"));
            }
            "ENDNOTEREF_MARK" if self.cur_endnote.is_some() => {
                return Err(LayoutError::Unported(Frontier::F7c, "ENDNOTEREF_MARK"));
            }
            "PAGE" => {
                let p = self.ps(ps)?;
                let (number, gi) = (p.page.number, p.geom);
                let g = self.geom(gi)?;
                let fmt = g.pg_fmt.clone().unwrap_or_else(|| "decimal".into());
                let chap_style = g.chap.clone();
                let num = word::format_number(number, Some(&fmt), &lang, &mut cap);
                let chap = match &chap_style {
                    Some(c) => self.chapter_of(cx, c.level, f64::from(ps.0)),
                    None => None,
                };
                match (chap, chap_style) {
                    (Some(ch), Some(c)) => {
                        let mut v = ch.to_vec();
                        v.extend(c.sep.encode_utf16());
                        v.extend_from_slice(num.units());
                        Some(v.into_boxed_slice())
                    }
                    _ => Some(num.units().into()),
                }
            }
            // NUMPAGES: the physical pages, less the gallery cover page when the numbering starts at 0
            "NUMPAGES" => {
                self.found.used_num_pages = true;
                let n = word::num_pages_shown(cx.input.total_pages_guess, self.first_page_number, self.cover_page);
                Some(utf16(&uo_jsnum::format::to_js_string(n)))
            }
            // the pages of the page's section (measured in Word: a cover in its own section + 3 → "3")
            "SECTIONPAGES" => {
                self.found.used_section_pages = true;
                let idx = self.geom(self.ps(ps)?.geom)?.index;
                let n = cx.input.section_pages_guess.get(&idx).copied().unwrap_or(cx.input.total_pages_guess);
                Some(utf16(&uo_jsnum::format::to_js_string(n)))
            }
            "SECTION" => {
                let idx = self.geom(self.ps(ps)?.geom)?.index;
                Some(utf16(&uo_jsnum::format::to_js_string(f64::from(idx) + 1.0)))
            }
            // DATE/TIME: Word recomputes them on export — the export instant
            "DATE" | "TIME" => {
                let now = cx.opts.clock.local_now();
                Some(word::date_field_value(instr, &now, &lang, &mut cap).units().into())
            }
            // the document property dates (Word reads them again on export; local time)
            "SAVEDATE" | "CREATEDATE" | "PRINTDATE" => {
                let cp = cx.ldoc.view.core_props();
                let iso = match name.as_str() {
                    "SAVEDATE" => cp.and_then(|c| c.modified.as_deref()),
                    "CREATEDATE" => cp.and_then(|c| c.created.as_deref()),
                    _ => cp.and_then(|c| c.last_printed.as_deref()),
                };
                match iso.filter(|s| !s.is_empty()) {
                    None => None,
                    Some(s) => {
                        let Some(t) = parse_iso_date(s, &cx.opts.clock.tz) else {
                            return Err(LayoutError::Unported(Frontier::F7c, "Date.parse outside the ISO format"));
                        };
                        let d = cx.opts.clock.local(t);
                        Some(word::date_field_value(instr, &d, &lang, &mut cap).units().into())
                    }
                }
            }
            _ => None,
        };
        if cap.take_truncated() {
            self.ctx.warn("field text truncated");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_iso_date;
    use crate::options::TimeZoneRule;

    #[test]
    fn iso_dates_parse_as_v8() {
        let utc = TimeZoneRule::Fixed { offset_min: 0 };
        assert_eq!(parse_iso_date("2026-01-15T12:00:00Z", &utc), Some(1_768_478_400_000.0));
        assert_eq!(parse_iso_date("1970-01-01", &utc), Some(0.0));
        assert_eq!(parse_iso_date("2024-02-29T00:00:00.1239Z", &utc), Some(1_709_164_800_123.0));
        assert_eq!(parse_iso_date("2024-02-30", &utc), None);
        assert_eq!(parse_iso_date("2024/02/03", &utc), None);
        let sp = TimeZoneRule::Fixed { offset_min: -180 };
        assert_eq!(parse_iso_date("1970-01-01T00:00", &sp), Some(10_800_000.0));
        assert_eq!(parse_iso_date("1970-01-01T00:00+01:00", &sp), Some(-3_600_000.0));
    }
}
