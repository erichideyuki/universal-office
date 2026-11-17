//! The helpers of `shared/charts/src/xml.ts` over the host's tree (`uo_opc::xml::Element`): the engine
//! never parses XML, it reads the tree the host hands (`childElements`, `firstChild`, `textContent`,
//! `descend`, `valNum`, `valBool`, `valStr`). `canonicalNs` is not here: the host's parse renames the
//! prefixes (`uo-opc` `CanonicalPrefixes` with [`CHART_PREFIXES`]).

use uo_jsnum::parse::parse_float;
use uo_opc::xml::Element;

/// `CANONICAL` of xml.ts: namespace URI → the prefix the engine matches names by (`c:`, `a:`, `r:`,
/// `xdr:`, `cdr:`, `mc:`, `c14:`, `c15:`, `c16:`, `c16r3:`, `cx:`…`cx8:`, `a14:`). The host parses the
/// chart part with `uo_opc::xml::CanonicalPrefixes { table: CHART_PREFIXES, default_namespace: true }`
/// (generators with a default namespace, own prefixes, or OOXML Strict). The Sheet does; the Slides host
/// does not (decision CH-4: the TS Slides bridge never canonicalized).
pub const CHART_PREFIXES: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/drawingml/2006/chart", "c"),
    ("http://purl.oclc.org/ooxml/drawingml/chart", "c"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://purl.oclc.org/ooxml/drawingml/main", "a"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://purl.oclc.org/ooxml/officeDocument/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing", "xdr"),
    ("http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing", "xdr"),
    ("http://schemas.openxmlformats.org/drawingml/2006/chartDrawing", "cdr"),
    ("http://purl.oclc.org/ooxml/drawingml/chartDrawing", "cdr"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/drawing/2007/8/2/chart", "c14"),
    ("http://schemas.microsoft.com/office/drawing/2012/chart", "c15"),
    ("http://schemas.microsoft.com/office/drawing/2014/chart", "c16"),
    ("http://schemas.microsoft.com/office/drawing/2017/03/chart", "c16r3"),
    ("http://schemas.microsoft.com/office/drawing/2014/chartex", "cx"),
    ("http://schemas.microsoft.com/office/drawing/2015/9/8/chartex", "cx1"),
    ("http://schemas.microsoft.com/office/drawing/2015/10/21/chartex", "cx2"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/9/chartex", "cx3"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/10/chartex", "cx4"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/11/chartex", "cx5"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/12/chartex", "cx6"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/13/chartex", "cx7"),
    ("http://schemas.microsoft.com/office/drawing/2016/5/14/chartex", "cx8"),
    ("http://schemas.microsoft.com/office/drawing/2010/main", "a14"),
];

/// `firstChild(el, name)` over an optional element.
pub(crate) fn first<'d>(el: Option<Element<'d>>, name: &str) -> Option<Element<'d>> {
    el?.first_child(name)
}

/// `descend(el, name)` over an optional element.
pub(crate) fn descend<'d>(el: Option<Element<'d>>, name: &str) -> Option<Element<'d>> {
    el?.descend(name)
}

/// `valNum(el, name)`: the `val` of a child through `parseFloat`, finite numbers only.
pub(crate) fn val_num(el: Option<Element<'_>>, name: &str) -> Option<f64> {
    let v = first(el, name)?.attr("val")?;
    let n = parse_float(v);
    n.is_finite().then_some(n)
}

/// `valStr(el, name)`: the `val` attribute of a child.
pub(crate) fn val_str<'d>(el: Option<Element<'d>>, name: &str) -> Option<&'d str> {
    first(el, name)?.attr("val")
}

/// `textContent(el)`.
pub(crate) fn text(el: Element<'_>) -> String {
    el.text_content()
}

/// JS truthiness of an optional string attribute (`if (a.attrs['x'])`): present and not empty.
pub(crate) fn truthy(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}
