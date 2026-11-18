//! The Slides PDF against PowerPoint's own PDF of synthetic decks (Slides wave 5, record `core/docs/modules/uo-pdf.md`
//! "PowerPoint rules of the PDF (wave 5)"): `tests/oracle_ppt/synthetic/*.pptx` with the values PowerPoint 365 pt-BR
//! wrote, measured once through COM (`*.links.tsv`, `*.faces.tsv`). The decks use Liberation Sans, installed on the
//! measuring machine and one of the repository's open fonts, so the glyph geometry is the same on both sides.
//!
//! - link rectangles: one per line and formatting run, per word in a justified paragraph, split at tabs, the ink bounds of
//!   the glyphs, kept off the page, none for a link whose range holds its text's last paragraph mark;
//! - hidden slides: left out, slide numbers kept, jumps to them not written, show jumps resolved by slide;
//! - fallback faces: PowerPoint's per-script families (`uo_dml_render::text::fallback`), the TS chain under the switch.
#![cfg(feature = "pdf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use uo_dml_render::text::fallback::FallbackCtx;
use uo_fonts::Face;
use uo_pptx::pdf::{FaceSet, FontSource, PdfFail, PdfOptions, pdf_ts_strict};
use uo_pptx::{Deck, Limits};

#[path = "support/open_fonts.rs"]
mod open_fonts;
#[path = "support/pdf_view.rs"]
mod pdf_view;

use open_fonts::open_fonts;

fn synthetic(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_ppt/synthetic").join(name)
}

fn deck(name: &str) -> Deck {
    Deck::open(&std::fs::read(synthetic(name)).unwrap(), Limits::default()).unwrap()
}

/// One link annotation: page, target (`uri:<address>` or `page:<index>`), `x0, y0, x1, y1` (pt, y down).
#[derive(Clone, Debug, PartialEq)]
struct Link {
    page: usize,
    target: String,
    rect: [f64; 4],
}

fn num(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

/// The link annotations of a PDF written by `uo-pdf`, page by page.
fn pdf_links(bytes: &[u8]) -> Vec<Link> {
    let v = pdf_view::view(bytes).unwrap();
    let dicts: Vec<String> = (1..=v.objs.len()).map(|n| v.dict_of(u32::try_from(n).unwrap())).collect();
    let pages: Vec<(usize, &String)> = dicts.iter().enumerate().filter(|(_, d)| d.starts_with("<< /Type /Page ")).collect();
    let page_index = |obj: usize| pages.iter().position(|(i, _)| *i + 1 == obj);
    let mut out = Vec::new();
    for (pi, (_, d)) in pages.iter().enumerate() {
        let h = {
            let at = d.find("/MediaBox [").unwrap() + 11;
            let end = d[at..].find(']').unwrap() + at;
            num(d[at..end].split(' ').nth(3).unwrap())
        };
        let Some(at) = d.find("/Annots [") else { continue };
        let end = d[at..].find(']').unwrap() + at;
        let refs: Vec<usize> = d[at + 9..end].split(" 0 R").filter_map(|t| uo_jsnum::text::trim(t).parse().ok()).collect();
        for r in refs {
            let a = &dicts[r - 1];
            let rs = a.find("/Rect [").unwrap() + 7;
            let re = a[rs..].find(']').unwrap() + rs;
            let v: Vec<f64> = a[rs..re].split(' ').map(num).collect();
            let target = if let Some(u) = a.find("/URI (") {
                let e = a[u + 6..].find(')').unwrap() + u + 6;
                format!("uri:{}", &a[u + 6..e])
            } else {
                let dpos = a.find("/Dest [").unwrap() + 7;
                let obj: usize = a[dpos..].split(' ').next().unwrap().parse().unwrap();
                format!("page:{}", page_index(obj).unwrap())
            };
            out.push(Link { page: pi, target, rect: [v[0], h - v[3], v[2], h - v[1]] });
        }
    }
    out
}

/// PowerPoint's links of a synthetic deck (`<name>.links.tsv`).
fn ppt_links(name: &str) -> Vec<Link> {
    std::fs::read_to_string(synthetic(name))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Link { page: f[0].parse().unwrap(), target: f[1].to_owned(), rect: [num(f[2]), num(f[3]), num(f[4]), num(f[5])] }
        })
        .collect()
}

/// Every PowerPoint link has a Rust link with the same page and target and every edge within `tol` pt (`checked` false:
/// not compared); the largest edge difference.
fn match_links(got: &[Link], want: &[Link], tol: f64, checked: impl Fn(&Link) -> bool) -> f64 {
    let mut used = vec![false; got.len()];
    let mut worst = 0.0f64;
    for w in want.iter().filter(|w| checked(w)) {
        let pick = got
            .iter()
            .enumerate()
            .find(|(i, g)| !used[*i] && g.page == w.page && g.target == w.target && (0..4).all(|k| (g.rect[k] - w.rect[k]).abs() < tol));
        let Some((i, g)) = pick else { panic!("no Rust link for PowerPoint's {w:?}\n got {got:#?}") };
        used[i] = true;
        worst = (0..4).map(|k| (g.rect[k] - w.rect[k]).abs()).fold(worst, uo_jsnum::math::max);
    }
    worst
}

/// Shape clicks (`pp-clicks.pptx`): PowerPoint's rectangle is the shape's box grown by `max(0, 0.6 em - left inset)` to the
/// left and, top-anchored, `max(0, 0.34 em - top inset)` upwards (an invisible text object it writes with the link). The
/// turned shape (an invisible object PowerPoint places in the turned frame) is not compared.
#[test]
fn shape_click_rectangles_follow_powerpoint() {
    let fonts = open_fonts();
    let mut d = deck("pp-clicks.pptx");
    let out = uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    let got = pdf_links(&out.bytes);
    let want = ppt_links("pp-clicks.links.tsv");
    assert_eq!(got.len(), want.len());
    // the turned shape: PowerPoint's rectangle starts at x 293.3
    let worst = match_links(&got, &want, 1.0, |w| (w.rect[0] - 293.33).abs() > 0.1);
    eprintln!("pp-clicks: {} shape clicks matched, worst edge {worst:.2} pt", want.len() - 1);
    // the TS rule: the shape's box
    let ts = uo_pptx::pdf(&mut d, &PdfOptions::ts_strict(&fonts)).unwrap();
    assert!(pdf_links(&ts.bytes).iter().any(|l| (l.rect[0] - 40.0).abs() < 0.01 && (l.rect[1] - 40.0).abs() < 0.01));
}

/// A complex-script character goes to the complex-script face even when the run's face has it (PowerPoint: Hebrew in
/// Liberation Sans is drawn in Arial, measured on 6 latin faces); the TS keeps the run's face. "Arial" is answered here
/// with Liberation Serif so that the two faces can be told apart.
#[test]
fn complex_script_characters_take_the_complex_script_face() {
    struct ArialIsSerif(FaceSet);
    impl FontSource for ArialIsSerif {
        fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
            let f = if family.eq_ignore_ascii_case("arial") { "Liberation Serif" } else { family };
            self.0.face(f, bold, italic)
        }
        fn fallback_ctx(&self, cp: u32, bold: bool, italic: bool, primary: &Face, ctx: &FallbackCtx<'_>) -> Option<Arc<Face>> {
            uo_pptx::pdf::powerpoint_chain(self, cp, bold, italic, primary, ctx)
        }
    }
    let fonts = ArialIsSerif(open_fonts());
    let lib = fonts.face("Liberation Sans", false, false).unwrap();
    assert_ne!(lib.glyph_id(0x5e2), 0, "Liberation Sans has Hebrew");
    // the Georgian line of pp-fallback becomes Hebrew (a Liberation Sans run, no a:cs, no language)
    let hebrew = |d: &mut Deck| {
        let t = d.presentation().unwrap().slides[0].target.clone().unwrap().to_string();
        let xml = String::from_utf8(d.parts().bytes(&t).unwrap().to_vec()).unwrap();
        let geor = "\u{10e5}\u{10d0}\u{10e0}\u{10d7}\u{10e3}\u{10da}\u{10d8}";
        assert!(xml.contains(geor));
        d.set_part(&t, Arc::from(xml.replace(geor, "\u{5e2}\u{5d1}\u{5e8}\u{5d9}\u{5ea}").into_bytes()));
    };
    let mut d = deck("pp-fallback.pptx");
    hebrew(&mut d);
    let pp = uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    let mut d = deck("pp-fallback.pptx");
    hebrew(&mut d);
    let ts = pdf_ts_strict(&mut d, &PdfOptions::new(&fonts)).unwrap();
    // PowerPoint: the 5 Hebrew letters in "Arial" too; the TS: only the characters Liberation Sans lacks fall back
    assert!(
        pp.report.fallback_glyphs >= ts.report.fallback_glyphs + 5,
        "PowerPoint {} vs TS {}",
        pp.report.fallback_glyphs,
        ts.report.fallback_glyphs
    );
}

#[test]
fn link_rectangles_follow_powerpoint() {
    let fonts = open_fonts();
    let mut d = deck("pp-rules.pptx");
    let out = uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    let got = pdf_links(&out.bytes);
    let want = ppt_links("pp-rules.links.tsv");
    eprintln!("pp-rules: PowerPoint {} links, Rust {} ({:?})", want.len(), got.len(), out.report);
    assert_eq!(out.report.pages, 3, "hidden slides 2 and 5 left out");
    assert_eq!(got.len(), want.len(), "link count\n got {got:#?}\nwant {want:#?}");
    let worst = match_links(&got, &want, 1.5, |_| true);
    eprintln!("pp-rules: every PowerPoint link matched, worst edge {worst:.2} pt");
    // off the page: the overflowing lines of the long link keep their rectangles below the media box
    assert!(got.iter().any(|g| g.rect[3] > 540.0) && got.iter().any(|g| g.rect[1] < 0.0));
}

/// Run-level and shape-level jumps (`pp-jumps.pptx`, Aptos text: the open fonts substitute it, so the run rectangles are
/// compared by page, target and order, the left edge within 2 pt): next/previous/first/last resolved by slide, jumps to the
/// hidden slides not written, a jump ending its text not written.
#[test]
fn slide_and_show_jumps_follow_powerpoint() {
    let fonts = open_fonts();
    let mut d = deck("pp-jumps.pptx");
    let out = uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    let got = pdf_links(&out.bytes);
    let want = ppt_links("pp-jumps.links.tsv");
    let key = |l: &Link| (l.page, l.target.clone());
    assert_eq!(got.iter().map(key).collect::<Vec<_>>(), want.iter().map(key).collect::<Vec<_>>(), "\n got {got:#?}\nwant {want:#?}");
    for (g, w) in got.iter().zip(&want) {
        // shape clicks: PowerPoint's rectangle reaches left of the shape (see `is_shape_click`); runs: the left edge
        let shape = w.rect[2] - w.rect[0] > 150.0;
        let edge = if shape { 2 } else { 0 };
        assert!((g.rect[edge] - w.rect[edge]).abs() < 2.0, "{g:?} vs PowerPoint {w:?}");
    }
}

#[test]
fn the_ts_switches_keep_the_ts_links_and_pages() {
    let fonts = open_fonts();
    let mut d = deck("pp-rules.pptx");
    let out = pdf_ts_strict(&mut d, &PdfOptions::new(&fonts)).unwrap();
    assert_eq!(out.report.pages, 5, "hidden slides kept");
    assert_eq!(out.report.hidden_slides, 0);
    assert_eq!(out.report.links_not_written, 0);
    let got = pdf_links(&out.bytes);
    // the TS join: the links ending their text are placed, cut to the page, the justified words joined
    assert!(got.iter().any(|g| g.target == "uri:https://example.com/p1"));
    assert!(got.iter().all(|g| g.rect[1] >= 0.0 && g.rect[3] <= 540.0));
    assert_eq!(got.iter().filter(|g| g.target == "uri:https://example.com/p8").count(), 1, "the justified link, one fragment");
}

#[test]
fn hidden_slides_are_left_out_with_their_numbers_and_jumps() {
    let fonts = open_fonts();
    let mut d = deck("pp-rules.pptx");
    let out = uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    assert_eq!((out.report.pages, out.report.hidden_slides), (3, 2));
    let v = pdf_view::view(&out.bytes).unwrap();
    assert_eq!(v.page_count(), 3);
    let (text, _) = v.text();
    // slide numbers keep the slide's own number (PowerPoint: page 2 shows "Slide 3", page 3 "Slide 4")
    assert!(text.contains("Slide3") || text.contains("Slide 3"), "{text}");
    assert!(text.contains("Slide4") || text.contains("Slide 4"), "{text}");
    assert!(
        !text.contains("hiddenlink") && !text.contains("hidden link") && !text.contains("hiddenlast") && !text.contains("hidden last"),
        "the hidden slides' text: {text}"
    );
    // jumps to the hidden slides 2 and 5 (to slide 2, previous, last) write nothing; to slide 4, next and first do
    let pages: Vec<String> = pdf_links(&out.bytes).into_iter().filter(|l| l.page == 1).map(|l| l.target).collect();
    assert_eq!(pages, ["page:2", "page:2", "page:0"]);
    assert_eq!(out.report.links_not_written, 3 + 4, "3 jumps to hidden slides, 4 rectangles of the 3 links ending their text");
}

#[test]
fn every_slide_hidden_gives_no_pdf_unless_kept() {
    let fonts = open_fonts();
    let mut d = deck("pp-rules.pptx");
    let targets: Vec<String> = d.presentation().unwrap().slides.iter().map(|s| s.target.clone().unwrap().to_string()).collect();
    for t in &targets {
        let xml = String::from_utf8(d.parts().bytes(t).unwrap().to_vec()).unwrap();
        let xml = if xml.contains(" show=\"0\"") { xml } else { xml.replacen("<p:sld ", "<p:sld show=\"0\" ", 1) };
        d.set_part(t, Arc::from(xml.into_bytes()));
    }
    assert_eq!(uo_pptx::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap_err(), PdfFail::NoVisibleSlides);
    let kept = uo_pptx::pdf(&mut d, &PdfOptions { ts_hidden_slides: true, ..PdfOptions::new(&fonts) }).unwrap();
    assert_eq!(kept.report.pages, 5);
}

/// The open fonts, recording every family the fallback asks for.
struct Recorder {
    inner: FaceSet,
    asked: RefCell<Vec<String>>,
}

impl FontSource for Recorder {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        self.asked.borrow_mut().push(family.to_owned());
        self.inner.face(family, bold, italic)
    }
    fn fallback(&self, cp: u32, bold: bool, italic: bool, primary: &Face) -> Option<Arc<Face>> {
        uo_pptx::pdf::fallback_chain(self, cp, bold, italic, primary)
    }
    fn fallback_ctx(&self, cp: u32, bold: bool, italic: bool, primary: &Face, ctx: &FallbackCtx<'_>) -> Option<Arc<Face>> {
        uo_pptx::pdf::powerpoint_chain(self, cp, bold, italic, primary, ctx)
    }
}

#[test]
fn fallback_faces_follow_powerpoint_per_script() {
    let want: Vec<(String, String)> = std::fs::read_to_string(synthetic("pp-fallback.faces.tsv"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let (a, b) = l.split_once('\t').unwrap();
            (a.to_owned(), b.to_owned())
        })
        .collect();
    let rec = Recorder { inner: open_fonts(), asked: RefCell::new(Vec::new()) };
    let mut d = deck("pp-fallback.pptx");
    uo_pptx::pdf(&mut d, &PdfOptions::new(&rec)).unwrap();
    let pp = rec.asked.take();
    // the language-free lines: PowerPoint's face of each script is asked for
    for (code, face) in want.iter().filter(|(c, _)| !c.ends_with("Ja") && !c.ends_with("Th")) {
        assert!(pp.iter().any(|f| f == face), "{code}: PowerPoint draws it in {face}; asked {pp:?}");
    }
    // ... and not the TS chain's own faces (Leelawadee UI for Thai, MS Mincho for Han)
    assert!(!pp.iter().any(|f| f == "Leelawadee UI" || f == "MS Mincho"), "{pp:?}");
    // the TS chain under the deck's switch
    let rec = Recorder { inner: open_fonts(), asked: RefCell::new(Vec::new()) };
    let mut d = deck("pp-fallback.pptx");
    pdf_ts_strict(&mut d, &PdfOptions::new(&rec)).unwrap();
    let ts = rec.asked.take();
    assert!(ts.iter().any(|f| f == "Leelawadee UI") && ts.iter().any(|f| f == "MS Mincho"), "{ts:?}");
    assert!(!ts.iter().any(|f| f == "Angsana New"), "{ts:?}");
}
