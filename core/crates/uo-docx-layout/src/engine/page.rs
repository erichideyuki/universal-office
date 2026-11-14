//! Page geometry, sections and columns (frontier F7a): `SectGeom`, `PageState`, the z-ordered
//! float layers and the wrap frames (engine.ts:35–151), and the `Engine` methods that make pages
//! and move between columns and sections (`geomOf`, `pageNumber`, `newPage`, `newPageAt`,
//! `makePage`, `continuousSection`, `recordBalance`, `nextColumnFits`, `nextColumnSection`,
//! `pageCols`, `colFrame`, `breakColumn`, `noteColContent`, `closeColumns`, `applyBalance`,
//! `pageBorders`; engine.ts:405–1080).

use crate::context::{NoteNumbering, NoteRestart};
use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{ItemSide, PassItems, PassLine};
use crate::keys::PageIx;
use crate::model::JsNumber;
use crate::output::{PlacedCell, PlacedObject};
use uo_jsnum::math as jm;
use uo_render::types::Item;
use uo_render::{OptF64, PageFillId, Sym};

use super::{BalanceInfo, Cx, Engine, js_eq};

/// A column of the text area: `{ x, w }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col {
    pub x: f64,
    pub w: f64,
}

/// `chap`: chapter numbers before the page number (`w:chapStyle`/`w:chapSep`).
#[derive(Clone, Debug)]
pub struct ChapStyle {
    pub level: f64,
    pub sep: Box<str>,
}

/// `w:lnNumType`.
#[derive(Clone, Debug)]
pub struct LnNum {
    pub count_by: f64,
    pub start: f64,
    pub restart: Box<str>,
    pub distance: f64,
}

/// `notes` of a section: effective numbering, footnote position, note-area columns.
#[derive(Clone, Debug)]
pub struct SectNotes {
    pub footnote: NoteNumbering,
    pub endnote: NoteNumbering,
    pub fn_pos: Box<str>,
    pub fn_cols: OptF64,
}

/// `SectGeom` (engine.ts:35): the geometry of a section, derived once per pass (cache).
#[derive(Clone, Debug)]
pub struct SectGeom {
    pub index: u32,
    pub page_w: f64,
    pub page_h: f64,
    pub mar_l: f64,
    pub mar_r: f64,
    pub mar_t: f64,
    pub mar_b: f64,
    pub hdr: f64,
    pub ftr: f64,
    pub cols: Vec<Col>,
    pub col_sep: bool,
    pub title_pg: bool,
    pub kind: Box<str>,
    pub v_align: Option<Box<str>>,
    pub pg_start: OptF64,
    pub pg_fmt: Option<Box<str>>,
    pub chap: Option<ChapStyle>,
    /// header/footer stories per slot `[default, first, even]` (index into `LDoc::stories`; K13:
    /// one part can serve several slots)
    pub headers: [Option<u32>; 3],
    pub footers: [Option<u32>; 3],
    /// `blocks`: `[blockStart, blockEnd]` of the section in the body (JS numbers)
    pub blocks: (f64, f64),
    pub ln_num: Option<LnNum>,
    pub line_pitch: OptF64,
    pub notes: SectNotes,
    /// `balKey` (default: the section index; the endnote area is balanced apart)
    pub bal_key: Option<i64>,
    /// WordCraft study (page; not TS state): the index in `Engine::geoms` of the canonical geometry this one belongs to
    /// (itself for a section's or an endnote flow's geometry; the canonical one for its even-page variant)
    pub base: u32,
    /// WordCraft study (page): the geometry of the pages whose document parity is EVEN when Word's margins differ there
    /// (mirrorMargins, bookFold*, printTwoOnOne); `None` = every page uses this one
    pub even: Option<u32>,
    /// WordCraft study (page): the header/footer's left and right margins when they differ from the body's (bookFold*:
    /// the body follows the fold, the header/footer the normal gutter/mirror rules)
    pub hf_mar: Option<(f64, f64)>,
}

/// WordCraft study (page; Word 16.0.20527, record uo-docx-layout.md "WordCraft study — page"): the margins of a page by
/// Word's rules for `w:gutter`, `w:rtlGutter`, `w:gutterAtTop`, `w:mirrorMargins`, `w:bookFold(Rev)Printing` and
/// `w:printTwoOnOne` — `(left, right, top, bottom)` of the body and the header/footer's `(left, right)`. `m` = the model's
/// margins; `even` = the page's document parity (the first section's start + the physical page index) is even.
fn word_margins(
    cx: &Cx<'_, '_>,
    sect: Option<&uo_docx::model::section::SectionProps>,
    page: (f64, f64),
    m: (f64, f64, f64, f64),
    even: bool,
) -> ((f64, f64, f64, f64), (f64, f64)) {
    let (l0, r0, t0, b0) = m;
    let g = sect.and_then(|p| p.gutter).and_then(JsNumber::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let facts = cx.ldoc.view.document().read_facts;
    let normal = if cx.opts.ts_no_gutter_mirror {
        m
    } else if facts.gutter_at_top {
        // the gutter above the top margin; mirrorMargins and rtlGutter ignored (g-gut-top*, g2-top0-mirror, g2-gut-top-rtl)
        (l0, r0, t0 + g, b0)
    } else {
        let (gl, gr) = if sect.and_then(|p| p.rtl_gutter) == Some(true) { (0.0, g) } else { (g, 0.0) };
        if cx.ldoc.view.formatting().settings.mirror_margins && even { (r0 + gr, l0 + gl, t0, b0) } else { (l0 + gl, r0 + gr, t0, b0) }
    };
    if cx.opts.ts_no_print_fold {
        return (normal, (normal.0, normal.1));
    }
    let land = page.0 > page.1;
    if facts.print_two_on_one {
        // portrait: the gutter at the fold, top/bottom swapping per page, the sides (header/footer too) plain; landscape:
        // the sides swap, the header/footer follow the body (g2-two-*, g3-two-land-gut)
        let body = match (land, even) {
            (true, false) => (r0 + g, l0, t0, b0),
            (true, true) => (l0, r0 + g, t0, b0),
            (false, false) => (l0, r0, b0 + g, t0),
            (false, true) => (l0, r0, t0, b0 + g),
        };
        return (body, if land { (body.0, body.1) } else { (l0, r0) });
    }
    if let Some(rev) = facts.book_fold {
        // the body by the fold (portrait: plain margins), the header/footer by the normal rules (g-fold-*, g2-fold-*)
        let body = match (land, rev, even) {
            (false, _, _) => (l0, r0, t0, b0),
            (true, false, false) => (r0 + g, l0, t0, b0),
            (true, false, true) => (l0, r0 + g, t0, b0),
            (true, true, false) => (l0 + g, r0, t0, b0),
            (true, true, true) => (r0, l0 + g, t0, b0),
        };
        return (body, (normal.0, normal.1));
    }
    (normal, (normal.0, normal.1))
}

impl SectGeom {
    /// `balKey(g)`: `g.balKey ?? g.index`.
    #[must_use]
    pub fn bal_key(&self) -> i64 {
        self.bal_key.unwrap_or(i64::from(self.index))
    }
}

/// `FloatBox` (engine.ts:71): a float's wrap box.
#[derive(Clone, Debug)]
pub struct FloatBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub wrap: Box<str>,
    pub side: Option<Box<str>>,
    pub dist_l: f64,
    pub dist_r: f64,
    pub dist_t: f64,
    pub dist_b: f64,
    pub body_only: bool,
    /// `poly`: the wrap polygon (page twips)
    pub poly: Option<Vec<(f64, f64)>>,
}

/// `WrapFrame` (engine.ts:121): a line's space around floats.
#[derive(Clone, Debug)]
pub struct WrapFrame {
    pub left: f64,
    pub right: f64,
    pub skip_to: OptF64,
    pub segs: Option<Vec<(f64, f64)>>,
    pub gap: bool,
}

/// `ZObj` (engine.ts:151): a float with its z order (`relativeHeight` / VML z-index).
#[derive(Clone, Debug, Default)]
pub struct ZObj {
    pub z: f64,
    pub seq: f64,
    pub items: PassItems,
}

/// The `LayoutPage` being built inside a `PageState` (its items are assembled from the layers at
/// the end of the pass).
#[derive(Clone, Debug, Default)]
pub struct PageOut {
    pub w: f64,
    pub h: f64,
    pub section: u32,
    pub number: f64,
    pub lines: Vec<PassLine>,
    pub objects: Option<Vec<PlacedObject>>,
    /// the content lines of the page's textboxes (`PlacedObject.content` ranges; added by F7b)
    pub content_lines: Vec<PassLine>,
    pub cells: Option<Vec<PlacedCell>>,
    pub hf_items: Option<u32>,
    pub background: Option<Sym>,
    pub background_fill: Option<PageFillId>,
    pub background_show: bool,
}

/// A note on a page: its id and its laid-out content.
#[derive(Clone, Debug)]
pub struct PageNote {
    /// the note id (a JS number: SameValueZero, F7c)
    pub id: crate::model::NumKey,
    pub content: crate::table::CellContent,
}

/// `PageState` (engine.ts:123). Output (`page`, the layers) and per-page working state (floats,
/// notes, note area) — the level-2 checkpoints of milestone 2 keep it whole.
#[derive(Clone, Debug, Default)]
pub struct PageState {
    pub page: PageOut,
    /// index into `Engine::geoms`
    pub geom: u32,
    pub body_top: f64,
    pub body_bottom: f64,
    pub behind: Vec<ZObj>,
    pub hf: PassItems,
    pub body: PassItems,
    pub front: Vec<ZObj>,
    pub hf_behind: Vec<ZObj>,
    pub hf_front: Vec<ZObj>,
    pub floats: Vec<FloatBox>,
    pub sect_page: u32,
    pub notes: Vec<PageNote>,
    pub note_area: f64,
    pub body_bottom_base: f64,
    pub notes_full: bool,
    pub cols: Option<Vec<Col>>,
    /// WordCraft study (page, A16; not TS state): a blank page Word inserts for a section's parity — no header, footer
    /// or page border, not counted in SECTIONPAGES
    pub blank: bool,
    /// WordCraft study (page): the page's document parity (the first section's start + the physical page index) is
    /// even — what Word's mirror margins, inside/outside floats and mirrorIndents follow
    pub doc_even: bool,
    /// WordCraft study (fields, B23; not TS state): every section geometry with content on the page — its own and each
    /// one a continuous break starts on it (SECTIONPAGES counts the page in each)
    pub sections: Vec<u32>,
    /// WordCraft study (fields, B23): the base geometry of the section a continuous break starts on this page
    pub cont_first: Option<u32>,
    /// WordCraft study (fields, B23): the page's index among its section's pages FOR THE NUMBERING (Word counts a
    /// continuous section from its first, shared page; `sect_page` keeps the TS count, which the first-page header uses)
    pub num_sect_page: u32,
}

/// `CHAP_SEP` (engine.ts:197): `w:chapSep` → the character between chapter and page.
fn chap_sep(v: &str) -> Option<&'static str> {
    match v {
        "hyphen" => Some("-"),
        "period" => Some("."),
        "colon" => Some(":"),
        "emDash" => Some("\u{2014}"),
        "enDash" => Some("\u{2013}"),
        _ => None,
    }
}

/// The width of the line between columns (`w:sep`) in Word's PDF: 0.84 pt (oracle sections, sep-*).
const COL_SEP_W: f64 = 16.8;

/// Columns past this count stop the layout: the TS allocates one object per column of a
/// `w:cols/@w:num` up to any number (Word allows 45) — F7a-6.
const MAX_COLUMNS: f64 = 4096.0;

/// `x ?? d` of an optional model number (`null` is nullish).
fn or(v: Option<JsNumber>, d: f64) -> f64 {
    v.and_then(JsNumber::as_f64).unwrap_or(d)
}

fn restart_of(s: Option<&str>) -> NoteRestart {
    match s {
        Some("eachPage") => NoteRestart::EachPage,
        Some("eachSect") => NoteRestart::EachSect,
        _ => NoteRestart::Continuous,
    }
}

impl Engine {
    /// `geomOf(s, i)` (engine.ts:784). `s` = `None` is the default section `run()` builds for a
    /// document without one (A4 portrait, 1701/1417 margins, every block).
    /// (WordCraft study, page: `even` = the geometry of the pages whose document parity is even, Word's margins)
    pub(crate) fn geom_of(&self, cx: &Cx<'_, '_>, s: Option<&uo_docx::model::Section>, i: usize, even: bool) -> LResult<SectGeom> {
        let sp = s.map(|s| &*s.sect_pr);
        let sect = s.and_then(|s| s.sect.as_deref());
        let num = |v: Option<JsNumber>, d: f64| v.map_or(d, JsNumber::to_number);
        let page_w = num(sp.map(|p| p.page_width_twips), 11906.0);
        let page_h = num(sp.map(|p| p.page_height_twips), 16838.0);
        let m0 = (
            num(sp.map(|p| p.margin_left_twips), 1701.0),
            num(sp.map(|p| p.margin_right_twips), 1701.0),
            num(sp.map(|p| p.margin_top_twips), 1417.0),
            num(sp.map(|p| p.margin_bottom_twips), 1417.0),
        );
        let ((mar_l, mar_r, mar_t, mar_b), hf_mar) = word_margins(cx, sect, (page_w, page_h), m0, even);
        let hf_mar = (!js_eq(hf_mar.0, mar_l) || !js_eq(hf_mar.1, mar_r)).then_some(hf_mar);
        let content_w = jm::max(720.0, page_w - mar_l - mar_r);
        let mut cols = Vec::new();
        let defs = sect.and_then(|s| s.col_defs.as_deref());
        let cols_pr = sp.and_then(|p| p.cols.as_ref());
        let n = jm::max(1.0, or(cols_pr.map(|c| c.count), 1.0));
        if n > MAX_COLUMNS {
            return Err(LayoutError::Limit(LimitKind::Capacity));
        }
        let n_defs = defs.map_or(0.0, |d| f64::from(u32::try_from(d.len()).unwrap_or(u32::MAX)));
        if n > 1.0 && defs.is_some() && n_defs >= n && sect.and_then(|s| s.equal_width) == Some(false) {
            let mut x = mar_l;
            // `defs.slice(0, n)`: ToIntegerOrInfinity(n) (n ≤ MAX_COLUMNS here)
            for d in defs.unwrap_or(&[]).iter().take(uo_jsnum::conv::to_uint32(n) as usize) {
                let (w, space) = (d.w.to_number(), d.space.to_number());
                cols.push(Col { x, w });
                x += w + space;
            }
        } else if n > 1.0 {
            let space = or(cols_pr.map(|c| c.space_twips), 720.0);
            let w = (content_w - space * (n - 1.0)) / n;
            let mut k = 0.0;
            while k < n {
                cols.push(Col { x: mar_l + k * (w + space), w });
                k += 1.0;
            }
        } else {
            cols.push(Col { x: mar_l, w: content_w });
        }
        // header/footer inherited from the previous section when the section does not define them
        let prev = if i > 0 { self.geoms.get(i - 1) } else { None };
        let pick = |own: Option<&uo_docx::js::JsRecord<crate::model::HfPartId>>, inherited: Option<[Option<u32>; 3]>| {
            let mut out = inherited.unwrap_or([None; 3]);
            for (k, slot) in ["default", "first", "even"].into_iter().enumerate() {
                if let Some(part) = own.and_then(|o| o.get(slot))
                    && let Some(d) = out.get_mut(k)
                {
                    // a part the reader keeps but no story derived from (none) reads as absent
                    *d = cx.ldoc.hf_stories.get(part).copied().or(*d);
                }
            }
            out
        };
        let headers = pick(s.map(|s| &s.headers), prev.map(|p| p.headers));
        let footers = pick(s.map(|s| &s.footers), prev.map(|p| p.footers));
        // notes: the section's footnotePr/endnotePr over the settings' (Word writes both);
        // defaults 1, 2, 3 at the foot and i, ii, iii at the end
        let st = &cx.ldoc.view.formatting().settings;
        let (sfn, sen) = (st.footnote_pr.as_ref(), st.endnote_pr.as_ref());
        let (pfn, pen) = (sp.and_then(|p| p.footnote_pr.as_ref()), sp.and_then(|p| p.endnote_pr.as_ref()));
        let fn_fmt = pfn.and_then(|p| p.num_fmt.as_deref()).or(sfn.and_then(|p| p.num_fmt.as_deref()));
        let fn_start = pfn.and_then(|p| p.num_start).or(sfn.and_then(|p| p.num_start));
        let fn_restart = pfn.and_then(|p| p.num_restart.as_deref()).or(sfn.and_then(|p| p.num_restart.as_deref()));
        let fn_pos = pfn.and_then(|p| p.pos.as_deref()).or(sfn.and_then(|p| p.pos.as_deref()));
        let en_fmt = pen.and_then(|p| p.num_fmt.as_deref()).or(sen.and_then(|p| p.num_fmt.as_deref()));
        let en_start = pen.and_then(|p| p.num_start).or(sen.and_then(|p| p.num_start));
        let en_restart = pen.and_then(|p| p.num_restart.as_deref()).or(sen.and_then(|p| p.num_restart.as_deref()));
        let notes = SectNotes {
            footnote: NoteNumbering {
                fmt: fn_fmt.unwrap_or("decimal").into(),
                start: or(fn_start, 1.0),
                restart: restart_of(fn_restart.or(Some("continuous"))),
            },
            // (an endnote does not restart per page)
            endnote: NoteNumbering {
                fmt: en_fmt.unwrap_or("lowerRoman").into(),
                start: or(en_start, 1.0),
                restart: if en_restart == Some("eachSect") { NoteRestart::EachSect } else { NoteRestart::Continuous },
            },
            fn_pos: fn_pos.unwrap_or("pageBottom").into(),
            fn_cols: match sp.and_then(|p| p.footnote_columns) {
                Some(c) if c.truthy() => OptF64::some(c.to_number()),
                _ => OptF64::NONE,
            },
        };
        let blocks_len = self.story_len(cx);
        let blocks = match s {
            Some(s) => (s.block_start.to_number(), s.block_end.to_number()),
            None => (0.0, blocks_len - 1.0),
        };
        let pg_fmt = sp.and_then(|p| p.pg_num_fmt.as_deref()).or(sect.and_then(|s| s.pg_num_fmt.as_deref()));
        let chap = sp.and_then(|p| p.pg_num_chap_style).filter(|c| c.truthy()).map(|c| ChapStyle {
            level: c.to_number(),
            sep: chap_sep(sp.and_then(|p| p.pg_num_chap_sep.as_deref()).unwrap_or("hyphen")).unwrap_or("-").into(),
        });
        let line_pitch = match sect.and_then(|s| s.doc_grid.as_ref()) {
            // (WordCraft study, lines L5: snapToChars is a line grid too in Word — `wcl2/l-cgrid-sc-*`)
            Some(dg)
                if (matches!(dg.r#type.as_deref(), Some("lines" | "linesAndChars"))
                    || (!cx.opts.ts_no_char_grid && dg.r#type.as_deref() == Some("snapToChars")))
                    && or(dg.line_pitch, 0.0) > 0.0 =>
            {
                OptF64::some(dg.line_pitch.map_or(f64::NAN, JsNumber::to_number))
            }
            _ => OptF64::NONE,
        };
        // (without w:countBy, or 0, Word does not number — oracle lnnum3/s-nocountby, s-countby0)
        let ln_num = sect.and_then(|s| s.ln_num_type.as_ref()).filter(|l| or(l.count_by, 0.0) >= 1.0).map(|l| LnNum {
            count_by: l.count_by.map_or(f64::NAN, JsNumber::to_number),
            start: or(l.start, 0.0),
            restart: l.restart.as_deref().unwrap_or("newPage").into(),
            // "Automatic" (no w:distance, or 0): 1/4" in one column, 1/8" in several (oracle lnnum2)
            distance: if or(l.distance, 0.0) > 0.0 {
                l.distance.map_or(f64::NAN, JsNumber::to_number)
            } else if cols.len() > 1 {
                180.0
            } else {
                360.0
            },
        });
        let index = u32::try_from(i).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        Ok(SectGeom {
            notes,
            index,
            page_w,
            page_h,
            mar_l,
            mar_r,
            mar_t,
            mar_b,
            base: index,
            even: None,
            hf_mar,
            hdr: or(sp.and_then(|p| p.header_distance_twips), 720.0),
            ftr: or(sp.and_then(|p| p.footer_distance_twips), 720.0),
            col_sep: cols_pr.is_some_and(|c| c.sep),
            title_pg: sp.and_then(|p| p.title_pg) == Some(true),
            kind: sect.map_or("nextPage", |s| &*s.r#type).into(),
            v_align: sect.and_then(|s| s.v_align.as_deref()).filter(|v| !v.is_empty()).map(Into::into),
            pg_start: match sp.and_then(|p| p.pg_num_start) {
                Some(v) => OptF64::some(v.to_number()),
                None => OptF64::NONE,
            },
            pg_fmt: pg_fmt.filter(|f| !f.is_empty()).map(Into::into),
            chap,
            cols,
            headers,
            footers,
            blocks,
            ln_num,
            line_pitch,
            bal_key: None,
        })
    }

    /// `pageNumber(g, sectPage)` (engine.ts:849): the numbering continues from the previous
    /// section, unless `pgNumType/@start`.
    fn page_number(&self, g: &SectGeom, sect_page: u32) -> f64 {
        if let Some(st) = g.pg_start.get() {
            return st + f64::from(sect_page);
        }
        let prev = self.pages.last();
        if sect_page == 0 {
            return prev.map_or(1.0, |p| p.page.number + 1.0);
        }
        prev.map_or(0.0, |p| p.page.number) + 1.0
    }

    /// `newPage(g, breakType)` (engine.ts:857): evenPage/oddPage add a blank page for parity.
    pub(crate) fn new_page(&mut self, cx: &mut Cx<'_, '_>, gi: u32, break_type: &str) -> LResult<()> {
        // (WordCraft study, page: a page's geometry may be its section's even-page variant)
        let gi = self.geom(gi)?.base;
        let sect_page = match self.cur {
            Some(c) if self.geom(self.ps(c)?.geom)?.base == gi => self.ps(c)?.sect_page.saturating_add(1),
            _ => 0,
        };
        let g = self.geom(gi)?.clone();
        // WordCraft study (fields, B23): Word numbers a continuous section from its first (shared) page — the page after
        // it is the section's 2nd (`pgNumType start` + 1); the frozen TS from its first own page (`ts_section_pages_shared`)
        let num_sect_page = match self.cur {
            Some(c) if !cx.opts.ts_section_pages_shared && self.geom(self.ps(c)?.geom)?.base == gi => {
                let ps = self.ps(c)?;
                if ps.cont_first == Some(gi) { 1 } else { ps.num_sect_page.saturating_add(1) }
            }
            _ => sect_page,
        };
        let mut number = self.page_number(&g, num_sect_page);
        let section_start = sect_page == 0 && !self.pages.is_empty();
        if cx.opts.ts_section_parity {
            if section_start && (break_type == "evenPage" || break_type == "oddPage") {
                let want_even = break_type == "evenPage";
                if js_eq(jm::rem(number, 2.0), 0.0) != want_even {
                    self.make_page(cx, gi, sect_page, number, false)?;
                    number += 1.0;
                    return self.new_page_at(cx, gi, sect_page + 1, number);
                }
            }
        } else if section_start && matches!(break_type, "nextPage" | "oddPage" | "evenPage") {
            // WordCraft study (page, A16; measured in modes 15, 14 and 12): (a) an odd/even break whose number has the wrong
            // parity bumps a restarted number, else takes a blank page and the number after it; (b) with
            // evenAndOddHeaders or mirrorMargins the number's parity must also be the document parity of the page, else
            // a blank page. The blank page is empty (no header/footer), has the new section's paper, counts in NUMPAGES
            // and in no SECTIONPAGES; the text page after it is the section's first page.
            let even_of = |v: f64| js_eq(jm::rem(v, 2.0), 0.0);
            if (break_type == "evenPage" || break_type == "oddPage") && even_of(number) != (break_type == "evenPage") {
                if g.pg_start.is_some() {
                    number += 1.0;
                } else {
                    self.make_page(cx, gi, sect_page, number, true)?;
                    number += 1.0;
                }
            }
            let st = &cx.ldoc.view.formatting().settings;
            if st.even_and_odd_headers || st.mirror_margins {
                let d = self.first_page_number + f64::from(u32::try_from(self.pages.len()).unwrap_or(u32::MAX));
                if even_of(number) != even_of(d) {
                    let blank_number = self.pages.last().map_or(1.0, |p| p.page.number + 1.0);
                    self.make_page(cx, gi, sect_page, blank_number, true)?;
                }
            }
        }
        self.new_page_at(cx, gi, sect_page, number)?;
        self.cur_ps_mut()?.num_sect_page = num_sect_page; // WordCraft study (fields, B23)
        Ok(())
    }

    /// WordCraft study (page): the geometry of section geometry `gi` (a canonical one) on a page of document parity
    /// `even`.
    pub(crate) fn geom_variant(&self, gi: u32, even: bool) -> LResult<u32> {
        let g = self.geom(gi)?;
        let base = g.base;
        Ok(if even { self.geom(base)?.even.unwrap_or(base) } else { base })
    }

    /// WordCraft study (page): the document parity of the NEXT page (the first section's start + its physical index).
    pub(crate) fn next_doc_even(&self) -> bool {
        let d = self.first_page_number + f64::from(u32::try_from(self.pages.len()).unwrap_or(u32::MAX));
        js_eq(jm::rem(d, 2.0), 0.0)
    }

    /// `newPageAt(g, sectPage, number)` (engine.ts:872).
    fn new_page_at(&mut self, cx: &mut Cx<'_, '_>, gi: u32, sect_page: u32, number: f64) -> LResult<()> {
        self.make_page(cx, gi, sect_page, number, false)?;
        self.col = 0;
        let body_top = self.cur_ps()?.body_top;
        self.col_top = body_top;
        self.y = body_top;
        self.col_used = Vec::new();
        self.balance_saved = None;
        self.apply_balance(cx)?;
        self.at_top = true;
        self.top_gap_prev_after = None;
        self.prev = None;
        self.ctx.begin_note_page();
        // endnotes that continue: the continuation separator (margin to margin) at the top
        // (oracle endnotes, en-fill50/en-split)
        if self.en_flow.is_some_and(|e| e.sep_done) {
            self.continuation_separator(cx, true)?;
        }
        if !self.float_carry.is_empty() {
            self.place_float_carry(cx)?;
        }
        // a page of notes only (a long continuation): the text goes on to the next one
        if self.cur_ps()?.notes_full {
            let g = self.cur_ps()?.geom;
            return self.new_page(cx, g, "nextPage");
        }
        // late floats of this page: placed before the text (F7b)
        let pix = u32::try_from(self.pages.len().saturating_sub(1)).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        if let Some(late) = cx.input.late_floats.get(&pix) {
            // a late float reads its anchor's paragraph, wherever it is (LA7)
            self.note_read_all();
            let fr = self.col_frame()?;
            let cur = self.cur_ix()?;
            let body_top = self.ps(cur)?.body_top;
            // (WordCraft study, page, B10: a provisional pin is not placed at the page start)
            for lf in late.iter().filter(|l| l.col == 0 && l.pin.is_none_or(|p| p.settled)) {
                self.place_late_float(cx, lf, cur, fr, body_top)?;
            }
        }
        self.place_late_tables(cx)
    }

    /// One late float at its page (or column) start: the TS's absolute one from the column frame and the body top;
    /// a pinned one (WordCraft study, page, B10/B11) at the position Word keeps for it.
    fn place_late_float(&mut self, cx: &mut Cx<'_, '_>, lf: &super::LateFloat, cur: PageIx, fr: Col, body_top: f64) -> LResult<()> {
        match lf.pin {
            None => {
                self.place_float(cx, lf.run, cur, fr.x, fr.w, body_top, None, 0.0, None, None)?;
            }
            Some(pin) => {
                self.float_line_top = pin.line_top;
                let r = self.place_float(cx, lf.run, cur, pin.col_x, pin.col_w, pin.para_top, None, 0.0, None, Some(pin.para_ref));
                self.float_line_top = None;
                r?;
            }
        }
        self.pre_placed.insert(lf.run);
        Ok(())
    }

    /// WordCraft study (page, B10): the pinned late floats of the column the flow just entered on the current page.
    fn place_late_col(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let pix = u32::try_from(self.pages.len().saturating_sub(1)).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        let col = self.col;
        let Some(late) = cx.input.late_floats.get(&pix) else { return Ok(()) };
        let todo: Vec<super::LateFloat> = late
            .iter()
            .filter(|l| l.col == col && l.col > 0 && l.pin.is_some_and(|p| p.settled) && !self.pre_placed.contains(&l.run))
            .copied()
            .collect();
        if todo.is_empty() {
            return Ok(());
        }
        self.note_read_all();
        let fr = self.col_frame()?;
        let cur = self.cur_ix()?;
        let top = self.col_top;
        for lf in &todo {
            self.place_late_float(cx, lf, cur, fr, top)?;
        }
        Ok(())
    }

    /// `makePage(g, sectPage, number)` (engine.ts:912). (WordCraft study, page: the geometry is the variant of the page's
    /// document parity; `blank` = a parity page Word leaves empty.)
    fn make_page(&mut self, cx: &mut Cx<'_, '_>, gi: u32, sect_page: u32, number: f64, blank: bool) -> LResult<()> {
        if self.pages.len() >= cx.opts.limits.max_pages as usize {
            return Err(LayoutError::Limit(LimitKind::Pages));
        }
        cx.budget.charge(1)?;
        let doc_even = self.next_doc_even();
        let gi = self.geom_variant(gi, doc_even)?;
        let g = self.geom(gi)?.clone();
        let ps = PageState {
            page: PageOut { w: g.page_w, h: g.page_h, section: g.index, number, ..PageOut::default() },
            geom: gi,
            body_top: g.mar_t,
            body_bottom: g.page_h - g.mar_b,
            sect_page,
            body_bottom_base: g.page_h - g.mar_b,
            blank,
            doc_even,
            sections: vec![gi], // WordCraft study (fields, B23)
            num_sect_page: sect_page,
            ..PageState::default()
        };
        if self.cur.is_some() {
            self.close_columns(cx)?;
            let c = self.cur_ix()?;
            self.paint_notes(cx, c)?;
        }
        self.pages.push(ps);
        if let Some(t) = self.trace.as_deref_mut() {
            t.touch(u32::try_from(self.pages.len() - 1).unwrap_or(u32::MAX));
        }
        self.cur = Some(PageIx(u32::try_from(self.pages.len() - 1).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?));
        self.col_max_y = 0.0;
        self.layout_header_footer(cx)?;
        let ps = self.cur_ps_mut()?;
        ps.body_bottom_base = ps.body_bottom;
        // the continuation of a note split on the previous page: first at this one's foot
        // (oracle fncont)
        if !self.note_carry.is_empty() && !blank {
            self.place_note_carry(cx, gi)?;
        }
        Ok(())
    }

    /// `applyBalance()` (engine.ts:405): the column height on the last page of a balanced section.
    pub(crate) fn apply_balance(&mut self, cx: &Cx<'_, '_>) -> LResult<()> {
        let key = self.geom(self.cur_ps()?.geom)?.bal_key();
        let Some(spec) = cx.input.balance.get(&key).copied() else { return Ok(()) };
        if spec.page as usize != self.pages.len().wrapping_sub(1) {
            return Ok(());
        }
        let col_top = self.col_top;
        let bottom = self.cur_ps()?.body_bottom;
        self.balance_saved = Some(bottom);
        self.balance_last_free = spec.last_free;
        self.cur_ps_mut()?.body_bottom = jm::min(bottom, col_top + spec.h);
        Ok(())
    }

    /// `continuousSection(g)` (engine.ts:946).
    pub(crate) fn continuous_section(&mut self, cx: &mut Cx<'_, '_>, gi: u32) -> LResult<()> {
        // the section that ends (several columns): what it took on this page (balancing)
        let ending = self.cur_ps()?.geom;
        self.record_balance(ending)?;
        if let Some(saved) = self.balance_saved.take() {
            self.cur_ps_mut()?.body_bottom = saved;
        }
        self.close_columns(cx)?;
        self.col_used = Vec::new();
        // columns before or after the break: the "after" of the previous section's last paragraph
        // stays ABOVE the top of the new columns (oracle sections, cont-*)
        let last_after = match &self.prev {
            Some(p) if !self.at_top => p.after,
            _ => 0.0,
        };
        let (ending_cols, new_cols) = (self.geom(ending)?.cols.len(), self.geom(gi)?.cols.len());
        if (ending_cols > 1 || new_cols > 1) && last_after > 0.0 && self.sect_gap_after.is_none() {
            self.y = jm::min(self.y + last_after, self.cur_ps()?.body_bottom);
            self.sect_gap_after = Some(last_after);
        }
        // same page; new column geometry from the end of the TALLEST column of the previous
        // section (corpus t89)
        self.y = jm::max(self.y, self.col_max_y);
        self.col_max_y = 0.0;
        let gv = self.geom_variant(gi, self.cur_ps()?.doc_even)?; // (WordCraft study, page)
        let base = self.geom(gi)?.base;
        let ps = self.cur_ps_mut()?;
        ps.geom = gv;
        ps.cols = None;
        // WordCraft study (fields, B23): the section has content on this page too
        ps.sections.push(gv);
        ps.cont_first = Some(base);
        self.col = 0;
        self.col_top = self.y;
        self.apply_balance(cx)
        // the "after" of the previous section's last paragraph collapses with the "before" of
        // this one's first, as between paragraphs (oracle colbreak, cb-textsect-*)
    }

    /// `recordBalance(ending)` (engine.ts:978).
    pub(crate) fn record_balance(&mut self, ending: u32) -> LResult<()> {
        let eg = self.geom(ending)?;
        if eg.cols.len() <= 1 {
            return Ok(());
        }
        let n = u32::try_from(eg.cols.len()).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        let key = eg.bal_key();
        let mut used = self.col_used.clone();
        used.push(self.y - self.col_top);
        let ps = self.cur_ps()?;
        let bottom = self.balance_saved.unwrap_or(ps.body_bottom);
        // the end of the last line of each column (one moved down by widows/orphans does not count)
        let cols = self.page_cols()?;
        let mut content = vec![0.0; cols.len()];
        for l in &ps.page.lines {
            if l.y < self.col_top - 1.0
                || l.y + l.height > bottom + 1.0
                || matches!(l.at.story, crate::output::LineStory::HeaderFooter { .. })
            {
                continue;
            }
            let mid = l.x + l.width / 2.0;
            let c = cols.iter().position(|k| mid < k.x + k.w + 1.0).unwrap_or(cols.len().saturating_sub(1));
            if let Some(slot) = content.get_mut(c) {
                *slot = jm::max(*slot, l.y + l.height - self.col_top);
            }
        }
        let sum = used.iter().fold(0.0, |a, b| a + jm::max(0.0, *b));
        let page = u32::try_from(self.pages.len().saturating_sub(1)).map_err(|_| LayoutError::Limit(LimitKind::Capacity))?;
        self.found.balance_found.insert(key, BalanceInfo { page, col_top: self.col_top, bottom, sum, n, used, content });
        Ok(())
    }

    /// `nextColumnFits(g)` (engine.ts:997): same number of columns and not in the last one.
    pub(crate) fn next_column_fits(&self, gi: u32) -> LResult<bool> {
        if self.cur.is_none() || self.geom(gi)?.cols.len() < 2 {
            return Ok(false);
        }
        let cols = self.page_cols()?;
        Ok(cols.len() == self.geom(gi)?.cols.len() && (self.col as usize) < cols.len() - 1)
    }

    /// `nextColumnSection(g)` (engine.ts:1007).
    pub(crate) fn next_column_section(&mut self, gi: u32) -> LResult<()> {
        let last_after = self.prev.as_ref().map_or(0.0, |p| p.after);
        if let Some(saved) = self.balance_saved.take() {
            self.cur_ps_mut()?.body_bottom = saved;
        }
        let cols = self.page_cols()?.to_vec();
        let gv = self.geom_variant(gi, self.cur_ps()?.doc_even)?; // (WordCraft study, page)
        let ps = self.cur_ps_mut()?;
        ps.cols = Some(cols);
        ps.geom = gv;
        self.col_used.push(self.y - self.col_top);
        self.col_max_y = jm::max(self.col_max_y, self.y);
        self.col += 1;
        self.y = self.col_top;
        self.at_top = true;
        self.prev = None;
        self.top_gap_prev_after = Some(last_after);
        Ok(())
    }

    /// `pageCols()` (engine.ts:1023).
    pub(crate) fn page_cols(&self) -> LResult<&[Col]> {
        let ps = self.cur_ps()?;
        match &ps.cols {
            Some(c) => Ok(c),
            None => Ok(&self.geom(ps.geom)?.cols),
        }
    }

    /// `colFrame()` (engine.ts:1027): `cols[col] ?? cols[0]!`.
    pub(crate) fn col_frame(&self) -> LResult<Col> {
        let cols = self.page_cols()?;
        cols.get(self.col as usize).or_else(|| cols.first()).copied().ok_or(LayoutError::Internal("colFrame: no column"))
    }

    /// `breakColumn()` (engine.ts:1034): the next column or a new page.
    pub(crate) fn break_column(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let gi = self.cur_ps()?.geom;
        let n = self.page_cols()?.len();
        // wave 7c (W, item 6): a keepLines paragraph taller than a column in mode 15 goes on on the next PAGE
        if (self.col as usize) < n.saturating_sub(1) && !self.kl_col_to_page {
            self.col_used.push(self.y - self.col_top);
            self.col_max_y = jm::max(self.col_max_y, self.y);
            self.col += 1;
            if self.balance_last_free
                && let Some(saved) = self.balance_saved
                && self.col as usize == n - 1
            {
                self.cur_ps_mut()?.body_bottom = saved;
            }
            self.y = self.col_top;
            self.at_top = true;
            self.prev = None;
            // endnotes in columns: the next column keeps the separator's place (no line —
            // oracle endnotes, en-cols)
            if self.en_flow.is_some_and(|e| e.sep_done) {
                self.continuation_separator(cx, false)?;
            }
            // WordCraft study (page, B10): the floats Word pins to this column (no late float has a column > 0 under
            // the frozen TS's switches)
            self.place_late_col(cx)
        } else {
            self.new_page(cx, gi, "nextPage")
        }
    }

    /// `noteColContent(bottom)` (engine.ts:1055): content in the current column down to `bottom`.
    pub(crate) fn note_col_content(&mut self, bottom: f64) -> LResult<()> {
        let c = self.col as usize;
        let lim = self.balance_saved.unwrap_or(self.cur_ps()?.body_bottom);
        if self.col_bottoms.len() <= c {
            self.col_bottoms.resize(c + 1, None);
        }
        if let Some(slot) = self.col_bottoms.get_mut(c) {
            *slot = Some(jm::max(slot.unwrap_or(f64::NEG_INFINITY), jm::min(bottom, lim)));
        }
        Ok(())
    }

    /// `closeColumns()` (engine.ts:1067): with `w:sep`, the line between columns — a black
    /// rectangle 0.84 pt wide in the middle of the gap, from the columns' top to the end of the
    /// longest column, only before the columns that have content on this page (oracle sections,
    /// sep-*).
    pub(crate) fn close_columns(&mut self, cx: &mut Cx<'_, '_>) -> LResult<()> {
        let b = std::mem::take(&mut self.col_bottoms);
        let Some(cur) = self.cur else { return Ok(()) };
        let ps = self.ps(cur)?;
        let cols = self.page_cols()?.to_vec();
        if !self.geom(ps.geom)?.col_sep || cols.len() < 2 || b.is_empty() {
            return Ok(());
        }
        let bottom = jm::max_n(&b.iter().filter_map(|v| *v).collect::<Vec<_>>());
        // (`!(bottom > colTop)`: NaN returns too)
        if bottom.partial_cmp(&self.col_top) != Some(std::cmp::Ordering::Greater) {
            return Ok(());
        }
        let col_top = self.col_top;
        let mut add = Vec::new();
        for c in 1..cols.len() {
            if b.get(c).copied().flatten().is_none() {
                continue;
            }
            let (Some(p), Some(q)) = (cols.get(c - 1), cols.get(c)) else { continue };
            let x = (p.x + p.w + q.x) / 2.0;
            add.push(x);
        }
        if add.is_empty() {
            return Ok(());
        }
        let fill = cx.res.symbols.intern(&[0x30; 6]).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
        let ps = self.ps_mut(cur)?;
        for x in add {
            ps.body.list.push_plain(Item::Rect {
                x: x - COL_SEP_W / 2.0,
                y: col_top,
                w: COL_SEP_W,
                h: bottom - col_top,
                fill,
                alpha: OptF64::NONE,
            });
            ps.body.side.push(ItemSide::default());
        }
        Ok(())
    }

    /// `pageBorders(ps, pb)` (engine.ts:770): the section's page borders on this page.
    pub(crate) fn page_borders(
        &self,
        cx: &mut Cx<'_, '_>,
        ps: &PageState,
        g: &SectGeom,
        pb: &crate::model::PageBorders,
    ) -> LResult<PassItems> {
        // (WordCraft study, page, A16: Word's parity page is completely empty)
        if ps.blank {
            return Ok(PassItems::new());
        }
        let first = ps.sect_page == 0;
        let display = pb.display.as_deref();
        if (display == Some("firstPage") && !first) || (display == Some("notFirstPage") && first) {
            return Ok(PassItems::new());
        }
        // header/footer present = something drawn above/below the body
        let top_of = |src: &PassItems, i: usize| -> Option<f64> {
            match src.list.items.get(i)? {
                Item::Glyphs { y, .. } => Some(*y),
                Item::Rect { y, .. } => Some(*y),
                Item::Image(im) => Some(im.y),
                Item::Shape(sh) => Some(sh.y),
                Item::Line { y1, .. } => Some(*y1),
                _ => None,
            }
        };
        let mut hf: Vec<(&PassItems, usize)> = (0..ps.hf.len()).map(|i| (&ps.hf, i)).collect();
        for set in [&ps.hf_behind, &ps.hf_front] {
            let mut objs: Vec<&ZObj> = set.iter().collect();
            objs.sort_by(|a, b| {
                let d = a.z - b.z;
                let d = if uo_jsnum::conv::truthy(d) { d } else { a.seq - b.seq };
                d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            for o in objs {
                hf.extend((0..o.items.len()).map(|i| (&o.items, i)));
            }
        }
        let set = &cx.ldoc.view.formatting().settings;
        let header = set.borders_do_not_surround_header != Some(true)
            && hf.iter().any(|&(s, i)| top_of(s, i).unwrap_or(f64::INFINITY) < ps.body_top);
        let footer = set.borders_do_not_surround_footer != Some(true)
            && hf.iter().any(|&(s, i)| top_of(s, i).unwrap_or(f64::NEG_INFINITY) > ps.body_bottom);
        let geom = crate::paint::PageBorderGeom {
            page_w: g.page_w,
            page_h: g.page_h,
            mar_l: g.mar_l,
            mar_r: g.mar_r,
            mar_t: g.mar_t,
            mar_b: g.mar_b,
            hdr: g.hdr,
            ftr: g.ftr,
        };
        crate::paint::page_border_items(cx, pb, &geom, header, footer)
    }
}
