//! Parity of timing, animation, playback and the slide show (frontier B6): replays `js/capture-timing.mjs`.
//!
//! Per deck of `timing-{samples,b1a,b6a}.capture`: the timing model of every slide (`t<N>`), the playback
//! timelines of four option sets (`tl.<v>`), `playbackStateAt` + `shapeOpacityAt` + `mediaStateAt` at sampled
//! times (`st.<v>`), every effect through `shape_anim_hooks` over a fixed base (`hk`), four random `SlideShow`
//! scripts (`sh.<k>`) and, from the sidecar `*.frames.gz`, every anim hook call the frozen painter made at sampled
//! frames (`fr`: the base in, the `PartState` out). `timing-formula` replays the formula evaluator over a corpus and
//! `fn/media` the numeric helpers. Set `UO_PPTX_DUMP=<dir>` to write the Rust texts of a failing facet.
#![cfg(feature = "canon")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_dml_render::canon::Json;
use uo_jsnum::math;
use uo_pptx::anim::{AnimBase, PartState, UnitRef};
use uo_pptx::anim_engine::{AnimCaches, ChartData, ShapeAnimContext, ShapeHooks, accel_decel, shape_anim_hooks};
use uo_pptx::anim_formula::{FormulaCache, FormulaVars};
use uo_pptx::canon::{bytes_digest, target_json, text_digest, timing_json, transition_json};
use uo_pptx::media::{MediaClock, MediaPlayState, gif_frame_at, media_commands, media_fade_alpha, media_shape_opacity, media_state_at};
use uo_pptx::playback::{
    PlaceClock, PlacedEffect, PlaybackOptions, PlaybackSlide, build_playback_timeline, playback_state_at, shape_opacity_at,
};
use uo_pptx::show::{SlideShow, SlideShowAction, SlideShowOptions};
use uo_pptx::timing::TimingModel;
use uo_pptx::{Deck, Limits};
use uo_testkit::sha256::sha256;
use uo_testkit::{Capture, Replay};

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn deck_path(key: &str) -> PathBuf {
    if let Some(rest) = key.strip_prefix("gen/") {
        let (group, n) = rest.split_once('/').unwrap();
        return captures().join("decks").join(group).join(format!("{n}.pptx"));
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples").join(key.strip_prefix("samples/").unwrap())
}

fn gunzip(b: &[u8]) -> String {
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    String::from_utf8(uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap()).unwrap()
}

fn dump(key: &str, facet: &str, text: &str) {
    if let Ok(dir) = std::env::var("UO_PPTX_DUMP") {
        let name = format!("timing.{}.{facet}.txt", key.replace('/', "__"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(name), text).unwrap();
    }
}

// ───────────────────────── encodings shared with the capture ─────────────────────────

fn bits(x: f64) -> String {
    let b = if x.is_nan() { 0x7ff8_0000_0000_0000 } else { x.to_bits() };
    format!("{b:016x}")
}

fn unbits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(char::from(b));
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn unesc(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn opt(v: Option<&str>) -> String {
    v.map_or("-".to_string(), |s| format!("+{}", esc(s)))
}

fn unopt(s: &str) -> Option<String> {
    s.strip_prefix('+').map(unesc)
}

fn base_tokens(b: &AnimBase) -> Vec<String> {
    vec![
        bits(b.x),
        bits(b.y),
        bits(b.w),
        bits(b.h),
        b.rot.map_or("-".to_string(), bits),
        opt(b.fill.as_ref().map(|c| c.as_str())),
        opt(b.line.as_ref().map(|c| c.as_str())),
        opt(b.text.as_ref().map(|c| c.as_str())),
    ]
}

fn parse_base(t: &[&str]) -> AnimBase {
    AnimBase {
        x: unbits(t[0]),
        y: unbits(t[1]),
        w: unbits(t[2]),
        h: unbits(t[3]),
        rot: (t[4] != "-").then(|| unbits(t[4])),
        fill: unopt(t[5]).map(|s| uo_dml_render::CssColor::new(&s)),
        line: unopt(t[6]).map(|s| uo_dml_render::CssColor::new(&s)),
        text: unopt(t[7]).map(|s| uo_dml_render::CssColor::new(&s)),
    }
}

fn unit_tokens(u: &UnitRef) -> Vec<String> {
    [u.para, u.k, u.count, u.k_para, u.count_para].iter().map(u32::to_string).collect()
}

fn parse_unit(t: &[&str]) -> UnitRef {
    let n = |i: usize| t[i].parse::<u32>().unwrap();
    UnitRef { para: n(0), k: n(1), count: n(2), k_para: n(3), count_para: n(4) }
}

fn state_tokens(st: Option<&PartState>) -> Vec<String> {
    let Some(st) = st else { return vec!["null".to_string()] };
    let mut v = vec!["S".to_string(), if st.visible { "1" } else { "0" }.to_string()];
    for x in [st.opacity, st.x, st.y, st.w, st.h, st.rot, st.sx, st.sy, st.mx, st.my] {
        v.push(bits(x));
    }
    v.push(st.filters.len().to_string());
    for f in &st.filters {
        v.push(esc(&f.filter));
        v.push(esc(&f.transition));
        v.push(bits(f.p));
    }
    v.push(opt(st.fill_color.as_ref().map(|c| c.as_str())));
    v.push(opt(st.line_color.as_ref().map(|c| c.as_str())));
    v.push(opt(st.text_color.as_ref().map(|c| c.as_str())));
    v.push(opt(st.font_weight.as_deref()));
    v.push(opt(st.font_style.as_deref()));
    v.push(st.underline.map_or("-", |u| if u { "1" } else { "0" }).to_string());
    v.push(st.font_scale.map_or("-".to_string(), bits));
    v.push(opt(st.font_family.as_deref()));
    v
}

fn digest_of(text: &str) -> String {
    let d = sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_dml_render::canon::hex(&d)[..32])
}

// ───────────────────────── variants ─────────────────────────

fn pb_opts(v: &str) -> PlaybackOptions {
    match v {
        "a" => PlaybackOptions::default(),
        "b" => PlaybackOptions { tick_ms: Some(33.0), transition_fps: Some(30.0), ..Default::default() },
        "c" => PlaybackOptions { use_timings: Some(false), default_slide_ms: Some(2500.0), ..Default::default() },
        "d" => PlaybackOptions { tick_ms: Some(1000.0 / 60.0), default_slide_ms: Some(4000.0), ..Default::default() },
        _ => panic!("variant {v}"),
    }
}

fn sh_opts(k: &str) -> SlideShowOptions {
    match k {
        "a" => SlideShowOptions::default(),
        "b" => SlideShowOptions { use_timings: Some(false), ..Default::default() },
        "c" => SlideShowOptions { animations: Some(false), ..Default::default() },
        "d" => SlideShowOptions { loop_: Some(true), ..Default::default() },
        _ => panic!("script {k}"),
    }
}

type Models = Vec<Option<Arc<TimingModel>>>;

// ───────────────────────── the JSON texts ─────────────────────────

fn num(x: f64) -> Json {
    Json::Num(x)
}

fn effect_rec(a: &uo_pptx::timing::TimeArena, e: &PlacedEffect) -> Json {
    let fx = &e.fx;
    let mut o = Json::obj();
    o.string("id", fx.id.as_deref()).st("presetClass", &fx.preset_class).n("presetId", fx.preset_id).n("presetSubtype", fx.preset_subtype);
    o.string("nodeType", fx.node_type.as_deref()).json("target", fx.target.as_ref().map(target_json));
    o.n("startMs", fx.start_ms).n("durMs", fx.dur_ms).n("blockMs", fx.block_ms);
    o.string("fill", fx.fill.as_deref())
        .string("filter", fx.filter.as_deref())
        .boolean("motion", fx.motion)
        .boolean("endOnNext", fx.end_on_next);
    let n = a.node(fx.node);
    o.put("node", Json::Arr(vec![Json::s(&n.tag), n.id.as_deref().map_or(Json::Null, Json::s)]));
    o.n("t0", e.t0).n("step", f64::from(e.step)).num("endAt", e.end_at);
    o.done()
}

fn tl_json(tl: &uo_pptx::playback::PlaybackTimeline) -> String {
    let mut o = Json::obj();
    o.put(
        "slides",
        Json::Arr(
            tl.slides
                .iter()
                .map(|s| {
                    let mut so = Json::obj();
                    so.n("index", f64::from(s.index)).n("startMs", s.start_ms);
                    so.put("transition", s.transition.as_ref().map_or(Json::Null, transition_json));
                    so.n("transitionMs", s.transition_ms).n("animStartMs", s.anim_start_ms);
                    so.put("stepStartMs", Json::Arr(s.step_start_ms.iter().map(|x| num(*x)).collect()));
                    so.n("endMs", s.end_ms);
                    so.put("effects", Json::Arr(s.effects.iter().map(|e| effect_rec(&s.model.nodes, e)).collect()));
                    so.done()
                })
                .collect(),
        ),
    );
    o.n("durationMs", tl.duration_ms).n("tickMs", tl.tick_ms).n("mediaClock", tl.media_clock);
    o.done().to_text()
}

/// `animTimes` of the capture (candidates sorted, evenly spaced picks).
fn anim_times(ps: &PlaybackSlide, limit: usize) -> Vec<f64> {
    let mut c: Vec<f64> = vec![0.0, -1.0];
    for e in &ps.effects {
        if e.t0 >= uo_pptx::timing_eval::STEP_PENDING {
            continue;
        }
        c.push(e.t0);
        if e.fx.dur_ms.is_finite() {
            c.push(e.t0 + e.fx.dur_ms / 2.0);
            c.push(e.t0 + e.fx.dur_ms - 1.0);
            c.push(e.t0 + e.fx.dur_ms + 10.0);
        } else {
            c.push(e.t0 + 700.0);
        }
    }
    let mut all: Vec<f64> = c.into_iter().filter(|x| x.is_finite()).collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    all.dedup_by(|a, b| *a == *b);
    if all.len() <= limit {
        return all;
    }
    let mut out: Vec<f64> = Vec::new();
    for i in 0..limit {
        let k = math::round(((i * (all.len() - 1)) as f64) / ((limit - 1) as f64));
        let x = all[k as usize];
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

fn state_times(tl: &uo_pptx::playback::PlaybackTimeline) -> Vec<f64> {
    let mut ts: Vec<f64> = vec![-5.0, 0.0, tl.duration_ms, tl.duration_ms + 1000.0, tl.duration_ms / 2.0];
    for s in &tl.slides {
        ts.extend([s.start_ms, s.start_ms + s.transition_ms / 2.0, s.anim_start_ms - 1.0, s.anim_start_ms, s.end_ms - 1.0, s.end_ms]);
        for ms in anim_times(s, 4) {
            ts.push(s.anim_start_ms + ms);
        }
    }
    let mut u: Vec<f64> = Vec::new();
    for x in ts.into_iter().filter(|x| x.is_finite()) {
        if !u.contains(&x) {
            u.push(x);
        }
    }
    u.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if u.len() > 80 {
        let step = u.len().div_ceil(80);
        return u.into_iter().enumerate().filter(|(i, _)| i % step == 0).map(|(_, x)| x).collect();
    }
    u
}

fn media_json(s: &MediaPlayState) -> Json {
    let mut o = Json::obj();
    o.put("started", Json::Bool(s.started))
        .put("playing", Json::Bool(s.playing))
        .n("timeSec", s.time_sec)
        .num("durMs", s.dur_ms)
        .boolean("ended", s.ended);
    o.done()
}

fn states_json(tl: &uo_pptx::playback::PlaybackTimeline, models: &Models) -> String {
    let clocks = [MediaClock::default(), MediaClock { latency_ms: Some(66.0), scale: Some(0.9) }];
    let mut out: Vec<Json> = Vec::new();
    for t in state_times(tl) {
        let st = playback_state_at(tl, t);
        let mut rec = Json::obj();
        rec.n("t", t).n("pos", st.pos.map_or(-1.0, |p| p as f64));
        rec.put("from", st.from.map_or(Json::Null, |f| num(f as f64)));
        rec.n("tp", st.transition_progress).n("anim", st.anim_ms).put("ended", Json::Bool(st.ended));
        if let Some(p) = st.pos {
            let slide = &tl.slides[p];
            let mut spids: Vec<&str> = Vec::new();
            for e in &slide.effects {
                if let Some(t) = &e.fx.target
                    && !spids.contains(&&*t.spid)
                {
                    spids.push(&t.spid);
                }
            }
            let mut op = Json::obj();
            for sp in spids {
                op.n(sp, shape_opacity_at(&slide.effects, &[sp], st.anim_ms));
            }
            rec.put("op", op.done());
            let m = models.get(slide.index as usize).and_then(|m| m.as_ref());
            let media = m.and_then(|m| m.media.as_deref()).unwrap_or(&[]);
            let arena = &slide.model.nodes;
            rec.put(
                "media",
                Json::Arr(
                    media
                        .iter()
                        .map(|mi| {
                            Json::Arr(
                                clocks
                                    .iter()
                                    .map(|ck| {
                                        media_json(&media_state_at(arena, &slide.effects, &mi.spid, st.anim_ms, ck, mi.loop_ == Some(true)))
                                    })
                                    .collect(),
                            )
                        })
                        .collect(),
                ),
            );
            rec.put(
                "cmds",
                Json::Arr(
                    media
                        .iter()
                        .map(|mi| {
                            Json::Arr(
                                media_commands(arena, &slide.effects, &mi.spid)
                                    .iter()
                                    .map(|c| {
                                        let mut co = Json::obj();
                                        co.n("atMs", c.at_ms).st("cmd", &c.cmd).num("durMs", c.dur_ms);
                                        co.done()
                                    })
                                    .collect(),
                            )
                        })
                        .collect(),
                ),
            );
        }
        out.push(rec.done());
    }
    Json::Arr(out).to_text()
}

// ───────────────────────── hooks ─────────────────────────

fn hook_flags(h: &ShapeHooks<'_>) -> String {
    let mut f: Vec<String> = Vec::new();
    if h.has_bg() {
        f.push("bg:1".into());
    }
    if h.has_para() {
        f.push("para:1".into());
    }
    if let Some(u) = h.unit_info() {
        f.push(format!("unit:{}", if u.letters { "lt" } else { "wd" }));
        f.push(format!("words:{}", u32::from(u.words)));
    }
    if !h.chart_parts().is_empty() {
        f.push(format!("chart:{}", h.chart_parts().len()));
    }
    if f.is_empty() { "plain".into() } else { f.join(",") }
}

fn chart_lines(h: &ShapeHooks<'_>, c: &mut AnimCaches, out: &mut Vec<String>) {
    for (j, p) in h.chart_parts().iter().enumerate() {
        let probe = if p.sel.data == ChartData::None {
            "-".to_string()
        } else {
            let mut s = String::new();
            for sr in [-1.0, 0.0, 1.0, 2.0] {
                for k in [-1.0, 0.0, 1.0, 2.0] {
                    s.push(if p.sel.data.matches(sr, k) { '1' } else { '0' });
                }
            }
            s
        };
        out.push(
            [
                "C".to_string(),
                j.to_string(),
                p.sel.key.as_deref().map_or("-".to_string(), esc),
                if p.sel.bg { "1" } else { "0" }.to_string(),
                if p.is_animated() { "1" } else { "0" }.to_string(),
                probe,
            ]
            .join("\t"),
        );
        if p.is_animated() {
            let st = h.chart_state(j, &HK_BASE, c);
            let mut t = vec!["A".to_string(), "chart".to_string(), j.to_string()];
            t.extend(base_tokens(&HK_BASE));
            t.push("=".into());
            t.extend(state_tokens(st.as_ref()));
            out.push(t.join("\t"));
        }
    }
}

static HK_BASE: std::sync::LazyLock<AnimBase> = std::sync::LazyLock::new(|| AnimBase {
    x: 0.5,
    y: 0.4,
    w: 0.2,
    h: 0.1,
    rot: Some(15.0),
    fill: Some(uo_dml_render::CssColor::new("#336699")),
    line: Some(uo_dml_render::CssColor::new("#CC0000")),
    text: Some(uo_dml_render::CssColor::new("#00AA00")),
});

fn hk_units() -> Vec<(UnitRef, Option<UnitRef>)> {
    let u = |para, k, count, k_para, count_para| UnitRef { para, k, count, k_para, count_para };
    vec![
        (u(0, 0, 6, 0, 3), None),
        (u(0, 2, 6, 2, 3), Some(u(0, 1, 4, 1, 2))),
        (u(1, 3, 6, 0, 3), Some(u(1, 2, 4, 0, 2))),
        (u(2, 5, 6, 2, 3), None),
    ]
}

fn line(parts: Vec<String>) -> String {
    parts.join("\t")
}

fn hook_sweep(ps: &PlaybackSlide, caches: &mut AnimCaches) -> Vec<String> {
    let model = &ps.model;
    let bg: Vec<&str> = model.anim_bg.as_deref().unwrap_or(&[]).iter().map(|s| &**s).collect();
    let anim_bg = |spid: &str| bg.contains(&spid);
    let mut lines = Vec::new();
    let mut spids: Vec<&str> = Vec::new();
    let mut dgms: Vec<Option<&str>> = vec![None];
    for e in &ps.effects {
        if let Some(t) = &e.fx.target {
            if !spids.contains(&&*t.spid) {
                spids.push(&t.spid);
            }
            if let Some(d) = t.dgm.as_deref().filter(|d| !d.is_empty())
                && !dgms.contains(&Some(d))
            {
                dgms.push(Some(d));
            }
        }
    }
    for ms in anim_times(ps, 6) {
        for sp in &spids {
            for dgm in &dgms {
                let ctx = ShapeAnimContext { anim_bg: Some(&anim_bg), dgm_id: *dgm, para_count: None };
                let h = shape_anim_hooks(&ps.effects, &model.nodes, &[sp, "g1"], ms, &ctx);
                lines.push(format!(
                    "H\t{}\t{}\t{}\t{}",
                    esc(sp),
                    bits(ms),
                    dgm.map_or("-".to_string(), esc),
                    h.as_ref().map_or("null".to_string(), hook_flags)
                ));
                let Some(h) = h else { continue };
                let b = &*HK_BASE;
                let mut t = vec!["A".to_string(), "all".to_string()];
                t.extend(base_tokens(b));
                t.push("=".into());
                t.extend(state_tokens(Some(&h.all(b, caches))));
                lines.push(line(t));
                if h.has_bg() {
                    let mut t = vec!["A".to_string(), "bg".to_string()];
                    t.extend(base_tokens(b));
                    t.push("=".into());
                    t.extend(state_tokens(h.bg(b, caches).as_ref()));
                    lines.push(line(t));
                }
                if h.has_para() {
                    for i in 0..3u32 {
                        let mut t = vec!["A".to_string(), "para".to_string(), i.to_string()];
                        t.extend(base_tokens(b));
                        t.push("=".into());
                        t.extend(state_tokens(h.para(i, b, caches).as_ref()));
                        lines.push(line(t));
                    }
                }
                if h.unit_info().is_some() {
                    for (u, w) in hk_units() {
                        let mut t = vec!["A".to_string(), "unit".to_string()];
                        t.extend(unit_tokens(&u));
                        t.extend(base_tokens(b));
                        t.push(w.as_ref().map_or("-".to_string(), |w| unit_tokens(w).join(",")));
                        t.push("=".into());
                        t.extend(state_tokens(h.unit_state(u, b, w, caches).as_ref()));
                        lines.push(line(t));
                    }
                }
                chart_lines(&h, caches, &mut lines);
            }
        }
    }
    lines
}

// ───────────────────────── the show scripts ─────────────────────────

fn fnv(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }
    fn below(&mut self, n: u32) -> u32 {
        let v = self.next();
        if n == 0 { 0 } else { v % n }
    }
}

const DT: [f64; 8] = [0.0, 0.0, 16.0, 100.0, 700.0, 1500.0, 4000.0, 12000.0];

fn action_name(a: SlideShowAction) -> &'static str {
    match a {
        SlideShowAction::None => "none",
        SlideShowAction::Transition => "transition",
        SlideShowAction::Step => "step",
        SlideShowAction::Slide => "slide",
        SlideShowAction::End => "end",
        SlideShowAction::Exit => "exit",
    }
}

struct ShowResult {
    text: String,
    /// per command: (now, slide, anim_ms) of the frame
    frames: Vec<(f64, Option<Arc<PlaybackSlide>>, f64)>,
}

fn run_show(key: &str, k: &str, models: &Models) -> ShowResult {
    let mut rng = Rng::new(fnv(&format!("{key}#{k}")));
    let n = u32::try_from(models.len()).unwrap();
    let mut now = 1000.0;
    let from = rng.below(n + 1);
    let mut ss = SlideShow::new(models.clone(), f64::from(from), now, sh_opts(k));
    let mut rec: Vec<Json> = Vec::new();
    let mut frames = Vec::new();
    for i in 0..40 {
        let kind = rng.below(100);
        now += DT[rng.below(8) as usize];
        let (mut name, ret): (String, SlideShowAction);
        if kind < 40 {
            let mouse = rng.below(2) == 1;
            ret = ss.next(now, mouse);
            name = "next".to_string();
            if mouse {
                name.push('m');
            }
        } else if kind < 55 {
            name = "prev".to_string();
            ret = ss.prev(now);
        } else if kind < 62 {
            let g = i64::from(rng.below(n + 2)) - 1;
            name = format!("go{g}");
            ret = ss.go_to(g, now);
        } else if kind < 66 {
            name = "first".to_string();
            ret = ss.first(now);
        } else if kind < 70 {
            name = "last".to_string();
            ret = ss.last(now);
        } else {
            name = "upd".to_string();
            ret = SlideShowAction::None;
        }
        let u = ss.update(now);
        let f = ss.frame(now);
        let mut ro = Json::obj();
        ro.n("i", f64::from(i)).st("name", &name).n("now", now).st("ret", action_name(ret)).st("u", action_name(u));
        ro.n("idx", ss.index().map_or(-1.0, |x| x as f64)).n("step", ss.step() as f64).n("steps", ss.steps() as f64);
        ro.put("ended", Json::Bool(ss.ended())).n("due", ss.due_at()).n("wake", ss.wake_at(now));
        let mut fo = Json::obj();
        fo.put("slide", f.slide.as_ref().map_or(Json::Null, |s| num(f64::from(s.index))));
        fo.n("animMs", f.anim_ms).n("tp", f.transition_progress).put("ended", Json::Bool(f.ended));
        fo.put(
            "from",
            f.from.as_ref().map_or(Json::Null, |o| {
                let mut x = Json::obj();
                x.n("index", f64::from(o.slide.index)).n("animMs", o.anim_ms).n("key", f64::from(o.key));
                x.done()
            }),
        );
        fo.put(
            "ps",
            f.slide.as_ref().map_or(Json::Null, |s| {
                let mut x = Json::obj();
                x.n("startMs", s.start_ms).put("transition", s.transition.as_ref().map_or(Json::Null, transition_json));
                x.n("transitionMs", s.transition_ms).n("animStartMs", s.anim_start_ms);
                x.put("stepStartMs", Json::Arr(s.step_start_ms.iter().map(|v| num(*v)).collect()));
                x.put(
                    "effects",
                    Json::Arr(
                        s.effects
                            .iter()
                            .map(|e| {
                                Json::Arr(vec![
                                    num(e.t0),
                                    num(f64::from(e.step)),
                                    e.end_at.map_or(Json::Null, num),
                                    num(e.fx.dur_ms),
                                    num(e.fx.start_ms),
                                ])
                            })
                            .collect(),
                    ),
                );
                x.done()
            }),
        );
        ro.put("f", fo.done());
        rec.push(ro.done());
        frames.push((now, f.slide.clone(), f.anim_ms));
    }
    ShowResult { text: Json::Arr(rec).to_text(), frames }
}

// ───────────────────────── the painted frames ─────────────────────────

struct Sections(HashMap<String, Vec<String>>);

fn sections(text: &str) -> Sections {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    let mut cur: Option<String> = None;
    for l in text.lines() {
        if let Some(k) = l.strip_prefix("#deck\t") {
            cur = Some(k.to_string());
            m.entry(k.to_string()).or_default().push(l.to_string());
        } else if let Some(k) = &cur {
            m.entry(k.clone()).or_default().push(l.to_string());
        }
    }
    Sections(m)
}

/// Replays one deck's section: the Rust lines of every frame, compared with the recorded ones.
fn replay_frames(key: &str, rec: &[String], models: &Models, caches: &mut AnimCaches) -> Vec<String> {
    let mut out: Vec<String> = vec![format!("#deck\t{key}")];
    // timelines by variant, show frames of script `a`
    let mut tls: HashMap<String, uo_pptx::playback::PlaybackTimeline> = HashMap::new();
    let mut shows: Option<ShowResult> = None;
    let mut cur: Option<(Arc<PlaybackSlide>, f64)> = None; // the frame's slide and anim time
    let mut i = 1;
    while i < rec.len() {
        let l = &rec[i];
        let t: Vec<&str> = l.split('\t').collect();
        match t[0] {
            "F" => {
                let (slide, anim) = if t[1] == "tl" {
                    let v = t[2];
                    let tl = tls.entry(v.to_string()).or_insert_with(|| build_playback_timeline(models, &pb_opts(v)));
                    let st = playback_state_at(tl, unbits(t[3]));
                    let ps = &tl.slides[st.pos.unwrap()];
                    (Arc::new(ps.clone()), st.anim_ms)
                } else {
                    let sh = shows.get_or_insert_with(|| run_show(key, "a", models));
                    let idx: usize = t[3].parse().unwrap();
                    let (_, s, a) = &sh.frames[idx];
                    (s.clone().unwrap(), *a)
                };
                out.push(l.clone());
                out.push(format!("P\t{}\t{}", slide.index, bits(anim)));
                cur = Some((slide, anim));
                // the recorded P line follows: skip it (compared through `out` at the end)
                i += 1;
                if rec.get(i).is_some_and(|x| x.starts_with("P\t")) {
                    i += 1;
                }
                continue;
            }
            "H" => {
                let ids: Vec<String> = t[1].split(',').map(unesc).collect();
                let dgm = (t[2] != "-").then(|| unesc(t[2]));
                let (ps, anim) = cur.clone().unwrap();
                let idr: Vec<&str> = ids.iter().map(String::as_str).collect();
                let bg: Vec<&str> = ps.model.anim_bg.as_deref().unwrap_or(&[]).iter().map(|s| &**s).collect();
                let anim_bg = |spid: &str| bg.contains(&spid);
                let ctx = ShapeAnimContext { anim_bg: Some(&anim_bg), dgm_id: dgm.as_deref(), para_count: None };
                let h = shape_anim_hooks(&ps.effects, &ps.model.nodes, &idr, anim, &ctx);
                out.push(format!("H\t{}\t{}\t{}", t[1], t[2], h.as_ref().map_or("null".to_string(), hook_flags)));
                // consume the lines of this hook set (C, A) up to the next H/F/O/E/#deck
                i += 1;
                while i < rec.len() {
                    let lt: Vec<&str> = rec[i].split('\t').collect();
                    if !matches!(lt[0], "C" | "A") {
                        break;
                    }
                    let Some(h) = h.as_ref() else { panic!("{key}: a hook line after a null hook set: {}", rec[i]) };
                    if lt[0] == "C" {
                        // the C lines of this set, all at once
                        chart_lines(h, caches, &mut out);
                        // and skip the recorded C/chart-A lines of the same group (they are re-checked by equality)
                        // Those lines come first in the record; the A all/bg/para/unit calls follow in the paint order.
                        while i < rec.len() {
                            let ct: Vec<&str> = rec[i].split('\t').collect();
                            let is_chart = ct[0] == "C" || (ct[0] == "A" && ct[1] == "chart");
                            if !is_chart {
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    let kind = lt[1];
                    let eq = lt.iter().position(|x| *x == "=").unwrap();
                    let (args, _) = lt.split_at(eq);
                    let mut o: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
                    o.push("=".into());
                    let st = match kind {
                        "all" => {
                            let b = parse_base(&args[2..10]);
                            Some(h.all(&b, caches))
                        }
                        "bg" => {
                            let b = parse_base(&args[2..10]);
                            h.bg(&b, caches)
                        }
                        "para" => {
                            let idx: u32 = args[2].parse().unwrap();
                            let b = parse_base(&args[3..11]);
                            h.para(idx, &b, caches)
                        }
                        "unit" => {
                            let u = parse_unit(&args[2..7]);
                            let b = parse_base(&args[7..15]);
                            let w = (args[15] != "-").then(|| {
                                let w: Vec<&str> = args[15].split(',').collect();
                                parse_unit(&w)
                            });
                            h.unit_state(u, &b, w, caches)
                        }
                        other => panic!("{key}: A kind {other}"),
                    };
                    o.extend(state_tokens(st.as_ref()));
                    out.push(line(o));
                    i += 1;
                }
                continue;
            }
            "O" => {
                let (ps, anim) = cur.clone().unwrap();
                let spid = unesc(t[1]);
                let op = media_shape_opacity(&ps.model.nodes, &ps.effects, &ps.model, &spid, anim, &MediaClock::default());
                out.push(format!("O\t{}\t{}", t[1], bits(op)));
            }
            _ => out.push(l.clone()),
        }
        i += 1;
    }
    out
}

// ───────────────────────── the replay ─────────────────────────

fn open_models(deck: &mut Deck) -> Result<Models, String> {
    let n = deck.presentation().map_err(|e| e.to_string())?.slides.len();
    let mut v: Models = Vec::new();
    for i in 0..n {
        match deck.slide(u32::try_from(i).unwrap()) {
            Ok(m) => v.push(m.timing.clone()),
            Err(_) => v.push(None),
        }
    }
    Ok(v)
}

fn first_diff(a: &[String], b: &[String]) -> String {
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("line {i}:\n  rust: {x}\n  ts:   {y}");
        }
    }
    format!("line counts differ: rust {} ts {}", a.len(), b.len())
}

fn replay(stem: &str) -> (usize, usize, uo_testkit::Report) {
    let cap = Capture::read(captures().join(format!("{stem}.capture"))).unwrap();
    let frames = sections(&gunzip(&std::fs::read(captures().join(format!("{stem}.frames.gz"))).unwrap()));
    let mut rp = Replay::new(&cap);
    let mut decks = 0;
    let mut calls = 0;
    let mut caches = AnimCaches::new();
    for rec in cap.records() {
        let key = rec.key.as_str();
        let bytes = std::fs::read(deck_path(key)).unwrap_or_else(|e| panic!("{key}: {e}"));
        rp.check(key, "input", &bytes_digest(&bytes));
        decks += 1;
        let mut deck = ts(Deck::open(&bytes, Limits::default()).unwrap());
        let models = match open_models(&mut deck) {
            Ok(m) => m,
            Err(e) => {
                rp.check(key, "read", &format!("err:{e}"));
                continue;
            }
        };
        rp.check(key, "read", "ok");
        rp.check(key, "slides", &models.len().to_string());
        for (i, m) in models.iter().enumerate() {
            let text = m.as_ref().map_or(String::new(), |m| timing_json(m).to_text());
            dump(key, &format!("t{i}"), &text);
            rp.check(key, &format!("t{i}"), &text_digest(&text));
        }
        let mut tla = None;
        for v in ["a", "b", "c", "d"] {
            let tl = build_playback_timeline(&models, &pb_opts(v));
            let text = tl_json(&tl);
            dump(key, &format!("tl.{v}"), &text);
            rp.check(key, &format!("tl.{v}"), &text_digest(&text));
            let st = states_json(&tl, &models);
            dump(key, &format!("st.{v}"), &st);
            rp.check(key, &format!("st.{v}"), &text_digest(&st));
            if v == "a" {
                tla = Some(tl);
            }
        }
        let tla = tla.unwrap();
        let mut sweep: Vec<String> = Vec::new();
        for ps in &tla.slides {
            if !ps.effects.is_empty() {
                sweep.push(format!("#slide\t{}", ps.index));
                sweep.extend(hook_sweep(ps, &mut caches));
            }
        }
        let sweep_text = sweep.join("\n");
        dump(key, "hk", &sweep_text);
        rp.check(key, "hk", &text_digest(&sweep_text));
        for k in ["a", "b", "c", "d"] {
            let r = run_show(key, k, &models);
            dump(key, &format!("sh.{k}"), &r.text);
            rp.check(key, &format!("sh.{k}"), &text_digest(&r.text));
        }
        // the painted frames
        let rec = frames.0.get(key).unwrap_or_else(|| panic!("{key}: no frame section"));
        let ours = replay_frames(key, rec, &models, &mut caches);
        calls += ours.iter().filter(|l| l.starts_with("A\t")).count();
        let text = ours.join("\n") + "\n";
        let d = text_digest(&text);
        dump(key, "fr", &text);
        if d != cap.get(key, "fr").unwrap() {
            // a readable first difference, then the facet comparison reports the mismatch
            eprintln!("{key}: frames differ\n{}", first_diff(&ours, rec));
        }
        rp.check(key, "fr", &d);
    }
    (decks, calls, rp.finish())
}

fn run(stem: &str, expect_decks: usize) {
    let (decks, calls, report) = replay(stem);
    eprintln!("{stem}: {decks} decks, {calls} hook calls, {}", report.summary());
    assert!(report.mismatches.is_empty(), "{}\n{}", report.summary(), report.describe(30));
    assert_eq!(decks, expect_decks);
}

#[test]
fn samples_timing_matches_the_ts() {
    run("timing-samples", 28);
}

#[test]
fn b1a_timing_matches_the_ts() {
    run("timing-b1a", 200);
}

#[test]
fn b6a_timing_matches_the_ts() {
    run("timing-b6a", 100);
}

// ───────────────────────── formulas and numeric helpers ─────────────────────────

#[test]
fn formula_evaluator_matches_the_ts() {
    let cap = Capture::read(captures().join("timing-formula.capture")).unwrap();
    let text = gunzip(&std::fs::read(captures().join("timing-formula.formulas.gz")).unwrap());
    let mk = |x: f64, y: f64, w: f64, h: f64, r: f64, d: Option<f64>| FormulaVars { x, y, w, h, r, dollar: d };
    let vars = [mk(0.5, 0.4, 0.2, 0.1, 15.0, None), mk(0.0, 0.0, 0.0, 0.0, 0.0, None), mk(1.25, -0.3, 3.0, 0.007, -90.0, Some(0.37))];
    let mut cache = FormulaCache::new();
    let mut bad: Vec<String> = Vec::new();
    let mut n = 0;
    for l in text.lines() {
        let t: Vec<&str> = l.split('\t').collect();
        let f = unesc(t[0]);
        for (k, v) in vars.iter().enumerate() {
            let got = bits(cache.eval(&f, v));
            n += 1;
            if got != t[1 + k] {
                bad.push(format!("{f:?} vars{k}: rust {got} ({}) ts {} ({})", cache.eval(&f, v), t[1 + k], unbits(t[1 + k])));
            }
        }
    }
    let rec = cap.record("formula/ts").unwrap();
    assert_eq!(digest_of(&text), rec.get("results").unwrap());
    eprintln!("formula: {} formulas, {n} evaluations, {} mismatches", text.lines().count(), bad.len());
    // The only declared differences: the formulas that exceed the bounds (length 4096, 1024 nodes, depth 128)
    // are NaN in Rust where the TS evaluates (or overflows its stack); the corpus includes four of them.
    let known = |s: &str| {
        s.contains("(((((((((") || s.contains("+1+1+1+1") || s.contains("------") || s.contains("sin(sin(") || s.contains("1,1,1,1,1,1")
    };
    let unexpected: Vec<&String> = bad.iter().filter(|b| !known(b)).collect();
    assert!(
        unexpected.is_empty(),
        "{} unexpected of {}:\n{}",
        unexpected.len(),
        bad.len(),
        unexpected.iter().take(20).map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
    for b in &bad {
        eprintln!("declared: {}", b.chars().take(100).collect::<String>());
    }
    assert!(bad.len() <= 12, "{}", bad.join("\n"));
}

#[test]
fn numeric_helpers_match_the_ts() {
    let cap = Capture::read(captures().join("timing-formula.capture")).unwrap();
    let rec = cap.record("fn/media").unwrap();
    let f = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, -0.1, 1.2, f64::NAN];
    let ad = [0.0, 0.1, 0.3, 0.5, 0.9, 1.0, f64::NAN, -0.2];
    let mut acc = Vec::new();
    for x in f {
        for p in ad {
            for q in ad {
                acc.push(bits(accel_decel(x, p, q)));
            }
        }
    }
    assert_eq!(digest_of(&acc.join(",")), rec.get("acc").unwrap(), "accelDecel");
    let delays: [&[f64]; 7] =
        [&[], &[100.0], &[100.0, 50.0], &[5.0, 5.0, 5.0], &[100.0, 50.0, 5.0, 200.0], &[0.0, 0.0], &[10.0, 11.0, 1000.0]];
    let el = [0.0, -1.0, 1.0, 99.0, 100.0, 150.0, 449.0, 450.0, 451.0, 900.0, 100_000.0, f64::NAN, f64::INFINITY];
    let rp = [0.0, 1.0, 3.0, f64::INFINITY];
    let mut gif = Vec::new();
    for d in delays {
        for e in el {
            for r in rp {
                gif.push(gif_frame_at(d, e, r).to_string());
            }
        }
    }
    assert_eq!(digest_of(&gif.join(",")), rec.get("gif").unwrap(), "gifFrameAt");
    let fades: [Option<(f64, f64)>; 5] = [None, Some((0.0, 0.0)), Some((500.0, 0.0)), Some((0.0, 500.0)), Some((500.0, 500.0))];
    let mut fa = Vec::new();
    for fd in fades {
        for p in [-10.0, 0.0, 100.0, 250.0, 500.0, 1000.0, 4000.0, f64::NAN] {
            for l in [None, Some(1000.0), Some(3000.0), Some(f64::INFINITY), Some(f64::NAN)] {
                fa.push(bits(media_fade_alpha(fd, p, l)));
            }
        }
    }
    assert_eq!(digest_of(&fa.join(",")), rec.get("fade").unwrap(), "mediaFadeAlpha");
}

/// `place_slide_effects` with the default clock is what the show uses (a compile-time use of the clock type).
#[test]
fn place_clock_defaults() {
    let c = PlaceClock::default();
    assert!(c.tick_ms.is_none() && c.media_clock.is_none());
}

/// Every TS switch on (`SlidesRules::ts_strict`): this file replays TS captures (Slides wave 5).
fn ts(mut d: Deck) -> Deck {
    d.set_rules(uo_dml_render::SlidesRules::ts_strict());
    d
}
