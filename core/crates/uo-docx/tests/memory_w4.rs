//! Memory gate of the D1b frontier W4/W4b (`write::emit::{notes, settings}`): M1 — every public function
//! returns the live bytes exactly, outputs dropped — over the corpus mini packages
//! (`tests/parity/settings-xml.inputs`) and the synthetic packages, in the variants where the parts are
//! missing, self-closing or truncated, and over the error paths (a settings part that is not XML: the
//! writers that parse it return the reader's error). M3: the text the string functions build against its
//! size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::hint::black_box;

use uo_alloc::{assert_no_leak, measure};
use uo_docx::ids::IdAlloc;
use uo_docx::model::formatting::NoteProps;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::emit::notes::{
    ENDNOTE_STYLES, FOOTNOTE_STYLES, NotePropsArg, empty_notes_part, ensure_note_parts, first_tag_open, free_rid_number,
    has_note_relationship, new_note_paragraphs, note_mark_field, note_props_inner, note_props_xml, note_ref_run, note_styles,
    patch_settings_notes, same_note_props, settings_props_element,
};
use uo_docx::write::emit::settings::{
    HyphenationSettings, copy_hf_part_xml, drop_hf_text, drop_unused_hf_parts, hf_part_xml, hf_target_path, is_hf_relationship,
    is_page_number_gallery, page_number_group_ids, quoted_attr, remove_override, self_closing_matches, val_is_off, val_pattern_test,
    write_default_tab_stop, write_display_background_shape, write_even_and_odd_headers, write_hyphenation_settings,
};
use uo_docx::write::matchers::element_matches;

#[allow(unused_imports)]
use crate::w4_support;

use w4_support::{Part, dec, inputs, package, synthetic_inputs, synthetic_package};

const SETTINGS: &str = "word/settings.xml";

fn replace(parts: &[Part], name: &str, with: &[u8]) -> Vec<Part> {
    parts.iter().map(|(n, d)| if n == name { (n.clone(), with.to_vec()) } else { (n.clone(), d.clone()) }).collect()
}

fn variants(parts: &[Part]) -> Vec<Vec<Part>> {
    let w = b"<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">";
    let mut selfclose = w.to_vec();
    selfclose.truncate(selfclose.len() - 1);
    selfclose.extend_from_slice(b"/>");
    let mut bare = w.to_vec();
    bare.extend_from_slice(b"</w:settings>");
    let none: Vec<Part> = parts.iter().filter(|(n, _)| n != SETTINGS).cloned().collect();
    vec![parts.to_vec(), replace(parts, SETTINGS, &selfclose), replace(parts, SETTINGS, &bare), none]
}

fn exercise(parts: &[Part]) {
    let mut pkg = package(parts);
    let hyph = HyphenationSettings { auto: true, caps: false, zone: Some(360.0), limit: 2.0 };
    let off = HyphenationSettings { auto: false, caps: true, zone: None, limit: 0.0 };
    black_box(write_default_tab_stop(&mut pkg, 567.4).is_ok());
    black_box(write_hyphenation_settings(&mut pkg, &hyph).is_ok());
    black_box(write_hyphenation_settings(&mut pkg, &off).is_ok());
    black_box(write_even_and_odd_headers(&mut pkg, true).is_ok());
    black_box(write_even_and_odd_headers(&mut pkg, false).is_ok());
    black_box(write_display_background_shape(&mut pkg, true).is_ok());
    black_box(write_display_background_shape(&mut pkg, false).is_ok());
    black_box(ensure_note_parts(&mut pkg).is_ok());
    let ids: HashSet<String> = ["rId1", "rId2", "rId3"].iter().map(|s| (*s).to_owned()).collect();
    black_box(drop_unused_hf_parts(&mut pkg, &ids).is_ok());
    if let Some((_, s)) = parts.iter().find(|(n, _)| n == SETTINGS) {
        let text = dec(s);
        let fp = NoteProps { pos: Some("pageBottom".into()), num_start: Some(uo_docx::js::JsNumber::Num(2.0)), ..NoteProps::default() };
        black_box(patch_settings_notes(&text, NotePropsArg::Unchanged, NotePropsArg::Unchanged));
        black_box(patch_settings_notes(&text, NotePropsArg::Set(&fp), NotePropsArg::Remove));
        black_box(patch_settings_notes(&text, NotePropsArg::Remove, NotePropsArg::Set(&fp)));
        black_box(settings_props_element(&text, "w:footnotePr"));
        black_box(element_matches(&text, "w:autoHyphenation"));
        black_box(self_closing_matches(&text, "w:defaultTabStop"));
        black_box(first_tag_open(&text, "w:compat"));
    }
    for (name, d) in parts {
        if name.starts_with("word/header") || name.starts_with("word/footer") {
            let mut c = 0.0;
            black_box(copy_hf_part_xml(&dec(d), &mut || {
                c += 1.0;
                c
            }));
        }
        if name == "word/_rels/document.xml.rels" {
            let t = dec(d);
            black_box(has_note_relationship(&t, "footnote"));
            black_box(free_rid_number(&t));
            let ids: HashSet<String> = ["rId1".to_owned()].into_iter().collect();
            black_box(drop_hf_text(&t, "<Types></Types>", &ids));
        }
    }
}

#[test]
fn m1_package_writers_return_the_memory() {
    let all = inputs();
    let mut n = 0;
    for (_, parts) in all.iter().step_by(7) {
        for v in variants(parts) {
            assert_no_leak(|| exercise(&v));
            n += 1;
        }
    }
    println!("M1: {n} packages, 0 B left each");
    assert!(n > 150);
}

#[test]
fn m1_error_paths_return_the_memory() {
    let parts = inputs().into_iter().find(|(_, p)| p.iter().any(|(n, d)| n == SETTINGS && dec(d).contains("</w:settings>"))).unwrap().1;
    let broken =
        replace(&parts, SETTINGS, b"<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:zoom>");
    assert_no_leak(|| {
        let mut pkg = package(&broken);
        assert!(write_default_tab_stop(&mut pkg, 720.0).is_err());
        assert!(write_hyphenation_settings(&mut pkg, &HyphenationSettings { auto: true, caps: true, zone: None, limit: 0.0 }).is_err());
        assert!(write_even_and_odd_headers(&mut pkg, true).is_err());
        assert!(write_display_background_shape(&mut pkg, true).is_err());
    });
    // a relationships part that is not XML: the settings part lookup fails first
    let bad_rels = replace(&parts, "word/_rels/document.xml.rels", b"<Relationships><Relationship");
    assert_no_leak(|| {
        let mut pkg = package(&bad_rels);
        black_box(write_default_tab_stop(&mut pkg, 720.0).is_err());
        black_box(write_even_and_odd_headers(&mut pkg, true).is_err());
    });
    // a nested part deeper than the limit
    let deep = format!(
        "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}{}</w:settings>",
        "<w:a>".repeat(1500),
        "</w:a>".repeat(1500)
    );
    let deep = replace(&parts, SETTINGS, deep.as_bytes());
    assert_no_leak(|| {
        let mut pkg = package(&deep);
        assert!(write_default_tab_stop(&mut pkg, 720.0).is_err());
    });
}

#[test]
fn m1_string_functions_return_the_memory() {
    assert_no_leak(|| black_box(note_props_inner(None)));
    assert_no_leak(|| {
        let p = NoteProps { pos: Some("pageBottom".into()), num_fmt: Some("decimal".into()), ..NoteProps::default() };
        black_box(note_props_xml("footnote", Some(&p)));
        black_box(same_note_props(Some(&p), None));
    });
    assert_no_leak(|| black_box(empty_notes_part("endnote")));
    assert_no_leak(|| {
        black_box(note_styles("footnote").map(|s| s.text.xml.len()));
        black_box(note_styles("other").is_none());
        black_box(FOOTNOTE_STYLES.r#ref.id.len() + ENDNOTE_STYLES.char.id.len());
        black_box(note_mark_field("endnote"));
    });
    assert_no_leak(|| {
        black_box(note_ref_run("footnote", 3.0, Some("Ref")));
        black_box(note_ref_run("endnote", f64::NAN, None));
        let mut ids = IdAlloc::new();
        black_box(new_note_paragraphs("footnote", Some("Text"), None, &mut ids));
        black_box(new_note_paragraphs("endnote", None, Some(""), &mut ids));
    });
    assert_no_leak(|| black_box(hf_part_xml("header", "<w:p/>")));
    assert_no_leak(|| black_box(hf_part_xml("other", "")));
    assert_no_leak(|| {
        let mut c = 0.0;
        black_box(copy_hf_part_xml(
            "<wp:docPr id=\"1\" name=\"a\"/><w:p w14:paraId=\"0A\" w14:textId=\"1B\"/><wp:docPr descr=\" id=&quot;\" id=\"22\"/>",
            &mut || {
                c += 1.0;
                c
            },
        ))
    });
    assert_no_leak(|| {
        black_box(remove_override("<Override PartName=\"/word/h.xml\" ContentType=\"a\"/>", "word/h.xml"));
        black_box(hf_target_path("./header1.xml"));
        black_box(is_hf_relationship("Type=\"x/relationships/header\""));
        black_box(quoted_attr("<Relationship Id=\"a\" Target=\"b\"/>", "Target"));
        black_box(is_page_number_gallery("<w:docPartGallery w:val=\"Page Numbers (Top)\"/>"));
        black_box(val_pattern_test("w:val=\"1ee21\"", "1e+21"));
        black_box(val_is_off("w:val=\"off\""));
    });
    assert_no_leak(|| {
        let ids: HashSet<String> = ["a".to_owned()].into_iter().collect();
        black_box(drop_hf_text(
            "<Relationships><Relationship Id=\"a\" Type=\"x/relationships/header\" Target=\"h.xml\"/></Relationships>",
            "<Types><Override PartName=\"/word/h.xml\" ContentType=\"a\"/></Types>",
            &ids,
        ))
    });
}

#[test]
fn m1_page_number_group_ids_return_the_memory() {
    let mut n = 0;
    for (_, parts) in synthetic_inputs().iter().step_by(5) {
        let pkg = synthetic_package(parts);
        let Ok(doc) = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }) else { continue };
        let Some(store) = doc.sources.as_ref() else { continue };
        assert_no_leak(|| {
            for (_, story) in store.stories.iter() {
                black_box(page_number_group_ids(Some(story), &store.parts));
            }
            black_box(page_number_group_ids(None, &store.parts));
        });
        n += 1;
    }
    assert!(n > 30);
}

/// M3: the text built by the string functions against its size. Budget: allocated ≤ 4 × the output + 64 KiB.
#[test]
fn m3_string_budget() {
    let all = inputs();
    let mut total_out = 0usize;
    let mut total_alloc = 0u64;
    for (_, parts) in all.iter().step_by(3) {
        let Some((_, s)) = parts.iter().find(|(n, _)| n == SETTINGS) else { continue };
        let text = dec(s);
        let (out, u) = measure(|| patch_settings_notes(&text, NotePropsArg::Unchanged, NotePropsArg::Unchanged));
        total_out += out.len();
        total_alloc += u.allocated;
        black_box(out);
    }
    assert!(total_alloc <= 4 * total_out as u64 + 64 * 1024 * all.len() as u64, "allocated {total_alloc} for {total_out} bytes");
}
