//! Paragraphs and runs of the body writer: the paragraphs of a story with their properties
//! (`collectParas`), the character runs (`segments`, `chpOf`), the run buffer, fields, hyperlinks,
//! bookmarks and comment markers, special characters.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use uo_text::space::is_space;

use super::fields::{HyperlinkField, hyperlink_args, is_form_field, is_shape_field, parse_field_args};
use super::{ChpEntry, DocxWriter, Kind, PartRels, Story, UNKNOWN_AUTHOR};
use crate::doc::DocError;
use crate::doc::fkp::run_index_from;
use crate::doc::formfields::ff_data_xml;
use crate::doc::numeric::int_of;
use crate::doc::ooxml_props::{PPrOpts, PropCtx, p_pr_inner, r_pr_inner};
use crate::doc::pictures::read_inline_picture;
use crate::doc::props::{Chp, ChpCtx, ChpKey, KeySet, Pap, PapCtx, PapKey, apply_chp, apply_pap, prune_none_borders};
use crate::doc::sprm::{Prl, prls, sgc_of};
use crate::doc::text::piece_at;
use crate::doc::xmlw::{AttrVal, Attrs, attrs, el_of, esc_text_units};
use uo_jsnum::math::ceil;

/// A paragraph collected from a story.
pub(super) struct PInfo<'a> {
    pub(super) cp_start: i64,
    /// exclusive; the mark is at `cp_end - 1`
    pub(super) cp_end: i64,
    pub(super) pap: Pap,
    pub(super) touched: KeySet<PapKey>,
    /// the table sprms (sgc 5) in effect, in order: the TAP of the row (the TTP mark)
    pub(super) tap_prls: Vec<Prl<'a>>,
    pub(super) depth: i64,
    pub(super) cell_end: bool,
    pub(super) row_end: bool,
}

/// The grpprls of a stretch of text: the CHPX run and the Prm of the piece, at most two.
#[derive(Clone, Copy)]
pub(super) struct Grpprls<'a> {
    items: [&'a [u8]; 2],
    len: usize,
}

impl<'a> Grpprls<'a> {
    fn new() -> Grpprls<'a> {
        Grpprls { items: [&[], &[]], len: 0 }
    }

    fn push(&mut self, g: &'a [u8]) {
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = g;
            self.len += 1;
        }
    }

    pub(super) fn as_slice(&self) -> &[&'a [u8]] {
        &self.items[..self.len]
    }
}

/// A stretch of text with the same direct character formatting.
pub(super) struct Segment<'a> {
    pub(super) cp_start: i64,
    pub(super) cp_end: i64,
    pub(super) grpprls: Grpprls<'a>,
}

/// The sprms of the left indent and the first-line indent (overridden by the level of the list in
/// `sprmPIlfo`): the index of the indent they set.
fn indent_slot(sprm: u16) -> Option<usize> {
    match sprm {
        0x840f | 0x845e | 0x4610 | 0x465f => Some(0),
        0x8411 | 0x8460 => Some(1),
        0x4456 => Some(2),
        0x4457 => Some(3),
        _ => None,
    }
}

const INDENT_KEYS: [PapKey; 4] = [PapKey::DxaLeft, PapKey::DxaLeft1, PapKey::DxcLeft, PapKey::DxcLeft1];

/// What the run buffer is collecting text for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextKind {
    T,
    Instr,
    Del,
    DelInstr,
}

/// Collects runs: characters in a row with the same `w:rPr` (and the same revision wrapper) share
/// one `w:r`.
pub(super) struct RunBuf {
    parts: String,
    cur_rpr: Option<String>,
    open_wrap: Vec<u16>,
    content: String,
    text_buf: Vec<u16>,
    text_kind: TextKind,
}

impl RunBuf {
    fn new() -> RunBuf {
        RunBuf {
            parts: String::new(),
            cur_rpr: None,
            open_wrap: Vec::new(),
            content: String::new(),
            text_buf: Vec::new(),
            text_kind: TextKind::T,
        }
    }

    fn flush_text(&mut self) {
        if self.text_buf.is_empty() {
            return;
        }
        let tag = match self.text_kind {
            TextKind::T => "w:t",
            TextKind::Instr => "w:instrText",
            TextKind::Del => "w:delText",
            TextKind::DelInstr => "w:delInstrText",
        };
        // /^\s|\s$/
        let space = self.text_buf.first().copied().is_some_and(is_space) || self.text_buf.last().copied().is_some_and(is_space);
        let sp = if space { " xml:space=\"preserve\"" } else { "" };
        self.content += &format!("<{tag}{sp}>{}</{tag}>", esc_text_units(&self.text_buf));
        self.text_buf.clear();
    }

    fn flush_run(&mut self) {
        self.flush_text();
        if let Some(rpr) = &self.cur_rpr
            && !self.content.is_empty()
        {
            self.parts += "<w:r>";
            if !rpr.is_empty() {
                self.parts += "<w:rPr>";
                self.parts += rpr;
                self.parts += "</w:rPr>";
            }
            self.parts += &self.content;
            self.parts += "</w:r>";
        }
        self.content.clear();
        self.cur_rpr = None;
    }

    fn set_wrap(&mut self, wrap: &[u16], next_id: &mut i64) {
        if wrap == self.open_wrap.as_slice() {
            return;
        }
        self.flush_run();
        if !self.open_wrap.is_empty() {
            let ins = self.open_wrap.starts_with(&[0x69, 0x6e, 0x73, 0x7c]);
            self.parts += if ins { "</w:ins>" } else { "</w:del>" };
        }
        self.open_wrap = wrap.to_vec();
        if !wrap.is_empty() {
            // `const [kind, author, date] = wrap.split('|')`
            let mut pieces = wrap.split(|&u| u == 0x7c);
            let kind: String = pieces.next().map(String::from_utf16_lossy).unwrap_or_default();
            let author = pieces.next();
            let date = pieces.next().filter(|d| !d.is_empty());
            let id = *next_id;
            *next_id += 1;
            let a = attrs(&Attrs::of(&[
                ("w:id", Some(AttrVal::Int(id))),
                ("w:author", author.map(AttrVal::Units)),
                ("w:date", date.map(AttrVal::Units)),
            ]));
            self.parts += &format!("<w:{kind}{a}>");
        }
    }

    fn begin(&mut self, rpr: &str, wrap: &[u16], next_id: &mut i64) {
        self.set_wrap(wrap, next_id);
        if self.cur_rpr.as_deref() != Some(rpr) {
            self.flush_run();
            self.cur_rpr = Some(rpr.to_owned());
        }
    }

    fn text(&mut self, rpr: &str, wrap: &[u16], c: u16, deleted: bool, next_id: &mut i64) {
        self.begin(rpr, wrap, next_id);
        let kind = if deleted { TextKind::Del } else { TextKind::T };
        if self.text_kind != kind {
            self.flush_text();
            self.text_kind = kind;
        }
        self.text_buf.push(c);
    }

    fn instr(&mut self, rpr: &str, wrap: &[u16], c: u16, deleted: bool, next_id: &mut i64) {
        self.begin(rpr, wrap, next_id);
        let kind = if deleted { TextKind::DelInstr } else { TextKind::Instr };
        if self.text_kind != kind {
            self.flush_text();
            self.text_kind = kind;
        }
        self.text_buf.push(c);
    }

    fn special(&mut self, rpr: &str, wrap: &[u16], xml: &str, next_id: &mut i64) {
        self.begin(rpr, wrap, next_id);
        self.flush_text();
        // a fldChar always has a run of its own (the Word requires it)
        if xml.starts_with("<w:fldChar") {
            self.flush_run();
            self.cur_rpr = Some(rpr.to_owned());
            self.content = xml.to_owned();
            self.flush_run();
            return;
        }
        self.content += xml;
        if xml.starts_with("<w:drawing")
            || xml.starts_with("<w:footnoteReference")
            || xml.starts_with("<w:endnoteReference")
            || xml.starts_with("<w:commentReference")
        {
            self.flush_run();
        }
    }

    fn raw(&mut self, xml: &str) {
        self.flush_run();
        self.parts += xml;
    }

    fn close(mut self, next_id: &mut i64) -> String {
        self.set_wrap(&[], next_id);
        self.flush_run();
        self.parts
    }
}

/// `dttmIso`: a DTTM as an ISO 8601 date.
pub(super) fn dttm_iso(v: Option<u32>) -> Option<String> {
    let v = v.filter(|&v| v != 0)?;
    let mint = v & 0x3f;
    let hr = (v >> 6) & 0x1f;
    let dom = (v >> 11) & 0x1f;
    let mon = (v >> 16) & 0xf;
    let yr = ((v >> 20) & 0x1ff) + 1900;
    if dom == 0 || mon == 0 {
        return None;
    }
    Some(format!("{yr}-{mon:02}-{dom:02}T{hr:02}:{mint:02}:00Z"))
}

impl<'a> DocxWriter<'a> {
    // ───────────────────────────── property emitters ─────────────────────────────

    /// `rPrInner(chp, touched, propCtx)`.
    pub(super) fn r_pr(&self, chp: &Chp, touched: &KeySet<ChpKey>) -> String {
        let ids = &self.style_ids;
        let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
        let ctx = PropCtx { fonts: &self.r.fonts, style_id: &sid };
        r_pr_inner(chp, touched, &ctx, true)
    }

    /// `pPrInner(pap, touched, propCtx, opts)` with `numId: numIdOf`.
    pub(super) fn p_pr(
        &self,
        pap: &Pap,
        touched: &KeySet<PapKey>,
        base_tabs: Option<&[crate::doc::props::TabStop]>,
        r_pr: Option<&str>,
        sect_pr: Option<&str>,
    ) -> String {
        let ids = &self.style_ids;
        let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
        let ctx = PropCtx { fonts: &self.r.fonts, style_id: &sid };
        let lfo_count = self.lfo_count;
        let num = move |n: u32| DocxWriter::num_id_of(lfo_count, n);
        p_pr_inner(pap, touched, &ctx, &PPrOpts { base_tabs, num_id: Some(&num), r_pr, sect_pr, with_style: true })
    }

    // ───────────────────────────── paragraphs ─────────────────────────────

    fn valid_para_style(&self, istd: u16) -> u16 {
        match self.r.styles.style(istd) {
            Some(st) if st.stk == 1 => istd,
            _ => 0,
        }
    }

    /// `prcData`: the grpprl at an offset of the `Data` stream (`cb` is an unsigned 16-bit count).
    fn prc_data(&self, off: u32) -> &'a [u8] {
        let Some(d) = self.r.data else { return &[] };
        let off = u64::from(off);
        if off + 2 > d.len() as u64 {
            return &[];
        }
        let r = crate::doc::bytes::ByteReader::new(d);
        let cb = u64::from(r.u16(off));
        r.sub(off + 2, (d.len() as u64).min(off + 2 + cb))
    }

    /// `collectParas`: the paragraphs of a range of CPs.
    pub(super) fn collect_paras(&mut self, cp_start: i64, cp_end: i64) -> Result<Vec<PInfo<'a>>, DocError> {
        let mut out = Vec::new();
        let mut cp = cp_start;
        let mut guard = 0u32;
        while cp < cp_end && guard < 2_000_000 {
            guard += 1;
            self.charge(1)?;
            let pa = self.r.paragraph_at(cp, cp_end);
            let p_end = (cp + 1).max(pa.end.min(cp_end));
            let mark_cp = p_end - 1;
            let mut grpprls: Vec<&'a [u8]> = Vec::with_capacity(2);
            if let Some(run) = &pa.run
                && !run.grpprl.is_empty()
            {
                grpprls.push(run.grpprl);
            }
            let prm = piece_at(&self.r.pt.pieces, mark_cp).and_then(|i| self.r.prm_grpprl(&self.r.pt.pieces[i]));
            if let Some(prm) = prm {
                grpprls.push(prm);
            }
            let istd = pa.run.map_or(0, |r| r.istd);
            let valid = self.valid_para_style(istd);
            let base = self.r.styles.resolve(valid);
            let mut pap = base.pap.clone();
            pap.istd = valid;
            pap.in_table = None;
            pap.ttp = None;
            pap.itap = None;
            pap.inner_table_cell = None;
            pap.inner_ttp = None;
            pap.tabs_cleared = Some(Vec::new());
            let mut touched: KeySet<PapKey> = KeySet::new();
            // sprmPIlfo (positive) applies the indent of the level of the list over the indent set
            // BEFORE it; only indents written after it survive as direct ones
            let mut seq = 0u32;
            let mut ilfo_seq = 0u32;
            let mut last_indent = [0u32; 4];
            let mut apply = |pap: &mut Pap, touched: &mut KeySet<PapKey>, p: &Prl<'a>| {
                seq += 1;
                if let Some(slot) = indent_slot(p.sprm) {
                    last_indent[slot] = seq;
                }
                if p.sprm == 0x460b {
                    ilfo_seq = if p.r.i16(p.op) > 0 { seq } else { 0 };
                }
                apply_pap(pap, p, touched, &mut PapCtx::default());
            };
            let mut tap_prls: Vec<Prl<'a>> = Vec::new();
            for g in &grpprls {
                for p in prls(g) {
                    let sgc = sgc_of(p.sprm);
                    if sgc == 5 {
                        tap_prls.push(p);
                        continue;
                    }
                    if sgc != 1 {
                        continue;
                    }
                    if (p.sprm == 0x646b || p.sprm == 0x6646) && self.r.data.is_some() {
                        // sprmPTableProps/sprmPHugePapx: a PrcData in the Data stream. Whoever
                        // processes it does NOT process the Prls after it in the same grpprl
                        // (versions for older readers)
                        let extra = self.prc_data(p.u32());
                        if !extra.is_empty() {
                            for q in prls(extra) {
                                let s2 = sgc_of(q.sprm);
                                if s2 == 5 {
                                    tap_prls.push(q);
                                } else if s2 == 1 {
                                    apply(&mut pap, &mut touched, &q);
                                }
                            }
                            break;
                        }
                        continue;
                    }
                    apply(&mut pap, &mut touched, &p);
                }
            }
            prune_none_borders(&pap, &base.pap, &mut touched);
            if ilfo_seq > 0 {
                for (slot, k) in INDENT_KEYS.into_iter().enumerate() {
                    if touched.has(k) && last_indent[slot] < ilfo_seq {
                        touched.delete(k);
                        pap.copy_key(k, &base.pap);
                    }
                }
            }
            if pap.istd != istd {
                pap.istd = self.valid_para_style(pap.istd);
            }
            let mark = self.r.char_at(mark_cp);
            let mut depth = pap.itap.unwrap_or(0);
            let in_table = pap.in_table == Some(true);
            if in_table && depth < 1 {
                depth = 1;
            }
            if !in_table && pap.itap.is_none() {
                depth = 0;
            }
            let mut cell_end = false;
            let mut row_end = false;
            if depth == 1 {
                if mark == Some(0x07) {
                    if pap.ttp == Some(true) {
                        row_end = true;
                    } else {
                        cell_end = true;
                    }
                }
            } else if depth > 1 {
                if pap.inner_ttp == Some(true) {
                    row_end = true;
                } else if pap.inner_table_cell == Some(true) || mark == Some(0x07) {
                    cell_end = true;
                }
            }
            out.push(PInfo { cp_start: cp, cp_end: p_end, pap, touched, tap_prls, depth, cell_end, row_end });
            cp = p_end;
        }
        Ok(out)
    }

    /// `renderParagraph`.
    pub(super) fn render_paragraph(
        &mut self,
        p: &PInfo<'a>,
        ctx: Story,
        rels: &mut PartRels,
        sect_pr: Option<&str>,
    ) -> Result<String, DocError> {
        let style_res = self.r.styles.resolve(p.pap.istd);
        let mark_chp = self.chp_for(p, p.cp_end - 1);
        let mut mark_rpr = self.revision_mark(&mark_chp.chp);
        mark_rpr += &self.r_pr(&mark_chp.chp, &mark_chp.touched);
        let p_pr =
            self.p_pr(&p.pap, &p.touched, style_res.pap.tabs.as_deref(), (!mark_rpr.is_empty()).then_some(mark_rpr.as_str()), sect_pr);
        let runs = self.render_runs(p, ctx, rels)?;
        Ok(if p_pr.is_empty() { format!("<w:p>{runs}</w:p>") } else { format!("<w:p><w:pPr>{p_pr}</w:pPr>{runs}</w:p>") })
    }

    /// The author of a revision (`revAuthors[ibst] ?? 'Autor desconhecido'`), as UTF-16.
    fn rev_author(&self, ibst: Option<u16>) -> Vec<u16> {
        match self.rev_authors.get(usize::from(ibst.unwrap_or(0))) {
            Some(a) => a.units().to_vec(),
            None => UNKNOWN_AUTHOR.encode_utf16().collect(),
        }
    }

    /// `revisionMark`: the `w:ins`/`w:del` that precedes the paragraph mark's properties.
    fn revision_mark(&mut self, chp: &Chp) -> String {
        let (tag, ibst, dttm) = if chp.rmark_ins == Some(true) {
            ("w:ins", chp.ibst_rmark, chp.dttm_rmark)
        } else if chp.rmark_del == Some(true) {
            ("w:del", chp.ibst_rmark_del, chp.dttm_rmark_del)
        } else {
            return String::new();
        };
        let author = self.rev_author(ibst);
        let id = self.rev_next;
        self.rev_next += 1;
        let date = dttm_iso(dttm);
        el_of(
            tag,
            &[("w:id", Some(AttrVal::Int(id))), ("w:author", Some(AttrVal::Units(&author))), ("w:date", date.as_deref().map(AttrVal::Str))],
            "",
        )
    }

    /// `wrapKey`: the key of the revision wrapper of a run (`ins|author|date`), empty for none.
    fn wrap_key(&self, chp: &Chp) -> Vec<u16> {
        let (kind, ibst, dttm) = if chp.rmark_ins == Some(true) {
            ("ins", chp.ibst_rmark, chp.dttm_rmark)
        } else if chp.rmark_del == Some(true) {
            ("del", chp.ibst_rmark_del, chp.dttm_rmark_del)
        } else {
            return Vec::new();
        };
        let mut key: Vec<u16> = kind.encode_utf16().collect();
        key.push(0x7c);
        key.extend(self.rev_author(ibst));
        key.push(0x7c);
        key.extend(dttm_iso(dttm).unwrap_or_default().encode_utf16());
        key
    }

    /// `chpFor`: the CHP and the touched properties of the character at `cp` (inside the paragraph).
    pub(super) fn chp_for(&mut self, p: &PInfo<'a>, cp: i64) -> Arc<ChpEntry> {
        let seg = self.segment(cp, cp + 1);
        self.chp_of(p.pap.istd, seg.grpprls.as_slice())
    }

    /// `chpOf`: the CHP of a paragraph style with direct character formatting applied.
    pub(super) fn chp_of(&mut self, istd: u16, grpprls: &[&'a [u8]]) -> Arc<ChpEntry> {
        // the TS keys the cache by the identity of the grpprl arrays; a slice is its address and length
        let key = (istd, grpprls.iter().map(|g| (g.as_ptr().addr(), g.len())).collect::<Vec<_>>());
        if let Some(hit) = self.chp_cache.get(&key) {
            return Arc::clone(hit);
        }
        let para_chp = self.r.styles.resolve(istd).chp.clone();
        let mut chp = para_chp.clone();
        let mut touched = KeySet::new();
        {
            let styles = &mut self.r.styles;
            let mut apply_char_style = |i: u16| styles.apply_char_style(i, &para_chp);
            let mut ctx = ChpCtx { base: para_chp.clone(), apply_char_style: Some(&mut apply_char_style), para_style: Some(&para_chp) };
            for g in grpprls {
                for q in prls(g) {
                    if sgc_of(q.sprm) == 2 {
                        apply_chp(&mut chp, &q, &mut ctx, &mut touched);
                    }
                }
            }
        }
        let res = Arc::new(ChpEntry { chp, touched });
        self.chp_cache.insert(key, Arc::clone(&res));
        res
    }

    /// One step of `segments(s, e)` (the generator): the stretch of CHPX at `cp`.
    pub(super) fn segment(&self, cp: i64, e: i64) -> Segment<'a> {
        let pieces = &self.r.pt.pieces;
        let Some(i) = piece_at(pieces, cp) else { return Segment { cp_start: cp, cp_end: e, grpprls: Grpprls::new() } };
        let p = &pieces[i];
        let cs = if p.compressed { 1.0 } else { 2.0 };
        let cp_start = i64::from(p.cp_start);
        let fc = p.fc + (cp - cp_start) as f64 * cs;
        let piece_end = i64::from(p.cp_end).min(e);
        let mut seg_end = piece_end;
        let mut grpprls = Grpprls::new();
        let idx = run_index_from(&self.r.chpx, fc);
        match self.r.chpx.get(idx) {
            Some(run) if f64::from(run.fc_start) <= fc => {
                seg_end = piece_end.min(cp_start + int_of(ceil((f64::from(run.fc_end) - p.fc) / cs)));
                if !run.grpprl.is_empty() {
                    grpprls.push(run.grpprl);
                }
            }
            Some(run) => {
                seg_end = piece_end.min((cp + 1).max(cp_start + int_of(ceil((f64::from(run.fc_start) - p.fc) / cs))));
            }
            None => {}
        }
        if let Some(prm) = self.r.prm_grpprl(p) {
            grpprls.push(prm);
        }
        if seg_end <= cp {
            seg_end = cp + 1;
        }
        Segment { cp_start: cp, cp_end: seg_end, grpprls }
    }

    // ───────────────────────────── runs ─────────────────────────────

    /// `renderRuns`.
    fn render_runs(&mut self, p: &PInfo<'a>, ctx: Story, rels: &mut PartRels) -> Result<String, DocError> {
        let mut out = RunBuf::new();
        let mark_cp = p.cp_end - 1;
        self.charge(u64::try_from(p.cp_end - p.cp_start).unwrap_or(0))?;
        // a simple HYPERLINK (inside the paragraph) becomes w:hyperlink, as the Word writes it
        let links = self.scan_hyperlinks(p, rels);
        let mut link: Option<HyperlinkField> = None;
        let mut seg_cp = p.cp_start;
        while seg_cp < p.cp_end {
            let seg = self.segment(seg_cp, p.cp_end);
            let entry = self.chp_of(p.pap.istd, seg.grpprls.as_slice());
            let chp = &entry.chp;
            let r_pr = self.r_pr(chp, &entry.touched);
            let wrap = self.wrap_key(chp);
            for cp in seg.cp_start..seg.cp_end {
                self.emit_markers(cp, &mut out, ctx, false);
                if cp == mark_cp {
                    continue;
                }
                if !self.skip_ranges.is_empty() && self.skip_ranges.iter().any(|&(a, b)| cp >= a && cp < b) {
                    continue;
                }
                let c = self.r.char_at(cp);
                // the result of a field nested INSIDE the instruction of another is still an instruction
                // (the Word writes w:instrText): any open field in its instruction counts
                let in_instr = self.field_stack.iter().any(|&f| f);
                if let Some(l) = &link {
                    if cp <= l.sep {
                        continue;
                    }
                    if cp == l.end {
                        if !l.unwrap {
                            out.raw("</w:hyperlink>");
                        }
                        link = None;
                        continue;
                    }
                } else if c == Some(0x13)
                    && let Some(l) = links.get(&cp)
                {
                    if !l.unwrap {
                        let a = attrs(&Attrs::of(&[
                            ("r:id", l.rid.as_deref().map(AttrVal::Str)),
                            ("w:anchor", l.anchor.as_deref().map(AttrVal::Units)),
                            ("w:tooltip", l.tooltip.as_deref().map(AttrVal::Units)),
                            ("w:tgtFrame", l.target.as_deref().map(AttrVal::Units)),
                            ("w:history", Some(AttrVal::Str("1"))),
                        ]));
                        out.raw(&format!("<w:hyperlink{a}>"));
                    }
                    link = Some(l.clone());
                    continue;
                }
                if c == Some(0x13) {
                    self.field_stack.push(true);
                    let ff = self.form_field_data(p, cp)?;
                    let begin = match ff {
                        Some(ff) => format!("<w:fldChar w:fldCharType=\"begin\">{ff}</w:fldChar>"),
                        None => "<w:fldChar w:fldCharType=\"begin\"/>".to_owned(),
                    };
                    out.special(&r_pr, &wrap, &begin, &mut self.rev_next);
                    continue;
                }
                if c == Some(0x14) {
                    if let Some(top) = self.field_stack.last_mut() {
                        *top = false;
                    }
                    out.special(&r_pr, &wrap, "<w:fldChar w:fldCharType=\"separate\"/>", &mut self.rev_next);
                    continue;
                }
                if c == Some(0x15) {
                    self.field_stack.pop();
                    out.special(&r_pr, &wrap, "<w:fldChar w:fldCharType=\"end\"/>", &mut self.rev_next);
                    continue;
                }
                let rmark_del = chp.rmark_del == Some(true);
                let f_spec = chp.f_spec == Some(true);
                if in_instr {
                    if let Some(c) = c
                        && (c >= 0x20 || c == 0x09)
                    {
                        out.instr(&r_pr, &wrap, c, rmark_del, &mut self.rev_next);
                    }
                    continue;
                }
                // a note with a custom mark: the first character of the mark carries the reference
                if ctx.kind == Kind::Main
                    && !f_spec
                    && let Some(c) = c
                    && c >= 0x20
                    && (self.footnote_ids.contains_key(&cp) || self.endnote_ids.contains_key(&cp))
                {
                    let reference = match self.footnote_ids.get(&cp) {
                        Some(&f) => el_of(
                            "w:footnoteReference",
                            &[("w:customMarkFollows", Some(AttrVal::Str("1"))), ("w:id", Some(AttrVal::Int(i64::from(f))))],
                            "",
                        ),
                        None => el_of(
                            "w:endnoteReference",
                            &[
                                ("w:customMarkFollows", Some(AttrVal::Str("1"))),
                                ("w:id", self.endnote_ids.get(&cp).map(|&e| AttrVal::Int(i64::from(e)))),
                            ],
                            "",
                        ),
                    };
                    let x = format!("{reference}<w:t>{}</w:t>", esc_text_units(&[c]));
                    out.special(&r_pr, &wrap, &x, &mut self.rev_next);
                    continue;
                }
                if let Some(cu) = c
                    && (f_spec || cu < 0x20)
                    && let Some(x) = self.special_char(cu, cp, chp, ctx, rels)?
                {
                    if !x.is_empty() {
                        out.special(&r_pr, &wrap, &x, &mut self.rev_next);
                    }
                    continue;
                }
                if let Some(sym) = &chp.symbol
                    && (f_spec || c == Some(0x28))
                {
                    let font = self.r.fonts.get(usize::from(sym.ftc)).map(|f| f.name.units());
                    let ch = format!("{:04X}", sym.xchar);
                    let x = el_of("w:sym", &[("w:font", font.map(AttrVal::Units)), ("w:char", Some(AttrVal::Str(&ch)))], "");
                    out.special(&r_pr, &wrap, &x, &mut self.rev_next);
                    continue;
                }
                if c.is_some_and(|c| c < 0x20) {
                    continue;
                }
                out.text(&r_pr, &wrap, c.unwrap_or(0), rmark_del, &mut self.rev_next);
            }
            seg_cp = seg.cp_end;
        }
        if let Some(l) = &link
            && !l.unwrap
        {
            out.raw("</w:hyperlink>");
        }
        self.emit_markers(p.cp_end, &mut out, ctx, true);
        Ok(out.close(&mut self.rev_next))
    }

    /// `formFieldData`: the FFData of the form field that begins at `begin` (a 0x01 with `sprmCFData`
    /// in the instruction).
    fn form_field_data(&mut self, p: &PInfo<'a>, begin: i64) -> Result<Option<String>, DocError> {
        self.charge(u64::try_from(p.cp_end - begin).unwrap_or(0))?;
        let mut depth = 0u32;
        let mut instr: Vec<u16> = Vec::new();
        let mut cp = begin + 1;
        while cp < p.cp_end - 1 {
            let c = self.r.char_at(cp);
            if c == Some(0x13) {
                depth += 1;
            } else if c == Some(0x15) {
                if depth == 0 {
                    return Ok(None);
                }
                depth -= 1;
            } else if c == Some(0x14) && depth == 0 {
                return Ok(None);
            } else if c == Some(0x01) && depth == 0 {
                // only form fields: other fields carry binary data too (0x01 + fData)
                if !is_form_field(&instr) {
                    return Ok(None);
                }
                let entry = self.chp_for(p, cp);
                if entry.chp.f_data == Some(true)
                    && let Some(loc) = entry.chp.pic_location
                {
                    return Ok(ff_data_xml(self.r.data, loc));
                }
            } else if depth == 0
                && let Some(c) = c
                && c >= 0x20
            {
                instr.push(c);
            }
            cp += 1;
        }
        Ok(None)
    }

    /// `text.indexOf('\x13', from)`, remembering where the last search stopped.
    fn next_field_begin(&mut self, from: i64) -> Option<i64> {
        let (searched, found) = self.next_begin;
        if from >= searched {
            match found {
                None if searched != i64::MAX => return None,
                Some(f) if f >= from => return Some(f),
                _ => {}
            }
        }
        let start = usize::try_from(from.max(0)).unwrap_or(usize::MAX);
        let hit = self.r.text.get(start..).and_then(|t| t.iter().position(|&u| u == 0x13)).and_then(|k| i64::try_from(start + k).ok());
        self.next_begin = (from, hit);
        hit
    }

    /// `scanHyperlinks`: the HYPERLINK fields of the top level that lie entirely inside the paragraph
    /// and have no field nested in their instruction: begin (CP) → separator, end and target.
    fn scan_hyperlinks(&mut self, p: &PInfo<'a>, rels: &mut PartRels) -> HashMap<i64, HyperlinkField> {
        let mut out = HashMap::new();
        match self.next_field_begin(p.cp_start) {
            Some(i) if i < p.cp_end => {}
            _ => return out,
        }
        // only outside the fields already open (they cross paragraphs)
        if !self.field_stack.is_empty() {
            return out;
        }
        struct Open {
            begin: i64,
            sep: Option<i64>,
            nested: bool,
        }
        let mut stack: Vec<Open> = Vec::new();
        let mut cp = p.cp_start;
        while cp < p.cp_end - 1 {
            let c = self.r.char_at(cp);
            if c == Some(0x13) {
                if let Some(top) = stack.last_mut() {
                    top.nested = top.sep.is_none() || top.nested;
                }
                stack.push(Open { begin: cp, sep: None, nested: false });
            } else if c == Some(0x14) && !stack.is_empty() {
                if let Some(top) = stack.last_mut()
                    && top.sep.is_none()
                {
                    top.sep = Some(cp);
                }
            } else if c == Some(0x15)
                && let Some(f) = stack.pop()
            {
                cp += 1;
                let Some(sep) = f.sep else { continue };
                if !stack.is_empty() || f.nested {
                    continue;
                }
                let lo = usize::try_from(f.begin + 1).unwrap_or(usize::MAX);
                let hi = usize::try_from(sep).unwrap_or(usize::MAX);
                let instr: &[u16] = self.r.text.get(lo..hi.min(self.r.text.len())).unwrap_or(&[]);
                // SHAPE (a drawing canvas or an inline shape): the Word drops the field and keeps the drawing
                if is_shape_field(instr) {
                    out.insert(
                        f.begin,
                        HyperlinkField { sep, end: cp - 1, rid: None, anchor: None, tooltip: None, target: None, unwrap: true },
                    );
                    continue;
                }
                let Some(rest) = hyperlink_args(instr) else { continue };
                let args = parse_field_args(rest);
                let mut url: Option<Vec<u16>> = None;
                let mut anchor: Option<Vec<u16>> = None;
                let mut tooltip: Option<Vec<u16>> = None;
                let mut target: Option<Vec<u16>> = None;
                let mut i = 0;
                while i < args.len() {
                    let a = &args[i];
                    if a.as_slice() == [0x5c, 0x6c] {
                        i += 1;
                        anchor = args.get(i).cloned();
                    } else if a.as_slice() == [0x5c, 0x6f] {
                        i += 1;
                        tooltip = args.get(i).cloned();
                    } else if a.as_slice() == [0x5c, 0x74] {
                        i += 1;
                        target = args.get(i).cloned();
                    } else if a.first() == Some(&0x5c) {
                        // \m and \n (and any other switch): nothing to keep
                    } else if url.is_none() {
                        url = Some(a.clone());
                    }
                    i += 1;
                }
                let has_url = url.as_ref().is_some_and(|u| !u.is_empty());
                let has_anchor = anchor.as_ref().is_some_and(|a| !a.is_empty());
                if !has_url && !has_anchor {
                    continue;
                }
                let rid = if has_url { url.as_ref().map(|u| rels.add("hyperlink", u, true)) } else { None };
                out.insert(f.begin, HyperlinkField { sep, end: cp - 1, rid, anchor, tooltip, target, unwrap: false });
                continue;
            }
            cp += 1;
        }
        out
    }

    /// `emitMarkers`: the bookmarks and comment ranges that start or end at this CP.
    fn emit_markers(&mut self, cp: i64, out: &mut RunBuf, ctx: Story, end_only: bool) {
        // bookmarks: absolute CPs (they apply in any story); comments only in the body
        if let Some(ends) = self.bk_ends.remove(&cp) {
            for id in ends {
                out.raw(&el_of("w:bookmarkEnd", &[("w:id", Some(AttrVal::Int(id)))], ""));
            }
        }
        if ctx.kind == Kind::Main
            && let Some(ends) = self.comment_ends.remove(&cp)
        {
            for id in ends {
                out.raw(&el_of("w:commentRangeEnd", &[("w:id", Some(AttrVal::Int(i64::try_from(id).unwrap_or(i64::MAX))))], ""));
            }
        }
        if end_only {
            return;
        }
        if let Some(starts) = self.bk_starts.remove(&cp) {
            for (id, name) in starts {
                out.raw(&el_of("w:bookmarkStart", &[("w:id", Some(AttrVal::Int(id))), ("w:name", Some(AttrVal::Units(name.units())))], ""));
            }
        }
        if ctx.kind == Kind::Main
            && let Some(starts) = self.comment_starts.remove(&cp)
        {
            for id in starts {
                out.raw(&el_of("w:commentRangeStart", &[("w:id", Some(AttrVal::Int(i64::try_from(id).unwrap_or(i64::MAX))))], ""));
            }
        }
    }

    /// `specialChar`: a special character as the XML of its run's content (`""` drops it), or `None`
    /// for plain text.
    fn special_char(&mut self, c: u16, cp: i64, chp: &Chp, ctx: Story, rels: &mut PartRels) -> Result<Option<String>, DocError> {
        match c {
            0x09 => return Ok(Some("<w:tab/>".into())),
            0x0b => return Ok(Some("<w:br/>".into())),
            0x0c => return Ok(Some("<w:br w:type=\"page\"/>".into())),
            0x0e => return Ok(Some("<w:br w:type=\"column\"/>".into())),
            0x1e => return Ok(Some("<w:noBreakHyphen/>".into())),
            0x1f => return Ok(Some("<w:softHyphen/>".into())),
            // a 0x0D that is not a paragraph mark (the Word writes w:cr)
            0x0d => return Ok(Some("<w:cr/>".into())),
            // a stray cell mark
            0x07 => return Ok(Some(String::new())),
            _ => {}
        }
        if chp.f_spec != Some(true) {
            return Ok(if c < 0x20 { Some(String::new()) } else { None });
        }
        let note = ctx.kind == Kind::Ftn || ctx.kind == Kind::Edn;
        match c {
            0x01 => {
                if chp.f_data == Some(true) {
                    return Ok(Some(String::new()));
                }
                let Some(loc) = chp.pic_location else { return Ok(Some(String::new())) };
                if chp.f_ole2 == Some(true) {
                    return Ok(Some(String::new()));
                }
                let Some(pic) = read_inline_picture(&self.r, loc) else { return Ok(Some(String::new())) };
                if pic.blip.is_none()
                    && let Some(hr) = &pic.hr
                {
                    return Ok(Some(super::drawing::hr_vml(&pic, hr)));
                }
                Ok(Some(self.inline_drawing(pic, rels)))
            }
            0x02 => {
                if ctx.kind == Kind::Ftn {
                    return Ok(Some("<w:footnoteRef/>".into()));
                }
                if ctx.kind == Kind::Edn {
                    return Ok(Some("<w:endnoteRef/>".into()));
                }
                if let Some(&f) = self.footnote_ids.get(&cp) {
                    return Ok(Some(el_of("w:footnoteReference", &[("w:id", Some(AttrVal::Int(i64::from(f))))], "")));
                }
                if let Some(&e) = self.endnote_ids.get(&cp) {
                    return Ok(Some(el_of("w:endnoteReference", &[("w:id", Some(AttrVal::Int(i64::from(e))))], "")));
                }
                Ok(Some(String::new()))
            }
            0x05 => {
                if ctx.kind == Kind::Atn {
                    return Ok(Some("<w:annotationRef/>".into()));
                }
                Ok(Some(match self.comment_by_ref.get(&cp) {
                    Some(&i) => el_of("w:commentReference", &[("w:id", Some(AttrVal::Int(i64::try_from(i).unwrap_or(i64::MAX))))], ""),
                    None => String::new(),
                }))
            }
            0x08 => {
                let f = if ctx.hdr { self.fspa_hdr.get(&cp) } else { self.fspa_main.get(&cp) }.copied();
                match f {
                    Some(f) => Ok(Some(self.anchor_drawing(&f, ctx, rels)?)),
                    None => Ok(Some(String::new())),
                }
            }
            0x03 => Ok(Some(if note { "<w:separator/>".into() } else { String::new() })),
            0x04 => Ok(Some(if note { "<w:continuationSeparator/>".into() } else { String::new() })),
            _ => Ok(if c < 0x20 { Some(String::new()) } else { None }),
        }
    }
}
