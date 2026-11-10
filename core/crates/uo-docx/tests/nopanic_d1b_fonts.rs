//! No-panic loops of the D1b font-embedding step: `read_docx_embedding` and `write_docx_embedding`
//! (both target rules) over seeded mutations of the 500 synthetic packages of
//! `tests/parity/font-embedding.inputs` (in the settings, font table, relationships and content
//! types: a character replaced, removed or duplicated, junk inserted — `$` patterns, `<`, `>`, `"`,
//! `/>`, tags of the step, non-ASCII, CRLF —, a tail cut), with hostile plans (family names made of
//! the same junk, long names, every variant, duplicates) and random GUID bytes; and the `odttf`
//! obfuscation with hostile keys (any string, non-ASCII, astral, short). A panic fails; the result
//! is not looked at except for what holds for any input: a write that succeeds leaves a package that
//! reads again, and the `.odttf` parts the step adds de-obfuscate to a face's bytes with a key
//! present in a font table (except where the TS drops the entry: a font table without a final
//! `</w:fonts>`, a `$` pattern that rewrote it). Then the work bounds: hostile texts of a few MB built to defeat the
//! one-pass rewrites (thousands of elements, starts without `>`, joins) finish in linear time.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::fonts_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fonts_support::{FaceSpec, PlanSpec, fonts, inputs, open_parts, plan_of};
use uo_docx::write::fonts::docx::{find_font_element_ci, font_entries, setting_on, strip_embed_tags};
use uo_docx::write::fonts::ooxml::{has_default_extension, read_rels_text, remove_elements};
use uo_docx::write::fonts::{TargetRule, read_docx_embedding_with, write_docx_embedding_with};
use uo_fonts::{FontEmbeddingSettings, VARIANTS};
use uo_testkit::generate::Rng;

const JUNK: &[&str] = &[
    "$",
    "$`",
    "$'",
    "$&",
    "$$",
    "<",
    ">",
    "\"",
    "/>",
    "</w:font>",
    "</W:FONT>",
    "<w:font w:name=\"",
    "<w:fonts",
    "</w:fonts>",
    "<w:settings",
    "<w:embedTrueTypeFonts",
    " w:val=\"0\"",
    "<w:embedRegular",
    "<Relationship",
    " Id=\"rId1\"",
    " Target=\"",
    "</Relationships>",
    "<Default",
    " Extension=\"odttf\"",
    "</Types>",
    "&amp;",
    "&",
    "é",
    "😀",
    "\r\n",
    "\u{3000}",
    "\u{feff}",
    "ſ",
    "\u{212a}",
    "x",
];

fn mutate(rng: &mut Rng, s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(chars.len() + 1);
        match rng.below(5) {
            0 if at < chars.len() => {
                chars.remove(at);
            }
            1 if at < chars.len() => {
                let c = chars[at];
                chars.insert(at, c);
            }
            2 => chars.truncate(at),
            _ => {
                let junk = JUNK[rng.below(JUNK.len())];
                for (k, c) in junk.chars().enumerate() {
                    chars.insert(at + k, c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

fn hostile_family(rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => "A".repeat(1 + rng.below(3000)),
        1 => String::new(),
        _ => (0..1 + rng.below(5)).map(|_| JUNK[rng.below(JUNK.len())]).collect(),
    }
}

#[test]
fn no_panic_on_mutated_packages_and_hostile_plans() {
    let ins = inputs();
    let fonts = fonts();
    let files: Vec<String> = {
        let mut v: Vec<String> = fonts.keys().cloned().collect();
        v.sort();
        v
    };
    let mut rng = Rng::new(0x0bad_f0e5);
    let (mut runs, mut ok, mut added, mut orphans) = (0usize, 0usize, 0usize, 0usize);
    for round in 0..6 {
        for (key, parts) in &ins.packages {
            let mut parts = parts.clone();
            for (name, bytes) in parts.iter_mut() {
                if name.ends_with(".odttf") || rng.below(2) == 0 {
                    continue;
                }
                let text = String::from_utf8_lossy(bytes).into_owned();
                *bytes = mutate(&mut rng, &text).into_bytes();
            }
            let base = &ins.plans[&format!("{key}#fonts")];
            let faces = (0..rng.below(6))
                .map(|_| FaceSpec {
                    family: if rng.below(2) == 0 {
                        hostile_family(&mut rng)
                    } else {
                        base.faces.first().map_or_else(String::new, |f| f.family.clone())
                    },
                    variant: VARIANTS[rng.below(4)],
                    file: files[rng.below(files.len())].clone(),
                    subsetted: rng.below(2) == 0,
                    panose: mutate(&mut rng, "020B0604020202020204"),
                })
                .collect();
            let spec = PlanSpec {
                settings: FontEmbeddingSettings {
                    embed: rng.below(4) != 0,
                    subset: rng.below(2) == 0,
                    skip_common_fonts: rng.below(2) == 0,
                },
                seed: rng.next_u32(),
                faces,
            };
            let plan = plan_of(&spec, &fonts);
            let rule = if round % 2 == 0 { TargetRule::Opc } else { TargetRule::Fonts09 };
            let mut pkg = open_parts(&parts);
            let _ = read_docx_embedding_with(&pkg, rule);
            let before: Vec<String> = pkg.part_names().map(str::to_owned).collect();
            let mut src = Rng::new(spec.seed);
            let mut guid = || {
                let mut b = [0u8; 16];
                for x in &mut b {
                    *x = src.next_u32() as u8;
                }
                b
            };
            let t = Instant::now();
            let res = write_docx_embedding_with(&mut pkg, &plan, &spec.settings, &mut guid, rule);
            assert!(t.elapsed() < Duration::from_secs(10), "{key}: {:?}", t.elapsed());
            runs += 1;
            if res.is_err() {
                continue;
            }
            ok += 1;
            assert!(read_docx_embedding_with(&pkg, rule).is_ok());
            // the parts the step added de-obfuscate to a face's bytes with a key written in a font table
            let tables: String =
                pkg.part_names().filter(|n| n.ends_with(".xml")).map(|n| pkg.part_text(n).unwrap().unwrap().into_owned()).collect();
            for name in pkg.part_names().filter(|n| n.ends_with(".odttf") && !before.iter().any(|b| b == n)) {
                let data = pkg.part(name).unwrap().unwrap();
                let found = tables.match_indices("w:fontKey=\"{").any(|(i, _)| {
                    let key = &tables[i + 11..(i + 49).min(tables.len())];
                    let plain = uo_fonts::odttf::deobfuscate(data, key);
                    plan.faces.iter().any(|f| f.bytes.as_ref() == plain.as_ref())
                });
                // without a `</w:fonts>` end the TS writes the part and drops the entry (its key)
                orphans += usize::from(!found);
                added += 1;
            }
        }
    }
    println!("{runs} runs, {ok} written, {added} font parts checked, {orphans} without their entry");
    assert!(ok > runs / 2 && added > 1000 && orphans * 4 < added);
}

#[test]
fn odttf_with_hostile_keys() {
    let mut rng = Rng::new(42);
    let data: Arc<[u8]> = Arc::from(rng.bytes(64));
    for _ in 0..20_000 {
        let key: String = (0..rng.below(50))
            .map(|_| match rng.below(6) {
                0 => char::from_u32(0x10000 + rng.next_u32() % 0xF_0000).unwrap_or('x'),
                1 => char::from_u32(rng.next_u32() % 0xD800).unwrap_or('x'),
                2 => ['{', '}', '-', ' ', '+', 'x', '\u{a0}'][rng.below(7)],
                _ => char::from(b"0123456789abcdefABCDEF"[rng.below(22)]),
            })
            .collect();
        let once = uo_fonts::odttf::obfuscate(&data, &key);
        assert_eq!(uo_fonts::odttf::obfuscate(&once, &key), data.as_ref());
        let _ = uo_fonts::odttf::deobfuscate(&once, &key);
    }
}

#[test]
fn hostile_texts_take_linear_time() {
    let n = 200_000;
    let cases: Vec<(&str, String)> = vec![
        ("removal of many elements", "<w:embedTrueTypeFonts/>".repeat(n)),
        ("removal of joined elements", "<w:embedTrue<w:embedTrueTypeFonts/>TypeFonts/>".repeat(n / 2)),
        ("starts without an end", "<w:embedTrueTypeFonts x".repeat(n)),
        ("font starts without >", "<w:font w:name=\"A\" ".repeat(n)),
        ("font starts, no close", "<w:font w:name=\"A\">".repeat(n)),
        ("names crossing >", "<w:font w:name=\"a>".repeat(n)),
        ("embed starts", "<w:embedRegular x".repeat(n)),
        ("defaults without >", "<Default x".repeat(n)),
        ("relationships without >", "<Relationship Id=\"".repeat(n)),
        ("ci starts", "<W:FONT w:name=\"über".repeat(n)),
    ];
    for (what, text) in &cases {
        let t = Instant::now();
        let _ = remove_elements(text, "w:embedTrueTypeFonts");
        let _ = setting_on(text, "embedTrueTypeFonts");
        let _ = font_entries(text);
        let _ = find_font_element_ci(text, "ÜBER");
        let _ = find_font_element_ci(text, &"A".repeat(500));
        let _ = strip_embed_tags(text);
        let _ = has_default_extension(text, "odttf");
        let _ = read_rels_text(text);
        let e = t.elapsed();
        println!("{what}: {} B in {e:?}", text.len());
        assert!(e < Duration::from_secs(20), "{what}: {e:?}");
    }
}
