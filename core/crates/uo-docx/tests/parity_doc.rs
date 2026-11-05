//! Replay of `tests/parity/doc.capture` (`js/capture-doc.mjs`, the frozen TS `docx-core/src/doc/**`):
//! the 11 `.doc` fixtures, generated mutations of them (byte edits and cuts, applied to the fixture
//! bytes: `edits=pos:val,...`, `cut=len`) and generated grpprls. The canonical streams mirror the
//! script's functions of the same name; every facet of a frontier that exists is compared exactly,
//! and the result of the comparison is asserted (`Report::assert_clean`): zero mismatches.
//!
//! Facets replayed here: F0 `input`, `isword`; F1 `rd`, `fib`, `text`, `fkp`, `fonts`, `sprm`; F2 `chp`, `chp2`, `pap`, `pap2`,
//! `pap3`, `tap`, `sep` (the appliers over every grpprl of the file and every generated one); F3 `stsh`, `stsh2`, `stsh3`,
//! `lists` (the files and 700 generated style sheets and 700 generated list tables); F5 `rpr`, `ppr`, `xml.styles`,
//! `styleids`, `xml.numbering`, `xml.settings`, `xml.settings2`, `dop` (also over 800 generated DOPs). The facets of the
//! frontiers not ported yet (`convert`, `parts`, `model`: F6) are skipped by name in [`NOT_YET`] and counted in the output.
//! `UO_DOC_SHOW=<key prefix>` prints the first mismatches under a prefix (with the start of a generated input's hex).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-doc.mjs`.
#![cfg(feature = "doc")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::borrow::Cow;
use std::collections::BTreeMap;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::doc::DocLimits;
use uo_docx::doc::bytes::ByteReader;
use uo_docx::doc::convert_doc_to_docx;
use uo_docx::doc::fib::{FcLcb, fc};
use uo_docx::doc::fkp::{FcRun, read_chpx_runs, read_papx_runs, read_plc};
use uo_docx::doc::fonts::{DocFont, read_fonts};
use uo_docx::doc::formfields::ff_data_xml;
use uo_docx::doc::lists::{Lists, Lvl, read_lists, read_lists_at};
use uo_docx::doc::numbering::build_numbering;
use uo_docx::doc::ooxml_props::{PPrOpts, PropCtx, p_pr_inner, r_pr_inner};
use uo_docx::doc::open::DocStreams;
use uo_docx::doc::pictures::{PicFx, PicInfo, pic_fx_of, read_inline_picture, shape_blip};
use uo_docx::doc::props::{
    Brc, Chp, ChpCtx, ChpKey, FieldRef, KeySet, Pap, PapCtx, PapKey, Sep, Shd, TabStop, Tap, Tc, Width, apply_chp, apply_pap, apply_sep,
    apply_tap, default_sep, finish_tap, prune_none_borders,
};
use uo_docx::doc::reader::{DocReader, NoteKind};
use uo_docx::doc::settings::{Dop, build_settings, read_dop};
use uo_docx::doc::sprm::prls;
use uo_docx::doc::stsh::{ResolvedStyle, StyleResolver, StyleSheet, read_stsh, read_stsh_at};
use uo_docx::doc::styles_xml::{STI_NAMES_LEN, StyleIds, build_styles, style_id_map};
use uo_docx::doc::text::{decode_text, read_piece_table, total_cp};
use uo_docx::doc::{DocError, DocLimitKind};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::text::Utf16;
use uo_drawingml::odraw::Shape;
use uo_opc::Package;
use uo_testkit::{Capture, Digest, Replay, Stream};

mod doc_support;
mod support;

use doc_support::{CAPTURE, input_bytes, le16, unhex};

/// What the facets `parts` and `model` digest: the docx the converter wrote (the TS `unzipParts` and `modelOf`).
fn parts_and_model(zip: &[u8]) -> (String, String) {
    let pkg = Package::open(zip, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let mut s = Stream::new();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    s.tag(b'Z').u32(u32::try_from(names.len()).unwrap());
    for n in &names {
        s.str(n).digest_of(pkg.part(n).unwrap().unwrap_or(&[]));
    }
    let parts = summary(s);
    let model = match read_document(&pkg, &ReadOptions::default()) {
        Ok(doc) => {
            let mut main = Sink(Stream::new());
            let mut order = Sink(Stream::new());
            {
                let mut w = CanonWriter::new(&mut main).with_order(&mut order).with_media(&doc.media);
                doc.canon_full(&mut w);
            }
            format!("{}/{}", summary(main.0), summary(order.0))
        }
        Err(_) => "err".to_owned(),
    };
    (parts, model)
}

fn summary(s: Stream) -> String {
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

// ───────────────────────── canonical streams (mirrored in js/capture-doc.mjs) ─────────────────────────

fn prl_list(s: &mut Stream, grpprl: &[u8]) {
    let list: Vec<_> = prls(grpprl).collect();
    s.u32(u32::try_from(list.len()).unwrap());
    for p in list {
        s.u32(u32::from(p.sprm)).u64(p.op).u64(p.len);
    }
}

fn fib_digest(f: &uo_docx::doc::fib::Fib) -> String {
    let mut s = Stream::new();
    s.tag(b'F').u32(u32::from(f.w_ident)).u32(u32::from(f.n_fib)).u32(u32::from(f.lid));
    for b in [f.f_dot, f.f_complex, f.f_encrypted, f.f_which_tbl_stm, f.f_obfuscated, f.f_ext_char, f.f_far_east] {
        s.bool(b);
    }
    s.u32(u32::from(f.lid_fe));
    for v in [f.ccp_text, f.ccp_ftn, f.ccp_hdd, f.ccp_mcr, f.ccp_atn, f.ccp_edn, f.ccp_txbx, f.ccp_hdr_txbx] {
        s.i64(i64::from(v));
    }
    for i in 0..93 {
        let p = f.fc_lcb(i);
        s.u32(p.fc).u32(p.lcb);
    }
    summary(s)
}

fn runs_stream(runs: &[FcRun<'_>], tag: u8) -> Stream {
    let mut s = Stream::new();
    s.tag(tag).u32(u32::try_from(runs.len()).unwrap());
    for run in runs {
        s.u32(run.fc_start).u32(run.fc_end).u32(u32::from(run.istd)).digest_of(run.grpprl);
        prl_list(&mut s, run.grpprl);
    }
    s
}

fn fkp_digest(chpx: &[FcRun<'_>], papx: &[FcRun<'_>]) -> String {
    let mut s = Stream::new();
    let c = runs_stream(chpx, b'c').finish().short();
    let p = runs_stream(papx, b'p').finish().short();
    s.str(&c).str(&p).u32(u32::try_from(chpx.len()).unwrap()).u32(u32::try_from(papx.len()).unwrap());
    summary(s)
}

// ───────────────────────── F2: props ─────────────────────────

fn v_num(s: &mut Stream, v: Option<f64>) {
    match v {
        None => s.tag(b'-'),
        Some(x) => s.tag(b'n').f64(x),
    };
}

fn v_bool(s: &mut Stream, v: Option<bool>) {
    match v {
        None => s.tag(b'-'),
        Some(x) => s.tag(b'b').bool(x),
    };
}

fn v_brc(s: &mut Stream, v: Option<&Brc>) {
    match v {
        None => s.tag(b'-'),
        Some(b) => s
            .tag(b'B')
            .f64(f64::from(b.color))
            .f64(f64::from(b.width))
            .f64(f64::from(b.brc_type))
            .f64(f64::from(b.space))
            .bool(b.shadow)
            .bool(b.frame),
    };
}

fn v_shd(s: &mut Stream, v: Option<&Shd>) {
    match v {
        None => s.tag(b'-'),
        Some(h) => s.tag(b'S').f64(f64::from(h.fore)).f64(f64::from(h.back)).f64(f64::from(h.pat)),
    };
}

fn v_width(s: &mut Stream, v: Option<Width>) {
    match v {
        None => s.tag(b'-'),
        Some(w) => s.tag(b'W').f64(f64::from(w.fts)).f64(f64::from(w.w)),
    };
}

fn v_shds(s: &mut Stream, v: Option<&Vec<Shd>>) {
    match v {
        None => s.tag(b'-'),
        Some(list) => {
            s.tag(b'a').u32(u32::try_from(list.len()).unwrap());
            for h in list {
                v_shd(s, Some(h));
            }
            &mut *s
        }
    };
}

fn v_shorts(s: &mut Stream, v: &[Option<i16>]) {
    s.tag(b'a').u32(u32::try_from(v.len()).unwrap());
    for x in v {
        v_num(s, x.map(f64::from));
    }
}

/// A field of `Chp`/`Pap` (the `val` of the script).
fn v_field(s: &mut Stream, v: Option<FieldRef<'_>>) {
    let Some(v) = v else {
        s.tag(b'-');
        return;
    };
    match v {
        FieldRef::Bool(b) => s.tag(b'b').bool(b),
        FieldRef::Num(n) => s.tag(b'n').f64(n),
        FieldRef::Brc(_) => {
            v_brc(
                s,
                match v {
                    FieldRef::Brc(b) => Some(b),
                    _ => None,
                },
            );
            s
        }
        FieldRef::Shd(h) => {
            v_shd(s, Some(h));
            s
        }
        FieldRef::Symbol(y) => s.tag(b'Y').f64(f64::from(y.ftc)).f64(f64::from(y.xchar)),
        FieldRef::Line(l) => s.tag(b'L').f64(f64::from(l.line)).bool(l.mult),
        FieldRef::Tabs(t) => {
            s.tag(b'a').u32(u32::try_from(t.len()).unwrap());
            for x in t {
                s.tag(b'T').f64(f64::from(x.pos)).f64(f64::from(x.jc)).f64(f64::from(x.tlc));
            }
            s
        }
        FieldRef::Shorts(t) => {
            s.tag(b'a').u32(u32::try_from(t.len()).unwrap());
            for x in t {
                s.tag(b'n').f64(f64::from(*x));
            }
            s
        }
        FieldRef::Bytes(b) => s.tag(b'y').bytes(b),
    };
}

fn chp_digest(s: &mut Stream, chp: &Chp, touched: &KeySet<ChpKey>) {
    s.tag(b'C');
    for &k in ChpKey::ALL {
        v_field(s, chp.field(k));
    }
    s.u32(u32::try_from(touched.len()).unwrap());
    for k in touched.keys() {
        s.str(k.name());
    }
}

fn pap_digest(s: &mut Stream, pap: &Pap, touched: &KeySet<PapKey>, istds: &[u16]) {
    s.tag(b'P').f64(f64::from(pap.istd));
    for &k in PapKey::ALL {
        v_field(s, pap.field(k));
    }
    s.u32(u32::try_from(touched.len()).unwrap());
    for k in touched.keys() {
        s.str(k.name());
    }
    s.u32(u32::try_from(istds.len()).unwrap());
    for &v in istds {
        s.f64(f64::from(v));
    }
}

fn tc_digest(s: &mut Stream, tc: &Tc) {
    s.tag(b'c');
    for v in
        [tc.horz_merge.into(), tc.vert_merge.into(), tc.vert_align.into(), tc.text_flow.into(), tc.fts_width.into(), f64::from(tc.w_width)]
    {
        s.f64(v);
    }
    for b in [tc.fit_text, tc.no_wrap, tc.hide_mark] {
        s.bool(b);
    }
    for b in [&tc.brc_top, &tc.brc_left, &tc.brc_bottom, &tc.brc_right] {
        v_brc(s, b.as_ref());
    }
    for b in tc.brc_set {
        s.bool(b);
    }
    v_shd(s, tc.shd.as_ref());
    for m in tc.mar {
        v_num(s, m.map(f64::from));
    }
}

fn tap_digest(s: &mut Stream, tap: &Tap) {
    s.tag(b'A');
    v_num(s, tap.jc.map(f64::from));
    v_num(s, tap.dxa_left.map(f64::from));
    v_num(s, tap.dxa_gap_half.map(f64::from));
    v_num(s, tap.row_height.map(f64::from));
    v_bool(s, tap.cant_split);
    v_bool(s, tap.header);
    let b = &tap.borders;
    for x in [&b.top, &b.left, &b.bottom, &b.right, &b.inside_h, &b.inside_v] {
        v_brc(s, x.as_ref());
    }
    s.bool(tap.borders_set);
    s.u32(u32::try_from(tap.centers.len()).unwrap());
    for &c in &tap.centers {
        s.f64(c as f64);
    }
    s.u32(u32::try_from(tap.tcs.len()).unwrap());
    for tc in &tap.tcs {
        tc_digest(s, tc);
    }
    for w in [tap.table_width, tap.width_before, tap.width_after, tap.width_indent] {
        v_width(s, w);
    }
    v_bool(s, tap.autofit);
    for m in tap.padding {
        v_num(s, m.map(f64::from));
    }
    v_num(s, tap.spacing.map(f64::from));
    v_num(s, tap.istd.map(f64::from));
    match tap.tlp {
        None => s.tag(b'-'),
        Some(t) => s.tag(b'P').f64(f64::from(t.itl)).f64(f64::from(t.grfatl)),
    };
    v_shd(s, tap.shd_table.as_ref());
    v_bool(s, tap.bidi);
    v_num(s, tap.pc.map(f64::from));
    for v in [tap.dxa_abs, tap.dya_abs, tap.dxa_from_text, tap.dya_from_text, tap.dxa_from_text_right, tap.dya_from_text_bottom] {
        v_num(s, v.map(f64::from));
    }
    v_bool(s, tap.no_allow_overlap);
    v_bool(s, tap.insert_built);
    v_shds(s, tap.shd_raw.as_ref());
    v_shds(s, tap.shd_def.as_ref());
    v_shds(s, tap.shd80.as_ref());
}

fn sep_digest(s: &mut Stream, e: &Sep) {
    s.tag(b'E');
    let n = |s: &mut Stream, v: f64| {
        s.tag(b'n').f64(v);
    };
    let b = |s: &mut Stream, v: bool| {
        s.tag(b'b').bool(v);
    };
    n(s, f64::from(e.bkc));
    b(s, e.title_page);
    n(s, f64::from(e.ccol_m1));
    n(s, f64::from(e.dxa_columns));
    b(s, e.evenly_spaced);
    v_shorts(s, &e.col_widths);
    v_shorts(s, &e.col_spacings);
    b(s, e.l_between);
    for v in [
        e.xa_page.into(),
        e.ya_page.into(),
        e.orientation.into(),
        e.dxa_left.into(),
        e.dxa_right.into(),
        e.dya_top.into(),
        e.dya_bottom.into(),
    ] {
        n(s, v);
    }
    for v in [f64::from(e.dza_gutter), e.dya_hdr_top.into(), e.dya_hdr_bottom.into(), e.nfc_pgn.into()] {
        n(s, v);
    }
    b(s, e.pgn_restart);
    for v in [f64::from(e.pgn_start), e.vjc.into(), e.lnn_mod.into(), e.dxa_lnn.into(), e.lnc.into(), e.lnn_min.into()] {
        n(s, v);
    }
    for x in [&e.brc_top, &e.brc_left, &e.brc_bottom, &e.brc_right] {
        v_brc(s, x.as_ref());
    }
    n(s, f64::from(e.pgb_prop));
    n(s, f64::from(e.clm));
    v_num(s, e.dya_line_pitch.map(f64::from));
    n(s, f64::from(e.dxt_char_space));
    n(s, f64::from(e.text_flow));
    b(s, e.bidi);
    b(s, e.rtl_gutter);
    v_num(s, e.fpc.map(f64::from));
    v_num(s, e.rnc_ftn.map(f64::from));
    v_num(s, e.n_ftn.map(f64::from));
    v_num(s, e.nfc_ftn_ref.map(f64::from));
    v_num(s, e.rnc_edn.map(f64::from));
    v_num(s, e.n_edn.map(f64::from));
    v_num(s, e.nfc_edn_ref.map(f64::from));
    b(s, e.f_endnote);
    n(s, f64::from(e.dm_bin_first));
    n(s, f64::from(e.dm_bin_other));
    v_num(s, e.dm_paper_req.map(f64::from));
    v_bool(s, e.protected_sec);
    v_num(s, e.cns_pgn.map(f64::from));
    v_num(s, e.heading_pgn.map(f64::from));
}

fn para_chp() -> Chp {
    Chp { hps: Some(24), bold: Some(true), ico: Some(2), ftc_ascii: Some(3), istd: Some(5), ..Chp::default() }
}

fn char_style(istd: u16) -> Chp {
    let mut c = para_chp();
    c.istd = Some(istd);
    if istd & 1 == 1 {
        c.italic = Some(true);
    } else {
        c.hps = Some(30 + i32::from(istd % 7));
    }
    c
}

fn initial_pap(i: usize) -> Pap {
    let tab = |pos, jc, tlc| TabStop { pos, jc, tlc };
    match i {
        0 => Pap { istd: 0, ..Pap::default() },
        1 => Pap {
            istd: 3,
            dxa_left: Some(100),
            tabs: Some(vec![tab(720, 0, 0), tab(1440, 1, 2)]),
            tabs_cleared: Some(vec![]),
            out_lvl: Some(2),
            widow_control: Some(true),
            jc: Some(1),
            ..Pap::default()
        },
        _ => Pap { istd: 12, out_lvl: Some(5), tabs: Some(vec![tab(360, 3, 1)]), tabs_cleared: Some(vec![100]), ..Pap::default() },
    }
}

const PROP_FACETS: [&str; 9] = ["chp", "chp2", "pap", "pap2", "pap3", "tap", "sep", "rpr", "ppr"];

fn new_streams() -> [Stream; 9] {
    std::array::from_fn(|_| Stream::new())
}

/// The fonts and the `styleId` the generated inputs run with (`GEN_FONTS` and `genStyleId` of the script).
fn gen_fonts() -> Vec<DocFont> {
    [("Arial", 0u8), ("Times New Roman", 128), ("Calibri", 134), ("Ágata & <Co>", 238), ("MS Mincho", 136)]
        .iter()
        .map(|&(n, charset)| DocFont {
            name: Utf16::from(n),
            alt_name: None,
            prq: 0,
            true_type: false,
            ff: 0,
            weight: 0,
            charset,
            panose: Vec::new(),
            sig: [0; 6],
        })
        .collect()
}

fn gen_style_id(istd: u16) -> Option<Cow<'static, str>> {
    (istd % 4 != 3).then(|| Cow::Owned(format!("Style{istd}")))
}

fn gen_num_id(ilfo: u32) -> i64 {
    if ilfo.is_multiple_of(5) { 0 } else { i64::from(ilfo) + 100 }
}

fn units(s: &str) -> Vec<u8> {
    le16(&s.encode_utf16().collect::<Vec<u16>>())
}

/// The appliers over one grpprl, into the rolling streams (`applyProps` of the script), and the
/// `w:rPr` and `w:pPr` content the property emitters write for the results.
fn apply_props(st: &mut [Stream; 9], grpprl: &[u8]) {
    let fonts = gen_fonts();
    let ctx = PropCtx { fonts: &fonts, style_id: &gen_style_id };
    let (c1, t1) = {
        let mut chp = Chp::default();
        let mut touched = KeySet::new();
        for p in prls(grpprl) {
            apply_chp(&mut chp, &p, &mut ChpCtx::empty(), &mut touched);
        }
        chp_digest(&mut st[0], &chp, &touched);
        (chp, touched)
    };
    let (c2, t2) = {
        let mut chp = para_chp();
        let mut touched = KeySet::new();
        let ps = para_chp();
        let mut f = char_style;
        let mut ctx = ChpCtx { base: para_chp(), apply_char_style: Some(&mut f), para_style: Some(&ps) };
        for p in prls(grpprl) {
            apply_chp(&mut chp, &p, &mut ctx, &mut touched);
        }
        chp_digest(&mut st[1], &chp, &touched);
        (chp, touched)
    };
    let mut paps = Vec::new();
    for i in 0..3 {
        let mut pap = initial_pap(i);
        let base = initial_pap(i);
        let mut touched = KeySet::new();
        let mut istds = Vec::new();
        let mut rec = |v: u16| istds.push(v);
        let mut ctx = PapCtx { on_istd: Some(&mut rec) };
        for p in prls(grpprl) {
            apply_pap(&mut pap, &p, &mut touched, &mut ctx);
        }
        prune_none_borders(&pap, &base, &mut touched);
        pap_digest(&mut st[2 + i], &pap, &touched, &istds);
        paps.push((pap, touched, base));
    }
    {
        let mut tap = Tap::new();
        for p in prls(grpprl) {
            apply_tap(&mut tap, &p);
        }
        finish_tap(&mut tap);
        tap_digest(&mut st[5], &tap);
    }
    {
        let mut sep = default_sep();
        for p in prls(grpprl) {
            apply_sep(&mut sep, &p);
        }
        sep_digest(&mut st[6], &sep);
    }
    st[7].tag(b'R').bytes(&units(&r_pr_inner(&c1, &t1, &ctx, true))).bytes(&units(&r_pr_inner(&c1, &t1, &ctx, false)));
    st[7].bytes(&units(&r_pr_inner(&c2, &t2, &ctx, true)));
    for (i, (pap, touched, base)) in paps.iter().enumerate() {
        let opts = match i {
            0 => PPrOpts::default(),
            1 => PPrOpts { base_tabs: base.tabs.as_deref(), with_style: false, ..PPrOpts::default() },
            _ => PPrOpts {
                base_tabs: Some(&[]),
                num_id: Some(&gen_num_id),
                r_pr: Some("<w:b/>"),
                sect_pr: Some("<w:sectPr/>"),
                with_style: true,
            },
        };
        st[8].tag(b'P').bytes(&units(&p_pr_inner(pap, touched, &ctx, &opts)));
    }
}

fn check_streams(replay: &mut Replay<'_>, key: &str, st: [Stream; 9], count: usize) {
    for (name, s) in PROP_FACETS.iter().zip(st) {
        replay.check(key, name, &format!("{count}:{}", s.finish().short()));
    }
}

/// The facets of a file: every CHPX and PAPX grpprl, and the section grpprls through `applySep`.
fn replay_props(replay: &mut Replay<'_>, key: &str, streams: &DocStreams, chpx: &[FcRun<'_>], papx: &[FcRun<'_>]) {
    let mut st = new_streams();
    let mut n = 0;
    for run in chpx.iter().chain(papx.iter()) {
        apply_props(&mut st, run.grpprl);
        n += 1;
    }
    let p = streams.fib.fc_lcb(fc::PlcfSed);
    if let Some(plc) = read_plc(&streams.table, p.fc, p.lcb, 12) {
        let w = ByteReader::new(&streams.word);
        for i in 0..plc.n {
            let fc_sepx = plc.r.u32(plc.data_at(i) + 2);
            let mut sep = default_sep();
            if fc_sepx != 0xffff_ffff && u64::from(fc_sepx) + 2 <= w.len() {
                let cb = u64::from(w.i16(u64::from(fc_sepx)).max(0).unsigned_abs());
                for q in prls(w.sub(u64::from(fc_sepx) + 2, u64::from(fc_sepx) + 2 + cb)) {
                    apply_sep(&mut sep, &q);
                }
            }
            sep_digest(&mut st[6], &sep);
            n += 1;
        }
    }
    check_streams(replay, key, st, n);
}

// ───────────────────────── F3: stsh and lists ─────────────────────────

fn v_opt<T>(s: &mut Stream, v: Option<T>, f: impl FnOnce(&mut Stream, T)) {
    match v {
        None => {
            s.tag(b'-');
        }
        Some(x) => {
            s.tag(b'+');
            f(s, x);
        }
    }
}

fn stsh_tables(s: &mut Stream, sheet: &StyleSheet<'_>) {
    let f = &sheet.stshif;
    s.tag(b'H');
    for v in [f.cstd, f.cb_std_base_in_file, f.sti_max_when_saved, f.ftc_asci, f.ftc_fe, f.ftc_other, f.ftc_bi] {
        s.f64(f64::from(v));
    }
    s.bool(f.std_stylenames_written);
    s.u32(u32::try_from(sheet.styles.len()).unwrap());
    for st in &sheet.styles {
        let Some(st) = st else {
            s.tag(b'-');
            continue;
        };
        s.tag(b'D');
        for v in [f64::from(st.istd), st.sti.into(), st.stk.into(), st.istd_base.into(), st.istd_next.into()] {
            s.f64(v);
        }
        v_opt(s, st.istd_link, |s, v| {
            s.f64(f64::from(v));
        });
        s.bytes(&le16(st.name.units()));
        v_opt(s, st.aliases.as_ref(), |s, v| {
            s.bytes(&le16(v.units()));
        });
        for b in [st.auto_redef, st.hidden, st.semi_hidden, st.locked, st.unhide_when_used, st.q_format] {
            s.bool(b);
        }
        v_opt(s, st.priority, |s, v| {
            s.f64(f64::from(v));
        });
        for u in [st.papx, st.chpx, st.tapx] {
            v_opt(s, u, |s, v| {
                s.bytes(v);
            });
        }
    }
}

fn resolved_digest(s: &mut Stream, res: &ResolvedStyle) {
    pap_digest(s, &res.pap, &res.own_pap, &[]);
    chp_digest(s, &res.chp, &res.own_chp);
}

/// The facets `stsh`, `stsh2` and `stsh3` of a style sheet (`stshFacets` of the script).
fn replay_stsh(replay: &mut Replay<'_>, key: &str, sheet: StyleSheet<'_>) {
    let n = sheet.styles.len();
    let ids: Vec<u16> = (0..n).chain([n, 0x0fff, 0xffff]).map(|i| u16::try_from(i).unwrap()).collect();
    let mut a = Stream::new();
    stsh_tables(&mut a, &sheet);
    let mut ra = StyleResolver::new(sheet.clone());
    for &i in &ids {
        resolved_digest(&mut a, &ra.resolve(i));
    }
    let mut b = Stream::new();
    let mut rb = StyleResolver::new(sheet.clone());
    for &i in ids.iter().rev() {
        b.f64(f64::from(i));
        resolved_digest(&mut b, &rb.resolve(i));
    }
    let mut c = Stream::new();
    let mut rc = StyleResolver::new(sheet);
    c.tag(b'Q');
    chp_digest(&mut c, &rc.root_chp, &KeySet::new());
    for &i in &ids {
        let (chp, own) = rc.resolve_char(i);
        chp_digest(&mut c, &chp, &own);
        let root = rc.root_chp.clone();
        let x = rc.apply_char_style(i, &root);
        chp_digest(&mut c, &x, &KeySet::new());
        let y = rc.apply_char_style(i, &para_chp());
        chp_digest(&mut c, &y, &KeySet::new());
    }
    replay.check(key, "stsh", &summary(a));
    replay.check(key, "stsh2", &summary(b));
    replay.check(key, "stsh3", &summary(c));
}

fn lvl_digest(s: &mut Stream, l: &Lvl<'_>) {
    s.tag(b'V');
    for v in [l.start.into(), l.nfc.into(), l.jc.into(), l.follow.into(), l.ilvl_restart_lim.into()] {
        s.f64(v);
    }
    for b in [l.legal, l.no_restart, l.tentative] {
        s.bool(b);
    }
    s.bytes(&le16(l.xst.units()));
    s.u32(u32::try_from(l.rgbxch_nums.len()).unwrap());
    for &n in &l.rgbxch_nums {
        s.f64(f64::from(n));
    }
    s.bytes(l.grpprl_papx).bytes(l.grpprl_chpx);
}

fn lists_digest(lists: &Lists<'_>) -> String {
    let mut s = Stream::new();
    s.tag(b'L').u32(u32::try_from(lists.lsts.len()).unwrap());
    for l in &lists.lsts {
        s.f64(f64::from(l.lsid)).f64(f64::from(l.tplc)).bool(l.simple).bool(l.hybrid);
        for v in l.rgistd_para {
            s.f64(f64::from(v));
        }
        s.u32(u32::try_from(l.levels.len()).unwrap());
        for lv in &l.levels {
            lvl_digest(&mut s, lv);
        }
    }
    s.u32(u32::try_from(lists.lfos.len()).unwrap());
    for f in &lists.lfos {
        s.f64(f64::from(f.lsid)).u32(u32::try_from(f.overrides.len()).unwrap());
        for o in &f.overrides {
            s.f64(f64::from(o.ilvl));
            v_opt(&mut s, o.start_at, |s, v| {
                s.f64(f64::from(v));
            });
            v_opt(&mut s, o.lvl.as_ref(), |s, v| lvl_digest(s, v));
        }
    }
    summary(s)
}

/// Whether resolving the styles in ascending and in descending order gives different results (a chain past 32
/// or a cycle: the memo keeps what the first resolution computed).
fn order_dependent(sheet: &StyleSheet<'_>) -> bool {
    let n = sheet.styles.len();
    let mut a = StyleResolver::new(sheet.clone());
    let mut b = StyleResolver::new(sheet.clone());
    let asc: Vec<_> = (0..n).map(|i| a.resolve(u16::try_from(i).unwrap())).collect();
    let mut desc: Vec<_> = (0..n).rev().map(|i| b.resolve(u16::try_from(i).unwrap())).collect();
    desc.reverse();
    asc != desc
}

fn replay_gen_stsh(replay: &mut Replay<'_>, key: &str, rec: &uo_testkit::capture::Record, stats: &mut Stats) {
    replay.skip(key, "hex");
    let table = unhex(rec.get("hex").unwrap());
    let pair = FcLcb { fc: 0, lcb: u32::try_from(table.len()).unwrap() };
    let sheet = read_stsh_at(pair, &table, &DocLimits::DEFAULT).unwrap();
    stats.sheets += 1;
    stats.styles += sheet.styles.iter().flatten().count();
    stats.order_dependent += usize::from(order_dependent(&sheet));
    let ids = style_id_map(&sheet.styles);
    let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
    let fonts = gen_fonts();
    let ctx = PropCtx { fonts: &fonts, style_id: &sid };
    let mut resolver = StyleResolver::new(sheet.clone());
    let xml = build_styles(&mut resolver, &fonts, &ctx, &ids, &|n| if (1..=3).contains(&n) { i64::from(n) } else { 0 });
    replay.check(key, "xml.styles", &xml_digest(Some(&xml)));
    replay.check(key, "styleids", &ids_digest(&sheet, &ids));
    replay_stsh(replay, key, sheet);
}

fn replay_gen_lists(replay: &mut Replay<'_>, key: &str, rec: &uo_testkit::capture::Record, stats: &mut Stats) {
    for f in ["hex", "lst", "lfo"] {
        replay.skip(key, f);
    }
    let table = unhex(rec.get("hex").unwrap());
    let pair = |f: &str| {
        let (a, b) = rec.get(f).unwrap().split_once(':').unwrap();
        FcLcb { fc: a.parse().unwrap(), lcb: b.parse().unwrap() }
    };
    let lists = read_lists_at(pair("lst"), pair("lfo"), &table);
    stats.lists += lists.lsts.len();
    stats.levels += lists.lsts.iter().map(|l| l.levels.len()).sum::<usize>();
    stats.overrides += lists.lfos.iter().map(|l| l.overrides.len()).sum::<usize>();
    stats.formatted += lists.lfos.iter().flat_map(|l| &l.overrides).filter(|o| o.lvl.is_some()).count();
    replay.check(key, "lists", &lists_digest(&lists));
    let fonts = gen_fonts();
    let sid = |i: u16| (!i.is_multiple_of(3)).then(|| Cow::Owned(format!("S{i}")));
    let ctx = PropCtx { fonts: &fonts, style_id: &sid };
    replay.check(key, "xml.numbering", &xml_digest(build_numbering(&lists, &ctx).as_deref()));
}

// ───────────────────────── F5: the part emitters ─────────────────────────

fn xml_digest(text: Option<&str>) -> String {
    let mut s = Stream::new();
    s.tag(b'X').bytes(&units(text.unwrap_or(""))).bool(text.is_none());
    summary(s)
}

/// The key order of `DOP_KEYS` in the script (the replay checks the metadata).
const DOP_KEYS: &str = "facingPages,mirrorMargins,dxaTab,dxaHotZ,cConsecHypLim,autoHyphen,hyphCapitals,revMarking,fpc,rncFtn,nFtn,rncEdn,nEdn,epc,zoom,copts60,copts80hi,copts,copts2,protForms,lockAtn,lockRev,lockAtnReadOnly,styleLock,enforceDocProt,docProtCur,printFormData,saveFormData,shadeFormData,includeSubdocsInStats,snapBorder,includeHeader,includeFooter,kerningPunct,justification";

/// `n_fib`: the TS holds `copts` as the signed result of `|` (an int32) when the version is 0xD9 or older, and
/// `copts2` always (`| 1`); the port keeps the bit patterns as `u32`, so the digest reads them as `i32` there.
fn dop_digest(d: &Dop, n_fib: u16) -> String {
    let mut s = Stream::new();
    s.tag(b'O');
    let b = |s: &mut Stream, v: bool| {
        s.tag(b'b').bool(v);
    };
    let n = |s: &mut Stream, v: f64| {
        s.tag(b'n').f64(v);
    };
    b(&mut s, d.facing_pages);
    b(&mut s, d.mirror_margins);
    n(&mut s, f64::from(d.dxa_tab));
    n(&mut s, f64::from(d.dxa_hot_z));
    n(&mut s, f64::from(d.c_consec_hyp_lim));
    b(&mut s, d.auto_hyphen);
    b(&mut s, d.hyph_capitals);
    b(&mut s, d.rev_marking);
    for v in [
        f64::from(d.fpc),
        d.rnc_ftn.into(),
        d.n_ftn.into(),
        d.rnc_edn.into(),
        d.n_edn.into(),
        d.epc.into(),
        d.zoom.into(),
        d.copts60.into(),
        d.copts80hi.into(),
    ] {
        n(&mut s, v);
    }
    for (i, v) in [d.copts, d.copts2].into_iter().enumerate() {
        match v {
            None => {
                s.tag(b'-');
            }
            Some(v) if i == 1 || n_fib <= 0xd9 => n(&mut s, f64::from(v as i32)),
            Some(v) => n(&mut s, f64::from(v)),
        }
    }
    for v in [d.prot_forms, d.lock_atn, d.lock_rev, d.lock_atn_read_only, d.style_lock, d.enforce_doc_prot] {
        b(&mut s, v);
    }
    n(&mut s, f64::from(d.doc_prot_cur));
    for v in [
        d.print_form_data,
        d.save_form_data,
        d.shade_form_data,
        d.include_subdocs_in_stats,
        d.snap_border,
        d.include_header,
        d.include_footer,
        d.kerning_punct,
    ] {
        b(&mut s, v);
    }
    n(&mut s, f64::from(d.justification));
    match d.grid {
        None => {
            s.tag(b'-');
        }
        Some(g) => {
            s.tag(b'G');
            for v in [g.xa, g.ya, g.dxa, g.dya, g.dy_display, g.dx_display] {
                s.f64(f64::from(v));
            }
            s.bool(g.follow_margins);
        }
    }
    summary(s)
}

/// The facets `xml.styles`, `styleids`, `xml.numbering`, `xml.settings`, `xml.settings2` and `dop` of a file
/// (`partFacets` of the script): the emitters over a fresh resolver, as the TS over a fresh reader.
fn replay_parts(replay: &mut Replay<'_>, key: &str, streams: &DocStreams, sheet: &StyleSheet<'_>, lists: &Lists<'_>, fonts: &[DocFont]) {
    let ids = style_id_map(&sheet.styles);
    let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
    let ctx = PropCtx { fonts, style_id: &sid };
    let lfo_count = lists.lfos.len() as u64;
    let num_id_of = |n: u32| if n >= 1 && u64::from(n) <= lfo_count { i64::from(n) } else { 0 };
    let mut resolver = StyleResolver::new(sheet.clone());
    let styles = build_styles(&mut resolver, fonts, &ctx, &ids, &num_id_of);
    replay.check(key, "xml.styles", &xml_digest(Some(&styles)));
    replay.check(key, "styleids", &ids_digest(sheet, &ids));
    replay.check(key, "xml.numbering", &xml_digest(build_numbering(lists, &ctx).as_deref()));
    let p = streams.fib.fc_lcb(fc::Dop);
    let dop: &[u8] = if p.lcb != 0 && u64::from(p.fc) + u64::from(p.lcb) <= streams.table.len() as u64 {
        &streams.table[p.fc as usize..(u64::from(p.fc) + u64::from(p.lcb)) as usize]
    } else {
        &[]
    };
    let n_fib = streams.fib.n_fib;
    replay.check(key, "xml.settings", &xml_digest(Some(&build_settings(dop, n_fib, fonts, sheet.stshif.ftc_fe, false, false))));
    replay.check(key, "xml.settings2", &xml_digest(Some(&build_settings(dop, n_fib, fonts, sheet.stshif.ftc_fe, true, true))));
    replay.check(key, "dop", &dop_digest(&read_dop(dop, n_fib), n_fib));
}

/// The `styleids` facet: the TS `Map` of `styleIdMap` in insertion order (built-in styles by `istd`, then the others).
fn ids_digest(sheet: &StyleSheet<'_>, ids: &StyleIds) -> String {
    let mut order: Vec<_> = sheet.styles.iter().flatten().collect();
    order.sort_by_key(|s| (usize::from(s.sti) >= STI_NAMES_LEN, s.istd));
    let mut s = Stream::new();
    s.tag(b'I').u32(u32::try_from(ids.len()).unwrap());
    for st in order {
        s.f64(f64::from(st.istd)).str(ids.get(st.istd).unwrap());
    }
    summary(s)
}

fn replay_gen_dop(replay: &mut Replay<'_>, key: &str, rec: &uo_testkit::capture::Record) {
    for f in ["hex", "nfib", "ftcfe"] {
        replay.skip(key, f);
    }
    let bytes = unhex(rec.get("hex").unwrap());
    let n_fib: u16 = rec.get("nfib").unwrap().parse().unwrap();
    let ftc_fe: u16 = rec.get("ftcfe").unwrap().parse().unwrap();
    let fonts = gen_fonts();
    replay.check(key, "xml.settings", &xml_digest(Some(&build_settings(&bytes, n_fib, &fonts, ftc_fe, false, false))));
    replay.check(key, "xml.settings2", &xml_digest(Some(&build_settings(&bytes, n_fib, &fonts, ftc_fe, true, true))));
    replay.check(key, "dop", &dop_digest(&read_dop(&bytes, n_fib), n_fib));
}

// ───────────────────────── F4: the reader assembly ─────────────────────────

/// A result of the reader: the corpus stays inside `DocLimits`.
fn within<T>(key: &str, r: Result<T, DocError>) -> T {
    r.unwrap_or_else(|e| panic!("{key}: the corpus stays inside DocLimits: {e}"))
}

fn v_opt_i64(s: &mut Stream, v: Option<i64>) {
    match v {
        None => s.tag(b'-'),
        Some(x) => s.tag(b'+').i64(x),
    };
}

fn units_le(u: &[u16]) -> Vec<u8> {
    le16(u)
}

fn blip_digest(s: &mut Stream, b: Option<&uo_drawingml::odraw::Blip<'_>>) {
    match b {
        None => s.tag(b'-'),
        Some(b) => s.tag(b'+').str(b.kind.ext()).u64(b.bytes.len() as u64).digest_of(&b.bytes),
    };
}

/// `opt(s, f.bright, …)`, `opt(s, f.contrast, …)`, `opt(s, f.gray, …)`, `opt(s, f.biLevel, …)`.
fn fx_fields(s: &mut Stream, f: &PicFx) {
    for v in [f.bright, f.contrast] {
        match v {
            None => s.tag(b'-'),
            Some(x) => s.tag(b'+').f64(x),
        };
    }
    for v in [f.gray, f.bi_level] {
        if v {
            s.tag(b'+').bool(true);
        } else {
            s.tag(b'-');
        }
    }
}

fn fx_digest(s: &mut Stream, fx: Option<&PicFx>) {
    match fx {
        None => {
            s.tag(b'-');
        }
        Some(f) => {
            s.tag(b'+');
            fx_fields(s, f);
        }
    }
}

fn pic_digest(s: &mut Stream, pic: Option<&PicInfo<'_>>) {
    let Some(p) = pic else {
        s.tag(b'-');
        return;
    };
    s.tag(b'+').f64(p.cx).f64(p.cy);
    match &p.name {
        None => s.tag(b'-'),
        Some(n) => s.tag(b'+').bytes(&units_le(n)),
    };
    blip_digest(s, p.blip.as_ref());
    match &p.crop {
        None => s.tag(b'-'),
        Some(c) => s.tag(b'+').f64(c.l).f64(c.t).f64(c.r).f64(c.b),
    };
    match &p.hr {
        None => {
            s.tag(b'-');
        }
        Some(h) => {
            s.tag(b'+').str(h.align);
            match h.pct {
                None => s.tag(b'-'),
                Some(v) => s.tag(b'+').f64(f64::from(v)),
            };
            match h.fill {
                None => s.tag(b'-'),
                Some(v) => s.tag(b'+').f64(f64::from(v)),
            };
            s.bool(h.noshade);
        }
    };
    fx_digest(s, p.fx.as_ref());
}

fn shape_digest(s: &mut Stream, rd: &DocReader<'_>, shape: &Shape<'_>) {
    s.f64(f64::from(shape.spid))
        .f64(f64::from(shape.shape_type))
        .f64(f64::from(shape.flags))
        .bool(rd.drawings.masters.contains(&shape.spid));
    match shape.z {
        None => s.tag(b'-'),
        Some(z) => s.tag(b'+').f64(f64::from(z)),
    };
    blip_digest(s, shape_blip(rd, shape).as_ref());
    fx_digest(s, pic_fx_of(Some(shape)).as_ref());
    s.u32(shape.children.as_ref().map_or(0xffff_ffff, |c| u32::try_from(c.len()).unwrap()));
    for c in shape.children.iter().flatten() {
        shape_digest(s, rd, c);
    }
}

/// The distinct `sprmCPicLocation` operands of the CHPX runs, in order (at most 64).
fn pic_locations(rd: &DocReader<'_>) -> Vec<i32> {
    let mut locs: Vec<i32> = Vec::new();
    for run in &rd.chpx {
        for p in prls(run.grpprl) {
            if p.sprm != 0x6a03 {
                continue;
            }
            let v = p.i32();
            if !locs.contains(&v) {
                locs.push(v);
            }
        }
        if locs.len() >= 64 {
            break;
        }
    }
    locs
}

/// The facets of the reader (`readerFacets` of the script) over a fresh reader, in the order of the script
/// (the pictures share the document's inflate budget).
fn replay_reader(replay: &mut Replay<'_>, key: &str, streams: &DocStreams, limits: &DocLimits) {
    let rd = match DocReader::new(streams, limits) {
        Ok(r) => r,
        Err(e) => panic!("{key}: the corpus stays inside DocLimits: {e}"),
    };
    let mut s = Stream::new();
    s.tag(b'S').u32(u32::try_from(rd.sections.len()).unwrap());
    for sec in &rd.sections {
        s.i64(sec.cp_start).i64(sec.cp_end);
        sep_digest(&mut s, &sec.sep);
    }
    replay.check(key, "sections", &summary(s));

    let mut s = Stream::new();
    s.tag(b'N');
    for kind in [NoteKind::Footnote, NoteKind::Endnote] {
        let l = rd.notes(kind);
        s.u32(u32::try_from(l.len()).unwrap());
        for n in &l {
            s.i64(n.ref_cp).bool(n.auto).i64(n.cp_start).i64(n.cp_end);
        }
    }
    replay.check(key, "notes", &summary(s));

    let mut s = Stream::new();
    let l = within(key, rd.comments());
    s.tag(b'M').u32(u32::try_from(l.len()).unwrap());
    for c in &l {
        s.i64(c.ref_cp).i64(c.cp_start).i64(c.cp_end).bytes(&units_le(c.author.units())).bytes(&units_le(c.initials.units()));
        v_opt_i64(&mut s, c.range_start);
        v_opt_i64(&mut s, c.range_end);
    }
    replay.check(key, "comments", &summary(s));

    let mut s = Stream::new();
    let l = within(key, rd.bookmarks());
    s.tag(b'K').u32(u32::try_from(l.len()).unwrap());
    for b in &l {
        s.bytes(&units_le(b.name.units())).i64(b.cp_start).i64(b.cp_end);
    }
    let a = within(key, rd.revision_authors());
    s.u32(u32::try_from(a.len()).unwrap());
    for x in &a {
        s.bytes(&units_le(x.units()));
    }
    replay.check(key, "bookmarks", &summary(s));

    let mut s = Stream::new();
    s.tag(b'U');
    for (a, b) in [rd.sub.main, rd.sub.ftn, rd.sub.hdd, rd.sub.atn, rd.sub.edn, rd.sub.txbx, rd.sub.hdr_txbx] {
        s.i64(a).i64(b);
    }
    let h = rd.header_stories();
    s.u32(u32::try_from(h.len()).unwrap());
    for x in &h {
        match x {
            None => s.tag(b'-'),
            Some((a, b)) => s.tag(b'+').i64(*a).i64(*b),
        };
    }
    replay.check(key, "subs", &summary(s));

    let mut s = Stream::new();
    s.tag(b'F');
    for hdr in [false, true] {
        let l = rd.fspas(hdr);
        s.u32(u32::try_from(l.len()).unwrap());
        for f in &l {
            s.i64(f.cp)
                .i64(i64::from(f.spid))
                .i64(i64::from(f.xa_left))
                .i64(i64::from(f.ya_top))
                .i64(i64::from(f.xa_right))
                .i64(i64::from(f.ya_bottom))
                .u32(u32::from(f.bx))
                .u32(u32::from(f.by))
                .u32(u32::from(f.wr))
                .u32(u32::from(f.wrk))
                .bool(f.below_text)
                .bool(f.anchor_lock);
        }
    }
    replay.check(key, "fspas", &summary(s));

    let mut s = Stream::new();
    s.tag(b'X');
    for hdr in [false, true] {
        let l = rd.textboxes(hdr);
        s.u32(u32::try_from(l.len()).unwrap());
        for t in &l {
            s.i64(i64::from(t.spid)).i64(t.cp_start).i64(t.cp_end);
        }
    }
    replay.check(key, "textboxes", &summary(s));

    let locs = pic_locations(&rd);
    let mut s = Stream::new();
    s.tag(b'D').u32(u32::try_from(locs.len()).unwrap());
    for &loc in &locs {
        s.i64(i64::from(loc));
        match ff_data_xml(rd.data, loc) {
            None => s.tag(b'-'),
            Some(x) => s.tag(b'+').bytes(&units_le(&x.encode_utf16().collect::<Vec<u16>>())),
        };
    }
    replay.check(key, "ffdata", &summary(s));

    let mut s = Stream::new();
    s.tag(b'I').u32(u32::try_from(locs.len()).unwrap());
    for &loc in &locs {
        s.i64(i64::from(loc));
        pic_digest(&mut s, read_inline_picture(&rd, loc).as_ref());
    }
    replay.check(key, "picf", &summary(s));

    let mut s = Stream::new();
    s.tag(b'H');
    for hdr in [false, true] {
        let l = rd.fspas(hdr);
        s.u32(u32::try_from(l.len()).unwrap());
        for f in &l {
            match rd.shape(i64::from(f.spid), hdr) {
                None => {
                    s.tag(b'-');
                }
                Some(sh) => {
                    s.tag(b'+');
                    shape_digest(&mut s, &rd, sh);
                }
            }
        }
    }
    match rd.drawings.background_shape() {
        None => s.tag(b'-'),
        Some(sh) => s.tag(b'+').f64(f64::from(sh.spid)),
    };
    for pib in [0u32, 1, 2, 3, 0xffff] {
        match rd.drawings.bse(pib) {
            None => s.tag(b'-'),
            Some(b) => s.tag(b'+').f64(f64::from(b.bt_win32)).f64(f64::from(b.size)).f64(f64::from(b.fo_delay)).bool(b.embedded.is_some()),
        };
    }
    replay.check(key, "shapes", &summary(s));
}

// ───────────────────────── replay of one input ─────────────────────────

/// What the generated tables contained (for the summary line).
#[derive(Default, Debug)]
struct Stats {
    sheets: usize,
    styles: usize,
    order_dependent: usize,
    lists: usize,
    levels: usize,
    overrides: usize,
    formatted: usize,
}

/// What a replay of the file facets measured (for the summary line).
#[derive(Default)]
struct Tally {
    classes: BTreeMap<&'static str, u32>,
}

fn replay_input(replay: &mut Replay<'_>, key: &str, rec: &uo_testkit::capture::Record, tally: &mut Tally) {
    let bytes = input_bytes(rec);
    replay.check(key, "input", &format!("{}:{}", bytes.len(), &Digest::of(&bytes).hex()[..16]));
    replay.check(key, "isword", if uo_docx::doc::is_word_binary(&bytes) { "1" } else { "0" });
    for (f, _) in &rec.facets {
        // `edits` and `cut` are the inputs themselves (applied above)
        if f == "edits" || f == "cut" {
            replay.skip(key, f);
        }
    }
    let limits = DocLimits::DEFAULT;
    match convert_doc_to_docx(&bytes, &limits) {
        Ok(zip) => {
            replay.check(key, "convert", "ok");
            let (parts, model) = parts_and_model(&zip);
            replay.check(key, "parts", &parts);
            replay.check(key, "model", &model);
        }
        // N1 (named in the record, "Stage 3 — D4"): a table or a text box nested past `max_nesting`; the TS
        // recurses until the JavaScript stack overflows (the catch-all, class `corrupt`), where the Rust
        // converter answers the limit
        Err(DocError::Limit { kind: DocLimitKind::Nesting, .. }) if replay.capture().get(key, "convert") == Some("corrupt") => {
            replay.accept(key, "convert", "limit", "N1: nesting past DocLimits::max_nesting where the TS overflows the JS stack");
            replay.skip(key, "parts");
            replay.skip(key, "model");
        }
        Err(e) => {
            replay.check(key, "convert", e.class());
        }
    }
    let streams = match DocStreams::open(&bytes) {
        Ok(s) => s,
        Err(e) => {
            *tally.classes.entry(e.class()).or_default() += 1;
            replay.check(key, "rd", e.class());
            return;
        }
    };
    let pt = match read_piece_table(&streams.fib, &streams.table) {
        Ok(p) => p,
        Err(e) => {
            *tally.classes.entry(e.class()).or_default() += 1;
            replay.check(key, "rd", e.class());
            return;
        }
    };
    replay.check(key, "rd", "ok");
    *tally.classes.entry("ok").or_default() += 1;
    replay.check(key, "fib", &fib_digest(&streams.fib));

    let total = total_cp(&streams.fib, &pt);
    let text = match decode_text(&pt, &streams.word, total, &limits) {
        Ok(t) => t,
        Err(DocError::Limit { .. }) => panic!("{key}: the corpus stays inside DocLimits"),
        Err(e) => panic!("{key}: {e}"),
    };
    let mut s = Stream::new();
    s.tag(b'T').u32(u32::try_from(pt.pieces.len()).unwrap());
    for p in &pt.pieces {
        s.u32(p.cp_start).u32(p.cp_end).f64(p.fc).bool(p.compressed).u32(u32::from(p.prm));
    }
    s.u32(u32::try_from(pt.prcs.len()).unwrap());
    for p in &pt.prcs {
        s.digest_of(p);
    }
    s.bytes(&le16(&text));
    replay.check(key, "text", &summary(s));

    let chpx = read_chpx_runs(&streams.fib, &streams.table, &streams.word);
    let papx = read_papx_runs(&streams.fib, &streams.table, &streams.word, streams.data.as_deref());
    replay.check(key, "fkp", &fkp_digest(&chpx, &papx));

    let fonts = read_fonts(&streams.fib, &streams.table);
    let mut s = Stream::new();
    s.tag(b'N').u32(u32::try_from(fonts.len()).unwrap());
    for f in &fonts {
        s.bytes(&le16(f.name.units())).bool(f.alt_name.is_some());
        if let Some(a) = &f.alt_name {
            s.bytes(&le16(a.units()));
        }
        s.u32(u32::from(f.prq)).bool(f.true_type).u32(u32::from(f.ff)).i64(i64::from(f.weight)).u32(u32::from(f.charset)).bytes(&f.panose);
        for v in f.sig {
            s.u32(v);
        }
    }
    replay.check(key, "fonts", &summary(s));

    replay_props(replay, key, &streams, &chpx, &papx);

    let sheet = read_stsh(&streams.fib, &streams.table, &limits).unwrap();
    replay_stsh(replay, key, sheet.clone());
    let lists = read_lists(&streams.fib, &streams.table);
    replay.check(key, "lists", &lists_digest(&lists));
    replay_parts(replay, key, &streams, &sheet, &lists, &fonts);
    replay_reader(replay, key, &streams, &limits);
}

fn replay_gen(replay: &mut Replay<'_>, key: &str, rec: &uo_testkit::capture::Record) {
    replay.skip(key, "hex");
    let g = unhex(rec.get("hex").unwrap());
    let mut s = Stream::new();
    prl_list(&mut s, &g);
    replay.check(key, "sprm", &summary(s));
    let mut st = new_streams();
    apply_props(&mut st, &g);
    check_streams(replay, key, st, 1);
}

#[test]
fn parity_doc() {
    let cap = Capture::parse(CAPTURE).unwrap();
    // the key lists of the script and of the macro are the same, in the same order
    let names = |keys: Vec<&str>| keys.join(",");
    assert_eq!(cap.meta("chp-keys").unwrap(), names(ChpKey::ALL.iter().map(|k| k.name()).collect()));
    assert_eq!(cap.meta("pap-keys").unwrap(), names(PapKey::ALL.iter().map(|k| k.name()).collect()));
    assert_eq!(cap.meta("dop-keys").unwrap(), DOP_KEYS);
    let mut replay = Replay::new(&cap);
    let mut tally = Tally::default();
    let mut stats = Stats::default();
    let (mut files, mut gens, mut gen_tables) = (0, 0, 0);
    for rec in cap.records() {
        if rec.key.starts_with("doc/") || rec.key.starts_with("mut/") {
            replay_input(&mut replay, &rec.key, rec, &mut tally);
            files += 1;
        } else if rec.key.starts_with("gen/grpprl/") {
            replay_gen(&mut replay, &rec.key, rec);
            gens += 1;
        } else if rec.key.starts_with("gen/stsh/") {
            replay_gen_stsh(&mut replay, &rec.key, rec, &mut stats);
            gen_tables += 1;
        } else if rec.key.starts_with("gen/dop/") {
            replay_gen_dop(&mut replay, &rec.key, rec);
            gen_tables += 1;
        } else if rec.key.starts_with("gen/lists/") {
            replay_gen_lists(&mut replay, &rec.key, rec, &mut stats);
            gen_tables += 1;
        } else {
            panic!("unknown record {}", rec.key);
        }
    }
    assert!(files >= 1600 && gens == 2500 && gen_tables == 2200, "the corpus changed: {files} files, {gens} grpprls");
    let report = replay.finish();
    eprintln!(
        "parity_doc: {} (files {files}, grpprls {gens}, generated tables {gen_tables}; reader classes {:?}; generated tables hold {stats:?})",
        report.summary(),
        tally.classes
    );
    if let Ok(prefix) = std::env::var("UO_DOC_SHOW") {
        // diagnostics: the first mismatches under a key prefix, with the grpprl of a generated input
        for m in report.mismatches.iter().filter(|m| m.key.starts_with(&prefix)).take(40) {
            let hex = cap.record(&m.key).and_then(|r| r.get("hex")).unwrap_or("");
            eprintln!("{} [{}] {}", m.key, m.facet, &hex[..hex.len().min(120)]);
        }
    }
    report.assert_clean();
}
