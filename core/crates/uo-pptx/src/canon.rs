//! The canonical JSON of the deck model (feature `canon`): the text of the model captures
//! (`js/capture-model.mjs`; rules in `core/docs/modules/uo-pptx-captures.md`, printer of the shape
//! types in `uo_dml_render::canon`).
//!
//! [`slide_record`] prints the slide WITHOUT `timing` (the capture's `n<N>` / `nmodel` facets);
//! [`slide_record_with_timing`] adds `parseSlideTiming`'s model (the `s<N>` / `model` facets, frontier B6).

use uo_dml_render::canon::{Canon, Json};

use crate::model::{LegacyAnimType, Presentation, SlideModel};
use crate::timing::{AnimTarget, EffectTiming, NodeId, TavValue, TimeArena, TimingModel, TransitionInfo};

/// The `Presentation` (`readPresentation`).
pub fn presentation_json(p: &Presentation) -> Json {
    let mut o = Json::obj();
    o.n("slideW", p.slide_w).n("slideH", p.slide_h).string("sizeType", p.size_type.as_deref());
    o.put(
        "slides",
        Json::Arr(
            p.slides
                .iter()
                .map(|s| {
                    let mut so = Json::obj();
                    so.st("id", &s.id).st("rId", &s.r_id).string("target", s.target.as_deref());
                    so.done()
                })
                .collect(),
        ),
    );
    if let Some(sh) = &p.show {
        let mut so = Json::obj();
        so.boolean("loop", sh.loop_)
            .boolean("useTimings", sh.use_timings)
            .boolean("showAnimation", sh.show_animation)
            .boolean("kiosk", sh.kiosk);
        if let Some((a, b)) = sh.range {
            so.put("range", Json::Arr(vec![Json::Num(a), Json::Num(b)]));
        }
        o.put("show", so.done());
    }
    o.done()
}

/// The `Slide` of `readSlide` (without `timing`).
pub fn slide_json(c: &Canon<'_>, s: &SlideModel) -> Json {
    let mut o = Json::obj();
    o.put("shapes", Json::Arr(s.shapes.iter().map(|x| c.shape(x)).collect()));
    o.json("bg", s.bg.as_ref().map(|f| c.fill(f)));
    o.json("bgImage", s.bg_image.as_ref().map(|i| c.image(i)));
    o.boolean("bgTile", s.bg_tile);
    if let Some(a) = &s.animations {
        o.put(
            "animations",
            Json::Arr(
                a.iter()
                    .map(|e| {
                        let mut eo = Json::obj();
                        eo.st("spid", &e.spid).n("delayMs", e.delay_ms).n("durMs", e.dur_ms);
                        eo.st("type", if e.kind == LegacyAnimType::Fade { "fade" } else { "appear" });
                        eo.boolean("clickGated", e.click_gated);
                        eo.done()
                    })
                    .collect(),
            ),
        );
    }
    if let Some(t) = &s.transition {
        let mut to = Json::obj();
        to.st("type", &t.kind).n("durMs", t.dur_ms).string("dir", t.dir.as_deref());
        o.put("transition", to.done());
    }
    o.done()
}

fn num_pair(p: (f64, f64)) -> Json {
    Json::Arr(vec![Json::Num(p.0), Json::Num(p.1)])
}

fn str_arr(v: &[Box<str>]) -> Json {
    Json::Arr(v.iter().map(|s| Json::s(s)).collect())
}

/// An animation target (`AnimTarget`).
pub fn target_json(t: &AnimTarget) -> Json {
    let mut o = Json::obj();
    o.st("spid", &t.spid);
    o.json("pRange", t.p_range.map(num_pair)).json("charRange", t.char_range.map(num_pair));
    o.string("sub", t.sub.as_deref());
    if let Some(c) = &t.chart {
        let mut co = Json::obj();
        co.n("series", c.series).n("category", c.category).st("step", &c.step);
        o.put("chart", co.done());
    }
    o.string("dgm", t.dgm.as_deref());
    o.done()
}

/// A time node and its whole subtree (`TimeNode`).
pub fn node_json(a: &TimeArena, id: NodeId) -> Json {
    let n = a.node(id);
    let mut o = Json::obj();
    o.st("tag", &n.tag).string("id", n.id.as_deref());
    o.put(
        "st",
        Json::Arr(
            n.st.iter()
                .map(|c| {
                    let mut co = Json::obj();
                    co.put("delayMs", c.delay_ms.map_or(Json::Null, Json::Num));
                    co.string("evt", c.evt.as_deref()).string("spid", c.spid.as_deref()).string("bookmark", c.bookmark.as_deref());
                    co.done()
                })
                .collect(),
        ),
    );
    o.num("dur", n.dur)
        .num("repeat", n.ctn_x().repeat)
        .boolean("autoRev", n.ctn_x().auto_rev)
        .num("accel", n.ctn_x().accel)
        .num("decel", n.ctn_x().decel);
    o.string("fill", n.fill.as_deref()).string("nodeType", n.node_type.as_deref()).string("presetClass", n.preset_class.as_deref());
    o.num("presetId", n.preset_id).num("presetSubtype", n.preset_subtype).string("grpId", n.grp_id.as_deref());
    if let Some(it) = &n.ctn_x().iterate {
        let mut io = Json::obj();
        io.st("type", &it.kind).put("backwards", Json::Bool(it.backwards)).num("pct", it.pct).num("absMs", it.abs_ms);
        o.put("iterate", io.done());
    }
    o.json("target", n.bhvr().target.as_ref().map(target_json));
    o.json("attrNames", n.bhvr().attr_names.as_deref().map(str_arr));
    o.string("filter", n.bhvr().filter.as_deref())
        .string("transition", n.bhvr().transition.as_deref())
        .string("setTo", n.bhvr().set_to.as_deref());
    o.string("path", n.bhvr_x().path.as_deref());
    o.json("tmFilter", n.ctn_x().tm_filter.as_ref().map(|v| Json::Arr(v.iter().map(|p| num_pair(*p)).collect())));
    o.num("spd", n.ctn_x().spd).string("additive", n.bhvr().additive.as_deref()).string("calcmode", n.bhvr().calcmode.as_deref());
    o.string("valueType", n.bhvr().value_type.as_deref());
    o.json(
        "tav",
        n.bhvr().tav.as_ref().map(|v| {
            Json::Arr(
                v.iter()
                    .map(|k| {
                        let mut ko = Json::obj();
                        ko.n("tm", k.tm);
                        ko.json(
                            "val",
                            k.val.as_ref().map(|x| match x {
                                TavValue::Num(n) => Json::Num(*n),
                                TavValue::Str(s) => Json::s(s),
                            }),
                        );
                        ko.string("fmla", k.fmla.as_deref()).color("color", k.color.as_ref());
                        ko.done()
                    })
                    .collect(),
            )
        }),
    );
    o.string("from", n.bhvr().from.as_deref()).string("to", n.bhvr().to.as_deref()).string("by", n.bhvr().by.as_deref());
    o.json("byXY", n.bhvr_x().by_xy.map(num_pair))
        .json("fromXY", n.bhvr_x().from_xy.map(num_pair))
        .json("toXY", n.bhvr_x().to_xy.map(num_pair));
    o.num("rotBy", n.bhvr_x().rot_by).num("rotFrom", n.bhvr_x().rot_from).num("rotTo", n.bhvr_x().rot_to);
    o.color("toColor", n.bhvr().to_color.as_ref()).color("fromColor", n.bhvr_x().from_color.as_ref());
    o.json("byHsl", n.bhvr_x().by_hsl.map(|(h, s, l)| Json::Arr(vec![Json::Num(h), Json::Num(s), Json::Num(l)])));
    o.string("clrSpc", n.bhvr_x().clr_spc.as_deref())
        .string("clrDir", n.bhvr_x().clr_dir.as_deref())
        .string("prLst", n.bhvr_x().pr_lst.as_deref());
    o.num("rAng", n.bhvr_x().r_ang)
        .string("origin", n.bhvr_x().origin.as_deref())
        .string("pathEditMode", n.bhvr_x().path_edit_mode.as_deref());
    o.json("endEvts", n.ctn_x().end_evts.as_deref().map(str_arr));
    o.string("cmd", n.bhvr_x().cmd.as_deref()).string("cmdType", n.bhvr_x().cmd_type.as_deref());
    if let Some(m) = &n.bhvr_x().media {
        let mut mo = Json::obj();
        mo.num("vol", m.vol).boolean("mute", m.mute).boolean("showWhenStopped", m.show_when_stopped);
        mo.num("numSld", m.num_sld).boolean("fullScrn", m.full_scrn);
        o.put("media", mo.done());
    }
    o.put("children", Json::Arr(a.kids(n).iter().map(|c| node_json(a, *c)).collect()));
    o.done()
}

fn effect_json(a: &TimeArena, e: &EffectTiming) -> Json {
    let mut o = Json::obj();
    o.string("id", e.id.as_deref()).st("presetClass", &e.preset_class).n("presetId", e.preset_id).n("presetSubtype", e.preset_subtype);
    o.string("nodeType", e.node_type.as_deref()).json("target", e.target.as_ref().map(target_json));
    o.n("startMs", e.start_ms).n("durMs", e.dur_ms).n("blockMs", e.block_ms);
    o.string("fill", e.fill.as_deref())
        .string("filter", e.filter.as_deref())
        .boolean("motion", e.motion)
        .boolean("endOnNext", e.end_on_next);
    o.put("node", node_json(a, e.node));
    o.done()
}

/// A transition (`SlideTransitionInfo`).
pub fn transition_json(t: &TransitionInfo) -> Json {
    let mut o = Json::obj();
    o.st("type", &t.kind).st("ns", &t.ns);
    o.put("attrs", Json::Obj(t.attrs.iter().map(|(k, v)| (k.to_string(), Json::s(v))).collect()));
    o.n("durMs", t.dur_ms).num("advTmMs", t.adv_tm_ms).put("advClick", Json::Bool(t.adv_click));
    o.done()
}

/// The `SlideTimingModel` of `parseSlideTiming`.
pub fn timing_json(t: &TimingModel) -> Json {
    let a = &t.nodes;
    let mut o = Json::obj();
    o.json("transition", t.transition.as_ref().map(transition_json));
    o.put(
        "steps",
        Json::Arr(
            t.steps
                .iter()
                .map(|s| {
                    let mut so = Json::obj();
                    so.put("auto", Json::Bool(s.auto)).n("durMs", s.dur_ms).put("hasIndefinite", Json::Bool(s.has_indefinite));
                    so.put("effects", Json::Arr(s.effects.iter().map(|e| effect_json(a, e)).collect()));
                    so.done()
                })
                .collect(),
        ),
    );
    o.put("interactive", Json::Arr(t.interactive.iter().map(|e| effect_json(a, e)).collect()));
    o.put("hidden", Json::Bool(t.hidden));
    o.json("animBg", t.anim_bg.as_deref().map(str_arr));
    o.json(
        "media",
        t.media.as_ref().map(|v| {
            Json::Arr(
                v.iter()
                    .map(|m| {
                        let mut mo = Json::obj();
                        mo.st("spid", &m.spid).st("kind", if m.kind == uo_dml_render::model::MediaKind::Video { "video" } else { "audio" });
                        mo.num("vol", m.vol).boolean("mute", m.mute).boolean("showWhenStopped", m.show_when_stopped);
                        mo.boolean("rewind", m.rewind).boolean("loop", m.loop_).boolean("fullScreen", m.full_screen);
                        mo.done()
                    })
                    .collect(),
            )
        }),
    );
    o.json(
        "bookmarkTriggers",
        t.bookmark_triggers.as_ref().map(|v| {
            Json::Arr(
                v.iter()
                    .map(|b| {
                        let mut bo = Json::obj();
                        bo.st("spid", &b.spid).st("bookmark", &b.bookmark).num("timeMs", b.time_ms).num("trimStartMs", b.trim_start_ms);
                        bo.put("effects", Json::Arr(b.effects.iter().map(|e| effect_json(a, e)).collect()));
                        bo.done()
                    })
                    .collect(),
            )
        }),
    );
    o.done()
}

/// `{"notes":<text>,"slide":<Slide>}` with `timing` (the `s<N>` / `model` facets, frontier B6).
pub fn slide_record_with_timing(c: &Canon<'_>, notes: &str, s: &SlideModel) -> String {
    let mut slide = slide_json(c, s);
    if let (Json::Obj(kv), Some(t)) = (&mut slide, &s.timing) {
        kv.push(("timing".to_string(), timing_json(t)));
    }
    let mut o = Json::obj();
    o.st("notes", notes).put("slide", slide);
    o.done().to_text()
}

/// `{"notes":<text>,"slide":<Slide>}`, the text of a slide record WITHOUT `timing` (`n<N>`).
pub fn slide_record(c: &Canon<'_>, notes: &str, s: &SlideModel) -> String {
    let mut o = Json::obj();
    o.st("notes", notes).put("slide", slide_json(c, s));
    o.done().to_text()
}

/// `len:sha256[0..32]` of a text (UTF-8): the form of the capture facets.
pub fn text_digest(text: &str) -> String {
    let d = uo_testkit::sha256::sha256(text.as_bytes());
    format!("{}:{}", text.len(), &uo_dml_render::canon::hex(&d)[..32])
}

/// `len:sha256[0..16]` of bytes (inputs).
pub fn bytes_digest(b: &[u8]) -> String {
    let d = uo_testkit::sha256::sha256(b);
    format!("{}:{}", b.len(), &uo_dml_render::canon::hex(&d)[..16])
}

/// `sha256[0..32]` over `pres` and each slide text joined by newlines (the `nmodel` facet).
pub fn model_hash(pres: &str, slides: &[String]) -> String {
    let mut buf = String::from(pres);
    for s in slides {
        buf.push('\n');
        buf.push_str(s);
    }
    uo_dml_render::canon::hex(&uo_testkit::sha256::sha256(buf.as_bytes()))[..32].to_string()
}
