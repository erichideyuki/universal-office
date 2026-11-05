//! The style sheet ([MS-DOC] STSH, `stsh.ts`): `Stshi` (`Stshif`: counts, default fonts) +
//! `rglpstd` (`LPStd`: `cbStd` + `STD`). Each STD is `Stdf` (`StdfBase` + `StdfPost2000`) + the
//! name (`Xstz`) + the UPXs (paragraph: PAPX + CHPX; character: CHPX; table: TAPX + PAPX + CHPX;
//! numbering: PAPX). Also resolves the inheritance (`basedOn`) into the complete CHP/PAP of each
//! style.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::bytes::ByteReader;
use super::fib::{FcLcb, Fib, fc};
use super::props::{Chp, ChpCtx, ChpKey, KeySet, Pap, PapCtx, PapKey, apply_chp, apply_pap, default_chp, prune_none_borders};
use super::sprm::prls;
use super::{DocError, DocLimitKind, DocLimits};
use crate::text::Utf16;

/// `Stshif`: the counts and the default fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stshif {
    pub cstd: u16,
    pub cb_std_base_in_file: u16,
    pub std_stylenames_written: bool,
    pub sti_max_when_saved: u16,
    pub ftc_asci: u16,
    pub ftc_fe: u16,
    pub ftc_other: u16,
    /// the default font of complex scripts (`Stshi.ftcBi`, right after the Stshif)
    pub ftc_bi: u16,
}

/// A style (`DocStyle`); the UPXs borrow the table stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocStyle<'a> {
    /// the index in the sheet (what `sprmPIstd` and `basedOn` refer to)
    pub istd: u16,
    pub sti: u16,
    /// 1 paragraph, 2 character, 3 table, 4 numbering
    pub stk: u8,
    pub istd_base: u16,
    pub istd_next: u16,
    pub istd_link: Option<u16>,
    pub name: Utf16,
    /// what follows the first comma of the stored name (possibly empty)
    pub aliases: Option<Utf16>,
    pub auto_redef: bool,
    pub hidden: bool,
    pub semi_hidden: bool,
    pub locked: bool,
    pub unhide_when_used: bool,
    pub q_format: bool,
    pub priority: Option<u16>,
    pub papx: Option<&'a [u8]>,
    pub chpx: Option<&'a [u8]>,
    pub tapx: Option<&'a [u8]>,
}

/// `ISTD_NIL`.
pub const ISTD_NIL: u16 = 0x0fff;

/// The style sheet: `styles[istd]` is `None` for an empty slot (`cbStd` 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleSheet<'a> {
    pub stshif: Stshif,
    pub styles: Vec<Option<DocStyle<'a>>>,
}

/// `readStsh`.
///
/// # Errors
/// [`DocError::Limit`] when the style names read past `limits.string_budget` of the table: a name
/// is read at the offset the style gives, not bounded by the style's own record, so overlapping
/// records can multiply a small table (the TS has no bound).
pub fn read_stsh<'a>(fib: &Fib, table: &'a [u8], limits: &DocLimits) -> Result<StyleSheet<'a>, DocError> {
    read_stsh_at(fib.fc_lcb(fc::Stshf), table, limits)
}

/// `readStsh` for the pair `fc`/`lcb` (what `fib.fcLcb(FC.Stshf)` gives).
///
/// # Errors
/// As [`read_stsh`].
pub fn read_stsh_at<'a>(pair: FcLcb, table: &'a [u8], limits: &DocLimits) -> Result<StyleSheet<'a>, DocError> {
    let (fc0, lcb) = (u64::from(pair.fc), u64::from(pair.lcb));
    let r = ByteReader::new(table);
    let empty = StyleSheet {
        stshif: Stshif {
            cstd: 0,
            cb_std_base_in_file: 10,
            std_stylenames_written: true,
            sti_max_when_saved: 0,
            ftc_asci: 0,
            ftc_fe: 0,
            ftc_other: 0,
            ftc_bi: 0,
        },
        styles: Vec::new(),
    };
    if lcb == 0 || fc0 + lcb > r.len() {
        return Ok(empty);
    }
    let end = fc0 + lcb;
    let cb_stshi = r.u16(fc0);
    let s = fc0 + 2;
    let cb_base = r.u16(s + 2);
    let stshif = Stshif {
        cstd: r.u16(s),
        cb_std_base_in_file: if cb_base == 0 { 10 } else { cb_base },
        std_stylenames_written: r.u16(s + 4) & 1 != 0,
        sti_max_when_saved: r.u16(s + 6),
        ftc_asci: r.u16(s + 12),
        ftc_fe: r.u16(s + 14),
        ftc_other: r.u16(s + 16),
        ftc_bi: if cb_stshi >= 20 { r.u16(s + 18) } else { 0 },
    };
    let mut off = s + u64::from(cb_stshi);
    let mut styles: Vec<Option<DocStyle<'a>>> = Vec::new();
    let mut budget = limits.string_budget(r.len());
    let base_len = u64::from(stshif.cb_std_base_in_file);
    let mut istd = 0u32;
    while istd < u32::from(stshif.cstd) && off + 2 <= end {
        let cb_std = u64::from(r.u16(off));
        let std = off + 2;
        off = std + cb_std;
        // the LPStd starts on an even boundary
        if (off - fc0) & 1 == 1 {
            off += 1;
        }
        let this = u16::try_from(istd).unwrap_or(u16::MAX);
        istd += 1;
        if cb_std == 0 {
            styles.push(None);
            continue;
        }
        let std_end = (std + cb_std).min(end);
        let (w0, w1, w2) = (r.u16(std), r.u16(std + 2), r.u16(std + 4));
        let grf = r.u16(std + 8);
        let mut st = DocStyle {
            istd: this,
            sti: w0 & 0x0fff,
            stk: (w1 & 0x0f) as u8,
            istd_base: (w1 >> 4) & 0x0fff,
            istd_next: (w2 >> 4) & 0x0fff,
            istd_link: None,
            name: Utf16::new(),
            aliases: None,
            auto_redef: grf & 0x0001 != 0,
            hidden: grf & 0x0002 != 0,
            semi_hidden: grf & 0x0100 != 0,
            locked: grf & 0x0200 != 0,
            unhide_when_used: grf & 0x0800 != 0,
            q_format: grf & 0x1000 != 0,
            priority: None,
            papx: None,
            chpx: None,
            tapx: None,
        };
        let cupx = u64::from(w2 & 0x0f);
        if base_len >= 18 {
            let link = r.u16(std + 10) & 0x0fff;
            if link != 0 && link != ISTD_NIL {
                st.istd_link = Some(link);
            }
            st.priority = Some((r.u16(std + 16) >> 4) & 0x0fff);
        }
        // the name: Xstz (cch u16 + UTF-16 + terminator)
        let mut o = std + base_len;
        let cch = u64::from(r.u16(o));
        let full = r.utf16(o + 2, cch);
        let read = full.len() as u64;
        if read > budget {
            return Err(DocError::Limit { kind: DocLimitKind::StringUnits, max: limits.string_budget(r.len()) });
        }
        budget -= read;
        o += 2 + cch * 2 + 2;
        match full.iter().position(|&c| c == u16::from(b',')) {
            Some(comma) => {
                st.name = Utf16::from_units(full[..comma].to_vec());
                st.aliases = Some(Utf16::from_units(full[comma + 1..].to_vec()));
            }
            None => st.name = Utf16::from_units(full),
        }
        // the UPXs, each with cbUpx (u16) and aligned to an even boundary
        let mut upxs: Vec<&'a [u8]> = Vec::new();
        let mut k = 0;
        while k < cupx && o + 2 <= std_end {
            let cb_upx = u64::from(r.u16(o));
            upxs.push(r.sub(o + 2, (o + 2 + cb_upx).min(std_end)));
            o += 2 + cb_upx;
            if (o - std) & 1 == 1 {
                o += 1;
            }
            k += 1;
        }
        // an UpxPapx starts with its istd (u16)
        let papx_of = |u: Option<&&'a [u8]>| -> Option<&'a [u8]> { u.and_then(|u| (u.len() >= 2).then(|| &u[2..])) };
        match st.stk {
            1 => {
                st.papx = papx_of(upxs.first());
                st.chpx = upxs.get(1).copied();
            }
            2 => st.chpx = upxs.first().copied(),
            3 => {
                st.tapx = upxs.first().copied();
                st.papx = papx_of(upxs.get(1));
                st.chpx = upxs.get(2).copied();
            }
            4 => st.papx = papx_of(upxs.first()),
            _ => {}
        }
        styles.push(Some(st));
    }
    Ok(StyleSheet { stshif, styles })
}

// ───────────────────────────── resolution ─────────────────────────────

/// A style with its inheritance resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStyle {
    pub pap: Pap,
    pub chp: Chp,
    /// the style's own delta (what its UPX touches), with final values
    pub own_pap: KeySet<PapKey>,
    pub own_chp: KeySet<ChpKey>,
}

/// How many `basedOn` links `resolve` follows (`depth < 32`); past it the chain ends at the
/// default style.
const RESOLVE_DEPTH: u32 = 32;

/// How many character styles `char_chain` follows (`chain.length < 32`).
const CHAIN_LEN: usize = 32;

/// `StyleResolver`: resolves styles lazily with memoization. The results depend on the order of
/// the calls (a chain longer than 32, or a cycle, is cut at the depth the first call reached:
/// the memo keeps what the first resolution of each style computed), so callers keep the call
/// order of the TS.
#[derive(Debug)]
pub struct StyleResolver<'a> {
    pub sheet: StyleSheet<'a>,
    memo: HashMap<u16, Arc<ResolvedStyle>>,
    char_memo: HashMap<u16, Arc<Vec<&'a [u8]>>>,
    pub root_chp: Chp,
}

impl<'a> StyleResolver<'a> {
    #[must_use]
    pub fn new(sheet: StyleSheet<'a>) -> StyleResolver<'a> {
        let f = sheet.stshif;
        let root_chp = Chp { ftc_bi: Some(f.ftc_bi), ..default_chp(f.ftc_asci, f.ftc_fe, f.ftc_other) };
        StyleResolver { sheet, memo: HashMap::new(), char_memo: HashMap::new(), root_chp }
    }

    /// `style(istd)`.
    #[must_use]
    pub fn style(&self, istd: u16) -> Option<&DocStyle<'a>> {
        self.sheet.styles.get(usize::from(istd)).and_then(Option::as_ref)
    }

    /// The parent of a style (`st.istdBase` when it is a style other than itself and the chain is
    /// not at `RESOLVE_DEPTH`), or `None` when the style stands on the defaults.
    fn parent_of(&self, istd: u16, depth: u32) -> Option<u16> {
        let st = self.style(istd)?;
        (st.istd_base != ISTD_NIL && st.istd_base != istd && depth < RESOLVE_DEPTH && self.style(st.istd_base).is_some())
            .then_some(st.istd_base)
    }

    /// `resolve`: the paragraph (or table, or numbering) style resolved with its inheritance. The
    /// recursion of the TS (at most `RESOLVE_DEPTH` + 1 frames) is a loop over the chain, memoizing
    /// from the deepest style up, as the TS returns.
    pub fn resolve(&mut self, istd: u16) -> Arc<ResolvedStyle> {
        // descend: the styles whose result is needed, deepest last, and the base at its end
        let mut chain: Vec<u16> = Vec::new();
        let mut cur = istd;
        let mut depth = 0;
        let base: Option<Arc<ResolvedStyle>> = loop {
            if let Some(hit) = self.memo.get(&cur) {
                break Some(Arc::clone(hit));
            }
            chain.push(cur);
            match self.parent_of(cur, depth) {
                Some(p) => {
                    cur = p;
                    depth += 1;
                }
                None => break None,
            }
        };
        // `chain` holds the frames that missed the memo: the last one is the deepest. Its parent is
        // the memo hit (or, when the loop ended on a style without a parent, the defaults).
        let mut parent: Option<Arc<ResolvedStyle>> = base;
        while let Some(id) = chain.pop() {
            let res = Arc::new(self.compute(id, parent.as_deref()));
            self.memo.insert(id, Arc::clone(&res));
            parent = Some(res);
        }
        // the first frame (depth 0) is the last computed, or the memo hit itself
        parent.unwrap_or_else(|| Arc::new(self.compute(istd, None)))
    }

    /// One frame of `resolve`: the style over its resolved parent (`None`: the defaults).
    fn compute(&self, istd: u16, parent: Option<&ResolvedStyle>) -> ResolvedStyle {
        let default_parent;
        let parent = match parent {
            Some(p) => p,
            None => {
                default_parent = ResolvedStyle {
                    pap: Pap { istd, widow_control: Some(true), ..Pap::default() },
                    chp: self.root_chp.clone(),
                    own_pap: KeySet::new(),
                    own_chp: KeySet::new(),
                };
                &default_parent
            }
        };
        let st = self.style(istd);
        let mut pap = Pap { istd, tabs_cleared: Some(Vec::new()), ..parent.pap.clone() };
        let mut chp = parent.chp.clone();
        let mut own_pap = KeySet::new();
        let mut own_chp = KeySet::new();
        if let Some(papx) = st.and_then(|s| s.papx) {
            let mut ctx = PapCtx::default();
            for p in prls(papx) {
                apply_pap(&mut pap, &p, &mut own_pap, &mut ctx);
            }
        }
        prune_none_borders(&pap, &parent.pap, &mut own_pap);
        if let Some(chpx) = st.and_then(|s| s.chpx) {
            let mut ctx = ChpCtx { base: parent.chp.clone(), apply_char_style: None, para_style: None };
            for p in prls(chpx) {
                if p.sprm != 0x4a30 {
                    apply_chp(&mut chp, &p, &mut ctx, &mut own_chp);
                }
            }
        }
        ResolvedStyle { pap, chp, own_pap, own_chp }
    }

    /// `charChain`: the CHPXs of the chain of a character style (root to leaf).
    fn char_chain(&mut self, istd: u16) -> Arc<Vec<&'a [u8]>> {
        if let Some(hit) = self.char_memo.get(&istd) {
            return Arc::clone(hit);
        }
        let mut chain: Vec<&'a [u8]> = Vec::new();
        let mut seen: HashSet<u16> = HashSet::new();
        let mut cur = istd;
        let mut len = 0;
        while cur != ISTD_NIL && !seen.contains(&cur) && len < CHAIN_LEN {
            seen.insert(cur);
            let Some(st) = self.style(cur) else { break };
            if st.stk != 2 {
                break;
            }
            if let Some(chpx) = st.chpx {
                chain.insert(0, chpx);
            }
            len = chain.len();
            cur = st.istd_base;
        }
        let chain = Arc::new(chain);
        self.char_memo.insert(istd, Arc::clone(&chain));
        chain
    }

    /// `applyCharStyle`: the CHP of a character style applied over the paragraph's.
    pub fn apply_char_style(&mut self, istd: u16, on_top: &Chp) -> Chp {
        let mut chp = on_top.clone();
        for g in self.char_chain(istd).iter() {
            let mut ctx = ChpCtx { base: chp.clone(), apply_char_style: None, para_style: None };
            let mut dummy = KeySet::new();
            for p in prls(g) {
                if p.sprm != 0x4a30 {
                    apply_chp(&mut chp, &p, &mut ctx, &mut dummy);
                }
            }
        }
        chp.istd = Some(istd);
        chp
    }

    /// `resolveChar`: the own delta of a character style (against its parent in the chain).
    pub fn resolve_char(&mut self, istd: u16) -> (Chp, KeySet<ChpKey>) {
        let base = self.style(istd).filter(|st| st.istd_base != ISTD_NIL && st.istd_base != istd).map(|st| st.istd_base);
        let parent_chp = match base {
            Some(b) => {
                let root = self.root_chp.clone();
                self.apply_char_style(b, &root)
            }
            None => self.root_chp.clone(),
        };
        let mut chp = parent_chp.clone();
        let mut own = KeySet::new();
        if let Some(chpx) = self.style(istd).and_then(|s| s.chpx) {
            let mut ctx = ChpCtx { base: parent_chp, apply_char_style: None, para_style: None };
            for p in prls(chpx) {
                if p.sprm != 0x4a30 {
                    apply_chp(&mut chp, &p, &mut ctx, &mut own);
                }
            }
        }
        (chp, own)
    }

    /// Forgets the memoized results (the memory-gate convention).
    pub fn clear(&mut self) {
        self.memo.clear();
        self.char_memo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One style to build: (sti, stk, istdBase, name, papx grpprl, chpx grpprl), `None` for an empty slot.
    type Spec<'s> = Option<(u16, u8, u16, &'s str, &'s [u8], &'s [u8])>;

    /// A table stream holding a style sheet at 0 (cbStshi 20, cbSTDBaseInFile 10).
    fn table(specs: &[Spec<'_>]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend(20u16.to_le_bytes());
        let mut stshif = [0u8; 20];
        stshif[0..2].copy_from_slice(&u16::try_from(specs.len()).unwrap().to_le_bytes());
        stshif[2..4].copy_from_slice(&10u16.to_le_bytes());
        stshif[4..6].copy_from_slice(&1u16.to_le_bytes());
        stshif[12..14].copy_from_slice(&1u16.to_le_bytes());
        stshif[14..16].copy_from_slice(&2u16.to_le_bytes());
        stshif[16..18].copy_from_slice(&3u16.to_le_bytes());
        stshif[18..20].copy_from_slice(&4u16.to_le_bytes());
        t.extend(stshif);
        for spec in specs {
            let Some((sti, stk, base, name, papx, chpx)) = spec else {
                t.extend(0u16.to_le_bytes());
                continue;
            };
            let mut std = vec![0u8; 10];
            std[0..2].copy_from_slice(&sti.to_le_bytes());
            std[2..4].copy_from_slice(&(u16::from(*stk) | (base << 4)).to_le_bytes());
            let cupx: u16 = match stk {
                1 => 2,
                3 => 3,
                _ => 1,
            };
            std[4..6].copy_from_slice(&cupx.to_le_bytes());
            let units: Vec<u16> = name.encode_utf16().collect();
            std.extend(u16::try_from(units.len()).unwrap().to_le_bytes());
            for u in units {
                std.extend(u.to_le_bytes());
            }
            std.extend([0, 0]);
            let upx = |std: &mut Vec<u8>, bytes: &[u8]| {
                if std.len() % 2 == 1 {
                    std.push(0);
                }
                std.extend(u16::try_from(bytes.len()).unwrap().to_le_bytes());
                std.extend(bytes);
            };
            let mut with_istd = vec![0u8, 0];
            with_istd.extend(*papx);
            match stk {
                1 => {
                    upx(&mut std, &with_istd);
                    upx(&mut std, chpx);
                }
                2 => upx(&mut std, chpx),
                3 => {
                    upx(&mut std, &[]);
                    upx(&mut std, &with_istd);
                    upx(&mut std, chpx);
                }
                _ => upx(&mut std, &with_istd),
            }
            t.extend(u16::try_from(std.len()).unwrap().to_le_bytes());
            t.extend(&std);
            if t.len() % 2 == 1 {
                t.push(0);
            }
        }
        t
    }

    fn sheet(t: &[u8]) -> StyleSheet<'_> {
        read_stsh_at(FcLcb { fc: 0, lcb: u32::try_from(t.len()).unwrap() }, t, &DocLimits::DEFAULT).unwrap()
    }

    // sprmPDxaLeft (0x840f) = 100; sprmPJc (0x2461) = 1; sprmCFBold (0x0835) = 1; sprmCHps (0x4a43) = 30
    const JC_RIGHT: &[u8] = &[0x61, 0x24, 1];
    const LEFT_100: &[u8] = &[0x0f, 0x84, 100, 0];
    const BOLD: &[u8] = &[0x35, 0x08, 1];
    const HPS_30: &[u8] = &[0x43, 0x4a, 30, 0];

    #[test]
    fn reads_the_stylesheet_header_names_aliases_and_upxs() {
        let t = table(&[
            Some((0, 1, 0x0fff, "Normal", JC_RIGHT, BOLD)),
            None,
            Some((65, 2, 0, "Strong,Forte", &[], BOLD)),
            Some((105, 3, 0xfff, "", LEFT_100, &[])),
        ]);
        let sh = sheet(&t);
        let f = sh.stshif;
        assert_eq!(
            (f.cstd, f.cb_std_base_in_file, f.std_stylenames_written, f.ftc_asci, f.ftc_fe, f.ftc_other, f.ftc_bi),
            (4, 10, true, 1, 2, 3, 4)
        );
        assert_eq!(sh.styles.len(), 4);
        let n = sh.styles[0].as_ref().unwrap();
        assert!(n.name.eq_str("Normal") && n.aliases.is_none());
        assert_eq!((n.sti, n.stk, n.istd_base, n.priority), (0, 1, 0x0fff, None));
        assert_eq!((n.papx, n.chpx, n.tapx), (Some(JC_RIGHT), Some(BOLD), None));
        assert!(sh.styles[1].is_none());
        let c = sh.styles[2].as_ref().unwrap();
        assert!(c.name.eq_str("Strong") && c.aliases.as_ref().unwrap().eq_str("Forte"));
        assert_eq!((c.stk, c.istd_base, c.chpx, c.papx), (2, 0, Some(BOLD), None));
        let tb = sh.styles[3].as_ref().unwrap();
        assert_eq!((tb.stk, tb.tapx, tb.papx, tb.chpx), (3, Some(&[][..]), Some(LEFT_100), Some(&[][..])));
    }

    #[test]
    fn a_table_past_the_stream_is_the_empty_sheet() {
        let sh = read_stsh_at(FcLcb { fc: 4, lcb: 100 }, &[0; 8], &DocLimits::DEFAULT).unwrap();
        assert_eq!((sh.stshif.cstd, sh.stshif.cb_std_base_in_file, sh.styles.len()), (0, 10, 0));
        let sh = read_stsh_at(FcLcb::default(), &[0; 8], &DocLimits::DEFAULT).unwrap();
        assert!(sh.styles.is_empty());
    }

    #[test]
    fn resolves_the_inheritance_and_keeps_the_own_delta() {
        let t = table(&[
            Some((0, 1, 0x0fff, "Normal", LEFT_100, HPS_30)),
            Some((1, 1, 0, "heading 1", JC_RIGHT, BOLD)),
            Some((65, 2, 0xfff, "Strong", &[], BOLD)),
        ]);
        let mut r = StyleResolver::new(sheet(&t));
        let h1 = r.resolve(1);
        assert_eq!((h1.pap.istd, h1.pap.dxa_left, h1.pap.jc, h1.pap.widow_control), (1, Some(100), Some(1), Some(true)));
        assert_eq!(h1.own_pap.keys(), &[PapKey::Jc]);
        assert_eq!((h1.chp.hps, h1.chp.bold, h1.chp.ftc_bi), (Some(30), Some(true), Some(4)));
        assert_eq!(h1.own_chp.keys(), &[ChpKey::Bold]);
        assert_eq!(h1.pap.tabs_cleared, Some(vec![]));
        // the parent's result is memoized and shared
        let n = r.resolve(0);
        assert!(Arc::ptr_eq(&n, &r.resolve(0)));
        assert_eq!(n.own_pap.keys(), &[PapKey::DxaLeft]);
        // a style that does not exist resolves to the defaults
        let missing = r.resolve(777);
        assert_eq!((missing.pap.istd, missing.pap.widow_control, missing.chp.hps), (777, Some(true), Some(20)));
        // a character style: its own delta and the chain over a paragraph's CHP
        let (chp, own) = r.resolve_char(2);
        assert_eq!((chp.bold, chp.hps, own.keys()), (Some(true), Some(20), &[ChpKey::Bold][..]));
        let on_top = Chp { italic: Some(true), ..Chp::default() };
        let x = r.apply_char_style(2, &on_top);
        assert_eq!((x.bold, x.italic, x.istd), (Some(true), Some(true), Some(2)));
        // not a character style: the chain is empty
        assert_eq!(r.apply_char_style(1, &on_top).bold, None);
    }

    #[test]
    fn a_chain_past_32_and_a_cycle_depend_on_the_order_of_the_calls() {
        // style i is based on i - 1; style 0 turns widow control off, the others only set the justification
        const WIDOW_OFF: &[u8] = &[0x31, 0x24, 0];
        let jcs: Vec<[u8; 3]> = (0..40u8).map(|i| [0x61, 0x24, i % 4]).collect();
        let specs: Vec<Spec<'_>> = (0..40u16)
            .map(|i| {
                let papx = if i == 0 { WIDOW_OFF } else { &jcs[usize::from(i)][..] };
                Some((200, 1, if i == 0 { 0x0fff } else { i - 1 }, "s", papx, &[][..]))
            })
            .collect();
        let t = table(&specs);
        // from the top, style 7 is 32 links below style 39: it stands on the defaults (widow control on)
        let mut a = StyleResolver::new(sheet(&t));
        a.resolve(39);
        assert_eq!(a.resolve(7).pap.widow_control, Some(true));
        // from the bottom up it inherits style 0 (widow control off)
        let mut b = StyleResolver::new(sheet(&t));
        for i in 0..40 {
            b.resolve(i);
        }
        assert_eq!(b.resolve(7).pap.widow_control, Some(false));
        assert_eq!(a.resolve(39).pap.jc, b.resolve(39).pap.jc);
        // a cycle is cut at depth 32 and terminates
        let cyc: Vec<Spec<'_>> = vec![Some((0, 1, 1, "a", &[], &[])), Some((0, 1, 0, "b", &[], &[]))];
        let tc = table(&cyc);
        let mut c = StyleResolver::new(sheet(&tc));
        assert_eq!(c.resolve(0).pap.istd, 0);
        assert_eq!(c.resolve(1).pap.istd, 1);
    }

    #[test]
    fn the_name_budget_stops_overlapping_names() {
        // 300 styles of three bytes each, every name claiming 0xFFFF units over the same bytes
        let mut t = Vec::new();
        t.extend(18u16.to_le_bytes());
        let mut stshif = [0u8; 18];
        stshif[0..2].copy_from_slice(&300u16.to_le_bytes());
        stshif[2..4].copy_from_slice(&1u16.to_le_bytes());
        t.extend(stshif);
        for _ in 0..300 {
            t.extend(1u16.to_le_bytes());
            t.extend([0xff, 0xff]);
        }
        t.extend(vec![0x41u8; 4000]);
        let pair = FcLcb { fc: 0, lcb: u32::try_from(t.len()).unwrap() };
        let tight = DocLimits { max_string_units: 1000, ..DocLimits::DEFAULT };
        assert_eq!(read_stsh_at(pair, &t, &tight).unwrap_err(), DocError::Limit { kind: DocLimitKind::StringUnits, max: 1000 });
        assert!(read_stsh_at(pair, &t, &DocLimits::DEFAULT).is_ok());
    }
}
