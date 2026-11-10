//! Parity of frontier R6 (facet `sources-blocks`): the table, wrapper and story part of the source
//! capture, which does not need the full-document reader (R5). Replays
//! `tests/parity/sources-blocks.capture`, written by `js/capture-sources-blocks.mjs` from the frozen
//! TS (`readBlocksOf` over the body, `readHeaderFooterPart` over the headers and footers, with
//! sources) and compares exactly the digest of the whole store (`body`, `hf`) and of each member
//! (`<body|hf>.next`, `.paras`, `.runs`, `.tables`, `.trs`, `.tcs`, `.stories`).
//!
//! The Rust side here is a test-only port of the source-relevant control flow of `fulldoc.ts`
//! (`unwrapRaw`, `readTable`, `readBlocksOf`, `readHeaderFooterPart`) written on top of the R6
//! builders and R4c's `read_paragraph`, so that the builders run against the TS before R5 lands;
//! R5's real `fulldoc.rs` replaces it in `parity_sources.rs`. Paragraphs come from the real
//! reader; the tables and cells are shells that only carry their `src`.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-sources-blocks.mjs`.
//! Trace one input: `UO_PARITY_TRACE=<key> UO_PARITY_TRACE_DIR=<dir> cargo test -p uo-docx --test
//! parity_sources_blocks -- --nocapture` writes `rust-sources-blocks-<body|hf>.trace`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::support;

use std::path::{Path, PathBuf};

use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::error::DocxError;
use uo_docx::ids::SourceKey;
use uo_docx::js::JsValue;
use uo_docx::model::sources::{SourceStore, SrcText, StoryGroup, WrapSeg};
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::paragraph::ReadContext;
use uo_docx::read::sources::{cell_source, group_of, row_source, span, story_of, table_source};
use uo_opc::Package;
use uo_opc::xml::Element;
use uo_testkit::{Capture, Replay, Stream, repo_root};

type Result<T> = std::result::Result<T, DocxError>;

struct Wrapped<'d> {
    el: Element<'d>,
    pre: Vec<WrapSeg>,
    post: Vec<WrapSeg>,
}

fn wrap_seg(raw: SrcText) -> WrapSeg {
    WrapSeg { raw, ..WrapSeg::default() }
}

/// `unwrapRaw(el, name, skip, xml)`.
fn unwrap_raw<'d>(rc: &mut ReadContext<'_, '_>, part: u32, el: Element<'d>, name: &str, skip: &[&str]) -> Vec<Wrapped<'d>> {
    struct St<'d> {
        out: Vec<Wrapped<'d>>,
        pending: Vec<WrapSeg>,
    }
    fn walk<'d>(rc: &mut ReadContext<'_, '_>, part: u32, st: &mut St<'d>, container: Element<'d>, top: bool, name: &str, skip: &[&str]) {
        for c in container.child_elements() {
            if c.name() == name {
                st.out.push(Wrapped { el: c, pre: std::mem::take(&mut st.pending), post: Vec::new() });
                continue;
            }
            let skipped = if top { skip.contains(&c.name()) } else { container.name() == "w:customXml" && c.name().ends_with("Pr") };
            if skipped {
                continue;
            }
            let content = match c.name() {
                "w:sdt" => c.first_child("w:sdtContent"),
                "w:customXml" => Some(c),
                _ => None,
            };
            let cp = content.and_then(|x| x.positions());
            if let (Some(content), Some(cp)) = (content, cp)
                && cp.content_start != cp.end
            {
                let cs = c.positions().unwrap();
                let mut start = cp.content_start;
                if content == c {
                    for k in c.child_elements() {
                        if k.name().ends_with("Pr") {
                            start = k.positions().unwrap().end;
                        } else {
                            break;
                        }
                    }
                }
                let id = rc.ctx.next_wrap_id();
                let before = std::mem::take(&mut st.pending);
                let n0 = st.out.len();
                st.pending = before.clone();
                st.pending.push(WrapSeg { open: Some(id), ..wrap_seg(span(part, cs.start, start)) });
                walk(rc, part, st, content, false, name, skip);
                if st.out.len() > n0 {
                    let mut tail = std::mem::take(&mut st.pending);
                    tail.push(WrapSeg { close: Some(id), ..wrap_seg(span(part, cp.content_end, cs.end)) });
                    if let Some(last) = st.out.last_mut() {
                        last.post.extend(tail);
                    }
                } else {
                    st.pending = before;
                    st.pending.push(wrap_seg(span(part, cs.start, cs.end)));
                }
                continue;
            }
            if let Some(p) = c.positions() {
                st.pending.push(wrap_seg(span(part, p.start, p.end)));
            }
        }
    }
    let mut st = St { out: Vec::new(), pending: Vec::new() };
    walk(rc, part, &mut st, el, true, name, skip);
    if !st.pending.is_empty()
        && let Some(last) = st.out.last_mut()
    {
        last.post.extend(st.pending);
    }
    st.out
}

/// What `readBlocksOf` does with one child of a container (the loop body), as block ids.
fn read_blocks_child(rc: &mut ReadContext<'_, '_>, part: u32, c: Element<'_>, out: &mut Vec<SourceKey>) -> Result<()> {
    match c.name() {
        "w:p" => {
            let p = rc.read_paragraph(c)?;
            out.push(p.props.src.unwrap_or(SourceKey(0)));
        }
        "w:tbl" => out.push(read_table(rc, part, c)?),
        "w:sdt" => {
            if let Some(content) = c.first_child("w:sdtContent") {
                read_blocks_of(rc, part, content, out)?;
            }
        }
        "w:customXml" => read_blocks_of(rc, part, c, out)?,
        _ => {}
    }
    Ok(())
}

/// `readBlocksOf(container, ctx)`.
fn read_blocks_of(rc: &mut ReadContext<'_, '_>, part: u32, container: Element<'_>, out: &mut Vec<SourceKey>) -> Result<()> {
    for c in container.child_elements() {
        read_blocks_child(rc, part, c, out)?;
    }
    Ok(())
}

/// The source flow of `readTable`; returns the table's `src`.
fn read_table(rc: &mut ReadContext<'_, '_>, part: u32, tbl_el: Element<'_>) -> Result<SourceKey> {
    let mut rows = 0;
    for tr_w in unwrap_raw(rc, part, tbl_el, "w:tr", &["w:tblPr", "w:tblGrid"]) {
        let tr_el = tr_w.el;
        let mut cells = 0;
        for tc_wr in unwrap_raw(rc, part, tr_el, "w:tc", &["w:trPr", "w:tblPrEx"]) {
            let tc_el = tc_wr.el;
            let tc_pr_el = tc_el.first_child("w:tcPr");
            let kids: Vec<Element<'_>> = tc_el.child_elements().filter(|k| k.name() != "w:tcPr").collect();
            let groups = kids.iter().any(|k| k.name() != "w:p" && k.name() != "w:tbl").then(Vec::<StoryGroup>::new);
            let groups = match groups {
                Some(mut g) => {
                    for k in &kids {
                        let mut produced = Vec::new();
                        read_blocks_child(rc, part, *k, &mut produced)?;
                        let store = rc.ctx.sources.as_ref().unwrap();
                        g.push(group_of(store, part, *k, &produced));
                    }
                    Some(g)
                }
                None => {
                    read_blocks_of(rc, part, tc_el, &mut Vec::new())?;
                    None
                }
            };
            let store = rc.ctx.sources.as_mut().unwrap();
            let src = cell_source(store, part, tc_el, tc_pr_el, groups, tc_wr.pre, tc_wr.post);
            store.add_cell(&mut Cell::default(), src);
            cells += 1;
        }
        if cells > 0 {
            let store = rc.ctx.sources.as_mut().unwrap();
            let src = row_source(store, part, tr_el, tr_el.first_child("w:trPr"), tr_el.first_child("w:tblPrEx"), tr_w.pre, tr_w.post);
            store.add_row(&mut Row::default(), src);
            rows += 1;
        }
    }
    let _ = rows;
    let store = rc.ctx.sources.as_mut().unwrap();
    let src = table_source(store, part, tbl_el, tbl_el.first_child("w:tblPr"), tbl_el.first_child("w:tblGrid"));
    let mut table = Table::default();
    Ok(store.add_table(&mut table, src))
}

/// `readHeaderFooterPart(pkg, path, store)` (the sources part), `Ok(false)` when the part is absent.
fn read_header_footer_part(ctx: &mut ReadCtx<'_>, path: &str) -> Result<bool> {
    let Some(doc) = ctx.xml_part(path)? else { return Ok(false) };
    let part = ctx.source_part(path, &doc).expect("sources on");
    let name = path.rsplit('/').next().unwrap();
    let mut rc = ReadContext::new(ctx, &format!("word/_rels/{name}.rels"))?;
    rc.set_source_part(Some(part));
    let root = doc.root();
    let mut story = story_of(rc.ctx.sources.as_ref().unwrap(), part, root);
    for c in root.child_elements() {
        let mut produced = Vec::new();
        read_blocks_child(&mut rc, part, c, &mut produced)?;
        story.groups.push(group_of(rc.ctx.sources.as_ref().unwrap(), part, c, &produced));
    }
    rc.ctx.sources.as_mut().unwrap().stories.insert(path.into(), story);
    Ok(true)
}

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

fn digest(store: &SourceStore, trace: Option<(&Path, &str)>, write: impl FnOnce(&mut CanonWriter<'_>)) -> String {
    let mut s = Sink(if trace.is_some() { Stream::traced() } else { Stream::new() });
    let mut ignore = Sink(Stream::new());
    {
        let mut w = CanonWriter::new(&mut s).with_order(&mut ignore).with_parts(&store.parts);
        write(&mut w);
    }
    if let Some((dir, name)) = trace {
        std::fs::write(dir.join(format!("rust-sources-blocks-{name}.trace")), s.0.trace()).unwrap();
    }
    s.0.finish().short()
}

fn digests(store: &SourceStore, prefix: &str, trace: Option<&Path>, out: &mut Vec<(String, String)>) {
    out.push((prefix.to_owned(), digest(store, trace.map(|d| (d, prefix)), |w| store.canon(w))));
    let mut member = |name: &str, f: &dyn Fn(&mut CanonWriter<'_>)| out.push((format!("{prefix}.{name}"), digest(store, None, f)));
    member("next", &|w| w.num(f64::from(store.next)));
    member("paras", &|w| store.paras.canon(w));
    member("runs", &|w| store.runs.canon(w));
    member("tables", &|w| store.tables.canon(w));
    member("trs", &|w| store.trs.canon(w));
    member("tcs", &|w| store.tcs.canon(w));
    member("stories", &|w| store.stories.canon(w));
}

fn facets(pkg: &Package, trace: Option<&Path>) -> Result<Vec<(String, String)>> {
    let opts = ReadOptions { sources: true, ..ReadOptions::default() };
    let mut out = Vec::new();
    {
        let mut ctx = ReadCtx::new(pkg, &opts);
        let doc = ctx.xml_part("word/document.xml")?.ok_or(DocxError::Corrupt("no document"))?;
        let part = ctx.source_part("word/document.xml", &doc).expect("sources on");
        let n = {
            let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels")?;
            rc.set_source_part(Some(part));
            let body = doc.root().first_child("w:body").unwrap_or(doc.root());
            let mut ids = Vec::new();
            read_blocks_of(&mut rc, part, body, &mut ids)?;
            ids.len()
        };
        out.push(("body.n".to_owned(), n.to_string()));
        digests(ctx.sources.as_ref().unwrap(), "body", trace, &mut out);
    }
    let mut names: Vec<&str> = pkg.part_names().filter(|p| is_header_footer(p)).collect();
    names.sort_unstable();
    let keep: Vec<&str> =
        names.into_iter().filter(|p| pkg.part_text(p).ok().flatten().is_some_and(|t| !t.contains("txbxContent"))).collect();
    if !keep.is_empty() {
        let mut ctx = ReadCtx::new(pkg, &opts);
        for p in &keep {
            read_header_footer_part(&mut ctx, p)?;
        }
        out.push(("hf.parts".to_owned(), keep.len().to_string()));
        digests(ctx.sources.as_ref().unwrap(), "hf", trace, &mut out);
    }
    Ok(out)
}

/// `/^word\/(header|footer)\d*\.xml$/`.
fn is_header_footer(p: &str) -> bool {
    let Some(rest) = p.strip_prefix("word/") else { return false };
    let Some(rest) = rest.strip_prefix("header").or_else(|| rest.strip_prefix("footer")) else { return false };
    rest.strip_suffix(".xml").is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()))
}

fn replay_package(replay: &mut Replay<'_>, key: &str, pkg: &Package, trace: Option<&str>) {
    let dir = std::env::var("UO_PARITY_TRACE_DIR").unwrap_or_else(|_| ".".into());
    let traced = (trace == Some(key)).then(|| PathBuf::from(dir));
    match facets(pkg, traced.as_deref()) {
        Ok(list) => {
            for (facet, value) in list {
                replay.check(key, &facet, &value);
            }
        }
        Err(e) => {
            if std::env::var("UO_PARITY_ERRORS").is_ok() {
                println!("{key}: error: {e}");
            }
            replay.check(key, "error", "err");
        }
    }
}

type Input = (String, Vec<(String, Vec<u8>)>);

fn quirk_inputs() -> Vec<Input> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/sources.inputs")).unwrap();
    let mut out: Vec<Input> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(key) = line.strip_prefix("@input ") {
            out.push((key.to_owned(), Vec::new()));
        } else if let Some((path, data)) = line.split_once('\t') {
            out.last_mut().unwrap().1.push((path.to_owned(), unescape(data).into_bytes()));
        }
    }
    out
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

/// Replays the docx containers of shard `shard` of `shards` (or the synthetic inputs when `shards == 0`).
fn run(shard: u32, shards: u32) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/sources-blocks.capture")).unwrap();
    let mine = |key: &str| {
        let c = key.rsplit_once('#').unwrap().0;
        if shards == 0 { c.starts_with("quirk/") } else { !c.starts_with("quirk/") && shard_of(c, shards) == shard }
    };
    let cap = cap.subset(mine);
    let trace = std::env::var("UO_PARITY_TRACE").ok();
    let mut replay = Replay::new(&cap);
    let limits = uo_opc::Limits::DEFAULT;
    let options = uo_opc::package::OpenOptions::default();
    if shards == 0 {
        let inputs = quirk_inputs();
        assert_eq!(inputs.len(), 400, "the synthetic inputs changed: recapture");
        for (key, parts) in inputs {
            let pkg = Package::from_parts(parts, &limits, &options);
            replay_package(&mut replay, &format!("{key}#sources-blocks"), &pkg, trace.as_deref());
        }
    } else {
        let root = repo_root(manifest);
        let mut files = Vec::new();
        corpus_files(&root.join("write/packages/docx-layout/test/oracle"), &mut files);
        corpus_files(&root.join("write/packages/docx-core/test/fixtures"), &mut files);
        for file in files {
            let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if shard_of(&rel, shards) != shard {
                continue;
            }
            let bytes = std::fs::read(&file).unwrap();
            let pkg = Package::open(&bytes, &limits, &options).unwrap();
            replay_package(&mut replay, &format!("{rel}#sources-blocks"), &pkg, trace.as_deref());
        }
    }
    let report = replay.finish();
    println!("sources-blocks parity shard {shard}/{shards}: {}", report.summary());
    let mut by_facet: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for m in &report.mismatches {
        *by_facet.entry(m.facet.as_str()).or_default() += 1;
    }
    if !by_facet.is_empty() {
        println!("mismatches by facet: {by_facet:?}");
    }
    report.assert_clean();
    assert!(report.equal > 0);
}

#[test]
fn sources_blocks_parity_shard_0() {
    run(0, 4);
}

#[test]
fn sources_blocks_parity_shard_1() {
    run(1, 4);
}

#[test]
fn sources_blocks_parity_shard_2() {
    run(2, 4);
}

#[test]
fn sources_blocks_parity_shard_3() {
    run(3, 4);
}

#[test]
fn sources_blocks_parity_quirks() {
    run(0, 0);
}
