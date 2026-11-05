//! Parity of the W5 write halves (`write::emit::background`, docx-core `page-background.ts` and
//! `watermark.ts`): replays `tests/parity/page-background.capture`, written by
//! `js/capture-page-background.mjs` from the frozen TS over the corpus backgrounds and seeded
//! inputs (`tests/parity/page-background.inputs`).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-page-background.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::JsonValue;
use uo_docx::model::media::{ImageData, MediaRef, MediaStore};
use uo_docx::read::DOCX;
use uo_docx::read::json_parse;
use uo_docx::read::page_background::read_page_background;
use uo_docx::write::emit::background::{
    BackgroundAdded, WORD_PATTERNS, WatermarkBox, WatermarkIds, WatermarkLayout, WatermarkPictureSpec, WatermarkSpec, WatermarkTextSpec,
    auto_text_box, explicit_text_box, one_color_gradient_color2, page_background_xml, pattern_gif, picture_box, resolve_color2,
    vml_color_attr, watermark_gallery, watermark_run_xml, watermark_texts, write_page_background,
};
use uo_jsnum::format::to_js_string;
use uo_jsnum::parse::to_number;
use uo_jsnum::text::utf16_len;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::xml::parse;
use uo_testkit::{Capture, Digest, Replay};

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn sha(s: &str) -> String {
    Digest::of(s.as_bytes()).short()
}

fn num(s: &str) -> f64 {
    to_number(s)
}

fn opt_s(f: &str) -> Option<&str> {
    f.strip_prefix('s')
}

fn opt_n(f: &str) -> Option<f64> {
    f.strip_prefix('n').map(num)
}

fn boxs(b: WatermarkBox) -> String {
    format!("{},{}", to_js_string(b.width_pt), to_js_string(b.height_pt))
}

fn layout(s: &str) -> WatermarkLayout {
    if s == "diagonal" { WatermarkLayout::Diagonal } else { WatermarkLayout::Horizontal }
}

fn bg_out(xml: &str) -> String {
    let Ok(doc) = parse(xml, &DOCX) else { return "err".to_owned() };
    let mut media = MediaStore::new();
    let bg = read_page_background(doc.root(), &mut |rid| {
        utf16_len(rid).is_multiple_of(2).then(|| {
            let id = media.add(format!("img:{rid}").as_bytes());
            ImageData { data: MediaRef(id), ext: "png".into(), ..ImageData::default() }
        })
    });
    match bg {
        None => "undefined".to_owned(),
        Some(bg) => {
            let rid = bg.fill.as_ref().is_some_and(|f| f.image.is_some()).then_some("rId9");
            sha(&page_background_xml(&bg, rid))
        }
    }
}

fn wm_text(f: &[&str]) -> String {
    let [text, font, size, color, semi, lay, w, h, n, spid, anchor, custom] = f else { panic!("fields") };
    let spec = WatermarkSpec::Text(WatermarkTextSpec {
        text: (*text).to_owned(),
        font: (*font).to_owned(),
        size_pt: (*size != "u").then(|| num(size)),
        color: (*color).to_owned(),
        semitransparent: *semi == "1",
        layout: layout(lay),
        lang: None,
    });
    let ids = WatermarkIds { n: num(n), spid: num(spid), anchor_id: (*anchor).to_owned() };
    let b = WatermarkBox { width_pt: num(w), height_pt: num(h) };
    let custom_fn = |h: &str| format!("vml:{h}");
    sha(&watermark_run_xml(&spec, b, &ids, None, (*custom == "1").then_some(&custom_fn as &dyn Fn(&str) -> String)))
}

fn wm_pic(f: &[&str]) -> String {
    let [title, washout, w, h, n, spid, anchor, rid] = f else { panic!("fields") };
    let spec = WatermarkSpec::Picture(WatermarkPictureSpec {
        data: std::sync::Arc::from(vec![1u8, 2, 3]),
        ext: "png".to_owned(),
        title: (*title).to_owned(),
        scale: None,
        washout: *washout == "1",
    });
    let ids = WatermarkIds { n: num(n), spid: num(spid), anchor_id: (*anchor).to_owned() };
    let b = WatermarkBox { width_pt: num(w), height_pt: num(h) };
    sha(&watermark_run_xml(&spec, b, &ids, opt_s(rid), None))
}

fn base_package(parts: &[(String, Vec<u8>)]) -> Package {
    const CT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>";
    let base: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), CT.as_bytes().to_vec()),
        ("_rels/.rels".into(), RELS.as_bytes().to_vec()),
        ("word/document.xml".into(), DOC.as_bytes().to_vec()),
    ];
    let mut pkg = Package::from_parts(base, &uo_opc::Limits::DEFAULT, &OpenOptions::default());
    for (p, d) in parts {
        pkg.set_part(p.clone(), d.clone());
    }
    pkg
}

fn text_of(v: &JsonValue) -> String {
    match v {
        JsonValue::Str(s) => s.to_string_lossy(),
        other => panic!("not a string: {other:?}"),
    }
}

/// One scenario of `wr`: the facets `added<j>` and `state<j>` per call.
fn run_scenario(json: &str, dump: bool) -> Vec<(String, String)> {
    let JsonValue::Object(root) = json_parse::parse(json).unwrap() else { panic!("object") };
    let JsonValue::Array(parts) = root.get("parts").unwrap() else { panic!("parts") };
    let parts: Vec<(String, Vec<u8>)> = parts
        .iter()
        .map(|p| {
            let JsonValue::Array(pair) = p else { panic!("pair") };
            (text_of(&pair[0]), text_of(&pair[1]).into_bytes())
        })
        .collect();
    let JsonValue::Array(ops) = root.get("ops").unwrap() else { panic!("ops") };
    let mut pkg = base_package(&parts);
    let mut res = Vec::new();
    let mut prev: Option<BackgroundAdded> = None;
    for (j, op) in ops.iter().enumerate() {
        let JsonValue::Object(op) = op else { panic!("op") };
        let ext = text_of(op.get("ext").unwrap());
        let use_prev = matches!(op.get("prev"), Some(JsonValue::Bool(true)));
        let mut media = MediaStore::new();
        let bg = match op.get("bg").unwrap() {
            JsonValue::Null => None,
            v => {
                let doc = parse(text_of(v), &DOCX).unwrap();
                read_page_background(doc.root(), &mut |_| {
                    let id = media.add(format!("DATA-{j}").as_bytes());
                    Some(ImageData { data: MediaRef(id), ext: ext.as_str().into(), ..ImageData::default() })
                })
            }
        };
        match write_page_background(&mut pkg, &media, bg.as_ref(), if use_prev { prev.as_ref() } else { None }) {
            Err(_) => {
                res.push((format!("added{j}"), "err".to_owned()));
                res.push((format!("state{j}"), "err".to_owned()));
            }
            Ok(added) => {
                res.push((format!("added{j}"), added.as_ref().map_or_else(|| "-".to_owned(), |a| format!("{}|{}", a.rid, a.media))));
                let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
                if dump {
                    println!("ADDED {added:?}");
                    for n in &names {
                        println!("PART {n} {:?}", String::from_utf8_lossy(pkg.part(n).unwrap().unwrap()));
                    }
                }
                let state: Vec<String> =
                    names.iter().map(|n| format!("{n}\0{}", Digest::of(pkg.part(n).unwrap().unwrap()).short())).collect();
                res.push((format!("state{j}"), sha(&state.join("\n"))));
                prev = added;
            }
        }
    }
    res
}

#[test]
fn page_background_and_watermark_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("page-background.capture")).unwrap();
    let inputs = std::fs::read_to_string(dir.join("page-background.inputs")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for line in inputs.lines().filter(|l| !l.starts_with('#')) {
        let fields: Vec<String> = line.split('\t').map(unescape).collect();
        let kind = fields[0].as_str();
        let i = counts.entry(kind.to_owned()).or_default();
        let key = format!("{kind}#{i}");
        *i += 1;
        let f: Vec<&str> = fields[1..].iter().map(String::as_str).collect();
        match kind {
            "bg" => {
                replay.check(&key, "out", &bg_out(f[0]));
            }
            "vca" => {
                replay.check(&key, "out", &vml_color_attr(f[0]));
            }
            "ocg" => {
                replay.check(&key, "out", &one_color_gradient_color2(num(f[0])));
            }
            "rc2" => {
                replay.check(&key, "out", &resolve_color2(opt_s(f[0]), f[1]));
            }
            "gif" => {
                let rows: Vec<f64> = f[0].split(',').filter(|s| !s.is_empty()).map(num).collect();
                let bytes = pattern_gif(&rows);
                replay.check(&key, "out", &Digest::of(&bytes).short());
                replay.check(&key, "len", &bytes.len().to_string());
            }
            "pat" => {
                let s: Vec<String> =
                    WORD_PATTERNS.iter().map(|p| format!("{}|{}|{}|{}", p.prst, p.title.pt, p.title.en, p.title.es)).collect();
                replay.check(&key, "out", &sha(&s.join("\n")));
            }
            "wm" => {
                let out = if f[0] == "text" { wm_text(&f[1..]) } else { wm_pic(&f[1..]) };
                replay.check(&key, "out", &out);
            }
            "box" => {
                let out = match f[0] {
                    "auto" => boxs(auto_text_box(f[1], layout(f[2]), num(f[3]))),
                    "exp" => boxs(explicit_text_box(num(f[1]), num(f[2]), num(f[3]))),
                    _ => boxs(picture_box(num(f[1]), num(f[2]), opt_n(f[3]), num(f[4]))),
                };
                replay.check(&key, "out", &out);
            }
            "gallery" => {
                let lay = |l: WatermarkLayout| if l == WatermarkLayout::Diagonal { "diagonal" } else { "horizontal" };
                let join = |b: [f64; 2]| format!("{},{}", to_js_string(b[0]), to_js_string(b[1]));
                let s: Vec<String> = watermark_gallery()
                    .iter()
                    .map(|g| {
                        format!(
                            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
                            g.id,
                            g.category,
                            g.text_pt,
                            g.text_en,
                            g.text_es,
                            lay(g.layout),
                            join(g.box_pt),
                            join(g.box_en),
                            join(g.box_es)
                        )
                    })
                    .collect();
                replay.check(&key, "out", &sha(&s.join("\n")));
            }
            "texts" => {
                let s: Vec<String> = ["en", "pt", "es"].iter().map(|l| watermark_texts(l).unwrap().join("|")).collect();
                replay.check(&key, "out", &sha(&s.join("\n")));
            }
            "wr" => {
                let dump = std::env::var("UO_DUMP").ok().and_then(|v| v.parse::<usize>().ok()) == Some(*i - 1);
                for (facet, v) in run_scenario(f[0], dump) {
                    replay.check(&key, &facet, &v);
                }
            }
            other => panic!("unknown input kind {other}"),
        }
    }
    let report = replay.finish();
    println!("page-background: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_PARITY_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(30)));
    }
    report.assert_clean();
    assert!(report.equal > 8_000);
}
