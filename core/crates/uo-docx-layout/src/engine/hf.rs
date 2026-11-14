//! Headers and footers (frontier F7c): `layoutHeaderFooter` per page and slot (engine.ts:1139; K13:
//! the slot names the ref, one part may serve several slots). The content is a static flow
//! (`static_layout.rs`); header/footer frames, floats and tables stop there with F7b.

use crate::error::{LResult, LayoutError};
use crate::keys::PageIx;
use crate::ldoc::LDoc;
use crate::model::StoryRef;
use crate::output::{HfSlot, HfWhich, LineRef, LineStory, RefPath};
use crate::para::{FieldSource, ParaEnv};
use crate::table::CellContent;
use crate::word;
use uo_jsnum::math as jm;

use super::{Cx, Engine, js_eq};

/// Which header/footer of a page: the slot used and the story laid out for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HfUse {
    pub which: HfWhich,
    pub slot: HfSlot,
    /// index into `LDoc::stories`
    pub story: u32,
}

/// The ref of a header/footer flow (`hdr:<slot>` / `ftr:<slot>`) over story `story`.
fn hf_ref(ldoc: &LDoc<'_>, which: HfWhich, slot: HfSlot, story: u32) -> LResult<LineRef> {
    let part = match ldoc.stories.get(story as usize).and_then(|s| s.sref) {
        Some(StoryRef::HeaderFooter(p)) => p,
        _ => return Err(LayoutError::Internal("header/footer: story without its part")),
    };
    Ok(LineRef { story: LineStory::HeaderFooter { which, slot, part }, path: RefPath::default() })
}

impl Engine {
    /// `layoutStatic` of a header/footer story at `y_origin`, on page `ps` (or none: a measure).
    fn hf_static(&mut self, cx: &mut Cx<'_, '_>, story: u32, rf: &LineRef, ps: Option<PageIx>, y_origin: f64) -> LResult<CellContent> {
        let blocks = self.story_blocks(cx, story)?;
        let g = self.geom(self.cur_ps()?.geom)?;
        // (WordCraft study, page: under a book fold the header/footer keep the normal gutter/mirror margins)
        let (ml, mr) = g.hf_mar.unwrap_or((g.mar_l, g.mar_r));
        let (x, width) = (ml, g.page_w - ml - mr);
        let env = ParaEnv { field: Some(FieldSource::HeaderFooter), hf: true, ..ParaEnv::default() };
        self.layout_static(cx, &blocks, x, width, &env, rf, ps, y_origin, None)
    }

    /// `ctx.pure(() => layoutStatic(…, scratchPage(ps), y))`.
    fn hf_probe(&mut self, cx: &mut Cx<'_, '_>, story: u32, rf: &LineRef, ps: Option<PageIx>, y_origin: f64) -> LResult<f64> {
        self.pure_static(cx, |eng, cx| {
            let target = match ps {
                Some(p) => Some(eng.make_scratch(p)?),
                None => None,
            };
            let r = eng.hf_static(cx, story, rf, target, y_origin);
            eng.drop_scratch();
            r.map(|c| c.height)
        })
    }

    /// `layoutHeaderFooter(ps)` (engine.ts:1139): the page's header and footer, the body top and
    /// bottom they push.
    pub(crate) fn layout_header_footer(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let pix = self.cur_ix()?;
        let ps = self.ps(pix)?;
        // WordCraft study (page, A16): Word's parity page has no header or footer
        if ps.blank {
            return Ok(());
        }
        let g = self.geom(ps.geom)?;
        let settings = &cx.ldoc.view.formatting().settings;
        let first = ps.sect_page == 0 && g.title_pg;
        let even = settings.even_and_odd_headers && js_eq(jm::rem(ps.page.number, 2.0), 0.0);
        let (k, slot) = if first {
            (1, HfSlot::First)
        } else if even {
            (2, HfSlot::Even)
        } else {
            (0, HfSlot::Default)
        };
        let has = |s: Option<u32>| s.filter(|s| cx.ldoc.stories.get(*s as usize).is_some_and(|st| !st.blocks.is_empty()));
        let hdr = has(g.headers.get(k).copied().flatten());
        let ftr = has(g.footers.get(k).copied().flatten());
        if hdr.is_none() && ftr.is_none() {
            return Ok(());
        }
        // numbering in a header/footer: its own, it does not advance the body's list (oracle
        // numbering3, hdr-list)
        let saved_num = std::mem::replace(&mut self.ctx.flow.numbering, word::numbering_state_for(cx.opts));
        let saved_page = self.field_page.replace(pix);
        self.in_hf = true;
        // WordCraft study (page): mirrorIndents in the header/footer follow the page (mi5-hdr)
        self.mirror_static = self.ps(pix)?.doc_even == (cx.ldoc.compat_mode() >= 15.0);
        let r = self.layout_hf_in(cx, pix, hdr, ftr, slot);
        self.mirror_static = false;
        self.in_hf = false;
        self.field_page = saved_page;
        self.ctx.flow.numbering = saved_num;
        r
    }

    fn layout_hf_in(&mut self, cx: &mut Cx<'_, '_>, pix: PageIx, hdr: Option<u32>, ftr: Option<u32>, slot: HfSlot) -> LResult<()> {
        let g = self.geom(self.ps(pix)?.geom)?.clone();
        if let Some(hdr) = hdr {
            let rf = hf_ref(cx.ldoc, HfWhich::Header, slot, hdr)?;
            // the "margin" of the header objects positioned from it is the REAL top of the body
            // (pushed by the header): measured first (corpus t43)
            let probe = self.hf_probe(cx, hdr, &rf, Some(pix), g.hdr)?;
            self.ps_mut(pix)?.body_top = jm::max(g.mar_t, g.hdr + probe);
            let r = self.hf_static(cx, hdr, &rf, Some(pix), g.hdr)?;
            let ps = self.ps_mut(pix)?;
            for i in 0..r.items.len() {
                ps.hf.push_shifted(&r.items, i, g.hdr, 0.0).map_err(|_| LayoutError::Limit(crate::error::LimitKind::Capacity))?;
            }
            for l in &r.lines {
                ps.page.lines.push(l.shifted(g.hdr));
            }
            // the body starts below the header if it runs into the margin
            ps.body_top = jm::max(g.mar_t, g.hdr + r.height);
        }
        if let Some(ftr) = ftr {
            let rf = hf_ref(cx.ldoc, HfWhich::Footer, slot, ftr)?;
            // the 1st measure has no page (no float is placed); the 2nd places the footer's
            // floats from the footer's real top (pure measures: only the final layout advances
            // lists/SEQ/notes)
            let probe = self.hf_probe(cx, ftr, &rf, None, 0.0)?;
            let mut top = g.page_h - g.ftr - probe;
            // the footer's floats may push the content: measured again with them, on a scratch page
            let probe2 = self.hf_probe(cx, ftr, &rf, Some(pix), top)?;
            // the body moves up only by the floats that follow the footer's text; absolutely
            // positioned ones do not count (measured in Word)
            let mut limit_h = probe;
            if probe2 > probe + 1.0 {
                top = g.page_h - g.ftr - probe2;
                self.only_text_floats = true;
                let r = self.hf_probe(cx, ftr, &rf, Some(pix), top);
                self.only_text_floats = false;
                limit_h = r?;
            }
            let r = self.hf_static(cx, ftr, &rf, Some(pix), top)?;
            let ps = self.ps_mut(pix)?;
            for i in 0..r.items.len() {
                ps.hf.push_shifted(&r.items, i, top, 0.0).map_err(|_| LayoutError::Limit(crate::error::LimitKind::Capacity))?;
            }
            for l in &r.lines {
                ps.page.lines.push(l.shifted(top));
            }
            ps.body_bottom = jm::min(g.page_h - g.mar_b, g.page_h - g.ftr - limit_h);
        }
        Ok(())
    }
}
