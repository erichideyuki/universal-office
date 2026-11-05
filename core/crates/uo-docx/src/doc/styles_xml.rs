//! The style sheet of the `.doc` to `styles.xml` (`styles-xml.ts`). The built-in styles
//! (`sti` below 4094) get the canonical English name the Word writes ("heading 1", "header",
//! "Normal Table"…) and the well-known `styleId`s ("Heading1", "Header", "TableNormal"…); the names
//! stored in the `.doc` are localized ("Título 1"). User styles keep their name and get a
//! `styleId` derived from it (only `[A-Za-z0-9-]`, as the Word does).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::fonts::DocFont;
use super::ooxml_props::{PPrOpts, PropCtx, brc_xml, lcid_tag, p_pr_inner, r_pr_inner, shd_xml, val_attr};
use super::props::{Tap, apply_tap};
use super::sprm::{prls, sgc_of};
use super::stsh::{DocStyle, ISTD_NIL, StyleResolver};
use super::xmlw::{AttrVal, Attrs, XML_DECL, attrs, el, ns};
use crate::text::Utf16;

// The names of the built-in styles by `sti` ([ECMA-376] 17.7.4.9, "Index within Built-in Styles"): the
// head (0-155, then the Word 2007+ names 156-171), six "Accent 1" table styles, four names, eight
// "Accent 1" table styles, the table styles of accents 2-6 (14 each) and seven last names.

const STI_HEAD: &[&str] = &[
    "Normal",
    "heading 1",
    "heading 2",
    "heading 3",
    "heading 4",
    "heading 5",
    "heading 6",
    "heading 7",
    "heading 8",
    "heading 9",
    "index 1",
    "index 2",
    "index 3",
    "index 4",
    "index 5",
    "index 6",
    "index 7",
    "index 8",
    "index 9",
    "toc 1",
    "toc 2",
    "toc 3",
    "toc 4",
    "toc 5",
    "toc 6",
    "toc 7",
    "toc 8",
    "toc 9",
    "Normal Indent",
    "footnote text",
    "annotation text",
    "header",
    "footer",
    "index heading",
    "caption",
    "table of figures",
    "envelope address",
    "envelope return",
    "footnote reference",
    "annotation reference",
    "line number",
    "page number",
    "endnote reference",
    "endnote text",
    "table of authorities",
    "macro",
    "toa heading",
    "List",
    "List Bullet",
    "List Number",
    "List 2",
    "List 3",
    "List 4",
    "List 5",
    "List Bullet 2",
    "List Bullet 3",
    "List Bullet 4",
    "List Bullet 5",
    "List Number 2",
    "List Number 3",
    "List Number 4",
    "List Number 5",
    "Title",
    "Closing",
    "Signature",
    "Default Paragraph Font",
    "Body Text",
    "Body Text Indent",
    "List Continue",
    "List Continue 2",
    "List Continue 3",
    "List Continue 4",
    "List Continue 5",
    "Message Header",
    "Subtitle",
    "Salutation",
    "Date",
    "Body Text First Indent",
    "Body Text First Indent 2",
    "Note Heading",
    "Body Text 2",
    "Body Text 3",
    "Body Text Indent 2",
    "Body Text Indent 3",
    "Block Text",
    "Hyperlink",
    "FollowedHyperlink",
    "Strong",
    "Emphasis",
    "Document Map",
    "Plain Text",
    "E-mail Signature",
    "HTML Top of Form",
    "HTML Bottom of Form",
    "Normal (Web)",
    "HTML Acronym",
    "HTML Address",
    "HTML Cite",
    "HTML Code",
    "HTML Definition",
    "HTML Keyboard",
    "HTML Preformatted",
    "HTML Sample",
    "HTML Typewriter",
    "HTML Variable",
    "Normal Table",
    "annotation subject",
    "No List",
    "Outline List 1",
    "Outline List 2",
    "Outline List 3",
    "Table Simple 1",
    "Table Simple 2",
    "Table Simple 3",
    "Table Classic 1",
    "Table Classic 2",
    "Table Classic 3",
    "Table Classic 4",
    "Table Colorful 1",
    "Table Colorful 2",
    "Table Colorful 3",
    "Table Columns 1",
    "Table Columns 2",
    "Table Columns 3",
    "Table Columns 4",
    "Table Columns 5",
    "Table Grid 1",
    "Table Grid 2",
    "Table Grid 3",
    "Table Grid 4",
    "Table Grid 5",
    "Table Grid 6",
    "Table Grid 7",
    "Table Grid 8",
    "Table List 1",
    "Table List 2",
    "Table List 3",
    "Table List 4",
    "Table List 5",
    "Table List 6",
    "Table List 7",
    "Table List 8",
    "Table 3D effects 1",
    "Table 3D effects 2",
    "Table 3D effects 3",
    "Table Contemporary",
    "Table Elegant",
    "Table Professional",
    "Table Subtle 1",
    "Table Subtle 2",
    "Table Web 1",
    "Table Web 2",
    "Table Web 3",
    "Balloon Text",
    "Table Grid",
    "Table Theme",
    "Placeholder Text",
    "No Spacing",
    "Light Shading",
    "Light List",
    "Light Grid",
    "Medium Shading 1",
    "Medium Shading 2",
    "Medium List 1",
    "Medium List 2",
    "Medium Grid 1",
    "Medium Grid 2",
    "Medium Grid 3",
    "Dark List",
    "Colorful Shading",
    "Colorful List",
    "Colorful Grid",
];

const STI_MID: &[&str] = &["Revision", "List Paragraph", "Quote", "Intense Quote"];

const STI_TAIL: &[&str] =
    &["Subtle Emphasis", "Intense Emphasis", "Subtle Reference", "Intense Reference", "Book Title", "Bibliography", "TOC Heading"];

const ACCENT_BASE: &[&str] = &[
    "Light Shading",
    "Light List",
    "Light Grid",
    "Medium Shading 1",
    "Medium Shading 2",
    "Medium List 1",
    "Medium List 2",
    "Medium Grid 1",
    "Medium Grid 2",
    "Medium Grid 3",
    "Dark List",
    "Colorful Shading",
    "Colorful List",
    "Colorful Grid",
];

/// One run of `STI_NAMES`: a list of names or the "Accent n" variants of `ACCENT_BASE[from..to]`.
enum Seg {
    Names(&'static [&'static str]),
    Accent { n: u8, from: usize, to: usize },
}

const SEGMENTS: &[Seg] = &[
    Seg::Names(STI_HEAD),
    Seg::Accent { n: 1, from: 0, to: 6 },
    Seg::Names(STI_MID),
    Seg::Accent { n: 1, from: 6, to: 14 },
    Seg::Accent { n: 2, from: 0, to: 14 },
    Seg::Accent { n: 3, from: 0, to: 14 },
    Seg::Accent { n: 4, from: 0, to: 14 },
    Seg::Accent { n: 5, from: 0, to: 14 },
    Seg::Accent { n: 6, from: 0, to: 14 },
    Seg::Names(STI_TAIL),
];

/// `STI_NAMES.length`.
pub const STI_NAMES_LEN: usize = 172 + 6 + 4 + 8 + 70 + 7;

/// `STI_NAMES[sti]`: the canonical name of a built-in style.
#[must_use]
pub fn sti_name(sti: usize) -> Option<Cow<'static, str>> {
    let mut i = sti;
    for seg in SEGMENTS {
        match seg {
            Seg::Names(list) => {
                if let Some(n) = list.get(i) {
                    return Some(Cow::Borrowed(n));
                }
                i -= list.len();
            }
            Seg::Accent { n, from, to } => {
                if i < to - from {
                    return Some(Cow::Owned(format!("{} Accent {n}", ACCENT_BASE[from + i])));
                }
                i -= to - from;
            }
        }
    }
    None
}

/// `STI_EXTRA`: the sparse `sti`s after 266 (Word 2013+), checked in the Word.
fn sti_extra(sti: u16) -> Option<&'static str> {
    (sti == 374).then_some("Unresolved Mention")
}

/// `SPECIAL_IDS`: the `styleId`s of the Word (English) that do not come from the name by dropping spaces.
fn special_id(name: &str) -> Option<&'static str> {
    Some(match name {
        "annotation text" => "CommentText",
        "annotation reference" => "CommentReference",
        "annotation subject" => "CommentSubject",
        "Normal Table" => "TableNormal",
        "table of figures" => "TableofFigures",
        "table of authorities" => "TableofAuthorities",
        "toa heading" => "TOAHeading",
        "Normal (Web)" => "NormalWeb",
        "E-mail Signature" => "E-mailSignature",
        "macro" => "MacroText",
        "List" => "List",
        "line number" => "LineNumber",
        _ => return None,
    })
}

/// `englishId`: the `styleId` of an English built-in name.
fn english_id(name: &str) -> String {
    if let Some(s) = special_id(name) {
        return s.to_owned();
    }
    // /^toc (\d)$/
    if let Some(d) = name.strip_prefix("toc ")
        && d.len() == 1
        && d.as_bytes()[0].is_ascii_digit()
    {
        return format!("TOC{d}");
    }
    // split(/\s+/).map(w => w ? w[0].toUpperCase() + w.slice(1) : '').join('').replace(/[^A-Za-z0-9-]/g, ''):
    // the names are ASCII, so the JS white space is the ASCII space here
    let mut out = String::new();
    for w in name.split(' ').filter(|w| !w.is_empty()) {
        let mut cs = w.chars();
        if let Some(c) = cs.next() {
            out.extend(c.to_uppercase());
            out.push_str(cs.as_str());
        }
    }
    out.retain(|c| c.is_ascii_alphanumeric() || c == '-');
    out
}

/// `styleName`: the name written in the `styles.xml`.
#[must_use]
pub fn style_name(st: &DocStyle<'_>) -> Utf16 {
    if let Some(n) = sti_name(usize::from(st.sti)) {
        return Utf16::from(n.as_ref());
    }
    if let Some(n) = sti_extra(st.sti) {
        return Utf16::from(n);
    }
    if st.name.is_empty() { Utf16::from(format!("Estilo {}", st.istd)) } else { st.name.clone() }
}

/// `styleIdMap`: the `styleId` of every style by `istd`.
#[derive(Debug, Clone, Default)]
pub struct StyleIds {
    ids: HashMap<u16, String>,
}

impl StyleIds {
    /// `ids.get(istd)`.
    #[must_use]
    pub fn get(&self, istd: u16) -> Option<&str> {
        self.ids.get(&istd).map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The ids in no particular order.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.ids.values().map(String::as_str)
    }
}

/// `styleIdMap(styles)`. The TS probes `base`, `base2`, `base3`… from the start for every style with the same
/// base, which is quadratic in the number of styles that share one; the ids that are taken stay taken, so the
/// port resumes each base at the last suffix it used and gives the same ids.
#[must_use]
pub fn style_id_map(styles: &[Option<DocStyle<'_>>]) -> StyleIds {
    let mut ids = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    // built-in styles first (reserved ids), then by istd
    let mut order: Vec<&DocStyle<'_>> = styles.iter().flatten().collect();
    order.sort_by_key(|s| (usize::from(s.sti) >= STI_NAMES_LEN, s.istd));
    let mut resume: HashMap<String, u64> = HashMap::new();
    let mut empty_resume: u64 = 0;
    for st in order {
        let builtin = usize::from(st.sti) < STI_NAMES_LEN || sti_extra(st.sti).is_some();
        let base: String = if builtin {
            english_id(&style_name(st).to_string_lossy())
        } else {
            // (st.name || '').replace(/[^A-Za-z0-9-]/g, ''): over UTF-16 units
            st.name
                .units()
                .iter()
                .filter(|&&c| c < 0x80 && ((c as u8).is_ascii_alphanumeric() || c == u16::from(b'-')))
                .map(|&c| char::from(c as u8))
                .collect()
        };
        let id;
        if base.is_empty() {
            // a name without ASCII characters (or empty): the Word uses "a", "a0", "a1"…
            let candidate = |j: u64| if j == 0 { "a".to_owned() } else { format!("a{}", j - 1) };
            let mut j = empty_resume;
            while used.contains(&candidate(j).to_lowercase()) {
                j += 1;
            }
            empty_resume = j;
            id = candidate(j);
        } else {
            let lc = base.to_lowercase();
            let mut k = resume.get(&lc).copied().unwrap_or(1);
            let mut cand = if k == 1 { base.clone() } else { format!("{base}{k}") };
            while used.contains(&cand.to_lowercase()) {
                k += 1;
                cand = format!("{base}{k}");
            }
            resume.insert(lc, k);
            id = cand;
        }
        used.insert(id.to_lowercase());
        ids.insert(st.istd, id);
    }
    StyleIds { ids }
}

/// `tableStylePr`: the TAPX of a table style as `w:tblPr` content (indent, borders, margins).
fn table_style_pr(st: &DocStyle<'_>) -> String {
    let Some(tapx) = st.tapx else { return String::new() };
    let mut tap = Tap::new();
    for p in prls(tapx) {
        if sgc_of(p.sprm) == 5 {
            apply_tap(&mut tap, &p);
        }
    }
    let mut x = String::new();
    let ind = match tap.width_indent {
        Some(w) if w.fts == 3 => Some(w.w),
        _ => tap.dxa_left,
    };
    let dxa = |name: &str, v: Option<i16>| -> String {
        el(name, &Attrs::of(&[("w:w", v.map(|v| AttrVal::Int(i64::from(v)))), ("w:type", Some(AttrVal::Str("dxa")))]), "")
    };
    if let Some(i) = ind {
        x += &dxa("w:tblInd", Some(i));
    }
    if tap.borders_set {
        let b = &tap.borders;
        let sides = [
            ("top", &b.top),
            ("left", &b.left),
            ("bottom", &b.bottom),
            ("right", &b.right),
            ("insideH", &b.inside_h),
            ("insideV", &b.inside_v),
        ];
        let inner: String = sides.iter().map(|(n, b)| brc_xml(&format!("w:{n}"), b.as_ref())).collect();
        x += &format!("<w:tblBorders>{inner}</w:tblBorders>");
    }
    if tap.shd_table.is_some() {
        x += &shd_xml(tap.shd_table.as_ref());
    }
    let p = tap.padding;
    if p.iter().any(Option::is_some) {
        let mut m = String::new();
        for (name, v) in [("w:top", p[0]), ("w:left", p[1]), ("w:bottom", p[2]), ("w:right", p[3])] {
            if v.is_some() {
                m += &dxa(name, v);
            }
        }
        x += &format!("<w:tblCellMar>{m}</w:tblCellMar>");
    }
    x
}

/// `styleXml`: one `w:style`.
fn style_xml(
    styles: &mut StyleResolver<'_>,
    st: &DocStyle<'_>,
    ids: &StyleIds,
    ctx: &PropCtx<'_, '_>,
    num_id_of: &dyn Fn(u32) -> i64,
) -> String {
    let kind = ["", "paragraph", "character", "table", "numbering"].get(usize::from(st.stk)).copied().unwrap_or("paragraph");
    let is_default =
        (st.stk == 1 && st.sti == 0) || (st.stk == 2 && st.sti == 65) || (st.stk == 3 && st.sti == 105) || (st.stk == 4 && st.sti == 107);
    let builtin = usize::from(st.sti) < STI_NAMES_LEN;
    // without a stored name (a user style) the Word omits w:name
    let mut x = String::new();
    if !(!builtin && st.name.is_empty()) {
        let name = style_name(st);
        x += &el("w:name", &val_attr(Some(AttrVal::Units(name.units()))), "");
    }
    if let Some(a) = &st.aliases
        && !a.is_empty()
    {
        x += &el("w:aliases", &val_attr(Some(AttrVal::Units(a.units()))), "");
    }
    if st.istd_base != ISTD_NIL
        && let Some(id) = ids.get(st.istd_base)
        && st.istd_base != st.istd
    {
        x += &el("w:basedOn", &val_attr(Some(AttrVal::Str(id))), "");
    }
    if st.stk == 1
        && st.istd_next != st.istd
        && st.istd_next != ISTD_NIL
        && let Some(id) = ids.get(st.istd_next)
    {
        x += &el("w:next", &val_attr(Some(AttrVal::Str(id))), "");
    }
    if let Some(l) = st.istd_link
        && let Some(id) = ids.get(l)
    {
        x += &el("w:link", &val_attr(Some(AttrVal::Str(id))), "");
    }
    for (on, tag) in [
        (st.auto_redef, "<w:autoRedefine/>"),
        (st.hidden, "<w:hidden/>"),
        (st.semi_hidden, "<w:semiHidden/>"),
        (st.unhide_when_used, "<w:unhideWhenUsed/>"),
        (st.q_format, "<w:qFormat/>"),
        (st.locked, "<w:locked/>"),
    ] {
        if on {
            x += tag;
        }
    }
    if st.stk == 1 || st.stk == 3 || st.stk == 4 {
        let res = styles.resolve(st.istd);
        let parent =
            if st.istd_base != ISTD_NIL && styles.style(st.istd_base).is_some() { Some(styles.resolve(st.istd_base)) } else { None };
        let base_tabs = parent.as_ref().and_then(|p| p.pap.tabs.as_deref());
        let opts = PPrOpts { with_style: false, base_tabs, num_id: Some(num_id_of), ..PPrOpts::default() };
        let ppr = p_pr_inner(&res.pap, &res.own_pap, ctx, &opts);
        if !ppr.is_empty() {
            x += &format!("<w:pPr>{ppr}</w:pPr>");
        }
        if st.stk != 4 {
            let rpr = r_pr_inner(&res.chp, &res.own_chp, ctx, false);
            if !rpr.is_empty() {
                x += &format!("<w:rPr>{rpr}</w:rPr>");
            }
        }
        if st.stk == 3 {
            let tbl = table_style_pr(st);
            if !tbl.is_empty() {
                x += &format!("<w:tblPr>{tbl}</w:tblPr>");
            }
        }
    } else if st.stk == 2 {
        let (chp, own) = styles.resolve_char(st.istd);
        let rpr = r_pr_inner(&chp, &own, ctx, false);
        if !rpr.is_empty() {
            x += &format!("<w:rPr>{rpr}</w:rPr>");
        }
    }
    let custom = !builtin && sti_extra(st.sti).is_none();
    el(
        "w:style",
        &Attrs::of(&[
            ("w:type", Some(AttrVal::Str(kind))),
            ("w:default", is_default.then_some(AttrVal::Str("1"))),
            ("w:customStyle", custom.then_some(AttrVal::Str("1"))),
            ("w:styleId", ids.get(st.istd).map(AttrVal::Str)),
        ]),
        &x,
    )
}

/// `buildStyles`: the `styles.xml` of the document. The first thing it does is resolve style 0 (the
/// memo of the resolver depends on the order of the calls: the body writer's own resolutions come first
/// in the TS).
pub fn build_styles(
    styles: &mut StyleResolver<'_>,
    fonts: &[DocFont],
    ctx: &PropCtx<'_, '_>,
    ids: &StyleIds,
    num_id_of: &dyn Fn(u32) -> i64,
) -> String {
    let f = styles.sheet.stshif;
    let font = |ftc: u16| fonts.get(usize::from(ftc)).map(|f| AttrVal::Units(f.name.units()));
    // docDefaults: the default fonts of the Stshif; the language of the Normal style (the Word moves it here)
    let normal = styles.resolve(0).chp.clone();
    let lang = el(
        "w:lang",
        &Attrs::of(&[
            ("w:val", Some(AttrVal::Str(lcid_tag(normal.lid).unwrap_or("pt-BR")))),
            ("w:eastAsia", Some(AttrVal::Str(lcid_tag(normal.lid_fe).or_else(|| lcid_tag(normal.lid)).unwrap_or("pt-BR")))),
            (
                "w:bidi",
                Some(AttrVal::Str(
                    lcid_tag(Some(match normal.lid_bi {
                        Some(b) if b != 0 && b != 0x400 => b,
                        _ => 0x0401,
                    }))
                    .unwrap_or("ar-SA"),
                )),
            ),
        ]),
        "",
    );
    let r_fonts = el(
        "w:rFonts",
        &Attrs::of(&[
            ("w:ascii", font(f.ftc_asci)),
            ("w:eastAsia", font(f.ftc_fe)),
            ("w:hAnsi", font(f.ftc_other)),
            ("w:cs", font(f.ftc_bi)),
        ]),
        "",
    );
    let mut x = format!("<w:docDefaults><w:rPrDefault><w:rPr>{r_fonts}{lang}</w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults>");
    let all: Vec<DocStyle<'_>> = styles.sheet.styles.iter().flatten().cloned().collect();
    for st in &all {
        x += &style_xml(styles, st, ids, ctx, num_id_of);
    }
    // old .doc files (Word 97/2000) lack these default styles; the Word creates them when saving
    let has = |sti: u16, stk: u8| all.iter().any(|s| s.sti == sti && s.stk == stk);
    let mut used: HashSet<String> = ids.values().map(str::to_lowercase).collect();
    let mut free_id = |base: &str| -> String {
        let mut id = base.to_owned();
        let mut k = 2;
        while used.contains(&id.to_lowercase()) {
            id = format!("{base}{k}");
            k += 1;
        }
        used.insert(id.to_lowercase());
        id
    };
    if !has(65, 2) {
        x += &format!(
            "<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"{}\"><w:name w:val=\"Default Paragraph Font\"/><w:semiHidden/></w:style>",
            free_id("DefaultParagraphFont")
        );
    }
    if !has(105, 3) {
        x += &format!(
            "<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"{}\"><w:name w:val=\"Normal Table\"/><w:semiHidden/>\
             <w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/>\
             <w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
            free_id("TableNormal")
        );
    }
    if !has(107, 4) {
        x += &format!(
            "<w:style w:type=\"numbering\" w:default=\"1\" w:styleId=\"{}\"><w:name w:val=\"No List\"/><w:semiHidden/></w:style>",
            free_id("NoList")
        );
    }
    let ns_attrs = attrs(&Attrs::of(&[("xmlns:w", Some(AttrVal::Str(ns::W))), ("xmlns:r", Some(AttrVal::Str(ns::R)))]));
    format!("{XML_DECL}<w:styles{ns_attrs}>{x}</w:styles>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(istd: u16, sti: u16, name: &str) -> DocStyle<'static> {
        DocStyle {
            istd,
            sti,
            stk: 1,
            istd_base: ISTD_NIL,
            istd_next: istd,
            istd_link: None,
            name: Utf16::from(name),
            aliases: None,
            auto_redef: false,
            hidden: false,
            semi_hidden: false,
            locked: false,
            unhide_when_used: false,
            q_format: false,
            priority: None,
            papx: None,
            chpx: None,
            tapx: None,
        }
    }

    #[test]
    fn the_sti_table_has_the_length_of_the_ts_array() {
        assert_eq!(sti_name(0).as_deref(), Some("Normal"));
        assert_eq!(sti_name(1).as_deref(), Some("heading 1"));
        assert_eq!(sti_name(155).as_deref(), Some("Table Theme"));
        assert_eq!(sti_name(156).as_deref(), Some("Placeholder Text"));
        assert_eq!(sti_name(171).as_deref(), Some("Colorful Grid"));
        assert_eq!(sti_name(172).as_deref(), Some("Light Shading Accent 1"));
        assert_eq!(sti_name(178).as_deref(), Some("Revision"));
        assert_eq!(sti_name(182).as_deref(), Some("Medium List 2 Accent 1"));
        assert_eq!(sti_name(266).as_deref(), Some("TOC Heading"));
        assert_eq!(sti_name(STI_NAMES_LEN), None);
        assert_eq!(sti_name(STI_NAMES_LEN - 1).as_deref(), Some("TOC Heading"));
        assert_eq!(sti_name(190).as_deref(), Some("Light Shading Accent 2"));
    }

    #[test]
    fn english_ids_and_the_style_id_map() {
        assert_eq!(english_id("heading 1"), "Heading1");
        assert_eq!(english_id("toc 3"), "TOC3");
        assert_eq!(english_id("annotation text"), "CommentText");
        assert_eq!(english_id("Table 3D effects 1"), "Table3DEffects1");
        let styles = vec![
            Some(style(0, 0, "Normal")),
            Some(style(1, 1, "Título 1")),
            Some(style(2, 4000, "Meu Estilo")),
            Some(style(3, 4000, "Meu Estilo")),
            Some(style(4, 4000, "日本語")),
            Some(style(5, 4000, "")),
            None,
            Some(style(7, 4000, "meu estilo")),
            Some(style(8, 4000, "a")),
        ];
        let ids = style_id_map(&styles);
        let id = |i: u16| ids.get(i).unwrap().to_owned();
        assert_eq!((id(0), id(1)), ("Normal".to_owned(), "Heading1".to_owned()));
        assert_eq!((id(2), id(3), id(7)), ("MeuEstilo".to_owned(), "MeuEstilo2".to_owned(), "meuestilo3".to_owned()));
        // names without ASCII characters: "a", "a0", "a1"…; a user style called "a" then meets the id "a" and gets "a2"
        assert_eq!((id(4), id(5)), ("a".to_owned(), "a0".to_owned()));
        assert_eq!(id(8), "a2");
        assert_eq!(ids.len(), 8);
        assert_eq!(style_name(&styles[1].clone().unwrap()).to_string_lossy(), "heading 1");
        assert_eq!(style_name(&style(9, 4000, "")).to_string_lossy(), "Estilo 9");
    }

    #[test]
    fn many_styles_with_one_name_stay_linear() {
        let styles: Vec<Option<DocStyle<'static>>> = (0..20_000u16).map(|i| Some(style(i, 4000, "x"))).collect();
        let start = std::time::Instant::now();
        let ids = style_id_map(&styles);
        assert_eq!((ids.get(0), ids.get(1), ids.get(19_999)), (Some("x"), Some("x2"), Some("x20000")));
        assert!(start.elapsed().as_secs() < 2);
    }
}
