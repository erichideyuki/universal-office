//! Shell exports of the wave-5 frontier R (the editor switch: review and document; record
//! `core/docs/modules/uo-bindings.md` "Wave 5 — R"): the line geometry of the edit scope that the TS
//! shell's screen overlays read (the field shading and the comment highlight, the spelling squiggles,
//! the search hits, the revision and comment anchors of the balloons), the selection rectangles of an
//! arbitrary range (the extra ranges of a multi-selection) and the document's embedded font families
//! (`knownFontFamilies()`). Conventions as `shell.rs`: `UoBuf` statuses 0 ok, 1 none, 11 null handle.
//!
//! The lines are the session's cached `EditLines` of the layout (`UoWriteSession::lines`): the edit
//! scope only (the body, or the context being edited), so the overlays of table cells, headers/footers,
//! notes and text boxes outside the edited story are not answered (declared ED-304).

use uo_docx_layout::hit::{EditLine, EditLines, EditPos, SelRect};

use super::codec as w;
use super::handle::UoWriteSession;
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf};
use crate::write::UoLayout;

fn line_wire(el: &EditLines, para: u32, l: &EditLine) -> w::ShellLine {
    let cs = el.chars(l);
    w::ShellLine {
        para,
        page: l.page,
        x: l.x,
        y: l.y,
        height: l.height,
        baseline: l.baseline,
        width: l.width,
        start: l.start,
        end: l.end,
        xf: l.xf.map(|m| m.to_vec()),
        o: cs.o.to_vec(),
        cx: cs.x.to_vec(),
        cw: cs.w.to_vec(),
    }
}

fn encode_lines(lines: Vec<w::ShellLine>) -> *mut UoBuf {
    let mut out = Vec::new();
    match w::encode_shell_lines(&mut out, &w::Delta::default(), &w::ShellLines { lines }) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(super::handle::BAD_ARGUMENT, format!("encode: {e:?}").into_bytes()),
    }
}

/// The lines of the edit scope on page `page` (every paragraph), in paragraph order.
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_page_lines(h: *mut UoWriteSession, layout: *const UoLayout, page: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let mut lines = Vec::new();
    for (pi, p) in el.paras.iter().enumerate() {
        let para = u32::try_from(pi).unwrap_or(u32::MAX);
        for ln in p.lines.iter().filter(|ln| ln.page == page) {
            lines.push(line_wire(el, para, ln));
        }
    }
    encode_lines(lines)
}

/// The lines of paragraph `para` of the edit scope (every page).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_para_lines(h: *mut UoWriteSession, layout: *const UoLayout, para: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let Some(p) = el.para(EditPos { para, offset: 0 }) else { return buf(NONE, Vec::new()) };
    let lines = p.lines.iter().map(|ln| line_wire(el, para, ln)).collect();
    encode_lines(lines)
}

/// The selection rectangles of the range `(ap, ao)`–`(fp, fo)` on page `page`, in either direction
/// (normalised first: a backward range answers the forward one's rectangles).
///
/// # Safety
/// `h`, `layout` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_range_rects(
    h: *mut UoWriteSession,
    layout: *const UoLayout,
    page: u32,
    ap: u32,
    ao: u32,
    fp: u32,
    fo: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the handles are null or live.
    let (Some(s), Some(l)) = (unsafe { h.as_mut() }, unsafe { layout.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(el) = s.lines(l) else { return buf(NONE, Vec::new()) };
    let (a, f) = (EditPos { para: ap, offset: ao }, EditPos { para: fp, offset: fo });
    let (start, end) = if (fp, fo) < (ap, ao) { (f, a) } else { (a, f) };
    let mut rects: Vec<SelRect> = Vec::new();
    if start != end {
        el.selection_rects(page, start, end, &mut rects);
    }
    let m =
        w::SelRects { rects: rects.iter().map(|r| w::SelRect { x: r.x, y: r.y, w: r.w, h: r.h, xf: r.xf.map(|m| m.to_vec()) }).collect() };
    let mut out = Vec::new();
    match w::encode_sel_rects(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(super::handle::BAD_ARGUMENT, format!("encode: {e:?}").into_bytes()),
    }
}

/// The families of the document's embedded fonts (UTF-8, one per line, each once, in document order).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_embedded_families(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let mut seen: Vec<&str> = Vec::new();
    for f in &s.embedded {
        if !seen.contains(&&*f.family) {
            seen.push(&f.family);
        }
    }
    buf(OK, seen.join("\n").into_bytes())
}
