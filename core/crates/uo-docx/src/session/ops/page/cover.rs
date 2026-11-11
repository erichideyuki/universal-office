//! D2b-F4: the cover pages: `insertCoverPage`, `removeCoverPage`, `coverPages()`, `hasCoverPage()` and
//! the model half of `paintCoverTemplate` (editor `universal-write.ts` 11957-12089; the templates are
//! [`cover_templates`](super::cover_templates)). The cover is a block `w:sdt` "Cover Pages" at the
//! start of the body, read as a fragment of `document.xml` with the document's sources (so that it
//! comes out raw at the save), whose last paragraph carries the page break.

use std::collections::HashSet;
use std::sync::Arc;

use super::super::super::command::{CommandError, Outcome};
use super::super::super::state::Selection;
use super::super::super::text::{DocPosition, para_text};
use super::super::super::{ExecCx, Session};
use super::PageLang;
use super::cover_templates::{COVER_NS, COVER_TEMPLATES, CoverEnv, CoverFields, CoverPage, cover_placeholders, cover_xml};
use super::rx;
use crate::model::Document;
use crate::model::para::Paragraph;
use crate::model::section::SectPr;
use crate::model::sources::StoryGroup;
use crate::model::story::Story;
use crate::word::obj;

type Res = Result<Outcome, CommandError>;

impl Session {
    fn cv_nothing(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    /// `coverPages()`: the templates of the gallery (id, name in the interface language).
    #[must_use]
    pub fn cover_pages(&self, lang: PageLang) -> Vec<(&'static str, &'static str)> {
        COVER_TEMPLATES.iter().map(|t| (t.id, t.name_for(lang))).collect()
    }

    /// `hasCoverPage()`.
    #[must_use]
    pub fn has_cover_page(&self) -> bool {
        !self.cover_para_indexes().is_empty()
    }

    /// `coverParaIndexes()`: the indices (in the body) of the paragraphs of the "Cover Pages" `w:sdt`.
    /// The groups are walked with an explicit stack (a hostile nesting must not overflow the stack).
    pub fn cover_para_indexes(&self) -> Vec<usize> {
        let Some(store) = self.doc.sources.as_deref() else { return Vec::new() };
        let Some(story) = store.stories.get("word/document.xml") else { return Vec::new() };
        let mut ids: HashSet<u32> = HashSet::new();
        let mut stack: Vec<(&[StoryGroup], bool)> = vec![(&story.groups, false)];
        while let Some((groups, in_cover)) = stack.pop() {
            for g in groups {
                let cover = in_cover || g.wrap_open.as_ref().is_some_and(|w| rx::has_cover_gallery(w.resolve(&store.parts)));
                if cover {
                    for b in &g.blocks {
                        if let Some(x) = b.as_f64()
                            && x >= 0.0
                            && x.fract() == 0.0
                            && x <= f64::from(u32::MAX)
                        {
                            ids.insert(uo_jsnum::conv::to_uint32(x));
                        }
                    }
                }
                if let Some(inner) = &g.inner {
                    stack.push((inner, cover));
                }
            }
        }
        if ids.is_empty() {
            return Vec::new();
        }
        self.doc.body.paras.iter().enumerate().filter(|(_, p)| p.props.src.is_some_and(|k| ids.contains(&k.0))).map(|(i, _)| i).collect()
    }

    /// `dropCoverPage()`: the paragraphs of the current cover leave the model (no undo step).
    fn drop_cover_page(&mut self) -> bool {
        let idx = self.cover_para_indexes();
        if idx.is_empty() {
            return false;
        }
        for &i in idx.iter().rev() {
            if let Some(paras) = self.paras_mut() {
                paras.remove(i);
            }
            self.shift_anchors(u32::try_from(i).unwrap_or(u32::MAX), -1);
        }
        if self.paras().is_some_and(|p| p.is_empty()) {
            let id = self.doc.ids.para();
            if let Some(paras) = self.paras_mut() {
                paras.push(Paragraph { id, ..Paragraph::default() });
            }
        }
        true
    }

    /// The reading half of `coverEnv()`: page and margins of the first section (pt), the theme
    /// accents (RRGGBB) and the fields (the document properties where they exist).
    fn cover_env_parts(&self, lang: PageLang) -> (CoverPage, String, String, CoverFields) {
        let sp = self.body_sect_pr_at(0);
        let col = |k: &str, d: &str| -> String {
            let v = self.doc.formatting.theme.colors.get(k).map_or(d, |s| &**s);
            uo_text::case::to_uppercase_str(rx::strip_hash(v))
        };
        let core = self.pkg.part_text("docProps/core.xml").ok().flatten().map(std::borrow::Cow::into_owned).unwrap_or_default();
        let app = self.pkg.part_text("docProps/app.xml").ok().flatten().map(std::borrow::Cow::into_owned).unwrap_or_default();
        let ph = cover_placeholders(lang);
        let or = |v: String, d: &str| if v.is_empty() { d.to_owned() } else { v };
        let fields = CoverFields {
            title: or(rx::xml_prop_trimmed(&core, "dc:title"), &ph.title),
            subtitle: or(rx::xml_prop_trimmed(&core, "dc:subject"), &ph.subtitle),
            author: or(rx::xml_prop_trimmed(&core, "dc:creator"), &ph.author),
            date: ph.date.clone(),
            company: or(rx::xml_prop_trimmed(&app, "Company"), &ph.company),
            summary: or(rx::xml_prop_trimmed(&core, "dc:description"), &ph.summary),
        };
        let pt = |x: crate::js::JsNumber| x.to_number() / 20.0;
        let page = CoverPage {
            w: pt(sp.page_width_twips),
            h: pt(sp.page_height_twips),
            top: pt(sp.margin_top_twips),
            bottom: pt(sp.margin_bottom_twips),
            left: pt(sp.margin_left_twips),
            right: pt(sp.margin_right_twips),
        };
        (page, col("accent1", "156082"), col("accent2", "E97132"), fields)
    }

    /// `coverXml(id, coverEnv())`: the XML of the cover `id` (the random ids are drawn here, as
    /// the TS does) and the env's title.
    fn cover_xml_now(&mut self, id: &str, lang: PageLang) -> (Option<String>, String) {
        let (page, accent1, accent2, fields) = self.cover_env_parts(lang);
        let title = fields.title.clone();
        let mut draw = || self.random_drawing_id();
        let mut env = CoverEnv { lang, page, accent1, accent2, fields, drawing_id: &mut draw };
        (cover_xml(id, &mut env), title)
    }

    /// `readBodyFragment(pkg, store, xml, COVER_NS)`: with the document's sources (`with_sources`),
    /// or without (the preview).
    fn read_cover_fragment(&mut self, xml: &str, with_sources: bool) -> Option<(Story, Vec<StoryGroup>)> {
        use crate::read::ctx::ReadCtx;
        let limits = crate::limits::DocxLimits::default();
        let opts = crate::read::ReadOptions { sources: false, limits };
        let doc: &mut Document = &mut self.doc;
        let mut ctx = ReadCtx::new(&self.pkg, &opts);
        ctx.ids = std::mem::take(&mut doc.ids);
        ctx.media = std::mem::take(&mut doc.media);
        if with_sources {
            ctx.sources = doc.sources.take().map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()));
        }
        let r = crate::read::fulldoc::read_body_fragment(&mut ctx, xml, COVER_NS);
        doc.ids = ctx.ids;
        doc.media = ctx.media;
        if with_sources {
            doc.sources = ctx.sources.map(Arc::new);
        }
        r.ok().flatten()
    }

    /// `insertCoverPage(id)`: the cover at the START of the document (it replaces the current one),
    /// with Different First Page on in the first section; one undo step; the title (a placeholder)
    /// ends up selected. The explicit `syncRunRPr(paras)` of the TS is the settle step (D2a-S1).
    pub(crate) fn exec_insert_cover_page(&mut self, id: &str, lang: PageLang, _cx: &mut ExecCx<'_>) -> Res {
        if !self.can_insert_section_break() {
            return self.cv_nothing();
        }
        let (xml, title) = self.cover_xml_now(id, lang);
        let Some(xml) = xml else { return self.cv_nothing() };
        let old_next = self.doc.sources.as_deref().map_or(0, |s| s.next);
        let Some((story, groups)) = self.read_cover_fragment(&xml, true) else { return self.cv_nothing() };
        self.begin_step(false, &[]);
        self.drop_cover_page();
        // the paragraphs of the fragment (the TS `flatMap` drops the tables)
        let news: Vec<Arc<Paragraph>> = story.paras.iter_arcs().cloned().collect();
        let n = news.len();
        if let Some(paras) = self.paras_mut() {
            for (k, p) in news.iter().enumerate() {
                paras.insert(k, Arc::clone(p));
            }
        }
        self.shift_anchors_from_start(i64::try_from(n).unwrap_or(i64::MAX));
        if let Some(arc) = self.doc.sources.as_mut()
            && let Some(st) = Arc::make_mut(arc).stories.get_mut("word/document.xml")
        {
            st.groups.splice(0..0, groups);
        }
        self.baseline_new_nodes(&story, old_next);
        self.edit_sect_pr_k(true, 0, |sp| obj::set(sp, "titlePg", |x| x.title_pg = Some(true)));
        // the title (the placeholder) selected: typing replaces it, else the caret in it
        let want: Vec<u16> = title.encode_utf16().collect();
        let ti = news.iter().position(|p| para_text(p) == want);
        let at = u32::try_from(ti.unwrap_or(0)).unwrap_or(0);
        let len = if ti.is_some() && rx::is_bracketed(&title) { u32::try_from(want.len()).unwrap_or(u32::MAX) } else { 0 };
        self.selection = Selection { anchor: DocPosition::new(at, 0), focus: DocPosition::new(at, len) };
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_structure();
        self.touch_formatting();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// `removeCoverPage()`: the whole "Cover Pages" `w:sdt` (the TS boolean is
    /// [`Session::has_cover_page`] asked before, F4-D14).
    pub(crate) fn exec_remove_cover_page(&mut self, _cx: &mut ExecCx<'_>) -> Res {
        if !self.can_insert_section_break() || !self.has_cover_page() {
            return self.cv_nothing();
        }
        self.begin_step(false, &[]);
        self.drop_cover_page();
        self.selection = Selection { anchor: DocPosition::new(0, 0), focus: DocPosition::new(0, 0) };
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_structure();
        self.touch_all();
        Ok(Outcome::Done(self.finish()))
    }

    /// The model half of `paintCoverTemplate(id, …)`: the blocks of the cover (read without
    /// sources) and the section properties of the preview page (the first section's, without
    /// Different First Page and page borders). The random ids are drawn as the TS does.
    pub fn cover_preview(&mut self, id: &str, lang: PageLang) -> Option<(Story, SectPr)> {
        let (xml, _) = self.cover_xml_now(id, lang);
        let (story, _) = self.read_cover_fragment(&xml?, false)?;
        let mut sp = (*self.body_sect_pr_at(0)).clone();
        obj::set(&mut sp, "titlePg", |x| x.title_pg = Some(false));
        obj::delete(&mut sp, "pgBorders");
        Some((story, sp))
    }
}
