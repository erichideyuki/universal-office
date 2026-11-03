//! The catalog of bundled open fonts, the metric-compatible substitutes and the families worth
//! probing for. The tables are generated once from `shared/fonts/src/catalog.ts`
//! (`tools/gen_catalog.mjs`, G9) and checked in as `catalog_tables.rs`. Lookups use own keys only:
//! the TS indexed plain objects, so `METRIC_SUBSTITUTES['constructor']` was a function there
//! (accepted by name, G9).

use crate::variant::Variant;

/// A bundled open family: its name and the file stem (`<file>-<variant>.ttf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledFamily {
    pub family: &'static str,
    pub file: &'static str,
}

include!("catalog_tables.rs");

/// The bundled family for a lowercase family key.
pub fn bundled(family: &str) -> Option<BundledFamily> {
    BUNDLED.iter().find(|(key, _)| *key == family).map(|(_, value)| *value)
}

/// `<file>-<variant>.ttf` with the variant in lower case (`bolditalic`).
pub fn bundled_file_name(file: &str, variant: Variant) -> String {
    let variant = match variant {
        Variant::Regular => "regular",
        Variant::Bold => "bold",
        Variant::Italic => "italic",
        Variant::BoldItalic => "bolditalic",
    };
    format!("{file}-{variant}.ttf")
}

/// The metric-compatible substitute of a lowercase family key (Calibri to Carlito, ...).
pub fn metric_substitute(family: &str) -> Option<&'static str> {
    METRIC_SUBSTITUTES.iter().find(|(key, _)| *key == family).map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use uo_alloc::assert_no_leak;

    #[test]
    fn lookups_use_own_keys_only() {
        assert_eq!(bundled("carlito"), Some(BundledFamily { family: "Carlito", file: "carlito" }));
        assert_eq!(bundled("liberation sans").map(|b| b.file), Some("liberationsans"));
        assert_eq!(bundled("Carlito"), None);
        assert_eq!(bundled("constructor"), None);
        assert_eq!(bundled("__proto__"), None);
        assert_eq!(metric_substitute("calibri"), Some("Carlito"));
        assert_eq!(metric_substitute("arial unicode ms"), Some("Liberation Sans"));
        assert_eq!(metric_substitute("toString"), None);
        assert_eq!(metric_substitute("constructor"), None);
        assert_eq!(metric_substitute(""), None);
        assert_eq!(DEFAULT_FAMILY, "Liberation Sans");
    }

    #[test]
    fn file_names_use_the_lower_case_variant() {
        assert_eq!(bundled_file_name("carlito", Variant::Regular), "carlito-regular.ttf");
        assert_eq!(bundled_file_name("carlito", Variant::Bold), "carlito-bold.ttf");
        assert_eq!(bundled_file_name("carlito", Variant::Italic), "carlito-italic.ttf");
        assert_eq!(bundled_file_name("carlito", Variant::BoldItalic), "carlito-bolditalic.ttf");
    }

    #[test]
    fn tables_keep_the_ts_order_and_duplicates() {
        assert_eq!(BUNDLED.len(), 5);
        assert_eq!(BUNDLED.first().map(|b| b.0), Some("liberation sans"));
        assert_eq!(METRIC_SUBSTITUTES.len(), 13);
        assert_eq!(METRIC_SUBSTITUTES.first(), Some(&("calibri", "Carlito")));
        assert_eq!(METRIC_SUBSTITUTES.last(), Some(&("cousine", "Liberation Mono")));
        assert_eq!(PROBE_FAMILIES.first(), Some(&"Arial"));
        assert_eq!(PROBE_FAMILIES.last(), Some(&"Ubuntu Mono"));
        assert_eq!(PROBE_FAMILIES.iter().filter(|f| **f == "Rockwell").count(), 2);
        for (key, _) in BUNDLED {
            assert_eq!(*key, crate::js::js_to_lowercase(key));
        }
    }

    fn quoted(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = line;
        while let Some(i) = rest.find('\'') {
            let tail = &rest[i + 1..];
            let Some(j) = tail.find('\'') else { break };
            out.push(tail[..j].to_owned());
            rest = &tail[j + 1..];
        }
        out
    }

    fn block<'a>(source: &'a str, name: &str, open: char, close: &str) -> &'a str {
        let at = source.find(&format!("const {name}")).unwrap();
        let tail = &source[at..];
        let from = tail.find(&format!("= {open}")).unwrap() + 2;
        let tail = &tail[from..];
        &tail[..tail.find(close).unwrap()]
    }

    fn without_comments(block: &str) -> String {
        block.lines().map(|l| l.find("//").map_or(l, |i| &l[..i])).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn the_tables_match_the_ts_source_while_it_exists() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/src/catalog.ts");
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("catalog.ts not found; the re-parse test needs the TS source");
            return;
        };
        let source = source.replace("\r\n", "\n");

        let default = quoted(source.lines().find(|l| l.contains("DEFAULT_FAMILY =")).unwrap());
        assert_eq!(default, [DEFAULT_FAMILY]);

        let bundled_block = without_comments(block(&source, "BUNDLED", '{', "\n};"));
        let mut expected_bundled = Vec::new();
        for line in bundled_block.lines().filter(|l| l.contains("family:")) {
            let q = quoted(line);
            let key = match crate::js::js_trim(line).strip_prefix('\'') {
                Some(_) => q[0].clone(),
                None => crate::js::js_trim(line).split(':').next().unwrap().to_owned(),
            };
            let values = &q[q.len() - 2..];
            expected_bundled.push((key, values[0].clone(), values[1].clone()));
        }
        let actual: Vec<(String, String, String)> =
            BUNDLED.iter().map(|(k, b)| ((*k).to_owned(), b.family.to_owned(), b.file.to_owned())).collect();
        assert_eq!(actual, expected_bundled);

        let subs_block = without_comments(block(&source, "METRIC_SUBSTITUTES", '{', "\n};"));
        let mut expected_subs = Vec::new();
        for line in subs_block.lines().filter(|l| l.contains(':')) {
            let q = quoted(line);
            let key = match crate::js::js_trim(line).strip_prefix('\'') {
                Some(_) => q[0].clone(),
                None => crate::js::js_trim(line).split(':').next().unwrap().to_owned(),
            };
            expected_subs.push((key, q[q.len() - 1].clone()));
        }
        let actual: Vec<(String, String)> = METRIC_SUBSTITUTES.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        assert_eq!(actual, expected_subs);

        let probes_block = without_comments(block(&source, "PROBE_FAMILIES", '[', "\n];"));
        let expected_probes: Vec<String> = probes_block.lines().flat_map(quoted).collect();
        let actual: Vec<String> = PROBE_FAMILIES.iter().map(|f| (*f).to_owned()).collect();
        assert_eq!(actual, expected_probes);
    }

    #[test]
    fn m1_no_leak() {
        assert_no_leak(|| (bundled("carlito"), bundled("nope"), metric_substitute("calibri"), metric_substitute("nope")));
        assert_no_leak(|| bundled_file_name("carlito", Variant::BoldItalic));
    }
}
