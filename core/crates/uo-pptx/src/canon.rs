//! The canonical JSON of the deck model (feature `canon`): the text of the model captures
//! (`js/capture-model.mjs`; rules in `core/docs/modules/uo-pptx-captures.md`, printer of the shape
//! types in `uo_dml_render::canon`).
//!
//! `timing` is not printed: `parseSlideTiming` is frontier B6, and the capture has the facets
//! `n<N>` / `nmodel` for the slide without `timing`.

use uo_dml_render::canon::{Canon, Json};

use crate::model::{LegacyAnimType, Presentation, SlideModel};

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

/// `{"notes":<text>,"slide":<Slide>}`, the text of a slide record (`s<N>` / `n<N>`).
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
