//! C2 `.xls` converter: hand-made files for the corners the generated corpus does not reach (BIFF2, no BOF, encrypted, not a workbook, no
//! sheets) and the hostile inputs the limits answer (a file hyperlink with 65,535 `..\`, a shared formula decompiled by many cells,
//! nested OfficeArt groups, 100,000 empty shapes), then a seeded mutation loop over a workbook with cells, formulas, a drawing and a
//! hyperlink: no panic, a bounded time, nothing left allocated (the counting allocator of `uo-alloc`).
#![cfg(feature = "xls")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use uo_alloc::assert_no_leak;
use uo_xlsx::xls::{self, XlsError, XlsLimits};

// ───────────────────────────── builders ─────────────────────────────

fn rec(ty: u16, data: &[u8]) -> Vec<u8> {
    let mut v = ty.to_le_bytes().to_vec();
    v.extend_from_slice(&u16::try_from(data.len()).unwrap().to_le_bytes());
    v.extend_from_slice(data);
    v
}

fn bof(dt: u16) -> Vec<u8> {
    let mut d = vec![0x00, 0x06];
    d.extend_from_slice(&dt.to_le_bytes());
    d.extend_from_slice(&[0xd3, 0x20, 0xcc, 0x07, 0, 0, 0, 0, 6, 0, 0, 0]);
    rec(0x0809, &d)
}

fn short_str(s: &str) -> Vec<u8> {
    let mut v = vec![u8::try_from(s.len()).unwrap(), 0];
    v.extend_from_slice(s.as_bytes());
    v
}

fn xf() -> Vec<u8> {
    rec(0x00e0, &[0, 0, 0, 0, 0xf5, 0xff, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0x20])
}

/// A BIFF8 workbook with one sheet; `sheet` are the records between the sheet's BOF and EOF.
fn workbook(globals_extra: &[Vec<u8>], sheet: &[Vec<u8>]) -> Vec<u8> {
    let mut g: Vec<u8> = bof(0x05);
    g.extend(rec(0x0031, &{
        let mut f = vec![200, 0, 0, 0, 0x7f, 0x7f, 0x90, 0x01, 0, 0, 0, 0, 0, 0];
        f.extend(short_str("Arial"));
        f
    }));
    g.extend(xf());
    for e in globals_extra {
        g.extend_from_slice(e);
    }
    // BOUNDSHEET: lbPlyPos, state, type, name
    let globals_len_without_bs = g.len() + 4 + 4 + 2 + short_str("S1").len() + 4; // + EOF
    let pos = u32::try_from(globals_len_without_bs).unwrap();
    let mut bs = pos.to_le_bytes().to_vec();
    bs.extend_from_slice(&[0, 0]);
    bs.extend(short_str("S1"));
    g.extend(rec(0x0085, &bs));
    g.extend(rec(0x000a, &[]));
    let mut s = bof(0x10);
    for r in sheet {
        s.extend_from_slice(r);
    }
    s.extend(rec(0x000a, &[]));
    g.extend(s);
    g
}

fn number(row: u16, col: u16, v: f64) -> Vec<u8> {
    let mut d = row.to_le_bytes().to_vec();
    d.extend_from_slice(&col.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&v.to_le_bytes());
    rec(0x0203, &d)
}

/// FORMULA with the cached number `v` and the rgce `ptg`.
fn formula(row: u16, col: u16, v: f64, ptg: &[u8]) -> Vec<u8> {
    let mut d = row.to_le_bytes().to_vec();
    d.extend_from_slice(&col.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&v.to_le_bytes());
    d.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    d.extend_from_slice(&u16::try_from(ptg.len()).unwrap().to_le_bytes());
    d.extend_from_slice(ptg);
    rec(0x0006, &d)
}

/// A Compound File v3 with ONE stream (padded to 4096: no mini stream).
fn cfb(name: &str, data: &[u8]) -> Vec<u8> {
    let ss = 512usize;
    let mut data = data.to_vec();
    if data.len() < 4096 {
        data.resize(4096, 0);
    }
    let n_data = data.len().div_ceil(ss);
    let mut n_fat = 1usize;
    while n_data + 1 + n_fat > n_fat * 128 {
        n_fat += 1;
    }
    let total = n_data + n_fat + 1;
    let mut b = vec![0u8; ss * (total + 1)];
    b[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    let put16 = |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put16(&mut b, 0x18, 0x3e);
    put16(&mut b, 0x1a, 3);
    put16(&mut b, 0x1c, 0xfffe);
    put16(&mut b, 0x1e, 9);
    put16(&mut b, 0x20, 6);
    put32(&mut b, 0x2c, u32::try_from(n_fat).unwrap());
    put32(&mut b, 0x30, u32::try_from(n_data + n_fat).unwrap());
    put32(&mut b, 0x38, 4096);
    put32(&mut b, 0x3c, 0xffff_fffe);
    put32(&mut b, 0x44, 0xffff_fffe);
    for i in 0..109 {
        put32(&mut b, 0x4c + i * 4, if i < n_fat { u32::try_from(n_data + i).unwrap() } else { 0xffff_ffff });
    }
    b[ss..ss + data.len()].copy_from_slice(&data);
    let mut fat = vec![0xffff_ffffu32; n_fat * 128];
    for (i, f) in fat.iter_mut().enumerate().take(n_data) {
        *f = if i + 1 < n_data { u32::try_from(i + 1).unwrap() } else { 0xffff_fffe };
    }
    for i in 0..n_fat {
        fat[n_data + i] = 0xffff_fffd;
    }
    fat[n_data + n_fat] = 0xffff_fffe;
    for (i, v) in fat.iter().enumerate() {
        put32(&mut b, ss * (1 + n_data) + i * 4, *v);
    }
    let d = ss * (1 + n_data + n_fat);
    let mut ent = |o: usize, nm: &str, ty: u8, child: u32, start: u32, size: u32| {
        let units: Vec<u8> = nm.encode_utf16().flat_map(u16::to_le_bytes).collect();
        b[d + o * 128..d + o * 128 + units.len()].copy_from_slice(&units);
        let nl = if nm.is_empty() { 0 } else { u16::try_from(units.len() + 2).unwrap() };
        b[d + o * 128 + 0x40..d + o * 128 + 0x42].copy_from_slice(&nl.to_le_bytes());
        b[d + o * 128 + 0x42] = ty;
        b[d + o * 128 + 0x43] = 1;
        for k in [0x44, 0x48] {
            b[d + o * 128 + k..d + o * 128 + k + 4].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        }
        b[d + o * 128 + 0x4c..d + o * 128 + 0x50].copy_from_slice(&child.to_le_bytes());
        b[d + o * 128 + 0x74..d + o * 128 + 0x78].copy_from_slice(&start.to_le_bytes());
        b[d + o * 128 + 0x78..d + o * 128 + 0x7c].copy_from_slice(&size.to_le_bytes());
    };
    ent(0, "Root Entry", 5, 1, 0xffff_fffe, 0);
    ent(1, name, 2, 0xffff_ffff, 0, u32::try_from(data.len()).unwrap());
    for o in [2usize, 3] {
        for k in [0x44, 0x48, 0x4c] {
            b[d + o * 128 + k..d + o * 128 + k + 4].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        }
    }
    b
}

fn part<'a>(p: &'a xls::XlsParts, name: &str) -> &'a [u8] {
    p.get(name).unwrap_or_else(|| panic!("no part {name}"))
}

// ───────────────────────────── errors ─────────────────────────────

#[test]
fn junk_is_a_format_error() {
    assert!(matches!(xls::convert(&[]), Err(XlsError::Format(_))));
    assert!(matches!(xls::convert(b"PK\x03\x04 not a compound file"), Err(XlsError::Format(_))));
    assert!(matches!(xls::convert(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 1, 2]), Err(XlsError::Format(_))));
    let mut hdr = vec![0u8; 600];
    hdr[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    assert!(matches!(xls::convert(&hdr), Err(XlsError::Format(_))));
    assert!(xls::is_xls_bytes(&hdr) && !xls::is_xls_bytes(b"PK"));
}

#[test]
fn not_a_workbook_and_encrypted() {
    for (stream, want) in [("WordDocument", "Word"), ("PowerPoint Document", "PowerPoint"), ("Other", "Workbook")] {
        match xls::convert(&cfb(stream, &[1, 2, 3])) {
            Err(XlsError::Format(m)) => assert!(m.contains(want), "{m}"),
            r => panic!("{r:?}"),
        }
    }
    assert_eq!(xls::convert(&cfb("EncryptedPackage", &[1])).unwrap_err(), XlsError::Encrypted);
    assert_eq!(xls::convert(&cfb("EncryptionInfo", &[1])).unwrap_err(), XlsError::Encrypted);
    // FILEPASS in the globals
    let wb = workbook(&[rec(0x002f, &[0, 0, 1, 0, 1, 0])], &[]);
    assert_eq!(xls::convert(&cfb("Workbook", &wb)).unwrap_err(), XlsError::Encrypted);
}

#[test]
fn old_formats_and_broken_streams() {
    // BIFF2 BOF (0x0009), BIFF3 (0x0209), BIFF4 (0x0409)
    for t in [0x0009u16, 0x0209, 0x0409] {
        let mut d = rec(t, &[0, 0, 5, 0]);
        d.extend(rec(0x000a, &[]));
        assert!(matches!(xls::convert(&cfb("Workbook", &d)), Err(XlsError::Format(m)) if m.contains("BIFF")));
    }
    assert!(matches!(xls::convert(&cfb("Workbook", &rec(0x0031, &[0; 20]))), Err(XlsError::Format(m)) if m.contains("BOF")));
    assert!(matches!(xls::convert(&cfb("Workbook", &[1, 2, 3])), Err(XlsError::Format(_))));
    // globals without a sheet
    let mut g = bof(0x05);
    g.extend(rec(0x000a, &[]));
    assert!(matches!(xls::convert(&cfb("Workbook", &g)), Err(XlsError::Format(m)) if m.contains("sheets")));
}

#[test]
fn a_minimal_workbook_converts() {
    let wb = workbook(&[], &[number(0, 0, 1.5), number(0, 1, -3.0), formula(1, 0, 2.0, &[0x1e, 2, 0, 0x1e, 3, 0, 0x03])]);
    let p = xls::convert(&cfb("Workbook", &wb)).unwrap();
    let names: Vec<&str> = p.items.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names[..2], ["[Content_Types].xml", "_rels/.rels"]);
    let sheet = String::from_utf8(part(&p, "xl/worksheets/sheet1.xml").to_vec()).unwrap();
    assert!(sheet.contains("<c r=\"A1\"><v>1.5</v></c>") && sheet.contains("<c r=\"B1\"><v>-3</v></c>"), "{sheet}");
    assert!(sheet.contains("<c r=\"A2\"><f>2+3</f><v>2</v></c>"), "{sheet}");
    assert!(sheet.contains("<dimension ref=\"A1:B2\"/>"), "{sheet}");
    // and the normal reader opens the converted package
    let book = uo_xlsx::book::Book::open_xls(&cfb("Workbook", &wb), uo_xlsx::book::ReadLimits::default()).unwrap();
    assert_eq!(book.workbook.sheets.len(), 1);
}

// ───────────────────────────── hostile ─────────────────────────────

/// HLINK to a file with `c_anti` parent steps.
fn hlink_file(c_anti: u16) -> Vec<u8> {
    let mut d = vec![0, 0, 0, 0, 0, 0, 0, 0]; // r1 r2 c1 c2
    d.extend_from_slice(&[0xd0, 0xc9, 0xea, 0x79, 0xf9, 0xba, 0xce, 0x11, 0x8c, 0x82, 0, 0xaa, 0, 0x4b, 0xa9, 0x0b]); // hlinkClsid
    d.extend_from_slice(&2u32.to_le_bytes()); // streamVersion
    d.extend_from_slice(&0x03u32.to_le_bytes()); // flags: hasMoniker, absolute
    d.extend_from_slice(&[0x03, 0x03, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]); // file moniker CLSID
    d.extend_from_slice(&c_anti.to_le_bytes());
    d.extend_from_slice(&6u32.to_le_bytes());
    d.extend_from_slice(b"a.xls\0");
    d.extend_from_slice(&[0xff, 0xff, 0xad, 0xde]);
    d.extend_from_slice(&[0u8; 20]);
    d.extend_from_slice(&0u32.to_le_bytes());
    rec(0x01b8, &d)
}

#[test]
fn file_hyperlink_steps_are_capped() {
    let wb = workbook(&[], &[number(0, 0, 1.0), hlink_file(65_535)]);
    let p = xls::convert(&cfb("Workbook", &wb)).unwrap();
    let rels = String::from_utf8(part(&p, "xl/worksheets/_rels/sheet1.xml.rels").to_vec()).unwrap();
    assert!(rels.contains("Target=\"../../"), "{}", &rels[..rels.len().min(300)]);
    assert!(rels.len() < 8000, "{}", rels.len());
}

#[test]
fn shared_formula_amplification_is_bounded() {
    // one SHRFMLA of ~4,000 bytes (a SUM of many area tokens), referenced by 30,000 ptgExp cells
    let mut rgce: Vec<u8> = Vec::new();
    for i in 0u16..440 {
        rgce.extend_from_slice(&[0x25]);
        rgce.extend_from_slice(&i.to_le_bytes());
        rgce.extend_from_slice(&(i + 1).to_le_bytes());
        rgce.extend_from_slice(&[0, 0, 3, 0]);
        if i > 0 {
            rgce.push(0x03);
        }
    }
    let mut shr = vec![0u8, 0, 0xff, 0xff, 0, 0xff, 0, 0]; // RefU
    shr.extend_from_slice(&u16::try_from(rgce.len()).unwrap().to_le_bytes());
    shr.extend_from_slice(&rgce);
    let mut sheet = vec![formula(0, 0, 1.0, &[0x01, 0, 0, 0, 0]), rec(0x04bc, &shr)];
    for r in 1u16..30_000 {
        sheet.push(formula(r, 0, 1.0, &[0x01, 0, 0, 0, 0]));
    }
    let wb = cfb("Workbook", &workbook(&[], &sheet));
    // the default limits let it through (a real file does that on purpose)
    let t = Instant::now();
    assert!(xls::convert(&wb).is_ok());
    assert!(t.elapsed().as_secs() < 30);
    // a small bound answers with a typed error instead of the size
    let small = XlsLimits { max_sheet_bytes: 1 << 20, ..XlsLimits::default() };
    assert_eq!(xls::convert_with(&wb, &small).unwrap_err(), XlsError::Limit("sheet size"));
}

fn oa(ver_inst: u16, ty: u16, body: &[u8]) -> Vec<u8> {
    let mut v = ver_inst.to_le_bytes().to_vec();
    v.extend_from_slice(&ty.to_le_bytes());
    v.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    v.extend_from_slice(body);
    v
}

#[test]
fn drawings_nested_and_many_shapes_are_bounded() {
    // 200 nested SpgrContainers
    let mut inner = oa(0x000f, 0xf004, &oa(0x0002, 0xf00a, &[0u8; 8]));
    for _ in 0..200 {
        let mut body = oa(0x000f, 0xf004, &oa(0x0002, 0xf00a, &[0u8; 8]));
        body.extend_from_slice(&inner);
        inner = oa(0x000f, 0xf003, &body);
    }
    let dg = oa(0x000f, 0xf002, &oa(0x000f, 0xf003, &inner));
    let mut wb_rec = Vec::new();
    for chunk in dg.chunks(8000) {
        wb_rec.push(rec(0x00ec, chunk));
    }
    let wb = workbook(&[], &wb_rec);
    let t = Instant::now();
    let _ = xls::convert(&cfb("Workbook", &wb));
    assert!(t.elapsed().as_secs() < 10);
    // 100,000 empty shape containers in one SpgrContainer
    let one = oa(0x000f, 0xf004, &[]);
    let mut many = Vec::new();
    for _ in 0..100_000 {
        many.extend_from_slice(&one);
    }
    let dg = oa(0x000f, 0xf002, &oa(0x000f, 0xf003, &many));
    let recs: Vec<Vec<u8>> = dg.chunks(8000).map(|c| rec(0x00ec, c)).collect();
    let t = Instant::now();
    let _ = xls::convert(&cfb("Workbook", &workbook(&[], &recs)));
    assert!(t.elapsed().as_secs() < 10);
}

#[test]
fn a_chart_over_a_huge_area_is_bounded() {
    // an AI record that refers to A1:IV65536 of sheet 0, 3,000 times in one chart substream
    let mut chart: Vec<Vec<u8>> = vec![bof(0x20), rec(0x1002, &[0; 16]), rec(0x1033, &[])];
    for _ in 0..3_000 {
        chart.push(rec(0x1003, &[0; 12]));
        chart.push(rec(0x1033, &[]));
        let mut ai = vec![1u8, 2, 0, 0, 0, 0];
        let rgce = [0x3b, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 0xff, 0x00];
        ai.extend_from_slice(&u16::try_from(rgce.len()).unwrap().to_le_bytes());
        ai.extend_from_slice(&rgce);
        chart.push(rec(0x1051, &ai));
        chart.push(rec(0x1034, &[]));
    }
    chart.push(rec(0x1034, &[]));
    chart.push(rec(0x000a, &[]));
    let sheet: Vec<Vec<u8>> = Vec::new();
    let mut wb = workbook(&[rec(0x0017, &[1, 0, 0, 0, 0, 0, 0, 0])], &sheet);
    for c in &chart {
        wb.extend_from_slice(c);
    }
    let t = Instant::now();
    let _ = xls::convert(&cfb("Workbook", &wb));
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
}

// ───────────────────────────── mutation loop ─────────────────────────────

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

/// The base workbook of the loop: cells, a shared formula, a note-less sheet with a file hyperlink and a drawing record.
fn base_records() -> Vec<Vec<u8>> {
    let dg = oa(
        0x000f,
        0xf002,
        &oa(0x000f, 0xf003, &{
            let mut sp = oa(0x000f, 0xf004, &{
                let mut b = oa(0x0012, 0xf00a, &[1, 0, 0, 0, 0x00, 0x0a, 0, 0]);
                b.extend(oa(0x0003 | (3 << 4), 0xf00b, &[0x81, 0x01, 0xff, 0, 0x10, 0, 0x81, 0x01, 0, 0, 0xff, 0]));
                b.extend(oa(0x0000, 0xf010, &[0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0]));
                b.extend(oa(0x0000, 0xf011, &[]));
                b
            });
            sp.extend(sp.clone());
            sp
        }),
    );
    let mut r =
        vec![number(0, 0, 1.0), number(0, 1, 2.0), formula(1, 0, 3.0, &[0x24, 0, 0, 0, 0xc0, 0x24, 1, 0, 0, 0xc0, 0x03]), hlink_file(2)];
    r.push(rec(0x00ec, &dg));
    r.push(rec(0x005d, &[0x15, 0, 0x12, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    r
}

fn mutate(rng: &mut Rng, stream: &[u8]) -> Vec<u8> {
    let mut recs: Vec<Vec<u8>> = Vec::new();
    let mut o = 0;
    while o + 4 <= stream.len() {
        let len = usize::from(u16::from_le_bytes([stream[o + 2], stream[o + 3]]));
        recs.push(stream[o..(o + 4 + len).min(stream.len())].to_vec());
        o += 4 + len;
    }
    for _ in 0..1 + rng.below(3) {
        let i = rng.below(recs.len());
        match rng.below(8) {
            0 | 1 => {
                let n = recs[i].len();
                if n > 4 {
                    let at = 4 + rng.below(n - 4);
                    recs[i][at] ^= 1 << rng.below(8);
                }
            }
            2 => {
                recs.remove(i);
            }
            3 => {
                let c = recs[i].clone();
                recs.insert(i, c);
            }
            4 if i + 1 < recs.len() => recs.swap(i, i + 1),
            5 => {
                let n = recs[i].len();
                if n > 5 {
                    let keep = rng.below(n - 4);
                    recs[i].truncate(4 + keep);
                    recs[i][2..4].copy_from_slice(&u16::try_from(keep).unwrap().to_le_bytes());
                }
            }
            6 => {
                let t = [0x0006u16, 0x04bc, 0x0221, 0x01b8, 0x00ec, 0x005d, 0x01b6, 0x00fd, 0x00bd, 0x0085, 0x0031, 0x00e0, 0x0017, 0x0018]
                    [rng.below(14)];
                recs[i][0..2].copy_from_slice(&t.to_le_bytes());
            }
            _ => {
                let n = rng.below(40);
                let mut y = vec![0u8; 4 + n];
                y[0..2].copy_from_slice(&[0x06, 0x00, 0xbc, 0x04, 0x21, 0x02, 0xb8, 0x01, 0xec, 0x00][rng.below(5) * 2..][..2]);
                y[2..4].copy_from_slice(&u16::try_from(n).unwrap().to_le_bytes());
                for b in &mut y[4..] {
                    *b = rng.next().to_le_bytes()[0];
                }
                recs.insert(i, y);
            }
        }
        if recs.is_empty() {
            break;
        }
    }
    recs.concat()
}

#[test]
fn mutated_workbooks_never_panic_and_leave_nothing() {
    let base = workbook(&[], &base_records());
    // the statics (the default font, the number formats…) are built by the first conversion: not a leak
    let _ = xls::convert(&cfb("Workbook", &base));
    let mut rng = Rng(0x1234_5678);
    let mut ok = 0usize;
    let started = Instant::now();
    for k in 0..3000 {
        let m = mutate(&mut rng, &base);
        let file = cfb("Workbook", &m);
        let t = Instant::now();
        let mut r = false;
        if k % 15 == 0 {
            // the leak check on a part of the loop (it counts every allocation)
            assert_no_leak(|| r = xls::convert(&file).is_ok());
        } else {
            r = xls::convert(&file).is_ok();
        }
        assert!(t.elapsed().as_millis() < 3000, "mutant {k} took {:?}", t.elapsed());
        ok += usize::from(r);
    }
    assert!(ok > 500, "{ok} of 3000 converted");
    eprintln!("{ok} of 3000 mutants converted in {:?}", started.elapsed());
}

#[test]
fn bit_flipped_and_truncated_containers_never_panic() {
    let base = cfb("Workbook", &workbook(&[], &base_records()));
    let mut rng = Rng(0xdead_beef);
    for _ in 0..1500 {
        let mut b = base.clone();
        for _ in 0..1 + rng.below(20) {
            let at = rng.below(b.len());
            b[at] ^= 1 << rng.below(8);
        }
        if rng.below(4) == 0 {
            b.truncate(rng.below(b.len()));
        }
        let _ = xls::convert(&b);
    }
}

#[test]
fn convert_and_drop_leaves_nothing() {
    let wb = cfb("Workbook", &workbook(&[], &base_records()));
    let _ = xls::convert(&wb);
    assert_no_leak(|| xls::convert(&wb).unwrap());
    #[cfg(feature = "read")]
    assert_no_leak(|| uo_xlsx::book::Book::open_xls(&wb, uo_xlsx::book::ReadLimits::default()).map(|mut b| b.load_all().is_ok()).is_ok());
}

// ───────────────────────────── memory (M1, M2) on the committed files ─────────────────────────────

fn committed() -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> =
        std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/workbooks/xls"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
    v.sort();
    v
}

/// M1: converting, opening with the normal reader, loading every sheet and dropping leaves nothing behind (every committed file,
/// and bit-flipped copies of each: the error paths).
#[test]
fn m1_committed_files_leave_nothing() {
    let files = committed();
    assert!(files.len() >= 12);
    let mut rng = Rng(77);
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let _ = xls::convert(&bytes); // warm the statics (number formats of the charts…)
        assert_no_leak(|| {
            let mut b = uo_xlsx::book::Book::open_xls(&bytes, uo_xlsx::book::ReadLimits::default()).unwrap();
            b.load_all().unwrap();
        });
        for _ in 0..3 {
            let mut m = bytes.clone();
            for _ in 0..1 + rng.below(30) {
                let at = rng.below(m.len());
                m[at] ^= 1 << rng.below(8);
            }
            assert_no_leak(|| {
                let _ = uo_xlsx::book::Book::open_xls(&m, uo_xlsx::book::ReadLimits::default()).map(|mut b| b.load_all());
            });
        }
    }
}

/// M2: converting the same file 300 times is flat.
#[test]
fn m2_repeated_conversion_is_flat() {
    let bytes = std::fs::read(committed().iter().find(|p| p.ends_with("e-charts.xls")).unwrap()).unwrap();
    let _ = xls::convert(&bytes);
    let series = uo_alloc::soak(300, 25, |_| {
        let p = xls::convert(&bytes).unwrap();
        std::hint::black_box(&p);
    });
    uo_alloc::assert_flat(&series, 2, 0);
}
