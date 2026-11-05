//! The formatting policy (`editor/src/formatting-policy.ts`, its pure half, ported 1:1): what the
//! host lets the user format. The session refuses the commands the policy forbids (the TS
//! `POLICY_GUARDS`, by TS method name: [`guard_allows`]) and the paste family sanitizes pasted
//! formatting with [`sanitize_run_props`] / [`sanitize_para_props`]. The ids of the bars and menus
//! the policy hides (`policyHiddenIds`) stay in the shell (UI).

use std::collections::BTreeSet;

use crate::js::JsNumber;
use crate::model::para::ParaProps;
use crate::model::run::RunProps;
use crate::word::obj;

/// `CharacterFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CharacterFormat {
    Bold,
    Italic,
    Underline,
    UnderlineStyles,
    Strike,
    DoubleStrike,
    Superscript,
    Subscript,
    Color,
    Highlight,
    Link,
    Caps,
    Hidden,
    CharSpacing,
    TextEffects,
}

/// `ListKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    Bullet,
    Decimal,
}

/// `indentation`: `true`, `'steps'` or `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indentation {
    #[default]
    All,
    Steps,
    None,
}

/// The boolean flags of the policy (`Flag`); absent = allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Flag {
    Font,
    FontSize,
    Alignment,
    Spacing,
    Borders,
    Shading,
    Tabs,
    StyleEditing,
    FormatPainter,
    PageSetup,
    PageBackground,
    HeadersFooters,
    Columns,
    PageBreaks,
    SectionBreaks,
    Tables,
    Images,
    ImageFormatting,
    Shapes,
}

/// `FormattingPolicy` (the host's property `formattingPolicy`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormattingPolicy {
    pub paragraph_styles: Option<Vec<Box<str>>>,
    pub character_styles: Option<Vec<Box<str>>>,
    /// `{ kinds, levels }`
    pub lists: Option<(Vec<ListKind>, Option<f64>)>,
    pub character: Option<Vec<CharacterFormat>>,
    pub indentation: Option<Indentation>,
    /// the flags set to `false` (absent or `true`: allowed)
    pub denied: BTreeSet<Flag>,
}

impl FormattingPolicy {
    /// `TEMPLATE_DOCUMENT_POLICY` (preset `template-document`).
    #[must_use]
    pub fn template_document() -> FormattingPolicy {
        use CharacterFormat as C;
        use Flag as F;
        FormattingPolicy {
            paragraph_styles: Some(
                ["Normal", "Heading1", "Heading2", "Heading3", "Title", "Subtitle"].into_iter().map(Into::into).collect(),
            ),
            character_styles: Some(Vec::new()),
            lists: Some((vec![ListKind::Bullet, ListKind::Decimal], Some(9.0))),
            character: Some(vec![C::Bold, C::Italic, C::Underline, C::Strike, C::Color, C::Highlight, C::Link]),
            indentation: Some(Indentation::Steps),
            denied: [
                F::Font,
                F::FontSize,
                F::Spacing,
                F::Borders,
                F::Shading,
                F::Tabs,
                F::StyleEditing,
                F::FormatPainter,
                F::PageSetup,
                F::PageBackground,
                F::HeadersFooters,
                F::SectionBreaks,
                F::ImageFormatting,
                F::Shapes,
            ]
            .into_iter()
            .collect(),
        }
    }

    /// `FORMATTING_POLICY_PRESETS[name]`.
    #[must_use]
    pub fn preset(name: &str) -> Option<FormattingPolicy> {
        (name == "template-document").then(FormattingPolicy::template_document)
    }

    /// `resolvePolicy(p)`.
    #[must_use]
    pub fn resolve(&self) -> ResolvedPolicy {
        let styles: Option<BTreeSet<String>> = self.paragraph_styles.as_ref().map(|v| v.iter().map(|s| style_key(s)).collect());
        let char_styles = match &self.character_styles {
            Some(v) => Some(v.iter().map(|s| style_key(s)).collect()),
            None => styles.as_ref().map(|_| BTreeSet::new()),
        };
        let lists = self.lists.as_ref().map(|(kinds, levels)| {
            // `Math.max(1, Math.min(9, Math.floor(levels ?? 9)))` (NaN stays NaN through min/max)
            let l = uo_jsnum::math::max(1.0, uo_jsnum::math::min(9.0, uo_jsnum::math::floor(levels.unwrap_or(9.0))));
            ResolvedLists { bullet: kinds.contains(&ListKind::Bullet), decimal: kinds.contains(&ListKind::Decimal), levels: l }
        });
        ResolvedPolicy {
            styles,
            char_styles,
            lists,
            chars: self.character.as_ref().map(|v| v.iter().copied().collect()),
            indentation: self.indentation.unwrap_or_default(),
            denied: self.denied.clone(),
        }
    }
}

/// `ResolvedPolicy.lists`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedLists {
    pub bullet: bool,
    pub decimal: bool,
    /// 1–9 (NaN for a NaN `levels`, as the TS)
    pub levels: f64,
}

/// `ResolvedPolicy`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResolvedPolicy {
    pub styles: Option<BTreeSet<String>>,
    pub char_styles: Option<BTreeSet<String>>,
    pub lists: Option<ResolvedLists>,
    pub chars: Option<BTreeSet<CharacterFormat>>,
    pub indentation: Indentation,
    denied: BTreeSet<Flag>,
}

impl ResolvedPolicy {
    /// `allows(flag)`.
    #[must_use]
    pub fn allows(&self, f: Flag) -> bool {
        !self.denied.contains(&f)
    }

    /// `char(f)`.
    #[must_use]
    pub fn char(&self, f: CharacterFormat) -> bool {
        self.chars.as_ref().is_none_or(|c| c.contains(&f))
    }
}

/// `styleKey(nameOrId)`: lower case, without white space, `_` and `-` (`/[\s_-]+/g`, JS `\s`).
#[must_use]
pub fn style_key(name: &str) -> String {
    let lower = uo_jsnum::text::to_lowercase(name);
    lower.chars().filter(|c| *c != '_' && *c != '-' && !uo_jsnum::text::is_js_space(*c)).collect()
}

/// `styleAllowed(rp, key)`.
#[must_use]
pub fn style_allowed(rp: Option<&ResolvedPolicy>, key: &str) -> bool {
    rp.and_then(|r| r.styles.as_ref()).is_none_or(|s| s.contains(key))
}

/// `/^heading([1-9])$/`.
fn heading_level(key: &str) -> Option<u32> {
    let d = key.strip_prefix("heading")?;
    let b = d.as_bytes();
    (b.len() == 1 && (b'1'..=b'9').contains(&b[0])).then(|| u32::from(b[0] - b'0'))
}

/// `nearestAllowedStyle(rp, key)`: `None` = Normal.
#[must_use]
pub fn nearest_allowed_style(rp: Option<&ResolvedPolicy>, key: &str) -> Option<String> {
    if style_allowed(rp, key) {
        return (key != "normal").then(|| key.to_owned());
    }
    let styles = rp.and_then(|r| r.styles.as_ref())?;
    if let Some(h) = heading_level(key) {
        return (1..h).rev().map(|n| format!("heading{n}")).find(|k| styles.contains(k));
    }
    (key == "title" && styles.contains("heading1")).then(|| "heading1".to_owned())
}

/// `builtinNameOfKey(key)`.
#[must_use]
pub fn builtin_name_of_key(key: &str) -> Option<String> {
    if let Some(h) = heading_level(key) {
        return Some(format!("heading {h}"));
    }
    match key {
        "title" => Some("Title".into()),
        "subtitle" => Some("Subtitle".into()),
        "normal" => Some("Normal".into()),
        _ => None,
    }
}

/// `policySplitsToPlain(rp)`.
#[must_use]
pub fn policy_splits_to_plain(rp: Option<&ResolvedPolicy>) -> Vec<&'static str> {
    let Some(rp) = rp else { return Vec::new() };
    let mut out = Vec::new();
    if rp.lists.is_some() {
        out.push("list:bullet");
        out.push("list:decimal");
    }
    if !rp.char(CharacterFormat::UnderlineStyles) {
        out.push("underline");
    }
    out
}

/// `off(k)` of `sanitizeRunProps`: the boolean ones go `false` when present, the rest go.
fn off(p: &mut RunProps, k: &str) {
    const BOOLS: [&str; 6] = ["bold", "italic", "underline", "strike", "allCaps", "smallCaps"];
    if BOOLS.contains(&k) {
        let present = <RunProps as crate::js::JsObject>::key_index(k).is_some_and(|i| obj::has_key(p, usize::from(i)));
        if present {
            obj::set(p, k, |p| match k {
                "bold" => p.bold = Some(false),
                "italic" => p.italic = Some(false),
                "underline" => p.underline = Some(false),
                "strike" => p.strike = Some(false),
                "allCaps" => p.all_caps = Some(false),
                _ => p.small_caps = Some(false),
            });
        }
    } else {
        obj::delete(p, k);
    }
}

/// `sanitizeRunProps(props, rp)`: a copy with only what the policy allows.
#[must_use]
pub fn sanitize_run_props(props: &RunProps, rp: Option<&ResolvedPolicy>) -> RunProps {
    use CharacterFormat as C;
    let mut p = props.clone();
    let Some(rp) = rp else { return p };
    if !rp.allows(Flag::Font) {
        off(&mut p, "fontAscii");
    }
    if !rp.allows(Flag::FontSize) {
        off(&mut p, "szHalfPt");
    }
    if !rp.char(C::Bold) {
        off(&mut p, "bold");
    }
    if !rp.char(C::Italic) {
        off(&mut p, "italic");
    }
    if !rp.char(C::Underline) {
        for k in ["underline", "underlineStyle", "underlineColor", "underlineTheme"] {
            off(&mut p, k);
        }
    }
    if !rp.char(C::UnderlineStyles) {
        for k in ["underlineStyle", "underlineColor", "underlineTheme"] {
            off(&mut p, k);
        }
    }
    if p.double_strike == Some(true) && !rp.char(C::DoubleStrike) {
        if rp.char(C::Strike) {
            obj::set(&mut p, "strike", |x| x.strike = Some(true));
        }
        off(&mut p, "doubleStrike");
    }
    if !rp.char(C::Strike) {
        off(&mut p, "strike");
    }
    let va = p.vert_align.as_deref();
    if (va == Some("superscript") && !rp.char(C::Superscript)) || (va == Some("subscript") && !rp.char(C::Subscript)) {
        off(&mut p, "vertAlign");
    }
    if !rp.char(C::Color) {
        off(&mut p, "color");
        off(&mut p, "colorTheme");
    }
    if !rp.char(C::Highlight) {
        off(&mut p, "highlight");
    }
    if !rp.char(C::Link) {
        off(&mut p, "link");
        off(&mut p, "linkTip");
    }
    if !rp.char(C::Caps) {
        off(&mut p, "allCaps");
        off(&mut p, "smallCaps");
    }
    if !rp.char(C::Hidden) {
        off(&mut p, "hidden");
        off(&mut p, "webHidden");
    }
    if !rp.char(C::CharSpacing) {
        for k in ["charSpacingTwips", "scalePct", "positionHalfPt", "kernHalfPt"] {
            off(&mut p, k);
        }
    }
    if !rp.char(C::TextEffects) {
        off(&mut p, "textFx");
        off(&mut p, "openType");
    }
    if let (Some(cs), Some(allowed)) = (p.char_style.as_deref().filter(|s| !s.is_empty()), rp.char_styles.as_ref()) {
        let key = style_key(cs);
        if !allowed.contains(&key) && !(p.link.as_deref().is_some_and(|l| !l.is_empty()) && key == "hyperlink") {
            off(&mut p, "charStyle");
        }
    }
    p
}

/// `StyleResolver`: how the paste resolves styles in the target document.
pub trait StyleResolver {
    fn key_of(&self, id: &str) -> String;
    fn id_of(&self, key: &str) -> Option<String>;
}

/// `sanitizeParaProps(mark, rp, styles)`.
#[must_use]
pub fn sanitize_para_props(mark: &ParaProps, rp: Option<&ResolvedPolicy>, styles: &dyn StyleResolver) -> ParaProps {
    let mut m = mark.clone();
    let Some(rp) = rp else { return m };
    if rp.styles.is_some() {
        if let Some(sid) = m.style_id.clone().filter(|s| !s.is_empty()) {
            let key = nearest_allowed_style(Some(rp), &styles.key_of(&sid));
            match key.and_then(|k| styles.id_of(&k)).filter(|s| !s.is_empty()) {
                Some(id) => obj::set(&mut m, "styleId", |x| x.style_id = Some(id.into())),
                None => obj::delete(&mut m, "styleId"),
            }
        }
        obj::delete(&mut m, "outlineLvl");
    }
    if !rp.allows(Flag::Alignment) {
        obj::delete(&mut m, "alignment");
    }
    if rp.indentation != Indentation::All {
        let left = m.indent_left_twips;
        for k in ["indentLeftTwips", "indentRightTwips", "indentFirstLineTwips"] {
            obj::delete(&mut m, k);
        }
        // `left && left > 0 && !m.numbering`: a truthy number above 0
        if rp.indentation == Indentation::Steps
            && let Some(JsNumber::Num(l)) = left
            && l > 0.0
            && m.numbering.is_none()
        {
            let steps = uo_jsnum::math::round(l / 720.0);
            if steps > 0.0 {
                obj::set(&mut m, "indentLeftTwips", |x| x.indent_left_twips = Some(JsNumber::Num(steps * 720.0)));
            }
        }
    }
    if !rp.allows(Flag::Spacing) {
        for k in [
            "lineTwentieths",
            "spaceBeforeTwips",
            "spaceAfterTwips",
            "spaceBeforeAuto",
            "spaceAfterAuto",
            "keepNext",
            "keepLines",
            "widowControl",
            "suppressAutoHyphens",
            "textboxTightWrap",
        ] {
            obj::delete(&mut m, k);
        }
    }
    if !rp.allows(Flag::Borders) {
        obj::delete(&mut m, "borders");
        obj::delete(&mut m, "boxBorder");
    }
    if !rp.allows(Flag::Shading) {
        obj::delete(&mut m, "shadingHex");
    }
    if !rp.allows(Flag::Tabs) {
        obj::delete(&mut m, "tabStops");
    }
    if !rp.allows(Flag::PageBreaks) {
        obj::delete(&mut m, "pageBreakBefore");
    }
    if !rp.allows(Flag::SectionBreaks) || !rp.allows(Flag::PageSetup) {
        obj::delete(&mut m, "sectionBreak");
    }
    if !rp.allows(Flag::PageSetup) {
        obj::delete(&mut m, "suppressLineNumbers");
    }
    if !rp.char(CharacterFormat::TextEffects) {
        obj::delete(&mut m, "dropCap");
    }
    m
}

/// `listKindOfFormat(numFmt)`.
#[must_use]
pub fn list_kind_of_format(num_fmt: Option<&str>) -> ListKind {
    if num_fmt == Some("bullet") { ListKind::Bullet } else { ListKind::Decimal }
}

/// `policyList(rp, kind, level)`: `None` = the policy allows no list.
#[must_use]
pub fn policy_list(rp: Option<&ResolvedPolicy>, kind: ListKind, level: f64) -> Option<(ListKind, f64)> {
    let Some(l) = rp.and_then(|r| r.lists) else { return Some((kind, level)) };
    let allowed = |k: ListKind| if k == ListKind::Bullet { l.bullet } else { l.decimal };
    let k = if allowed(kind) {
        Some(kind)
    } else if l.bullet {
        Some(ListKind::Bullet)
    } else if l.decimal {
        Some(ListKind::Decimal)
    } else {
        None
    };
    k.map(|k| (k, uo_jsnum::math::max(0.0, uo_jsnum::math::min(l.levels - 1.0, level))))
}

/// The arguments a guard reads (`args` of `POLICY_GUARDS`), as the command gives them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GuardArg<'a> {
    #[default]
    None,
    Str(&'a str),
    Num(f64),
    /// `setParagraphLayout(o)`: which keys `o` has (left/right, before/after)
    Layout {
        indents: bool,
        spacing: bool,
    },
    /// `applyListLibrary(kind, index)`
    ListLibrary {
        bullet: bool,
        index: Option<f64>,
    },
}

/// What a guard needs of the session (`PolicyGuardHost`): the style key of a style id.
pub trait PolicyGuardHost {
    fn style_key_of(&self, id: &str) -> String;
}

/// `POLICY_GUARDS[method](rp, args, host)`: whether the policy lets the TS method run (methods
/// without a guard always run). `args` is the argument the guard reads.
#[must_use]
pub fn guard_allows(rp: &ResolvedPolicy, method: &str, arg: GuardArg<'_>, host: &dyn PolicyGuardHost) -> bool {
    use CharacterFormat as C;
    use Flag as F;
    let ch = |f: C| rp.char(f);
    let flag = |f: F| rp.allows(f);
    let indent_all = rp.indentation == Indentation::All;
    let lists_free = rp.lists.is_none();
    let styles_free = rp.styles.is_none();
    let s = match arg {
        GuardArg::Str(s) => Some(s),
        _ => None,
    };
    match method {
        "toggleRunProp" => match s.unwrap_or("") {
            "bold" => ch(C::Bold),
            "italic" => ch(C::Italic),
            "underline" => ch(C::Underline),
            "strike" => ch(C::Strike),
            "doubleStrike" => ch(C::DoubleStrike),
            "allCaps" | "smallCaps" => ch(C::Caps),
            "hidden" => ch(C::Hidden),
            _ => true,
        },
        "toggleUnderline" => ch(C::Underline) && (s.is_none() || s == Some("single") || ch(C::UnderlineStyles)),
        "setUnderline" => s == Some("none") || (ch(C::Underline) && (s == Some("single") || s.is_none() || ch(C::UnderlineStyles))),
        "setUnderlineColor" => ch(C::UnderlineStyles),
        "toggleVertAlign" => ch(if s == Some("subscript") { C::Subscript } else { C::Superscript }),
        "setFontFamily" | "setDefaultFont" => flag(F::Font),
        "setFontSize" | "growFont" => flag(F::FontSize),
        "setFontFormat" => flag(F::Font) && flag(F::FontSize) && rp.chars.is_none(),
        "setColor" => ch(C::Color),
        "setHighlight" => ch(C::Highlight),
        "setLink" | "insertHyperlink" => ch(C::Link),
        "setCharSpacing" => ch(C::CharSpacing),
        "setOpenType" | "setTextEffects" | "setTextGradient" | "applyTextEffectPreset" => ch(C::TextEffects),
        "copyFormat" => flag(F::FormatPainter),
        "setAlignment" => flag(F::Alignment),
        "indent" => rp.indentation != Indentation::None,
        "setSpecialIndent" | "setIndentRight" | "setParagraphIndents" => indent_all,
        "setParagraphLayout" => match arg {
            GuardArg::Layout { indents, spacing } => (!indents || indent_all) && (!spacing || flag(F::Spacing)),
            _ => true,
        },
        "setLineSpacing" | "setSpaceBefore" | "setSpaceAfter" | "setParagraphFlow" => flag(F::Spacing),
        "setShading" => flag(F::Shading),
        "toggleBorder" | "setParagraphBorders" | "insertHorizontalLine" => flag(F::Borders),
        "setTabStops" | "setDefaultTabStop" => flag(F::Tabs),
        "applyStyle" => styles_free || style_allowed(Some(rp), &s.map_or_else(|| "normal".to_owned(), |id| host.style_key_of(id))),
        "createStyle" | "modifyStyle" | "renameStyle" | "deleteStyle" | "updateStyleFromSelection" | "setStyleInGallery" => {
            flag(F::StyleEditing)
        }
        "createListStyle" | "modifyListStyle" => flag(F::StyleEditing) && lists_free,
        "toggleList" => rp.lists.is_none_or(|l| if s == Some("bullet") { l.bullet } else { l.decimal }),
        "applyListLibrary" => match (rp.lists, arg) {
            (None, _) => true,
            (Some(l), GuardArg::ListLibrary { bullet, index }) => {
                index.is_none() || (index == Some(0.0) && if bullet { l.bullet } else { l.decimal })
            }
            (Some(_), _) => false,
        },
        "applyMultilevelLibrary" => lists_free || matches!(arg, GuardArg::None),
        "applyListLevels" | "applyListStyle" | "defineListLevel" | "defineMultilevelList" | "setNumberingValue" => lists_free,
        "setListLevel" => rp.lists.is_none_or(|l| match arg {
            GuardArg::Num(n) => n < l.levels,
            _ => false,
        }),
        "insertPageBreak" | "insertBlankPage" => flag(F::PageBreaks),
        "insertColumnBreak" => flag(F::PageBreaks) && flag(F::Columns),
        "insertSectionBreak" => flag(F::SectionBreaks) || (s == Some("nextColumn") && flag(F::Columns)),
        "setColumns" => flag(F::Columns),
        "setPageSetup"
        | "setSectionStart"
        | "setVerticalAlignment"
        | "setLineNumbering"
        | "setLineNumberingOptions"
        | "toggleLineNumberSuppression"
        | "setHyphenation"
        | "setHyphenationOptions"
        | "applyManualHyphenation" => flag(F::PageSetup),
        "setPageColor" | "setPageFill" | "setWatermark" | "setPageBorders" => flag(F::PageBackground),
        "enterHeaderFooter"
        | "insertHeaderFooterTemplate"
        | "setDifferentFirstPage"
        | "setDifferentOddEven"
        | "setHeaderFooterDistance"
        | "setPageNumberFormat" => flag(F::HeadersFooters),
        "insertTable" | "convertTextToTable" => flag(F::Tables),
        "insertImage" => flag(F::Images),
        "setImageFormat"
        | "setImageBorder"
        | "setImageCrop"
        | "setImageRecolor"
        | "setImageTransparency"
        | "setImageTransparentColor"
        | "setImageLayout"
        | "setImageWrap"
        | "setImagePosition"
        | "setImageRotation" => flag(F::ImageFormatting),
        "insertTextbox" | "drawTextbox" | "insertCoverPage" | "setShapeFill" | "setShapeOutline" | "setShapeEffect" | "setShapeStyle"
        | "setShapeFormat" => flag(F::Shapes),
        "insertTableOfContents" | "insertTocTemplate" | "insertTableOfFigures" | "insertCaption" => styles_free,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ids;
    impl PolicyGuardHost for Ids {
        fn style_key_of(&self, id: &str) -> String {
            style_key(id)
        }
    }

    #[test]
    fn template_policy() {
        let rp = FormattingPolicy::template_document().resolve();
        assert!(rp.allows(Flag::Alignment));
        assert!(!rp.allows(Flag::Font));
        assert!(guard_allows(&rp, "toggleRunProp", GuardArg::Str("bold"), &Ids));
        assert!(!guard_allows(&rp, "toggleRunProp", GuardArg::Str("hidden"), &Ids));
        assert!(guard_allows(&rp, "applyStyle", GuardArg::Str("Heading 2"), &Ids));
        assert!(!guard_allows(&rp, "applyStyle", GuardArg::Str("Heading4"), &Ids));
        assert!(guard_allows(&rp, "applyStyle", GuardArg::None, &Ids));
        assert_eq!(nearest_allowed_style(Some(&rp), "heading5").as_deref(), Some("heading3"));
        assert_eq!(nearest_allowed_style(Some(&rp), "title").as_deref(), Some("title"));
        assert_eq!(style_key("Heading 1"), "heading1");
        assert_eq!(style_key("a_b-c\u{a0}d"), "abcd");
        assert_eq!(policy_list(Some(&rp), ListKind::Decimal, 12.0), Some((ListKind::Decimal, 8.0)));
    }

    #[test]
    fn sanitize_runs() {
        let rp = FormattingPolicy::template_document().resolve();
        let mut p = RunProps::default();
        obj::set(&mut p, "fontAscii", |x| x.font_ascii = Some("Arial".into()));
        obj::set(&mut p, "doubleStrike", |x| x.double_strike = Some(true));
        obj::set(&mut p, "allCaps", |x| x.all_caps = Some(true));
        let s = sanitize_run_props(&p, Some(&rp));
        assert_eq!(s.font_ascii, None);
        assert_eq!(s.double_strike, None);
        assert_eq!(s.strike, Some(true));
        assert_eq!(s.all_caps, Some(false));
    }
}
