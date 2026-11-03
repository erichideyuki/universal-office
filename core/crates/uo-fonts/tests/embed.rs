//! The embedding plan: the two phases (the requests the shell must fetch, then the plan over the
//! faces it answers), the skip rules in the TS order, the subset decision and the failures that
//! reject the plan. Fonts are built in code (`frontier_d_support`).
#![allow(clippy::unwrap_used, clippy::panic)]

mod frontier_d_support;

use std::cell::RefCell;
use std::sync::Arc;

use frontier_d_support::{TestFont, directory, table};
use uo_alloc::assert_no_leak;
use uo_fonts::embed::{EmbeddingPolicy, FaceRequest, FontUsage};
use uo_fonts::{
    EmbeddingPlan, FontError, FontLimits, FxHashSet, POWERPOINT_COMMON_FONTS, SkipReason, Variant, WORD_COMMON_FONTS, embedding_requests,
    plan_font_embedding,
};
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

const LIMITS: FontLimits = FontLimits::DEFAULT;

fn word<'a>() -> EmbeddingPolicy<'a> {
    EmbeddingPolicy { embed: true, subset: false, skip_common_fonts: true, common_fonts: &WORD_COMMON_FONTS, all_variants: false }
}

fn powerpoint<'a>() -> EmbeddingPolicy<'a> {
    EmbeddingPolicy { embed: true, subset: true, skip_common_fonts: true, common_fonts: &POWERPOINT_COMMON_FONTS, all_variants: true }
}

fn none() -> FxHashSet<String> {
    FxHashSet::default()
}

fn names(requests: &[FaceRequest]) -> Vec<(String, Variant)> {
    requests.iter().map(|r| (r.family.clone(), r.variant)).collect()
}

fn font(family: &str) -> Arc<[u8]> {
    Arc::from(TestFont { family: family.to_owned(), ..TestFont::default() }.build())
}

#[test]
fn nothing_is_requested_or_planned_without_embedding() {
    let usage = [FontUsage { family: "Calibri", variants: &[Variant::Regular], text: "abc" }];
    let policy = EmbeddingPolicy { embed: false, ..word() };
    assert!(embedding_requests(&usage, &policy, &none()).is_empty());
    let plan = plan_font_embedding(&usage, &policy, &none(), &|_| panic!("no face is fetched"), &LIMITS).unwrap();
    assert!(plan.faces.is_empty() && plan.skipped.is_empty());
}

#[test]
fn word_asks_for_the_used_variants_in_variants_order_and_groups_by_lower_case_family() {
    let usage = [
        FontUsage { family: "Calibri", variants: &[Variant::Bold], text: "ab" },
        FontUsage { family: "Cambria", variants: &[Variant::Regular], text: "c" },
        FontUsage { family: "CALIBRI", variants: &[Variant::Regular, Variant::BoldItalic], text: "bd" },
        FontUsage { family: "", variants: &[Variant::Regular], text: "x" },
        FontUsage { family: "calibri", variants: &[Variant::Bold], text: "" },
    ];
    let requests = embedding_requests(&usage, &word(), &none());
    assert_eq!(
        names(&requests),
        [
            ("Calibri".to_owned(), Variant::Regular),
            ("Calibri".to_owned(), Variant::Bold),
            ("Calibri".to_owned(), Variant::BoldItalic),
            ("Cambria".to_owned(), Variant::Regular),
        ],
        "the first spelling is kept, the variants are in VARIANTS order"
    );
}

#[test]
fn common_fonts_and_already_embedded_ones_are_skipped_in_the_ts_order() {
    let usage = [
        FontUsage { family: "Arial", variants: &[Variant::Regular], text: "a" },
        FontUsage { family: "Times New Roman", variants: &[Variant::Regular], text: "a" },
        FontUsage { family: "Calibri", variants: &[Variant::Regular], text: "a" },
        FontUsage { family: "Cambria", variants: &[Variant::Regular], text: "a" },
    ];
    let mut already = none();
    already.insert("times new roman".to_owned());
    already.insert("cambria".to_owned());
    let requests = embedding_requests(&usage, &word(), &already);
    assert_eq!(names(&requests), [("Calibri".to_owned(), Variant::Regular)], "Word leaves out only its three common fonts");
    let plan = plan_font_embedding(&usage, &word(), &already, &|_| None, &LIMITS).unwrap();
    assert_eq!(
        plan.skipped,
        [
            ("Arial".to_owned(), SkipReason::Common),
            ("Times New Roman".to_owned(), SkipReason::AlreadyEmbedded),
            ("Calibri".to_owned(), SkipReason::Unavailable),
            ("Cambria".to_owned(), SkipReason::AlreadyEmbedded),
        ],
        "already-embedded is checked before the common fonts"
    );
    let keep_common = EmbeddingPolicy { skip_common_fonts: false, ..word() };
    assert_eq!(embedding_requests(&usage[..2], &keep_common, &none()).len(), 2);
}

#[test]
fn powerpoint_asks_for_all_four_variants_and_never_embeds_its_common_fonts() {
    let usage = [
        FontUsage { family: "Calibri", variants: &[Variant::Regular], text: "a" },
        FontUsage { family: "Aptos", variants: &[Variant::Bold], text: "a" },
        FontUsage { family: "Lato", variants: &[Variant::Italic], text: "a" },
    ];
    let requests = embedding_requests(&usage, &powerpoint(), &none());
    assert_eq!(
        names(&requests),
        [
            ("Lato".to_owned(), Variant::Regular),
            ("Lato".to_owned(), Variant::Bold),
            ("Lato".to_owned(), Variant::Italic),
            ("Lato".to_owned(), Variant::BoldItalic),
        ]
    );
}

#[test]
fn the_plan_asks_for_exactly_the_requested_faces() {
    let usage = [
        FontUsage { family: "Calibri", variants: &[Variant::Regular, Variant::Bold], text: "ab" },
        FontUsage { family: "Lato", variants: &[Variant::Italic], text: "c" },
    ];
    let asked = RefCell::new(Vec::new());
    let plan = plan_font_embedding(
        &usage,
        &word(),
        &none(),
        &|r| {
            asked.borrow_mut().push(r.clone());
            Some(font(&r.family))
        },
        &LIMITS,
    )
    .unwrap();
    assert_eq!(asked.into_inner(), embedding_requests(&usage, &word(), &none()));
    assert_eq!(plan.faces.len(), 3);
    assert_eq!(
        plan.faces.iter().map(|f| (f.family.as_str(), f.variant)).collect::<Vec<_>>(),
        [("Calibri", Variant::Regular), ("Calibri", Variant::Bold), ("Lato", Variant::Italic)]
    );
    assert!(plan.faces.iter().all(|f| !f.subsetted) && plan.skipped.is_empty());
    assert_eq!(plan.faces[0].info.names.family, "Calibri".encode_utf16().collect::<Vec<_>>());
}

#[test]
fn a_family_with_no_file_is_unavailable_and_one_with_only_restricted_files_is_a_licence_skip() {
    let usage = [
        FontUsage { family: "Gone", variants: &[Variant::Regular, Variant::Bold], text: "a" },
        FontUsage { family: "Locked", variants: &[Variant::Regular, Variant::Bold], text: "a" },
        FontUsage { family: "Mixed", variants: &[Variant::Regular, Variant::Bold], text: "a" },
        FontUsage { family: "Bitmap", variants: &[Variant::Regular], text: "a" },
    ];
    let restricted: Arc<[u8]> = Arc::from(TestFont { fs_type: 0x0002, ..TestFont::default() }.build());
    let bitmap_only: Arc<[u8]> = Arc::from(TestFont { fs_type: 0x0200, ..TestFont::default() }.build());
    let open = font("Mixed");
    let plan = plan_font_embedding(
        &usage,
        &word(),
        &none(),
        &|r| match (r.family.as_str(), r.variant) {
            ("Locked", _) => Some(restricted.clone()),
            ("Mixed", Variant::Regular) => Some(open.clone()),
            ("Mixed", _) => Some(restricted.clone()),
            ("Bitmap", _) => Some(bitmap_only.clone()),
            _ => None,
        },
        &LIMITS,
    )
    .unwrap();
    assert_eq!(plan.faces.len(), 1);
    assert_eq!(plan.faces[0].family, "Mixed");
    assert_eq!(
        plan.skipped,
        [
            ("Gone".to_owned(), SkipReason::Unavailable),
            ("Locked".to_owned(), SkipReason::License),
            ("Bitmap".to_owned(), SkipReason::License)
        ]
    );
}

#[test]
fn a_subset_keeps_the_glyphs_of_the_text_and_the_layout_tables() {
    let usage = [FontUsage { family: "Lato", variants: &[Variant::Regular], text: "AC\u{1F600}" }];
    let source = font("Lato");
    let policy = EmbeddingPolicy { subset: true, ..word() };
    let plan = plan_font_embedding(&usage, &policy, &none(), &|_| Some(source.clone()), &LIMITS).unwrap();
    let face = &plan.faces[0];
    assert!(face.subsetted);
    assert!(face.bytes.len() < source.len() || directory(&face.bytes).len() != directory(&source).len());
    assert!(table(&face.bytes, b"GPOS").is_some() && table(&face.bytes, b"DSIG").is_none());
    let loca = table(&face.bytes, b"loca").unwrap();
    let glyf = table(&face.bytes, b"glyf").unwrap();
    let at = |i: usize| u32::from_be_bytes([loca[i * 4], loca[i * 4 + 1], loca[i * 4 + 2], loca[i * 4 + 3]]) as usize;
    let size = |g: usize| at(g + 1) - at(g);
    assert!(size(1) > 0 && size(3) > 0, "A and C map to glyphs 1 and 3 in the test cmap");
    assert_eq!(size(2), 0, "B is not in the text");
    assert!(glyf.len() < table(&source, b"glyf").unwrap().len());
    assert_eq!(face.info.panose, "020B0502040204020203");
}

#[test]
fn without_outlines_or_without_the_subset_option_the_source_bytes_are_shared() {
    let usage = [FontUsage { family: "Lato", variants: &[Variant::Regular], text: "A" }];
    let source = font("Lato");
    let whole = plan_font_embedding(&usage, &word(), &none(), &|_| Some(source.clone()), &LIMITS).unwrap();
    assert!(Arc::ptr_eq(&whole.faces[0].bytes, &source) && !whole.faces[0].subsetted);
    let no_glyf: Vec<SfntTable> = TestFont::default().tables().into_iter().filter(|t| &t.tag != b"glyf" && &t.tag != b"loca").collect();
    let cff: Arc<[u8]> = Arc::from(build_sfnt(&no_glyf, &SfntOptions { version: 0x4F54_544F, ..SfntOptions::default() }));
    let policy = EmbeddingPolicy { subset: true, ..word() };
    let plan = plan_font_embedding(&usage, &policy, &none(), &|_| Some(cff.clone()), &LIMITS).unwrap();
    assert!(Arc::ptr_eq(&plan.faces[0].bytes, &cff));
    assert!(!plan.faces[0].subsetted, "no TrueType outlines: nothing to cut");
}

#[test]
fn subsetted_records_the_intention_even_when_the_subsetter_gave_the_input_back() {
    let usage = [FontUsage { family: "Lato", variants: &[Variant::Regular], text: "A" }];
    let tables: Vec<SfntTable> = TestFont::default().tables().into_iter().filter(|t| &t.tag != b"maxp").collect();
    let uncuttable: Arc<[u8]> = Arc::from(build_sfnt(&tables, &SfntOptions::default()));
    let policy = EmbeddingPolicy { subset: true, ..word() };
    let plan = plan_font_embedding(&usage, &policy, &none(), &|_| Some(uncuttable.clone()), &LIMITS).unwrap();
    assert!(plan.faces[0].subsetted);
    assert!(Arc::ptr_eq(&plan.faces[0].bytes, &uncuttable));
}

#[test]
fn a_malformed_cmap_or_name_table_rejects_the_whole_plan() {
    let usage = [
        FontUsage { family: "Good", variants: &[Variant::Regular], text: "A" },
        FontUsage { family: "Bad", variants: &[Variant::Regular], text: "A" },
    ];
    let mut cmap = BeWriter::new();
    cmap.u16(0).u16(60_000);
    let tables: Vec<SfntTable> = TestFont::default()
        .tables()
        .into_iter()
        .map(|t| if &t.tag == b"cmap" { SfntTable::new(b"cmap", cmap.as_bytes().to_vec()) } else { t })
        .collect();
    let mut with_bad_cmap = build_sfnt(&tables, &SfntOptions::default());
    let cmap_entry = directory(&with_bad_cmap).iter().position(|t| &t.0 == b"cmap").unwrap();
    let at = 12 + 16 * cmap_entry + 8;
    let end = u32::try_from(with_bad_cmap.len() - 2).unwrap();
    with_bad_cmap[at..at + 4].copy_from_slice(&end.to_be_bytes());
    let bad: Arc<[u8]> = Arc::from(with_bad_cmap);
    let good = font("Good");
    let policy = EmbeddingPolicy { subset: true, ..word() };
    let result =
        plan_font_embedding(&usage, &policy, &none(), &|r| Some(if r.family == "Bad" { bad.clone() } else { good.clone() }), &LIMITS);
    assert!(result.is_err(), "a glyph_ids_for failure fails the plan, as the TS throw does");
    let ok = plan_font_embedding(&usage, &word(), &none(), &|r| Some(if r.family == "Bad" { bad.clone() } else { good.clone() }), &LIMITS)
        .unwrap();
    assert_eq!(ok.faces.len(), 2, "without the subset the cmap is never read");
    let tiny: Arc<[u8]> = Arc::from(vec![0u8, 1, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let info_fails = plan_font_embedding(&usage[1..], &word(), &none(), &|_| Some(tiny.clone()), &LIMITS);
    assert!(matches!(info_fails, Err(FontError::OutOfBounds)), "face_info on a directory cut short rejects the plan: {info_fails:?}");
}

#[test]
fn m1_every_path_leaks_nothing() {
    let usage = [
        FontUsage { family: "Calibri", variants: &[Variant::Regular, Variant::Bold], text: "AB" },
        FontUsage { family: "arial", variants: &[Variant::Regular], text: "a" },
        FontUsage { family: "Lato", variants: &[Variant::Italic], text: "C" },
    ];
    let source = font("x");
    let policy = EmbeddingPolicy { subset: true, ..word() };
    let mut already = none();
    already.insert("lato".to_owned());
    assert_no_leak(|| embedding_requests(&usage, &word(), &none()));
    assert_no_leak(|| embedding_requests(&usage, &powerpoint(), &already));
    assert_no_leak(|| embedding_requests(&usage, &EmbeddingPolicy { embed: false, ..word() }, &none()));
    assert_no_leak(|| plan_font_embedding(&usage, &policy, &none(), &|_| Some(source.clone()), &LIMITS).unwrap());
    assert_no_leak(|| plan_font_embedding(&usage, &powerpoint(), &already, &|_| None, &LIMITS).unwrap());
    let tiny: Arc<[u8]> = Arc::from(vec![0u8, 1, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_no_leak(|| plan_font_embedding(&usage, &word(), &none(), &|_| Some(tiny.clone()), &LIMITS).is_err());
    assert_no_leak(EmbeddingPlan::default);
}
