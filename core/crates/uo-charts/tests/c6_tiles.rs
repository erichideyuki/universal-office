//! C-6: the 261 `ops` facets of `charts-c1a` that [`RecCanvas`] cannot print (the charts that paint a picture TILE, whose pattern the
//! shell painter builds) go through the product path: the engine paints into a Slides display list
//! (`uo_dml_render::emit::emit_chart_with`), the list is encoded, and `js/compare-tiles.mjs` paints it with the TS interpreter
//! (`paint-slides.ts`) over the recording context and compares the draw-form digest with the capture's facet.
//!
//! `UO_C6_TILES_OUT=<dir>` writes one frame file per facet (`<key with / as __>.<view>.<sel>.uolc`: `"UOLC"`, u32 canvas width, u32
//! canvas height, u32 list length, the list message) and `index.tsv` (`key TAB view TAB sel TAB emit result`); without it the test
//! only checks that every such facet emits, validates and encodes. Run:
//!
//! ```text
//! UO_C6_TILES_OUT=D:/tmp/c6tiles cargo test -p uo-charts --test c6_tiles -- --nocapture
//! node core/crates/uo-charts/js/compare-tiles.mjs --frames D:/tmp/c6tiles
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

mod support;

use std::fmt::Write as _;
use std::path::PathBuf;

use support::data::{load_data, load_measures, read_capture};
use support::stages::{self, mm, params, setup};
use uo_dml_render::emit::{EmitCaches, Env, emit_chart_with};
use uo_dml_render::ids::MediaId;
use uo_dml_render::source::{PartSource, R};
use uo_dml_render::text::MeasureTable;
use uo_render::slides::{Media, PxSize, SlidesEncoder, SlidesResources, ViewBox, validate};

/// No package: the chart's pictures are media rows of the resources and the answers come from the capture.
struct NoSource;

impl PartSource for NoSource {
    fn part(&self, _: &str) -> Option<std::sync::Arc<[u8]>> {
        None
    }
    fn xml(&self, _: &str) -> R<Option<std::sync::Arc<uo_opc::xml::Document>>> {
        Ok(None)
    }
    fn media(&self, _: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _: MediaId) -> Option<(Box<str>, std::sync::Arc<[u8]>)> {
        None
    }
}

#[test]
fn tile_facets_through_the_slides_list() {
    let cap = read_capture("charts-c1a");
    let data = load_data("c1a");
    let table = load_measures("c1a");
    let by_key: std::collections::HashMap<&str, _> = data.iter().map(|c| (c.key.as_str(), c)).collect();
    let out_dir = std::env::var("UO_C6_TILES_OUT").ok().map(PathBuf::from);
    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let (mut facets, mut emitted, mut thrown) = (0, 0, 0);
    let mut index = String::new();
    for rec in cap.records() {
        let Some(chart) = by_key.get(rec.key.as_str()).copied() else { continue };
        for (facet, _) in &rec.facets {
            let Some(rest) = facet.strip_prefix("ops.") else { continue };
            let (v, s) = rest.split_once('.').unwrap_or((rest, "full"));
            let view = stages::view(v);
            // the facets RecCanvas skips: the stream of a chart that paints a tile
            let Some(rc) = stages::ops(chart, &table, &view, s) else { continue };
            if !rc.unsupported {
                continue;
            }
            facets += 1;
            let (host, cs) = setup(chart).expect("the chart parses");
            let m = mm(chart, &table);
            let p = params(chart, &view, s, true);
            let mt = MeasureTable::default();
            let env = Env { src: &NoSource, table: &mt };
            let mut res = SlidesResources::new();
            let mut caches = EmitCaches::new();
            let (cw, ch) = if v == "a" { (480.0, 300.0) } else { (400.0, 270.0) };
            let mut cx =
                uo_dml_render::emit::Cx::new(&env, &mut res, &mut caches, ViewBox { ox: 0.0, oy: 0.0, scale: 1.0, sw: cw, sh: ch });
            // `BlipKey(i)` is the index of the chart's picture: media row i
            let sym = |cx: &mut uo_dml_render::emit::Cx<'_>, s: &str| cx.sym(s);
            for (i, im) in chart.images.iter().enumerate() {
                let part = sym(&mut cx, &format!("chart-image-{i}"));
                let ext = sym(&mut cx, "png");
                let row = Media { part, ext, byte_len: 0, has_svg: false, natural: Some(PxSize { w: im.w as u32, h: im.h as u32 }) };
                let id = cx.res.media(row).expect("media row");
                assert_eq!(usize::try_from(id).unwrap(), i, "media row i is picture i");
            }
            let r = emit_chart_with(&mut cx, &cs, &host, &p, &m);
            let list = cx.list;
            let result = match &r {
                Ok(()) => "ok".to_owned(),
                Err(e) => {
                    thrown += 1;
                    format!("err:{e:?}")
                }
            };
            if r.is_ok() {
                emitted += 1;
                validate(&list, &res.lens()).expect("the list validates");
                let mut msg = Vec::new();
                SlidesEncoder::new(1).encode(&list, &res, &mut msg).expect("the list encodes");
                if let Some(d) = &out_dir {
                    let mut f = b"UOLC".to_vec();
                    f.extend_from_slice(&(cw as u32).to_le_bytes());
                    f.extend_from_slice(&(ch as u32).to_le_bytes());
                    f.extend_from_slice(&u32::try_from(msg.len()).unwrap().to_le_bytes());
                    f.extend_from_slice(&msg);
                    let name = format!("{}.{v}.{s}.uolc", rec.key.replace('/', "__"));
                    std::fs::write(d.join(name), f).unwrap();
                }
            }
            let _ = writeln!(index, "{}\t{v}\t{s}\t{result}", rec.key);
        }
    }
    if let Some(d) = &out_dir {
        std::fs::write(d.join("index.tsv"), index).unwrap();
    }
    eprintln!("c6_tiles: {facets} tile facets, {emitted} emitted, {thrown} threw");
    assert!(facets >= 261, "the 261 skipped tile facets are found ({facets})");
}
