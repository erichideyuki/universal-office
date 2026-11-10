//! Parity of frontier R2 (facet `table-styles`): replays `tests/parity/table-styles.capture`,
//! written by `js/capture-table-styles.mjs` from the frozen TS `table-styles.ts` (read side) and
//! `style-names-es.ts`: the built-in list, `CORE_TABLE_STYLES`, `localStyleId`, `themeHex` over the
//! theme colours x tint/shade combinations, `materializeStyleXml` of every built-in style,
//! `styleDefOf`, `styleIdByName` over the formatting of every corpus document, and
//! `tableStyleNameEs` over 285 names. Inputs (colour sets, names) in `tests/parity/table-styles.inputs`.
//!
//! Named differences of `tableStyleNameEs` (R2.md item 9): a name with a line terminator makes the
//! TS throw (`T`, the regex `exec` is `null`), and a name that is an `Object.prototype` member finds
//! a function in the plain object `TABLE_FIXED` (`X`); the Rust function returns `None` for both.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::{JsRecord, JsValue, Str};
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::read_doc_formatting;
use uo_docx::read::style_names_es::table_style_name_es;
use uo_docx::read::table_styles::{
    CORE_TABLE_STYLES, builtin_table_styles, local_style_id, materialize_style_xml, style_def_of, style_id_by_name, theme_hex,
};
use uo_opc::Package;
use uo_testkit::{Capture, Replay, Stream, repo_root};

const T: &str = "R2: a name with a line terminator makes the TS throw (exec is null), tableStyleNameEs is None here";
const X: &str = "R2: an Object.prototype member name finds a function in the TS TABLE_FIXED, tableStyleNameEs is None here";

const THEME_COLORS: [&str; 32] = [
    "text1",
    "background1",
    "text2",
    "background2",
    "dark1",
    "light1",
    "dark2",
    "light2",
    "hyperlink",
    "followedHyperlink",
    "dk1",
    "lt1",
    "dk2",
    "lt2",
    "accent1",
    "accent2",
    "accent3",
    "accent4",
    "accent5",
    "accent6",
    "hlink",
    "folHlink",
    "weird",
    "",
    "Accent1",
    "constructor",
    "__proto__",
    "toString",
    "hasOwnProperty",
    "valueOf",
    "isPrototypeOf",
    "toLocaleString",
];

type Combo = (Option<&'static str>, Option<&'static str>);
const COMBOS: [Combo; 19] = [
    (None, None),
    (None, Some("")),
    (Some(""), None),
    (Some("80"), None),
    (Some("BF"), None),
    (None, Some("40")),
    (Some("ff"), Some("00")),
    (Some("00"), Some("ff")),
    (Some("zz"), Some("1")),
    (Some("1"), Some("zz")),
    (Some(""), Some("")),
    (Some("A"), Some("B")),
    (Some("FFF"), None),
    (Some("8000000"), None),
    (None, Some("BF")),
    (Some("99"), Some("99")),
    (Some("33"), None),
    (None, Some("ff")),
    (Some("20"), Some("ff")),
];

const BY_NAME: [&str; 8] = ["Table Grid", "table grid", "NORMAL TABLE", "x", "Tabela com grade", "", "Light Grid", "İ"];

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

struct Inputs {
    colors: Vec<JsRecord<Str>>,
    names: Vec<String>,
    xmls: Vec<String>,
    locals: Vec<String>,
}

fn inputs() -> Inputs {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/table-styles.inputs")).unwrap();
    let mut i = Inputs { colors: Vec::new(), names: Vec::new(), xmls: Vec::new(), locals: Vec::new() };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split('\t');
        match f.next() {
            Some("colors") => {
                let rest: Vec<&str> = f.collect();
                let mut rec = JsRecord::new();
                for pair in rest.chunks(2) {
                    rec.insert(&unescape(pair[0]), unescape(pair.get(1).copied().unwrap_or("")).into());
                }
                i.colors.push(rec);
            }
            Some("name") => i.names.push(unescape(f.next().unwrap_or(""))),
            Some("xml") => i.xmls.push(unescape(f.next().unwrap_or(""))),
            Some("local") => i.locals.push(unescape(f.next().unwrap_or(""))),
            _ => {}
        }
    }
    i
}

fn shard_of(container: &str, shards: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in container.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h % shards
}

fn corpus_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            corpus_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "docx") {
            out.push(p);
        }
    }
}

fn strs(items: &[String]) -> String {
    let mut st = Stream::new();
    for x in items {
        st.str(x);
    }
    st.finish().short()
}

fn replay_functions(replay: &mut Replay<'_>) {
    let inp = inputs();
    let builtin = builtin_table_styles();
    {
        let mut st = Stream::new();
        st.u32(u32::try_from(builtin.len()).unwrap());
        for b in &builtin {
            st.str(&b.name);
            st.str(&b.local);
            st.str(&b.id);
            st.bool(b.modern);
            st.str(&b.xml);
        }
        replay.check("builtin", "list", &st.finish().short());
        replay.check("builtin", "count", &builtin.len().to_string());
    }
    {
        let mut st = Stream::new();
        for d in &CORE_TABLE_STYLES {
            st.str(d.name);
            st.str(d.id);
            st.str(d.local);
            st.str(d.xml);
        }
        for l in &inp.locals {
            st.str(&local_style_id(l));
        }
        replay.check("core", "core", &st.finish().short());
    }
    for (n, colors) in inp.colors.iter().enumerate() {
        let mut out = Vec::new();
        for tc in THEME_COLORS {
            for (tint, shade) in COMBOS {
                out.push(theme_hex(colors, tc, tint, shade));
            }
        }
        replay.check(&format!("themeHex#{n}"), "themeHex", &strs(&out));
        let mat: Vec<String> = builtin.iter().map(|b| materialize_style_xml(&b.xml, colors)).collect();
        replay.check(&format!("materialize#{n}"), "materialize", &strs(&mat));
    }
    {
        let mut s = Sink(Stream::new());
        let empty = JsRecord::new();
        for b in &builtin {
            let def = style_def_of(&materialize_style_xml(&b.xml, &empty)).unwrap();
            def.canon(&mut CanonWriter::new(&mut s));
        }
        for x in &inp.xmls {
            match style_def_of(x) {
                Ok(def) => def.canon(&mut CanonWriter::new(&mut s)),
                Err(_) => {
                    s.0.str("throws");
                }
            }
        }
        replay.check("styleDefOf", "defs", &s.0.finish().short());
    }
    for (i, name) in inp.names.iter().enumerate() {
        let key = format!("es#{i}");
        let actual = match table_style_name_es(name) {
            Some(s) => uo_jsnum::json::string(&s),
            None => "U".to_owned(),
        };
        match replay.capture().get(&key, "es") {
            Some("T") if actual == "U" => replay.accept(&key, "es", &actual, T),
            Some("X") if actual == "U" => replay.accept(&key, "es", &actual, X),
            _ => replay.check(&key, "es", &actual),
        };
    }
}

fn replay_docs(replay: &mut Replay<'_>, shard: u32, shards: u32) {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    corpus_files(&root.join("write/packages/docx-layout/test/oracle"), &mut files);
    corpus_files(&root.join("write/packages/docx-core/test/fixtures"), &mut files);
    let limits = uo_opc::Limits::DEFAULT;
    let options = uo_opc::package::OpenOptions::default();
    for file in files {
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if shard_of(&rel, shards) != shard {
            continue;
        }
        let bytes = std::fs::read(&file).unwrap();
        let pkg = Package::open(&bytes, &limits, &options).unwrap();
        let opts = ReadOptions::default();
        let mut ctx = ReadCtx::new(&pkg, &opts);
        let f = read_doc_formatting(&mut ctx).unwrap();
        let mut st = Stream::new();
        for n in BY_NAME {
            let r = style_id_by_name(Some(&f), n);
            st.bool(r.is_some());
            if let Some(r) = r {
                st.str(r);
            }
        }
        st.bool(style_id_by_name(None, "Table Grid").is_none());
        replay.check(&format!("{rel}#idByName"), "idByName", &st.finish().short());
    }
}

fn run(shard: u32, shards: u32) {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/table-styles.capture")).unwrap();
    let mine = |key: &str| {
        if shards == 0 {
            !key.ends_with("#idByName")
        } else {
            key.ends_with("#idByName") && shard_of(key.rsplit_once('#').unwrap().0, shards) == shard
        }
    };
    let cap = cap.subset(mine);
    let mut replay = Replay::new(&cap);
    if shards == 0 {
        replay_functions(&mut replay);
    } else {
        replay_docs(&mut replay, shard, shards);
    }
    let report = replay.finish();
    println!("table-styles parity shard {shard}/{shards}: {}", report.summary());
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn table_styles_parity_functions() {
    run(0, 0);
}

#[test]
fn table_styles_parity_docs_shard_0() {
    run(0, 2);
}

#[test]
fn table_styles_parity_docs_shard_1() {
    run(1, 2);
}
