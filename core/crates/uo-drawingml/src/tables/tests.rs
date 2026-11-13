//! The generated tables against the frozen TS they came from (G11): while `shared/drawingml/src`
//! exists in the repository, the sources are parsed again here (a second implementation of the
//! tokenization, independent of `tools/gen-tables.mjs`) and compared with the checked-in tables.
//! The sRGB tables of `bevel.ts` are recomputed with `uo_jsnum::math::pow`, which also checks the
//! pow port against the Node capture the generator ran on. With `UO_TS_PARITY=1` the generator
//! itself is run again (`--no-srgb`, it needs Node) and its output is compared byte for byte
//! with the checked-in files.

use std::path::{Path, PathBuf};

use uo_jsnum::math::pow;
use uo_jsnum::parse::to_number;
use uo_jsnum::text::{split_js_space, trim};

use super::{adjust, light, patterns, presets, srgb, word_bevel};
use crate::num::f32_store;
use crate::preset::def::{Cmd, Guide, Op, PathDef, PresetDef};

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t')
}

fn ts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/drawingml/src")
}

/// The TS source, or `None` (with a note) when it is not in the checkout.
fn ts_source(name: &str) -> Option<String> {
    match std::fs::read_to_string(ts_dir().join(name)) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("{name} not found; the re-parse test needs the TS source");
            None
        }
    }
}

// ── a reader of the TS literals ─────────────────────────────────────────────────────────────────

/// Index just past the string literal that starts at `i` (a quote).
fn skip_string(b: &[u8], i: usize) -> usize {
    let quote = b[i];
    let mut j = i + 1;
    while b[j] != quote {
        j += if b[j] == b'\\' { 2 } else { 1 };
    }
    j + 1
}

/// Index just past the bracketed literal that starts at `i`.
fn skip_bracketed(b: &[u8], i: usize) -> usize {
    let mut depth = 0;
    let mut j = i;
    loop {
        match b[j] {
            b'\'' | b'"' | b'`' => j = skip_string(b, j),
            b'/' if b[j + 1] == b'/' => {
                while b[j] != b'\n' {
                    j += 1;
                }
            }
            b'{' | b'[' => {
                depth += 1;
                j += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                j += 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => j += 1,
        }
    }
}

/// The literal `{...}` or `[...]` of `const NAME ... = <literal>`.
fn literal<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src.find(&format!("const {name}")).unwrap_or_else(|| panic!("const {name} not found"));
    let b = src.as_bytes();
    let mut i = at + src[at..].find('=').unwrap() + 1;
    while is_ws(b[i]) {
        i += 1;
    }
    &src[i..skip_bracketed(b, i)]
}

/// The top-level items of a `{...}` (key, raw value) or `[...]` (empty key, raw item) literal.
fn items(lit: &str) -> Vec<(String, String)> {
    let b = lit.as_bytes();
    let object = b[0] == b'{';
    let mut out = Vec::new();
    let mut i = 1;
    loop {
        while i < b.len() && (is_ws(b[i]) || b[i] == b',') {
            i += 1;
        }
        if i + 1 >= b.len() {
            return out;
        }
        let mut key = String::new();
        if object {
            if b[i] == b'\'' || b[i] == b'"' {
                let end = skip_string(b, i);
                key = lit[i + 1..end - 1].to_owned();
                i = end;
            } else {
                let start = i;
                while b[i].is_ascii_alphanumeric() || b[i] == b'_' {
                    i += 1;
                }
                key = lit[start..i].to_owned();
            }
            while b[i] != b':' {
                i += 1;
            }
            i += 1;
            while is_ws(b[i]) {
                i += 1;
            }
        }
        let start = i;
        i = match b[i] {
            b'\'' | b'"' => skip_string(b, i),
            b'[' | b'{' => skip_bracketed(b, i),
            _ => {
                while b[i] != b',' && b[i] != b'}' && b[i] != b']' {
                    i += 1;
                }
                i
            }
        };
        out.push((key, lit[start..i].to_owned()));
    }
}

/// The text of a single-quoted TS string (no escapes in these sources).
fn unquote(raw: &str) -> &str {
    let s = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).unwrap_or_else(|| panic!("not a string: {raw}"));
    assert!(!s.contains('\\'), "escape in {s}");
    s
}

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64"),
        }
    };
    let b = s.as_bytes();
    assert_eq!(b.len() % 4, 0);
    let mut out = Vec::new();
    for chunk in b.chunks(4) {
        let pad = chunk.iter().filter(|c| **c == b'=').count();
        let n = chunk.iter().take(4 - pad).fold(0u32, |acc, c| (acc << 6) | val(*c)) << (6 * pad);
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    out
}

fn words(s: &str) -> Vec<&str> {
    split_js_space(trim(s)).filter(|w| !w.is_empty()).collect()
}

// ── presets ─────────────────────────────────────────────────────────────────────────────────────

fn parse_guides(s: &str) -> Vec<(String, Vec<String>)> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(|e| {
            let i = e.find('=').unwrap();
            (trim(&e[..i]).to_owned(), words(&e[i + 1..]).iter().map(|w| (*w).to_owned()).collect())
        })
        .collect()
}

fn check_guide(got: &Guide, want: &(String, Vec<String>), what: &str) {
    assert_eq!(got.name, want.0, "{what}");
    assert_eq!(got.op, Op::parse(&want.1[0]), "{what} {}", want.0);
    for (k, arg) in got.args.iter().enumerate().take(3) {
        assert_eq!(*arg, want.1.get(k + 1).map_or("0", String::as_str), "{what} {} arg {k}", want.0);
    }
}

fn check_path(got: &PathDef, raw: &str, what: &str) {
    let (attrs, cmds) = raw.split_once('~').unwrap_or((raw, ""));
    let mut parts = attrs.split(',');
    let (w, h, fill, stroke) =
        (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    assert_eq!(got.w.map(f64::to_bits), (!w.is_empty()).then(|| to_number(w).to_bits()), "{what} w");
    assert_eq!(got.h.map(f64::to_bits), (!h.is_empty()).then(|| to_number(h).to_bits()), "{what} h");
    let want_fill = match fill {
        "" => Some(0.0),
        "none" => None,
        "darken" => Some(-0.4),
        "darkenLess" => Some(-0.2),
        "lighten" => Some(0.4),
        "lightenLess" => Some(0.2),
        other => panic!("fill {other}"),
    };
    assert_eq!(got.fill.map(f64::to_bits), want_fill.map(f64::to_bits), "{what} fill");
    assert_eq!(got.stroke, stroke != "false" && stroke != "0", "{what} stroke");
    let want_cmds: Vec<Vec<&str>> = if cmds.is_empty() { Vec::new() } else { cmds.split(';').map(words).collect() };
    assert_eq!(got.cmds.len(), want_cmds.len(), "{what} commands");
    for (g, w) in got.cmds.iter().zip(&want_cmds) {
        check_cmd(g, w, what);
    }
}

fn check_cmd(got: &Cmd, want: &[&str], what: &str) {
    let op = want.first().and_then(|t| (t.len() == 1 && "MLQCAZ".contains(t)).then(|| t.as_bytes()[0])).unwrap_or(0);
    assert_eq!(got.op, op, "{what}");
    assert_eq!(got.args, want.get(1..).unwrap_or(&[]), "{what}");
}

fn check_preset(got: &PresetDef, raw: &str, name: &str) {
    let mut sections = raw.split('|');
    let (av, gd, ps, rc) =
        (sections.next().unwrap_or(""), sections.next().unwrap_or(""), sections.next().unwrap_or(""), sections.next().unwrap_or(""));
    for (guides, text, what) in [(got.av, av, "av"), (got.gd, gd, "gd")] {
        let want = parse_guides(text);
        assert_eq!(guides.len(), want.len(), "{name} {what}");
        for (g, w) in guides.iter().zip(&want) {
            check_guide(g, w, &format!("{name} {what}"));
        }
    }
    let paths: Vec<&str> = if ps.is_empty() { Vec::new() } else { ps.split('#').collect() };
    assert_eq!(got.paths.len(), paths.len(), "{name} paths");
    for (g, w) in got.paths.iter().zip(&paths) {
        check_path(g, w, name);
    }
    let rect = words(rc);
    assert_eq!(got.rect.map(|r| r.to_vec()), (rect.len() == 4).then_some(rect), "{name} rect");
}

#[test]
fn presets_equal_the_ts_definitions() {
    let Some(src) = ts_source("preset-defs.ts") else { return };
    let entries = items(literal(&src, "PRESET_DEFS"));
    assert_eq!(entries.len(), 187);
    assert_eq!(presets::PRESETS.len(), entries.len());
    for ((name, raw), (got_name, got)) in entries.iter().zip(presets::PRESETS) {
        assert_eq!(name, got_name, "key order");
        check_preset(got, unquote(raw), name);
    }
    let mut seen = vec![false; presets::PRESETS.len()];
    for w in presets::PRESET_INDEX.windows(2) {
        assert!(presets::PRESETS[usize::from(w[0])].0 < presets::PRESETS[usize::from(w[1])].0, "the index is sorted by name");
    }
    for &i in presets::PRESET_INDEX {
        seen[usize::from(i)] = true;
    }
    assert!(seen.iter().all(|s| *s) && presets::PRESET_INDEX.len() == presets::PRESETS.len(), "the index is a permutation");
}

// ── light, bevel, patterns, adjust ──────────────────────────────────────────────────────────────

#[test]
fn light_tables_and_profiles_equal_the_ts() {
    let Some(src) = ts_source("light3d.ts") else { return };
    let tables = items(literal(&src, "TABLES"));
    assert_eq!(tables.len(), light::LIGHT_TABLES.len());
    for ((key, raw), (got_key, got)) in tables.iter().zip(light::LIGHT_TABLES) {
        assert_eq!(key, got_key);
        assert_eq!(base64(unquote(raw)), *got, "{key}");
        assert_eq!(got.len(), (24 * 11 + 1) * 4);
    }
    let profiles = items(literal(&src, "PROFILES"));
    assert_eq!(profiles.len(), light::PROFILES.len());
    for ((key, raw), (got_key, got)) in profiles.iter().zip(light::PROFILES) {
        assert_eq!(key, got_key);
        let want: Vec<u64> = items(raw).iter().map(|(_, v)| to_number(v).to_bits()).collect();
        assert_eq!(got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), want, "{key}");
    }
}

#[test]
fn word_bevel_equals_the_ts() {
    let Some(src) = ts_source("bevel-word.ts") else { return };
    let tables = items(literal(&src, "WORD_BEVEL"));
    assert_eq!(tables.len(), word_bevel::WORD_BEVEL.len());
    for ((key, raw), (got_key, got)) in tables.iter().zip(word_bevel::WORD_BEVEL) {
        assert_eq!(key, got_key);
        assert_eq!(base64(unquote(raw)), *got, "{key}");
        assert_eq!(got.len(), 24 * 25 * 2);
    }
}

#[test]
fn patterns_equal_the_ts() {
    let Some(src) = ts_source("patterns.ts") else { return };
    let pats = items(literal(&src, "PRESET_PATTERNS"));
    assert_eq!(pats.len(), patterns::PRESET_PATTERNS.len());
    for ((key, raw), (got_key, got)) in pats.iter().zip(patterns::PRESET_PATTERNS) {
        assert_eq!(key, got_key);
        let hex = unquote(raw);
        let want: Vec<u8> = (0..8).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
        assert_eq!(got.to_vec(), want, "{key}");
    }
}

fn lut(src: &str, name: &str) -> Vec<(u64, Vec<u8>)> {
    items(literal(src, name))
        .iter()
        .map(|(_, raw)| {
            let pair = items(raw);
            let knots = unquote(&pair[1].1).split(' ').map(|k| k.parse::<u8>().unwrap()).collect();
            (to_number(&pair[0].1).to_bits(), knots)
        })
        .collect()
}

#[test]
fn adjust_tables_equal_the_ts() {
    let Some(src) = ts_source("adjust.ts") else { return };
    for (name, got) in [("BRIGHT_LUT", adjust::BRIGHT_LUT), ("CONTRAST_LUT", adjust::CONTRAST_LUT)] {
        let want = lut(&src, name);
        assert_eq!(got.len(), want.len(), "{name}");
        for ((gp, gk), (wp, wk)) in got.iter().map(|(p, k)| (p.to_bits(), k.to_vec())).zip(want) {
            assert_eq!(gp, wp, "{name}");
            assert_eq!(gk, wk, "{name}");
            assert_eq!(gk.len(), 33);
        }
    }
    let temp: Vec<[u64; 3]> = items(literal(&src, "TEMP_TABLE"))
        .iter()
        .map(|(_, raw)| {
            let v = items(raw);
            [to_number(&v[0].1).to_bits(), to_number(&v[1].1).to_bits(), to_number(&v[2].1).to_bits()]
        })
        .collect();
    let got: Vec<[u64; 3]> = adjust::TEMP_TABLE.iter().map(|(k, r, b)| [k.to_bits(), r.to_bits(), b.to_bits()]).collect();
    assert_eq!(got, temp);
}

/// The loops of `bevel.ts`, with `uo_jsnum::math::pow` for `Math.pow`: the generator evaluated them
/// with Math.pow of Node 24.15.0 linux-x64 (glibc 2.39, non-FMA pow), so this is also a check of
/// the pow port on 512 values.
#[test]
fn srgb_tables_equal_the_loops_of_bevel_ts() {
    for v in 0..256u32 {
        let x = f64::from(v) / 255.0;
        let want = if x <= 0.04045 { x / 12.92 } else { pow((x + 0.055) / 1.055, 2.4) };
        let want = f32_store(want);
        assert_eq!(srgb::SRGB_LIN_BITS[v as usize], want.to_bits(), "SRGB_LIN[{v}]");
    }
    for i in 0..4096u32 {
        let x = f64::from(i) / 4095.0;
        let v = 255.0 * (if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * pow(x, 1.0 / 2.4) - 0.055 });
        let want = uo_jsnum::math::round(v);
        assert_eq!(f64::from(srgb::LIN_SRGB[i as usize]), want, "LIN_SRGB[{i}]");
    }
}

// ── the generator, run again ────────────────────────────────────────────────────────────────────

/// `UO_TS_PARITY=1` (and a checkout of main in `UO_TS_ROOT` with `shared/drawingml/dist`): the
/// generator runs again into a temporary directory and its files equal the checked-in ones.
#[test]
fn the_generator_reproduces_the_checked_in_tables() {
    if !uo_testkit::opt_in("UO_TS_PARITY", "re-run of tools/gen-tables.mjs") {
        return;
    }
    let root = std::env::var("UO_TS_ROOT").expect("UO_TS_ROOT");
    let out = std::env::temp_dir().join(format!("uo-drawingml-tables-{}", std::process::id()));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/gen-tables.mjs");
    let status =
        std::process::Command::new("node").arg(&script).args(["--ts-root", &root, "--no-srgb", "--out"]).arg(&out).status().expect("node");
    assert!(status.success());
    for file in ["presets.rs", "light.rs", "word_bevel.rs", "patterns.rs", "adjust.rs"] {
        let fresh = std::fs::read(out.join(file)).unwrap();
        let checked_in = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tables").join(file)).unwrap();
        assert!(fresh == checked_in, "{file} differs from the generator output");
    }
    std::fs::remove_dir_all(&out).unwrap();
}
