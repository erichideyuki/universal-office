//! The chart host of `emit::chart` (frontier C-6): the host over a theme, the parsed-chart cache and the per-model bounds cache
//! (memory gate M5). The paint itself is compared with the frozen TS in `uo-pptx` (`drawops-samples`, `drawops-c1d`) and in
//! `uo-charts` (`tests/c6_tiles.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_charts::Canvas;
use uo_charts::emit::Rect;
use uo_charts::host::{ChartHost, ColorSource};
use uo_dml_render::emit::{
    ChartCache, Cx, EmitCaches, Env, MAX_BOUNDS_KEYS, MAX_CHART_ITEMS, MAX_CHART_MODELS, SlidesCanvas, SlidesHost, bounds_key, model_of,
};
use uo_dml_render::ids::MediaId;
use uo_dml_render::model::{ChartRef, EMU_PER_POINT};
use uo_dml_render::source::{PartSource, R};
use uo_dml_render::text::MeasureTable;
use uo_dml_render::theme::Theme;
use uo_opc::xml::{Document, ParseOptions, parse};
use uo_render::slides::{SItem, SlidesResources, ViewBox};

/// A package of charts: every part is the same small `c:chartSpace`.
struct Charts;

impl PartSource for Charts {
    fn part(&self, _: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        if !name.starts_with("ppt/charts/") {
            return Ok(None);
        }
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea/></c:chart></c:chartSpace>"#;
        Ok(parse(xml.to_owned(), &ParseOptions::LENIENT).ok().map(Arc::new))
    }
    fn media(&self, _: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

fn chart(n: usize) -> ChartRef {
    ChartRef { part: format!("ppt/charts/chart{n}.xml").into(), theme: Arc::new(Theme::default()), images: None, format: None }
}

#[test]
fn the_model_cache_is_capped_and_least_recently_used() {
    let mut cache = ChartCache::default();
    let src = Charts;
    for n in 0..(MAX_CHART_MODELS * 3) {
        assert!(model_of(&src, &mut cache, &chart(n)).is_some());
        assert!(cache.sizes().0 <= MAX_CHART_MODELS);
    }
    assert_eq!(cache.sizes().0, MAX_CHART_MODELS);
    // a hit returns the same model and keeps it past newer ones
    let first = chart(1000);
    let a = model_of(&src, &mut cache, &first).unwrap();
    for n in 0..(MAX_CHART_MODELS - 1) {
        model_of(&src, &mut cache, &chart(2000 + n)).unwrap();
    }
    let b = model_of(&src, &mut cache, &first).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "the entry was used last-but-one: still cached");
    // one more distinct chart evicts the least recently used (not `first`, touched just now)
    model_of(&src, &mut cache, &chart(3000)).unwrap();
    assert!(Arc::ptr_eq(&a, &model_of(&src, &mut cache, &first).unwrap()));
    cache.clear();
    assert_eq!(cache.sizes(), (0, 0));
}

#[test]
fn the_bounds_cache_is_capped_per_model_and_least_recently_used() {
    let mut cache = ChartCache::default();
    let src = Charts;
    let c = chart(1);
    model_of(&src, &mut cache, &c).unwrap();
    let key = uo_dml_render::emit::model_key(&c);
    let r = |i: f64| Rect { x: i, y: 0.0, w: 10.0, h: 5.0 };
    assert_eq!(cache.bounds_get(&key, "k"), None, "never asked");
    for i in 0..(MAX_BOUNDS_KEYS * 3) {
        cache.bounds_put(&key, &format!("k{i}"), if i % 2 == 0 { Some(r(i as f64)) } else { None });
        assert!(cache.sizes().1 <= MAX_BOUNDS_KEYS);
    }
    assert_eq!(cache.sizes().1, MAX_BOUNDS_KEYS);
    // the oldest are gone, the newest answer (`None` is an answer: "nothing is drawn")
    assert_eq!(cache.bounds_get(&key, "k0"), None);
    let last = MAX_BOUNDS_KEYS * 3 - 1;
    assert_eq!(cache.bounds_get(&key, &format!("k{last}")), Some(None));
    // a bounds put for a model that is not cached is dropped
    cache.bounds_put("no such model", "k", Some(r(1.0)));
    assert_eq!(cache.bounds_get("no such model", "k"), None);
}

#[test]
fn the_bounds_key_is_the_ts_string() {
    let r = Rect { x: 17.5, y: 9.25, w: 360.5, h: 240.25 };
    assert_eq!(bounds_key("s0", &r, 0.8833), "s0@17.50,9.25,360.50,240.25,0.8833");
    assert_eq!(bounds_key("bg", &Rect { x: 0.0, y: 0.0, w: 480.0, h: 300.0 }, 4.0 / 3.0), "bg@0.00,0.00,480.00,300.00,1.3333333333333333");
    assert!((EMU_PER_POINT - 12700.0).abs() < f64::EPSILON);
}

#[test]
fn the_host_resolves_colours_over_the_palette_under_the_theme() {
    let c = chart(1);
    let host = SlidesHost::new(&c);
    // a theme without colours: the Office palette of `FALLBACK`
    let doc = parse(r#"<a:schemeClr xmlns:a="x" val="accent2"/>"#.to_owned(), &ParseOptions::LENIENT).unwrap();
    assert_eq!(host.color(ColorSource::Element(doc.root())).unwrap().as_str(), "#ED7D31");
    assert_eq!(host.color(ColorSource::Scheme("tx1")).unwrap().as_str(), "#000000");
    assert_eq!(host.color(ColorSource::Scheme("bg1")).unwrap().as_str(), "#FFFFFF");
    // a name nobody defines is black, as `colorFromDml`'s lookup falls through
    assert_eq!(host.color(ColorSource::Scheme("nope")).unwrap().as_str(), "#000000");
    let srgb = parse(r#"<a:srgbClr xmlns:a="x" val="102030"/>"#.to_owned(), &ParseOptions::LENIENT).unwrap();
    assert_eq!(host.color(ColorSource::Element(srgb.root())).unwrap().as_str(), "#102030");
    assert!(host.fill_style(1, "#FF0000").is_none(), "no theme styles: the fill style is absent");
    assert!(host.theme_line_widths().is_empty() && host.user_shapes().is_none());
}

#[test]
fn a_hostile_chart_is_capped_and_the_list_stays_balanced() {
    let table = MeasureTable::default();
    let env = Env { src: &Charts, table: &table, rules: uo_dml_render::SlidesRules::ts_strict() };
    let mut res = SlidesResources::new();
    let mut caches = EmitCaches::new();
    let mut cx = Cx::new(&env, &mut res, &mut caches, ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: 100.0, sh: 100.0 });
    let mut canvas = SlidesCanvas::new(&mut cx);
    // an unbalanced save (the emit threw in the middle), then far more calls than the cap allows
    canvas.save();
    for i in 0..(MAX_CHART_ITEMS + 50_000) {
        canvas.save();
        canvas.fill_rect(i as f64, 0.0, 1.0, 1.0);
        canvas.restore();
    }
    canvas.save();
    assert!(canvas.finish(), "the cap was hit");
    let saves = cx.list.items.iter().filter(|i| matches!(i, SItem::Save)).count();
    let restores = cx.list.items.iter().filter(|i| matches!(i, SItem::Restore)).count();
    assert_eq!(saves, restores, "every save the list holds is restored");
    assert!(cx.list.items.len() <= MAX_CHART_ITEMS + 8, "the chart's items stop at the cap ({})", cx.list.items.len());
}
