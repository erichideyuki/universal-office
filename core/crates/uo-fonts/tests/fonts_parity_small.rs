//! Stage-4 parity of the non-font facets of uo-fonts: the keys of `tests/parity/fonts.cap` that are
//! not fonts (`odttf/`, `style/`, `lang/`, `variant/`, `catalog`, `hyphen`, `embed/`), written by
//! `tests/parity_support/capture_fonts_small.mjs` from the frozen TS:
//! - `odttf`, `style`, `lang`: the shared corpus of frontier D read again (295 odttf keys as UTF-16
//!   code units, 119 style names, 266 language tags), through `obfuscate_utf16`/`deobfuscate_utf16`,
//!   `variant_of_style` and `hyphenation_pattern_id`;
//! - `catalog`: the whole tables (`DEFAULT_FAMILY`, `METRIC_SUBSTITUTES`, `BUNDLED`,
//!   `PROBE_FAMILIES`, every file name) in insertion order;
//! - `loadids`: which pattern sets `loadHyphenation` loads for 107 lists of the language tags (a fresh TS module
//!   instance per list; the 3 lists with `constructor`, `toString` or `__proto__` are rejected by the TS: G9);
//! - `hyphen`: the pattern fields of the seven data files (id, minimums, every token), and the points
//!   and `hyphenate(word, l, r)` for l, r in 1..=3 of every language over the letter runs of the
//!   2,742 oracle documents (1,567 words), their upper-case and capitalized forms and every
//!   exception word;
//! - `embed`: 700 seeded scenarios of the embedding plan against a stub resolver (own file or not,
//!   document faces, already-embedded families, restricted, bitmap-only, CFF and malformed-cmap
//!   fonts, both apps, all settings): the sequence of `documentFace` calls the TS made (against
//!   `embedding_requests` too: S13), the `loadFace` calls and the plan (faces, subsetted flags,
//!   info, skipped) or its rejection.
//!
//! Accepted by name: only G9 (above); everything else must be equal. The NFC property of the pattern data is
//! checked in the TS (`nfc=true` in the capture) and holds for the Rust data because its tokens
//! are the TS tokens (`fields`).
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod fonts_parity_support;
mod frontier_d_support;
mod gpos_support;

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use fonts_parity_support::corpus::embed_pool;
use fonts_parity_support::facets::write_info;
use uo_fonts::catalog::BUNDLED;
use uo_fonts::hyphen::{TS_HYPHENATION_PATTERN_IDS, parse_patterns};
use uo_fonts::{
    DEFAULT_FAMILY, EmbeddingPolicy, FaceRequest, FontLimits, FontUsage, FxHashSet, HyphenationRegistry, Hyphenator, METRIC_SUBSTITUTES,
    POWERPOINT_COMMON_FONTS, PROBE_FAMILIES, SkipReason, VARIANTS, Variant, WORD_COMMON_FONTS, bundled_file_name, embedding_requests,
    hyphenation_pattern_id, is_bold_variant, is_italic_variant, odttf, pattern_ids_to_load, plan_font_embedding, variant_of,
    variant_of_style,
};
use uo_testkit::stream::Digest;
use uo_testkit::{Capture, Replay, Stream, repo_root};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
const LANGS: [&str; 7] = ["pt", "en-us", "es", "fr", "de-1996", "it", "nl"];

fn units(field: &str) -> Option<Vec<u16>> {
    if field == "~" {
        return None;
    }
    Some(field.split(',').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect())
}

fn text(field: &str) -> Option<String> {
    units(field).map(|u| String::from_utf16(&u).unwrap())
}

fn done(s: Stream) -> String {
    let items = s.items();
    format!("{items}:{}", s.finish().short())
}

fn sha(bytes: &[u8]) -> String {
    Digest::of(bytes).short()
}

fn variant_of_letter(c: char) -> Variant {
    match c {
        'r' => Variant::Regular,
        'b' => Variant::Bold,
        'i' => Variant::Italic,
        'z' => Variant::BoldItalic,
        other => panic!("variant {other}"),
    }
}

fn reason(r: SkipReason) -> &'static str {
    match r {
        SkipReason::Common => "common",
        SkipReason::License => "license",
        SkipReason::Unavailable => "unavailable",
        SkipReason::AlreadyEmbedded => "already-embedded",
    }
}

fn small_facets(replay: &mut Replay<'_>) -> [usize; 6] {
    let parity = Path::new(MANIFEST).join("tests/parity");
    let corpus = std::fs::read_to_string(parity.join("frontier_d_inputs.txt")).unwrap();
    let lines: Vec<Vec<&str>> = corpus.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').collect()).collect();
    let data: Vec<u8> = (0..100u32).map(|i| ((i * 7 + 3) & 0xff) as u8).collect();
    let (mut n_odttf, mut n_style, mut n_lang) = (0, 0, 0);
    let (mut n_load, mut n_load_rejected) = (0, 0);
    for l in lines.iter().filter(|l| l[0] == "odttf") {
        let key = format!("odttf/{n_odttf}");
        n_odttf += 1;
        replay.check(&key, "in", l[1]);
        let k = units(l[1]).unwrap();
        replay.check(&key, "ob", &sha(&odttf::obfuscate_utf16(&data, &k)));
        let de = odttf::deobfuscate_utf16(&data, &k);
        let de = if matches!(de, std::borrow::Cow::Borrowed(_)) { "same".to_owned() } else { sha(&de) };
        replay.check(&key, "de", &de);
    }
    for l in lines.iter().filter(|l| l[0] == "style") {
        let key = format!("style/{n_style}");
        n_style += 1;
        replay.check(&key, "in", l[1]);
        let style = text(l[1]);
        replay.check(&key, "variant", variant_of_style(style.as_deref()).map_or("null", Variant::name));
    }
    for l in lines.iter().filter(|l| l[0] == "lang") {
        let key = format!("lang/{n_lang}");
        n_lang += 1;
        replay.check(&key, "in", l[1]);
        let lang = text(l[1]);
        replay.check(&key, "id", hyphenation_pattern_id(lang.as_deref()).unwrap_or("undefined"));
    }
    {
        let tags: Vec<Option<String>> = lines.iter().filter(|l| l[0] == "lang").map(|l| text(l[1])).collect();
        let (mut from, mut k) = (0, 0);
        while from < tags.len() {
            let size = 1 + (k % 4);
            let list = &tags[from..(from + size).min(tags.len())];
            from += size;
            let key = format!("loadids/{k}");
            k += 1;
            replay.check(
                &key,
                "in",
                &list
                    .iter()
                    .map(|t| t.as_ref().map_or("~".to_owned(), |t| t.encode_utf16().count().to_string()))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            let registry = HyphenationRegistry::new();
            let loaded = pattern_ids_to_load(list.iter().map(Option::as_deref), &registry);
            let ids = TS_HYPHENATION_PATTERN_IDS.iter().filter(|id| loaded.contains(id)).copied().collect::<Vec<_>>().join(",");
            if replay.capture().get(&key, "ids") == Some("R") {
                n_load_rejected += 1;
                replay.accept(&key, "ids", &ids, "G9: the TS indexes a plain object (LOADERS['constructor'] is Object.prototype.constructor) and rejects; own keys only here");
            } else {
                replay.check(&key, "ids", &ids);
            }
            n_load += 1;
        }
    }
    for (a, b) in [(false, false), (true, false), (false, true), (true, true)] {
        replay.check(&format!("variant/{a}{b}"), "of", variant_of(a, b).name());
    }
    for v in VARIANTS {
        replay.check(&format!("variant/{}", v.name()), "flags", &format!("{},{}", is_bold_variant(v), is_italic_variant(v)));
    }

    let mut s = Stream::new();
    s.str(DEFAULT_FAMILY);
    replay.check("catalog", "default", &done(s));
    let mut s = Stream::new();
    for (k, v) in METRIC_SUBSTITUTES {
        s.str(k).str(v);
    }
    replay.check("catalog", "substitutes", &done(s));
    let mut s = Stream::new();
    for (k, v) in BUNDLED {
        s.str(k).str(v.family).str(v.file);
    }
    replay.check("catalog", "bundled", &done(s));
    let mut s = Stream::new();
    for f in PROBE_FAMILIES {
        s.str(f);
    }
    replay.check("catalog", "probe", &done(s));
    let mut s = Stream::new();
    for (_, v) in BUNDLED {
        for variant in VARIANTS {
            s.str(&bundled_file_name(v.file, variant));
        }
    }
    replay.check("catalog", "files", &done(s));

    let oracle = std::fs::read_to_string(parity.join("fonts_oracle_words.txt")).unwrap();
    let oracle: Vec<&str> = oracle.lines().filter(|l| !l.is_empty()).collect();
    replay.check("hyphen", "ids", &TS_HYPHENATION_PATTERN_IDS.join(","));
    let mut hyphen_words = 0;
    for id in LANGS {
        let file = std::fs::read_to_string(Path::new(MANIFEST).join(format!("data/hyphen/{id}.txt"))).unwrap();
        let patterns = parse_patterns(&file).unwrap();
        let key = format!("hyphen/{id}");
        let mut f = Stream::new();
        f.str(patterns.id).u32(patterns.left).u32(patterns.right);
        let pats: Vec<&str> = tokens(patterns.patterns);
        let excs: Vec<&str> = tokens(patterns.exceptions);
        f.u64(pats.len() as u64);
        for t in &pats {
            f.str(t);
        }
        f.u64(excs.len() as u64);
        for t in &excs {
            f.str(t);
        }
        replay.check(&key, "fields", &done(f));
        assert_eq!(replay.capture().get(&key, "nfc"), Some("true"), "the TS data is not NFC");
        replay.skip(&key, "nfc");
        let h = Hyphenator::new(&patterns);
        let mut words: Vec<String> = oracle.iter().map(|w| (*w).to_owned()).collect();
        for w in &oracle {
            words.push(w.to_uppercase());
            let mut chars = w.chars();
            words.push(chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str()));
        }
        for e in &excs {
            words.push(e.replace('-', ""));
        }
        replay.check(&key, "words", &words.len().to_string());
        hyphen_words += words.len();
        for (chunk, group) in words.chunks(250).enumerate() {
            let mut st = Stream::new();
            for w in group {
                st.tag(b'w');
                let points = h.points(w);
                st.u32(points.len() as u32);
                for p in &points {
                    st.u32(*p);
                }
                for l in 1..=3 {
                    for r in 1..=3 {
                        let hy = h.hyphenate(w, l, r);
                        st.u32(hy.len() as u32);
                        for p in &hy {
                            st.u32(*p);
                        }
                    }
                }
            }
            replay.check(&key, &format!("chunk{chunk}"), &done(st));
        }
    }
    [n_odttf, n_style, n_lang, hyphen_words, n_load, n_load_rejected]
}

struct Resolved {
    family: String,
    source: String,
    have: Vec<(Variant, String)>,
    load: Vec<(Variant, String)>,
}

fn variant_map(field: &str) -> Vec<(Variant, String)> {
    field
        .split('+')
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (v, name) = item.split_once('=').unwrap();
            (variant_of_letter(v.chars().next().unwrap()), name.to_owned())
        })
        .collect()
}

fn embed_facets(replay: &mut Replay<'_>) -> (usize, usize, usize, std::collections::BTreeMap<&'static str, usize>) {
    let root = repo_root(MANIFEST);
    let pool: Vec<(&'static str, Arc<[u8]>)> = embed_pool(&root).into_iter().map(|(n, b)| (n, Arc::from(b))).collect();
    for (name, bytes) in &pool {
        replay.check(&format!("embed/pool/{name}"), "in", &format!("{}:{}", bytes.len(), sha(bytes)));
    }
    let bytes_of = |name: &str| pool.iter().find(|(n, _)| *n == name).map(|(_, b)| b.clone()).unwrap();
    let scenarios = std::fs::read_to_string(Path::new(MANIFEST).join("tests/parity/fonts_embed_scenarios.txt")).unwrap();
    let (mut count, mut faces_total, mut rejected) = (0, 0, 0);
    let mut tally: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    for line in scenarios.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (id, app, bits, already_field, usage_field, resolver_field, doc_field) = (f[1], f[2], f[3], f[4], f[5], f[6], f[7]);
        let key = format!("embed/{id}");
        let usage_owned: Vec<(String, Vec<Variant>, String)> = usage_field
            .split(';')
            .map(|item| {
                let mut p = item.split('|');
                let (family, vs, hex) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
                let text: String = hex
                    .split(',')
                    .filter(|h| !h.is_empty())
                    .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
                    .collect();
                (family.to_owned(), vs.chars().map(variant_of_letter).collect(), text)
            })
            .collect();
        let usage: Vec<FontUsage<'_>> = usage_owned.iter().map(|(fam, vs, t)| FontUsage { family: fam, variants: vs, text: t }).collect();
        let table: Vec<Resolved> = resolver_field
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|item| {
                let p: Vec<&str> = item.split('|').collect();
                Resolved { family: p[0].to_owned(), source: p[1].to_owned(), have: variant_map(p[2]), load: variant_map(p[3]) }
            })
            .collect();
        let doc: Vec<(String, Variant, String)> = doc_field
            .split('+')
            .filter(|s| !s.is_empty())
            .map(|item| {
                let (family, rest) = item.split_once('|').unwrap();
                let (v, name) = rest.split_once('=').unwrap();
                (family.to_owned(), variant_of_letter(v.chars().next().unwrap()), name.to_owned())
            })
            .collect();
        let already: FxHashSet<String> = already_field.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect();
        let slides = app == "slides";
        let bits = bits.as_bytes();
        let policy = EmbeddingPolicy {
            embed: bits[0] == b'1',
            subset: bits[1] == b'1',
            skip_common_fonts: slides || bits[2] == b'1',
            common_fonts: if slides { &POWERPOINT_COMMON_FONTS } else { &WORD_COMMON_FONTS },
            all_variants: slides,
        };
        let doc_calls = RefCell::new(Stream::new());
        let load_calls = RefCell::new(Stream::new());
        let seen: RefCell<Vec<FaceRequest>> = RefCell::new(Vec::new());
        let stub = |req: &FaceRequest| -> Option<Arc<[u8]>> {
            seen.borrow_mut().push(req.clone());
            doc_calls.borrow_mut().str(&req.family).str(req.variant.name());
            if let Some((_, _, name)) = doc.iter().find(|(fam, v, _)| *fam == req.family && *v == req.variant) {
                return Some(bytes_of(name));
            }
            let key = req.family.to_lowercase();
            let r = table.iter().find(|r| r.family.to_lowercase() == key)?;
            if !matches!(r.source.as_str(), "host" | "local-access" | "bundled") {
                return None;
            }
            if let Some((_, name)) = r.have.iter().find(|(v, _)| *v == req.variant) {
                return Some(bytes_of(name));
            }
            load_calls.borrow_mut().str(&r.family).str(req.variant.name());
            r.load.iter().find(|(v, _)| *v == req.variant).map(|(_, name)| bytes_of(name))
        };
        let plan = plan_font_embedding(&usage, &policy, &already, &stub, &FontLimits::NATIVE);
        let requests = embedding_requests(&usage, &policy, &already);
        if plan.is_ok() {
            assert_eq!(requests, *seen.borrow(), "{key}: embedding_requests is not the sequence of calls the plan makes");
        } else {
            assert_eq!(requests[..seen.borrow().len()], *seen.borrow(), "{key}: a rejected plan asked for faces outside the requests");
        }
        let mut out = Stream::new();
        match &plan {
            Ok(p) => {
                out.tag(b'P').u64(p.faces.len() as u64);
                faces_total += p.faces.len();
                for face in &p.faces {
                    *tally.entry(if face.subsetted { "face subsetted" } else { "face whole" }).or_default() += 1;
                    out.str(&face.family).str(face.variant.name()).digest_of(&face.bytes).bool(face.subsetted);
                    write_info(&mut out, &face.info);
                }
                out.u64(p.skipped.len() as u64);
                for (family, r) in &p.skipped {
                    *tally.entry(reason(*r)).or_default() += 1;
                    out.str(family).str(reason(*r));
                }
            }
            Err(_) => {
                out.tag(b'R');
                rejected += 1;
            }
        }
        replay.check(&key, "in", &format!("{app},{}", String::from_utf8_lossy(bits)));
        let n_doc = seen.borrow().len();
        replay.check(&key, "doc", &format!("{n_doc}:{}", doc_calls.into_inner().finish().short()));
        let n_load = load_calls.borrow().items() / 2;
        replay.check(&key, "load", &format!("{n_load}:{}", load_calls.into_inner().finish().short()));
        replay.check(&key, "plan", &done(out));
        count += 1;
    }
    (count, faces_total, rejected, tally)
}

#[test]
fn small_facets_match_the_ts() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/fonts.cap")).unwrap();
    let mut replay = Replay::new(&cap);
    let [n_odttf, n_style, n_lang, hyphen_words, n_load, n_load_rejected] = small_facets(&mut replay);
    let (scenarios, faces, rejected, tally) = embed_facets(&mut replay);
    let mut report = replay.finish();
    let small = |k: &str| {
        !(k.starts_with("file/") || k.starts_with("embedded/") || k.starts_with("gen/") || k.starts_with("ttc/") || k.starts_with("mut/"))
    };
    report.missing.retain(|(k, _)| small(k));
    eprintln!(
        "fonts parity [small]: {n_odttf} odttf keys, {n_style} styles, {n_lang} tags, {hyphen_words} hyphenation words (7 languages), {n_load} `loadHyphenation` lists ({n_load_rejected} rejected by the TS: G9), {scenarios} embedding scenarios ({faces} faces planned, {rejected} rejected, {tally:?}): {}",
        report.summary()
    );
    report.assert_clean();
    assert!(n_odttf > 200 && n_style > 100 && n_lang > 200 && hyphen_words > 10_000 && scenarios == 772);
}

/// The tokens of a pattern or exception list: the data files separate them by spaces and line breaks.
fn tokens(text: &str) -> Vec<&str> {
    text.split([' ', '\n', '\t']).filter(|t| !t.is_empty()).collect()
}
