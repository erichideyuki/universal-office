//! Behaviour of the D2b-F7 commands (record `docs/modules/uo-docx-d2b/F7.md`): pictures and drawing
//! objects, their two-phase commands (insert, change, adjust, compress, contour), the pending and
//! cancel paths, boxes and chains, with the no-op, error and lock paths.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use std::sync::Arc;

use d2a_support::Cx;
use uo_docx::js::Nullable;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::drawing::{AlignKind, DrawingCmd, GalleryBox, ObjFacts, ObjFrame, ObjRect, TextboxOptions};
use uo_docx::session::ops::pictures::{AdjustArg, ArrangeOp, EffectKind, LayoutInput, PictureCmd, RecolorArg};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{Command, CommandError, DocPosition, FixedEnv, Job, JobResult, OpenOptions, Outcome, Selection, Session};
use uo_docx::write::emit::fx::PicWrapKind;

fn open_body(body: &str) -> Session {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    let bytes = pkg.save(&uo_opc::package::SaveOptions::default()).unwrap();
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f,
    0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 5, 0, 1, 0xff, 0x56, 0xc7,
    0x2f, 0x0d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn exec(s: &mut Session, cx: &mut Cx, c: Command) -> Outcome {
    s.exec(c, &mut cx.cx()).unwrap()
}

fn pic(s: &mut Session, cx: &mut Cx, c: PictureCmd) -> Outcome {
    exec(s, cx, Command::Picture(c))
}

fn dr(s: &mut Session, cx: &mut Cx, c: DrawingCmd) -> Outcome {
    exec(s, cx, Command::Drawing(c))
}

/// Inserts a 100 x 50 px picture at the caret and answers its decode job.
fn insert_picture(s: &mut Session, cx: &mut Cx) {
    let out = pic(s, cx, PictureCmd::Insert { data: Arc::from(PNG), ext: "png".into(), ole: None, max_width_twips: None });
    let Outcome::NeedsJobs(jobs) = out else { panic!("insert asks the shell for the size") };
    assert!(matches!(jobs.as_slice(), [Job::DecodeImage { .. }]));
    let id = jobs[0].id();
    let done = s.complete(id, JobResult::Image { width: 100, height: 50, rgba: None }, &mut cx.cx()).unwrap();
    assert!(matches!(done, Outcome::Done(_)));
}

/// The index of the first image run of paragraph `para`.
fn image_run(s: &Session, para: usize) -> u32 {
    let p = s.document().body.paras.get(para).unwrap();
    u32::try_from(p.runs.iter().position(|r| r.ext().is_some_and(|e| e.image.is_some())).expect("an image run")).unwrap()
}

fn raw(s: &Session, para: usize, run: u32) -> String {
    let r = &s.document().body.paras.get(para).unwrap().runs[run as usize];
    let e = r.ext().unwrap();
    if let Some(i) = e.image.as_deref() {
        return i.raw_drawing_xml.as_deref().unwrap().to_owned();
    }
    e.textbox.as_deref().and_then(|t| t.raw_drawing_xml.as_deref()).unwrap().to_owned()
}

fn select_picture(s: &mut Session, cx: &mut Cx) -> u32 {
    let run = image_run(s, 0);
    dr(s, cx, DrawingCmd::SelectInline { para: 0, run });
    run
}

#[test]
fn insert_is_two_phase_one_step_and_undoes() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    assert!(!s.can_undo());
    insert_picture(&mut s, &mut cx);
    assert!(s.can_undo(), "the step is taken when the shell answers");
    let run = image_run(&s, 0);
    let x = raw(&s, 0, run);
    assert!(x.contains("<wp:inline"), "{x}");
    assert!(x.contains("Imagem 1"), "{x}");
    // the picture at the end of the text, 100 x 50 px at 96 dpi = 1500 x 750 twips
    let img = s.document().body.paras.get(0).unwrap().runs[run as usize].ext().unwrap().image.clone().unwrap();
    assert_eq!(img.width_twips.to_number(), 1500.0);
    assert_eq!(img.height_twips.to_number(), 750.0);
    let mut cx2 = Cx::new();
    s.undo(&mut cx2.cx());
    assert!(s.document().body.paras.get(0).unwrap().runs.iter().all(|r| r.ext().is_none_or(|e| e.image.is_none())));
}

#[test]
fn pending_blocks_other_commands_and_cancel_leaves_no_step() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    let out = pic(&mut s, &mut cx, PictureCmd::Insert { data: Arc::from(PNG), ext: "png".into(), ole: None, max_width_twips: None });
    assert!(matches!(out, Outcome::NeedsJobs(_)));
    let busy = s.exec(Command::Settings(SettingsCmd::SetTracking { on: true }), &mut cx.cx());
    assert_eq!(busy.unwrap_err(), CommandError::Busy);
    s.cancel_pending();
    assert!(!s.can_undo());
    assert!(s.document().body.paras.get(0).unwrap().runs.iter().all(|r| r.ext().is_none_or(|e| e.image.is_none())));
    // a failed decode: nothing happens
    let out =
        pic(&mut s, &mut cx, PictureCmd::Insert { data: Arc::from(&b"junk"[..]), ext: "png".into(), ole: None, max_width_twips: None });
    let Outcome::NeedsJobs(jobs) = out else { panic!() };
    s.complete(jobs[0].id(), JobResult::Failed, &mut cx.cx()).unwrap();
    assert!(!s.can_undo());
    // an unknown job
    assert_eq!(s.complete(uo_docx::session::JobId(99), JobResult::Failed, &mut cx.cx()).unwrap_err(), CommandError::UnknownJob);
}

#[test]
fn format_commands_each_take_one_step_and_write_the_xml() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    let run = select_picture(&mut s, &mut cx);
    let mut steps = 0usize;
    let mut check = |s: &Session, what: &str, pred: &dyn Fn(&str) -> bool| {
        steps += 1;
        let x = raw(s, 0, run);
        assert!(pred(&x), "{what}: {x}");
    };
    pic(&mut s, &mut cx, PictureCmd::SetRotation { deg: 90.0 });
    check(&s, "rot", &|x| x.contains("rot=\"5400000\""));
    pic(&mut s, &mut cx, PictureCmd::Flip { vertical: false });
    check(&s, "flipH", &|x| x.contains("flipH=\"1\""));
    pic(&mut s, &mut cx, PictureCmd::SetAltText { text: Some("a picture".into()), title: None, decorative: None });
    check(&s, "alt", &|x| x.contains("descr=\"a picture\""));
    pic(&mut s, &mut cx, PictureCmd::SetSize { width_pt: Some(200.0), height_pt: None, lock_aspect: Some(true) });
    check(&s, "size", &|x| x.contains("cx=\"2540000\""));
    pic(&mut s, &mut cx, PictureCmd::SetBorder { color: None, weight_pt: Some(2.0), dash: Some("dash".into()) });
    check(&s, "border", &|x| x.contains("<a:ln w=\"25400\""));
    pic(&mut s, &mut cx, PictureCmd::SetEffect { kind: EffectKind::Shadow, preset: Some("outer-br".into()) });
    check(&s, "shadow", &|x| x.contains("<a:outerShdw"));
    pic(&mut s, &mut cx, PictureCmd::SetTransparency { pct: 30.0 });
    check(&s, "transparency", &|x| x.contains("alphaModFix amt=\"70000\""));
    pic(&mut s, &mut cx, PictureCmd::SetRecolor { arg: RecolorArg::Preset("grayscale".into()) });
    check(&s, "recolor", &|x| x.contains("<a:grayscl/>"));
    pic(&mut s, &mut cx, PictureCmd::CropToShape { prst: "ellipse".into() });
    check(&s, "geom", &|x| x.contains("prst=\"ellipse\""));
    pic(&mut s, &mut cx, PictureCmd::SetCrop { l: 0.1, t: 0.0, r: 0.1, b: 0.0 });
    check(&s, "crop", &|x| x.contains("<a:srcRect"));
    assert!(s.history().undo_len() >= steps);
    // a repeat that changes nothing takes no step
    let n = s.history().undo_len();
    pic(&mut s, &mut cx, PictureCmd::SetRotation { deg: 90.0 });
    assert_eq!(s.history().undo_len(), n);
    // reset takes the effects away
    pic(&mut s, &mut cx, PictureCmd::Reset { size: false, natural_pt: None });
    let x = raw(&s, 0, run);
    assert!(!x.contains("<a:outerShdw") && !x.contains("<a:grayscl/>"), "{x}");
    for _ in 0..(s.history().undo_len()) {
        s.undo(&mut cx.cx());
    }
    assert!(!raw_has_image(&s));
}

fn raw_has_image(s: &Session) -> bool {
    s.document().body.paras.get(0).unwrap().runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some()))
}

#[test]
fn inline_to_floating_wrap_places_in_the_same_step() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    let run = select_picture(&mut s, &mut cx);
    let n = s.history().undo_len();
    pic(&mut s, &mut cx, PictureCmd::SetWrap { kind: PicWrapKind::Square });
    assert_eq!(s.history().undo_len(), n + 1);
    let x = raw(&s, 0, run);
    assert!(x.contains("<wp:anchor"), "{x}");
    assert!(x.contains("wrapSquare"), "{x}");
    let frame = ObjFrame {
        page_w: 12240.0,
        page_h: 15840.0,
        mar_l: 1440.0,
        mar_r: 1440.0,
        mar_t: 1440.0,
        mar_b: 1440.0,
        col_x: 1440.0,
        col_w: 9360.0,
        body_top: 1440.0,
        para_top: 1440.0,
    };
    dr(&mut s, &mut cx, DrawingCmd::PlaceAt { frame, x: 2440.0, y: 1940.0 });
    let x = raw(&s, 0, run);
    assert!(x.contains("<wp:posOffset>635000</wp:posOffset>"), "{x}");
    assert_eq!(s.history().undo_len(), n + 1, "the placement is in the step of the wrap");
    // the same wrap again: nothing
    pic(&mut s, &mut cx, PictureCmd::SetWrap { kind: PicWrapKind::Square });
    assert_eq!(s.history().undo_len(), n + 1);
    // arrange, position, align, lock anchor, layout
    pic(&mut s, &mut cx, PictureCmd::Arrange { op: ArrangeOp::BehindText });
    assert!(raw(&s, 0, run).contains("behindDoc=\"1\""));
    pic(&mut s, &mut cx, PictureCmd::SetPosition { preset: "top-right".into(), facts: ObjFacts::default() });
    assert!(raw(&s, 0, run).contains("<wp:align>right</wp:align>"));
    pic(&mut s, &mut cx, PictureCmd::SetLockAnchor { on: true });
    assert!(raw(&s, 0, run).contains("locked=\"1\""));
    let m = s.history().undo_len();
    pic(
        &mut s,
        &mut cx,
        PictureCmd::SetLayout {
            o: Box::new(LayoutInput {
                wrap: Some(PicWrapKind::TopAndBottom),
                width_pt: Some(100.0),
                rotation: Some(10.0),
                ..LayoutInput::default()
            }),
            facts: ObjFacts::default(),
        },
    );
    assert_eq!(s.history().undo_len(), m + 1, "the layout dialog is one step");
    assert!(raw(&s, 0, run).contains("wrapTopAndBottom"));
    dr(&mut s, &mut cx, DrawingCmd::DeleteSelectedShape);
    assert!(raw_has_image(&s), "deleteSelectedShape leaves pictures alone");
    pic(&mut s, &mut cx, PictureCmd::DeleteSelectedImage);
    assert!(!raw_has_image(&s));
}

#[test]
fn adjust_decodes_then_encodes_and_neutral_needs_no_job() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    select_picture(&mut s, &mut cx);
    // already neutral: nothing to bake, no job
    let out = pic(&mut s, &mut cx, PictureCmd::SetAdjust { o: AdjustArg { saturation: Some(1.0), ..AdjustArg::default() } });
    assert!(matches!(out, Outcome::Done(_)));
    let out =
        pic(&mut s, &mut cx, PictureCmd::SetAdjust { o: AdjustArg { bright: Some(0.2), saturation: Some(2.0), ..AdjustArg::default() } });
    let Outcome::NeedsJobs(jobs) = out else { panic!("adjust decodes") };
    assert!(matches!(jobs.as_slice(), [Job::DecodeImage { .. }]));
    // the neutral request above took its step (the TS snapshots before it patches); the second
    // request takes none until the shell answers
    let before = s.history().undo_len();
    assert_eq!(before, 2);
    let px: Arc<[u8]> = Arc::from(vec![10u8, 20, 30, 255]);
    let out = s.complete(jobs[0].id(), JobResult::Image { width: 1, height: 1, rgba: Some(px) }, &mut cx.cx()).unwrap();
    let Outcome::NeedsJobs(jobs) = out else { panic!("then the encode of the processed pixels") };
    let Job::EncodeImage { width, height, rgba, mime, .. } = &jobs[0] else { panic!() };
    assert_eq!((*width, *height, rgba.len(), &**mime), (1, 1, 4, "image/png"));
    assert_eq!(s.history().undo_len(), before);
    let done = s.complete(jobs[0].id(), JobResult::Bytes(Arc::from(PNG)), &mut cx.cx()).unwrap();
    assert!(matches!(done, Outcome::Done(_)));
    assert_eq!(s.history().undo_len(), before + 1);
    let run = image_run(&s, 0);
    let x = raw(&s, 0, run);
    assert!(x.contains("a14:imgProps"), "{x}");
    assert!(x.contains("a14:imgLayer"), "{x}");
    // neutral again: back to the layer
    let out =
        pic(&mut s, &mut cx, PictureCmd::SetAdjust { o: AdjustArg { bright: Some(0.0), saturation: Some(1.0), ..AdjustArg::default() } });
    assert!(matches!(out, Outcome::Done(_)));
    assert!(!raw(&s, 0, run).contains("a14:imgLayer"));
}

#[test]
fn change_and_compress_are_two_phase() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    select_picture(&mut s, &mut cx);
    let out = pic(&mut s, &mut cx, PictureCmd::Change { data: Arc::from(PNG), ext: "png".into() });
    let Outcome::NeedsJobs(jobs) = out else { panic!() };
    let n = s.history().undo_len();
    s.complete(jobs[0].id(), JobResult::Image { width: 40, height: 80, rgba: None }, &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), n + 1);
    let x = raw(&s, 0, image_run(&s, 0));
    assert!(x.contains("cy="), "{x}");
    // compress: 100 x 50 px at 220 ppi for a 1500 x 750 twip picture shrinks to 229 x 114 -> larger
    // than the source: nothing changes; at 10 ppi it does
    let out = pic(&mut s, &mut cx, PictureCmd::Compress { ppi: 10.0, delete_cropped: false, all: true });
    let Outcome::NeedsJobs(jobs) = out else { panic!("one decode per picture") };
    assert_eq!(jobs.len(), 1);
    let px: Arc<[u8]> = Arc::from(vec![200u8; 100 * 50 * 4]);
    let out = s.complete(jobs[0].id(), JobResult::Image { width: 100, height: 50, rgba: Some(px) }, &mut cx.cx()).unwrap();
    let Outcome::NeedsJobs(enc) = out else { panic!("then one encode") };
    assert!(matches!(&enc[0], Job::EncodeImage { mime, .. } if &**mime == "image/png"));
    let m = s.history().undo_len();
    s.complete(enc[0].id(), JobResult::Bytes(Arc::from(PNG)), &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), m + 1, "one step for all the pictures");
    assert_eq!(s.compress_result(), 1);
    // nothing to compress
    let out = pic(&mut s, &mut cx, PictureCmd::Compress { ppi: 330.0, delete_cropped: false, all: false });
    let Outcome::NeedsJobs(jobs) = out else { panic!() };
    let px: Arc<[u8]> = Arc::from(vec![200u8; 4]);
    let out = s.complete(jobs[0].id(), JobResult::Image { width: 1, height: 1, rgba: Some(px) }, &mut cx.cx()).unwrap();
    assert!(matches!(out, Outcome::Done(_)));
    assert_eq!(s.compress_result(), 0);
}

fn three_pictures_session() -> (Session, Cx) {
    // one picture per paragraph (an insert over a selected picture replaces it)
    let mut s = open_body(&(p("a") + &p("b") + &p("c")));
    let mut cx = Cx::new();
    for para in 0..3u32 {
        s.set_selection_raw(Selection::caret(DocPosition::new(para, 0)));
        insert_picture(&mut s, &mut cx);
    }
    let n = s.document().body.paras.iter().flat_map(|p| p.runs.iter()).filter(|r| r.ext().is_some_and(|e| e.image.is_some())).count();
    assert_eq!(n, 3);
    (s, cx)
}

fn jobs_of(out: Outcome) -> Vec<Job> {
    let Outcome::NeedsJobs(jobs) = out else { panic!("jobs expected") };
    jobs
}

fn img(n: u8) -> JobResult {
    JobResult::Image { width: 100, height: 50, rgba: Some(Arc::from(vec![n; 100 * 50 * 4])) }
}

fn answer_all(s: &mut Session, cx: &mut Cx, jobs: &[Job], r: impl Fn() -> JobResult) -> Outcome {
    let mut last = None;
    for j in jobs {
        last = Some(s.complete(j.id(), r(), &mut cx.cx()).unwrap());
    }
    last.unwrap()
}

#[test]
fn cancelled_compress_leaves_no_stale_answers() {
    let (mut s, mut cx) = three_pictures_session();
    let cmd = || PictureCmd::Compress { ppi: 10.0, delete_cropped: false, all: true };
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    assert_eq!(jobs.len(), 3);
    // one answer comes in (the command waits for the others), then the shell gives up
    let first = s.complete(jobs[0].id(), img(10), &mut cx.cx()).unwrap();
    assert!(matches!(first, Outcome::NeedsJobs(ref j) if j.is_empty()));
    s.cancel_pending();
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    let enc = jobs_of(answer_all(&mut s, &mut cx, &jobs, || img(20)));
    assert_eq!(enc.len(), 3, "each picture gets its own encode");
    let last = answer_all(&mut s, &mut cx, &enc, || JobResult::Bytes(Arc::from(PNG)));
    assert!(matches!(last, Outcome::Done(_)));
    assert_eq!(s.compress_result(), 3);
}

#[test]
fn cancelled_compress_encode_round_does_not_poison_the_next() {
    let (mut s, mut cx) = three_pictures_session();
    let cmd = || PictureCmd::Compress { ppi: 10.0, delete_cropped: false, all: true };
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    assert_eq!(jobs_of(answer_all(&mut s, &mut cx, &jobs, || img(10))).len(), 3);
    s.cancel_pending(); // cancelled during the encode round
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    let enc = jobs_of(answer_all(&mut s, &mut cx, &jobs, || img(10)));
    assert_eq!(enc.len(), 3, "the next compress starts with its decode round");
    let last = answer_all(&mut s, &mut cx, &enc, || JobResult::Bytes(Arc::from(PNG)));
    assert!(matches!(last, Outcome::Done(_)));
    assert_eq!(s.compress_result(), 3);
}

#[test]
fn compress_ignores_zero_sized_and_short_decodes() {
    let (mut s, mut cx) = three_pictures_session();
    // every picture is cropped, so "delete cropped areas" resamples what the shell decoded
    for para in 0..3usize {
        let run = image_run(&s, para);
        dr(&mut s, &mut cx, DrawingCmd::SelectInline { para: u32::try_from(para).unwrap(), run });
        pic(&mut s, &mut cx, PictureCmd::SetCrop { l: 0.1, t: 0.0, r: 0.1, b: 0.0 });
    }
    let jobs = jobs_of(pic(&mut s, &mut cx, PictureCmd::Compress { ppi: 10.0, delete_cropped: true, all: true }));
    let answers = [
        JobResult::Image { width: 0, height: 50, rgba: Some(Arc::from(vec![1u8; 16])) },
        JobResult::Image { width: 100, height: 0, rgba: Some(Arc::from(vec![1u8; 16])) },
        JobResult::Image { width: 100, height: 50, rgba: Some(Arc::from(vec![1u8; 7])) },
    ];
    let mut last = None;
    for (j, a) in jobs.iter().zip(answers) {
        last = Some(s.complete(j.id(), a, &mut cx.cx()).unwrap());
    }
    assert!(matches!(last, Some(Outcome::Done(_))));
    assert_eq!(s.compress_result(), 0);
}

#[test]
fn locked_sessions_refuse_everything_silently() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    select_picture(&mut s, &mut cx);
    exec(&mut s, &mut cx, Command::Settings(SettingsCmd::SetReadView { on: true }));
    let n = s.history().undo_len();
    pic(&mut s, &mut cx, PictureCmd::SetRotation { deg: 10.0 });
    pic(&mut s, &mut cx, PictureCmd::DeleteSelectedImage);
    let out = pic(&mut s, &mut cx, PictureCmd::Insert { data: Arc::from(PNG), ext: "png".into(), ole: None, max_width_twips: None });
    assert!(matches!(out, Outcome::Done(_)));
    assert_eq!(s.history().undo_len(), n);
}

#[test]
fn boxes_insert_format_link_and_delete() {
    let mut s = open_body(&(p("one") + &p("two") + &p("three")));
    let mut cx = Cx::new();
    let ins = |s: &mut Session, cx: &mut Cx, id: &str| {
        dr(s, cx, DrawingCmd::InsertTextbox { id: id.into(), page: None, theme_accent1: None, lang: "en".into(), user: None });
    };
    ins(&mut s, &mut cx, "simple");
    let tb_paths = |s: &Session| -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (pi, p) in s.document().body.paras.iter().enumerate() {
            for (ri, r) in p.runs.iter().enumerate() {
                if r.ext().is_some_and(|e| e.textbox.is_some()) {
                    v.push((pi, ri));
                }
            }
        }
        v
    };
    assert_eq!(tb_paths(&s).len(), 1);
    let (pi, ri) = tb_paths(&s)[0];
    let x = raw(&s, pi, u32::try_from(ri).unwrap());
    assert!(x.contains("Text Box 1"), "{x}");
    // unknown template: nothing
    let n = s.history().undo_len();
    ins(&mut s, &mut cx, "nope");
    assert_eq!(s.history().undo_len(), n);
    // select it and format it as a shape
    dr(&mut s, &mut cx, DrawingCmd::SelectObject { index: Some(0) });
    dr(&mut s, &mut cx, DrawingCmd::SetShapeFill { fill: None });
    assert!(raw(&s, pi, u32::try_from(ri).unwrap()).contains("<a:noFill/>"));
    dr(&mut s, &mut cx, DrawingCmd::SetShapeOutline { color: Some(Nullable::Null), weight_pt: None, dash: None, compound: None });
    dr(&mut s, &mut cx, DrawingCmd::ChangeShape { prst: "roundRect".into() });
    assert!(raw(&s, pi, u32::try_from(ri).unwrap()).contains("roundRect"));
    dr(
        &mut s,
        &mut cx,
        DrawingCmd::SetTextboxOptions {
            o: TextboxOptions { anchor: Some("ctr".into()), margin_l_pt: Some(10.0), ..TextboxOptions::default() },
        },
    );
    assert!(raw(&s, pi, u32::try_from(ri).unwrap()).contains("anchor=\"ctr\""));
    dr(&mut s, &mut cx, DrawingCmd::SetShapeFormat { fmt: Box::default() });
    dr(&mut s, &mut cx, DrawingCmd::SetShapeStyle { n: 3.0 });
    dr(&mut s, &mut cx, DrawingCmd::SetShapeStyle { n: 999.0 });
    // the picture-family commands on a shape go through OnShape
    dr(&mut s, &mut cx, DrawingCmd::OnShape(Box::new(PictureCmd::SetRotation { deg: 15.0 })));
    assert!(raw(&s, pi, u32::try_from(ri).unwrap()).contains("rot=\"900000\""));
    dr(&mut s, &mut cx, DrawingCmd::SetPaneItemHidden { key: None, hidden: true });
    assert!(raw(&s, pi, u32::try_from(ri).unwrap()).contains("hidden=\"1\""));
    dr(&mut s, &mut cx, DrawingCmd::RenamePaneItem { key: "1".into(), name: "My box".into() });
    // two boxes, linked
    ins(&mut s, &mut cx, "note");
    assert_eq!(tb_paths(&s).len(), 2);
    dr(&mut s, &mut cx, DrawingCmd::SelectObject { index: Some(0) });
    let keys: Vec<String> = s.pane_keys();
    assert_eq!(keys.len(), 2, "{keys:?}");
    let n = s.history().undo_len();
    dr(&mut s, &mut cx, DrawingCmd::LinkTextboxTo { key: keys[1].as_str().into() });
    assert!(s.history().undo_len() <= n + 1);
    dr(&mut s, &mut cx, DrawingCmd::BreakTextboxLink);
    dr(&mut s, &mut cx, DrawingCmd::SelectObject { index: Some(0) });
    dr(&mut s, &mut cx, DrawingCmd::DeleteSelectedShape);
    assert_eq!(tb_paths(&s).len(), 1);
    // a user gallery item
    let item = GalleryBox { xml: raw(&s, tb_paths(&s)[0].0, u32::try_from(tb_paths(&s)[0].1).unwrap()).into() };
    dr(
        &mut s,
        &mut cx,
        DrawingCmd::InsertTextbox { id: "user:x".into(), page: None, theme_accent1: None, lang: "pt".into(), user: Some(item) },
    );
    assert_eq!(tb_paths(&s).len(), 2);
    // align with two objects selected
    let rects = vec![
        ObjFacts { rect: Some(ObjRect { x: 100.0, y: 100.0, w: 500.0, h: 300.0 }), frame: None },
        ObjFacts { rect: Some(ObjRect { x: 900.0, y: 700.0, w: 300.0, h: 300.0 }), frame: None },
    ];
    dr(&mut s, &mut cx, DrawingCmd::AlignObjects { kind: AlignKind::Left, rects });
    // undo them all
    for _ in 0..s.history().undo_len() {
        s.undo(&mut cx.cx());
    }
    assert!(tb_paths(&s).is_empty());
}

#[test]
fn undo_of_a_step_taken_in_a_box_reenters_it() {
    let mut s = open_body(&p("one"));
    let mut cx = Cx::new();
    dr(
        &mut s,
        &mut cx,
        DrawingCmd::InsertTextbox { id: "simple".into(), page: Some(0), theme_accent1: None, lang: "en".into(), user: None },
    );
    assert!(matches!(s.context(), uo_docx::session::EditContext::Textbox { .. }));
    s.undo(&mut cx.cx());
    assert!(matches!(s.context(), uo_docx::session::EditContext::Body));
    s.redo(&mut cx.cx());
}

#[test]
fn edit_selected_textbox_enters_with_the_caret_at_the_end_and_a_step_is_priced() {
    use uo_docx::session::ops::context::EditContext;
    let mut s = open_body(&(p("one") + &p("two")));
    let mut cx = Cx::new();
    // nothing selected: no box, no event
    dr(&mut s, &mut cx, DrawingCmd::EditSelectedTextbox { page: 0 });
    assert!(!matches!(s.context(), EditContext::Textbox { .. }));
    dr(&mut s, &mut cx, DrawingCmd::InsertTextbox { id: "simple".into(), page: None, theme_accent1: None, lang: "en".into(), user: None });
    // a command that took a step went through the settle and the pricing of the history
    assert!(s.history_bytes() > 0, "the insert step was not priced");
    dr(&mut s, &mut cx, DrawingCmd::SelectObject { index: Some(0) });
    dr(&mut s, &mut cx, DrawingCmd::EditSelectedTextbox { page: 0 });
    assert!(matches!(s.context(), EditContext::Textbox { .. }));
    assert_eq!(s.float_selection(), None);
    let sel = s.selection();
    assert_eq!(sel.anchor, sel.focus);
    let last = s.paras().unwrap().len() - 1;
    assert_eq!(sel.focus.para as usize, last);
    let len = uo_docx::session::text::para_length(s.paras().unwrap().get(last).unwrap());
    assert_eq!(sel.focus.offset as usize, len);
    // inside the box `floatSel` is null: the command does nothing
    let n = s.history().undo_len();
    dr(&mut s, &mut cx, DrawingCmd::EditSelectedTextbox { page: 0 });
    assert_eq!(s.history().undo_len(), n);
    // the text box of the drawn kind takes the language of the interface
    let mut s = open_body(&p("one"));
    dr(
        &mut s,
        &mut cx,
        DrawingCmd::CreateDrawnTextbox {
            page: 0,
            rect: ObjRect { x: 100.0, y: 100.0, w: 1200.0, h: 600.0 },
            vertical: false,
            anchor_para: None,
            lang: "pt".into(),
        },
    );
    let x = s
        .document()
        .body
        .paras
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| r.ext().and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.as_deref()).map(str::to_owned))
        .unwrap();
    assert!(x.contains("Caixa de Texto 1"), "{x}");
}

#[test]
fn cancelled_adjust_encode_round_does_not_poison_the_next_adjust() {
    let mut s = open_body(&p("hello"));
    let mut cx = Cx::new();
    insert_picture(&mut s, &mut cx);
    select_picture(&mut s, &mut cx);
    let cmd = || PictureCmd::SetAdjust { o: AdjustArg { bright: Some(0.2), saturation: Some(2.0), ..AdjustArg::default() } };
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    let px = || JobResult::Image { width: 1, height: 1, rgba: Some(Arc::from(vec![10u8, 20, 30, 255])) };
    let enc = jobs_of(s.complete(jobs[0].id(), px(), &mut cx.cx()).unwrap());
    assert!(matches!(enc.as_slice(), [Job::EncodeImage { .. }]));
    s.cancel_pending(); // cancelled during the encode round
    let n = s.history().undo_len();
    let jobs = jobs_of(pic(&mut s, &mut cx, cmd()));
    assert!(matches!(jobs.as_slice(), [Job::DecodeImage { .. }]));
    let enc = jobs_of(s.complete(jobs[0].id(), px(), &mut cx.cx()).unwrap());
    let done = s.complete(enc[0].id(), JobResult::Bytes(Arc::from(PNG)), &mut cx.cx()).unwrap();
    assert!(matches!(done, Outcome::Done(_)));
    assert_eq!(s.history().undo_len(), n + 1);
}

#[test]
fn align_objects_leaves_out_entries_without_a_rect() {
    let mut s = open_body(&(p("one") + &p("two") + &p("three")));
    let mut cx = Cx::new();
    for _ in 0..3 {
        dr(
            &mut s,
            &mut cx,
            DrawingCmd::InsertTextbox { id: "simple".into(), page: None, theme_accent1: None, lang: "en".into(), user: None },
        );
    }
    let keys = s.pane_keys();
    assert_eq!(keys.len(), 3, "{keys:?}");
    dr(&mut s, &mut cx, DrawingCmd::SelectPaneItems { keys: keys.iter().map(|k| k.as_str().into()).collect() });
    let tb_paths = |s: &Session| -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (pi, p) in s.document().body.paras.iter().enumerate() {
            for (ri, r) in p.runs.iter().enumerate() {
                if r.ext().is_some_and(|e| e.textbox.is_some()) {
                    v.push((pi, ri));
                }
            }
        }
        v
    };
    let all = tb_paths(&s);
    let xml = |s: &Session, i: usize| raw(s, all[i].0, u32::try_from(all[i].1).unwrap());
    let frame = ObjFrame {
        page_w: 12240.0,
        page_h: 15840.0,
        mar_l: 1440.0,
        mar_r: 1440.0,
        mar_t: 1440.0,
        mar_b: 1440.0,
        col_x: 1440.0,
        col_w: 9360.0,
        body_top: 1440.0,
        para_top: 1440.0,
    };
    let at = |x: f64| ObjFacts { rect: Some(ObjRect { x, y: 100.0, w: 500.0, h: 300.0 }), frame: Some(frame) };
    let before: Vec<String> = (0..3).map(|i| xml(&s, i)).collect();
    let n = s.history().undo_len();
    // the middle one is inline or not laid out (no rect): the two others are aligned, it is not moved
    dr(&mut s, &mut cx, DrawingCmd::AlignObjects { kind: AlignKind::Left, rects: vec![at(100.0), ObjFacts::default(), at(900.0)] });
    assert_eq!(s.history().undo_len(), n + 1, "one step for the objects moved");
    let after: Vec<String> = (0..3).map(|i| xml(&s, i)).collect();
    assert_eq!(before[1], after[1], "the entry without a rect is left alone");
    assert_ne!(before[2], after[2], "the object to the right moved to the left edge");
    // a list of another length than the selection is a no-op
    let n = s.history().undo_len();
    dr(&mut s, &mut cx, DrawingCmd::AlignObjects { kind: AlignKind::Left, rects: vec![at(100.0), at(900.0)] });
    assert_eq!(s.history().undo_len(), n);
}
