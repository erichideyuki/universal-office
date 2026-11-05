//! Parity of the pure model files of D2b-F1 against the frozen TS (record "D2b frontier briefs",
//! `docs/modules/uo-docx-d2b/F1.md`): replays `tests/parity/d2b-f1-ops.capture`, written by
//! `js/capture-d2b-f1-ops.mjs` — `caseMapped` (text-case.ts), the text-effect gallery and its
//! builders (text-effects-presets.ts), the ruler rules (`tabInfo`, `tabsDirect`, `dedupeTabs`,
//! `indentsDirect` of navigation.ts), the paragraph rules (`flowValue`, `flowDirect`,
//! `spacingDirect`, the limits of text-layout.ts), `themeTintShade` (page-design.ts) and
//! `plainTextSdt` (source.ts, its two regexes). Every result is compared as the TS prints it
//! (JSON, so the key order of the objects is part of it).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f1-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsNumber, JsonWriter};
use uo_docx::model::effects::FxColor;
use uo_docx::model::para::ParaProps;
use uo_docx::model::props::{IndSpec, PPr, TabSpec};
use uo_docx::session::ops::format::{
    CaseMode, ThemeColorRef, case_mapped, fx_outline, text_effect_gallery, text_glow, text_gradient, theme_tint_shade,
};
use uo_docx::session::ops::paragraph::{
    FLOW_FLAGS, IndentInfo, LAYOUT_INDENT_LIMITS, LAYOUT_SPACING_LIMITS, SpacingArg, TabStopInfo, dedupe_tabs, flow_direct, flow_value,
    indents_direct, plain_text_sdt, spacing_direct, tab_info, tabs_direct,
};
use uo_jsnum::json::{number, string};
use uo_jsnum::parse::to_number;
use uo_testkit::{Capture, Replay};

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn hex(u: &[u16]) -> String {
    u.iter().map(|x| format!("{x:x}")).collect::<Vec<_>>().join(".")
}

fn mode(s: &str) -> CaseMode {
    match s {
        "upper" => CaseMode::Upper,
        "lower" => CaseMode::Lower,
        "sentence" => CaseMode::Sentence,
        "title" => CaseMode::Title,
        _ => CaseMode::Toggle,
    }
}

fn json_of(v: &dyn uo_docx::js::JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap_or_else(|| "undefined".to_owned())
}

fn opt_num(s: &str) -> Option<f64> {
    if s == "-" { None } else { Some(to_number(s)) }
}

fn spec_list(s: &str) -> Vec<TabSpec> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|t| {
            let p: Vec<&str> = t.split(',').collect();
            let mut spec = TabSpec { val: p[0].into(), pos: JsNumber::Num(to_number(p[1])), ..TabSpec::default() };
            if !p[2].is_empty() {
                uo_docx::word::obj::set(&mut spec, "leader", |x| x.leader = Some(p[2].into()));
            }
            spec
        })
        .collect()
}

fn info_list(s: &str) -> Vec<TabStopInfo> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|t| {
            let p: Vec<&str> = t.split(',').collect();
            TabStopInfo { pos: to_number(p[0]), align: p[1].to_owned(), leader: (!p[2].is_empty()).then(|| p[2].to_owned()) }
        })
        .collect()
}

fn info_json(t: &TabStopInfo) -> String {
    let mut s = format!("{{\"pos\":{},\"align\":{}", number(t.pos), string(&t.align));
    if let Some(l) = &t.leader {
        s.push_str(&format!(",\"leader\":{}", string(l)));
    }
    s.push('}');
    s
}

fn infos_json(v: &[TabStopInfo]) -> String {
    format!("[{}]", v.iter().map(info_json).collect::<Vec<_>>().join(","))
}

fn colors(ci: usize) -> FxColor {
    let mods = |v: &[(&str, f64)]| Some(v.iter().map(|(k, x)| ((*k).into(), JsNumber::Num(*x))).collect());
    match ci {
        0 => FxColor { rgb: Some("000000".into()), ..FxColor::default() },
        1 => FxColor { scheme: Some("accent1".into()), ..FxColor::default() },
        2 => FxColor { rgb: Some("FF8800".into()), mods: mods(&[("lumMod", 75000.0)]), ..FxColor::default() },
        _ => FxColor { scheme: Some("tx1".into()), mods: mods(&[("alpha", 1000.0), ("lumOff", 5.0)]), ..FxColor::default() },
    }
}

fn run(key: &str, input: &str) -> String {
    let (kind, rest) = key.split_once('#').unwrap_or((key, ""));
    match kind {
        "case" => {
            let (m, h) = input.split_once(':').unwrap();
            hex(&case_mapped(&units(h), mode(m)))
        }
        "gallery" => {
            let i: usize = rest.parse().unwrap();
            json_of(&text_effect_gallery()[i])
        }
        "shadow" => {
            let i: usize = rest.parse().unwrap();
            let s = if i == 0 {
                uo_docx::session::ops::format::shadow_outer_bottom_right()
            } else {
                uo_docx::session::ops::format::shadow_outer_bottom()
            };
            json_of(&s)
        }
        "grad" | "glow" | "outline" => {
            let ci: usize = rest.split(':').next().unwrap().parse().unwrap();
            let p: Vec<&str> = input.rsplitn(3, '|').collect();
            match kind {
                "grad" => json_of(&text_gradient(&colors(ci), to_number(p[1]), p[0] == "true")),
                "glow" => json_of(&text_glow(to_number(p[0]), &colors(ci))),
                _ => json_of(&fx_outline(colors(ci), to_number(p[1]), p[0])),
            }
        }
        "tint" => {
            let (a, b) = input.split_once('|').unwrap();
            let r = ThemeColorRef { theme_color: "accent1".into(), tint: opt_num(a), shade: opt_num(b) };
            match theme_tint_shade(&r) {
                (Some(t), _) => format!("{{\"themeTint\":\"{t}\"}}"),
                (_, Some(s)) => format!("{{\"themeShade\":\"{s}\"}}"),
                _ => "{}".to_owned(),
            }
        }
        "tabs" => {
            let (inh, want) = input.split_once('|').unwrap();
            let inh = if inh == "-" { None } else { Some(spec_list(inh)) };
            let d = tabs_direct(inh.as_deref(), &info_list(want));
            d.map_or_else(|| "null".to_owned(), |v| json_of(&v))
        }
        "dedupe" => infos_json(&dedupe_tabs(&info_list(input))),
        "tabinfo" => {
            let v: Vec<TabStopInfo> = spec_list(input).iter().filter(|t| &*t.val != "clear").map(tab_info).collect();
            infos_json(&v)
        }
        "ind" => {
            let (inh, want) = input.split_once('|').unwrap();
            let inh = if inh == "-" {
                None
            } else {
                let p: Vec<Option<f64>> = inh.split(',').map(opt_num).collect();
                let n = |x: Option<f64>| x.map(JsNumber::Num);
                Some(IndSpec { left: n(p[0]), right: n(p[1]), hanging: n(p[2]), first_line: n(p[3]), ..IndSpec::default() })
            };
            let w: Vec<f64> = want.split(',').map(to_number).collect();
            let d = indents_direct(inh.as_ref(), IndentInfo { left: w[0], right: w[1], first_line: w[2] });
            let mut s = String::from("{");
            let names = ["indentLeftTwips", "indentRightTwips", "indentFirstLineTwips"];
            let mut first = true;
            for (n, v) in names.iter().zip(d) {
                if let Some(v) = v {
                    if !first {
                        s.push(',');
                    }
                    first = false;
                    s.push_str(&format!("\"{n}\":{}", number(v)));
                }
            }
            s.push('}');
            s
        }
        "flowvalue" => {
            let (k, v) = input.split_once('|').unwrap();
            let mut ppr = PPr::default();
            let b = match v {
                "-" => None,
                x => Some(x == "true"),
            };
            match k {
                "widowControl" => ppr.widow_control = b,
                "keepNext" => ppr.keep_next = b,
                "keepLines" => ppr.keep_lines = b,
                "pageBreakBefore" => ppr.page_break_before = b,
                "suppressLineNumbers" => ppr.suppress_line_numbers = b,
                _ => ppr.suppress_auto_hyphens = b,
            }
            assert!(FLOW_FLAGS.contains(&k));
            flow_value(&ppr, k).to_string()
        }
        "flowdirect" => {
            let (a, b) = input.split_once('|').unwrap();
            flow_direct(a == "true", b == "true").map_or_else(|| "null".to_owned(), |v| v.to_string())
        }
        "spacing" => {
            let p: Vec<&str> = input.split('|').collect();
            let (tw, auto) = p[0].split_once(',').unwrap();
            let want = if p[1] == "auto" { SpacingArg::Auto } else { SpacingArg::Twips(to_number(p[1])) };
            let mut patch = ParaProps::default();
            spacing_direct(&mut patch, to_number(tw), auto == "true", want, p[2] == "before");
            json_of(&patch)
        }
        "limits" => format!(
            "{{\"indent\":[{},{}],\"spacing\":[{},{}]}}",
            number(LAYOUT_INDENT_LIMITS.0),
            number(LAYOUT_INDENT_LIMITS.1),
            number(LAYOUT_SPACING_LIMITS.0),
            number(LAYOUT_SPACING_LIMITS.1)
        ),
        "sdt" => plain_text_sdt(Some(&String::from_utf16_lossy(&units(input)))).to_string(),
        _ => panic!("kind {kind}"),
    }
}

#[test]
fn d2b_f1_pure_model_files_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f1-ops.capture")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut n = 0;
    for rec in cap.records() {
        let Some(input) = rec.get("in") else { continue };
        let actual = run(&rec.key, input);
        replay.check(&rec.key, "in", input);
        replay.check(&rec.key, "out", &actual);
        n += 1;
    }
    let report = replay.finish();
    println!("d2b-f1-ops: {n} cases, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
}
