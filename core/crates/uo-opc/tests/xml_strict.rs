//! The strict mode (gate 1, G6): what the C# Editing API reads (`DocxDocument.Parse`:
//! `XmlReader` with DTD prohibited, namespaces resolved, close tags checked, duplicates refused,
//! depth 256 counted as `XmlReader.Depth`). `xml_support/strict_oracle.txt` was written by the
//! real `XmlReader` (`strict_oracle.cs`); every line is replayed here.
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::error::{Error, LimitKind, XmlErrorKind};
use uo_opc::xml::{Element, Node, ParseOptions, parse};
use xml_support::{q, unq};

const STRICT: ParseOptions<'static> = ParseOptions::STRICT;

fn n(ns: Option<&str>) -> String {
    let s = q(ns.unwrap_or(""));
    s[1..s.len() - 1].to_owned()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the oracle splits a processing instruction at XML white space (the .NET XmlReader), not at the JS white space set"
)]
fn sdump(e: Element<'_>) -> String {
    let mut out = format!("{{{}}}{}[", n(e.namespace()), e.local_name());
    let attrs: Vec<String> = e
        .attrs()
        .filter(|a| a.name != "xmlns" && !a.name.starts_with("xmlns:"))
        .map(|a| format!("{{{}}}{}={}", n(a.namespace), a.name.split_once(':').map_or(a.name, |(_, l)| l), q(a.value)))
        .collect();
    out.push_str(&attrs.join(","));
    out.push_str("]{");
    let kids: Vec<String> = e
        .children()
        .map(|n| match n {
            Node::Element(c) => sdump(c),
            Node::Text(t) | Node::CData(t) => q(t),
            Node::Comment(c) => format!("!{}", q(c)),
            Node::Pi(p) => format!("?{}", p.split(|c: char| c.is_ascii_whitespace()).next().unwrap_or("")),
        })
        .collect();
    out.push_str(&kids.join(","));
    out.push('}');
    out
}

#[test]
fn strict_matches_xml_reader() {
    let text = include_str!("xml_support/strict_oracle.txt");
    let mut mismatches = Vec::new();
    let (mut trees, mut errs, mut depth) = (0, 0, 0);
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (input, expected) = line.split_once('\t').unwrap();
        let input = unq(input);
        let got = parse(input.as_str(), &STRICT);
        match (&got, expected) {
            (Err(Error::Limit { kind: LimitKind::XmlDepth, .. }), "DEPTH") => depth += 1,
            (Err(Error::Xml { .. }), "ERR") => errs += 1,
            (Ok(d), exp) if sdump(d.root()) == exp => trees += 1,
            (Ok(d), exp) => mismatches.push(format!("{input:?}\n  .net: {exp}\n  rust: {}", sdump(d.root()))),
            (Err(e), exp) => mismatches.push(format!("{input:?}\n  .net: {exp}\n  rust: {e:?}")),
        }
    }
    println!("strict: {trees} equal trees, {errs} errors on both sides, {depth} depth limits");
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(trees > 200 && errs > 200 && depth >= 4);
}

fn kind(xml: &str) -> XmlErrorKind {
    match parse(xml, &STRICT) {
        Err(Error::Xml { kind, .. }) => kind,
        other => panic!("{xml}: {other:?}"),
    }
}

#[test]
fn strict_error_classes() {
    assert_eq!(kind("<!DOCTYPE a><a/>"), XmlErrorKind::Dtd);
    assert_eq!(kind("<a><!DOCTYPE b></a>"), XmlErrorKind::Dtd);
    assert_eq!(kind("<a></b>"), XmlErrorKind::MismatchedClose);
    assert_eq!(kind("<p:a/>"), XmlErrorKind::UndeclaredPrefix);
    assert_eq!(kind("<a p:b=\"1\"/>"), XmlErrorKind::UndeclaredPrefix);
    assert_eq!(kind("<a b=\"1\" b=\"2\"/>"), XmlErrorKind::DuplicateAttribute);
    assert_eq!(kind("<a xmlns:p=\"u\" xmlns:q=\"u\" p:x=\"1\" q:x=\"2\"/>"), XmlErrorKind::DuplicateAttribute);
    assert_eq!(kind("<a>&e;</a>"), XmlErrorKind::UndeclaredEntity);
    assert_eq!(kind("<a>&#1;</a>"), XmlErrorKind::InvalidCharRef);
    assert_eq!(kind("<a>&#xD800;</a>"), XmlErrorKind::InvalidCharRef);
    assert_eq!(kind("<a>&#X41;</a>"), XmlErrorKind::Unexpected);
    assert_eq!(kind("<1a/>"), XmlErrorKind::InvalidName);
    assert_eq!(kind("<a xmlns:p=\"\"/>"), XmlErrorKind::InvalidName);
    assert_eq!(kind("<a"), XmlErrorKind::Unclosed);
    assert_eq!(kind("<a>"), XmlErrorKind::Unclosed);
    assert_eq!(kind(""), XmlErrorKind::NoRoot);
    assert_eq!(kind("<a/><b/>"), XmlErrorKind::Unexpected);
    assert_eq!(kind("<a>]]></a>"), XmlErrorKind::Unexpected);
}

#[test]
fn strict_depth_counts_like_xml_reader() {
    // an element at Depth 256 is allowed, content inside it (Depth 257) is not
    let ok = "<a>".repeat(257) + &"</a>".repeat(257);
    assert!(parse(ok.as_str(), &STRICT).is_ok());
    let text = "<a>".repeat(257) + "x" + &"</a>".repeat(257);
    assert!(matches!(parse(text.as_str(), &STRICT), Err(Error::Limit { kind: LimitKind::XmlDepth, max: 256, .. })));
    let deep = "<a>".repeat(258) + &"</a>".repeat(258);
    assert!(matches!(parse(deep.as_str(), &STRICT), Err(Error::Limit { kind: LimitKind::XmlDepth, .. })));
    // deep input never builds a deep stack: 100k levels stop at the limit
    let huge = "<a>".repeat(100_000);
    assert!(matches!(parse(huge.as_str(), &STRICT), Err(Error::Limit { kind: LimitKind::XmlDepth, .. })));
    // without a limit, 100k levels parse
    let unlimited = ParseOptions { max_depth: None, ..STRICT };
    let big = "<a>".repeat(100_000) + &"</a>".repeat(100_000);
    assert_eq!(parse(big.as_str(), &unlimited).unwrap().element_count(), 100_000);
}

#[test]
fn strict_namespaces_and_nodes() {
    let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let x = format!(
        "<?xml version=\"1.0\"?><!-- before --><w:document xmlns:w=\"{w}\" xmlns=\"urn:d\"><w:t xml:space=\"preserve\"> a </w:t><b><!-- c --><?pi data?><![CDATA[<x>]]></b></w:document>"
    );
    let d = parse(x.as_str(), &STRICT).unwrap();
    let r = d.root();
    assert_eq!(r.namespace(), Some(w));
    assert_eq!(r.local_name(), "document");
    let t = r.first_child("w:t").unwrap();
    assert_eq!(t.attr_ns("http://www.w3.org/XML/1998/namespace", "space"), Some("preserve"));
    let b = r.first_child("b").unwrap();
    assert_eq!(b.namespace(), Some("urn:d"));
    let kids: Vec<Node<'_>> = b.children().collect();
    assert!(matches!(kids[0], Node::Comment(" c ")));
    assert!(matches!(kids[1], Node::Pi("pi data")));
    assert!(matches!(kids[2], Node::CData("<x>")));
    // unprefixed attributes have no namespace
    let e = parse("<a xmlns=\"urn:d\" v=\"1\"/>", &STRICT).unwrap();
    assert_eq!(e.root().attrs().find(|a| a.name == "v").unwrap().namespace, None);
}

#[test]
fn strict_normalizes_like_xml_reader() {
    let d = parse("<a v=\"x\r\ny\tz&#10;\">a\r\nb\rc&#13;<![CDATA[d\r\ne]]></a>", &STRICT).unwrap();
    assert_eq!(d.root().attr("v"), Some("x y z\n"));
    assert_eq!(d.root().text_content(), "a\nb\nc\rd\ne");
}

#[test]
fn hostile_declarations_and_attributes_stay_linear() {
    let n = 50_000;
    // many declarations on one element, each prefix used once
    let mut x = String::from("<a");
    for i in 0..n {
        x.push_str(&format!(" xmlns:p{i}=\"u{i}\""));
    }
    for i in 0..n {
        x.push_str(&format!(" p{i}:v=\"1\""));
    }
    x.push_str("><p0:b/></a>");
    let t = std::time::Instant::now();
    let d = parse(x.as_str(), &STRICT).unwrap();
    assert_eq!(d.root().attr_count(), 2 * n);
    assert_eq!(d.root().first_child("p0:b").unwrap().namespace(), Some("u0"));
    // the same URI under two prefixes: duplicate expanded name found at the end
    let y = x.replace("><p0:b/></a>", " xmlns:z=\"u7\" z:v=\"2\"/>");
    assert_eq!(kind(&y), XmlErrorKind::DuplicateAttribute);
    // lenient renaming with many non-canonical prefixes of a known namespace
    let mc = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    let mut w = String::from("<a");
    for i in 0..n {
        w.push_str(&format!(" xmlns:m{i}=\"{mc}\" m{i}:v{i}=\"1\""));
    }
    w.push_str("/>");
    let d = parse(w.as_str(), &xml_support::DOCX).unwrap();
    assert_eq!(d.root().attr("mc:v7"), Some("1"));
    assert!(t.elapsed().as_secs() < 20, "quadratic: {:?}", t.elapsed());
}
