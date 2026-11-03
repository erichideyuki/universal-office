//! P3 unit facets of frontier F3 (record "Wave 1 — F3"): every call of `renderEmf`, `renderWmf`,
//! `pictureLook`, `resolvePageFill`, `resolveTextFx` and `shapeExtras` the frozen engine made while
//! laying out a corpus, replayed alone on its dumped inputs (`<key>.f3.jsonl`, written by
//! `tools/capture-layout.mjs --dump` through tools/lib/facets-f3.mjs) and compared exactly (the
//! per-call canonical stream). Before the D1a reader reaches digest parity this is how F3 is
//! compared (LQ22); afterwards the same facets come from `layout_parity`.
//!
//! Always: the committed dumps of `tests/captures/f3` (the .NET synthetic inputs: pictures, page
//! fills, text effects, textboxes, shapes). Opt-in: `UO_F3_DUMP=<dir>[;<dir>…]` (e.g. the oracle
//! corpus, regenerated in the same run on the same machine, LQ21) and `UO_F3_CAPTURE=<capture>`
//! to also check the per-document `p3.*` values of the capture against the replayed calls.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod f3_support;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{FontIndex, Transcript};
use f3_support::*;
use uo_docx_layout::drawing::{ResWriter, picture_look, resolve_page_fill, resolve_text_fx, shape_extras, shape_geom};
use uo_docx_layout::items::PassItems;
use uo_docx_layout::metafile::{MetafileFrame, MetafileLimits, render_emf, render_wmf};
use uo_render::types::{Blip, ImageItem, ShapeItem};
use uo_render::{GlyphRunHead, ImageRes, Item, OptF64, Resources};

#[derive(Default)]
struct Tally {
    calls: BTreeMap<&'static str, (usize, usize)>,
    mismatches: Vec<String>,
    docs: usize,
}

fn facet_name(f: &str) -> &'static str {
    match f {
        "emf" => "p3.emf",
        "wmf" => "p3.wmf",
        "picture" => "p3.picture",
        "pagefill" => "p3.pagefill",
        "textfx" => "p3.textfx",
        _ => "p3.shape",
    }
}

/// One call replayed: its value.
fn replay_call(o: &J, media: &mut Media, theme: &uo_docx::model::formatting::Theme, faces: &mut DumpFaces<'_>) -> String {
    let mut res = Resources::new();
    let mut origins = Vec::new();
    let f = o.get("f").and_then(J::str).unwrap_or("");
    let r: Result<uo_docx_layout::Layout, uo_docx_layout::error::LayoutError> = (|| match f {
        "emf" | "wmf" => {
            let bytes = match o.get("hex").and_then(J::str) {
                Some(h) => unhex(h),
                None => media.by_sha.get(o.get("bytes").and_then(J::str).unwrap_or("")).cloned().unwrap_or_default(),
            };
            let d = o.get("dest").map(J::arr).unwrap_or(&[]);
            let n = |i: usize| d.get(i).map_or(f64::NAN, |v| jn(v).to_number());
            let dest = MetafileFrame { x: n(0), y: n(1), w: n(2), h: n(3) };
            let mut out = PassItems::new();
            {
                let mut w = ResWriter::new(&mut res, &mut origins);
                if f == "emf" {
                    render_emf(&bytes, dest, MetafileLimits::default(), faces, &mut w, &mut out)?;
                } else {
                    render_wmf(&bytes, dest, MetafileLimits::default(), faces, &mut w, &mut out)?;
                }
            }
            Ok(items_page(res, out))
        }
        "picture" => {
            let im = inline_image(o.get("im").unwrap_or(&J::Null), media);
            let image = empty_image(&mut res);
            let pic = picture_look(&im, theme, &mut ResWriter::new(&mut res, &mut origins))?;
            let mut l = one_page(res);
            l.pages[0].page.list.push_plain(Item::Image(Box::new(ImageItem {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                image,
                crop: None,
                rot: OptF64::NONE,
                flip_h: false,
                flip_v: false,
                pic: pic.map(Box::new),
            })));
            Ok(l)
        }
        "pagefill" => {
            let bg = page_background(o.get("bg").unwrap_or(&J::Null), media);
            let pc = o.get("pageColor").and_then(J::str).map(str::to_owned);
            let mut warn = |_: &str| {};
            let pf = resolve_page_fill(&bg, pc.as_deref(), &media.store, &mut ResWriter::new(&mut res, &mut origins), &mut warn)?;
            let id = pf.map(|p| res.add_page_fill(p).expect("fill"));
            let mut l = one_page(res);
            l.pages[0].page.background_fill = id;
            Ok(l)
        }
        "textfx" => {
            let fx = o.get("fx").filter(|v| !v.is_null()).map(text_effects);
            let text: Vec<u16> = o.get("text").and_then(J::str).unwrap_or("").encode_utf16().collect();
            let face = empty_face(&mut res);
            let r = resolve_text_fx(fx.as_ref(), theme, &text, &mut ResWriter::new(&mut res, &mut origins))?;
            let id = r.map(|x| res.add_text_fx(x).expect("fx"));
            let color = res.symbols.intern_str("").expect("sym");
            let mut l = one_page(res);
            let head = GlyphRunHead {
                face,
                color,
                size_pt: 0.0,
                y: 0.0,
                text: &[],
                scale_x: OptF64::NONE,
                alpha: OptF64::NONE,
                fx: id,
                flags: Default::default(),
            };
            l.pages[0].page.list.push_glyphs(head, std::iter::empty())?;
            Ok(l)
        }
        _ => {
            let s = shape_style(o.get("s").unwrap_or(&J::Null), media);
            let (geom, ex) = {
                let mut w = ResWriter::new(&mut res, &mut origins);
                (shape_geom(&s, &mut w)?, shape_extras(&s, theme, &mut w)?)
            };
            let blip = match ex.blip {
                Some(b) => {
                    let bytes: Arc<[u8]> = media.store.get_arc(b.image.data.0).cloned().unwrap_or_else(|| Arc::from(&[][..]));
                    let ext = res.symbols.intern_str(&b.image.ext).expect("sym");
                    // a blip's image is the model `{ data, ext }` object: no size (the TS walker
                    // writes `num(undefined)`, NaN)
                    let image = res
                        .add_image(ImageRes {
                            bytes,
                            ext,
                            svg: None,
                            effects: None,
                            width_twips: f64::NAN,
                            height_twips: f64::NAN,
                            natural_px: None,
                        })
                        .expect("image");
                    Some(Blip { image, tile: b.tile, alpha: b.alpha })
                }
                None => None,
            };
            let mut l = one_page(res);
            l.pages[0].page.list.push_plain(Item::Shape(Box::new(ShapeItem {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                geom,
                fill: None,
                stroke: None,
                line: None,
                grad: ex.grad,
                shadow: ex.shadow,
                pattern: ex.pattern,
                blip,
                fx: ex.fx,
            })));
            Ok(l)
        }
    })();
    match r {
        Ok(l) => value_of(&l),
        // the TS threw (`v` is `error`; the message is shown, not compared)
        Err(e) => format!("error ({e})"),
    }
}

fn replay_dir(dir: &Path, fonts: &mut FontIndex, caps: &[uo_testkit::Capture], t: &mut Tally) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".f3.jsonl"))
        .collect();
    files.sort();
    for p in files {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let key = name.trim_end_matches(".f3.jsonl").to_owned();
        if let Ok(tr) = Transcript::read(&dir.join(format!("{key}.transcript.tsv"))) {
            fonts.add_transcript_sources(&tr);
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let mut media = Media::default();
        let mut theme = uo_docx::model::formatting::Theme::default();
        let mut answers: HashMap<String, FaceAnswer> = HashMap::new();
        let mut per_facet: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        t.docs += 1;
        for (n, line) in text.lines().enumerate() {
            let o = parse(line).unwrap_or_else(|e| panic!("{name}:{}: {e}", n + 1));
            match o.get("f").and_then(J::str).unwrap_or("") {
                "media" => {
                    media.by_sha.insert(rs(&o, "sha").into(), unhex(o.get("hex").and_then(J::str).unwrap_or("")));
                }
                "theme" => theme = f3_support::theme(o.get("theme").unwrap_or(&J::Null), &mut media),
                "note" => {}
                "face" => {
                    answers.insert(rs(&o, "req").into(), FaceAnswer::of(&o));
                }
                f => {
                    let get = |h: &str| fonts.get(h);
                    let mut faces =
                        DumpFaces { answers: &answers, bytes: &get, ids: HashMap::new(), warnings: Vec::new(), missing: Vec::new() };
                    let v = replay_call(&o, &mut media, &theme, &mut faces);
                    let want = o.get("v").and_then(J::str).unwrap_or("");
                    let facet = facet_name(f);
                    let e = t.calls.entry(facet).or_default();
                    e.0 += 1;
                    if v == want || (want == "error" && v.starts_with("error")) {
                        e.1 += 1;
                    } else if t.mismatches.len() < 40 {
                        t.mismatches.push(format!(
                            "{key} line {}: {facet} rust {v} ts {want} {}",
                            n + 1,
                            if faces.missing.is_empty() { String::new() } else { format!("(faces missing: {:?})", faces.missing) }
                        ));
                    }
                    per_facet.entry(facet).or_default().push(want.to_owned());
                }
            }
        }
        // the dump's calls are the capture's: their aggregate is the capture value
        for cap in caps {
            for (facet, vals) in &per_facet {
                if let Some(c) = cap
                    .get(&key, facet)
                    .or_else(|| cap.records().iter().find(|r| r.key.replace(['/', '\\', ':'], "__") == key).and_then(|r| r.get(facet)))
                {
                    let a = aggregate(vals);
                    if a != c && t.mismatches.len() < 40 {
                        t.mismatches.push(format!("{key}: {facet} aggregate {a} capture {c}"));
                    }
                }
            }
        }
    }
}

fn report(t: &Tally) {
    for (f, (n, ok)) in &t.calls {
        eprintln!("{f}: {ok}/{n} calls equal");
    }
    assert!(t.mismatches.is_empty(), "{} mismatches:\n{}", t.mismatches.len(), t.mismatches.join("\n"));
}

#[test]
fn f3_facets_committed_dumps() {
    let dir = common::manifest().join("tests/captures/f3");
    if !dir.exists() {
        return;
    }
    let mut fonts = FontIndex::open_fonts();
    let mut t = Tally::default();
    replay_dir(&dir, &mut fonts, &[], &mut t);
    report(&t);
    assert!(t.docs > 0);
}

#[test]
fn f3_facets_dumps_of_a_capture_run() {
    let Ok(dirs) = std::env::var("UO_F3_DUMP") else {
        eprintln!("skipped: set UO_F3_DUMP=<dump dir>[;…] (and UO_F3_CAPTURE) for the corpus facets");
        return;
    };
    let caps: Vec<uo_testkit::Capture> =
        std::env::var("UO_F3_CAPTURE").map(|c| c.split(';').map(|p| uo_testkit::Capture::read(p).unwrap()).collect()).unwrap_or_default();
    let mut fonts = FontIndex::open_fonts();
    let mut t = Tally::default();
    for d in dirs.split(';') {
        replay_dir(Path::new(d), &mut fonts, &caps, &mut t);
    }
    eprintln!("{} documents", t.docs);
    report(&t);
}
