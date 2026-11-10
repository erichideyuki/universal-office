//! No panic in the D1b frontier W4/W4b (`write::emit::{notes, settings}`) on hostile input: the corpus mini
//! packages (`tests/parity/settings-xml.inputs`) with their settings, relationships, content types and
//! header/footer parts mutated (bytes flipped, cut, duplicated, `>` removed, attributes doubled), through
//! every writer, and finishing in linear time on hostile texts and models (two to three million characters
//! without a `>`, as many unclosed `<wp:docPr`, relationships, overrides, `w:val="` followed by a long run
//! of `e`, a hundred thousand relationships to drop, story groups nested two thousand deep and two hundred
//! thousand wide).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

use uo_docx::js::JsNumber;
use uo_docx::model::sources::{SourcePart, SrcText, StoryGroup, StorySource};
use uo_docx::write::emit::notes::{NotePropsArg, ensure_note_parts, patch_settings_notes, settings_props_element};
use uo_docx::write::emit::settings::{
    HyphenationSettings, copy_hf_part_xml, drop_hf_text, drop_unused_hf_parts, page_number_group_ids, remove_override,
    self_closing_matches, val_pattern_test, write_default_tab_stop, write_display_background_shape, write_even_and_odd_headers,
    write_hyphenation_settings,
};
use uo_opc::Package;
use uo_testkit::generate::Rng;

#[allow(unused_imports)]
use crate::w4_support;

use w4_support::{Part, dec, inputs, package};

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut s: Vec<u8> = text.as_bytes().to_vec();
    for _ in 0..1 + rng.below(4) {
        if s.is_empty() {
            break;
        }
        let at = rng.below(s.len());
        match rng.below(8) {
            0 => s[at] = b"<>/\"='&w: x"[rng.below(11)],
            1 => s.truncate(at),
            2 => {
                let end = (at + 1 + rng.below(200)).min(s.len());
                let dup = s[at..end].to_vec();
                s.splice(at..at, dup);
            }
            3 => {
                if let Some(k) = s[at..].iter().position(|&b| b == b'>') {
                    s.remove(at + k);
                }
            }
            4 => {
                let ins: &[u8] = [
                    &b" w:val=\"0\""[..],
                    b"<w:footnotePr w:x=\"1\"/>",
                    b"</w:footnotePr>",
                    b"<w:evenAndOddHeaders/>",
                    b"<wp:docPr id=\"1\"",
                    b"$&",
                    b"\xF0\x9F\x98\x80",
                ][rng.below(7)];
                s.splice(at..at, ins.iter().copied());
            }
            5 => {
                let end = (at + rng.below(64)).min(s.len());
                s.drain(at..end);
            }
            6 => {
                let ins: &[u8] = [
                    &b"<Relationship Id=\"rId1\" Type=\"x/relationships/header\" Target=\"header1.xml\"/>"[..],
                    b"<w:compat",
                    b"<Override PartName=\"/word/header1.xml\"",
                    b"xmlns:w=\"x\"",
                ][rng.below(4)];
                s.splice(at..at, ins.iter().copied());
            }
            _ => s[at] = s[at].wrapping_add(1),
        }
    }
    String::from_utf8_lossy(&s).into_owned()
}

fn run_all(pkg: &mut Package, rng: &mut Rng) {
    black_box(write_default_tab_stop(pkg, [720.0, 567.4, f64::NAN, 1e21, -3.0][rng.below(5)]).is_ok());
    black_box(
        write_hyphenation_settings(
            pkg,
            &HyphenationSettings {
                auto: rng.one_in(2),
                caps: rng.one_in(2),
                zone: rng.one_in(2).then_some(360.0),
                limit: rng.below(4) as f64,
            },
        )
        .is_ok(),
    );
    black_box(write_even_and_odd_headers(pkg, rng.one_in(2)).is_ok());
    black_box(write_display_background_shape(pkg, rng.one_in(2)).is_ok());
    black_box(ensure_note_parts(pkg).is_ok());
    let ids: HashSet<String> = ["rId1", "rId2", "rId3", "rId4"].iter().filter(|_| rng.one_in(2)).map(|s| (*s).to_owned()).collect();
    black_box(drop_unused_hf_parts(pkg, &ids).is_ok());
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    for n in names {
        if (n.starts_with("word/header") || n.starts_with("word/footer")) && !n.contains("_rels") {
            let text = pkg.part_text(&n).unwrap().map(std::borrow::Cow::into_owned).unwrap_or_default();
            let mut c = 0.0;
            black_box(copy_hf_part_xml(&text, &mut || {
                c += 1.0;
                c
            }));
        }
    }
    let settings = pkg.part_text("word/settings.xml").unwrap().map(std::borrow::Cow::into_owned).unwrap_or_default();
    black_box(patch_settings_notes(&settings, NotePropsArg::Unchanged, NotePropsArg::Remove));
}

#[test]
fn hostile_packages_do_not_panic() {
    let all = inputs();
    let mut rng = Rng::new(0x57a4_bad5);
    let mut n = 0;
    for _ in 0..1500 {
        let (_, parts) = &all[rng.below(all.len())];
        let mut parts: Vec<Part> = parts.clone();
        for (name, data) in &mut parts {
            let hot = name == "word/settings.xml" || name == "word/_rels/document.xml.rels" || name == "[Content_Types].xml";
            if (hot || name.starts_with("word/header")) && rng.below(3) != 0 {
                *data = mutate(&mut rng, &dec(data)).into_bytes();
            }
        }
        let mut pkg = package(&parts);
        run_all(&mut pkg, &mut rng);
        n += 1;
    }
    assert_eq!(n, 1500);
}

fn timed<R>(what: &str, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    let e = t.elapsed();
    assert!(e.as_secs() < 10, "{what} took {e:?}");
    r
}

#[test]
fn hostile_texts_finish_in_linear_time() {
    let big = |unit: &str, bytes: usize| unit.repeat(bytes / unit.len());
    let footnote = big("<w:footnotePr ", 2_500_000);
    timed("footnotePr", || {
        black_box(settings_props_element(&footnote, "w:footnotePr"));
        black_box(patch_settings_notes(&footnote, NotePropsArg::Unchanged, NotePropsArg::Unchanged).len());
    });
    let closed = big("<w:footnotePr w:x=\"1\"/>", 2_500_000);
    timed("footnotePr/", || black_box(patch_settings_notes(&closed, NotePropsArg::Remove, NotePropsArg::Remove).len()));
    let docpr = big("<wp:docPr ", 2_500_000);
    timed("docPr", || {
        let mut c = 0.0;
        black_box(copy_hf_part_xml(&docpr, &mut || {
            c += 1.0;
            c
        }))
        .len()
    });
    let docpr_ids = big("<wp:docPr descr=\"a\" id=\"1\"/>", 2_500_000);
    timed("docPr ids", || {
        let mut c = 0.0;
        black_box(copy_hf_part_xml(&docpr_ids, &mut || {
            c += 1.0;
            c
        }))
        .len()
    });
    let one_tag_many_docpr = format!("{}{}", big("<wp:docPr ", 1_000_000), "id=\"1\">");
    timed("docPr, one tag", || {
        let mut c = 0.0;
        black_box(copy_hf_part_xml(&one_tag_many_docpr, &mut || {
            c += 1.0;
            c
        }))
        .len()
    });
    let spaces = big(" w14:paraId=\"0A\"", 2_500_000);
    timed("w14 ids", || {
        let mut c = 0.0;
        black_box(copy_hf_part_xml(&spaces, &mut || {
            c += 1.0;
            c
        }))
        .len()
    });
    let rels = big("<Relationship ", 2_500_000);
    timed("relationships", || black_box(self_closing_matches(&rels, "Relationship").len()));
    let overrides = big("<Override PartName=\"/word/h.xml\"", 2_500_000);
    timed("overrides", || black_box(remove_override(&overrides, "word/h.xml").len()));
    let vals = format!("{}{}", big("w:val=\"1e", 1_000_000), "e".repeat(1_500_000));
    timed("val pattern", || black_box(val_pattern_test(&vals, "1e+21")));
    let compat = big("<w:compat", 2_500_000);
    timed("compat", || black_box(patch_settings_notes(&compat, NotePropsArg::Unchanged, NotePropsArg::Unchanged).len()));
    let refs = format!("<w:footnotePr>{}</w:footnotePr>", big("<w:footnote ", 2_500_000));
    timed("footnote refs", || black_box(patch_settings_notes(&refs, NotePropsArg::Unchanged, NotePropsArg::Unchanged).len()));
}

#[test]
fn many_relationships_to_drop_finish() {
    let mut rels = String::from("<Relationships>");
    let mut ct = String::from("<Types>");
    let mut ids = HashSet::new();
    for i in 0..20_000 {
        rels.push_str(&format!("<Relationship Id=\"rId{i}\" Type=\"x/relationships/header\" Target=\"header{i}.xml\"/>"));
        ct.push_str(&format!("<Override PartName=\"/word/header{i}.xml\" ContentType=\"a\"/>"));
        ids.insert(format!("rId{i}"));
    }
    rels.push_str("</Relationships>");
    ct.push_str("</Types>");
    let (r, c, gone) = timed("drop 20k", || drop_hf_text(&rels, &ct, &ids));
    assert_eq!(gone.len(), 20_000);
    assert_eq!(r, "<Relationships></Relationships>");
    assert_eq!(c, "<Types></Types>");
}

fn gallery(blocks: &[f64], inner: Option<Vec<StoryGroup>>, gallery: bool) -> StoryGroup {
    StoryGroup {
        raw: SrcText::default(),
        blocks: blocks.iter().map(|b| JsNumber::Num(*b)).collect(),
        wrap_open: gallery.then(|| SrcText::Owned("<w:docPartGallery w:val=\"Page Numbers (Top)\"/>".into())),
        inner,
        ..StoryGroup::default()
    }
}

#[test]
fn hostile_story_groups_finish() {
    let parts: Vec<SourcePart> = Vec::new();
    let mut deep = gallery(&[1.0], None, false);
    for i in 0..2000 {
        deep = gallery(&[f64::from(i)], Some(vec![deep]), i % 500 == 0);
    }
    let story = StorySource { groups: vec![deep], ..StorySource::default() };
    let ids = timed("deep groups", || page_number_group_ids(Some(&story), &parts));
    assert!(!ids.is_empty());
    let wide = StorySource {
        groups: (0..200_000).map(|i| gallery(&[f64::from(i), f64::NAN, 0.0], None, i % 3 == 0)).collect(),
        ..StorySource::default()
    };
    let ids = timed("wide groups", || page_number_group_ids(Some(&wide), &parts));
    assert_eq!(ids.len(), 66_666);
}
