//! WordCraft study — tables, the TCond aside (record `core/docs/modules/uo-docx-layout.md` "WordCraft study —
//! tables"): Word reads `w:tblPr` in order and a `w:tblStyle` after other children discards what came before it
//! (measured, `uo-docx-layout/tests/oracle_wc/tblorder`). `read_tbl_pr_word` keeps the (last) `w:tblStyle` and the
//! children after it; the read `tblPr` (the model, the writer) is unchanged.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::js::{JsValue, JsonWriter};
use uo_docx::read::DOCX;
use uo_docx::read::props::{read_tbl_pr, read_tbl_pr_word};
use uo_opc::xml::{Document, parse};

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

fn doc(inner: &str) -> Document {
    parse(format!("<w:document {NS}><w:tblPr>{inner}</w:tblPr></w:document>"), &DOCX).unwrap()
}

fn json(v: &dyn JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap()
}

#[test]
fn a_late_table_style_keeps_only_itself_and_what_follows() {
    let d = doc(
        r#"<w:tblW w:w="9000" w:type="dxa"/><w:jc w:val="center"/><w:tblStyle w:val="TCond"/><w:tblLook w:val="04A0" w:firstRow="1"/><w:tblInd w:w="100" w:type="dxa"/>"#,
    );
    let el = d.root().first_child("w:tblPr");
    let word = read_tbl_pr_word(el).unwrap();
    assert_eq!(word.tbl_style.as_deref(), Some("TCond"));
    assert!(word.tbl_w.is_none() && word.jc.is_none(), "the direct properties before tblStyle are dropped: {}", json(&word));
    assert!(word.tbl_look.is_some() && word.tbl_ind.is_some(), "the ones after it are kept: {}", json(&word));
    // the model's read is unchanged (every child)
    let read = read_tbl_pr(el).unwrap();
    assert!(read.tbl_w.is_some() && read.jc.is_some() && read.tbl_look.is_some());
}

#[test]
fn a_first_or_absent_table_style_gives_no_word_view() {
    for inner in [
        r#"<w:tblStyle w:val="TCond"/><w:tblW w:w="9000" w:type="dxa"/>"#,
        r#"<w:tblW w:w="9000" w:type="dxa"/><w:jc w:val="center"/>"#,
        "",
    ] {
        let d = doc(inner);
        assert!(read_tbl_pr_word(d.root().first_child("w:tblPr")).is_none(), "{inner}");
    }
    assert!(read_tbl_pr_word(None).is_none());
}

#[test]
fn the_last_table_style_decides() {
    let d = doc(r#"<w:tblStyle w:val="A"/><w:tblW w:w="9000" w:type="dxa"/><w:tblStyle w:val="B"/><w:jc w:val="right"/>"#);
    let word = read_tbl_pr_word(d.root().first_child("w:tblPr")).unwrap();
    assert_eq!(word.tbl_style.as_deref(), Some("B"));
    assert!(word.tbl_w.is_none());
    assert_eq!(word.jc.as_deref(), Some("right"));
}
