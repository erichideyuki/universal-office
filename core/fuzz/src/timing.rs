//! Fuzz targets of Slides timing and animation (`uo-pptx` frontier B6; record `core/docs/modules/uo-pptx.md`
//! "B6 — timing and playback").
//!
//! - `slide_timing`: structure-aware: byte 0 selects the limits and a wrapping, the rest (UTF-8, lossy) is the
//!   CONTENT of a `<p:sld>`: `<p:transition>`, `mc:AlternateContent`, a `<p:timing>` tree of any depth and width,
//!   hostile `repeatCount`/`dur`/`spd`/`tmFilter`/`<p:iterate>` values, formulas, motion paths. The model is parsed
//!   with small limits, printed twice (determinism), then run through the four playback timelines, the states, a
//!   slideshow driven by the input bytes, the hooks of every effect and the slot table.
//! - `anim_formula`: the input (UTF-8, lossy) is an animation formula: it must give NaN or a finite number, the
//!   same through the capped cache, a cache of 3 entries and a fresh one.
//!
//! Invariants beyond "no panic": node and depth caps hold, every `NodeId` resolves, a timing text read twice is the
//! same canonical text, a formula returns an infinity only as a numeric literal, and the caches stay under their caps.
use std::path::Path;
use std::sync::Arc;

use uo_dml_render::CssColor;
use uo_dml_render::source::PartSource;
use uo_opc::xml::{CharRefPolicy, ContentPi, Element, ParseOptions, parse};
use uo_pptx::anim::{AnimBase, UnitRef};
use uo_pptx::anim_engine::{AnimCaches, ShapeAnimContext, shape_anim_hooks};
use uo_pptx::anim_formula::{FormulaCache, FormulaVars};
use uo_pptx::canon::timing_json;
use uo_pptx::media::MediaClock;
use uo_pptx::playback::{PlaybackOptions, build_playback_timeline, playback_state_at, shape_opacity_at};
use uo_pptx::show::{SlideShow, SlideShowOptions};
use uo_pptx::slots::{SlotKind, SlotShape, SlotSpec, SlotTemplate, evaluate_slots};
use uo_pptx::timing::{AnimTarget, TimingModel};
use uo_pptx::timing_parse::{TimingInputs, parse_slide_timing};
use uo_pptx::{Deck, Limits};

const NS: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\"";

fn units(t: Option<&AnimTarget>, kind: &str) -> f64 {
    // a count derived from the target, so range targets change it
    let base = if kind == "el" { 1.0 } else { 5.0 };
    base + t.map_or(0.0, |t| t.spid.len() as f64)
}

fn model_of(sld: &str, limits: &Limits) -> Option<TimingModel> {
    let opts = ParseOptions { content_pi: ContentPi::Skip, bad_char_ref: CharRefPolicy::Replace, ..ParseOptions::LENIENT };
    let doc = parse(sld, &opts).ok()?;
    let color = |_: Element<'_>| Some(CssColor::new("#336699"));
    let hb = |spid: &str| spid.len() % 2 == 0;
    let inp = TimingInputs { units: &units, color: Some(&color), has_background: Some(&hb) };
    Some(parse_slide_timing(doc.root(), &inp, limits))
}

/// `slide_timing`: the content of a `<p:sld>` read as a time model and played.
pub fn slide_timing(data: &[u8]) {
    let Some((&sel, rest)) = data.split_first() else { return };
    let limits = Limits {
        max_time_depth: [4, 16, 64, 256][usize::from(sel & 3)],
        max_time_nodes: [64, 1_000, 20_000, 100_000][usize::from((sel >> 2) & 3)],
        ..Limits::default()
    };
    let content = String::from_utf8_lossy(rest);
    let wrap = (sel >> 4) & 3;
    let sld = match wrap {
        0 => format!("<p:sld {NS}>{content}</p:sld>"),
        1 => format!(
            "<p:sld {NS} show=\"0\"><mc:AlternateContent><mc:Choice Requires=\"p14\">{content}</mc:Choice></mc:AlternateContent></p:sld>"
        ),
        _ => format!(
            "<p:sld {NS}><p:timing><p:tnLst><p:par><p:cTn id=\"1\"><p:childTnLst>{content}</p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>"
        ),
    };
    let Some(m) = model_of(&sld, &limits) else { return };
    assert!(m.nodes.nodes.len() <= limits.max_time_nodes as usize, "node cap");
    // every id resolves, the children spans are inside the arena
    for e in m
        .steps
        .iter()
        .flat_map(|s| &s.effects)
        .chain(m.interactive.iter())
        .chain(m.bookmark_triggers.iter().flatten().flat_map(|b| &b.effects))
    {
        assert!((e.node.0 as usize) < m.nodes.nodes.len(), "dangling node id");
    }
    for n in &m.nodes.nodes {
        assert!((n.children.start as usize) + (n.children.len as usize) <= m.nodes.children.len(), "children span");
    }
    // the same input gives the same canonical text
    let a = timing_json(&m).to_text();
    let b = timing_json(&model_of(&sld, &limits).unwrap_or_default()).to_text();
    assert_eq!(a, b, "timing read twice");
    // playback
    let m = Arc::new(m);
    let models = vec![Some(m.clone()), None, Some(m.clone())];
    for opts in [
        PlaybackOptions::default(),
        PlaybackOptions { tick_ms: Some(33.0), transition_fps: Some(30.0), ..Default::default() },
        PlaybackOptions { use_timings: Some(false), default_slide_ms: Some(1e300), ..Default::default() },
        PlaybackOptions { tick_ms: Some(f64::NAN), transition_fps: Some(f64::INFINITY), ..Default::default() },
    ] {
        let tl = build_playback_timeline(&models, &opts);
        let mut caches = AnimCaches::new();
        for t in [f64::NAN, -1.0, 0.0, 1234.5, tl.duration_ms, f64::INFINITY] {
            let st = playback_state_at(&tl, t);
            let Some(p) = st.pos else { continue };
            let ps = &tl.slides[p];
            for ms in [st.anim_ms, 0.0, 777.0] {
                let _ = shape_opacity_at(&ps.effects, &["4"], ms);
                let spids: Vec<&str> = ps.effects.iter().filter_map(|e| e.fx.target.as_ref().map(|t| &*t.spid)).take(4).collect();
                for sp in &spids {
                    let ctx = ShapeAnimContext::default();
                    if let Some(h) = shape_anim_hooks(&ps.effects, &ps.model.nodes, &[sp], ms, &ctx) {
                        let base = AnimBase { x: 0.5, y: 0.5, w: 0.2, h: 0.1, rot: Some(10.0), fill: None, line: None, text: None };
                        let _ = h.all(&base, &mut caches);
                        let _ = h.bg(&base, &mut caches);
                        let _ = h.para(0, &base, &mut caches);
                        let u = UnitRef { para: 0, k: 1, count: 3, k_para: 1, count_para: 3 };
                        let _ = h.unit_state(u, &base, None, &mut caches);
                        for j in 0..h.chart_parts().len() {
                            let _ = h.chart_state(j, &base, &mut caches);
                        }
                    }
                }
                if !spids.is_empty() {
                    let mut tpl = SlotTemplate::default();
                    tpl.shapes.push(SlotShape {
                        ids: vec![spids[0].into()],
                        dgm_id: None,
                        has_media: true,
                        inherited: false,
                        has_xfrm: true,
                    });
                    let base = AnimBase { x: 0.5, y: 0.5, w: 0.2, h: 0.1, rot: None, fill: None, line: None, text: None };
                    for kind in [SlotKind::Shape, SlotKind::Background, SlotKind::Paragraph(0), SlotKind::ChartPart(0), SlotKind::Media] {
                        tpl.slots.push(SlotSpec { kind, shape: 0, base: base.clone(), word: None });
                    }
                    let _ = evaluate_slots(&tpl, ps, ms, &MediaClock::default(), &mut caches);
                }
            }
        }
    }
    // a slide show driven by the input bytes
    let mut ss = SlideShow::new(models, f64::from(sel), 0.0, SlideShowOptions { loop_: Some(sel & 0x40 != 0), ..Default::default() });
    let mut now = 0.0;
    for (i, b) in rest.iter().take(64).enumerate() {
        now += f64::from(*b) * 37.0;
        match b % 8 {
            0 | 1 => ss.next(now, i % 2 == 0),
            2 => ss.prev(now),
            3 => ss.go_to(i64::from(*b % 5) - 1, now),
            4 => ss.first(now),
            5 => ss.last(now),
            _ => ss.update(now),
        };
        let _ = ss.frame(now);
        let _ = ss.wake_at(now);
        let _ = ss.due_at();
    }
}

/// `anim_formula`: one formula text.
pub fn anim_formula(data: &[u8]) {
    let src = String::from_utf8_lossy(data);
    let vars = [
        FormulaVars { x: 0.5, y: 0.25, w: 0.2, h: 0.1, r: 15.0, dollar: Some(0.5) },
        FormulaVars::default(),
        FormulaVars { x: f64::NAN, y: f64::INFINITY, w: -1e308, h: 1e-320, r: 0.0, dollar: None },
    ];
    let mut big = FormulaCache::new();
    let mut tiny = FormulaCache::with_cap(3);
    for v in &vars {
        let a = big.eval(&src, v);
        // a purely numeric text is the number itself, `1e999` included (the TS returns the `parseFloat` as is)
        let numeric = src.trim().bytes().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'e' | b'E'));
        assert!(a.is_nan() || a.is_finite() || numeric, "infinity from a formula: {src:?}");
        let b = tiny.eval(&src, v);
        let c = FormulaCache::with_cap(1).eval(&src, v);
        let again = big.eval(&src, v);
        let same = |x: f64, y: f64| (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits();
        assert!(same(a, b) && same(a, c) && same(a, again), "cache changed a result: {src:?}");
        assert!(tiny.len() <= 3 && big.len() <= 1024);
    }
}

/// Seeds: the `<p:sld>` content of generated B6 decks (timing, transition) and a spread of formulas.
pub fn timing_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let dir = repo.join("core/crates/uo-pptx/tests/captures/decks/b6a");
    for n in ["0000", "0003", "0007", "0012", "0021", "0034"] {
        let Ok(bytes) = std::fs::read(dir.join(format!("{n}.pptx"))) else { continue };
        let Ok(mut deck) = Deck::open(&bytes, Limits::default()) else { continue };
        let Ok(count) = deck.presentation().map(|p| p.slides.len()) else { continue };
        for i in 1..=count {
            let src = deck.source();
            let Some(text) = src.text(&format!("ppt/slides/slide{i}.xml")) else { continue };
            let (Some(open_end), Some(close)) = (text.find("<p:cSld>"), text.rfind("</p:sld>")) else { continue };
            // everything after the shape tree: clrMapOvr, transition, timing
            let after = text[open_end..close].find("</p:cSld>").map_or("", |k| &text[open_end + k + "</p:cSld>".len()..close]);
            let mut v = vec![[0x03u8, 0x0f, 0x2f, 0x3f][i % 4]];
            v.extend_from_slice(after.as_bytes());
            out.push(("slide_timing", v));
        }
    }
    for f in [
        "1+#ppt_h/2",
        "#ppt_x+0.4*cos(pi*(1-$))",
        "0-#ppt_w/2",
        "max(1,2,min(3,$))^2",
        "sqrt(-4)+ln(0)",
        "(((1)))",
        "1/0",
        "rand(5)*floor(2.7)",
        "-(-(-1))",
        "deg(rad(180))",
        "$*$",
        "abc(1)",
    ] {
        out.push(("anim_formula", f.as_bytes().to_vec()));
    }
    out
}
