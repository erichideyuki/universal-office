//! D2b-F4: the header/footer template galleries, the pure half: editor `hf-templates.ts` (the
//! TEMPLATES table, `hfTemplates`, `galleryKind`, `placeholderText`, `hfTemplateXml` and its
//! helpers). The templates are CODE, not data: the XML depends on the style id, the theme accent,
//! the language and the random drawing ids, so it is ported as code and proven byte-equal by the
//! Node ops capture (`js/capture-d2b-f4-ops.mjs`).
//!
//! Not here: `hfBandLabel`/`sameAsPreviousLabel` (the band labels are the shell's) and
//! `DATE_TIME_FORMATS` (F10).

use uo_jsnum::format::to_js_string;

use super::super::page::PageLang;
use super::HfKind;
use crate::write::js::esc_attr as esc;

/// `HfGallery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfGallery {
    Header,
    Footer,
    PageTop,
    PageBottom,
    PageMargin,
    PageCurrent,
}

impl HfGallery {
    /// The TS string (`'header' | 'footer' | 'pageTop' | …`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            HfGallery::Header => "header",
            HfGallery::Footer => "footer",
            HfGallery::PageTop => "pageTop",
            HfGallery::PageBottom => "pageBottom",
            HfGallery::PageMargin => "pageMargin",
            HfGallery::PageCurrent => "pageCurrent",
        }
    }

    /// The gallery of a TS string.
    #[must_use]
    pub fn parse(s: &str) -> Option<HfGallery> {
        Some(match s {
            "header" => HfGallery::Header,
            "footer" => HfGallery::Footer,
            "pageTop" => HfGallery::PageTop,
            "pageBottom" => HfGallery::PageBottom,
            "pageMargin" => HfGallery::PageMargin,
            "pageCurrent" => HfGallery::PageCurrent,
            _ => return None,
        })
    }

    /// Every gallery, in the TS table's order.
    pub const ALL: [HfGallery; 6] =
        [HfGallery::Header, HfGallery::Footer, HfGallery::PageTop, HfGallery::PageBottom, HfGallery::PageMargin, HfGallery::PageCurrent];
}

/// `HfTemplate`: the id and the name per language (`en`, `pt`, `es`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HfTemplate {
    pub id: &'static str,
    pub name: [&'static str; 3],
}

impl HfTemplate {
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

const fn t(id: &'static str, en: &'static str, pt: &'static str, es: &'static str) -> HfTemplate {
    HfTemplate { id, name: [en, pt, es] }
}

const NUM_TOP_BOTTOM: [HfTemplate; 6] = [
    t("plainLeft", "Plain number left", "Número simples à esquerda", "Número sin formato a la izquierda"),
    t("plainCenter", "Plain number centered", "Número simples centralizado", "Número sin formato centrado"),
    t("plainRight", "Plain number right", "Número simples à direita", "Número sin formato a la derecha"),
    t("pageXofY", "Page X of Y", "Página X de Y", "Página X de Y"),
    t("accentRule", "Accent rule", "Linha de destaque", "Línea de énfasis"),
    t("brackets", "Brackets", "Colchetes", "Corchetes"),
];

const HEADER_T: [HfTemplate; 6] = [
    t("blank", "Blank", "Em branco", "En blanco"),
    t("blank3", "Blank (three columns)", "Em branco (três colunas)", "En blanco (tres columnas)"),
    t("title", "Centered title", "Título centralizado", "Título centrado"),
    t("rule", "Title and author", "Título e autor", "Título y autor"),
    t("band", "Color band", "Faixa colorida", "Banda de color"),
    t("accent", "Accent bar", "Barra de destaque", "Barra de énfasis"),
];

const FOOTER_T: [HfTemplate; 6] = [
    t("blank", "Blank", "Em branco", "En blanco"),
    t("blank3", "Blank (three columns)", "Em branco (três colunas)", "En blanco (tres columnas)"),
    t("pageXofY", "Page X of Y", "Página X de Y", "Página X de Y"),
    t("rule", "Title and page", "Título e página", "Título y página"),
    t("band", "Color band", "Faixa colorida", "Banda de color"),
    t("accent", "Accent number", "Número em destaque", "Número destacado"),
];

const MARGIN_T: [HfTemplate; 3] = [
    t("right", "Right margin", "Margem direita", "Margen derecho"),
    t("left", "Left margin", "Margem esquerda", "Margen izquierdo"),
    t("circle", "Circle, right", "Círculo à direita", "Círculo a la derecha"),
];

const CURRENT_T: [HfTemplate; 4] = [
    t("plain", "Plain number", "Número simples", "Número sin formato"),
    t("pageXofY", "Page X of Y", "Página X de Y", "Página X de Y"),
    t("brackets", "Brackets", "Colchetes", "Corchetes"),
    t("dashes", "Dashes", "Traços", "Guiones"),
];

/// `hfTemplates(gallery)`: the templates of a gallery (the gallery's order).
#[must_use]
pub fn hf_templates(g: HfGallery) -> &'static [HfTemplate] {
    match g {
        HfGallery::Header => &HEADER_T,
        HfGallery::Footer => &FOOTER_T,
        HfGallery::PageTop | HfGallery::PageBottom => &NUM_TOP_BOTTOM,
        HfGallery::PageMargin => &MARGIN_T,
        HfGallery::PageCurrent => &CURRENT_T,
    }
}

/// `galleryKind(gallery)`: the part a template occupies (`None`: Current Position, no part).
#[must_use]
pub fn gallery_kind(g: HfGallery) -> Option<HfKind> {
    match g {
        HfGallery::Footer | HfGallery::PageBottom => Some(HfKind::Footer),
        HfGallery::PageCurrent => None,
        _ => Some(HfKind::Header),
    }
}

/// `placeholderText(lang)`.
#[must_use]
pub fn placeholder_text(lang: PageLang) -> &'static str {
    match lang {
        PageLang::Pt => "[Digite aqui]",
        PageLang::En => "[Type here]",
        PageLang::Es => "[Escribe aquí]",
    }
}

/// `HfTemplateEnv`: the context of a template. `drawing_id` is `() => 1 + floor(random * 0x7ffffffe)`.
pub struct HfTemplateEnv<'a> {
    pub style_id: Option<&'a str>,
    pub accent: &'a str,
    pub lang: PageLang,
    pub drawing_id: &'a mut dyn FnMut() -> f64,
}

const WHITE: &str = "<w:color w:val=\"FFFFFF\" w:themeColor=\"background1\"/>";

fn run(text: &str, r_pr: &str) -> String {
    let rp = if r_pr.is_empty() { String::new() } else { format!("<w:rPr>{r_pr}</w:rPr>") };
    format!("<w:r>{rp}<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(text))
}

fn field(instr: &str, shown: &str, r_pr: &str) -> String {
    let r = if r_pr.is_empty() { String::new() } else { format!("<w:rPr>{r_pr}</w:rPr>") };
    let shown_pr = if r_pr.is_empty() { "<w:rPr><w:noProof/></w:rPr>".to_owned() } else { format!("<w:rPr>{r_pr}<w:noProof/></w:rPr>") };
    format!(
        "<w:r>{r}<w:fldChar w:fldCharType=\"begin\"/></w:r><w:r>{r}<w:instrText xml:space=\"preserve\"> {} </w:instrText></w:r>\
         <w:r>{r}<w:fldChar w:fldCharType=\"separate\"/></w:r><w:r>{shown_pr}<w:t>{}</w:t></w:r><w:r>{r}<w:fldChar w:fldCharType=\"end\"/></w:r>",
        esc(instr),
        esc(shown)
    )
}

fn page(r_pr: &str) -> String {
    field("PAGE   \\* MERGEFORMAT", "1", r_pr)
}

fn pages(r_pr: &str) -> String {
    field("NUMPAGES   \\* MERGEFORMAT", "1", r_pr)
}

fn ptab(al: &str) -> String {
    format!("<w:r><w:ptab w:relativeTo=\"margin\" w:alignment=\"{al}\" w:leader=\"none\"/></w:r>")
}

fn color(hex: &str) -> String {
    format!("<w:color w:val=\"{hex}\" w:themeColor=\"accent1\"/>")
}

fn pick<'a>(lang: PageLang, pt: &'a str, en: &'a str, es: &'a str) -> &'a str {
    match lang {
        PageLang::Pt => pt,
        PageLang::En => en,
        PageLang::Es => es,
    }
}

fn para(env: &HfTemplateEnv<'_>, body: &str, p_pr: &str) -> String {
    let style = match env.style_id {
        Some(s) if !s.is_empty() => format!("<w:pStyle w:val=\"{}\"/>", esc(s)),
        _ => String::new(),
    };
    format!("<w:p><w:pPr>{style}{p_pr}</w:pPr>{body}</w:p>")
}

fn page_number_sdt(env: &mut HfTemplateEnv<'_>, gallery: &str, inner: &str) -> String {
    let id = to_js_string((env.drawing_id)());
    format!(
        "<w:sdt><w:sdtPr><w:id w:val=\"{id}\"/><w:docPartObj><w:docPartGallery w:val=\"Page Numbers ({gallery})\"/><w:docPartUnique/></w:docPartObj></w:sdtPr>\
         <w:sdtContent>{inner}</w:sdtContent></w:sdt>"
    )
}

fn page_x_of_y(env: &HfTemplateEnv<'_>, num_r_pr: &str) -> String {
    if env.lang == PageLang::En {
        run("Page ", "") + &page(num_r_pr) + &run(" of ", "") + &pages(num_r_pr)
    } else {
        run("Página ", "") + &page(num_r_pr) + &run(" de ", "") + &pages(num_r_pr)
    }
}

fn margin_box(env: &mut HfTemplateEnv<'_>, side: &str, circle: bool) -> String {
    let id_n = (env.drawing_id)();
    let id = to_js_string(id_n);
    let size = if circle { "457200" } else { "520700" };
    let h = if circle { "457200" } else { "368300" };
    let fill = if circle { "<a:solidFill><a:schemeClr val=\"accent1\"/></a:solidFill>" } else { "<a:noFill/>" };
    let r_pr = if circle {
        format!("<w:b/>{WHITE}<w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/>")
    } else {
        format!("<w:b/>{}<w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/>", color(env.accent))
    };
    let inner = format!("<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>{}</w:p>", page(&r_pr));
    let name = pick(env.lang, "Caixa de Texto", "Text Box", "Cuadro de texto");
    let n1000 = to_js_string(id_n % 1000.0);
    let prst = if circle { "ellipse" } else { "rect" };
    format!(
        "<w:r><w:rPr><w:noProof/></w:rPr><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>\
         <wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"251659264\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"{side}Margin\"><wp:align>center</wp:align></wp:positionH>\
         <wp:positionV relativeFrom=\"margin\"><wp:align>center</wp:align></wp:positionV>\
         <wp:extent cx=\"{size}\" cy=\"{h}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/>\
         <wp:docPr id=\"{id}\" name=\"{name} {n1000}\"/><wp:cNvGraphicFramePr/>\
         <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
         <wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{size}\" cy=\"{h}\"/></a:xfrm>\
         <a:prstGeom prst=\"{prst}\"><a:avLst/></a:prstGeom>{fill}<a:ln><a:noFill/></a:ln></wps:spPr>\
         <wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx>\
         <wps:bodyPr rot=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\" anchorCtr=\"0\" upright=\"1\"><a:noAutofit/></wps:bodyPr>\
         </wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice></mc:AlternateContent></w:r>"
    )
}

fn bdr(side: &str, sz: u32, hex: &str, theme: bool) -> String {
    let space = if side == "left" { 4 } else { 1 };
    let th = if theme { " w:themeColor=\"accent1\"" } else { "" };
    format!("<w:pBdr><w:{side} w:val=\"single\" w:sz=\"{sz}\" w:space=\"{space}\" w:color=\"{hex}\"{th}/></w:pBdr>")
}

fn jc(v: &str) -> String {
    format!("<w:jc w:val=\"{v}\"/>")
}

/// `hfTemplateXml(gallery, id, env)`: the content (children of `w:hdr`/`w:ftr`) of template `id`;
/// Current Position gives ONE paragraph whose runs go at the cursor. `None` for an unknown id
/// (the random drawing ids that the TS draws before are drawn here too).
#[must_use]
pub fn hf_template_xml(gallery: HfGallery, id: &str, env: &mut HfTemplateEnv<'_>) -> Option<String> {
    let lang = env.lang;
    let ph = placeholder_text(lang);
    let title = pick(lang, "[Título do documento]", "[Document title]", "[Título del documento]");
    let author = pick(lang, "[Nome do autor]", "[Author name]", "[Nombre del autor]");
    let accent = env.accent.to_owned();
    let shd = format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{accent}\" w:themeFill=\"accent1\"/>");
    let three = run(ph, "") + &ptab("center") + &run(ph, "") + &ptab("right") + &run(ph, "");
    match gallery {
        HfGallery::Header | HfGallery::Footer => {
            let is_h = gallery == HfGallery::Header;
            let spacing = "<w:spacing w:before=\"60\" w:after=\"60\"/>";
            Some(match id {
                "blank" => para(env, &run(ph, ""), ""),
                "blank3" => para(env, &three, ""),
                "title" => para(
                    env,
                    &run(title, &format!("<w:smallCaps/>{}<w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/>", color(&accent))),
                    &format!("{}<w:jc w:val=\"center\"/>", bdr("bottom", 12, &accent, true)),
                ),
                "rule" => {
                    if is_h {
                        para(
                            env,
                            &(run(title, "<w:b/>") + &ptab("right") + &run(author, &color(&accent))),
                            &bdr("bottom", 6, "A6A6A6", false),
                        )
                    } else {
                        para(env, &(run(title, &color(&accent)) + &ptab("right") + &page("<w:b/>")), &bdr("top", 6, "A6A6A6", false))
                    }
                }
                "band" => {
                    if is_h {
                        para(
                            env,
                            &(run(title, &format!("<w:b/>{WHITE}")) + &ptab("right") + &run(author, WHITE)),
                            &format!("{shd}{spacing}"),
                        )
                    } else {
                        para(env, &(run(title, WHITE) + &ptab("right") + &page(&format!("<w:b/>{WHITE}"))), &format!("{shd}{spacing}"))
                    }
                }
                "accent" => {
                    if is_h {
                        para(env, &run(title, &format!("<w:b/>{}", color(&accent))), &bdr("left", 24, &accent, true))
                    } else {
                        para(
                            env,
                            &(run("— ", &color(&accent))
                                + &page(&format!("<w:b/>{}<w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/>", color(&accent)))
                                + &run(" —", &color(&accent))),
                            "<w:jc w:val=\"center\"/>",
                        )
                    }
                }
                "pageXofY" => {
                    if is_h {
                        return None;
                    }
                    para(env, &page_x_of_y(env, "<w:b/>"), "<w:jc w:val=\"center\"/>")
                }
                _ => return None,
            })
        }
        HfGallery::PageTop | HfGallery::PageBottom => {
            let top = gallery == HfGallery::PageTop;
            let g = if top { "Top of Page" } else { "Bottom of Page" };
            let inner: Option<String> = match id {
                "plainLeft" => Some(para(env, &page(""), "")),
                "plainCenter" => Some(para(env, &page(""), &jc("center"))),
                "plainRight" => Some(para(env, &page(""), &jc("right"))),
                "pageXofY" => Some(para(env, &page_x_of_y(env, "<w:b/>"), &jc("right"))),
                "accentRule" => Some(para(
                    env,
                    &page(&format!("<w:b/>{}", color(&accent))),
                    &format!("{}{}", bdr(if top { "bottom" } else { "top" }, 12, &accent, true), jc("right")),
                )),
                "brackets" => Some(para(env, &(run("[ ", "") + &page("") + &run(" ]", "")), &jc("center"))),
                _ => None,
            };
            // the number + an empty paragraph of the style inside the sdt: both built BEFORE the sdt id is drawn
            let inner = inner?;
            let tail = para(env, "", "");
            Some(page_number_sdt(env, g, &(inner + &tail)))
        }
        HfGallery::PageMargin => {
            let boxed = match id {
                "right" => margin_box(env, "right", false),
                "left" => margin_box(env, "left", false),
                "circle" => margin_box(env, "right", true),
                _ => return None,
            };
            let p = para(env, &boxed, "");
            Some(page_number_sdt(env, "Margins", &p))
        }
        HfGallery::PageCurrent => Some(match id {
            "plain" => para(env, &page(""), ""),
            "pageXofY" => para(env, &page_x_of_y(env, ""), ""),
            "brackets" => para(env, &(run("[", "") + &page("") + &run("]", "")), ""),
            "dashes" => para(env, &(run("– ", "") + &page("") + &run(" –", "")), ""),
            _ => return None,
        }),
    }
}
