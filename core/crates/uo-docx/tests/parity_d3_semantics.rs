//! D3 parity of the .NET semantics (record `docs/modules/uo-docx-d3.md`, "Parity harness"):
//! replays `tests/parity/d3/semantics.capture`, written by the C# harness
//! (`dotnet-harness semantics`) from the frozen C# on .NET 10 — `Uri.TryCreate` + scheme
//! allowlists, `Uri.UnescapeDataString`/`EscapeDataString`, `CustomInlineTag.TryParse`/`Format`,
//! the regex sites (`Colors`, `VmlSize`, `UniquePartPath`), `int`/`long.TryParse`, `Trim`,
//! `ToUpperInvariant`, `OrdinalIgnoreCase`, `DateTime.TryParse`, `ImageInfo.TryRead`,
//! `DocxPictures.Fit`, the `0.##` format and the internal helpers (`FirstWord`,
//! `RemoveInvalidChars`, `BuiltInName`, `TryParseHyperlink`, `IsWholeWord`, `RelativeTarget`,
//! `NextRelId`). Inputs come from the capture itself.
//!
//! Recapture: `dotnet run --project core/crates/uo-docx/dotnet-harness -- semantics
//! core/crates/uo-docx/tests/parity/d3/semantics.capture core/crates/uo-docx/tests/parity/custom-tag.inputs.txt`.
#![cfg(feature = "editing-api")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

use uo_docx::api::dom::{Dom, NS_V};
use uo_docx::api::{custom_tag, document, fields, image_info::ImageInfo, net, ooxml, paragraph_text, pictures, properties, shapes, styles};
use uo_testkit::{Capture, Replay};

/// The capture's escape (C# `CaptureWriter.Esc`), back to text; `None` for a lone surrogate.
fn unescape(s: &str) -> Option<String> {
    let mut units: Vec<u16> = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut b = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut b));
            continue;
        }
        match it.next() {
            Some('t') => units.push(9),
            Some('n') => units.push(10),
            Some('r') => units.push(13),
            Some('\\') => units.push(0x5C),
            Some('u') => {
                let h: String = it.by_ref().take(4).collect();
                units.push(u16::from_str_radix(&h, 16).ok()?);
            }
            Some(o) => units.push(u32::from(o) as u16),
            None => {}
        }
    }
    String::from_utf16(&units).ok()
}

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

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

const DEFAULT: &[&str] = &["http", "https", "mailto"];
const WIDE: &[&str] = &["http", "https", "mailto", "ftp", "tel", "news", "file", "ldap"];

/// A double as .NET `ToString("R")` (shortest round trip; exponent as `E+XX`).
fn r(d: f64) -> String {
    let s = uo_jsnum::format::to_js_string(d);
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = if let Some(x) = e.strip_prefix('-') { ("-", x) } else { ("+", e.trim_start_matches('+')) };
            format!("{m}E{sign}{digits:0>2}")
        }
        None => s,
    }
}

fn unique_doc() -> document::DocxDocument {
    let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let ct = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = format!(r#"<?xml version="1.0"?><w:document xmlns:w="{w}"><w:body><w:p/></w:body></w:document>"#);
    let files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", doc.as_bytes()),
        ("word/media/image1.png", &[1]),
        ("word/media/image.png", &[1]),
        ("word/media/image2.png", &[1]),
        ("word/media/x9.png", &[1]),
        ("word/media/x1.png", &[1]),
        ("word/a", &[1]),
        ("a1", &[1]),
    ];
    let bytes = uo_opc::zip::write(files, uo_opc::zip::WriteOptions::default()).unwrap();
    document::DocxDocument::open(&bytes).unwrap()
}

fn compute(site: &str, input: &str, unique: &document::DocxDocument) -> String {
    let split = |s: &str| s.split('\u{1}').map(str::to_owned).collect::<Vec<_>>();
    match site {
        "uri" => format!("{}{}", b(fields::is_allowed_url(input, DEFAULT)), b(fields::is_allowed_url(input, WIDE))),
        "tag" => match custom_tag::try_parse(Some(input)) {
            Some((t, j)) => format!("{t}\u{1}{j}"),
            None => "-".to_owned(),
        },
        "tagfmt" => {
            let p = split(input);
            match custom_tag::format(&p[0], &p[1]) {
                Ok(s) => s,
                Err(e) => format!("!{}", e.class()),
            }
        }
        "unescape" => net::unescape_data_string(input),
        "escape" => net::escape_data_string(input),
        "color" => match shapes::normalize_hex(Some(input)) {
            Ok(Some(h)) => h,
            Ok(None) => "-".to_owned(),
            Err(e) => format!("!{}", e.class()),
        },
        "firstword" => fields::first_word(input),
        "removeinvalid" => ooxml::remove_invalid_chars(input),
        "builtin" => styles::built_in_name(input).unwrap_or_else(|| "-".to_owned()),
        "hyperlink" => match fields::try_parse_hyperlink(input, DEFAULT) {
            Some(fields::HyperlinkTarget::Url(u)) => format!("1{u}\u{1}-"),
            Some(fields::HyperlinkTarget::Anchor(a)) => format!("1-\u{1}{a}"),
            None => "0".to_owned(),
        },
        "ints" => {
            let o = |v: Option<i64>| v.map_or("-".to_owned(), |x| x.to_string());
            format!(
                "{}|{}|{}|{}",
                o(net::parse_int(input).map(i64::from)),
                o(net::parse_int(input).map(i64::from)),
                o(net::parse_int_digits(input).map(i64::from)),
                o(net::parse_long(input))
            )
        }
        "trim" => format!("{}\u{1}{}\u{1}{}", net::trim(input), net::trim_start(input), net::to_upper_invariant(input)),
        "date" => match properties::parse_date(input) {
            Some(d) => {
                format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:07}|Utc", d.year, d.month, d.day, d.hour, d.minute, d.second, d.ticks)
            }
            None => "-".to_owned(),
        },
        "vml" => {
            let mut dom = Dom::new();
            let shape_name = dom.name(NS_V, "shape");
            let style_name = dom.name(uo_docx::api::dom::NS_NONE, "style");
            let shape = dom.new_element_with(shape_name, &[(style_name, input)]);
            match pictures::fit_vml(&mut dom, shape, 100.0, 50.0) {
                Ok(v) => format!("{}{}", b(v), dom.attr(shape, style_name).unwrap_or("")),
                Err(e) => format!("!{}", e.class()),
            }
        }
        "wholeword" => {
            let p = split(input);
            let t = net::units(&p[0]);
            b(paragraph_text::is_whole_word(&t, p[1].parse().unwrap(), p[2].parse().unwrap())).to_owned()
        }
        "relative" => {
            let p = split(input);
            document::relative_target(&p[0], &p[1])
        }
        "ignorecase" => {
            let p = split(input);
            let (a, bb) = (net::units(&p[0]), net::units(&p[1]));
            let oic = net::index_of(&a, &bb, 0, true).map_or(-1, |i| i as i64);
            let ord = net::index_of(&a, &bb, 0, false).map_or(-1, |i| i as i64);
            format!("{}{oic},{ord}", b(net::eq_ignore_case(&p[0], &p[1])))
        }
        "nextrelid" => {
            let ids: Vec<String> = if input.is_empty() { Vec::new() } else { split(input) };
            document::next_rel_id(ids.iter().map(String::as_str))
        }
        "image" => {
            let bytes: Vec<u8> = (0..input.len() / 2).map(|i| u8::from_str_radix(&input[2 * i..2 * i + 2], 16).unwrap()).collect();
            match ImageInfo::try_read(&bytes) {
                Ok(Some(i)) => format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}",
                    i.extension,
                    i.content_type,
                    i.width_px,
                    i.height_px,
                    r(i.dpi_x),
                    r(i.dpi_y),
                    i.width_emu(),
                    i.height_emu()
                ),
                Ok(None) => "-".to_owned(),
                Err(e) => format!("!{}", e.class()),
            }
        }
        "fit" => {
            let p: Vec<&str> = input.split('|').collect();
            let o = |s: &str| if s == "-" { None } else { Some(s.parse::<i64>().unwrap()) };
            let (x, y) = pictures::fit(p[0].parse().unwrap(), p[1].parse().unwrap(), o(p[2]), o(p[3]));
            format!("{x}|{y}")
        }
        "fmt" => net::format_0_00(uo_jsnum::parse::to_number(input)),
        "unique" => unique.unique_part_path(input),
        other => panic!("unknown site {other}"),
    }
}

#[test]
fn semantics_match_dotnet() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/d3/semantics.capture")).unwrap();
    let unique = unique_doc();
    let mut replay = Replay::new(&cap);
    let mut by_site: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut shown = 0;
    for rec in cap.records() {
        let site = rec.key.split(':').next().unwrap().to_owned();
        replay.skip(&rec.key, "in");
        let Some(input) = unescape(rec.get("in").unwrap()) else {
            replay.accept(&rec.key, "out", "", "lone surrogate input (a Rust &str cannot hold it)");
            continue;
        };
        let got = esc(&compute(&site, &input, &unique));
        let e = by_site.entry(site.clone()).or_default();
        e.0 += 1;
        if !replay.check(&rec.key, "out", &got) {
            e.1 += 1;
            if shown < 400 && std::env::var("UO_D3_SHOW").is_ok() {
                shown += 1;
                eprintln!("{site}: in={:?}\n   cs={}\n   rs={}", input, rec.get("out").unwrap(), got);
            }
        }
    }
    for (s, (t, m)) in &by_site {
        eprintln!("{s}: {t} cases, {m} mismatches");
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}
