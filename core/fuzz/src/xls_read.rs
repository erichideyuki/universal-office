//! Fuzz target of the `uo-xlsx` `.xls` converter (feature `xls`, record `core/docs/modules/uo-xlsx.md` "C2 - as built", `xls_read`): byte 0
//! picks how the rest is used. Mode 0: the bytes ARE the file (Compound File structure, chains, directory). Mode 1: the bytes are the
//! `Workbook` stream (BIFF records) inside a minimal valid Compound File, so mutations reach the record readers instead of dying in the
//! container. Mode 2: the bytes are the records of ONE sheet substream between a fixed valid globals and the sheet's EOF (cells,
//! formulas, drawings, charts, notes, conditional formats…). Whatever converts is opened with the normal reader and loaded whole: no panic,
//! memory bounded by the limits below.

use uo_opc::cfb::{CfbFile, is_cfb};
use uo_xlsx::book::{Book, ReadLimits};
use uo_xlsx::xls::{XlsLimits, convert_with};

fn rec(ty: u16, data: &[u8]) -> Vec<u8> {
    let mut v = ty.to_le_bytes().to_vec();
    v.extend_from_slice(&u16::try_from(data.len().min(0xffff)).unwrap_or(0).to_le_bytes());
    v.extend_from_slice(&data[..data.len().min(0xffff)]);
    v
}

fn bof(dt: u16) -> Vec<u8> {
    let mut d = vec![0x00, 0x06];
    d.extend_from_slice(&dt.to_le_bytes());
    d.extend_from_slice(&[0xd3, 0x20, 0xcc, 0x07, 0, 0, 0, 0, 6, 0, 0, 0]);
    rec(0x0809, &d)
}

/// A BIFF8 workbook of one sheet whose records are `sheet`.
fn workbook(sheet: &[u8]) -> Vec<u8> {
    let mut g = bof(0x05);
    let mut font = vec![200, 0, 0, 0, 0x7f, 0x7f, 0x90, 0x01, 0, 0, 0, 0, 0, 0, 5, 0];
    font.extend_from_slice(b"Arial");
    g.extend(rec(0x0031, &font));
    g.extend(rec(0x00e0, &[0, 0, 0, 0, 0xf5, 0xff, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0x20]));
    let pos = u32::try_from(g.len() + 4 + 4 + 2 + 4 + 4).unwrap_or(0);
    let mut bs = pos.to_le_bytes().to_vec();
    bs.extend_from_slice(&[0, 0, 2, 0, b'S', b'1']);
    g.extend(rec(0x0085, &bs));
    g.extend(rec(0x000a, &[]));
    g.extend(bof(0x10));
    g.extend_from_slice(sheet);
    g.extend(rec(0x000a, &[]));
    g
}

/// A Compound File v3 with ONE stream (padded to 4096: no mini stream).
fn cfb_file(name: &str, data: &[u8]) -> Vec<u8> {
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
    let p16 = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let p32 = |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    p16(&mut b, 0x18, 0x3e);
    p16(&mut b, 0x1a, 3);
    p16(&mut b, 0x1c, 0xfffe);
    p16(&mut b, 0x1e, 9);
    p16(&mut b, 0x20, 6);
    p32(&mut b, 0x2c, u32::try_from(n_fat).unwrap_or(1));
    p32(&mut b, 0x30, u32::try_from(n_data + n_fat).unwrap_or(0));
    p32(&mut b, 0x38, 4096);
    p32(&mut b, 0x3c, 0xffff_fffe);
    p32(&mut b, 0x44, 0xffff_fffe);
    for i in 0..109 {
        p32(&mut b, 0x4c + i * 4, if i < n_fat { u32::try_from(n_data + i).unwrap_or(0) } else { 0xffff_ffff });
    }
    b[ss..ss + data.len()].copy_from_slice(&data);
    let mut fat = vec![0xffff_ffffu32; n_fat * 128];
    for (i, f) in fat.iter_mut().enumerate().take(n_data) {
        *f = if i + 1 < n_data { u32::try_from(i + 1).unwrap_or(0) } else { 0xffff_fffe };
    }
    for i in 0..n_fat {
        fat[n_data + i] = 0xffff_fffd;
    }
    fat[n_data + n_fat] = 0xffff_fffe;
    for (i, v) in fat.iter().enumerate() {
        p32(&mut b, ss * (1 + n_data) + i * 4, *v);
    }
    let d = ss * (1 + n_data + n_fat);
    let mut ent = |o: usize, nm: &str, ty: u8, child: u32, start: u32, size: u32| {
        let units: Vec<u8> = nm.encode_utf16().flat_map(u16::to_le_bytes).collect();
        b[d + o * 128..d + o * 128 + units.len()].copy_from_slice(&units);
        let nl = if nm.is_empty() { 0 } else { u16::try_from(units.len() + 2).unwrap_or(0) };
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
    ent(1, name, 2, 0xffff_ffff, 0, u32::try_from(data.len()).unwrap_or(0));
    for o in [2usize, 3] {
        for k in [0x44, 0x48, 0x4c] {
            b[d + o * 128 + k..d + o * 128 + k + 4].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        }
    }
    b
}

/// Bounds of the target: small sheets, small total, few sheets, short cell text.
fn limits() -> (XlsLimits, ReadLimits) {
    (
        XlsLimits { max_sheet_bytes: 16 << 20, max_total_bytes: 48 << 20 },
        ReadLimits { max_sheets: 64, max_depth: 200, max_shared_strings: 1 << 16, max_cell_text: 1 << 16 },
    )
}

pub fn xls_read(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else { return };
    let file = match mode % 3 {
        0 => rest.to_vec(),
        1 => cfb_file("Workbook", rest),
        _ => cfb_file("Workbook", &workbook(rest)),
    };
    let (xl, rl) = limits();
    let Ok(parts) = convert_with(&file, &xl) else { return };
    // the converted package goes through the normal reader
    let pkg = uo_opc::Package::from_parts(
        parts.items,
        &uo_opc::Limits {
            max_xml_part_bytes: 32 << 20,
            max_binary_part_bytes: 32 << 20,
            max_total_bytes: 96 << 20,
            max_entries: 1000,
            max_xml_depth: 200,
            max_xml_elements: 800_000,
        },
        &uo_opc::package::OpenOptions { case_fold: uo_opc::package::CaseFold::Ascii, ..uo_opc::package::OpenOptions::default() },
    );
    let Ok(mut book) = Book::from_package(pkg, rl) else { return };
    let _ = book.load_all();
    assert!(book.store.check_invariants().is_ok());
}

/// Seeds: the committed `.xls` files (mode 0), their Workbook streams (mode 1) and the minimal workbook of mode 2.
pub fn xls_read_seeds(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = vec![vec![], vec![0], vec![2], {
        let mut v = vec![2u8];
        v.extend(rec(0x0203, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f]));
        v
    }];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let Ok(bytes) = std::fs::read(e.path()) else { continue };
        if bytes.len() > (1 << 20) {
            continue;
        }
        if is_cfb(&bytes) {
            if let Ok(f) = CfbFile::open(&bytes) {
                if let Ok(Some(stream)) = f.stream("Workbook") {
                    let mut v = vec![1u8];
                    v.extend_from_slice(&stream);
                    out.push(v);
                }
            }
        }
        let mut v = vec![0u8];
        v.extend_from_slice(&bytes);
        out.push(v);
    }
    out
}
