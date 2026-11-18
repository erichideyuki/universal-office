//! Links (wave 4): `uo_pptx::links::slide_links` against the frozen TS viewer's hit-test boxes (`js/capture-links.mjs`,
//! `tests/captures/links.tsv`: the `RunUrl` boxes bit-equal over the tracked samples and the committed generated decks), the
//! shape clicks, slide and show jumps of a constructed slide, and their PDF annotations (internal jumps, the run marks of a
//! rotated shape), M1 and hostile XML.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_pptx::links::{LinkBox, LinkSource, LinkTarget, ShowJump, slide_links};
use uo_pptx::pdf::{PX_PER_EMU, PdfOptions};
use uo_pptx::{Deck, Limits, View};

#[path = "support/open_fonts.rs"]
mod open_fonts;

use open_fonts::{open_fonts, root};

/// The view of the capture (`js/capture-links.mjs`).
const VIEW: View = View { scale: 96.0 / 914400.0, ox: 3.0, oy: 5.0 };

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

/// The capture's deck set: tracked `.pptx` samples and the generated groups, keyed as the capture keys them.
fn deck_set() -> Vec<(String, PathBuf)> {
    let out = std::process::Command::new("git").args(["-C"]).arg(root()).args(["ls-files", "slides/samples"]).output().unwrap();
    let mut list: Vec<(String, PathBuf)> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(uo_jsnum::text::trim)
        .filter(|s| s.to_ascii_lowercase().ends_with(".pptx"))
        .map(|s| (format!("samples/{}", s.trim_start_matches("slides/samples/")), root().join(s)))
        .collect();
    list.sort();
    for g in ["b1a", "b3a", "b5b", "b6a", "c1d"] {
        let dir = captures().join("decks").join(g);
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        for p in names.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pptx")) {
            list.push((format!("{g}/{}", p.file_name().unwrap().to_string_lossy()), p));
        }
    }
    list
}

type Expect = BTreeMap<(String, u32), Vec<(u32, [f64; 4], String)>>;

fn expected() -> (Expect, usize) {
    let text = std::fs::read_to_string(captures().join("links.tsv")).unwrap();
    let mut map: Expect = BTreeMap::new();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let key = (f[0].to_owned(), f[1].parse::<u32>().unwrap());
        if f.len() == 3 {
            map.entry(key).or_default();
            continue;
        }
        let num = uo_jsnum::parse::to_number;
        map.entry(key).or_default().push((f[2].parse().unwrap(), [num(f[3]), num(f[4]), num(f[5]), num(f[6])], f[7].to_owned()));
        n += 1;
    }
    (map, n)
}

#[test]
fn run_url_boxes_equal_the_ts_hit_test() {
    let (exp, total) = expected();
    assert!(total > 500, "boxes in the capture: {total}");
    let (mut equal, mut decks, mut extra) = (0usize, 0usize, 0usize);
    for (key, path) in deck_set() {
        let Ok(mut deck) = Deck::open(&std::fs::read(&path).unwrap(), Limits::default()) else { continue };
        decks += 1;
        let n = deck.presentation().unwrap().slides.len();
        for si in 0..u32::try_from(n).unwrap() {
            let want = exp.get(&(key.clone(), si)).cloned().unwrap_or_default();
            let got: Vec<(u32, [f64; 4], String)> = match slide_links(&mut deck, si, VIEW) {
                Ok(v) => v
                    .into_iter()
                    .filter(|b| b.source == LinkSource::RunUrl)
                    .map(|b| match b.target {
                        LinkTarget::Url(u) => (b.shape, b.rect, u.to_string()),
                        t => panic!("{key} slide {si}: a RunUrl box with target {t:?}"),
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            for (g, w) in got.iter().zip(&want) {
                assert_eq!(g.0, w.0, "{key} slide {si}: shape");
                for k in 0..4 {
                    assert_eq!(g.1[k].to_bits(), w.1[k].to_bits(), "{key} slide {si} shape {}: box {:?} vs {:?}", g.0, g.1, w.1);
                }
                assert_eq!(g.2, w.2, "{key} slide {si}: url");
                equal += 1;
            }
            assert_eq!(got.len(), want.len(), "{key} slide {si}: box count");
            extra += got.len().saturating_sub(want.len());
        }
    }
    eprintln!("links: {decks} decks, {equal} of {total} RunUrl boxes bit-equal to the TS hit-test, {extra} extra");
    assert_eq!(equal, total);
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

fn sp(id: u32, click: &str, x: u64, rot: u32, body: &str) -> String {
    let rot = if rot == 0 { String::new() } else { format!(r#" rot="{rot}""#) };
    let tx = if body.is_empty() { String::new() } else { format!("<p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody>") };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="s{id}">{click}</p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm{rot}><a:off x="{x}" y="914400"/><a:ext cx="1828800" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="DDEEFF"/></a:solidFill></p:spPr>{tx}</p:sp>"#
    )
}

fn run(text: &str, link: &str) -> String {
    format!(r#"<a:p><a:r><a:rPr lang="en-US" sz="1800">{link}</a:rPr><a:t>{text}</a:t></a:r></a:p>"#)
}

/// A generated deck (3+ slides) whose first slide is replaced by shapes with every kind of action.
fn action_deck() -> (Deck, u32) {
    let path = captures().join("decks/b1a/0000.pptx");
    let mut deck = Deck::open(&std::fs::read(path).unwrap(), Limits::default()).unwrap();
    let (slide1, slide3, n) = {
        let p = deck.presentation().unwrap();
        (p.slides[0].target.clone().unwrap().to_string(), p.slides[2].target.clone().unwrap().to_string(), p.slides.len())
    };
    let rels_name = format!("ppt/slides/_rels/{}.rels", slide1.rsplit('/').next().unwrap());
    let rels = String::from_utf8(deck.parts().bytes(&rels_name).unwrap().to_vec()).unwrap();
    let slide3_rel = format!("../slides/{}", slide3.rsplit('/').next().unwrap());
    let add = format!(
        r#"<Relationship Id="rIdJ" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="{slide3_rel}"/><Relationship Id="rIdU" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/shape" TargetMode="External"/><Relationship Id="rIdW" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/run" TargetMode="External"/><Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="file:///C:/x.doc" TargetMode="External"/></Relationships>"#
    );
    let rels = rels.replace("</Relationships>", &add);
    let jump = r#"<a:hlinkClick r:id="rIdJ" action="ppaction://hlinksldjump"/>"#;
    let shapes = [
        // a slide jump on a shape turned 90 degrees
        sp(2, jump, 914400, 5_400_000, ""),
        // an external address on the shape, a "next slide" jump on its run
        sp(
            3,
            r#"<a:hlinkClick r:id="rIdU"/>"#,
            3_657_600,
            0,
            &run("Next", r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/>"#),
        ),
        // a group with "last slide": its first child has no click (inherits it), the second ends the show
        format!(
            r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="4" name="g"><a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=lastslide"/></p:cNvPr><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="6858000"/><a:chOff x="0" y="0"/><a:chExt cx="9144000" cy="6858000"/></a:xfrm></p:grpSpPr>{}{}</p:grpSp>"#,
            sp(5, "", 6_400_800, 0, ""),
            sp(6, r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=endshow"/>"#, 914_400 * 2, 0, "")
        ),
        // a run with an http address (the TS rule), a macro and a file link that give nothing placeable
        sp(7, r#"<a:hlinkClick r:id="" action="macro:Foo"/>"#, 914_400 * 4, 0, &run("Run link", r#"<a:hlinkClick r:id="rIdW"/>"#)),
        sp(8, r#"<a:hlinkClick r:id="rIdF"/>"#, 914_400 * 6, 0, ""),
    ];
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{}</p:spTree></p:cSld></p:sld>"#,
        shapes.concat()
    );
    deck.set_part(&slide1, Arc::from(xml.into_bytes()));
    deck.set_part(&rels_name, Arc::from(rels.into_bytes()));
    (deck, u32::try_from(n).unwrap())
}

#[test]
fn shape_clicks_slide_and_show_jumps() {
    let (mut deck, n) = action_deck();
    let slide = deck.slide(0).unwrap();
    let own: Vec<(usize, &str)> =
        slide.shapes.iter().enumerate().filter(|(_, s)| s.inherited != Some(true)).map(|(i, s)| (i, &*s.id)).collect();
    let idx = |id: &str| u32::try_from(own.iter().find(|(_, s)| *s == id).unwrap_or_else(|| panic!("shape {id} in {own:?}")).0).unwrap();
    let links = slide_links(&mut deck, 0, VIEW).unwrap();
    let pick = |id: &str| -> Vec<(LinkTarget, LinkSource)> {
        links.iter().filter(|b| b.shape == idx(id)).map(|b| (b.target.clone(), b.source)).collect()
    };
    assert_eq!(pick("2"), vec![(LinkTarget::Slide(2), LinkSource::ShapeClick)]);
    assert_eq!(
        pick("3"),
        vec![
            (LinkTarget::Url("https://example.org/shape".into()), LinkSource::ShapeClick),
            (LinkTarget::Show(ShowJump::Next), LinkSource::RunJump)
        ]
    );
    assert_eq!(pick("5"), vec![(LinkTarget::Show(ShowJump::Last), LinkSource::ShapeClick)]);
    assert_eq!(pick("6"), vec![(LinkTarget::Show(ShowJump::End), LinkSource::ShapeClick)]);
    assert_eq!(pick("7"), vec![(LinkTarget::Url("https://example.org/run".into()), LinkSource::RunUrl)]);
    assert_eq!(pick("8"), vec![(LinkTarget::Url("file:///C:/x.doc".into()), LinkSource::ShapeClick)]);
    // the box is the unrotated xfrm box at the view, as the TS hit-test
    let b: &LinkBox = links.iter().find(|b| b.shape == idx("2")).unwrap();
    assert_eq!(b.rect, [3.0 + 914400.0 * VIEW.scale, 5.0 + 914400.0 * VIEW.scale, 1828800.0 * VIEW.scale, 914400.0 * VIEW.scale]);

    // the PDF: slide jump + next + last are internal, the shape address and the run piece are URIs, the file link is
    // refused by the scheme rule and "end show" has no PDF form
    // (the TS-era link rules and the hidden slide kept: `ts_link_rects`, `ts_hidden_slides`; PowerPoint's rules below)
    let fonts = open_fonts();
    let pp = uo_pptx::pdf(&mut deck, &PdfOptions::new(&fonts)).unwrap();
    // PowerPoint: the deck's hidden slide is left out; the run jump and the run address end their text (no
    // `a:endParaRPr`), so PowerPoint writes no annotation for them
    assert_eq!(pp.report.pages + pp.report.hidden_slides, n);
    assert_eq!(pp.report.hidden_slides, 1);
    assert!(pp.report.links_not_written >= 2, "not written {}", pp.report.links_not_written);
    let out = uo_pptx::pdf(&mut deck, &PdfOptions::ts_strict(&fonts)).unwrap();
    let r = &out.report;
    assert_eq!(r.pages, n);
    eprintln!(
        "action deck: {} links, {} internal, {} not written, {} hidden slides, missing {:?}",
        r.links, r.internal_links, r.links_not_written, r.hidden_slides, r.missing
    );
    assert!(r.internal_links >= 3, "internal links {}", r.internal_links);
    assert_eq!(r.missing.get("hyperlink:action").copied().unwrap_or(0), 1);
    assert!(r.missing.get("hyperlink:scheme").copied().unwrap_or(0) >= 1);
    let text = String::from_utf8_lossy(&out.bytes);
    assert!(text.contains("https://example.org/shape") && text.contains("https://example.org/run"));
    assert!(text.contains("/Dest"), "an internal destination");
    // the turned shape: its link box is the rotated box (a 2 x 1 in shape turned 90 degrees is 1 x 2 in on the page)
    let rot = out.bytes.windows(5).filter(|w| w == b"/Rect").count();
    assert!(rot >= 5, "link rectangles: {rot}");
    let _ = PX_PER_EMU;
}

#[test]
fn slide_links_returns_live_bytes_and_survives_hostile_xml() {
    let (mut deck, _) = action_deck();
    let _ = deck.slide(0).unwrap();
    let _ = slide_links(&mut deck, 0, VIEW).unwrap();
    assert_no_leak(|| {
        let v = slide_links(&mut deck, 0, VIEW).unwrap();
        assert!(!v.is_empty());
        drop(v);
    });
    // hostile: actions without ids, unknown jumps, a jump to a missing part, deep groups
    let path = captures().join("decks/b1a/0000.pptx");
    let mut deck = Deck::open(&std::fs::read(path).unwrap(), Limits::default()).unwrap();
    let slide1 = deck.presentation().unwrap().slides[0].target.clone().unwrap().to_string();
    let mut inner = sp(9, r#"<a:hlinkClick action="ppaction://hlinksldjump"/>"#, 0, 0, "");
    for d in 0..200 {
        inner = format!(
            r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{}" name=""><a:hlinkClick r:id="nope" action="ppaction://hlinkshowjump?jump=sideways"/></p:cNvPr><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{inner}</p:grpSp>"#,
            100 + d
        );
    }
    let xml = format!(
        r#"<p:sld {NS}><p:cSld><p:spTree>{inner}{}</p:spTree></p:cSld></p:sld>"#,
        sp(10, r#"<a:hlinkClick r:id="rId999"/>"#, 0, 0, "")
    );
    deck.set_part(&slide1, Arc::from(xml.into_bytes()));
    if let Ok(v) = slide_links(&mut deck, 0, VIEW) {
        assert!(v.iter().all(|b| b.source != LinkSource::ShapeClick || matches!(b.target, LinkTarget::Show(_))));
    }
}
