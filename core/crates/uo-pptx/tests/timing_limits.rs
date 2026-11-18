//! B6 gates: memory (M1 live bytes return to the start, M2 slideshow soak, M5 caps exercised past their size)
//! and security (hostile timing trees, NaN durations, huge repeat counts, formula and path bombs: bounded, no
//! panic). Everything is seconds-bounded; `UO_PPTX_TIMING_SOAK=<frames>` lengthens the soak.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_dml_render::CssColor;
use uo_opc::xml::{CharRefPolicy, ContentPi, ParseOptions, parse};
use uo_pptx::anim::{AnimBase, UnitRef};
use uo_pptx::anim_engine::{AnimCaches, ShapeAnimContext, shape_anim_hooks};
use uo_pptx::anim_formula::{FormulaCache, FormulaVars, MAX_DEPTH, MAX_FORMULA_LEN, MAX_NODES};
use uo_pptx::media::{MediaClock, media_state_at};
use uo_pptx::playback::{PlaybackOptions, build_playback_timeline, playback_state_at, shape_opacity_at};
use uo_pptx::show::{SlideShow, SlideShowOptions};
use uo_pptx::slots::{SlotKind, SlotShape, SlotSpec, SlotTemplate, evaluate_slots};
use uo_pptx::timing::{AnimTarget, NodeId, TimeArena, TimeCond, TimeNode, TimingModel};
use uo_pptx::timing_parse::{TimingInputs, parse_slide_timing};
use uo_pptx::{Deck, Limits};

fn deck_bytes(group: &str, n: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks").join(group).join(format!("{n}.pptx"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

type Models = Vec<Option<Arc<TimingModel>>>;

fn models_of(bytes: &[u8]) -> Models {
    Deck::open(bytes, Limits::default()).unwrap().timing_models()
}

/// The first b6a deck that has per-unit effects, a motion path and media (the soak's workload).
fn busy_deck() -> (String, Models) {
    for i in 0..100 {
        let n = format!("{i:04}");
        let models = models_of(&deck_bytes("b6a", &n));
        let mut iter = false;
        let mut path = false;
        let mut media = false;
        for m in models.iter().flatten() {
            media |= m.media.as_ref().is_some_and(|x| !x.is_empty());
            for e in m.steps.iter().flat_map(|s| &s.effects) {
                iter |= m.nodes.node(e.node).ctn_x().iterate.is_some();
                path |= e.motion == Some(true);
            }
        }
        if iter && path && media {
            return (n, models);
        }
    }
    panic!("no busy b6a deck");
}

fn xml(text: &str) -> uo_opc::xml::Document {
    let opts = ParseOptions { content_pi: ContentPi::Skip, bad_char_ref: CharRefPolicy::Replace, ..ParseOptions::LENIENT };
    parse(text, &opts).unwrap()
}

fn units(_: Option<&AnimTarget>, kind: &str) -> f64 {
    if kind == "el" { 1.0 } else { 7.0 }
}

fn parse_model(doc: &uo_opc::xml::Document, limits: &Limits) -> TimingModel {
    let color = |_: uo_opc::xml::Element<'_>| Some(CssColor::new("#112233"));
    let hb = |_: &str| true;
    let inp = TimingInputs { units: &units, color: Some(&color), has_background: Some(&hb) };
    parse_slide_timing(doc.root(), &inp, limits)
}

const BASE: AnimBase = AnimBase { x: 0.5, y: 0.5, w: 0.2, h: 0.1, rot: None, fill: None, line: None, text: None };

// ───────────────────────── M1 ─────────────────────────

#[test]
fn m1_timing_operations_return_to_the_start() {
    let (_, models) = busy_deck();
    // the parse (from the XML text): the model and the document drop
    let sld = "<p:sld xmlns:p=\"p\"><p:timing><p:tnLst><p:par><p:cTn id=\"1\"><p:childTnLst><p:seq><p:cTn id=\"2\" nodeType=\"mainSeq\"><p:childTnLst><p:par><p:cTn id=\"3\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst><p:childTnLst><p:par><p:cTn id=\"4\" presetClass=\"entr\" nodeType=\"clickEffect\" presetID=\"1\"><p:childTnLst><p:set><p:cBhvr><p:cTn id=\"5\" dur=\"1\" fill=\"hold\"/><p:tgtEl><p:spTgt spid=\"4\"/></p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val=\"visible\"/></p:to></p:set></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>";
    assert_no_leak(|| {
        let doc = xml(sld);
        parse_model(&doc, &Limits::default())
    });
    // timelines, states, the opacity, the media state
    assert_no_leak(|| {
        for opts in [PlaybackOptions::default(), PlaybackOptions { tick_ms: Some(33.0), transition_fps: Some(30.0), ..Default::default() }]
        {
            let tl = build_playback_timeline(&models, &opts);
            for t in [-1.0, 0.0, 777.0, tl.duration_ms, tl.duration_ms + 5.0] {
                let st = playback_state_at(&tl, t);
                if let Some(p) = st.pos {
                    let s = &tl.slides[p];
                    let _ = shape_opacity_at(&s.effects, &["4"], st.anim_ms);
                    let _ = media_state_at(&s.model.nodes, &s.effects, "4", st.anim_ms, &MediaClock::default(), false);
                }
            }
        }
    });
    // hooks and the slot table, with the caches cleared at the end
    let tl = build_playback_timeline(&models, &PlaybackOptions::default());
    let mut caches = AnimCaches::new();
    for ps in &tl.slides {
        if ps.effects.is_empty() {
            continue;
        }
        let tpl = template_of(ps);
        assert_no_leak(|| {
            for ms in [0.0, 250.0, 1500.0, 9000.0] {
                let t = evaluate_slots(&tpl, ps, ms, &MediaClock::default(), &mut caches);
                assert_eq!(t.values.len(), tpl.slots.len());
            }
            caches.clear();
        });
    }
    // every SlideShow transition
    assert_no_leak(|| {
        let mut ss = SlideShow::new(models.clone(), 0.0, 0.0, SlideShowOptions::default());
        let mut now = 0.0;
        for k in 0..80 {
            now += 700.0;
            match k % 7 {
                0 => ss.next(now, false),
                1 => ss.prev(now),
                2 => ss.go_to(i64::from(k % 3), now),
                3 => ss.first(now),
                4 => ss.last(now),
                _ => ss.update(now),
            };
            let _ = ss.frame(now);
            let _ = ss.wake_at(now);
        }
    });
}

/// A slot template over the spids an effect list targets: a shape slot, background, paragraphs 0-2, units, chart parts.
fn template_of(ps: &uo_pptx::playback::PlaybackSlide) -> SlotTemplate {
    let mut spids: Vec<Box<str>> = Vec::new();
    for e in &ps.effects {
        if let Some(t) = &e.fx.target
            && !spids.contains(&t.spid)
        {
            spids.push(t.spid.clone());
        }
    }
    let mut tpl = SlotTemplate::default();
    for (i, sp) in spids.iter().enumerate() {
        tpl.shapes.push(SlotShape { ids: vec![sp.clone()], dgm_id: None, has_media: i % 4 == 3, inherited: false, has_xfrm: true });
        let shape = u32::try_from(i).unwrap();
        let mk = |kind| SlotSpec { kind, shape, base: BASE.clone(), word: None };
        tpl.slots.push(mk(SlotKind::Shape));
        tpl.slots.push(mk(SlotKind::Background));
        for p in 0..3 {
            tpl.slots.push(mk(SlotKind::Paragraph(p)));
        }
        for k in 0..6u32 {
            tpl.slots.push(mk(SlotKind::Unit(UnitRef { para: k / 3, k, count: 6, k_para: k % 3, count_para: 3 })));
        }
        tpl.slots.push(mk(SlotKind::ChartPart(0)));
        tpl.slots.push(mk(SlotKind::Media));
    }
    tpl
}

/// M3: a slot evaluation of a frame allocates at most 64 KiB (the budget of `uo-pptx.md`), measured over the
/// busiest slide of the deck (a template of one shape slot, a background, 3 paragraphs, 6 units, a chart part).
#[test]
fn m3_slot_evaluation_allocates_under_budget() {
    let (_, models) = busy_deck();
    let tl = build_playback_timeline(&models, &PlaybackOptions::default());
    let mut caches = AnimCaches::new();
    let mut worst = 0u64;
    for ps in tl.slides.iter().filter(|p| !p.effects.is_empty()) {
        let tpl = template_of(ps);
        for ms in [0.0, 400.0, 1200.0, 5000.0] {
            // warm the caches (parsed formulas and paths are retained by design), then measure
            let _ = evaluate_slots(&tpl, ps, ms, &MediaClock::default(), &mut caches);
            let (t, u) = measure(|| evaluate_slots(&tpl, ps, ms, &MediaClock::default(), &mut caches));
            worst = worst.max(u.allocated);
            assert!(u.allocated < 64 * 1024, "{} slots allocated {} B", t.values.len(), u.allocated);
        }
    }
    eprintln!("slot evaluation: worst frame allocated {worst} B");
}

// ───────────────────────── M2 ─────────────────────────

/// A slideshow driven for `UO_PPTX_TIMING_SOAK` (default 1,000) frames over an animated deck (per-unit effects,
/// motion paths, media): live bytes sampled every 50 frames are flat.
#[test]
fn m2_slideshow_soak_is_flat() {
    let frames: usize = std::env::var("UO_PPTX_TIMING_SOAK").ok().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let (name, models) = busy_deck();
    let mut ss = SlideShow::new(models.clone(), 0.0, 0.0, SlideShowOptions::default());
    let mut caches = AnimCaches::new();
    let mut now = 0.0;
    let t0 = Instant::now();
    let mut evaluated = 0usize;
    let series = soak(frames, 50, |i| {
        now += 33.0;
        if i % 40 == 0 {
            ss.next(now, false);
        }
        if i % 400 == 399 {
            ss.first(now);
        }
        ss.update(now);
        let f = ss.frame(now);
        if let Some(ps) = &f.slide
            && !ps.effects.is_empty()
        {
            let tpl = template_of(ps);
            let table = evaluate_slots(&tpl, ps, f.anim_ms, &MediaClock::default(), &mut caches);
            evaluated += table.values.len();
        }
    });
    eprintln!("soak {name}: {frames} frames, {evaluated} slots, {:?}, live {:?}", t0.elapsed(), &series[series.len().saturating_sub(4)..]);
    assert!(t0.elapsed().as_secs() < 20, "the soak took {:?}", t0.elapsed());
    // the caches are capped and the template/table are per frame: flat after the first samples
    assert_flat(&series, 3, 64 * 1024);
    assert!(evaluated > 0);
}

// ───────────────────────── M5 ─────────────────────────

#[test]
fn m5_formula_cache_is_capped_and_neutral() {
    let vars = FormulaVars { x: 0.5, y: 0.25, w: 0.2, h: 0.1, r: 0.0, dollar: Some(0.3) };
    let mut small = FormulaCache::with_cap(16);
    let mut big = FormulaCache::new();
    let mut plain = FormulaCache::with_cap(1);
    for i in 0..3000 {
        let f = format!("#ppt_x+{i}*#ppt_w+cos($)");
        let a = small.eval(&f, &vars);
        let b = big.eval(&f, &vars);
        let c = plain.eval(&f, &vars);
        assert_eq!(a.to_bits(), b.to_bits());
        assert_eq!(a.to_bits(), c.to_bits());
        assert!(small.len() <= 16);
        assert!(big.len() <= 1024);
    }
    assert_eq!(small.len(), 16);
    assert_eq!(big.len(), 1024, "the default cache holds 1,024 formulas");
    // a formula that does not parse is cached too (as `None`) and stays NaN
    assert!(big.eval("1+", &vars).is_nan() && big.eval("1+", &vars).is_nan());
    // after clear the bytes go back
    big.clear();
    assert!(big.is_empty());
}

fn path_node(path: &str) -> (TimeArena, NodeId) {
    let mut a = TimeArena::default();
    let mut n = TimeNode { tag: "animMotion".into(), dur: Some(1000.0), fill: Some("hold".into()), ..TimeNode::default() };
    n.bhvr_x_mut().path = Some(path.into());
    n.bhvr_mut().attr_names = Some(vec!["ppt_x".into(), "ppt_y".into()]);
    a.nodes.push(n);
    (a, NodeId(0))
}

fn effect_over(a: TimeArena, id: NodeId) -> (uo_pptx::playback::PlaybackSlide, Arc<TimingModel>) {
    use uo_pptx::playback::PlacedEffect;
    use uo_pptx::timing::{AnimStep, EffectTiming};
    let fx = EffectTiming {
        id: None,
        preset_class: "path".into(),
        preset_id: 0.0,
        preset_subtype: 0.0,
        node_type: None,
        target: Some(AnimTarget { spid: "4".into(), ..AnimTarget::default() }),
        start_ms: 0.0,
        dur_ms: 1000.0,
        block_ms: 1000.0,
        fill: None,
        filter: None,
        motion: Some(true),
        end_on_next: None,
        node: id,
    };
    let model = Arc::new(TimingModel {
        steps: vec![AnimStep { auto: true, dur_ms: 1000.0, has_indefinite: false, effects: vec![fx.clone()] }],
        nodes: a,
        ..TimingModel::default()
    });
    let ps = uo_pptx::playback::PlaybackSlide {
        index: 0,
        start_ms: 0.0,
        transition: None,
        transition_ms: 0.0,
        anim_start_ms: 0.0,
        step_start_ms: vec![0.0],
        end_ms: 5000.0,
        effects: vec![PlacedEffect { fx, t0: 0.0, step: 0, end_at: None }],
        model: model.clone(),
    };
    (ps, model)
}

#[test]
fn m5_path_cache_is_capped_and_neutral() {
    let mut small = AnimCaches::with_caps(8, 8);
    let mut big = AnimCaches::new();
    for i in 0..600 {
        let path = format!("M 0 0 L {} 0.1 C 0.1 0.{} 0.2 0.3 0.{} 0 E", i % 97, i % 10, i % 7);
        let (a, id) = path_node(&path);
        let (ps, _m) = effect_over(a, id);
        let ctx = ShapeAnimContext::default();
        let hs = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], 500.0, &ctx).unwrap();
        let hb = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], 500.0, &ctx).unwrap();
        let (x, y) = (hs.all(&BASE, &mut small), hb.all(&BASE, &mut big));
        assert_eq!((x.mx.to_bits(), x.my.to_bits()), (y.mx.to_bits(), y.my.to_bits()));
        assert!(small.path_len() <= 8);
        assert!(big.path_len() <= 256);
    }
    assert_eq!(small.path_len(), 8);
    assert_eq!(big.path_len(), 256, "the default cache holds 256 paths");
    // the M1 side: live bytes return after the caches are dropped
    assert_no_leak(|| {
        let mut c = AnimCaches::new();
        for i in 0..300 {
            let (a, id) = path_node(&format!("M 0 0 L 0.{} 0.1 E", i));
            let (ps, _m) = effect_over(a, id);
            let h = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], 300.0, &ShapeAnimContext::default()).unwrap();
            let _ = h.all(&BASE, &mut c);
        }
        c.clear();
    });
}

// ───────────────────────── security ─────────────────────────

#[test]
fn hostile_deep_and_wide_trees_are_bounded() {
    // 60,000 nested <p:par><p:cTn><p:childTnLst>: the depth cap drops the rest
    let depth = 60_000;
    let mut s = String::from("<p:sld xmlns:p=\"p\"><p:timing><p:tnLst>");
    for _ in 0..depth {
        s.push_str("<p:par><p:cTn><p:childTnLst>");
    }
    for _ in 0..depth {
        s.push_str("</p:childTnLst></p:cTn></p:par>");
    }
    s.push_str("</p:tnLst></p:timing></p:sld>");
    let doc = xml(&s);
    let t0 = Instant::now();
    let limits = Limits::default();
    let m = parse_model(&doc, &limits);
    assert!(m.nodes.nodes.len() <= limits.max_time_depth as usize, "{} nodes", m.nodes.nodes.len());
    assert!(t0.elapsed().as_secs() < 5, "deep tree took {:?}", t0.elapsed());
    // 100,000 siblings in a main sequence, with a node cap of 20,000
    let mut w = String::from(
        "<p:sld xmlns:p=\"p\"><p:timing><p:tnLst><p:par><p:cTn id=\"1\"><p:childTnLst><p:seq><p:cTn nodeType=\"mainSeq\"><p:childTnLst>",
    );
    for _ in 0..100_000 {
        w.push_str("<p:par><p:cTn presetClass=\"entr\"><p:childTnLst><p:set><p:cBhvr><p:cTn dur=\"1\"/><p:tgtEl><p:spTgt spid=\"4\"/></p:tgtEl></p:cBhvr><p:to><p:strVal val=\"visible\"/></p:to></p:set></p:childTnLst></p:cTn></p:par>");
    }
    w.push_str("</p:childTnLst></p:cTn></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>");
    let doc = xml(&w);
    let small = Limits { max_time_nodes: 20_000, ..Limits::default() };
    let t0 = Instant::now();
    let m = parse_model(&doc, &small);
    assert_eq!(m.nodes.nodes.len(), 20_000);
    assert!(t0.elapsed().as_secs() < 10, "wide tree took {:?}", t0.elapsed());
    // the timeline over it is bounded too
    let tl = build_playback_timeline(&[Some(Arc::new(m))], &PlaybackOptions::default());
    assert_eq!(tl.slides.len(), 1);
}

#[test]
fn hostile_repeats_nan_and_infinite_durations_do_not_panic() {
    // repeat counts, durations and speeds taken from hostile files
    for rc in ["1e30", "99999999999", "indefinite", "-5", "0", "1e-300", "NaN", "Infinity"] {
        for dur in ["indefinite", "1e308", "-1", "0", "NaN", "1e-320"] {
            let s = format!(
                "<p:sld xmlns:p=\"p\"><p:timing><p:tnLst><p:par><p:cTn id=\"1\"><p:childTnLst><p:seq><p:cTn nodeType=\"mainSeq\"><p:childTnLst><p:par><p:cTn><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst><p:childTnLst><p:par><p:cTn presetClass=\"entr\" repeatCount=\"{rc}\" autoRev=\"1\" spd=\"1\" accel=\"99999999\" decel=\"-5\" fill=\"hold\"><p:iterate type=\"lt\"><p:tmAbs val=\"1e999\"/></p:iterate><p:childTnLst><p:anim from=\"0\" to=\"#ppt_w\"><p:cBhvr><p:cTn dur=\"{dur}\" fill=\"hold\"/><p:tgtEl><p:spTgt spid=\"4\"/></p:tgtEl><p:attrNameLst><p:attrName>ppt_w</p:attrName></p:attrNameLst></p:cBhvr></p:anim></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>"
            );
            let doc = xml(&s);
            let m = Arc::new(parse_model(&doc, &Limits::default()));
            for opts in
                [PlaybackOptions::default(), PlaybackOptions { tick_ms: Some(33.0), transition_fps: Some(30.0), ..Default::default() }]
            {
                let tl = build_playback_timeline(&[Some(m.clone())], &opts);
                let mut c = AnimCaches::new();
                for t in [f64::NAN, -1.0, 0.0, 10.0, 1e9, f64::INFINITY, f64::NEG_INFINITY] {
                    let st = playback_state_at(&tl, t);
                    if let Some(p) = st.pos {
                        let ps = &tl.slides[p];
                        let ctx = ShapeAnimContext::default();
                        for ms in [st.anim_ms, 0.0, 100.0, f64::NAN, f64::INFINITY] {
                            if let Some(h) = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], ms, &ctx) {
                                let _ = h.all(&BASE, &mut c);
                                let u = UnitRef { para: 0, k: u32::MAX, count: 0, k_para: u32::MAX, count_para: u32::MAX };
                                let _ = h.unit_state(u, &BASE, None, &mut c);
                            }
                        }
                    }
                }
            }
            let mut ss = SlideShow::new(vec![Some(m)], f64::NAN, f64::NAN, SlideShowOptions::default());
            for now in [f64::NAN, 0.0, f64::INFINITY, 1.0, f64::NEG_INFINITY, 5.0] {
                ss.next(now, true);
                ss.prev(now);
                ss.update(now);
                let _ = ss.frame(now);
                let _ = ss.wake_at(now);
            }
        }
    }
}

/// Models built by hand (the parser would never make them): NaN everywhere, empty keyframes, a 1-point filter.
#[test]
fn hostile_hand_built_models_do_not_panic() {
    use uo_pptx::timing::{Keyframe, TavValue};
    let nan = f64::NAN;
    let mut a = TimeArena::default();
    let behaviour = |tag: &str| {
        let mut n = TimeNode {
            tag: tag.into(),
            dur: Some(nan),
            st: vec![TimeCond { delay_ms: Some(nan), ..TimeCond::default() }],
            ..TimeNode::default()
        };
        let c = n.ctn_x_mut();
        c.repeat = Some(nan);
        c.accel = Some(nan);
        c.decel = Some(1e308);
        c.spd = Some(0.0);
        c.tm_filter = Some(vec![]);
        let b = n.bhvr_mut();
        b.tav = Some(vec![]);
        b.attr_names = Some(vec!["ppt_x".into(), "style.color".into(), "fillcolor".into()]);
        b.from = Some("1e999".into());
        b.to = Some("-".into());
        b.by = Some("((".into());
        b.set_to = Some("#ppt_x/0".into());
        b.to_color = Some(CssColor::new("#zzzzzz"));
        let x = n.bhvr_x_mut();
        x.from_xy = Some((nan, f64::INFINITY));
        x.by_xy = Some((nan, nan));
        x.rot_by = Some(nan);
        x.by_hsl = Some((nan, nan, nan));
        x.from_color = Some(CssColor::new(""));
        n
    };
    for tag in ["set", "anim", "animEffect", "animScale", "animRot", "animMotion", "animClr", "cmd", "par", "seq", "excl"] {
        a.nodes.push(behaviour(tag));
    }
    // a keyframe list with NaN times and a single keyframe
    let mut kf = behaviour("anim");
    kf.bhvr_mut().tav = Some(vec![Keyframe {
        tm: nan,
        val: Some(TavValue::Str("#ppt_x".into())),
        fmla: Some("$*$".into()),
        color: Some(CssColor::new("#fff")),
    }]);
    a.nodes.push(kf);
    let mut effects = Vec::new();
    for i in 0..a.nodes.len() {
        effects.push(uo_pptx::timing::EffectTiming {
            id: None,
            preset_class: "emph".into(),
            preset_id: 0.0,
            preset_subtype: 0.0,
            node_type: None,
            target: Some(AnimTarget { spid: "4".into(), ..AnimTarget::default() }),
            start_ms: nan,
            dur_ms: nan,
            block_ms: nan,
            fill: Some("hold".into()),
            filter: Some("fade".into()),
            motion: None,
            end_on_next: Some(true),
            node: NodeId(u32::try_from(i).unwrap()),
        });
    }
    let model = Arc::new(TimingModel {
        steps: vec![uo_pptx::timing::AnimStep { auto: false, dur_ms: nan, has_indefinite: true, effects: effects.clone() }],
        interactive: effects,
        nodes: a,
        ..TimingModel::default()
    });
    let mut c = AnimCaches::new();
    let tl = build_playback_timeline(
        &[Some(model.clone()), None],
        &PlaybackOptions { tick_ms: Some(nan), transition_fps: Some(nan), ..Default::default() },
    );
    for t in [nan, 0.0, 1e300, f64::INFINITY] {
        let st = playback_state_at(&tl, t);
        if let Some(p) = st.pos {
            let ps = &tl.slides[p];
            for ms in [nan, 0.0, 1.0, f64::INFINITY, -1e300] {
                let ctx = ShapeAnimContext::default();
                if let Some(h) = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], ms, &ctx) {
                    let _ = h.all(&BASE, &mut c);
                }
                let _ = shape_opacity_at(&ps.effects, &["4"], ms);
                let _ =
                    media_state_at(&ps.model.nodes, &ps.effects, "4", ms, &MediaClock { latency_ms: Some(nan), scale: Some(nan) }, false);
            }
        }
    }
}

#[test]
fn hostile_formulas_are_bounded() {
    let vars = FormulaVars::default();
    let mut c = FormulaCache::new();
    let t0 = Instant::now();
    let bombs: Vec<String> = vec![
        "(".repeat(200_000),
        format!("{}1{}", "(".repeat(5000), ")".repeat(5000)),
        "-".repeat(1_000_000),
        "1+".repeat(500_000),
        format!("max({})", "1,".repeat(300_000)),
        "sin(".repeat(100_000),
        "1^".repeat(200_000),
        "9".repeat(10_000_000),
        "#ppt_x".repeat(100_000),
        format!("{}1", "+".repeat(500_000)),
        "$".repeat(1_000_000),
        "1e".repeat(100_000),
        "1.".repeat(100_000),
    ];
    for b in &bombs {
        let v = c.eval(b, &vars);
        let _ = v;
    }
    // the bounds themselves: a formula inside them evaluates, one past them is NaN
    assert_eq!(c.eval(&format!("{}1{}", "(".repeat(MAX_DEPTH / 2), ")".repeat(MAX_DEPTH / 2)), &vars), 1.0);
    assert!(c.eval(&format!("{}1{}", "(".repeat(MAX_DEPTH * 2), ")".repeat(MAX_DEPTH * 2)), &vars).is_nan());
    assert!(c.eval(&format!("1{}", "+1".repeat(MAX_NODES)), &vars).is_nan());
    assert!(c.eval(&format!("1{}", "+1".repeat(MAX_NODES / 4)), &vars).is_finite());
    assert!(
        c.eval(&format!("1+{}", " ".repeat(MAX_FORMULA_LEN)), &vars).is_nan()
            || c.eval(&format!("1+{}", " ".repeat(MAX_FORMULA_LEN)), &vars).is_nan()
    );
    assert!(t0.elapsed().as_secs() < 10, "formula bombs took {:?}", t0.elapsed());
    assert!(c.len() <= 1024);
}

#[test]
fn hostile_motion_paths_are_bounded() {
    let mut c = AnimCaches::new();
    let t0 = Instant::now();
    let bombs = [
        "C 0 0 0 0 0 0 ".repeat(100_000),
        "L 1 1 ".repeat(500_000),
        "M ".repeat(300_000),
        "1e".repeat(200_000),
        "Z ".repeat(400_000),
        format!("M 0 0 {}", "c 1 1 1 1 1 1 ".repeat(4000)),
    ];
    for b in &bombs {
        let (a, id) = path_node(b);
        let (ps, _m) = effect_over(a, id);
        let h = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], 500.0, &ShapeAnimContext::default()).unwrap();
        let _ = h.all(&BASE, &mut c);
    }
    assert!(t0.elapsed().as_secs() < 10, "path bombs took {:?}", t0.elapsed());
    assert!(c.path_len() <= 256);
    let (_, usage) = measure(|| {
        let (a, id) = path_node(&bombs[5]);
        let (ps, _m) = effect_over(a, id);
        let h = shape_anim_hooks(&ps.effects, &ps.model.nodes, &["4"], 500.0, &ShapeAnimContext::default()).unwrap();
        let mut c = AnimCaches::new();
        h.all(&BASE, &mut c).x
    });
    assert!(usage.peak_above_start < 64 << 20, "{usage:?}");
}

/// Random command sequences on a show over the b6a decks, with hostile clocks.
#[test]
fn slideshow_random_drive_does_not_panic() {
    let mut seed = 0x9e37_79b9_u32;
    let mut rnd = |n: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % n
    };
    let t0 = Instant::now();
    for d in 0..12 {
        let models = models_of(&deck_bytes("b6a", &format!("{:04}", d * 7)));
        for opts in [SlideShowOptions::default(), SlideShowOptions { loop_: Some(true), animations: Some(false), ..Default::default() }] {
            let mut ss = SlideShow::new(models.clone(), f64::from(rnd(5)), 0.0, opts);
            let mut now = 0.0;
            for _ in 0..300 {
                now += [0.0, 16.0, 700.0, 1e6, -5.0][rnd(5) as usize];
                match rnd(8) {
                    0 | 1 => ss.next(now, rnd(2) == 1),
                    2 => ss.prev(now),
                    3 => ss.go_to(i64::from(rnd(9)) - 2, now),
                    4 => ss.first(now),
                    5 => ss.last(now),
                    _ => ss.update(now),
                };
                let f = ss.frame(now);
                let _ = (f.anim_ms, ss.due_at(), ss.wake_at(now), ss.step(), ss.steps());
            }
        }
    }
    assert!(t0.elapsed().as_secs() < 20, "{:?}", t0.elapsed());
}
