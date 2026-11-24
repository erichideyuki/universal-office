//! The theme of the `.xls` → `xl/theme/theme1.xml` (`theme-xml.ts`). Excel 2007+ writes the theme in the THEME record:
//! "default" (no data = the Office 2007 theme) or custom (the zip package of the themeManager, from which `theme1.xml`
//! comes). The folders of Excel 97–2003 have no theme — current Excel opens them with the Office 2007 theme (the cell
//! colours of the BIFF are all indexed/RGB, the theme only weighs on the theme colours of the XFEXT).

use uo_opc::flate;
use uo_opc::zip::{self, ZipEntry};

use super::xmlw::XML_HEAD;

fn solid(inner: &str) -> String {
    format!("<a:solidFill><a:schemeClr val=\"phClr\">{inner}</a:schemeClr></a:solidFill>")
}

/// The Office 2007 theme (the default of the `.xls` folders).
pub fn office_2007_theme() -> String {
    let srgb = |v: &str| format!("<a:srgbClr val=\"{v}\"/>");
    let slots: [(&str, String); 12] = [
        ("dk1", "<a:sysClr val=\"windowText\" lastClr=\"000000\"/>".to_owned()),
        ("lt1", "<a:sysClr val=\"window\" lastClr=\"FFFFFF\"/>".to_owned()),
        ("dk2", srgb("1F497D")),
        ("lt2", srgb("EEECE1")),
        ("accent1", srgb("4F81BD")),
        ("accent2", srgb("C0504D")),
        ("accent3", srgb("9BBB59")),
        ("accent4", srgb("8064A2")),
        ("accent5", srgb("4BACC6")),
        ("accent6", srgb("F79646")),
        ("hlink", srgb("0000FF")),
        ("folHlink", srgb("800080")),
    ];
    let mut x = String::new();
    x.push_str(XML_HEAD);
    x.push_str("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\"><a:themeElements>");
    x.push_str("<a:clrScheme name=\"Office\">");
    for (n, c) in &slots {
        x.push_str(&format!("<a:{n}>{c}</a:{n}>"));
    }
    x.push_str("</a:clrScheme>");
    x.push_str("<a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"Cambria\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>");
    x.push_str("<a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>");
    x.push_str("<a:fmtScheme name=\"Office\"><a:fillStyleLst>");
    x.push_str(&solid(""));
    x.push_str(&solid("<a:tint val=\"50000\"/>"));
    x.push_str(&solid("<a:shade val=\"80000\"/>"));
    x.push_str("</a:fillStyleLst><a:lnStyleLst>");
    for w in [9525, 25400, 38100] {
        x.push_str(&format!("<a:ln w=\"{w}\">{}</a:ln>", solid("")));
    }
    x.push_str("</a:lnStyleLst><a:effectStyleLst>");
    for _ in 0..3 {
        x.push_str("<a:effectStyle><a:effectLst/></a:effectStyle>");
    }
    x.push_str("</a:effectStyleLst><a:bgFillStyleLst>");
    x.push_str(&solid(""));
    x.push_str(&solid("<a:tint val=\"40000\"/>"));
    x.push_str(&solid("<a:shade val=\"80000\"/>"));
    x.push_str("</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>");
    x
}

/// `unzipSync(data, { filter })` as the converter uses it: the entries that pass the filter, inflated (a damaged one
/// fails the whole call, as the TS throws). A repeated name keeps its first position with the last entry.
pub fn unzip_filtered(
    data: &[u8],
    max_entries: u32,
    cap: u64,
    filter: &mut dyn FnMut(&ZipEntry) -> bool,
) -> Option<Vec<(String, Vec<u8>)>> {
    let entries = zip::read_filtered(data, max_entries, filter).ok()?;
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let bytes =
            if e.method == 0 { e.compressed(data).to_vec() } else { flate::inflate_raw(e.compressed(data), Some(e.size), cap).ok()? };
        out.push((e.name, bytes));
    }
    Some(out)
}

/// `/theme\/theme\/theme\d*\.xml$/i`.
fn is_theme_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let Some(stem) = l.strip_suffix(".xml") else { return false };
    stem.trim_end_matches(|c: char| c.is_ascii_digit()).ends_with("theme/theme/theme")
}

/// `themeXml`: `theme1.xml` from the THEME record (the zip of the themeManager) or the Office 2007 default.
pub fn theme_xml(theme_zip: Option<&[u8]>) -> String {
    if let Some(z) = theme_zip.filter(|z| z.len() > 4 && z[0] == 0x50 && z[1] == 0x4b) {
        let files = unzip_filtered(z, 4096, 4 << 20, &mut |e| is_theme_name(&e.name) && e.size < 4 * 1024 * 1024);
        if let Some((_, bytes)) = files.as_ref().and_then(|f| f.first()) {
            let xml = uo_jsnum::text::decode_utf8(bytes);
            if xml.contains("clrScheme") {
                return xml.into_owned();
            }
        }
    }
    office_2007_theme()
}
