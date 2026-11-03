//! The theme part (`a:theme`: `a:clrScheme`, `a:fontScheme`, `a:fmtScheme`), the same part in
//! docx, pptx and xlsx (uo-docx plan G12). [`read`] gives an app-neutral, as-written view of it;
//! each app builds its own theme from the view with its rules (Write: `readTheme` of docx-core
//! `ooxml/parts.ts`, in uo-docx; Slides: `slides-core/src/theme.ts`; Sheet: `sheet-core/src/theme.ts`
//! with its integer HLS tint, which stays in Sheet).
//!
//! The lookups follow docx-core's `find(root, name)`: the first element with that name in a
//! depth-first, document-order walk below the root (the root itself excluded). The walk is
//! iterative (the uo-opc tree is bounded by its depth limit, the TS recursion is not ported).
//! Style list entries are returned as element ids of the parsed part, because what an entry
//! means is app-specific (Write reads them with its `readSpPr`).

use uo_opc::xml::{Document, Element, ElementId};

use crate::dml::{DmlColor, read_color};

/// One font set (`a:majorFont` / `a:minorFont`), as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontSet {
    /// `typeface` of the first `a:latin` child (`None`: no element or no attribute)
    pub latin: Option<Box<str>>,
    /// `typeface` of the first `a:ea` child
    pub ea: Option<Box<str>>,
    /// `typeface` of the first `a:cs` child
    pub cs: Option<Box<str>>,
    /// every `a:font` child in document order
    pub fonts: Vec<ScriptFont>,
}

/// One `a:font` of a font set, as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptFont {
    pub script: Option<Box<str>>,
    pub typeface: Option<Box<str>>,
}

/// One child of `a:clrScheme` (`a:dk1`, `a:accent1`...), as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemeEntry {
    /// the element name as the XML reader gives it (`a:dk1`)
    pub name: Box<str>,
    /// every child element read as a colour, in document order (normally one `a:srgbClr` or
    /// `a:sysClr`)
    pub colors: Vec<DmlColor>,
}

impl SchemeEntry {
    /// The first child colour with this element name (`firstChild(c, name)`).
    pub fn first(&self, name: &str) -> Option<&DmlColor> {
        self.colors.iter().find(|c| &*c.name == name)
    }
}

/// The as-written view of a theme part.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemePart {
    /// `a:majorFont` of the first `a:fontScheme` (`None` without either element)
    pub major: Option<FontSet>,
    /// `a:minorFont` of the first `a:fontScheme`
    pub minor: Option<FontSet>,
    /// the children of the first `a:clrScheme` (`None` without one)
    pub color_scheme: Option<Vec<SchemeEntry>>,
    /// `w` of every `a:ln` child of the first `a:lnStyleLst` (`None` without one)
    pub line_widths: Option<Vec<Option<Box<str>>>>,
    /// every child element of the first `a:fillStyleLst`
    pub fill_styles: Option<Vec<ElementId>>,
    /// every child element of the first `a:bgFillStyleLst`
    pub bg_fill_styles: Option<Vec<ElementId>>,
    /// every `a:effectStyle` child of the first `a:effectStyleLst`
    pub effect_styles: Option<Vec<ElementId>>,
}

/// `find(root, name)`: the first element named `name` below `root`, depth first in document
/// order, `root` excluded.
pub fn find<'d>(root: Element<'d>, name: &str) -> Option<Element<'d>> {
    root.descend(name)
}

fn font_set(el: Element<'_>) -> FontSet {
    let typeface = |n: &str| el.first_child(n).and_then(|c| c.attr("typeface")).map(Box::from);
    FontSet {
        latin: typeface("a:latin"),
        ea: typeface("a:ea"),
        cs: typeface("a:cs"),
        fonts: el
            .children_named("a:font")
            .map(|f| ScriptFont { script: f.attr("script").map(Box::from), typeface: f.attr("typeface").map(Box::from) })
            .collect(),
    }
}

/// Reads the theme view from a parsed theme part (docx-core prefixes: `a:`).
pub fn read(doc: &Document) -> ThemePart {
    let root = doc.root();
    let font_scheme = find(root, "a:fontScheme");
    let ids = |list: Option<Element<'_>>, only: Option<&str>| {
        list.map(|l| l.child_elements().filter(|c| only.is_none_or(|n| c.name() == n)).map(Element::id).collect::<Vec<_>>())
    };
    ThemePart {
        major: font_scheme.and_then(|f| f.first_child("a:majorFont")).map(font_set),
        minor: font_scheme.and_then(|f| f.first_child("a:minorFont")).map(font_set),
        color_scheme: find(root, "a:clrScheme").map(|clr| {
            clr.child_elements()
                .map(|c| SchemeEntry { name: c.name().into(), colors: c.child_elements().map(|k| read_color(k, "")).collect() })
                .collect()
        }),
        line_widths: find(root, "a:lnStyleLst").map(|l| l.children_named("a:ln").map(|ln| ln.attr("w").map(Box::from)).collect()),
        fill_styles: ids(find(root, "a:fillStyleLst"), None),
        bg_fill_styles: ids(find(root, "a:bgFillStyleLst"), None),
        effect_styles: ids(find(root, "a:effectStyleLst"), Some("a:effectStyle")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_opc::xml::{ParseOptions, parse};

    #[test]
    fn reads_the_view() {
        let xml = r#"<a:theme><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:accent1><a:srgbClr val="4472c4"/></a:accent1></a:clrScheme>
<a:fontScheme><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:font script="Jpan" typeface="游ゴシック Light"/></a:majorFont></a:fontScheme>
<a:fmtScheme><a:fillStyleLst><a:solidFill/><a:gradFill/></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"/><a:ln/></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle/><a:x/><a:effectStyle/></a:effectStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;
        let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
        let t = read(&doc);
        let major = t.major.unwrap();
        assert_eq!(major.latin.as_deref(), Some("Calibri Light"));
        assert_eq!(major.ea.as_deref(), Some(""));
        assert_eq!(major.fonts.len(), 1);
        assert!(t.minor.is_none());
        let cs = t.color_scheme.unwrap();
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].first("a:sysClr").and_then(|c| c.last_clr.as_deref()), Some("000000"));
        assert_eq!(cs[1].first("a:srgbClr").and_then(|c| c.val.as_deref()), Some("4472c4"));
        assert_eq!(t.line_widths.unwrap(), vec![Some("6350".into()), None]);
        assert_eq!(t.fill_styles.unwrap().len(), 2);
        assert_eq!(t.effect_styles.unwrap().len(), 2);
        assert!(t.bg_fill_styles.is_none());
    }
}
