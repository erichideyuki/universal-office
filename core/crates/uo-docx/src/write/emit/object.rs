//! Frontier **W2** (drawing objects; `serialize.ts` and `shape-xml.ts`): `drawingXml`,
//! `oleObjectXml` (`OLE_MIME`), `patchDrawingGeometry`, `pictureXml` (the `uw:media`/`uw:layer`
//! markers through [`crate::write::ctx::Writer::media_rel_id`]), `shapeOut`, `docPrName`,
//! `beginShapeContext` (`chainNext` by [`crate::ids::DrawingId`]), `shapeColor`,
//! `editedShapeXml`, `fillMedia`, `editedGroupXml`, `freshDrawingIds` (the per-document counter
//! [`crate::write::ctx::Writer::fresh_id`]); the `shape-xml.ts` write half (`shapeFillXml`,
//! `styleRefsXml`, `bodyPrXml`, `patchShape` and its helpers, `newTextboxDrawingXml`,
//! `hiddenLineXml`, `vmlPt`, `vmlColorOf`, `vmlFallback`, `textboxAlternateContent`,
//! `txbxContentOf`) with its read half (`readShapeFormat`: D2b needs it, R4b did not).
//!
//! Every regex of the TS is a hand matcher here (G9: linear, exact on any text; checked against V8
//! by `js/regex-sites/w2.mjs` and `tests/regex_w2.rs`). Offsets are UTF-8 byte offsets; each pattern
//! begins and ends with ASCII, so a match is the TS UTF-16 match. The declarations of the nine kinds
//! are in `core/docs/modules/uo-docx-d1b/W2.md`.

use std::collections::HashMap;
use std::sync::Arc;

use uo_jsnum::format::{to_js_string, to_radix_string};
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, utf16_slice};
use uo_opc::xml::Element;

use super::fx::{PictureChange, SPPR_ORDER, XmlPatch, dml_color_xml, patch_picture, pic_line_xml};
use crate::error::DocxError;
use crate::ids::DrawingId;
use crate::js::{JsNumber, JsRecord, Nullable, Str};
use crate::model::drawing::ShapeStyle;
use crate::model::media::MediaRef;
use crate::model::picture::{PicColor, PicLine, PictureFormat};
use crate::model::run::{DrawingTextbox, InlineImage, Run, TextboxView};
use crate::model::story::Block;
use crate::read::picture_format::{parse_drawing, parts_of, pic_bool, pic_int, read_dml_color, read_picture_format};
use crate::write::ctx::{MediaOut, Writer};
use crate::write::js::{esc_attr, or_num, truthy_str};
use crate::write::lossless::WResult;
use crate::write::matchers::{boundary_at, find_from};

/// `EMU_PER_TWIP` (`document.ts`).
const EMU_PER_TWIP: f64 = 635.0;
const REL_PACKAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package";
const REL_HDPHOTO: &str = "http://schemas.microsoft.com/office/2007/relationships/hdphoto";

/// The geometry `patchDrawingGeometry` writes back (`{ widthTwips, heightTwips, offsetXTwips?,
/// offsetYTwips? }` of an image or a textbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub width_twips: JsNumber,
    pub height_twips: JsNumber,
    pub offset_x_twips: Option<JsNumber>,
    pub offset_y_twips: Option<JsNumber>,
}

impl Geometry {
    #[must_use]
    pub fn of_image(i: &InlineImage) -> Geometry {
        Geometry {
            width_twips: i.width_twips,
            height_twips: i.height_twips,
            offset_x_twips: i.offset_x_twips,
            offset_y_twips: i.offset_y_twips,
        }
    }

    #[must_use]
    pub fn of_textbox(t: &DrawingTextbox) -> Geometry {
        Geometry {
            width_twips: t.width_twips,
            height_twips: t.height_twips,
            offset_x_twips: t.offset_x_twips,
            offset_y_twips: t.offset_y_twips,
        }
    }
}

// ───────────────────────────── regex sites (hand matchers) ─────────────────────────────

/// `s.slice(i)` bounds helper: the text from `i`, empty past the end (never panics).
fn tail(s: &str, i: usize) -> &str {
    s.get(i..).unwrap_or("")
}

/// The first `/<NAME\b[^>]*\/>/` match from byte `from`: `(start, end)`. A candidate is a `<NAME` that
/// ends at a `\b`; it matches when the first `>` after it is preceded by `/` (`[^>]*` cannot cross a
/// `>`, so the closing `/>` is the first `>`'s). `gts` memoises the `>` scan (linear over a text).
fn self_closing_tag_from(s: &str, name: &str, from: usize, gts: &mut crate::write::fonts::ooxml::NextFind<'_>) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let mut at = from;
    while let Some(i) = find_from(s, &open, at) {
        at = i + 1;
        let after = i + open.len();
        if !boundary_at(s, after) {
            continue;
        }
        let gt = gts.at(after)?;
        if gt > after && s.as_bytes().get(gt - 1) == Some(&b'/') {
            return Some((i, gt + 1));
        }
    }
    None
}

/// `tag.replace(/ATTR="\d+"/, () => `ATTR="${value}"`)`: the first `ATTR="` followed by digits and a
/// `"` has its digits replaced (`attr` is `cx` or `cy`).
fn replace_first_digit_attr(tag: &str, attr: &str, value: &str) -> String {
    let needle = format!("{attr}=\"");
    let mut at = 0;
    while let Some(i) = find_from(tag, &needle, at) {
        at = i + 1;
        let v = i + needle.len();
        let digits = tag.as_bytes().get(v..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits > 0 && tag.as_bytes().get(v + digits) == Some(&b'"') {
            return format!("{}{value}{}", tag.get(..v).unwrap_or(""), tail(tag, v + digits));
        }
    }
    tag.to_owned()
}

/// `raw.replace(/<wp:positionH\b[^>]*>[\s\S]*?<\/wp:positionH>/, (b) => b.replace(/(<wp:posOffset>)-?\d+(<\/wp:posOffset>)/, `$1${v}$2`))`
/// for `H` or `V`: the first holder's first numeric `wp:posOffset` replaced.
fn replace_pos_offset(raw: &str, axis: char, value: &str) -> String {
    let open = format!("<wp:position{axis}");
    let close = format!("</wp:position{axis}>");
    let mut at = 0;
    while let Some(i) = find_from(raw, &open, at) {
        at = i + 1;
        let after = i + open.len();
        if !boundary_at(raw, after) {
            continue;
        }
        let Some(gt) = find_from(raw, ">", after) else { return raw.to_owned() };
        let Some(c) = find_from(raw, &close, gt + 1) else { return raw.to_owned() };
        let end = c + close.len();
        let block = raw.get(i..end).unwrap_or("");
        return format!("{}{}{}", raw.get(..i).unwrap_or(""), replace_pos_offset_in(block, value), tail(raw, end));
    }
    raw.to_owned()
}

/// `b.replace(/(<wp:posOffset>)-?\d+(<\/wp:posOffset>)/, `$1${v}$2`)`.
fn replace_pos_offset_in(block: &str, value: &str) -> String {
    const OPEN: &str = "<wp:posOffset>";
    const CLOSE: &str = "</wp:posOffset>";
    let mut at = 0;
    while let Some(i) = find_from(block, OPEN, at) {
        at = i + 1;
        let v = i + OPEN.len();
        let mut j = v;
        if block.as_bytes().get(j) == Some(&b'-') {
            j += 1;
        }
        let digits = block.as_bytes().get(j..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits > 0 && tail(block, j + digits).starts_with(CLOSE) {
            return format!("{}{value}{}", block.get(..v).unwrap_or(""), tail(block, j + digits));
        }
    }
    block.to_owned()
}

/// `patchDrawingGeometry(raw, obj)`: the current size and position written into the raw
/// `w:drawing` (the first `wp:extent`, every self-closing `a:ext`, the first numeric `posOffset` of
/// each position holder that has an offset); idempotent when nothing changed.
pub fn patch_drawing_geometry(raw: &str, geo: &Geometry) -> WResult<String> {
    let cx = to_js_string(math::round(geo.width_twips.to_number() * EMU_PER_TWIP));
    let cy = to_js_string(math::round(geo.height_twips.to_number() * EMU_PER_TWIP));
    let with_size = |m: &str| replace_first_digit_attr(&replace_first_digit_attr(m, "cx", &cx), "cy", &cy);
    let mut gts = crate::write::fonts::ooxml::NextFind::new(raw, ">", false);
    let mut out = String::with_capacity(raw.len());
    // `.replace(/<wp:extent\b[^>]*\/>/, …)`: the first match only
    let mut base = String::new();
    let mut work: &str = raw;
    if let Some((s, e)) = self_closing_tag_from(raw, "wp:extent", 0, &mut gts) {
        base.push_str(raw.get(..s).unwrap_or(""));
        base.push_str(&with_size(raw.get(s..e).unwrap_or("")));
        base.push_str(tail(raw, e));
        work = &base;
    }
    // `.replace(/<a:ext\b[^>]*\/>/g, …)`: every match
    let mut gts2 = crate::write::fonts::ooxml::NextFind::new(work, ">", false);
    let mut last = 0;
    let mut from = 0;
    while let Some((s, e)) = self_closing_tag_from(work, "a:ext", from, &mut gts2) {
        out.push_str(work.get(last..s).unwrap_or(""));
        out.push_str(&with_size(work.get(s..e).unwrap_or("")));
        last = e;
        from = e;
    }
    out.push_str(tail(work, last));
    if let Some(x) = geo.offset_x_twips {
        out = replace_pos_offset(&out, 'H', &to_js_string(math::round(x.to_number() * EMU_PER_TWIP)));
    }
    if let Some(y) = geo.offset_y_twips {
        out = replace_pos_offset(&out, 'V', &to_js_string(math::round(y.to_number() * EMU_PER_TWIP)));
    }
    Ok(out)
}

/// The text between the quotes of the first `needle` (`NAME="`) at a `\b` followed by `[^"]*"`:
/// `Some((value_start, value_end))`.
fn quoted_value_after(s: &str, p_end: usize) -> Option<(usize, usize)> {
    let q = find_from(s, "\"", p_end)?;
    Some((p_end, q))
}

/// `/<wp:docPr\b[^>]*\bATTR…/` with the greedy `[^>]*`: the last position in the tag region where
/// `needle` (`ATTR="…`) sits at a `\b` and `accept(end_of_needle)` holds; the first `<wp:docPr`
/// candidate that has one decides. Linear: the regions of candidates that end at the same `>` nest, so
/// a region that has no such position is scanned once (the candidates inside it fail too).
fn doc_pr_greedy<R>(raw: &str, needle: &str, mut accept: impl FnMut(usize) -> Option<R>) -> Option<R> {
    const OPEN: &str = "<wp:docPr";
    let mut gts = crate::write::fonts::ooxml::NextFind::new(raw, ">", false);
    let mut failed_end: Option<usize> = None;
    let mut from = 0;
    while let Some(i) = find_from(raw, OPEN, from) {
        from = i + 1;
        let after = i + OPEN.len();
        if !boundary_at(raw, after) {
            continue;
        }
        let end = gts.at(after).unwrap_or(raw.len());
        if failed_end == Some(end) {
            continue;
        }
        let mut hi = end;
        while let Some(rel) = raw.get(after..hi).and_then(|r| r.rfind(needle)) {
            let p = after + rel;
            if boundary_at(raw, p)
                && let Some(r) = accept(p + needle.len())
            {
                return Some(r);
            }
            hi = p;
        }
        failed_end = Some(end);
    }
    None
}

/// `docPrName(raw)`: `/<wp:docPr\b[^>]*\bname="([^"]*)"/.exec(raw)?.[1]` (greedy: the last `name`
/// of the tag).
#[must_use]
pub fn doc_pr_name(raw: Option<&str>) -> Option<String> {
    let raw = raw.filter(|r| !r.is_empty())?;
    doc_pr_greedy(raw, "name=\"", |e| quoted_value_after(raw, e)).map(|(a, b)| raw.get(a..b).unwrap_or("").to_owned())
}

/// `/\bNAME="(\d+)"/.exec(raw)?.[1]` (the first match anywhere).
fn first_digit_attr<'s>(raw: &'s str, name: &str) -> Option<&'s str> {
    let needle = format!("{name}=\"");
    let mut at = 0;
    while let Some(i) = find_from(raw, &needle, at) {
        at = i + 1;
        if !boundary_at(raw, i) {
            continue;
        }
        let v = i + needle.len();
        let digits = raw.as_bytes().get(v..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits > 0 && raw.as_bytes().get(v + digits) == Some(&b'"') {
            return raw.get(v..v + digits);
        }
    }
    None
}

/// `/\bNAME="1"/.test(raw)`.
fn has_attr_one(raw: &str, name: &str) -> bool {
    let needle = format!("{name}=\"1\"");
    let mut at = 0;
    while let Some(i) = find_from(raw, &needle, at) {
        at = i + 1;
        if boundary_at(raw, i) {
            return true;
        }
    }
    false
}

/// `xml.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/g, …)` and
/// `.replace(/\s(?:wp14:anchorId|wp14:editId|o:spid)="[^"]*"/g, '')` of `freshDrawingIds`.
#[must_use]
pub fn fresh_ids_text(xml: &str, counter: &mut f64) -> String {
    const OPEN: &str = "<wp:docPr";
    let mut gts = crate::write::fonts::ooxml::NextFind::new(xml, ">", false);
    let mut ids = crate::write::fonts::ooxml::NextFind::new(xml, "id=\"", false);
    let mut with_ids = String::with_capacity(xml.len());
    let mut last = 0;
    let mut from = 0;
    // the regions of candidates that end at the same `>` nest: one with no `id="…"` leaves the rest of its
    // region without one too (linear where the lazy `[^>]*?` of V8 is quadratic)
    let mut failed_end: Option<usize> = None;
    while let Some(i) = find_from(xml, OPEN, from) {
        from = i + 1;
        let after = i + OPEN.len();
        if !boundary_at(xml, after) {
            continue;
        }
        let end = gts.at(after).unwrap_or(xml.len());
        if failed_end == Some(end) {
            continue;
        }
        let mut hit = None;
        let mut at = after;
        while let Some(p) = ids.at(at) {
            if p >= end {
                break;
            }
            at = p + 1;
            if !boundary_at(xml, p) {
                continue;
            }
            let v = p + 4;
            let digits = xml.as_bytes().get(v..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count());
            if digits > 0 && xml.as_bytes().get(v + digits) == Some(&b'"') {
                hit = Some((v, v + digits + 1));
                break;
            }
        }
        if let Some((v, e)) = hit {
            *counter += 1.0;
            with_ids.push_str(xml.get(last..v).unwrap_or(""));
            with_ids.push_str(&to_js_string(900_000.0 + *counter));
            with_ids.push('"');
            last = e;
            from = e;
        } else {
            failed_end = Some(end);
        }
    }
    with_ids.push_str(tail(xml, last));
    remove_anchor_attrs(&with_ids)
}

/// `/\s(?:wp14:anchorId|wp14:editId|o:spid)="[^"]*"/g` replaced by nothing.
fn remove_anchor_attrs(s: &str) -> String {
    const NAMES: [&str; 3] = ["wp14:anchorId=\"", "wp14:editId=\"", "o:spid=\""];
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    let mut skip_until = 0;
    let mut no_quote_left = false;
    for (i, c) in s.char_indices() {
        if i < skip_until || no_quote_left || !is_js_space(c) {
            continue;
        }
        let start = i + c.len_utf8();
        let rest = tail(s, start);
        let Some(n) = NAMES.iter().find(|n| rest.starts_with(**n)) else { continue };
        let Some(q) = find_from(s, "\"", start + n.len()) else {
            no_quote_left = true;
            continue;
        };
        out.push_str(s.get(last..i).unwrap_or(""));
        last = q + 1;
        skip_until = last;
    }
    out.push_str(tail(s, last));
    out
}

/// `freshDrawingIds(xml)`: a copy of an object gets new drawing ids (unique `wp:docPr` ids, from
/// the per-document counter) and loses the anchor and shape ids that must not repeat.
pub fn fresh_drawing_ids(w: &mut Writer<'_>, xml: &str) -> WResult<String> {
    Ok(fresh_ids_text(xml, w.fresh_id))
}

/// `onceShape(obj, xml)`: a rebuilt object already written in this save (a pasted copy) goes
/// out with new drawing ids.
pub fn once_shape(w: &mut Writer<'_>, obj: DrawingId, xml: String) -> WResult<String> {
    if w.cx.emitted_shapes.contains(&obj) {
        return fresh_drawing_ids(w, &xml);
    }
    w.cx.emitted_shapes.insert(obj);
    Ok(xml)
}

// ───────────────────────────── images and OLE objects ─────────────────────────────

fn image_of(run: &Run) -> WResult<&InlineImage> {
    run.ext().and_then(|e| e.image.as_deref()).ok_or(DocxError::Write("an image run without image (TS TypeError)"))
}

fn media_bytes(w: &Writer<'_>, r: MediaRef) -> WResult<Arc<[u8]>> {
    w.doc.media.get_arc(r.0).cloned().ok_or(DocxError::Write("a media reference outside the store (TS: undefined bytes)"))
}

/// `++drawingId` as the number the template prints.
fn next_drawing_id(w: &mut Writer<'_>) -> u32 {
    w.cx.drawing_id = w.cx.drawing_id.saturating_add(1);
    w.cx.drawing_id
}

/// `OLE_MIME[ext] ?? 'application/octet-stream'` (own keys only, W-D12 style).
fn ole_mime(ext: &str) -> &'static str {
    match ext {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// `drawingXml(run)`: a new inline picture (no raw drawing), queued in `mediaOut`.
pub fn drawing_xml<'a>(w: &mut Writer<'a>, run: &'a Run) -> WResult<String> {
    let img = image_of(run)?;
    let n = next_drawing_id(w);
    let rid = format!("rIdWnImg{n}");
    let data = media_bytes(w, img.data)?;
    w.cx.media_out.push(MediaOut {
        rid: rid.clone(),
        path: format!("word/media/wnimage{n}.{}", img.ext),
        data,
        ext: img.ext.to_string(),
        rel_type: None,
        mime: None,
    });
    let cx = to_js_string(math::round(img.width_twips.to_number() * EMU_PER_TWIP));
    let cy = to_js_string(math::round(img.height_twips.to_number() * EMU_PER_TWIP));
    Ok(format!(
        "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
<wp:docPr id=\"{n}\" name=\"Imagem {n}\"/>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"Imagem {n}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
    ))
}

/// `oleObjectXml(run)`: an embedded OLE object as Word writes it (`w:object` with the VML preview
/// and the `o:OLEObject` pointing at the package in `word/embeddings`).
pub fn ole_object_xml<'a>(w: &mut Writer<'a>, run: &'a Run) -> WResult<String> {
    let img = image_of(run)?;
    let ole = img.ole.as_deref().ok_or(DocxError::Write("an OLE run without ole (TS TypeError)"))?;
    let n = next_drawing_id(w);
    let (rid_img, rid_ole) = (format!("rIdWnImg{n}"), format!("rIdWnOle{n}"));
    let img_data = media_bytes(w, img.data)?;
    w.cx.media_out.push(MediaOut {
        rid: rid_img.clone(),
        path: format!("word/media/wnimage{n}.{}", img.ext),
        data: img_data,
        ext: img.ext.to_string(),
        rel_type: None,
        mime: None,
    });
    let ole_data = media_bytes(w, ole.data)?;
    w.cx.media_out.push(MediaOut {
        rid: rid_ole.clone(),
        path: format!("word/embeddings/wnobject{n}.{}", ole.ext),
        data: ole_data,
        ext: ole.ext.to_string(),
        rel_type: Some(REL_PACKAGE.to_owned()),
        mime: Some(ole_mime(&ole.ext).to_owned()),
    });
    let nf = f64::from(n);
    let (wt, ht) = (img.width_twips.to_number(), img.height_twips.to_number());
    let wd = to_js_string(math::round(wt / 2.0) / 10.0);
    let hd = to_js_string(math::round(ht / 2.0) / 10.0);
    let shape = format!("_x0000_i{}", to_js_string(1025.0 + nf));
    let obj_digits = to_js_string(1_800_000_000.0 + nf * 7919.0);
    let obj_id = format!("_{}", utf16_slice(&obj_digits, 0, 10));
    let (dxa, dya) = (to_js_string(math::round(wt)), to_js_string(math::round(ht)));
    Ok(format!(
        "<w:object xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" w:dxaOrig=\"{dxa}\" w:dyaOrig=\"{dya}\">\
<v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\">\
<v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/><v:f eqn=\"sum @0 1 0\"/><v:f eqn=\"sum 0 0 @1\"/>\
<v:f eqn=\"prod @2 1 2\"/><v:f eqn=\"prod @3 21600 pixelWidth\"/><v:f eqn=\"prod @3 21600 pixelHeight\"/><v:f eqn=\"sum @0 0 1\"/>\
<v:f eqn=\"prod @6 1 2\"/><v:f eqn=\"prod @7 21600 pixelWidth\"/><v:f eqn=\"sum @8 21600 0\"/><v:f eqn=\"prod @7 21600 pixelHeight\"/>\
<v:f eqn=\"sum @10 21600 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" o:connecttype=\"rect\"/><o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>\
<v:shape id=\"{shape}\" type=\"#_x0000_t75\" style=\"width:{wd}pt;height:{hd}pt\" o:ole=\"\"><v:imagedata r:id=\"{rid_img}\" o:title=\"\"/></v:shape>\
<o:OLEObject Type=\"Embed\" ProgID=\"{}\" ShapeID=\"{shape}\" DrawAspect=\"Content\" ObjectID=\"{obj_id}\" r:id=\"{rid_ole}\"/>\
</w:object>",
        esc_attr(&ole.prog_id)
    ))
}

/// `xml.replace(/r:embed="MARK"/g, () => `r:embed="${f()}"`)`: `f` runs once per occurrence, in order.
fn replace_embed_marks(
    w: &mut Writer<'_>,
    xml: &str,
    mark: &str,
    mut f: impl FnMut(&mut Writer<'_>) -> WResult<String>,
) -> WResult<String> {
    let needle = format!("r:embed=\"{mark}\"");
    if !xml.contains(&needle) {
        return Ok(xml.to_owned());
    }
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    let mut at = 0;
    while let Some(i) = find_from(xml, &needle, at) {
        out.push_str(xml.get(last..i).unwrap_or(""));
        out.push_str(&format!("r:embed=\"{}\"", f(w)?));
        last = i + needle.len();
        at = last;
    }
    out.push_str(tail(xml, last));
    Ok(out)
}

/// `pictureXml(img, source)`: an image with raw XML: the edited one (`img.edited`) or the source's,
/// with the current geometry and the new-media markers resolved (`r:embed="uw:media"` → the bytes
/// of `img.data`; `"uw:layer"` → the original of the `a14` layer, `img.layer`).
pub fn picture_xml(w: &mut Writer<'_>, img: &InlineImage, source: Option<&str>) -> WResult<String> {
    let src = if img.edited != Some(true) && truthy_str(source) {
        source.unwrap_or("")
    } else {
        img.raw_drawing_xml.as_deref().ok_or(DocxError::Write("pictureXml without rawDrawingXml (TS TypeError)"))?
    };
    let mut xml = patch_drawing_geometry(src, &Geometry::of_image(img))?;
    xml = replace_embed_marks(w, &xml, "uw:media", |w| {
        let data = media_bytes(w, img.data)?;
        w.media_rel_id(&data, &img.ext, None)
    })?;
    if let Some(layer) = img.layer.as_ref()
        && xml.contains("r:embed=\"uw:layer\"")
    {
        let rel = if &*layer.ext == "wdp" { Some(REL_HDPHOTO) } else { None };
        xml = replace_embed_marks(w, &xml, "uw:layer", |w| {
            let data = media_bytes(w, layer.data)?;
            w.media_rel_id(&data, &layer.ext, rel)
        })?;
    }
    Ok(xml)
}

// ───────────────────────────── rebuilt shapes (wps + VML fallback) ─────────────────────────────

/// `forEachTextbox(blocks, fn)` (fulldoc.ts:441): every textbox of the blocks in document order —
/// top-level paragraphs, cells, group items and boxes inside boxes. Iterative (an explicit stack of
/// pending work, children pushed in reverse), so the depth is not a stack depth.
pub fn for_each_textbox<'a>(blocks: &[Block<'a>], mut f: impl FnMut(&'a DrawingTextbox)) {
    enum Item<'a> {
        Block(Block<'a>),
        Run(&'a Run),
    }
    let mut stack: Vec<Item<'a>> = blocks.iter().rev().map(|b| Item::Block(*b)).collect();
    while let Some(item) = stack.pop() {
        match item {
            Item::Block(Block::Paragraph(p)) => stack.extend(p.runs.iter().rev().map(Item::Run)),
            Item::Block(Block::Table(t)) => {
                for row in t.rows.iter().rev() {
                    for cell in row.cells.iter().rev() {
                        let mut bs: Vec<Block<'a>> = cell.content.blocks().collect();
                        bs.reverse();
                        stack.extend(bs.into_iter().map(Item::Block));
                    }
                }
            }
            Item::Run(r) => {
                let Some(e) = r.ext() else { continue };
                if let Some(g) = e.group.as_deref() {
                    stack.extend(g.items.iter().rev().map(|it| Item::Run(&it.run)));
                }
                if let Some(tb) = e.textbox.as_deref() {
                    f(tb);
                    if tb.view != TextboxView::Phantom {
                        let mut bs: Vec<Block<'a>> = tb.content.blocks().collect();
                        bs.reverse();
                        stack.extend(bs.into_iter().map(Item::Block));
                    }
                }
            }
        }
    }
}

/// A `Map<number, …>` key by SameValueZero (`NaN` is one key, `-0` is `0`, `null` its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StoryKey {
    Null,
    Num(u64),
}

fn story_key(x: JsNumber) -> StoryKey {
    match x {
        JsNumber::Null => StoryKey::Null,
        JsNumber::Num(v) if v.is_nan() => StoryKey::Num(f64::NAN.to_bits()),
        JsNumber::Num(v) => StoryKey::Num(if v == 0.0 { 0.0f64.to_bits() } else { v.to_bits() }),
    }
}

/// `beginShapeContext(blocks)`: the textbox chains of the part about to be written. A chain is its
/// head (the first box with a story id) and the boxes linked to that id, in `seq` order; every box
/// but the last learns the `docPr` name of the next one (the VML `mso-next-textbox`).
pub fn begin_shape_context(w: &mut Writer<'_>, blocks: &[Block<'_>]) -> WResult<()> {
    w.cx.chain_next.clear();
    let mut heads: Vec<(StoryKey, &DrawingTextbox)> = Vec::new();
    let mut head_index: HashMap<StoryKey, usize> = HashMap::new();
    let mut links: HashMap<StoryKey, Vec<&DrawingTextbox>> = HashMap::new();
    for_each_textbox(blocks, |tb| {
        if let Some(l) = tb.link.as_ref() {
            if !matches!(l.id, JsNumber::Num(v) if v.is_nan()) {
                links.entry(story_key(l.id)).or_default().push(tb);
            }
        } else if let Some(s) = tb.story {
            let k = story_key(s);
            if let std::collections::hash_map::Entry::Vacant(v) = head_index.entry(k) {
                v.insert(heads.len());
                heads.push((k, tb));
            }
        }
    });
    for (key, head) in heads {
        let mut ms: Vec<&DrawingTextbox> = vec![head];
        if let Some(ls) = links.get_mut(&key) {
            ls.sort_by(|a, b| {
                let sa = a.link.as_ref().map_or(0.0, |l| l.seq.to_number());
                let sb = b.link.as_ref().map_or(0.0, |l| l.seq.to_number());
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            });
            ms.extend(ls.iter().copied());
        }
        for pair in ms.windows(2) {
            if let [cur, next] = pair
                && let Some(n) = doc_pr_name(next.raw_drawing_xml.as_deref()).filter(|n| !n.is_empty())
            {
                w.cx.chain_next.insert(cur.drawing, Str::from(n));
            }
        }
    }
    Ok(())
}

/// The own names of `Object.prototype`: a lookup of one in a plain object literal gives a function
/// (or the prototype), not `undefined`.
fn is_object_prototype_name(s: &str) -> bool {
    matches!(
        s,
        "constructor"
            | "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__proto__"
            | "toLocaleString"
    )
}

/// `shapeColor(c)`: a DrawingML colour as `RRGGBB` in the document's theme (`lumMod`/`lumOff` in HSL,
/// `shade`/`tint` linear). The base colour string is read by UTF-16 slices as the TS does (a value
/// that is not hex gives `NaN` channels, which print as `NAN`). A `prstClr` named like an
/// `Object.prototype` member is a TS `TypeError` (`base.slice` of a function): an error here.
pub fn shape_color(theme: &indexmap::IndexMap<Str, Str>, c: &PicColor) -> WResult<String> {
    let base: String = match &*c.kind {
        "srgbClr" => c.val.to_string(),
        "prstClr" => match &*c.val {
            "black" => "000000".to_owned(),
            "white" => "FFFFFF".to_owned(),
            "red" => "FF0000".to_owned(),
            v if is_object_prototype_name(v) => {
                return Err(DocxError::Write("shapeColor: prstClr named like an Object.prototype member (TS TypeError)"));
            }
            _ => "000000".to_owned(),
        },
        "sysClr" => match c.last_clr.as_deref() {
            Some(l) => l.to_owned(),
            None => (if &*c.val == "window" { "FFFFFF" } else { "000000" }).to_owned(),
        },
        _ => {
            let alias = match &*c.val {
                "tx1" => "dk1",
                "bg1" => "lt1",
                "tx2" => "dk2",
                "bg2" => "lt2",
                v => v,
            };
            theme.get(alias).filter(|_| !is_object_prototype_name(&c.val)).map_or_else(|| "000000".to_owned(), |v| v.to_string())
        }
    };
    let chan = |i: i64| parse_int(&utf16_slice(&base, i, i + 2), 16) / 255.0;
    let (mut r, mut g, mut b) = (chan(0), chan(2), chan(4));
    for (name, v0) in c.mods.as_deref().unwrap_or(&[]) {
        let v = v0.to_number() / 100_000.0;
        match &**name {
            "shade" => {
                r *= v;
                g *= v;
                b *= v;
            }
            "tint" => {
                r = 1.0 - (1.0 - r) * v;
                g = 1.0 - (1.0 - g) * v;
                b = 1.0 - (1.0 - b) * v;
            }
            n @ ("lumMod" | "lumOff") => {
                let mx = math::max_n(&[r, g, b]);
                let mn = math::min_n(&[r, g, b]);
                let mut l = (mx + mn) / 2.0;
                let d = mx - mn;
                let s = if d == 0.0 { 0.0 } else { d / (1.0 - math::abs(2.0 * l - 1.0)) };
                let mut hh = if d == 0.0 {
                    0.0
                } else if mx == r {
                    math::rem((g - b) / d, 6.0)
                } else if mx == g {
                    (b - r) / d + 2.0
                } else {
                    (r - g) / d + 4.0
                };
                hh = math::rem(hh * 60.0 + 360.0, 360.0);
                l = if n == "lumMod" { l * v } else { math::min(1.0, l + v) };
                let cc = (1.0 - math::abs(2.0 * l - 1.0)) * s;
                let x = cc * (1.0 - math::abs(math::rem(hh / 60.0, 2.0) - 1.0));
                let m = l - cc / 2.0;
                let (a1, b1, c1) = if hh < 60.0 {
                    (cc, x, 0.0)
                } else if hh < 120.0 {
                    (x, cc, 0.0)
                } else if hh < 180.0 {
                    (0.0, cc, x)
                } else if hh < 240.0 {
                    (0.0, x, cc)
                } else if hh < 300.0 {
                    (x, 0.0, cc)
                } else {
                    (cc, 0.0, x)
                };
                (r, g, b) = (a1 + m, b1 + m, c1 + m);
            }
            _ => {}
        }
    }
    let mut out = String::with_capacity(6);
    for x in [r, g, b] {
        let v = math::round(math::max(0.0, math::min(1.0, x)) * 255.0);
        // `NaN.toString(16)` is "NaN" (uo-jsnum's radix routine is integer only)
        let h = if v.is_nan() { "NaN".to_owned() } else { to_radix_string(v, 16).unwrap_or_default() };
        if h.len() < 2 {
            out.push('0');
        }
        out.push_str(&h);
    }
    Ok(out.to_ascii_uppercase())
}

/// The options of `vmlFallback` (`{ color, next?, spid, outline? }`).
pub struct VmlOptions<'f> {
    /// DmlColor → RRGGBB (the document's theme)
    pub color: &'f dyn Fn(&PicColor) -> WResult<String>,
    /// the `docPr` name of the next box of the chain (`mso-next-textbox`)
    pub next: Option<&'f str>,
    /// the number of the `o:spid`
    pub spid: u32,
    /// the path of a preset that is not a rectangle: `(prst, adj, w, h) → path` (none in the writer)
    #[allow(clippy::type_complexity)]
    pub outline: Option<&'f dyn Fn(&str, Option<&JsRecord<JsNumber>>, f64, f64) -> Option<String>>,
}

/// `shapeOut(drawing, inner, o, vml)`: a rebuilt shape as the wps plus the VML fallback
/// (`mc:AlternateContent`) or, in compatibility mode (or a box that came in VML), the VML only.
pub fn shape_out(w: &Writer<'_>, drawing: &str, inner: &str, o: &VmlOptions<'_>, vml: bool) -> WResult<String> {
    if !w.cx.vml_only && !vml {
        return textbox_alternate_content(drawing, inner, o);
    }
    Ok(strip_fallback(&vml_fallback(drawing, inner, o)?))
}

/// `v.replace(/^<mc:Fallback>/, '').replace(/<\/mc:Fallback>$/, '')`.
#[must_use]
pub fn strip_fallback(v: &str) -> String {
    let v = v.strip_prefix("<mc:Fallback>").unwrap_or(v);
    v.strip_suffix("</mc:Fallback>").unwrap_or(v).to_owned()
}

/// `fillMedia(raw, st)`: a new picture fill (Format Shape › Picture/Texture): the media enters the
/// part being written and the provisional `r:embed="uw:fill"` becomes its relationship.
fn fill_media(w: &mut Writer<'_>, raw: &str, st: Option<&ShapeStyle>) -> WResult<String> {
    let Some(img) = st.and_then(|s| s.blip.as_deref()).map(|b| &b.image) else { return Ok(raw.to_owned()) };
    if !raw.contains("r:embed=\"uw:fill\"") {
        return Ok(raw.to_owned());
    }
    let data = media_bytes(w, img.data)?;
    let rid = w.media_rel_id(&data, &img.ext, None)?;
    Ok(raw.replace("r:embed=\"uw:fill\"", &format!("r:embed=\"{rid}\"")))
}

fn next_spid(w: &mut Writer<'_>) -> u32 {
    w.cx.vml_spid = w.cx.vml_spid.saturating_add(1);
    w.cx.vml_spid
}

/// The most rebuilt text boxes written one inside another: `editedShapeXml` writes a box's content
/// twice (for the VML fallback and for the wps), so n levels are 2^n work in the TS; beyond this depth
/// the save is refused (W-D15; the reader allows 64 levels).
pub const MAX_REBUILT_BOX_DEPTH: u32 = 16;

/// `editedShapeXml(run)`: a text box or shape edited (or new): `mc:AlternateContent` with the wps and
/// the VML fallback rebuilt from it, as Word does when saving.
pub fn edited_shape_xml<'a>(w: &mut Writer<'a>, run: &'a Run) -> WResult<String> {
    let ext = run.ext();
    if let Some(tb) = ext.and_then(|e| e.textbox.as_deref())
        && let Some(raw0) = tb.raw_drawing_xml.as_deref()
    {
        if w.cx.shape_depth >= MAX_REBUILT_BOX_DEPTH {
            return Err(DocxError::Write("rebuilt text boxes nested deeper than 16 (the TS writes each level twice: 2^depth work)"));
        }
        w.cx.shape_depth += 1;
        let r = edited_textbox_xml(w, tb, raw0);
        w.cx.shape_depth -= 1;
        return r;
    }
    if let Some(sh) = ext.and_then(|e| e.shape.as_deref())
        && let Some(raw0) = sh.raw_drawing_xml.as_deref()
    {
        let raw = fill_media(w, raw0, Some(&sh.style))?;
        let spid = next_spid(w);
        let theme = &w.cx.shape_theme;
        let color = |c: &PicColor| shape_color(theme, c);
        let o = VmlOptions { color: &color, next: None, spid, outline: None };
        return shape_out(w, &raw, "", &o, false);
    }
    Ok(String::new())
}

/// The text box branch of `editedShapeXml`: the content is written first for the VML fallback
/// (`inner`), then again inside the wps (`drawingWithParas`), in this order (revision ids, claimed
/// sources and `o:spid` numbers advance between the two).
fn edited_textbox_xml<'a>(w: &mut Writer<'a>, tb: &'a DrawingTextbox, raw0: &str) -> WResult<String> {
    let inner = if tb.link.is_some() { String::new() } else { w.txbx_inner(tb)? };
    let raw = fill_media(w, raw0, tb.shape.as_deref())?;
    let geo = patch_drawing_geometry(&raw, &Geometry::of_textbox(tb))?;
    let drawing = if tb.link.is_some() { geo } else { w.drawing_with_paras(&geo, tb)? };
    let next = w.cx.chain_next.get(&tb.drawing).cloned();
    let spid = next_spid(w);
    let content = if inner.is_empty() && tb.link.is_none() { "<w:p/>" } else { &inner };
    let theme = &w.cx.shape_theme;
    let color = |c: &PicColor| shape_color(theme, c);
    let o = VmlOptions { color: &color, next: next.as_deref().filter(|n| !n.is_empty()), spid, outline: None };
    shape_out(w, &drawing, content, &o, tb.vml == Some(true))
}

/// `editedGroupXml(srcObj, raw)`: a group edited in the Selection Pane (name, hidden, order): the new
/// wps in the `mc:Choice` and the `v:group` fallback with the same name, visibility and order.
pub fn edited_group_xml(w: &mut Writer<'_>, src_obj: &str, raw: &str) -> WResult<String> {
    let _ = w;
    Ok(edited_group_text(src_obj, raw))
}

/// The text work of `editedGroupXml(srcObj, raw)`.
#[must_use]
pub fn edited_group_text(src_obj: &str, raw: &str) -> String {
    let xml = replace_first_drawing(src_obj, raw);
    if !has_alternate_content(&xml) {
        return raw.to_owned();
    }
    let name = doc_pr_name(Some(raw)).filter(|n| !n.is_empty());
    let hidden = has_doc_pr_hidden(raw);
    let rh = relative_height_of(raw);
    let behind = has_behind_doc(raw);
    retag_v_group(&xml, name.as_deref(), hidden, rh, behind)
}

/// `/<wp:docPr\b[^>]*\bhidden="1"/.test(raw)`.
#[must_use]
pub fn has_doc_pr_hidden(raw: &str) -> bool {
    doc_pr_greedy(raw, "hidden=\"1\"", |_| Some(())).is_some()
}

/// `/\brelativeHeight="(\d+)"/.exec(raw)?.[1]`.
#[must_use]
pub fn relative_height_of(raw: &str) -> Option<&str> {
    first_digit_attr(raw, "relativeHeight")
}

/// `/\bbehindDoc="1"/.test(raw)`.
#[must_use]
pub fn has_behind_doc(raw: &str) -> bool {
    has_attr_one(raw, "behindDoc")
}

/// `srcObj.replace(/<w:drawing\b[\s\S]*?<\/w:drawing>/, () => raw)`.
#[must_use]
pub fn replace_first_drawing(src: &str, raw: &str) -> String {
    const OPEN: &str = "<w:drawing";
    const CLOSE: &str = "</w:drawing>";
    let mut at = 0;
    while let Some(i) = find_from(src, OPEN, at) {
        at = i + 1;
        let after = i + OPEN.len();
        if !boundary_at(src, after) {
            continue;
        }
        let Some(c) = find_from(src, CLOSE, after) else { break };
        return format!("{}{raw}{}", src.get(..i).unwrap_or(""), tail(src, c + CLOSE.len()));
    }
    src.to_owned()
}

/// `/<mc:AlternateContent\b/.test(xml)`.
#[must_use]
pub fn has_alternate_content(xml: &str) -> bool {
    const OPEN: &str = "<mc:AlternateContent";
    let mut at = 0;
    while let Some(i) = find_from(xml, OPEN, at) {
        if boundary_at(xml, i + OPEN.len()) {
            return true;
        }
        at = i + 1;
    }
    false
}

/// `xml.replace(/<v:group\b[^>]*>/, (tag) => …)`: the first `v:group` start tag takes the new name,
/// visibility and z-order.
fn retag_v_group(xml: &str, name: Option<&str>, hidden: bool, rh: Option<&str>, behind: bool) -> String {
    const OPEN: &str = "<v:group";
    let mut at = 0;
    while let Some(i) = find_from(xml, OPEN, at) {
        at = i + 1;
        let after = i + OPEN.len();
        if !boundary_at(xml, after) {
            continue;
        }
        let Some(gt) = find_from(xml, ">", after) else { break };
        let tag = xml.get(i..=gt).unwrap_or("");
        let t = retag(tag, name, hidden, rh, behind);
        return format!("{}{t}{}", xml.get(..i).unwrap_or(""), tail(xml, gt + 1));
    }
    xml.to_owned()
}

/// `/\sid="[^"]*"/`: the first match (whitespace included): `(start, end)`.
fn find_id_attr(tag: &str) -> Option<(usize, usize)> {
    for (i, c) in tag.char_indices() {
        if !is_js_space(c) {
            continue;
        }
        let start = i + c.len_utf8();
        let Some(r) = tail(tag, start).strip_prefix("id=\"") else { continue };
        let q = r.find('"')?;
        return Some((i, start + 4 + q + 1));
    }
    None
}

fn retag(tag: &str, name: Option<&str>, hidden: bool, rh: Option<&str>, behind: bool) -> String {
    let mut t = tag.to_owned();
    if let Some(n) = name {
        let id = format!(" id=\"{}\"", esc_attr(n));
        if let Some((s, e)) = find_id_attr(&t) {
            t = format!("{}{id}{}", t.get(..s).unwrap_or(""), tail(&t, e));
        } else if let Some(rest) = t.strip_prefix("<v:group") {
            t = format!("<v:group{id}{rest}");
        }
    }
    // `t.replace(/style="([^"]*)"/, (_m, st) => …)`: the first `style="…"` (an unclosed one is the last)
    if let Some(i) = find_from(&t, "style=\"", 0)
        && let Some(q) = find_from(&t, "\"", i + 7)
    {
        let st = t.get(i + 7..q).unwrap_or("");
        let mut s = remove_visibility(st);
        s.push_str(if hidden { ";visibility:hidden" } else { ";visibility:visible" });
        if let Some(rh) = rh {
            let z = if behind { to_js_string(-(uo_jsnum::parse::to_number(rh) - 2048.0)) } else { rh.to_owned() };
            s = replace_z_index(&s, &z);
        }
        let s = s.strip_prefix(';').unwrap_or(&s);
        t = format!("{}style=\"{s}\"{}", t.get(..i).unwrap_or(""), tail(&t, q + 1));
    }
    t
}

/// `st.replace(/;?visibility:[^;]*/g, '')`.
fn remove_visibility(st: &str) -> String {
    let mut out = String::with_capacity(st.len());
    let mut i = 0;
    while i < st.len() {
        let rest = tail(st, i);
        let skip = if rest.starts_with(";visibility:") {
            Some(";visibility:".len())
        } else if rest.starts_with("visibility:") {
            Some("visibility:".len())
        } else {
            None
        };
        match skip {
            Some(n) => {
                let body = tail(st, i + n);
                i += n + body.find(';').unwrap_or(body.len());
            }
            None => {
                let c = rest.chars().next().unwrap_or(' ');
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// `s.replace(/z-index:-?\d+/, `z-index:${z}`)` (the first match; `z` has no `$`).
fn replace_z_index(s: &str, z: &str) -> String {
    const NEEDLE: &str = "z-index:";
    let mut at = 0;
    while let Some(i) = find_from(s, NEEDLE, at) {
        at = i + 1;
        let mut j = i + NEEDLE.len();
        if s.as_bytes().get(j) == Some(&b'-') {
            j += 1;
        }
        let digits = s.as_bytes().get(j..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits > 0 {
            return format!("{}z-index:{z}{}", s.get(..i).unwrap_or(""), tail(s, j + digits));
        }
    }
    s.to_owned()
}

// ───────────────────────────── shape-xml.ts: types ─────────────────────────────

/// A gradient stop (`{ pos, color }`).
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub pos: f64,
    pub color: PicColor,
}

/// `{ l, t, r, b }` of numbers (`fillToRect`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillRect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// A gradient fill (`ShapeFill` `kind: 'gradient'`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GradientFill {
    pub stops: Vec<GradientStop>,
    /// `lin: { ang, scaled }`
    pub lin: Option<(f64, bool)>,
    /// `path: { path, fillToRect? }`
    pub path: Option<(String, Option<FillRect>)>,
    pub rot_with_shape: Option<bool>,
    pub flip: Option<String>,
    pub tile_rect: bool,
}

/// A picture fill's tile (`{ tx, ty, sx, sy, flip, algn }`).
#[derive(Debug, Clone, PartialEq)]
pub struct FillTile {
    pub tx: f64,
    pub ty: f64,
    pub sx: f64,
    pub sy: f64,
    pub flip: String,
    pub algn: String,
}

/// The fill of the `wps:spPr` (`ShapeFill`). `Style` = none declared (the `fillRef` of the
/// `wps:style` rules).
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeFill {
    None,
    Style,
    Solid(PicColor),
    Gradient(Box<GradientFill>),
    Pattern {
        prst: String,
        fg: PicColor,
        bg: PicColor,
    },
    Picture {
        embed: String,
        tile: Option<FillTile>,
        alpha: Option<f64>,
    },
    /// another fill (`a:grpFill`, an exotic `a:blipFill`): raw
    Raw(String),
}

/// One reference of `wps:style` (`{ idx, color }`).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleRef {
    pub idx: f64,
    pub color: Option<PicColor>,
}

/// `wps:fontRef` (`idx` is `minor | major | none` as read; any attribute text).
#[derive(Debug, Clone, PartialEq)]
pub struct FontRef {
    pub idx: String,
    pub color: Option<PicColor>,
}

/// `wps:style`: theme references (Shape Styles).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeStyleRefs {
    pub ln: StyleRef,
    pub fill: StyleRef,
    pub effect: StyleRef,
    pub font: FontRef,
}

/// `TextboxBody.anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAnchor {
    T,
    Ctr,
    B,
}

impl BodyAnchor {
    fn as_str(self) -> &'static str {
        match self {
            BodyAnchor::T => "t",
            BodyAnchor::Ctr => "ctr",
            BodyAnchor::B => "b",
        }
    }
}

/// `TextboxBody.wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyWrap {
    Square,
    None,
}

impl BodyWrap {
    fn as_str(self) -> &'static str {
        match self {
            BodyWrap::Square => "square",
            BodyWrap::None => "none",
        }
    }
}

/// `TextboxBody.autofit`: `a:noAutofit | a:spAutoFit | a:normAutofit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAutofit {
    None,
    Shape,
    Norm,
}

impl BodyAutofit {
    fn xml(self) -> &'static str {
        match self {
            BodyAutofit::Shape => "<a:spAutoFit/>",
            BodyAutofit::Norm => "<a:normAutofit/>",
            BodyAutofit::None => "<a:noAutofit/>",
        }
    }
}

/// `wps:bodyPr` of a text box (insets in EMU).
#[derive(Debug, Clone, PartialEq)]
pub struct TextboxBody {
    pub l_ins: f64,
    pub t_ins: f64,
    pub r_ins: f64,
    pub b_ins: f64,
    pub anchor: BodyAnchor,
    pub anchor_ctr: bool,
    /// `horz | vert | vert270 | eaVert | mongolianVert | wordArtVert | wordArtVertRtl` as read
    pub vert: String,
    pub wrap: BodyWrap,
    pub autofit: BodyAutofit,
    /// do not rotate the text (with the shape)
    pub upright: bool,
    pub rot: f64,
}

/// `Partial<TextboxBody>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextboxBodyPatch {
    pub l_ins: Option<f64>,
    pub t_ins: Option<f64>,
    pub r_ins: Option<f64>,
    pub b_ins: Option<f64>,
    pub anchor: Option<BodyAnchor>,
    pub anchor_ctr: Option<bool>,
    pub vert: Option<String>,
    pub wrap: Option<BodyWrap>,
    pub autofit: Option<BodyAutofit>,
    pub upright: Option<bool>,
    pub rot: Option<f64>,
}

/// `DEFAULT_TEXTBOX_BODY`: the `bodyPr` defaults of a new box (measured).
#[must_use]
pub fn default_textbox_body() -> TextboxBody {
    TextboxBody {
        l_ins: 91440.0,
        t_ins: 45720.0,
        r_ins: 91440.0,
        b_ins: 45720.0,
        anchor: BodyAnchor::T,
        anchor_ctr: false,
        vert: "horz".to_owned(),
        wrap: BodyWrap::Square,
        autofit: BodyAutofit::None,
        upright: false,
        rot: 0.0,
    }
}

impl TextboxBody {
    /// `{ ...this, ...b }`.
    fn merged(&self, b: &TextboxBodyPatch) -> TextboxBody {
        TextboxBody {
            l_ins: b.l_ins.unwrap_or(self.l_ins),
            t_ins: b.t_ins.unwrap_or(self.t_ins),
            r_ins: b.r_ins.unwrap_or(self.r_ins),
            b_ins: b.b_ins.unwrap_or(self.b_ins),
            anchor: b.anchor.unwrap_or(self.anchor),
            anchor_ctr: b.anchor_ctr.unwrap_or(self.anchor_ctr),
            vert: b.vert.clone().unwrap_or_else(|| self.vert.clone()),
            wrap: b.wrap.unwrap_or(self.wrap),
            autofit: b.autofit.unwrap_or(self.autofit),
            upright: b.upright.unwrap_or(self.upright),
            rot: b.rot.unwrap_or(self.rot),
        }
    }
}

/// `ShapeFormat.story`: the story of the box — `wps:txbx` (the chain id, if linked) or
/// `wps:linkedTxbx`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeStory {
    Txbx { id: Option<f64> },
    Linked { id: f64, seq: f64 },
}

/// The formatting of a shape or text box (`ShapeFormat`: the picture format without `blip`,
/// `srcRect` and `embed` — they are in `base` and not meaningful here — plus the shape parts).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeFormat {
    pub base: PictureFormat,
    pub fill: ShapeFill,
    pub style: Option<ShapeStyleRefs>,
    /// `wps:cNvSpPr txBox="1"`
    pub textbox: bool,
    pub body: Option<TextboxBody>,
    pub story: Option<ShapeStory>,
    /// `wps:wsp normalEastAsianFlow`
    pub normal_east_asian_flow: bool,
}

// ───────────────────────────── shape-xml.ts: reading ─────────────────────────────

fn black() -> PicColor {
    PicColor { kind: "srgbClr".into(), val: "000000".into(), ..PicColor::default() }
}

fn white() -> PicColor {
    PicColor { kind: "srgbClr".into(), val: "FFFFFF".into(), ..PicColor::default() }
}

fn int_attr(el: Option<Element<'_>>, name: &str) -> f64 {
    pic_int(el.and_then(|e| e.attr(name)), 0.0)
}

fn read_fill(sp_pr: Option<Element<'_>>) -> ShapeFill {
    let Some(sp_pr) = sp_pr else { return ShapeFill::Style };
    for k in sp_pr.child_elements() {
        match k.name() {
            "a:noFill" => return ShapeFill::None,
            "a:solidFill" => return ShapeFill::Solid(read_dml_color(Some(k)).unwrap_or_else(black)),
            "a:gradFill" => {
                let gs = k.first_child("a:gsLst");
                let lin = k.first_child("a:lin");
                let path = k.first_child("a:path");
                let ftr = path.and_then(|p| p.first_child("a:fillToRect"));
                let stops = gs
                    .map(|g| {
                        g.children_named("a:gs")
                            .map(|s| GradientStop { pos: int_attr(Some(s), "pos"), color: read_dml_color(Some(s)).unwrap_or_else(black) })
                            .collect()
                    })
                    .unwrap_or_default();
                return ShapeFill::Gradient(Box::new(GradientFill {
                    stops,
                    lin: lin.map(|l| (int_attr(Some(l), "ang"), pic_bool(l.attr("scaled"), false))),
                    path: path.map(|p| {
                        (
                            p.attr("path").unwrap_or("circle").to_owned(),
                            ftr.map(|f| FillRect {
                                l: int_attr(Some(f), "l"),
                                t: int_attr(Some(f), "t"),
                                r: int_attr(Some(f), "r"),
                                b: int_attr(Some(f), "b"),
                            }),
                        )
                    }),
                    rot_with_shape: k.attr("rotWithShape").map(|v| pic_bool(Some(v), false)),
                    flip: k.attr("flip").filter(|s| !s.is_empty()).map(str::to_owned),
                    tile_rect: k.first_child("a:tileRect").is_some(),
                }));
            }
            "a:pattFill" => {
                return ShapeFill::Pattern {
                    prst: k.attr("prst").unwrap_or("pct5").to_owned(),
                    fg: read_dml_color(k.first_child("a:fgClr")).unwrap_or_else(black),
                    bg: read_dml_color(k.first_child("a:bgClr")).unwrap_or_else(white),
                };
            }
            "a:blipFill" => {
                let blip = k.first_child("a:blip");
                let tile = k.first_child("a:tile");
                let amf = blip.and_then(|b| b.first_child("a:alphaModFix"));
                let Some(embed) = blip.and_then(|b| b.attr("r:embed")).filter(|s| !s.is_empty()) else {
                    return ShapeFill::Raw(k.source_slice().unwrap_or("").to_owned());
                };
                return ShapeFill::Picture {
                    embed: embed.to_owned(),
                    tile: tile.map(|t| FillTile {
                        tx: int_attr(Some(t), "tx"),
                        ty: int_attr(Some(t), "ty"),
                        sx: pic_int(t.attr("sx"), 100_000.0),
                        sy: pic_int(t.attr("sy"), 100_000.0),
                        flip: t.attr("flip").unwrap_or("none").to_owned(),
                        algn: t.attr("algn").unwrap_or("tl").to_owned(),
                    }),
                    alpha: amf.map(|a| pic_int(a.attr("amt"), 100_000.0)),
                };
            }
            "a:grpFill" => return ShapeFill::Raw(k.source_slice().unwrap_or("").to_owned()),
            _ => {}
        }
    }
    ShapeFill::Style
}

fn read_style_refs(st: Option<Element<'_>>) -> Option<ShapeStyleRefs> {
    let st = st?;
    let r = |n: &str| {
        let e = st.first_child(n);
        StyleRef { idx: int_attr(e, "idx"), color: read_dml_color(e) }
    };
    let fr = st.first_child("a:fontRef");
    Some(ShapeStyleRefs {
        ln: r("a:lnRef"),
        fill: r("a:fillRef"),
        effect: r("a:effectRef"),
        font: FontRef { idx: fr.and_then(|f| f.attr("idx")).unwrap_or("minor").to_owned(), color: read_dml_color(fr) },
    })
}

fn read_body(b: Option<Element<'_>>) -> Option<TextboxBody> {
    let b = b?;
    let anchor = match b.attr("anchor") {
        Some("ctr") => BodyAnchor::Ctr,
        Some("b") => BodyAnchor::B,
        _ => BodyAnchor::T,
    };
    Some(TextboxBody {
        l_ins: pic_int(b.attr("lIns"), 91440.0),
        t_ins: pic_int(b.attr("tIns"), 45720.0),
        r_ins: pic_int(b.attr("rIns"), 91440.0),
        b_ins: pic_int(b.attr("bIns"), 45720.0),
        anchor,
        anchor_ctr: pic_bool(b.attr("anchorCtr"), false),
        vert: b.attr("vert").unwrap_or("horz").to_owned(),
        wrap: if b.attr("wrap") == Some("none") { BodyWrap::None } else { BodyWrap::Square },
        autofit: if b.first_child("a:spAutoFit").is_some() {
            BodyAutofit::Shape
        } else if b.first_child("a:normAutofit").is_some() {
            BodyAutofit::Norm
        } else {
            BodyAutofit::None
        },
        upright: pic_bool(b.attr("upright"), false),
        rot: int_attr(Some(b), "rot"),
    })
}

/// `readShapeFormat(raw)`: the formatting of a shape or text box (`wps:wsp`) in a raw `w:drawing`;
/// `None` when it is not one (or does not parse).
#[must_use]
pub fn read_shape_format(raw: &str) -> Option<ShapeFormat> {
    let base = read_picture_format(raw, true)?;
    let doc = parse_drawing(raw)?;
    let p = parts_of(&doc)?;
    let wsp = p.wsp?;
    let nv = wsp.first_child("wps:cNvSpPr");
    let txbx = wsp.first_child("wps:txbx");
    let linked = wsp.first_child("wps:linkedTxbx");
    let mut out = ShapeFormat {
        base,
        fill: read_fill(p.sp_pr),
        style: read_style_refs(wsp.first_child("wps:style")),
        textbox: pic_bool(nv.and_then(|n| n.attr("txBox")), false),
        body: None,
        story: None,
        normal_east_asian_flow: pic_bool(wsp.attr("normalEastAsianFlow"), false),
    };
    if txbx.is_some() || linked.is_some() {
        out.body = Some(read_body(wsp.first_child("wps:bodyPr")).unwrap_or_else(default_textbox_body));
        out.story = Some(match linked {
            Some(l) => ShapeStory::Linked { id: int_attr(Some(l), "id"), seq: pic_int(l.attr("seq"), 1.0) },
            None => ShapeStory::Txbx { id: txbx.and_then(|t| t.attr("id")).map(|v| pic_int(Some(v), 0.0)) },
        });
    }
    Some(out)
}

// ───────────────────────────── shape-xml.ts: writing ─────────────────────────────

fn num(x: f64) -> String {
    to_js_string(x)
}

fn rnd(x: f64) -> String {
    to_js_string(math::round(x))
}

/// `shapeFillXml(f)`.
#[must_use]
pub fn shape_fill_xml(f: &ShapeFill) -> String {
    match f {
        ShapeFill::None => "<a:noFill/>".to_owned(),
        ShapeFill::Style => String::new(),
        ShapeFill::Solid(c) => format!("<a:solidFill>{}</a:solidFill>", dml_color_xml(c)),
        ShapeFill::Gradient(g) => {
            let gs: String = g.stops.iter().map(|s| format!("<a:gs pos=\"{}\">{}</a:gs>", rnd(s.pos), dml_color_xml(&s.color))).collect();
            let shade = if let Some((path, ftr)) = &g.path {
                let rect = ftr.map_or_else(String::new, |r| {
                    format!("<a:fillToRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>", num(r.l), num(r.t), num(r.r), num(r.b))
                });
                format!("<a:path path=\"{path}\">{rect}</a:path>")
            } else if let Some((ang, scaled)) = g.lin {
                format!("<a:lin ang=\"{}\" scaled=\"{}\"/>", rnd(ang), u8::from(scaled))
            } else {
                String::new()
            };
            let flip = g.flip.as_deref().filter(|s| !s.is_empty()).map_or_else(String::new, |f| format!(" flip=\"{f}\""));
            let rot = g.rot_with_shape.map_or_else(String::new, |r| format!(" rotWithShape=\"{}\"", u8::from(r)));
            let tile = if g.tile_rect { "<a:tileRect/>" } else { "" };
            format!("<a:gradFill{flip}{rot}><a:gsLst>{gs}</a:gsLst>{shade}{tile}</a:gradFill>")
        }
        ShapeFill::Pattern { prst, fg, bg } => format!(
            "<a:pattFill prst=\"{prst}\"><a:fgClr>{}</a:fgClr><a:bgClr>{}</a:bgClr></a:pattFill>",
            dml_color_xml(fg),
            dml_color_xml(bg)
        ),
        ShapeFill::Picture { embed, tile, alpha } => {
            let blip = match alpha {
                Some(a) if *a < 100_000.0 => format!("<a:blip r:embed=\"{embed}\"><a:alphaModFix amt=\"{}\"/></a:blip>", rnd(*a)),
                _ => format!("<a:blip r:embed=\"{embed}\"/>"),
            };
            let t = tile.as_ref().map_or_else(
                || "<a:stretch><a:fillRect/></a:stretch>".to_owned(),
                |t| {
                    format!(
                        "<a:tile tx=\"{}\" ty=\"{}\" sx=\"{}\" sy=\"{}\" flip=\"{}\" algn=\"{}\"/>",
                        num(t.tx),
                        num(t.ty),
                        num(t.sx),
                        num(t.sy),
                        t.flip,
                        t.algn
                    )
                },
            );
            format!("<a:blipFill>{blip}{t}</a:blipFill>")
        }
        ShapeFill::Raw(x) => x.clone(),
    }
}

/// `styleRefsXml(s)`.
#[must_use]
pub fn style_refs_xml(s: &ShapeStyleRefs) -> String {
    let c = |x: &Option<PicColor>| x.as_ref().map_or_else(|| "<a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/>".to_owned(), dml_color_xml);
    format!(
        "<wps:style><a:lnRef idx=\"{}\">{}</a:lnRef><a:fillRef idx=\"{}\">{}</a:fillRef><a:effectRef idx=\"{}\">{}</a:effectRef><a:fontRef idx=\"{}\">{}</a:fontRef></wps:style>",
        num(s.ln.idx),
        c(&s.ln.color),
        num(s.fill.idx),
        c(&s.fill.color),
        num(s.effect.idx),
        c(&s.effect.color),
        s.font.idx,
        s.font.color.as_ref().map_or_else(String::new, dml_color_xml)
    )
}

/// `bodyPrXml(b)`: the whole `wps:bodyPr` as Word writes it (attributes in the measured order).
#[must_use]
pub fn body_pr_xml(b: &TextboxBody) -> String {
    format!(
        "<wps:bodyPr rot=\"{}\" spcFirstLastPara=\"0\" vertOverflow=\"overflow\" horzOverflow=\"overflow\" vert=\"{}\" wrap=\"{}\" lIns=\"{}\" tIns=\"{}\" rIns=\"{}\" bIns=\"{}\" numCol=\"1\" spcCol=\"0\" rtlCol=\"0\" fromWordArt=\"0\" anchor=\"{}\" anchorCtr=\"{}\" forceAA=\"0\"{} compatLnSpc=\"1\"><a:prstTxWarp prst=\"textNoShape\"><a:avLst/></a:prstTxWarp>{}</wps:bodyPr>",
        num(b.rot),
        b.vert,
        b.wrap.as_str(),
        rnd(b.l_ins),
        rnd(b.t_ins),
        rnd(b.r_ins),
        rnd(b.b_ins),
        b.anchor.as_str(),
        u8::from(b.anchor_ctr),
        if b.upright { " upright=\"1\"" } else { "" },
        b.autofit.xml()
    )
}

const HIDDEN_LINE_URI: &str = "{91240B29-F687-4F45-9708-019B960494DF}";

/// `hiddenLineXml(prev)`: no outline as Word writes it (oracle `line-none`): the `a:ln` with `a:noFill`
/// (and the previous width) and, for the `a:extLst` of the `spPr`, the `a14:hiddenLine` with the
/// previous line. Returns `(ln, ext)`.
#[must_use]
pub fn hidden_line_xml(prev: Option<&PicLine>) -> (String, String) {
    let w = prev.and_then(|p| p.w).map_or_else(String::new, |w| format!(" w=\"{}\"", rnd(w.to_number())));
    let fill = match prev.and_then(|p| match &p.fill {
        Some(Nullable::Val(c)) => Some(c),
        _ => None,
    }) {
        Some(c) => format!("<a:solidFill>{}</a:solidFill>", dml_color_xml(c)),
        None => "<a:solidFill><a:prstClr val=\"black\"/></a:solidFill>".to_owned(),
    };
    (
        format!("<a:ln{w}><a:noFill/></a:ln>"),
        format!(
            "<a:ext uri=\"{HIDDEN_LINE_URI}\"><a14:hiddenLine xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\"{w}>{fill}</a14:hiddenLine></a:ext>"
        ),
    )
}

/// The position of a new box (`posH`/`posV` of `NewTextboxOptions`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewPos {
    pub relative_from: String,
    pub offset: Option<f64>,
    pub align: Option<String>,
    pub pct: Option<f64>,
    pub fallback: Option<f64>,
}

/// `NewTextboxOptions.sizeRelH/V`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSizeRel {
    pub relative_from: String,
    pub pct: f64,
}

/// `NewTextboxOptions.wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWrap {
    None,
    Square,
    TopAndBottom,
}

/// `NewTextboxOptions.dist` (`{ t, b, l, r }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewDist {
    pub t: f64,
    pub b: f64,
    pub l: f64,
    pub r: f64,
}

/// `NewTextboxOptions.geom`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewGeom {
    pub prst: String,
    /// `adj`, in the order of `Object.entries`
    pub adj: JsRecord<f64>,
}

/// The options of a new text box (EMU), `NewTextboxOptions`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewTextboxOptions {
    pub id: f64,
    pub name: String,
    pub cx: f64,
    pub cy: f64,
    pub pos_h: Option<NewPos>,
    pub pos_v: Option<NewPos>,
    pub relative_height: f64,
    pub wrap: Option<NewWrap>,
    pub behind_doc: bool,
    pub dist: Option<NewDist>,
    pub size_rel_h: Option<NewSizeRel>,
    pub size_rel_v: Option<NewSizeRel>,
    /// fill/outline/effects of the `spPr` (default: `lt1` + 6350 `prstClr black`)
    pub fill: Option<ShapeFill>,
    /// `line?: PicLine | null`: `None` = default, `Some(None)` = no outline
    pub line: Option<Option<PicLine>>,
    /// `spPrExtra`: more `spPr` (`a:effectLst`…) after the outline
    pub sp_pr_extra: Option<String>,
    /// `spPrRaw`: the children of the `spPr` after the shape, ready (the theme's default box, `a:txDef`)
    pub sp_pr_raw: Option<String>,
    pub geom: Option<NewGeom>,
    pub style: Option<ShapeStyleRefs>,
    pub body: Option<TextboxBodyPatch>,
    /// the blocks of the `w:txbxContent`
    pub content: String,
    /// `wps:linkedTxbx` instead of the content
    pub linked: Option<(f64, f64)>,
}

/// `NEW_TEXTBOX_LINE`: the outline of a new box drawn in the UI (measured on the corpus).
#[must_use]
pub fn new_textbox_line() -> PicLine {
    PicLine {
        w: Some(JsNumber::Num(6350.0)),
        fill: Some(Nullable::Val(PicColor { kind: "prstClr".into(), val: "black".into(), ..PicColor::default() })),
        ..PicLine::default()
    }
}

/// `NEW_TEXTBOX_FILL`.
#[must_use]
pub fn new_textbox_fill() -> ShapeFill {
    ShapeFill::Solid(PicColor { kind: "schemeClr".into(), val: "lt1".into(), ..PicColor::default() })
}

fn new_pos_xml(tag: &str, p: &NewPos) -> String {
    if p.align.is_none()
        && let Some(pct) = p.pct
    {
        let pct_tag = if tag == "wp:positionH" { "wp14:pctPosHOffset" } else { "wp14:pctPosVOffset" };
        return format!(
            "<mc:AlternateContent><mc:Choice Requires=\"wp14\"><{tag} relativeFrom=\"{}\"><{pct_tag}>{}</{pct_tag}></{tag}></mc:Choice><mc:Fallback><{tag} relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></{tag}></mc:Fallback></mc:AlternateContent>",
            p.relative_from,
            rnd(pct),
            rnd(p.fallback.unwrap_or(0.0))
        );
    }
    let inner = match &p.align {
        Some(a) => format!("<wp:align>{a}</wp:align>"),
        None => format!("<wp:posOffset>{}</wp:posOffset>", rnd(p.offset.unwrap_or(0.0))),
    };
    format!("<{tag} relativeFrom=\"{}\">{inner}</{tag}>", p.relative_from)
}

/// `newTextboxDrawingXml(o)`: the `w:drawing` (`mc:Choice`) of a new text box, as Word writes it.
#[must_use]
pub fn new_textbox_drawing_xml(o: &NewTextboxOptions) -> String {
    let (cx, cy) = (rnd(o.cx), rnd(o.cy));
    let d = o.dist.unwrap_or(NewDist { t: 0.0, b: 0.0, l: 114_300.0, r: 114_300.0 });
    let wrap = match o.wrap {
        Some(NewWrap::Square) => "<wp:wrapSquare wrapText=\"bothSides\"/>",
        Some(NewWrap::TopAndBottom) => "<wp:wrapTopAndBottom/>",
        _ => "<wp:wrapNone/>",
    };
    let ln = match &o.line {
        Some(None) => "<a:ln><a:noFill/></a:ln>".to_owned(),
        Some(Some(l)) => pic_line_xml(l),
        None => pic_line_xml(&new_textbox_line()),
    };
    let fill = shape_fill_xml(o.fill.as_ref().unwrap_or(&new_textbox_fill()));
    let (prst, adj_src) = match &o.geom {
        Some(g) => (g.prst.as_str(), Some(&g.adj)),
        None => ("rect", None),
    };
    let adj: String =
        adj_src.map(|a| a.iter().map(|(n, v)| format!("<a:gd name=\"{n}\" fmla=\"val {}\"/>", rnd(*v))).collect()).unwrap_or_default();
    let story = match o.linked {
        Some((id, seq)) => format!("<wps:linkedTxbx id=\"{}\" seq=\"{}\"/>", num(id), num(seq)),
        None => format!("<wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx>", if o.content.is_empty() { "<w:p/>" } else { &o.content }),
    };
    let sp_pr_rest = match &o.sp_pr_raw {
        Some(r) => r.clone(),
        None => format!("{fill}{ln}{}", o.sp_pr_extra.as_deref().unwrap_or("")),
    };
    let default_pos_h = NewPos { relative_from: "column".to_owned(), offset: Some(0.0), ..NewPos::default() };
    let default_pos_v = NewPos { relative_from: "paragraph".to_owned(), offset: Some(0.0), ..NewPos::default() };
    let body = default_textbox_body().merged(&o.body.clone().unwrap_or_default());
    let mut out = String::new();
    out.push_str("<w:drawing>");
    out.push_str(&format!(
        "<wp:anchor distT=\"{}\" distB=\"{}\" distL=\"{}\" distR=\"{}\" simplePos=\"0\" relativeHeight=\"{}\" behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">",
        num(d.t),
        num(d.b),
        num(d.l),
        num(d.r),
        num(o.relative_height),
        u8::from(o.behind_doc)
    ));
    out.push_str("<wp:simplePos x=\"0\" y=\"0\"/>");
    out.push_str(&new_pos_xml("wp:positionH", o.pos_h.as_ref().unwrap_or(&default_pos_h)));
    out.push_str(&new_pos_xml("wp:positionV", o.pos_v.as_ref().unwrap_or(&default_pos_v)));
    out.push_str(&format!("<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>{wrap}"));
    out.push_str(&format!("<wp:docPr id=\"{}\" name=\"{}\"/><wp:cNvGraphicFramePr/>", num(o.id), esc_attr(&o.name)));
    out.push_str("<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">");
    out.push_str("<wps:wsp><wps:cNvSpPr txBox=\"1\"/>");
    out.push_str(&format!(
        "<wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"{prst}\"><a:avLst{}</a:prstGeom>{sp_pr_rest}</wps:spPr>",
        if adj.is_empty() { "/>".to_owned() } else { format!(">{adj}</a:avLst>") }
    ));
    if let Some(s) = &o.style {
        out.push_str(&style_refs_xml(s));
    }
    out.push_str(&story);
    out.push_str(&body_pr_xml(&body));
    out.push_str("</wps:wsp></a:graphicData></a:graphic>");
    if let Some(r) = &o.size_rel_h {
        out.push_str(&format!(
            "<wp14:sizeRelH relativeFrom=\"{}\"><wp14:pctWidth>{}</wp14:pctWidth></wp14:sizeRelH>",
            r.relative_from,
            rnd(r.pct)
        ));
    }
    if let Some(r) = &o.size_rel_v {
        out.push_str(&format!(
            "<wp14:sizeRelV relativeFrom=\"{}\"><wp14:pctHeight>{}</wp14:pctHeight></wp14:sizeRelV>",
            r.relative_from,
            rnd(r.pct)
        ));
    }
    out.push_str("</wp:anchor></w:drawing>");
    out
}

// ───────────────────────────── shape-xml.ts: the VML fallback ─────────────────────────────

/// `vmlPt(emu)`: a length in pt as the VML of Word: no trailing zeros, no 0 before the point
/// (`.5pt`), `0` without a unit.
#[must_use]
pub fn vml_pt(emu: f64) -> String {
    let v = math::round((emu / 12700.0) * 100.0) / 100.0;
    if v == 0.0 {
        return "0".to_owned();
    }
    let s = to_js_string(v);
    let s = if let Some(rest) = s.strip_prefix("0.") {
        format!(".{rest}")
    } else if let Some(rest) = s.strip_prefix("-0.") {
        format!("-.{rest}")
    } else {
        s
    };
    format!("{s}pt")
}

/// `vmlColorOf(hex)`: a known name, a short `#rgb` or `#rrggbb` (lower case, as Word).
#[must_use]
pub fn vml_color_of(hex: &str) -> String {
    let h = uo_text::case::to_uppercase_str(hex);
    match h.as_str() {
        "000000" => return "black".to_owned(),
        "FFFFFF" => return "white".to_owned(),
        "FF0000" => return "red".to_owned(),
        "00FF00" => return "lime".to_owned(),
        "0000FF" => return "blue".to_owned(),
        "FFFF00" => return "yellow".to_owned(),
        _ => {}
    }
    let l = uo_jsnum::text::to_lowercase(&h);
    let u: Vec<u16> = l.encode_utf16().collect();
    let at = |i: usize| u.get(i).copied();
    if at(0) == at(1) && at(2) == at(3) && at(4) == at(5) {
        let pick = |i: usize| u.get(i).map_or_else(|| "undefined".to_owned(), |&x| String::from_utf16_lossy(&[x]));
        return format!("#{}{}{}", pick(0), pick(2), pick(4));
    }
    format!("#{l}")
}

fn h_rel(rf: &str) -> Option<&'static str> {
    Some(match rf {
        "column" => "text",
        "page" => "page",
        "margin" => "margin",
        "character" => "char",
        "leftMargin" => "left-margin-area",
        "rightMargin" => "right-margin-area",
        "insideMargin" => "inner-margin-area",
        "outsideMargin" => "outer-margin-area",
        _ => return None,
    })
}

fn v_rel(rf: &str) -> Option<&'static str> {
    Some(match rf {
        "paragraph" => "text",
        "page" => "page",
        "margin" => "margin",
        "line" => "line",
        "topMargin" => "top-margin-area",
        "bottomMargin" => "bottom-margin-area",
        "insideMargin" => "inner-margin-area",
        "outsideMargin" => "outer-margin-area",
        _ => return None,
    })
}

fn flow(vert: &str) -> Option<&'static str> {
    Some(match vert {
        "vert" => "layout-flow:vertical",
        "vert270" => "layout-flow:vertical;mso-layout-flow-alt:bottom-to-top",
        "eaVert" => "layout-flow:vertical-ideographic",
        "mongolianVert" | "wordArtVert" | "wordArtVertRtl" => "layout-flow:vertical;mso-layout-flow-alt:top-to-bottom",
        _ => return None,
    })
}

/// `vmlFallback(drawingXml, content, o)`: the `mc:Fallback` (VML) of a text box from its `w:drawing`
/// (wps), as Word writes it (oracle `textbox-lab`): `shapetype _x0000_t202` + `v:shape` with the
/// `docPr` name as id, position/size/z/wrap in the style, solid fill and outline, `v:textbox` with
/// margins, direction, fit and the link (`mso-next-textbox`), `w10:wrap`. Gradient/pattern/picture
/// fills become the colour of the first stop (the fallback is approximate); a preset other than a
/// rectangle gets its own path (`o:spt` 100) from `o.outline`. Empty when the drawing is not a shape.
pub fn vml_fallback(drawing_xml: &str, content: &str, o: &VmlOptions<'_>) -> WResult<String> {
    let Some(f) = read_shape_format(drawing_xml) else { return Ok(String::new()) };
    let b = &f.base;
    let default_body = default_textbox_body();
    let body = f.body.as_ref().unwrap_or(&default_body);
    let mut style: Vec<String> = Vec::new();
    let pos_part = |p: Option<&crate::model::picture::PicPos>, axis: char| -> String {
        let v = match p {
            Some(p) if p.align.is_some() => "0".to_owned(),
            Some(p) => vml_pt(or_num(p.offset, 0.0)),
            None => vml_pt(0.0),
        };
        format!("margin-{}:{v}", if axis == 'h' { "left" } else { "top" })
    };
    if !b.inline {
        style.push("position:absolute".to_owned());
        style.push(pos_part(b.pos_h.as_ref(), 'h'));
        style.push(pos_part(b.pos_v.as_ref(), 'v'));
    }
    style.push(format!("width:{}", vml_pt(b.cx.to_number())));
    style.push(format!("height:{}", vml_pt(b.cy.to_number())));
    if b.rot.truthy() {
        style.push(format!("rotation:{}", num(math::round((b.rot.to_number() / 60000.0) * 100.0) / 100.0)));
    }
    if b.flip_h || b.flip_v {
        style.push(format!("flip:{}{}", if b.flip_h { "x" } else { "" }, if b.flip_v { "y" } else { "" }));
    }
    if !b.inline {
        let rh = or_num(b.relative_height, 251_659_264.0);
        style.push(format!("z-index:{}", if b.behind_doc { num(-(rh - 2048.0)) } else { num(rh) }));
    }
    style.push(format!("visibility:{}", if b.doc_pr.hidden == Some(true) { "hidden" } else { "visible" }));
    style.push(format!("mso-wrap-style:{}", body.wrap.as_str()));
    if !b.inline {
        style.push(format!("mso-wrap-distance-left:{}", vml_pt(b.dist.l.to_number())));
        style.push(format!("mso-wrap-distance-top:{}", vml_pt(b.dist.t.to_number())));
        style.push(format!("mso-wrap-distance-right:{}", vml_pt(b.dist.r.to_number())));
        style.push(format!("mso-wrap-distance-bottom:{}", vml_pt(b.dist.b.to_number())));
        let h_from = b.pos_h.as_ref().map_or("column", |p| &p.relative_from);
        let v_from = b.pos_v.as_ref().map_or("paragraph", |p| &p.relative_from);
        let (h_r, v_r) = (h_rel(h_from).unwrap_or("text"), v_rel(v_from).unwrap_or("text"));
        style.push(format!("mso-position-horizontal:{}", b.pos_h.as_ref().and_then(|p| p.align.as_deref()).unwrap_or("absolute")));
        style.push(format!("mso-position-horizontal-relative:{h_r}"));
        style.push(format!("mso-position-vertical:{}", b.pos_v.as_ref().and_then(|p| p.align.as_deref()).unwrap_or("absolute")));
        style.push(format!("mso-position-vertical-relative:{v_r}"));
        if let Some(s) = b.size_rel_h.as_ref().filter(|s| s.pct.truthy()) {
            style.push(format!("mso-width-percent:{}", rnd(s.pct.to_number() / 100.0)));
            style.push(format!("mso-width-relative:{}", s.relative_from));
        }
        if let Some(s) = b.size_rel_v.as_ref().filter(|s| s.pct.truthy()) {
            style.push(format!("mso-height-percent:{}", rnd(s.pct.to_number() / 100.0)));
            style.push(format!("mso-height-relative:{}", s.relative_from));
        }
    }
    style.push(format!(
        "v-text-anchor:{}",
        match body.anchor {
            BodyAnchor::Ctr => "middle",
            BodyAnchor::B => "bottom",
            BodyAnchor::T => "top",
        }
    ));
    // fill
    let mut attrs: Vec<String> = Vec::new();
    let mut fill_el = String::new();
    let color1: Option<&PicColor> = match &f.fill {
        ShapeFill::Solid(c) => Some(c),
        ShapeFill::Gradient(g) => g.stops.first().map(|s| &s.color),
        ShapeFill::Pattern { bg, .. } => Some(bg),
        _ => None,
    };
    if matches!(f.fill, ShapeFill::None) {
        attrs.push("filled=\"f\"".to_owned());
        fill_el = "<v:fill o:detectmouseclick=\"t\"/>".to_owned();
    } else if let Some(c1) = color1 {
        attrs.push(format!("fillcolor=\"{}\"", vml_color_of(&(o.color)(c1)?)));
        let al = c1.mods.as_deref().and_then(|m| m.iter().find(|m| &*m.0 == "alpha")).map(|m| m.1.to_number());
        if let Some(al) = al
            && al < 100_000.0
        {
            fill_el = format!("<v:fill opacity=\"{}f\"/>", num(math::floor((math::round((al / 100_000.0) * 255.0) / 255.0) * 65536.0)));
        }
    }
    // outline
    let mut stroke_el = String::new();
    let ln: Option<&Nullable<Box<PicLine>>> = b.line.as_ref();
    match ln {
        Some(Nullable::Null) => attrs.push("stroked=\"f\"".to_owned()),
        Some(Nullable::Val(l)) if matches!(l.fill, Some(Nullable::Null)) => {
            attrs.push("stroked=\"f\"".to_owned());
            if let Some(w) = l.w {
                attrs.push(format!("strokeweight=\"{}\"", vml_pt(w.to_number())));
            }
        }
        Some(Nullable::Val(l)) => {
            if let Some(Nullable::Val(c)) = &l.fill {
                let col = (o.color)(c)?;
                if uo_text::case::to_uppercase_str(&col) != "000000" {
                    attrs.push(format!("strokecolor=\"{}\"", vml_color_of(&col)));
                }
            }
            attrs.push(format!("strokeweight=\"{}\"", vml_pt(or_num(l.w, 9525.0))));
            let mut sub: Vec<String> = Vec::new();
            if let Some(d) = l.dash.as_deref().filter(|d| !d.is_empty() && *d != "solid") {
                sub.push(format!("dashstyle=\"{d}\""));
            }
            if let Some(c) = l.cmpd.as_deref().filter(|c| !c.is_empty() && *c != "sng") {
                sub.push(format!("linestyle=\"{c}\""));
            }
            if !sub.is_empty() {
                stroke_el = format!("<v:stroke {}/>", sub.join(" "));
            }
        }
        None => {}
    }
    // geometry
    let geom = &*b.geom;
    let rect = geom == "rect" || geom == "custom";
    let mut type_attr = " type=\"#_x0000_t202\"".to_owned();
    let mut shapetype = if rect {
        "<v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>"
    } else {
        ""
    }
    .to_owned();
    let mut tag = "v:shape";
    if geom == "roundRect" {
        tag = "v:roundrect";
        let adj = or_num(b.adj.as_ref().and_then(|a| a.get("adj")).copied(), 16667.0);
        type_attr = format!(" arcsize=\"{}f\"", rnd((adj / 100_000.0) * 65536.0));
    } else if geom == "ellipse" {
        tag = "v:oval";
        type_attr = String::new();
    } else if !rect && let Some(outline) = o.outline {
        let cx = math::max(1.0, math::round(b.cx.to_number()));
        let cy = math::max(1.0, math::round(b.cy.to_number()));
        if let Some(path) = outline(geom, b.adj.as_ref(), cx, cy).filter(|p| !p.is_empty()) {
            type_attr = format!(" coordsize=\"{},{}\" o:spt=\"100\" path=\"{path}\"", num(cx), num(cy));
            shapetype = String::new();
        }
    }
    // text
    let mut tb_style: Vec<String> = Vec::new();
    if body.vert != "horz"
        && let Some(fl) = flow(&body.vert)
    {
        tb_style.push(fl.to_owned());
    }
    if body.autofit == BodyAutofit::Shape {
        tb_style.push("mso-fit-shape-to-text:t".to_owned());
    }
    if let Some(next) = o.next.filter(|n| !n.is_empty()) {
        tb_style.push(format!("mso-next-textbox:#{next}"));
    }
    let ins = [body.l_ins, body.t_ins, body.r_ins, body.b_ins];
    let def_ins = ins == [91440.0, 45720.0, 91440.0, 45720.0];
    let inset_attr =
        if def_ins { String::new() } else { format!(" inset=\"{}\"", ins.iter().map(|v| vml_pt(*v)).collect::<Vec<_>>().join(",")) };
    let tb_attrs =
        format!("{inset_attr}{}", if tb_style.is_empty() { String::new() } else { format!(" style=\"{}\"", tb_style.join(";")) });
    let name = b.doc_pr.name.as_deref().map_or_else(|| format!("_x0000_s{}", o.spid), str::to_owned);
    let title = b.doc_pr.title.as_deref().filter(|s| !s.is_empty());
    let descr = b.doc_pr.descr.as_deref().filter(|s| !s.is_empty());
    let alt = if descr.is_some() || title.is_some() {
        let parts: Vec<String> = [title.map(|t| format!("T\u{ed}tulo: {t}")), descr.map(|d| format!("Descri\u{e7}\u{e3}o: {d}"))]
            .into_iter()
            .flatten()
            .collect();
        format!(" alt=\"{}\"", esc_attr(&parts.join(" - ")))
    } else {
        String::new()
    };
    let wrap_type = match &*b.wrap {
        "square" => Some("square"),
        "tight" => Some("tight"),
        "through" => Some("through"),
        "topAndBottom" => Some("topAndBottom"),
        _ => None,
    };
    let ax = b.pos_h.as_ref().map(|p| &*p.relative_from).filter(|r| !r.is_empty() && *r != "column").map(|r| {
        if h_rel(r) == Some("char") {
            "char"
        } else if r == "page" {
            "page"
        } else {
            "margin"
        }
    });
    let ay = b.pos_v.as_ref().map(|p| &*p.relative_from).filter(|r| !r.is_empty() && *r != "paragraph").map(|r| {
        if r == "line" {
            "line"
        } else if r == "page" {
            "page"
        } else {
            "margin"
        }
    });
    let w10 = if !b.inline && (wrap_type.is_some() || ax.is_some() || ay.is_some()) {
        format!(
            "<w10:wrap{}{}{}/>",
            wrap_type.map_or_else(String::new, |t| format!(" type=\"{t}\"")),
            ax.map_or_else(String::new, |a| format!(" anchorx=\"{a}\"")),
            ay.map_or_else(String::new, |a| format!(" anchory=\"{a}\""))
        )
    } else {
        String::new()
    };
    Ok(format!(
        "<mc:Fallback><w:pict>{shapetype}<{tag} id=\"{}\" o:spid=\"_x0000_s{}\"{type_attr} style=\"{}\"{alt}{}>{fill_el}{stroke_el}<v:textbox{tb_attrs}><w:txbxContent>{content}</w:txbxContent></v:textbox>{w10}</{tag}></w:pict></mc:Fallback>",
        esc_attr(&name),
        o.spid,
        style.join(";"),
        if attrs.is_empty() { String::new() } else { format!(" {}", attrs.join(" ")) }
    ))
}

/// `textboxAlternateContent(drawingXml, content, o)`: the whole `mc:AlternateContent` of a box: the
/// wps (`mc:Choice`) and the VML fallback.
pub fn textbox_alternate_content(drawing_xml: &str, content: &str, o: &VmlOptions<'_>) -> WResult<String> {
    Ok(format!(
        "<mc:AlternateContent><mc:Choice Requires=\"wps\">{drawing_xml}</mc:Choice>{}</mc:AlternateContent>",
        vml_fallback(drawing_xml, content, o)?
    ))
}

/// `txbxContentOf(drawingXml)`: the content (blocks) of the first `w:txbxContent` of a raw
/// `w:drawing` (`/<w:txbxContent\b[^>]*>([\s\S]*?)<\/w:txbxContent>/`), `None` without a box.
#[must_use]
pub fn txbx_content_of(drawing_xml: &str) -> Option<String> {
    const OPEN: &str = "<w:txbxContent";
    const CLOSE: &str = "</w:txbxContent>";
    let mut at = 0;
    while let Some(i) = find_from(drawing_xml, OPEN, at) {
        at = i + 1;
        let after = i + OPEN.len();
        if !boundary_at(drawing_xml, after) {
            continue;
        }
        let gt = find_from(drawing_xml, ">", after)?;
        let c = find_from(drawing_xml, CLOSE, gt + 1)?;
        return drawing_xml.get(gt + 1..c).map(str::to_owned);
    }
    None
}

// ───────────────────────────── shape-xml.ts: surgical edits ─────────────────────────────

/// `ShapeChange.story`: the box becomes the first of a chain (`id`), a later one (`linkedTxbx`) or has
/// its own content again (`txbx` without id, with `content`).
#[derive(Debug, Clone, PartialEq)]
pub enum StoryChange {
    Txbx { id: Option<f64>, content: Option<String> },
    Linked { id: f64, seq: f64 },
}

/// `ShapeChange.stylePreset`: the `spPr` children after the geometry (fill and outline) and the
/// `wps:style` of a Shape Style.
#[derive(Debug, Clone, PartialEq)]
pub struct StylePreset {
    pub sp_pr: String,
    pub style: String,
}

/// `ShapeChange` (EMU): a `PictureChange` plus the shape parts. `None` = `undefined`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeChange {
    /// `extends PictureChange`
    pub pic: PictureChange,
    pub fill: Option<ShapeFill>,
    /// `outline?: PicLine | null` (`Some(None)` = `null` = no outline)
    pub outline: Option<Option<PicLine>>,
    /// `style?: ShapeStyleRefs | null`
    pub style: Option<Option<ShapeStyleRefs>>,
    pub body: Option<TextboxBodyPatch>,
    pub story: Option<StoryChange>,
    pub style_preset: Option<StylePreset>,
}

const FILL_NAMES: [&str; 6] = ["a:noFill", "a:solidFill", "a:gradFill", "a:blipFill", "a:pattFill", "a:grpFill"];
const WSP_ORDER: [&str; 10] = [
    "wps:cNvPr",
    "wps:cNvSpPr",
    "wps:cNvCnPr",
    "wps:nvPr",
    "wps:spPr",
    "wps:style",
    "wps:extLst",
    "wps:txbx",
    "wps:linkedTxbx",
    "wps:bodyPr",
];

/// `partsOf(raw)`: the parsed drawing (the TS throws on text that does not parse or has no
/// `wp:inline`/`wp:anchor`: an error here).
fn parse_parts(raw: &str) -> WResult<uo_opc::xml::Document> {
    let doc = parse_drawing(raw).ok_or(DocxError::Write("parseXml of the drawing failed (TS throws)"))?;
    parts_of(&doc).ok_or(DocxError::Write("w:drawing without wp:inline/wp:anchor (TS throws)"))?;
    Ok(doc)
}

fn parts<'d>(doc: &'d uo_opc::xml::Document) -> WResult<crate::read::picture_format::Parts<'d>> {
    parts_of(doc).ok_or(DocxError::Write("w:drawing without wp:inline/wp:anchor (TS throws)"))
}

fn positions(el: Element<'_>) -> WResult<uo_opc::xml::Positions> {
    el.positions().ok_or(DocxError::Write("an element without source positions (TS `!` on `s`)"))
}

/// `patchShape(raw, ch)`: applies the changes to the raw `w:drawing` of a shape or text box. The
/// parts the shape shares with a picture (anchor, wrap, position, size, `docPr`, locks, `xfrm`,
/// effects, 3D) are `patchPicture`'s; each step parses the text again.
pub fn patch_shape(raw: &str, ch: &ShapeChange) -> WResult<String> {
    let mut xml = patch_picture(raw, &ch.pic)?;
    if let Some(sp) = &ch.style_preset {
        xml = patch_style_preset(&xml, sp)?;
    }
    if let Some(line) = &ch.outline {
        let prev = read_picture_format(raw, true).and_then(|f| f.line);
        xml = patch_outline(&xml, line.as_ref(), prev.as_ref())?;
    }
    if let Some(f) = &ch.fill {
        xml = patch_fill(&xml, f)?;
    }
    if let Some(s) = &ch.style {
        xml = patch_style(&xml, s.as_ref())?;
    }
    if let Some(b) = &ch.body {
        xml = patch_body(&xml, b)?;
    }
    if let Some(s) = &ch.story {
        xml = patch_story(&xml, s)?;
    }
    Ok(xml)
}

fn patch_fill(raw: &str, f: &ShapeFill) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let Some(sp) = p.sp_pr else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    let cur: Vec<Element<'_>> = sp.child_elements().filter(|k| FILL_NAMES.contains(&k.name())).collect();
    let xml = shape_fill_xml(f);
    if let Some((first, rest)) = cur.split_first() {
        patch.replace(*first, &xml)?;
        for k in rest {
            patch.remove(*k)?;
        }
    } else if !xml.is_empty() {
        patch.insert_child(sp, "a:solidFill", &xml, &SPPR_ORDER)?;
    }
    patch.apply()
}

/// Shape Style: the fill and the outline of the `spPr` replaced by the style's (effects and 3D
/// stay) and the `wps:style`.
fn patch_style_preset(raw: &str, sp: &StylePreset) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let (Some(wsp), Some(sp_pr)) = (p.wsp, p.sp_pr) else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    let kids: Vec<Element<'_>> = sp_pr.child_elements().collect();
    for k in kids.iter().filter(|k| FILL_NAMES.contains(&k.name()) || k.name() == "a:ln") {
        patch.remove(*k)?;
    }
    let geom = kids.iter().rev().find(|k| matches!(k.name(), "a:xfrm" | "a:prstGeom" | "a:custGeom"));
    let pp = positions(sp_pr)?;
    if let Some(g) = geom {
        patch.insert_at(positions(*g)?.end, &sp.sp_pr);
    } else if pp.content_start == pp.end {
        patch.set_content(sp_pr, &sp.sp_pr)?;
    } else {
        patch.insert_at(pp.content_start, &sp.sp_pr);
    }
    patch.put_child(wsp, "wps:style", &sp.style, &WSP_ORDER)?;
    patch.apply()
}

fn patch_style(raw: &str, s: Option<&ShapeStyleRefs>) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let Some(wsp) = p.wsp else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    patch.put_child(wsp, "wps:style", &s.map_or_else(String::new, style_refs_xml), &WSP_ORDER)?;
    patch.apply()
}

fn patch_body(raw: &str, b: &TextboxBodyPatch) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let Some(wsp) = p.wsp else { return Ok(raw.to_owned()) };
    let el = wsp.first_child("wps:bodyPr");
    let cur = read_body(el).unwrap_or_else(default_textbox_body);
    let next = cur.merged(b);
    let mut patch = XmlPatch::new(raw);
    let Some(el) = el else {
        patch.insert_child(wsp, "wps:bodyPr", &body_pr_xml(&next), &WSP_ORDER)?;
        return patch.apply();
    };
    // only the attributes that changed; the autofit is a child
    let mut set: Vec<(&str, Option<String>)> = Vec::new();
    let mut num_changed = |name: &'static str, new: Option<f64>, old: f64| {
        if let Some(v) = new
            && v != old
        {
            set.push((name, Some(rnd(v))));
        }
    };
    num_changed("lIns", b.l_ins, cur.l_ins);
    num_changed("tIns", b.t_ins, cur.t_ins);
    num_changed("rIns", b.r_ins, cur.r_ins);
    num_changed("bIns", b.b_ins, cur.b_ins);
    if let Some(v) = b.anchor
        && v != cur.anchor
    {
        set.push(("anchor", Some(v.as_str().to_owned())));
    }
    if let Some(v) = b.anchor_ctr
        && v != cur.anchor_ctr
    {
        set.push(("anchorCtr", Some(u8::from(v).to_string())));
    }
    if let Some(v) = &b.vert
        && *v != cur.vert
    {
        set.push(("vert", Some(v.clone())));
    }
    if let Some(v) = b.wrap
        && v != cur.wrap
    {
        set.push(("wrap", Some(v.as_str().to_owned())));
    }
    if let Some(v) = b.upright
        && v != cur.upright
    {
        set.push(("upright", if v { Some("1".to_owned()) } else { None }));
    }
    if let Some(v) = b.rot
        && v != cur.rot
    {
        set.push(("rot", Some(num(v))));
    }
    if !set.is_empty() {
        patch.set_attrs(el, &set)?;
    }
    if let Some(a) = b.autofit
        && a != cur.autofit
    {
        let old = el.child_elements().find(|k| matches!(k.name(), "a:spAutoFit" | "a:noAutofit" | "a:normAutofit"));
        let ep = positions(el)?;
        if let Some(old) = old {
            patch.replace(old, a.xml())?;
        } else if ep.content_start == ep.end {
            patch.set_content(el, a.xml())?;
        } else {
            patch.insert_at(ep.content_end, a.xml());
        }
    }
    patch.apply()
}

fn patch_story(raw: &str, s: &StoryChange) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let Some(wsp) = p.wsp else { return Ok(raw.to_owned()) };
    let txbx = wsp.first_child("wps:txbx");
    let linked = wsp.first_child("wps:linkedTxbx");
    let Some(cur) = txbx.or(linked) else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    match s {
        StoryChange::Linked { id, seq } => patch.replace(cur, &format!("<wps:linkedTxbx id=\"{}\" seq=\"{}\"/>", num(*id), num(*seq)))?,
        StoryChange::Txbx { id, content: None } if txbx.is_some() => {
            if let Some(t) = txbx {
                patch.set_attrs(t, &[("id", id.map(num))])?;
            }
        }
        StoryChange::Txbx { id, content } => {
            let id_attr = id.map_or_else(String::new, |i| format!(" id=\"{}\"", num(i)));
            patch.replace(
                cur,
                &format!("<wps:txbx{id_attr}><w:txbxContent>{}</w:txbxContent></wps:txbx>", content.as_deref().unwrap_or("<w:p/>")),
            )?;
        }
    }
    patch.apply()
}

/// The outline of a shape: the new line (the `a14:hiddenLine` goes away) or "No Outline" with the
/// hiddenLine keeping the previous one.
fn patch_outline(raw: &str, line: Option<&PicLine>, prev: Option<&Nullable<Box<PicLine>>>) -> WResult<String> {
    let doc = parse_parts(raw)?;
    let p = parts(&doc)?;
    let Some(sp) = p.sp_pr else { return Ok(raw.to_owned()) };
    let mut patch = XmlPatch::new(raw);
    let ext = sp.first_child("a:extLst");
    let hid = ext.and_then(|e| e.children_named("a:ext").find(|x| x.attr("uri") == Some(HIDDEN_LINE_URI)));
    match line {
        None => {
            let prev_line: Option<&PicLine> = match prev {
                Some(Nullable::Val(l)) => Some(&**l),
                _ => None,
            };
            let basis = match prev_line {
                Some(l) if !matches!(l.fill, Some(Nullable::Null)) => l.clone(),
                other => PicLine {
                    w: Some(other.and_then(|l| l.w).filter(|w| !w.is_null()).unwrap_or(JsNumber::Num(6350.0))),
                    ..PicLine::default()
                },
            };
            let (ln, ext_xml) = hidden_line_xml(Some(&basis));
            patch.put_child(sp, "a:ln", &ln, &SPPR_ORDER)?;
            if let Some(h) = hid {
                patch.replace(h, &ext_xml)?;
            } else if let Some(e) = ext {
                patch.insert_at(positions(e)?.content_end, &ext_xml);
            } else {
                patch.insert_child(sp, "a:extLst", &format!("<a:extLst>{ext_xml}</a:extLst>"), &SPPR_ORDER)?;
            }
        }
        Some(l) => {
            patch.put_child(sp, "a:ln", &pic_line_xml(l), &SPPR_ORDER)?;
            if let Some(h) = hid {
                match ext {
                    Some(e) if e.child_elements().count() == 1 => patch.remove(e)?,
                    _ => patch.remove(h)?,
                }
            }
        }
    }
    patch.apply()
}
