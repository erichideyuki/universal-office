//! Shared helpers of the D2b-F1 tests: the argument tables of the F1 ops of the Chromium harness
//! (`js/capture-d2b-f1-session.mjs`, same tables by index) and the decoding of an op code into
//! session commands.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod hostile;

use uo_docx::js::JsNumber;
use uo_docx::model::effects::{FxColor, FxFill, FxGlow, FxOutline, FxShadow};
use uo_docx::model::props::BorderSpec;
use uo_docx::session::ops::format::{
    CaseMode, ColorArg, FontFormat, FormatCmd, FxPart, GradientArg, OtPart, RunToggle, ThemeColorRef, VertKind,
};
use uo_docx::session::ops::paragraph::{
    BorderPreset, FlowArgs, IndentPatch, LayoutArgs, ParaBorderSide, ParagraphCmd, SpacingArg, SpecialIndent, TabOnly, TabStopInfo,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Outcome, Session};
use uo_docx::text::Utf16;
use uo_jsnum::parse::to_number;

pub const TOGGLES: [RunToggle; 8] = [
    RunToggle::Bold,
    RunToggle::Italic,
    RunToggle::Underline,
    RunToggle::Strike,
    RunToggle::DoubleStrike,
    RunToggle::AllCaps,
    RunToggle::SmallCaps,
    RunToggle::Hidden,
];
pub const UL: [&str; 7] = ["none", "single", "double", "thick", "dotted", "wave", "words"];
pub const CASES: [CaseMode; 6] = [CaseMode::Upper, CaseMode::Lower, CaseMode::Sentence, CaseMode::Title, CaseMode::Toggle, CaseMode::Next];
pub const COLORS: [Option<&str>; 8] = [
    Some("ff0000"),
    Some("#00aa00"),
    Some("accent1"),
    Some("accent2:t0.4"),
    Some("text1:s0.25"),
    Some("background1:t0.8"),
    Some("dark2"),
    None,
];
pub const ALIGN: [&str; 4] = ["left", "center", "right", "both"];
pub const FAMILIES: [&str; 3] = ["Arial", "Calibri", "Liberation Sans"];
pub const HL: [&str; 4] = ["yellow", "green", "red", "-"];
pub const SIZES: [f64; 6] = [8.0, 10.5, 11.25, 14.0, 22.5, 72.4];
pub const PRESETS: [BorderPreset; 10] = [
    BorderPreset::Bottom,
    BorderPreset::Top,
    BorderPreset::Left,
    BorderPreset::Right,
    BorderPreset::None,
    BorderPreset::All,
    BorderPreset::Outside,
    BorderPreset::Inside,
    BorderPreset::InsideH,
    BorderPreset::InsideV,
];

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `colorOf(i)` of the harness.
pub fn color(i: usize) -> Option<ColorArg> {
    let c = COLORS[i]?;
    let is_hex = c.trim_start_matches('#').len() == 6 && c.trim_start_matches('#').chars().all(|ch| ch.is_ascii_hexdigit());
    if is_hex {
        return Some(ColorArg::Hex(c.to_owned()));
    }
    let (name, m) = match c.split_once(':') {
        Some((a, b)) => (a, Some(b)),
        None => (c, None),
    };
    let (mut tint, mut shade) = (None, None);
    if let Some(m) = m {
        let v: f64 = to_number(&m[1..]);
        if m.starts_with('t') {
            tint = Some(v);
        } else {
            shade = Some(v);
        }
    }
    Some(ColorArg::Theme(ThemeColorRef { theme_color: name.to_owned(), tint, shade }))
}

fn rgb(v: &str, mods: &[(&str, f64)]) -> FxColor {
    FxColor {
        rgb: Some(v.into()),
        mods: (!mods.is_empty()).then(|| mods.iter().map(|(k, x)| ((*k).into(), n(*x))).collect()),
        ..FxColor::default()
    }
}

fn scheme(v: &str, mods: &[(&str, f64)]) -> FxColor {
    FxColor {
        scheme: Some(v.into()),
        mods: (!mods.is_empty()).then(|| mods.iter().map(|(k, x)| ((*k).into(), n(*x))).collect()),
        ..FxColor::default()
    }
}

fn solid(c: FxColor) -> FxFill {
    FxFill { r#type: "solid".into(), color: Some(c), ..FxFill::default() }
}

pub fn fx(i: usize) -> Vec<FxPart> {
    match i {
        0 => vec![FxPart::Fill(Some(solid(rgb("FF0000", &[]))))],
        1 => vec![FxPart::Outline(None)],
        2 => vec![FxPart::Shadow(Some(FxShadow {
            blur_rad: n(50800.0),
            dist: n(38100.0),
            dir: n(2_700_000.0),
            sx: n(100_000.0),
            sy: n(100_000.0),
            kx: n(0.0),
            ky: n(0.0),
            algn: "tl".into(),
            color: rgb("000000", &[("alpha", 60000.0)]),
            ..FxShadow::default()
        }))],
        3 => vec![
            FxPart::Glow(Some(FxGlow { rad: n(63500.0), color: scheme("accent1", &[("alpha", 60000.0)]), ..FxGlow::default() })),
            FxPart::Fill(None),
        ],
        4 => vec![FxPart::Props3d(Some("<w14:props3d/>".into())), FxPart::Scene3d(None)],
        _ => vec![
            FxPart::Fill(Some(solid(scheme("accent2", &[])))),
            FxPart::Outline(Some(FxOutline {
                w: Some(n(9525.0)),
                cap: Some("flat".into()),
                cmpd: Some("sng".into()),
                algn: Some("ctr".into()),
                fill: Some(Box::new(solid(rgb("00FF00", &[])))),
                dash: Some("solid".into()),
                join: Some("round".into()),
                ..FxOutline::default()
            })),
        ],
    }
}

pub fn ot(i: usize) -> Vec<OtPart> {
    match i {
        0 => vec![OtPart::Ligatures(Some("standard".into()))],
        1 => vec![OtPart::NumForm(Some("oldStyle".into())), OtPart::NumSpacing(Some("tabular".into()))],
        2 => vec![OtPart::Ligatures(None), OtPart::NumForm(Some("default".into()))],
        3 => vec![OtPart::StylisticSets(Some(vec![1.0, 3.0])), OtPart::CntxtAlts(Some(true))],
        _ => vec![OtPart::StylisticSets(Some(vec![])), OtPart::CntxtAlts(Some(false))],
    }
}

pub fn grad(i: usize) -> Option<GradientArg> {
    match i {
        0 => None,
        1 => Some(GradientArg { angle_deg: 90.0, light: true }),
        _ => Some(GradientArg { angle_deg: 45.5, light: false }),
    }
}

pub fn ff(i: usize) -> FontFormat {
    let d = FontFormat::default();
    match i {
        0 => FontFormat { font_family: Some("Arial".into()), size_pt: Some(12.5), bold: Some(true), ..d },
        1 => FontFormat {
            underline: Some("double".into()),
            underline_color: Some(Some(ColorArg::Hex("00FF00".into()))),
            color: Some(Some(ColorArg::Theme(ThemeColorRef { theme_color: "accent1".into(), tint: Some(0.4), shade: None }))),
            ..d
        },
        2 => FontFormat { strike: Some(true), superscript: Some(true), scale_pct: Some(100.0), spacing_twips: Some(0.0), ..d },
        3 => FontFormat {
            italic: Some(false),
            subscript: Some(true),
            scale_pct: Some(150.0),
            spacing_twips: Some(20.0),
            position_half_pt: Some(4.0),
            kern_min_half_pt: Some(0.0),
            ..d
        },
        4 => FontFormat {
            underline: Some("none".into()),
            color: Some(None),
            underline_color: Some(None),
            small_caps: Some(true),
            all_caps: Some(false),
            hidden: Some(true),
            ..d
        },
        5 => d,
        _ => FontFormat { double_strike: Some(true), underline: Some("single".into()), ..d },
    }
}

fn tab(pos: f64, align: &str, leader: Option<&str>) -> TabStopInfo {
    TabStopInfo { pos, align: align.into(), leader: leader.map(Into::into) }
}

pub fn tabs(i: usize) -> (Vec<TabStopInfo>, Option<TabOnly>) {
    match i {
        0 => (vec![], None),
        1 => (vec![tab(720.0, "left", None), tab(3600.0, "right", Some("dot"))], None),
        2 => (vec![tab(1440.0, "center", None), tab(1440.0, "decimal", None)], None),
        3 => (vec![tab(2000.4, "bar", None)], None),
        4 => (vec![], Some(TabOnly { add: Some(vec![tab(1000.0, "left", None)]), remove: Some(vec![720.0]) })),
        _ => (vec![], Some(TabOnly { add: None, remove: Some(vec![1440.2]) })),
    }
}

pub fn flow(i: usize) -> FlowArgs {
    let d = FlowArgs::default();
    match i {
        0 => FlowArgs { keep_next: Some(true), ..d },
        1 => FlowArgs { widow_control: Some(false), keep_lines: Some(true), ..d },
        2 => FlowArgs { page_break_before: Some(true), suppress_line_numbers: Some(true), suppress_auto_hyphens: Some(true), ..d },
        3 => FlowArgs { textbox_tight_wrap: Some("allLines".into()), ..d },
        4 => FlowArgs { widow_control: Some(true), keep_next: Some(false), ..d },
        _ => d,
    }
}

pub fn layout(i: usize) -> LayoutArgs {
    let d = LayoutArgs::default();
    match i {
        0 => LayoutArgs { left: Some(720.0), ..d },
        1 => LayoutArgs { right: Some(360.0), before: Some(SpacingArg::Auto), ..d },
        2 => LayoutArgs { before: Some(SpacingArg::Twips(240.0)), after: Some(SpacingArg::Twips(120.0)), ..d },
        3 => LayoutArgs { after: Some(SpacingArg::Auto), left: Some(-100_000.0), ..d },
        4 => LayoutArgs { before: Some(SpacingArg::Twips(0.0)), ..d },
        5 => LayoutArgs { left: Some(0.0), right: Some(0.0), ..d },
        _ => LayoutArgs { before: Some(SpacingArg::Twips(12.6)), ..d },
    }
}

fn spec(val: &str, sz: Option<f64>, space: Option<f64>, color: Option<&str>) -> BorderSpec {
    BorderSpec { val: val.into(), sz: sz.map(n), space: space.map(n), color: color.map(Into::into), ..BorderSpec::default() }
}

pub fn borders(i: usize) -> Option<Vec<(ParaBorderSide, Option<BorderSpec>)>> {
    match i {
        0 => None,
        1 => Some(vec![
            (ParaBorderSide::Top, Some(spec("single", Some(8.0), Some(2.0), Some("FF0000")))),
            (ParaBorderSide::Bottom, Some(spec("double", None, None, None))),
        ]),
        2 => Some(vec![(ParaBorderSide::Left, Some(spec("single", None, None, None))), (ParaBorderSide::Top, None)]),
        3 => Some(vec![
            (ParaBorderSide::Between, Some(spec("nil", None, None, None))),
            (ParaBorderSide::Bar, Some(spec("single", None, None, Some("auto")))),
        ]),
        _ => Some(vec![]),
    }
}

pub fn indents(i: usize) -> IndentPatch {
    let d = IndentPatch::default();
    match i {
        0 => IndentPatch { left: Some(720.0), ..d },
        1 => IndentPatch { first_line: Some(-360.0), right: Some(200.0), ..d },
        2 => IndentPatch { left: Some(0.0), right: Some(0.0), first_line: Some(0.0) },
        3 => IndentPatch { first_line: Some(360.0), ..d },
        _ => IndentPatch { left: Some(1000.5), ..d },
    }
}

fn num_or_none(s: &str) -> Option<f64> {
    if s == "-" { None } else { Some(to_number(s)) }
}

pub fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn done(o: Result<Outcome, uo_docx::session::CommandError>, code: &str) -> ChangeSet {
    match o {
        Ok(Outcome::Done(cs)) => cs,
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F1 scripts"),
        Err(e) => panic!("{code}: {e}"),
    }
}

/// The `ulButton` the shell keeps: after a command, per [`FormatCmd::underline_button_after`].
pub struct Shell {
    pub ul_button: String,
}

impl Default for Shell {
    fn default() -> Self {
        Shell { ul_button: "single".to_owned() }
    }
}

/// Runs one op code of the harness (the D2a ops and the F1 ops), returning the change set.
pub fn step(s: &mut Session, code: &str, cx: &mut super::d2a_support::Cx, sh: &mut Shell) -> ChangeSet {
    let f: Vec<&str> = code.split(':').collect();
    let i = |k: usize| f[k].parse::<usize>().unwrap();
    let pos = |k: usize| DocPosition::new(f[k].parse().unwrap(), f[k + 1].parse().unwrap());
    let run = |s: &mut Session, cmd: Command, cx: &mut super::d2a_support::Cx, sh: &mut Shell| -> ChangeSet {
        if let Command::Format(fc) = &cmd {
            sh.ul_button = fc.underline_button_after(&sh.ul_button);
        }
        done(s.exec(cmd, &mut cx.cx()), code)
    };
    let fmt = |c: FormatCmd| Command::Format(c);
    let para = |c: ParagraphCmd| Command::Paragraph(c);
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection { anchor: pos(1), focus: pos(3) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "un" => return s.undo(&mut cx.cx()),
        "re" => return s.redo(&mut cx.cx()),
        "pt" => {
            run(s, Command::Text(TextCmd::SetSelection { anchor: pos(1), focus: pos(1) }), cx, sh);
            let painter = s.copy_format();
            run(s, Command::Text(TextCmd::SetSelection { anchor: pos(3), focus: pos(5) }), cx, sh);
            return match painter {
                Some(p) => run(s, fmt(FormatCmd::ApplyPainter { painter: Box::new(p) }), cx, sh),
                None => done(s.exec(Command::Text(TextCmd::SetSelection { anchor: pos(3), focus: pos(5) }), &mut cx.cx()), code),
            };
        }
        "tg" => fmt(FormatCmd::ToggleRunProp { key: TOGGLES[i(1)], button: sh.ul_button.clone() }),
        "tu" => {
            fmt(FormatCmd::ToggleUnderline { style: if f[1] == "-" { None } else { Some(f[1].to_owned()) }, button: sh.ul_button.clone() })
        }
        "su" => fmt(FormatCmd::SetUnderline { style: UL[i(1)].to_owned() }),
        "uc" => fmt(FormatCmd::SetUnderlineColor { color: color(i(1)), button: sh.ul_button.clone() }),
        "fx" => fmt(FormatCmd::SetTextEffects { patch: fx(i(1)) }),
        "fp" => {
            let v: f64 = to_number(f[1]);
            fmt(FormatCmd::ApplyTextEffectPreset { index: if v < 0.0 { None } else { Some(v) } })
        }
        "ot" => fmt(FormatCmd::SetOpenType { patch: ot(i(1)) }),
        "tgr" => fmt(FormatCmd::SetTextGradient { v: grad(i(1)) }),
        "ff" => fmt(FormatCmd::SetFontFormat { f: Box::new(ff(i(1))) }),
        "va" => fmt(FormatCmd::ToggleVertAlign { kind: if i(1) == 1 { VertKind::Subscript } else { VertKind::Superscript } }),
        "fs" => fmt(FormatCmd::SetFontSize { pt: SIZES[i(1)] }),
        "gf" => fmt(FormatCmd::GrowFont { delta: to_number(f[1]) }),
        "fam" => fmt(FormatCmd::SetFontFamily { name: FAMILIES[i(1)].to_owned() }),
        "co" => fmt(FormatCmd::SetColor { color: color(i(1)) }),
        "hl" => fmt(FormatCmd::SetHighlight { name: if HL[i(1)] == "-" { None } else { Some(HL[i(1)].to_owned()) } }),
        "cf" => fmt(FormatCmd::ClearFormatting),
        "cc" => fmt(FormatCmd::ChangeCase { mode: CASES[i(1)] }),
        "cs" => fmt(FormatCmd::SetCharSpacing { twips: num_or_none(f[1]) }),
        "ss" => {
            let cs = run(s, fmt(FormatCmd::SelectSimilarFormatting), cx, sh);
            let sel = s.selection();
            let _ = cs;
            return run(s, Command::Text(TextCmd::SetSelection { anchor: sel.anchor, focus: sel.focus }), cx, sh);
        }
        "al" => para(ParagraphCmd::SetAlignment { alignment: ALIGN[i(1)].to_owned() }),
        "in" => para(ParagraphCmd::Indent { delta: if f[1] == "1" { 1 } else { -1 } }),
        "ls" => para(ParagraphCmd::SetLineSpacing { line: to_number(f[1]) }),
        "sa" => para(ParagraphCmd::SetSpaceAfter { twips: num_or_none(f[1]) }),
        "sb" => para(ParagraphCmd::SetSpaceBefore { twips: num_or_none(f[1]) }),
        "sh" => para(ParagraphCmd::SetShading { hex: if f[1] == "-" { None } else { Some(f[1].to_owned()) } }),
        "si" => {
            para(ParagraphCmd::SetSpecialIndent { kind: [SpecialIndent::None, SpecialIndent::FirstLine, SpecialIndent::Hanging][i(1)] })
        }
        "ir" => para(ParagraphCmd::SetIndentRight { twips: num_or_none(f[1]) }),
        "pi" => para(ParagraphCmd::SetParagraphIndents { ind: indents(i(1)) }),
        "ts" => {
            let (tabs, only) = tabs(i(1));
            para(ParagraphCmd::SetTabStops { tabs, only })
        }
        "pf" => para(ParagraphCmd::SetParagraphFlow { f: flow(i(1)) }),
        "pl" => para(ParagraphCmd::SetParagraphLayout { v: layout(i(1)) }),
        "pb" => para(ParagraphCmd::SetParagraphBorders { sides: borders(i(1)) }),
        "bp" => para(ParagraphCmd::ParagraphBorderPreset { kind: PRESETS[i(1)] }),
        "tb" => para(ParagraphCmd::ToggleBorder),
        "hr" => para(ParagraphCmd::InsertHorizontalLine),
        "cp" => para(ParagraphCmd::ClearParagraphFormatting),
        _ => panic!("step {code}"),
    };
    run(s, cmd, cx, sh)
}
