//! Unit tests of the D1b font-embedding step (`write::fonts`): the one-pass rewrites against the
//! literal TS loops they replace (random texts built to hit the joins), the round trip read → write
//! on → read → write off → read on a minimal package (the `.odttf` parts de-obfuscate to the font with
//! the `w:fontKey` written next to them), the substitution cap, and the small pieces.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::DocxError;
use uo_docx::write::fonts::docx::{find_font_element_ci, font_entries};
use uo_docx::write::fonts::ooxml::{
    FontPartNames, find_element, fonts09_target, group_by_family, guid_of, remove_elements, replace_range, substitute, unesc_attr,
};
use uo_docx::write::fonts::{StoredEmbeddingSettings, read_docx_embedding, word_embedding_policy, write_docx_embedding};
use uo_fonts::{EmbeddingPlan, FaceToEmbed, FontEmbeddingSettings, Variant, WORD_DEFAULT_EMBEDDING, face_info};
use uo_opc::package::OpenOptions;
use uo_opc::{Limits, Package};
use uo_testkit::generate::Rng;

/// The TS loop over `findElement`, literally (`String` re-slicing).
fn remove_elements_naive(xml: &str, qname: &str) -> String {
    let mut x = xml.to_owned();
    while let Some((s, e)) = find_element(&x, qname) {
        x = format!("{}{}", &x[..s], &x[e..]);
    }
    x
}

#[test]
fn remove_elements_is_the_ts_loop() {
    let pieces = ["<ab", "<a", "b", "/>", ">", "</ab>", " ", "x", "<ab/>", "<ab>", "é", "\u{3000}", "/", "<", "</a"];
    let mut rng = Rng::new(0x5eed_0f0e);
    let mut joins = 0;
    for _ in 0..40_000 {
        let mut s = String::new();
        for _ in 0..rng.below(16) {
            s.push_str(pieces[rng.below(pieces.len())]);
        }
        let want = remove_elements_naive(&s, "ab");
        let got = remove_elements(&s, "ab");
        assert_eq!(got, want, "on {s:?}");
        // a removal that creates a new element out of the text around it
        if want.matches("<ab").count() > 0 && s.matches("<ab").count() > want.matches("<ab").count() + 1 {
            joins += 1;
        }
    }
    assert!(joins > 100, "the generator hits the joins ({joins})");
    // the straddle written out
    assert_eq!(remove_elements("<a<ab/>b/>x", "ab"), "x");
    assert_eq!(remove_elements("<a<ab/>b>y</ab>z", "ab"), "z");
}

fn unesc_naive(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&apos;", "'").replace("&amp;", "&")
}

#[test]
fn unescape_is_the_chained_replaces() {
    let pieces = ["&", "amp;", "quot;", "lt;", "gt;", "apos;", "&amp;", "&lt;", "x", ";", "é"];
    let mut rng = Rng::new(7);
    for _ in 0..20_000 {
        let s: String = (0..rng.below(10)).map(|_| pieces[rng.below(pieces.len())]).collect();
        assert_eq!(unesc_attr(&s), unesc_naive(&s), "on {s:?}");
    }
}

#[test]
fn substitutions_follow_get_substitution() {
    assert_eq!(substitute("a$$b$&c$`d$'e$1$<$", "M", "B", "A"), "a$bMcBdAe$1$<$");
    assert_eq!(replace_range("xxMyy", 2, 3, "[$`|$&|$']", 1000).unwrap(), "xx[xx|M|yy]yy");
    // over the cap: refused before anything is built
    assert!(replace_range("0123456789", 0, 1, "$`$`$`$'$'$'", 20).is_none());
    assert!(replace_range("0123456789", 0, 1, "$`$`$`$'$'$'", 64).is_some());
}

#[test]
fn guid_and_grouping() {
    let g = guid_of([0xff; 16]);
    assert_eq!(g, "{FFFFFFFF-FFFF-4FFF-BFFF-FFFFFFFFFFFF}");
    assert_eq!(guid_of([0; 16]), "{00000000-0000-4000-8000-000000000000}");
    let face = |family: &str, variant| FaceToEmbed {
        family: family.into(),
        variant,
        bytes: Arc::from(&[][..]),
        subsetted: false,
        info: face_info(&font()).unwrap(),
    };
    let faces = [
        face("B", Variant::BoldItalic),
        face("A", Variant::Bold),
        face("B", Variant::Regular),
        face("b", Variant::Italic),
        face("A", Variant::Regular),
    ];
    let g = group_by_family(&faces);
    let shape: Vec<(&str, Vec<Variant>)> = g.iter().map(|(f, v)| (*f, v.iter().map(|x| x.variant).collect())).collect();
    assert_eq!(
        shape,
        vec![
            ("B", vec![Variant::Regular, Variant::BoldItalic]),
            ("A", vec![Variant::Regular, Variant::Bold]),
            ("b", vec![Variant::Italic])
        ]
    );
}

#[test]
fn legacy_target_rule_and_part_names() {
    assert_eq!(fonts09_target("word/fontTable.xml", "fonts/font1.odttf"), "word/fonts/font1.odttf");
    assert_eq!(fonts09_target("word/fontTable.xml", "/x/y"), "x/y");
    assert_eq!(fonts09_target("word/fontTable.xml", "../../../a/./b//c"), "a/b//c");
    let pkg = Package::from_parts(
        [("word/fonts/FONT1.odttf".to_owned(), vec![1]), ("word/fonts/font3.odttf".to_owned(), vec![1])],
        &Limits::DEFAULT,
        &OpenOptions::default(),
    );
    let mut names = FontPartNames::new(&pkg);
    let taken: Vec<String> = (0..3).map(|_| names.take("word/fonts/", "odttf")).collect();
    assert_eq!(taken, ["word/fonts/font2.odttf", "word/fonts/font4.odttf", "word/fonts/font5.odttf"]);
}

#[test]
fn font_table_matchers_on_their_edge_cases() {
    // a self-closing font takes the next one's content (TS)
    let t = "<w:font w:name=\"a\"/><w:font w:name=\"b\"><w:embedBold/></w:font>";
    let e = font_entries(t);
    assert_eq!(e.len(), 1);
    assert_eq!(&t[e[0].value.0..e[0].value.1], "a");
    assert!(t[e[0].content.0..e[0].content.1].contains("<w:embedBold/>"));
    // the family element folds non-ASCII case (`i` without `u`) but nothing into ASCII
    assert!(find_font_element_ci("<w:font w:name=\"ÜBER\"/>", "über").is_some());
    assert!(find_font_element_ci("<w:font w:name=\"ſ\"/>", "s").is_none());
    assert!(find_font_element_ci("<w:font w:name=\"\u{212a}\"/>", "k").is_none());
}

fn font() -> Vec<u8> {
    std::fs::read(uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR")).join("shared/fonts/files/carlito-regular.ttf")).unwrap()
}

fn minimal() -> Package {
    let parts = [
        (
            "[Content_Types].xml",
            "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>",
        ),
        (
            "word/document.xml",
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>",
        ),
    ];
    Package::from_parts(parts.map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec())), &Limits::DEFAULT, &OpenOptions::default())
}

fn plan(families: &[(&str, Variant)]) -> EmbeddingPlan {
    let bytes: Arc<[u8]> = Arc::from(font());
    let info = face_info(&bytes).unwrap();
    EmbeddingPlan {
        faces: families
            .iter()
            .map(|&(f, v)| FaceToEmbed {
                family: f.into(),
                variant: v,
                bytes: bytes.clone(),
                subsetted: v == Variant::Bold,
                info: info.clone(),
            })
            .collect(),
        skipped: Vec::new(),
    }
}

fn text(pkg: &Package, name: &str) -> String {
    pkg.part_text(name).unwrap().unwrap().into_owned()
}

#[test]
fn round_trip_on_then_off() {
    let mut pkg = minimal();
    let r = read_docx_embedding(&pkg).unwrap();
    assert_eq!(r.settings, StoredEmbeddingSettings { embed: false, subset: None, skip_common_fonts: None });
    assert_eq!(r.settings.over(WORD_DEFAULT_EMBEDDING), WORD_DEFAULT_EMBEDDING);
    let on = FontEmbeddingSettings { embed: true, subset: true, skip_common_fonts: false };
    let p = plan(&[("Carlito", Variant::Bold), ("A&B", Variant::Regular), ("Carlito", Variant::Regular)]);
    let mut n = 0u8;
    let mut guid = || {
        n += 1;
        [n; 16]
    };
    write_docx_embedding(&mut pkg, &p, &on, &mut guid).unwrap();
    let settings = text(&pkg, "word/settings.xml");
    assert!(settings.ends_with("<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:embedTrueTypeFonts/><w:embedSystemFonts/><w:saveSubsetFonts/></w:settings>"));
    let table = text(&pkg, "word/fontTable.xml");
    // Carlito first (first appearance), regular before bold; the second family escaped
    assert!(table.contains("<w:font w:name=\"Carlito\"><w:panose1 w:val=\""), "{table}");
    assert!(table.contains(
        "<w:embedRegular r:id=\"rId1\" w:fontKey=\"{01010101-0101-4101-8101-010101010101}\"/><w:embedBold r:id=\"rId2\" w:subsetted=\"1\""
    ));
    assert!(table.contains("<w:font w:name=\"A&amp;B\">"));
    let ct = text(&pkg, "[Content_Types].xml");
    assert!(ct.contains("<Default Extension=\"odttf\""));
    assert!(ct.contains("PartName=\"/word/fontTable.xml\"") && ct.contains("PartName=\"/word/settings.xml\""));
    let doc_rels = text(&pkg, "word/_rels/document.xml.rels");
    assert!(doc_rels.contains("Target=\"settings.xml\"") && doc_rels.contains("Target=\"fontTable.xml\""));
    // the parts de-obfuscate to the font with their key
    for (i, key) in ["{01010101-0101-4101-8101-010101010101}", "{02020202-0202-4202-8202-020202020202}"].iter().enumerate() {
        let part = pkg.part(&format!("word/fonts/font{}.odttf", i + 1)).unwrap().unwrap();
        assert_eq!(uo_fonts::odttf::deobfuscate(part, key).as_ref(), font().as_slice());
    }
    let r = read_docx_embedding(&pkg).unwrap();
    assert_eq!(r.settings, StoredEmbeddingSettings { embed: true, subset: Some(true), skip_common_fonts: Some(false) });
    assert_eq!(r.embedded, ["carlito", "a&b"]);
    assert_eq!(r.embedded_set().len(), 2);
    let policy = word_embedding_policy(&r.settings.over(WORD_DEFAULT_EMBEDDING));
    assert!(policy.embed && policy.subset && !policy.skip_common_fonts && !policy.all_variants);
    assert_eq!(policy.common_fonts, ["Arial", "Times New Roman", "Courier New"]);

    let off = FontEmbeddingSettings { embed: false, subset: false, skip_common_fonts: true };
    write_docx_embedding(&mut pkg, &p, &off, &mut || [0; 16]).unwrap();
    assert!(!pkg.part_names().any(|n| n.ends_with(".odttf")));
    assert!(!text(&pkg, "word/fontTable.xml").contains("<w:embed"));
    assert!(!text(&pkg, "word/_rels/fontTable.xml.rels").contains("Relationship "));
    assert!(!text(&pkg, "word/settings.xml").contains("embed"));
    let r = read_docx_embedding(&pkg).unwrap();
    assert!(!r.settings.embed && r.embedded.is_empty());
}

#[test]
fn a_dollar_in_a_family_name_is_a_substitution_pattern() {
    // the TS builds the new `<w:font>` as a `replace` template: `$`` is the text before `</w:fonts>`
    let mut pkg = minimal();
    let on = FontEmbeddingSettings { embed: true, subset: false, skip_common_fonts: true };
    write_docx_embedding(&mut pkg, &plan(&[("x$`y", Variant::Regular)]), &on, &mut || [0; 16]).unwrap();
    let table = text(&pkg, "word/fontTable.xml");
    let before = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">";
    assert!(table.starts_with(&format!("{before}<w:font w:name=\"x{before}y\">")), "{table}");
}

#[test]
fn errors_settings_without_root_and_the_cap() {
    let mut pkg = minimal();
    pkg.set_part_text(
        "word/_rels/document.xml.rels",
        "<Relationships><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/></Relationships>",
    );
    pkg.set_part_text("word/settings.xml", "<s:settings/>");
    let on = FontEmbeddingSettings { embed: true, subset: false, skip_common_fonts: true };
    assert!(matches!(write_docx_embedding(&mut pkg, &plan(&[]), &on, &mut || [0; 16]), Err(DocxError::Corrupt(_))));

    // `$`` in a family name doubles the font table each time: refused at the XML part cap
    let limits = Limits { max_xml_part_bytes: 64 << 10, ..Limits::DEFAULT };
    let base = minimal();
    let parts: Vec<(String, Vec<u8>)> = base.part_names().map(|n| (n.to_owned(), base.part(n).unwrap().unwrap().to_vec())).collect();
    let mut pkg = Package::from_parts(parts, &limits, &OpenOptions::default());
    let families: Vec<String> = (0..20).map(|i| format!("F{i} $`")).collect();
    let spec: Vec<(&str, Variant)> = families.iter().map(|f| (f.as_str(), Variant::Regular)).collect();
    let err = write_docx_embedding(&mut pkg, &plan(&spec), &on, &mut || [0; 16]).unwrap_err();
    assert!(matches!(err, DocxError::Package(uo_opc::Error::Limit { kind: uo_opc::error::LimitKind::PartBytes, .. })), "{err:?}");
}
