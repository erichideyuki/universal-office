//! Frontier R4b (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `document.ts`
//! lines 185–1583 outside R4a's mapping functions: the objects a run carries — pictures,
//! text boxes, shapes, groups (DrawingML `w:drawing`, VML `w:pict`/`w:object`, SmartArt through its
//! drawing part), `syncImageFromXml`/`syncShapeFromXml`/`syncGroupFromXml` and
//! `blipFxImageEffects`. The read side of `picture-xml.ts` is `picture_format`.
//!
//! The TS `ReadContext` members these functions use (`resolveImage`, `resolvePart`,
//! `readBlocks`/`readBlocksGrouped`, the source store) are the [`ObjectHost`] trait: frontier R4c's
//! `ReadContext` struct implements it (no stored closures, A7). Every function returns
//! `Result`: the host reads media, parts and text box content, which can fail with a limit, and
//! the exceptions the TS lets escape (SmartArt with a negative style index, the VML text box
//! whose attribute breaks the generated XML) are `Err` values, never caught here, as in the TS.
//!
//! Object runs are returned as [`RunExt`] (the TS `Omit<Run, 'props'>`: `text` is always U+FFFC
//! and exactly one of `image`/`textbox`/`shape`/`group` is set); the caller adds the props.
//! The declarations of the nine kinds are in `core/docs/modules/uo-docx-d1a/R4b.md`.

use std::collections::HashMap;
use std::sync::Arc;

use uo_jsnum::conv::truthy;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::trim;
use uo_opc::xml::{Document as XmlDocument, Element};

use super::DOCX;
use super::drawing::{
    EMU_PER_TWIP_D, VmlStyle, parse_vml_style, read_sp_pr, read_wp_anchor, srgb_of, vml_anchor, vml_len_twips, vml_shape_style,
};
use super::matchers::{find_lit_then_boundary, is_one_of, keep_ascii_digits};
use super::paragraph::{BlocksMode, ReadContext};
use super::picture_format::read_picture_format;
use super::plain::{default_run_props, plain_paragraphs_with};
use super::props_map::{para_props_from_ppr, run_props_from_rpr};
use super::vml_shape::{VmlLang, vml_single_textbox, vml_textbox_to_drawing};
use super::watermark::strip_quotes;
use crate::error::DocxError;
use crate::ids::{IdAlloc, MediaId, TxbxSrc};
use crate::js::{JsAssign, JsNumber, JsObject, KeyOrder, Slot, Str};
use crate::model::color::DrawingColor;
use crate::model::drawing::{BlipTile, DrawingAnchor, Ltrb, ShapeBlip, ShapeStyle};
use crate::model::media::{ImageData, MediaRef};
use crate::model::picture::PicBlipFx;
use crate::model::props::{ColorSpec, IndSpec, PPr, RFonts, RPr, SpacingSpec, UnderlineSpec};
use crate::model::run::{
    DrawingGroup, DrawingShape, DrawingTextbox, GroupItem, ImageEffects, InlineImage, OleData, RunExt, RunProps, TextboxGeom, TextboxLink,
    TextboxView, VmlLine,
};
use crate::model::{Paragraph, Run, Story};
use crate::text::Utf16;

/// `OBJ`: the replacement character of an object run.
pub const OBJ: char = '\u{FFFC}';

/// A media part resolved by `ctx.resolveImage(rid)` (`{ data, ext }`; the TS `path` is not used
/// by these readers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImage {
    pub data: MediaId,
    pub ext: Str,
}

/// What `textboxBlocks` gets from the host.
#[derive(Debug, Clone, PartialEq)]
pub enum TextboxRead {
    /// `ctx.readBlocks` exists (the full-document read): the blocks of the `w:txbxContent`, and the
    /// source groups when the read keeps sources (`ctx.sources.addTextboxGroups`, done by the host)
    Blocks { story: Story, txbx_src: Option<TxbxSrc> },
    /// no `readBlocks` (comments, the sync functions): the `w:p` children read as paragraphs
    Paragraphs(Vec<Paragraph>),
}

/// The members of the TS `ReadContext` the object readers use.
pub trait ObjectHost {
    /// The id allocator of the document (`DrawingId`s and the paragraphs of the G20 fallback).
    fn ids(&mut self) -> &mut IdAlloc;
    /// `ctx.resolveImage(rid)`: the media part of a relationship, `None` for a missing or
    /// external one.
    fn resolve_image(&mut self, rid: &str) -> Result<Option<ResolvedImage>, DocxError>;
    /// Stores a payload in the document's media (the empty `Uint8Array` of a missing picture).
    fn add_media(&mut self, bytes: &[u8]) -> MediaId;
    /// `ctx.resolvePart?.(rid)`: the XML part a relationship names (SmartArt parts).
    fn resolve_part(&mut self, rid: &str) -> Result<Option<XmlDocument>, DocxError>;
    /// `ReadCtx::enter_nested` (G20).
    fn enter_nested(&mut self) -> bool;
    /// `ReadCtx::leave_nested`.
    fn leave_nested(&mut self);
    /// `textboxBlocks`' reading of the `w:txbxContent` (the block reader `fulldoc::read_blocks_of`
    /// grouped, or the paragraphs when the host has none).
    fn read_textbox_content(&mut self, content: Element<'_>) -> Result<TextboxRead, DocxError>;
    /// `DEFAULT_RUN_PROPS` behind an `Arc` the host may share between runs (the read's is interned,
    /// record "Model size — P1 done"); a new one by default.
    fn default_run_props(&mut self) -> Arc<RunProps> {
        Arc::new(default_run_props())
    }
    /// Counts a paragraph of the G20 plain fallback against the host's `max_paragraphs_per_part`
    /// (`ReadCtx::count_paragraph`); a host without per-part limits counts nothing.
    fn count_paragraph(&mut self) -> Result<(), DocxError> {
        Ok(())
    }
    /// Counts a run of the G20 plain fallback against the host's `max_runs_per_part`
    /// (`ReadCtx::count_run`); a host without per-part limits counts nothing.
    fn count_run(&mut self) -> Result<(), DocxError> {
        Ok(())
    }
}

macro_rules! put {
    ($o:expr, $f:ident, $k:literal, $v:expr) => {{
        $o.$f = Some($v);
        $o.mark($k);
    }};
}

macro_rules! float_to {
    ($t:expr, $anchor:expr) => {{
        $t.floating = Some(true);
        $t.mark("floating");
        $t.anchor = Some(Box::new($anchor.clone()));
        $t.mark("anchor");
    }};
}

macro_rules! rel_fields {
    ($t:expr, $anchor:expr) => {{
        $t.rel_h = Some($anchor.pos_h.relative_from.clone());
        $t.mark("relH");
        $t.rel_v = Some($anchor.pos_v.relative_from.clone());
        $t.mark("relV");
        if let Some(o) = $anchor.pos_h.offset {
            $t.offset_x_twips = Some(o);
            $t.mark("offsetXTwips");
        }
        if let Some(o) = $anchor.pos_v.offset {
            $t.offset_y_twips = Some(o);
            $t.mark("offsetYTwips");
        }
    }};
}

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `x || 0`.
fn or0(x: f64) -> f64 {
    if truthy(x) { x } else { 0.0 }
}

/// `parseInt(v ?? d, 10)`.
fn pint(v: Option<&str>, d: &str) -> f64 {
    parse_int(v.unwrap_or(d), 10)
}

/// `/^(1|true)$/.test(v)` / `v === '1' || v === 'true'`.
fn is_on(v: Option<&str>) -> bool {
    v.is_some_and(|v| is_one_of(v, &["1", "true"]))
}

fn obj_text() -> Utf16 {
    Utf16::from_units(vec![0xfffc])
}

/// `{ text: OBJ, props: { ...DEFAULT_RUN_PROPS }, ...ext }`: the run of a group item.
fn item_run(ext: Box<RunExt>, host: &mut dyn ObjectHost) -> Run {
    Run { text: obj_text(), props: host.default_run_props(), ext: Some(ext), ..Run::default() }
}

/// A new text box of `w`×`h` twips with its drawing id (`widthTwips`, `heightTwips` marked), on
/// the heap: the readers of a text box keep it across the read of its content, which recurses.
#[inline(never)]
fn new_textbox(w: f64, h: f64, host: &mut dyn ObjectHost) -> Box<DrawingTextbox> {
    let mut tb =
        Box::new(DrawingTextbox { width_twips: n(w), height_twips: n(h), drawing: host.ids().drawing(), ..DrawingTextbox::default() });
    tb.mark("widthTwips");
    tb.mark("heightTwips");
    tb
}

/// `delete o.k` for an optional field the caller already cleared.
fn forget<T: JsObject>(o: &mut T, name: &str) {
    if let Some(i) = T::key_index(name) {
        o.cleared_mut().clear(u32::from(i));
        o.key_order_mut().forget(i);
    }
}

// ───────────────────────── image effects ─────────────────────────

/// `vmlFixed`: a VML value (`19661f` in 16.16, or a number); `None` when absent, empty or not
/// finite.
fn vml_fixed(v: Option<&str>) -> Option<f64> {
    let v = v.filter(|s| !s.is_empty())?;
    let x = if v.ends_with('f') { parse_int(v, 10) / 65536.0 } else { parse_float(v) };
    x.is_finite().then_some(x)
}

/// `vmlImageEffects`: the colour effects of a `v:imagedata`.
fn vml_image_effects(el: Element<'_>) -> Option<ImageEffects> {
    let mut fx = ImageEffects::default();
    let gain = vml_fixed(el.attr("gain"));
    let bl = vml_fixed(el.attr("blacklevel"));
    if let Some(g) = gain
        && g != 1.0
    {
        put!(fx, gain, "gain", n(g));
    }
    if let Some(b) = bl
        && truthy(b)
    {
        put!(fx, bright, "bright", n(2.0 * b));
    }
    if matches!(el.attr("grayscale"), Some("t" | "true")) {
        put!(fx, gray, "gray", true);
    }
    if matches!(el.attr("bilevel"), Some("t" | "true")) {
        put!(fx, bi_level, "biLevel", n(0.5));
    }
    has_keys(&fx).then_some(fx)
}

fn has_keys<T: JsObject>(o: &T) -> bool {
    (0..T::keys().len()).any(|i| !matches!(o.slot(i), Slot::Absent))
}

/// `blipFxImageEffects`: the colour effects of a blip (`a:lum`, `a:grayscl`, `a:biLevel`; values in
/// 1/100000) in the drawing format.
#[must_use]
pub fn blip_fx_image_effects(b: &PicBlipFx) -> Option<ImageEffects> {
    let mut fx = ImageEffects::default();
    if let Some(lum) = &b.lum {
        let br = lum.bright.map_or(0.0, JsNumber::to_number) / 100_000.0;
        let c = math::max(-1.0, math::min(0.999, lum.contrast.map_or(0.0, JsNumber::to_number) / 100_000.0));
        if truthy(c) {
            put!(fx, gain, "gain", n(if c > 0.0 { 1.0 / (1.0 - c) } else { 1.0 + c }));
        }
        if truthy(br) {
            put!(fx, bright, "bright", n(br));
        }
        if truthy(br) || truthy(c) {
            put!(fx, floor, "floor", true);
        }
    }
    if b.grayscl == Some(true) {
        put!(fx, gray, "gray", true);
    }
    if let Some(bi) = b.bi_level {
        put!(fx, bi_level, "biLevel", n(bi.to_number() / 100_000.0));
    }
    has_keys(&fx).then_some(fx)
}

/// `blipImageEffects`: the effects of an `a:blip`.
fn blip_image_effects(blip: Element<'_>) -> Option<ImageEffects> {
    let int = |v: Option<&str>, d: &str| or0(pint(v, d));
    let mut b = PicBlipFx::default();
    if let Some(lum) = blip.first_child("a:lum") {
        b.lum = Some(crate::model::picture::BlipLum {
            bright: Some(n(int(lum.attr("bright"), "0"))),
            contrast: Some(n(int(lum.attr("contrast"), "0"))),
            ..crate::model::picture::BlipLum::default()
        });
    }
    if blip.first_child("a:grayscl").is_some() {
        b.grayscl = Some(true);
    }
    if let Some(bi) = blip.first_child("a:biLevel") {
        b.bi_level = Some(n(int(bi.attr("thresh"), "50000")));
    }
    blip_fx_image_effects(&b)
}

// ───────────────────────── pictures, text boxes ─────────────────────────

/// What `readPic` reads: a `pic:pic`, or the `a:blipFill` of a shape standing in for one (the TS
/// builds `{ name: 'pic:pic', children: [blipFill] }`).
#[derive(Debug, Clone, Copy)]
enum PicRoot<'d> {
    Pic(Element<'d>),
    BlipFill(Element<'d>),
}

/// `readPic`: the image of a `pic:pic` (blip, crop, style).
fn read_pic(root: PicRoot<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<InlineImage>, DocxError> {
    let (blip, src_rect, sp_pr) = match root {
        PicRoot::Pic(pic) => (pic.descend("a:blip"), pic.descend("a:srcRect"), pic.first_child("pic:spPr")),
        PicRoot::BlipFill(bf) => (bf.descend("a:blip"), bf.descend("a:srcRect"), None),
    };
    let rid = blip.and_then(|b| b.attr("r:embed").or_else(|| b.attr("r:link"))).filter(|r| !r.is_empty());
    let Some(rid) = rid else { return Ok(None) };
    let media = match host.resolve_image(rid)? {
        Some(m) => m,
        None => ResolvedImage { data: host.add_media(&[]), ext: "png".into() },
    };
    let svg_rid = blip.and_then(|b| b.descend("asvg:svgBlip")).and_then(|s| s.attr("r:embed")).filter(|r| !r.is_empty());
    let svg = match svg_rid {
        Some(r) => host.resolve_image(r)?,
        None => None,
    };
    let mut img = InlineImage {
        data: MediaRef(media.data),
        ext: media.ext,
        width_twips: n(w_tw),
        height_twips: n(h_tw),
        drawing: host.ids().drawing(),
        ..InlineImage::default()
    };
    img.mark("data");
    img.mark("ext");
    if let Some(s) = svg {
        put!(img, svg, "svg", MediaRef(s.data));
    }
    img.mark("widthTwips");
    img.mark("heightTwips");
    if let Some(fx) = blip.and_then(blip_image_effects) {
        put!(img, effects, "effects", fx);
    }
    if let Some(src) = src_rect
        && ["l", "t", "r", "b"].iter().any(|k| src.attr(k).is_some_and(|v| !v.is_empty()))
    {
        let f = |k: &str| or0(pint(src.attr(k), "0")) / 100_000.0;
        put!(img, crop, "crop", Ltrb { l: n(f("l")), t: n(f("t")), r: n(f("r")), b: n(f("b")), ..Ltrb::default() });
    }
    if let Some(sp_pr) = sp_pr {
        let st = read_sp_pr(Some(sp_pr), None);
        if st.rot.is_some_and(JsNumber::truthy) || st.flip_h == Some(true) || st.flip_v == Some(true) || st.line.is_some() {
            put!(img, style, "style", Box::new(st));
        }
    }
    Ok(Some(img))
}

/// `bodyPrOf`: the text box fields of a `wps:bodyPr` (or `a:bodyPr`).
fn body_pr_of(tb: &mut DrawingTextbox, body_pr: Option<Element<'_>>) {
    let Some(body_pr) = body_pr else { return };
    if body_pr.descend("a:spAutoFit").is_some() {
        put!(tb, auto_fit, "autoFit", true);
    }
    if body_pr.attr("wrap") == Some("none") {
        put!(tb, no_wrap, "noWrap", true);
    }
    let ins_tw = |name: &str, def: f64| match body_pr.attr(name) {
        Some(v) => math::round(parse_int(v, 10) / EMU_PER_TWIP_D),
        None => def,
    };
    put!(
        tb,
        insets,
        "insets",
        Ltrb {
            l: n(ins_tw("lIns", 144.0)),
            t: n(ins_tw("tIns", 72.0)),
            r: n(ins_tw("rIns", 144.0)),
            b: n(ins_tw("bIns", 72.0)),
            ..Ltrb::default()
        }
    );
    if let Some(anc @ ("t" | "ctr" | "b")) = body_pr.attr("anchor") {
        put!(tb, v_anchor, "vAnchor", anc.into());
    }
    if let Some(vert) = body_pr.attr("vert").filter(|v| !v.is_empty() && *v != "horz") {
        put!(tb, vert, "vert", vert.into());
    }
    if is_on(body_pr.attr("upright")) {
        put!(tb, upright, "upright", true);
    }
    if is_on(body_pr.attr("fromWordArt")) {
        let prst = body_pr.first_child("a:prstTxWarp").and_then(|p| p.attr("prst")).unwrap_or("textPlain");
        put!(tb, word_art, "wordArt", prst.into());
    }
}

/// `textboxBlocks`: the content of a `w:txbxContent` into the text box (A2: one [`Story`] with the
/// [`TextboxView`] of the TS `paras`/`blocks` pair), inside `enter_nested` (G20). The recursion of
/// nested stories goes through here: a small frame, the rest is in functions that return first.
#[inline(never)]
fn textbox_blocks(txbx: Element<'_>, host: &mut dyn ObjectHost, tb: &mut DrawingTextbox) -> Result<(), DocxError> {
    if !host.enter_nested() {
        return textbox_plain(txbx, host, tb);
    }
    let read = host.read_textbox_content(txbx);
    host.leave_nested();
    textbox_store(read?, tb);
    Ok(())
}

/// The G20 fallback of [`textbox_blocks`] past `max_nesting`: the paragraphs as plain text,
/// counted against the per-part limits as R5's `plain_cell_content` counts a cell's (the `w:p`
/// count first, before anything is built; then one run per non-empty paragraph), so content buried
/// below the nesting limit cannot go around them (record, "R5 follow-ups", handed on).
#[inline(never)]
fn textbox_plain(txbx: Element<'_>, host: &mut dyn ObjectHost, tb: &mut DrawingTextbox) -> Result<(), DocxError> {
    for _ in std::iter::once(txbx).chain(txbx.descendants()).filter(|e| e.name() == "w:p") {
        host.count_paragraph()?;
    }
    let props = host.default_run_props();
    let mut story = Story::new();
    for p in plain_paragraphs_with(txbx, host.ids(), &props) {
        if !p.runs.is_empty() {
            host.count_run()?;
        }
        story.push_paragraph(p);
    }
    tb.view = view_of(&story);
    tb.content = story;
    Ok(())
}

/// What the host read of a text box's content, into the text box.
#[inline(never)]
fn textbox_store(read: TextboxRead, tb: &mut DrawingTextbox) {
    match read {
        TextboxRead::Blocks { story, txbx_src } => {
            tb.view = view_of(&story);
            tb.content = story;
            tb.txbx_src = txbx_src;
        }
        TextboxRead::Paragraphs(paras) => {
            let mut story = Story::new();
            for p in paras {
                story.push_paragraph(p);
            }
            tb.content = story;
            tb.view = TextboxView::ParasOnly;
        }
    }
}

/// `paras.length ? synced : phantom` over the paragraphs of the blocks (cells included).
fn view_of(story: &Story) -> TextboxView {
    if story.flat_paragraphs().is_empty() { TextboxView::Phantom } else { TextboxView::Synced }
}

/// `{ name: 'a:solidFill', children: childElements(fontRef) }` through `readSpPr(...).fill`: the
/// fill colour of a `a:fontRef`.
fn font_ref_color(font_ref: Element<'_>) -> Option<DrawingColor> {
    srgb_of(Some(font_ref)).map(|c| c.color)
}

/// `readGraphicObject`: a drawing object (`wps:wsp`, `pic:pic`, group) as a run without props.
/// Boxed, and a small frame: it is on the native stack once per nested group or text box.
fn read_graphic_object(obj: Element<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<Box<RunExt>>, DocxError> {
    match obj.name() {
        "pic:pic" => read_pic_run(obj, host, w_tw, h_tw),
        "wps:wsp" => read_wsp(obj, host, w_tw, h_tw),
        "wpg:wgp" | "wpg:grpSp" | "wpc:wpc" => read_group(obj, host, w_tw, h_tw),
        _ => Ok(None),
    }
}

/// The `pic:pic` case of [`read_graphic_object`].
#[inline(never)]
fn read_pic_run(obj: Element<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<Box<RunExt>>, DocxError> {
    let image = read_pic(PicRoot::Pic(obj), host, w_tw, h_tw)?;
    Ok(image.map(|i| Box::new(RunExt { image: Some(Box::new(i)), ..RunExt::default() })))
}

/// `readSpPr(spPr, style)` on the heap (kept by [`read_wsp`] across the read of a text box).
#[inline(never)]
fn boxed_sp_pr(sp_pr: Option<Element<'_>>, style_el: Option<Element<'_>>) -> Box<ShapeStyle> {
    Box::new(read_sp_pr(sp_pr, style_el))
}

/// A `wps:wsp`: a text box (its content read through [`textbox_blocks`]), a picture fill or a
/// shape. Only what the recursion needs stays in this frame.
#[inline(never)]
fn read_wsp(obj: Element<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<Box<RunExt>>, DocxError> {
    let sp_pr = obj.first_child("wps:spPr");
    let style_el = obj.first_child("wps:style");
    let style = boxed_sp_pr(sp_pr, style_el);
    let txbx = obj.first_child("wps:txbx");
    let content = txbx.and_then(|t| t.first_child("w:txbxContent"));
    let linked = if content.is_none() { obj.first_child("wps:linkedTxbx") } else { None };
    if content.is_none() && linked.is_none() {
        return wsp_shape(sp_pr, style, host, w_tw, h_tw);
    }
    let mut tb = new_textbox(w_tw, h_tw, host);
    if let Some(content) = content {
        textbox_blocks(content, host, &mut tb)?;
    }
    wsp_textbox(obj, WspParts { sp_pr, style_el, txbx, linked }, style, tb, host).map(Some)
}

/// The elements of a `wps:wsp` the text box case reads after the content.
#[derive(Clone, Copy)]
struct WspParts<'d> {
    sp_pr: Option<Element<'d>>,
    style_el: Option<Element<'d>>,
    txbx: Option<Element<'d>>,
    linked: Option<Element<'d>>,
}

/// The text box of a `wps:wsp` after its content: story id or link, `bodyPr`, font colour, picture
/// fill, shape style, geometry and rotation.
#[inline(never)]
fn wsp_textbox(
    obj: Element<'_>,
    parts: WspParts<'_>,
    mut style: Box<ShapeStyle>,
    mut tb: Box<DrawingTextbox>,
    host: &mut dyn ObjectHost,
) -> Result<Box<RunExt>, DocxError> {
    let WspParts { sp_pr, style_el, txbx, linked } = parts;
    if let Some(linked) = linked {
        tb.view = TextboxView::Synced;
        put!(
            tb,
            link,
            "link",
            TextboxLink {
                id: n(or0(pint(linked.attr("id"), "0"))),
                seq: n({
                    let q = pint(linked.attr("seq"), "1");
                    if truthy(q) { q } else { 1.0 }
                }),
                ..TextboxLink::default()
            }
        );
    } else if let Some(txbx) = txbx {
        let sid = parse_int(txbx.attr("id").unwrap_or(""), 10);
        if sid.is_finite() {
            put!(tb, story, "story", n(sid));
        }
    }
    body_pr_of(&mut tb, obj.first_child("wps:bodyPr"));
    let font_ref = style_el.unwrap_or(obj).first_child("a:fontRef");
    if let Some(fc) = font_ref.and_then(font_ref_color) {
        put!(tb, font_color, "fontColor", fc);
    }
    let bf = sp_pr.and_then(|s| s.first_child("a:blipFill"));
    let blip = bf.and_then(|b| b.first_child("a:blip"));
    let media = match blip.and_then(|b| b.attr("r:embed")).filter(|r| !r.is_empty()) {
        Some(r) => host.resolve_image(r)?,
        None => None,
    };
    if let (Some(bf), Some(media)) = (bf, media) {
        let tile = bf.first_child("a:tile");
        let tn = |k: &str, d: &str| or0(pint(tile.and_then(|t| t.attr(k)), d));
        let amf = blip.and_then(|b| b.first_child("a:alphaModFix"));
        let mut sb =
            ShapeBlip { image: ImageData { data: MediaRef(media.data), ext: media.ext, ..ImageData::default() }, ..ShapeBlip::default() };
        if let Some(t) = tile {
            sb.tile = Some(BlipTile {
                tx: n(tn("tx", "0")),
                ty: n(tn("ty", "0")),
                sx: n(tn("sx", "100000")),
                sy: n(tn("sy", "100000")),
                algn: t.attr("algn").unwrap_or("tl").into(),
                flip: t.attr("flip").unwrap_or("none").into(),
                ..BlipTile::default()
            });
        }
        if let Some(amf) = amf {
            sb.alpha = Some(n(or0(pint(amf.attr("amt"), "100000")) / 100_000.0));
        }
        put!(style, blip, "blip", Box::new(sb));
    }
    if style.fill.is_some()
        || style.line.is_some()
        || style.gradient.is_some()
        || style.blip.is_some()
        || style.pattern.is_some()
        || style.effect_ref.is_some()
    {
        put!(tb, shape, "shape", Box::new((*style).clone()));
    }
    put!(tb, line_inset, "lineInset", style.line_inset.clone().unwrap_or_default());
    if !style.geom.is_empty() && &*style.geom != "rect" && &*style.geom != "custom" {
        let mut g = TextboxGeom { prst: style.geom.clone(), ..TextboxGeom::default() };
        if let Some(adj) = &style.adj {
            g.adj = Some(adj.clone());
        }
        put!(tb, geom, "geom", g);
    }
    if style.rot.is_some_and(JsNumber::truthy) || style.flip_v == Some(true) {
        let rot = style.rot.map_or(0.0, JsNumber::to_number);
        put!(tb, rot, "rot", n(math::rem(rot + if style.flip_v == Some(true) { 180.0 } else { 0.0 }, 360.0)));
    }
    Ok(Box::new(RunExt { textbox: Some(tb), ..RunExt::default() }))
}

/// A `wps:wsp` without a text box: its picture fill as an image, or the shape.
#[inline(never)]
fn wsp_shape(
    sp_pr: Option<Element<'_>>,
    style: Box<ShapeStyle>,
    host: &mut dyn ObjectHost,
    w_tw: f64,
    h_tw: f64,
) -> Result<Option<Box<RunExt>>, DocxError> {
    if let Some(bf) = sp_pr.and_then(|s| s.first_child("a:blipFill"))
        && let Some(image) = read_pic(PicRoot::BlipFill(bf), host, w_tw, h_tw)?
    {
        return Ok(Some(Box::new(RunExt { image: Some(Box::new(image)), ..RunExt::default() })));
    }
    let mut shape = DrawingShape {
        style: *style,
        width_twips: n(w_tw),
        height_twips: n(h_tw),
        drawing: host.ids().drawing(),
        ..DrawingShape::default()
    };
    shape.mark("style");
    shape.mark("widthTwips");
    shape.mark("heightTwips");
    Ok(Some(Box::new(RunExt { shape: Some(Box::new(shape)), ..RunExt::default() })))
}

/// The child scale of a `wpg` group: `a:chOff`, and EMU per child unit on each axis.
#[derive(Clone, Copy)]
struct GroupScale {
    off_x: f64,
    off_y: f64,
    sx: f64,
    sy: f64,
}

/// `num(el, k)` of `readGroup`: an integer attribute, 0 when missing or not a number.
fn group_num(el: Option<Element<'_>>, k: &str) -> f64 {
    or0(pint(el.and_then(|e| e.attr(k)), "0"))
}

/// The scale of a `wpg` group's children (`a:chOff`, `a:chExt` or `a:ext`).
#[inline(never)]
fn group_scale(xfrm: Option<Element<'_>>, w_tw: f64, h_tw: f64) -> GroupScale {
    let ch_off = xfrm.and_then(|x| x.first_child("a:chOff"));
    let ch_ext = xfrm.and_then(|x| x.first_child("a:chExt"));
    let ext = xfrm.and_then(|x| x.first_child("a:ext"));
    let either = |a: f64, b: f64| if truthy(a) { a } else { b };
    let cw = either(group_num(ch_ext, "cx"), group_num(ext, "cx"));
    let ch = either(group_num(ch_ext, "cy"), group_num(ext, "cy"));
    let sx = if truthy(cw) { (w_tw * EMU_PER_TWIP_D) / cw } else { 1.0 };
    let sy = if truthy(ch) { (h_tw * EMU_PER_TWIP_D) / ch } else { 1.0 };
    GroupScale { off_x: group_num(ch_off, "x"), off_y: group_num(ch_off, "y"), sx, sy }
}

/// The position and size of one group child, in twips (`x`, `y`, `w`, `h`).
#[inline(never)]
fn group_child_box(child: Element<'_>, scale: GroupScale) -> [f64; 4] {
    let cpr = child.first_child("wps:spPr").or_else(|| child.first_child("pic:spPr")).or_else(|| child.first_child("wpg:grpSpPr"));
    let cx = cpr.and_then(|c| c.first_child("a:xfrm"));
    let off = cx.and_then(|c| c.first_child("a:off"));
    let cext = cx.and_then(|c| c.first_child("a:ext"));
    let x = ((group_num(off, "x") - scale.off_x) * scale.sx) / EMU_PER_TWIP_D;
    let y = ((group_num(off, "y") - scale.off_y) * scale.sy) / EMU_PER_TWIP_D;
    let w = (group_num(cext, "cx") * scale.sx) / EMU_PER_TWIP_D;
    let h = (group_num(cext, "cy") * scale.sy) / EMU_PER_TWIP_D;
    [x, y, w, h]
}

/// `items.push({ x, y, w, h, run })`.
#[inline(never)]
fn push_group_item(items: &mut Vec<GroupItem>, [x, y, w, h]: [f64; 4], run: Box<RunExt>, host: &mut dyn ObjectHost) {
    items.push(GroupItem { x: n(x), y: n(y), w: n(w), h: n(h), run: item_run(run, host), ..GroupItem::default() });
}

/// A `wpg:wgp`/`wpg:grpSp`/`wpc:wpc`: its children as group items. A nested group enters
/// `max_nesting` (R4b item 4); a small frame, on the native stack once per nested group.
#[inline(never)]
fn read_group(obj: Element<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<Box<RunExt>>, DocxError> {
    let xfrm = obj.first_child("wpg:grpSpPr").and_then(|g| g.first_child("a:xfrm"));
    let scale = group_scale(xfrm, w_tw, h_tw);
    let mut items: Vec<GroupItem> = Vec::new();
    for child in obj.child_elements() {
        if !matches!(child.name(), "wps:wsp" | "pic:pic" | "wpg:grpSp") {
            continue;
        }
        let place = group_child_box(child, scale);
        let nested_group = child.name() == "wpg:grpSp";
        if nested_group && !host.enter_nested() {
            continue;
        }
        let run = read_graphic_object(child, host, place[2], place[3]);
        if nested_group {
            host.leave_nested();
        }
        if let Some(run) = run? {
            push_group_item(&mut items, place, run, host);
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(group_run(xfrm, items, host, w_tw, h_tw)))
}

/// The group of [`read_group`] once its items are read: size, drawing id, rotation and flips.
#[inline(never)]
fn group_run(xfrm: Option<Element<'_>>, items: Vec<GroupItem>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Box<RunExt> {
    let mut group =
        DrawingGroup { width_twips: n(w_tw), height_twips: n(h_tw), items, drawing: host.ids().drawing(), ..DrawingGroup::default() };
    group.mark("widthTwips");
    group.mark("heightTwips");
    group.mark("items");
    let rot = pint(xfrm.and_then(|x| x.attr("rot")), "0");
    if truthy(rot) {
        put!(group, rot, "rot", n(rot / 60000.0));
    }
    if xfrm.and_then(|x| x.attr("flipH")) == Some("1") {
        put!(group, flip_h, "flipH", true);
    }
    if xfrm.and_then(|x| x.attr("flipV")) == Some("1") {
        put!(group, flip_v, "flipV", true);
    }
    Box::new(RunExt { group: Some(Box::new(group)), ..RunExt::default() })
}

// ───────────────────────── SmartArt ─────────────────────────

/// `dmlRunColor` over the colour element itself (`dmlRunColor({ children: [c] })`).
fn dml_run_color_el(c: Element<'_>) -> Option<ColorSpec> {
    match c.name() {
        "a:srgbClr" => {
            let mut s = ColorSpec { val: Some(upper(c.attr("val").unwrap_or("000000"))), ..ColorSpec::default() };
            s.mark("val");
            Some(s)
        }
        "a:sysClr" => {
            let v = match c.attr("lastClr") {
                Some(l) => l,
                None if c.attr("val") == Some("window") => "FFFFFF",
                None => "000000",
            };
            Some(ColorSpec { val: Some(upper(v)), ..ColorSpec::default() })
        }
        "a:prstClr" => {
            Some(ColorSpec { val: Some((if c.attr("val") == Some("white") { "FFFFFF" } else { "000000" }).into()), ..ColorSpec::default() })
        }
        "a:schemeClr" => {
            let mut spec = ColorSpec { theme_color: Some(c.attr("val").unwrap_or("tx1").into()), ..ColorSpec::default() };
            spec.mark("themeColor");
            let frac = |k: &str| -> Option<Str> {
                let m = c.first_child(k)?;
                let x = math::round((or0(pint(m.attr("val"), "100000")) / 100_000.0) * 255.0);
                let hex = crate::word::num::js_to_radix(x, 16);
                Some(upper(&crate::word::num::pad_start_zero(&hex, 2)))
            };
            let t = frac("a:tint");
            let sh = frac("a:shade");
            if let Some(t) = t.filter(|t| !t.is_empty()) {
                put!(spec, theme_tint, "themeTint", t);
            }
            if let Some(sh) = sh.filter(|s| !s.is_empty()) {
                put!(spec, theme_shade, "themeShade", sh);
            }
            Some(spec)
        }
        _ => None,
    }
}

/// `dmlRunColor(el)`: a DrawingML colour child of `el` as a run colour; tint and shade of a scheme
/// colour become `themeTint`/`themeShade`.
fn dml_run_color(el: Option<Element<'_>>) -> Option<ColorSpec> {
    let el = el?;
    let c = el
        .first_child("a:srgbClr")
        .or_else(|| el.first_child("a:sysClr"))
        .or_else(|| el.first_child("a:schemeClr"))
        .or_else(|| el.first_child("a:prstClr"))?;
    dml_run_color_el(c)
}

/// `toUpperCase` of an attribute value.
fn upper(s: &str) -> Str {
    uo_text::case::to_uppercase_str(s).into()
}

/// `dmlParagraphs`: the paragraphs of a SmartArt `dsp:txBody`, with every format explicit.
fn dml_paragraphs(body: Element<'_>, style_el: Option<Element<'_>>, tx_fill: Option<&ColorSpec>, ids: &mut IdAlloc) -> Vec<Paragraph> {
    let font_ref = style_el.and_then(|s| s.first_child("a:fontRef"));
    let ref_color: Option<ColorSpec> = tx_fill.cloned().or_else(|| dml_run_color(font_ref));
    let ref_kind = if font_ref.and_then(|f| f.attr("idx")) == Some("major") { "major" } else { "minor" };
    let rpr_of = |r_pr: Option<Element<'_>>| -> RPr {
        let sz_attr = pint(r_pr.and_then(|r| r.attr("sz")), "1800");
        let sz = math::round((if truthy(sz_attr) { sz_attr } else { 1800.0 }) / 50.0);
        let face = |tag: &str| -> Option<&str> {
            r_pr.and_then(|r| r.first_child(tag)).and_then(|f| f.attr("typeface")).filter(|s| !s.is_empty())
        };
        let kind = |f: Option<&str>| -> &'static str {
            match f {
                Some(f) => {
                    if f.starts_with("+mj") {
                        "major"
                    } else {
                        "minor"
                    }
                }
                None => ref_kind,
            }
        };
        let lt = face("a:latin");
        let ea = face("a:ea");
        let cs = face("a:cs");
        let mut rf = RFonts::default();
        if let Some(l) = lt.filter(|l| !l.starts_with('+')) {
            put!(rf, ascii, "ascii", l.into());
            put!(rf, h_ansi, "hAnsi", l.into());
        } else {
            put!(rf, ascii_theme, "asciiTheme", format!("{}HAnsi", kind(lt)).into());
            put!(rf, h_ansi_theme, "hAnsiTheme", format!("{}HAnsi", kind(lt)).into());
        }
        if let Some(e) = ea.filter(|e| !e.starts_with('+')) {
            put!(rf, east_asia, "eastAsia", e.into());
        } else {
            put!(rf, east_asia_theme, "eastAsiaTheme", format!("{}EastAsia", kind(ea)).into());
        }
        if let Some(c) = cs.filter(|c| !c.starts_with('+')) {
            put!(rf, cs, "cs", c.into());
        } else {
            put!(rf, cstheme, "cstheme", format!("{}Bidi", kind(cs)).into());
        }
        let on = |k: &str| is_on(r_pr.and_then(|r| r.attr(k)));
        let color = dml_run_color(r_pr.and_then(|r| r.first_child("a:solidFill")))
            .or_else(|| ref_color.clone())
            .unwrap_or_else(|| ColorSpec { theme_color: Some("tx1".into()), ..ColorSpec::default() });
        let mut rpr = RPr::default();
        put!(rpr, r_fonts, "rFonts", Box::new(rf));
        put!(rpr, sz, "sz", n(sz));
        put!(rpr, sz_cs, "szCs", n(sz));
        put!(rpr, b, "b", on("b"));
        put!(rpr, b_cs, "bCs", on("b"));
        put!(rpr, i, "i", on("i"));
        put!(rpr, i_cs, "iCs", on("i"));
        put!(rpr, color, "color", Box::new(color));
        if let Some(u) = r_pr.and_then(|r| r.attr("u")).filter(|u| !u.is_empty() && *u != "none") {
            put!(
                rpr,
                u,
                "u",
                Box::new(UnderlineSpec { val: (if u == "dbl" { "double" } else { "single" }).into(), ..UnderlineSpec::default() })
            );
        }
        rpr
    };
    let num = |el: Option<Element<'_>>| or0(pint(el.and_then(|e| e.attr("val")), "0"));
    let edges = is_on(body.first_child("a:bodyPr").and_then(|b| b.attr("spcFirstLastPara")));
    let aps: Vec<Element<'_>> = body.children_named("a:p").collect();
    let mut out: Vec<Paragraph> = Vec::new();
    for (k, ap) in aps.iter().enumerate() {
        let p_pr = ap.first_child("a:pPr");
        let mut runs: Vec<Run> = Vec::new();
        for c in ap.child_elements() {
            if !matches!(c.name(), "a:r" | "a:br" | "a:fld") {
                continue;
            }
            let t = c.first_child("a:t");
            let text = if c.name() == "a:br" { "\n".to_string() } else { t.map_or_else(String::new, |t| t.text_content()) };
            if text.is_empty() {
                continue;
            }
            let rpr = rpr_of(c.first_child("a:rPr"));
            runs.push(Run {
                text: Utf16::from(text),
                props: Arc::new(run_props_from_rpr(Some(&rpr))),
                rpr: Some(Arc::new(rpr)),
                ..Run::default()
            });
        }
        let first_r = ap.first_child("a:r");
        let mark = rpr_of(ap.first_child("a:endParaRPr").or_else(|| first_r.and_then(|r| r.first_child("a:rPr"))));
        let mark_sz = mark.sz.map_or(0.0, JsNumber::to_number);
        let mut szs: Vec<f64> = vec![mark_sz];
        szs.extend(runs.iter().map(|r| r.rpr.as_ref().and_then(|x| x.sz).map_or(0.0, JsNumber::to_number)));
        let sz_pt = math::max_n(&szs) / 2.0;
        let gap = |tag: &str| -> f64 {
            let e = p_pr.and_then(|p| p.first_child(tag));
            let pts = e.and_then(|e| e.first_child("a:spcPts"));
            let pct = e.and_then(|e| e.first_child("a:spcPct"));
            if let Some(pts) = pts {
                math::round(num(Some(pts)) / 5.0)
            } else if let Some(pct) = pct {
                math::round((num(Some(pct)) / 100_000.0) * 1.2 * sz_pt * 20.0)
            } else {
                0.0
            }
        };
        let ln = p_pr.and_then(|p| p.first_child("a:lnSpc"));
        let ln_pct = ln.and_then(|l| l.first_child("a:spcPct"));
        let ln_pts = ln.and_then(|l| l.first_child("a:spcPts"));
        let algn = p_pr.and_then(|p| p.attr("algn"));
        let emu = |kk: &str| math::round(or0(pint(p_pr.and_then(|p| p.attr(kk)), "0")) / EMU_PER_TWIP_D);
        let indent = emu("indent");
        let jc = match algn {
            Some("ctr") => "center",
            Some("r") => "right",
            Some("just") => "both",
            Some("dist") => "distribute",
            _ => "left",
        };
        let mut spacing = SpacingSpec {
            before: Some(n(if k == 0 && !edges { 0.0 } else { gap("a:spcBef") })),
            after: Some(n(if k + 1 == aps.len() && !edges { 0.0 } else { gap("a:spcAft") })),
            ..SpacingSpec::default()
        };
        if let Some(lp) = ln_pts {
            spacing.line = Some(n(math::round(num(Some(lp)) / 5.0)));
            spacing.line_rule = Some("exact".into());
        } else {
            spacing.line = Some(n(if let Some(lp) = ln_pct { math::round((240.0 * num(Some(lp))) / 100_000.0) } else { 240.0 }));
            spacing.line_rule = Some("auto".into());
        }
        let mut ind = IndSpec { left: Some(n(emu("marL"))), right: Some(n(0.0)), ..IndSpec::default() };
        if indent < 0.0 {
            ind.hanging = Some(n(-indent));
        } else {
            ind.first_line = Some(n(indent));
        }
        let mut ppr = PPr::default();
        put!(ppr, jc, "jc", jc.into());
        put!(ppr, spacing, "spacing", Box::new(spacing));
        put!(ppr, ind, "ind", Box::new(ind));
        put!(ppr, contextual_spacing, "contextualSpacing", false);
        put!(ppr, r_pr, "rPr", Box::new(mark));
        let mut para = Paragraph::empty(ids.para());
        para.runs = runs;
        para.props = Arc::new(para_props_from_ppr(Some(&ppr)));
        para.ppr = Some(Arc::new(ppr));
        out.push(para);
    }
    out
}

/// A group of `dsp:sp` shapes and text boxes of the SmartArt drawing part (`readDiagram`).
fn read_diagram(gd: Element<'_>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Option<RunExt>, DocxError> {
    let rel_ids = gd.first_child("dgm:relIds");
    let data_doc = match rel_ids {
        Some(r) => host.resolve_part(r.attr("r:dm").unwrap_or(""))?,
        None => None,
    };
    let data = data_doc.as_ref().map(XmlDocument::root);
    let ext = data.and_then(|d| d.descend("dsp:dataModelExt"));
    let drawing_doc = match ext {
        Some(e) => host.resolve_part(e.attr("relId").unwrap_or(""))?,
        None => None,
    };
    let drawing = drawing_doc.as_ref().map(XmlDocument::root);
    let Some(tree) = drawing.and_then(|d| d.descend("dsp:spTree")) else { return Ok(None) };
    let colors_doc = match rel_ids {
        Some(r) => host.resolve_part(r.attr("r:cs").unwrap_or(""))?,
        None => None,
    };
    let colors = colors_doc.as_ref().map(XmlDocument::root);
    let mut lbls: HashMap<&str, Element<'_>> = HashMap::new();
    if let Some(colors) = colors {
        for l in colors.children_named("dgm:styleLbl") {
            lbls.insert(l.attr("name").unwrap_or(""), l);
        }
    }
    let mut tx_fill: HashMap<&str, Option<ColorSpec>> = HashMap::new();
    if let Some(pt_lst) = data.and_then(|d| d.descend("dgm:ptLst")) {
        for pt in pt_lst.children_named("dgm:pt") {
            let pr = pt.first_child("dgm:prSet");
            let lst = pr.and_then(|p| lbls.get(p.attr("presStyleLbl").unwrap_or("")).copied());
            let fills = lst.and_then(|l| l.first_child("dgm:txFillClrLst"));
            let cs: Vec<Element<'_>> = fills.map_or_else(Vec::new, |f| f.child_elements().collect());
            let Some(model_id) = pt.attr("modelId").filter(|m| !m.is_empty()) else { continue };
            if cs.is_empty() {
                continue;
            }
            let (Some(pr), Some(fills)) = (pr, fills) else { continue };
            let i = or0(pint(pr.attr("presStyleIdx"), "0"));
            let len = cs.len();
            let flen = f64::from(u32::try_from(len).unwrap_or(u32::MAX));
            let c = if fills.attr("meth") == Some("cycle") {
                uo_jsnum::conv::js_index(math::rem(i, flen), len).and_then(|k| cs.get(k))
            } else {
                uo_jsnum::conv::js_index(math::min(i, flen - 1.0), len).and_then(|k| cs.get(k))
            };
            let Some(c) = c else {
                return Err(DocxError::Corrupt("SmartArt colour list index out of range (TS: TypeError reading the colour element)"));
            };
            tx_fill.insert(model_id, dml_run_color_el(*c));
        }
    }
    let num_tw = |el: Option<Element<'_>>, k: &str| or0(pint(el.and_then(|e| e.attr(k)), "0")) / EMU_PER_TWIP_D;
    let rect = |xf: Element<'_>| {
        let off = xf.first_child("a:off");
        let e = xf.first_child("a:ext");
        (num_tw(off, "x"), num_tw(off, "y"), num_tw(e, "cx"), num_tw(e, "cy"))
    };
    let mut items: Vec<GroupItem> = Vec::new();
    let mut stack = vec![tree.child_elements()];
    while let Some(top) = stack.last_mut() {
        let Some(sp) = top.next() else {
            stack.pop();
            continue;
        };
        if sp.name() == "dsp:grpSp" {
            stack.push(sp.child_elements());
            continue;
        }
        let sp_pr = if sp.name() == "dsp:sp" { sp.first_child("dsp:spPr") } else { None };
        let Some(xf) = sp_pr.and_then(|s| s.first_child("a:xfrm")) else { continue };
        let (bx, by, bw, bh) = rect(xf);
        let style_el = sp.first_child("dsp:style");
        let style = read_sp_pr(sp_pr, style_el);
        if style.fill.is_some() || style.line.is_some() || style.gradient.is_some() {
            let mut shape =
                DrawingShape { style, width_twips: n(bw), height_twips: n(bh), drawing: host.ids().drawing(), ..DrawingShape::default() };
            shape.mark("style");
            shape.mark("widthTwips");
            shape.mark("heightTwips");
            items.push(GroupItem {
                x: n(bx),
                y: n(by),
                w: n(bw),
                h: n(bh),
                run: item_run(Box::new(RunExt { shape: Some(Box::new(shape)), ..RunExt::default() }), host),
                ..GroupItem::default()
            });
        }
        let body = sp.first_child("dsp:txBody");
        let fill = tx_fill.get(sp.attr("modelId").unwrap_or("")).and_then(Option::as_ref);
        let paras = match body {
            Some(b) => dml_paragraphs(b, style_el, fill, host.ids()),
            None => Vec::new(),
        };
        let Some(body) = body else { continue };
        if !paras.iter().any(|p| !p.runs.is_empty()) {
            continue;
        }
        let txf = sp.first_child("dsp:txXfrm");
        let (tx, ty, tw, th) = txf.map_or((bx, by, bw, bh), rect);
        let mut tb = DrawingTextbox { width_twips: n(tw), height_twips: n(th), drawing: host.ids().drawing(), ..DrawingTextbox::default() };
        let mut story = Story::new();
        for p in paras {
            story.push_paragraph(p);
        }
        tb.content = story;
        tb.view = TextboxView::Synced;
        tb.mark("widthTwips");
        tb.mark("heightTwips");
        body_pr_of(&mut tb, body.first_child("a:bodyPr"));
        let rot = pint(txf.unwrap_or(xf).attr("rot"), "0");
        if truthy(rot) {
            put!(tb, rot, "rot", n(rot / 60000.0));
        }
        items.push(GroupItem {
            x: n(tx),
            y: n(ty),
            w: n(tw),
            h: n(th),
            run: item_run(Box::new(RunExt { textbox: Some(Box::new(tb)), ..RunExt::default() }), host),
            ..GroupItem::default()
        });
    }
    if items.is_empty() {
        return Ok(None);
    }
    let mut group =
        DrawingGroup { width_twips: n(w_tw), height_twips: n(h_tw), items, drawing: host.ids().drawing(), ..DrawingGroup::default() };
    group.mark("widthTwips");
    group.mark("heightTwips");
    group.mark("items");
    Ok(Some(RunExt { group: Some(Box::new(group)), ..RunExt::default() }))
}

// ───────────────────────── w:drawing ─────────────────────────

/// `readDrawingRun(drawing, ctx)`: a `w:drawing` as an object run (picture, text box, shape or
/// group). `raw` is the TS `drawing.raw` (the raw text of the `w:drawing`; `Element::raw` for a
/// parsed document, the text itself for the sync functions).
pub fn read_drawing_run(drawing: Element<'_>, raw: Option<&str>, host: &mut dyn ObjectHost) -> Result<Option<RunExt>, DocxError> {
    Ok(drawing_run(drawing, raw, host)?.map(|run| *run))
}

/// [`read_drawing_run`], boxed. The recursion of nested stories goes through here (a text box's
/// paragraphs hold drawings): a small frame, the rest is in functions that return first.
#[inline(never)]
fn drawing_run(drawing: Element<'_>, raw: Option<&str>, host: &mut dyn ObjectHost) -> Result<Option<Box<RunExt>>, DocxError> {
    let raw = raw.filter(|r| !r.is_empty());
    let anchor_el = drawing.first_child("wp:anchor");
    let Some(container) = anchor_el.or_else(|| drawing.first_child("wp:inline")) else { return Ok(None) };
    let extent = container.first_child("wp:extent");
    let w_tw = pint(extent.and_then(|e| e.attr("cx")), "0") / EMU_PER_TWIP_D;
    let h_tw = pint(extent.and_then(|e| e.attr("cy")), "0") / EMU_PER_TWIP_D;
    let gd = container.descend("a:graphicData");
    let obj = gd.and_then(|g| g.child_elements().find(|c| matches!(c.name(), "pic:pic" | "wps:wsp" | "wpg:wgp" | "wpc:wpc")));
    let dgm = obj.is_none() && gd.and_then(|g| g.attr("uri")).is_some_and(is_diagram_uri);
    if obj.is_none() && !dgm {
        return Ok(None);
    }
    let run = match obj {
        Some(o) => read_graphic_object(o, host, w_tw, h_tw)?,
        None => Some(diagram_or_rect(gd, host, w_tw, h_tw)?),
    };
    let Some(mut run) = run else { return Ok(None) };
    finish_drawing_run(&mut run, container, anchor_el, raw);
    Ok(Some(run))
}

/// A `w:drawing` whose graphic data is a diagram: the SmartArt, or a plain rectangle.
#[inline(never)]
fn diagram_or_rect(gd: Option<Element<'_>>, host: &mut dyn ObjectHost, w_tw: f64, h_tw: f64) -> Result<Box<RunExt>, DocxError> {
    let diagram = match gd {
        Some(g) => read_diagram(g, host, w_tw, h_tw)?,
        None => None,
    };
    Ok(Box::new(diagram.unwrap_or_else(|| {
        let mut style = ShapeStyle { geom: "rect".into(), ..ShapeStyle::default() };
        style.mark("geom");
        let mut shape =
            DrawingShape { style, width_twips: n(w_tw), height_twips: n(h_tw), drawing: host.ids().drawing(), ..DrawingShape::default() };
        shape.mark("style");
        shape.mark("widthTwips");
        shape.mark("heightTwips");
        RunExt { shape: Some(Box::new(shape)), ..RunExt::default() }
    })))
}

/// The tail of `readDrawingRun` once the object is read: effect extent, anchor, `docPr hidden`,
/// raw XML and picture format.
#[inline(never)]
fn finish_drawing_run(run: &mut RunExt, container: Element<'_>, anchor_el: Option<Element<'_>>, raw: Option<&str>) {
    let anchor = read_wp_anchor(anchor_el);
    let ee = if anchor_el.is_none() { container.first_child("wp:effectExtent") } else { None };
    if let Some(ee) = ee {
        let e = |k: &str| or0(pint(ee.attr(k), "0")) / EMU_PER_TWIP_D;
        if run.image.is_some() || run.shape.is_some() {
            let v = Ltrb { l: n(e("l")), t: n(e("t")), r: n(e("r")), b: n(e("b")), ..Ltrb::default() };
            if truthy(e("l")) || truthy(e("t")) || truthy(e("r")) || truthy(e("b")) {
                if let Some(img) = run.image.as_mut() {
                    put!(img, inline_effect, "inlineEffect", v);
                } else if let Some(sh) = run.shape.as_mut() {
                    put!(sh, inline_effect, "inlineEffect", v);
                }
            }
        }
        if let Some(g) = run.group.as_mut() {
            for it in &mut g.items {
                it.x = n(it.x.to_number() + e("l"));
                it.y = n(it.y.to_number() + e("t"));
            }
            g.width_twips = n(g.width_twips.to_number() + (e("l") + e("r")));
            g.height_twips = n(g.height_twips.to_number() + (e("t") + e("b")));
        }
    }
    if let Some(anchor) = &anchor {
        if let Some(t) = run.image.as_mut() {
            float_to!(t, anchor);
            if anchor.behind_doc == Some(true) {
                put!(t, behind_doc, "behindDoc", true);
            }
        } else if let Some(t) = run.textbox.as_mut() {
            float_to!(t, anchor);
            if anchor.behind_doc == Some(true) {
                put!(t, behind_doc, "behindDoc", true);
            }
        } else if let Some(t) = run.shape.as_mut() {
            float_to!(t, anchor);
        } else if let Some(t) = run.group.as_mut() {
            float_to!(t, anchor);
        }
    }
    let doc_pr = container.first_child("wp:docPr");
    if doc_pr.is_some_and(|d| is_on(d.attr("hidden"))) {
        if let Some(t) = run.textbox.as_mut() {
            put!(t, hidden, "hidden", true);
        } else if let Some(t) = run.shape.as_mut() {
            put!(t, hidden, "hidden", true);
        } else if let Some(t) = run.group.as_mut() {
            put!(t, hidden, "hidden", true);
        }
    }
    if let Some(raw) = raw {
        if let Some(t) = run.shape.as_mut() {
            put!(t, raw_drawing_xml, "rawDrawingXml", raw.into());
        }
        if let Some(t) = run.group.as_mut() {
            put!(t, raw_drawing_xml, "rawDrawingXml", raw.into());
        }
    }
    if let Some(t) = run.image.as_mut() {
        if let Some(anchor) = &anchor {
            rel_fields!(t, anchor);
        }
        if let Some(raw) = raw {
            put!(t, raw_drawing_xml, "rawDrawingXml", raw.into());
        }
    }
    if let Some(t) = run.textbox.as_mut() {
        if let Some(anchor) = &anchor {
            rel_fields!(t, anchor);
        }
        if let Some(raw) = raw {
            put!(t, raw_drawing_xml, "rawDrawingXml", raw.into());
        }
    }
    if let (Some(img), Some(raw)) = (run.image.as_mut(), raw)
        && let Some(fmt) = read_picture_format(raw, false)
    {
        put!(img, format, "format", Box::new(fmt));
    }
}

// ───────────────────────── sync from the raw XML ─────────────────────────

/// The host of the sync functions: the `ctx` the TS builds there (`resolveImage` answers the
/// image's own bytes for any id, no parts, no block reader). The paragraphs a text box would read
/// are discarded by the sync (their keys are kept), so none are read.
struct SyncHost {
    ids: IdAlloc,
    image: Option<(MediaId, Str)>,
    depth: u32,
}

impl SyncHost {
    fn new(image: Option<(MediaId, Str)>) -> SyncHost {
        SyncHost { ids: IdAlloc::new(), image, depth: 0 }
    }
}

impl ObjectHost for SyncHost {
    fn ids(&mut self) -> &mut IdAlloc {
        &mut self.ids
    }

    fn resolve_image(&mut self, _rid: &str) -> Result<Option<ResolvedImage>, DocxError> {
        Ok(self.image.clone().map(|(data, ext)| ResolvedImage { data, ext }))
    }

    fn add_media(&mut self, _bytes: &[u8]) -> MediaId {
        self.image.as_ref().map(|i| i.0).unwrap_or_default()
    }

    fn resolve_part(&mut self, _rid: &str) -> Result<Option<XmlDocument>, DocxError> {
        Ok(None)
    }

    fn enter_nested(&mut self) -> bool {
        if self.depth >= crate::limits::DocxLimits::DEFAULT.max_nesting {
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave_nested(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn read_textbox_content(&mut self, _content: Element<'_>) -> Result<TextboxRead, DocxError> {
        Ok(TextboxRead::Paragraphs(Vec::new()))
    }
}

/// The key names of an object in insertion order.
fn key_order_of<T: JsObject>(o: &T) -> Vec<u8> {
    let mut out = Vec::new();
    o.for_each_in_order(&mut |k, _| {
        if let Some(i) = T::key_index(k) {
            out.push(i);
        }
    });
    out
}

/// `for (const k of new Set([...Object.keys(dst), ...Object.keys(src)])) { if (keep.has(k))
/// continue; if (src[k] === undefined) delete dst[k]; else dst[k] = src[k]; }`.
fn sync_keys<T: JsAssign>(dst: &mut T, src: &T, keep: &[&str]) {
    let mut order = key_order_of(dst);
    let mut all = order.clone();
    for i in key_order_of(src) {
        if !all.contains(&i) {
            all.push(i);
        }
    }
    for i in all {
        let name = T::keys().get(usize::from(i)).copied().unwrap_or("");
        if keep.contains(&name) {
            continue;
        }
        let idx = usize::from(i);
        if matches!(src.slot(idx), Slot::Val(_)) {
            if !order.contains(&i) {
                order.push(i);
            }
            dst.assign_slot(src, idx);
        } else {
            dst.remove_slot(idx);
            order.retain(|&x| x != i);
        }
    }
    let mut ko = KeyOrder::default();
    for i in order {
        ko.touch(i);
    }
    *dst.key_order_mut() = ko;
}

/// `syncImageFromXml(img)`: re-reads the layout fields of an image from its raw XML after an edit
/// (size, anchor, wrap, crop, rotation, effects, format). Nothing changes when the raw text is
/// missing or does not parse (the TS `try`), or holds no picture.
pub fn sync_image_from_xml(img: &mut InlineImage) {
    let Some(raw) = img.raw_drawing_xml.clone().filter(|r| !r.is_empty()) else { return };
    let Ok(doc) = uo_opc::xml::parse(&*raw, &DOCX) else { return };
    let mut host = SyncHost::new(Some((img.data.0, img.ext.clone())));
    let Ok(Some(run)) = read_drawing_run(doc.root(), Some(&*raw), &mut host) else { return };
    let Some(fresh) = run.image else { return };
    const KEYS: [&str; 14] = [
        "widthTwips",
        "heightTwips",
        "floating",
        "offsetXTwips",
        "offsetYTwips",
        "relH",
        "relV",
        "behindDoc",
        "anchor",
        "inlineEffect",
        "crop",
        "style",
        "effects",
        "format",
    ];
    let mut order = key_order_of(img);
    for k in KEYS {
        let Some(i) = InlineImage::key_index(k) else { continue };
        let idx = usize::from(i);
        if matches!(fresh.slot(idx), Slot::Val(_)) {
            if !order.contains(&i) {
                order.push(i);
            }
            img.assign_slot(&fresh, idx);
        } else {
            img.remove_slot(idx);
            order.retain(|&x| x != i);
        }
    }
    let mut ko = KeyOrder::default();
    for i in order {
        ko.touch(i);
    }
    img.keys = ko;
}

/// `syncShapeFromXml(run)`: re-reads the layout fields of a text box or shape (and, for a run
/// that only has a group, of the group) from its raw XML. The content, the raw XML, `edited` and,
/// for a VML box, `vml`, `vmlId`, `vmlNext`, `story` and `link` stay.
///
/// Item 9 of the declarations: a text box whose raw XML now reads as a shape (or the reverse) is
/// left as it is; the TS mixes the keys of the two kinds into one object, which the model cannot
/// represent.
pub fn sync_shape_from_xml(run: &mut RunExt) {
    if run.group.is_some() && run.textbox.is_none() && run.shape.is_none() {
        if let Some(g) = run.group.as_mut() {
            sync_group_from_xml(g);
        }
        return;
    }
    let raw = match (&run.textbox, &run.shape) {
        (Some(t), _) => t.raw_drawing_xml.clone(),
        (None, Some(s)) => s.raw_drawing_xml.clone(),
        _ => None,
    };
    let Some(raw) = raw.filter(|r| !r.is_empty()) else { return };
    let Ok(doc) = uo_opc::xml::parse(&*raw, &DOCX) else { return };
    let mut host = SyncHost::new(None);
    let Ok(Some(fresh)) = read_drawing_run(doc.root(), Some(&*raw), &mut host) else { return };
    let blip = match (&run.textbox, &run.shape) {
        (Some(t), _) => t.shape.as_ref().and_then(|s| s.blip.clone()),
        (None, Some(s)) => s.style.blip.clone(),
        _ => None,
    };
    if let Some(dst) = run.textbox.as_mut() {
        let Some(src) = fresh.textbox.as_ref() else { return };
        let mut keep: Vec<&str> = vec!["rawDrawingXml", "edited", "vml", "vmlId", "vmlNext"];
        if dst.vml == Some(true) {
            keep.extend(["story", "link"]);
        }
        sync_keys(&mut **dst, &**src, &keep);
    } else if let Some(dst) = run.shape.as_mut() {
        let Some(src) = fresh.shape.as_ref() else { return };
        sync_keys(&mut **dst, &**src, &["rawDrawingXml", "edited"]);
    }
    if let Some(blip) = blip {
        let has_blip_fill = find_lit_then_boundary(&raw, "<a:blipFill").is_some();
        let st: Option<&mut ShapeStyle> =
            if let Some(t) = run.textbox.as_mut() { t.shape.as_deref_mut() } else { run.shape.as_mut().map(|s| &mut s.style) };
        if let Some(st) = st
            && st.blip.is_none()
            && has_blip_fill
        {
            st.blip = Some(blip);
            st.mark("blip");
        }
    }
}

/// `syncGroupFromXml(g)`: anchor, size, rotation/flips and hidden re-read from the raw XML; the
/// items are rescaled to the new size.
fn sync_group_from_xml(g: &mut DrawingGroup) {
    let Some(raw) = g.raw_drawing_xml.clone().filter(|r| !r.is_empty()) else { return };
    let Ok(doc) = uo_opc::xml::parse(&*raw, &DOCX) else { return };
    let mut host = SyncHost::new(None);
    let Ok(Some(run)) = read_drawing_run(doc.root(), Some(&*raw), &mut host) else { return };
    let Some(fresh) = run.group else { return };
    let gw = g.width_twips.to_number();
    let gh = g.height_twips.to_number();
    let sx = if truthy(gw) { fresh.width_twips.to_number() / gw } else { 1.0 };
    let sy = if truthy(gh) { fresh.height_twips.to_number() / gh } else { 1.0 };
    if sx != 1.0 || sy != 1.0 {
        for it in &mut g.items {
            it.x = n(it.x.to_number() * sx);
            it.y = n(it.y.to_number() * sy);
            it.w = n(it.w.to_number() * sx);
            it.h = n(it.h.to_number() * sy);
            let Some(ext) = it.run.ext.as_mut() else { continue };
            if let Some(o) = ext.textbox.as_mut() {
                o.width_twips = n(o.width_twips.to_number() * sx);
                o.height_twips = n(o.height_twips.to_number() * sy);
            } else if let Some(o) = ext.shape.as_mut() {
                o.width_twips = n(o.width_twips.to_number() * sx);
                o.height_twips = n(o.height_twips.to_number() * sy);
            } else if let Some(o) = ext.image.as_mut() {
                o.width_twips = n(o.width_twips.to_number() * sx);
                o.height_twips = n(o.height_twips.to_number() * sy);
            }
        }
    }
    sync_keys(g, &fresh, &["items", "rawDrawingXml", "edited"]);
}

// ───────────────────────── VML ─────────────────────────

/// `{ ...st, position: 'absolute' }` (the style is re-parsed from its declarations).
fn with_position_absolute(st: &VmlStyle) -> VmlStyle {
    let mut text = String::new();
    for (k, v) in st.entries() {
        text.push_str(k);
        text.push(':');
        text.push_str(v);
        text.push(';');
    }
    text.push_str("position:absolute");
    parse_vml_style(Some(&text))
}

/// `/bold|[6-9]00/.test(s)`.
#[must_use]
pub fn has_bold_weight(s: &str) -> bool {
    s.contains("bold") || s.as_bytes().windows(3).any(|w| matches!(w, [b'6'..=b'9', b'0', b'0']))
}

/// `/\/drawingml\/2006\/diagram$/.test(uri)`: the `a:graphicData` uri of a SmartArt.
#[must_use]
pub fn is_diagram_uri(uri: &str) -> bool {
    uri.ends_with("/drawingml/2006/diagram")
}

/// `next.replace(/^#/, '')`.
#[must_use]
pub fn strip_leading_hash(s: &str) -> &str {
    s.strip_prefix('#').unwrap_or(s)
}

/// `el.name.replace(/^v:/, '')`.
#[must_use]
pub fn strip_vml_prefix(name: &str) -> &str {
    name.strip_prefix("v:").unwrap_or(name)
}

fn first_target_width(r: &RunExt) -> (f64, f64) {
    if let Some(i) = &r.image {
        (i.width_twips.to_number(), i.height_twips.to_number())
    } else if let Some(t) = &r.textbox {
        (t.width_twips.to_number(), t.height_twips.to_number())
    } else if let Some(s) = &r.shape {
        (s.width_twips.to_number(), s.height_twips.to_number())
    } else {
        (0.0, 0.0)
    }
}

/// A VML text box between [`vml_shape_start`] and the read of its content.
struct VmlTextbox<'d> {
    tb: Box<DrawingTextbox>,
    st: VmlStyle,
    anchor: Option<DrawingAnchor>,
    content: Element<'d>,
    txb: Element<'d>,
}

/// What [`vml_shape_start`] found.
enum VmlStart<'d> {
    /// the object (or nothing): no text box content to read
    Done(Option<Box<RunExt>>),
    /// a text box: its content is read by [`vml_shape_of`], then [`vml_textbox_finish`]
    Textbox(Box<VmlTextbox<'d>>),
}

/// `shapeOf(el, inGroup?)` of `readVmlRuns`, boxed. The recursion of nested stories goes through
/// here (a text box's paragraphs hold `w:pict`s): a small frame, the rest is in
/// [`vml_shape_start`] and [`vml_textbox_finish`], which return before or start after the read
/// of the content.
#[inline(never)]
fn vml_shape_of(
    container: Element<'_>,
    el: Element<'_>,
    in_group: Option<(f64, f64)>,
    host: &mut dyn ObjectHost,
) -> Result<Option<Box<RunExt>>, DocxError> {
    let mut start = match vml_shape_start(el, in_group, host)? {
        VmlStart::Done(run) => return Ok(run),
        VmlStart::Textbox(start) => start,
    };
    textbox_blocks(start.content, host, &mut start.tb)?;
    vml_textbox_finish(container, el, in_group, start).map(Some)
}

/// `float`, `anchor`, `behindDoc` and the relative position of a floating VML object.
fn apply_vml_anchor(run: &mut RunExt, anchor: &DrawingAnchor) {
    if let Some(t) = run.image.as_mut() {
        float_to!(t, anchor);
        if anchor.behind_doc == Some(true) {
            put!(t, behind_doc, "behindDoc", true);
        }
        rel_fields!(t, anchor);
    } else if let Some(t) = run.textbox.as_mut() {
        float_to!(t, anchor);
        if anchor.behind_doc == Some(true) {
            put!(t, behind_doc, "behindDoc", true);
        }
        rel_fields!(t, anchor);
    } else if let Some(t) = run.shape.as_mut() {
        float_to!(t, anchor);
    }
}

/// The text box of `shapeOf` after its content: insets, fit, wrap, flow, anchor, ids, shape
/// style and the DrawingML twin of a lone text box.
#[inline(never)]
fn vml_textbox_finish(
    container: Element<'_>,
    el: Element<'_>,
    in_group: Option<(f64, f64)>,
    start: Box<VmlTextbox<'_>>,
) -> Result<Box<RunExt>, DocxError> {
    let VmlTextbox { mut tb, st, anchor, txb, .. } = *start;
    let inset: Vec<&str> = txb.attr("inset").unwrap_or("").split(',').collect();
    let ins = |i: usize, def: f64| -> f64 {
        match inset.get(i).map(|s| trim(s)).filter(|s| !s.is_empty()) {
            Some(t) => vml_len_twips(Some(t)).unwrap_or(def),
            None => def,
        }
    };
    put!(
        tb,
        insets,
        "insets",
        Ltrb { l: n(ins(0, 144.0)), t: n(ins(1, 72.0)), r: n(ins(2, 144.0)), b: n(ins(3, 72.0)), ..Ltrb::default() }
    );
    let tb_style = parse_vml_style(txb.attr("style"));
    if tb_style.get("mso-fit-shape-to-text") == Some("t") {
        put!(tb, auto_fit, "autoFit", true);
    }
    if parse_vml_style(el.attr("style")).get("mso-wrap-style") == Some("none") {
        put!(tb, no_wrap, "noWrap", true);
    }
    let flow = tb_style.get("layout-flow");
    let alt = tb_style.get("mso-layout-flow-alt");
    if flow == Some("vertical") {
        let v = match alt {
            Some("bottom-to-top") => "vert270",
            Some("top-to-bottom") => "mongolianVert",
            _ => "vert",
        };
        put!(tb, vert, "vert", v.into());
    } else if flow == Some("vertical-ideographic") {
        put!(tb, vert, "vert", "eaVert".into());
    }
    match st.get("v-text-anchor") {
        Some("middle" | "middle-center" | "middle-baseline") => put!(tb, v_anchor, "vAnchor", "ctr".into()),
        Some("bottom" | "bottom-center" | "bottom-baseline") => put!(tb, v_anchor, "vAnchor", "b".into()),
        _ => {}
    }
    if let Some(id) = el.attr("id").filter(|s| !s.is_empty()) {
        put!(tb, vml_id, "vmlId", id.into());
    }
    if let Some(next) = tb_style.get("mso-next-textbox").filter(|s| !s.is_empty()) {
        put!(tb, vml_next, "vmlNext", strip_leading_hash(next).into());
    }
    let sh = vml_shape_style(el, "rect");
    if sh.fill.is_some() || sh.line.is_some() {
        put!(tb, shape, "shape", Box::new(sh));
    }
    if in_group.is_none() && vml_single_textbox(container) == Some(el) {
        let digits = keep_ascii_digits(el.attr("o:spid").unwrap_or(""));
        let id = 1.0 + or0(parse_int(&digits, 10));
        if let Some(raw) = vml_textbox_to_drawing(el, id, VmlLang::default())?.filter(|r| !r.is_empty()) {
            put!(tb, raw_drawing_xml, "rawDrawingXml", raw.into());
        }
    }
    let mut run = RunExt { textbox: Some(tb), ..RunExt::default() };
    if let Some(anchor) = &anchor {
        apply_vml_anchor(&mut run, anchor);
    }
    Ok(Box::new(run))
}

/// The part of `shapeOf` before a text box's content: the whole object when it is not a text box.
#[inline(never)]
fn vml_shape_start<'d>(el: Element<'d>, in_group: Option<(f64, f64)>, host: &mut dyn ObjectHost) -> Result<VmlStart<'d>, DocxError> {
    let st = parse_vml_style(el.attr("style"));
    if st.get("visibility") == Some("hidden") || st.get("display") == Some("none") {
        return Ok(VmlStart::Done(None));
    }
    let mut w = vml_len_twips(st.get("width")).unwrap_or(0.0);
    let mut h = vml_len_twips(st.get("height")).unwrap_or(0.0);
    let mut anchor: Option<DrawingAnchor> = if in_group.is_some() { None } else { vml_anchor(&st, el.descend("w10:wrap")) };
    if let Some((sx, sy)) = in_group {
        w = or0(parse_float(st.get("width").unwrap_or("0"))) * sx;
        h = or0(parse_float(st.get("height").unwrap_or("0"))) * sy;
    }
    if let Some(textpath) = el.descend("v:textpath") {
        let text = textpath.attr("string").unwrap_or("");
        if trim(text).is_empty() {
            return Ok(VmlStart::Done(None));
        }
        let tps = parse_vml_style(textpath.attr("style"));
        let fam_raw = { trim(strip_quotes(tps.get("font-family").unwrap_or(""))).to_string() };
        let sh = vml_shape_style(el, "rect");
        let color: Str = sh.fill.as_ref().map_or_else(|| "000000".to_string(), |c| c.0.to_ts_string()).into();
        let a = sh.fill_alpha;
        let mut rpr = RPr::default();
        if !fam_raw.is_empty() {
            let mut rf = RFonts::default();
            put!(rf, ascii, "ascii", fam_raw.as_str().into());
            put!(rf, h_ansi, "hAnsi", fam_raw.as_str().into());
            put!(rf, cs, "cs", fam_raw.as_str().into());
            put!(rpr, r_fonts, "rFonts", Box::new(rf));
        }
        put!(rpr, color, "color", Box::new(ColorSpec { val: Some(color.clone()), ..ColorSpec::default() }));
        put!(rpr, sz, "sz", n(72.0));
        if let Some(av) = a.map(JsNumber::to_number)
            && av < 1.0
        {
            put!(
                rpr,
                text_fill,
                "textFill",
                Box::new(crate::model::props::TextFill {
                    color: Some(color.clone()),
                    alpha: Some(n(math::max(0.0, av))),
                    ..crate::model::props::TextFill::default()
                })
            );
        }
        if has_bold_weight(tps.get("font-weight").unwrap_or("")) {
            put!(rpr, b, "b", true);
        }
        if tps.get("font-style").unwrap_or("").contains("italic") {
            put!(rpr, i, "i", true);
        }
        let mut wppr = PPr::default();
        put!(wppr, jc, "jc", "center".into());
        put!(
            wppr,
            spacing,
            "spacing",
            Box::new(SpacingSpec {
                before: Some(n(0.0)),
                after: Some(n(0.0)),
                line: Some(n(240.0)),
                line_rule: Some("auto".into()),
                ..SpacingSpec::default()
            })
        );
        let mut para = Paragraph::empty(host.ids().para());
        para.runs = vec![Run {
            text: Utf16::from(text),
            props: Arc::new(run_props_from_rpr(Some(&rpr))),
            rpr: Some(Arc::new(rpr)),
            ..Run::default()
        }];
        para.props = Arc::new(para_props_from_ppr(Some(&wppr)));
        para.ppr = Some(Arc::new(wppr));
        let mut tb = DrawingTextbox { width_twips: n(w), height_twips: n(h), drawing: host.ids().drawing(), ..DrawingTextbox::default() };
        let mut story = Story::new();
        story.push_paragraph(para);
        tb.content = story;
        tb.view = TextboxView::Synced;
        tb.mark("widthTwips");
        tb.mark("heightTwips");
        put!(tb, vml, "vml", true);
        put!(tb, insets, "insets", Ltrb { l: n(0.0), t: n(0.0), r: n(0.0), b: n(0.0), ..Ltrb::default() });
        put!(tb, v_anchor, "vAnchor", "ctr".into());
        put!(tb, word_art, "wordArt", "textPlain".into());
        let rot = parse_float(st.get("rotation").unwrap_or("0"));
        if truthy(rot) {
            put!(tb, rot, "rot", n(rot));
        }
        if let Some(anchor) = &anchor {
            float_to!(tb, anchor);
            if anchor.behind_doc == Some(true) {
                put!(tb, behind_doc, "behindDoc", true);
            }
            rel_fields!(tb, anchor);
        }
        return Ok(VmlStart::Done(Some(Box::new(RunExt { textbox: Some(Box::new(tb)), ..RunExt::default() }))));
    }
    let imagedata = el.first_child("v:imagedata");
    let rid = imagedata.and_then(|i| i.attr("r:id").or_else(|| i.attr("r:pict"))).filter(|r| !r.is_empty());
    let mut run: RunExt;
    if let (Some(rid), Some(imagedata)) = (rid, imagedata) {
        let Some(media) = host.resolve_image(rid)? else { return Ok(VmlStart::Done(None)) };
        let mut image = InlineImage {
            data: MediaRef(media.data),
            ext: media.ext,
            width_twips: n(w),
            height_twips: n(h),
            drawing: host.ids().drawing(),
            ..InlineImage::default()
        };
        image.mark("data");
        image.mark("ext");
        image.mark("widthTwips");
        image.mark("heightTwips");
        put!(image, vml, "vml", true);
        let crop = |k: &str| -> f64 {
            match imagedata.attr(k).filter(|v| !v.is_empty()) {
                None => 0.0,
                Some(v) => {
                    if v.ends_with('f') {
                        parse_int(v, 10) / 65536.0
                    } else {
                        parse_float(v)
                    }
                }
            }
        };
        let (cl, ct, cr, cb) = (crop("cropleft"), crop("croptop"), crop("cropright"), crop("cropbottom"));
        if truthy(cl) || truthy(ct) || truthy(cr) || truthy(cb) {
            put!(image, crop, "crop", Ltrb { l: n(cl), t: n(ct), r: n(cr), b: n(cb), ..Ltrb::default() });
        }
        if let Some(fx) = vml_image_effects(imagedata) {
            put!(image, effects, "effects", fx);
        }
        run = RunExt { image: Some(Box::new(image)), ..RunExt::default() };
    } else {
        let txb = el.first_child("v:textbox");
        let content = txb.and_then(|t| t.first_child("w:txbxContent"));
        if let (Some(content), Some(txb)) = (content, txb) {
            let mut tb = new_textbox(w, h, host);
            put!(tb, vml, "vml", true);
            return Ok(VmlStart::Textbox(Box::new(VmlTextbox { tb, st, anchor, content, txb })));
        } else {
            let tag = strip_vml_prefix(el.name());
            let ty = el.attr("type").unwrap_or("");
            let declared = el.attr("fillcolor").is_some_and(|s| !s.is_empty())
                || el.attr("strokecolor").is_some_and(|s| !s.is_empty())
                || el.attr("filled").is_some_and(|s| !s.is_empty())
                || el.attr("stroked").is_some_and(|s| !s.is_empty())
                || el.first_child("v:fill").is_some()
                || el.first_child("v:stroke").is_some();
            if tag == "shape" && (ty.contains("_x0000_t75") || !declared) {
                return Ok(VmlStart::Done(None));
            }
            let geom = if tag == "oval" {
                "ellipse"
            } else if tag == "roundrect" {
                "roundRect"
            } else if tag == "line" || ty.contains("_x0000_t32") {
                "line"
            } else {
                "rect"
            };
            let mut style = vml_shape_style(el, geom);
            if geom == "line" {
                style.fill = None;
                forget(&mut style, "fill");
            }
            let mut shape =
                DrawingShape { style, width_twips: n(w), height_twips: n(h), drawing: host.ids().drawing(), ..DrawingShape::default() };
            shape.mark("style");
            shape.mark("widthTwips");
            shape.mark("heightTwips");
            if el.attr("o:hr") == Some("t") {
                put!(shape, hr, "hr", true);
                let pct = parse_int(el.attr("o:hrpct").unwrap_or(""), 10);
                if pct > 0.0 {
                    put!(shape, hr_pct, "hrPct", n(pct));
                }
                if el.attr("o:hrnoshade") == Some("t") {
                    put!(shape, hr_no_shade, "hrNoShade", true);
                }
                if let Some(a) = el.attr("o:hralign").filter(|s| !s.is_empty()) {
                    put!(shape, hr_align, "hrAlign", a.into());
                }
            }
            if tag == "line" {
                let pt = |s: Option<&str>| -> Vec<f64> {
                    s.unwrap_or("0,0").split(',').map(|v| vml_len_twips(Some(trim(v))).unwrap_or(0.0)).collect()
                };
                let from = pt(el.attr("from"));
                let to = pt(el.attr("to"));
                let at = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(0.0);
                let (x1, y1, x2, y2) = (at(&from, 0), at(&from, 1), at(&to, 0), at(&to, 1));
                put!(shape, line, "line", VmlLine { x1: n(x1), y1: n(y1), x2: n(x2), y2: n(y2), ..VmlLine::default() });
                shape.width_twips = n(math::abs(x2 - x1));
                shape.height_twips = n(math::abs(y2 - y1));
                if anchor.is_none() && in_group.is_none() {
                    anchor = vml_anchor(&with_position_absolute(&st), None);
                }
            }
            run = RunExt { shape: Some(Box::new(shape)), ..RunExt::default() };
        }
    }
    if let Some(anchor) = &anchor {
        apply_vml_anchor(&mut run, anchor);
    }
    Ok(VmlStart::Done(Some(Box::new(run))))
}

/// `readVmlRuns(container, ctx, inline)`: a `w:pict` or `w:object` as object runs; `inline` for
/// `w:object` (the OLE preview is in line and carries its embedded file).
pub fn read_vml_runs(container: Element<'_>, inline: bool, host: &mut dyn ObjectHost) -> Result<Vec<RunExt>, DocxError> {
    Ok(vml_runs(container, inline, host)?.into_iter().map(|run| *run).collect())
}

/// [`read_vml_runs`], boxed. The recursion of nested stories goes through here (a text box's
/// paragraphs hold `w:pict`s): a small frame.
#[expect(clippy::vec_box, reason = "each box becomes the `ext` of a run as it is, with no copy of the `RunExt`")]
#[inline(never)]
fn vml_runs(container: Element<'_>, inline: bool, host: &mut dyn ObjectHost) -> Result<Vec<Box<RunExt>>, DocxError> {
    let mut out: Vec<Box<RunExt>> = Vec::new();
    for el in container.child_elements() {
        let name = el.name();
        if name == "v:shapetype" || name.starts_with("o:") || name.starts_with("w10:") {
            continue;
        }
        if name == "v:group" {
            if let Some(run) = vml_group(el, host, container)? {
                out.push(run);
            }
            continue;
        }
        if name.starts_with("v:") {
            let Some(mut r) = vml_shape_of(container, el, None, host)? else { continue };
            if inline && r.image.is_some() {
                inline_ole_image(&mut r, container, host)?;
            }
            out.push(r);
        }
    }
    Ok(out)
}

/// The preview picture of a `w:object`: in line, with its embedded file (`o:OLEObject`).
#[inline(never)]
fn inline_ole_image(r: &mut RunExt, container: Element<'_>, host: &mut dyn ObjectHost) -> Result<(), DocxError> {
    let Some(img) = r.image.as_mut() else { return Ok(()) };
    img.floating = None;
    forget(&mut **img, "floating");
    img.anchor = None;
    forget(&mut **img, "anchor");
    let oo = container.descend("o:OLEObject");
    let rid = oo.and_then(|o| o.attr("r:id")).filter(|r| !r.is_empty());
    if let (Some(oo), Some(rid)) = (oo, rid)
        && oo.attr("Type") != Some("Link")
        && let Some(part) = host.resolve_image(rid)?
    {
        put!(
            img,
            ole,
            "ole",
            Box::new(OleData {
                prog_id: oo.attr("ProgID").unwrap_or("").into(),
                data: MediaRef(part.data),
                ext: part.ext,
                ..OleData::default()
            })
        );
    }
    Ok(())
}

/// The size of a `v:group` in twips, the scale of its children and its coordinate origin.
#[derive(Clone, Copy)]
struct VmlGroupGeom {
    w: f64,
    h: f64,
    sx: f64,
    sy: f64,
    origin_x: f64,
    origin_y: f64,
}

/// The geometry of a `v:group` (`style`, `coordsize`, `coordorigin`).
#[inline(never)]
fn vml_group_geom(el: Element<'_>) -> VmlGroupGeom {
    let st = parse_vml_style(el.attr("style"));
    let w = vml_len_twips(st.get("width")).unwrap_or(0.0);
    let h = vml_len_twips(st.get("height")).unwrap_or(0.0);
    let cs: Vec<f64> = el
        .attr("coordsize")
        .unwrap_or("1000,1000")
        .split(',')
        .map(|v| {
            let x = parse_float(v);
            if truthy(x) { x } else { 1000.0 }
        })
        .collect();
    let co: Vec<f64> = el.attr("coordorigin").unwrap_or("0,0").split(',').map(|v| or0(parse_float(v))).collect();
    let cs0 = cs.first().copied().unwrap_or(1000.0);
    let cs1 = cs.get(1).copied();
    let sx = w / (if truthy(cs0) { cs0 } else { 1.0 });
    let sy = h
        / (match cs1 {
            Some(v) if truthy(v) => v,
            _ => 1.0,
        });
    VmlGroupGeom { w, h, sx, sy, origin_x: co.first().copied().unwrap_or(0.0), origin_y: co.get(1).copied().unwrap_or(0.0) }
}

/// One child of a `v:group` as a group item (`left`/`top` of its style, scaled).
#[inline(never)]
fn push_vml_item(items: &mut Vec<GroupItem>, c: Element<'_>, g: VmlGroupGeom, r: Box<RunExt>, host: &mut dyn ObjectHost) {
    let cst = parse_vml_style(c.attr("style"));
    let x = (or0(parse_float(cst.get("left").unwrap_or("0"))) - g.origin_x) * g.sx;
    let y = (or0(parse_float(cst.get("top").unwrap_or("0"))) - g.origin_y) * g.sy;
    let (tw, th) = first_target_width(&r);
    items.push(GroupItem { x: n(x), y: n(y), w: n(tw), h: n(th), run: item_run(r, host), ..GroupItem::default() });
}

/// A `v:group`: its VML children as group items. A small frame: a text box in a group is on the
/// recursion of nested stories.
#[inline(never)]
fn vml_group(el: Element<'_>, host: &mut dyn ObjectHost, container: Element<'_>) -> Result<Option<Box<RunExt>>, DocxError> {
    let g = vml_group_geom(el);
    let mut items: Vec<GroupItem> = Vec::new();
    for c in el.child_elements() {
        if !c.name().starts_with("v:") || c.name() == "v:shapetype" {
            continue;
        }
        let Some(r) = vml_shape_of(container, c, Some((g.sx, g.sy)), host)? else { continue };
        push_vml_item(&mut items, c, g, r, host);
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(vml_group_run(el, g, items, host)))
}

/// The group of [`vml_group`] once its items are read: size, drawing id and anchor.
#[inline(never)]
fn vml_group_run(el: Element<'_>, g: VmlGroupGeom, items: Vec<GroupItem>, host: &mut dyn ObjectHost) -> Box<RunExt> {
    let mut group =
        DrawingGroup { width_twips: n(g.w), height_twips: n(g.h), items, drawing: host.ids().drawing(), ..DrawingGroup::default() };
    group.mark("widthTwips");
    group.mark("heightTwips");
    group.mark("items");
    if let Some(anchor) = vml_anchor(&parse_vml_style(el.attr("style")), el.descend("w10:wrap")) {
        float_to!(group, anchor);
    }
    Box::new(RunExt { group: Some(Box::new(group)), ..RunExt::default() })
}

// ───────────────────────── the host of the document reads ─────────────────────────

/// The host of the full-document and comments reads: R4c's `ReadContext` (relationships, media,
/// parts, the nesting guard). The content of a text box is read by the block reader the full
/// document injects (`BlocksMode::Blocks`/`Grouped`: R5's `fulldoc::read_textbox_content`); without one (`BlocksMode::None`, the comments) the
/// `w:p` children are read as paragraphs, as `textboxBlocks` does.
impl ObjectHost for ReadContext<'_, '_> {
    fn ids(&mut self) -> &mut IdAlloc {
        &mut self.ctx.ids
    }

    fn resolve_image(&mut self, rid: &str) -> Result<Option<ResolvedImage>, DocxError> {
        Ok(ReadContext::resolve_image(self, rid)?.map(|m| ResolvedImage { data: m.id, ext: m.ext }))
    }

    fn add_media(&mut self, bytes: &[u8]) -> MediaId {
        self.ctx.media.add(bytes)
    }

    fn resolve_part(&mut self, rid: &str) -> Result<Option<XmlDocument>, DocxError> {
        ReadContext::resolve_part(self, rid)
    }

    fn enter_nested(&mut self) -> bool {
        self.ctx.enter_nested()
    }

    fn leave_nested(&mut self) {
        self.ctx.leave_nested();
    }

    fn default_run_props(&mut self) -> Arc<RunProps> {
        ReadContext::default_run_props(self)
    }

    fn count_paragraph(&mut self) -> Result<(), DocxError> {
        self.ctx.count_paragraph()
    }

    fn count_run(&mut self) -> Result<(), DocxError> {
        self.ctx.count_run()
    }

    fn read_textbox_content(&mut self, content: Element<'_>) -> Result<TextboxRead, DocxError> {
        match self.blocks {
            BlocksMode::None => {
                let mut paras = Vec::new();
                for p in content.children_named("w:p") {
                    paras.push(self.read_paragraph(p)?);
                }
                Ok(TextboxRead::Paragraphs(paras))
            }
            BlocksMode::Blocks | BlocksMode::Grouped => {
                let (story, txbx_src) = super::fulldoc::read_textbox_content(self, content, self.blocks == BlocksMode::Grouped)?;
                Ok(TextboxRead::Blocks { story, txbx_src })
            }
        }
    }
}

/// The object readers as frontier R4c calls them (`readDrawingRun(drawing, ctx)`,
/// `readVmlRuns(container, ctx, inline)`): the object runs as [`Run`]s carrying their text and rare
/// fields (the props are the caller's: `pushRun`/`make`).
pub mod with_context {
    use super::{ObjectHost, ReadContext, RunExt, obj_text};
    use crate::error::DocxError;
    use crate::model::Run;
    use uo_opc::xml::Element;

    fn as_run(ext: Box<RunExt>) -> Run {
        Run { text: obj_text(), ext: Some(ext), ..Run::default() }
    }

    /// An object run of [`super::object_runs`] as a [`Run`] carrying its text and rare fields.
    pub(crate) fn object_run(ext: Box<RunExt>) -> Run {
        as_run(ext)
    }

    /// `readDrawingRun(drawing, ctx)`.
    #[inline(never)]
    pub fn read_drawing_run(drawing: Element<'_>, rc: &mut ReadContext<'_, '_>) -> Result<Option<Run>, DocxError> {
        Ok(super::drawing_run(drawing, drawing.raw(), rc as &mut dyn ObjectHost)?.map(as_run))
    }

    /// `readVmlRuns(container, ctx, inline)`.
    #[inline(never)]
    pub fn read_vml_runs(container: Element<'_>, rc: &mut ReadContext<'_, '_>, inline: bool) -> Result<Vec<Run>, DocxError> {
        Ok(super::vml_runs(container, inline, rc as &mut dyn ObjectHost)?.into_iter().map(as_run).collect())
    }
}

// ───────────────────────── the run-level dispatch ─────────────────────────

/// The object runs of one child of a `w:r` (`readSingleRun`'s `w:drawing`, `w:pict`/`w:object` and
/// `mc:AlternateContent` cases): the drawing (`mc:Choice`, then the VML of `mc:Fallback` when the
/// drawing reads as nothing). Any other element gives no runs.
pub fn read_object_runs(el: Element<'_>, host: &mut dyn ObjectHost) -> Result<Vec<RunExt>, DocxError> {
    Ok(object_runs(el, host)?.into_iter().map(|run| *run).collect())
}

/// [`read_object_runs`], boxed: what R4c's `run_object` reads (and turns into runs with
/// [`with_context::object_run`] as it pushes them). The recursion of nested stories goes through
/// here: a small frame.
#[expect(clippy::vec_box, reason = "each box becomes the `ext` of a run as it is, with no copy of the `RunExt`")]
#[inline(never)]
pub(crate) fn object_runs(el: Element<'_>, host: &mut dyn ObjectHost) -> Result<Vec<Box<RunExt>>, DocxError> {
    match el.name() {
        "w:drawing" => Ok(drawing_run(el, el.raw(), host)?.into_iter().collect()),
        "w:pict" | "w:object" => vml_runs(el, el.name() == "w:object", host),
        "mc:AlternateContent" => {
            let choice = el.first_child("mc:Choice");
            if let Some(d) = choice.and_then(|c| c.descend("w:drawing"))
                && let Some(r) = drawing_run(d, d.raw(), host)?
            {
                return Ok(vec![r]);
            }
            let fb = el.first_child("mc:Fallback");
            match fb.and_then(|f| f.descend("w:pict").or_else(|| f.descend("w:object"))) {
                Some(p) => vml_runs(p, p.name() == "w:object", host),
                None => Ok(Vec::new()),
            }
        }
        _ => Ok(Vec::new()),
    }
}
