//! The small facets of frontier D against the frozen TS: replays `tests/parity/frontier_d.cap`
//! (written by `tests/parity_support/capture_frontier_d.mjs` over the shared corpus
//! `tests/parity/frontier_d_inputs.txt`):
//! - `odttf`: `obfuscate`/`deobfuscate` over keys as UTF-16 code units (astral, lone surrogates,
//!   signs, spaces, `0x`, short keys);
//! - `style`: `variant_of_style`;
//! - `lang`: `hyphenation_pattern_id`;
//! - `hyphen`: `points` and `hyphenate` for minimums 1..=3 over the words of every language, with
//!   the pattern files read from `data/hyphen` (the files the feature `hyphen-data` embeds);
//! - `catalog`: the lookups and file names.
//!
//! No accepted differences: everything here must be equal.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use uo_fonts::hyphen::parse_patterns;
use uo_fonts::{
    Hyphenator, VARIANTS, Variant, bundled, bundled_file_name, hyphenation_pattern_id, metric_substitute, odttf, variant_of_style,
};
use uo_testkit::{Capture, Replay, Stream};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

fn units(field: &str) -> Option<Vec<u16>> {
    if field == "~" {
        return None;
    }
    Some(field.split(',').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect())
}

fn text(field: &str) -> Option<String> {
    units(field).map(|u| String::from_utf16(&u).unwrap())
}

fn digest(bytes: &[u8]) -> String {
    uo_testkit::stream::Digest::of(bytes).short()
}

#[test]
fn frontier_d_matches_the_ts() {
    let cap = Capture::read(Path::new(MANIFEST).join("tests/parity/frontier_d.cap")).unwrap();
    let corpus = std::fs::read_to_string(Path::new(MANIFEST).join("tests/parity/frontier_d_inputs.txt")).unwrap();
    let lines: Vec<Vec<&str>> = corpus.lines().filter(|l| !l.is_empty()).map(|l| l.split('\t').collect()).collect();
    let mut replay = Replay::new(&cap);

    let data: Vec<u8> = (0..100u32).map(|i| ((i * 7 + 3) & 0xff) as u8).collect();
    let (mut odttf_n, mut style_n, mut lang_n, mut bundled_n) = (0, 0, 0, 0);
    for line in lines.iter().filter(|l| l[0] == "odttf") {
        let key = format!("odttf/{odttf_n}");
        odttf_n += 1;
        replay.check(&key, "in", line[1]);
        let k = units(line[1]).unwrap();
        replay.check(&key, "ob", &digest(&odttf::obfuscate_utf16(&data, &k)));
        let de = odttf::deobfuscate_utf16(&data, &k);
        let de = if matches!(de, std::borrow::Cow::Borrowed(_)) { "same".to_owned() } else { digest(&de) };
        replay.check(&key, "de", &de);
    }
    for line in lines.iter().filter(|l| l[0] == "style") {
        let key = format!("style/{style_n}");
        style_n += 1;
        replay.check(&key, "in", line[1]);
        let style = text(line[1]);
        let actual = variant_of_style(style.as_deref()).map_or("null", Variant::name);
        replay.check(&key, "variant", actual);
    }
    for line in lines.iter().filter(|l| l[0] == "lang") {
        let key = format!("lang/{lang_n}");
        lang_n += 1;
        replay.check(&key, "in", line[1]);
        let lang = text(line[1]);
        replay.check(&key, "id", hyphenation_pattern_id(lang.as_deref()).unwrap_or("undefined"));
    }

    // (the TS's ids: `en-us-word` is the Rust layout's Word data, WordCraft study D1)
    replay.check("hyphen", "ids", &uo_fonts::hyphen::TS_HYPHENATION_PATTERN_IDS.join(","));
    let mut words_total = 0;
    for id in uo_fonts::hyphen::TS_HYPHENATION_PATTERN_IDS {
        let file = std::fs::read_to_string(Path::new(MANIFEST).join(format!("data/hyphen/{id}.txt"))).unwrap();
        let patterns = parse_patterns(&file).unwrap();
        assert_eq!(patterns.id, id);
        let h = Hyphenator::new(&patterns);
        let words: Vec<&str> = lines.iter().filter(|l| l[0] == "word" && l[1] == id).map(|l| l[2]).collect();
        words_total += words.len();
        let key = format!("hyphen/{id}");
        replay.check(&key, "words", &words.len().to_string());
        replay.check(&key, "minimums", &format!("{},{}", patterns.left, patterns.right));
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
            replay.check(&key, &format!("chunk{chunk}"), &st.finish().short());
        }
    }

    for f in [
        "calibri",
        "cambria",
        "arial",
        "arial mt",
        "arial unicode ms",
        "helvetica",
        "times",
        "tinos",
        "courier",
        "cousine",
        "carlito",
        "caladea",
        "liberation sans",
        "liberation mono",
        "verdana",
        "Calibri",
        "",
    ] {
        bundled_n += 1;
        let key = format!("catalog/{f}");
        replay.check(&key, "substitute", metric_substitute(f).unwrap_or("null"));
        let b = bundled(f).map_or("null".to_owned(), |b| format!("{{\"family\":\"{}\",\"file\":\"{}\"}}", b.family, b.file));
        replay.check(&key, "bundled", &b);
    }
    for v in VARIANTS {
        replay.check(&format!("catalog/file/{}", v.name()), "name", &bundled_file_name("carlito", v));
    }

    let report = replay.finish();
    eprintln!(
        "frontier D small facets: {odttf_n} odttf keys, {style_n} styles, {lang_n} tags, {words_total} hyphenation words, {bundled_n} catalog names: {}",
        report.summary()
    );
    report.assert_clean();
    assert!(odttf_n > 200 && style_n > 100 && lang_n > 200 && words_total > 10_000);
}
