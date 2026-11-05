//! The `.doc` reader assembly (`reader.ts` minus `loadDrawings`, which is `uo_drawingml::odraw`):
//! the streams, the piece table and the text, the FKP runs, the styles, fonts and lists, the
//! sections, notes, comments, bookmarks, header stories, floating shapes and text boxes. The
//! conversion to OOXML is `docx_writer`.
//!
//! A [`DocReader`] borrows the [`DocStreams`] it was made from (runs, grpprls, style records and
//! shape properties are slices of them). Character positions are `i64` (a CP is a `u32` of the
//! file plus the base of its story, which passes 2^32); offsets are `u64` in the byte reader.

use std::cell::Cell;
use std::rc::Rc;

use uo_drawingml::odraw::{BlipBudget, OdrawLimits, Rec, Shape, WordDrawings, read_word_drawings, recs_in};
use uo_jsnum::math::ceil;

use super::bytes::ByteReader;
use super::fib::{FcLcb, Fib, fc};
use super::fkp::{FcRun, Plc, read_chpx_runs, read_papx_runs, read_plc, run_at};
use super::fonts::{DocFont, read_fonts};
use super::lists::{Lists, read_lists};
use super::numeric::int_of;
use super::open::DocStreams;
use super::props::{Sep, apply_sep, default_sep};
use super::sprm::prls;
use super::stsh::{StyleResolver, read_stsh};
use super::text::{Piece, PieceTable, decode_text, piece_at, read_piece_table, total_cp};
use super::{DocError, DocLimitKind, DocLimits};
use crate::text::Utf16;

/// A section: its CP range in the main story and its properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub cp_start: i64,
    pub cp_end: i64,
    pub sep: Sep,
}

/// A footnote or endnote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// the CP of the reference in the main text
    pub ref_cp: i64,
    /// automatic numbering (`FRD.nAuto > 0`) or a custom mark
    pub auto: bool,
    /// the text of the note (absolute CPs)
    pub cp_start: i64,
    pub cp_end: i64,
}

/// A comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub ref_cp: i64,
    pub cp_start: i64,
    pub cp_end: i64,
    /// the owner of the annotation (shared with the owners table: a name is not copied per comment)
    pub author: Rc<Utf16>,
    pub initials: Utf16,
    /// the commented range (annotation bookmark), if any
    pub range_start: Option<i64>,
    pub range_end: Option<i64>,
}

/// A bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub name: Rc<Utf16>,
    pub cp_start: i64,
    pub cp_end: i64,
}

/// A floating shape anchor (`FSPA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fspa {
    pub cp: i64,
    pub spid: i32,
    pub xa_left: i32,
    pub ya_top: i32,
    pub xa_right: i32,
    pub ya_bottom: i32,
    pub bx: u8,
    pub by: u8,
    pub wr: u8,
    pub wrk: u8,
    pub below_text: bool,
    pub anchor_lock: bool,
}

/// The story of a text box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxbxStory {
    pub spid: i32,
    pub cp_start: i64,
    pub cp_end: i64,
}

/// `[start, end)` of every story in the whole text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subdocs {
    pub main: (i64, i64),
    pub ftn: (i64, i64),
    pub hdd: (i64, i64),
    pub atn: (i64, i64),
    pub edn: (i64, i64),
    pub txbx: (i64, i64),
    pub hdr_txbx: (i64, i64),
}

/// Which note story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Footnote,
    Endnote,
}

/// An entry of an STTB: the string and the extra data of the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SttbEntry<'a> {
    pub s: Utf16,
    pub extra: Option<&'a [u8]>,
}

/// The paragraph that contains a CP (`paragraphAt`).
#[derive(Debug, Clone, Copy)]
pub struct ParaAt<'a> {
    pub end: i64,
    pub run: Option<FcRun<'a>>,
}

/// The reader.
pub struct DocReader<'a> {
    pub fib: &'a Fib,
    pub word: &'a [u8],
    pub table: &'a [u8],
    pub data: Option<&'a [u8]>,
    pub pt: PieceTable<'a>,
    /// the text of every story, one UTF-16 unit per CP
    pub text: Vec<u16>,
    pub chpx: Vec<FcRun<'a>>,
    pub papx: Vec<FcRun<'a>>,
    /// the style resolver; its `sheet` is the TS `stsh`
    pub styles: StyleResolver<'a>,
    pub fonts: Vec<DocFont>,
    pub lists: Lists<'a>,
    pub sub: Subdocs,
    pub sections: Vec<Section>,
    pub dop: &'a [u8],
    /// what the document may still inflate (metafile BLIPs), shared by every picture in the TS order
    pub blip_budget: Cell<BlipBudget>,
    /// the drawings (`loadDrawings`)
    pub drawings: Rc<WordDrawings<'a>>,
    string_left: Cell<u64>,
}

/// `cp - base`, `Math.ceil((fc - base) / width)` of the TS as an integer.
fn ceil_div(a: f64, b: f64, width: f64) -> i64 {
    int_of(ceil((a - b) / width))
}

impl<'a> DocReader<'a> {
    /// The constructor of `DocReader` after the streams (`open::DocStreams::open` is its head).
    ///
    /// # Errors
    /// The piece table errors (`NoClx`, `NoPcdt`, `EmptyPieceTable`) and the limits of `limits`.
    pub fn new(streams: &'a DocStreams, limits: &DocLimits) -> Result<DocReader<'a>, DocError> {
        let fib = &streams.fib;
        let word: &'a [u8] = &streams.word;
        let table: &'a [u8] = &streams.table;
        let data = streams.data.as_deref();
        let pt = read_piece_table(fib, table)?;
        let counts = [fib.ccp_text, fib.ccp_ftn, fib.ccp_hdd, fib.ccp_mcr, fib.ccp_atn, fib.ccp_edn, fib.ccp_txbx, fib.ccp_hdr_txbx]
            .map(|v| i64::from(v).max(0));
        let mut at = 0i64;
        let mut span = |n: i64| {
            let s = (at, at + n);
            at += n;
            s
        };
        let main = span(counts[0]);
        let ftn = span(counts[1]);
        let hdd = span(counts[2]);
        span(counts[3]);
        let atn = span(counts[4]);
        let edn = span(counts[5]);
        let txbx = span(counts[6]);
        let hdr_txbx = span(counts[7]);
        let sub = Subdocs { main, ftn, hdd, atn, edn, txbx, hdr_txbx };
        let text = decode_text(&pt, word, total_cp(fib, &pt), limits)?;
        let chpx = read_chpx_runs(fib, table, word);
        let papx = read_papx_runs(fib, table, word, data);
        let styles = StyleResolver::new(read_stsh(fib, table, limits)?);
        let fonts = read_fonts(fib, table);
        let lists = read_lists(fib, table);
        let sections = read_sections(fib, table, word, main.1);
        let dop = fib.fc_lcb(fc::Dop);
        let dop_end = u64::from(dop.fc) + u64::from(dop.lcb);
        let dop_bytes =
            if dop.lcb != 0 && dop_end <= table.len() as u64 { ByteReader::new(table).sub(u64::from(dop.fc), dop_end) } else { &[] };
        let dgg = fib.fc_lcb(fc::DggInfo);
        let drawings = Rc::new(read_word_drawings(table, dgg.fc, dgg.lcb, &OdrawLimits::WORD));
        Ok(DocReader {
            fib,
            word,
            table,
            data,
            pt,
            text,
            chpx,
            papx,
            styles,
            fonts,
            lists,
            sub,
            sections,
            dop: dop_bytes,
            blip_budget: Cell::new(BlipBudget::document()),
            drawings,
            string_left: Cell::new(limits.string_budget(table.len() as u64)),
        })
    }

    /// The piece table's pieces.
    #[must_use]
    pub fn pieces(&self) -> &[Piece] {
        &self.pt.pieces
    }

    /// `text.charCodeAt(cp)`: `None` is the NaN of a CP outside the text.
    #[must_use]
    pub fn char_at(&self, cp: i64) -> Option<u16> {
        usize::try_from(cp).ok().and_then(|i| self.text.get(i)).copied()
    }

    /// The grpprl of the Prm of a piece (fast save).
    #[must_use]
    pub fn prm_grpprl(&self, piece: &Piece) -> Option<&'a [u8]> {
        if piece.prm & 1 != 0 { self.pt.prcs.get(usize::from(piece.prm >> 1)).copied() } else { None }
    }

    /// `paragraphAt`: the end of the paragraph that contains `cp` ([MS-DOC] 2.4.2): the PAPX run of
    /// the FC; when it ends inside the piece the paragraph ends there, else it goes on in the next
    /// piece.
    #[must_use]
    pub fn paragraph_at(&self, cp: i64, limit: i64) -> ParaAt<'a> {
        let mut cur = cp;
        let mut guard = 0u32;
        while cur < limit && guard < 100_000 {
            guard += 1;
            let Some(i) = piece_at(&self.pt.pieces, cur) else { return ParaAt { end: limit, run: None } };
            let p = &self.pt.pieces[i];
            let cs = if p.compressed { 1.0 } else { 2.0 };
            let cp_start = i64::from(p.cp_start);
            let fc0 = p.fc + (cur - cp_start) as f64 * cs;
            let run = run_at(&self.papx, fc0).copied();
            let piece_fc_end = p.fc + (i64::from(p.cp_end) - cp_start) as f64 * cs;
            if let Some(run) = run
                && f64::from(run.fc_end) <= piece_fc_end
                && f64::from(run.fc_end) > fc0
            {
                let end = cp_start + ceil_div(f64::from(run.fc_end), p.fc, cs);
                return ParaAt { end: end.min(limit), run: Some(run) };
            }
            cur = i64::from(p.cp_end);
        }
        ParaAt { end: limit, run: None }
    }

    /// `headerStories`: the `[start, end)` (absolute) of the header and footer stories by index of
    /// the `Plcfhdd`.
    #[must_use]
    pub fn header_stories(&self) -> Vec<Option<(i64, i64)>> {
        let p = self.fib.fc_lcb(fc::PlcfHdd);
        let Some(plc) = read_plc(self.table, p.fc, p.lcb, 0) else { return Vec::new() };
        if self.fib.ccp_hdd == 0 {
            return Vec::new();
        }
        let base = self.sub.hdd.0;
        let ccp = i64::from(self.fib.ccp_hdd);
        let mut out = Vec::new();
        // the last CP is undefined; the one before it only closes the last story
        for i in 0..plc.cps.len().saturating_sub(2) {
            let (a, b) = (i64::from(plc.cps[i]), i64::from(plc.cps[i + 1]));
            out.push(if b > a && b <= ccp { Some((base + a, base + b)) } else { None });
        }
        out
    }

    /// `notes`: the footnotes or the endnotes.
    #[must_use]
    pub fn notes(&self, kind: NoteKind) -> Vec<Note> {
        let (ref_idx, txt_idx) = match kind {
            NoteKind::Footnote => (fc::PlcffndRef, fc::PlcffndTxt),
            NoteKind::Endnote => (fc::PlcfendRef, fc::PlcfendTxt),
        };
        let (rf, tx) = (self.fib.fc_lcb(ref_idx), self.fib.fc_lcb(txt_idx));
        let ref_plc = read_plc(self.table, rf.fc, rf.lcb, 2);
        let txt_plc = read_plc(self.table, tx.fc, tx.lcb, 0);
        let (Some(ref_plc), Some(txt_plc)) = (ref_plc, txt_plc) else { return Vec::new() };
        let base = match kind {
            NoteKind::Footnote => self.sub.ftn.0,
            NoteKind::Endnote => self.sub.edn.0,
        };
        let mut out = Vec::new();
        for i in 0..ref_plc.n {
            let Ok(k) = usize::try_from(i) else { break };
            let (Some(&a), Some(&b)) = (txt_plc.cps.get(k), txt_plc.cps.get(k + 1)) else { break };
            out.push(Note {
                ref_cp: i64::from(ref_plc.cps[k]),
                auto: ref_plc.r.i16(ref_plc.data_at(i)) > 0,
                cp_start: base + i64::from(a),
                cp_end: base + i64::from(b),
            });
        }
        out
    }

    /// Spends `units` of the string budget of the conversion.
    fn spend_strings(&self, units: u64) -> Result<(), DocError> {
        let left = self.string_left.get();
        if units > left {
            self.string_left.set(0);
            return Err(DocError::Limit { kind: DocLimitKind::StringUnits, max: left });
        }
        self.string_left.set(left - units);
        Ok(())
    }

    /// `comments`.
    ///
    /// # Errors
    /// [`DocError::Limit`] when the strings read pass the string budget.
    pub fn comments(&self) -> Result<Vec<Comment>, DocError> {
        let (rf, tx) = (self.fib.fc_lcb(fc::PlcfandRef), self.fib.fc_lcb(fc::PlcfandTxt));
        let ref_plc = read_plc(self.table, rf.fc, rf.lcb, 30);
        let txt_plc = read_plc(self.table, tx.fc, tx.lcb, 0);
        let (Some(ref_plc), Some(txt_plc)) = (ref_plc, txt_plc) else { return Ok(Vec::new()) };
        let owners = self.xst_array(fc::GrpXstAtnOwners)?;
        let base = self.sub.atn.0;
        // annotation bookmarks (the commented ranges)
        let (bkf, bkl) = (self.fib.fc_lcb(fc::PlcfAtnBkf), self.fib.fc_lcb(fc::PlcfAtnBkl));
        let bkf_plc = read_plc(self.table, bkf.fc, bkf.lcb, 4);
        let bkl_plc = read_plc(self.table, bkl.fc, bkl.lcb, 0);
        let sttb = self.sttb(fc::SttbfAtnBkmk)?;
        // a JS Map: a later tag replaces the range of an earlier one
        let mut ranges: std::collections::HashMap<i64, (i64, i64)> = std::collections::HashMap::new();
        if let (Some(bkf_plc), Some(bkl_plc)) = (&bkf_plc, &bkl_plc) {
            for i in 0..bkf_plc.n {
                let Ok(k) = usize::try_from(i) else { break };
                let ibkl = usize::from(bkf_plc.r.u16(bkf_plc.data_at(i)));
                let end = bkl_plc.cps.get(ibkl).copied();
                let l_tag = match sttb.get(k).and_then(|e| e.extra) {
                    Some(extra) => i64::from(ByteReader::new(extra).i32(2)),
                    None => i64::try_from(k).unwrap_or(i64::MAX),
                };
                if let Some(end) = end {
                    ranges.insert(l_tag, (i64::from(bkf_plc.cps[k]), i64::from(end)));
                }
            }
        }
        let mut out = Vec::new();
        for i in 0..ref_plc.n {
            let Ok(k) = usize::try_from(i) else { break };
            let (Some(&a), Some(&b)) = (txt_plc.cps.get(k), txt_plc.cps.get(k + 1)) else { break };
            let o = ref_plc.data_at(i);
            let cch_init = u64::from(ref_plc.r.u16(o)).min(9);
            let initials = Utf16::from_units(ref_plc.r.utf16(o + 2, cch_init));
            let ibst = ref_plc.r.i16(o + 20);
            let l_tag = i64::from(ref_plc.r.i32(o + 26));
            let range = ranges.get(&l_tag).copied();
            let author = usize::try_from(ibst).ok().and_then(|i| owners.get(i)).cloned().unwrap_or_default();
            out.push(Comment {
                ref_cp: i64::from(ref_plc.cps[k]),
                cp_start: base + i64::from(a),
                cp_end: base + i64::from(b),
                author,
                initials,
                range_start: range.map(|r| r.0),
                range_end: range.map(|r| r.1),
            });
        }
        Ok(out)
    }

    /// `xstArray`: consecutive Xst (`cch` u16 + UTF-16).
    fn xst_array(&self, idx: usize) -> Result<Vec<Rc<Utf16>>, DocError> {
        let FcLcb { fc: fc0, lcb } = self.fib.fc_lcb(idx);
        let (fc0, lcb) = (u64::from(fc0), u64::from(lcb));
        if lcb == 0 || fc0 + lcb > self.table.len() as u64 {
            return Ok(Vec::new());
        }
        let r = ByteReader::new(self.table);
        let mut out = Vec::new();
        let mut o = fc0;
        while o + 2 <= fc0 + lcb {
            let cch = u64::from(r.u16(o));
            self.spend_strings(cch)?;
            out.push(Rc::new(Utf16::from_units(r.utf16(o + 2, cch))));
            o += 2 + cch * 2;
        }
        Ok(out)
    }

    /// `sttb`: an STTB (extended or not): the strings and the extra data.
    ///
    /// # Errors
    /// [`DocError::Limit`] when the strings read pass the string budget.
    pub fn sttb(&self, idx: usize) -> Result<Vec<SttbEntry<'a>>, DocError> {
        let FcLcb { fc: fc0, lcb } = self.fib.fc_lcb(idx);
        let (fc0, lcb) = (u64::from(fc0), u64::from(lcb));
        if lcb == 0 || fc0 + lcb > self.table.len() as u64 {
            return Ok(Vec::new());
        }
        let r = ByteReader::new(self.table);
        let mut o = fc0;
        let mut c_data = r.u16(o);
        o += 2;
        let mut ext = false;
        if c_data == 0xffff {
            ext = true;
            c_data = r.u16(o);
            o += 2;
        }
        let cb_extra = u64::from(r.u16(o));
        o += 2;
        let mut out = Vec::new();
        let mut i = 0u16;
        while i < c_data && o < fc0 + lcb {
            let s: Vec<u16>;
            if ext {
                let cch = u64::from(r.u16(o));
                self.spend_strings(cch)?;
                s = r.utf16(o + 2, cch);
                o += 2 + cch * 2;
            } else {
                let cch = u64::from(r.u8(o));
                self.spend_strings(cch)?;
                s = (0..cch).map(|k| u16::from(r.u8(o + 1 + k))).collect();
                o += 1 + cch;
            }
            let extra = if cb_extra != 0 { Some(r.sub(o, o + cb_extra)) } else { None };
            o += cb_extra;
            out.push(SttbEntry { s: Utf16::from_units(s), extra });
            i += 1;
        }
        Ok(out)
    }

    /// `bookmarks`.
    ///
    /// # Errors
    /// [`DocError::Limit`] when the strings read pass the string budget.
    pub fn bookmarks(&self) -> Result<Vec<Bookmark>, DocError> {
        let names = self.sttb(fc::SttbfBkmk)?;
        let (bkf, bkl) = (self.fib.fc_lcb(fc::PlcfBkf), self.fib.fc_lcb(fc::PlcfBkl));
        let bkf_plc = read_plc(self.table, bkf.fc, bkf.lcb, 4);
        let bkl_plc = read_plc(self.table, bkl.fc, bkl.lcb, 0);
        let (Some(bkf_plc), Some(bkl_plc)) = (bkf_plc, bkl_plc) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let Ok(k) = u64::try_from(i) else { break };
            if k >= bkf_plc.n {
                break;
            }
            let ibkl = usize::from(bkf_plc.r.u16(bkf_plc.data_at(k)));
            let Some(&end) = bkl_plc.cps.get(ibkl) else { continue };
            out.push(Bookmark { name: Rc::new(name.s), cp_start: i64::from(bkf_plc.cps[i]), cp_end: i64::from(end) });
        }
        Ok(out)
    }

    /// `revisionAuthors`: the authors of the revisions (`SttbfRMark`).
    ///
    /// # Errors
    /// [`DocError::Limit`] when the strings read pass the string budget.
    pub fn revision_authors(&self) -> Result<Vec<Rc<Utf16>>, DocError> {
        Ok(self.sttb(fc::SttbfRMark)?.into_iter().map(|e| Rc::new(e.s)).collect())
    }

    /// `fspas`: the floating shape anchors of the main document or of the headers.
    #[must_use]
    pub fn fspas(&self, hdr: bool) -> Vec<Fspa> {
        let p = self.fib.fc_lcb(if hdr { fc::PlcSpaHdr } else { fc::PlcSpaMom });
        let Some(plc) = read_plc(self.table, p.fc, p.lcb, 26) else { return Vec::new() };
        let base = if hdr { self.sub.hdd.0 } else { 0 };
        let mut out = Vec::new();
        for i in 0..plc.n {
            let Ok(k) = usize::try_from(i) else { break };
            let o = plc.data_at(i);
            let r = plc.r;
            let flags = r.u16(o + 20);
            out.push(Fspa {
                cp: base + i64::from(plc.cps[k]),
                spid: r.i32(o),
                xa_left: r.i32(o + 4),
                ya_top: r.i32(o + 8),
                xa_right: r.i32(o + 12),
                ya_bottom: r.i32(o + 16),
                bx: ((flags >> 1) & 3) as u8,
                by: ((flags >> 3) & 3) as u8,
                wr: ((flags >> 5) & 0xf) as u8,
                wrk: ((flags >> 9) & 0xf) as u8,
                below_text: flags & 0x4000 != 0,
                anchor_lock: flags & 0x8000 != 0,
            });
        }
        out
    }

    /// `shape`: the shape of a story's drawing by `spid` (the other drawing when it is not there).
    #[must_use]
    pub fn shape(&self, spid: i64, hdr: bool) -> Option<&Shape<'a>> {
        // a spid is read as `u32`; the FSPA's is an `i32` (a negative one is never in the map)
        self.drawings.shape(u32::try_from(spid).ok()?, hdr)
    }

    /// `textboxes`: the stories of the text boxes (by `lid`, the `spid`) of the main document or of
    /// the headers.
    #[must_use]
    pub fn textboxes(&self, hdr: bool) -> Vec<TxbxStory> {
        let p = self.fib.fc_lcb(if hdr { fc::PlcfHdrtxbxTxt } else { fc::PlcftxbxTxt });
        let Some(plc) = read_plc(self.table, p.fc, p.lcb, 22) else { return Vec::new() };
        let base = if hdr { self.sub.hdr_txbx.0 } else { self.sub.txbx.0 };
        let mut out = Vec::new();
        for i in 0..plc.n {
            let Ok(k) = usize::try_from(i) else { break };
            let o = plc.data_at(i);
            let lid = plc.r.i32(o + 14);
            let (a, b) = (i64::from(plc.cps[k]), i64::from(plc.cps[k + 1]));
            if b > a {
                out.push(TxbxStory { spid: lid, cp_start: base + a, cp_end: base + b });
            }
        }
        out
    }

    /// `dataRecords`: the OfficeArt records of the `Data` stream in `[start, end)`.
    pub fn data_records(&self, start: u64, end: u64) -> Vec<Rec<'a>> {
        match self.data {
            Some(d) => recs_in(d, start, end.min(d.len() as u64)).collect(),
            None => Vec::new(),
        }
    }
}

/// `readSections`.
fn read_sections(fib: &Fib, table: &[u8], word: &[u8], main_end: i64) -> Vec<Section> {
    let p = fib.fc_lcb(fc::PlcfSed);
    let plc: Option<Plc<'_>> = read_plc(table, p.fc, p.lcb, 12);
    let mut out: Vec<Section> = Vec::new();
    let w = ByteReader::new(word);
    if let Some(plc) = plc {
        for i in 0..plc.n {
            let Ok(k) = usize::try_from(i) else { break };
            let mut sep = default_sep();
            let fc_sepx = u64::from(plc.r.u32(plc.data_at(i) + 2));
            if fc_sepx != 0xffff_ffff && fc_sepx + 2 <= w.len() {
                let cb = u64::from(w.i16(fc_sepx).max(0).unsigned_abs());
                let grpprl = w.sub(fc_sepx + 2, fc_sepx + 2 + cb);
                for pr in prls(grpprl) {
                    apply_sep(&mut sep, &pr);
                }
            }
            let cp_start = i64::from(plc.cps[k]).min(main_end);
            let cp_end = i64::from(plc.cps[k + 1]).min(main_end);
            if cp_end > cp_start || out.is_empty() {
                out.push(Section { cp_start, cp_end, sep });
            }
        }
    }
    if out.is_empty() {
        out.push(Section { cp_start: 0, cp_end: main_end, sep: default_sep() });
    }
    if let Some(last) = out.last_mut() {
        last.cp_end = main_end;
    }
    out
}
