//! The globals substream of the workbook (`globals.ts`, [MS-XLS] 2.1.7.20.1): fonts, number formats, XFs, palette,
//! the sheet list (BOUNDSHEET), the shared string table, the date mode, the window (active tab) and the drawing group.

use std::collections::{HashMap, HashSet};

use super::XlsError;
use super::biff::{Codepage, Rd, Rec, RichString, read_chars, read_rich_ext_string, read_xl_string, rt};
use super::formula::{Area, formula_areas};
use crate::read::theme::INDEXED_COLORS_DEFAULT;

#[derive(Clone, Debug, Default)]
pub struct XlsFont {
    /// Height in twips (1/20 pt).
    pub height: u16,
    pub italic: bool,
    pub strike: bool,
    pub outline: bool,
    pub shadow: bool,
    /// Weight (400 normal, 700 bold).
    pub weight: u16,
    /// 0 none, 1 superscript, 2 subscript.
    pub script: u16,
    /// 0 none, 1 single, 2 double, 0x21 single accounting, 0x22 double accounting.
    pub underline: u8,
    pub family: u8,
    pub charset: u8,
    /// Colour index (icv); 0x7FFF = automatic.
    pub color: u16,
    pub name: String,
}

/// The full colour of an XF extension (XFEXT / FullColorExt).
#[derive(Clone, Debug)]
pub struct FullColor {
    /// 0 auto, 1 indexed, 2 RGB, 3 theme, 4 undefined.
    pub ty: u16,
    /// `RRGGBB` when `ty` = 2.
    pub rgb: Option<String>,
    /// The value when `ty` is not 2 (`index`).
    pub index: Option<u32>,
    /// Tint/shade −1..1 (nTintShade / 32767).
    pub tint: f64,
}

#[derive(Clone, Debug, Default)]
pub struct XlsXf {
    pub font: u16,
    pub fmt: u16,
    /// alc: 0 general, 1 left, 2 center, 3 right, 4 fill, 5 justify, 6 center across, 7 distributed.
    pub halign: u8,
    pub wrap: bool,
    /// alcV: 0 top, 1 center, 2 bottom, 3 justify, 4 distributed.
    pub valign: u8,
    /// Text rotation (0–90 counter-clockwise, 91–180 clockwise, 255 stacked).
    pub rot: u16,
    pub indent: u8,
    pub shrink: bool,
    pub b_left: u32,
    pub b_right: u32,
    pub b_top: u32,
    pub b_bottom: u32,
    pub b_diag: u32,
    pub c_left: u32,
    pub c_right: u32,
    pub c_top: u32,
    pub c_bottom: u32,
    pub c_diag: u32,
    /// grbitDiag: bit0 diagonal down, bit1 diagonal up.
    pub diag: u32,
    /// fls: fill pattern (0 none, 1 solid, 2…).
    pub pattern: u32,
    pub fore: u32,
    pub back: u32,
    /// Full colours of the XFEXT (Excel 2007+): key = extType (4 fg, 5 bg, 7–11 borders, 13 font).
    pub ext: Vec<(u16, FullColor)>,
}

impl XlsXf {
    pub fn ext(&self, ty: u16) -> Option<&FullColor> {
        self.ext.iter().find(|(t, _)| *t == ty).map(|(_, c)| c)
    }
}

#[derive(Clone, Debug)]
pub struct XlsSheetInfo {
    pub name: String,
    /// 0 visible, 1 hidden, 2 very hidden.
    pub state: u8,
    /// dt: 0 sheet/dialog, 1 XLM macro, 2 chart, 6 VBA module.
    pub ty: u8,
    /// Offset of the sheet's BOF in the stream (lbPlyPos).
    pub pos: u32,
}

/// An EXTERNSHEET entry: (workbook, first sheet, last sheet) (−1/−2: deleted/invalid sheet).
#[derive(Clone, Copy, Debug)]
pub struct Xti {
    pub book: u16,
    pub tab: i16,
    pub last: i16,
}

/// A defined name of BIFF8 (Lbl) with its formula still in RPN (decompiled at conversion).
#[derive(Clone, Debug)]
pub struct XlsNameDef {
    pub name: String,
    /// Scope sheet (BOUNDSHEET index, 0-based) or −1 = workbook.
    pub sheet: i32,
    pub hidden: bool,
    pub builtin: bool,
    /// Function/macro (fFunc/fOB): not a workbook name.
    pub macro_: bool,
    /// The record's data (the formula is read from `off`).
    pub data: Vec<u8>,
    pub off: usize,
    pub cce: usize,
}

#[derive(Debug)]
pub struct XlsGlobals {
    pub biff8: bool,
    pub cp: Codepage,
    pub date1904: bool,
    /// Fonts IN THE ORDER of the records (BIFF index 4 does not exist: see [`font_index`]).
    pub fonts: Vec<XlsFont>,
    pub formats: Vec<(u16, String)>,
    pub xfs: Vec<XlsXf>,
    /// The effective palette (64 colours `#RRGGBB`, indices 0–63).
    pub palette: Vec<String>,
    pub sheets: Vec<XlsSheetInfo>,
    pub sst: Vec<RichString>,
    pub active_tab: u16,
    /// MSODRAWINGGROUP concatenated (OfficeArtDggContainer).
    pub drawing_group: Option<Vec<u8>>,
    /// The workbook theme (THEME record, Excel 2007+): the zip package of the theme.
    pub theme_zip: Option<Vec<u8>>,
    /// SUPBOOK: true = the workbook itself.
    pub supbooks: Vec<bool>,
    /// SUPBOOKs of add-ins (cch = 0x3A01).
    pub addin_books: HashSet<usize>,
    pub xti: Vec<Xti>,
    /// The built-in `_FilterDatabase` name by sheet (BOUNDSHEET index): the autofilter area.
    pub filter_db: HashMap<i32, Area>,
    /// Defined names in file order (ptgName is 1-based).
    pub names: Vec<String>,
    pub name_defs: Vec<XlsNameDef>,
    /// RECALCID.dwBuild (→ calcPr calcId).
    pub recalc_id: Option<u32>,
    /// EXTERNNAME per SUPBOOK.
    pub extern_names: Vec<Vec<String>>,
}

impl XlsGlobals {
    pub fn format_of(&self, id: u16) -> Option<&str> {
        self.formats.iter().rev().find(|(i, _)| *i == id).map(|(_, s)| s.as_str())
    }
}

/// `BUILTIN_NAMES`.
fn builtin_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0 => "Consolidate_Area",
        1 => "Auto_Open",
        2 => "Auto_Close",
        3 => "Extract",
        4 => "Database",
        5 => "Criteria",
        6 => "Print_Area",
        7 => "Print_Titles",
        8 => "Recorder",
        9 => "Data_Form",
        10 => "Auto_Activate",
        11 => "Auto_Deactivate",
        12 => "Sheet_Title",
        13 => "_FilterDatabase",
        _ => return None,
    })
}

/// `fontIndex`: BIFF `ifnt` → position in `fonts` (the format omits index 4).
pub fn font_index(ifnt: u16) -> usize {
    if ifnt < 4 { usize::from(ifnt) } else { usize::from(ifnt) - 1 }
}

const RECALCID: u16 = 0x01c1;
const CONTINUE_FRT: u16 = 0x0812;

fn hex2(n: u8) -> String {
    format!("{n:02X}")
}

/// `cellLikeName`: a name that is a cell reference in the .xlsx grid (`SHH20`) or R1C1 gets a leading `_`.
fn cell_like_name(name: &str) -> String {
    let b = name.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    if (1..=3).contains(&letters) && letters < b.len() && b[letters..].iter().all(u8::is_ascii_digit) {
        let col = b[..letters].iter().fold(0u64, |a, ch| a * 26 + u64::from(ch.to_ascii_uppercase() - 64));
        let row = uo_jsnum::parse::parse_int(&name[letters..], 10);
        if col <= 16384 && (1.0..=1_048_576.0).contains(&row) {
            return format!("_{name}");
        }
    }
    // /^[Rr]\d*[Cc]\d*$/ or /^[RrCc]$/
    let rc = {
        let mut it = b.iter().peekable();
        let ok_r = matches!(it.next(), Some(b'R' | b'r'));
        if ok_r {
            while it.peek().is_some_and(|c| c.is_ascii_digit()) {
                it.next();
            }
            let ok_c = matches!(it.next(), Some(b'C' | b'c'));
            ok_c && {
                while it.peek().is_some_and(|c| c.is_ascii_digit()) {
                    it.next();
                }
                it.next().is_none()
            }
        } else {
            false
        }
    };
    if rc || matches!(b, [b'R' | b'r' | b'C' | b'c']) { format!("_{name}") } else { name.to_owned() }
}

/// `readSst`: cstTotal u32, cstUnique u32, then the strings (across the CONTINUEs).
fn read_sst(r: &Rd<'_>, conts: &[usize]) -> Vec<RichString> {
    let unique = i64::from(r.u32(4));
    let mut out = Vec::new();
    let mut p = 8usize;
    // each string takes at least 3 bytes (a hostile cstUnique allocates nothing)
    let room = i64::try_from(r.len()).unwrap_or(0) - 8;
    let max = unique.min(room.div_euclid(3) + 1);
    let mut i = 0i64;
    while i < max && p + 3 <= r.len() {
        let (s, next) = read_rich_ext_string(r, p, conts);
        out.push(s);
        p = next;
        i += 1;
    }
    out
}

/// XF of BIFF8 (20 bytes, [MS-XLS] 2.4.353).
fn read_xf8(r: &Rd<'_>) -> XlsXf {
    let al = r.u8(6);
    let ind = r.u8(8);
    let b1 = r.u32(10);
    let b2 = r.u32(14);
    let f = u32::from(r.u16(18));
    XlsXf {
        font: r.u16(0),
        fmt: r.u16(2),
        halign: al & 7,
        wrap: al & 8 != 0,
        valign: (al >> 4) & 7,
        rot: u16::from(r.u8(7)),
        indent: ind & 0x0f,
        shrink: ind & 0x10 != 0,
        b_left: b1 & 0xf,
        b_right: (b1 >> 4) & 0xf,
        b_top: (b1 >> 8) & 0xf,
        b_bottom: (b1 >> 12) & 0xf,
        c_left: (b1 >> 16) & 0x7f,
        c_right: (b1 >> 23) & 0x7f,
        diag: (b1 >> 30) & 3,
        c_top: b2 & 0x7f,
        c_bottom: (b2 >> 7) & 0x7f,
        c_diag: (b2 >> 14) & 0x7f,
        b_diag: (b2 >> 21) & 0xf,
        pattern: (b2 >> 26) & 0x3f,
        fore: f & 0x7f,
        back: (f >> 7) & 0x7f,
        ext: Vec::new(),
    }
}

/// XF of BIFF5/7 (16 bytes).
fn read_xf5(r: &Rd<'_>) -> XlsXf {
    let al = r.u8(6);
    let ori = r.u8(7) & 3;
    let a = r.u32(8);
    let b = r.u32(12);
    XlsXf {
        font: r.u16(0),
        fmt: r.u16(2),
        halign: al & 7,
        wrap: al & 8 != 0,
        valign: (al >> 4) & 7,
        rot: match ori {
            1 => 255,
            2 => 90,
            3 => 180,
            _ => 0,
        },
        indent: 0,
        shrink: false,
        fore: a & 0x7f,
        back: (a >> 7) & 0x7f,
        pattern: (a >> 16) & 0x3f,
        b_bottom: (a >> 22) & 7,
        c_bottom: (a >> 25) & 0x7f,
        b_top: b & 7,
        b_left: (b >> 3) & 7,
        b_right: (b >> 6) & 7,
        c_top: (b >> 9) & 0x7f,
        c_left: (b >> 16) & 0x7f,
        c_right: (b >> 23) & 0x7f,
        diag: 0,
        b_diag: 0,
        c_diag: 0,
        ext: Vec::new(),
    }
}

/// XFEXT ([MS-XLS] 2.4.355): full colours (RGB/theme) written by Excel 2007+ beside the XF.
fn read_xf_ext(r: &Rd<'_>, xfs: &mut [XlsXf]) {
    let ixfe = usize::from(r.u16(14));
    let Some(xf) = xfs.get_mut(ixfe) else { return };
    let n = usize::from(r.u16(18));
    let mut p = 20usize;
    let mut i = 0;
    while i < n && p + 4 <= r.len() {
        let ty = r.u16(p);
        let cb = usize::from(r.u16(p + 2));
        if cb < 4 {
            break;
        }
        // FullColorExt: xclrType u16, nTintShade i16, xclrValue u32
        if (4..=5).contains(&ty) || (7..=11).contains(&ty) || ty == 13 {
            let ct = r.u16(p + 4);
            let tint = f64::from(r.i16(p + 6)) / 32767.0;
            let v = r.u32(p + 8);
            let mut fc = FullColor { ty: ct, rgb: None, index: None, tint };
            if ct == 2 {
                fc.rgb = Some(format!("{}{}{}", hex2((v & 0xff) as u8), hex2(((v >> 8) & 0xff) as u8), hex2(((v >> 16) & 0xff) as u8)));
            } else {
                fc.index = Some(v);
            }
            if let Some(slot) = xf.ext.iter_mut().find(|(t, _)| *t == ty) {
                slot.1 = fc;
            } else {
                xf.ext.push((ty, fc));
            }
        }
        p += cb;
        i += 1;
    }
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(Vec::len).sum());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// `readGlobals`: reads the globals records (from the first BOF to the matching EOF).
pub fn read_globals<'a>(recs: impl Iterator<Item = Rec<'a>>, biff8: bool) -> Result<XlsGlobals, XlsError> {
    let mut g = XlsGlobals {
        biff8,
        cp: Codepage::new(if biff8 { 1200 } else { 1252 }),
        date1904: false,
        fonts: Vec::new(),
        formats: Vec::new(),
        xfs: Vec::new(),
        palette: INDEXED_COLORS_DEFAULT.iter().take(64).map(|s| (*s).to_owned()).collect(),
        sheets: Vec::new(),
        sst: Vec::new(),
        active_tab: 0,
        drawing_group: None,
        theme_zip: None,
        supbooks: Vec::new(),
        addin_books: HashSet::new(),
        xti: Vec::new(),
        filter_db: HashMap::new(),
        names: Vec::new(),
        name_defs: Vec::new(),
        recalc_id: None,
        extern_names: Vec::new(),
    };
    let mut dg: Vec<Vec<u8>> = Vec::new();
    let mut theme: Option<Vec<Vec<u8>>> = None;
    let mut first = true;
    for rec in recs {
        let r = rec.rd();
        if first {
            first = false;
            if rec.ty == rt::BOF {
                continue;
            }
        }
        match rec.ty {
            rt::EOF => {
                if !dg.is_empty() {
                    g.drawing_group = Some(concat(&dg));
                }
                if let Some(t) = &theme {
                    g.theme_zip = Some(concat(t));
                }
                return Ok(g);
            }
            rt::FILEPASS => return Err(XlsError::Encrypted),
            rt::CODEPAGE => {
                if !biff8 {
                    g.cp = Codepage::new(r.u16(0));
                }
            }
            rt::DATEMODE => g.date1904 = r.u16(0) == 1,
            rt::WINDOW1 => g.active_tab = r.u16(10),
            rt::FONT => {
                let grbit = r.u16(2);
                g.fonts.push(XlsFont {
                    height: r.u16(0),
                    italic: grbit & 0x02 != 0,
                    strike: grbit & 0x08 != 0,
                    outline: grbit & 0x10 != 0,
                    shadow: grbit & 0x20 != 0,
                    color: r.u16(4),
                    weight: r.u16(6),
                    script: r.u16(8),
                    underline: r.u8(10),
                    family: r.u8(11),
                    charset: r.u8(12),
                    name: read_xl_string(&r, 14, biff8, &g.cp, 1, &[]).0,
                });
            }
            rt::FORMAT => {
                let id = r.u16(0);
                let s = if biff8 { read_xl_string(&r, 2, true, &g.cp, 2, &rec.conts) } else { read_xl_string(&r, 2, false, &g.cp, 1, &[]) };
                // Map.set: a repeated id keeps its place and takes the last text
                if let Some(slot) = g.formats.iter_mut().find(|(i, _)| *i == id) {
                    slot.1 = s.0;
                } else {
                    g.formats.push((id, s.0));
                }
            }
            rt::XF => g.xfs.push(if biff8 { read_xf8(&r) } else { read_xf5(&r) }),
            rt::XFEXT => {
                if biff8 {
                    read_xf_ext(&r, &mut g.xfs);
                }
            }
            rt::PALETTE => {
                let n = usize::from(r.u16(0)).min(56);
                for i in 0..n {
                    let o = 2 + i * 4;
                    if o + 3 > r.len() {
                        break;
                    }
                    if let Some(slot) = g.palette.get_mut(8 + i) {
                        *slot = format!("#{}{}{}", hex2(r.u8(o)), hex2(r.u8(o + 1)), hex2(r.u8(o + 2)));
                    }
                }
            }
            rt::BOUNDSHEET => {
                let pos = r.u32(0);
                let state = r.u8(4) & 3;
                let ty = r.u8(5);
                let name = read_xl_string(&r, 6, biff8, &g.cp, 1, &[]).0;
                g.sheets.push(XlsSheetInfo { name, state, ty, pos });
            }
            rt::SST => {
                if biff8 {
                    g.sst = read_sst(&r, &rec.conts);
                }
            }
            rt::MSODRAWINGGROUP => dg.push(rec.data.to_vec()),
            rt::SUPBOOK => {
                // cch = 0x0401: references to the workbook itself
                if r.u16(2) == 0x3a01 {
                    g.addin_books.insert(g.supbooks.len());
                }
                g.supbooks.push(r.u16(2) == 0x0401);
            }
            rt::NAME => {
                // Lbl (BIFF8): grbit, chKey, cch, cce, reserved, itab (1-based), 4 reserved, the name (options + chars),
                // the formula. Built-ins: one character with the code (0x0D = _FilterDatabase).
                if biff8 {
                    let grbit = r.u16(0);
                    let cch = usize::from(r.u8(3));
                    let cce = usize::from(r.u16(4));
                    let itab = i32::from(r.u16(8));
                    let high = r.u8(14) & 1 != 0;
                    let f_off = 15 + cch * if high { 2 } else { 1 };
                    let name = if grbit & 0x20 != 0 {
                        let code = if high { r.u16(15) } else { u16::from(r.u8(15)) };
                        if code == 0x0d
                            && itab > 0
                            && let Some(area) = formula_areas(&r, f_off, cce).into_iter().next()
                        {
                            g.filter_db.insert(itab - 1, area);
                        }
                        builtin_name(code).map_or_else(|| format!("_xlnm{code}"), str::to_owned)
                    } else {
                        cell_like_name(&read_chars(&r, 15, cch, high, &[]).0)
                    };
                    g.names.push(name.clone());
                    g.name_defs.push(XlsNameDef {
                        name,
                        sheet: itab - 1,
                        hidden: grbit & 1 != 0,
                        builtin: grbit & 0x20 != 0,
                        macro_: grbit & 0x0e != 0,
                        data: rec.data.to_vec(),
                        off: f_off,
                        cce,
                    });
                } else {
                    g.names.push(String::new());
                }
            }
            rt::EXTERNNAME => {
                // external names (add-in functions): the text is at offset 6
                if biff8 && !g.supbooks.is_empty() {
                    let k = g.supbooks.len() - 1;
                    while g.extern_names.len() <= k {
                        g.extern_names.push(Vec::new());
                    }
                    let t = read_xl_string(&r, 6, true, &g.cp, 1, &[]).0;
                    g.extern_names[k].push(t);
                }
            }
            rt::EXTERNSHEET => {
                if biff8 {
                    let n = usize::from(r.u16(0));
                    let mut i = 0;
                    let mut o = 2;
                    while i < n && o + 6 <= r.len() {
                        g.xti.push(Xti { book: r.u16(o), tab: r.i16(o + 2), last: r.i16(o + 4) });
                        i += 1;
                        o += 6;
                    }
                }
            }
            RECALCID => {
                // FrtHeaderOld (rt u16 = 0x01C1 + grbitFrt u16) + dwBuild u32 (junk from other writers is ignored)
                if r.len() >= 8 && r.u16(0) == RECALCID && r.u32(4) > 0 && r.u32(4) < 1_000_000 {
                    g.recalc_id = Some(r.u32(4));
                }
            }
            rt::THEME => {
                // FrtHeader (12 bytes) + dwThemeVersion: 0 = CUSTOM theme (the zip package follows); 124226 = the Office
                // 2007 default (no data)
                if r.u32(12) == 0 && r.len() > 16 {
                    theme = Some(vec![r.sub(16, r.len()).to_vec()]);
                    continue;
                }
            }
            CONTINUE_FRT => {
                // continuation of the THEME: FrtHeaderOld (rt u16 + grbitFrt u16) + data
                if let Some(t) = theme.as_mut() {
                    t.push(r.sub(4, r.len()).to_vec());
                    continue;
                }
            }
            _ => {}
        }
        if let Some(t) = theme.take() {
            g.theme_zip = Some(concat(&t));
        }
    }
    if !dg.is_empty() {
        g.drawing_group = Some(concat(&dg));
    }
    if let Some(t) = &theme {
        g.theme_zip = Some(concat(t));
    }
    Ok(g)
}
