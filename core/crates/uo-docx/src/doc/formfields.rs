//! Form fields (`FORMTEXT` / `FORMCHECKBOX` / `FORMDROPDOWN`, `formfields.ts`): the character 0x01
//! with `sprmCFData` in the instruction of the field points (`sprmCPicLocation`) to a
//! `NilPICFAndBinData` in the `Data` stream whose binary data is an `FFData` ([MS-DOC] 2.9.75). It
//! becomes `w:ffData` inside the `w:fldChar` "begin", as the Word writes it.

use super::bytes::ByteReader;
use super::xmlw::{AttrVal, el_of};

/// An `Xstz`: `cch` (u16, at most 0x7fff) + UTF-16 characters + terminator; the string and the
/// offset after it.
fn xstz(r: ByteReader<'_>, off: u64) -> (Vec<u16>, u64) {
    let cch = u64::from(r.u16(off).min(0x7fff));
    (r.utf16(off + 2, cch), off + 2 + cch * 2 + 2)
}

/// `read`: the next `Xstz` while two bytes remain before `end`, else the empty string.
fn read(r: ByteReader<'_>, o: &mut u64, end: u64) -> Vec<u16> {
    if *o + 2 > end {
        return Vec::new();
    }
    let (s, next) = xstz(r, *o);
    *o = next;
    s
}

fn val<'a>(name: &str, v: AttrVal<'a>) -> String {
    el_of(name, &[("w:val", Some(v))], "")
}

/// `ffDataXml`: the `w:ffData` of the form field whose data starts at `fc` of the `Data` stream.
#[must_use]
pub fn ff_data_xml(data: Option<&[u8]>, fc: i32) -> Option<String> {
    let data = data?;
    // `fc` is a JS number: negative is refused, and the sums do not wrap
    let fc = u64::try_from(fc).ok()?;
    if fc + 0x44 + 10 > data.len() as u64 {
        return None;
    }
    let r = ByteReader::new(data);
    let lcb = i64::from(r.i32(fc));
    let cb_header = r.u16(fc + 4);
    if cb_header != 0x44 || lcb < i64::from(cb_header) {
        return None;
    }
    let end = (data.len() as u64).min(fc + u64::try_from(lcb).unwrap_or(0));
    let mut o = fc + u64::from(cb_header);
    // FFData.version: the spec asks for 0xFFFFFFFF, but a .doc with 0 was measured in the Word: not validated
    let bits = r.u16(o + 4);
    let i_type = bits & 3;
    let i_res = (bits >> 2) & 0x1f;
    let f_own_help = bits & 0x80 != 0;
    let f_own_stat = bits & 0x100 != 0;
    let f_prot = bits & 0x200 != 0;
    let i_size = bits & 0x400 != 0;
    let i_type_txt = (bits >> 11) & 7;
    let f_recalc = bits & 0x4000 != 0;
    let cch = r.u16(o + 6);
    let hps = r.u16(o + 8);
    o += 10;
    let name = read(r, &mut o, end);
    let mut text_def = Vec::new();
    let mut w_def = 0u16;
    if i_type == 0 {
        text_def = read(r, &mut o, end);
    } else {
        w_def = r.u16(o);
        o += 2;
    }
    let format = read(r, &mut o, end);
    let help = read(r, &mut o, end);
    let stat = read(r, &mut o, end);
    let entry_mcr = read(r, &mut o, end);
    let exit_mcr = read(r, &mut o, end);
    let mut entries: Vec<Vec<u16>> = Vec::new();
    if i_type == 2 && o + 6 <= end {
        // an extended STTB: fExtend (0xFFFF), cData, cbExtra, entries of cch (u16) + UTF-16
        let mut p = o;
        if r.u16(p) == 0xffff {
            p += 2;
        }
        let n = u64::from(r.u16(p));
        let cb_extra = u64::from(r.u16(p + 2));
        p += 4;
        let mut i = 0;
        while i < n && p + 2 <= end {
            let c = u64::from(r.u16(p));
            entries.push(r.utf16(p + 2, c));
            p += 2 + c * 2 + cb_extra;
            i += 1;
        }
    }
    let mut x = val("w:name", AttrVal::Units(&name));
    if !f_prot {
        x += "<w:enabled/>";
    }
    x += if f_recalc { "<w:calcOnExit/>" } else { "<w:calcOnExit w:val=\"0\"/>" };
    if !entry_mcr.is_empty() {
        x += &val("w:entryMacro", AttrVal::Units(&entry_mcr));
    }
    if !exit_mcr.is_empty() {
        x += &val("w:exitMacro", AttrVal::Units(&exit_mcr));
    }
    if f_own_help && !help.is_empty() {
        x += &el_of("w:helpText", &[("w:type", Some(AttrVal::Str("text"))), ("w:val", Some(AttrVal::Units(&help)))], "");
    }
    if f_own_stat && !stat.is_empty() {
        x += &el_of("w:statusText", &[("w:type", Some(AttrVal::Str("text"))), ("w:val", Some(AttrVal::Units(&stat)))], "");
    }
    if i_type == 1 {
        let mut c = if i_size { val("w:size", AttrVal::Int(i64::from(hps))) } else { "<w:sizeAuto/>".to_owned() };
        c += &val("w:default", AttrVal::Int(i64::from(w_def != 0)));
        if i_res != 25 {
            c += &val("w:checked", AttrVal::Int(i64::from(i_res != 0)));
        }
        x += &format!("<w:checkBox>{c}</w:checkBox>");
    } else if i_type == 2 {
        let mut c = String::new();
        if i_res != 0 && i_res != 25 {
            c += &val("w:result", AttrVal::Int(i64::from(i_res)));
        }
        if w_def != 0 {
            c += &val("w:default", AttrVal::Int(i64::from(w_def)));
        }
        for e in &entries {
            c += &val("w:listEntry", AttrVal::Units(e));
        }
        x += &format!("<w:ddList>{c}</w:ddList>");
    } else {
        let mut c = String::new();
        let kind = ["regular", "number", "date", "currentDate", "currentTime", "calculated"].get(usize::from(i_type_txt)).copied();
        if let Some(kind) = kind
            && kind != "regular"
        {
            c += &val("w:type", AttrVal::Str(kind));
        }
        if !text_def.is_empty() {
            c += &val("w:default", AttrVal::Units(&text_def));
        }
        if cch != 0 {
            c += &val("w:maxLength", AttrVal::Int(i64::from(cch)));
        }
        if !format.is_empty() {
            c += &val("w:format", AttrVal::Units(&format));
        }
        x += &if c.is_empty() { "<w:textInput/>".to_owned() } else { format!("<w:textInput>{c}</w:textInput>") };
    }
    Some(format!("<w:ffData>{x}</w:ffData>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A NilPICFAndBinData with an FFData of a checkbox.
    fn checkbox(bits: u16) -> Vec<u8> {
        let mut d = vec![0u8; 0x44];
        d.extend([0u8; 4]);
        d.extend(bits.to_le_bytes());
        d.extend([0u8; 4]);
        // the name: cch 1, "a", terminator; then the default word
        d.extend([1, 0, 0x61, 0, 0, 0]);
        d.extend([0, 0]);
        let n = d.len();
        d[0..4].copy_from_slice(&u32::try_from(n).unwrap().to_le_bytes());
        d[4..6].copy_from_slice(&0x44u16.to_le_bytes());
        d
    }

    #[test]
    fn a_checkbox() {
        let d = checkbox(1);
        let x = ff_data_xml(Some(&d), 0).unwrap();
        assert!(x.starts_with("<w:ffData><w:name w:val=\"a\"/><w:enabled/><w:calcOnExit w:val=\"0\"/>"));
        assert!(x.contains("<w:checkBox><w:sizeAuto/><w:default w:val=\"0\"/><w:checked w:val=\"0\"/></w:checkBox>"));
    }

    #[test]
    fn short_or_negative_data_is_nothing() {
        assert_eq!(ff_data_xml(None, 0), None);
        assert_eq!(ff_data_xml(Some(&[0; 10]), 0), None);
        assert_eq!(ff_data_xml(Some(&checkbox(1)), -1), None);
    }
}
