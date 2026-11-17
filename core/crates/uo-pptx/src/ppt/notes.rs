//! Speaker notes (NotesContainer) → `ppt/notesSlides/notesSlideN.xml` + a minimal `notesMaster`
//! (`ppt/notes.ts`). The viewer does not draw notes; the one who consumes them is the exporter
//! (Markdown/HTML reads the text of the `body` placeholder of the notesSlide).
//!
//! In the .ppt the notes page has the placeholders "slide image" (sldImg) and "body" (body, text
//! type 2 = notes) — often with an undefined anchor (0x80000001 in every field: inherit from the
//! notes master). That is why the shapes come out with `<p:spPr/>` (inheritance from the
//! notesMaster) and the text with explicit properties, as in the slides.

use super::oa::OaShape;
use super::reader::{PptDocument, PptSlide};
use super::shapeconv::placeholder_type;
use super::text::read_client_textbox;
use super::textconv::{TextConvCtx, TextHost, emu, paragraphs_xml};
use super::xmlw::{XML_DECL, el, n, ns_attrs, s};

const SPTREE_HEAD: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

/// "Undefined" anchor of the binary (inherits from the notes master).
const UNDEF: i32 = -2_147_483_647; // 0x80000001 as i32

fn valid_anchor(sh: &OaShape<'_>) -> bool {
    sh.anchor.is_some_and(|a| a.l != UNDEF && a.t != UNDEF && a.r > a.l && a.b > a.t)
}

/// The shapes (flattening groups) of a drawing.
fn flat_shapes<'a>(doc: &'a PptDocument<'a>, s: Option<&PptSlide<'a>>) -> Vec<&'a OaShape<'a>> {
    fn walk<'a>(doc: &'a PptDocument<'a>, sh: &'a OaShape<'a>, out: &mut Vec<&'a OaShape<'a>>) {
        match &sh.children {
            Some(kids) => {
                for &k in kids {
                    if let Some(c) = doc.shapes.get(k) {
                        walk(doc, c, out);
                    }
                }
            }
            None => out.push(sh),
        }
    }
    let mut out = Vec::new();
    if let Some(root) = s.and_then(|x| x.drawing.root).and_then(|r| doc.shapes.get(r)) {
        for c in doc.shapes.kids(root) {
            walk(doc, c, &mut out);
        }
    }
    out
}

/// The notes of a slide (`NotesAtom.slideIdRef` = slideId of the slide).
#[must_use]
pub fn notes_for<'a>(doc: &'a PptDocument<'a>, slide: &PptSlide<'_>) -> Option<&'a PptSlide<'a>> {
    doc.notes.iter().find(|nt| nt.slide_id_ref == Some(slide.slide_id))
}

/// `<p:notes>` with the text of the body; `None` when there is no text.
#[must_use]
pub fn notes_slide_xml<'a>(doc: &'a PptDocument<'a>, notes: &PptSlide<'a>, ctx: &TextConvCtx<'a>) -> Option<String> {
    let mut body = String::new();
    for sh in flat_shapes(doc, Some(notes)) {
        if placeholder_type(sh) != Some("body") {
            continue;
        }
        let Some(cb) = &sh.client_textbox else { continue };
        let ct = read_client_textbox(cb);
        let block = match ct.block {
            Some(b) => Some(b),
            None => ct.outline_ref.and_then(|r| notes.outline.get(usize::try_from(r).ok()?)).map(|b| (**b).clone()),
        };
        if let Some(block) = block
            && !block.text.iter().all(|&u| uo_jsnum::text::is_js_space_unit(u))
        {
            let mut host: Option<&mut dyn TextHost> = None;
            body.push_str(&paragraphs_xml(&block, ctx, &mut host));
        }
    }
    if body.is_empty() {
        return None;
    }
    let ph = |id: u32, name: &str, ty: &str, idx: Option<u32>, tx_body: &str| -> String {
        el(
            "p:sp",
            vec![],
            &(el(
                "p:nvSpPr",
                vec![],
                &(el("p:cNvPr", vec![("id", n(id)), ("name", s(name))], "")
                    + "<p:cNvSpPr/>"
                    + &el("p:nvPr", vec![], &el("p:ph", vec![("type", s(ty)), ("idx", idx.and_then(n))], ""))),
            ) + "<p:spPr/>"
                + tx_body),
        )
    };
    let tree = ph(2, "Imagem do Slide", "sldImg", None, "")
        + &ph(3, "Anotações", "body", Some(1), &el("p:txBody", vec![], &format!("<a:bodyPr/><a:lstStyle/>{body}")));
    Some(format!(
        "{XML_DECL}{}",
        el(
            "p:notes",
            ns_attrs(),
            &(el("p:cSld", vec![], &el("p:spTree", vec![], &format!("{SPTREE_HEAD}{tree}")))
                + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>")
        )
    ))
}

/// `<p:notesMaster>` with the placeholders sldImg/body in the positions of the .ppt's notes master
/// (or in PowerPoint's default layout: image in the top half, body below).
#[must_use]
pub fn notes_master_xml<'a>(doc: &'a PptDocument<'a>) -> String {
    let w = f64::from(doc.notes_w);
    let h = f64::from(doc.notes_h);
    let rect = |ty: &str, dflt: [f64; 4]| -> [f64; 4] {
        let sh = flat_shapes(doc, doc.notes_master.as_ref()).into_iter().find(|x| placeholder_type(x) == Some(ty) && valid_anchor(x));
        match sh.and_then(|x| x.anchor) {
            Some(a) => [f64::from(a.l), f64::from(a.t), f64::from(a.r) - f64::from(a.l), f64::from(a.b) - f64::from(a.t)],
            None => dflt,
        }
    };
    let img = rect("sldImg", [w * 0.15, h * 0.08, w * 0.7, h * 0.39]);
    let body = rect("body", [w * 0.1, h * 0.5, w * 0.8, h * 0.39]);
    let sp = |id: u32, name: &str, ty: &str, r: [f64; 4], idx: Option<u32>, txt: bool| -> String {
        el(
            "p:sp",
            vec![],
            &(el(
                "p:nvSpPr",
                vec![],
                &(el("p:cNvPr", vec![("id", n(id)), ("name", s(name))], "")
                    + "<p:cNvSpPr/>"
                    + &el("p:nvPr", vec![], &el("p:ph", vec![("type", s(ty)), ("idx", idx.and_then(n))], ""))),
            ) + &el(
                "p:spPr",
                vec![],
                &(el(
                    "a:xfrm",
                    vec![],
                    &(el("a:off", vec![("x", n(emu(r[0]))), ("y", n(emu(r[1])))], "")
                        + &el("a:ext", vec![("cx", n(emu(r[2]))), ("cy", n(emu(r[3])))], "")),
                ) + "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>"),
            ) + if txt { "<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"pt-BR\"/></a:p></p:txBody>" } else { "" }),
        )
    };
    format!(
        "{XML_DECL}{}",
        el(
            "p:notesMaster",
            ns_attrs(),
            &(el(
                "p:cSld",
                vec![],
                &el(
                    "p:spTree",
                    vec![],
                    &(SPTREE_HEAD.to_owned()
                        + &sp(2, "Imagem do Slide", "sldImg", img, None, false)
                        + &sp(3, "Anotações", "body", body, Some(1), true))
                )
            ) + "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>")
        )
    )
}
