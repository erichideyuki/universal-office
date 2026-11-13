//! Wave 7c (V, P1) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7c — V"): the merge-state item
//! "the preview layouts are built without the per-layout bookmark sync". The frozen TS lays every preview mini-document
//! out with `bookmarks: new Map()` (universal-write.ts 7118 TOC gallery, 12113 `paintPreviewPage` — covers and
//! watermarks —, 18505 header/footer gallery, 7874 `renderClipPng`, 20182 `paintStyleThumb`) and calls `layoutDocument`
//! directly, without `buildEngineDoc`'s scan of the run marks. A stale offset can only come from a non-empty index: this
//! test proves every preview kind of the session (and the TOC gallery's) carries an EMPTY index after an edit that moved
//! the bookmarks of the session's document, so a REF/PAGEREF in a preview resolves exactly as the TS's empty map does.
//! The layout reads the bookmarks only through `DocView::bookmarks()` (= `Document::bookmarks`, no mark scan:
//! uo-docx-layout `engine/field_refs.rs`, `navigation.rs`).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2a_support::Cx;
use uo_docx::model::Document;
use uo_docx::session::ops::clipboard::{ClipboardCmd, PasteMode, RichPick};
use uo_docx::session::ops::header_footer::hf_templates::HfGallery;
use uo_docx::session::ops::page::PageLang;
use uo_docx::session::ops::previews::PreviewKind;
use uo_docx::session::ops::references::toc::TocTemplateId;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn fixture() -> Vec<u8> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../uo-docx-layout/tests/fidelity/ref-after-note.docx");
    std::fs::read(p).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> Outcome {
    s.exec(c, &mut cx.cx()).unwrap()
}

fn sel(s: &mut Session, cx: &mut Cx, a: (u32, u32), f: (u32, u32)) {
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn assert_empty(d: &Document, what: &str) {
    assert!(d.bookmarks.starts.is_empty() && d.bookmarks.ends.is_empty(), "{what}: the preview's bookmark index is not empty");
}

#[test]
fn every_preview_lays_out_with_an_empty_bookmark_index() {
    let mut cx = Cx::new();
    let mut s = Session::open(fixture(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert!(s.document().bookmarks.starts.len() >= 2, "the fixture's bookmarks bm1/bm2 are indexed");
    // an edit in the bookmarked paragraph: the session's offsets move (B gate-10: synced at the next layout)
    sel(&mut s, &mut cx, (0, 0), (0, 0));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("novo ") }));
    sel(&mut s, &mut cx, (0, 5), (0, 5));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("x") }));
    let mut n = 0usize;
    // covers and watermarks (`paintPreviewPage`, 12113)
    for lang in [PageLang::Pt, PageLang::En, PageLang::Es] {
        for (id, _) in s.cover_pages(lang) {
            if let Some(p) = s.preview_document(&PreviewKind::Cover { id: id.into(), lang }) {
                assert_empty(&p.document, id);
                n += 1;
            }
        }
        for e in s.watermark_gallery_entries(lang) {
            if let Some(p) = s.preview_document(&PreviewKind::Watermark { id: e.item.id.into(), lang }) {
                assert_empty(&p.document, e.item.id);
                n += 1;
            }
        }
        // the header/footer galleries (18505), every gallery and template
        for g in
            [HfGallery::Header, HfGallery::Footer, HfGallery::PageTop, HfGallery::PageBottom, HfGallery::PageMargin, HfGallery::PageCurrent]
        {
            let ids: Vec<String> = s.header_footer_templates(g, lang).into_iter().map(|(id, _)| id.to_owned()).collect();
            for id in ids {
                let p = s.preview_document(&PreviewKind::HeaderFooter { gallery: g, id: id.clone(), lang, section: 0 });
                let p = p.unwrap_or_else(|| panic!("{g:?} {id}"));
                assert_empty(&p.document, &id);
                n += 1;
            }
        }
    }
    // the table-style thumbnails (20182)
    let p = s.preview_document(&PreviewKind::TableStyleName { name: "Grid Table 4 Accent 1".into(), look: None }).unwrap();
    assert_empty(&p.document, "table style");
    n += 1;
    let xml = r#"<w:style xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:type="table" w:styleId="T"><w:name w:val="T"/></w:style>"#;
    let p = s.preview_document(&PreviewKind::TableStyleXml { xml: xml.into(), look: None, lang: PageLang::Pt }).unwrap();
    assert_empty(&p.document, "table style xml");
    n += 1;
    // the TOC gallery (7118)
    for id in [TocTemplateId::Auto1, TocTemplateId::Auto2, TocTemplateId::Manual] {
        let p = s.toc_template_preview(id).unwrap();
        assert_empty(&p.document, "toc");
        n += 1;
    }
    // a Paste as Picture of the paragraph holding `REF bm1` (`renderClipPng`, 7874): the clip's REF reads the empty index
    sel(&mut s, &mut cx, (2, 0), (2, 14));
    run(&mut s, &mut cx, Command::Clipboard(ClipboardCmd::Copy));
    sel(&mut s, &mut cx, (3, 0), (3, 0));
    let o = run(
        &mut s,
        &mut cx,
        Command::Clipboard(ClipboardCmd::PasteData {
            text: Utf16::from("REF fechado: x"),
            html: None,
            image: None,
            rich: RichPick::WithRich,
            mode: Some(PasteMode::Picture),
            list: None,
            layout: d2b_f8_support::layout(),
        }),
    );
    assert!(matches!(o, Outcome::NeedsJobs(_)), "the picture paste waits for its render");
    let p = s.preview_document(&PreviewKind::RenderClip).unwrap();
    assert_empty(&p.document, "render clip");
    n += 1;
    // the session's own index is untouched (the previews clone the document)
    assert!(s.document().bookmarks.starts.len() >= 2);
    println!("previews checked: {n}, every bookmark index empty");
    assert!(n > 40, "{n}");
}
