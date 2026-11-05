//! Hostile F1 commands: every command with the arguments a careless or malicious caller may pass.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{CASES, PRESETS, TOGGLES, fx};
use uo_docx::js::JsNumber;
use uo_docx::model::props::BorderSpec;
use uo_docx::session::Command;
use uo_docx::session::ops::format::{ColorArg, FontFormat, FormatCmd, GradientArg, OtPart, ThemeColorRef, VertKind};
use uo_docx::session::ops::paragraph::{
    FlowArgs, IndentPatch, LayoutArgs, ParaBorderSide, ParagraphCmd, SpacingArg, SpecialIndent, TabOnly, TabStopInfo,
};
use uo_testkit::generate::Rng;

/// A number a hostile caller may pass.
pub fn wild_f(rng: &mut Rng) -> f64 {
    [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        0.0,
        1e300,
        -1e300,
        1e-300,
        0.5,
        2_147_483_648.0,
        -2_147_483_649.0,
        9_007_199_254_740_993.0,
        720.0,
        -720.0,
    ][rng.below(14)]
}

fn wild_s(rng: &mut Rng) -> String {
    let long = "x".repeat(300);
    ["", "#", "##", "none", "single", "auto", "\u{0}", "😀", "a:b", long.as_str(), "default", "accent1", "FFFFFF"][rng.below(13)].to_owned()
}

fn wild_color(rng: &mut Rng) -> Option<ColorArg> {
    match rng.below(3) {
        0 => None,
        1 => Some(ColorArg::Hex(wild_s(rng))),
        _ => Some(ColorArg::Theme(ThemeColorRef {
            theme_color: wild_s(rng),
            tint: (rng.below(2) == 0).then(|| wild_f(rng)),
            shade: (rng.below(2) == 0).then(|| wild_f(rng)),
        })),
    }
}

fn wild_tabs(rng: &mut Rng) -> Vec<TabStopInfo> {
    (0..rng.below(5))
        .map(|_| TabStopInfo { pos: wild_f(rng), align: wild_s(rng), leader: (rng.below(2) == 0).then(|| wild_s(rng)) })
        .collect()
}

fn coin(rng: &mut Rng) -> Option<bool> {
    (rng.below(2) == 0).then(|| rng.below(2) == 0)
}

fn some_f(rng: &mut Rng) -> Option<f64> {
    (rng.below(2) == 0).then(|| wild_f(rng))
}

fn spacing(rng: &mut Rng) -> Option<SpacingArg> {
    match rng.below(3) {
        0 => None,
        1 => Some(SpacingArg::Auto),
        _ => Some(SpacingArg::Twips(wild_f(rng))),
    }
}

fn wild_border(rng: &mut Rng) -> (ParaBorderSide, Option<BorderSpec>) {
    let side = [
        ParaBorderSide::Top,
        ParaBorderSide::Left,
        ParaBorderSide::Bottom,
        ParaBorderSide::Right,
        ParaBorderSide::Between,
        ParaBorderSide::Bar,
    ][rng.below(6)];
    let spec = (rng.below(3) != 0).then(|| BorderSpec {
        val: wild_s(rng).as_str().into(),
        sz: some_f(rng).map(JsNumber::Num),
        space: some_f(rng).map(JsNumber::Num),
        color: (rng.below(2) == 0).then(|| wild_s(rng).as_str().into()),
        ..BorderSpec::default()
    });
    (side, spec)
}

/// One F1 command with hostile arguments (any command of either family).
pub fn hostile_f1(rng: &mut Rng) -> Command {
    match rng.below(35) {
        0 => Command::Format(FormatCmd::ToggleRunProp { key: TOGGLES[rng.below(8)], button: wild_s(rng) }),
        1 => Command::Format(FormatCmd::ToggleUnderline { style: (rng.below(2) == 0).then(|| wild_s(rng)), button: wild_s(rng) }),
        2 => Command::Format(FormatCmd::SetUnderline { style: wild_s(rng) }),
        3 => Command::Format(FormatCmd::SetUnderlineColor { color: wild_color(rng), button: wild_s(rng) }),
        4 => Command::Format(FormatCmd::SetTextEffects { patch: fx(rng.below(6)) }),
        5 => Command::Format(FormatCmd::ApplyTextEffectPreset { index: (rng.below(3) != 0).then(|| wild_f(rng)) }),
        6 => Command::Format(FormatCmd::SetOpenType {
            patch: vec![
                OtPart::Ligatures((rng.below(2) == 0).then(|| wild_s(rng))),
                OtPart::StylisticSets((rng.below(2) == 0).then(|| vec![wild_f(rng)])),
                OtPart::CntxtAlts(coin(rng)),
            ],
        }),
        7 => Command::Format(FormatCmd::SetTextGradient {
            v: (rng.below(3) != 0).then(|| GradientArg { angle_deg: wild_f(rng), light: rng.below(2) == 0 }),
        }),
        8 => Command::Format(FormatCmd::SetFontFormat {
            f: Box::new(FontFormat {
                font_family: (rng.below(2) == 0).then(|| wild_s(rng)),
                size_pt: some_f(rng),
                bold: coin(rng),
                underline: (rng.below(2) == 0).then(|| wild_s(rng)),
                underline_color: (rng.below(2) == 0).then(|| wild_color(rng)),
                color: (rng.below(2) == 0).then(|| wild_color(rng)),
                superscript: coin(rng),
                subscript: coin(rng),
                scale_pct: some_f(rng),
                spacing_twips: some_f(rng),
                position_half_pt: some_f(rng),
                kern_min_half_pt: some_f(rng),
                ..FontFormat::default()
            }),
        }),
        9 => Command::Format(FormatCmd::ToggleVertAlign {
            kind: if rng.below(2) == 0 { VertKind::Superscript } else { VertKind::Subscript },
        }),
        10 => Command::Format(FormatCmd::SetFontSize { pt: wild_f(rng) }),
        11 => Command::Format(FormatCmd::GrowFont { delta: wild_f(rng) }),
        12 => Command::Format(FormatCmd::SetFontFamily { name: wild_s(rng) }),
        13 => Command::Format(FormatCmd::SetColor { color: wild_color(rng) }),
        14 => Command::Format(FormatCmd::SetHighlight { name: (rng.below(2) == 0).then(|| wild_s(rng)) }),
        15 => Command::Format(FormatCmd::ClearFormatting),
        16 => Command::Format(FormatCmd::ChangeCase { mode: CASES[rng.below(6)] }),
        17 => Command::Format(FormatCmd::SetCharSpacing { twips: some_f(rng) }),
        18 => Command::Format(FormatCmd::SelectSimilarFormatting),
        19 => Command::Paragraph(ParagraphCmd::SetAlignment { alignment: wild_s(rng) }),
        20 => Command::Paragraph(ParagraphCmd::Indent { delta: [1, -1, 0, 127, -128][rng.below(5)] }),
        21 => Command::Paragraph(ParagraphCmd::SetLineSpacing { line: wild_f(rng) }),
        22 => Command::Paragraph(ParagraphCmd::SetSpaceAfter { twips: some_f(rng) }),
        23 => Command::Paragraph(ParagraphCmd::SetSpecialIndent {
            kind: [SpecialIndent::None, SpecialIndent::FirstLine, SpecialIndent::Hanging][rng.below(3)],
        }),
        24 => Command::Paragraph(ParagraphCmd::SetParagraphIndents {
            ind: IndentPatch { left: some_f(rng), right: some_f(rng), first_line: some_f(rng) },
        }),
        25 => Command::Paragraph(ParagraphCmd::SetTabStops {
            tabs: wild_tabs(rng),
            only: (rng.below(2) == 0).then(|| TabOnly {
                add: (rng.below(2) == 0).then(|| wild_tabs(rng)),
                remove: (rng.below(2) == 0).then(|| vec![wild_f(rng), wild_f(rng)]),
            }),
        }),
        26 => Command::Paragraph(ParagraphCmd::SetParagraphFlow {
            f: FlowArgs {
                widow_control: coin(rng),
                keep_next: coin(rng),
                textbox_tight_wrap: (rng.below(2) == 0).then(|| wild_s(rng)),
                ..FlowArgs::default()
            },
        }),
        27 => Command::Paragraph(ParagraphCmd::SetParagraphLayout {
            v: LayoutArgs { left: some_f(rng), right: some_f(rng), before: spacing(rng), after: spacing(rng) },
        }),
        28 => Command::Paragraph(ParagraphCmd::SetParagraphBorders {
            sides: (rng.below(4) != 0).then(|| (0..rng.below(4)).map(|_| wild_border(rng)).collect()),
        }),
        29 => Command::Paragraph(ParagraphCmd::ParagraphBorderPreset { kind: PRESETS[rng.below(10)] }),
        30 => Command::Paragraph(ParagraphCmd::ToggleBorder),
        31 => Command::Paragraph(ParagraphCmd::InsertHorizontalLine),
        32 => Command::Paragraph(ParagraphCmd::ClearParagraphFormatting),
        33 => Command::Paragraph(ParagraphCmd::SetSpaceBefore { twips: some_f(rng) }),
        _ => Command::Paragraph(ParagraphCmd::SetShading { hex: (rng.below(2) == 0).then(|| wild_s(rng)) }),
    }
}
