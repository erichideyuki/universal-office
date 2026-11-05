//! A transcription of the parts of `canvas.ts` that read the encoder's hints: `paintGlyphs`
//! (lines 315-385) and `paintShape` (427-510) over the DECODED message (the generated wire types),
//! logging the context calls in the form `tools/capture-e.mjs` records. It is the contract the TS
//! shell implements (frontier D): when this log equals the frozen painter's, the hints are enough.

use super::{Arg, Op, n, op, st};
use uo_jsnum::math::max;
use uo_render::codec as w;
use uo_render::codec::Tables;
use uo_render::encode::hints::{batchable, is_hex6};

fn cps(s: &[u16]) -> Vec<u32> {
    char::decode_utf16(s.iter().copied()).map(|c| c.map_or(0xfffd, u32::from)).collect()
}

/// `paintItems` of one item: `save`, the item, `restore` (depth 0).
pub fn paint_items(item: &w::Item, tables: &Tables, kerning: bool, as_local: bool) -> Vec<Op> {
    let mut log = vec![op("save", vec![])];
    match item {
        w::Item::Glyphs(g) => {
            let upem = tables.faces.get(g.face as usize).map_or(1000.0, |f| f64::from(f.upem));
            paint_glyphs(&mut log, g, tables, upem, kerning);
        }
        w::Item::Shape(s) => paint_shape(&mut log, s, tables, as_local),
        _ => {}
    }
    log.push(op("restore", vec![]));
    log
}

fn paint_glyphs(log: &mut Vec<Op>, g: &w::GlyphRun, tables: &Tables, upem: f64, kerning: bool) {
    let count = g.glyphs.len();
    if count == 0 || !is_hex6(tables.sym_units(g.color)) {
        return;
    }
    let sx = g.scale_x.unwrap_or(1.0);
    let skew = if g.synth_italic { -0.21 } else { 0.0 };
    let plain = sx == 1.0 && skew == 0.0;
    let batch = plain && kerning;
    let mut i = 0usize;
    while i < count {
        let gx = g.glyphs.x.get(i).copied().unwrap_or(0.0);
        let outline = g.outlines.as_ref().and_then(|o| o.iter().find(|o| o.index as usize == i));
        if let Some(ol) = outline {
            let s = (g.size_pt * 20.0) / upem;
            let gxs = s * sx;
            let k = if g.synth_italic { 0.21 } else { 0.0 };
            log.push(op("beginPath", vec![]));
            for c in &ol.cmds {
                match c {
                    w::OutlineCmd::M(p) => log.push(op("moveTo", vec![n(gx + p.x * gxs + k * p.y * s), n(g.y - p.y * s)])),
                    w::OutlineCmd::L(p) => log.push(op("lineTo", vec![n(gx + p.x * gxs + k * p.y * s), n(g.y - p.y * s)])),
                    w::OutlineCmd::Q(q) => log.push(op(
                        "quadraticCurveTo",
                        vec![n(gx + q.x1 * gxs + k * q.y1 * s), n(g.y - q.y1 * s), n(gx + q.x * gxs + k * q.y * s), n(g.y - q.y * s)],
                    )),
                    _ => log.push(op("closePath", vec![])),
                }
            }
            log.push(op("fill", vec![]));
            i += 1;
            continue;
        }
        let mut cp = g.glyphs.cp.get(i).copied().unwrap_or(0);
        if let Some(m) = g.remap.as_ref().and_then(|r| r.iter().find(|m| m.index as usize == i)) {
            cp = m.cp;
        }
        let mut ch = vec![cp];
        if batch && batchable(cp) {
            while g.joins.as_ref().is_some_and(|j| j.get(i + 1).copied().unwrap_or(false)) {
                ch.push(0x200c);
                ch.push(g.glyphs.cp.get(i + 1).copied().unwrap_or(0));
                i += 1;
            }
        }
        let chs = Arg::S(ch);
        if plain {
            log.push(op("fillText", vec![chs.clone(), n(gx), n(g.y)]));
            if g.synth_bold {
                log.push(op("strokeText", vec![chs, n(gx), n(g.y)]));
            }
        } else {
            log.push(op("save", vec![]));
            log.push(op("translate", vec![n(gx), n(g.y)]));
            log.push(op("transform", vec![n(sx), n(0.0), n(skew), n(1.0), n(0.0), n(0.0)]));
            log.push(op("fillText", vec![chs.clone(), n(0.0), n(0.0)]));
            if g.synth_bold {
                log.push(op("strokeText", vec![chs, n(0.0), n(0.0)]));
            }
            log.push(op("restore", vec![]));
        }
        i += 1;
    }
}

fn trace(log: &mut Vec<Op>, cmds: &[w::PathCmd]) {
    for c in cmds {
        match c {
            w::PathCmd::M(p) => log.push(op("moveTo", vec![n(p.x), n(p.y)])),
            w::PathCmd::L(p) => log.push(op("lineTo", vec![n(p.x), n(p.y)])),
            w::PathCmd::C(c) => log.push(op("bezierCurveTo", vec![n(c.x1), n(c.y1), n(c.x2), n(c.y2), n(c.x), n(c.y)])),
            w::PathCmd::Z => log.push(op("closePath", vec![])),
        }
    }
}

fn stroke_with(log: &mut Vec<Op>, width_pt: f64, dash: bool) {
    let wd = max(0.25, width_pt);
    let pattern = if dash { Arg::A(vec![n(wd * 3.0 * 20.0), n(wd * 2.0 * 20.0)]) } else { Arg::A(vec![]) };
    log.push(op("setLineDash", vec![pattern]));
    log.push(op("stroke", vec![]));
    log.push(op("setLineDash", vec![Arg::A(vec![])]));
}

fn paint_shape(log: &mut Vec<Op>, it: &w::ShapeItem, tables: &Tables, as_local: bool) {
    let fill = it.fill.is_some_and(|f| is_hex6(tables.sym_units(f)));
    let stroke = it.stroke.as_ref().filter(|s| is_hex6(tables.sym_units(s.color)));
    if !fill && stroke.is_none() && it.grad.is_none() && it.pattern.is_none() && it.blip.is_none() {
        return;
    }
    let fill_part = |log: &mut Vec<Op>, p: &w::ShapePart| {
        log.push(op("beginPath", vec![]));
        trace(log, &p.cmds);
        log.push(op("fill", vec![st(if p.even_odd { "evenodd" } else { "nonzero" })]));
    };
    let stroke_part = |log: &mut Vec<Op>, p: &w::ShapePart, width_pt: f64, dash: bool| {
        log.push(op("beginPath", vec![]));
        trace(log, &p.cmds);
        stroke_with(log, width_pt, dash);
    };
    let dash_of = |s: &w::Stroke| s.dash.is_some_and(|d| !tables.sym_units(d).is_empty());
    // the local item of the effects path has no shadow (canvas.ts:430)
    if let Some(sh) = it.shadow.as_ref().filter(|s| !as_local && is_hex6(tables.sym_units(s.color))) {
        log.push(op("save", vec![]));
        log.push(op("translate", vec![n(sh.dx), n(sh.dy)]));
        for p in &it.parts {
            if p.fill && (fill || it.grad.is_some() || it.pattern.is_some() || it.blip.is_some()) {
                fill_part(log, p);
            }
            if p.stroke
                && let Some(s) = stroke
            {
                stroke_part(log, p, s.width_pt, dash_of(s));
            }
        }
        log.push(op("restore", vec![]));
    }
    if it.pattern.is_some() || it.blip.is_some() {
        // the raster branches are the wasm paint exports (tested in uo-wasm)
    } else if let Some(geom) = &it.grad_geom {
        let create = match geom {
            w::GradGeom::Axial(a) => op("createLinearGradient", vec![n(a.x0), n(a.y0), n(a.x1), n(a.y1)]),
            w::GradGeom::Radial(r) => op("createRadialGradient", vec![n(r.x0), n(r.y0), n(0.0), n(r.x0), n(r.y0), n(r.r)]),
        };
        log.push(create);
        if let Some(smp) = &it.grad_samples {
            for i in 0..64usize {
                let b = |k: usize| smp.get(i * 3 + k).copied().unwrap_or(0);
                log.push(op("addColorStop", vec![n(i as f64 / 63.0), st(&format!("#{:02X}{:02X}{:02X}", b(0), b(1), b(2)))]));
            }
        }
        for p in &it.parts {
            if p.fill {
                fill_part(log, p);
            }
        }
    } else if it.grad.is_none() && fill {
        for p in &it.parts {
            if p.fill {
                fill_part(log, p);
            }
        }
    }
    if let Some(s) = stroke {
        for p in &it.parts {
            if p.stroke {
                stroke_part(log, p, s.width_pt, dash_of(s));
            }
        }
    }
}

/// Counts for the corpus coverage asserts.
pub fn count(log: &[Op], name: &str) -> usize {
    log.iter().filter(|o| o.name == name).count()
}

pub fn units(s: &[u16]) -> Vec<u32> {
    cps(s)
}
