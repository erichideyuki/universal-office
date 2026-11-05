//! Text boxes of the drawing family (D2b-F7): the gallery templates (`shape-ops.ts`
//! `textboxTemplateSpec`), `insertTextbox`, `createDrawnTextbox`, the links of a chain
//! (`linkTextboxes`, `breakTextboxLink`), `setAsDefaultTextbox` and `alignObjects`.

use std::sync::Arc;

use uo_jsnum::math;

use super::super::super::command::{CommandError, Event, Outcome};
use super::super::super::state::Selection;
use super::super::super::text::{DocPosition, insert_index, normalize_range, para_length, props_at};
use super::super::context::EditContext;
use super::super::pictures::PictureCmd;
use super::pure::Z_FIRST;
use super::{AlignKind, DrawingCmd, GalleryBox, ObjFacts, ObjRect, Path, Step, change_is_finite, format_of};
use crate::js::{JsNumber, Nullable, Str};
use crate::model::picture::{PicColor, PicLine, PicPos};
use crate::model::run::{Run, TextboxView};
use crate::model::story::Story;
use crate::session::{ExecCx, Session};
use crate::word::obj;
use crate::write::emit::fx::PictureChange;
use crate::write::emit::object::{
    BodyAnchor, BodyAutofit, NewPos, NewSizeRel, NewTextboxOptions, NewWrap, ShapeChange, ShapeFill, StoryChange, TextboxBodyPatch,
    new_textbox_drawing_xml, new_textbox_fill, new_textbox_line, patch_shape,
};
use crate::write::emit::settings::hf_part_xml;

type Res = Result<Outcome, CommandError>;

/// `TextboxTemplateSpec`: what goes into the XML of a gallery box.
pub(crate) struct TemplateSpec {
    cx: f64,
    cy: f64,
    pos_h: NewPos,
    pos_v: NewPos,
    size_rel_h: Option<NewSizeRel>,
    size_rel_v: Option<NewSizeRel>,
    wrap: NewWrap,
    fill: ShapeFill,
    line: Option<PicLine>,
    body: TextboxBodyPatch,
    content: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn run_xml(text: &str, rpr: &str) -> String {
    let pr = if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") };
    format!("<w:r>{pr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(text))
}

fn para_xml(body: &str, ppr: &str, rpr: &str) -> String {
    let r = if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") };
    format!("<w:p><w:pPr>{ppr}{r}</w:pPr>{body}</w:p>")
}

fn sz(half: u32) -> String {
    format!("<w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/>")
}

fn scheme(val: &str, mods: &[(&str, f64)]) -> PicColor {
    PicColor {
        kind: "schemeClr".into(),
        val: val.into(),
        mods: (!mods.is_empty()).then(|| mods.iter().map(|(k, v)| (Str::from(*k), JsNumber::Num(*v))).collect()),
        ..PicColor::default()
    }
}

struct Texts {
    body: &'static str,
    quote: &'static str,
    source: &'static str,
    title: &'static str,
    side: &'static str,
    note: &'static str,
}

fn texts(lang: &str) -> Texts {
    if lang == "es" {
        Texts {
            body: "[Usa este espacio para destacar un punto clave del documento. Para mover el cuadro de texto, arrastra su borde.]",
            quote: "[\u{201c}Capta la atenci\u{f3}n del lector con una cita destacada del documento.\u{201d}]",
            source: "[Cita la fuente aqu\u{ed}.]",
            title: "[T\u{ed}tulo de la barra lateral]",
            side: "[La barra lateral es un buen lugar para informaci\u{f3}n complementaria: una lista de puntos, un resumen o una sugerencia. Se queda junto al margen y acompa\u{f1}a a la p\u{e1}gina.]",
            note: "[Nota]",
        }
    } else if lang == "pt" {
        Texts {
            body: "[Use este espa\u{e7}o para destacar um ponto-chave do documento. Para mover a caixa de texto, arraste a borda dela.]",
            quote: "[\u{201c}Chame a aten\u{e7}\u{e3}o do leitor com uma cita\u{e7}\u{e3}o marcante do documento.\u{201d}]",
            source: "[Cite a fonte aqui.]",
            title: "[T\u{ed}tulo da barra lateral]",
            side: "[A barra lateral \u{e9} um bom lugar para informa\u{e7}\u{f5}es complementares: uma lista de pontos, um resumo ou uma dica. Ela fica junto \u{e0} margem e acompanha a p\u{e1}gina.]",
            note: "[Nota]",
        }
    } else {
        Texts {
            body: "[Use this space to highlight a key point from your document. To move this text box, drag its border.]",
            quote: "[\u{201c}Grab your reader\u{2019}s attention with a great quote from the document.\u{201d}]",
            source: "[Cite your source here.]",
            title: "[Sidebar title]",
            side: "[A sidebar is a good place for supporting information: a list of points, a summary or a tip. It sits by the margin and stays with the page.]",
            note: "[Note]",
        }
    }
}

fn body_patch(l: f64, t: f64, r: f64, b: f64, autofit: BodyAutofit) -> TextboxBodyPatch {
    TextboxBodyPatch {
        l_ins: Some(l),
        t_ins: Some(t),
        r_ins: Some(r),
        b_ins: Some(b),
        autofit: Some(autofit),
        ..TextboxBodyPatch::default()
    }
}

fn pos(rel: &str, align: Option<&str>, offset: Option<f64>) -> NewPos {
    NewPos { relative_from: rel.into(), align: align.map(str::to_owned), offset, ..NewPos::default() }
}

/// `textboxTemplateSpec(id, { accent, lang })`.
pub(crate) fn template_spec(id: &str, accent: &str, lang: &str) -> Option<TemplateSpec> {
    let t = texts(lang);
    let pad = 0.1 * 914_400.0;
    let accent_c = |k: u32| scheme(&format!("accent{k}"), &[]);
    let white = "<w:color w:val=\"FFFFFF\" w:themeColor=\"background1\"/>";
    let acc = format!("<w:color w:val=\"{accent}\" w:themeColor=\"accent1\"/>");
    let r = |x: f64| math::round(x);
    Some(match id {
        "simple" => TemplateSpec {
            cx: r(2.5 * 914_400.0),
            cy: r(914_400.0),
            pos_h: pos("margin", Some("center"), None),
            pos_v: pos("paragraph", None, Some(0.0)),
            size_rel_h: None,
            size_rel_v: None,
            wrap: NewWrap::Square,
            fill: ShapeFill::Solid(scheme("lt1", &[])),
            line: Some(PicLine {
                w: Some(JsNumber::Num(6350.0)),
                fill: Some(Nullable::Val(PicColor { kind: "prstClr".into(), val: "black".into(), ..PicColor::default() })),
                ..PicLine::default()
            }),
            body: body_patch(91440.0, 45720.0, 91440.0, 45720.0, BodyAutofit::Shape),
            content: para_xml(&run_xml(t.body, ""), "", ""),
        },
        "quote" => TemplateSpec {
            cx: r(3.0 * 914_400.0),
            cy: r(1.2 * 914_400.0),
            pos_h: pos("margin", Some("right"), None),
            pos_v: pos("paragraph", None, Some(0.0)),
            size_rel_h: Some(NewSizeRel { relative_from: "margin".into(), pct: 55000.0 }),
            size_rel_v: None,
            wrap: NewWrap::Square,
            fill: ShapeFill::None,
            line: None,
            body: body_patch(182_880.0, 182_880.0, 182_880.0, 182_880.0, BodyAutofit::Shape),
            content: para_xml(
                &run_xml(t.quote, &format!("<w:i/><w:iCs/>{acc}{}", sz(28))),
                &format!(
                    "<w:pBdr><w:left w:val=\"single\" w:sz=\"24\" w:space=\"8\" w:color=\"{accent}\" w:themeColor=\"accent1\"/></w:pBdr><w:spacing w:after=\"120\"/>"
                ),
                &format!("<w:i/><w:iCs/>{acc}{}", sz(28)),
            ) + &para_xml(&run_xml(t.source, &sz(18)), "<w:jc w:val=\"right\"/>", &sz(18)),
        },
        "sidebarRight" | "sidebarLeft" => {
            let right = id == "sidebarRight";
            TemplateSpec {
                cx: r(2.2 * 914_400.0),
                cy: r(9.0 * 914_400.0),
                pos_h: pos("page", Some(if right { "right" } else { "left" }), None),
                pos_v: pos("page", Some("top"), None),
                size_rel_h: Some(NewSizeRel { relative_from: "page".into(), pct: 28000.0 }),
                size_rel_v: Some(NewSizeRel { relative_from: "page".into(), pct: 100_000.0 }),
                wrap: NewWrap::Square,
                fill: ShapeFill::Solid(accent_c(1)),
                line: None,
                body: body_patch(228_600.0, 914_400.0, 228_600.0, 457_200.0, BodyAutofit::None),
                content: para_xml(
                    &run_xml(t.title, &format!("<w:b/><w:bCs/>{white}{}", sz(32))),
                    "<w:spacing w:after=\"240\"/>",
                    &format!("<w:b/><w:bCs/>{white}{}", sz(32)),
                ) + &para_xml(&run_xml(t.side, &format!("{white}{}", sz(22))), "", &format!("{white}{}", sz(22))),
            }
        }
        "band" => TemplateSpec {
            cx: r(6.0 * 914_400.0),
            cy: r(1.1 * 914_400.0),
            pos_h: pos("margin", Some("center"), None),
            pos_v: pos("paragraph", None, Some(0.0)),
            size_rel_h: Some(NewSizeRel { relative_from: "margin".into(), pct: 100_000.0 }),
            size_rel_v: None,
            wrap: NewWrap::TopAndBottom,
            fill: ShapeFill::Solid(scheme("accent1", &[("lumMod", 20000.0), ("lumOff", 80000.0)])),
            line: None,
            body: body_patch(pad * 2.0, pad, pad * 2.0, pad, BodyAutofit::Shape),
            content: para_xml(
                &run_xml(t.title, &format!("<w:b/><w:bCs/>{acc}{}", sz(26))),
                "<w:spacing w:after=\"60\"/>",
                &format!("<w:b/><w:bCs/>{acc}{}", sz(26)),
            ) + &para_xml(&run_xml(t.body, ""), "", ""),
        },
        "note" => TemplateSpec {
            cx: r(2.0 * 914_400.0),
            cy: r(1.4 * 914_400.0),
            pos_h: pos("margin", Some("right"), None),
            pos_v: pos("paragraph", None, Some(0.0)),
            size_rel_h: None,
            size_rel_v: None,
            wrap: NewWrap::Square,
            fill: ShapeFill::Solid(scheme("accent4", &[("lumMod", 20000.0), ("lumOff", 80000.0)])),
            line: Some(PicLine { w: Some(JsNumber::Num(12700.0)), fill: Some(Nullable::Val(scheme("accent4", &[]))), ..PicLine::default() }),
            body: body_patch(91440.0, 91440.0, 91440.0, 91440.0, BodyAutofit::Shape),
            content: para_xml(&run_xml(t.note, &format!("<w:b/><w:bCs/>{}", sz(20))), "<w:spacing w:after=\"60\"/>", &format!("<w:b/><w:bCs/>{}", sz(20)))
                + &para_xml(&run_xml(t.body, &sz(18)), "", &sz(18)),
        },
        _ => return None,
    })
}

fn box_name(lang: &str) -> &'static str {
    match lang {
        "pt" => "Caixa de Texto",
        "es" => "Cuadro de texto",
        _ => "Text Box",
    }
}

/// `str.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/, …)` and its `name`/`relativeHeight` twins, on a
/// user gallery item: the first match only.
fn replace_attr(xml: &str, tag: Option<&str>, attr: &str, digits_only: bool, new: &str) -> String {
    let pat = format!("{attr}=\"");
    let search_from = match tag {
        Some(t) => match xml.find(t) {
            Some(i) => i,
            None => return xml.to_owned(),
        },
        None => 0,
    };
    let end_of_tag = tag.map(|_| xml[search_from..].find('>').map_or(xml.len(), |g| search_from + g));
    let mut at = search_from;
    while let Some(rel) = xml[at..].find(&pat) {
        let i = at + rel;
        if end_of_tag.is_some_and(|e| i >= e) {
            break;
        }
        let prev_word = i > 0 && xml.as_bytes()[i - 1].is_ascii_alphanumeric();
        if !prev_word {
            let vs = i + pat.len();
            let rest = &xml[vs..];
            let ve = rest.find('"');
            if let Some(ve) = ve {
                let val = &rest[..ve];
                let ok = if digits_only { !val.is_empty() && val.bytes().all(|c| c.is_ascii_digit()) } else { true };
                if ok {
                    return format!("{}{new}{}", &xml[..vs], &xml[vs + ve..]);
                }
            }
        }
        at = i + pat.len();
    }
    xml.to_owned()
}

impl Session {
    // ── the theme's default box ────────────────────────────────────────────────────────────

    /// `themePartPath()`.
    fn theme_part_path(&self) -> Option<String> {
        let rels = self.pkg.part_text("word/_rels/document.xml.rels").ok().flatten()?;
        let mut from = 0usize;
        while let Some(i) = rels[from..].find("<Relationship") {
            let s = from + i;
            let e = rels[s..].find('>').map_or(rels.len(), |g| s + g + 1);
            let tag = &rels[s..e];
            if let Some(ty) = attr_of(tag, "Type")
                && ty.ends_with("/theme")
                && let Some(t) = attr_of(tag, "Target")
            {
                let t = t.strip_prefix('/').unwrap_or(t);
                let t = t.strip_prefix("word/").unwrap_or(t);
                let t = t.strip_prefix("./").unwrap_or(t);
                return Some(format!("word/{t}"));
            }
            from = e;
        }
        None
    }

    /// `textboxDefaults()`: the `spPr` children and the `bodyPr` attributes of `a:txDef`.
    fn textbox_defaults(&self) -> Option<(String, TextboxBodyPatch)> {
        let path = self.theme_part_path()?;
        let x = self.pkg.part_text(&path).ok().flatten()?;
        let od = x.find("<a:objectDefaults>")?;
        let td_s = od + x[od..].find("<a:txDef>")? + "<a:txDef>".len();
        let td_e = td_s + x[td_s..].find("</a:txDef>")?;
        let td = &x[td_s..td_e];
        let sp_pr = match td.find("<a:spPr>") {
            Some(a) => {
                let a = a + "<a:spPr>".len();
                td[a..].find("</a:spPr>").map(|b| td[a..a + b].to_owned()).unwrap_or_default()
            }
            None => String::new(),
        };
        let bp = td.find("<a:bodyPr").map(|a| {
            let rest = &td[a + "<a:bodyPr".len()..];
            let e = rest.find('>').unwrap_or(rest.len());
            rest[..e].trim_end_matches('/').to_owned()
        });
        let bp = bp.unwrap_or_default();
        let at = |n: &str| -> Option<String> {
            let pat = format!(" {n}=\"");
            let i = bp.find(&pat)?;
            let s = i + pat.len();
            let e = bp[s..].find('"')?;
            Some(bp[s..s + e].to_owned())
        };
        let mut body = TextboxBodyPatch::default();
        let ins = |k: &str| at(k).map(|v| uo_jsnum::parse::to_number(&v));
        body.l_ins = ins("lIns");
        body.t_ins = ins("tIns");
        body.r_ins = ins("rIns");
        body.b_ins = ins("bIns");
        body.vert = at("vert").filter(|v| !v.is_empty());
        body.wrap = match at("wrap").as_deref() {
            Some("none") => Some(crate::write::emit::object::BodyWrap::None),
            Some("square") => Some(crate::write::emit::object::BodyWrap::Square),
            _ => None,
        };
        body.anchor = match at("anchor").as_deref() {
            Some("t") => Some(BodyAnchor::T),
            Some("ctr") => Some(BodyAnchor::Ctr),
            Some("b") => Some(BodyAnchor::B),
            _ => None,
        };
        if td.contains("<a:spAutoFit/>") {
            body.autofit = Some(BodyAutofit::Shape);
        }
        Some((sp_pr, body))
    }

    /// `newTextboxXml(o)`: the `w:drawing` of a new box and its id.
    fn new_textbox_xml(
        &mut self,
        cx: f64,
        cy: f64,
        content: &str,
        vert: Option<&str>,
        template: Option<&TemplateSpec>,
        lang: &str,
    ) -> (String, f64) {
        let id = self.next_drawing_id();
        let zs = self.z_values(None);
        let z = zs.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| math::max(m, v)))).map_or(Z_FIRST, |m| m + 1024.0);
        let name = format!("{} {}", box_name(lang), uo_jsnum::format::to_js_string(id));
        if let Some(t) = template {
            let o = NewTextboxOptions {
                id,
                name,
                cx: t.cx,
                cy: t.cy,
                pos_h: Some(t.pos_h.clone()),
                pos_v: Some(t.pos_v.clone()),
                relative_height: z,
                wrap: Some(t.wrap),
                size_rel_h: t.size_rel_h.clone(),
                size_rel_v: t.size_rel_v.clone(),
                fill: Some(t.fill.clone()),
                line: Some(t.line.clone()),
                body: Some(t.body.clone()),
                content: t.content.clone(),
                ..NewTextboxOptions::default()
            };
            return (new_textbox_drawing_xml(&o), id);
        }
        let def = self.textbox_defaults();
        let mut body = def.as_ref().map(|d| d.1.clone()).unwrap_or_default();
        if let Some(v) = vert {
            body.vert = Some(v.to_owned());
        }
        let mut o = NewTextboxOptions { id, name, cx, cy, relative_height: z, body: Some(body), content: content.to_owned(), ..NewTextboxOptions::default() };
        match &def {
            Some((sp, _)) => o.sp_pr_raw = Some(sp.clone()),
            None => {
                o.fill = Some(new_textbox_fill());
                o.line = Some(Some(new_textbox_line()));
            }
        }
        (new_textbox_drawing_xml(&o), id)
    }

    /// `textboxRunFromXml(drawing)`: the run the reader makes of the drawing.
    fn textbox_run_from_xml(&mut self, drawing: &str) -> Option<Run> {
        let xml = format!("<w:p><w:r><w:rPr><w:noProof/></w:rPr>{drawing}</w:r></w:p>");
        let story = self.read_hf_part_story("word/uw-template.xml", &hf_part_xml("header", &xml)).ok().flatten()?;
        let mut run = story.paras.get(0)?.runs.iter().find(|r| r.ext().is_some_and(|e| e.textbox.is_some()))?.clone();
        let tb = run.ext_mut().textbox.as_deref_mut()?;
        obj::set(tb, "edited", |t| t.edited = Some(true));
        Some(run)
    }

    /// `placeAnchorRun(run)`: the new object's run goes at the caret; its path in the story.
    fn place_anchor_run(&mut self, mut run: Run) -> Result<Path, CommandError> {
        if self.has_selection() {
            let pos = self.delete_selection()?;
            self.selection = Selection { anchor: pos, focus: pos };
        }
        let at = self.selection.focus;
        let base = match self.pending_props().cloned() {
            Some(c) => c.props,
            None => self.paras().map_or_else(crate::read::plain::default_run_props, |ps| props_at(ps, at).props),
        };
        let mut props = self.new_run_props_from(&base);
        obj::set(&mut props, "noProof", |p| p.no_proof = Some(true));
        run.props = Arc::new(props);
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        let p = paras.make_mut(at.para as usize).ok_or(CommandError::Position)?;
        let idx = insert_index(p, at.offset as usize).min(p.runs.len());
        p.runs.insert(idx, run);
        self.touch_para(at.para as usize);
        Ok(vec![Step::Para { para: at.para, run: u32::try_from(idx).unwrap_or(u32::MAX) }])
    }

    /// The part of `enterTextbox` the model keeps: the box at `path` (a top-level run of the
    /// context) is edited; `caret` the selection inside.
    fn enter_textbox_at(&mut self, path: &Path, page: u32) -> bool {
        let Some((para, run)) = super::loc::top_level(path) else { return false };
        if !matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. }) {
            return false;
        }
        let all = self.textbox_paths();
        let Some(host) = all.iter().position(|p| p == path) else { return false };
        let host = u32::try_from(host).unwrap_or(u32::MAX);
        let probe = EditContext::Textbox { outer: Box::new(self.ctx.clone()), para, run, host, focus: host, page, saved: self.selection };
        if crate::session::ops::context::story_of(&self.doc, &probe).is_none() {
            return false;
        }
        self.set_float_sel(None);
        self.ctx = probe;
        self.selection = Selection::default();
        self.pending_props = None;
        self.emit(Event::TbMode { active: true });
        true
    }

    /// `insertTextbox(id)`.
    pub(crate) fn insert_textbox(&mut self, id: &str, page: Option<u32>, accent1: Option<&str>, lang: &str, user: Option<&GalleryBox>) -> Res {
        let done = |s: &mut Session| Ok(Outcome::Done(s.change_set()));
        if self.paras().is_none() || self.locked() || matches!(self.ctx, EditContext::Textbox { .. } | EditContext::Note { .. }) {
            return done(self);
        }
        let accent = accent1.unwrap_or("156082");
        let xml = if let (true, Some(u)) = (id.starts_with("user:"), user) {
            let nid = self.next_drawing_id();
            let zs = self.z_values(None);
            let z = zs.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| math::max(m, v)))).map_or(Z_FIRST, |m| m + 1024.0);
            let nid_s = uo_jsnum::format::to_js_string(nid);
            let x = replace_attr(&u.xml, Some("<wp:docPr"), "id", true, &nid_s);
            let x = replace_attr(&x, Some("<wp:docPr"), "name", false, &format!("{} {nid_s}", box_name(lang)));
            replace_attr(&x, None, "relativeHeight", true, &uo_jsnum::format::to_js_string(z))
        } else {
            let Some(spec) = template_spec(id, accent, lang) else { return done(self) };
            self.new_textbox_xml(spec.cx, spec.cy, &spec.content.clone(), None, Some(&spec), lang).0
        };
        let Some(run) = self.textbox_run_from_xml(&xml) else { return done(self) };
        self.begin_step(false, &[]);
        if matches!(self.ctx, EditContext::Cell { .. }) {
            self.exit_cell();
        }
        let path = match self.place_anchor_run(run) {
            Ok(p) => p,
            Err(e) => {
                self.discard_step();
                return Err(e);
            }
        };
        self.changed();
        if let Some(pg) = page
            && self.enter_textbox_at(&path, pg)
        {
            let first = self.paras().and_then(|ps| ps.iter().position(|q| para_length(q) > 0));
            if let Some(f) = first {
                let len = self.paras().and_then(|ps| ps.get(f)).map_or(0, para_length);
                let (f, len) = (u32::try_from(f).unwrap_or(0), u32::try_from(len).unwrap_or(0));
                self.selection = Selection { anchor: DocPosition::new(f, 0), focus: DocPosition::new(f, len) };
            }
        }
        done(self)
    }

    /// `createDrawnTextbox(page, rect, vertical, anchorPos)` (the placement is `PlaceAt`).
    pub(crate) fn create_drawn_textbox(&mut self, page: u32, r: ObjRect, vertical: bool, anchor_para: Option<u32>) -> Res {
        let done = |s: &mut Session| Ok(Outcome::Done(s.change_set()));
        if self.paras().is_none() || self.locked() || matches!(self.ctx, EditContext::Textbox { .. } | EditContext::Note { .. }) {
            return done(self);
        }
        let (xml, _) = self.new_textbox_xml(math::round(r.w * 635.0), math::round(r.h * 635.0), "<w:p/>", vertical.then_some("eaVert"), None, "en");
        let Some(run) = self.textbox_run_from_xml(&xml) else { return done(self) };
        self.begin_step(false, &[]);
        if matches!(self.ctx, EditContext::Cell { .. }) {
            self.exit_cell();
        }
        if let Some(p) = anchor_para {
            let last = self.paras().map_or(0, |ps| ps.len().saturating_sub(1));
            let at = DocPosition::new(p.min(u32::try_from(last).unwrap_or(0)), 0);
            self.selection = Selection { anchor: at, focus: at };
        } else {
            let f = self.selection.focus;
            self.selection = Selection { anchor: f, focus: f };
        }
        let path = match self.place_anchor_run(run) {
            Ok(p) => p,
            Err(e) => {
                self.discard_step();
                return Err(e);
            }
        };
        self.f7.placed = Some(path.clone());
        self.changed();
        self.enter_textbox_at(&path, page);
        done(self)
    }

    // ── chains ─────────────────────────────────────────────────────────────────────────────

    /// `linkRefusal(from, to)`: whether `to` cannot be the box after `from`.
    fn link_refused(&self, from: &Path, to: &Path) -> bool {
        let Some(t) = self.run_ref(to).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()) else { return true };
        if t.raw_drawing_xml.is_none() || to == from {
            return true;
        }
        if t.story.is_some() || t.link.is_some() {
            return true;
        }
        if t.content.flat_paragraphs().iter().any(|q| para_length(q) > 0) || !t.content.tables.is_empty() {
            return true;
        }
        let mine = self.drawing_paths();
        !mine.contains(to) || !mine.contains(from)
    }

    /// `linkTextboxes(from, to)`.
    pub(crate) fn link_textboxes(&mut self, from: &Path, to: &Path) -> bool {
        if self.locked() || self.link_refused(from, to) {
            return false;
        }
        let chain = self.textbox_chain(from);
        let ids: Vec<f64> = self
            .drawing_paths()
            .iter()
            .map(|p| {
                self.run_ref(p).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).map_or(0.0, |t| match (t.story, &t.link) {
                    (Some(s), _) => s.to_number(),
                    (None, Some(l)) => l.id.to_number(),
                    _ => 0.0,
                })
            })
            .collect();
        let max = ids.iter().copied().fold(0.0, math::max);
        let id = match &chain {
            Some(c) => c.first().and_then(|p| self.run_ref(p)).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).and_then(|t| t.story).map_or(f64::NAN, JsNumber::to_number),
            None => max + 1.0,
        };
        let seq = chain.as_ref().map_or(1.0, |c| c.len() as f64);
        if !id.is_finite() {
            return false;
        }
        self.begin_step(false, &[]);
        if chain.is_none() {
            let raw = self.run_ref(from).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.as_deref().map(str::to_owned));
            if let Some(raw) = raw
                && let Ok(x) = patch_shape(&raw, &ShapeChange { story: Some(StoryChange::Txbx { id: Some(id), content: None }), ..ShapeChange::default() })
            {
                self.apply_drawing_xml(from, x, false);
            }
        }
        if let Some(run) = self.run_mut(to)
            && let Some(t) = run.ext_mut().textbox.as_deref_mut()
        {
            t.content = Story::new();
            t.view = TextboxView::ParasOnly;
        }
        let raw = self.run_ref(to).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.as_deref().map(str::to_owned));
        if let Some(raw) = raw
            && let Ok(x) = patch_shape(&raw, &ShapeChange { story: Some(StoryChange::Linked { id, seq }), ..ShapeChange::default() })
        {
            self.apply_drawing_xml(to, x, false);
        }
        self.after_picture_edit();
        true
    }

    /// `breakTextboxLink()`.
    pub(crate) fn break_textbox_link(&mut self) -> bool {
        let Some(path) = self.draw_path() else { return false };
        if self.run_ref(&path).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).is_none() {
            return false;
        }
        let Some(chain) = self.textbox_chain(&path) else { return false };
        if self.locked() {
            return false;
        }
        let Some(k) = chain.iter().position(|p| *p == path) else { return false };
        if k == chain.len() - 1 {
            return false;
        }
        self.begin_step(false, &[]);
        for t in chain.iter().skip(k + 1) {
            let raw = self.run_ref(t).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).and_then(|b| b.raw_drawing_xml.as_deref().map(str::to_owned));
            let Some(raw) = raw else { continue };
            let pid = self.doc.ids.para();
            if let Some(run) = self.run_mut(t)
                && let Some(b) = run.ext_mut().textbox.as_deref_mut()
            {
                let mut s = Story::new();
                s.push_paragraph(crate::model::para::Paragraph { id: pid, ..Default::default() });
                b.content = s;
                b.view = TextboxView::ParasOnly;
            }
            if let Ok(x) = patch_shape(&raw, &ShapeChange { story: Some(StoryChange::Txbx { id: None, content: Some("<w:p/>".into()) }), ..ShapeChange::default() }) {
                self.apply_drawing_xml(t, x, false);
            }
        }
        if k == 0 {
            let first = &chain[0];
            let raw = self.run_ref(first).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref()).and_then(|b| b.raw_drawing_xml.as_deref().map(str::to_owned));
            if let Some(raw) = raw
                && let Ok(x) = patch_shape(&raw, &ShapeChange { story: Some(StoryChange::Txbx { id: None, content: None }), ..ShapeChange::default() })
            {
                self.apply_drawing_xml(first, x, false);
            }
        }
        self.after_picture_edit();
        true
    }

    // ── the document's default box ─────────────────────────────────────────────────────────

    /// `setAsDefaultTextbox()`: the box's fill, outline and body go to the theme's `a:txDef`.
    pub(crate) fn set_as_default_textbox(&mut self) -> bool {
        let Some(path) = self.draw_path() else { return false };
        let Some(run) = self.run_ref(&path) else { return false };
        let Some(raw) = run.ext().and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.clone()) else { return false };
        let Some(tpath) = self.theme_part_path() else { return false };
        let Some(theme) = self.pkg.part_text(&tpath).ok().flatten().map(|c| c.into_owned()) else { return false };
        let Some(super::Fmt::Shape(sf)) = format_of(run) else { return false };
        let Some(b) = sf.body.clone() else { return false };
        if self.locked() {
            return false;
        }
        let sp = between(&raw, "<wps:spPr", "</wps:spPr>").unwrap_or_default();
        let sp_in = strip_geometry(&strip_xfrm(&sp));
        let d = crate::write::emit::object::default_textbox_body();
        let mut ins = String::new();
        for (k, v, dv) in [("lIns", b.l_ins, d.l_ins), ("tIns", b.t_ins, d.t_ins), ("rIns", b.r_ins, d.r_ins), ("bIns", b.b_ins, d.b_ins)] {
            if v != dv {
                ins.push_str(&format!(" {k}=\"{}\"", uo_jsnum::format::to_js_string(v)));
            }
        }
        let wrap = match b.wrap {
            crate::write::emit::object::BodyWrap::Square => "square",
            crate::write::emit::object::BodyWrap::None => "none",
        };
        let anchor = match b.anchor {
            BodyAnchor::T => "t",
            BodyAnchor::Ctr => "ctr",
            BodyAnchor::B => "b",
        };
        let attrs = format!(
            "vert=\"{}\" wrap=\"{wrap}\"{ins}{} rtlCol=\"0\"",
            b.vert,
            if anchor == "t" { String::new() } else { format!(" anchor=\"{anchor}\"") }
        );
        let body = if b.autofit == BodyAutofit::Shape { format!("<a:bodyPr {attrs}><a:spAutoFit/></a:bodyPr>") } else { format!("<a:bodyPr {attrs}/>") };
        let tx_def = format!("<a:txDef><a:spPr>{sp_in}</a:spPr>{body}<a:lstStyle/></a:txDef>");
        let next = if theme.contains("<a:objectDefaults/>") {
            theme.replacen("<a:objectDefaults/>", &format!("<a:objectDefaults>{tx_def}</a:objectDefaults>"), 1)
        } else if let (Some(i), Some(j)) = (theme.find("<a:txDef>"), theme.find("</a:txDef>")) {
            format!("{}{tx_def}{}", &theme[..i], &theme[j + "</a:txDef>".len()..])
        } else if theme.contains("<a:objectDefaults>") {
            theme.replacen("</a:objectDefaults>", &format!("{tx_def}</a:objectDefaults>"), 1)
        } else {
            theme.replacen("</a:themeElements>", &format!("</a:themeElements><a:objectDefaults>{tx_def}</a:objectDefaults>"), 1)
        };
        self.pkg.set_part_text(tpath, next);
        self.changed();
        true
    }

    // ── alignObjects ───────────────────────────────────────────────────────────────────────

    /// `alignObjects(kind)` with the facts of the selected floating objects (the selected object
    /// first, then the others).
    pub(crate) fn align_objects(&mut self, kind: AlignKind, objs: &[ObjFacts], cx: &mut ExecCx<'_>) -> Res {
        let n = objs.len();
        if n < 2 {
            if !matches!(kind, AlignKind::DistributeH | AlignKind::DistributeV) && !self.draw_is_image() {
                let k = kind;
                return self.exec_pictures(PictureCmd::Align { kind: k, relative_to: None, rects: Vec::new() }, cx);
            }
            return Ok(Outcome::Done(self.change_set()));
        }
        if self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let mut paths: Vec<Path> = Vec::new();
        if let Some(p) = self.sel_path() {
            paths.push(p);
        }
        for p in self.f7.multi.clone() {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        if paths.len() != n {
            return Ok(Outcome::Done(self.change_set()));
        }
        let rect = |o: &ObjFacts| o.rect.unwrap_or_default();
        let l = objs.iter().map(|o| rect(o).x).fold(f64::INFINITY, math::min);
        let r = objs.iter().map(|o| rect(o).x + rect(o).w).fold(f64::NEG_INFINITY, math::max);
        let t = objs.iter().map(|o| rect(o).y).fold(f64::INFINITY, math::min);
        let b = objs.iter().map(|o| rect(o).y + rect(o).h).fold(f64::NEG_INFINITY, math::max);
        let mut moves: Vec<(usize, f64, f64)> = Vec::new();
        if matches!(kind, AlignKind::DistributeH | AlignKind::DistributeV) {
            let hor = kind == AlignKind::DistributeH;
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|a, c| {
                let (ra, rc) = (rect(&objs[*a]), rect(&objs[*c]));
                let (va, vc) = if hor { (ra.x, rc.x) } else { (ra.y, rc.y) };
                va.partial_cmp(&vc).unwrap_or(std::cmp::Ordering::Equal)
            });
            let total: f64 = order.iter().map(|i| if hor { rect(&objs[*i]).w } else { rect(&objs[*i]).h }).sum();
            let gap = ((if hor { r - l } else { b - t }) - total) / math::max(1.0, (n - 1) as f64);
            let mut at = if hor { l } else { t };
            for i in order {
                let rc = rect(&objs[i]);
                if hor {
                    moves.push((i, math::round(at - rc.x), 0.0));
                } else {
                    moves.push((i, 0.0, math::round(at - rc.y)));
                }
                at += (if hor { rc.w } else { rc.h }) + gap;
            }
        } else {
            for (i, o) in objs.iter().enumerate() {
                let rc = rect(o);
                let dx = match kind {
                    AlignKind::Left => l - rc.x,
                    AlignKind::Right => r - (rc.x + rc.w),
                    AlignKind::Center => (l + r) / 2.0 - (rc.x + rc.w / 2.0),
                    _ => 0.0,
                };
                let dy = match kind {
                    AlignKind::Top => t - rc.y,
                    AlignKind::Bottom => b - (rc.y + rc.h),
                    AlignKind::Middle => (t + b) / 2.0 - (rc.y + rc.h / 2.0),
                    _ => 0.0,
                };
                moves.push((i, math::round(dx), math::round(dy)));
            }
        }
        self.begin_step(false, &[]);
        for (i, dx, dy) in moves {
            if (dx == 0.0 && dy == 0.0) || !dx.is_finite() || !dy.is_finite() {
                continue;
            }
            self.move_float_by(&paths[i], &objs[i], dx, dy);
        }
        self.changed();
        Ok(Outcome::Done(self.change_set()))
    }

    /// `moveFloatBy(h, dx, dy)` with a frame: the positions keep their references.
    fn move_float_by(&mut self, path: &Path, o: &ObjFacts, dx: f64, dy: f64) {
        let (Some(fr), Some(rc)) = (o.frame, o.rect) else { return };
        let Some(f) = self.run_ref(path).and_then(format_of) else { return };
        let b = f.base();
        let ph = super::super::pictures::pos_for(b.pos_h.as_ref(), true, rc.x + dx, &fr);
        let pv = super::super::pictures::pos_for(b.pos_v.as_ref(), false, rc.y + dy, &fr);
        let ch = ShapeChange {
            pic: PictureChange {
                pos_h: if dx != 0.0 { ph } else { None },
                pos_v: if dy != 0.0 { pv } else { None },
                ..PictureChange::default()
            },
            ..ShapeChange::default()
        };
        if !change_is_finite(&ch) {
            return;
        }
        if let Some(x) = self.patch_drawing_pub(path, &ch) {
            self.apply_drawing_xml(path, x, false);
        }
    }

    /// `reenterTextbox(te)`: after an undo/redo of a step taken inside a box, back into it (the
    /// box of the focus, else the host), with the selection of the step.
    pub(crate) fn reenter_textbox(&mut self, host: u32, focus: u32, page: u32, sel: Selection) {
        let all = self.textbox_paths();
        let Some(path) = all.get(focus as usize).or_else(|| all.get(host as usize)).cloned() else { return };
        // the box holding the text is the head of its chain
        let head = self.textbox_chain(&path).and_then(|c| c.first().cloned()).unwrap_or_else(|| path.clone());
        let Some((para, run)) = super::loc::top_level(&head) else { return };
        if !matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. }) {
            return;
        }
        let hidx = all.iter().position(|p| *p == head).and_then(|i| u32::try_from(i).ok()).unwrap_or(host);
        let fidx = all.iter().position(|p| *p == path).and_then(|i| u32::try_from(i).ok()).unwrap_or(hidx);
        let probe = EditContext::Textbox { outer: Box::new(self.ctx.clone()), para, run, host: hidx, focus: fidx, page, saved: self.selection };
        if crate::session::ops::context::story_of(&self.doc, &probe).is_none() {
            return;
        }
        self.set_float_sel(None);
        self.ctx = probe;
        self.pending_props = None;
        self.emit(Event::TbMode { active: true });
        self.selection = crate::session::ops::context::clamp_selection(self.paras(), sel);
    }
}

fn attr_of<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let mut from = 0usize;
    while let Some(i) = tag[from..].find(&pat) {
        let s = from + i;
        let prev_ok = s == 0 || !tag.as_bytes()[s - 1].is_ascii_alphanumeric();
        if prev_ok {
            let v = s + pat.len();
            let e = tag[v..].find('"')?;
            return Some(&tag[v..v + e]);
        }
        from = s + pat.len();
    }
    None
}

/// The inside of the first `<open ...>…close` element.
fn between(s: &str, open: &str, close: &str) -> Option<String> {
    let i = s.find(open)?;
    let gt = i + s[i..].find('>')? + 1;
    let j = gt + s[gt..].find(close)?;
    Some(s[gt..j].to_owned())
}

/// `.replace(/<a:xfrm\b[\s\S]*?<\/a:xfrm>|<a:xfrm\/>/, '')`.
fn strip_xfrm(s: &str) -> String {
    if let Some(i) = s.find("<a:xfrm") {
        let after = &s[i + 7..];
        if after.starts_with('/') && after.starts_with("/>") {
            return format!("{}{}", &s[..i], &s[i + 9..]);
        }
        if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
            && let Some(j) = s[i..].find("</a:xfrm>")
        {
            return format!("{}{}", &s[..i], &s[i + j + 9..]);
        }
    }
    s.to_owned()
}

/// `.replace(/<a:prstGeom\b[\s\S]*?<\/a:prstGeom>|<a:prstGeom\b[^>]*\/>|<a:custGeom\b[\s\S]*?<\/a:custGeom>/, '')`.
fn strip_geometry(s: &str) -> String {
    let cands = [("<a:prstGeom", "</a:prstGeom>"), ("<a:custGeom", "</a:custGeom>")];
    let mut best: Option<(usize, usize)> = None;
    for (open, close) in cands {
        if let Some(i) = s.find(open) {
            let e = if let Some(j) = s[i..].find(close) {
                i + j + close.len()
            } else {
                continue;
            };
            let gt = s[i..].find('>').map(|g| i + g);
            // the self-closing form ends at its own `/>`
            let end = match gt {
                Some(g) if s[..g].ends_with('/') && g < e => g + 1,
                _ => e,
            };
            if best.is_none_or(|(bi, _)| i < bi) {
                best = Some((i, end));
            }
        }
    }
    match best {
        Some((i, e)) => format!("{}{}", &s[..i], &s[e..]),
        None => s.to_owned(),
    }
}

#[allow(dead_code)]
fn _keep(_: Nullable<()>, _: PicPos, _: DrawingCmd, _: &str, _: normalize_range_alias) {}
#[allow(non_camel_case_types)]
type normalize_range_alias = ();
#[allow(dead_code)]
fn _nr() {
    let _ = normalize_range;
}
