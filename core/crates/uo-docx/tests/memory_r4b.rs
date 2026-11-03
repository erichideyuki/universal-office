//! Memory gate M1 of frontier R4b: every public function of `read::inline_objects` and
//! `read::picture_format` returns the live bytes exactly to their value before the call, output
//! (and the host with its media) dropped, on the elements of the quirk corpus: the object runs of
//! every `w:drawing`/`w:pict`/`w:object`/`mc:AlternateContent` (including the ones that read
//! nothing and the ones that raise: a SmartArt colour index, the generated text of a VML box that
//! does not parse), the picture formats, the sync functions, and the JSON of the results.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/r4b_host.rs"]
mod r4b_host;

use std::hint::black_box;

use r4b_host::{QuirkHost, empty_package, quirk_cases};
use uo_alloc::assert_no_leak;
use uo_docx::js::JsonWriter;
use uo_docx::model::picture::{BlipLum, PicBlipFx};
use uo_docx::read::DOCX;
use uo_docx::read::inline_objects::{
    blip_fx_image_effects, read_drawing_run, read_object_runs, read_vml_runs, sync_image_from_xml, sync_shape_from_xml,
};
use uo_docx::read::picture_format::{
    parse_drawing, parts_of, read_3d, read_blip, read_dml_color, read_effects, read_pic_line, read_picture_format,
};
use uo_opc::xml::parse;

const TARGETS: [&str; 4] = ["w:drawing", "w:pict", "w:object", "mc:AlternateContent"];

#[test]
fn m1_readers_over_the_quirk_corpus() {
    let pkg = empty_package();
    let cases = quirk_cases();
    assert!(cases.len() > 2000);
    let (mut runs_n, mut errors, mut formats, mut synced) = (0, 0, 0, 0);
    for (_, frag, parts) in &cases {
        let doc = parse(frag.as_str(), &DOCX).unwrap();
        let root = doc.root();
        for el in std::iter::once(root).chain(root.descendants()) {
            if !TARGETS.contains(&el.name()) {
                continue;
            }
            assert_no_leak(|| {
                let mut host = QuirkHost::new(&pkg, parts.clone());
                black_box(read_object_runs(el, &mut host))
            });
            assert_no_leak(|| {
                let mut host = QuirkHost::new(&pkg, parts.clone());
                let r = read_object_runs(el, &mut host);
                if let Ok(runs) = &r {
                    for run in runs {
                        for v in [
                            run.image.as_deref().map(|x| x as &dyn uo_docx::js::JsValue),
                            run.textbox.as_deref().map(|x| x as &dyn uo_docx::js::JsValue),
                            run.shape.as_deref().map(|x| x as &dyn uo_docx::js::JsValue),
                            run.group.as_deref().map(|x| x as &dyn uo_docx::js::JsValue),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            black_box(JsonWriter::stringify(v, Some(&host.ctx.media)));
                        }
                    }
                }
                black_box(r)
            });
            let mut host = QuirkHost::new(&pkg, parts.clone());
            match read_object_runs(el, &mut host) {
                Ok(runs) => runs_n += runs.len(),
                Err(_) => errors += 1,
            }
            if el.name() == "w:drawing" {
                let raw = el.raw().unwrap_or("");
                assert_no_leak(|| {
                    let mut host = QuirkHost::new(&pkg, parts.clone());
                    black_box(read_drawing_run(el, el.raw(), &mut host))
                });
                assert_no_leak(|| black_box(read_picture_format(raw, false)));
                assert_no_leak(|| black_box(read_picture_format(raw, true)));
                assert_no_leak(|| black_box(parse_drawing(raw)));
                assert_no_leak(|| {
                    let d = parse_drawing(raw);
                    black_box(d.as_ref().and_then(parts_of).map(|p| p.container.name().len()))
                });
                formats += usize::from(read_picture_format(raw, true).is_some());
                // the sync functions: read, edit the raw text, sync
                assert_no_leak(|| {
                    let mut host = QuirkHost::new(&pkg, parts.clone());
                    if let Ok(mut runs) = read_object_runs(el, &mut host) {
                        for run in &mut runs {
                            let edited = raw.replacen("<wp:inline", "<wp:anchor", 1).replacen("</wp:inline>", "</wp:anchor>", 1);
                            if let Some(i) = run.image.as_mut() {
                                i.raw_drawing_xml = Some(edited.as_str().into());
                                sync_image_from_xml(i);
                            } else {
                                if let Some(t) = run.textbox.as_mut() {
                                    t.raw_drawing_xml = Some(edited.as_str().into());
                                } else if let Some(s) = run.shape.as_mut() {
                                    s.raw_drawing_xml = Some(edited.as_str().into());
                                } else if let Some(g) = run.group.as_mut() {
                                    g.raw_drawing_xml = Some(edited.as_str().into());
                                }
                                sync_shape_from_xml(run);
                            }
                        }
                        black_box(runs.len())
                    } else {
                        0
                    }
                });
                synced += 1;
            }
            if matches!(el.name(), "w:pict" | "w:object") {
                assert_no_leak(|| {
                    let mut host = QuirkHost::new(&pkg, parts.clone());
                    black_box(read_vml_runs(el, el.name() == "w:object", &mut host))
                });
            }
        }
        // the picture-xml pieces on any element of the fragment
        for el in std::iter::once(root).chain(root.descendants()) {
            assert_no_leak(|| black_box(read_dml_color(Some(el))));
            assert_no_leak(|| black_box(read_pic_line(Some(el))));
            assert_no_leak(|| black_box(read_effects(Some(el))));
            assert_no_leak(|| black_box(read_blip(Some(el))));
            assert_no_leak(|| black_box(read_3d(Some(el), el.first_child("a:sp3d"))));
        }
    }
    assert!(runs_n > 1000 && errors > 20 && formats > 100 && synced > 300, "{runs_n} {errors} {formats} {synced}");
}

#[test]
fn m1_blip_effects() {
    for lum in [None, Some((None, None)), Some((Some(10000.0), Some(20000.0))), Some((Some(f64::NAN), Some(-1e300)))] {
        for gray in [None, Some(true)] {
            let fx = PicBlipFx {
                lum: lum.map(|(b, c): (Option<f64>, Option<f64>)| BlipLum {
                    bright: b.map(uo_docx::js::JsNumber::Num),
                    contrast: c.map(uo_docx::js::JsNumber::Num),
                    ..BlipLum::default()
                }),
                grayscl: gray,
                bi_level: Some(uo_docx::js::JsNumber::Num(25000.0)),
                ..PicBlipFx::default()
            };
            assert_no_leak(|| black_box(blip_fx_image_effects(&fx)));
        }
    }
}
