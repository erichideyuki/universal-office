//! Formulas of BIFF8 (`formula.ts`, rgce): the areas of a name or a chart series (`ptgRef3d`/`ptgArea3d` with the
//! EXTERNSHEET index, local `ptgRef`/`ptgArea`) and the decompiler of the RPN to the text Excel shows (English, commas,
//! as the `<f>` of the .xlsx).

use super::biff::Rd;
use super::data::FTAB;
use super::globals::XlsGlobals;
use super::xmlw::{col_name, push_u};

/// A 0-based inclusive area; `ixti` = index in the EXTERNSHEET (−1 = local).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub ixti: i32,
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
}

/// `formulaAreas`: the areas of a formula in `[off, off + cce)`; stops at the first unsupported token.
pub fn formula_areas(r: &Rd<'_>, off: usize, cce: usize) -> Vec<Area> {
    let mut out = Vec::new();
    let end = r.len().min(off.saturating_add(cce));
    let mut p = off;
    let c14 = |o: usize| u32::from(r.u16(o) & 0x3fff);
    while p < end {
        let ptg = r.u8(p);
        let base = (ptg & 0x1f) | 0x20;
        if ptg == 0x10 || ptg == 0x15 {
            p += 1;
            continue;
        }
        if ptg == 0x29 || ptg == 0x49 || ptg == 0x69 {
            p += 3;
            continue;
        }
        if ptg < 0x20 {
            break;
        }
        match base {
            0x3a => {
                out.push(Area {
                    ixti: i32::from(r.u16(p + 1)),
                    r1: u32::from(r.u16(p + 3)),
                    c1: c14(p + 5),
                    r2: u32::from(r.u16(p + 3)),
                    c2: c14(p + 5),
                });
                p += 7;
            }
            0x3b => {
                out.push(Area {
                    ixti: i32::from(r.u16(p + 1)),
                    r1: u32::from(r.u16(p + 3)),
                    r2: u32::from(r.u16(p + 5)),
                    c1: c14(p + 7),
                    c2: c14(p + 9),
                });
                p += 11;
            }
            0x24 => {
                out.push(Area { ixti: -1, r1: u32::from(r.u16(p + 1)), c1: c14(p + 3), r2: u32::from(r.u16(p + 1)), c2: c14(p + 3) });
                p += 5;
            }
            0x25 => {
                out.push(Area { ixti: -1, r1: u32::from(r.u16(p + 1)), r2: u32::from(r.u16(p + 3)), c1: c14(p + 5), c2: c14(p + 7) });
                p += 9;
            }
            _ => break, // errors, names, operators: stop
        }
    }
    out
}

/// What a decompilation needs: the workbook (sheets by EXTERNSHEET, names, external names), the base cell (0-based)
/// for `ptgRefN`/`ptgAreaN` and the relative-3D flag (shared formula/name: `ptgRef3d`/`ptgArea3d` with relative bits
/// hold an OFFSET, like RefN/AreaN, [MS-XLS] RgceLocRel).
pub struct FormulaContext<'g> {
    pub g: &'g XlsGlobals,
    pub row: u32,
    pub col: u32,
    pub rel3d: bool,
}

impl<'g> FormulaContext<'g> {
    pub fn new(g: &'g XlsGlobals) -> Self {
        FormulaContext { g, row: 0, col: 0, rel3d: false }
    }

    /// The sheet prefix of an EXTERNSHEET index without the `!` (quoted if needed, 3D = `Sheet1:Sheet3`), `#REF`
    /// (deleted sheet) or `None` (external/invalid).
    fn sheet_of(&self, ixti: usize) -> Option<String> {
        let g = self.g;
        let x = g.xti.get(ixti)?;
        if !g.supbooks.get(usize::from(x.book)).copied().unwrap_or(false) {
            return None;
        }
        if x.tab < 0 || x.last < 0 {
            return Some("#REF".to_owned());
        }
        let (Some(a), Some(b)) =
            (g.sheets.get(usize::try_from(x.tab).unwrap_or(usize::MAX)), g.sheets.get(usize::try_from(x.last).unwrap_or(usize::MAX)))
        else {
            return Some("#REF".to_owned());
        };
        if x.last == x.tab {
            return Some(quote_sheet(&a.name));
        }
        let both = format!("{}:{}", a.name, b.name);
        Some(if quote_sheet(&a.name) == a.name && quote_sheet(&b.name) == b.name {
            both
        } else {
            format!("'{}'", both.replace('\'', "''"))
        })
    }

    /// `sheet(ixti)` of the decompiler: `name!`, `#REF!` (deleted sheet) or `None` (external/invalid).
    fn sheet_prefix(&self, ixti: u16) -> Option<String> {
        let s = self.sheet_of(usize::from(ixti))?;
        Some(if s == "#REF" { "#REF!".to_owned() } else { format!("{s}!") })
    }

    /// The defined name (1-based).
    fn name_of(&self, i: u32) -> Option<String> {
        let g = self.g;
        let k = usize::try_from(i).ok()?.checked_sub(1)?;
        let nm = g.names.get(k)?;
        // a macro function name (UDF of VBA/XLM, fFunc): a call to a user function: Excel shows =@Sum(…) even with a
        // native name (measured: lo-user_defined_function)
        let udf = g.name_defs.get(k).is_some_and(|d| d.macro_ && !d.builtin);
        Some(if udf && !nm.is_empty() { format!("_xludf.{nm}") } else { nm.clone() })
    }

    /// An external name (ptgNameX: ixti + 1-based index). An add-in function (SUPBOOK of an add-in): Excel 365 does
    /// not resolve it — it shows `=@ISEVEN(A1)` and gives #NAME? (measured on the corpus).
    fn extern_of(&self, ixti: usize, i: u32) -> Option<String> {
        let g = self.g;
        let x = g.xti.get(ixti)?;
        let k = usize::try_from(i).ok()?.checked_sub(1)?;
        let nm = g.extern_names.get(usize::from(x.book)).and_then(|v| v.get(k))?;
        let starts_xl = nm.len() >= 3 && nm.as_bytes()[..3].eq_ignore_ascii_case(b"_xl");
        Some(if g.addin_books.contains(&usize::from(x.book)) && !starts_xl { format!("_xll.{nm}") } else { nm.clone() })
    }
}

/// `quoteSheet`: a sheet name as it appears in a formula (quoted if it has a space/symbol).
pub fn quote_sheet(name: &str) -> String {
    let mut it = name.chars();
    let first_ok = it.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c >= '\u{c0}');
    let rest_ok = first_ok && it.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c >= '\u{c0}');
    // /^[A-Za-z]{1,3}\d+$/
    let b = name.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let cell_like = (1..=3).contains(&letters) && letters < b.len() && b[letters..].iter().all(u8::is_ascii_digit);
    if rest_ok && !cell_like { name.to_owned() } else { format!("'{}'", name.replace('\'', "''")) }
}

fn bin_op(ptg: u8) -> &'static str {
    match ptg {
        0x03 => "+",
        0x04 => "-",
        0x05 => "*",
        0x06 => "/",
        0x07 => "^",
        0x08 => "&",
        0x09 => "<",
        0x0a => "<=",
        0x0b => "=",
        0x0c => ">=",
        0x0d => ">",
        0x0e => "<>",
        0x0f => " ",
        0x10 => ",",
        _ => ":",
    }
}

/// `ERR`: the error literal of a code (`#N/A` when unknown).
pub fn err_text(code: u8) -> &'static str {
    match code {
        0x00 => "#NULL!",
        0x07 => "#DIV/0!",
        0x0f => "#VALUE!",
        0x17 => "#REF!",
        0x1d => "#NAME?",
        0x24 => "#NUM!",
        _ => "#N/A",
    }
}

/// A1 with `$` by the relative bits (column: bit 14, row: bit 15 of the column field).
fn a1(out: &mut String, row: u16, col_field: u16) {
    if col_field & 0x4000 == 0 {
        out.push('$');
    }
    col_name(out, u32::from(col_field & 0x3fff));
    if col_field & 0x8000 == 0 {
        out.push('$');
    }
    push_u(out, u64::from(row) + 1);
}

/// Sign-extends the low 16 / 8 bits (`(x << 16) >> 16`, `(x << 24) >> 24`).
fn s16(v: u16) -> i64 {
    i64::from(i16::from_le_bytes(v.to_le_bytes()))
}

fn s8(v: u16) -> i64 {
    i64::from(i8::from_le_bytes([(v & 0xff) as u8]))
}

/// RefN/AreaN position: offsets relative to the base cell (BIFF8: row i16, column i8 with 256 columns).
fn rel_pos(row_field: u16, col_field: u16, rel: bool, ctx: &FormulaContext<'_>) -> (i64, i64, bool, bool) {
    let r_rel = col_field & 0x8000 != 0;
    let c_rel = col_field & 0x4000 != 0;
    let r = if rel && r_rel { (i64::from(ctx.row) + s16(row_field) + 65536).rem_euclid(65536) } else { i64::from(row_field) };
    let c = if rel && c_rel { (i64::from(ctx.col) + s8(col_field) + 256).rem_euclid(256) } else { i64::from(col_field & 0x3fff) };
    (r, c, !r_rel, !c_rel)
}

fn a1_rel(out: &mut String, row_field: u16, col_field: u16, ctx: &FormulaContext<'_>) {
    let (r, c, r_abs, c_abs) = rel_pos(row_field, col_field, true, ctx);
    if c_abs {
        out.push('$');
    }
    col_name(out, u32::try_from(c).unwrap_or(0));
    if r_abs {
        out.push('$');
    }
    push_u(out, u64::try_from(r + 1).unwrap_or(0));
}

/// Area A1; a whole row/column of BIFF8 (65536 rows / 256 columns) becomes `$1:$65536` / `A:A` as Excel writes it.
fn area_text(out: &mut String, r1f: u16, r2f: u16, c1f: u16, c2f: u16, rel: bool, ctx: &FormulaContext<'_>) {
    let a = rel_pos(r1f, c1f, rel, ctx);
    let b = rel_pos(r2f, c2f, rel, ctx);
    let col = |o: &mut String, x: &(i64, i64, bool, bool)| {
        if x.3 {
            o.push('$');
        }
        col_name(o, u32::try_from(x.1).unwrap_or(0));
    };
    let row = |o: &mut String, x: &(i64, i64, bool, bool)| {
        if x.2 {
            o.push('$');
        }
        push_u(o, u64::try_from(x.0 + 1).unwrap_or(0));
    };
    // the whole sheet (A1:IV65536): Excel writes the ROWS ($1:$65536)
    if a.1 == 0 && b.1 == 0xff {
        row(out, &a);
        out.push(':');
        row(out, &b);
    } else if a.0 == 0 && b.0 == 0xffff {
        col(out, &a);
        out.push(':');
        col(out, &b);
    } else {
        col(out, &a);
        row(out, &a);
        out.push(':');
        col(out, &b);
        row(out, &b);
    }
}

/// `numText`: a number as Excel writes it in a formula (measured with Formula2): 15 significant digits; decimal when it
/// fits 21 characters, else scientific.
pub fn num_text(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return "0".to_owned();
    }
    let neg = v < 0.0;
    let e14 = uo_jsnum::format::to_exponential(v.abs(), 14).unwrap_or_default();
    let (m, e) = e14.split_once('e').unwrap_or((&e14, "0"));
    let mut digits: String = m.chars().filter(|&c| c != '.').collect();
    while digits.ends_with('0') {
        digits.pop();
    }
    if digits.is_empty() {
        digits.push('0');
    }
    let exp = e.parse::<i64>().unwrap_or(0);
    let dec = if exp >= 0 {
        let n = usize::try_from(exp + 1).unwrap_or(0);
        let mut int: String = digits.chars().take(n).collect();
        while int.len() < n {
            int.push('0');
        }
        let frac: String = digits.chars().skip(n).collect();
        if frac.is_empty() { int } else { format!("{int}.{frac}") }
    } else {
        format!("0.{}{}", "0".repeat(usize::try_from(-exp - 1).unwrap_or(0)), digits)
    };
    let out = if dec.len() <= 21 {
        dec
    } else {
        let mut s = String::new();
        s.push_str(&digits[..1]);
        if digits.len() > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('E');
        s.push(if exp < 0 { '-' } else { '+' });
        s.push_str(&exp.abs().to_string());
        s
    };
    if neg { format!("-{out}") } else { out }
}

/// The units of a string stored as characters (8-bit or UTF-16), as `String.fromCharCode` builds it, then the text.
pub fn chars_text(r: &Rd<'_>, at: usize, cch: usize, high: bool) -> String {
    let units: Vec<u16> = (0..cch).map(|k| if high { r.u16(at + k * 2) } else { u16::from(r.u8(at + k)) }).collect();
    String::from_utf16_lossy(&units)
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// An array constant (PtgExtraArray, BIFF8) at `p`: the `{…}` text and the end.
fn array_const(r: &Rd<'_>, mut p: usize) -> Option<(String, usize)> {
    if p + 3 > r.len() {
        return None;
    }
    let cols = usize::from(r.u8(p)) + 1;
    let rows = usize::from(r.u16(p + 1)) + 1;
    p += 3;
    let mut out: Vec<String> = Vec::new();
    for _ in 0..rows {
        let mut row: Vec<String> = Vec::new();
        for _ in 0..cols {
            if p >= r.len() {
                return None;
            }
            match r.u8(p) {
                0x00 => {
                    row.push(String::new());
                    p += 9;
                }
                0x01 => {
                    row.push(num_text(r.f64(p + 1)));
                    p += 9;
                }
                0x02 => {
                    let cch = usize::from(r.u16(p + 1));
                    let high = r.u8(p + 3) & 1 != 0;
                    row.push(quoted(&chars_text(r, p + 4, cch, high)));
                    p += 4 + cch * if high { 2 } else { 1 };
                }
                0x04 => {
                    row.push(if r.u8(p + 1) != 0 { "TRUE" } else { "FALSE" }.to_owned());
                    p += 9;
                }
                0x10 => {
                    row.push(err_text(r.u8(p + 1)).to_owned());
                    p += 9;
                }
                _ => return None,
            }
        }
        out.push(row.join(","));
    }
    Some((format!("{{{}}}", out.join(";")), p))
}

/// `decompile`: the RPN formula in `[off, off + cce)` as the Excel text (no leading `=`). The extra data (array
/// constants, `ptgMem*` areas) come AFTER the rgce, in token order. `None` = unsupported token (reference to another
/// workbook, future extensions…).
pub fn decompile(r: &Rd<'_>, off: usize, cce: usize, ctx: &FormulaContext<'_>) -> Option<String> {
    let mut out = String::new();
    if decompile_into(r, off, cce, ctx, &mut out) { Some(out) } else { None }
}

/// [`decompile`] into `out` (cleared first); `false` = unsupported token.
///
/// The TS keeps a stack of strings; here the operands live in ONE buffer and the stack holds the start of each: an operand ends where
/// the next begins, so the operators, the commas and the parentheses are inserted in the buffer (what follows is a few bytes).
pub fn decompile_into(r: &Rd<'_>, off: usize, cce: usize, ctx: &FormulaContext<'_>, out: &mut String) -> bool {
    out.clear();
    let end = r.len().min(off.saturating_add(cce));
    let mut extra = off.saturating_add(cce);
    let mut st: Vec<usize> = Vec::with_capacity(16);
    let mut p = off;
    macro_rules! push_item {
        ($($e:expr),*) => {{
            st.push(out.len());
            $( out.push_str($e); )*
        }};
    }
    while p < end {
        let ptg = r.u8(p);
        if (0x03..=0x11).contains(&ptg) {
            if st.len() < 2 {
                return false;
            }
            let Some(sb) = st.pop() else { return false };
            out.insert_str(sb, bin_op(ptg));
            p += 1;
            continue;
        }
        match ptg {
            0x01 | 0x02 => return false, // ptgExp/ptgTbl: handled by the caller
            0x12 | 0x13 => {
                let Some(&a) = st.last() else { return false };
                out.insert(a, if ptg == 0x12 { '+' } else { '-' });
                p += 1;
                continue;
            }
            0x14 => {
                if st.is_empty() {
                    return false;
                }
                out.push('%');
                p += 1;
                continue;
            }
            0x15 => {
                let Some(&a) = st.last() else { return false };
                out.insert(a, '(');
                out.push(')');
                p += 1;
                continue;
            }
            0x16 => {
                push_item!();
                p += 1;
                continue;
            }
            0x17 => {
                let cch = usize::from(r.u8(p + 1));
                let high = r.u8(p + 2) & 1 != 0;
                let q = quoted(&chars_text(r, p + 3, cch, high));
                push_item!(&q);
                p += 3 + cch * if high { 2 } else { 1 };
                continue;
            }
            0x18 => return false, // extensions (ptgElfLel…): not supported
            0x19 => {
                let kind = r.u8(p + 1);
                let w = usize::from(r.u16(p + 2));
                if kind & 0x10 != 0 {
                    let Some(&a) = st.last() else { return false };
                    out.insert_str(a, "SUM(");
                    out.push(')');
                }
                p += 4 + if kind & 0x04 != 0 { (w + 1) * 2 } else { 0 };
                continue;
            }
            0x1c => {
                push_item!(err_text(r.u8(p + 1)));
                p += 2;
                continue;
            }
            0x1d => {
                push_item!(if r.u8(p + 1) != 0 { "TRUE" } else { "FALSE" });
                p += 2;
                continue;
            }
            0x1e => {
                st.push(out.len());
                push_u(out, u64::from(r.u16(p + 1)));
                p += 3;
                continue;
            }
            0x1f => {
                let t = num_text(r.f64(p + 1));
                push_item!(&t);
                p += 9;
                continue;
            }
            _ => {}
        }
        let base = (ptg & 0x1f) | 0x20;
        match base {
            0x20 => {
                // ptgArray: the values are in the extra data
                let Some((text, next)) = array_const(r, extra) else { return false };
                push_item!(&text);
                extra = next;
                p += 8;
            }
            0x21 | 0x22 => {
                let var_args = base == 0x22;
                let argc = if var_args { usize::from(r.u8(p + 1) & 0x7f) } else { 0 };
                let tab = r.u16(p + if var_args { 2 } else { 1 }) & 0x7fff;
                p += if var_args { 4 } else { 3 };
                if tab == 255 {
                    // add-in / user-defined function: the first argument is the name
                    let n = argc;
                    if n > st.len() || n == 0 {
                        return false;
                    }
                    let first = st.len() - n;
                    // the name must not be empty: its text ends where the next item (or the end) begins
                    let name_end = st.get(first + 1).copied().unwrap_or(out.len());
                    if name_end == st[first] {
                        return false;
                    }
                    // `name(arg2,arg3,…)`
                    for k in (2..n).rev() {
                        out.insert(st[first + k], ',');
                    }
                    if n > 1 {
                        out.insert(st[first + 1], '(');
                    } else {
                        out.push('(');
                    }
                    out.push(')');
                    st.truncate(first + 1);
                    continue;
                }
                let Ok(k) = FTAB.binary_search_by_key(&tab, |e| e.0) else { return false };
                let (_, name, fixed) = FTAB[k];
                let n = if var_args { i64::try_from(argc).unwrap_or(0) } else { i64::from(fixed) };
                if n < 0 {
                    return false;
                }
                let n = usize::try_from(n).unwrap_or(0);
                if n > st.len() {
                    return false;
                }
                // `name(a1,a2,…,an)`
                let first = st.len() - n;
                let start = if n > 0 { st[first] } else { out.len() };
                for k in (1..n).rev() {
                    out.insert(st[first + k], ',');
                }
                out.insert_str(start, name);
                out.insert(start + name.len(), '(');
                out.push(')');
                st.truncate(first);
                st.push(start);
            }
            0x23 => {
                let Some(nm) = ctx.name_of(r.u32(p + 1)) else { return false };
                push_item!(&nm);
                p += 5;
            }
            0x24 => {
                st.push(out.len());
                a1(out, r.u16(p + 1), r.u16(p + 3));
                p += 5;
            }
            0x25 => {
                st.push(out.len());
                area_text(out, r.u16(p + 1), r.u16(p + 3), r.u16(p + 5), r.u16(p + 7), false, ctx);
                p += 9;
            }
            0x26..=0x28 => {
                // ptgMemArea/MemErr/MemNoMem: the area comes in the next tokens; MemArea has extra data (cAreas + Ref8U)
                if base == 0x26 {
                    extra += 2 + usize::from(r.u16(extra)) * 8;
                }
                p += 7;
            }
            0x29 => p += 3,
            0x2a => {
                push_item!("#REF!");
                p += 5;
            }
            0x2b => {
                push_item!("#REF!");
                p += 9;
            }
            0x2c => {
                st.push(out.len());
                a1_rel(out, r.u16(p + 1), r.u16(p + 3), ctx);
                p += 5;
            }
            0x2d => {
                st.push(out.len());
                area_text(out, r.u16(p + 1), r.u16(p + 3), r.u16(p + 5), r.u16(p + 7), true, ctx);
                p += 9;
            }
            0x39 => {
                let Some(nm) = ctx.extern_of(usize::from(r.u16(p + 1)), u32::from(r.u16(p + 3))) else { return false };
                push_item!(&nm);
                p += 7;
            }
            0x3a => {
                let Some(sh) = ctx.sheet_prefix(r.u16(p + 1)) else { return false };
                if sh == "#REF!" {
                    push_item!("#REF!");
                    p += 7;
                    continue;
                }
                st.push(out.len());
                out.push_str(&sh);
                if ctx.rel3d {
                    a1_rel(out, r.u16(p + 3), r.u16(p + 5), ctx);
                } else {
                    a1(out, r.u16(p + 3), r.u16(p + 5));
                }
                p += 7;
            }
            0x3b => {
                let Some(sh) = ctx.sheet_prefix(r.u16(p + 1)) else { return false };
                if sh == "#REF!" {
                    push_item!("#REF!");
                    p += 11;
                    continue;
                }
                st.push(out.len());
                out.push_str(&sh);
                area_text(out, r.u16(p + 3), r.u16(p + 5), r.u16(p + 7), r.u16(p + 9), ctx.rel3d, ctx);
                p += 11;
            }
            0x3c | 0x3d => {
                st.push(out.len());
                match ctx.sheet_prefix(r.u16(p + 1)) {
                    Some(s) if s != "#REF!" => {
                        out.push_str(&s);
                        out.push_str("#REF!");
                    }
                    _ => out.push_str("#REF!"),
                }
                p += if base == 0x3c { 7 } else { 11 };
            }
            _ => return false,
        }
    }
    st.len() == 1
}
