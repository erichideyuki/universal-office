//! F0 invariants: the size budget of the pass-scope items, the JSON key rules (K9), the TS ref
//! strings of the line refs (K13), the work budget (K16) and the list of stage-3 stubs (which may
//! only shrink: `LayoutError::Unported` and every `PENDING` list are gone before gate 2).

use uo_docx_layout::json::{JsonBuilder, JsonKey};
use uo_docx_layout::keys::{PageIx, TbKey};
use uo_docx_layout::model::{HfPartId, NoteKind, NumKey};
use uo_docx_layout::options::{Budget, LayoutLimits, WorkInput};
use uo_docx_layout::output::{HfSlot, HfWhich, LineRef, LineStory, RefPath};
use uo_docx_layout::{LayoutError, LimitKind};

#[test]
fn line_refs_print_the_ts_strings() {
    let r = |story, p: &[u32]| LineRef { story, path: RefPath(p.into()) }.to_ts();
    assert_eq!(r(LineStory::Body, &[12]), "b12");
    assert_eq!(r(LineStory::Body, &[3, 0, 1, 0]), "b3/r0/c1/b0");
    assert_eq!(r(LineStory::Body, &[3, 0, 1]), "b3/r0/c1");
    let hf = LineStory::HeaderFooter { which: HfWhich::Header, slot: HfSlot::Default, part: HfPartId(4) };
    assert_eq!(r(hf, &[0]), "hdr:default/b0");
    let ftr = LineStory::HeaderFooter { which: HfWhich::Footer, slot: HfSlot::Even, part: HfPartId(4) };
    assert_eq!(r(ftr, &[2]), "ftr:even/b2");
    assert_eq!(r(LineStory::Note(NoteKind::Footnote, NumKey::new(2.0)), &[0]), "fn2/b0");
    assert_eq!(r(LineStory::Note(NoteKind::Endnote, NumKey::new(7.0)), &[1]), "en7/b1");
    assert_eq!(r(LineStory::Textbox(TbKey(3)), &[3]), "txbx/b3");
}

#[test]
fn json_keys_follow_the_js_rules() {
    // JSON.stringify([{ a: NaN, b: -0, c: 'x\u{d800}' }, [1, undefined→null, Infinity]])
    let mut b = JsonBuilder::new();
    b.begin_array();
    b.begin_object();
    b.key("a").num(f64::NAN);
    b.key("b").num(-0.0);
    b.key("c").str16(&[0x78, 0xd800]);
    b.end_object();
    b.begin_array().num(1.0).null().num(f64::INFINITY).end_array();
    b.end_array();
    let k = b.finish();
    assert_eq!(&*k.0, r#"[{"a":null,"b":0,"c":"x\ud800"},[1,null,null]]"#);
    assert!(JsonKey::json_eq(&k, &k.clone()));
}

#[test]
fn the_work_budget_is_proportional_to_the_input() {
    let l = LayoutLimits::default();
    let small = Budget::new(&l, &WorkInput { glyphs: 10, ..WorkInput::default() });
    let mut big = Budget::new(&l, &WorkInput { glyphs: 10_000_000, balanced_sections: 20, nesting: 2, max_page_h: 16838.0 });
    let mut tiny = small.clone();
    assert!(tiny.charge(l.work_floor + 1_000_000_000).is_err_and(|e| e == LayoutError::Limit(LimitKind::Work)));
    assert!(big.charge(l.work_floor * 10).is_ok());
    assert_eq!(PageIx(3).ix(), 3);
}

#[test]
fn stage3_stubs_are_listed() {
    // The count may only go down; gate 2 needs both lists empty and no `Unported` left.
    assert!(uo_render::geom::PENDING.len() <= 3);
    assert!(uo_docx_layout::query::PENDING.len() <= 9);
}
