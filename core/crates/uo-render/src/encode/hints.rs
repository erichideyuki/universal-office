//! The canvas hints of §3.3: what the TS canvas painter (`canvas.ts`) would have asked a font or
//! a geometry function for, computed here in the canvas units and the canvas arithmetic, because
//! the shell has neither after the switch. Each function names the lines of `canvas.ts` it
//! mirrors; the parity facets (`tests/e_*.rs`) replay the frozen `paintItems` and compare.

use crate::codec as w;
use crate::geom::{LineUnit, ShapePart as RustPart, path_bounds, shape_parts};
use crate::list::{ItemList, NO_GID};
use crate::opt::OptF64;
use crate::paint::fill::{GradBox, gradient_geometry, gradient_samples, is_rect_gradient, stops_of};
use crate::paint::text_fx::{RunView, grad_of, run_box};
use crate::paint::{GradGeom, PaintLimits};
use crate::resources::Resources;
use crate::types::{GeomEval, Item, PathCmd, ResolvedFill, ShapeItem};
use uo_fonts::{Face, OutlineCmd};
use uo_jsnum::math::abs;

/// `/^[0-9A-Fa-f]{6}$/` over UTF-16 units.
#[must_use]
pub fn is_hex6(s: &[u16]) -> bool {
    s.len() == 6 && s.iter().all(|&c| u8::try_from(c).is_ok_and(|b| b.is_ascii_hexdigit()))
}

/// `batchable(cp)` (canvas.ts:392): scripts without contextual composition, whose advance is the
/// font's, so consecutive glyphs can go out in one `fillText`.
#[must_use]
pub fn batchable(cp: u32) -> bool {
    (0x20..0x0300).contains(&cp)
        || (0x0370..0x0590).contains(&cp)
        || ((0x2000..0x2070).contains(&cp) && cp != 0x200c && cp != 0x200d)
        || (0x20a0..0x20d0).contains(&cp)
}

/// The glyph hints of one run.
#[derive(Default, Debug)]
pub struct GlyphHints {
    pub joins: Option<Vec<bool>>,
    pub remap: Option<Vec<w::CpRemap>>,
    pub outlines: Option<Vec<w::GlyphOutline>>,
}

fn wire_outline(cmds: &[OutlineCmd]) -> Vec<w::OutlineCmd> {
    cmds.iter()
        .map(|c| match *c {
            OutlineCmd::M { x, y } => w::OutlineCmd::M(w::Pt { x, y }),
            OutlineCmd::L { x, y } => w::OutlineCmd::L(w::Pt { x, y }),
            OutlineCmd::Q { cx, cy, x, y } => w::OutlineCmd::Q(w::Quad { x1: cx, y1: cy, x, y }),
            OutlineCmd::Z => w::OutlineCmd::Z,
        })
        .collect()
}

/// The hints of `paintGlyphs` (canvas.ts:315-385) for a run: the gid outlines (357-371), the F0xx
/// remap (375) and the batch joins (350-385), the loop simulated exactly as the TS runs it with
/// `'fontKerning' in ctx` true:
///
/// - `outlines`: every glyph with a `gid` whose face answers an outline (font units, y up). Empty
///   outlines are kept (`if (ol)` is truthy for an empty array);
/// - `remap`: where `cp` is F000-F0FF, the face has no glyph for it and has one for `cp - 0xF000`
///   (the drawn code point), for the glyphs that go through `fillText`;
/// - `joins`: a bit per glyph, true for a glyph the TS appends to the batch begun at an earlier
///   glyph (ZWNJ between them). Only for a *plain* run (`scaleX === 1`, no synthetic italic);
///   `None` when no glyph joins. The painter uses it only when the context has `fontKerning`;
///   without it every glyph is drawn on its own (so `remap` and `outlines` do not depend on it).
#[must_use]
pub fn glyph_hints(
    face: &Face,
    size_pt: f64,
    scale_x: OptF64,
    synth_italic: bool,
    list: &ItemList,
    glyphs: crate::list::Span,
) -> GlyphHints {
    let Some(r) = glyphs.range() else { return GlyphHints::default() };
    let (Some(cps), Some(xs)) = (list.glyphs.cp.get(r.clone()), list.glyphs.x.get(r.clone())) else { return GlyphHints::default() };
    let gids: &[u32] = if list.glyphs.gid.is_empty() { &[] } else { list.glyphs.gid.get(r).unwrap_or(&[]) };
    let gid_of = |i: usize| gids.get(i).copied().filter(|&g| g != NO_GID);
    let n = cps.len();
    let upem = {
        let u = face.metrics().units_per_em;
        if u == 0 { 1000.0 } else { f64::from(u) }
    };
    let plain = scale_x.or(1.0) == 1.0 && !synth_italic;
    // `adv(cp)` (canvas.ts:337)
    let adv = |cp: u32| (f64::from(face.advance_width(face.glyph_id(cp))) * size_pt * 20.0) / upem;

    let mut outlines: Vec<w::GlyphOutline> = Vec::new();
    let mut outline_at = vec![false; n];
    for i in 0..n {
        if let Some(g) = gid_of(i)
            && let Some(ol) = face.glyph_outline(g)
            && let (Some(slot), Ok(index)) = (outline_at.get_mut(i), u32::try_from(i))
        {
            *slot = true;
            outlines.push(w::GlyphOutline { index, cmds: wire_outline(&ol) });
        }
    }

    // the F0xx remap of every glyph that goes through `fillText` (canvas.ts:375)
    let mut remap: Vec<w::CpRemap> = Vec::new();
    for (k, &cp) in cps.iter().enumerate() {
        if outline_at.get(k).copied().unwrap_or(false) {
            continue;
        }
        if (0xf000..=0xf0ff).contains(&cp)
            && face.glyph_id(cp) == 0
            && face.glyph_id(cp - 0xf000) != 0
            && let Ok(index) = u32::try_from(k)
        {
            remap.push(w::CpRemap { index, cp: cp - 0xf000 });
        }
    }

    // the batches (canvas.ts:350-385): outline glyphs `continue` before the batch, the start of a
    // batch is tested after the remap, a follower is tested with its raw code point
    let mut joins = vec![false; n];
    let mut any_join = false;
    let mut i = 0usize;
    let mut ri = 0usize;
    while i < n {
        let Some(&cp0) = cps.get(i) else { break };
        if outline_at.get(i).copied().unwrap_or(false) {
            i += 1;
            continue;
        }
        // `remap` is sorted by index: the cursor only moves forward
        while remap.get(ri).is_some_and(|m| usize::try_from(m.index).is_ok_and(|k| k < i)) {
            ri += 1;
        }
        let cp = remap.get(ri).filter(|m| usize::try_from(m.index).ok() == Some(i)).map_or(cp0, |m| m.cp);
        if plain && batchable(cp) {
            while i + 1 < n {
                let (Some(&ncp), Some(&nx), Some(&pcp), Some(&px)) = (cps.get(i + 1), xs.get(i + 1), cps.get(i), xs.get(i)) else { break };
                // `!batchable(n.cp) || Math.abs(n.x - (prev.x + adv(prev.cp))) > 0.25` (NaN does not break)
                if !batchable(ncp) || abs(nx - (px + adv(pcp))) > 0.25 {
                    break;
                }
                i += 1;
                if let Some(j) = joins.get_mut(i) {
                    *j = true;
                    any_join = true;
                }
            }
        }
        i += 1;
    }
    GlyphHints {
        joins: if plain && any_join { Some(joins) } else { None },
        remap: if remap.is_empty() { None } else { Some(remap) },
        outlines: if outlines.is_empty() { None } else { Some(outlines) },
    }
}

fn wire_geom(g: GradGeom) -> w::GradGeom {
    match g {
        GradGeom::Axial { x0, y0, x1, y1 } => w::GradGeom::Axial(w::AxialGeom { x0, y0, x1, y1 }),
        GradGeom::Radial { x0, y0, r } => w::GradGeom::Radial(w::RadialGeom { x0, y0, r }),
    }
}

/// The box and gradient geometry of an fx run (canvas.ts:289-300): `runBox(it)` and, for a text
/// fill / outline that is a gradient, `gradientGeometry(gradOf(f), box, box corners)`.
pub struct FxHints {
    pub r#box: Option<w::Box>,
    pub fill_geom: Option<w::GradGeom>,
    pub outline_geom: Option<w::GradGeom>,
}

#[must_use]
pub fn fx_hints(list: &ItemList, item: &Item, res: &Resources) -> FxHints {
    let none = FxHints { r#box: None, fill_geom: None, outline_geom: None };
    let Item::Glyphs { run, .. } = item else { return none };
    let Some(side) = list.run(*run) else { return none };
    let Some(fx) = side.fx.and_then(|id| res.text_fx(id)) else { return none };
    let Some(view) = RunView::of(list, item, res) else { return none };
    let Some(b) = run_box(&view) else { return none };
    let geom_of = |f: &ResolvedFill| -> Option<w::GradGeom> {
        let g = grad_of(f)?;
        let frame = GradBox { x: b.x, y: b.y, w: b.w, h: b.h, rot: 0.0 };
        Some(wire_geom(gradient_geometry(&g, &res.symbols, frame, (b.x, b.y, b.x + b.w, b.y + b.h))))
    };
    FxHints {
        r#box: Some(w::Box { x: b.x, y: b.y, w: b.w, h: b.h }),
        fill_geom: fx.fill.as_ref().and_then(geom_of),
        outline_geom: fx.outline.as_ref().and_then(|o| geom_of(&o.fill)),
    }
}

fn wire_cmds(cmds: &[PathCmd]) -> Vec<w::PathCmd> {
    cmds.iter()
        .map(|c| match *c {
            PathCmd::M { x, y } => w::PathCmd::M(w::Pt { x, y }),
            PathCmd::L { x, y } => w::PathCmd::L(w::Pt { x, y }),
            PathCmd::C { x1, y1, x2, y2, x, y } => w::PathCmd::C(w::Cubic { x1, y1, x2, y2, x, y }),
            PathCmd::Z => w::PathCmd::Z,
        })
        .collect()
}

/// Whether `paintShape` takes the effects path (canvas.ts:428): `it.fx && it.style.geom !== 'line'`.
#[must_use]
pub fn shape_has_effects(sh: &ShapeItem, res: &Resources) -> bool {
    sh.fx.is_some() && !res.sym(sh.geom.geom).iter().copied().eq("line".encode_utf16())
}

/// The shape hints of `paintShape` (canvas.ts:427-510): the parts in twips and, for a gradient
/// that is not the rectangular one, the geometry and the 64 samples. A shape with effects is
/// painted by the canvas through a local item (`x = y = 0`, `rot = 0`, no `fx`/`shadow`,
/// canvas.ts:430), so its hints are those of that item. Nothing is computed for a shape the
/// canvas returns early on (no valid fill, stroke, gradient, pattern or blip) unless it has
/// effects.
#[derive(Default)]
pub struct ShapeHints {
    pub parts: Vec<w::ShapePart>,
    pub grad_geom: Option<w::GradGeom>,
    pub grad_samples: Option<Vec<u8>>,
}

#[must_use]
pub fn shape_hints(sh: &ShapeItem, res: &Resources, lim: &PaintLimits) -> ShapeHints {
    let effects = shape_has_effects(sh, res);
    let fill_ok = sh.fill.is_some_and(|f| is_hex6(res.sym(f)));
    let stroke_ok = sh.stroke.is_some_and(|s| is_hex6(res.sym(s.color)));
    let painted = fill_ok || stroke_ok || sh.grad.is_some() || sh.pattern.is_some() || sh.blip.is_some();
    if !painted && !effects {
        return ShapeHints::default();
    }
    if sh.geom.eval == GeomEval::Drawingml {
        // the Slides evaluator is uo-drawingml's, run by the painter that needs it
        return ShapeHints::default();
    }
    let (x, y, w_, h) = if effects { (0.0, 0.0, sh.w, sh.h) } else { (sh.x, sh.y, sh.w, sh.h) };
    let mut geom = sh.geom.clone();
    if effects {
        geom.rot = OptF64::some(0.0);
    }
    let parts: Vec<RustPart> = shape_parts(LineUnit::CanvasTwips, &geom, res, [x, y, w_, h], sh.line);
    let mut out = ShapeHints::default();
    if sh.pattern.is_none()
        && sh.blip.is_none()
        && let Some(g) = &sh.grad
        && !is_rect_gradient(Some(g), &res.symbols)
    {
        let all: Vec<PathCmd> = parts.iter().flat_map(|p| p.cmds.iter().copied()).collect();
        let pb = path_bounds(&all);
        let frame = GradBox { x, y, w: w_, h, rot: geom.rot.or(0.0) };
        out.grad_geom = Some(wire_geom(gradient_geometry(g, &res.symbols, frame, pb)));
        out.grad_samples = gradient_samples(&stops_of(g, &res.symbols), 64, lim).ok();
    }
    out.parts =
        parts.iter().map(|p| w::ShapePart { cmds: wire_cmds(&p.cmds), fill: p.fill, stroke: p.stroke, even_odd: p.even_odd }).collect();
    out
}
