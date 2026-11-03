//! M1 (stage 4, memory gate): the public operations the per-module memory tests
//! (`package_memory`, `zip_memory`, `flate_memory`, `cfb_memory`, `xml_memory`) did not reach —
//! part names, relationship and content-type helpers, raw surgery, escapes, tree accessors,
//! zip/text decoding and the read-only package queries — on success and on error paths. Every
//! one returns live bytes exactly to their starting value once its outputs are dropped.
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod cfb_support;
mod xml_support;

use cfb_support::{Spec, build, pattern, root_streams};
use uo_alloc::assert_no_leak;
use uo_opc::cfb::{self, CfbFile};
use uo_opc::content_types::{self, ContentTypes};
use uo_opc::flate;
use uo_opc::package::{self, CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::part_name::{escape_segment, fold_ascii, resolve_relationship_target};
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::raw::{self, Insert, Kid};
use uo_opc::xml::write::{self, Escape};
use uo_opc::xml::{ParseOptions, parse};
use uo_opc::zip::{self, WriteOptions};
use uo_opc::{Limits, clear_caches, limits};
use xml_support::{DOCX, DOCX_POS, SHEET, SLIDES};

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://x/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://e.com/?a=1&amp;b=2\" TargetMode=\"External\"/></Relationships>";
const TYPES: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"x\"/></Types>";

#[test]
fn m1_part_names() {
    clear_caches();
    assert_no_leak(|| resolve_relationship_target("/word/document.xml", "../media/%E2%82%AC image.png"));
    assert_no_leak(|| resolve_relationship_target("/word/document.xml", "%2e%2e/%FF"));
    assert_no_leak(|| resolve_relationship_target("/a", "http://x/y"));
    assert_no_leak(|| escape_segment("a b€%"));
    assert_no_leak(|| fold_ascii("Word/Document.XML"));
    assert_no_leak(|| limits::is_xml_like("word/_rels/document.xml.rels"));
    assert_no_leak(|| Limits::DEFAULT.part_cap("ppt/media/video.mp4"));
}

#[test]
fn m1_rels_helpers() {
    let list = rels::parse_rels(RELS, &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_no_leak(|| rels::rels_part_for("word/document.xml"));
    assert_no_leak(|| rels::rels_part_for_legacy("foo.xml"));
    assert_no_leak(|| rels::find_by_id(&list, "rId2").map(|r| r.target.clone()));
    assert_no_leak(|| rels::find_by_type_suffix(&list, "/styles").map(|r| r.id.clone()));
    assert_no_leak(rels::empty_rels);
    assert_no_leak(|| rels::next_rel_id_after_max(&list));
    assert_no_leak(|| rels::first_free_rel_id(&list, "rId", 1));
    assert_no_leak(|| Relationship::new("rId9", "t", "a&b.xml").external().to_xml());
    assert_no_leak(|| rels::write_rels(&list, write::XML_DECL_LF));
    assert_no_leak(|| rels::add_rel(None, &Relationship::new("rId1", "t", "x.xml"), Insert::EndOfRoot, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| {
        rels::add_rel(
            Some("<Relationships/>"),
            &Relationship::new("rId1", "t", "x.xml"),
            Insert::BeforeLastCloseTrimmed,
            &uo_opc::Limits::DEFAULT,
        )
        .unwrap()
    });
    assert_no_leak(|| rels::set_rel_target(RELS, &DOCX, &uo_opc::Limits::DEFAULT, "rId1", "other.xml").unwrap());
    assert_no_leak(|| rels::set_rel_target(RELS, &DOCX, &uo_opc::Limits::DEFAULT, "missing", "other.xml").unwrap());
    // error paths: the parse fails after part of the tree was built
    let broken = &RELS[..RELS.len() - 30];
    assert_no_leak(|| rels::parse_rels(broken, &DOCX, &uo_opc::Limits::DEFAULT).unwrap_err());
    assert_no_leak(|| rels::remove_rels(broken, &DOCX, &uo_opc::Limits::DEFAULT, |_| true).unwrap_err());
    assert_no_leak(|| rels::set_rel_target(broken, &DOCX, &uo_opc::Limits::DEFAULT, "rId1", "x").unwrap_err());
    assert_no_leak(|| {
        rels::add_rel(Some("<Relationships><a"), &Relationship::new("r", "t", "x"), Insert::EndOfRoot, &uo_opc::Limits::DEFAULT)
            .unwrap_err()
    });
}

#[test]
fn m1_content_types() {
    let ct = ContentTypes::parse(TYPES, &DOCX, &uo_opc::Limits::DEFAULT).unwrap();
    assert_no_leak(|| ct.override_of_exact("word/document.xml").map(str::len));
    assert_no_leak(|| ct.default_of("XML").map(str::len));
    assert_no_leak(|| ct.content_type_of_exact("word/styles.xml").map(str::len));
    assert_no_leak(|| content_types::default_xml("png", "image/png"));
    assert_no_leak(|| content_types::override_xml("a&b.xml", "x\"y"));
    assert_no_leak(|| content_types::add_default(TYPES, "png", "image/png", Insert::AfterRootOpen, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| {
        content_types::add_default(TYPES, "png", "image/png", Insert::BeforeFirst("Override"), &uo_opc::Limits::DEFAULT).unwrap()
    });
    assert_no_leak(|| content_types::add_override(TYPES, "a.xml", "t", Insert::BeforeFirstClose, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| content_types::remove_default(TYPES, &DOCX, &uo_opc::Limits::DEFAULT, "xml").unwrap());
    assert_no_leak(|| content_types::remove_default(TYPES, &DOCX, &uo_opc::Limits::DEFAULT, "bin").unwrap());
    let broken = "<Types><Default Extension=\"xml\"";
    assert_no_leak(|| ContentTypes::parse(broken, &DOCX, &uo_opc::Limits::DEFAULT).unwrap_err());
    assert_no_leak(|| content_types::remove_override(broken, &DOCX, &uo_opc::Limits::DEFAULT, "a").unwrap_err());
    assert_no_leak(|| content_types::add_default(broken, "png", "t", Insert::EndOfRoot, &uo_opc::Limits::DEFAULT).unwrap_err());
}

#[test]
fn m1_raw_surgery_and_escapes() {
    let kids = vec![Kid { name: "w:b".into(), raw: "<w:b/>".into(), el: None }, Kid { name: "w:i".into(), raw: "<w:i/>".into(), el: None }];
    assert_no_leak(|| raw::join_kids("<w:rPr>", &kids, "</w:rPr>"));
    assert_no_leak(|| {
        let mut k = kids.clone();
        raw::set_kid(&mut k, &["w:b", "w:i"], "w:b", None);
        raw::set_kid(&mut k, &["w:b", "w:i"], "w:i", Some("<w:i w:val=\"0\"/>"));
        k
    });
    assert_no_leak(|| raw::insert_child(TYPES, "Types", "<x/>", Insert::BeforeLastCloseTrimmed, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| raw::insert_child("<Types/>", "Types", "<x/>", Insert::EndOfRoot, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| raw::insert_child("<Other/>", "Types", "<x/>", Insert::AfterRootOpen, &uo_opc::Limits::DEFAULT).unwrap());
    assert_no_leak(|| raw::insert_child("<Types><a", "Types", "<x/>", Insert::BeforeFirst("a"), &uo_opc::Limits::DEFAULT).unwrap_err());
    assert_no_leak(|| raw::expand_self_closing("<Relationships xmlns=\"u\" />", "Relationships"));
    assert_no_leak(|| raw::remove_range(TYPES, 10, 20));
    assert_no_leak(|| raw::remove_range(TYPES, 10, 1 << 30));
    assert_no_leak(|| raw::with_attrs("<w:x/>", "w:y", &[("a", None)]));
    for e in [Escape::TEXT, Escape::ATTR, Escape::ATTR_NO_GT, Escape::ALL, Escape::TEXT_VALID, Escape::ATTR_VALID] {
        assert_no_leak(|| write::escape("a&b<c>\"d'\u{1}e", e).into_owned());
        assert_no_leak(|| write::escape("plain", e).len());
    }
    assert_no_leak(|| write::strip_invalid("a\u{1}\u{FFFE}b").into_owned());
    assert_no_leak(|| (write::esc_text("<").into_owned(), write::esc_attr("\"").into_owned(), write::esc_attr_no_gt(">").into_owned()));
    assert_no_leak(|| (write::esc_all("'").into_owned(), write::esc_text_valid("\u{0}").into_owned()));
    assert_no_leak(|| write::el("w:t", &[("xml:space", Some("preserve")), ("x", None)], "a"));
    assert_no_leak(|| {
        let mut s = String::new();
        write::write_attrs(&mut s, &[("a", Some("1&")), ("b", None)]);
        write::escape_into(&mut s, "<>", Escape::ALL);
        s
    });
    assert_no_leak(|| {
        let mut w = write::XmlWriter::new();
        w.start("a").attr_opt("b", None).attr_opt("c", Some("1")).raw("<raw/>").start("d").end().end();
        w.finish()
    });
}

#[test]
fn m1_tree_accessors() {
    let src = "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p w:rsidR=\"1\"><w:r><w:t>a&amp;b</w:t></w:r><w:drawing><x/></w:drawing></w:p><!--c--><w:p/></w:body></w:document>";
    let doc = parse(src, &DOCX_POS).unwrap();
    let root = doc.root();
    assert_no_leak(|| root.descendants().count());
    assert_no_leak(|| root.descend("w:body").map(|b| b.children_named("w:p").count()));
    assert_no_leak(|| root.descend("w:body").and_then(|b| b.first_child("w:p")).map(|p| p.attrs().count()));
    assert_no_leak(|| root.descend("w:body").and_then(|b| b.last_child("w:p")).map(|p| p.child_count()));
    assert_no_leak(|| root.descend("w:drawing").and_then(|d| d.raw()).map(str::len));
    assert_no_leak(|| root.descend("w:t").map(|t| t.text_content()));
    assert_no_leak(|| {
        root.children().filter_map(|n| n.as_element()).map(|e| e.local_name().len() + e.namespace().map_or(0, str::len)).sum::<usize>()
    });
    assert_no_leak(|| root.descend("w:p").and_then(|p| p.positions().map(|x| x.end)).unwrap_or(0));
    assert_no_leak(|| root.descend("w:p").and_then(|p| p.source_slice()).map(str::len));
    assert_no_leak(|| doc.name_id("w:p").map(|id| doc.name(id).len()));
    assert_no_leak(|| root.descend("w:p").and_then(|p| p.attr("w:rsidR")).map(str::len));
    let strict = parse(src, &ParseOptions::STRICT).unwrap();
    assert_no_leak(|| {
        strict
            .root()
            .descend("w:p")
            .and_then(|p| p.attr_ns("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "rsidR"))
            .map(str::len)
    });
    assert_no_leak(|| doc.heap_bytes());
    for o in [&SHEET, &SLIDES] {
        assert_no_leak(|| parse(src, o).unwrap().element_count());
    }
}

#[test]
fn m1_decoding_and_checksums() {
    let latin = [0x63u8, 0x61, 0x66, 0xE9];
    assert_no_leak(|| zip::decode_utf8(&latin).into_owned());
    assert_no_leak(|| zip::decode_latin1(&latin));
    assert_no_leak(|| package::decode_text(&[0xEF, 0xBB, 0xBF, b'a'], false).into_owned());
    assert_no_leak(|| package::decode_text(&[0xFF, 0xFE, b'a', 0, 0x3D, 0xD8, 7], true).into_owned());
    assert_no_leak(|| package::decode_text(&[0xFE, 0xFF, 0, b'a'], true).into_owned());
    assert_no_leak(|| flate::adler32(b"abc"));
    assert_no_leak(|| flate::crc32_update(flate::crc32(b"ab"), b"c"));
    assert_no_leak(|| flate::inflate_stream(&flate::deflate_raw(b"hello hello", 6), flate::Framing::Raw, 1 << 20).unwrap());
    assert_no_leak(|| flate::inflate_stream(&[0x78, 0x9c], flate::Framing::Zlib, 1 << 20).unwrap_err());
}

fn small_cfb() -> Vec<u8> {
    let small = pattern(700, 1);
    let big = pattern(5000, 2);
    let streams: [(&str, &[u8]); 2] = [("WordDocument", &big), ("1Table", &small)];
    let (child, nodes) = root_streams(&streams);
    build(&Spec::v3(child, nodes)).bytes
}

#[test]
fn m1_cfb_queries() {
    let bytes = small_cfb();
    // warm-up (first-use allocations of the runtime are not the reader's)
    drop(CfbFile::open(&bytes));
    assert_no_leak(|| cfb::is_cfb(&bytes));
    assert_no_leak(|| cfb::read_cfb(&bytes).unwrap().entries().len());
    assert_no_leak(|| {
        let f = cfb::read_cfb(&bytes).unwrap();
        let e = f.entry("worddocument").map(|e| e.size);
        let s = f.stream("1TABLE").unwrap().map(|s| s.len());
        let none = f.stream("missing").unwrap();
        (e, s, none, f.has("1table"), format!("{f:?}"))
    });
}

#[test]
fn m1_package_queries_and_save_errors() {
    let parts: Vec<(String, Vec<u8>)> = (0..10).map(|i| (format!("Word/Part{i}.xml"), format!("<p>{i}</p>").into_bytes())).collect();
    let bytes = zip::write(parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions::default()).unwrap();
    for fold in [CaseFold::None, CaseFold::Ascii] {
        let opts = OpenOptions { case_fold: fold, utf16_text: true, skip_impossible_ratio: true };
        assert_no_leak(|| {
            let p = Package::open(&bytes, &Limits::DEFAULT, &opts).unwrap();
            let names = p.part_names().map(str::len).sum::<usize>();
            let found = p.find_name("word/part3.xml").map(str::to_owned);
            let text = p.find_text("WORD/PART4.XML").unwrap().map(|t| t.into_owned());
            let missing = p.find_text("nope").unwrap().is_none();
            (
                names,
                found,
                text,
                missing,
                p.contains("Word/Part1.xml"),
                p.produced_bytes(),
                p.len(),
                p.is_empty(),
                format!("{p:?}"),
                p.limits().max_entries,
                p.options().case_fold,
            )
        });
    }
    // save with a part name over 65,535 bytes fails after other entries were written
    let mut p = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    p.set_part("n".repeat(70_000), vec![1, 2, 3]);
    assert_no_leak(|| p.save(&SaveOptions::default()).unwrap_err());
    // stored save of a deflated entry whose stream is damaged
    let mut w = zip::ZipWriter::new(WriteOptions::default());
    w.add_raw("bad.bin", zip::RawEntry { method: 8, crc32: 0, size: 10, data: &[0xff, 0xff] }).unwrap();
    let bad = w.finish();
    let p = Package::open(&bad, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    assert_no_leak(|| p.save(&SaveOptions { stored: vec!["bad.bin".into()], ..SaveOptions::default() }).unwrap_err());
    assert_no_leak(|| p.verify().unwrap_err());
    // not a zip at all
    assert_no_leak(|| Package::open(b"PK\x05\x06 not really", &Limits::DEFAULT, &OpenOptions::default()).unwrap_err());
    assert_no_leak(|| Package::open_owned(vec![0; 100], &Limits::DEFAULT, &OpenOptions::default()).unwrap_err());
}
