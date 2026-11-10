//! Parity of the D1b frontier W4/W4b (`write::emit::{notes, settings}`: docx-core `notes.ts`,
//! `settings-xml.ts`, `hf-ops.ts`): replays `tests/parity/settings-xml.capture`, written by
//! `js/capture-settings-xml.mjs` from the frozen TS (`writeDefaultTabStop`, `writeHyphenationSettings`,
//! `writeEvenAndOddHeaders`, `writeDisplayBackgroundShape`, `ensureNoteParts`, `patchSettingsNotes`,
//! `hfPartXml`, `copyHfPartXml`, `dropUnusedHfParts`, `pageNumberGroupIds`) over the 314 distinct mini
//! packages of the corpus (`tests/parity/settings-xml.inputs`: content types, document relationships,
//! settings, notes, header/footer parts and their relationships), as they are and in the named variants of
//! the capture script (missing, self-closing, bare, stripped, flags off, prefixed, BOM, truncated, ...),
//! with the arguments of its lists. A package op compares the digest of the summary (the part order and
//! each changed part's short SHA-256) or `err`; a pure op the digest of its output.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-settings-xml.mjs`.
//! `UO_PARITY_SHOW=<key>` prints the Rust summary of one record next to the TS one
//! (`UO_EXPLAIN=<key>` in the capture script).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use uo_docx::error::DocxError;
use uo_docx::js::NumKey;
use uo_docx::model::formatting::NoteProps;
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::emit::notes::{NotePropsArg, ensure_note_parts, patch_settings_notes};
use uo_docx::write::emit::settings::{
    HyphenationSettings, copy_hf_part_xml, drop_unused_hf_parts, hf_part_xml, is_hf_relationship, page_number_group_ids, quoted_attr,
    self_closing_matches, write_default_tab_stop, write_display_background_shape, write_even_and_odd_headers, write_hyphenation_settings,
};
use uo_docx::write::matchers::{boundary_at, find_from, remove_elements_lazy};
use uo_opc::package::OpenOptions;
use uo_opc::{Limits, Package};
use uo_testkit::{Capture, Digest, Replay, repo_root};

#[allow(unused_imports)]
use crate::w4_support;

use w4_support::{Part, dec, inputs, package, synthetic_inputs, synthetic_package};

const SETTINGS: &str = "word/settings.xml";
const RELS: &str = "word/_rels/document.xml.rels";
const CTYPES: &str = "[Content_Types].xml";
const W_NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

fn edit(parts: &[Part], name: &str, f: impl Fn(&str) -> String) -> Vec<Part> {
    parts.iter().map(|(n, d)| if n == name { (n.clone(), f(&dec(d)).into_bytes()) } else { (n.clone(), d.clone()) }).collect()
}

fn drop_part(parts: &[Part], name: &str) -> Vec<Part> {
    parts.iter().filter(|(n, _)| n != name).cloned().collect()
}

const ELEMENTS: [&str; 9] = [
    "w:defaultTabStop",
    "w:autoHyphenation",
    "w:consecutiveHyphenLimit",
    "w:hyphenationZone",
    "w:doNotHyphenateCaps",
    "w:evenAndOddHeaders",
    "w:displayBackgroundShape",
    "w:footnotePr",
    "w:endnotePr",
];

fn strip_elements(t: &str) -> String {
    ELEMENTS.iter().fold(t.to_owned(), |x, n| remove_elements_lazy(&x, n))
}

/// `t.replace('</w:settings>', with)`: the first, the text as a literal.
fn replace_close(t: &str, with: &str) -> String {
    t.replacen("</w:settings>", with, 1)
}

/// `t.replace(/<Relationship\b[^>]*relationships\/settings"[^>]*\/>/g, '')`.
fn remove_settings_rels(t: &str) -> String {
    let lit = "relationships/settings\"";
    let open = "<Relationship";
    let (mut out, mut last, mut from) = (String::new(), 0, 0);
    while let Some(i) = find_from(t, open, from) {
        from = i + 1;
        let pos = i + open.len();
        if !boundary_at(t, pos) {
            continue;
        }
        let Some(gt) = t[pos..].find('>').map(|k| pos + k) else { break };
        let first_lit = t[pos..gt].find(lit).map(|k| pos + k + lit.len());
        if first_lit.is_some_and(|e| e < gt) && t.as_bytes()[gt - 1] == b'/' {
            out.push_str(&t[last..i]);
            last = gt + 1;
            from = gt + 1;
        }
    }
    out.push_str(&t[last..]);
    out
}

/// `t.replace(/>\s*</g, '><')`.
fn one_line(t: &str) -> String {
    let (mut out, mut i) = (String::new(), 0);
    while i < t.len() {
        let c = t[i..].chars().next().unwrap();
        if c == '>' {
            let mut j = i + 1;
            while let Some(s) = t[j..].chars().next().filter(|c| uo_jsnum::text::is_js_space(*c)) {
                j += s.len_utf8();
            }
            if t[j..].starts_with('<') {
                out.push_str("><");
                i = j + 1;
                continue;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

const VARIANTS: [&str; 17] = [
    "asis",
    "nosettings",
    "norel",
    "nosettings_norel",
    "selfclose",
    "bare",
    "space",
    "strip",
    "off",
    "zero",
    "prefixed",
    "bom",
    "noct",
    "emptyct",
    "emptyrels",
    "badxml",
    "oneline",
];

fn variant(name: &str, p: &[Part]) -> Vec<Part> {
    match name {
        "asis" => p.to_vec(),
        "nosettings" => drop_part(p, SETTINGS),
        "norel" => edit(p, RELS, remove_settings_rels),
        "nosettings_norel" => edit(&drop_part(p, SETTINGS), RELS, remove_settings_rels),
        "selfclose" => edit(p, SETTINGS, |_| format!("<w:settings {W_NS}/>")),
        "bare" => {
            edit(p, SETTINGS, |_| format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:settings {W_NS}></w:settings>"))
        }
        "space" => edit(p, SETTINGS, |_| format!("<w:settings {W_NS}> </w:settings>")),
        "strip" => edit(p, SETTINGS, strip_elements),
        "off" => edit(p, SETTINGS, |t| {
            replace_close(
                &strip_elements(t),
                "<w:evenAndOddHeaders w:val=\"false\"/><w:displayBackgroundShape w:val=\"off\"/></w:settings>",
            )
        }),
        "zero" => edit(p, SETTINGS, |t| {
            replace_close(
                &strip_elements(t),
                "<w:evenAndOddHeaders w:val=\"0\"></w:evenAndOddHeaders><w:displayBackgroundShape/><w:defaultTabStop w:val=\"1e+21\"/><w:autoHyphenation w:val=\"0\"/></w:settings>",
            )
        }),
        "prefixed" => edit(p, SETTINGS, |t| {
            t.replacen("xmlns:w=", "xmlns:ns0=", 1).replace("</w:", "</ns0:").replace("<w:", "<ns0:").replace(" w:", " ns0:")
        }),
        "bom" => p
            .iter()
            .map(|(n, d)| {
                if n == SETTINGS {
                    let mut v = vec![0xef, 0xbb, 0xbf];
                    v.extend_from_slice(d);
                    (n.clone(), v)
                } else {
                    (n.clone(), d.clone())
                }
            })
            .collect(),
        "noct" => drop_part(p, CTYPES),
        "emptyct" => p.iter().map(|(n, d)| (n.clone(), if n == CTYPES { Vec::new() } else { d.clone() })).collect(),
        "emptyrels" => p.iter().map(|(n, d)| (n.clone(), if n == RELS { Vec::new() } else { d.clone() })).collect(),
        "badxml" => edit(p, SETTINGS, |t| replace_close(t, "")),
        "oneline" => edit(p, SETTINGS, one_line),
        other => panic!("unknown variant {other}"),
    }
}

fn summary(before: &[Part], pkg: &Package) -> String {
    let b: HashMap<&str, &[u8]> = before.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut lines = vec![format!("order {}", names.join("|"))];
    for n in &names {
        let d = pkg.part(n).unwrap().unwrap_or(&[]);
        if b.get(n.as_str()) != Some(&d) {
            lines.push(format!("{n}={}", Digest::of(d).short()));
        }
    }
    lines.join("\n")
}

fn sha(text: &str) -> String {
    Digest::of(text.as_bytes()).short()
}

struct Runner<'a, 'c> {
    replay: &'a mut Replay<'c>,
    show: Option<String>,
}

impl Runner<'_, '_> {
    fn put(&mut self, key: &str, text: &str) {
        if self.show.as_deref() == Some(key) {
            println!("rust summary of {key}:\n{text}");
        }
        let v = if text == "err" { "err".to_owned() } else { sha(text) };
        self.replay.check(key, "out", &v);
    }

    fn pkg_op(&mut self, key: &str, parts: &[Part], f: impl FnOnce(&mut Package) -> Result<(), DocxError>) -> Option<Package> {
        let mut pkg = package(parts);
        match f(&mut pkg) {
            Ok(()) => {
                let text = summary(parts, &pkg);
                self.put(key, &text);
                Some(pkg)
            }
            Err(_) => {
                self.put(key, "err");
                None
            }
        }
    }
}

fn hf_ids(rels: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for (a, b) in self_closing_matches(rels, "Relationship") {
        let tag = &rels[a..b];
        if !is_hf_relationship(tag) {
            continue;
        }
        if let Some(id) = quoted_attr(tag, "Id") {
            ids.push(id.to_owned());
        }
    }
    ids
}

fn note_props(pos: Option<&str>, num_fmt: Option<&str>, num_start: Option<f64>, num_restart: Option<&str>) -> NoteProps {
    use uo_docx::js::{JsNumber, Str};
    NoteProps {
        pos: pos.map(Str::from),
        num_fmt: num_fmt.map(Str::from),
        num_start: num_start.map(JsNumber::Num),
        num_restart: num_restart.map(Str::from),
        ..NoteProps::default()
    }
}

fn is_hf_part(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("word/") else { return false };
    (rest.starts_with("header") || rest.starts_with("footer")) && !rest.contains('/') && rest.ends_with(".xml")
}

fn num_key_positions(story: &uo_docx::model::sources::StorySource, parts: &[uo_docx::model::sources::SourcePart]) -> String {
    let mut flat: Vec<NumKey> = Vec::new();
    let mut stack = vec![story.groups.iter()];
    while let Some(it) = stack.last_mut() {
        let Some(g) = it.next() else {
            stack.pop();
            continue;
        };
        flat.extend(g.blocks.iter().map(|b| NumKey::new(b.to_number())));
        if let Some(inner) = g.inner.as_ref() {
            stack.push(inner.iter());
        }
    }
    let ids = page_number_group_ids(Some(story), parts);
    let pos: Vec<String> = ids.iter().map(|id| flat.iter().position(|x| x == id).map_or(-1, |p| p as i64).to_string()).collect();
    format!("[{}]", pos.join(","))
}

fn page_numbers(replay: &mut Replay<'_>, key: &str, pkg: &Package) {
    let ckey = format!("pn#{key}");
    let Ok(doc) = read_document(pkg, &ReadOptions { sources: true, ..ReadOptions::default() }) else {
        return;
    };
    let Some(store) = doc.sources.as_ref() else { return };
    for (path, story) in store.stories.iter() {
        let Some(rest) = path.strip_prefix("word/") else { continue };
        if !(rest.starts_with("header") || rest.starts_with("footer")) {
            continue;
        }
        replay.check(&ckey, &format!("ids:{path}"), &num_key_positions(story, &store.parts));
    }
}

fn corpus_file(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

#[test]
fn settings_xml_matches_the_ts() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/settings-xml.capture")).unwrap();
    let mut replay = Replay::new(&cap);
    let show = std::env::var("UO_PARITY_SHOW").ok();
    {
        let mut r = Runner { replay: &mut replay, show };
        for (di, (rel, parts)) in inputs().iter().enumerate() {
            for vname in VARIANTS {
                if vname != "asis" && di % 4 != 0 {
                    continue;
                }
                let vparts = variant(vname, parts);
                if vname != "asis" && vparts == *parts {
                    continue;
                }
                let base = format!("{rel}#{vname}");
                for (label, tw) in [("720", 720.0), ("567.4", 567.4), ("0", 0.0), ("-0.4", -0.4), ("1e+21", 1e21), ("NaN", f64::NAN)] {
                    r.pkg_op(&format!("{base}#tab:{label}"), &vparts, |p| write_default_tab_stop(p, tw));
                }
                let hyph = [
                    HyphenationSettings { auto: true, caps: true, zone: None, limit: 0.0 },
                    HyphenationSettings { auto: false, caps: false, zone: Some(360.0), limit: 3.0 },
                    HyphenationSettings { auto: true, caps: false, zone: Some(0.0), limit: 2.5 },
                    HyphenationSettings { auto: false, caps: true, zone: None, limit: 0.0 },
                    HyphenationSettings { auto: true, caps: true, zone: Some(-1.5), limit: 1e21 },
                    HyphenationSettings { auto: false, caps: true, zone: Some(f64::NAN), limit: f64::NAN },
                ];
                for (i, s) in hyph.iter().enumerate() {
                    r.pkg_op(&format!("{base}#hyph:{i}"), &vparts, |p| write_hyphenation_settings(p, s));
                }
                for on in [true, false] {
                    r.pkg_op(&format!("{base}#even:{on}"), &vparts, |p| write_even_and_odd_headers(p, on));
                    r.pkg_op(&format!("{base}#bg:{on}"), &vparts, |p| write_display_background_shape(p, on));
                }
                let mut ret = None;
                let key = format!("{base}#ensure");
                r.pkg_op(&key, &vparts, |p| {
                    ret = Some(ensure_note_parts(p)?);
                    Ok(())
                });
                if let Some(b) = ret {
                    r.replay.check(&key, "ret", &b.to_string());
                }
                if let Some((_, s)) = vparts.iter().find(|(n, _)| n == SETTINGS) {
                    let text = dec(s);
                    let set_fp = note_props(Some("pageBottom"), Some("decimal"), Some(1.0), Some("continuous"));
                    let set_ep = note_props(Some("docEnd"), None, None, None);
                    let zero = note_props(None, None, Some(0.0), None);
                    let cases: [(&str, NotePropsArg<'_>, NotePropsArg<'_>); 4] = [
                        ("keep", NotePropsArg::Unchanged, NotePropsArg::Unchanged),
                        ("set", NotePropsArg::Set(&set_fp), NotePropsArg::Set(&set_ep)),
                        ("remove", NotePropsArg::Remove, NotePropsArg::Remove),
                        ("mixed", NotePropsArg::Set(&zero), NotePropsArg::Remove),
                    ];
                    for (name, f, e) in cases {
                        let out = patch_settings_notes(&text, f, e);
                        r.put(&format!("{base}#patch:{name}"), &out);
                    }
                }
                let ids = vparts.iter().find(|(n, _)| n == RELS).map(|(_, d)| hf_ids(&dec(d))).unwrap_or_default();
                let sets: [(&str, Vec<String>); 4] = [
                    ("all", ids.clone()),
                    ("first", ids.iter().take(1).cloned().collect()),
                    ("last", ids.iter().rev().take(1).cloned().collect()),
                    ("none", vec!["rIdNope".to_owned()]),
                ];
                for (sn, set) in sets {
                    let key = format!("{base}#drop:{sn}");
                    r.replay.check(&key, "args", &set.join("|"));
                    let drop: HashSet<String> = set.into_iter().collect();
                    r.pkg_op(&key, &vparts, |p| drop_unused_hf_parts(p, &drop));
                }
                for (name, d) in &vparts {
                    if !is_hf_part(name) {
                        continue;
                    }
                    let mut c = 100.0;
                    let out = copy_hf_part_xml(&dec(d), &mut || {
                        c += 1.0;
                        c
                    });
                    r.put(&format!("{base}#copy:{name}"), &out);
                }
            }
        }
        for kind in ["header", "footer"] {
            for (i, inner) in ["", "<w:p/>", "<w:p><w:r><w:t>é😀&amp;</w:t></w:r></w:p>"].iter().enumerate() {
                r.put(&format!("hfpart#{kind}#{i}"), &hf_part_xml(kind, inner));
            }
        }
    }
    let root = repo_root(manifest);
    let synthetic = synthetic_inputs();
    let keys: Vec<String> = cap
        .records()
        .iter()
        .filter_map(|r| r.key.strip_prefix("pn#"))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for key in keys {
        if key.starts_with("quirk/") {
            let (_, parts) = synthetic.iter().find(|(k, _)| *k == key).unwrap();
            page_numbers(&mut replay, &key, &synthetic_package(parts));
        } else {
            let bytes = std::fs::read(corpus_file(&root, &key)).unwrap();
            let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
            page_numbers(&mut replay, &key, &pkg);
        }
    }
    let report = replay.finish();
    println!("settings-xml: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
    assert!(report.equal > 30_000);
}
