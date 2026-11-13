//! D2b family: paragraph formatting: `setAlignment`, `indent`, `setLineSpacing`, `setSpaceAfter/Before`, `setShading`, `setSpecialIndent`, `setIndentRight`, `setParagraphIndents`, `setTabStops`, `setParagraphFlow`, `setParagraphLayout`, `setParagraphBorders`, `insertHorizontalLine`, `toggleBorder`, `clearParagraphFormatting`.
//!
//! Owned by frontier D2b-F1 (record "D2a — session (architecture)", "D2b frontier briefs" and
//! `docs/modules/uo-docx-d2b/F1.md`): ported 1:1 from `universal-write.ts` (4139–4145, 4539,
//! 4658–4665, 5758–5768, 17194–17250, 17530–17620, 20015–20110), the ruler rules of
//! `navigation.ts` (`tabInfo`, `tabsDirect`, `dedupeTabs`, `indentsDirect`) and the paragraph
//! rules of `text-layout.ts` (`flowValue`, `flowDirect`, `spacingDirect`, `common`,
//! `PARAGRAPH_LAYOUT_LIMITS`) that the commands need.
//!
//! Every command goes through `formatPara` (a step, `applyPara` on the main range and the extra
//! ranges; tracked as `w:pPrChange` when tracking is on) or builds its own per-paragraph patches
//! over the paragraph's inherited and effective `pPr` (the ruler's "write only what differs from
//! the inherited" rule).

use std::sync::Arc;

use uo_jsnum::math;

use super::super::command::{CommandError, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::revisions;
use super::super::state::Selection;
use super::super::text::{
    CarriedProps, DocPosition, DocRange, OpError, clear_para_formatting, insert_index, json_or_null, normalize_range, para_length,
    without_revisions,
};
use super::super::{ExecCx, Session};
use super::context::{EditContext, story_of_mut};
use crate::js::{JsNumber, JsObject, JsValue, Str};
use crate::model::para::{BorderEdge, ParaProps, ParaTabStop, Paragraph, ParagraphBorders};
use crate::model::props::{BorderSpec, IndSpec, PPr, TabSpec};
use crate::model::run::{DrawingShape, Run, RunExt};
use crate::model::sources::StoryGroup;
use crate::text::Utf16;
use crate::word::direct::{effective_ppr, inherited_ppr};
use crate::word::obj;
use crate::word::resolve::StyleResolver;

/// `patch.key = value` where `value` may be `undefined`: a present value is a key, an absent one
/// a key holding `undefined`.
macro_rules! pset {
    ($p:expr, $key:expr, $field:ident, $val:expr) => {
        match $val {
            ::core::option::Option::Some(v) => obj::set(&mut $p, $key, |p| p.$field = ::core::option::Option::Some(v)),
            ::core::option::Option::None => obj::set_undefined(&mut $p, $key, |p| p.$field = ::core::option::Option::None),
        }
    };
}

// ───────────────────────────── arguments ─────────────────────────────

/// `TabStopInfo` (navigation.ts): an effective tab stop as the ruler shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStopInfo {
    /// twips from the left edge of the paragraph frame
    pub pos: f64,
    /// `left | center | right | decimal | bar`
    pub align: String,
    pub leader: Option<String>,
}

/// `IndentInfo`: effective indents (twips; `first_line < 0` = hanging).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndentInfo {
    pub left: f64,
    pub right: f64,
    pub first_line: f64,
}

/// `ParagraphFlow` (text-layout.ts): the flags of FLOW_FLAGS in that order, then `textboxTightWrap`; `None` = the TS
/// `null` (the selected paragraphs differ). Wave 7 (A1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParagraphFlow {
    pub flags: [Option<bool>; 6],
    pub textbox_tight_wrap: Option<String>,
}

/// `ParagraphLayoutInfo` (text-layout.ts): `None` = the TS `null`. Wave 7 (A1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParagraphLayoutInfo {
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub before: Option<SpacingArg>,
    pub after: Option<SpacingArg>,
}

/// `Partial<IndentInfo>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IndentPatch {
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub first_line: Option<f64>,
}

/// The `only` argument of `setTabStops`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabOnly {
    pub add: Option<Vec<TabStopInfo>>,
    pub remove: Option<Vec<f64>>,
}

/// `setParagraphFlow(f)`: the fields present change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowArgs {
    pub widow_control: Option<bool>,
    pub keep_next: Option<bool>,
    pub keep_lines: Option<bool>,
    pub page_break_before: Option<bool>,
    pub suppress_line_numbers: Option<bool>,
    pub suppress_auto_hyphens: Option<bool>,
    pub textbox_tight_wrap: Option<String>,
}

/// A spacing box: a value in twips or Automatic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpacingArg {
    Auto,
    Twips(f64),
}

/// `setParagraphLayout(v)`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutArgs {
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub before: Option<SpacingArg>,
    pub after: Option<SpacingArg>,
}

/// `ParaBorderSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaBorderSide {
    Top,
    Left,
    Bottom,
    Right,
    Between,
    Bar,
}

impl ParaBorderSide {
    fn key(self) -> &'static str {
        match self {
            ParaBorderSide::Top => "top",
            ParaBorderSide::Left => "left",
            ParaBorderSide::Bottom => "bottom",
            ParaBorderSide::Right => "right",
            ParaBorderSide::Between => "between",
            ParaBorderSide::Bar => "bar",
        }
    }

    fn edge(self, b: &ParagraphBorders) -> Option<&BorderEdge> {
        match self {
            ParaBorderSide::Top => b.top.as_ref(),
            ParaBorderSide::Left => b.left.as_ref(),
            ParaBorderSide::Bottom => b.bottom.as_ref(),
            ParaBorderSide::Right => b.right.as_ref(),
            ParaBorderSide::Between => b.between.as_ref(),
            ParaBorderSide::Bar => b.bar.as_ref(),
        }
    }

    fn set(self, b: &mut ParagraphBorders, e: BorderEdge) {
        let k = self.key();
        match self {
            ParaBorderSide::Top => obj::set(b, k, |b| b.top = Some(e)),
            ParaBorderSide::Left => obj::set(b, k, |b| b.left = Some(e)),
            ParaBorderSide::Bottom => obj::set(b, k, |b| b.bottom = Some(e)),
            ParaBorderSide::Right => obj::set(b, k, |b| b.right = Some(e)),
            ParaBorderSide::Between => obj::set(b, k, |b| b.between = Some(e)),
            ParaBorderSide::Bar => obj::set(b, k, |b| b.bar = Some(e)),
        }
    }

    const ALL: [ParaBorderSide; 6] = [
        ParaBorderSide::Top,
        ParaBorderSide::Left,
        ParaBorderSide::Bottom,
        ParaBorderSide::Right,
        ParaBorderSide::Between,
        ParaBorderSide::Bar,
    ];
}

/// `paragraphBorderPreset(kind)` of the Borders button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderPreset {
    Bottom,
    Top,
    Left,
    Right,
    None,
    All,
    Outside,
    Inside,
    InsideH,
    InsideV,
}

/// `setSpecialIndent(kind)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialIndent {
    None,
    FirstLine,
    Hanging,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum ParagraphCmd {
    /// `setAlignment(a)`: `left | center | right | both`
    SetAlignment { alignment: String },
    /// `indent(delta)`: a list level (F2's `changeListLevel`) or 720 twips
    Indent { delta: i8 },
    /// `setLineSpacing(line)`
    SetLineSpacing { line: f64 },
    /// `setSpaceAfter(twips)`
    SetSpaceAfter { twips: Option<f64> },
    /// `setSpaceBefore(twips)`
    SetSpaceBefore { twips: Option<f64> },
    /// `setShading(hex)`
    SetShading { hex: Option<String> },
    /// `setSpecialIndent(kind)`
    SetSpecialIndent { kind: SpecialIndent },
    /// `setIndentRight(twips)`
    SetIndentRight { twips: Option<f64> },
    /// `setParagraphIndents(ind)`
    SetParagraphIndents { ind: IndentPatch },
    /// `setTabStops(tabs, only)`
    SetTabStops { tabs: Vec<TabStopInfo>, only: Option<TabOnly> },
    /// `setParagraphFlow(f)`
    SetParagraphFlow { f: FlowArgs },
    /// `setParagraphLayout(v)`
    SetParagraphLayout { v: LayoutArgs },
    /// `setParagraphBorders(sides)`: `None` = `null` (no borders); each side `None` = `null` (removes it),
    /// absent from the list = as it is
    SetParagraphBorders { sides: Option<Vec<(ParaBorderSide, Option<BorderSpec>)>> },
    /// `paragraphBorderPreset(kind)`
    ParagraphBorderPreset { kind: BorderPreset },
    /// `toggleBorder()`
    ToggleBorder,
    /// `insertHorizontalLine()`
    InsertHorizontalLine,
    /// `clearParagraphFormatting()` (the inspector)
    ClearParagraphFormatting,
}

impl ParagraphCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match self {
            ParagraphCmd::SetAlignment { .. } => Some(("setAlignment", GuardArg::None)),
            ParagraphCmd::Indent { .. } => Some(("indent", GuardArg::None)),
            ParagraphCmd::SetLineSpacing { .. } => Some(("setLineSpacing", GuardArg::None)),
            ParagraphCmd::SetSpaceAfter { .. } => Some(("setSpaceAfter", GuardArg::None)),
            ParagraphCmd::SetSpaceBefore { .. } => Some(("setSpaceBefore", GuardArg::None)),
            ParagraphCmd::SetShading { .. } => Some(("setShading", GuardArg::None)),
            ParagraphCmd::SetSpecialIndent { .. } => Some(("setSpecialIndent", GuardArg::None)),
            ParagraphCmd::SetIndentRight { .. } => Some(("setIndentRight", GuardArg::None)),
            ParagraphCmd::SetParagraphIndents { .. } => Some(("setParagraphIndents", GuardArg::None)),
            ParagraphCmd::SetTabStops { .. } => Some(("setTabStops", GuardArg::None)),
            ParagraphCmd::SetParagraphFlow { .. } => Some(("setParagraphFlow", GuardArg::None)),
            ParagraphCmd::SetParagraphLayout { v } => Some((
                "setParagraphLayout",
                GuardArg::Layout { indents: v.left.is_some() || v.right.is_some(), spacing: v.before.is_some() || v.after.is_some() },
            )),
            ParagraphCmd::SetParagraphBorders { .. } => Some(("setParagraphBorders", GuardArg::None)),
            // `paragraphBorderPreset` itself is not guarded; its `none` calls the guarded
            // `setParagraphBorders(null)`
            ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::None } => Some(("setParagraphBorders", GuardArg::None)),
            ParagraphCmd::ParagraphBorderPreset { .. } | ParagraphCmd::ClearParagraphFormatting => None,
            ParagraphCmd::ToggleBorder => Some(("toggleBorder", GuardArg::None)),
            ParagraphCmd::InsertHorizontalLine => Some(("insertHorizontalLine", GuardArg::None)),
        }
    }
}

// ───────────────────────────── pure model files ─────────────────────────────

/// `PARAGRAPH_LAYOUT_LIMITS.indent`.
pub const LAYOUT_INDENT_LIMITS: (f64, f64) = (-31680.0, 31680.0);
/// `PARAGRAPH_LAYOUT_LIMITS.spacing`.
pub const LAYOUT_SPACING_LIMITS: (f64, f64) = (0.0, 31680.0);

/// `clamp(v, [lo, hi])` of text-layout.ts: `Math.min(hi, Math.max(lo, Math.round(v)))`.
#[must_use]
pub fn clamp_round(v: f64, (lo, hi): (f64, f64)) -> f64 {
    math::min(hi, math::max(lo, math::round(v)))
}

/// `a === b` for the two optional numbers of the model and a patch value.
fn strict_eq(a: Option<f64>, b: Option<JsNumber>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(JsNumber::Num(y))) => x == y,
        _ => false,
    }
}

fn num(v: Option<JsNumber>) -> f64 {
    v.map_or(0.0, JsNumber::to_number)
}

/// `tabInfo(t)`: a typed tab → the ruler's (`num` becomes left; `clear` never gets here).
#[must_use]
pub fn tab_info(t: &TabSpec) -> TabStopInfo {
    let align = match &*t.val {
        v @ ("center" | "right" | "decimal" | "bar") => v.to_owned(),
        _ => "left".to_owned(),
    };
    let leader = t.leader.as_deref().filter(|l| !l.is_empty() && *l != "none").map(str::to_owned);
    TabStopInfo { pos: math::round(t.pos.to_number()), align, leader }
}

fn same_tab(a: &TabSpec, b: &TabStopInfo) -> bool {
    let ai = tab_info(a);
    math::round(a.pos.to_number()) == b.pos && ai.align == b.align && ai.leader.unwrap_or_default() == b.leader.clone().unwrap_or_default()
}

fn key_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// `dedupeTabs(tabs)`: one tab per position (the last wins), in order.
#[must_use]
pub fn dedupe_tabs(tabs: &[TabStopInfo]) -> Vec<TabStopInfo> {
    let mut m: Vec<(f64, TabStopInfo)> = Vec::new();
    for t in tabs {
        let key = math::round(t.pos);
        let info = TabStopInfo { pos: key, ..t.clone() };
        match m.iter_mut().find(|(k, _)| key_eq(*k, key)) {
            Some(slot) => slot.1 = info,
            None => m.push((key, info)),
        }
    }
    let mut v: Vec<TabStopInfo> = m.into_iter().map(|(_, i)| i).collect();
    obj::js_sort(&mut v, |a, b| a.pos - b.pos);
    v
}

/// `tabsDirect(inherited, wanted)`: the minimal direct tabs that take the inherited ones to the
/// wanted (Word's rule: equal to the inherited writes nothing, a vanished inherited one is a
/// `clear`, in position order). `None`: nothing to write.
#[must_use]
pub fn tabs_direct(inherited: Option<&[TabSpec]>, wanted: &[TabStopInfo]) -> Option<Vec<ParaTabStop>> {
    let inh: Vec<&TabSpec> = inherited.unwrap_or(&[]).iter().filter(|t| &*t.val != "clear").collect();
    let mut out: Vec<ParaTabStop> = Vec::new();
    let want = dedupe_tabs(wanted);
    for w in &want {
        let h = inh.iter().find(|t| math::round(t.pos.to_number()) == w.pos);
        if h.is_some_and(|h| same_tab(h, w)) {
            continue;
        }
        let mut ts = ParaTabStop { pos_twips: JsNumber::Num(w.pos), align: w.align.as_str().into(), ..ParaTabStop::default() };
        if let Some(l) = w.leader.as_deref().filter(|l| !l.is_empty()) {
            obj::set(&mut ts, "leader", |t| t.leader = Some(l.into()));
        }
        out.push(ts);
    }
    for h in &inh {
        let pos = math::round(h.pos.to_number());
        if !want.iter().any(|w| w.pos == pos) {
            out.push(ParaTabStop { pos_twips: JsNumber::Num(pos), align: "clear".into(), ..ParaTabStop::default() });
        }
    }
    obj::js_sort(&mut out, |a, b| a.pos_twips.to_number() - b.pos_twips.to_number());
    if out.is_empty() { None } else { Some(out) }
}

/// `indentsDirect(inherited, want)`: the direct indents; an attribute equal to the inherited
/// is `None` (`undefined`).
#[must_use]
pub fn indents_direct(inherited: Option<&IndSpec>, want: IndentInfo) -> [Option<f64>; 3] {
    let inh = inherited.cloned().unwrap_or_default();
    let (l, r, f) = (math::round(want.left), math::round(want.right), math::round(want.first_line));
    let inh_f = match inh.hanging {
        Some(h) => -h.to_number(),
        None => num(inh.first_line),
    };
    [
        if l == math::round(num(inh.left)) { None } else { Some(l) },
        if r == math::round(num(inh.right)) { None } else { Some(r) },
        if f == math::round(inh_f) { None } else { Some(f) },
    ]
}

/// The flow flags (`FLOW_FLAGS`).
pub const FLOW_FLAGS: [&str; 6] =
    ["widowControl", "keepNext", "keepLines", "pageBreakBefore", "suppressLineNumbers", "suppressAutoHyphens"];

/// `flowValue(ppr, k)`: the effective value of a flag in a resolved `pPr` (widow control is ON
/// unless something turns it off).
#[must_use]
pub fn flow_value(ppr: &PPr, k: &str) -> bool {
    match k {
        "widowControl" => ppr.widow_control != Some(false),
        "keepNext" => ppr.keep_next == Some(true),
        "keepLines" => ppr.keep_lines == Some(true),
        "pageBreakBefore" => ppr.page_break_before == Some(true),
        "suppressLineNumbers" => ppr.suppress_line_numbers == Some(true),
        "suppressAutoHyphens" => ppr.suppress_auto_hyphens == Some(true),
        _ => false,
    }
}

/// `flowDirect(inherited, want)`: equal to the inherited writes nothing.
#[must_use]
pub fn flow_direct(inherited: bool, want: bool) -> Option<bool> {
    if want == inherited { None } else { Some(want) }
}

/// `common(vals)` for a list of JSON-comparable values: the common one or `None`.
#[must_use]
pub fn common<T: Clone>(vals: &[T], key: impl Fn(&T) -> String) -> Option<T> {
    let first = vals.first()?;
    let k = key(first);
    if vals.iter().all(|v| key(v) == k) { Some(first.clone()) } else { None }
}

/// The paragraph patch of `spacingDirect(inherited, want, side)`: `{ [auto]: …, [twips]: … }`.
pub fn spacing_direct(patch: &mut ParaProps, inherited_twips: f64, inherited_auto: bool, want: SpacingArg, before: bool) {
    match want {
        SpacingArg::Auto => {
            let v = if inherited_auto { None } else { Some(true) };
            set_auto(patch, before, v);
        }
        SpacingArg::Twips(w) => {
            let v = clamp_round(w, LAYOUT_SPACING_LIMITS);
            set_auto(patch, before, if inherited_auto { Some(false) } else { None });
            let t = if !inherited_auto && v == inherited_twips { None } else { Some(JsNumber::Num(v)) };
            if before {
                pset!(*patch, "spaceBeforeTwips", space_before_twips, t);
            } else {
                pset!(*patch, "spaceAfterTwips", space_after_twips, t);
            }
        }
    }
}

fn set_auto(patch: &mut ParaProps, before: bool, v: Option<bool>) {
    if before {
        pset!(*patch, "spaceBeforeAuto", space_before_auto, v);
    } else {
        pset!(*patch, "spaceAfterAuto", space_after_auto, v);
    }
}

/// `CLEARED_PARA` (universal-write.ts 894): every direct paragraph format `undefined`.
#[must_use]
pub fn cleared_para() -> ParaProps {
    let mut p = ParaProps::default();
    for k in [
        "numbering",
        "alignment",
        "spaceBeforeTwips",
        "spaceAfterTwips",
        "lineTwentieths",
        "indentLeftTwips",
        "indentRightTwips",
        "indentFirstLineTwips",
        "borders",
        "shadingHex",
    ] {
        obj::set_undefined(&mut p, k, |_| {});
    }
    p
}

/// `/^<w:sdt[\s>]/.test(open)`.
#[must_use]
pub fn sdt_open_matches(open: &str) -> bool {
    open.strip_prefix("<w:sdt").and_then(|r| r.chars().next()).is_some_and(|c| c == '>' || uo_jsnum::text::is_js_space(c))
}

/// `/<w:(?:text|date|comboBox|dropDownList)/.test(head)`: `` after the name means the next
/// character is not a word character (`[A-Za-z0-9_]`; V8 without the `u`/`i` flags).
#[must_use]
pub fn sdt_kind_matches(head: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(i) = head.get(from..).and_then(|h| h.find("<w:")) {
        let at = from + i + 3;
        let tail = head.get(at..).unwrap_or("");
        for name in ["text", "date", "comboBox", "dropDownList"] {
            if let Some(after) = tail.strip_prefix(name)
                && !after.chars().next().is_some_and(is_word)
            {
                return true;
            }
        }
        from = at;
    }
    false
}

/// `plainTextSdt(open)` (source.ts 323): the opening of a `w:sdt` Word treats as plain text
/// (text, date, combo box, drop-down list), looked for before `</w:sdtPr>`.
#[must_use]
pub fn plain_text_sdt(open: Option<&str>) -> bool {
    let Some(open) = open.filter(|s| !s.is_empty()) else { return false };
    if !sdt_open_matches(open) {
        return false;
    }
    let head = match open.find("</w:sdtPr>") {
        Some(end) => open.get(..end).unwrap_or(open),
        None => open,
    };
    sdt_kind_matches(head)
}

// ───────────────────────────── the session ─────────────────────────────

type Res = Result<Outcome, CommandError>;

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn at(para: usize) -> DocRange {
    let p = DocPosition::new(u32_of(para), 0);
    DocRange::new(p, p)
}

impl Session {
    fn pdone(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    /// `applyParaOne(range, patch)`: tracked as `w:pPrChange` when tracking is on.
    fn apply_para_one(&mut self, range: DocRange, patch: &ParaProps) -> Result<(), CommandError> {
        let nr = normalize_range(range);
        if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            revisions::track_apply_para_props(paras, range, patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            super::super::text::apply_para_props(paras, range, patch)?;
        }
        for pi in nr.start.para..=nr.end.para {
            self.touch_para(pi as usize);
        }
        Ok(())
    }

    /// `applyPara(range, patch)`: the same on every span of a multi-selection.
    pub(crate) fn apply_para(&mut self, range: DocRange, patch: &ParaProps) -> Result<(), CommandError> {
        let a = normalize_range(range);
        let b = normalize_range(self.range());
        let more = if a.start == b.start && a.end == b.end { self.extra_ranges() } else { Vec::new() };
        self.apply_para_one(range, patch)?;
        for r in more {
            self.apply_para_one(r, patch)?;
        }
        Ok(())
    }

    /// `formatPara(patch)`.
    fn format_para(&mut self, patch: &ParaProps) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        self.begin_step(false, &[]);
        let range = self.range();
        if let Err(e) = self.apply_para(range, patch) {
            self.discard_step();
            return Err(e);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// The patches of paragraphs `[start, end]` and the step that applies them: `snapshot()`
    /// first when `step_first`, else only when there is something to apply.
    fn apply_para_patches(&mut self, patches: Vec<(usize, ParaProps)>, step_first: bool) -> Res {
        if patches.is_empty() && !step_first {
            return self.pdone();
        }
        self.begin_step(false, &[]);
        for (pi, patch) in &patches {
            if let Err(e) = self.apply_para(at(*pi), patch) {
                self.discard_step();
                return Err(e);
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    fn selected_para_range(&self) -> (usize, usize) {
        let DocRange { start, end } = normalize_range(self.range());
        (start.para as usize, end.para as usize)
    }

    /// `effectivePPrOf(p)`.
    fn effective_of(&self, p: &Paragraph, r: &mut StyleResolver) -> PPr {
        effective_ppr(&self.doc.formatting, r, p.ppr.as_deref(), &p.props)
    }

    /// `inheritedPPrOf(p)`.
    fn inherited_of(&self, p: &Paragraph, r: &mut StyleResolver) -> PPr {
        inherited_ppr(&self.doc.formatting, r, p.ppr.as_deref(), &p.props)
    }

    // ── queries (the state getters of the TS element) ──────────────────────────────────────

    /// `paragraphBorderSides()`: the border sides EVERY selected paragraph has (the Borders button state).
    #[must_use]
    pub fn paragraph_border_sides(&self) -> Vec<ParaBorderSide> {
        let (s, e) = self.selected_para_range();
        let Some(paras) = self.paras() else { return Vec::new() };
        ParaBorderSide::ALL
            .into_iter()
            .filter(|sd| (s..=e).filter_map(|i| paras.get(i)).all(|p| p.props.borders.as_deref().is_some_and(|b| sd.edge(b).is_some())))
            .collect()
    }

    /// `getParagraphIndents()`: the effective indents of the paragraph at the caret (twips).
    #[must_use]
    pub fn paragraph_indents(&self) -> IndentInfo {
        let Some(p) = self.paras().and_then(|ps| ps.get(self.selection.focus.para as usize)) else {
            return IndentInfo { left: 0.0, right: 0.0, first_line: 0.0 };
        };
        let mut resolver = StyleResolver::new();
        let ind = self.effective_of(p, &mut resolver).ind.as_deref().cloned().unwrap_or_default();
        IndentInfo {
            left: num(ind.left),
            right: num(ind.right),
            first_line: match ind.hanging {
                Some(h) => -h.to_number(),
                None => num(ind.first_line),
            },
        }
    }

    /// `getTabStops().tabs`: the effective tabs of the paragraph at the caret (the style's and the
    /// list's included, the cleared ones out), one per position.
    #[must_use]
    pub fn paragraph_tab_stops(&self) -> Vec<TabStopInfo> {
        let Some(p) = self.paras().and_then(|ps| ps.get(self.selection.focus.para as usize)) else { return Vec::new() };
        let mut resolver = StyleResolver::new();
        let tabs: Vec<TabStopInfo> = self
            .effective_of(p, &mut resolver)
            .tabs
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|t| &*t.val != "clear")
            .map(tab_info)
            .collect();
        dedupe_tabs(&tabs)
    }

    // ── wave 7 (A1): the remaining getters of the Paragraph dialogs ─────────────────────────

    /// `getParagraphFlow()` (universal-write.ts 17555): the EFFECTIVE flags of the selected paragraphs, each the
    /// common value or `None` (the TS `null`) when they differ; `None` overall without a story.
    #[must_use]
    pub fn paragraph_flow(&self) -> Option<ParagraphFlow> {
        let paras = self.paras()?;
        let (s, e) = self.selected_para_range();
        let mut resolver = StyleResolver::new();
        let eff: Vec<PPr> = (s..=e).filter_map(|i| paras.get(i)).map(|p| self.effective_of(p, &mut resolver)).collect();
        let mut flags = [None; 6];
        for (slot, k) in flags.iter_mut().zip(FLOW_FLAGS) {
            let vals: Vec<bool> = eff.iter().map(|x| flow_value(x, k)).collect();
            *slot = common(&vals, |v| v.to_string());
        }
        let tight: Vec<String> = eff.iter().map(|x| x.textbox_tight_wrap.as_deref().unwrap_or("none").to_owned()).collect();
        Some(ParagraphFlow { flags, textbox_tight_wrap: common(&tight, Clone::clone) })
    }

    /// `getParagraphLayout()` (17597): the effective left/right indents and spacing before/after of the selected
    /// paragraphs (`Auto` for autospacing), each common or `None`.
    #[must_use]
    pub fn paragraph_layout(&self) -> Option<ParagraphLayoutInfo> {
        let paras = self.paras()?;
        let (s, e) = self.selected_para_range();
        let mut resolver = StyleResolver::new();
        let eff: Vec<PPr> = (s..=e).filter_map(|i| paras.get(i)).map(|p| self.effective_of(p, &mut resolver)).collect();
        let key = |v: &f64| uo_jsnum::format::to_js_string(*v);
        let ind = |f: fn(&IndSpec) -> Option<JsNumber>| -> Option<f64> {
            let vals: Vec<f64> = eff.iter().map(|x| x.ind.as_deref().and_then(f).map_or(0.0, JsNumber::to_number)).collect();
            common(&vals, key)
        };
        let sp = |before: bool| -> Option<SpacingArg> {
            let vals: Vec<SpacingArg> = eff
                .iter()
                .map(|x| {
                    let s = x.spacing.as_deref();
                    let auto = s.and_then(|s| if before { s.before_autospacing } else { s.after_autospacing }).unwrap_or(false);
                    if auto {
                        SpacingArg::Auto
                    } else {
                        SpacingArg::Twips(s.and_then(|s| if before { s.before } else { s.after }).map_or(0.0, JsNumber::to_number))
                    }
                })
                .collect();
            common(&vals, |v| match v {
                SpacingArg::Auto => "\"auto\"".to_owned(),
                SpacingArg::Twips(t) => uo_jsnum::format::to_js_string(*t),
            })
        };
        Some(ParagraphLayoutInfo { left: ind(|i| i.left), right: ind(|i| i.right), before: sp(true), after: sp(false) })
    }

    /// `getParagraphBorders()` (20135): the borders of the paragraph at the caret as dialog specs, in the TS side
    /// order (`color` 'auto' when absent).
    #[must_use]
    pub fn paragraph_borders(&self) -> Vec<(ParaBorderSide, BorderEdge)> {
        let Some(b) = self.paras().and_then(|ps| ps.get(self.selection.focus.para as usize)).and_then(|p| p.props.borders.as_deref())
        else {
            return Vec::new();
        };
        ParaBorderSide::ALL.into_iter().filter_map(|sd| sd.edge(b).map(|e| (sd, e.clone()))).collect()
    }

    /// The document's `w:defaultTabStop` as `getDefaultTabStop()` answers it (17287).
    #[must_use]
    pub fn default_tab_stop_now(&self) -> f64 {
        self.default_tab_stop()
    }

    // ── the commands ───────────────────────────────────────────────────────────────────────

    /// `indent(delta)`.
    fn indent(&mut self, delta: i8) -> Res {
        if self.change_list_level(i32::from(delta))? {
            return self.pdone();
        }
        let cur = self.paras().and_then(|ps| ps.get(self.selection.focus.para as usize)).and_then(|p| p.props.indent_left_twips).map_or(
            0.0,
            |v| match v {
                JsNumber::Num(x) => x,
                JsNumber::Null => 0.0,
            },
        );
        let v = math::max(0.0, cur + f64::from(delta) * 720.0);
        let mut patch = ParaProps::default();
        // `Math.max(0, …) || undefined`
        pset!(patch, "indentLeftTwips", indent_left_twips, if uo_jsnum::conv::truthy(v) { Some(JsNumber::Num(v)) } else { None });
        self.format_para(&patch)
    }

    /// `setParagraphIndents(ind)`.
    fn set_paragraph_indents(&mut self, ind: IndentPatch) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        let (s, e) = self.selected_para_range();
        let mut resolver = StyleResolver::new();
        let mut patches: Vec<(usize, ParaProps)> = Vec::new();
        if let Some(paras) = self.paras() {
            for pi in s..=e {
                let Some(p) = paras.get(pi) else { continue };
                let eff = self.effective_of(p, &mut resolver).ind.as_deref().cloned().unwrap_or_default();
                let eff_first = match eff.hanging {
                    Some(h) => -h.to_number(),
                    None => num(eff.first_line),
                };
                let want = IndentInfo {
                    left: ind.left.unwrap_or_else(|| num(eff.left)),
                    right: ind.right.unwrap_or_else(|| num(eff.right)),
                    first_line: ind.first_line.unwrap_or(eff_first),
                };
                let inh = self.inherited_of(p, &mut resolver);
                let d = indents_direct(inh.ind.as_deref(), want);
                if strict_eq(d[0], p.props.indent_left_twips)
                    && strict_eq(d[1], p.props.indent_right_twips)
                    && strict_eq(d[2], p.props.indent_first_line_twips)
                {
                    continue;
                }
                patches.push((pi, indent_patch(d)));
            }
        }
        self.apply_para_patches(patches, true)
    }

    /// `setTabStops(tabs, only)`.
    fn set_tab_stops(&mut self, tabs: &[TabStopInfo], only: Option<&TabOnly>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        let (s, e) = self.selected_para_range();
        let mut resolver = StyleResolver::new();
        let mut patches: Vec<(usize, ParaProps)> = Vec::new();
        if let Some(paras) = self.paras() {
            for pi in s..=e {
                let Some(p) = paras.get(pi) else { continue };
                let mut want: Vec<TabStopInfo> = tabs.to_vec();
                if let Some(only) = only {
                    // several paragraphs: each keeps its own and only gains/loses the touched ones
                    let cur: Vec<TabStopInfo> = self
                        .effective_of(p, &mut resolver)
                        .tabs
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .filter(|t| &*t.val != "clear")
                        .map(tab_info)
                        .collect();
                    let rm: Vec<f64> = only.remove.as_deref().unwrap_or(&[]).iter().map(|x| math::round(*x)).collect();
                    want = cur.into_iter().filter(|t| !rm.iter().any(|r| key_eq(*r, t.pos))).collect();
                    want.extend(only.add.iter().flatten().cloned());
                }
                let inh = self.inherited_of(p, &mut resolver);
                let d = tabs_direct(inh.tabs.as_deref(), &want);
                let a = json_or_null(d.as_ref().map(|v| v as &dyn JsValue));
                let b = json_or_null(p.props.tab_stops.as_ref().map(|v| v as &dyn JsValue));
                if a == b {
                    continue;
                }
                let mut patch = ParaProps::default();
                pset!(patch, "tabStops", tab_stops, d);
                patches.push((pi, patch));
            }
        }
        self.apply_para_patches(patches, true)
    }

    /// `setParagraphFlow(f)`.
    fn set_paragraph_flow(&mut self, f: &FlowArgs) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        let (s, e) = self.selected_para_range();
        let mut resolver = StyleResolver::new();
        let wants: [Option<bool>; 6] =
            [f.widow_control, f.keep_next, f.keep_lines, f.page_break_before, f.suppress_line_numbers, f.suppress_auto_hyphens];
        let mut patches: Vec<(usize, ParaProps)> = Vec::new();
        if let Some(paras) = self.paras() {
            for pi in s..=e {
                let Some(p) = paras.get(pi) else { continue };
                let inh = self.inherited_of(p, &mut resolver);
                let mut patch = ParaProps::default();
                for (k, want) in FLOW_FLAGS.iter().zip(wants) {
                    let Some(want) = want else { continue };
                    let d = flow_direct(flow_value(&inh, k), want);
                    let cur = match *k {
                        "widowControl" => p.props.widow_control,
                        "keepNext" => p.props.keep_next,
                        "keepLines" => p.props.keep_lines,
                        "pageBreakBefore" => p.props.page_break_before,
                        "suppressLineNumbers" => p.props.suppress_line_numbers,
                        _ => p.props.suppress_auto_hyphens,
                    };
                    if d != cur {
                        match *k {
                            "widowControl" => pset!(patch, *k, widow_control, d),
                            "keepNext" => pset!(patch, *k, keep_next, d),
                            "keepLines" => pset!(patch, *k, keep_lines, d),
                            "pageBreakBefore" => pset!(patch, *k, page_break_before, d),
                            "suppressLineNumbers" => pset!(patch, *k, suppress_line_numbers, d),
                            _ => pset!(patch, *k, suppress_auto_hyphens, d),
                        }
                    }
                }
                if let Some(t) = &f.textbox_tight_wrap {
                    let inherited = inh.textbox_tight_wrap.as_deref().unwrap_or("none");
                    let d: Option<Str> = if t == inherited { None } else { Some(t.as_str().into()) };
                    if d.as_deref() != p.props.textbox_tight_wrap.as_deref() {
                        pset!(patch, "textboxTightWrap", textbox_tight_wrap, d);
                    }
                }
                if !obj::key_indices(&patch).is_empty() {
                    patches.push((pi, patch));
                }
            }
        }
        self.apply_para_patches(patches, false)
    }

    /// `setParagraphLayout(v)`.
    fn set_paragraph_layout(&mut self, v: LayoutArgs) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        let (s, e) = self.selected_para_range();
        let clamp_ind = |x: Option<f64>| x.map(|x| clamp_round(x, LAYOUT_INDENT_LIMITS));
        let (left, right) = (clamp_ind(v.left), clamp_ind(v.right));
        let mut resolver = StyleResolver::new();
        let mut patches: Vec<(usize, ParaProps)> = Vec::new();
        if let Some(paras) = self.paras() {
            for pi in s..=e {
                let Some(p) = paras.get(pi) else { continue };
                let inh = self.inherited_of(p, &mut resolver);
                let mut patch = ParaProps::default();
                if left.is_some() || right.is_some() {
                    let eff = self.effective_of(p, &mut resolver).ind.as_deref().cloned().unwrap_or_default();
                    let eff_first = match eff.hanging {
                        Some(h) => -h.to_number(),
                        None => num(eff.first_line),
                    };
                    let d = indents_direct(
                        inh.ind.as_deref(),
                        IndentInfo {
                            left: left.unwrap_or_else(|| num(eff.left)),
                            right: right.unwrap_or_else(|| num(eff.right)),
                            first_line: eff_first,
                        },
                    );
                    patch = indent_patch(d);
                }
                for (before, want) in [(true, v.before), (false, v.after)] {
                    let Some(want) = want else { continue };
                    let sp = inh.spacing.as_deref();
                    let (tw, auto) = match (before, sp) {
                        (true, Some(sp)) => (num(sp.before), sp.before_autospacing == Some(true)),
                        (false, Some(sp)) => (num(sp.after), sp.after_autospacing == Some(true)),
                        (_, None) => (0.0, false),
                    };
                    spacing_direct(&mut patch, tw, auto, want, before);
                }
                let changed = obj::key_indices(&patch).into_iter().any(|i| {
                    let k = ParaProps::keys().get(usize::from(i)).copied().unwrap_or("");
                    patch_differs(&patch, &p.props, k)
                });
                if changed {
                    patches.push((pi, patch));
                }
            }
        }
        self.apply_para_patches(patches, false)
    }

    /// `setParagraphBorders(sides)`.
    fn set_paragraph_borders(&mut self, sides: Option<&[(ParaBorderSide, Option<BorderSpec>)]>) -> Res {
        let mut patch = ParaProps::default();
        let Some(sides) = sides else {
            pset!(patch, "borders", borders, None);
            pset!(patch, "boxBorder", box_border, None);
            return self.format_para(&patch);
        };
        let cur: ParagraphBorders = self
            .paras()
            .and_then(|ps| ps.get(self.selection.focus.para as usize))
            .and_then(|p| p.props.borders.as_deref().cloned())
            .unwrap_or_default();
        let mut out = cur;
        for (side, b) in sides {
            let Some(b) = b else {
                obj::delete(&mut out, side.key());
                continue;
            };
            if &*b.val == "none" || &*b.val == "nil" {
                obj::delete(&mut out, side.key());
                continue;
            }
            // the space to the text is Word's: 1 pt top/bottom, 4 pt on the sides
            let default_space = if matches!(side, ParaBorderSide::Top | ParaBorderSide::Bottom) { 1.0 } else { 4.0 };
            let size = match b.sz {
                Some(JsNumber::Num(x)) => x,
                _ => 4.0,
            };
            let space = match b.space {
                Some(JsNumber::Num(x)) => x,
                _ => default_space,
            };
            let color = b.color.as_deref().filter(|c| !c.is_empty() && *c != "auto");
            let mut e = BorderEdge {
                style: b.val.clone(),
                size_eighth_pt: JsNumber::Num(size),
                space_pt: JsNumber::Num(space),
                ..BorderEdge::default()
            };
            if let Some(c) = color {
                e.color = Some(c.into());
                // `{ style, sizeEighthPt, spacePt, color }`
                obj::set_key_order(&mut e, &[0, 1, 3, 2]);
            }
            side.set(&mut out, e);
        }
        let has = !obj::key_indices(&out).is_empty();
        pset!(patch, "borders", borders, if has { Some(Box::new(out)) } else { None });
        pset!(patch, "boxBorder", box_border, None);
        self.format_para(&patch)
    }

    /// `paragraphBorderPreset(kind)`.
    fn paragraph_border_preset(&mut self, kind: BorderPreset) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        if kind == BorderPreset::None {
            return self.set_paragraph_borders(None);
        }
        use ParaBorderSide as S;
        let sides: &[S] = match kind {
            BorderPreset::Bottom => &[S::Bottom],
            BorderPreset::Top => &[S::Top],
            BorderPreset::Left => &[S::Left],
            BorderPreset::Right => &[S::Right],
            BorderPreset::All => &[S::Top, S::Left, S::Bottom, S::Right, S::Between],
            BorderPreset::Outside => &[S::Top, S::Left, S::Bottom, S::Right],
            BorderPreset::Inside | BorderPreset::InsideH => &[S::Between],
            BorderPreset::InsideV => &[S::Bar],
            BorderPreset::None => &[],
        };
        let (s, e) = self.selected_para_range();
        let has = self.paras().is_some_and(|ps| {
            (s..=e)
                .filter_map(|i| ps.get(i))
                .all(|p| sides.iter().all(|sd| p.props.borders.as_deref().is_some_and(|b| sd.edge(b).is_some())))
        });
        self.begin_step(false, &[]);
        for pi in s..=e {
            let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { continue };
            let mut out: ParagraphBorders = p.props.borders.as_deref().cloned().unwrap_or_default();
            for sd in sides {
                if has {
                    obj::delete(&mut out, sd.key());
                } else {
                    let space = match sd {
                        S::Left | S::Right => 4.0,
                        S::Bar => 0.0,
                        _ => 1.0,
                    };
                    sd.set(&mut out, edge("single", 4.0, space));
                }
            }
            let any = !obj::key_indices(&out).is_empty();
            let mut patch = ParaProps::default();
            pset!(patch, "borders", borders, if any { Some(Box::new(out)) } else { None });
            pset!(patch, "boxBorder", box_border, None);
            if let Err(err) = self.apply_para(at(pi), &patch) {
                self.discard_step();
                return Err(err);
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `toggleBorder()`.
    fn toggle_border(&mut self) -> Res {
        let has = self
            .paras()
            .and_then(|ps| ps.get(self.selection.focus.para as usize))
            .is_some_and(|p| p.props.box_border == Some(true) || p.props.borders.is_some());
        let mut patch = ParaProps::default();
        pset!(patch, "boxBorder", box_border, None);
        let borders = if has {
            None
        } else {
            let mut b = ParagraphBorders::default();
            for s in [ParaBorderSide::Top, ParaBorderSide::Left, ParaBorderSide::Bottom, ParaBorderSide::Right] {
                s.set(&mut b, edge("single", 4.0, 1.0));
            }
            Some(Box::new(b))
        };
        pset!(patch, "borders", borders, borders);
        self.format_para(&patch)
    }

    /// `clearParagraphFormatting()` (`styleEdit`).
    fn clear_paragraph_formatting(&mut self) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.pdone();
        }
        self.begin_step(false, &[]);
        let range = self.range();
        let nr = normalize_range(range);
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        if let Err(e) = clear_para_formatting(paras, range) {
            self.discard_step();
            return Err(e.into());
        }
        for pi in nr.start.para..=nr.end.para {
            self.touch_para(pi as usize);
        }
        Ok(Outcome::Done(self.finish()))
    }

    // ── insertHorizontalLine ───────────────────────────────────────────────────────────────

    /// `canInsertField()`: editable, not the read view, and not inside a plain-text content control.
    fn can_insert_field_f1(&self) -> bool {
        if self.paras().is_none() || self.locked() {
            return false;
        }
        let DocRange { start, end } = normalize_range(self.range());
        let at: Vec<DocPosition> = if start == end { vec![end] } else { vec![start, end] };
        !at.iter().any(|pos| self.in_plain_text_control_at(*pos))
    }

    /// `inPlainTextControlAt` for one position.
    fn in_plain_text_control_at(&self, pos: DocPosition) -> bool {
        let Some(store) = self.doc.sources.as_deref() else { return false };
        let Some(p) = self.paras().and_then(|ps| ps.get(pos.para as usize)) else { return false };
        let run_at = |o: i64| -> Option<&Run> {
            let mut acc = 0i64;
            for r in &p.runs {
                let len = i64::try_from(r.text.len()).unwrap_or(i64::MAX);
                if o >= acc && o < acc + len {
                    return Some(r);
                }
                acc += len;
            }
            None
        };
        let o = i64::from(pos.offset);
        let around = [run_at(o - 1), run_at(o)];
        let Some(id) = p.props.src else { return false };
        let key = JsNumber::Num(f64::from(id.0));
        let parts = &store.parts;
        let in_groups = |groups: &[StoryGroup]| -> bool {
            fn walk(groups: &[StoryGroup], key: JsNumber, parts: &[crate::model::sources::SourcePart]) -> bool {
                for g in groups {
                    if !g.blocks.contains(&key) {
                        continue;
                    }
                    if plain_text_sdt(g.wrap_open.as_ref().map(|s| s.resolve(parts)))
                        || g.inner.as_deref().is_some_and(|i| walk(i, key, parts))
                    {
                        return true;
                    }
                }
                false
            }
            walk(groups, key, parts)
        };
        if store.stories.values().any(|s| in_groups(&s.groups)) {
            return true;
        }
        if store.tcs.values().any(|c| in_groups(c.groups.as_deref().unwrap_or(&[]))) {
            return true;
        }
        if store.txbx_groups.iter().any(|g| in_groups(g)) {
            return true;
        }
        let sids: Vec<u32> = around.iter().flatten().filter_map(|r| r.props.src.map(|k| k.0)).collect();
        if sids.is_empty() {
            return false;
        }
        store.paras.get(&id).is_some_and(|ps| {
            ps.segs.iter().any(|s| {
                plain_text_sdt(s.wrap_open.as_ref().map(|t| t.resolve(parts)))
                    && s.sids.iter().any(|x| matches!(x, JsNumber::Num(v) if sids.iter().any(|k| f64::from(*k) == *v)))
            })
        })
    }

    /// `splitAt(pos)` (guarded; tracked when tracking is on).
    fn split_at_f1(&mut self, pos: DocPosition) -> Result<DocPosition, CommandError> {
        let rev = self.opts.tracking.then(|| self.new_rev());
        let ctx = self.ctx.clone();
        let doc = &mut self.doc;
        let mut ids = std::mem::take(&mut doc.ids);
        let out = match story_of_mut(doc, &ctx) {
            Some(story) => match &rev {
                Some(rev) => guarded::track_split_paragraph(&mut story.paras, &mut ids, pos, rev),
                None => guarded::split_paragraph(&mut story.paras, &mut ids, pos),
            },
            None => Err(OpError::Position),
        };
        self.doc.ids = ids;
        let out = out?;
        self.touch_para(pos.para as usize);
        self.touch_structure();
        Ok(out)
    }

    /// `deleteSelection()` (text.rs's, private there): the selected range goes.
    fn delete_selection_f1(&mut self) -> Result<DocPosition, CommandError> {
        if self.opts.tracking {
            let rev = self.new_rev();
            let range = self.range();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            let r = guarded::track_delete(paras, range, &rev)?;
            for removed in &r.merged {
                self.shift_anchors(removed.saturating_sub(1), -1);
                self.touch_structure();
            }
            self.touch_para(r.end.para as usize);
            return Ok(self.canon_pos(r.end));
        }
        let range = self.range();
        let DocRange { start, end } = normalize_range(range);
        let delta = end.para.saturating_sub(start.para);
        if delta > 0 && matches!(self.ctx, EditContext::Body) {
            let story = self.story_mut().ok_or(CommandError::Position)?;
            let mut i = 0;
            while i < story.tables.len() {
                let gone = story.tables.get(i).is_some_and(|a| a.before_para > start.para && a.before_para <= end.para);
                if gone {
                    story.tables.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        let pos = guarded::delete_range(paras, range)?;
        if delta > 0 {
            self.shift_anchors(start.para, -i64::from(delta));
            self.touch_structure();
        }
        self.touch_para(pos.para as usize);
        Ok(pos)
    }

    /// `insertHorizontalLine()`: the selection becomes the line Word inserts, in a paragraph of
    /// its own (`<w:pict><v:rect … o:hr="t"/></w:pict>`).
    fn insert_horizontal_line(&mut self) -> Res {
        if self.paras().is_none() || self.locked() || !self.can_insert_field_f1() {
            return self.pdone();
        }
        self.begin_step(false, &[]);
        let r = self.insert_horizontal_line_step();
        if let Err(e) = r {
            self.discard_step();
            return Err(e);
        }
        Ok(Outcome::Done(self.finish()))
    }

    fn insert_horizontal_line_step(&mut self) -> Result<(), CommandError> {
        if self.has_selection() {
            let pos = self.delete_selection_f1()?;
            self.selection = Selection::caret(pos);
        }
        let mut pos = self.selection.focus;
        // `p` is the paragraph object BEFORE the split (the head keeps it)
        let orig = pos.para as usize;
        if pos.offset > 0 {
            self.shift_anchors(pos.para, 1);
            pos = self.split_at_f1(pos)?;
        }
        // `p.runs[insertIndex(p, pos.offset)]?.props ?? currentProps()`
        let neighbour: Option<CarriedProps> = {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            let p = paras.make_mut(orig).ok_or(CommandError::Position)?;
            let idx = insert_index(p, pos.offset as usize);
            p.runs.get(idx).map(CarriedProps::of_run)
        };
        let base = match neighbour {
            Some(b) => b,
            None => self.current_props_f1(),
        };
        let run = self.hr_run(base);
        let target_len = self.paras().and_then(|ps| ps.get(pos.para as usize)).map_or(0, para_length);
        if target_len > 0 {
            self.shift_anchors(pos.para, 1);
            self.split_at_f1(pos)?;
        }
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        let p = paras.make_mut(pos.para as usize).ok_or(CommandError::Position)?;
        p.runs.insert(0, run);
        self.touch_para(pos.para as usize);
        let n = self.paras().map_or(0, crate::pvec::PVec::len);
        let next = (pos.para as usize + 1).min(n.saturating_sub(1));
        self.selection = Selection::caret(DocPosition::new(u32_of(next), 0));
        Ok(())
    }

    fn current_props_f1(&self) -> CarriedProps {
        match self.paras() {
            Some(paras) => super::super::text::props_at(paras, self.selection.focus),
            None => CarriedProps::plain(crate::read::plain::default_run_props()),
        }
    }

    /// `hrRun(props)`: the run of the VML horizontal line (the reader recognises it as `o:hr`).
    fn hr_run(&mut self, base: CarriedProps) -> Run {
        let xml = "<w:pict><v:rect id=\"_x0000_i1025\" style=\"width:0;height:1.5pt\" o:hralign=\"center\" o:hrstd=\"t\" o:hr=\"t\" fillcolor=\"#a0a0a0\" stroked=\"f\"/></w:pict>";
        // `newRunProps(props)`: without the revisions, and the author's insertion with tracking on
        let mut props = without_revisions(&base.props);
        if self.opts.tracking {
            let rev = self.new_rev();
            obj::set(&mut props, "ins", |p| p.ins = Some(Box::new(rev)));
        }
        obj::delete(&mut props, "src");
        let mut shape = DrawingShape { drawing: self.doc.ids.drawing(), ..DrawingShape::default() };
        // `style: { fill: 'A0A0A0' }`: the TS style has no `geom`; the Rust style requires one
        // (declared in the record: `rect`)
        shape.style.geom = "rect".into();
        shape.style.fill = Some(crate::model::color::DrawingColor::plain("A0A0A0"));
        shape.width_twips = JsNumber::Num(0.0);
        shape.height_twips = JsNumber::Num(30.0);
        obj::set(&mut shape, "hr", |s| s.hr = Some(true));
        obj::set(&mut shape, "hrAlign", |s| s.hr_align = Some("center".into()));
        obj::set(&mut shape, "rawVml", |s| s.raw_vml = Some(xml.into()));
        let ext = RunExt { shape: Some(Box::new(shape)), ..RunExt::default() };
        Run { text: Utf16::from("\u{FFFC}"), props: Arc::new(props), ext: Some(Box::new(ext)), rpr_link: base.link, ..Run::default() }
    }

    pub(crate) fn exec_paragraph(&mut self, cmd: ParagraphCmd, _cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            ParagraphCmd::SetAlignment { alignment } => {
                let mut patch = ParaProps::default();
                obj::set(&mut patch, "alignment", |p| p.alignment = Some(alignment.as_str().into()));
                self.format_para(&patch)
            }
            ParagraphCmd::Indent { delta } => self.indent(delta),
            ParagraphCmd::SetLineSpacing { line } => {
                let mut patch = ParaProps::default();
                obj::set(&mut patch, "lineTwentieths", |p| p.line_twentieths = Some(JsNumber::Num(line)));
                self.format_para(&patch)
            }
            ParagraphCmd::SetSpaceAfter { twips } => {
                let mut patch = ParaProps::default();
                pset!(patch, "spaceAfterTwips", space_after_twips, twips.map(JsNumber::Num));
                pset!(patch, "spaceAfterAuto", space_after_auto, None);
                self.format_para(&patch)
            }
            ParagraphCmd::SetSpaceBefore { twips } => {
                let mut patch = ParaProps::default();
                pset!(patch, "spaceBeforeTwips", space_before_twips, twips.map(JsNumber::Num));
                pset!(patch, "spaceBeforeAuto", space_before_auto, None);
                self.format_para(&patch)
            }
            ParagraphCmd::SetShading { hex } => {
                let mut patch = ParaProps::default();
                pset!(patch, "shadingHex", shading_hex, hex.map(|h| Str::from(h.as_str())));
                self.format_para(&patch)
            }
            ParagraphCmd::SetSpecialIndent { kind } => {
                let mut patch = ParaProps::default();
                let v = match kind {
                    SpecialIndent::None => None,
                    SpecialIndent::FirstLine => Some(JsNumber::Num(708.0)),
                    SpecialIndent::Hanging => Some(JsNumber::Num(-708.0)),
                };
                pset!(patch, "indentFirstLineTwips", indent_first_line_twips, v);
                self.format_para(&patch)
            }
            ParagraphCmd::SetIndentRight { twips } => {
                let mut patch = ParaProps::default();
                pset!(patch, "indentRightTwips", indent_right_twips, twips.map(JsNumber::Num));
                self.format_para(&patch)
            }
            ParagraphCmd::SetParagraphIndents { ind } => self.set_paragraph_indents(ind),
            ParagraphCmd::SetTabStops { tabs, only } => self.set_tab_stops(&tabs, only.as_ref()),
            ParagraphCmd::SetParagraphFlow { f } => self.set_paragraph_flow(&f),
            ParagraphCmd::SetParagraphLayout { v } => self.set_paragraph_layout(v),
            ParagraphCmd::SetParagraphBorders { sides } => self.set_paragraph_borders(sides.as_deref()),
            ParagraphCmd::ParagraphBorderPreset { kind } => self.paragraph_border_preset(kind),
            ParagraphCmd::ToggleBorder => self.toggle_border(),
            ParagraphCmd::InsertHorizontalLine => self.insert_horizontal_line(),
            ParagraphCmd::ClearParagraphFormatting => self.clear_paragraph_formatting(),
        }
    }
}

/// `edge`: `{ style, sizeEighthPt, spacePt }` (no colour: the key order is the declaration's).
fn edge(style: &str, size: f64, space: f64) -> BorderEdge {
    BorderEdge { style: style.into(), size_eighth_pt: JsNumber::Num(size), space_pt: JsNumber::Num(space), ..BorderEdge::default() }
}

/// `{ indentLeftTwips, indentRightTwips, indentFirstLineTwips }`, a value or `undefined` each.
fn indent_patch(d: [Option<f64>; 3]) -> ParaProps {
    let mut patch = ParaProps::default();
    pset!(patch, "indentLeftTwips", indent_left_twips, d[0].map(JsNumber::Num));
    pset!(patch, "indentRightTwips", indent_right_twips, d[1].map(JsNumber::Num));
    pset!(patch, "indentFirstLineTwips", indent_first_line_twips, d[2].map(JsNumber::Num));
    patch
}

/// `patch[k] !== props[k]` for the keys `setParagraphLayout` patches.
fn patch_differs(patch: &ParaProps, cur: &ParaProps, key: &str) -> bool {
    fn n(v: Option<JsNumber>) -> Option<f64> {
        match v {
            Some(JsNumber::Num(x)) => Some(x),
            _ => None,
        }
    }
    fn eqn(a: Option<JsNumber>, b: Option<JsNumber>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(JsNumber::Null), Some(JsNumber::Null)) => true,
            (x, y) => n(x).is_some() && n(x) == n(y),
        }
    }
    let same = match key {
        "indentLeftTwips" => eqn(patch.indent_left_twips, cur.indent_left_twips),
        "indentRightTwips" => eqn(patch.indent_right_twips, cur.indent_right_twips),
        "indentFirstLineTwips" => eqn(patch.indent_first_line_twips, cur.indent_first_line_twips),
        "spaceBeforeTwips" => eqn(patch.space_before_twips, cur.space_before_twips),
        "spaceAfterTwips" => eqn(patch.space_after_twips, cur.space_after_twips),
        "spaceBeforeAuto" => patch.space_before_auto == cur.space_before_auto,
        "spaceAfterAuto" => patch.space_after_auto == cur.space_after_auto,
        _ => true,
    };
    !same
}

#[allow(dead_code)]
fn unused(_: &Selection) {}
