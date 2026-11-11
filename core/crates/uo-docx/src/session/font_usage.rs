//! `fontUsage()` of the editor (universal-write.ts 2317–2335, LQ15) and the save-time glue of the
//! fonts on save (`saveDocxWithFonts`, 2297; the `fontEmbedding` settings, 2162/2241/2283), in two
//! phases with no callbacks: [`begin_save_with_fonts`] says which faces the plan wants
//! ([`uo_fonts::embedding_requests`]), the shell gathers their bytes, [`finish_save_with_fonts`]
//! plans ([`uo_fonts::plan_font_embedding`]), writes the fonts into the package and saves.
//!
//! `fontUsage` is the model half of the TS (its layout is not read): per paragraph the spans of
//! `buildSpans` (10859) reduced to what reaches the output — the family, the variant and the text.
//! Declared reductions (record uo-bindings.md "Milestone 2: the session in wasm", fontUsage):
//! - the editor's built-in `STYLES` table (533), the fallback of `styleOf` for a style the document
//!   does not define, has no `fontAscii`, `italic`, `allCaps` or `smallCaps` and `bold: false`, so it
//!   cannot change the family, the variant or the text: it is left out;
//! - a custom inline's `renderer.measure` (an image width) never reaches the usage: not called;
//! - the size, the letter spacing and the image sizes are not computed.
//!
//! The order is the TS order: the paragraphs of `this.model` (the edited story: the body's
//! top-level paragraphs; a cell's top-level paragraphs; a note's, a header/footer part's and a
//! textbox's paragraphs flattened with their cells — the TS swaps `this.model` to those arrays in
//! `enterCell` 18753, `enterNote` 18811, `enterHeaderFooter` 2668, `enterTextbox` 2750), each
//! followed by the paragraphs of its textbox runs (recursively); then the tables of
//! `this.tableAnchors` (the body's anchors in the body, cell and note contexts; none in a
//! header/footer or textbox, which set it to `[]`) row by row, cell by cell, each cell's
//! paragraphs then its nested tables; then every section's header and footer slots `default`,
//! `first`, `even` (the part's paragraphs flattened with their cells, as `readHeaderFooterPart`
//! keeps them).

use std::sync::Arc;

use uo_fonts::embed::{EmbeddingPlan, FaceRequest, FontEmbeddingSettings, FontUsage, SkipReason, WORD_DEFAULT_EMBEDDING};
use uo_fonts::{FontLimits, Variant};
use uo_opc::Package;
use uo_text::case::{to_lowercase, to_uppercase};

use super::ops::context::EditContext;
use super::{ChangeSet, Event, Session};
use crate::DocxError;
use crate::js::Str;
use crate::model::run::{RunProps, TextboxView};
use crate::model::{Paragraph, Story, Table};
use crate::text::Utf16;
use crate::write::fonts::{read_docx_embedding, word_embedding_policy, write_docx_embedding};

/// One entry of `fontUsage()`: `{ family, variants: [variant], text }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontUse {
    pub family: Str,
    pub variant: Variant,
    pub text: Utf16,
}

/// The style fields `buildSpans` reads through `styleOf` (the document's resolved style).
#[derive(Clone, Copy, Default)]
struct StyleLook<'a> {
    bold: bool,
    italic: bool,
    font_ascii: Option<&'a str>,
    all_caps: bool,
    small_caps: bool,
}

const OBJECT: u16 = 0xFFFC;

fn truthy(b: Option<bool>) -> bool {
    b == Some(true)
}

/// `isLower(c)`: `c.toLowerCase() === c && c.toUpperCase() !== c`, per UTF-16 code unit.
fn is_lower(c: u16) -> bool {
    let u = [c];
    *to_lowercase(&u) == u[..] && *to_uppercase(&u) != u[..]
}

impl Session {
    /// `fontUsage()` (see the module comment): the families, variants and texts of the document in
    /// the TS order, entries with an empty family or text left out.
    #[must_use]
    pub fn font_usage(&self) -> Vec<FontUse> {
        let doc = self.document();
        let dd = doc.styles.get("docDefaults").map(|s| &s.run);
        let mut out = Vec::new();
        let mut w = Walker { doc_default_font: dd.and_then(|r| r.font_ascii.as_deref()), styles: &doc.styles, out: &mut out };
        // this.model.paragraphs
        if let Some(story) = self.story() {
            match self.context() {
                EditContext::Body | EditContext::Cell { .. } => {
                    for p in story.paras.iter() {
                        w.para(p);
                    }
                }
                EditContext::Note { .. } | EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } => {
                    for p in story.flat_paragraphs() {
                        w.para(p);
                    }
                }
            }
        }
        // this.tableAnchors
        match self.context() {
            EditContext::Body | EditContext::Cell { .. } | EditContext::Note { .. } => {
                for a in doc.body.tables.iter() {
                    w.table(&a.table);
                }
            }
            EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } => {}
        }
        // this.fullDoc.sections: headers then footers, slots default, first, even
        for sec in doc.sections.iter() {
            for set in [&sec.headers, &sec.footers] {
                for slot in ["default", "first", "even"] {
                    if let Some(part) = set.get(slot).and_then(|id| doc.hf.get(id.index())) {
                        for p in part.story.flat_paragraphs() {
                            w.para(p);
                        }
                    }
                }
            }
        }
        out
    }
}

struct Walker<'a, 'o> {
    doc_default_font: Option<&'a str>,
    styles: &'a crate::model::formatting::StylesMap,
    out: &'o mut Vec<FontUse>,
}

impl<'a> Walker<'a, '_> {
    /// `styleOf(p)` reduced to the fields `buildSpans` reads for the usage.
    fn style_of(&self, p: &Paragraph) -> StyleLook<'a> {
        let Some(id) = p.props.style_id.as_deref() else { return StyleLook::default() };
        match self.styles.get(id) {
            Some(s) => StyleLook {
                bold: truthy(s.run.bold),
                italic: truthy(s.run.italic),
                font_ascii: s.run.font_ascii.as_deref(),
                all_caps: truthy(s.run.all_caps),
                small_caps: truthy(s.run.small_caps),
            },
            // the built-in STYLES fallback: nothing that reaches the usage (module comment)
            None => StyleLook::default(),
        }
    }

    fn push(&mut self, family: Option<&str>, variant: Variant, text: Vec<u16>) {
        // `if (s.family && s.text)`
        if let Some(f) = family
            && !f.is_empty()
            && !text.is_empty()
        {
            self.out.push(FontUse { family: f.into(), variant, text: Utf16::from_units(text) });
        }
    }

    /// `paras([p])`: the spans of `buildSpans(p)`, then the textboxes of its runs.
    fn para(&mut self, p: &Paragraph) {
        let st = self.style_of(p);
        for r in &p.runs {
            let props: &RunProps = &r.props;
            let bold = truthy(props.bold) || st.bold;
            let italic = truthy(props.italic) || st.italic;
            let variant = match (bold, italic) {
                (true, true) => Variant::BoldItalic,
                (true, false) => Variant::Bold,
                (false, true) => Variant::Italic,
                (false, false) => Variant::Regular,
            };
            // `r.props.fontAscii ?? st?.fontAscii ?? dd.run.fontAscii`
            let family = props.font_ascii.as_deref().or(st.font_ascii).or(self.doc_default_font);
            let ext = r.ext();
            if ext.is_some_and(|e| e.textbox.is_some()) {
                // `{ text: '' }`
                continue;
            }
            if ext.is_some_and(|e| e.image.is_some()) {
                // `r.text || '￼'`
                let text = if r.text.units().is_empty() { vec![OBJECT] } else { r.text.units().to_vec() };
                self.push(family, variant, text);
                continue;
            }
            if ext.is_some_and(|e| e.custom.is_some()) {
                self.push(family, variant, vec![OBJECT]);
                continue;
            }
            let all_caps = truthy(props.all_caps) || st.all_caps;
            let small_caps = truthy(props.small_caps) || st.small_caps;
            if small_caps && !all_caps {
                self.small_caps_spans(r.text.units(), family, variant);
                continue;
            }
            let text = if all_caps || small_caps { to_uppercase(r.text.units()).into_owned() } else { r.text.units().to_vec() };
            self.push(family, variant, text);
        }
        for r in &p.runs {
            if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                for q in textbox_paras(&tb.content, tb.view) {
                    self.para(q);
                }
            }
        }
    }

    /// `smallCapsSpans`: runs of lower / not lower code units; a lower run upper-cased unless that
    /// changes its length.
    fn small_caps_spans(&mut self, text: &[u16], family: Option<&str>, variant: Variant) {
        let mut i = 0;
        while i < text.len() {
            let lower = text.get(i).is_some_and(|&c| is_lower(c));
            let mut j = i;
            while j < text.len() && text.get(j).is_some_and(|&c| is_lower(c)) == lower {
                j += 1;
            }
            let seg = text.get(i..j).unwrap_or(&[]);
            let t = if lower {
                let up = to_uppercase(seg);
                if up.len() == seg.len() { up.into_owned() } else { seg.to_vec() }
            } else {
                seg.to_vec()
            };
            self.push(family, variant, t);
            i = j;
        }
    }

    /// `table(t)`: every row, every cell's paragraphs then its nested tables.
    fn table(&mut self, t: &Table) {
        for row in &t.rows {
            for c in &row.cells {
                for p in c.content.paras.iter() {
                    self.para(p);
                }
                for a in c.content.tables.iter() {
                    self.table(&a.table);
                }
            }
        }
    }
}

/// A textbox's TS `paras` view ([`TextboxView`]).
fn textbox_paras(content: &Story, view: TextboxView) -> Vec<&Paragraph> {
    match view {
        TextboxView::Synced => content.flat_paragraphs(),
        // one fresh empty paragraph: no span, no run
        TextboxView::Phantom => Vec::new(),
        TextboxView::ParasOnly | TextboxView::TopLevel => content.paras.iter().collect(),
    }
}

// ───────────────────────────── the fonts on save ─────────────────────────────

/// The session's `fontEmbedding` at open (TS 2162: `{ ...DEFAULT_EMBEDDING.write, ...readDocxEmbedding(pkg).settings }`).
/// A package whose settings do not read gives the defaults.
#[must_use]
pub fn initial_embedding_settings(pkg: &Package) -> FontEmbeddingSettings {
    match read_docx_embedding(pkg) {
        Ok(e) => e.settings.over(WORD_DEFAULT_EMBEDDING),
        Err(_) => WORD_DEFAULT_EMBEDDING,
    }
}

/// A change of `fontEmbedding` (`{ ...this.fontEmbedding, ...patch }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmbeddingPatch {
    pub embed: Option<bool>,
    pub subset: Option<bool>,
    pub skip_common_fonts: Option<bool>,
}

/// `set fontEmbedding(v)` (TS 2283–2288): nothing when nothing changes; otherwise the new settings,
/// the document dirty (`markDirty`, no undo step) and the change set of that.
pub fn set_font_embedding(s: &mut Session, cur: &mut FontEmbeddingSettings, patch: EmbeddingPatch) -> Option<ChangeSet> {
    let next = FontEmbeddingSettings {
        embed: patch.embed.unwrap_or(cur.embed),
        subset: patch.subset.unwrap_or(cur.subset),
        skip_common_fonts: patch.skip_common_fonts.unwrap_or(cur.skip_common_fonts),
    };
    if next == *cur {
        return None;
    }
    *cur = next;
    s.changed();
    Some(s.finish())
}

/// Phase 1 of `saveDocxWithFonts`: the saved package and what the plan needs.
#[derive(Debug)]
pub struct FontSave {
    /// the saved package (`DocxPackage.load(this.saveDocx())`)
    pub bytes: Vec<u8>,
    /// the families the package embeds already (lower-cased)
    pub already: uo_fonts::FxHashSet<String>,
    /// `fontUsage()` at the save, its texts in UTF-8 (a lone surrogate becomes U+FFFD)
    pub usage: Vec<(String, Variant, String)>,
    pub settings: FontEmbeddingSettings,
    /// the faces to fetch, in the TS order (empty when `embed` is off)
    pub requests: Vec<FaceRequest>,
}

impl FontSave {
    fn usage(&self) -> Vec<FontUsage<'_>> {
        self.usage.iter().map(|(f, v, t)| FontUsage { family: f, variants: std::slice::from_ref(v), text: t }).collect()
    }
}

/// Phase 1: save, read what the package embeds, and the faces the plan will ask for.
///
/// # Errors
/// The save's or the package's [`DocxError`].
pub fn begin_save_with_fonts(s: &mut Session, settings: FontEmbeddingSettings) -> Result<FontSave, DocxError> {
    let bytes = s.save_docx()?;
    let usage: Vec<(String, Variant, String)> =
        s.font_usage().into_iter().map(|u| (u.family.to_string(), u.variant, String::from_utf16_lossy(u.text.units()))).collect();
    let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).map_err(DocxError::Package)?;
    let already = read_docx_embedding(&pkg)?.embedded_set();
    let mut save = FontSave { bytes, already, usage, settings, requests: Vec::new() };
    if settings.embed {
        let policy = word_embedding_policy(&settings);
        save.requests = uo_fonts::embed::embedding_requests(&save.usage(), &policy, &save.already);
    }
    Ok(save)
}

/// Phase 2: the plan over the faces the caller has (`faces`: the document's own face first, then the
/// host's, as the TS `documentFace ?? resolver`), the fonts written into the package, the bytes and
/// `uw-font-embedding` (`embedded`: the families of the plan's faces, once each, in plan order;
/// `skipped`: the skipped families except the already-embedded ones — the reasons are dropped by
/// [`Event::FontEmbedding`]'s shape, declared).
///
/// # Errors
/// A malformed font (the plan fails as the TS throw), or the package's [`DocxError`].
pub fn finish_save_with_fonts(
    f: &FontSave,
    faces: &dyn Fn(&FaceRequest) -> Option<Arc<[u8]>>,
    guid: &mut dyn FnMut() -> [u8; 16],
    limits: &FontLimits,
) -> Result<(Vec<u8>, Event), DocxError> {
    let mut pkg =
        Package::open(&f.bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).map_err(DocxError::Package)?;
    let plan = if f.settings.embed {
        let policy = word_embedding_policy(&f.settings);
        uo_fonts::embed::plan_font_embedding(&f.usage(), &policy, &f.already, faces, limits)
            .map_err(|_| DocxError::Write("font embedding: a face does not read"))?
    } else {
        EmbeddingPlan::default()
    };
    write_docx_embedding(&mut pkg, &plan, &f.settings, guid)?;
    let mut embedded: Vec<Str> = Vec::new();
    for face in &plan.faces {
        if !embedded.iter().any(|e| **e == *face.family) {
            embedded.push(face.family.as_str().into());
        }
    }
    let skipped: Vec<Str> =
        plan.skipped.iter().filter(|(_, r)| *r != SkipReason::AlreadyEmbedded).map(|(fam, _)| Str::from(fam.as_str())).collect();
    let bytes = pkg.save(&uo_opc::package::SaveOptions::default())?;
    Ok((bytes, Event::FontEmbedding { embedded, skipped }))
}
