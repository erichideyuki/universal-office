//! The Rust side of the draw-op parity gate (`uo-render.md` "R-S3/R-S4", `js/compare-drawops.mjs`): emits the Slides
//! display list of every (slide, view) of one deck into a `.uolf` frame file, answering the measurement requests from the
//! deterministic stub the TS captures were recorded with (`js/lib/record-ctx.mjs` `stubMeasure`).
//!
//! ```text
//! cargo run -p uo-pptx --example emit_uolf --release -- <deck.pptx> <manifest.txt> <out.uolf>
//! ```
//! `manifest.txt`: one `slide view scale ox oy` per frame (the entries of the capture: scale and origin printed by the
//! JS driver `js/emit-uolf.mjs`). `.uolf`: `"UOLF"`, version 1, then per frame `u32 listLen`, the list message,
//! `u32 nBitmaps`, per bitmap added by this message `u32 len` + the RGBA bytes.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::fs;

use uo_dml_render::emit::EmitCaches;
use uo_dml_render::text::{Capabilities, MAX_ROUNDS, MeasureTable};
use uo_pptx::{Deck, EmitFail, Limits, View, emit_slide};
use uo_render::slides::{SlidesEncoder, SlidesResources};

#[path = "../tests/support/stub_measure.rs"]
mod stub;
use stub::answer;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: emit_uolf <deck.pptx> <manifest.txt> <out.uolf>");
        std::process::exit(2);
    }
    let bytes = fs::read(&a[1]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", a[1]);
        std::process::exit(2);
    });
    let manifest = fs::read_to_string(&a[2]).unwrap_or_default();
    #[cfg(feature = "ppt")]
    let opened = if uo_pptx::ppt::is_cfb(&bytes) {
        {
            // the TS converter's charts (fallback pictures) unless PowerPoint's rules are asked (`UO_EMIT_RULES=ppt`)
            let o = if std::env::var("UO_EMIT_RULES").as_deref() == Ok("ppt") {
                uo_pptx::ppt::ConvertOptions::default()
            } else {
                uo_pptx::ppt::ConvertOptions::ts_strict()
            };
            uo_pptx::ppt::convert_with(&bytes, &o).and_then(|p| Deck::from_parts(p, Limits::default()))
        }
    } else {
        Deck::open(&bytes, Limits::default())
    };
    #[cfg(not(feature = "ppt"))]
    let opened = Deck::open(&bytes, Limits::default());
    let mut deck = match opened {
        Ok(mut d) => {
            // the draw-op parity against the frozen TS: every TS switch on (Slides wave 5); `UO_EMIT_RULES=ppt` = PowerPoint's
            if std::env::var("UO_EMIT_RULES").as_deref() != Ok("ppt") {
                d.set_rules(uo_dml_render::SlidesRules::ts_strict());
            }
            d
        }
        Err(e) => {
            eprintln!("open: {e}");
            std::process::exit(3);
        }
    };
    let mut out: Vec<u8> = b"UOLF".to_vec();
    out.push(1);
    let mut res = SlidesResources::new();
    let mut enc = SlidesEncoder::new(1);
    let mut table = MeasureTable::default();
    table.caps = Capabilities::default();
    // the canvas capabilities the capture was recorded with (`--caps` of the capture): only these two change the layout
    let caps = std::env::var("UO_EMIT_CAPS").unwrap_or_default();
    table.caps.font_kerning = !caps.split(',').any(|c| c == "nokerning");
    table.caps.letter_spacing = !caps.split(',').any(|c| c == "nospacing");
    let mut bitmaps_sent = 0usize;
    let mut failures = 0;
    for line in manifest.lines().filter(|l| !uo_jsnum::text::trim(l).is_empty()) {
        let f: Vec<&str> = uo_jsnum::text::split_js_space(line).collect();
        let (Some(slide), Some(scale), Some(ox), Some(oy)) = (
            f.first().and_then(|s| s.parse::<u32>().ok()),
            f.get(2).map(|s| uo_jsnum::parse::to_number(s)),
            f.get(3).map(|s| uo_jsnum::parse::to_number(s)),
            f.get(4).map(|s| uo_jsnum::parse::to_number(s)),
        ) else {
            continue;
        };
        let mut caches = EmitCaches::new();
        let mut rounds = 0;
        let list = loop {
            match emit_slide(&mut deck, slide, View { scale, ox, oy }, &table, &mut res, &mut caches) {
                Ok(e) => {
                    if !e.chart_frames.is_empty() && std::env::var_os("UO_EMIT_CHARTS").is_some() {
                        eprintln!("charts: slide {slide} view {scale}: {} chart frame(s)", e.chart_frames.len());
                    }
                    break Some(e.list);
                }
                Err(EmitFail::Need(n)) => {
                    rounds += 1;
                    if rounds > MAX_ROUNDS {
                        eprintln!(
                            "slide {slide}: still missing measures after {MAX_ROUNDS} rounds: {:?}",
                            n.0.iter().take(3).collect::<Vec<_>>()
                        );
                        break None;
                    }
                    if std::env::var_os("UO_EMIT_DEBUG").is_some() {
                        eprintln!("slide {slide} round {rounds}: {} reqs, first {:?}", n.0.len(), n.0.first());
                    }
                    answer(&deck, &mut table, &n.0);
                }
                Err(EmitFail::Deck(e)) => {
                    eprintln!("slide {slide}: {e}");
                    break None;
                }
            }
        };
        let mut msg = Vec::new();
        let ok = match &list {
            Some(l) => enc.encode(l, &res, &mut msg).map_err(|e| eprintln!("slide {slide}: encode: {e}")).is_ok(),
            None => false,
        };
        if !ok {
            failures += 1;
            msg.clear();
        }
        out.extend_from_slice(&u32::try_from(msg.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&msg);
        let now = res.lens()[4];
        out.extend_from_slice(&u32::try_from(now - bitmaps_sent).unwrap_or(0).to_le_bytes());
        for i in bitmaps_sent..now {
            let data = res.bitmap_bytes(u32::try_from(i).unwrap_or(0));
            let d: &[u8] = data.map_or(&[], |d| &d[..]);
            // B5b: bit 31 of the length marks a metafile recipe (not RGBA)
            let flag = if res.is_recipe(u32::try_from(i).unwrap_or(0)) { 0x8000_0000 } else { 0 };
            out.extend_from_slice(&(u32::try_from(d.len()).unwrap_or(0) | flag).to_le_bytes());
            out.extend_from_slice(d);
        }
        bitmaps_sent = now;
    }
    if let Err(e) = fs::write(&a[3], &out) {
        eprintln!("{}: {e}", a[3]);
        std::process::exit(2);
    }
    if failures > 0 {
        std::process::exit(1);
    }
}
