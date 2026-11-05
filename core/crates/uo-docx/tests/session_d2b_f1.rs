//! D2b-F1 behaviour (record `docs/modules/uo-docx-d2b/F1.md`): one test per command (M1) with the
//! error and no-op paths — a caret keeps the format for the typed text, a selection takes one undo
//! step, tracking turns formatting into revisions, the formatting policy refuses (`uw-blocked`),
//! the lock and the read view answer silently, invalid arguments change nothing — over the blank
//! document, the generated documents and the corpus.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f1_support;
mod support;

use d2a_support::Cx;
use d2b_f1_support::{ff, fx, ot};
use uo_docx::js::{JsNumber, JsonWriter};
use uo_docx::model::props::BorderSpec;
use uo_docx::model::run::RunProps;
use uo_docx::session::ops::format::{
    CaseMode, ColorArg, FontFormat, FormatCmd, FormatPainter, FxPart, GradientArg, OtPart, RunToggle, ThemeColorRef, VertKind,
};
use uo_docx::session::ops::paragraph::{
    BorderPreset, FlowArgs, IndentPatch, LayoutArgs, ParaBorderSide, ParagraphCmd, SpacingArg, SpecialIndent, TabOnly, TabStopInfo,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn blank() -> Session {
    Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> ChangeSet {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(cs) => cs,
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn fmt(s: &mut Session, cx: &mut Cx, c: FormatCmd) -> ChangeSet {
    run(s, cx, Command::Format(c))
}

fn para(s: &mut Session, cx: &mut Cx, c: ParagraphCmd) -> ChangeSet {
    run(s, cx, Command::Paragraph(c))
}

fn typed(s: &mut Session, cx: &mut Cx, t: &str) {
    run(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(t) }));
}

fn select(s: &mut Session, cx: &mut Cx, a: (u32, u32), f: (u32, u32)) {
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

/// A blank document with "Hello World" in paragraph 0 and "Second one" in paragraph 1, everything selected
/// in paragraph 0 `[0, 5)`.
fn doc() -> (Session, Cx) {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "Hello World");
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    typed(&mut s, &mut cx, "Second one");
    select(&mut s, &mut cx, (0, 0), (0, 5));
    (s, cx)
}

fn props(s: &Session, para: usize, run: usize) -> RunProps {
    (*s.document().body.paras.get(para).unwrap().runs.get(run).unwrap().props).clone()
}

fn run_text(s: &Session, para: usize, run: usize) -> String {
    s.document().body.paras.get(para).unwrap().runs.get(run).unwrap().text.to_string_lossy()
}

fn pp(s: &Session, para: usize) -> uo_docx::model::para::ParaProps {
    (*s.document().body.paras.get(para).unwrap().props).clone()
}

fn json<T: uo_docx::js::JsValue>(v: &T) -> String {
    JsonWriter::stringify(v, None).unwrap()
}

fn steps(s: &Session) -> usize {
    s.history().undo_len()
}

// ───────────────────────────── character formatting ─────────────────────────────

#[test]
fn toggle_run_prop_on_a_selection_and_a_caret() {
    let (mut s, mut cx) = doc();
    let n = steps(&s);
    let c = fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    assert_eq!(steps(&s), n + 1);
    assert_eq!(
        c.events,
        vec![Event::DirtyChange { dirty: true }, Event::Change].into_iter().filter(|e| c.events.contains(e)).collect::<Vec<_>>()
    );
    assert_eq!(run_text(&s, 0, 0), "Hello");
    assert_eq!(props(&s, 0, 0).bold, Some(true));
    assert_eq!(props(&s, 0, 1).bold, Some(false));
    // toggling again (the first run is all bold now) turns it off and merges the runs back
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    assert_eq!(props(&s, 0, 0).bold, Some(false));
    // undo
    s.undo(&mut cx.cx());
    assert_eq!(props(&s, 0, 0).bold, Some(true));
    // a caret: no step, the props wait for the typed text
    select(&mut s, &mut cx, (1, 10), (1, 10));
    let n = steps(&s);
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Italic, button: "single".into() });
    assert_eq!(steps(&s), n);
    assert_eq!(s.pending_props().unwrap().props.italic, Some(true));
    typed(&mut s, &mut cx, "!");
    assert!(s.pending_props().is_none());
    assert_eq!(run_text(&s, 1, 1), "!");
    assert_eq!(props(&s, 1, 1).italic, Some(true));
}

#[test]
fn strike_and_double_strike_exclude_each_other() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Strike, button: "single".into() });
    assert_eq!(props(&s, 0, 0).strike, Some(true));
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::DoubleStrike, button: "single".into() });
    assert_eq!(props(&s, 0, 0).double_strike, Some(true));
    assert_eq!(props(&s, 0, 0).strike, Some(false));
}

#[test]
fn underline_styles_button_and_colour() {
    let (mut s, mut cx) = doc();
    // Ctrl+U with the button on `single`
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Underline, button: "single".into() });
    assert_eq!(props(&s, 0, 0).underline, Some(true));
    assert_eq!(props(&s, 0, 0).underline_style, None);
    // the whole selection already has it: off
    fmt(&mut s, &mut cx, FormatCmd::ToggleUnderline { style: None, button: "single".into() });
    assert_eq!(props(&s, 0, 0).underline, Some(false));
    // a style: the guard of the policy and the shell's button
    let cmd = FormatCmd::SetUnderline { style: "double".into() };
    assert_eq!(cmd.underline_button_after("single"), "double");
    assert_eq!(FormatCmd::SetUnderline { style: "none".into() }.underline_button_after("dotted"), "dotted");
    fmt(&mut s, &mut cx, cmd);
    assert_eq!(props(&s, 0, 0).underline_style.as_deref(), Some("double"));
    // toggling with another style over a double: the simple style goes on (mixed/different)
    fmt(&mut s, &mut cx, FormatCmd::ToggleUnderline { style: Some("single".into()), button: "double".into() });
    assert_eq!(props(&s, 0, 0).underline, Some(true));
    assert_eq!(props(&s, 0, 0).underline_style, None);
    fmt(&mut s, &mut cx, FormatCmd::SetUnderline { style: "none".into() });
    assert_eq!(props(&s, 0, 0).underline, Some(false));
    // the colour of a theme: tint 0.4 → 99, and the underline turns on with the button's style
    fmt(
        &mut s,
        &mut cx,
        FormatCmd::SetUnderlineColor {
            color: Some(ColorArg::Theme(ThemeColorRef { theme_color: "accent1".into(), tint: Some(0.4), shade: None })),
            button: "dotted".into(),
        },
    );
    let p = props(&s, 0, 0);
    assert_eq!(p.underline, Some(true));
    assert_eq!(p.underline_style.as_deref(), Some("dotted"));
    assert_eq!(p.underline_theme.as_ref().unwrap().tint.as_deref(), Some("99"));
    assert!(p.underline_color.is_some());
    // automatic (null): the colour keys stay as `undefined`
    fmt(&mut s, &mut cx, FormatCmd::SetUnderlineColor { color: None, button: "single".into() });
    assert_eq!(props(&s, 0, 0).underline_color, None);
}

#[test]
fn text_effects_patch_preset_open_type_and_gradient() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::SetTextEffects { patch: fx(0) });
    assert!(json(&props(&s, 0, 0).text_fx.unwrap()).contains("\"FF0000\""));
    fmt(&mut s, &mut cx, FormatCmd::SetTextEffects { patch: fx(2) });
    let t = props(&s, 0, 0).text_fx.unwrap();
    assert!(t.fill.is_some() && t.shadow.is_some());
    // the last part removed: the key is `undefined`
    fmt(&mut s, &mut cx, FormatCmd::SetTextEffects { patch: vec![FxPart::Fill(None), FxPart::Shadow(None)] });
    assert!(props(&s, 0, 0).text_fx.is_none());
    // the gallery: a valid index sets, null clears, a bad index is no step
    let n = steps(&s);
    fmt(&mut s, &mut cx, FormatCmd::ApplyTextEffectPreset { index: Some(4.0) });
    assert_eq!(steps(&s), n + 1);
    assert!(props(&s, 0, 0).text_fx.unwrap().props3d.is_some());
    for bad in [15.0, -1.0, 1.5, f64::NAN, 1e300, f64::INFINITY] {
        let n = steps(&s);
        fmt(&mut s, &mut cx, FormatCmd::ApplyTextEffectPreset { index: Some(bad) });
        assert_eq!(steps(&s), n, "{bad}");
    }
    fmt(&mut s, &mut cx, FormatCmd::ApplyTextEffectPreset { index: None });
    assert!(props(&s, 0, 0).text_fx.is_none());
    // OpenType: set, then the removal values
    fmt(&mut s, &mut cx, FormatCmd::SetOpenType { patch: ot(3) });
    let o = props(&s, 0, 0).open_type.unwrap();
    assert_eq!(o.stylistic_sets.as_ref().map(Vec::len), Some(2));
    assert_eq!(o.cntxt_alts, Some(true));
    fmt(
        &mut s,
        &mut cx,
        FormatCmd::SetOpenType {
            patch: vec![OtPart::Ligatures(Some("default".into())), OtPart::StylisticSets(Some(vec![])), OtPart::CntxtAlts(Some(false))],
        },
    );
    assert!(props(&s, 0, 0).open_type.is_none());
    // the gradient takes the colour of the text (or of the theme)
    fmt(&mut s, &mut cx, FormatCmd::SetColor { color: Some(ColorArg::Hex("ff8800".into())) });
    fmt(&mut s, &mut cx, FormatCmd::SetTextGradient { v: Some(GradientArg { angle_deg: 90.0, light: true }) });
    let fill = props(&s, 0, 0).text_fx.unwrap().fill.unwrap();
    assert!(json(&*fill).contains("\"FF8800\""));
    fmt(&mut s, &mut cx, FormatCmd::SetTextGradient { v: None });
    assert!(props(&s, 0, 0).text_fx.is_none());
    // a caret keeps the effect for the typed text
    select(&mut s, &mut cx, (1, 10), (1, 10));
    fmt(&mut s, &mut cx, FormatCmd::SetTextEffects { patch: fx(0) });
    assert!(s.pending_props().unwrap().props.text_fx.is_some());
}

#[test]
fn font_format_size_family_colour_and_spacing() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::SetFontFormat { f: Box::new(ff(0)) });
    let p = props(&s, 0, 0);
    assert_eq!(p.font_ascii.as_deref(), Some("Arial"));
    assert_eq!(p.sz_half_pt, Some(JsNumber::Num(25.0)));
    assert_eq!(p.bold, Some(true));
    // an empty format changes nothing (no step)
    let n = steps(&s);
    fmt(&mut s, &mut cx, FormatCmd::SetFontFormat { f: Box::new(FontFormat::default()) });
    assert_eq!(steps(&s), n);
    // the size rounds half up (11.25 pt → 22.5 → 23 half points)
    fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 11.25 });
    assert_eq!(props(&s, 0, 0).sz_half_pt, Some(JsNumber::Num(23.0)));
    // grow: the minimum is one point
    fmt(&mut s, &mut cx, FormatCmd::GrowFont { delta: -100.0 });
    assert_eq!(props(&s, 0, 0).sz_half_pt, Some(JsNumber::Num(2.0)));
    fmt(&mut s, &mut cx, FormatCmd::SetFontFamily { name: "Calibri".into() });
    assert_eq!(props(&s, 0, 0).font_ascii.as_deref(), Some("Calibri"));
    // colours: hex (with # and case), theme with a shade, automatic
    fmt(&mut s, &mut cx, FormatCmd::SetColor { color: Some(ColorArg::Hex("#aabbcc".into())) });
    assert_eq!(props(&s, 0, 0).color.as_deref(), Some("AABBCC"));
    fmt(
        &mut s,
        &mut cx,
        FormatCmd::SetColor {
            color: Some(ColorArg::Theme(ThemeColorRef { theme_color: "accent2".into(), tint: None, shade: Some(0.25) })),
        },
    );
    let p = props(&s, 0, 0);
    assert_eq!(p.color_theme.as_ref().unwrap().shade.as_deref(), Some("BF"));
    assert_eq!(p.color_theme.as_ref().unwrap().theme_color.as_ref(), "accent2");
    assert!(p.color.is_some());
    fmt(&mut s, &mut cx, FormatCmd::SetColor { color: None });
    assert_eq!(props(&s, 0, 0).color, None);
    assert_eq!(props(&s, 0, 0).color_theme, None);
    fmt(&mut s, &mut cx, FormatCmd::SetHighlight { name: Some("yellow".into()) });
    assert_eq!(props(&s, 0, 0).highlight.as_deref(), Some("yellow"));
    fmt(&mut s, &mut cx, FormatCmd::SetHighlight { name: None });
    assert_eq!(props(&s, 0, 0).highlight, None);
    fmt(&mut s, &mut cx, FormatCmd::SetCharSpacing { twips: Some(20.0) });
    assert_eq!(props(&s, 0, 0).char_spacing_twips, Some(JsNumber::Num(20.0)));
    fmt(&mut s, &mut cx, FormatCmd::SetCharSpacing { twips: None });
    assert_eq!(props(&s, 0, 0).char_spacing_twips, None);
    // the dialog's zero values are `undefined`, scale 100 too
    let f = FontFormat {
        scale_pct: Some(100.0),
        spacing_twips: Some(0.0),
        position_half_pt: Some(0.0),
        kern_min_half_pt: Some(0.0),
        ..FontFormat::default()
    };
    fmt(&mut s, &mut cx, FormatCmd::SetFontFormat { f: Box::new(f) });
    let p = props(&s, 0, 0);
    assert!(p.scale_pct.is_none() && p.position_half_pt.is_none() && p.kern_half_pt.is_none());
}

#[test]
fn vertical_alignment_toggles() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::ToggleVertAlign { kind: VertKind::Superscript });
    assert_eq!(props(&s, 0, 0).vert_align.as_deref(), Some("superscript"));
    fmt(&mut s, &mut cx, FormatCmd::ToggleVertAlign { kind: VertKind::Subscript });
    assert_eq!(props(&s, 0, 0).vert_align.as_deref(), Some("subscript"));
    fmt(&mut s, &mut cx, FormatCmd::ToggleVertAlign { kind: VertKind::Subscript });
    assert_eq!(props(&s, 0, 0).vert_align, None);
}

#[test]
fn clear_formatting_selection_caret_and_tracked() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 20.0 });
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "center".into() });
    assert_eq!(pp(&s, 0).alignment.as_deref(), Some("center"));
    fmt(&mut s, &mut cx, FormatCmd::ClearFormatting);
    assert_eq!(props(&s, 0, 0).sz_half_pt, None);
    assert_eq!(pp(&s, 0).alignment, None);
    // a caret: the paragraph goes back to plain and the typed text is plain
    select(&mut s, &mut cx, (1, 3), (1, 3));
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    fmt(&mut s, &mut cx, FormatCmd::ClearFormatting);
    assert_eq!(s.pending_props().unwrap().props.bold, Some(false));
    // tracked: the formatting becomes a revision (`fmt`/`pfmt`) and the direct format is kept as data
    select(&mut s, &mut cx, (0, 0), (0, 5));
    fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 20.0 });
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetTracking { on: true }));
    fmt(&mut s, &mut cx, FormatCmd::ClearFormatting);
    assert!(props(&s, 0, 0).fmt.is_some());
}

#[test]
fn change_case_every_mode() {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "hello WORLD. second Sentence! ǆ ß");
    select(&mut s, &mut cx, (0, 0), (0, 33));
    let all = |s: &Session| s.document().body.paras.get(0).unwrap().runs.iter().map(|r| r.text.to_string_lossy()).collect::<String>();
    for (mode, want) in [
        (CaseMode::Upper, "HELLO WORLD. SECOND SENTENCE! Ǆ ß"),
        (CaseMode::Lower, "hello world. second sentence! ǆ ß"),
        (CaseMode::Sentence, "Hello world. Second sentence! Ǆ ß"),
        (CaseMode::Title, "Hello World. Second Sentence! Ǆ ß"),
        (CaseMode::Toggle, "hELLO wORLD. sECOND sENTENCE! ǆ ß"),
    ] {
        fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode });
        assert_eq!(all(&s), want, "{mode:?}");
    }
    // Shift+F3: not all upper, first letter lower, has `.` → sentence
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Lower });
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Next });
    assert_eq!(all(&s), "Hello world. Second sentence! Ǆ ß");
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Next });
    assert!(all(&s).starts_with("HELLO WORLD."));
    // (`ß` keeps "all upper" from being true: `all === all.toUpperCase()` compares 'ß' with 'SS')
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Next });
    assert!(all(&s).starts_with("HELLO WORLD."));
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Lower });
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Upper });
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Lower });
    assert!(all(&s).starts_with("hello world."));
    // it takes `allCaps` off, and a caret does nothing (no step)
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::AllCaps, button: "single".into() });
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Upper });
    assert_eq!(props(&s, 0, 0).all_caps, Some(false));
    select(&mut s, &mut cx, (0, 2), (0, 2));
    let n = steps(&s);
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Upper });
    assert_eq!(steps(&s), n);
}

#[test]
fn format_painter_and_select_similar() {
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "right".into() });
    select(&mut s, &mut cx, (0, 2), (0, 2));
    let painter: FormatPainter = s.copy_format().unwrap();
    assert_eq!(painter.run.bold, Some(true));
    assert_eq!(painter.para.alignment.as_deref(), Some("right"));
    select(&mut s, &mut cx, (1, 0), (1, 6));
    fmt(&mut s, &mut cx, FormatCmd::ApplyPainter { painter: Box::new(painter) });
    assert_eq!(props(&s, 1, 0).bold, Some(true));
    assert_eq!(pp(&s, 1).alignment.as_deref(), Some("right"));
    // select similar: the main range is the stretch of the selection
    select(&mut s, &mut cx, (0, 1), (0, 3));
    fmt(&mut s, &mut cx, FormatCmd::SelectSimilarFormatting);
    assert_eq!(s.selection().anchor, DocPosition::new(0, 0));
    assert_eq!(s.selection().focus, DocPosition::new(0, 5));
    // the policy forbids the painter
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetPolicy { policy: Some(FormattingPolicy::template_document()) }));
    assert!(s.copy_format().is_none());
}

#[test]
fn selection_formatting_is_one_undo_step_and_events_are_in_ts_order() {
    let (mut s, mut cx) = doc();
    let n = steps(&s);
    let c = fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 14.0 });
    assert_eq!(steps(&s), n + 1);
    assert!(c.events.contains(&Event::Change));
    assert!(c.dirty && c.can_undo);
    assert_eq!(c.stories.len(), 1);
    assert!(!c.stories[0].structure);
    s.undo(&mut cx.cx());
    assert_eq!(props(&s, 0, 0).sz_half_pt, None);
    assert_eq!(steps(&s), n);
}

// ───────────────────────────── paragraph formatting ─────────────────────────────

#[test]
fn alignment_spacing_shading_and_indents() {
    let (mut s, mut cx) = doc();
    select(&mut s, &mut cx, (0, 2), (1, 2));
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "both".into() });
    assert_eq!(pp(&s, 0).alignment.as_deref(), Some("both"));
    assert_eq!(pp(&s, 1).alignment.as_deref(), Some("both"));
    para(&mut s, &mut cx, ParagraphCmd::SetLineSpacing { line: 360.0 });
    assert_eq!(pp(&s, 1).line_twentieths, Some(JsNumber::Num(360.0)));
    para(&mut s, &mut cx, ParagraphCmd::SetSpaceAfter { twips: Some(120.0) });
    para(&mut s, &mut cx, ParagraphCmd::SetSpaceBefore { twips: Some(60.0) });
    let p = pp(&s, 0);
    assert_eq!(p.space_after_twips, Some(JsNumber::Num(120.0)));
    assert_eq!(p.space_before_twips, Some(JsNumber::Num(60.0)));
    // the auto flag is `undefined` (a key)
    assert!(json(&p).contains("\"spaceAfterTwips\":120"));
    para(&mut s, &mut cx, ParagraphCmd::SetShading { hex: Some("ffff00".into()) });
    assert_eq!(pp(&s, 0).shading_hex.as_deref(), Some("ffff00"));
    para(&mut s, &mut cx, ParagraphCmd::SetShading { hex: None });
    assert_eq!(pp(&s, 0).shading_hex, None);
    para(&mut s, &mut cx, ParagraphCmd::SetSpecialIndent { kind: SpecialIndent::Hanging });
    assert_eq!(pp(&s, 0).indent_first_line_twips, Some(JsNumber::Num(-708.0)));
    para(&mut s, &mut cx, ParagraphCmd::SetSpecialIndent { kind: SpecialIndent::None });
    assert_eq!(pp(&s, 0).indent_first_line_twips, None);
    para(&mut s, &mut cx, ParagraphCmd::SetIndentRight { twips: Some(360.0) });
    assert_eq!(pp(&s, 0).indent_right_twips, Some(JsNumber::Num(360.0)));
    // indent: 720 per step, never below 0 (0 is `undefined`)
    para(&mut s, &mut cx, ParagraphCmd::Indent { delta: 1 });
    assert_eq!(pp(&s, 1).indent_left_twips, Some(JsNumber::Num(720.0)));
    para(&mut s, &mut cx, ParagraphCmd::Indent { delta: -1 });
    para(&mut s, &mut cx, ParagraphCmd::Indent { delta: -1 });
    assert_eq!(pp(&s, 1).indent_left_twips, None);
}

#[test]
fn paragraph_indents_write_only_what_differs_from_the_inherited() {
    let (mut s, mut cx) = doc();
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphIndents { ind: IndentPatch { left: Some(720.0), first_line: Some(-360.0), ..IndentPatch::default() } },
    );
    let p = pp(&s, 0);
    assert_eq!(p.indent_left_twips, Some(JsNumber::Num(720.0)));
    assert_eq!(p.indent_first_line_twips, Some(JsNumber::Num(-360.0)));
    assert_eq!(s.paragraph_indents().left, 720.0);
    assert_eq!(s.paragraph_indents().first_line, -360.0);
    // back to the inherited (0): the keys go `undefined`
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphIndents { ind: IndentPatch { left: Some(0.0), first_line: Some(0.0), right: Some(0.0) } },
    );
    let p = pp(&s, 0);
    assert_eq!(p.indent_left_twips, None);
    assert_eq!(p.indent_first_line_twips, None);
    // nothing to change: the step is still taken (the TS snapshots first)
    let n = steps(&s);
    para(&mut s, &mut cx, ParagraphCmd::SetParagraphIndents { ind: IndentPatch::default() });
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn tab_stops_replace_and_only() {
    let (mut s, mut cx) = doc();
    let t = |pos: f64, a: &str, l: Option<&str>| TabStopInfo { pos, align: a.into(), leader: l.map(Into::into) };
    para(&mut s, &mut cx, ParagraphCmd::SetTabStops { tabs: vec![t(3600.0, "right", Some("dot")), t(720.0, "left", None)], only: None });
    let p = pp(&s, 0);
    let tabs = p.tab_stops.unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].pos_twips, JsNumber::Num(720.0));
    assert_eq!(&*tabs[1].align, "right");
    assert_eq!(s.paragraph_tab_stops().len(), 2);
    // only: the other paragraphs keep their own
    select(&mut s, &mut cx, (0, 0), (1, 1));
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetTabStops {
            tabs: vec![],
            only: Some(TabOnly { add: Some(vec![t(1000.0, "center", None)]), remove: Some(vec![720.0]) }),
        },
    );
    let p0: Vec<f64> = pp(&s, 0).tab_stops.unwrap().iter().map(|x| x.pos_twips.to_number()).collect();
    assert_eq!(p0, vec![1000.0, 3600.0]);
    let p1: Vec<f64> = pp(&s, 1).tab_stops.unwrap().iter().map(|x| x.pos_twips.to_number()).collect();
    assert_eq!(p1, vec![1000.0]);
    // clearing all: the key is `undefined`
    para(&mut s, &mut cx, ParagraphCmd::SetTabStops { tabs: vec![], only: None });
    assert!(pp(&s, 0).tab_stops.is_none());
}

#[test]
fn flow_and_layout_boxes() {
    let (mut s, mut cx) = doc();
    // flow: only what differs from the inherited; widow control is ON by default
    let n = steps(&s);
    para(&mut s, &mut cx, ParagraphCmd::SetParagraphFlow { f: FlowArgs { widow_control: Some(true), ..FlowArgs::default() } });
    assert_eq!(steps(&s), n);
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphFlow { f: FlowArgs { widow_control: Some(false), keep_next: Some(true), ..FlowArgs::default() } },
    );
    assert_eq!(pp(&s, 0).widow_control, Some(false));
    assert_eq!(pp(&s, 0).keep_next, Some(true));
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphFlow {
            f: FlowArgs { widow_control: Some(true), textbox_tight_wrap: Some("allLines".into()), ..FlowArgs::default() },
        },
    );
    assert_eq!(pp(&s, 0).widow_control, None);
    assert_eq!(pp(&s, 0).textbox_tight_wrap.as_deref(), Some("allLines"));
    // layout: clamped, auto, no step when nothing changes
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphLayout { v: LayoutArgs { left: Some(1e9), before: Some(SpacingArg::Auto), ..LayoutArgs::default() } },
    );
    let p = pp(&s, 0);
    assert_eq!(p.indent_left_twips, Some(JsNumber::Num(31680.0)));
    assert_eq!(p.space_before_auto, Some(true));
    let n = steps(&s);
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphLayout { v: LayoutArgs { left: Some(31680.0), before: Some(SpacingArg::Auto), ..LayoutArgs::default() } },
    );
    assert_eq!(steps(&s), n);
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphLayout {
            v: LayoutArgs { after: Some(SpacingArg::Twips(-50.0)), right: Some(f64::NAN), ..LayoutArgs::default() },
        },
    );
    assert_eq!(pp(&s, 0).space_after_twips, Some(JsNumber::Num(0.0)));
}

fn spec(val: &str, sz: Option<f64>, color: Option<&str>) -> BorderSpec {
    BorderSpec { val: val.into(), sz: sz.map(JsNumber::Num), color: color.map(Into::into), ..BorderSpec::default() }
}

#[test]
fn borders_presets_and_toggle() {
    let (mut s, mut cx) = doc();
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphBorders {
            sides: Some(vec![
                (ParaBorderSide::Top, Some(spec("single", Some(8.0), Some("ff0000")))),
                (ParaBorderSide::Left, Some(spec("single", None, Some("auto")))),
            ]),
        },
    );
    let b = pp(&s, 0).borders.unwrap();
    // `{ style, sizeEighthPt, spacePt, color }`: the colour key comes last
    assert_eq!(
        json(&*b),
        "{\"top\":{\"style\":\"single\",\"sizeEighthPt\":8,\"spacePt\":1,\"color\":\"ff0000\"},\"left\":{\"style\":\"single\",\"sizeEighthPt\":4,\"spacePt\":4}}"
    );
    assert_eq!(s.paragraph_border_sides(), vec![ParaBorderSide::Top, ParaBorderSide::Left]);
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphBorders {
            sides: Some(vec![(ParaBorderSide::Top, None), (ParaBorderSide::Left, Some(spec("nil", None, None)))]),
        },
    );
    assert!(pp(&s, 0).borders.is_none());
    // presets toggle: all selected paragraphs already have them → off
    select(&mut s, &mut cx, (0, 0), (1, 3));
    para(&mut s, &mut cx, ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::Bottom });
    assert!(pp(&s, 0).borders.is_some() && pp(&s, 1).borders.is_some());
    para(&mut s, &mut cx, ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::Bottom });
    assert!(pp(&s, 0).borders.is_none() && pp(&s, 1).borders.is_none());
    para(&mut s, &mut cx, ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::All });
    assert_eq!(s.paragraph_border_sides().len(), 5);
    para(&mut s, &mut cx, ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::None });
    assert!(pp(&s, 0).borders.is_none());
    // the button on the caret paragraph
    select(&mut s, &mut cx, (0, 0), (0, 0));
    para(&mut s, &mut cx, ParagraphCmd::ToggleBorder);
    assert!(pp(&s, 0).borders.is_some());
    para(&mut s, &mut cx, ParagraphCmd::ToggleBorder);
    assert!(pp(&s, 0).borders.is_none());
}

#[test]
fn horizontal_line_takes_a_paragraph_of_its_own() {
    let (mut s, mut cx) = doc();
    select(&mut s, &mut cx, (0, 5), (0, 5));
    let n = s.paras().unwrap().len();
    para(&mut s, &mut cx, ParagraphCmd::InsertHorizontalLine);
    // "Hello" / line / " World" / "Second one"
    assert_eq!(s.paras().unwrap().len(), n + 2);
    let l = s.document().body.paras.get(1).unwrap();
    assert_eq!(l.runs.len(), 1);
    assert!(l.runs.first().unwrap().ext().unwrap().shape.as_ref().unwrap().hr == Some(true));
    assert_eq!(run_text(&s, 2, 0), " World");
    assert_eq!(s.selection().focus, DocPosition::new(2, 0));
    // one undo step puts the document back
    s.undo(&mut cx.cx());
    assert_eq!(s.paras().unwrap().len(), n);
    // the document still saves and reopens with the line
    para(&mut s, &mut cx, ParagraphCmd::InsertHorizontalLine);
    let bytes = s.save_docx().unwrap();
    assert!(Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).is_ok());
}

#[test]
fn clear_paragraph_formatting_keeps_the_style() {
    let (mut s, mut cx) = doc();
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "center".into() });
    para(&mut s, &mut cx, ParagraphCmd::SetShading { hex: Some("cccccc".into()) });
    para(&mut s, &mut cx, ParagraphCmd::ClearParagraphFormatting);
    assert_eq!(pp(&s, 0).alignment, None);
    assert_eq!(pp(&s, 0).shading_hex, None);
}

// ───────────────────────────── error and refusal paths ─────────────────────────────

#[test]
fn locked_read_view_and_policy_answer_without_a_change() {
    let (mut s, mut cx) = doc();
    let n = steps(&s);
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let c = fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    assert!(c.events.is_empty());
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "center".into() });
    para(&mut s, &mut cx, ParagraphCmd::InsertHorizontalLine);
    fmt(&mut s, &mut cx, FormatCmd::ChangeCase { mode: CaseMode::Upper });
    fmt(&mut s, &mut cx, FormatCmd::ClearFormatting);
    assert_eq!(steps(&s), n);
    assert_eq!(props(&s, 0, 0).bold, Some(false));
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: false }));
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetReadView { on: true }));
    fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 30.0 });
    assert_eq!(steps(&s), n);
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetReadView { on: false }));
    // the template policy: fonts, spacing, borders, shading, tabs and the painter are denied
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetPolicy { policy: Some(FormattingPolicy::template_document()) }));
    for (c, name) in [
        (Command::Format(FormatCmd::SetFontSize { pt: 30.0 }), "setFontSize"),
        (Command::Format(FormatCmd::SetFontFamily { name: "Arial".into() }), "setFontFamily"),
        (Command::Format(FormatCmd::SetFontFormat { f: Box::new(ff(0)) }), "setFontFormat"),
        (Command::Format(FormatCmd::GrowFont { delta: 1.0 }), "growFont"),
        (Command::Format(FormatCmd::SetCharSpacing { twips: Some(5.0) }), "setCharSpacing"),
        (Command::Format(FormatCmd::ToggleVertAlign { kind: VertKind::Superscript }), "toggleVertAlign"),
        (Command::Format(FormatCmd::SetTextEffects { patch: fx(0) }), "setTextEffects"),
        (Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::AllCaps, button: "single".into() }), "toggleRunProp"),
        (Command::Paragraph(ParagraphCmd::SetLineSpacing { line: 300.0 }), "setLineSpacing"),
        (Command::Paragraph(ParagraphCmd::SetShading { hex: Some("ffffff".into()) }), "setShading"),
        (Command::Paragraph(ParagraphCmd::ToggleBorder), "toggleBorder"),
        (Command::Paragraph(ParagraphCmd::InsertHorizontalLine), "insertHorizontalLine"),
        (Command::Paragraph(ParagraphCmd::SetTabStops { tabs: vec![], only: None }), "setTabStops"),
        (Command::Paragraph(ParagraphCmd::ParagraphBorderPreset { kind: BorderPreset::None }), "setParagraphBorders"),
        (Command::Paragraph(ParagraphCmd::SetSpecialIndent { kind: SpecialIndent::Hanging }), "setSpecialIndent"),
        (
            Command::Paragraph(ParagraphCmd::SetParagraphLayout {
                v: LayoutArgs { before: Some(SpacingArg::Twips(5.0)), ..LayoutArgs::default() },
            }),
            "setParagraphLayout",
        ),
    ] {
        let n = steps(&s);
        let cs = run(&mut s, &mut cx, c);
        assert_eq!(cs.events, vec![Event::Blocked { command: name.into() }], "{name}");
        assert_eq!(steps(&s), n);
    }
    // the allowed ones run: bold, alignment, Steps indentation
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    assert_eq!(props(&s, 0, 0).bold, Some(true));
    para(&mut s, &mut cx, ParagraphCmd::SetAlignment { alignment: "center".into() });
    assert_eq!(pp(&s, 0).alignment.as_deref(), Some("center"));
    // layout with only indents: the indent rule (Steps) forbids it; spacing only: Spacing denied
    let cs = para(&mut s, &mut cx, ParagraphCmd::SetParagraphLayout { v: LayoutArgs { left: Some(1.0), ..LayoutArgs::default() } });
    assert_eq!(cs.events, vec![Event::Blocked { command: "setParagraphLayout".into() }]);
}

#[test]
fn hostile_but_valid_arguments_change_nothing_or_something_sane() {
    let (mut s, mut cx) = doc();
    // a size from NaN / infinity is a property (a number the model can hold), not a panic
    for pt in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e300] {
        fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt });
    }
    for d in [f64::NAN, f64::NEG_INFINITY, 1e300] {
        fmt(&mut s, &mut cx, FormatCmd::GrowFont { delta: d });
    }
    // tab stops with NaN / duplicate / huge positions
    let t = |pos: f64| TabStopInfo { pos, align: "left".into(), leader: None };
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetTabStops { tabs: vec![t(f64::NAN), t(f64::NAN), t(1e300), t(-1e300), t(f64::INFINITY)], only: None },
    );
    para(
        &mut s,
        &mut cx,
        ParagraphCmd::SetParagraphIndents { ind: IndentPatch { left: Some(f64::NAN), right: Some(f64::INFINITY), first_line: Some(-0.0) } },
    );
    // the document still saves and reopens
    let bytes = s.save_docx().unwrap();
    assert!(Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).is_ok());
}

#[test]
fn works_inside_a_cell_a_note_and_a_header_over_the_corpus() {
    // every F1 op of the harness tables over corpus documents with tables: the story of the context
    let mut cx = Cx::new();
    let mut ran = 0;
    let mut sh = d2b_f1_support::Shell::default();
    for path in d2a_support::sample(40) {
        let Some(mut s) = d2a_support::open(std::fs::read(&path).unwrap()) else { continue };
        let Some(id) = s.document().notes.footnotes.keys().next().map(|k| k.get()) else { continue };
        run(
            &mut s,
            &mut cx,
            Command::Context(uo_docx::session::ops::context::ContextCmd::EnterNote { footnote: true, id, caret: None, at_end: true }),
        );
        let len = s.story().unwrap().paras.iter().next().map_or(0, uo_docx::session::text::para_length);
        select(&mut s, &mut cx, (0, 0), (0, u32::try_from(len).unwrap()));
        for code in ["tg:0", "fs:2", "co:3", "ff:1", "cc:3", "al:1", "pb:1", "bp:5", "ts:1", "pf:2", "cf"] {
            d2b_f1_support::step(&mut s, code, &mut cx, &mut sh);
            ran += 1;
        }
        s.undo(&mut cx.cx());
        if ran > 40 {
            break;
        }
    }
    assert!(ran > 0, "no corpus document with a footnote");
}

#[test]
fn json_of_keys_is_in_ts_order() {
    // the key order of a patched run (an `Object.assign` of a literal): existing keys stay, new go last
    let (mut s, mut cx) = doc();
    fmt(&mut s, &mut cx, FormatCmd::SetFontSize { pt: 12.0 });
    fmt(&mut s, &mut cx, FormatCmd::SetColor { color: Some(ColorArg::Hex("ff0000".into())) });
    fmt(&mut s, &mut cx, FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() });
    let p = props(&s, 0, 0);
    let j = json(&p);
    let i = |k: &str| j.find(k).unwrap();
    assert!(i("\"bold\"") < i("\"szHalfPt\"") && i("\"szHalfPt\"") < i("\"color\""), "{j}");
}
