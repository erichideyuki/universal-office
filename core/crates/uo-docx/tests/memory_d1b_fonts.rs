//! Memory gate M1 of the D1b font-embedding step: `read_docx_embedding` and `write_docx_embedding`
//! (on, off, without faces, the TypeError path, the substitution cap) and the matchers return the
//! live bytes exactly to their value before the call, package and outputs dropped
//! (`uo_alloc::assert_no_leak`), over synthetic packages of `tests/parity/font-embedding.inputs` and
//! a corpus document. The package is built inside each measured closure (its lazily built
//! ASCII-folded name index is part of what must be returned). Bytes are printed (`--nocapture`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::fonts_support;

use std::hint::black_box;

use fonts_support::{fonts, guid_source, inputs, open_parts, plan_of};
use uo_alloc::{assert_no_leak, measure};
use uo_docx::write::fonts::docx::{find_font_element_ci, font_entries, setting_on, strip_embed_tags};
use uo_docx::write::fonts::ooxml::{read_rels_text, remove_elements, replace_range};
use uo_docx::write::fonts::{read_docx_embedding, write_docx_embedding};
use uo_fonts::FontEmbeddingSettings;
use uo_opc::Package;

#[test]
fn m1_read_and_write_return_the_memory() {
    let ins = inputs();
    let fonts = fonts();
    let mut checked = 0;
    for (key, parts) in ins.packages.iter().take(120) {
        let spec = &ins.plans[&format!("{key}#fonts")];
        for embed in [true, false] {
            let settings = FontEmbeddingSettings { embed, ..spec.settings };
            let plan = plan_of(spec, &fonts);
            assert_no_leak(|| {
                let mut pkg = open_parts(parts);
                black_box(read_docx_embedding(&pkg).ok());
                let mut guid = guid_source(spec.seed);
                black_box(write_docx_embedding(&mut pkg, &plan, &settings, &mut guid).is_ok());
                black_box(read_docx_embedding(&pkg).ok());
                drop(pkg);
            });
            checked += 1;
        }
    }
    println!("M1: {checked} read → write → read runs, 0 B left each");
}

#[test]
fn m1_error_paths_return_the_memory() {
    let fonts = fonts();
    let spec = fonts_support::PlanSpec {
        settings: FontEmbeddingSettings { embed: true, subset: true, skip_common_fonts: false },
        seed: 9,
        faces: (0..6)
            .map(|i| fonts_support::FaceSpec {
                family: format!("F{i} $`$`"),
                variant: uo_fonts::Variant::Regular,
                file: "carlito-regular.ttf".into(),
                subsetted: false,
                panose: "00".into(),
            })
            .collect(),
    };
    let plan = plan_of(&spec, &fonts);
    let base = vec![
        ("[Content_Types].xml".to_owned(), b"<Types></Types>".to_vec()),
        ("word/document.xml".to_owned(), b"<w:document/>".to_vec()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            b"<Relationships><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/></Relationships>"
                .to_vec(),
        ),
    ];
    // settings without `w:settings`: the TypeError path
    let mut no_root = base.clone();
    no_root.push(("word/settings.xml".to_owned(), b"<s:settings/>".to_vec()));
    let usage = assert_no_leak(|| {
        let mut pkg = open_parts(&no_root);
        assert!(write_docx_embedding(&mut pkg, &plan, &spec.settings, &mut guid_source(1)).is_err());
    });
    println!("M1 TypeError path: peak {} B", usage.peak_above_start);
    // the substitution cap
    let limits = uo_opc::Limits { max_xml_part_bytes: 32 << 10, ..uo_opc::Limits::DEFAULT };
    let usage = assert_no_leak(|| {
        let mut pkg = Package::from_parts(base.clone(), &limits, &uo_opc::package::OpenOptions::default());
        assert!(write_docx_embedding(&mut pkg, &plan, &spec.settings, &mut guid_source(1)).is_err());
    });
    println!("M1 cap path: peak {} B", usage.peak_above_start);
}

#[test]
fn m1_matchers_return_the_memory() {
    let table =
        "<w:fonts><w:font w:name=\"A\"><w:embedRegular r:id=\"rId1\"/></w:font><W:FONT W:NAME=\"über\"/><w:font w:name=\"x\">".repeat(50);
    assert_no_leak(|| {
        black_box(font_entries(&table));
        black_box(find_font_element_ci(&table, "ÜBER"));
        black_box(strip_embed_tags(&table));
        black_box(setting_on(&table, "embedRegular"));
        black_box(remove_elements(&table, "w:font"));
        black_box(read_rels_text("<Relationship Id=\"a\" Target=\"b&amp;\"/>"));
        black_box(replace_range(&table, 0, 5, "$`$'", u64::MAX));
    });
    let (_, usage) = measure(|| remove_elements(&table, "w:font"));
    println!("remove_elements over {} B: allocated {} B, peak {} B", table.len(), usage.allocated, usage.peak_above_start);
}
