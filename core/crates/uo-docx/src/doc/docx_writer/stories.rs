//! The stories of the document: the body with its sections (`w:sectPr`), headers and footers,
//! footnotes and endnotes, comments.

use std::collections::HashMap;

use super::body::PInfo;
use super::{DocxWriter, FileData, Kind, PartRels, Story, UNKNOWN_AUTHOR};
use crate::doc::DocError;
use crate::doc::ooxml_props::{brc_xml, num_fmt_of};
use crate::doc::props::Sep;
use crate::doc::reader::{Comment, Note, NoteKind};
use crate::doc::xmlw::{AttrVal, XML_DECL, content_ns, el_of};

/// The `w:sectPr` of each section and the paragraph (by its end CP) that closes it.
pub(super) struct SectMap<'s> {
    ends: HashMap<i64, usize>,
    prs: &'s [String],
}

impl SectMap<'_> {
    /// `sectPrOf(p)`.
    pub(super) fn of(&self, p: &PInfo<'_>) -> Option<&str> {
        self.ends.get(&p.cp_end).map(|&si| self.prs[si].as_str())
    }
}

fn int(v: i64) -> Option<AttrVal<'static>> {
    Some(AttrVal::Int(v))
}

/// `|| undefined` of a number: zero is not written.
fn nonzero(v: i64) -> Option<AttrVal<'static>> {
    (v != 0).then_some(AttrVal::Int(v))
}

/// `numFmtOf` of a number that may be wider than the table's keys.
fn num_fmt(nfc: u16) -> &'static str {
    u8::try_from(nfc).map_or("decimal", num_fmt_of)
}

/// `notePr`: the properties of the footnotes or endnotes of a section.
fn note_pr(pos: Option<&str>, nfc: Option<u16>, start: Option<u16>, rnc: Option<u8>, endnote: bool) -> String {
    let mut x = String::new();
    if let Some(pos) = pos
        && pos != "pageBottom"
    {
        x += &el_of("w:pos", &[("w:val", Some(AttrVal::Str(pos)))], "");
    }
    if let Some(nfc) = nfc
        && nfc != if endnote { 2 } else { 0 }
    {
        x += &el_of("w:numFmt", &[("w:val", Some(AttrVal::Str(num_fmt(nfc))))], "");
    }
    if let Some(start) = start
        && start > 1
    {
        x += &el_of("w:numStart", &[("w:val", int(i64::from(start)))], "");
    }
    if let Some(rnc) = rnc
        && rnc != 0
    {
        let v = ["continuous", "eachSect", "eachPage"].get(usize::from(rnc)).copied().unwrap_or("continuous");
        x += &el_of("w:numRestart", &[("w:val", Some(AttrVal::Str(v)))], "");
    }
    x
}

impl<'a> DocxWriter<'a> {
    /// `renderMain`: the body.
    pub(super) fn render_main(&mut self, rels: &mut PartRels) -> Result<String, DocError> {
        let ctx = Story { kind: Kind::Main, hdr: false };
        let (s, e) = self.r.sub.main;
        let mut paras = self.collect_paras(s, e)?;
        // marks the end of each section (but the last, which goes in w:body)
        let n = self.r.sections.len();
        let mut ends: HashMap<i64, usize> = HashMap::new();
        for (i, sec) in self.r.sections.iter().enumerate() {
            if i + 1 < n {
                ends.insert(sec.cp_end, i);
            }
        }
        let hdr_stories = self.r.header_stories();
        let mut sect_prs: Vec<String> = Vec::new();
        for i in 0..n {
            let sep = self.r.sections[i].sep.clone();
            let x = self.sect_pr_xml(&sep, i, &hdr_stories, rels)?;
            sect_prs.push(x);
        }
        self.field_stack.clear();
        let map = SectMap { ends, prs: &sect_prs };
        let len = paras.len();
        let body = self.render_blocks(&mut paras, 0, len, 0, ctx, rels, Some(&map))?;
        Ok(body + sect_prs.last().map_or("", String::as_str))
    }

    /// `sectPrXml`.
    fn sect_pr_xml(
        &mut self,
        sep: &Sep,
        index: usize,
        stories: &[Option<(i64, i64)>],
        doc_rels: &mut PartRels,
    ) -> Result<String, DocError> {
        // the order of the stories in each section: even header, odd header, even footer, odd footer, first header, first footer
        const KINDS: [(usize, bool, &str); 6] =
            [(0, true, "even"), (1, true, "default"), (4, true, "first"), (2, false, "even"), (3, false, "default"), (5, false, "first")];
        let mut hdr_refs = String::new();
        let mut ftr_refs = String::new();
        for (k, header, ty) in KINDS {
            let Some(Some(st)) = stories.get(6 + index * 6 + k).copied() else { continue };
            let rid = self.render_header_part(st, header, doc_rels)?;
            let x = el_of(
                if header { "w:headerReference" } else { "w:footerReference" },
                &[("w:type", Some(AttrVal::Str(ty))), ("r:id", Some(AttrVal::Str(&rid)))],
                "",
            );
            if header {
                hdr_refs += &x;
            } else {
                ftr_refs += &x;
            }
        }
        let mut x = hdr_refs + &ftr_refs;
        let ftn_pos = sep.fpc.and_then(|f| ["pageBottom", "pageBottom", "beneathText", "sectEnd"].get(usize::from(f)).copied());
        let ftn_pr = note_pr(ftn_pos, sep.nfc_ftn_ref, sep.n_ftn, sep.rnc_ftn, false);
        if !ftn_pr.is_empty() {
            x += &format!("<w:footnotePr>{ftn_pr}</w:footnotePr>");
        }
        let edn_pr = note_pr(None, sep.nfc_edn_ref, sep.n_edn, sep.rnc_edn, true);
        if !edn_pr.is_empty() {
            x += &format!("<w:endnotePr>{edn_pr}</w:endnotePr>");
        }
        let bkc = ["continuous", "nextColumn", "nextPage", "evenPage", "oddPage"].get(usize::from(sep.bkc)).copied().unwrap_or("nextPage");
        if bkc != "nextPage" {
            x += &el_of("w:type", &[("w:val", Some(AttrVal::Str(bkc)))], "");
        }
        x += &el_of(
            "w:pgSz",
            &[
                ("w:w", int(i64::from(sep.xa_page))),
                ("w:h", int(i64::from(sep.ya_page))),
                ("w:orient", (sep.orientation == 2).then_some(AttrVal::Str("landscape"))),
                ("w:code", sep.dm_paper_req.and_then(|c| nonzero(i64::from(c)))),
            ],
            "",
        );
        x += &el_of(
            "w:pgMar",
            &[
                ("w:top", int(i64::from(sep.dya_top))),
                ("w:right", int(i64::from(sep.dxa_right))),
                ("w:bottom", int(i64::from(sep.dya_bottom))),
                ("w:left", int(i64::from(sep.dxa_left))),
                ("w:header", int(i64::from(sep.dya_hdr_top))),
                ("w:footer", int(i64::from(sep.dya_hdr_bottom))),
                ("w:gutter", int(i64::from(sep.dza_gutter))),
            ],
            "",
        );
        if sep.dm_bin_first != 0 || sep.dm_bin_other != 0 {
            x += &el_of(
                "w:paperSrc",
                &[("w:first", nonzero(i64::from(sep.dm_bin_first))), ("w:other", nonzero(i64::from(sep.dm_bin_other)))],
                "",
            );
        }
        if sep.brc_top.is_some() || sep.brc_left.is_some() || sep.brc_bottom.is_some() || sep.brc_right.is_some() {
            let pgb = sep.pgb_prop;
            let apply_to = pgb & 7;
            let display = match apply_to {
                1 => Some(AttrVal::Str("firstPage")),
                2 => Some(AttrVal::Str("notFirstPage")),
                _ => None,
            };
            let a = [
                ("w:display", display),
                ("w:zOrder", (pgb & 0x8 != 0).then_some(AttrVal::Str("back"))),
                ("w:offsetFrom", (((pgb >> 5) & 7) == 1).then_some(AttrVal::Str("page"))),
            ];
            let mut b = String::new();
            for (name, brc) in
                [("w:top", &sep.brc_top), ("w:left", &sep.brc_left), ("w:bottom", &sep.brc_bottom), ("w:right", &sep.brc_right)]
            {
                if brc.is_some() {
                    b += &brc_xml(name, brc.as_ref());
                }
            }
            x += &el_of("w:pgBorders", &a, &b);
        }
        if sep.lnn_mod != 0 {
            let restart = ["newPage", "newSection", "continuous"].get(usize::from(sep.lnc)).copied().unwrap_or("newPage");
            x += &el_of(
                "w:lnNumType",
                &[
                    ("w:countBy", int(i64::from(sep.lnn_mod))),
                    ("w:start", nonzero(i64::from(sep.lnn_min))),
                    ("w:distance", nonzero(i64::from(sep.dxa_lnn))),
                    ("w:restart", Some(AttrVal::Str(restart))),
                ],
                "",
            );
        }
        let heading = sep.heading_pgn.filter(|&h| h != 0);
        if sep.nfc_pgn != 0 || sep.pgn_restart || heading.is_some() {
            let chap_sep =
                ["hyphen", "period", "colon", "emDash", "enDash"].get(usize::from(sep.cns_pgn.unwrap_or(0))).copied().unwrap_or("hyphen");
            x += &el_of(
                "w:pgNumType",
                &[
                    ("w:fmt", (sep.nfc_pgn != 0).then_some(AttrVal::Str(num_fmt_of(sep.nfc_pgn)))),
                    ("w:start", sep.pgn_restart.then_some(AttrVal::Int(i64::from(sep.pgn_start)))),
                    ("w:chapStyle", heading.map(|h| AttrVal::Int(i64::from(h)))),
                    ("w:chapSep", heading.map(|_| AttrVal::Str(chap_sep))),
                ],
                "",
            );
        }
        let cols = i64::from(sep.ccol_m1) + 1;
        if cols > 1 && !sep.evenly_spaced {
            self.charge(u64::try_from(cols).unwrap_or(0))?;
            let mut c = String::new();
            for i in 0..usize::try_from(cols).unwrap_or(0) {
                let w = sep.col_widths.get(i).copied().flatten().unwrap_or(0);
                let space = if i64::try_from(i).unwrap_or(0) < cols - 1 {
                    Some(AttrVal::Int(i64::from(sep.col_spacings.get(i).copied().flatten().unwrap_or(0))))
                } else {
                    None
                };
                c += &el_of("w:col", &[("w:w", int(i64::from(w))), ("w:space", space)], "");
            }
            x += &el_of(
                "w:cols",
                &[
                    ("w:num", int(cols)),
                    ("w:space", int(i64::from(sep.dxa_columns))),
                    ("w:equalWidth", Some(AttrVal::Str("0"))),
                    ("w:sep", Some(AttrVal::Bool(sep.l_between))),
                ],
                &c,
            );
        } else {
            x += &el_of(
                "w:cols",
                &[
                    ("w:num", if cols > 1 { int(cols) } else { None }),
                    ("w:space", int(i64::from(sep.dxa_columns))),
                    ("w:sep", Some(AttrVal::Bool(sep.l_between))),
                ],
                "",
            );
        }
        if sep.protected_sec == Some(true) {
            x += "<w:formProt w:val=\"0\"/>";
        }
        if sep.vjc != 0 {
            let v = ["top", "center", "both", "bottom"].get(usize::from(sep.vjc)).copied().unwrap_or("top");
            x += &el_of("w:vAlign", &[("w:val", Some(AttrVal::Str(v)))], "");
        }
        if !sep.f_endnote {
            x += "<w:noEndnote/>";
        }
        if sep.title_page {
            x += "<w:titlePg/>";
        }
        if sep.text_flow != 0 {
            let v = ["lrTb", "tbRl", "btLr", "lrTbV", "tbRlV", "tbLrV"].get(usize::from(sep.text_flow)).copied().unwrap_or("lrTb");
            x += &el_of("w:textDirection", &[("w:val", Some(AttrVal::Str(v)))], "");
        }
        if sep.bidi {
            x += "<w:bidi/>";
        }
        if sep.rtl_gutter {
            x += "<w:rtlGutter/>";
        }
        // SClmOperand: 0 default, 1 lines and characters, 2 lines only, 3 snap to characters
        let grid_type = ["default", "linesAndChars", "lines", "snapToChars"].get(usize::from(sep.clm)).copied().unwrap_or("default");
        x += &el_of(
            "w:docGrid",
            &[
                ("w:type", (sep.clm != 0).then_some(AttrVal::Str(grid_type))),
                ("w:linePitch", sep.dya_line_pitch.map(|v| AttrVal::Int(i64::from(v)))),
                ("w:charSpace", nonzero(i64::from(sep.dxt_char_space))),
            ],
            "",
        );
        Ok(format!("<w:sectPr>{x}</w:sectPr>"))
    }

    /// `renderHeaderPart`: a header or footer as a part of its own. The id of the relationship (in the
    /// relationships of the document).
    fn render_header_part(&mut self, story: (i64, i64), header: bool, doc_rels: &mut PartRels) -> Result<String, DocError> {
        let mut rels = PartRels::new();
        let ctx = Story { kind: Kind::Hdr, hdr: true };
        let mut paras = self.collect_paras(story.0, story.1)?;
        // each story ends with a "guard" paragraph that is not shown
        if paras.len() > 1
            && let Some(last) = paras.last()
            && last.cp_end - last.cp_start == 1
        {
            paras.pop();
        }
        let saved = std::mem::take(&mut self.field_stack);
        let len = paras.len();
        let body = self.render_blocks(&mut paras, 0, len, 0, ctx, &mut rels, None);
        self.field_stack = saved;
        let mut body = body?;
        if body.is_empty() {
            body = "<w:p/>".to_owned();
        }
        let kind = if header { "header" } else { "footer" };
        let n = if header {
            self.hdr_n += 1;
            self.hdr_n
        } else {
            self.ftr_n += 1;
            self.ftr_n
        };
        let name = format!("{kind}{n}.xml");
        let tag = if header { "w:hdr" } else { "w:ftr" };
        self.files.set(format!("word/{name}"), FileData::Text(format!("{XML_DECL}<{tag}{}>{body}</{tag}>", content_ns())));
        if rels.size() > 0 {
            self.files.set(format!("word/_rels/{name}.rels"), FileData::Text(rels.xml()));
        }
        self.overrides
            .push((format!("/word/{name}"), format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml")));
        Ok(doc_rels.add_str(kind, &name, false))
    }

    /// `renderNotes`: the footnotes or endnotes part.
    pub(super) fn render_notes(&mut self, notes: &[Note], kind: NoteKind) -> Result<String, DocError> {
        let (tag, root, name, story_kind) = match kind {
            NoteKind::Footnote => ("w:footnote", "w:footnotes", "footnotes.xml", Kind::Ftn),
            NoteKind::Endnote => ("w:endnote", "w:endnotes", "endnotes.xml", Kind::Edn),
        };
        let mut rels = PartRels::new();
        let ctx = Story { kind: story_kind, hdr: false };
        let sep = |ty: &str, id: i64, inner: &str| {
            format!(
                "<{tag} w:type=\"{ty}\" w:id=\"{id}\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><{inner}/></w:r></w:p></{tag}>"
            )
        };
        let mut x = sep("separator", -1, "w:separator");
        x += &sep("continuationSeparator", 0, "w:continuationSeparator");
        for (i, n) in notes.iter().enumerate() {
            let mut paras = self.collect_paras(n.cp_start, n.cp_end)?;
            self.field_stack.clear();
            let len = paras.len();
            let mut body = self.render_blocks(&mut paras, 0, len, 0, ctx, &mut rels, None)?;
            if body.is_empty() {
                body = "<w:p/>".to_owned();
            }
            x += &format!("<{tag} w:id=\"{}\">{body}</{tag}>", i + 1);
        }
        if rels.size() > 0 {
            self.files.set(format!("word/_rels/{name}.rels"), FileData::Text(rels.xml()));
        }
        Ok(format!("{XML_DECL}<{root}{}>{x}</{root}>", content_ns()))
    }

    /// `renderComments`: the comments part.
    pub(super) fn render_comments(&mut self, comments: &[Comment]) -> Result<String, DocError> {
        let mut rels = PartRels::new();
        let ctx = Story { kind: Kind::Atn, hdr: false };
        let mut x = String::new();
        for (i, c) in comments.iter().enumerate() {
            let mut paras = self.collect_paras(c.cp_start, c.cp_end)?;
            self.field_stack.clear();
            let len = paras.len();
            let mut body = self.render_blocks(&mut paras, 0, len, 0, ctx, &mut rels, None)?;
            if body.is_empty() {
                body = "<w:p/>".to_owned();
            }
            let author: Vec<u16> = if c.author.is_empty() { UNKNOWN_AUTHOR.encode_utf16().collect() } else { c.author.units().to_vec() };
            x += &el_of(
                "w:comment",
                &[
                    ("w:id", int(i64::try_from(i).unwrap_or(i64::MAX))),
                    ("w:author", Some(AttrVal::Units(&author))),
                    ("w:initials", (!c.initials.is_empty()).then_some(AttrVal::Units(c.initials.units()))),
                ],
                &body,
            );
        }
        if rels.size() > 0 {
            self.files.set("word/_rels/comments.xml.rels".into(), FileData::Text(rels.xml()));
        }
        Ok(format!("{XML_DECL}<w:comments{}>{x}</w:comments>", content_ns()))
    }
}
