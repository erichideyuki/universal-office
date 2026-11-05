//! Lists of the `.doc` to `numbering.xml` (`numbering.ts`): each LSTF is a `w:abstractNum`
//! (`nsid` = `lsid`, `tmpl` = `tplc`) and each LFO a `w:num` (`numId` = the 1-based index of the
//! LFO, the value of `sprmPIlfo`), with `w:lvlOverride` for the LFOLVLs.

use std::collections::HashMap;

use super::lists::{Lists, Lvl};
use super::ooxml_props::{PPrOpts, PropCtx, num_fmt_of, p_pr_inner, r_pr_inner, val_attr};
use super::props::{Chp, ChpCtx, KeySet, Pap, PapCtx, apply_chp, apply_pap};
use super::sprm::prls;
use super::xmlw::{AttrVal, Attrs, XML_DECL, el, ns};

/// `hex8`: `(v >>> 0).toString(16).toUpperCase().padStart(8, '0')`.
fn hex8(v: i32) -> String {
    format!("{:08X}", v as u32)
}

/// `lvlText`: the `xst` of a level as `w:lvlText` (the markers become `%1`, `%2`…), UTF-16 units.
#[must_use]
pub fn lvl_text(lvl: &Lvl<'_>) -> Vec<u16> {
    let mut s: Vec<u16> = Vec::new();
    for (i, &c) in lvl.xst.units().iter().enumerate() {
        let marked = lvl.rgbxch_nums.iter().any(|&m| usize::from(m) == i + 1);
        if marked && c <= 8 {
            s.push(u16::from(b'%'));
            s.extend((u32::from(c) + 1).to_string().encode_utf16());
        } else {
            s.push(c);
        }
    }
    s
}

fn lvl_xml(lvl: &Lvl<'_>, ilvl: i64, ctx: &PropCtx<'_, '_>, p_style: Option<&str>) -> String {
    let int = |v: i64| Some(AttrVal::Int(v));
    let mut x = String::new();
    x += &el("w:start", &val_attr(int(i64::from(lvl.start))), "");
    x += &el("w:numFmt", &val_attr(Some(AttrVal::Str(num_fmt_of(lvl.nfc)))), "");
    if lvl.no_restart {
        x += &el("w:lvlRestart", &val_attr(int(i64::from(lvl.ilvl_restart_lim))), "");
    }
    if let Some(ps) = p_style
        && !ps.is_empty()
    {
        x += &el("w:pStyle", &val_attr(Some(AttrVal::Str(ps))), "");
    }
    if lvl.legal {
        x += "<w:isLgl/>";
    }
    if lvl.follow == 1 {
        x += &el("w:suff", &val_attr(Some(AttrVal::Str("space"))), "");
    } else if lvl.follow == 2 {
        x += &el("w:suff", &val_attr(Some(AttrVal::Str("nothing"))), "");
    }
    let text = lvl_text(lvl);
    x += &el("w:lvlText", &val_attr(Some(AttrVal::Units(&text))), "");
    let jc = ["left", "center", "right"].get(usize::from(lvl.jc)).copied().unwrap_or("left");
    x += &el("w:lvlJc", &val_attr(Some(AttrVal::Str(jc))), "");
    let mut pap = Pap::default();
    let mut pt = KeySet::new();
    let mut pctx = PapCtx::default();
    for p in prls(lvl.grpprl_papx) {
        apply_pap(&mut pap, &p, &mut pt, &mut pctx);
    }
    let ppr = p_pr_inner(&pap, &pt, ctx, &PPrOpts { with_style: false, base_tabs: Some(&[]), ..PPrOpts::default() });
    if !ppr.is_empty() {
        x += &format!("<w:pPr>{ppr}</w:pPr>");
    }
    let mut chp = Chp::default();
    let mut ct = KeySet::new();
    for p in prls(lvl.grpprl_chpx) {
        apply_chp(&mut chp, &p, &mut ChpCtx::empty(), &mut ct);
    }
    let rpr = r_pr_inner(&chp, &ct, ctx, false);
    if !rpr.is_empty() {
        x += &format!("<w:rPr>{rpr}</w:rPr>");
    }
    el("w:lvl", &Attrs::of(&[("w:ilvl", int(ilvl)), ("w:tentative", lvl.tentative.then_some(AttrVal::Str("1")))]), &x)
}

/// `buildNumbering`: `numbering.xml`, or `None` when the document has no list.
#[must_use]
pub fn build_numbering(lists: &Lists<'_>, ctx: &PropCtx<'_, '_>) -> Option<String> {
    if lists.lsts.is_empty() {
        return None;
    }
    let mut x = String::new();
    let mut abs_by_lsid: HashMap<i32, i64> = HashMap::new();
    for (i, l) in lists.lsts.iter().enumerate() {
        let i = i as i64;
        abs_by_lsid.insert(l.lsid, i);
        let mut c = String::new();
        c += &el("w:nsid", &Attrs::of(&[("w:val", Some(AttrVal::Str(&hex8(l.lsid))))]), "");
        let mlt = if l.simple {
            "singleLevel"
        } else if l.hybrid {
            "hybridMultilevel"
        } else {
            "multilevel"
        };
        c += &el("w:multiLevelType", &Attrs::of(&[("w:val", Some(AttrVal::Str(mlt)))]), "");
        c += &el("w:tmpl", &Attrs::of(&[("w:val", Some(AttrVal::Str(&hex8(l.tplc))))]), "");
        for (k, lvl) in l.levels.iter().enumerate() {
            let istd = l.rgistd_para.get(k).copied();
            let p_style = istd.filter(|&i| i != 0x0fff).and_then(|i| (ctx.style_id)(i));
            c += &lvl_xml(lvl, k as i64, ctx, p_style.as_deref());
        }
        x += &el("w:abstractNum", &Attrs::of(&[("w:abstractNumId", Some(AttrVal::Int(i)))]), &c);
    }
    for (i, lfo) in lists.lfos.iter().enumerate() {
        let Some(&abs) = abs_by_lsid.get(&lfo.lsid) else { continue };
        let mut c = el("w:abstractNumId", &Attrs::of(&[("w:val", Some(AttrVal::Int(abs)))]), "");
        for ov in &lfo.overrides {
            let mut o = String::new();
            if let Some(s) = ov.start_at {
                o += &el("w:startOverride", &Attrs::of(&[("w:val", Some(AttrVal::Int(i64::from(s))))]), "");
            }
            if let Some(l) = &ov.lvl {
                o += &lvl_xml(l, i64::from(ov.ilvl), ctx, None);
            }
            if !o.is_empty() {
                c += &el("w:lvlOverride", &Attrs::of(&[("w:ilvl", Some(AttrVal::Int(i64::from(ov.ilvl))))]), &o);
            }
        }
        x += &el("w:num", &Attrs::of(&[("w:numId", Some(AttrVal::Int(i as i64 + 1)))]), &c);
    }
    Some(format!("{XML_DECL}<w:numbering xmlns:w=\"{}\" xmlns:r=\"{}\">{x}</w:numbering>", ns::W, ns::R))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::bytes::ByteReader;
    use crate::doc::fib::FcLcb;
    use crate::doc::lists::{read_lists_at, read_lvl};
    use std::borrow::Cow;

    fn lvl_bytes(start: i32, nfc: u8, flags: u8, nums: &[u8], papx: &[u8], text: &[u16]) -> Vec<u8> {
        let mut b = vec![0u8; 28];
        b[0..4].copy_from_slice(&start.to_le_bytes());
        b[4] = nfc;
        b[5] = flags;
        b[6..6 + nums.len()].copy_from_slice(nums);
        b[15] = 2;
        b[25] = u8::try_from(papx.len()).unwrap();
        b.extend(papx);
        b.extend(u16::try_from(text.len()).unwrap().to_le_bytes());
        for u in text {
            b.extend(u.to_le_bytes());
        }
        b
    }

    #[test]
    fn markers_become_percent_numbers() {
        let mut t = vec![0u8];
        t.extend(lvl_bytes(1, 0, 0, &[1], &[], &[0, 0x2e]));
        let l = read_lists_at(FcLcb::default(), FcLcb::default(), &t);
        assert!(l.lsts.is_empty());
        let (lvl, _) = read_lvl(ByteReader::new(&t[1..]), 0, 100);
        assert_eq!(String::from_utf16_lossy(&lvl_text(&lvl)), "%1.");
        // a marker position with a code above 8 is not a marker; an unmarked code stays
        let bytes2 = lvl_bytes(1, 0, 0, &[1, 4], &[], &[9, 1, 0x61, 2]);
        let (lvl2, _) = read_lvl(ByteReader::new(&bytes2), 0, 100);
        assert_eq!(lvl_text(&lvl2), vec![9, 1, 0x61, u16::from(b'%'), u16::from(b'3')]);
    }

    #[test]
    fn builds_abstract_nums_and_nums() {
        // one list of one level (simple), referenced by two LFOs: the second has a start-at override; a third points nowhere
        let mut t = Vec::new();
        t.extend(1i16.to_le_bytes());
        let mut lstf = vec![0u8; 28];
        lstf[0..4].copy_from_slice(&0x1234i32.to_le_bytes());
        lstf[4..8].copy_from_slice(&7i32.to_le_bytes());
        lstf[8..10].copy_from_slice(&0x0fffu16.to_le_bytes());
        lstf[26] = 1;
        t.extend(lstf);
        let lcb = t.len() as u32;
        t.extend(lvl_bytes(3, 4, 0x04, &[1], &[0x0f, 0x84, 0xd0, 0x02], &[0, 0x29]));
        let lfo_fc = t.len() as u32;
        t.extend(3u32.to_le_bytes());
        for (lsid, n) in [(0x1234i32, 0u8), (0x1234, 1), (99, 0)] {
            t.extend(lsid.to_le_bytes());
            t.extend([0u8; 8]);
            t.push(n);
            t.extend([0u8; 3]);
        }
        t.extend([0u8; 4]);
        t.extend([0u8; 4]);
        t.extend(8i32.to_le_bytes());
        t.extend((0x10u32 | 1).to_le_bytes());
        t.extend([0u8; 4]);
        let lfo_lcb = t.len() as u32 - lfo_fc;
        let lists = read_lists_at(FcLcb { fc: 0, lcb }, FcLcb { fc: lfo_fc, lcb: lfo_lcb }, &t);
        let sid = |i: u16| (i == 3).then_some(Cow::Borrowed("Lista"));
        let fonts = [];
        let ctx = PropCtx { fonts: &fonts, style_id: &sid };
        let x = build_numbering(&lists, &ctx).unwrap();
        assert!(x.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:numbering xmlns:w="));
        assert!(x.contains("<w:abstractNum w:abstractNumId=\"0\"><w:nsid w:val=\"00001234\"/><w:multiLevelType w:val=\"singleLevel\"/><w:tmpl w:val=\"00000007\"/>"));
        assert!(x.contains(
            "<w:lvl w:ilvl=\"0\"><w:start w:val=\"3\"/><w:numFmt w:val=\"lowerLetter\"/><w:isLgl/><w:suff w:val=\"nothing\"/><w:lvlText w:val=\"%1)\"/>\
             <w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"720\"/></w:pPr></w:lvl>"
        ));
        assert!(x.contains("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>"));
        assert!(x.contains("<w:num w:numId=\"2\"><w:abstractNumId w:val=\"0\"/><w:lvlOverride w:ilvl=\"1\"><w:startOverride w:val=\"8\"/></w:lvlOverride></w:num>"));
        assert!(!x.contains("w:numId=\"3\""));
        assert_eq!(build_numbering(&Lists::default(), &ctx), None);
    }
}
