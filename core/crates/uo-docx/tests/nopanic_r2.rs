//! No-panic loops of frontier R2: every public reader of `read::parts`, `read::styles`,
//! `read::table_styles` and `read::style_names_es` over the synthetic inputs of
//! `js/quirk-parts.mjs`, seeded mutations of them (a character replaced, removed, duplicated; a
//! junk string inserted; bytes cut), random strings for the string functions, and the shapes
//! that stress the iterative walks (a `basedOn` chain past the limit, a deeply nested theme list,
//! many styles). A panic or a stack overflow fails the test; the result is not looked at (the
//! parity tests look at it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsRecord, Str};
use uo_docx::limits::DocxLimits;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::{
    numbering_images, parse_text, read_doc_formatting, read_font_table_text, read_numbering_text, read_settings_text,
    read_style_sheet_text, read_theme_text, target_ext, vml_len_pt,
};
use uo_docx::read::style_names_es::{family_replace, table_style_name_es};
use uo_docx::read::styles::{read_styles, read_styles_text};
use uo_docx::read::table_styles::{materialize_style_xml, style_def_of, theme_hex};
use uo_opc::Package;

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

const JUNK: [&str; 20] = [
    "",
    "-",
    ".",
    "NaN",
    "1e999",
    "99999999999999999999999999",
    "\u{0}",
    "%",
    "12pt%",
    "ÿ",
    "𝒜",
    " ",
    "0x",
    "-0x1",
    "1e",
    "Infinity",
    "\"",
    "<",
    "&#x110000;",
    "~",
];

type Parts = Vec<(String, String)>;

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn inputs() -> Vec<Parts> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/parts.inputs")).unwrap();
    let mut out: Vec<Parts> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            out.push(Vec::new());
        } else if let Some((path, data)) = line.split_once('\t')
            && !data.starts_with("hex:")
        {
            out.last_mut().unwrap().push((path.to_owned(), unescape(data)));
        }
    }
    out
}

fn mutate(rng: &mut Rng, s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = chars.clone();
    for _ in 0..1 + rng.below(3) {
        let at = rng.below(out.len().max(1));
        match rng.below(5) {
            0 if at < out.len() => out[at] = char::from_u32(rng.next() % 0x2fff + 1).unwrap_or('x'),
            1 if at < out.len() => {
                out.remove(at);
            }
            2 if at < out.len() => {
                let c = out[at];
                out.insert(at, c);
            }
            3 => {
                let j: Vec<char> = JUNK[rng.below(JUNK.len())].chars().collect();
                for (k, c) in j.into_iter().enumerate() {
                    out.insert((at + k).min(out.len()), c);
                }
            }
            _ => out.truncate(at),
        }
    }
    out.into_iter().collect()
}

fn read_all_texts(parts: &Parts, limits: &DocxLimits) -> usize {
    let mut n = 0;
    for (path, text) in parts {
        match path.as_str() {
            "word/styles.xml" => {
                n += usize::from(read_style_sheet_text(text).is_ok());
                n += usize::from(read_styles_text(text, limits).is_ok());
            }
            "word/numbering.xml" => n += usize::from(read_numbering_text(text, limits).is_ok()),
            "word/theme/theme1.xml" => n += usize::from(read_theme_text(text).is_ok()),
            "word/settings.xml" => n += usize::from(read_settings_text(text).is_ok()),
            "word/fontTable.xml" => n += usize::from(read_font_table_text(text).is_ok()),
            _ => {}
        }
    }
    n
}

fn read_package(parts: &Parts) -> usize {
    let pkg = Package::from_parts(
        parts.iter().map(|(p, t)| (p.clone(), t.clone().into_bytes())),
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    );
    let opts = ReadOptions::default();
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let mut n = usize::from(read_doc_formatting(&mut ctx).is_ok());
    n += usize::from(read_styles(&ctx).is_ok());
    n += usize::from(numbering_images(&ctx, "word/numbering.xml").is_ok());
    n
}

#[test]
fn inputs_and_mutations_do_not_panic() {
    let all = inputs();
    let limits = DocxLimits::DEFAULT;
    let mut rng = Rng(0x00c0_ffee);
    for parts in &all {
        read_all_texts(parts, &limits);
        read_package(parts);
    }
    for round in 0..4 {
        for parts in &all {
            let mutated: Parts =
                parts.iter().map(|(p, t)| (p.clone(), if rng.below(2) == 0 { mutate(&mut rng, t) } else { t.clone() })).collect();
            read_all_texts(&mutated, &limits);
            read_package(&mutated);
        }
        let _ = round;
    }
}

#[test]
fn strings_do_not_panic() {
    let mut rng = Rng(0x0bad_cafe);
    let colors: JsRecord<Str> = {
        let mut r = JsRecord::new();
        for (k, v) in [("accent1", "4472C4"), ("dk1", "xyz"), ("lt1", ""), ("accent2", "é😀é")] {
            r.insert(k, v.into());
        }
        r
    };
    let alphabet: Vec<char> = "ab ;:.-0123456789wdthigptincmpxAccent~\"/>\n\u{2028}é😀=<{ID}{A}".chars().collect();
    for _ in 0..4000 {
        let len = rng.below(40);
        let s: String = (0..len).map(|_| alphabet[rng.below(alphabet.len())]).collect();
        let _ = vml_len_pt(&s, "width");
        let _ = vml_len_pt(&s, "height");
        let _ = target_ext(&s);
        let _ = table_style_name_es(&s);
        let _ = table_style_name_es(&format!("{s} Accent 1"));
        for r in 0..29 {
            let _ = family_replace(r, &s);
        }
        let _ = materialize_style_xml(&format!("w:color=\"~\"{s}"), &colors);
        let _ = materialize_style_xml(&format!("w:fill=\"~\" w:themeFill=\"{s}\" w:themeFillTint=\"{s}\" {s}"), &colors);
        let _ = theme_hex(&colors, &s, Some(&s), Some(&s));
        let _ = theme_hex(&colors, "accent1", Some(&s), None);
        let _ = theme_hex(&colors, "dk1", None, Some(&s));
        let _ = theme_hex(&colors, "lt1", Some("80"), Some("80"));
        let _ = theme_hex(&colors, "accent2", Some("80"), Some("80"));
        let _ = style_def_of(&s);
        let _ = parse_text(&s);
    }
}

#[test]
fn shapes_that_stress_the_walks_do_not_panic() {
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let a = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
    let limits = DocxLimits::DEFAULT;
    // a basedOn chain past the limit, a cycle, a long run of styles
    let n = usize::try_from(limits.max_based_on_chain).unwrap() + 50;
    let chain: String =
        (0..n).map(|i| format!("<w:style w:type=\"paragraph\" w:styleId=\"S{i}\"><w:basedOn w:val=\"S{}\"/></w:style>", i + 1)).collect();
    assert!(read_styles_text(&format!("<w:styles {w}>{chain}</w:styles>"), &limits).is_err());
    let cycle: String = (0..200)
        .map(|i| format!("<w:style w:type=\"paragraph\" w:styleId=\"S{i}\"><w:basedOn w:val=\"S{}\"/></w:style>", (i + 1) % 200))
        .collect();
    assert_eq!(read_styles_text(&format!("<w:styles {w}>{cycle}</w:styles>"), &limits).unwrap().len(), 201);
    let many: String =
        (0..30_000).map(|i| format!("<w:style w:type=\"paragraph\" w:styleId=\"M{i}\"><w:rPr><w:b/></w:rPr></w:style>")).collect();
    assert_eq!(read_styles_text(&format!("<w:styles {w}>{many}</w:styles>"), &limits).unwrap().len(), 30_001);
    assert_eq!(read_style_sheet_text(&format!("<w:styles {w}>{many}</w:styles>")).unwrap().styles.len(), 30_000);
    // a theme whose style lists nest 900 levels deep and hold many entries
    let deep_open: String = "<a:x>".repeat(900);
    let deep_close: String = "</a:x>".repeat(900);
    let theme = format!(
        "<a:theme {a}><a:fmtScheme><a:fillStyleLst><a:gradFill><a:gsLst>{deep_open}{deep_close}</a:gsLst></a:gradFill>{}</a:fillStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst>{deep_open}{deep_close}</a:effectLst></a:effectStyle></a:effectStyleLst></a:fmtScheme></a:theme>",
        "<a:solidFill/>".repeat(2000)
    );
    let t = read_theme_text(&theme).unwrap();
    assert_eq!(t.fill_styles.unwrap().len(), 2001);
    // levels, overrides, fonts and pic bullets in numbers
    let lvls: String = (0..8).map(|i| format!("<w:lvl w:ilvl=\"{i}\"/>")).collect();
    let nums: String = (0..5000).map(|i| format!("<w:num w:numId=\"{i}\"><w:lvlOverride w:ilvl=\"1\"/></w:num>")).collect();
    let numbering = format!("<w:numbering {w}><w:abstractNum w:abstractNumId=\"1\">{lvls}</w:abstractNum>{nums}</w:numbering>");
    assert_eq!(read_numbering_text(&numbering, &limits).unwrap().nums.len(), 5000);
    let fonts: String = (0..5000).map(|i| format!("<w:font w:name=\"F{i}\"><w:sig w:usb0=\"1\"/></w:font>")).collect();
    assert_eq!(read_font_table_text(&format!("<w:fonts {w}>{fonts}</w:fonts>")).unwrap().len(), 5000);
}
