//! D3 parity of the Editing API (record `docs/modules/uo-docx-d3.md`, "Parity harness"):
//! replays `tests/parity/d3/corpus.capture`, written by the C# harness (`dotnet-harness corpus`,
//! `Scripts.cs`) from the frozen C# Editing API on .NET 10, over every document of
//! `tests/parity/d3/corpus.list` (the harness fixtures and every tracked docx outside `core/`)
//! and 18 scripted operation sequences. Facets per (document, script): `ret` (the API results),
//! `pkg` (the digest of the saved package: each part's name and bytes in order; the random ids
//! of the import scripts masked on both sides) and `err` (the exception class).
//!
//! `UO_D3_FILTER=<substring>` replays the keys containing it; `UO_D3_DUMP=<dir>` writes the
//! saved parts of each mismatching key for a diff with the harness's `--dump`.
//!
//! Recapture: `dotnet run --project core/crates/uo-docx/dotnet-harness -- corpus
//! core/crates/uo-docx/tests/parity/d3/corpus.capture . core/crates/uo-docx/tests/parity/d3/corpus.list`
//! (from the repository root).
#![cfg(feature = "editing-api")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::api::body_writer::{BodyWriter, BodyWriterOptions, ParagraphFormat, RunFormat, VerticalAlign};
use uo_docx::api::properties::UtcDateTime;
use uo_docx::api::row_template::RowTemplate;
use uo_docx::api::sanitizer::{self, SanitizeOptions};
use uo_docx::api::shapes::{self, HeaderFooterType};
use uo_docx::api::template::{self, ApplyContentOptions};
use uo_docx::api::text::{self, TextScope};
use uo_docx::api::{ApiError, DocxDocument, ErrorKind, custom_inlines, fields, net, numbering, pictures, review, samples};
use uo_testkit::{Capture, Digest, Replay, repo_root};

const SCRIPTS: &[&str] = &[
    "open", "content", "output", "valid", "empty", "apply", "applysec", "import", "text", "fields", "unsafe", "props", "writer", "shapes",
    "inlines", "parts", "rowtpl", "samples",
];

/// C# `CaptureWriter.Esc` (per UTF-16 unit: controls and surrogates as `\uXXXX`).
fn esc(s: &str) -> String {
    let bs = char::from(92u8);
    let mut res = String::new();
    for c in s.chars() {
        match c as u32 {
            92 => {
                res.push(bs);
                res.push(bs);
            }
            9 => {
                res.push(bs);
                res.push('t');
            }
            10 => {
                res.push(bs);
                res.push('n');
            }
            13 => {
                res.push(bs);
                res.push('r');
            }
            u if u < 0x20 || u == 0x7F => res.push_str(&format!("{bs}u{u:04X}")),
            u if u > 0xFFFF => {
                let mut b = [0u16; 2];
                for x in c.encode_utf16(&mut b) {
                    res.push_str(&format!("{bs}u{x:04X}"));
                }
            }
            _ => res.push(c),
        }
    }
    res
}

fn digest_str(s: &str) -> String {
    Digest::of(s.as_bytes()).short()
}

/// C# `CaptureWriter.Ret`.
fn ret(s: &str) -> String {
    let e = esc(s);
    if e.encode_utf16().count() <= 96 { format!("={e}") } else { format!("#{}", digest_str(s)) }
}

/// C# `Scripts.Mask`.
fn mask(data: &[u8]) -> Vec<u8> {
    let keys: [&[u8]; 3] = [b"paraId=\"", b"nsid w:val=\"", b"<w:id w:val=\""];
    let mut o = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let hit = keys.iter().find(|k| data[i..].starts_with(k));
        let Some(k) = hit else {
            o.push(data[i]);
            i += 1;
            continue;
        };
        o.extend_from_slice(k);
        i += k.len();
        let end = data[i..].iter().position(|&b| b == b'"').map_or(data.len(), |p| i + p);
        o.push(b'#');
        i = end;
    }
    o
}

/// C# `Scripts.Pkg` over the saved parts.
fn pkg(parts: &[(String, Vec<u8>)], masked: bool, dump: Option<&Path>) -> String {
    let mut buf = Vec::new();
    for (name, data) in parts {
        let data = if masked { mask(data) } else { data.clone() };
        if let Some(d) = dump {
            let p = d.join(name.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR));
            std::fs::create_dir_all(p.parent().unwrap()).ok();
            std::fs::write(&p, &data).ok();
        }
        buf.extend_from_slice(name.as_bytes());
        buf.push(0);
        buf.extend_from_slice(data.len().to_string().as_bytes());
        buf.push(0);
        buf.extend_from_slice(&data);
    }
    Digest::of(&buf).short()
}

fn b(v: bool) -> String {
    if v { "1".into() } else { "0".into() }
}

fn s(v: Option<String>) -> String {
    v.unwrap_or_else(|| "<null>".to_owned())
}

/// `Scripts.Verdict`: JSON length in UTF-16 units.
fn verdict(ty: &str, json: &str) -> bool {
    !json.encode_utf16().count().is_multiple_of(3) && !ty.contains("bogus")
}

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let v: Vec<u32> = chunk.iter().map(|&c| u32::from(val(c))).collect();
        let n = v.iter().enumerate().fold(0u32, |a, (k, &x)| a | (x << (18 - 6 * k)));
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// The frozen `DocxBuilder.PngOfSize`.
fn png_of_size(w: u32, h: u32) -> Vec<u8> {
    let mut p = base64("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==");
    p[16..20].copy_from_slice(&w.to_be_bytes());
    p[20..24].copy_from_slice(&h.to_be_bytes());
    p
}

fn utc_o(d: Option<UtcDateTime>) -> String {
    d.map_or_else(|| "<null>".to_owned(), |d| d.round_trip())
}

fn run(script: &str, bytes: &[u8], next: &[u8], log: &mut Vec<String>) -> Result<Vec<(String, Vec<u8>)>, ApiError> {
    let mut doc = DocxDocument::open(bytes)?;
    doc.set_seed(0x5EED);
    let png = png_of_size(2000, 1000);
    match script {
        "open" => {
            log.push(doc.main_part_path().to_owned());
            log.push(doc.part_names().len().to_string());
        }
        "content" => sanitizer::sanitize(&mut doc, &SanitizeOptions::content())?,
        "output" => sanitizer::sanitize(&mut doc, &SanitizeOptions::output())?,
        "valid" => {
            let opts = SanitizeOptions { validate_custom_inlines: true, ..SanitizeOptions::content() };
            let stage = sanitizer::sanitize_begin(&mut doc, &opts)?;
            let verdicts: Vec<bool> = stage
                .inlines()
                .iter()
                .map(|i| {
                    log.push(format!("{}\u{1}{}", i.ty, i.json));
                    verdict(&i.ty, &i.json)
                })
                .collect();
            stage.finish(&mut doc, &verdicts)?;
        }
        "empty" => template::empty(&mut doc)?,
        "apply" => template::apply_content(
            &mut doc,
            next,
            &ApplyContentOptions { sample_markers: vec!["#{Heading1}".into(), "#{Normal}".into()], ..ApplyContentOptions::default() },
        )?,
        "applysec" => template::apply_content(
            &mut doc,
            next,
            &ApplyContentOptions { preserve_sections: true, fit_pictures: true, ..ApplyContentOptions::default() },
        )?,
        "import" => {
            let mut src = DocxDocument::open(next)?;
            src.set_seed(0xD00D);
            template::import_body(&mut doc, &mut src)?;
        }
        "text" => {
            log.push(digest_str(&text::get_plain_text(&mut doc, true)?));
            let instr: Vec<String> =
                fields::get_instructions(&mut doc)?.into_iter().map(|f| format!("{}\u{2}{}", f.name, f.text)).collect();
            log.push(digest_str(&instr.join("\u{1}")));
            log.push(b(text::contains(&mut doc, "the", true, true, TextScope::Document)?));
            log.push(text::replace(&mut doc, "the", Some("THE·X"), false, true, TextScope::Document)?.to_string());
            log.push(
                text::replace_parameters(&mut doc, &[("Title", Some("T\tx")), ("Missing", None), ("Version", Some("1\r\n2"))])?.to_string(),
            );
            log.push(text::replace(&mut doc, "a", Some("b\nc"), true, false, TextScope::Body)?.to_string());
            log.push(
                text::remove_paragraphs_containing(&mut doc, &["zz", "#{Normal}", "Lorem"], true, true, TextScope::Document)?.to_string(),
            );
        }
        "fields" => {
            log.push(fields::replace_disallowed_with_result(&mut doc, Some(&["PAGE", "REF", "HYPERLINK"]), Some(&["https"]))?.to_string());
            log.push(fields::convert_hyperlink_fields(&mut doc, None)?.to_string());
            log.push(fields::get_instructions(&mut doc)?.len().to_string());
        }
        "unsafe" => {
            sanitizer::remove_unsafe_content(&mut doc, Some(&["http", "https", "mailto", "file"]))?;
            sanitizer::remove_custom_xml(&mut doc)?;
            sanitizer::merge_sections(&mut doc)?;
            sanitizer::remove_headers_and_footers(&mut doc)?;
            review::disable_track_changes(&mut doc)?;
            review::remove_protection(&mut doc)?;
        }
        "props" => {
            let mut p = doc.properties();
            for v in [
                p.title()?,
                p.subject()?,
                p.author()?,
                p.keywords()?,
                p.comments()?,
                p.last_author()?,
                p.category()?,
                p.company()?,
                p.manager()?,
                p.application_name()?,
            ] {
                log.push(s(v));
            }
            log.push(utc_o(p.last_save_date()?));
            log.push(utc_o(p.created_date()?));
            log.push(p.custom_property_names()?.join(","));
            p.set_title(Some("Título\u{1} x"))?;
            p.set_author(None)?;
            p.set_company(Some("ACME"))?;
            p.set_subject(Some(""))?;
            p.set_custom_property("k", "v")?;
            p.set_custom_property("k", "w")?;
            p.set_custom_property("z", "")?;
            p.set_last_save_date(Some(UtcDateTime { year: 2024, month: 5, day: 6, hour: 7, minute: 8, second: 9, ticks: 0 }))?;
            p.set_created_date(None)?;
            log.push(s(p.title()?));
            log.push(s(p.company()?));
            log.push(p.custom_property_names()?.join(","));
            log.push(utc_o(p.last_save_date()?));
            let mid = doc.save_parts()?;
            log.push(pkg(&mid, false, None));
            let mut p = doc.properties();
            p.remove_personal_metadata()?;
            log.push(p.custom_property_names()?.join(","));
        }
        "writer" => {
            let opts = BodyWriterOptions {
                link_color: if bytes.len().is_multiple_of(2) { None } else { Some("#0563c1".into()) },
                ..BodyWriterOptions::default()
            };
            let mut wr = BodyWriter::new(&mut doc, opts)?;
            log.push(b(wr.line_is_empty()));
            wr.add_text(
                &mut doc,
                "Hello\tworld\nnext\r\nline\u{B}v\u{1}x",
                &RunFormat { bold: true, color: Some("red".into()), background: Some("#ff0".into()), ..RunFormat::default() },
            )?;
            wr.add_text(&mut doc, "link", &RunFormat { link: Some("www.example.com".into()), underline: true, ..RunFormat::default() })?;
            wr.add_text(
                &mut doc,
                "bad",
                &RunFormat {
                    link: Some("javascript:alert(1)".into()),
                    italic: true,
                    strike: true,
                    vertical_align: Some(VerticalAlign::Subscript),
                    ..RunFormat::default()
                },
            )?;
            wr.add_text(&mut doc, "", &RunFormat::default())?;
            wr.add_break(&mut doc)?;
            log.push(b(wr.add_image(&mut doc, &png, None)?));
            log.push(b(wr.add_image(&mut doc, &[1, 2, 3], None)?));
            log.push(b(wr.line_is_empty()));
            wr.end_paragraph(
                &mut doc,
                &ParagraphFormat {
                    style_id: Some("Heading1".into()),
                    alignment: Some("center".into()),
                    list_num_id: Some("1".into()),
                    list_level: 12,
                    left_indent_twips: 360,
                    keep_with_next: true,
                    ..ParagraphFormat::default()
                },
            )?;
            wr.add_text(
                &mut doc,
                "sup",
                &RunFormat {
                    vertical_align: Some(VerticalAlign::Superscript),
                    link: Some(" https://example.com/x ".into()),
                    color: Some("rgb(1, 2, 300)".into()),
                    ..RunFormat::default()
                },
            )?;
            wr.end_paragraph(&mut doc, &ParagraphFormat::default())?;
            match wr.end_paragraph(&mut doc, &ParagraphFormat { alignment: Some("middle".into()), ..ParagraphFormat::default() }) {
                Err(ApiError { kind: ErrorKind::Argument, .. }) => log.push("bad-align".into()),
                Err(e) => return Err(e),
                Ok(()) => {}
            }
            wr.add_text(&mut doc, "  ", &RunFormat::default())?;
            wr.finish(&mut doc)?;
        }
        "shapes" => {
            log.push(pictures::fit_to_page(&mut doc)?.to_string());
            let found = shapes::find_containing(&mut doc, "a")?;
            log.push(found.len().to_string());
            if let Some(s0) = found.first() {
                log.push(digest_str(&s0.text(&doc)));
                s0.set_fill_color(&mut doc, "#FF0000")?;
                s0.set_text_color(&mut doc, "00ff00", false)?;
            }
            if let Some(s1) = found.get(1) {
                s1.set_fill_transparent(&mut doc)?;
                s1.clear_text(&mut doc)?;
                s1.set_text_color(&mut doc, "blue", true)?;
            }
            if let Some(s0) = found.first() {
                match s0.set_fill_color(&mut doc, "nope") {
                    Err(ApiError { kind: ErrorKind::Argument, .. }) => log.push("bad-color".into()),
                    Err(e) => return Err(e),
                    Ok(()) => {}
                }
            }
            log.push(shapes::replace_text_with_image(&mut doc, "the", &png, Some(100.0), Some(50.5))?.to_string());
            let footer = shapes::first_shape_in(&mut doc, true, HeaderFooterType::Default)?;
            log.push(footer.map_or_else(|| "<null>".to_owned(), |f| digest_str(&f.text(&doc))));
            if let Some(f) = footer {
                f.remove(&mut doc)?;
            }
            log.push(b(shapes::clear_header_footer(&mut doc, false, HeaderFooterType::Default)?));
            log.push(b(shapes::clear_header_footer(&mut doc, true, HeaderFooterType::First)?));
            log.push(shapes::remove_containing(&mut doc, "zzz")?.to_string());
            log.push(s(numbering::add_restarted_instance(&mut doc, "1")?));
        }
        "inlines" => {
            log.push(custom_inlines::replace_text(&mut doc, "the", "x.y", "{\"a\":[1,2]}", "F\u{7}b")?.to_string());
            let list = custom_inlines::find(&mut doc, None)?;
            for c in &list {
                log.push(format!(
                    "{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
                    c.ty,
                    c.json,
                    c.part_path,
                    s(c.title(&doc)),
                    c.text(&doc),
                    b(c.is_attached(&doc))
                ));
            }
            for (k, c) in list.iter().enumerate() {
                let r = match k % 4 {
                    0 => c.replace_with_hyperlink(&mut doc, "https://e.com/x", "L", true, None).map(|v| log.push(b(v))),
                    1 => c.replace_with_text(&mut doc, if k % 8 == 1 { "" } else { "T" }),
                    2 => c.unwrap(&mut doc),
                    _ => c.replace_with_hyperlink(&mut doc, "ftp://e.com", "F", false, None).map(|v| log.push(b(v))),
                };
                match r {
                    Err(ApiError { kind: ErrorKind::InvalidOperation, .. }) => log.push("detached".into()),
                    Err(e) => return Err(e),
                    Ok(()) => {}
                }
            }
            if let Some(c) = list.first() {
                match c.unwrap(&mut doc) {
                    Err(ApiError { kind: ErrorKind::InvalidOperation, .. }) => log.push("again".into()),
                    Err(e) => return Err(e),
                    Ok(()) => {}
                }
            }
            let all = custom_inlines::find(&mut doc, None)?;
            let verdicts: Vec<bool> = all.iter().map(|i| verdict(&i.ty, &i.json)).collect();
            log.push(custom_inlines::unwrap_where(&mut doc, &all, &verdicts)?.to_string());
        }
        "parts" => {
            review::remove_comments(&mut doc)?;
            review::accept_all_revisions(&mut doc)?;
            for part in doc.part_names() {
                let bytes = doc.get_part_bytes(&part)?;
                let ct = doc.get_content_type(&part)?;
                log.push(format!("{part}:{}:{}", bytes.map_or_else(|| "-".to_owned(), |b| Digest::of(&b).short()), s(ct)));
            }
            log.push(format!("{}{}", b(doc.has_part("WORD/DOCUMENT.XML")), b(doc.has_part("nope"))));
            let main = doc.main_part_path().to_owned();
            for r in doc.get_relationships(&main)? {
                log.push(format!("{}\u{1}{}\u{1}{}\u{1}{}", r.id, r.rel_type, r.target, b(r.external)));
            }
            log.push(s(doc.get_relationship("", "rId1")?.map(|r| r.target)));
            doc = doc.try_clone()?;
        }
        "rowtpl" => {
            let t = match RowTemplate::take(&mut doc, "the")? {
                Some(t) => Some(t),
                None => RowTemplate::take(&mut doc, "#{Version}")?,
            };
            log.push(if t.is_some() { "row".into() } else { "<null>".into() });
            if let Some(mut t) = t {
                t.append(&mut doc, &[("Version", Some("1.0")), ("y", None), ("the", Some("x"))], true)?;
                t.append(&mut doc, &[], false)?;
                log.push(t.appended_rows().to_string());
            }
        }
        "samples" => {
            log.push(b(samples::contains(&mut doc, "Heading1")?));
            let mut m = samples::extract(&mut doc, &["Heading1", "Normal", "BulletList", "Heading1", "Logo"])?;
            m.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in &m {
                log.push(format!("{k}\u{1}{}\u{1}{}", s(v.style_id()), s(v.num_id())));
            }
            let opts = BodyWriterOptions {
                reuse_empty_last_paragraph: false,
                trim_trailing_empty_paragraph: false,
                fit_pictures: false,
                ..BodyWriterOptions::default()
            };
            let mut wr = BodyWriter::new(&mut doc, opts)?;
            for (_, v) in m {
                wr.add_text(&mut doc, "from sample", &RunFormat::default())?;
                wr.end_paragraph(&mut doc, &ParagraphFormat { base: Some(v), keep_with_next: true, ..ParagraphFormat::default() })?;
            }
            wr.finish(&mut doc)?;
        }
        other => panic!("unknown script {other}"),
    }
    let _ = net::trim;
    doc.save_parts()
}

#[test]
fn editing_api_matches_csharp() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = repo_root(manifest);
    let cap = Capture::read(Path::new(manifest).join("tests/parity/d3/corpus.capture")).unwrap();
    let list: Vec<String> = std::fs::read_to_string(Path::new(manifest).join("tests/parity/d3/corpus.list"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let filter = std::env::var("UO_D3_FILTER").ok();
    let dump = std::env::var("UO_D3_DUMP").ok();
    let cap = cap.subset(|k| filter.as_deref().is_none_or(|f| k.contains(f)));
    let mut replay = Replay::new(&cap);
    let mut by_script: std::collections::BTreeMap<&str, (usize, usize)> = std::collections::BTreeMap::new();
    for (i, file) in list.iter().enumerate() {
        if filter.as_deref().is_some_and(|f| !SCRIPTS.iter().any(|s| format!("{file}|{s}").contains(f))) {
            continue;
        }
        let bytes = std::fs::read(root.join(file)).unwrap();
        let next = std::fs::read(root.join(&list[(i + 1) % list.len()])).unwrap();
        for &script in SCRIPTS {
            let key = format!("{file}|{script}");
            if cap.record(&key).is_none() {
                continue;
            }
            let mut log = Vec::new();
            let result = run(script, &bytes, &next, &mut log);
            let masked = matches!(script, "apply" | "applysec" | "import");
            let e = by_script.entry(script).or_default();
            e.0 += 1;
            let mut ok = replay.check(&key, "ret", &ret(&log.join("|")));
            match &result {
                Ok(parts) => {
                    let d = pkg(parts, masked, None);
                    if !replay.check(&key, "pkg", &d) {
                        ok = false;
                        if let Some(dir) = &dump {
                            let sanitized: String =
                                key.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
                            pkg(parts, masked, Some(&Path::new(dir).join(sanitized)));
                        }
                    }
                }
                Err(e) => {
                    if !replay.check(&key, "err", &e.class()) {
                        ok = false;
                    }
                }
            }
            if !ok {
                e.1 += 1;
            }
        }
    }
    for (s, (t, m)) in &by_script {
        eprintln!("{s}: {t} runs, {m} mismatching");
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}
