//! Replay of `tests/captures/theme.capture` (`tools/capture-theme.mjs`, the frozen TS
//! `readTheme`): `theme::read` gives the as-written view from which Word's rules (applied here
//! as uo-docx R2 will: colours, font sets, line widths) rebuild the TS theme exactly, on the
//! distinct theme parts of the oracle corpus and on seeded synthetic themes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_drawingml::theme::{FontSet, ThemePart, read};
use uo_jsnum::json::{push_number, push_string};
use uo_jsnum::object::js_object_order;
use uo_opc::xml::{ParseOptions, parse};

/// Word's `ThemeFontSet` from the view: `{ scripts, latin?, ea?, cs? }` with truthy values only.
fn font_set_json(out: &mut String, set: Option<&FontSet>) {
    let mut scripts: Vec<(String, String)> = Vec::new();
    if let Some(set) = set {
        for f in &set.fonts {
            if let (Some(s), Some(t)) = (&f.script, &f.typeface)
                && !s.is_empty()
                && !t.is_empty()
            {
                match scripts.iter_mut().find(|(k, _)| k == &**s) {
                    Some(e) => e.1 = t.to_string(),
                    None => scripts.push((s.to_string(), t.to_string())),
                }
            }
        }
    }
    out.push_str("{\"scripts\":{");
    for (i, (k, v)) in js_object_order(scripts).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_string(out, k);
        out.push(':');
        push_string(out, v);
    }
    out.push('}');
    if let Some(set) = set {
        for (name, v) in [("latin", &set.latin), ("ea", &set.ea), ("cs", &set.cs)] {
            if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                out.push_str(&format!(",\"{name}\":"));
                push_string(out, v);
            }
        }
    }
    out.push('}');
}

/// `JSON.stringify` of Word's theme (without the shape-style lists).
fn word_theme_json(t: &ThemePart) -> String {
    let mut out = String::from("{\"major\":");
    font_set_json(&mut out, t.major.as_ref());
    out.push_str(",\"minor\":");
    font_set_json(&mut out, t.minor.as_ref());
    out.push_str(",\"colors\":{");
    let mut colors: Vec<(String, String)> = Vec::new();
    for e in t.color_scheme.iter().flatten() {
        let key = e.name.strip_prefix("a:").unwrap_or(&e.name).to_string();
        // `srgb ?? sys?.lastClr ?? (sys?.val === 'window' ? 'FFFFFF' : sys?.val === 'windowText' ? '000000' : undefined)`
        let srgb = e.first("a:srgbClr").and_then(|c| c.val.as_deref());
        let sys = e.first("a:sysClr");
        let rule = || match sys.and_then(|s| s.val.as_deref()) {
            Some("window") => Some("FFFFFF"),
            Some("windowText") => Some("000000"),
            _ => None,
        };
        let v = srgb.or_else(|| sys.and_then(|s| s.last_clr.as_deref())).or_else(rule).map(String::from);
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            let v = v.to_uppercase();
            match colors.iter_mut().find(|(k, _)| *k == key) {
                Some(c) => c.1 = v,
                None => colors.push((key, v)),
            }
        }
    }
    for (i, (k, v)) in js_object_order(colors).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_string(&mut out, k);
        out.push(':');
        push_string(&mut out, v);
    }
    out.push('}');
    if let Some(ws) = &t.line_widths {
        out.push_str(",\"lineWidthsPt\":[");
        for (i, w) in ws.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let n = uo_jsnum::parse::parse_int(w.as_deref().unwrap_or("0"), 10);
            push_number(&mut out, (if uo_jsnum::conv::truthy(n) { n } else { 0.0 }) / 12700.0);
        }
        out.push(']');
    }
    out.push('}');
    out
}

fn theme_json(xml: &str) -> String {
    let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
    word_theme_json(&read(&doc))
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../write/packages/docx-layout/test/oracle")
}

fn docx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            docx_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "docx") {
            out.push(p);
        }
    }
}

#[test]
fn theme_view_rebuilds_the_ts_theme() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/captures/theme.capture")).unwrap();
    let mut wanted: Vec<(String, String)> = Vec::new();
    let mut synthetic = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.starts_with('@')) {
        let (key, json) = line.split_once('\t').unwrap();
        match key.strip_prefix("xml:") {
            Some(xml) => {
                assert_eq!(theme_json(xml), json, "{xml}");
                synthetic += 1;
            }
            None => wanted.push((key.to_string(), json.to_string())),
        }
    }
    assert_eq!(synthetic, 300);
    let mut files = Vec::new();
    docx_files(&corpus(), &mut files);
    let limits = uo_opc::Limits::DEFAULT;
    for f in files {
        if wanted.is_empty() {
            break;
        }
        let bytes = std::fs::read(&f).unwrap();
        let Ok(pkg) = uo_opc::Package::open(&bytes, &limits, &uo_opc::package::OpenOptions::default()) else { continue };
        let Ok(Some(t)) = pkg.part_text("word/theme/theme1.xml") else { continue };
        let h = uo_testkit::sha256::sha256(t.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
        if let Some(i) = wanted.iter().position(|(k, _)| *k == h) {
            let (_, json) = wanted.swap_remove(i);
            assert_eq!(theme_json(&t), json, "{}", f.display());
        }
    }
    assert!(wanted.is_empty(), "theme parts not found in the corpus: {wanted:?}");
}
