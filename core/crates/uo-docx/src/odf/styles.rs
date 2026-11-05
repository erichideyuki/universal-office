//! docx-core `odf/styles.ts`: the ODF style cascade.
//!
//! Styles live in `content.xml` `office:automatic-styles` (direct formatting named `P1`, `T1`…) and
//! in `styles.xml` `office:styles` (named styles: `Standard`, `Heading 1`…) and
//! `office:automatic-styles` (page layout…). A `style:style` may have `style:parent-style-name`;
//! the resolution accumulates `style:text-properties` (run) and `style:paragraph-properties`
//! (paragraph) from the parent to the child. The `text:list-style`s are collected by name, to
//! decide bullet against number.
//!
//! Differences from the TS, all declared in the record (`uo-docx-d5/D5b.md`):
//! - [`OdfStyleIndex::resolve_style`] walks the `parent-style-name` chain with a loop (the TS
//!   recurses), up to `DocxLimits::max_based_on_chain` links; a chain of the TS that is deeper
//!   than its stack would be a `RangeError` there and is a limit error here;
//! - the result of an acyclic chain is cached per style (at most [`CACHE_CAP`] entries), where the
//!   TS resolves again at every paragraph; a chain that ends in a cycle is never cached (its
//!   result depends on the style the walk started from, through the `seen` set).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::trim;
use uo_opc::xml::Element;

use crate::error::{DocxError, DocxLimitKind};
use crate::js::{JsAssign, JsMap, JsNumber, JsObject, NumKey, Slot, Str};
use crate::model::{ParaProps, RunProps};

use super::props::{ResolvedParaProps, ResolvedRunProps};
use super::units::{length_to_half_pt, length_to_twips};

/// Most resolved styles kept by [`OdfStyleIndex`] (M5).
pub const CACHE_CAP: usize = 4096;

/// `RawStyle`: one `style:style` (or `style:default-style`) with the property elements it holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawStyle<'d> {
    pub family: Option<&'d str>,
    pub parent: Option<&'d str>,
    /// the list style associated (`style:list-style-name`), if any
    pub list_style_name: Option<&'d str>,
    /// `style:text-properties`
    pub text: Option<Element<'d>>,
    /// `style:paragraph-properties`
    pub para: Option<Element<'d>>,
    /// `style:table-column-properties` (column width)
    pub column: Option<Element<'d>>,
    /// `style:table-cell-properties` (cell borders and background)
    pub cell: Option<Element<'d>>,
    /// `style:graphic-properties` (frames: wrap, position, padding)
    pub graphic: Option<Element<'d>>,
}

/// `ListKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Decimal,
}

impl ListKind {
    /// The model's `numbering.kind` string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ListKind::Bullet => "bullet",
            ListKind::Decimal => "decimal",
        }
    }
}

/// `ResolvedStyle` of the ODF reader (not the docx one): the accumulated run and paragraph
/// properties and the inherited list style name.
#[derive(Debug, Clone, Default)]
pub struct ResolvedStyle<'d> {
    pub run: ResolvedRunProps,
    pub para: ResolvedParaProps,
    /// `style:list-style-name` inherited (for paragraphs that belong to a list)
    pub list_style_name: Option<&'d str>,
}

/// `OdfStyleIndex`.
#[derive(Debug, Default)]
pub struct OdfStyleIndex<'d> {
    /// `style:name` → the style (automatic and named; a named one overwrites a duplicate), in `Map`
    /// insertion order
    pub styles: IndexMap<&'d str, RawStyle<'d>>,
    /// `text:style-name` of lists → level (1-based) → kind
    pub list_styles: HashMap<&'d str, JsMap<NumKey, ListKind>>,
    /// `style:default-style` by family (`paragraph`…)
    pub defaults: HashMap<&'d str, RawStyle<'d>>,
    cache: HashMap<&'d str, Arc<ResolvedStyle<'d>>>,
    empty: Arc<ResolvedStyle<'d>>,
}

fn read_raw_style(st: Element<'_>) -> RawStyle<'_> {
    RawStyle {
        family: st.attr("style:family"),
        parent: st.attr("style:parent-style-name"),
        list_style_name: st.attr("style:list-style-name"),
        text: st.first_child("style:text-properties"),
        para: st.first_child("style:paragraph-properties"),
        column: st.first_child("style:table-column-properties"),
        cell: st.first_child("style:table-cell-properties"),
        graphic: st.first_child("style:graphic-properties"),
    }
}

fn read_list_style(ls: Element<'_>) -> JsMap<NumKey, ListKind> {
    let mut levels: JsMap<NumKey, ListKind> = JsMap::new();
    for lvl in ls.child_elements() {
        let level = NumKey::new(parse_int(lvl.attr("text:level").unwrap_or("1"), 10));
        match lvl.name() {
            "text:list-level-style-bullet" | "text:list-level-style-image" => {
                levels.insert(level, ListKind::Bullet);
            }
            "text:list-level-style-number" => {
                levels.insert(level, ListKind::Decimal);
            }
            _ => {}
        }
    }
    levels
}

/// `collectStyles(container, index)`.
fn collect_styles<'d>(container: Option<Element<'d>>, index: &mut OdfStyleIndex<'d>) {
    let Some(container) = container else { return };
    for el in container.child_elements() {
        match el.name() {
            "style:style" => {
                if let Some(name) = el.attr("style:name").filter(|n| !n.is_empty()) {
                    index.styles.insert(name, read_raw_style(el));
                }
            }
            "style:default-style" => {
                if let Some(family) = el.attr("style:family").filter(|n| !n.is_empty()) {
                    index.defaults.insert(family, read_raw_style(el));
                }
            }
            "text:list-style" => {
                if let Some(name) = el.attr("style:name").filter(|n| !n.is_empty()) {
                    index.list_styles.insert(name, read_list_style(el));
                }
            }
            _ => {}
        }
    }
}

/// `buildStyleIndex(contentRoot, stylesRoot)`: the named styles (`styles.xml`) first, the automatic
/// ones (`content.xml`) over them, so that an automatic `P1` wins on a name collision.
#[must_use]
pub fn build_style_index<'d>(content_root: Element<'d>, styles_root: Option<Element<'d>>) -> OdfStyleIndex<'d> {
    let mut index = OdfStyleIndex::default();
    if let Some(root) = styles_root {
        collect_styles(root.first_child("office:styles"), &mut index);
        collect_styles(root.first_child("office:automatic-styles"), &mut index);
    }
    collect_styles(content_root.first_child("office:automatic-styles"), &mut index);
    index
}

/// `isTrue(v)`.
fn is_true(v: Option<&str>) -> bool {
    matches!(v, Some("bold" | "italic" | "true" | "700"))
}

fn s(x: &str) -> Str {
    x.into()
}

/// `/^super\b/` and `/^sub\b/` (`\b` after a word: the next character is not `[A-Za-z0-9_]` or
/// there is none).
fn word_prefix(v: &str, word: &str) -> bool {
    v.strip_prefix(word).is_some_and(|rest| rest.bytes().next().is_none_or(|b| !uo_jsnum::text::is_js_word(b)))
}

/// `readTextProps(el)`: `style:text-properties` of ONE style (no inheritance).
#[must_use]
pub fn read_text_props(el: Option<Element<'_>>) -> RunProps {
    let mut p = RunProps::default();
    let Some(el) = el else { return p };
    if let Some(v) = el.attr("fo:font-weight") {
        p.bold = Some(is_true(Some(v)));
    }
    if let Some(v) = el.attr("fo:font-style") {
        p.italic = Some(v == "italic" || v == "oblique");
    }
    if let Some(u) = el.attr("style:text-underline-style") {
        p.underline = Some(u != "none");
    }
    if let Some(l) = el.attr("style:text-line-through-style") {
        p.strike = Some(l != "none");
    }
    if let Some(c) = el.attr("fo:color") {
        p.color = Some(norm_hex(c));
    }
    if let Some(bg) = el.attr("fo:background-color")
        && bg != "transparent"
    {
        p.highlight = Some(norm_hex(bg));
    }
    if let Some(size) = length_to_half_pt(el.attr("fo:font-size")) {
        p.sz_half_pt = Some(JsNumber::Num(size));
    }
    if let Some(font) = el.attr("style:font-name").or_else(|| el.attr("fo:font-family")).filter(|f| !f.is_empty()) {
        p.font_ascii = Some(strip_quotes(font));
    }
    if el.attr("fo:font-variant") == Some("small-caps") || el.attr("style:font-variant") == Some("small-caps") {
        p.small_caps = Some(true);
    }
    if el.attr("fo:text-transform") == Some("uppercase") {
        p.all_caps = Some(true);
    }
    if let Some(pos) = el.attr("style:text-position").filter(|p| !p.is_empty()) {
        if word_prefix(pos, "super") || pos.bytes().next().is_some_and(|b| (b'1'..=b'9').contains(&b)) {
            p.vert_align = Some(s("superscript"));
        } else if word_prefix(pos, "sub") || pos.starts_with('-') {
            p.vert_align = Some(s("subscript"));
        }
    }
    p
}

/// `readParaPropsStyle(el)`: `style:paragraph-properties` of ONE style (no inheritance).
#[must_use]
pub fn read_para_props_style(el: Option<Element<'_>>) -> ParaProps {
    let mut p = ParaProps::default();
    let Some(el) = el else { return p };
    match el.attr("fo:text-align") {
        Some("start" | "left") => p.alignment = Some(s("left")),
        Some("end" | "right") => p.alignment = Some(s("right")),
        Some("center") => p.alignment = Some(s("center")),
        Some("justify") => p.alignment = Some(s("both")),
        _ => {}
    }
    if let Some(v) = length_to_twips(el.attr("fo:margin-left")) {
        p.indent_left_twips = Some(JsNumber::Num(v));
    }
    if let Some(v) = length_to_twips(el.attr("fo:margin-right")) {
        p.indent_right_twips = Some(JsNumber::Num(v));
    }
    if let Some(v) = length_to_twips(el.attr("fo:text-indent")) {
        p.indent_first_line_twips = Some(JsNumber::Num(v));
    }
    if let Some(v) = length_to_twips(el.attr("fo:margin-top")) {
        p.space_before_twips = Some(JsNumber::Num(v));
    }
    if let Some(v) = length_to_twips(el.attr("fo:margin-bottom")) {
        p.space_after_twips = Some(JsNumber::Num(v));
    }
    if let Some(bg) = el.attr("fo:background-color")
        && bg != "transparent"
    {
        p.shading_hex = Some(norm_hex(bg));
    }
    if el.attr("fo:break-before") == Some("page") {
        p.page_break_before = Some(true);
    }
    p
}

/// `normHex(v)`: `v.replace(/^#/, '').toUpperCase()`.
#[must_use]
pub fn norm_hex(v: &str) -> Str {
    uo_text::case::to_uppercase_str(v.strip_prefix('#').unwrap_or(v)).into()
}

/// `stripQuotes(v)`: `v.replace(/^['"]|['"]$/g, '').split(',')[0].trim()`. The global replace
/// removes a leading quote and a trailing one (a lone quote once).
#[must_use]
pub fn strip_quotes(v: &str) -> Str {
    let b = v.as_bytes();
    let is_quote = |c: Option<&u8>| matches!(c, Some(b'\'' | b'"'));
    let start = usize::from(is_quote(b.first()));
    let end = if b.len() >= 2 && is_quote(b.last()) { b.len() - 1 } else { b.len() };
    let inner = v.get(start..end.max(start)).unwrap_or("");
    let first = inner.split(',').next().unwrap_or("");
    trim(first).into()
}

/// `{ ...base, ...over }` over the keys `over` holds with a value: a key without a value (absent or
/// `undefined`) is not copied, as `mergeRunProps` and the spreads of `resolveStyle` of the TS (the
/// readers here never store an `undefined`).
pub fn overlay<T: JsObject + JsAssign>(base: &mut T, over: &T) {
    for i in 0..T::keys().len() {
        if matches!(over.slot(i), Slot::Val(_)) {
            base.assign_slot(over, i);
        }
    }
}

impl<'d> OdfStyleIndex<'d> {
    /// `resolveStyle(index, name)`: the style by name, applying the `style:parent-style-name` chain
    /// (with protection against cycles) and the default styles of the family. `max_chain` is
    /// `DocxLimits::max_based_on_chain`.
    pub fn resolve_style(&mut self, name: Option<&'d str>, max_chain: u32) -> Result<Arc<ResolvedStyle<'d>>, DocxError> {
        let Some(first) = name.filter(|n| !n.is_empty()) else { return Ok(self.empty.clone()) };
        if let Some(hit) = self.cache.get(first) {
            return Ok(hit.clone());
        }
        let mut chain: Vec<(&'d str, RawStyle<'d>)> = Vec::new();
        let mut seen: HashSet<&'d str> = HashSet::new();
        let mut base = self.empty.clone();
        let mut cyclic = false;
        let mut cur = Some(first);
        while let Some(n) = cur {
            if !chain.is_empty()
                && let Some(hit) = self.cache.get(n)
            {
                base = hit.clone();
                break;
            }
            if !seen.insert(n) {
                cyclic = true;
                break;
            }
            let Some(raw) = self.styles.get(n).copied() else { break };
            chain.push((n, raw));
            if chain.len() as u64 > u64::from(max_chain) {
                return Err(DocxError::Limit { kind: DocxLimitKind::BasedOnChain, max: u64::from(max_chain), part: None });
            }
            cur = raw.parent.filter(|p| !p.is_empty());
        }
        let mut acc = base;
        for (n, raw) in chain.iter().rev() {
            let mut run = RunProps::default();
            let mut para = ParaProps::default();
            if let Some(def) = raw.family.filter(|f| !f.is_empty()).and_then(|f| self.defaults.get(f)) {
                run = read_text_props(def.text);
                para = read_para_props_style(def.para);
            }
            overlay(&mut run, &acc.run);
            overlay(&mut para, &acc.para);
            overlay(&mut run, &read_text_props(raw.text));
            overlay(&mut para, &read_para_props_style(raw.para));
            let list_style_name = raw.list_style_name.or(acc.list_style_name);
            acc = Arc::new(ResolvedStyle { run, para, list_style_name });
            if !cyclic && self.cache.len() < CACHE_CAP {
                self.cache.insert(n, acc.clone());
            }
        }
        Ok(acc)
    }

    /// Drops the cache (the memory gate's `clear_caches`).
    pub fn clear_cache(&mut self) {
        // a new map, not `clear()`: `clear` keeps the bucket table (about 200 KB at the cap)
        self.cache = HashMap::new();
    }
}
