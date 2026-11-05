//! D2b family (frontier D2b-F4): text layout options: the pure rules of editor `text-layout.ts`
//! (line numbering, hyphenation limits; Word rules measured in `write/tools/textlayout-lab`), and
//! the document-level commands `setDefaultTabStop`, `setHyphenation`, `setHyphenationOptions`.
//! The manual hyphenation (`nextManualHyphenation`) and the paragraph-flow/layout half of
//! `text-layout.ts` are the layout family's (D2b-F10) and F1's (`ops/paragraph.rs`).
//!
//! The settings these commands edit live in [`SettingsStep`] (`dt`, `hy`: the EDITED values) and in
//! `doc.formatting.settings` (what the engine reads); an undo restores both through
//! [`Session::restore_settings`], with the TS rule: a value different from the current one is
//! APPLIED (and so becomes an edited value that the save writes).

use std::sync::Arc;

use uo_jsnum::math::{max, min, round};

use super::super::command::{CommandError, Outcome};
use super::super::policy::GuardArg;
use super::super::state::SettingsStep;
use super::super::{ExecCx, Session};
use crate::js::JsNumber;
use crate::model::section::{LineNumbering, SectPr};
use crate::word::obj;
use crate::write::emit::settings::HyphenationSettings;

// ───────────────────────── line numbering (pure) ─────────────────────────

/// A menu item of Line Numbers (`LineNumberMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineNumberMode {
    None,
    Continuous,
    Page,
    Section,
}

/// The line numbering of a section as the dialog shows it (`LineNumberingInfo`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineNumberingInfo {
    pub mode: LineNumberMode,
    /// "Start at"
    pub start_at: f64,
    /// "Count by"
    pub count_by: f64,
    /// "From text" in twips; 0 = Auto
    pub distance_twips: f64,
}

/// The restart rule of the dialog (`LineNumberingOptions.restart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRestart {
    Page,
    Section,
    Continuous,
}

/// The Line Numbers dialog (`LineNumberingOptions`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineNumberingOptions {
    pub on: bool,
    pub start_at: f64,
    pub count_by: f64,
    pub distance_twips: f64,
    pub restart: LineRestart,
}

/// `LINE_NUMBER_LIMITS`: what Word accepts.
pub const LINE_NUMBER_LIMITS_START_AT: (f64, f64) = (1.0, 32767.0);
pub const LINE_NUMBER_LIMITS_COUNT_BY: (f64, f64) = (1.0, 100.0);
pub const LINE_NUMBER_LIMITS_DISTANCE: (f64, f64) = (0.0, 31680.0);

fn clamp((lo, hi): (f64, f64), v: f64) -> f64 {
    min(hi, max(lo, round(v)))
}

/// `lineNumberingOn(ln)`: no `countBy`, or 0, and Word does not number (oracle lnnum3).
#[must_use]
pub fn line_numbering_on(ln: Option<&LineNumbering>) -> bool {
    ln.is_some_and(|l| l.count_by.map_or(0.0, JsNumber::to_number) >= 1.0)
}

/// `lineNumberingInfo(sp)`.
#[must_use]
pub fn line_numbering_info(sp: &SectPr) -> LineNumberingInfo {
    let ln = sp.ln_num_type.as_ref();
    let on = line_numbering_on(ln);
    let num = |x: Option<JsNumber>| x.and_then(JsNumber::as_f64);
    LineNumberingInfo {
        mode: if !on {
            LineNumberMode::None
        } else {
            match ln.and_then(|l| l.restart.as_deref()) {
                Some("continuous") => LineNumberMode::Continuous,
                Some("newSection") => LineNumberMode::Section,
                _ => LineNumberMode::Page,
            }
        },
        start_at: num(ln.and_then(|l| l.start)).unwrap_or(0.0) + 1.0,
        count_by: if on { ln.and_then(|l| num(l.count_by)).unwrap_or(f64::NAN) } else { 1.0 },
        distance_twips: num(ln.and_then(|l| l.distance)).unwrap_or(0.0),
    }
}

/// What the session remembers of a section whose numbering was switched off (Word remembers the
/// start and the distance; "count by" goes back to 1: oracle ln-off-on).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LineNumberMemory {
    pub start: Option<f64>,
    pub distance: Option<f64>,
}

/// `withLineNumberMode(sp, mode, memory)`: the `w:lnNumType` after a menu item. None removes it; the
/// others only change the restart and keep start/count by/distance (oracle ln-cont-over); turning
/// on from scratch: count by 1 and what the session remembered.
#[must_use]
pub fn with_line_number_mode(sp: &SectPr, mode: LineNumberMode, memory: Option<LineNumberMemory>) -> Option<LineNumbering> {
    if mode == LineNumberMode::None {
        return None;
    }
    let cur = sp.ln_num_type.as_ref().filter(|l| line_numbering_on(Some(l)));
    let mut out = match cur {
        Some(c) => c.clone(),
        None => {
            let mut o = LineNumbering { count_by: Some(JsNumber::Num(1.0)), ..LineNumbering::default() };
            if let Some(m) = memory {
                if let Some(s) = m.start.filter(|s| uo_jsnum::conv::truthy(*s)) {
                    obj::set(&mut o, "start", |x| x.start = Some(JsNumber::Num(s)));
                }
                if let Some(d) = m.distance.filter(|d| uo_jsnum::conv::truthy(*d)) {
                    obj::set(&mut o, "distance", |x| x.distance = Some(JsNumber::Num(d)));
                }
            }
            o
        }
    };
    obj::delete(&mut out, "restart");
    match mode {
        LineNumberMode::Continuous => obj::set(&mut out, "restart", |x| x.restart = Some("continuous".into())),
        LineNumberMode::Section => obj::set(&mut out, "restart", |x| x.restart = Some("newSection".into())),
        _ => {}
    }
    Some(out)
}

/// `lineNumberingFromOptions(o)`: the `w:lnNumType` of the dialog (values clamped to Word's limits).
#[must_use]
pub fn line_numbering_from_options(o: &LineNumberingOptions) -> Option<LineNumbering> {
    if !o.on {
        return None;
    }
    let start = clamp(LINE_NUMBER_LIMITS_START_AT, o.start_at) - 1.0;
    let distance = clamp(LINE_NUMBER_LIMITS_DISTANCE, o.distance_twips);
    let mut out =
        LineNumbering { count_by: Some(JsNumber::Num(clamp(LINE_NUMBER_LIMITS_COUNT_BY, o.count_by))), ..LineNumbering::default() };
    if uo_jsnum::conv::truthy(start) {
        obj::set(&mut out, "start", |x| x.start = Some(JsNumber::Num(start)));
    }
    if uo_jsnum::conv::truthy(distance) {
        obj::set(&mut out, "distance", |x| x.distance = Some(JsNumber::Num(distance)));
    }
    match o.restart {
        LineRestart::Continuous => obj::set(&mut out, "restart", |x| x.restart = Some("continuous".into())),
        LineRestart::Section => obj::set(&mut out, "restart", |x| x.restart = Some("newSection".into())),
        LineRestart::Page => {}
    }
    Some(out)
}

// ───────────────────────── hyphenation (pure) ─────────────────────────

/// The Hyphenation options of the document (`HyphenationInfo`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HyphenationInfo {
    /// Automatically hyphenate document (`w:autoHyphenation`)
    pub auto: bool,
    /// Hyphenate words in CAPS (false is `w:doNotHyphenateCaps`)
    pub caps: bool,
    /// Hyphenation zone (twips; without the element, Word pt-BR's 425)
    pub zone_twips: f64,
    /// Limit consecutive hyphens to (0 = no limit)
    pub limit: f64,
}

/// `Partial<HyphenationInfo>` for `setHyphenationOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HyphenationPatch {
    pub auto: Option<bool>,
    pub caps: Option<bool>,
    pub zone_twips: Option<f64>,
    pub limit: Option<f64>,
}

/// `HYPHENATION_LIMITS`.
pub const HYPHENATION_LIMIT: (f64, f64) = (0.0, 32767.0);
pub const HYPHENATION_ZONE: (f64, f64) = (1.0, 31680.0);

/// `JSON.stringify(h)` of a `HyphenationSettings` in the key order it was built with: the order of
/// `hyphenationSettings()` from the file is `auto, caps, zone, limit`; the one of
/// `setHyphenationOptions` is `auto, caps, limit, zone` (so two equal values with a zone can still
/// differ as strings, which the TS comparison sees).
#[must_use]
pub fn hyphenation_json(h: &HyphenationSettings, edited_order: bool) -> String {
    let n = |x: f64| uo_jsnum::json::number(x);
    let zone = h.zone.map(|z| format!(",\"zone\":{}", n(z)));
    if edited_order {
        format!("{{\"auto\":{},\"caps\":{},\"limit\":{}{}}}", h.auto, h.caps, n(h.limit), zone.unwrap_or_default())
    } else {
        format!("{{\"auto\":{},\"caps\":{}{},\"limit\":{}}}", h.auto, h.caps, zone.unwrap_or_default(), n(h.limit))
    }
}

// ───────────────────────── the commands ─────────────────────────

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum TextLayoutCmd {
    /// `setDefaultTabStop(twips)`: the document's default tab stop (`w:defaultTabStop`)
    SetDefaultTabStop { twips: f64 },
    /// `setHyphenation(mode)`: None / Automatic
    SetHyphenation { auto: bool },
    /// `setHyphenationOptions(o)`
    SetHyphenationOptions { o: HyphenationPatch },
}

impl TextLayoutCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        Some(match self {
            TextLayoutCmd::SetDefaultTabStop { .. } => ("setDefaultTabStop", GuardArg::None),
            TextLayoutCmd::SetHyphenation { .. } => ("setHyphenation", GuardArg::None),
            TextLayoutCmd::SetHyphenationOptions { .. } => ("setHyphenationOptions", GuardArg::None),
        })
    }
}

type Res = Result<Outcome, CommandError>;

impl Session {
    pub(crate) fn exec_text_layout(&mut self, cmd: TextLayoutCmd, _cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            TextLayoutCmd::SetDefaultTabStop { twips } => self.set_default_tab_stop(twips),
            TextLayoutCmd::SetHyphenation { auto } => {
                self.set_hyphenation_options(HyphenationPatch { auto: Some(auto), ..HyphenationPatch::default() })
            }
            TextLayoutCmd::SetHyphenationOptions { o } => self.set_hyphenation_options(o),
        }
    }

    // ── queries ────────────────────────────────────────────────────────────────────────────

    /// `hyphenationSettings()`: the edited options, or those the settings carry.
    #[must_use]
    pub fn hyphenation_settings(&self) -> HyphenationSettings {
        if let Some(h) = self.settings.hyphenation {
            return h;
        }
        hyphenation_of_formatting(&self.doc.formatting.settings)
    }

    /// `getHyphenation()`: the options of the dialog (the zone without the element is 425).
    #[must_use]
    pub fn hyphenation(&self) -> HyphenationInfo {
        let h = self.hyphenation_settings();
        HyphenationInfo { auto: h.auto, caps: h.caps, zone_twips: h.zone.unwrap_or(425.0), limit: h.limit }
    }

    // ── default tab stop ───────────────────────────────────────────────────────────────────

    /// `setDefaultTabStop(twips)`: relayout and `w:defaultTabStop` in the settings at the save.
    fn set_default_tab_stop(&mut self, twips: f64) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let v = max(1.0, round(twips));
        if v == self.default_tab_stop() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        self.apply_default_tab(v);
        self.touch_formatting();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// `applyDefaultTab(v)`.
    pub(crate) fn apply_default_tab(&mut self, v: f64) {
        let mut st = (*self.settings).clone();
        st.default_tab = Some(v);
        self.settings = Arc::new(st);
        let f = Arc::make_mut(&mut self.doc.formatting);
        obj::set(&mut f.settings, "defaultTabStop", |s| s.default_tab_stop = JsNumber::Num(v));
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
    }

    // ── hyphenation ────────────────────────────────────────────────────────────────────────

    /// `setHyphenationOptions(o)`: one undo step; written in `settings.xml`.
    fn set_hyphenation_options(&mut self, o: HyphenationPatch) -> Res {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let cur = self.hyphenation_settings();
        let lim = |v: f64, r: (f64, f64)| clamp(r, v);
        let next = HyphenationSettings {
            auto: o.auto.unwrap_or(cur.auto),
            caps: o.caps.unwrap_or(cur.caps),
            limit: o.limit.map_or(cur.limit, |v| lim(v, HYPHENATION_LIMIT)),
            zone: match o.zone_twips {
                Some(z) => Some(lim(z, HYPHENATION_ZONE)),
                None => cur.zone,
            },
        };
        let cur_edited = self.settings.hyphenation.is_some();
        if hyphenation_json(&next, true) == hyphenation_json(&cur, cur_edited) {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        self.apply_hyphenation(next);
        self.touch_formatting();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// `applyHyphenation(h)`.
    pub(crate) fn apply_hyphenation(&mut self, h: HyphenationSettings) {
        let mut st = (*self.settings).clone();
        st.hyphenation = Some(h);
        self.settings = Arc::new(st);
        let f = Arc::make_mut(&mut self.doc.formatting);
        let s = &mut f.settings;
        obj::set(s, "autoHyphenation", |x| x.auto_hyphenation = h.auto);
        obj::set(s, "doNotHyphenateCaps", |x| x.do_not_hyphenate_caps = !h.caps);
        match h.zone {
            Some(z) => obj::set(s, "hyphenationZone", |x| x.hyphenation_zone = Some(JsNumber::Num(z))),
            None => obj::delete(s, "hyphenationZone"),
        }
        if h.limit > 0.0 {
            obj::set(s, "consecutiveHyphenLimit", |x| x.consecutive_hyphen_limit = Some(JsNumber::Num(h.limit)));
        } else {
            obj::delete(s, "consecutiveHyphenLimit");
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
    }

    /// The settings half of the restore of `popHistory` (`dt`, `hy`, `bg`/`bgd`, `eo`, `np`): each
    /// value of the step that differs from the current one is APPLIED, which makes it an edited
    /// value (the TS `applyDefaultTab`, `applyHyphenation`, `setEvenOddFlag` set their field).
    pub(crate) fn restore_settings(&mut self, st: Arc<SettingsStep>) {
        let before = Arc::clone(&self.settings);
        // `if (state.np) this.noteSettings = state.np` (F5): the step's value
        let mut cur = (*self.settings).clone();
        cur.note_settings = st.note_settings.clone();
        self.settings = Arc::new(cur);
        let l = self.f4.loaded.clone();
        let want_dt = st.default_tab.unwrap_or(l.default_tab);
        if want_dt != self.default_tab_stop() {
            self.apply_default_tab(want_dt);
        }
        let want_hy = st.hyphenation.unwrap_or(l.hyph);
        let step_edited = st.hyphenation.is_some();
        let cur_hy = self.hyphenation_settings();
        if hyphenation_json(&want_hy, step_edited) != hyphenation_json(&cur_hy, self.settings.hyphenation.is_some()) {
            self.apply_hyphenation(want_hy);
        }
        self.restore_page_background(&st);
        let want_eo = st.even_odd.unwrap_or(l.even_odd);
        if want_eo != self.even_odd_headers() {
            self.set_even_odd_flag(want_eo);
        }
        if *before != *self.settings {
            self.touch_formatting();
        }
    }
}

/// `hyphenationSettings()` from the document settings: `{ auto, caps, zone?, limit }`.
#[must_use]
pub fn hyphenation_of_formatting(s: &crate::model::formatting::Settings) -> HyphenationSettings {
    HyphenationSettings {
        auto: s.auto_hyphenation,
        caps: !s.do_not_hyphenate_caps,
        zone: s.hyphenation_zone.and_then(JsNumber::as_f64),
        limit: s.consecutive_hyphen_limit.and_then(JsNumber::as_f64).unwrap_or(0.0),
    }
}
