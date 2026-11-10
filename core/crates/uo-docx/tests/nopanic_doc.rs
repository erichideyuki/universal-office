//! No-panic loops of the `.doc` converter (D4): the reader pipeline over seeded mutations of the
//! fixtures (random byte edits, structure-aimed edits, cuts, zero and 0xFF fills, every size of
//! the FIB counters), over the capture's own mutations with tight limits, and the byte-level
//! readers (`prls`, `ByteReader`, `read_plc`, `xmlw`) over random bytes and strings. A panic or a
//! stack overflow fails the test; the result is not looked at here (the parity test looks at it),
//! except that a limit is a `DocError::Limit` value.
#![cfg(feature = "doc")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::doc_support;

use std::hint::black_box;

use doc_support::Rng;
use uo_docx::doc::bytes::ByteReader;
use uo_docx::doc::fkp::{read_chpx_runs, read_papx_runs, read_plc, run_at, run_index_from};
use uo_docx::doc::fonts::read_fonts;
use uo_docx::doc::formfields::ff_data_xml;
use uo_docx::doc::lists::read_lists;
use uo_docx::doc::open::DocStreams;
use uo_docx::doc::pictures::{pic_fx_of, read_inline_picture, shape_blip};
use uo_docx::doc::reader::{DocReader, NoteKind};
use uo_docx::doc::sprm::prls;
use uo_docx::doc::stsh::{StyleResolver, read_stsh};
use uo_docx::doc::text::{decode_text, fc_of_cp, piece_at, read_piece_table, total_cp};
use uo_docx::doc::xmlw::{esc_attr_units, esc_text_units};
use uo_docx::doc::{DocError, DocLimitKind, DocLimits, convert_doc_to_docx, is_word_binary};

/// The reader assembly (F4) over a file: every operation, the string budget as a value.
fn reader_pipeline(bytes: &[u8], limits: &DocLimits) -> usize {
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let rd = match DocReader::new(&streams, limits) {
        Ok(rd) => rd,
        Err(DocError::Limit { .. } | DocError::NoClx | DocError::NoPcdt | DocError::EmptyPieceTable) => return 1,
        Err(e) => panic!("unexpected {e}"),
    };
    let mut n = rd.header_stories().len() + rd.notes(NoteKind::Footnote).len() + rd.notes(NoteKind::Endnote).len();
    for r in [rd.comments().map(|c| c.len()), rd.bookmarks().map(|b| b.len()), rd.revision_authors().map(|a| a.len())] {
        match r {
            Ok(k) => n += k,
            Err(DocError::Limit { kind: DocLimitKind::StringUnits, .. }) => {}
            Err(e) => panic!("unexpected {e}"),
        }
    }
    for hdr in [false, true] {
        for f in rd.fspas(hdr) {
            if let Some(shape) = rd.shape(i64::from(f.spid), hdr) {
                n += usize::from(pic_fx_of(Some(shape)).is_some()) + usize::from(shape_blip(&rd, shape).is_some());
            }
        }
        n += rd.textboxes(hdr).len();
    }
    for cp in [-1, 0, 3, 77, i64::from(u32::MAX), i64::MAX] {
        n += usize::from(rd.paragraph_at(cp, i64::from(rd.fib.ccp_text)).run.is_some());
    }
    for run in rd.chpx.iter().chain(rd.papx.iter()).take(200) {
        let fc = i32::from_le_bytes(run.fc_start.to_le_bytes());
        n += usize::from(read_inline_picture(&rd, fc).is_some()) + ff_data_xml(rd.data, fc).map_or(0, |x| x.len());
        n += usize::from(read_inline_picture(&rd, -fc).is_some());
    }
    n + rd.data_records(0, u64::MAX).len() + rd.data_records(u64::MAX, 3).len()
}

/// The reader pipeline; a text limit must come back as a value.
fn pipeline(bytes: &[u8], limits: &DocLimits) -> usize {
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let Ok(pt) = read_piece_table(&streams.fib, &streams.table) else { return 1 };
    let total = total_cp(&streams.fib, &pt);
    let text = match decode_text(&pt, &streams.word, total, limits) {
        Ok(t) => t.len(),
        Err(DocError::Limit { kind: DocLimitKind::TextUnits, max }) => {
            assert_eq!(max, limits.max_text_units);
            0
        }
        Err(e) => panic!("unexpected {e}"),
    };
    for p in &pt.pieces {
        black_box(piece_at(&pt.pieces, i64::from(p.cp_start)));
        black_box(fc_of_cp(&pt.pieces, i64::from(p.cp_end)));
    }
    let chpx = read_chpx_runs(&streams.fib, &streams.table, &streams.word);
    let papx = read_papx_runs(&streams.fib, &streams.table, &streams.word, streams.data.as_deref());
    for r in chpx.iter().chain(papx.iter()) {
        black_box(run_at(&chpx, f64::from(r.fc_start) + 0.5));
        black_box(run_index_from(&papx, f64::from(r.fc_end)));
        black_box(prls(r.grpprl).count());
    }
    let styles = match read_stsh(&streams.fib, &streams.table, limits) {
        Ok(sheet) => {
            let n = sheet.styles.len().min(300);
            let mut r = StyleResolver::new(sheet);
            let mut total = 0;
            for i in (0..n).chain([0x0fff, 0xffff]) {
                let i = u16::try_from(i).unwrap();
                total += r.resolve(i).own_chp.len() + r.resolve_char(i).1.len();
                let root = r.root_chp.clone();
                total += usize::from(r.apply_char_style(i, &root).istd.is_some());
            }
            total
        }
        Err(DocError::Limit { kind: DocLimitKind::StringUnits, max }) => {
            assert_eq!(max, limits.string_budget(streams.table.len() as u64));
            0
        }
        Err(e) => panic!("unexpected {e}"),
    };
    let lists = read_lists(&streams.fib, &streams.table);
    text + read_fonts(&streams.fib, &streams.table).len() + styles + lists.lsts.len() + lists.lfos.len()
}

#[test]
fn mutated_fixtures_never_panic() {
    let mut rng = Rng(0x00d0_c0de);
    let limits = DocLimits { max_text_units: 1 << 22, ..DocLimits::DEFAULT };
    let mut total = 0usize;
    for (_, orig) in doc_support::fixtures() {
        for k in 0..400 {
            let mut b = orig.clone();
            match k % 8 {
                // a few random bytes anywhere
                0..=2 => {
                    for _ in 0..=rng.below(16) {
                        let p = rng.below(b.len());
                        b[p] = rng.next().to_le_bytes()[0];
                    }
                }
                // bytes in the first sectors (header, FIB, directory)
                3 => {
                    for _ in 0..=rng.below(8) {
                        let p = rng.below(b.len().min(2048));
                        b[p] = [0, 0xff, 0x7f, 0x80][rng.below(4)];
                    }
                }
                // a stretch filled with zeros or 0xFF
                4 => {
                    let p = rng.below(b.len());
                    let n = rng.below(600);
                    let v = if rng.below(2) == 0 { 0 } else { 0xff };
                    for x in b.iter_mut().skip(p).take(n) {
                        *x = v;
                    }
                }
                // a cut
                5 => b.truncate(rng.below(b.len() + 1)),
                // a word of the stream overwritten with a special value
                6 => {
                    let p = rng.below(b.len().saturating_sub(4));
                    let v = [0u32, 1, 0xffff_ffff, 0x4000_0001, 0x7fff_ffff, 0x8000_0000][rng.below(6)];
                    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
                }
                // garbage of the same length
                _ => {
                    for x in b.iter_mut().skip(rng.below(512)).step_by(3) {
                        *x = rng.next().to_le_bytes()[1];
                    }
                }
            }
            total += pipeline(&b, &limits) + reader_pipeline(&b, &limits);
            black_box(is_word_binary(&b));
            black_box(convert_doc_to_docx(&b, &DocLimits::DEFAULT).is_err());
        }
    }
    black_box(total);
}

#[test]
fn the_capture_mutations_with_tight_limits() {
    let cap = doc_support::capture();
    let tight = DocLimits { max_text_units: 64, max_string_units: 64, max_work: 64, string_units_per_byte: 0, max_nesting: 2 };
    let mut n = 0;
    for rec in cap.records() {
        if rec.key.starts_with("mut/") {
            let bytes = doc_support::input_bytes(rec);
            black_box(pipeline(&bytes, &tight));
            black_box(reader_pipeline(&bytes, &tight));
            black_box(convert_doc_to_docx(&bytes, &tight).is_ok());
            black_box(convert_doc_to_docx(&bytes, &DocLimits::DEFAULT).is_ok());
            n += 1;
        }
    }
    assert!(n >= 1600);
}

#[test]
fn byte_readers_over_random_bytes() {
    let mut rng = Rng(0x0bad_f00d);
    for _ in 0..4000 {
        let len = rng.below(300);
        let bytes: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        black_box(prls(&bytes).count());
        let r = ByteReader::new(&bytes);
        for off in [0u64, 1, 7, 299, 300, u64::from(u32::MAX), u64::MAX, u64::MAX - 1] {
            black_box((r.u8(off), r.i8(off), r.u16(off), r.i16(off), r.u32(off), r.i32(off)));
            black_box(r.sub(off, off.wrapping_add(8)).len());
            black_box(r.utf16(off, 65_535).len());
        }
        black_box(r.sub(rng.below(400) as u64, rng.below(400) as u64).len());
        black_box(r.at_f64(f64::from(rng.next()) / 7.0));
        for cb in [0, 4, 8, 22] {
            if let Some(p) = read_plc(&bytes, rng.below(64) as u32, rng.next() % 300, cb) {
                black_box(p.cps.len());
                black_box(p.r.u32(p.data_at(p.n)));
            }
        }
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from(c[0]) | (u16::from(*c.get(1).unwrap_or(&0xd8)) << 8)).collect();
        black_box(esc_text_units(&units).len());
        black_box(esc_attr_units(&units).len());
    }
    // a Prl walk over a long grpprl of nothing but variable operands
    black_box(prls(&[0x0d, 0xc6, 0xff].repeat(2000)).count());
    black_box(prls(&vec![0xff; 70_000]).count());
}

/// The appliers over random grpprls (sprms of every class with random operands, long and short, the
/// special sizes) and over random starting states.
#[test]
fn appliers_over_random_grpprls_never_panic() {
    use uo_docx::doc::props::{
        Chp, ChpCtx, KeySet, Pap, PapCtx, TabStop, Tap, apply_chp, apply_pap, apply_sep, apply_tap, default_chp, default_sep, finish_tap,
    };
    let mut rng = Rng(0x5eed_0f02);
    let ps = default_chp(1, 2, 3);
    for k in 0..6000 {
        let len = rng.below(120);
        let mut g: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        // plant a sprm of a class every few bytes: the opcode space of props.ts
        for i in (0..g.len().saturating_sub(1)).step_by(6) {
            let hi = [0x08, 0x2a, 0x4a, 0x24, 0x44, 0x84, 0xc6, 0xd6, 0x56, 0x76, 0x94, 0xb0, 0x30, 0x50, 0x70, 0x64, 0xca, 0x66, 0xf6]
                [rng.below(19)];
            g[i + 1] = hi;
        }
        let mut chp = if k % 2 == 0 { Chp::default() } else { ps.clone() };
        let mut t = KeySet::new();
        let mut style = |istd: u16| Chp { istd: Some(istd), hps: Some(i32::from(istd)), ..Chp::default() };
        let mut ctx =
            ChpCtx { base: Chp::default(), apply_char_style: if k % 3 == 0 { Some(&mut style) } else { None }, para_style: Some(&ps) };
        for p in prls(&g) {
            apply_chp(&mut chp, &p, &mut ctx, &mut t);
        }
        let mut pap = Pap { istd: u16::try_from(k % 20).unwrap(), tabs: Some(vec![TabStop { pos: 720, jc: 0, tlc: 0 }]), ..Pap::default() };
        let mut pt = KeySet::new();
        for p in prls(&g) {
            apply_pap(&mut pap, &p, &mut pt, &mut PapCtx::default());
        }
        let mut tap = Tap::new();
        for p in prls(&g) {
            apply_tap(&mut tap, &p);
        }
        finish_tap(&mut tap);
        let mut sep = default_sep();
        for p in prls(&g) {
            apply_sep(&mut sep, &p);
        }
        black_box((chp, pap, tap, sep));
    }
}

/// The property emitters over random touched sets and random property values, the part emitters over
/// random DOPs and over the style sheets and list tables the pipeline reads from mutated files.
#[test]
fn emitters_never_panic() {
    use std::borrow::Cow;
    use uo_docx::doc::fonts::DocFont;
    use uo_docx::doc::numbering::build_numbering;
    use uo_docx::doc::ooxml_props::{PPrOpts, PropCtx, p_pr_inner, r_pr_inner};
    use uo_docx::doc::props::{Brc, Chp, ChpKey, KeySet, Pap, PapKey, Shd, TabStop};
    use uo_docx::doc::settings::build_settings;
    use uo_docx::doc::styles_xml::{build_styles, style_id_map};
    use uo_docx::text::Utf16;
    let mut rng = Rng(0xe317_77e7);
    let fonts: Vec<DocFont> = (0..3u8)
        .map(|i| DocFont {
            name: Utf16::from_units(vec![0xd800, 0x41, u16::from(i)]),
            alt_name: None,
            prq: 0,
            true_type: false,
            ff: 0,
            weight: 0,
            charset: 128,
            panose: Vec::new(),
            sig: [0; 6],
        })
        .collect();
    let sid = |i: u16| (!i.is_multiple_of(3)).then(|| Cow::Owned(format!("S{i}")));
    let ctx = PropCtx { fonts: &fonts, style_id: &sid };
    let brc = |rng: &mut Rng| {
        (rng.below(2) == 0).then(|| Brc {
            color: [-1, 0, 0xff0000, 0xffffff][rng.below(4)],
            width: rng.next().to_le_bytes()[0],
            brc_type: rng.next().to_le_bytes()[1],
            space: rng.next().to_le_bytes()[2],
            shadow: rng.below(2) == 0,
            frame: rng.below(2) == 0,
            from_new: false,
        })
    };
    for _ in 0..3000 {
        // character properties: random values and a random touched set
        let mut chp = Chp::default();
        let mut t = KeySet::new();
        for &k in ChpKey::ALL {
            if rng.below(2) == 0 {
                t.add(k);
            }
        }
        let n = |rng: &mut Rng| (rng.below(3) == 0).then(|| rng.next());
        chp.istd = Some(rng.next() as u16);
        chp.ftc_ascii = n(&mut rng).map(|v| v as u16);
        chp.ftc_fe = Some(rng.next() as u16);
        chp.ico = Some(rng.next() as u8);
        chp.cv = n(&mut rng).map(|v| (v & 0xff_ffff) as i32);
        chp.cv_ul = Some(-1);
        chp.kul = Some(rng.next() as u8);
        chp.highlight = Some(rng.next() as u8);
        chp.kcd = Some(rng.next() as u8);
        chp.lid = Some(rng.next() as u16);
        chp.lid_fe = Some(rng.next() as u16);
        chp.hps = Some(rng.next() as i32);
        chp.brc = brc(&mut rng);
        chp.shd = (rng.below(2) == 0).then(|| Shd { fore: -1, back: rng.next() as i32 & 0xff_ffff, pat: rng.next() as u16 });
        chp.bold = Some(rng.below(2) == 0);
        black_box(r_pr_inner(&chp, &t, &ctx, rng.below(2) == 0));
        // paragraph properties
        let mut pap = Pap { istd: rng.next() as u16, ..Pap::default() };
        let mut pt = KeySet::new();
        for &k in PapKey::ALL {
            if rng.below(2) == 0 {
                pt.add(k);
            }
        }
        pap.jc = Some(rng.next() as u8);
        pap.ilfo = Some(rng.next() as i16);
        pap.ilvl = Some(rng.next() as u8);
        pap.pc = Some(rng.next() as u8);
        pap.wr = Some(rng.next() as u8);
        pap.dcs = Some(rng.next() as u16);
        pap.w_height_abs = Some(rng.next() as u16);
        pap.dxa_abs = Some([-4, -8, -12, -16, -20, 0, 5, i16::MIN][rng.below(8)]);
        pap.dya_abs = Some([-4, -8, -12, -16, -20, 0, 5, i16::MIN][rng.below(8)]);
        pap.dxa_left1 = Some(rng.next() as i16);
        pap.dxc_left1 = Some(rng.next() as i16);
        pap.out_lvl = Some(rng.next() as u8);
        pap.w_align_font = Some(rng.next() as u16);
        pap.brc_top = brc(&mut rng);
        pap.tabs =
            Some((0..rng.below(6)).map(|_| TabStop { pos: rng.next() as i16, jc: rng.next() as u8, tlc: rng.next() as u8 }).collect());
        pap.tabs_cleared = Some((0..rng.below(4)).map(|_| rng.next() as i16).collect());
        let base = [TabStop { pos: 720, jc: 0, tlc: 0 }];
        let numid = |n: u32| i64::from(n) * 3;
        let opts =
            PPrOpts { base_tabs: Some(&base), num_id: Some(&numid), r_pr: Some("<w:b/>"), sect_pr: None, with_style: rng.below(2) == 0 };
        black_box(p_pr_inner(&pap, &pt, &ctx, &opts));
    }
    // random DOPs
    for _ in 0..2000 {
        let len = [0, 4, 60, 88, 92, 410, 520, 600, 700][rng.below(9)];
        let dop: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        let n_fib = [0xc1, 0xd9, 0x112][rng.below(3)];
        black_box(build_settings(&dop, n_fib, &fonts, rng.next() as u16, rng.below(2) == 0, rng.below(2) == 0));
    }
    // the styles and lists of mutated files
    for (_, orig) in doc_support::fixtures() {
        for _ in 0..60 {
            let mut b = orig.clone();
            for _ in 0..=rng.below(12) {
                let p = rng.below(b.len());
                b[p] = rng.next().to_le_bytes()[0];
            }
            let Ok(streams) = DocStreams::open(&b) else { continue };
            let Ok(sheet) = read_stsh(&streams.fib, &streams.table, &DocLimits::DEFAULT) else { continue };
            let lists = read_lists(&streams.fib, &streams.table);
            let fonts = read_fonts(&streams.fib, &streams.table);
            let ids = style_id_map(&sheet.styles);
            let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
            let ctx = PropCtx { fonts: &fonts, style_id: &sid };
            let mut r = StyleResolver::new(sheet);
            black_box(build_styles(&mut r, &fonts, &ctx, &ids, &|n| i64::from(n)));
            black_box(build_numbering(&lists, &ctx));
        }
    }
}
