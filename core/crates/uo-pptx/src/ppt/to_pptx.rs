//! The binary model (reader) → an in-memory PresentationML package (`ppt/to-pptx.ts`).
//!
//! It generates a minimal but valid package: `[Content_Types].xml`, `_rels/.rels`,
//! `ppt/presentation.xml` (+rels), a slideMaster + "blank" layout(s) + theme per master of the .ppt
//! actually used, `ppt/slides/slideN.xml` (+rels) and `ppt/media/*` for the pictures.
//!
//! Principles:
//! - everything EXPLICIT in the shape (position, fill, line, text with size/colour/font/bullet): the
//!   inheritance of the .ppt (master styles, colour scheme) is resolved here, not delegated to the
//!   reader's cascade;
//! - shape ids = the .ppt's spid (the PPT10 animations point at them);
//! - master → `<p:sldMaster>` with the decorations (shapes without a placeholder), the background
//!   and the txStyles; slides that hide the master's objects use a layout with showMasterSp="0".
//!
//! Geometry: master units (576/inch) × 1587.5 = EMU. Groups keep the .ppt's child space (FSPGR)
//! scaled by the same factor (chOff/chExt).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::anim::{AnimCtx, build_timing_xml};
use super::context::{ConvCtx, ConvertOptions, OMap, PartCtx, PartHost, PartRels, TEXT_BUDGET, default_scheme, default_scheme_at};
use super::error::PptError;
use super::notes::{notes_for, notes_master_xml, notes_slide_xml};
use super::oa::OaShape;
use super::reader::{PptDocument, PptSlide, SlideKind};
use super::shapeconv::{ShapeOpts, background_color, background_xml, placeholder_type, shape_xml};
use super::textconv::{TextConvCtx, TextHost, emu, levels_xml, resolve_level};
use super::transition::transition_xml;
use super::xmlw::{Attrs, XML_DECL, el, el_a, n, ns, ns_attrs, s};

const SPTREE_HEAD: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

const CLR_MAP: &str = "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>";

fn sld_sz_type(t: u16) -> Option<&'static str> {
    ["screen4x3", "letter", "A4", "35mm", "overhead", "banner"].get(usize::from(t)).copied()
}

/// Theme generated from the .ppt's colour scheme (8 colours: background, text, shadow, title, fill,
/// accent, hyperlink, followed hyperlink) and from the fonts of the title/body master styles.
fn theme_xml(name: &str, scheme: &[String], major: &str, minor: &str) -> String {
    let c = |i: usize| -> String { scheme.get(i).cloned().or_else(|| default_scheme_at(i).map(str::to_owned)).unwrap_or_default() };
    let clr = |tag: &str, v: String| -> String { el(&format!("a:{tag}"), vec![], &el("a:srgbClr", vec![("val", s(v))], "")) };
    let fonts = |f: &str| -> String { el("a:latin", vec![("typeface", s(f))], "") + "<a:ea typeface=\"\"/><a:cs typeface=\"\"/>" };
    let ph_solid = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let ln_list: String = [6350, 12700, 19050]
        .iter()
        .map(|w| {
            el(
                "a:ln",
                vec![("w", n(*w)), ("cap", s("flat")), ("cmpd", s("sng")), ("algn", s("ctr"))],
                &format!("{ph_solid}<a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/>"),
            )
        })
        .collect();
    format!(
        "{XML_DECL}{}",
        el_a(
            "a:theme",
            &Attrs::of(vec![("xmlns:a", s(ns::A)), ("name", s(name))]),
            &(el(
                "a:themeElements",
                vec![],
                &(el(
                    "a:clrScheme",
                    vec![("name", s(name))],
                    &(clr("dk1", c(1))
                        + &clr("lt1", c(0))
                        + &clr("dk2", c(3))
                        + &clr("lt2", c(2))
                        + &clr("accent1", c(4))
                        + &clr("accent2", c(5))
                        + &clr("accent3", c(6))
                        + &clr("accent4", c(7))
                        + &clr("accent5", c(4))
                        + &clr("accent6", c(5))
                        + &clr("hlink", c(6))
                        + &clr("folHlink", c(7)))
                ) + &el(
                    "a:fontScheme",
                    vec![("name", s(name))],
                    &(el("a:majorFont", vec![], &fonts(major)) + &el("a:minorFont", vec![], &fonts(minor)))
                ) + &el(
                    "a:fmtScheme",
                    vec![("name", s(name))],
                    &(el("a:fillStyleLst", vec![], &format!("{ph_solid}{ph_solid}{ph_solid}"))
                        + &el("a:lnStyleLst", vec![], &ln_list)
                        + &el("a:effectStyleLst", vec![], &"<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3))
                        + &el("a:bgFillStyleLst", vec![], &format!("{ph_solid}{ph_solid}{ph_solid}"))),
                )),
            ) + "<a:objectDefaults/><a:extraClrSchemeLst/>")
        )
    )
}

/// `/<a:TAG>\s*<a:(?:srgbClr val|sysClr[^>]*lastClr)="([0-9A-Fa-f]{6})"/` first match, upper-cased.
fn pick_theme_color(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<a:{tag}>");
    let mut from = 0usize;
    while let Some(k) = xml.get(from..).and_then(|r| r.find(&open)) {
        let st = from + k + open.len();
        from = st;
        let rest = xml.get(st..).unwrap_or("");
        let rest = rest.trim_start_matches(uo_jsnum::text::is_js_space);
        let Some(after) = rest.strip_prefix("<a:") else { continue };
        let hex6 = |t: &str| -> Option<String> {
            let h = t.get(..6)?;
            if h.bytes().all(|c| c.is_ascii_hexdigit()) && t.get(6..)?.starts_with('"') { Some(h.to_uppercase()) } else { None }
        };
        if let Some(v) = after.strip_prefix("srgbClr val=\"").and_then(hex6) {
            return Some(v);
        }
        if let Some(sys) = after.strip_prefix("sysClr") {
            // `[^>]*` greedy: the last `lastClr="` of the tag first
            let span_end = sys.find('>').unwrap_or(sys.len());
            let span = sys.get(..span_end).unwrap_or("");
            let mut cands: Vec<usize> = span.match_indices("lastClr=\"").map(|(i, _)| i).collect();
            cands.reverse();
            for ci in cands {
                if let Some(v) = sys.get(ci + 9..).and_then(hex6) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The round-trip theme is only used if the base colours (dk1, lt1, dk2, lt2, accent1, accent2) match
/// the master's scheme — otherwise the .ppt was edited afterwards by an old version and the binary
/// scheme stands.
fn theme_matches_scheme(xml: &str, scheme: &[String]) -> bool {
    [("dk1", 1usize), ("lt1", 0), ("dk2", 3), ("lt2", 2), ("accent1", 4), ("accent2", 5)]
        .iter()
        .all(|(tag, i)| pick_theme_color(xml, tag) == scheme.get(*i).map(|c| c.to_uppercase()))
}

/// The master (main or title) by the slideId that masterIdRef references.
fn master_by_id(doc: &PptDocument<'_>, id: u32) -> Option<usize> {
    doc.masters.iter().position(|m| m.slide_id == id)
}

/// The main master (for text styles) of any master.
fn main_master_of(doc: &PptDocument<'_>, m: Option<usize>) -> Option<usize> {
    let first_main = || doc.masters.iter().position(|x| x.kind == SlideKind::MainMaster);
    let Some(mi) = m else { return first_main() };
    let mm = doc.masters.get(mi)?;
    if mm.kind == SlideKind::MainMaster {
        return Some(mi);
    }
    master_by_id(doc, mm.master_id_ref).or_else(first_main)
}

/// The shapes of the root group of a drawing.
fn root_children<'a>(doc: &'a PptDocument<'a>, s: &PptSlide<'a>) -> Vec<&'a OaShape<'a>> {
    match s.drawing.root.and_then(|r| doc.shapes.get(r)) {
        Some(root) => doc.shapes.kids(root),
        None => Vec::new(),
    }
}

fn background_of<'a>(doc: &'a PptDocument<'a>, s: &PptSlide<'a>) -> Option<&'a OaShape<'a>> {
    s.drawing.background.and_then(|b| doc.shapes.get(b))
}

/// Header/footer of the 97-2003 model: slides have no shapes of their own for date/number/footer —
/// the MASTER's placeholders appear according to the HeadersFootersContainer (of the slide or, in
/// its absence, of the document), except in title slides with fOmitTitlePlace. 2007+ files already
/// carry the shapes in the slide: it only injects what the slide lacks. Text: meta characters →
/// number/date/footer.
fn header_footer_shapes<'a>(
    doc: &'a PptDocument<'a>,
    sl: &PptSlide<'a>,
    m: Option<usize>,
    pc: &mut PartCtx<'_, 'a>,
) -> Result<String, PptError> {
    let Some(hf) = sl.headers_footers.as_ref().or(doc.slide_headers_footers.as_ref()) else { return Ok(String::new()) };
    let Some(m) = m.and_then(|i| doc.masters.get(i)) else { return Ok(String::new()) };
    let mut want: HashSet<&str> = HashSet::new();
    if hf.has_date {
        want.insert("dt");
    }
    if hf.has_slide_number {
        want.insert("sldNum");
    }
    if hf.has_footer {
        want.insert("ftr");
    }
    if want.is_empty() {
        return Ok(String::new());
    }
    if doc.omit_title_place && sl.geom == 0 {
        return Ok(String::new()); // SL_TitleSlide
    }
    let mut own: HashSet<&str> = HashSet::new();
    for c in root_children(doc, sl) {
        if let Some(t) = placeholder_type(c) {
            own.insert(t);
        }
    }
    let mut hf_text = super::textconv::HfText::default();
    if hf.footer.as_deref().is_some_and(|x| !x.is_empty()) {
        hf_text.footer = hf.footer.clone();
    }
    if hf.header.as_deref().is_some_and(|x| !x.is_empty()) {
        hf_text.header = hf.header.clone();
    }
    // FIXED date (fHasUserDate): the user's text, even empty — not today's date
    if hf.has_user_date && !hf.has_today_date {
        hf_text.user_date = Some(hf.user_date.clone().unwrap_or_default());
    }
    pc.text.hf = Some(hf_text);
    let mut xml = String::new();
    for c in root_children(doc, m) {
        let t = placeholder_type(c);
        if let Some(t) = t
            && want.contains(t)
            && !own.contains(t)
        {
            xml.push_str(&shape_xml(c, pc, ShapeOpts::default())?);
        }
    }
    Ok(xml)
}

fn rels_path(part: &str) -> String {
    let i = part.rfind('/').unwrap_or(0);
    format!("{}/_rels/{}.rels", part.get(..i).unwrap_or(""), part.get(i + 1..).unwrap_or(""))
}

struct PartOut {
    path: String,
    xml: String,
    rels: PartRels,
}

fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "pict" => "image/pict",
        _ => "application/octet-stream",
    }
}

/// Converts the binary model into OPC parts (path → bytes), in the order of writing.
pub fn ppt_to_pptx_parts<'a>(
    doc: &'a PptDocument<'a>,
    opts: &ConvertOptions,
    today: (i32, u32, u32),
) -> Result<OMap<String, Vec<u8>>, PptError> {
    let mut conv = ConvCtx {
        doc,
        media: OMap::new(),
        parts: OMap::new(),
        media_exts: OMap::new(),
        use_round_trip: opts.use_round_trip,
        overrides: OMap::new(),
        diagrams: 0,
        charts: 0,
        ts_ppt_charts: opts.ts_ppt_charts,
        text_budget: TEXT_BUDGET,
        complex_media: OMap::new(),
        recolor_media: OMap::new(),
        rt: super::roundtrip::RoundTrip::default(),
        today,
        // the counter of the a:fld GUIDs starts at 1 for every conversion (see `textconv::field_id`)
        field_seq: Rc::new(std::cell::Cell::new(0)),
    };
    let mut out: OMap<String, Vec<u8>> = OMap::new();
    let mut put = |path: &str, xml: String| out.set(path.to_owned(), xml.into_bytes());
    let font_of = |idx: Option<i32>| -> Option<String> { doc.fonts.get(usize::try_from(idx?).ok()?)?.as_ref().map(|f| f.name.clone()) };

    // masters actually used (in the order of the document)
    let used: HashSet<u32> = doc.slides.iter().map(|sl| sl.master_id_ref).collect();
    let mut masters: Vec<usize> =
        (0..doc.masters.len()).filter(|&i| doc.masters.get(i).is_some_and(|m| used.contains(&m.slide_id))).collect();
    if masters.is_empty() && !doc.masters.is_empty() {
        masters = vec![0];
    }
    let mut master_index: HashMap<u32, usize> = HashMap::new();
    for (i, &mi) in masters.iter().enumerate() {
        if let Some(m) = doc.masters.get(mi) {
            master_index.insert(m.slide_id, i + 1);
        }
    }
    let mut needs_no_master_layout: HashSet<usize> = HashSet::new();
    for sl in &doc.slides {
        if sl.flags & 1 == 0 {
            needs_no_master_layout.insert(master_index.get(&sl.master_id_ref).copied().unwrap_or(1));
        }
    }

    // ── masters, layouts, themes
    let mut layout_counter = 0usize;
    let mut layout_id_seed = 2_147_483_648f64 + f64::from(u32::try_from(masters.len()).unwrap_or(u32::MAX)) + 1.0;
    // `${master}:${m|n}` → layout number (n = without the master's objects)
    let mut layout_for: HashMap<(usize, bool), usize> = HashMap::new();
    let mut master_xmls: Vec<PartOut> = Vec::new();
    let mut layout_xmls: Vec<PartOut> = Vec::new();
    let mut theme_xmls: Vec<(String, String)> = Vec::new();
    for (mi_pos, &mi) in masters.iter().enumerate() {
        let Some(m) = doc.masters.get(mi) else { continue };
        let k = mi_pos + 1;
        let main_i = main_master_of(doc, Some(mi));
        let main = main_i.and_then(|i| doc.masters.get(i));
        let scheme = m.color_scheme.clone().or_else(|| main.and_then(|x| x.color_scheme.clone())).unwrap_or_else(default_scheme);
        let mut pc = PartCtx::new(&mut conv, scheme.clone(), main.and_then(|x| x.master_styles.clone()), m.outline.clone(), None);
        let bg_shape = background_of(doc, m).or_else(|| main.and_then(|x| background_of(doc, x)));
        pc.bg_color = background_color(bg_shape, &pc);
        pc.is_master = true;
        // decorations of the main master under a title master with fMasterObjects
        let mut tree = String::new();
        if m.kind == SlideKind::TitleMaster
            && m.flags & 1 != 0
            && let Some(mn) = main
            && main_i != Some(mi)
        {
            for c in root_children(doc, mn) {
                tree.push_str(&shape_xml(c, &mut pc, ShapeOpts { skip_placeholders: true, in_group: false })?);
            }
        }
        for c in root_children(doc, m) {
            tree.push_str(&shape_xml(c, &mut pc, ShapeOpts::default())?);
        }
        let bg = background_xml(bg_shape, &mut pc);
        // layouts of this master
        let mut lids: Vec<(f64, usize)> = Vec::new();
        let mk_layout = |no_master: bool,
                         layout_counter: &mut usize,
                         layout_id_seed: &mut f64,
                         layout_xmls: &mut Vec<PartOut>,
                         layout_for: &mut HashMap<(usize, bool), usize>,
                         lids: &mut Vec<(f64, usize)>| {
            *layout_counter += 1;
            let mut lrels = PartRels::new();
            lrels.add("slideMaster", &format!("../slideMasters/slideMaster{k}.xml"), false);
            let mut a = ns_attrs();
            a.push(("type", s("blank")));
            a.push(("preserve", n(1)));
            a.push(("showMasterSp", if no_master { n(0) } else { None }));
            let lxml = format!(
                "{XML_DECL}{}",
                el(
                    "p:sldLayout",
                    a,
                    &(el(
                        "p:cSld",
                        vec![("name", s(if no_master { "Em branco (sem objetos do mestre)" } else { "Em branco" }))],
                        &el("p:spTree", vec![], SPTREE_HEAD)
                    ) + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>")
                )
            );
            layout_xmls.push(PartOut { path: format!("ppt/slideLayouts/slideLayout{}.xml", *layout_counter), xml: lxml, rels: lrels });
            layout_for.insert((k, !no_master), *layout_counter);
            lids.push((*layout_id_seed, *layout_counter));
            *layout_id_seed += 1.0;
        };
        mk_layout(false, &mut layout_counter, &mut layout_id_seed, &mut layout_xmls, &mut layout_for, &mut lids);
        if needs_no_master_layout.contains(&k) {
            mk_layout(true, &mut layout_counter, &mut layout_id_seed, &mut layout_xmls, &mut layout_for, &mut lids);
        }
        let mut lid_xml = String::new();
        for (id, num) in &lids {
            let rid = pc.rels.add("slideLayout", &format!("../slideLayouts/slideLayout{num}.xml"), false);
            lid_xml.push_str(&el("p:sldLayoutId", vec![("id", n(*id)), ("r:id", s(rid))], ""));
        }
        pc.rels.add("theme", &format!("../theme/theme{k}.xml"), false);
        let tx_styles = {
            let mut host = PartHost { conv: &mut *pc.conv, rels: &mut pc.rels };
            let mut h: Option<&mut dyn TextHost> = Some(&mut host);
            el(
                "p:txStyles",
                vec![],
                &(levels_xml("p:titleStyle", &pc.text, 0, &mut h)
                    + &levels_xml("p:bodyStyle", &pc.text, 1, &mut h)
                    + &levels_xml("p:otherStyle", &pc.text, 4, &mut h)),
            )
        };
        let mxml = format!(
            "{XML_DECL}{}",
            el(
                "p:sldMaster",
                ns_attrs(),
                &(el(
                    "p:cSld",
                    vec![("name", m.name.as_deref().and_then(s))],
                    &(bg + &el("p:spTree", vec![], &format!("{SPTREE_HEAD}{tree}")))
                ) + CLR_MAP
                    + &el("p:sldLayoutIdLst", vec![], &lid_xml)
                    + &tx_styles)
            )
        );
        let ctx_major = resolve_level(&pc.text, 0, 0).cf.font;
        let ctx_minor = resolve_level(&pc.text, 1, 0).cf.font;
        let major = font_of(ctx_major).unwrap_or_else(|| "Calibri".to_owned());
        let minor = font_of(ctx_minor).unwrap_or_else(|| "Calibri".to_owned());
        let mrels = std::mem::take(&mut pc.rels);
        drop(pc);
        master_xmls.push(PartOut { path: format!("ppt/slideMasters/slideMaster{k}.xml"), xml: mxml, rels: mrels });
        // original theme of the round trip (PowerPoint 2007+) when the base colours match the scheme
        let rt = if conv.use_round_trip {
            let blob = m.round_trip_theme.or_else(|| main.and_then(|x| x.round_trip_theme));
            conv.rt.round_trip_theme_xml(blob)
        } else {
            None
        };
        let txml = match rt {
            Some(r) if theme_matches_scheme(&r, &scheme) => (*r).clone(),
            _ => theme_xml(&m.name.clone().unwrap_or_else(|| format!("Tema {k}")), &scheme, &major, &minor),
        };
        theme_xmls.push((format!("ppt/theme/theme{k}.xml"), txml));
    }

    // ── slides (+ notes)
    struct SlideOut {
        part: PartOut,
        id: f64,
    }
    let mut slide_xmls: Vec<SlideOut> = Vec::new();
    let mut notes_xmls: Vec<PartOut> = Vec::new();
    let mut used_sld_ids: HashSet<i64> = HashSet::new();
    let first_slide_number = f64::from(doc.first_slide_number);
    for (si, sl) in doc.slides.iter().enumerate() {
        let m_i = master_by_id(doc, sl.master_id_ref).or_else(|| masters.first().copied());
        let m = m_i.and_then(|i| doc.masters.get(i));
        let main_i = main_master_of(doc, m_i);
        let main = main_i.and_then(|i| doc.masters.get(i));
        let k = m.and_then(|mm| master_index.get(&mm.slide_id).copied()).unwrap_or(1);
        // fMasterScheme (bit 1): the master's scheme; otherwise the slide's own
        let scheme = if sl.flags & 2 != 0 {
            m.and_then(|x| x.color_scheme.clone())
                .or_else(|| main.and_then(|x| x.color_scheme.clone()))
                .or_else(|| sl.color_scheme.clone())
                .unwrap_or_else(default_scheme)
        } else {
            sl.color_scheme.clone().or_else(|| m.and_then(|x| x.color_scheme.clone())).unwrap_or_else(default_scheme)
        };
        #[allow(clippy::cast_precision_loss)]
        let slide_no = first_slide_number + si as f64;
        let mut pc =
            PartCtx::new(&mut conv, scheme.clone(), main.and_then(|x| x.master_styles.clone()), sl.outline.clone(), Some(slide_no));
        pc.slide_id = Some(sl.slide_id);
        // fMasterObjects (bit 0) off → a layout that hides the master's decorations
        let layout_n = layout_for.get(&(k, sl.flags & 1 != 0)).or_else(|| layout_for.get(&(k, true))).copied().unwrap_or(1);
        pc.rels.add("slideLayout", &format!("../slideLayouts/slideLayout{layout_n}.xml"), false);
        // fMasterBackground (bit 2) off → own background
        let own_bg = if sl.flags & 4 == 0 { background_of(doc, sl) } else { None };
        let bg_shape = own_bg.or_else(|| m.and_then(|x| background_of(doc, x)));
        pc.bg_color = background_color(bg_shape, &pc);
        let mut tree = String::new();
        for c in root_children(doc, sl) {
            tree.push_str(&shape_xml(c, &mut pc, ShapeOpts::default())?);
        }
        tree = header_footer_shapes(doc, sl, m_i, &mut pc)? + &tree;
        let bg = if own_bg.is_some() { background_xml(own_bg, &mut pc) } else { String::new() };
        let mut rtc = std::mem::take(&mut pc.conv.rt);
        let timing = {
            let actx = AnimCtx { texts: &pc.texts, spids: &pc.spids, ooxml_ids: &pc.ooxml_ids };
            build_timing_xml(sl, &actx, &mut rtc)
        };
        pc.conv.rt = rtc;
        let mut a = ns_attrs();
        a.push(("show", if sl.transition.is_some_and(|t| t.hidden) { n(0) } else { None }));
        a.push(("showMasterSp", if sl.flags & 1 != 0 { None } else { n(0) }));
        let xml = format!(
            "{XML_DECL}{}",
            el(
                "p:sld",
                a,
                &(el("p:cSld", vec![], &(bg + &el("p:spTree", vec![], &format!("{SPTREE_HEAD}{tree}"))))
                    + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"
                    + &transition_xml(sl.transition.as_ref())
                    + &timing)
            )
        );
        let mut id =
            if sl.slide_id >= 256 && sl.slide_id < 2_147_483_648 { i64::from(sl.slide_id) } else { 256 + i64::try_from(si).unwrap_or(0) };
        while used_sld_ids.contains(&id) {
            id += 1;
        }
        used_sld_ids.insert(id);
        // speaker notes (text of the body, "notes" style of the main master)
        let nt = notes_for(doc, sl);
        let mut notes_ctx = TextConvCtx::new(
            doc,
            nt.and_then(|x| x.color_scheme.clone()).unwrap_or_else(|| scheme.clone()),
            today,
            Rc::clone(&pc.conv.field_seq),
        );
        notes_ctx.slide_number = Some(slide_no);
        if let Some(st) = main.and_then(|x| x.master_styles.clone()) {
            notes_ctx.styles = Some(st);
        }
        let nxml = nt.and_then(|x| notes_slide_xml(doc, x, &notes_ctx));
        if let Some(nxml) = nxml {
            let nn = notes_xmls.len() + 1;
            let mut nrels = PartRels::new();
            nrels.add("notesMaster", "../notesMasters/notesMaster1.xml", false);
            nrels.add("slide", &format!("../slides/slide{}.xml", si + 1), false);
            notes_xmls.push(PartOut { path: format!("ppt/notesSlides/notesSlide{nn}.xml"), xml: nxml, rels: nrels });
            pc.rels.add("notesSlide", &format!("../notesSlides/notesSlide{nn}.xml"), false);
        }
        #[allow(clippy::cast_precision_loss)]
        let idf = id as f64;
        let rels = std::mem::take(&mut pc.rels);
        drop(pc);
        slide_xmls.push(SlideOut { part: PartOut { path: format!("ppt/slides/slide{}.xml", si + 1), xml, rels }, id: idf });
    }
    // the notes master (with a theme of its own, a copy of the first) only if there are notes
    let notes_master: Option<PartOut> = if notes_xmls.is_empty() {
        None
    } else {
        let theme_n = theme_xmls.len() + 1;
        let first = theme_xmls.first().map(|t| t.1.clone());
        theme_xmls.push((
            format!("ppt/theme/theme{theme_n}.xml"),
            first.unwrap_or_else(|| theme_xml("Notas", &default_scheme(), "Calibri", "Calibri")),
        ));
        let mut rels = PartRels::new();
        rels.add("theme", &format!("../theme/theme{theme_n}.xml"), false);
        Some(PartOut { path: "ppt/notesMasters/notesMaster1.xml".to_owned(), xml: notes_master_xml(doc), rels })
    };

    // ── presentation.xml + rels
    let mut prels = PartRels::new();
    let mut m_ids = String::new();
    for (i, mx) in master_xmls.iter().enumerate() {
        let rid = prels.add("slideMaster", mx.path.strip_prefix("ppt/").unwrap_or(&mx.path), false);
        m_ids.push_str(&el(
            "p:sldMasterId",
            vec![("id", n(2_147_483_648f64 + f64::from(u32::try_from(i).unwrap_or(0)))), ("r:id", s(rid))],
            "",
        ));
    }
    let nm_ids = match &notes_master {
        Some(nm) => {
            let rid = prels.add("notesMaster", nm.path.strip_prefix("ppt/").unwrap_or(&nm.path), false);
            el("p:notesMasterIdLst", vec![], &el("p:notesMasterId", vec![("r:id", s(rid))], ""))
        }
        None => String::new(),
    };
    let mut s_ids = String::new();
    for sx in &slide_xmls {
        let rid = prels.add("slide", sx.part.path.strip_prefix("ppt/").unwrap_or(&sx.part.path), false);
        s_ids.push_str(&el("p:sldId", vec![("id", n(sx.id)), ("r:id", s(rid))], ""));
    }
    prels.add("presProps", "presProps.xml", false);
    prels.add("viewProps", "viewProps.xml", false);
    prels.add("theme", "theme/theme1.xml", false);
    prels.add("tableStyles", "tableStyles.xml", false);
    let first_main = main_master_of(doc, masters.first().copied());
    let def_scheme = masters
        .first()
        .and_then(|&i| doc.masters.get(i))
        .and_then(|m| m.color_scheme.clone())
        .or_else(|| first_main.and_then(|i| doc.masters.get(i)).and_then(|m| m.color_scheme.clone()))
        .unwrap_or_else(default_scheme);
    let def_ctx = TextConvCtx::new(doc, def_scheme, today, Rc::clone(&conv.field_seq));
    let mut a = ns_attrs();
    a.push(("saveSubsetFonts", n(1)));
    a.push(("firstSlideNum", if doc.first_slide_number != 1 { n(doc.first_slide_number) } else { None }));
    let mut no_host: Option<&mut dyn TextHost> = None;
    let pres_xml = format!(
        "{XML_DECL}{}",
        el(
            "p:presentation",
            a,
            &(el("p:sldMasterIdLst", vec![], &m_ids)
                + &nm_ids
                + &(if s_ids.is_empty() { String::new() } else { el("p:sldIdLst", vec![], &s_ids) })
                + &el(
                    "p:sldSz",
                    vec![
                        ("cx", n(emu(f64::from(doc.slide_w)))),
                        ("cy", n(emu(f64::from(doc.slide_h)))),
                        ("type", sld_sz_type(doc.slide_size_type).and_then(s))
                    ],
                    ""
                )
                + &el("p:notesSz", vec![("cx", n(emu(f64::from(doc.notes_w)))), ("cy", n(emu(f64::from(doc.notes_h))))], "")
                + &levels_xml("p:defaultTextStyle", &def_ctx, 4, &mut no_host))
        )
    );

    // ── assembly (order: content types, rels, docProps, presentation, …)
    let mut overrides: Vec<(String, String)> = vec![(
        "/ppt/presentation.xml".to_owned(),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml".to_owned(),
    )];
    let ov = |p: &str, t: &str| (format!("/{p}"), t.to_owned());
    for x in &master_xmls {
        overrides.push(ov(&x.path, "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"));
    }
    for x in &slide_xmls {
        overrides.push(ov(&x.part.path, "application/vnd.openxmlformats-officedocument.presentationml.slide+xml"));
    }
    for x in &layout_xmls {
        overrides.push(ov(&x.path, "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"));
    }
    for (p, _) in &theme_xmls {
        overrides.push(ov(p, "application/vnd.openxmlformats-officedocument.theme+xml"));
    }
    if let Some(nm) = &notes_master {
        overrides.push(ov(&nm.path, "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"));
    }
    for x in &notes_xmls {
        overrides.push(ov(&x.path, "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"));
    }
    overrides
        .push(("/ppt/presProps.xml".to_owned(), "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml".to_owned()));
    overrides
        .push(("/ppt/viewProps.xml".to_owned(), "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml".to_owned()));
    overrides.push((
        "/ppt/tableStyles.xml".to_owned(),
        "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml".to_owned(),
    ));
    overrides.push(("/docProps/core.xml".to_owned(), "application/vnd.openxmlformats-package.core-properties+xml".to_owned()));
    overrides.push(("/docProps/app.xml".to_owned(), "application/vnd.openxmlformats-officedocument.extended-properties+xml".to_owned()));
    for (p, t) in conv.overrides.iter() {
        overrides.push((format!("/{p}"), t.clone()));
    }
    let mut exts: Vec<String> = vec!["rels".to_owned(), "xml".to_owned()];
    exts.extend(conv.media_exts.iter().map(|(e, ())| e.clone()));
    let defaults: String = exts
        .iter()
        .map(|ext| {
            let ct = match ext.as_str() {
                "rels" => "application/vnd.openxmlformats-package.relationships+xml",
                "xml" => "application/xml",
                other => mime(other),
            };
            el("Default", vec![("Extension", s(ext.clone())), ("ContentType", s(ct))], "")
        })
        .collect();
    let ovs: String =
        overrides.iter().map(|(p, t)| el("Override", vec![("PartName", s(p.clone())), ("ContentType", s(t.clone()))], "")).collect();
    put("[Content_Types].xml", format!("{XML_DECL}{}", el("Types", vec![("xmlns", s(ns::CT))], &(defaults + &ovs))));
    let mut root_rels = PartRels::new();
    root_rels.add("officeDocument", "ppt/presentation.xml", false);
    root_rels.add("http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml", false);
    root_rels.add("extended-properties", "docProps/app.xml", false);
    put("_rels/.rels", root_rels.xml());
    put(
        "docProps/core.xml",
        format!(
            "{XML_DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>Apresentação convertida de .ppt</dc:title></cp:coreProperties>"
        ),
    );
    put(
        "docProps/app.xml",
        format!(
            "{XML_DECL}{}",
            el(
                "Properties",
                vec![("xmlns", s("http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"))],
                &format!("<Application>Universal Slides (conversão .ppt)</Application><Slides>{}</Slides>", slide_xmls.len())
            )
        ),
    );
    put("ppt/presentation.xml", pres_xml);
    put("ppt/_rels/presentation.xml.rels", prels.xml());
    let ordered: Vec<&PartOut> = master_xmls
        .iter()
        .chain(layout_xmls.iter())
        .chain(slide_xmls.iter().map(|x| &x.part))
        .chain(notes_master.iter())
        .chain(notes_xmls.iter())
        .collect();
    for x in ordered {
        put(&x.path, x.xml.clone());
        put(&rels_path(&x.path), x.rels.xml());
    }
    for (p, x) in &theme_xmls {
        put(p, x.clone());
    }
    put("ppt/presProps.xml", format!("{XML_DECL}{}", el("p:presentationPr", ns_attrs(), "")));
    put(
        "ppt/viewProps.xml",
        format!(
            "{XML_DECL}{}",
            el(
                "p:viewPr",
                ns_attrs(),
                "<p:normalViewPr/><p:slideViewPr><p:cSldViewPr><p:cViewPr><p:scale><a:sx n=\"1\" d=\"1\"/><a:sy n=\"1\" d=\"1\"/></p:scale><p:origin x=\"0\" y=\"0\"/></p:cViewPr><p:guideLst/></p:cSldViewPr></p:slideViewPr><p:gridSpacing cx=\"72008\" cy=\"72008\"/>"
            )
        ),
    );
    put(
        "ppt/tableStyles.xml",
        format!("{XML_DECL}{}", el("a:tblStyleLst", vec![("xmlns:a", s(ns::A)), ("def", s("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"))], "")),
    );
    for (path, bytes) in conv.parts.into_vec() {
        out.set(path, bytes);
    }
    Ok(out)
}
