//! D2b-F4: the cover page templates, the pure half: editor `cover-templates.ts` (`COVER_TEMPLATES`,
//! `coverPlaceholders`, `COVER_NS`, `coverXml` and its helpers). The templates are CODE (page
//! geometry, theme accents, document properties, language and random ids go into the XML); the
//! Node ops capture proves the bytes (`js/capture-d2b-f4-ops.mjs`).

use uo_jsnum::conv::truthy;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{max, round};

use super::PageLang;
use crate::write::js::esc_text as esc;

/// `CoverFields`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverFields {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub date: String,
    pub company: String,
    pub summary: String,
}

/// The page and margins of the first section, in pt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverPage {
    pub w: f64,
    pub h: f64,
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// `CoverEnv`. `drawing_id` is `() => 1 + floor(random * 0x7ffffffe)`.
pub struct CoverEnv<'a> {
    pub lang: PageLang,
    pub page: CoverPage,
    pub accent1: String,
    pub accent2: String,
    pub fields: CoverFields,
    pub drawing_id: &'a mut dyn FnMut() -> f64,
}

/// `CoverTemplate`: the id and the name per language (`en`, `pt`, `es`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverTemplate {
    pub id: &'static str,
    pub name: [&'static str; 3],
}

impl CoverTemplate {
    /// `name[lang]`.
    #[must_use]
    pub fn name_for(&self, lang: PageLang) -> &'static str {
        match lang {
            PageLang::En => self.name[0],
            PageLang::Pt => self.name[1],
            PageLang::Es => self.name[2],
        }
    }
}

/// `COVER_TEMPLATES`.
pub const COVER_TEMPLATES: [CoverTemplate; 8] = [
    CoverTemplate { id: "band", name: ["Band", "Faixa", "Banda"] },
    CoverTemplate { id: "sidebar", name: ["Sidebar", "Lateral", "Lateral"] },
    CoverTemplate { id: "frame", name: ["Frame", "Moldura", "Marco"] },
    CoverTemplate { id: "block", name: ["Block", "Bloco", "Bloque"] },
    CoverTemplate { id: "minimal", name: ["Minimal", "Minimalista", "Minimalista"] },
    CoverTemplate { id: "base", name: ["Base", "Base", "Base"] },
    CoverTemplate { id: "duotone", name: ["Duotone", "Duas Cores", "Bitono"] },
    CoverTemplate { id: "diagonal", name: ["Diagonal", "Diagonal", "Diagonal"] },
];

/// `coverPlaceholders(lang)`.
#[must_use]
pub fn cover_placeholders(lang: PageLang) -> CoverFields {
    let f = |title: &str, subtitle: &str, author: &str, date: &str, company: &str, summary: &str| CoverFields {
        title: title.to_owned(),
        subtitle: subtitle.to_owned(),
        author: author.to_owned(),
        date: date.to_owned(),
        company: company.to_owned(),
        summary: summary.to_owned(),
    };
    match lang {
        PageLang::Pt => f(
            "[Título do documento]",
            "[Subtítulo do documento]",
            "[Nome do autor]",
            "[Data]",
            "[Nome da empresa]",
            "[Resumo do documento: um parágrafo curto sobre o conteúdo.]",
        ),
        PageLang::En => f(
            "[Document title]",
            "[Document subtitle]",
            "[Author name]",
            "[Date]",
            "[Company name]",
            "[Document summary: a short paragraph about the contents.]",
        ),
        PageLang::Es => f(
            "[Título del documento]",
            "[Subtítulo del documento]",
            "[Nombre del autor]",
            "[Fecha]",
            "[Nombre de la empresa]",
            "[Resumen del documento: un párrafo breve sobre el contenido.]",
        ),
    }
}

/// `COVER_NS`.
pub const COVER_NS: &str = concat!(
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
    "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" ",
    "xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" ",
    "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ",
    "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\""
);

const EMU: f64 = 12700.0;

/// `tw(pt)`: `Math.max(0, Math.round(pt * 20))`.
fn tw(pt: f64) -> f64 {
    max(0.0, round(pt * 20.0))
}

fn num(x: f64) -> String {
    to_js_string(x)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ink {
    White,
    Accent1,
    /// in the TS `Ink` union; no template writes text in it
    #[allow(dead_code)]
    Accent2,
    Dark,
    Grey,
}

fn ink_xml(ink: Ink, env: &CoverEnv<'_>) -> String {
    match ink {
        Ink::White => "<w:color w:val=\"FFFFFF\" w:themeColor=\"background1\"/>".to_owned(),
        Ink::Accent1 => format!("<w:color w:val=\"{}\" w:themeColor=\"accent1\"/>", env.accent1),
        Ink::Accent2 => format!("<w:color w:val=\"{}\" w:themeColor=\"accent2\"/>", env.accent2),
        Ink::Grey => "<w:color w:val=\"595959\" w:themeColor=\"text1\" w:themeTint=\"A6\"/>".to_owned(),
        Ink::Dark => "<w:color w:val=\"262626\" w:themeColor=\"text1\" w:themeTint=\"D9\"/>".to_owned(),
    }
}

/// The options of `para`.
struct ParaOpts {
    size: f64,
    ink: Ink,
    before: Option<f64>,
    jc: Option<&'static str>,
    ind_left: Option<f64>,
    bold: bool,
    major: bool,
    caps: bool,
}

impl ParaOpts {
    fn new(size: f64, ink: Ink) -> Self {
        ParaOpts { size, ink, before: None, jc: None, ind_left: None, bold: false, major: false, caps: false }
    }
    fn before(mut self, v: f64) -> Self {
        self.before = Some(v);
        self
    }
    fn jc(mut self, v: &'static str) -> Self {
        self.jc = Some(v);
        self
    }
    fn ind(mut self, v: f64) -> Self {
        self.ind_left = Some(v);
        self
    }
    fn major(mut self) -> Self {
        self.major = true;
        self
    }
    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

fn para(text: &str, o: &ParaOpts, env: &CoverEnv<'_>) -> String {
    let mut ppr =
        format!("<w:pPr><w:spacing w:before=\"{}\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/>", num(tw(o.before.unwrap_or(0.0))));
    if let Some(il) = o.ind_left
        && truthy(il)
    {
        ppr.push_str(&format!("<w:ind w:left=\"{}\"/>", num(tw(il))));
    }
    if let Some(j) = o.jc
        && j != "left"
    {
        ppr.push_str(&format!("<w:jc w:val=\"{j}\"/>"));
    }
    ppr.push_str("</w:pPr>");
    let mut rpr = String::from("<w:rPr>");
    if o.major {
        rpr.push_str(
            "<w:rFonts w:asciiTheme=\"majorHAnsi\" w:eastAsiaTheme=\"majorEastAsia\" w:hAnsiTheme=\"majorHAnsi\" w:cstheme=\"majorBidi\"/>",
        );
    }
    if o.bold {
        rpr.push_str("<w:b/><w:bCs/>");
    }
    if o.caps {
        rpr.push_str("<w:caps/>");
    }
    rpr.push_str(&ink_xml(o.ink, env));
    let sz = num(round(o.size * 2.0));
    rpr.push_str(&format!("<w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/></w:rPr>"));
    format!("<w:p>{ppr}<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(text))
}

/// The state of one `coverXml` call: the env and the module-level `zOrder` of the TS (reset at the
/// start of every call).
struct Cx<'e, 'a> {
    env: &'e mut CoverEnv<'a>,
    z: f64,
}

impl Cx<'_, '_> {
    fn z_next(&mut self) -> f64 {
        self.z += 1.0;
        self.z
    }
    fn lang_name(&self, pt: &'static str, en: &'static str, es: &'static str) -> &'static str {
        match self.env.lang {
            PageLang::Pt => pt,
            PageLang::En => en,
            PageLang::Es => es,
        }
    }
}

/// The options of `shape`.
struct ShapeOpts {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    geom: Option<&'static str>,
    fill: Option<&'static str>,
    tint: Option<f64>,
    line: Option<(&'static str, f64)>,
    flip_h: bool,
}

impl ShapeOpts {
    fn new(x: f64, y: f64, w: f64, h: f64, fill: &'static str) -> Self {
        ShapeOpts { x, y, w, h, geom: None, fill: Some(fill), tint: None, line: None, flip_h: false }
    }
}

fn shape(cx: &mut Cx<'_, '_>, o: &ShapeOpts) -> String {
    let id_n = (cx.env.drawing_id)();
    let id = num(id_n);
    let fill = match o.fill {
        None | Some("none") => "<a:noFill/>".to_owned(),
        Some(f) => {
            let mods = match o.tint {
                Some(t) if truthy(t) => {
                    format!("<a:lumMod val=\"{}\"/><a:lumOff val=\"{}\"/>", num(round((1.0 - t) * 100_000.0)), num(round(t * 100_000.0)))
                }
                _ => String::new(),
            };
            format!("<a:solidFill><a:schemeClr val=\"{f}\">{mods}</a:schemeClr></a:solidFill>")
        }
    };
    let ln = match o.line {
        Some((c, wpt)) => {
            format!("<a:ln w=\"{}\"><a:solidFill><a:schemeClr val=\"{c}\"/></a:solidFill></a:ln>", num(round(wpt * EMU)))
        }
        None => "<a:ln><a:noFill/></a:ln>".to_owned(),
    };
    let cx_ = num(round(o.w * EMU));
    let cy = num(round(o.h * EMU));
    let z = num(251_650_000.0 + 1024.0 * cx.z_next());
    let name = cx.lang_name("Retângulo", "Rectangle", "Rectángulo");
    let n1000 = num(id_n % 1000.0);
    let flip = if o.flip_h { " flipH=\"1\"" } else { "" };
    let geom = o.geom.unwrap_or("rect");
    format!(
        "<w:r><w:rPr><w:noProof/></w:rPr><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>\
         <wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{z}\" behindDoc=\"1\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionH>\
         <wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionV>\
         <wp:extent cx=\"{cx_}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/>\
         <wp:docPr id=\"{id}\" name=\"{name} {n1000}\"/><wp:cNvGraphicFramePr/>\
         <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
         <wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm{flip}><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx_}\" cy=\"{cy}\"/></a:xfrm>\
         <a:prstGeom prst=\"{geom}\"><a:avLst/></a:prstGeom>{fill}{ln}</wps:spPr>\
         <wps:bodyPr rot=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" anchor=\"ctr\" anchorCtr=\"0\" upright=\"1\"><a:noAutofit/></wps:bodyPr>\
         </wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice></mc:AlternateContent></w:r>",
        num(round(o.x * EMU)),
        num(round(o.y * EMU)),
    )
}

fn text_box(cx: &mut Cx<'_, '_>, x: f64, y: f64, w: f64, h: f64, anchor: &str, paras: &str) -> String {
    let id_n = (cx.env.drawing_id)();
    let id = num(id_n);
    let cx_ = num(round(w * EMU));
    let cy = num(round(h * EMU));
    let z = num(251_680_000.0 + 1024.0 * cx.z_next());
    let name = cx.lang_name("Caixa de Texto", "Text Box", "Cuadro de texto");
    let n1000 = num(id_n % 1000.0);
    format!(
        "<w:r><w:rPr><w:noProof/></w:rPr><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>\
         <wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{z}\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionH>\
         <wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionV>\
         <wp:extent cx=\"{cx_}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/>\
         <wp:docPr id=\"{id}\" name=\"{name} {n1000}\"/><wp:cNvGraphicFramePr/>\
         <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
         <wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx_}\" cy=\"{cy}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></wps:spPr>\
         <wps:txbx><w:txbxContent>{paras}</w:txbxContent></wps:txbx>\
         <wps:bodyPr rot=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"{anchor}\" anchorCtr=\"0\" upright=\"1\"><a:noAutofit/></wps:bodyPr>\
         </wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice></mc:AlternateContent></w:r>",
        num(round(x * EMU)),
        num(round(y * EMU)),
    )
}

fn info_box(cx: &mut Cx<'_, '_>, x: f64, y: f64, w: f64, ink: Ink, jc: &'static str) -> String {
    let lines = {
        let f = &cx.env.fields;
        para(&f.author, &ParaOpts::new(12.0, ink).jc(jc).bold(), cx.env)
            + &para(&f.date, &ParaOpts::new(11.0, ink).jc(jc).before(4.0), cx.env)
            + &para(&f.company, &ParaOpts::new(11.0, ink).jc(jc).before(4.0), cx.env)
    };
    text_box(cx, x, y, w, 66.0, "b", &lines)
}

fn anchor_para(runs: &str) -> String {
    format!("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>{runs}</w:p>")
}

/// `coverXml(id, env)`: the cover `id` (children of `w:body`): the "Cover Pages" `w:sdt` with the
/// shapes (in the first paragraph), the title, the subtitle, the summary and the author block; the
/// last paragraph carries the page break. `None` for an unknown id (the `sdtId` random is drawn
/// before the switch, as in the TS).
#[must_use]
pub fn cover_xml(id: &str, env: &mut CoverEnv<'_>) -> Option<String> {
    let p = env.page;
    let cw = p.w - p.left - p.right;
    let sdt_id = num((env.drawing_id)());
    let open = format!(
        "<w:sdt><w:sdtPr><w:id w:val=\"{sdt_id}\"/><w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/><w:docPartUnique/></w:docPartObj></w:sdtPr><w:sdtContent>"
    );
    let close = "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p></w:sdtContent></w:sdt>";
    let at = |y: f64| -> f64 { max(0.0, y - p.top - 14.0) };
    let bottom_y = p.h - p.bottom - 66.0;
    let f = env.fields.clone();
    let mut cx = Cx { env, z: 0.0 };
    let body = match id {
        "band" => {
            let by = p.h * 0.3;
            let s = shape(&mut cx, &ShapeOpts::new(0.0, by, p.w, 160.0, "accent1"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::Dark, "left");
            let e = &*cx.env;
            anchor_para(&(s + &i))
                + &para(&f.title, &ParaOpts::new(40.0, Ink::White).before(at(by + 30.0)).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::White).before(10.0), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(70.0), e)
        }
        "sidebar" => {
            let side = 110.0;
            let ind = max(0.0, side + 30.0 - p.left);
            let s = shape(&mut cx, &ShapeOpts::new(0.0, 0.0, side, p.h, "accent1"));
            let i = info_box(&mut cx, p.left + ind, bottom_y, cw - ind, Ink::Dark, "left");
            let e = &*cx.env;
            anchor_para(&(s + &i))
                + &para(&f.title, &ParaOpts::new(44.0, Ink::Accent1).before(at(p.h * 0.32)).ind(ind).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::Dark).before(12.0).ind(ind), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(30.0).ind(ind), e)
        }
        "frame" => {
            let mut o1 = ShapeOpts::new(30.0, 30.0, p.w - 60.0, p.h - 60.0, "none");
            o1.line = Some(("accent1", 3.0));
            let s1 = shape(&mut cx, &o1);
            let s2 = shape(&mut cx, &ShapeOpts::new(p.w / 2.0 - 40.0, p.h * 0.38 + 66.0, 80.0, 3.0, "accent2"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::Dark, "center");
            let e = &*cx.env;
            anchor_para(&(s1 + &s2 + &i))
                + &para(&f.title, &ParaOpts::new(40.0, Ink::Accent1).before(at(p.h * 0.38 - 10.0)).jc("center").major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::Dark).before(30.0).jc("center"), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(24.0).jc("center"), e)
        }
        "block" => {
            let by = p.h * 0.28;
            let s = shape(&mut cx, &ShapeOpts::new(p.left, by, cw, 200.0, "accent1"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::Grey, "center");
            let e = &*cx.env;
            anchor_para(&(s + &i))
                + &para(&f.title, &ParaOpts::new(40.0, Ink::White).before(at(by + 50.0)).jc("center").major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::White).before(12.0).jc("center"), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(100.0).jc("center"), e)
        }
        "minimal" => {
            let ty = p.h * 0.36;
            let s = shape(&mut cx, &ShapeOpts::new(p.left, ty - 18.0, 60.0, 4.0, "accent1"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::Grey, "left");
            let e = &*cx.env;
            anchor_para(&(s + &i))
                + &para(&f.title, &ParaOpts::new(36.0, Ink::Dark).before(at(ty)).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::Grey).before(10.0), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(30.0), e)
        }
        "base" => {
            let by = p.h * 0.7;
            let s = shape(&mut cx, &ShapeOpts::new(0.0, by, p.w, p.h - by, "accent1"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::White, "left");
            let e = &*cx.env;
            anchor_para(&(s + &i))
                + &para(&f.title, &ParaOpts::new(42.0, Ink::Accent1).before(at(p.h * 0.3)).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::Dark).before(12.0), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(30.0), e)
        }
        "duotone" => {
            let top = p.h * 0.45;
            let s1 = shape(&mut cx, &ShapeOpts::new(0.0, 0.0, p.w, top, "accent1"));
            let s2 = shape(&mut cx, &ShapeOpts::new(0.0, top, p.w, 8.0, "accent2"));
            let i = info_box(&mut cx, p.left, bottom_y, cw, Ink::Dark, "left");
            let e = &*cx.env;
            anchor_para(&(s1 + &s2 + &i))
                + &para(&f.title, &ParaOpts::new(40.0, Ink::White).before(at(top - 120.0)).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::White).before(10.0), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(70.0), e)
        }
        "diagonal" => {
            let mut o1 = ShapeOpts::new(0.0, p.h * 0.55, p.w * 0.8, p.h * 0.45, "accent1");
            o1.geom = Some("rtTriangle");
            let s1 = shape(&mut cx, &o1);
            let mut o2 = ShapeOpts::new(0.0, p.h * 0.72, p.w * 0.5, p.h * 0.28, "accent2");
            o2.geom = Some("rtTriangle");
            let s2 = shape(&mut cx, &o2);
            let i = info_box(&mut cx, p.w * 0.45, bottom_y, p.w * 0.55 - p.right, Ink::Dark, "right");
            let e = &*cx.env;
            anchor_para(&(s1 + &s2 + &i))
                + &para(&f.title, &ParaOpts::new(42.0, Ink::Accent1).before(at(p.h * 0.22)).major(), e)
                + &para(&f.subtitle, &ParaOpts::new(16.0, Ink::Dark).before(12.0), e)
                + &para(&f.summary, &ParaOpts::new(11.0, Ink::Grey).before(30.0), e)
        }
        _ => return None,
    };
    Some(open + &body + close)
}
