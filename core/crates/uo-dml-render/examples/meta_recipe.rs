//! The Rust side of the B5b parity harness (`uo-pptx/js/compare-meta.mjs`): rasterizes every input of a `capture-meta.mjs`
//! input file at the displays of a manifest and writes the recipes, answering `measureText` from the deterministic stub the TS
//! capture was recorded with (`js/lib/record-ctx.mjs` `stubMeasure`) and reporting every compressed EMF+ bitmap as not
//! decodable (Node has no `createImageBitmap`).
//!
//! ```text
//! cargo run -p uo-dml-render --example meta_recipe --release -- <inputs> <manifest> <out>
//! ```
//! `manifest`: one `key w h` per rasterization (`-` for no display). Formats: `js/lib/meta-io.mjs`.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::collections::HashMap;
use std::fs;

use uo_dml_render::metafile::{Decoded, Measure, Metrics, Outcome, Raster, rasterize_special};

#[path = "../tests/support/meta_stub.rs"]
mod stub;

fn rd_u32(b: &[u8], p: &mut usize) -> u32 {
    let v = u32::from_le_bytes([b[*p], b[*p + 1], b[*p + 2], b[*p + 3]]);
    *p += 4;
    v
}

struct Stub;

impl Measure for Stub {
    fn metrics(&mut self, font: &[u16], text: &[u16]) -> Option<Metrics> {
        Some(stub::stub_metrics(font, text))
    }
    fn decoded(&mut self, _key: &str) -> Decoded {
        Decoded::Failed
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: meta_recipe <inputs> <manifest> <out>");
        std::process::exit(2);
    }
    let inputs = fs::read(&a[1]).unwrap_or_default();
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut p = 0;
    while p + 4 <= inputs.len() {
        let kl = rd_u32(&inputs, &mut p) as usize;
        let key = String::from_utf8_lossy(&inputs[p..p + kl]).into_owned();
        p += kl;
        let n = rd_u32(&inputs, &mut p) as usize;
        files.insert(key, inputs[p..p + n].to_vec());
        p += n;
    }
    let manifest = fs::read_to_string(&a[2]).unwrap_or_default();
    let mut out: Vec<u8> = Vec::new();
    let mut n = 0;
    let t0 = std::time::Instant::now();
    for line in manifest.lines().filter(|l| !uo_jsnum::text::trim(l).is_empty()) {
        let f: Vec<&str> = line.split(' ').collect();
        let (Some(key), Some(w), Some(h)) = (f.first(), f.get(1), f.get(2)) else { continue };
        let display = if *w == "-" { None } else { Some((uo_jsnum::parse::to_number(w), uo_jsnum::parse::to_number(h))) };
        let Some(bytes) = files.get(*key) else { continue };
        n += 1;
        out.extend_from_slice(&u32::try_from(key.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.push(u8::from(display.is_some()));
        let (dw, dh) = display.unwrap_or((0.0, 0.0));
        out.extend_from_slice(&dw.to_le_bytes());
        out.extend_from_slice(&dh.to_le_bytes());
        if key.starts_with("eot") {
            match uo_fonts::eot::read(bytes) {
                Some(f) => {
                    out.push(4);
                    out.extend_from_slice(&u32::try_from(f.len()).unwrap_or(0).to_le_bytes());
                    out.extend_from_slice(&f);
                }
                None => out.push(0),
            }
            continue;
        }
        match rasterize_special(bytes, display, &mut Stub) {
            Outcome::Done(None) => out.push(0),
            Outcome::Done(Some(Raster::Recipe(r))) => {
                out.push(1);
                out.extend_from_slice(&u32::try_from(r.bytes.len()).unwrap_or(0).to_le_bytes());
                out.extend_from_slice(&r.bytes);
            }
            Outcome::Need(v) => {
                out.push(3);
                out.extend_from_slice(&u32::try_from(v.len()).unwrap_or(0).to_le_bytes());
            }
        }
    }
    if let Err(e) = fs::write(&a[3], &out) {
        eprintln!("{}: {e}", a[3]);
        std::process::exit(2);
    }
    eprintln!("{n} rasterizations in {:.2} s", t0.elapsed().as_secs_f64());
}
