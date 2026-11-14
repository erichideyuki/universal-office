//! No-panic loops of uo-docx-layout frontier F2 (`uo_docx::word`, record "Wave 1 — F2", security
//! plan §7): seeded hostile inputs through every entry point — field codes (formulas with deep
//! nesting, unbalanced braces, huge pictures, lone surrogates), date pictures and instants out of
//! range, number formats of huge and fractional counters, numbering definitions with cycles and
//! level texts full of placeholders, style chains with cycles, property merges of random OOXML,
//! and V8's sort with NaN comparators. Every call returns (no panic, no unbounded work) and every
//! generated text stays within its cap.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::DocxLimits;
use uo_docx::js::{JsNumber, JsRecord};
use uo_docx::model::formatting::DocFormatting;
use uo_docx::model::props::{PPr, RPr};
use uo_docx::model::run::Run;
use uo_docx::read::DOCX;
use uo_docx::read::parts::{read_numbering_text, read_settings, read_style_sheet_text};
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{para_props_from_ppr, run_props_from_rpr};
use uo_docx::text::Utf16;
use uo_docx::word::{self, FormulaError, LocalDateTime, NumberingState, StyleResolver, TextCap};
use uo_opc::xml::parse;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
    }
    fn pick<'a, T>(&mut self, a: &'a [T]) -> &'a T {
        &a[self.below(a.len())]
    }
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#;

fn soup(rng: &mut Rng, alphabet: &[&str], max: usize) -> Vec<u16> {
    let n = rng.below(max);
    let mut out: Vec<u16> = Vec::new();
    for _ in 0..n {
        let piece = *rng.pick(alphabet);
        if piece == "SUR" {
            out.push(0xd800);
        } else {
            out.extend(piece.encode_utf16());
        }
    }
    out
}

/// Word's formula evaluator (WordCraft study, B25) over operator runs of 100,000 and 400,000 terms on a 1 MiB stack
/// (the stack of wasm32): a run of one precedence is one flat node, so neither the evaluation nor the drop of the
/// tree recurses per operator (a left-deep tree aborted the process at 10,000 terms).
#[test]
fn word_formula_operator_runs_on_a_small_stack() {
    let run = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let u = |s: String| -> Vec<u16> { s.encode_utf16().collect() };
            let mut cap = TextCap::new(4096);
            let mut none = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
            let pt = word::numwords::WordsLang::Pt;
            let mut eval = |code: String| -> Option<String> {
                word::evaluate_field_formula_word(&u(code), &mut none, pt, "pt-BR", 256, &mut cap)
                    .ok()
                    .flatten()
                    .map(|v| String::from_utf16_lossy(v.units()))
            };
            let sum = |n: usize, op: &str, t: &str| format!("= {t}{}", format!("{op}{t}").repeat(n - 1));
            assert_eq!(eval(sum(100_000, "+", "1")).as_deref(), Some("100000"));
            assert_eq!(eval(sum(400_000, "+", "1")).as_deref(), Some("400000"));
            assert_eq!(eval(sum(100_000, "*", "1")).as_deref(), Some("1"));
            assert_eq!(eval(sum(100_000, "^", "1")).as_deref(), Some("1"));
            assert_eq!(eval(sum(100_000, "-", "1")).as_deref(), Some("-99998"));
            let _ = eval(format!("= 1{}", "%".repeat(100_000)));
            let _ = eval(format!("= {}", "1+2*3^4%-".repeat(50_000) + "1"));
            let _ = eval(format!("= SUM({})", "1;".repeat(100_000) + "1"));
            let _ = eval(format!("= ({}) = 1", "(1+1)+".repeat(100_000) + "1"));
        })
        .unwrap();
    run.join().unwrap();
}

#[test]
fn hostile_field_codes() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let alphabet = [
        "=",
        " ",
        "{",
        "}",
        "(",
        ")",
        "-",
        "+",
        "*",
        "/",
        "0",
        "1",
        "9",
        ",",
        ".",
        "\\",
        "#",
        "\\*",
        "\"",
        "a",
        " PAGE ",
        " NUMPAGES ",
        "ROMAN",
        "SUR",
        "1.000",
        ";",
        "'",
        "x",
        "\t",
    ];
    let answers = ["12", "999999999999999999999", "1.000.000,5", "-", "", "1e3", "-0", "NaN", "3,14159265358979323846264338327950288"];
    let mut cap = TextCap::new(4096);
    for _ in 0..20_000 {
        let code = soup(&mut rng, &alphabet, 120);
        let pick = rng.below(answers.len());
        let mut value = |_: &str, _: &[u16]| -> Option<Vec<u16>> { (pick != 0).then(|| answers[pick].encode_utf16().collect()) };
        if let Ok(Some(v)) = word::evaluate_field_formula(&code, &mut value, Some("pt-BR"), 256, &mut cap) {
            assert!(v.len() <= 4096);
        }
        let _ = word::field_name(&code);
        let _ = word::has_nested_field(Some(&String::from_utf16_lossy(&code)));
    }
    let mut none = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
    let u = |s: String| -> Vec<u16> { s.encode_utf16().collect() };
    // nesting the TS overflows its stack with: a limit here (K11c)
    for deep in [
        format!("={}1{}", "(".repeat(100_000), ")".repeat(100_000)),
        format!("={}1", "-".repeat(1_000_000)),
        format!("={}1", "+-".repeat(300_000)),
    ] {
        assert_eq!(word::evaluate_field_formula(&u(deep), &mut none, None, 256, &mut cap), Err(FormulaError::Depth));
    }
    // 10^5 nested fields: the TS guard (200 replacements) gives "cannot compute"
    let nested = format!("={}{}", "{ = ".repeat(1000), "}".repeat(1000));
    assert_eq!(word::evaluate_field_formula(&u(nested), &mut none, None, 256, &mut cap), Ok(None));
    // pictures as long as the instruction: capped results
    let pic = format!("=1234567 \\# \"{}\"", "0".repeat(200_000));
    let v = word::evaluate_field_formula(&u(pic), &mut none, None, 256, &mut cap).unwrap().unwrap();
    assert_eq!(v.len(), 4096);
    assert!(cap.take_truncated());
    let decimals = format!("=1 \\# \"0,{}\"", "0".repeat(500));
    assert_eq!(word::evaluate_field_formula(&u(decimals), &mut none, None, 256, &mut cap), Ok(None));
    // Word's evaluator (WordCraft study, B25) over the same soup
    for _ in 0..20_000 {
        let code = soup(&mut rng, &alphabet, 120);
        let mut value = |_: &str, _: &[u16]| -> Option<Vec<u16>> { Some("1,5".encode_utf16().collect()) };
        let words = word::numwords::WordsLang::of(rng.pick(&["pt-BR", "en-US", "es-ES"]));
        if let Ok(Some(v)) = word::evaluate_field_formula_word(&code, &mut value, words, "pt-BR", 256, &mut cap) {
            assert!(v.len() <= 4096);
        }
    }
    for v in [f64::NAN, f64::INFINITY, -1e308, 1e21, 0.5, -0.0] {
        let _ = word::format_picture(v, "#.##0,00x;(0);'z'");
        let _ = word::format_formula_result(
            v,
            &word::CellNumber { n: v, decimals: 1e6, thousands: true, prefix: Some("R$ ".into()), suffix: Some("%".into()) },
        );
    }
}

#[test]
fn hostile_dates_and_numbers() {
    let mut rng = Rng(42);
    let alphabet = [
        "d", "D", "M", "y", "Y", "H", "h", "m", "s", "S", "'", "am/pm", "AM/PM", " ", "\\@", "\"", "\\*", "Upper", "caps", "FIRSTCAP", "x",
        "SUR", "ß", "😀",
    ];
    let instants = [0.0, -1.0, 8.64e15, -8.64e15, 8.64e15 + 1.0, f64::NAN, f64::INFINITY, 1e300, -62_198_755_200_000.0, 1.5];
    let mut cap = TextCap::new(2000);
    for _ in 0..20_000 {
        let s = soup(&mut rng, &alphabet, 200);
        let t = *rng.pick(&instants);
        let off = *rng.pick(&[0.0, -10_800_000.0, 50_400_000.0, f64::NAN]);
        let d = LocalDateTime::at(t, off);
        let _ = word::format_word_date(&d, &s, rng.pick(&["pt-BR", "en-US", "EN", ""]));
        let v = word::date_field_value(&s, &d, "pt-BR", &mut cap);
        assert!(v.len() <= 2000);
    }
    let long = "dddd ".repeat(100_000).encode_utf16().collect::<Vec<u16>>();
    let mut instr: Vec<u16> = " DATE \\@ \"".encode_utf16().collect();
    instr.extend(&long);
    instr.push(u16::from(b'"'));
    let v = word::date_field_value(&instr, &LocalDateTime::at(0.0, 0.0), "pt-BR", &mut cap);
    assert_eq!(v.len(), 2000);
    assert!(cap.take_truncated());
    let fmts = [
        "decimal",
        "decimalZero",
        "upperRoman",
        "lowerRoman",
        "upperLetter",
        "lowerLetter",
        "ordinal",
        "cardinalText",
        "ordinalText",
        "hex",
        "decimalEnclosedCircle",
        "decimalEnclosedParen",
        "decimalEnclosedFullstop",
        "numberInDash",
        "chicago",
        "none",
        "bullet",
        "x",
    ];
    let ns = [
        0.0,
        -0.0,
        1.0,
        2.5,
        -3.0,
        20.5,
        1e15,
        1e300,
        -1e300,
        9_007_199_254_740_993.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
    ];
    let mut small = TextCap::new(1000);
    for f in fmts {
        for n in ns {
            for lang in ["pt-BR", "en-US"] {
                let v = word::format_number(n, Some(f), lang, &mut small);
                assert!(v.len() <= 1000, "{f} {n}");
            }
        }
    }
}

fn hostile_formatting(rng: &mut Rng) -> DocFormatting {
    // numStyleLink cycles, overrides of absent levels, level texts full of placeholders
    let lvl_texts = ["%1", "%1%2%3%4%5%6%7%8%9", "%%%%", "%9%9%9", ""];
    let mut abs = String::new();
    for a in 0..4 {
        let mut lv = String::new();
        for l in 0..9 {
            let text = rng.pick(&lvl_texts).repeat(1 + rng.below(if a == 0 { 600 } else { 60 }));
            lv.push_str(&format!(
                r#"<w:lvl w:ilvl="{l}"><w:start w:val="{}"/><w:numFmt w:val="{}"/><w:lvlRestart w:val="{}"/><w:lvlText w:val="{text}"/></w:lvl>"#,
                rng.pick(&["1", "1e15", "-5", "x", "9007199254740993"]),
                rng.pick(&["upperRoman", "lowerLetter", "chicago", "decimal", "bullet", "cardinalText"]),
                rng.pick(&["0", "1", "9", "-1"]),
            ));
        }
        let link = if rng.below(2) == 0 { r#"<w:numStyleLink w:val="NS"/>"# } else { "" };
        abs.push_str(&format!(r#"<w:abstractNum w:abstractNumId="{a}">{link}{lv}</w:abstractNum>"#));
    }
    let mut nums = String::new();
    for n in 1..6 {
        nums.push_str(&format!(
            r#"<w:num w:numId="{n}"><w:abstractNumId w:val="{}"/><w:lvlOverride w:ilvl="{}"><w:startOverride w:val="{}"/></w:lvlOverride></w:num>"#,
            rng.below(5),
            rng.below(10),
            rng.pick(&["3", "x", "1e300"])
        ));
    }
    let numbering = format!(r#"<w:numbering {W}>{abs}{nums}</w:numbering>"#);
    // basedOn cycles and a long chain
    let mut styles = String::from(
        r#"<w:style w:type="numbering" w:styleId="NS"><w:name w:val="ns"/><w:pPr><w:numPr><w:numId w:val="3"/></w:numPr></w:pPr></w:style>"#,
    );
    for i in 0..300 {
        styles.push_str(&format!(
            r#"<w:style w:type="{}" w:styleId="S{i}"><w:name w:val="heading {}"/><w:basedOn w:val="S{}"/><w:pPr><w:numPr><w:numId w:val="{}"/></w:numPr><w:ind w:left="1"/></w:pPr><w:rPr><w:b/></w:rPr><w:tblStylePr w:type="firstRow"><w:rPr><w:i/></w:rPr></w:tblStylePr></w:style>"#,
            rng.pick(&["paragraph", "paragraph", "character", "table"]),
            rng.below(12),
            (i + 1) % 300,
            rng.below(7)
        ));
    }
    DocFormatting {
        styles: read_style_sheet_text(&format!(r#"<w:styles {W}>{styles}</w:styles>"#)).unwrap(),
        numbering: read_numbering_text(&numbering, &DocxLimits::default()).unwrap(),
        settings: read_settings(None),
        font_table: JsRecord::new(),
        ..DocFormatting::default()
    }
}

#[test]
fn hostile_numbering_and_styles() {
    let mut rng = Rng(7);
    for _ in 0..6 {
        let fmt = hostile_formatting(&mut rng);
        let mut ns = NumberingState::new("pt-BR", TextCap::new(5000));
        let mut sr = StyleResolver::new();
        for _ in 0..200 {
            let num = JsNumber::Num(f64::from(u32::try_from(rng.below(7)).unwrap()));
            let ilvl = *rng.pick(&[
                JsNumber::Num(0.0),
                JsNumber::Num(4.0),
                JsNumber::Num(8.0),
                JsNumber::Num(f64::NAN),
                JsNumber::Null,
                JsNumber::Num(-3.0),
            ]);
            if let Some(m) = ns.next(&fmt, num, ilvl) {
                // a bullet's text is its level text as written (bounded by the document, not generated)
                let bullet = m.level_in(&fmt).is_some_and(|l| l.num_fmt.as_deref() == Some("bullet"));
                assert!(bullet || m.text.len() <= 5000);
                assert!(m.chapter.as_ref().is_none_or(|c| c.len() <= 5000));
            }
            let id = format!("S{}", rng.below(310));
            let ps = sr.para_style(&fmt, Some(&id));
            let _ = sr.table_style(&fmt, Some(&id));
            let _ = sr.char_style(&fmt, Some(&id));
            assert!(StyleResolver::para_chain(&fmt, Some(&id)).len() <= 32);
            let props = para_props_from_ppr(Some(&ps.p_pr));
            let _ = word::resolve_ppr(&fmt, &mut sr, Some(&ps.p_pr), &props, None);
        }
    }
}

#[test]
fn hostile_merges_and_sort() {
    let mut rng = Rng(99);
    let kids = [
        r#"<w:b w:val="x"/>"#,
        r#"<w:rFonts w:ascii="" w:asciiTheme="majorBidi"/>"#,
        r#"<w:sz w:val="1e400"/>"#,
        r#"<w:color w:val="zz" w:themeColor="constructor" w:themeTint="NaN"/>"#,
        r#"<w:shd w:val="pct999999999999999999999" w:fill="ß" w:color="😀"/>"#,
        r#"<w:lang w:val=""/>"#,
        r#"<w:rPrChange w:author=""><w:rPr><w:rPrChange w:author="x"><w:rPr><w:caps/></w:rPr></w:rPrChange></w:rPr></w:rPrChange>"#,
        r#"<w:u w:val="" w:color="auto"/>"#,
        r#"<w:highlight w:val="__proto__"/>"#,
    ];
    let pkids = [
        r#"<w:tabs><w:tab w:val="clear" w:pos="x"/><w:tab w:pos="-0"/><w:tab w:pos="NaN"/></w:tabs>"#,
        r#"<w:ind w:hanging="1e309" w:firstLineChars="-1"/>"#,
        r#"<w:numPr><w:numId w:val="x"/><w:ilvl w:val="99999999999"/></w:numPr>"#,
        r#"<w:spacing w:line="-0" w:lineRule=""/>"#,
        r#"<w:outlineLvl w:val="-7"/>"#,
        r#"<w:pBdr><w:bar w:val="" w:sz="NaN"/></w:pBdr>"#,
    ];
    let theme = DocFormatting::default().theme;
    for _ in 0..3000 {
        let a: String = (0..rng.below(6)).map(|_| *rng.pick(&kids)).collect();
        let b: String = (0..rng.below(6)).map(|_| *rng.pick(&kids)).collect();
        let ra: Option<RPr> = read_rpr(Some(parse(format!("<w:rPr {W}>{a}</w:rPr>"), &DOCX).unwrap().root()));
        let rb: Option<RPr> = read_rpr(Some(parse(format!("<w:rPr {W}>{b}</w:rPr>"), &DOCX).unwrap().root()));
        let m = word::merge_rpr(ra.as_ref(), rb.as_ref());
        let c = word::cascade_rpr(ra.as_ref(), &[rb.as_ref(), Some(&m)], ra.as_ref());
        let _ = word::shading_fill(c.shd.as_deref(), &theme);
        let _ = word::resolve_color(c.color.as_deref().map(word::ColorRef::of_color), &theme);
        let props = run_props_from_rpr(ra.as_ref());
        let run = Run { text: Utf16::from("x"), props: Arc::new(props), rpr: ra.map(Arc::new), ..Run::default() };
        let _ = word::direct_rpr(&run);
        let pa: String = (0..rng.below(6)).map(|_| *rng.pick(&pkids)).collect();
        let pp: Option<PPr> = read_ppr(Some(parse(format!("<w:pPr {W}>{pa}</w:pPr>"), &DOCX).unwrap().root()));
        let m2 = word::merge_ppr(pp.as_ref(), pp.as_ref());
        let _ = word::direct_ppr(Some(&m2), &para_props_from_ppr(pp.as_ref()));
    }
    for n in [0usize, 1, 2, 63, 64, 65, 200] {
        let mut v: Vec<(f64, usize)> = (0..n).map(|i| (*rng.pick(&[f64::NAN, 1.0, -1.0, 0.0, -0.0, f64::INFINITY]), i)).collect();
        word::obj::js_sort(&mut v, |a, b| a.0 - b.0);
        let mut ids: Vec<usize> = v.iter().map(|x| x.1).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..n).collect::<Vec<_>>());
    }
}
