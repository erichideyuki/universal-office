//! Fuzz target `smartart` (frontier B5a, `core/docs/modules/uo-dml-render.md` "B5a — SmartArt").
//!
//! The input is split at NUL bytes into the data model, the layout definition, the quick style and the
//! colours (lossy UTF-8, each parsed with the Slides XML preset; a part that does not parse is absent).
//! Byte 0 and 1 choose the frame size. The engine (`regenerate_diagram`) and the fallback
//! (`diagram_fallback_shapes`, with the layout's `uniqueId`) run over a default theme: no panic, no hang
//! (the engine's own bounds `MAX_PRES`/`MAX_ITER`/scale budget), and the same input gives the same output.
use uo_dml_render::model::Xfrm;
use uo_dml_render::smartart::xt::{X, from_element};
use uo_dml_render::smartart::{diagram_fallback_shapes, diagram_styler, regenerate_diagram};
use uo_dml_render::source::parse_xml;
use uo_dml_render::theme::Theme;

fn part(b: Option<&[u8]>) -> Option<X> {
    let text = String::from_utf8_lossy(b?).into_owned();
    let doc = parse_xml(text).ok()?;
    from_element(doc.root())
}

/// `smartart`: regenerate and fall back over four fuzzed parts.
pub fn smartart(data: &[u8]) {
    if data.len() > 200_000 {
        return;
    }
    let (head, rest) = data.split_at(data.len().min(2));
    let cx = f64::from(head.first().copied().unwrap_or(1)) * 40_000.0;
    let cy = f64::from(head.get(1).copied().unwrap_or(1)) * 30_000.0;
    let mut it = rest.split(|&b| b == 0);
    let d = part(it.next());
    let l = part(it.next());
    let q = part(it.next());
    let c = part(it.next());
    let theme = Theme::default();
    let a = regenerate_diagram(d.as_ref(), l.as_ref(), q.as_ref(), c.as_ref(), cx, cy, &theme);
    let b = regenerate_diagram(d.as_ref(), l.as_ref(), q.as_ref(), c.as_ref(), cx, cy, &theme);
    assert_eq!(a, b, "regeneration is deterministic");
    if let Some(d) = &d {
        let styler = diagram_styler(c.as_ref(), q.as_ref(), &theme);
        let uid = l.as_ref().and_then(|l| l.attr("uniqueId")).unwrap_or("");
        let frame = Xfrm { x: 0.0, y: 0.0, cx, cy, ..Xfrm::default() };
        let _ = diagram_fallback_shapes(d, uid, &frame, &theme, Some(&styler));
    }
}
