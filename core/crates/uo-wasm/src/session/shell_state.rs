//! The toolbar state of the editor shell (`getSelectionState()`, universal-write.ts 2992–3011) over a
//! session: the run props at the caret (`pendingProps ?? currentProps()`) and the paragraph props, with
//! the font and size the toolbar shows filled from the style cascade as the TS does.
//!
//! Declared (record "Milestone 2: the editor switch"): a port of TS behaviour placed in uo-wasm because
//! uo-docx and the session schema belong to neither frontier of the wave (gate item: move it to uo-docx
//! `Session::selection_state()` and a `QueryOp`); `view` and `marks` are the shell's own state and
//! `table` (`getTableState()`) is not ported (the shell answers null); `withLatent` (a built-in style the
//! document never defined) and the editor's built-in `STYLES` table (documents without `styles.xml`) are
//! not ported: such ids resolve as undefined styles (ED-14).

use uo_docx::model::formatting::DocFormatting;
use uo_docx::model::props::RPr;
use uo_docx::model::run::RunProps;
use uo_docx::read::plain::default_run_props;
use uo_docx::session::text::{CarriedProps, props_at};
use uo_docx::word::resolve::{FontSlot, StyleResolver, cascade_rpr, font_of_slot};

use super::codec as w;
use super::handle::{BAD_ARGUMENT, UoWriteSession};
use super::wire::IntoWire;
use crate::render::{NULL_HANDLE, OK, UoBuf, buf};

/// `DEFAULT_FONT` of universal-write.ts.
const DEFAULT_FONT: &str = "Liberation Sans";

/// `lookOf(pPr, rPr, theme, dd)` reduced to the two fields `getSelectionState` reads: the font
/// family and the size in points.
struct Look {
    font_family: String,
    size_pt: f64,
}

/// `styleLook(docStyles, theme, id)` (styles.ts 285): `styleProps` then `lookOf`.
fn style_look(fmt: &DocFormatting, id: &str) -> Look {
    let mut res = StyleResolver::new();
    let dd = StyleResolver::doc_defaults(fmt);
    let character = fmt.styles.styles.get(id).is_some_and(|d| d.r#type.as_ref() == "character");
    let r_pr: RPr = if character {
        let para = res.para_style(fmt, None);
        let ch = res.char_style(fmt, Some(id));
        cascade_rpr(dd.r_pr.as_deref(), &[Some(&para.r_pr), Some(&ch)], None)
    } else {
        // (the look's paragraph half, `mergePPr(dd.pPr, ps.pPr)`, is not read by `getSelectionState`)
        let ps = res.para_style(fmt, Some(id));
        cascade_rpr(dd.r_pr.as_deref(), &[Some(&ps.r_pr)], None)
    };
    let theme = &fmt.theme;
    let font_family = font_of_slot(r_pr.r_fonts.as_deref(), FontSlot::Ascii, theme)
        .or_else(|| font_of_slot(r_pr.r_fonts.as_deref(), FontSlot::HAnsi, theme))
        .or_else(|| font_of_slot(dd.r_pr.as_deref().and_then(|r| r.r_fonts.as_deref()), FontSlot::Ascii, theme))
        .or(theme.minor.latin.as_deref())
        .unwrap_or("Times New Roman")
        .to_owned();
    let sz = r_pr.sz.and_then(uo_docx::js::JsNumber::as_f64).unwrap_or(20.0);
    Look { font_family, size_pt: sz / 2.0 }
}

/// `getSelectionState()` without `view`, `marks` and `table`: (run, para, whether the run props are
/// the pending ones).
fn selection_state(s: &UoWriteSession) -> (RunProps, uo_docx::model::para::ParaProps, bool) {
    let session = &s.session;
    let pending = session.pending_props().cloned();
    let has_pending = pending.is_some();
    let focus = session.selection().focus;
    let current = || match session.paras() {
        Some(paras) => props_at(paras, focus),
        None => CarriedProps::plain(default_run_props()),
    };
    let mut run = pending.unwrap_or_else(current).props;
    let doc = session.document();
    let p = session.paras().and_then(|ps| usize::try_from(focus.para).ok().and_then(|i| ps.get(i)));
    // `styleOf(p)`: the document's resolved style (the built-in table is not ported, ED-14)
    let st = p.and_then(|p| p.props.style_id.as_deref()).and_then(|id| doc.styles.get(id));
    let dd = doc.styles.get("docDefaults");
    let mut para = p.map(|p| (*p.props).clone()).unwrap_or_default();
    if para.alignment.is_none() {
        para.alignment = st.and_then(|s| s.para.alignment.clone());
    }
    let has_xml = session.package().contains("word/styles.xml");
    let look = if has_xml && (run.font_ascii.is_none() || run.sz_half_pt.is_none()) {
        let fmt = &*doc.formatting;
        let default_para = fmt.styles.defaults.paragraph.as_deref().unwrap_or("Normal");
        let id = run.char_style.as_deref().or_else(|| p.and_then(|p| p.props.style_id.as_deref())).unwrap_or(default_para);
        Some(style_look(fmt, id))
    } else {
        None
    };
    if run.font_ascii.is_none() {
        let f = look
            .as_ref()
            .map(|l| l.font_family.clone())
            .or_else(|| st.and_then(|s| s.run.font_ascii.as_deref().map(str::to_owned)))
            .or_else(|| dd.and_then(|d| d.run.font_ascii.as_deref().map(str::to_owned)))
            .unwrap_or_else(|| DEFAULT_FONT.to_owned());
        run.font_ascii = Some(f.into_boxed_str());
    }
    if run.sz_half_pt.is_none() {
        let half = |x: f64| uo_jsnum::math::round(x * 2.0);
        let v = match (&look, st) {
            (Some(l), _) => half(l.size_pt),
            // `st.sizePt`: the style's `szHalfPt / 2`, 11 without one
            (None, Some(s)) => half(s.run.sz_half_pt.and_then(uo_docx::js::JsNumber::as_f64).map_or(11.0, |h| h / 2.0)),
            (None, None) => dd.and_then(|d| d.run.sz_half_pt.and_then(uo_docx::js::JsNumber::as_f64)).unwrap_or(22.0),
        };
        run.sz_half_pt = Some(uo_docx::js::JsNumber::Num(v));
    }
    (run, para, has_pending)
}

fn encode_state(run: &RunProps, para: &uo_docx::model::para::ParaProps, pending: bool) -> Result<Vec<u8>, String> {
    let wr: w::RunProps = run.into_wire().map_err(|e| e.to_string())?;
    let wp: w::ParaProps = para.into_wire().map_err(|e| e.to_string())?;
    let mut rb = Vec::new();
    {
        let mut wtr = uo_codec::Writer::new(&mut rb);
        wr.encode(&mut wtr);
        wtr.finish().map_err(|e| format!("encode: {e:?}"))?;
    }
    let mut pb = Vec::new();
    {
        let mut wtr = uo_codec::Writer::new(&mut pb);
        wp.encode(&mut wtr);
        wtr.finish().map_err(|e| format!("encode: {e:?}"))?;
    }
    let mut out = Vec::with_capacity(5 + rb.len() + pb.len());
    out.extend_from_slice(&u32::try_from(rb.len()).map_err(|_| "run props too large".to_owned())?.to_le_bytes());
    out.extend_from_slice(&rb);
    out.extend_from_slice(&pb);
    out.push(u8::from(pending));
    Ok(out)
}

/// The toolbar state at the selection (see the module comment): u32 byte length of the run props,
/// the run props (the generated wire `RunProps` struct, no message header), the paragraph props (wire
/// `ParaProps`), then u8 flags (bit 0: the run props are the pending ones). Status 5 when the props
/// have no wire form (the detail says why).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_shell_selection_state(h: *const UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let (run, para, pending) = selection_state(s);
    match encode_state(&run, &para, pending) {
        Ok(out) => buf(OK, out),
        Err(d) => buf(BAD_ARGUMENT, d.into_bytes()),
    }
}
