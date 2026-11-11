//! Parity of D2b-F10's pure model files: replays `tests/parity/d2b-f10-ops.capture`, written by
//! `js/capture-d2b-f10-ops.mjs` from the frozen editor's `references.ts`, `toc.ts`,
//! `text-layout.ts` and `hf-templates.ts` (+ docx-layout's `formatWordDate`): the TOC instruction
//! parser and writer, the dialog options, cross-reference and caption instructions, the TC fields,
//! the formats of the `toc N` styles, Add Text, the Manual Hyphenation helpers, the hidden bookmark
//! names, the entry paragraphs (canonical digest), `dateTimeFormats` at pinned instants for the
//! three languages, and `runsBetween` over corpus paragraphs.
//!
//! Recapture: `UO_TS_ROOT=<built checkout> node --experimental-transform-types --no-warnings
//! core/crates/uo-docx/js/capture-d2b-f10-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::ids::IdAlloc;
use uo_docx::model::para::Paragraph;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::env::{DateParts, LocalNow};
use uo_docx::session::ops::references::datetime::date_time_samples;
use uo_docx::session::ops::references::hyphen::{manual_points, offset_after_letters, word_around};
use uo_docx::session::ops::references::pure::{
    CaptionChapter, CaptionFormat, CaptionNumbering, ChapterSeparator, CrossRefKind, CrossRefType, TocLook, TocOptions, TocSource,
    TocStyle, caption_fields, caption_range, chapter_separator_text, cross_ref_instr, cross_ref_kinds, hidden_bookmark_name,
    parse_toc_instr, ref_field_target, runs_between, seq_label, toc_entry_paragraphs, toc_instr,
};
use uo_docx::session::ops::references::toc::{
    AddTextKind, AddTextPlan, TOC_TEMPLATES, TableOfContentsOptions, TocFormat, add_text_plan, dialog_options_from, manual_placeholder,
    parse_tc_instr, tc_in_toc, tc_instr, toc_format_style, toc_options_from, toc_sdt_pr,
};
use uo_docx::session::ops::styles::UiLang;
use uo_jsnum::format::to_js_string;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, Stream, repo_root};

const KINDS: [CrossRefKind; 11] = [
    CrossRefKind::Text,
    CrossRefKind::Page,
    CrossRefKind::Number,
    CrossRefKind::NumberNoContext,
    CrossRefKind::NumberFullContext,
    CrossRefKind::Position,
    CrossRefKind::NoteNumber,
    CrossRefKind::NoteNumberFormatted,
    CrossRefKind::CaptionEntire,
    CrossRefKind::CaptionLabel,
    CrossRefKind::CaptionText,
];
const KIND_NAMES: [&str; 11] = [
    "text",
    "page",
    "number",
    "numberNoContext",
    "numberFullContext",
    "position",
    "noteNumber",
    "noteNumberFormatted",
    "captionEntire",
    "captionLabel",
    "captionText",
];
const TYPES: [CrossRefType; 6] = [
    CrossRefType::Numbered,
    CrossRefType::Heading,
    CrossRefType::Bookmark,
    CrossRefType::Footnote,
    CrossRefType::Endnote,
    CrossRefType::Caption,
];
const FORMATS: [CaptionFormat; 5] = [
    CaptionFormat::Arabic,
    CaptionFormat::AlphabeticUpper,
    CaptionFormat::AlphabeticLower,
    CaptionFormat::RomanUpper,
    CaptionFormat::RomanLower,
];
const SEPS: [ChapterSeparator; 5] =
    [ChapterSeparator::Hyphen, ChapterSeparator::Period, ChapterSeparator::Colon, ChapterSeparator::EmDash, ChapterSeparator::EnDash];
const TOC_FORMATS: [TocFormat; 7] = [
    TocFormat::Template,
    TocFormat::Classic,
    TocFormat::Distinctive,
    TocFormat::Fancy,
    TocFormat::Modern,
    TocFormat::Formal,
    TocFormat::Simple,
];

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

fn units(h: &str) -> Vec<u16> {
    h.split('.').filter(|x| !x.is_empty()).map(|x| u16::from_str_radix(x, 16).unwrap()).collect()
}

fn unhex(h: &str) -> String {
    String::from_utf16_lossy(&units(h))
}

fn n(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

fn pair(o: (f64, f64)) -> String {
    format!("{},{}", to_js_string(o.0), to_js_string(o.1))
}

fn canon_opts(o: &TocOptions) -> String {
    let mut p: Vec<String> = Vec::new();
    if let Some(x) = o.outline {
        p.push(format!("o={}", pair(x)));
    }
    if o.outline_levels {
        p.push("u".into());
    }
    if !o.styles.is_empty() {
        p.push(format!(
            "t={}",
            o.styles.iter().map(|s| format!("{}:{}", hs(&s.style), to_js_string(s.level))).collect::<Vec<_>>().join(",")
        ));
    }
    if let Some(c) = &o.caption {
        p.push(format!("c={}", hs(c)));
    }
    if let Some(b) = o.include_label {
        p.push(format!("il={}", u8::from(b)));
    }
    if o.fields {
        p.push("f".into());
    }
    if let Some(id) = &o.field_id {
        p.push(format!("fid={}", hs(id)));
    }
    if let Some(x) = o.field_levels {
        p.push(format!("l={}", pair(x)));
    }
    if o.hyperlinks {
        p.push("h".into());
    }
    if o.no_page_numbers {
        p.push("n".into());
    }
    if let Some(x) = o.no_page_levels {
        p.push(format!("nl={}", pair(x)));
    }
    if let Some(s) = &o.separator {
        p.push(format!("p={}", hs(s)));
    }
    if o.hide_in_web {
        p.push("z".into());
    }
    if p.is_empty() { "-".into() } else { p.join(";") }
}

fn canon_dialog(d: &TableOfContentsOptions) -> String {
    let b = |k: &str, v: Option<bool>| v.map(|v| format!("{k}={}", u8::from(v)));
    let mut p: Vec<Option<String>> = vec![b("pn", d.page_numbers), b("ra", d.right_align), b("hy", d.hyperlinks)];
    p.push(d.levels.map(|v| format!("lv={}", to_js_string(v))));
    p.push(b("us", d.use_styles));
    p.push(
        d.styles.as_ref().map(|s| {
            format!("st={}", s.iter().map(|x| format!("{}:{}", hs(&x.style), to_js_string(x.level))).collect::<Vec<_>>().join(","))
        }),
    );
    p.push(b("ol", d.outline_levels));
    p.push(b("fl", d.fields));
    p.push(d.field_id.as_ref().map(|s| format!("fi={}", hs(s))));
    let v: Vec<String> = p.into_iter().flatten().collect();
    if v.is_empty() { "-".into() } else { v.join(";") }
}

fn parse_dialog(text: &str) -> TableOfContentsOptions {
    let mut d = TableOfContentsOptions::default();
    if text == "-" {
        return d;
    }
    for part in text.split(';') {
        let (k, v) = part.split_once('=').unwrap();
        let bo = || Some(v == "1");
        match k {
            "pn" => d.page_numbers = bo(),
            "ra" => d.right_align = bo(),
            "hy" => d.hyperlinks = bo(),
            "lv" => d.levels = Some(n(v)),
            "us" => d.use_styles = bo(),
            "st" => {
                d.styles = Some(
                    v.split(',')
                        .filter(|x| !x.is_empty())
                        .map(|x| {
                            let (s, l) = x.split_once(':').unwrap();
                            TocStyle { style: unhex(s), level: n(l) }
                        })
                        .collect(),
                );
            }
            "ol" => d.outline_levels = bo(),
            "fl" => d.fields = bo(),
            "fi" => d.field_id = Some(unhex(v)),
            _ => panic!("dialog key {k}"),
        }
    }
    d
}

fn digest(paras: &[Paragraph]) -> String {
    use uo_docx::js::JsValue;
    let mut s = Sink(Stream::new());
    let media = uo_docx::model::media::MediaStore::default();
    for (i, p) in paras.iter().enumerate() {
        s.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut s).with_media(&media);
        p.canon(&mut w);
    }
    s.0.finish().short()
}

fn run(code: &str) -> String {
    let (name, rest) = code.split_once(':').unwrap();
    let a: Vec<&str> = rest.split(':').collect();
    match name {
        "pi" => {
            let o = parse_toc_instr(&unhex(a[0]));
            format!("{}|{}", canon_opts(&o), hs(&toc_instr(&o)))
        }
        "to" => {
            let d = parse_dialog(rest);
            let o = toc_options_from(&d);
            format!("{}|{}|{}", canon_opts(&o), hs(&toc_instr(&o)), canon_dialog(&dialog_options_from(&o)))
        }
        "ck" => cross_ref_kinds(TYPES[a[0].parse::<usize>().unwrap()])
            .iter()
            .map(|k| KIND_NAMES[KINDS.iter().position(|x| x == k).unwrap()])
            .collect::<Vec<_>>()
            .join(","),
        "ri" => {
            let hyper = [None, Some(true), Some(false)][a[2].parse::<usize>().unwrap()];
            let sep = (a[3] != "~").then(|| unhex(a[3]));
            hs(&cross_ref_instr(&unhex(a[0]), KINDS[a[1].parse::<usize>().unwrap()], hyper, sep.as_deref()))
        }
        "rt" => hs(&ref_field_target(Some(&unhex(a[0])))),
        "sl" => hs(&seq_label(Some(&unhex(a[0])))),
        "pc" => match parse_tc_instr(Some(&unhex(a[0]))) {
            Some(t) => format!("{}|{}|{}|{}", hs(&t.text), hs(&t.id), to_js_string(t.level), u8::from(t.no_page)),
            None => "~".into(),
        },
        "tc" => {
            let id = (a[1] != "~").then(|| unhex(a[1]));
            let lv = (a[2] != "~").then(|| n(a[2]));
            hs(&tc_instr(&unhex(a[0]), id.as_deref(), lv))
        }
        "ti" => {
            let o = parse_toc_instr(&unhex(a[0]));
            let tc = parse_tc_instr(Some(&unhex(a[1]))).unwrap();
            tc_in_toc(&o, &tc).to_string()
        }
        "cf" => {
            let chapter = (a[2] != "~").then(|| CaptionChapter { level: n(a[2]), separator: SEPS[a[3].parse::<usize>().unwrap()] });
            let r = caption_fields(&unhex(a[0]), &CaptionNumbering { format: FORMATS[a[1].parse::<usize>().unwrap()], chapter });
            format!("{}|{}", r.chapter.as_deref().map_or_else(|| "~".to_owned(), hs), hs(&r.seq))
        }
        "cs" => hs(chapter_separator_text(SEPS[a[0].parse::<usize>().unwrap()])),
        "cr" => {
            let r = caption_range(KINDS[a[0].parse::<usize>().unwrap()], a[1].parse().unwrap(), a[2].parse().unwrap());
            format!("{},{}", r.0, r.1)
        }
        "tf" => {
            let (p, r) = toc_format_style(TOC_FORMATS[a[0].parse::<usize>().unwrap()], n(a[1]));
            format!("{}|{}", hs(&p), hs(&r))
        }
        "ap" => {
            let kind = match a[1] {
                "normal" => AddTextKind::Normal,
                "heading" => AddTextKind::Heading,
                _ => AddTextKind::Other,
            };
            match add_text_plan(n(a[0]), kind) {
                AddTextPlan::Heading(h) => format!("{}|0|u", to_js_string(h)),
                AddTextPlan::Normal => "~|1|u".into(),
                AddTextPlan::OutlineNone => "~|0|n".into(),
                AddTextPlan::Outline(l) => format!("~|0|{}", to_js_string(l)),
            }
        }
        "mp" => {
            let lang = match a[0] {
                "en" => UiLang::En,
                "pt" => UiLang::Pt,
                _ => UiLang::Es,
            };
            hs(&manual_placeholder(lang, n(a[1])))
        }
        "ms" => hs(&format!("{}|{}", toc_sdt_pr(-123.0), toc_sdt_pr(5.0))),
        "tt" => {
            let id = ["auto1", "auto2", "manual"].iter().position(|x| *x == a[0]).unwrap();
            let t = &TOC_TEMPLATES[id];
            format!(
                "{}|{}|{}|{}|{}|{}|{}",
                hs(t.name.en),
                hs(t.name.pt),
                hs(t.name.es),
                hs(t.heading.en),
                hs(t.heading.pt),
                hs(t.heading.es),
                u8::from(t.manual)
            )
        }
        "wa" => {
            let r = word_around(&units(a[0]), a[1].parse().unwrap());
            format!("{},{}", r.0, r.1)
        }
        "mp2" => {
            let pts: Vec<u32> = if a[1] == "-" { Vec::new() } else { a[1].split(',').map(|x| x.parse().unwrap()).collect() };
            let (w, p) = manual_points(&units(a[0]), &pts);
            format!(
                "{}|{}",
                hx(&w),
                if p.is_empty() { "-".to_owned() } else { p.iter().map(ToString::to_string).collect::<Vec<_>>().join(",") }
            )
        }
        "oa" => offset_after_letters(&units(a[0]), a[1].parse().unwrap(), n(a[2])).to_string(),
        "hn" => {
            let seed = uo_jsnum::parse::to_number(a[1]);
            let taken_n: usize = a[2].parse().unwrap();
            let taken: Vec<String> = (0..taken_n)
                .map(|i| {
                    let s = to_js_string(seed + i as f64);
                    let s = format!("{s:0>9}");
                    format!("{}{}", a[0], &s[s.len() - 9..])
                })
                .collect();
            let mut s = seed;
            let name = hidden_bookmark_name(a[0], &mut |x| taken.iter().any(|t| t == x), &mut s);
            format!("{name}|{}", to_js_string(s))
        }
        "tp" => {
            let o = parse_toc_instr(&unhex(a[0]));
            let sources: Vec<TocSource> = if a[4] == "-" {
                Vec::new()
            } else {
                a[4].split('|')
                    .map(|x| {
                        let f: Vec<&str> = x.split('~').collect();
                        TocSource {
                            level: n(f[0]),
                            text: uo_docx::text::Utf16::from_units(units(f[1])),
                            number: (f[2] != "-").then(|| uo_docx::text::Utf16::from_units(units(f[2]))),
                            page: f[3].to_owned(),
                            bookmark: unhex(f[4]),
                            no_page: f[5] == "1",
                        }
                    })
                    .collect()
            };
            let look = TocLook { hyperlink_style: unhex(a[1]), tab_pos: n(a[2]), leader: (a[3] != "~").then(|| a[3].to_owned()) };
            let mut ids = IdAlloc::new();
            let paras = toc_entry_paragraphs(&o, &sources, &look, &mut ids, &mut |l| format!("S{}", to_js_string(l)).into());
            digest(&paras)
        }
        "dt" => {
            let lang = match a[0] {
                "en" => UiLang::En,
                "pt" => UiLang::Pt,
                _ => UiLang::Es,
            };
            let t = DateParts {
                year: a[1].parse().unwrap(),
                month: a[2].parse().unwrap(),
                day: a[3].parse().unwrap(),
                hour: a[4].parse().unwrap(),
                minute: a[5].parse().unwrap(),
            };
            let now = LocalNow { local: t, utc: t };
            date_time_samples(lang, &now).iter().map(|(p, s)| format!("{}={}", hs(p), hx(s))).collect::<Vec<_>>().join("|")
        }
        _ => panic!("code {name}"),
    }
}

#[test]
fn d2b_f10_pure_files_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f10-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut replay = Replay::new(&cap);
    let (mut pure, mut docs) = (0usize, 0usize);
    for rec in cap.records() {
        if rec.key.starts_with("pure#") {
            let op = cap.get(&rec.key, "op").unwrap();
            replay.check(&rec.key, "op", op);
            replay.check(&rec.key, "ret", &run(op));
            pure += 1;
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for rec in cap.records().iter().filter(|r| !r.key.starts_with("pure#")) {
        let (rel, _) = rec.key.rsplit_once("#rb").unwrap();
        if seen.iter().any(|s| s == &rec.key) {
            continue;
        }
        seen.push(rec.key.clone());
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        let op = cap.get(&rec.key, "op").unwrap();
        let f: Vec<usize> = op.split(':').map(|x| x.parse().unwrap()).collect();
        let Some(p) = doc.body.paras.get(f[0]) else { continue };
        let runs = runs_between(p, f[1], f[2]);
        let mut ids = IdAlloc::new();
        let para = Paragraph { runs, id: ids.para(), ..Paragraph::default() };
        replay.check(&rec.key, "op", op);
        replay.check(&rec.key, "ret", &digest(&[para]));
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f10-ops: {pure} pure records, {docs} runsBetween over corpus paragraphs, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
    assert!(pure > 2500 && docs > 300);
}
