//! Parity of the paste-tree boundary and the HTML rules of D2b-F8 (record
//! `core/docs/modules/uo-docx-d2b/F8.md`): replays `tests/parity/d2b-f8-html.capture`, written in
//! headless Chromium by `js/capture-d2b-f8-html.mjs`. Per input the shell encoder's payload (`pt`)
//! is decoded here; then `n`/`depth` (the tree), `hp` (`html_to_paras` against the frozen
//! `htmlToParas` over the browser's own DOM, the run props with their key order), `gate` (the
//! `pasteData` gates over the raw HTML) and `tb` (`tables_from_html` against the frozen
//! `tablesFromHtml`).
//!
//! Recapture: `UO_TS_ROOT=<frozen checkout> UO_CHROMIUM=<chrome-headless-shell.exe> node
//! core/crates/uo-docx/js/capture-d2b-f8-html.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2b_f8_support;

use d2b_f8_support::hx;
use uo_docx::ids::IdAlloc;
use uo_docx::js::JsObject;
use uo_docx::model::run::RunProps;
use uo_docx::session::ops::clipboard::cells::tables_from_html;
use uo_docx::session::ops::clipboard::html::{TitleStyle, html_to_paras};
use uo_docx::session::ops::clipboard::rx;
use uo_docx::session::ops::clipboard::tree::{PasteLimits, PasteTree, decode_payload};
use uo_docx::word::obj;
use uo_jsnum::format::to_js_string;
use uo_testkit::{Capture, Replay, repo_root};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let Some(v) = B64.iter().position(|x| *x == c) else { continue };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

pub fn b64_encode(b: &[u8]) -> String {
    let mut out = String::new();
    for ch in b.chunks(3) {
        let n = (u32::from(ch[0]) << 16) | (u32::from(*ch.get(1).unwrap_or(&0)) << 8) | u32::from(*ch.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= ch.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn depth_of(t: &PasteTree) -> usize {
    let mut max = 0;
    let mut ends: Vec<usize> = Vec::new();
    for (i, n) in t.nodes.iter().enumerate() {
        while ends.last().is_some_and(|&e| e <= i) {
            ends.pop();
        }
        ends.push(i + n.size as usize);
        max = max.max(ends.len());
    }
    max
}

/// `Object.keys(p).join(',') + '#' + JSON.stringify(p)`.
pub fn props_text(p: &RunProps) -> String {
    let keys = RunProps::keys();
    let names: Vec<&str> = obj::key_indices(p).iter().map(|i| keys[usize::from(*i)]).collect();
    format!("{}#{}", names.join(","), obj::json_of(Some(p)).unwrap_or_default())
}

fn num(v: Option<f64>) -> String {
    v.map(to_js_string).unwrap_or_default()
}

#[test]
fn d2b_f8_html_rules_match_the_frozen_ts() {
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let cap = Capture::read(root.join("core/crates/uo-docx/tests/parity/d2b-f8-html.capture")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut inputs = 0usize;
    let mut deepest = 0usize;
    for j in 0.. {
        let key = format!("h#{j}");
        let Some(pt) = cap.get(&key, "pt") else { break };
        replay.skip(&key, "pt");
        let bytes = b64_decode(pt);
        let h = decode_payload(&bytes, &PasteLimits::default()).unwrap_or_else(|e| panic!("{key}: {e:?}"));
        inputs += 1;
        replay.check(&key, "n", &h.fragment.nodes.len().to_string());
        let d = depth_of(&h.fragment);
        deepest = deepest.max(d);
        replay.check(&key, "depth", &d.to_string());
        let paras = html_to_paras(&h);
        let hp = paras
            .iter()
            .map(|p| {
                let b = &p.block;
                let runs = p
                    .runs
                    .iter()
                    .map(|r| {
                        let img = r.image.as_ref().map_or(String::new(), |i| {
                            let b = b64_encode(&i.data);
                            format!("{},{},{},{},{}", i.ext, i.data.len(), &b[..b.len().min(40)], num(i.w_px), num(i.h_px))
                        });
                        format!("{}|{}|{}|{}", hx(r.text.units()), d2b_f8_support::hs(&props_text(&r.props)), u8::from(r.block_bold), img)
                    })
                    .collect::<Vec<_>>()
                    .join(";");
                [
                    b.align.unwrap_or("").to_owned(),
                    num(b.heading),
                    match b.style {
                        Some(TitleStyle::Title) => "title".to_owned(),
                        Some(TitleStyle::Subtitle) => "subtitle".to_owned(),
                        None => String::new(),
                    },
                    b.list.map_or(String::new(), |l| format!("{},{},{}", u8::from(l.ordered), to_js_string(l.level), to_js_string(l.key))),
                    u8::from(b.pre).to_string(),
                    u8::from(b.web).to_string(),
                    runs,
                ]
                .join("~")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let hp = b64_encode(hp.as_bytes());
        // F8-D10: `<font color="constructor">` — `NAMED[s]` of the TS finds `Object.prototype`
        // (a function as the colour); here only the 18 names are colours
        let raw_s = String::from_utf16_lossy(h.raw.units());
        let proto =
            ["constructor", "__proto__", "toString", "valueOf", "hasOwnProperty"].iter().any(|p| raw_s.contains(&format!("color=\"{p}\"")));
        if proto && cap.get(&key, "hp") != Some(hp.as_str()) {
            replay.accept(&key, "hp", &hp, "F8-D10");
        } else {
            replay.check(&key, "hp", &hp);
        }
        let raw = h.raw.units();
        let gate = format!(
            "{}{}{}",
            u8::from(!rx::is_blank(raw)),
            u8::from(rx::has_table_tag(raw)),
            u8::from(rx::has_block_tag(&rx::strip_tables(raw)))
        );
        replay.check(&key, "gate", &gate);
        let mut ids = IdAlloc::default();
        let tables = tables_from_html(&h.tables, 9000.0, &mut ids);
        let tb = tables
            .iter()
            .map(|t| {
                let grid = t.grid_cols_twips.iter().map(|g| to_js_string(g.to_number())).collect::<Vec<_>>().join(".");
                let rows = t
                    .rows
                    .iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|c| {
                                let span = c.tc_pr.as_ref().and_then(|p| p.grid_span).map_or(1.0, |g| g.to_number());
                                let vm = c.tc_pr.as_ref().and_then(|p| p.v_merge.as_deref()).map_or("", |v| &v[..v.len().min(1)]);
                                let ps = c
                                    .content
                                    .paras
                                    .iter()
                                    .map(|p| {
                                        p.runs
                                            .iter()
                                            .map(|r| format!("{}{}", hx(r.text.units()), if r.props.bold == Some(true) { "*" } else { "" }))
                                            .collect::<Vec<_>>()
                                            .join("+")
                                    })
                                    .collect::<Vec<_>>()
                                    .join("/");
                                format!("{}{vm}:{ps}", to_js_string(span))
                            })
                            .collect::<Vec<_>>()
                            .join(";")
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                format!("{}x{grid}={rows}", t.rows.len())
            })
            .collect::<Vec<_>>()
            .join("\n");
        replay.check(&key, "tb", &b64_encode(tb.as_bytes()));
    }
    let report = replay.finish();
    println!("d2b-f8-html: {inputs} inputs, deepest tree {deepest}; {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
}
