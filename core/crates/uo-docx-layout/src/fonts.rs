//! Faces of a layout call (LA6; TS `fonts.ts`, frontier F1): the provider answers and their
//! identity, the per-call `FaceResolver` cache (K4, LQ19), the per-layout face table, the
//! DirectWrite metrics (`faceMetrics`), the measured superscript/subscript tables, the Word
//! substitution chain (`FaceResolver`), the embedded-font provider (`withEmbeddedFonts`) and the
//! `SfntFont` kerning scale (`kernTwips`/`kernGlyphsTwips`, sfnt.ts:183–199, K7).
//!
//! The provider is a **named policy of the uo-fonts registry** (frontier P, LQ8): `viewer`,
//! `server-substitute`, `oracle` (tests), `editor` (milestone 2), plus the P2 transcript replay of
//! the parity tests. Each reproduces its TS cache keying, because the identity of the answer is
//! observable: kerning, ligatures and run merging apply only across the **same** face object
//! (K2a). This crate sees them through the registry's [`FontProvider`]; a new [`AnswerId`] gets
//! the next [`FaceId`] (first-use order) and a [`FaceRes`] in the layout's resources.
//!
//! Identity sites (LA3): `metricsCache` (`WeakMap<LayoutFont>`, fonts.ts:55) → the metrics stored
//! on the face entry, computed once when the answer enters the table; `FaceResolver.cache`
//! (`Map<string, FontFace>`, new per pass) → [`FaceResolverCache`], per layout call (K4); the
//! `withEmbeddedFonts` cache (`Map<string, FontFace | null>`) → [`EmbeddedFontProvider`]'s
//! answer cache, keyed exactly as the TS.

use std::sync::{Arc, Mutex, PoisonError};

use rustc_hash::FxHashMap;

use crate::error::{LResult, LayoutError, LimitKind};
use crate::math::MathFontInfo;
use crate::model::{EmbeddedFont, FontInfo, JsRecord};
use uo_render::{FaceId, FaceRes, Resources};

/// The provider answers come from the uo-fonts registry (frontier P, LQ8): a `FontAnswer` with
/// its [`AnswerId`] (equal ids = the same TS `FontFace` object), the parsed face pinned while the
/// answer lives, and the PDF's advance table (PQ1).
pub use uo_fonts::registry::{AnswerId, FontAnswer, FontProvider};

/// The document's font table (`fmt.fontTable`, `Record<string, FontTableInfo>`), read with own
/// keys only (the TS `own()` helper).
pub type FontTable = JsRecord<FontInfo>;

/// `FaceMetrics` (fonts.ts:41), computed once per face (`faceMetrics`, the DirectWrite rules).
/// Fractions of the em.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceMetrics {
    pub ascent: f64,
    pub descent: f64,
    pub line_height: f64,
    /// positive = below the baseline
    pub underline_pos: f64,
    pub underline_thick: f64,
    /// the ascent without the line gap (`ascent` = this + gap)
    pub bare_ascent: f64,
    /// `supOffset`: OS/2 `ySuperscriptYOffset` when the font has a positive one
    pub sup_offset: Option<f64>,
}

/// One face of the layout: the answer (it pins the face while the layout lives), its id in
/// `Resources.faces`, its metrics and the equation font info (`math.ts` `infoCache`, filled on
/// first use).
#[derive(Clone, Debug)]
pub struct FaceEntry {
    pub answer: Arc<FontAnswer>,
    pub id: FaceId,
    pub metrics: FaceMetrics,
    /// `fontInfo(face)` of `math.ts` (`infoCache`, a `WeakMap` keyed by the font object): `None`
    /// until an equation asks for it
    pub math: Option<Arc<MathFontInfo>>,
}

/// The faces of one `Layout`, in first-use order (`FaceId` = first use of an answer identity).
#[derive(Debug, Default)]
pub struct FaceTable {
    /// indexed by `FaceId::index()` (the faces this table registered; `None` for a resource
    /// face added by something else)
    entries: Vec<Option<FaceEntry>>,
    by_answer: FxHashMap<AnswerId, FaceId>,
}

impl FaceTable {
    #[must_use]
    pub fn new() -> FaceTable {
        FaceTable::default()
    }

    /// The `FaceId` of an answer: the existing one when this identity is already in the table,
    /// else the next id with a new [`FaceRes`] in `res` and its metrics (K2a: identity, never the
    /// bytes). `Err(Limit(Capacity))` past `u32` faces or symbols.
    pub fn intern(&mut self, answer: &Arc<FontAnswer>, res: &mut Resources) -> LResult<FaceId> {
        if let Some(&id) = self.by_answer.get(&answer.id()) {
            return Ok(id);
        }
        let capacity = LayoutError::Limit(LimitKind::Capacity);
        let key = res.symbols.intern_str(&answer.key).ok_or_else(|| capacity.clone())?;
        let family = res.symbols.intern_str(&answer.family).ok_or_else(|| capacity.clone())?;
        let id = res
            .add_face(FaceRes {
                key,
                family,
                bold: answer.bold,
                italic: answer.italic,
                synth_bold: answer.synth_bold,
                synth_italic: answer.synth_italic,
                stand_in: answer.stand_in,
                face: Some(answer.face().clone()),
                bytes: Some(Arc::clone(answer.bytes())),
                advances: answer.advances.clone(),
            })
            .ok_or(capacity)?;
        let ix = id.index();
        if self.entries.len() <= ix {
            self.entries.resize_with(ix + 1, || None);
        }
        let entry = FaceEntry { answer: Arc::clone(answer), id, metrics: face_metrics(answer.face()), math: None };
        if let Some(slot) = self.entries.get_mut(ix) {
            *slot = Some(entry);
        }
        self.by_answer.insert(answer.id(), id);
        Ok(id)
    }

    /// The entry of a face this table registered.
    #[must_use]
    pub fn entry(&self, id: FaceId) -> Option<&FaceEntry> {
        self.entries.get(id.index()).and_then(Option::as_ref)
    }

    /// The parsed face (`face.font`).
    #[must_use]
    pub fn face(&self, id: FaceId) -> Option<&uo_fonts::Face> {
        self.entry(id).map(|e| e.answer.face())
    }

    /// `faceMetrics(face.font)`.
    #[must_use]
    pub fn metrics(&self, id: FaceId) -> Option<FaceMetrics> {
        self.entry(id).map(|e| e.metrics)
    }

    /// `fontInfo(face)` of `math.ts` (43–57): computed on first use, kept on the entry
    /// (`infoCache`). `None` for a face this table does not hold.
    pub fn math_info(&mut self, id: FaceId) -> Option<Arc<MathFontInfo>> {
        let e = self.entries.get_mut(id.index()).and_then(Option::as_mut)?;
        if e.math.is_none() {
            e.math = Some(Arc::new(MathFontInfo::of_face(e.answer.face(), e.answer.bytes())));
        }
        e.math.clone()
    }

    /// The faces in `FaceId` order.
    pub fn entries(&self) -> impl Iterator<Item = &FaceEntry> + '_ {
        self.entries.iter().filter_map(Option::as_ref)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_answer.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_answer.is_empty()
    }

    /// A copy holding the faces with an id below `faces` (the resource faces of a checkpoint;
    /// the table only grows within a call, LA7). The lazily computed equation info stays.
    #[must_use]
    pub fn prefix(&self, faces: usize) -> FaceTable {
        FaceTable {
            entries: self.entries.iter().take(faces).cloned().collect(),
            by_answer: self.by_answer.iter().filter(|(_, id)| id.index() < faces).map(|(a, id)| (*a, *id)).collect(),
        }
    }

    /// The same faces as `other` (identities and ids; LA7).
    #[must_use]
    pub fn same_as(&self, other: &FaceTable) -> bool {
        self.entries.len() == other.entries.len()
            && self.by_answer.len() == other.by_answer.len()
            && self.entries.iter().zip(&other.entries).all(|(a, b)| match (a, b) {
                (Some(a), Some(b)) => a.id == b.id && a.answer.id() == b.answer.id(),
                (None, None) => true,
                _ => false,
            })
    }
}

/// `FaceResolver.cache` (fonts.ts:225): per **layout call** (K4, LQ19), keyed as the TS
/// (`${fam.toLowerCase()}|${bold?1:0}${italic?1:0}`, the family trimmed), the resolved face after
/// the Word substitution chain. The TS throws when nothing resolves, so a key is never cached
/// without a face.
///
/// `resolved` is an `IndexMap` (insertion order, never observed: only `get`/`insert`) so a
/// checkpoint of the incremental relayout can truncate it to its length at a page start (LA7,
/// D-LA7-4); it was an `FxHashMap`.
#[derive(Debug, Default)]
pub struct FaceResolverCache {
    resolved: indexmap::IndexMap<Box<str>, FaceId, rustc_hash::FxBuildHasher>,
    /// every request the provider answered `None`, first miss first (`Layout::missing_faces`;
    /// added by F8)
    misses: indexmap::IndexSet<crate::output::FaceRequest>,
    /// every provider request and the identity of its answer, in order (`Some` only while an
    /// incremental relayout records: the next relayout asks them again to know the provider
    /// still answers the same faces, LA7)
    pub log: Option<Vec<ProviderAsk>>,
    /// every key asked (cached or not) and its face, first ask first (`Some` only while the
    /// paragraph memo records one paragraph: a hit needs the same keys cached with the same
    /// faces, LA7 level 1)
    pub tap: Option<indexmap::IndexMap<Box<str>, FaceId, rustc_hash::FxBuildHasher>>,
}

/// One provider request of a recorded layout call and its answer's identity (LA7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAsk {
    pub family: Box<str>,
    pub bold: bool,
    pub italic: bool,
    pub answer: Option<AnswerId>,
}

/// What the resolver works over: the provider of the call, the document's font table, the face
/// table and the resources the faces are registered in.
pub struct FaceEnv<'e> {
    pub provider: &'e dyn FontProvider,
    pub font_table: &'e FontTable,
    pub table: &'e mut FaceTable,
    pub res: &'e mut Resources,
}

impl FaceResolverCache {
    #[must_use]
    pub fn new() -> FaceResolverCache {
        FaceResolverCache::default()
    }

    /// `FaceResolver.get(family, bold, italic)` (fonts.ts:235): the face of the family (absent or
    /// blank = Times New Roman) after Word's substitution chain, cached per lower-case key.
    /// `Err(NoFace)` where the TS throws (`nenhuma fonte disponível`).
    pub fn get(&mut self, env: &mut FaceEnv<'_>, family: Option<&str>, bold: bool, italic: bool) -> LResult<FaceId> {
        let trimmed = uo_jsnum::text::trim(family.unwrap_or(""));
        let fam = if trimmed.is_empty() { "Times New Roman" } else { trimmed };
        let key = format!("{}|{}{}", uo_text::case::to_lowercase_str(fam), u8::from(bold), u8::from(italic));
        if let Some(&id) = self.resolved.get(key.as_str()) {
            if let Some(t) = self.tap.as_mut()
                && !t.contains_key(key.as_str())
            {
                t.insert(key.into_boxed_str(), id);
            }
            return Ok(id);
        }
        let answer = lookup(env.provider, env.font_table, fam, bold, italic, &mut self.misses, self.log.as_mut())?;
        let id = env.table.intern(&answer, env.res)?;
        let key = key.into_boxed_str();
        if let Some(t) = self.tap.as_mut() {
            t.insert(key.clone(), id);
        }
        self.resolved.insert(key, id);
        Ok(id)
    }

    /// The face cached for a key of [`FaceResolverCache::get`] (no request, no insertion; LA7
    /// level 1).
    #[must_use]
    pub fn cached(&self, key: &str) -> Option<FaceId> {
        self.resolved.get(key).copied()
    }

    /// The provider misses of the call, first miss first (F8: `Layout::missing_faces`).
    pub fn take_misses(&mut self) -> Vec<crate::output::FaceRequest> {
        std::mem::take(&mut self.misses).into_iter().collect()
    }

    /// The provider misses so far (a copy: a recorded call keeps its cache, LA7).
    #[must_use]
    pub fn misses(&self) -> Vec<crate::output::FaceRequest> {
        self.misses.iter().cloned().collect()
    }

    /// (`resolved`, `misses`) lengths: the state at a checkpoint (LA7).
    #[must_use]
    pub fn lens(&self) -> (usize, usize) {
        (self.resolved.len(), self.misses.len())
    }

    /// A copy holding only the first `resolved` entries and `misses` misses: the cache as it was
    /// when it had those lengths (both only grow within a call). The log is not copied.
    #[must_use]
    pub fn prefix(&self, resolved: usize, misses: usize) -> FaceResolverCache {
        FaceResolverCache {
            resolved: self.resolved.iter().take(resolved).map(|(k, v)| (k.clone(), *v)).collect(),
            misses: self.misses.iter().take(misses).cloned().collect(),
            log: None,
            tap: None,
        }
    }

    /// The entries from `resolved` on and the misses from `misses` on equal `other`'s (the
    /// growth since a checkpoint, LA7).
    #[must_use]
    pub fn same_from(&self, other: &FaceResolverCache, resolved: usize, misses: usize) -> bool {
        self.resolved.len() == other.resolved.len()
            && self.misses.len() == other.misses.len()
            && self.resolved.iter().skip(resolved).zip(other.resolved.iter().skip(resolved)).all(|(a, b)| a == b)
            && self.misses.iter().skip(misses).zip(other.misses.iter().skip(misses)).all(|(a, b)| a == b)
    }

    /// Entries cached (one per lower-case key resolved in this call).
    #[must_use]
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }
}

/// `deviceEmPt` (typography/lineheight.ts): the em of a size at 600 DPI.
#[must_use]
pub fn device_em_pt(size_pt: f64) -> f64 {
    (uo_jsnum::math::round((size_pt * 600.0) / 72.0) * 72.0) / 600.0
}

/// `faceMetrics(font)` (fonts.ts:57): the DirectWrite vertical metrics (typo metrics with
/// `USE_TYPO_METRICS`, else win ascent/descent with the hhea gap above, else hhea), the East
/// Asian 1.3× rule, the post-table underline and the OS/2 superscript offset.
#[must_use]
pub fn face_metrics(font: &uo_fonts::Face) -> FaceMetrics {
    let fm = font.metrics();
    let u = if fm.units_per_em == 0 { 1000.0 } else { f64::from(fm.units_per_em) };
    let hhea_total = f64::from(fm.ascender) - f64::from(fm.descender) + f64::from(fm.line_gap);
    let (mut asc, mut desc, mut gap);
    if fm.fs_selection & 0x80 != 0 {
        asc = f64::from(fm.s_typo_ascender);
        desc = -f64::from(fm.s_typo_descender);
        gap = f64::from(fm.s_typo_line_gap);
    } else if fm.us_win_ascent > 0 || fm.us_win_descent > 0 {
        asc = f64::from(fm.us_win_ascent);
        desc = f64::from(fm.us_win_descent);
        gap = uo_jsnum::math::max(0.0, hhea_total - (asc + desc));
    } else {
        asc = f64::from(fm.ascender);
        desc = -f64::from(fm.descender);
        gap = f64::from(fm.line_gap);
    }
    if (fm.code_page_range1.unwrap_or(0) >> 17) & 0x1f != 0 {
        let extra = uo_jsnum::math::round(0.15 * (asc + desc));
        asc += extra;
        desc += extra;
        gap = 0.0;
    }
    let line_height = (asc + desc + gap) / u;
    let sup = fm.y_superscript_y_offset.filter(|&y| y > 0);
    FaceMetrics {
        ascent: (gap + asc) / u,
        descent: desc / u,
        line_height,
        underline_pos: if fm.underline_position != 0 { -f64::from(fm.underline_position) / u } else { 0.1 },
        underline_thick: if fm.underline_thickness > 0 { f64::from(fm.underline_thickness) / u } else { 0.05 },
        bare_ascent: asc / u,
        sup_offset: sup.map(|y| f64::from(y) / u),
    }
}

/// `VERT_TABLE` (fonts.ts:108): superscript raise and subscript drop measured in Word, in half
/// points, one base-36 digit per size from 6 to 24 pt in half-point steps.
fn vert_row(key: &str) -> Option<[&'static str; 2]> {
    Some(match key {
        "arial" => ["5555666767878999aabbbcccdcdeeefffgggh", "1111221112221222222322233322333344434"],
        "calibri" => ["5544555677888999aabbabbbcccdddeeefffg", "1221122212222322233333334434444544455"],
        "courier new" => ["35445556567667778899999aababbccccdddd", "1221122222222322333233344434445444555"],
        "segoe ui" => ["4556666778989aaabbccbddceeefffggghhhi", "1121112221222232223333333344334445444"],
        "times new roman" => ["4555666777899999aabbbcccddeffefffgggh", "1112211222222222322233323233333333344"],
        "tahoma" => ["4555666777888999aabbbcccdddeeefffgggh", "0111122211222212222223222333323333444"],
        "verdana" => ["4555666777888999aabbbcccdddeeefffgggh", "0111112211222232222233222333223333444"],
        "cambria" => ["333444455556666777788889999aaaabbbbcc", "1111111112221222222222333323333333344"],
        "georgia" => ["334444555666677778888999aaaabbbbccccd", "1111111122221222222222333322333333333"],
        "century gothic" => ["3344445555666677788889999aaaabbbbcccc", "1111111111122212222222222233333333333"],
        _ => return None,
    })
}

/// `vertTable(family, base, k)` (fonts.ts:122).
fn vert_table(family: &str, base: f64, k: usize) -> Option<f64> {
    let f = uo_text::case::to_lowercase_str(family);
    // VERT_ALIAS
    let key = if f == "calibri light" { "calibri" } else { f.as_str() };
    let t = vert_row(key)?.get(k).copied()?;
    let i = (base - 6.0) * 2.0;
    if t.is_empty() || !uo_jsnum::conv::is_integer(i) || i < 0.0 {
        return None;
    }
    // `i >= t.length` (the table strings are ASCII: one unit per byte)
    let ix = uo_jsnum::conv::js_index(i, t.len())?;
    let digit = t.get(ix..=ix)?;
    Some(uo_jsnum::parse::parse_int(digit, 36) / 2.0)
}

/// `superscriptRaiseFor(family, m, base, sup)` (fonts.ts:132): the measured table, else the
/// formula.
#[must_use]
pub fn superscript_raise_for(family: &str, m: &FaceMetrics, base: f64, sup: f64) -> f64 {
    vert_table(family, base, 0).unwrap_or_else(|| superscript_raise_pt(m, base, sup))
}

/// `subscriptDropPt(family, base)` (fonts.ts:137): the measured table, else 8% of the size.
#[must_use]
pub fn subscript_drop_pt(family: &str, base: f64) -> f64 {
    vert_table(family, base, 1).unwrap_or(base * 0.08)
}

/// `superscriptRaisePt(m, base, sup)` (fonts.ts:141).
#[must_use]
pub fn superscript_raise_pt(m: &FaceMetrics, base: f64, sup: f64) -> f64 {
    let half = |v: f64| uo_jsnum::math::floor(v * 2.0 + 0.5) / 2.0;
    let align = half(m.ascent * base - m.bare_ascent * sup);
    match m.sup_offset {
        Some(s) => uo_jsnum::math::min(half(s * base), align),
        None => align,
    }
}

/// `isSymbolFont(font)` (fonts.ts:150): OS/2 code page bit 31 or a glyph for U+F020.
#[must_use]
pub fn is_symbol_font(font: &uo_fonts::Face) -> bool {
    font.is_symbol_font()
}

/// `kernGlyphsTwips(g1, g2, sizePt)` of `SfntFont` (sfnt.ts:188, K7): the Office kerning pair
/// (`uo_fonts::Face::kern_units`: 0 for ids above 0xFFFF or no pair) scaled at the 600-DPI em,
/// in twips.
#[must_use]
pub fn sfnt_kern_glyphs_twips(font: &uo_fonts::Face, g1: u32, g2: u32, size_pt: f64) -> f64 {
    let units = font.kern_units(g1, g2);
    if units == 0 {
        return 0.0;
    }
    let device_em_pt = (uo_jsnum::math::round((size_pt * 600.0) / 72.0) * 72.0) / 600.0;
    (f64::from(units) * device_em_pt * 20.0) / f64::from(font.metrics().units_per_em)
}

/// `kernTwips(prevCodePoint, codePoint, sizePt)` of `SfntFont` (sfnt.ts:183, K7).
#[must_use]
pub fn sfnt_kern_twips(font: &uo_fonts::Face, prev: u32, cp: u32, size_pt: f64) -> f64 {
    sfnt_kern_glyphs_twips(font, font.glyph_id(prev), font.glyph_id(cp), size_pt)
}

// ───────────────────────── font substitution ─────────────────────────

/// `ALIASES` (fonts.ts:161): what Windows/Word substitute (FontSubstitutes and PostScript names),
/// looked up by the lower-case name (own keys only).
fn alias_of(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "helvetica" | "helvetica neue" | "arialmt" | "arial mt" | "arial unicode ms" => "Arial",
        "ms sans serif" | "ms shell dlg" => "Microsoft Sans Serif",
        "tms rmn" | "times" | "times roman" | "timesnewromanpsmt" | "ms serif" => "Times New Roman",
        "courier" | "courier newpsmt" | "couriernewpsmt" => "Courier New",
        "ms shell dlg 2" => "Tahoma",
        "symbolmt" => "Symbol",
        "wingdings-regular" | "zapfdingbats" => "Wingdings",
        _ => return None,
    })
}

/// `CJK_DEFAULT` (fonts.ts:215): what Word puts in place of an absent family with an East Asian
/// charset in the font table, `[sans serif, serif]`.
fn cjk_default(charset_upper: &str) -> Option<[&'static str; 2]> {
    Some(match charset_upper {
        "80" => ["Yu Gothic", "Yu Gothic"],
        "81" => ["Dotum", "Batang"],
        "86" => ["Microsoft YaHei", "SimSun"],
        "88" => ["Microsoft JhengHei", "PMingLiU"],
        _ => return None,
    })
}

/// The result of `parsePostScriptName` (fonts.ts:170). `bold`/`italic` are `true | undefined`
/// in the TS (here `false` = undefined).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostScriptName {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

/// The suffixes of the `parsePostScriptName` pattern (the alternation, anchored by `$`).
const PS_STYLES: [&str; 13] = [
    "Bold",
    "BoldMT",
    "Italic",
    "ItalicMT",
    "BoldItalic",
    "BoldItalicMT",
    "Regular",
    "MT",
    "Roman",
    "BoldOblique",
    "Oblique",
    "Light",
    "Semibold",
];

/// `parsePostScriptName(name)` (fonts.ts:170): "Arial-BoldMT", "Calibri-Italic" → family and
/// variant. The pattern `^([A-Za-z][A-Za-z0-9 ]*?)[-,](Bold|…|Semibold)$` as a hand matcher: the
/// first group cannot hold `-` or `,`, so the separator is the first of them and the rest must be
/// one of the suffixes exactly (JS `$` without `m` is the end of the input only).
#[must_use]
pub fn parse_post_script_name(name: &str) -> Option<PostScriptName> {
    let b = name.as_bytes();
    let sep = b.iter().position(|&c| c == b'-' || c == b',')?;
    let (head, rest) = (b.get(..sep)?, b.get(sep + 1..)?);
    let (first, tail) = head.split_first()?;
    if !first.is_ascii_alphabetic() || !tail.iter().all(|&c| c.is_ascii_alphanumeric() || c == b' ') {
        return None;
    }
    let style = PS_STYLES.iter().find(|s| s.as_bytes() == rest)?;
    // head is ASCII here
    let mut fam = std::str::from_utf8(head).ok()?;
    fam = fam.strip_suffix("MT").unwrap_or(fam);
    fam = fam.strip_suffix("PS").unwrap_or(fam);
    // `.replace(/([a-z])([A-Z])/g, '$1 $2')`
    let fb = fam.as_bytes();
    let mut family = String::with_capacity(fb.len() + 4);
    let mut i = 0;
    while let Some(&c) = fb.get(i) {
        family.push(char::from(c));
        if c.is_ascii_lowercase()
            && let Some(&n) = fb.get(i + 1)
            && n.is_ascii_uppercase()
        {
            family.push(' ');
            family.push(char::from(n));
            i += 2;
            continue;
        }
        i += 1;
    }
    let st = style.to_ascii_lowercase();
    Some(PostScriptName { family, bold: st.contains("bold"), italic: st.contains("italic") || st.contains("oblique") })
}

/// `genericFor(info)` (fonts.ts:193): Word's default family for a generic font-table family.
fn generic_for(info: Option<&FontInfo>) -> &'static str {
    match info.and_then(|i| i.family.as_deref()) {
        Some("roman" | "script" | "decorative") => "Times New Roman",
        Some("modern") => "Courier New",
        Some("swiss") => "Arial",
        _ => {
            // PANOSE: byte 0 = family (2 = Latin text), byte 1 = serif style (11-13 = sans)
            if let Some(p) = info.and_then(|i| i.panose1.as_deref())
                && !p.is_empty()
                && uo_jsnum::text::utf16_len(p) >= 4
            {
                let serif = uo_jsnum::parse::parse_int(&uo_jsnum::text::utf16_slice(p, 2, 4), 16);
                if (11.0..=13.0).contains(&serif) {
                    return "Arial";
                }
                if serif > 0.0 && serif < 11.0 {
                    return "Times New Roman";
                }
            }
            "Times New Roman"
        }
    }
}

/// The default fallbacks of `FaceResolver` (fonts.ts:229).
const FALLBACKS: [&str; 4] = ["Times New Roman", "Arial", "Liberation Serif", "Liberation Sans"];

/// One request of the substitution chain.
struct Try {
    family: String,
    bold: bool,
    italic: bool,
}

/// `altName.split(/[,;]/)`, each trimmed (JS `trim`).
fn alt_names(alt: &str) -> impl Iterator<Item = &str> + '_ {
    alt.split([',', ';']).map(uo_jsnum::text::trim)
}

/// `FaceResolver.lookup(fam, bold, italic)` (fonts.ts:246): the requests in Word's order, the
/// first the provider answers; a host stand-in for the asked family gives way to what Word maps
/// that name to (East Asian script font, FontSubstitutes, PostScript name) when the provider has
/// one that is not a stand-in. `Err(NoFace)` where the TS throws.
fn lookup(
    provider: &dyn FontProvider,
    font_table: &FontTable,
    fam: &str,
    bold: bool,
    italic: bool,
    misses: &mut indexmap::IndexSet<crate::output::FaceRequest>,
    mut log: Option<&mut Vec<ProviderAsk>>,
) -> LResult<Arc<FontAnswer>> {
    // every request the provider has nothing for is noted (F8)
    let mut ask = |family: &str, b: bool, i: bool| -> Option<Arc<FontAnswer>> {
        let a = provider.face(family, b, i);
        if a.is_none() && misses.len() < MAX_MISSES {
            misses.insert(crate::output::FaceRequest { family: family.into(), bold: b, italic: i });
        }
        if let Some(log) = log.as_deref_mut() {
            log.push(ProviderAsk { family: family.into(), bold: b, italic: i, answer: a.as_ref().map(|x| x.id()) });
        }
        a
    };
    let lower = uo_text::case::to_lowercase_str(fam);
    let mut tries: Vec<Try> = vec![Try { family: fam.to_owned(), bold, italic }];
    let alias = alias_of(&lower);
    let info0 = font_table.get(fam);
    // East Asian charset in the font table: the script's default font (sans × serif), before
    // Word's own mapping; the altName only for a family Word does not know
    let charset_upper = info0.and_then(|i| i.charset.as_deref()).map(uo_text::case::to_uppercase_str).unwrap_or_default();
    let serif = usize::from(info0.and_then(|i| i.family.as_deref()) == Some("roman"));
    if let Some(cjk) = cjk_default(&charset_upper).and_then(|row| row.get(serif).copied()) {
        if alias.is_none()
            && let Some(alt) = info0.and_then(|i| i.alt_name.as_deref())
            && !alt.is_empty()
        {
            for a in alt_names(alt) {
                tries.push(Try { family: a.to_owned(), bold, italic });
            }
        }
        tries.push(Try { family: cjk.to_owned(), bold, italic });
    }
    if let Some(a) = alias {
        tries.push(Try { family: a.to_owned(), bold, italic });
    }
    if let Some(ps) = parse_post_script_name(fam) {
        let a2 = alias_of(&uo_text::case::to_lowercase_str(&ps.family)).map_or(ps.family.clone(), str::to_owned);
        tries.push(Try { family: a2, bold: bold || ps.bold, italic: italic || ps.italic });
    }
    // up to here: what Word puts in place of THIS name
    let mapped_end = tries.len();
    let info = info0;
    if let Some(alt) = info.and_then(|i| i.alt_name.as_deref())
        && !alt.is_empty()
    {
        for a in alt_names(alt) {
            tries.push(Try { family: a.to_owned(), bold, italic });
        }
    }
    let symbolic = info.and_then(|i| i.charset.as_deref()) == Some("02") || info.and_then(|i| i.family.as_deref()) == Some("decorative");
    // absent family without altName: serif (roman) or outside the font table → Cambria;
    // swiss/auto/modern → Calibri; an Arabic signature → Arial or Sakkal Majalla
    if !symbolic {
        let usb0 = info.and_then(|i| i.sig.as_ref()).and_then(|s| s.get("usb0")).filter(|v| !v.is_empty());
        let arabic = usb0.is_some_and(|v| uo_jsnum::conv::to_int32(uo_jsnum::parse::parse_int(v, 16)) & (1 << 13) != 0);
        let roman = info.and_then(|i| i.family.as_deref()) == Some("roman");
        if arabic {
            tries.push(Try { family: (if roman { "Sakkal Majalla" } else { "Arial" }).to_owned(), bold, italic });
        } else {
            tries.push(Try { family: (if info.is_none() || roman { "Cambria" } else { "Calibri" }).to_owned(), bold, italic });
        }
    }
    tries.push(Try { family: generic_for(info).to_owned(), bold, italic });
    for fb in FALLBACKS {
        tries.push(Try { family: fb.to_owned(), bold, italic });
    }
    for (i, t) in tries.iter().enumerate() {
        if t.family.is_empty() {
            continue;
        }
        let Some(face) = ask(&t.family, t.bold, t.italic) else { continue };
        // the asked family came as the host's stand-in (not installed): what Word puts in its
        // place comes first; without any of them, the metric-compatible stand-in
        if i == 0 && face.stand_in {
            for c in tries.get(1..mapped_end).unwrap_or(&[]) {
                let f = if c.family.is_empty() { None } else { ask(&c.family, c.bold, c.italic) };
                if let Some(f) = f
                    && !f.stand_in
                {
                    return Ok(f);
                }
            }
        }
        return Ok(face);
    }
    Err(LayoutError::NoFace)
}

// ───────────────────────── embedded fonts ─────────────────────────

/// Cap of the provider misses one call notes (M5; F8).
pub const MAX_MISSES: usize = 4_096;

/// Cap of the embedded provider's answer cache (M5): past it a request is answered without being
/// remembered (a new answer identity each time, as the registry's viewer cap, R7 there).
pub const MAX_EMBEDDED_ANSWERS: usize = 16_384;

/// The embedded faces of one family: variant as written in the document → face and its document
/// budget ticket (the TS `Map<string, { font, bytes }>`, insertion order).
type EmbeddedFamily = Vec<(Box<str>, uo_fonts::registry::CachedFace, Option<uo_fonts::registry::Ticket>)>;

/// `withEmbeddedFonts(provider, embedded, makeFont)` (fonts.ts:315) as a registry-side wrapper:
/// the document's embedded faces first (Word uses them when the family is not installed, and even
/// when it is), then the host provider; a variant the document does not embed while the family
/// exists on the host → the host's face.
pub struct EmbeddedFontProvider {
    inner: Arc<dyn FontProvider>,
    /// lower-case family → variant (as written in the document) → face; TS `Map` order
    by_fam: FxHashMap<String, EmbeddedFamily>,
    cache: Mutex<FxHashMap<String, Option<Arc<FontAnswer>>>>,
    /// the document ledger the embedded faces were charged to (its refusals: `"font budget"`)
    ledger: Option<Arc<uo_fonts::registry::FaceLedger>>,
}

impl EmbeddedFontProvider {
    /// `withEmbeddedFonts`: `inner` itself when the document embeds nothing. Each face is parsed
    /// through the process face cache (`makeFont`); one that does not parse is ignored (the TS
    /// `try/catch`). With a document `ledger`, a face the budget refuses is ignored as well (LA6:
    /// past the budget a face resolves as a miss; the layout warns `"font budget"`).
    pub fn wrap(
        inner: Arc<dyn FontProvider>,
        embedded: &[EmbeddedFont],
        cache: &uo_fonts::registry::FaceCache,
        ledger: Option<&Arc<uo_fonts::registry::FaceLedger>>,
    ) -> Arc<dyn FontProvider> {
        if embedded.is_empty() {
            return inner;
        }
        Arc::new(EmbeddedFontProvider::new(inner, embedded, cache, ledger))
    }

    /// The wrapper itself (also over an empty list), for the tests and a binding that keeps it.
    #[must_use]
    pub fn new(
        inner: Arc<dyn FontProvider>,
        embedded: &[EmbeddedFont],
        cache: &uo_fonts::registry::FaceCache,
        ledger: Option<&Arc<uo_fonts::registry::FaceLedger>>,
    ) -> EmbeddedFontProvider {
        let mut by_fam: FxHashMap<String, EmbeddedFamily> = FxHashMap::default();
        for e in embedded {
            let Ok(face) = cache.get_or_parse(Arc::clone(&e.data)) else { continue };
            let ticket = match ledger {
                Some(l) => match l.charge(face.content_key(), face.cost()) {
                    Some(t) => Some(t),
                    None => continue,
                },
                None => None,
            };
            let m = by_fam.entry(uo_text::case::to_lowercase_str(&e.family)).or_default();
            // `m.set(e.variant, …)`: a repeated variant replaces the value in place
            match m.iter_mut().find(|(v, _, _)| *v == e.variant) {
                Some(slot) => *slot = (e.variant.clone(), face, ticket),
                None => m.push((e.variant.clone(), face, ticket)),
            }
        }
        EmbeddedFontProvider { inner, by_fam, cache: Mutex::new(FxHashMap::default()), ledger: ledger.cloned() }
    }

    /// Requests remembered (M5: at most [`MAX_EMBEDDED_ANSWERS`]).
    #[must_use]
    pub fn cached(&self) -> usize {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    fn answer(&self, family: &str, bold: bool, italic: bool, fam: &EmbeddedFamily) -> Option<Arc<FontAnswer>> {
        let want = match (bold, italic) {
            (true, true) => "boldItalic",
            (true, false) => "bold",
            (false, true) => "italic",
            (false, false) => "regular",
        };
        let order = [
            want,
            if bold { "bold" } else { "regular" },
            if italic { "italic" } else { "regular" },
            "regular",
            "bold",
            "italic",
            "boldItalic",
        ];
        let mut out: Option<Arc<FontAnswer>> = None;
        for v in order {
            let Some((_, face, ticket)) = fam.iter().find(|(name, _, _)| &**name == v) else { continue };
            let fb = v == "bold" || v == "boldItalic";
            let fi = v == "italic" || v == "boldItalic";
            out = Some(FontAnswer::new(uo_fonts::registry::AnswerParts {
                key: format!("embed:{family}|{v}"),
                family: family.to_owned(),
                face: face.clone(),
                bold,
                italic,
                synth_bold: bold && !fb,
                synth_italic: italic && !fi,
                stand_in: false,
                advances: None,
                ticket: ticket.clone(),
            }));
            break;
        }
        // the variant asked for is not embedded but the family exists on the host: prefer the host
        if let Some(o) = &out
            && (o.synth_bold || o.synth_italic)
            && let Some(host) = self.inner.face(family, bold, italic)
        {
            out = Some(host);
        }
        out
    }
}

impl FontProvider for EmbeddedFontProvider {
    fn budget_refusals(&self) -> u64 {
        self.inner.budget_refusals().max(self.ledger.as_ref().map_or(0, |l| l.refused()))
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let lower = uo_text::case::to_lowercase_str(family);
        let Some(fam) = self.by_fam.get(&lower) else { return self.inner.face(family, bold, italic) };
        let key = format!("{lower}|{bold}|{italic}");
        if let Some(hit) = self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return hit.clone();
        }
        // asked outside the lock: the host provider may take its own locks
        let out = self.answer(family, bold, italic, fam);
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(hit) = cache.get(&key) {
            // another thread answered first: its answer is the one the cache keeps
            return hit.clone();
        }
        if cache.len() < MAX_EMBEDDED_ANSWERS {
            cache.insert(key, out.clone());
        }
        out
    }
}

// ───────────────────────── the engine context ─────────────────────────

impl crate::engine::Cx<'_, '_> {
    /// `ctx.faces.get(family, bold, italic)`: the resolver of this layout call (K4) over the
    /// call's provider and the document's font table.
    pub fn face(&mut self, family: Option<&str>, bold: bool, italic: bool) -> LResult<FaceId> {
        let mut env = FaceEnv {
            provider: self.provider,
            font_table: &self.ldoc.view.formatting().font_table,
            table: &mut *self.fonts,
            res: &mut *self.res,
        };
        self.resolver.get(&mut env, family, bold, italic)
    }
}
