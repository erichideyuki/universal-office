//! Wave 5, front O of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 5 — O: objects and
//! the page"): the object geometry and the section facts the editor's TS shell needs over a session and its
//! layout. Additive exports next to `shell.rs` (same conventions: `UoBuf` statuses 0 ok, 11 null handle).
//!
//! - `uo_shell_objects(h, l, active_page)`: the port of `enumFloats` (the engine branch) and `enumInlinePics`
//!   of universal-write.ts. The TS holds the `Run` objects; here the identity is the drawing id the layout
//!   keeps on each `PlacedObject` and each image (`ImageOrigin::Media { drawing }`), joined with uo-docx's
//!   [`Session::drawing_objects`]/[`Session::inline_pictures`] (ED-201). Answered as JSON text (UTF-8).
//! - `uo_shell_section_info(h, hf_section)`: what `getSection()`, `getLineNumbering()`, `getPageBorders()` and
//!   `lineNumbersSuppressed()` read, the model objects as their `JSON.stringify` text (uo-docx `JsonWriter`,
//!   the TS key order); the shell computes the TS answers with the TS helpers of docx-core (pure functions).
//!
//! [`Session::drawing_objects`]: uo_docx::session::Session::drawing_objects
//! [`Session::inline_pictures`]: uo_docx::session::Session::inline_pictures

use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::session::ops::drawing::PaneKind;
use uo_docx_layout::output::{ImageOrigin, Layer, ObjectFrame};
use uo_jsnum::json::{push_number, push_string};
use uo_render::Item;

use super::handle::UoWriteSession;
use crate::render::{NULL_HANDLE, OK, UoBuf, buf};
use crate::write::{UoLayout, layout_geom, layout_view};

fn kind_name(k: PaneKind) -> &'static str {
    match k {
        PaneKind::Image => "image",
        PaneKind::Textbox => "textbox",
        PaneKind::Shape => "shape",
        PaneKind::Group => "group",
    }
}

/// `"name":<number>` with a leading comma when `comma`.
fn field(out: &mut String, comma: bool, name: &str, x: f64) {
    if comma {
        out.push(',');
    }
    push_string(out, name);
    out.push(':');
    push_number(out, x);
}

fn frame_json(out: &mut String, f: &ObjectFrame) {
    // the session's `ObjFrame` field names and order (the `facts` of SetPosition/SetLayout/PlaceAt)
    out.push('{');
    field(out, false, "pageW", f.page_w);
    field(out, true, "pageH", f.page_h);
    field(out, true, "marL", f.mar_l);
    field(out, true, "marR", f.mar_r);
    field(out, true, "marT", f.mar_t);
    field(out, true, "marB", f.mar_b);
    field(out, true, "colX", f.col_x);
    field(out, true, "colW", f.col_w);
    field(out, true, "bodyTop", f.body_top);
    field(out, true, "paraTop", f.para_top);
    out.push('}');
}

/// A count or an index as a JS number.
fn num(n: usize) -> f64 {
    u32::try_from(n).map_or(f64::MAX, f64::from)
}

fn json_of(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap_or_else(|| "null".to_owned())
}

/// The floating objects and inline pictures of the session's context on layout `l` (JSON, UTF-8):
///
/// `{ scope: 'body'|'hf'|'tb', fs: index|null, sel: [drawing id…], floats: [{ i, kind, id, page, x, y, w,
/// h, rot, behind, editable, content, key|null, image|null, frame|null }], inline: [{ para, run, id, page, x, y, w, h, rot, image }] }`
///
/// `floats` follows `enumFloats()` (engine branch): every page's `objects` in order, the header/footer
/// layer only in a header/footer context and only on `active_page` (−1: any page), objects of the
/// context's drawing runs, the first placement of each. `inline` follows `enumInlinePics()`: the image
/// items (and the clip pushes with a source and a box) of the body layer (the header/footer layer of
/// `active_page` in a header/footer context) whose image is a model picture in line. Empty in a text box
/// (the TS `tbEdit`; the floats are listed there, the shell skips them in its hit tests). `box`: the text box in edit. `rot` of an inline picture is the image item's (the TS reads the model's style rot,
/// which the layout copies). `image` is the layout's image index (`imageBytes`).
///
/// # Safety
/// `h`, `l` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_objects(h: *const UoWriteSession, l: *const UoLayout, active_page: i32, flags: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_ref() }, unsafe { l.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let session = &s.session;
    let geom = layout_geom(l);
    let (in_hf, in_box) = session.drawing_scope();
    // the floating objects are listed in a text box too (the shell paints the handles of the box in edit and places a box it
    // just drew; its hit tests skip them there, as the TS `enumFloats` is empty in `tbEdit`); `flags` is reserved
    let _ = flags;
    let objects = session.drawing_objects();
    let inline = session.inline_pictures();
    let mut out = String::with_capacity(256);
    out.push_str("{\"scope\":");
    push_string(
        &mut out,
        if in_hf {
            "hf"
        } else if in_box {
            "tb"
        } else {
            "body"
        },
    );
    out.push_str(",\"box\":");
    match session.edited_box_drawing() {
        Some(d) => push_number(&mut out, f64::from(d.0)),
        None => out.push_str("null"),
    }
    out.push_str(",\"fs\":");
    match session.float_selection() {
        Some(i) => push_number(&mut out, f64::from(i)),
        None => out.push_str("null"),
    }
    out.push_str(",\"sel\":[");
    for (n, d) in session.selected_drawings().iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        push_number(&mut out, f64::from(d.0));
    }
    out.push_str("],\"floats\":[");
    let page_ok = |pg: usize| !in_hf || active_page < 0 || usize::try_from(active_page).is_ok_and(|a| a == pg);
    let mut seen: Vec<u32> = Vec::new();
    let mut first = true;
    {
        for (pg, page) in geom.pages.iter().enumerate() {
            if !page_ok(pg) {
                continue;
            }
            for o in page.objects.iter().flatten() {
                let Some(d) = o.drawing else { continue };
                if o.hf != in_hf || seen.contains(&d.0) {
                    continue;
                }
                let Some((i, obj)) = objects.iter().enumerate().find(|(_, x)| x.id == d) else { continue };
                seen.push(d.0);
                if !first {
                    out.push(',');
                }
                first = false;
                out.push('{');
                field(&mut out, false, "i", num(i));
                out.push_str(",\"kind\":");
                push_string(&mut out, kind_name(obj.kind));
                field(&mut out, true, "id", f64::from(d.0));
                field(&mut out, true, "page", num(pg));
                field(&mut out, true, "x", o.x);
                field(&mut out, true, "y", o.y);
                field(&mut out, true, "w", o.w);
                field(&mut out, true, "h", o.h);
                field(&mut out, true, "rot", obj.rot);
                out.push_str(if matches!(o.layer, Layer::Behind) { ",\"behind\":true" } else { ",\"behind\":false" });
                out.push_str(if obj.editable { ",\"editable\":true" } else { ",\"editable\":false" });
                out.push_str(if o.content.is_some() { ",\"content\":true" } else { ",\"content\":false" });
                out.push_str(",\"key\":");
                match &obj.pane_key {
                    Some(k) => push_string(&mut out, k),
                    None => out.push_str("null"),
                }
                // a picture's media in the layout's image table (`imageBytes`: the natural size, the previews)
                out.push_str(",\"image\":");
                match geom.image_origins.iter().position(|x| matches!(x, ImageOrigin::Media { drawing: Some(e), .. } if *e == d)) {
                    Some(k) if matches!(obj.kind, PaneKind::Image) => push_number(&mut out, num(k)),
                    _ => out.push_str("null"),
                }
                out.push_str(",\"frame\":");
                match &o.frame {
                    Some(f) => frame_json(&mut out, f),
                    None => out.push_str("null"),
                }
                out.push('}');
            }
        }
    }
    out.push(']');
    // wave 7c (V, ED-106b): a box in edit that is not one of the context's own drawing runs (a box inside a box: the
    // base story's drawing paths do not descend into a box) — its rectangle is the layout's page object of that run with
    // laid-out content, as the TS `tbEdit.rectTwips` (universal-write.ts 16470–16497, `o.run === focusRun`), so
    // `activeFloat()` frames it and gives it handles (13590)
    out.push_str(",\"boxFloat\":");
    match session.edited_box().filter(|(_, _, own)| !own) {
        Some((d, rot, _)) => {
            let hit = geom.pages.iter().enumerate().find_map(|(pg, page)| {
                if !page_ok(pg) {
                    return None;
                }
                page.objects.iter().flatten().find(|o| o.drawing == Some(d) && o.content.is_some() && o.hf == in_hf).map(|o| (pg, o))
            });
            match hit {
                Some((pg, o)) => {
                    out.push('{');
                    field(&mut out, false, "i", -1.0);
                    out.push_str(",\"kind\":\"textbox\"");
                    field(&mut out, true, "id", f64::from(d.0));
                    field(&mut out, true, "page", num(pg));
                    field(&mut out, true, "x", o.x);
                    field(&mut out, true, "y", o.y);
                    field(&mut out, true, "w", o.w);
                    field(&mut out, true, "h", o.h);
                    field(&mut out, true, "rot", rot);
                    out.push_str(if matches!(o.layer, Layer::Behind) { ",\"behind\":true" } else { ",\"behind\":false" });
                    out.push_str(",\"editable\":true,\"content\":true,\"key\":null,\"image\":null,\"frame\":");
                    match &o.frame {
                        Some(f) => frame_json(&mut out, f),
                        None => out.push_str("null"),
                    }
                    out.push('}');
                }
                None => out.push_str("null"),
            }
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"inline\":[");
    let mut first = true;
    if !in_box && !inline.is_empty() {
        let pic_of = |img: uo_render::ImageId| -> Option<&(uo_docx::ids::DrawingId, u32, u32)> {
            let d = match geom.image_origins.get(img.index())? {
                ImageOrigin::Media { drawing: Some(d), .. } => *d,
                _ => return None,
            };
            inline.iter().find(|x| x.0 == d)
        };
        let view = layout_view(l);
        for (pg, page) in geom.pages.iter().enumerate() {
            if in_hf && !page_ok(pg) {
                continue;
            }
            // the display lists live in the layout's render view (`layout_handle` moved them out of the geometry)
            let Some(view_page) = view.page(pg) else { continue };
            let items = &view_page.list.items;
            let hf_n = page.hf_items.map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX)).min(items.len());
            let range = if in_hf { 0..hf_n } else { hf_n..items.len() };
            for idx in range {
                let Some(it) = items.get(idx) else { continue };
                let (img, x, y, w, hh, rot) = match it {
                    Item::Image(im) => (im.image, im.x, im.y, im.w, im.h, im.rot.get().unwrap_or(0.0)),
                    Item::ClipPush { x, y, w, h } => {
                        let Some(src) = page.clip_src.iter().find(|c| usize::try_from(c.0).is_ok_and(|c0| c0 == idx)).map(|c| c.1) else {
                            continue;
                        };
                        let Some(y) = y.get() else { continue };
                        (src, x.get().unwrap_or(f64::NAN), y, w.get().unwrap_or(f64::NAN), h.get().unwrap_or(f64::NAN), 0.0)
                    }
                    _ => continue,
                };
                let Some(&(d, para, run)) = pic_of(img) else { continue };
                if !first {
                    out.push(',');
                }
                first = false;
                out.push('{');
                field(&mut out, false, "para", f64::from(para));
                field(&mut out, true, "run", f64::from(run));
                field(&mut out, true, "id", f64::from(d.0));
                field(&mut out, true, "page", num(pg));
                field(&mut out, true, "x", x);
                field(&mut out, true, "y", y);
                field(&mut out, true, "w", w);
                field(&mut out, true, "h", hh);
                field(&mut out, true, "rot", rot);
                field(&mut out, true, "image", num(img.index()));
                out.push('}');
            }
        }
    }
    out.push_str("]}");
    buf(OK, out.into_bytes())
}

/// The section facts of the caret (JSON, UTF-8): `{ index, count, touched: [sectPr…], pageBorderSection:
/// sectPr, firstSect: SectionProps|null, suppressed, titlePg: [bool…], background: PageBackground|null, lastSect: sectPr|null,
/// bgShown, evenOdd }` (see `Session::shell_section_now`); `hf_section` is the
/// section of the page in header/footer edit (−1: none).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_section_info(h: *const UoWriteSession, hf_section: i32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let info = s.session.shell_section_now(u32::try_from(hf_section).ok());
    let mut out = String::with_capacity(512);
    out.push('{');
    field(&mut out, false, "index", num(info.first));
    field(&mut out, true, "count", num(info.count));
    out.push_str(",\"touched\":[");
    for (n, sp) in info.touched.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        out.push_str(&json_of(&**sp));
    }
    out.push_str("],\"pageBorderSection\":");
    out.push_str(&json_of(&*info.page_border_section));
    out.push_str(",\"firstSect\":");
    match &info.first_section_props {
        Some(p) => out.push_str(&json_of(&**p)),
        None => out.push_str("null"),
    }
    out.push_str(if info.line_numbers_suppressed { ",\"suppressed\":true" } else { ",\"suppressed\":false" });
    out.push_str(",\"titlePg\":[");
    for (n, t) in info.title_pg.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        out.push_str(if *t { "true" } else { "false" });
    }
    // the page background (`pageBackground()`: `fullDoc.background` and `settings.displayBackgroundShape`) and
    // `evenOddHeaders()` (`hfTypeOfPage`), read from the session's document
    let doc = s.session.document();
    out.push_str("],\"background\":");
    match &doc.background {
        Some(b) => out.push_str(&json_of(&**b)),
        None => out.push_str("null"),
    }
    // the body's final sectPr (the TS `sect()` = `model.sectPr`: the Read mode zoom, the picture width of `insertImage`)
    out.push_str(",\"lastSect\":");
    match doc.sections.last() {
        Some(sc) => out.push_str(&json_of(&*sc.sect_pr)),
        None => out.push_str("null"),
    }
    let settings = &doc.formatting.settings;
    out.push_str(if settings.display_background_shape { ",\"bgShown\":true" } else { ",\"bgShown\":false" });
    out.push_str(if settings.even_and_odd_headers { ",\"evenOdd\":true}" } else { ",\"evenOdd\":false}" });
    buf(OK, out.into_bytes())
}
