//! Data validation of BIFF8 (DV, after the DVAL) → `<dataValidation>` (`dv.ts`): type, operator, messages and the
//! formulas (an explicit list `a\0b` → `a,b`).

use super::biff::{Codepage, Rd, read_xl_string};
use super::formula::{FormulaContext, chars_text, decompile};
use super::xmlw::{range_ref_s, xml_text_s};

const TYPES: [&str; 8] = ["none", "whole", "decimal", "list", "date", "time", "textLength", "custom"];
const OPS: [&str; 8] = ["between", "notBetween", "equal", "notEqual", "greaterThan", "lessThan", "greaterThanOrEqual", "lessThanOrEqual"];
const ERR_STYLES: [&str; 3] = ["stop", "warning", "information"];

/// `readDv`: DV → `<dataValidation>` (`None` = invalid record).
pub fn read_dv(r: &Rd<'_>, cp: &Codepage, ctx: &mut FormulaContext<'_>) -> Option<String> {
    let fl = r.u32(0);
    let mut p = 4usize;
    let mut strs: Vec<String> = Vec::with_capacity(4);
    for _ in 0..4 {
        let (text, next) = read_xl_string(r, p, true, cp, 2, &[]);
        strs.push(if text == "\0" { String::new() } else { text });
        p = next;
    }
    let f1_off = p;
    let cce1 = usize::from(r.u16(p));
    p += 4 + cce1;
    let cce2 = usize::from(r.u16(p));
    p += 4 + cce2;
    let n = usize::from(r.u16(p));
    let mut refs: Vec<String> = Vec::new();
    let mut first = (0u32, 0u32);
    let mut i = 0;
    let mut o = p + 2;
    while i < n && o + 8 <= r.len() {
        let (r1, r2, c1, c2) = (u32::from(r.u16(o)), u32::from(r.u16(o + 2)), u32::from(r.u16(o + 4)), u32::from(r.u16(o + 6)));
        i += 1;
        o += 8;
        if r2 < r1 || c2 < c1 {
            continue;
        }
        if refs.is_empty() {
            first = (r1, c1);
        }
        refs.push(range_ref_s(r1, c1, r2, c2));
    }
    if refs.is_empty() {
        return None;
    }
    ctx.row = first.0;
    ctx.col = first.1;
    let ty = fl & 0xf;
    // explicit list: a ptgStr with the items separated by \0
    let decode = |ctx: &FormulaContext<'_>, off: usize, cce: usize| -> Option<String> {
        if cce == 0 {
            return None;
        }
        if ty == 3 && fl & 0x80 != 0 && r.u8(off) == 0x17 {
            let cch = usize::from(r.u8(off + 1));
            let high = r.u8(off + 2) & 1 != 0;
            let s = chars_text(r, off + 3, cch, high);
            return Some(format!("\"{}\"", s.split('\0').collect::<Vec<_>>().join(",").replace('"', "\"\"")));
        }
        decompile(r, off, cce, ctx)
    };
    let formula1 = decode(ctx, f1_off + 4, cce1);
    let formula2 = decode(ctx, f1_off + 4 + cce1 + 4, cce2);
    let mut x = format!("<dataValidation type=\"{}\"", TYPES.get(usize::try_from(ty).unwrap_or(usize::MAX)).copied().unwrap_or("none"));
    if ty != 0 && ty != 3 && ty != 7 {
        x.push_str(&format!(
            " operator=\"{}\"",
            OPS.get(usize::try_from((fl >> 20) & 0xf).unwrap_or(usize::MAX)).copied().unwrap_or("between")
        ));
    }
    if let Some(es) = ERR_STYLES.get(usize::try_from((fl >> 4) & 7).unwrap_or(usize::MAX))
        && *es != "stop"
    {
        x.push_str(&format!(" errorStyle=\"{es}\""));
    }
    if fl & 0x100 != 0 {
        x.push_str(" allowBlank=\"1\"");
    }
    if fl & 0x200 != 0 {
        x.push_str(" showDropDown=\"1\"");
    }
    if fl & 0x40000 != 0 {
        x.push_str(" showInputMessage=\"1\"");
    }
    if fl & 0x80000 != 0 {
        x.push_str(" showErrorMessage=\"1\"");
    }
    for (k, name) in [(1, "errorTitle"), (3, "error"), (0, "promptTitle"), (2, "prompt")] {
        if !strs[k].is_empty() {
            x.push_str(&format!(" {name}=\"{}\"", xml_text_s(&strs[k])));
        }
    }
    x.push_str(&format!(" sqref=\"{}\">", refs.join(" ")));
    if let Some(f) = formula1 {
        x.push_str(&format!("<formula1>{}</formula1>", xml_text_s(&f)));
    }
    if let Some(f) = formula2 {
        x.push_str(&format!("<formula2>{}</formula2>", xml_text_s(&f)));
    }
    x.push_str("</dataValidation>");
    Some(x)
}
