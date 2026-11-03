//! Parity self-test of `uo-drawingml` inside the wasm module (feature `dml-selftest`, hybrid mode,
//! continuous verification): the module replays the committed TS captures of the crate
//! (`uo-drawingml/tests/captures/*.capture`: presets, custom guides, arcs, colour, patterns,
//! adjusts, blip, light, bevel, raster and effects) with the replay functions of the native parity
//! tests (`uo-drawingml/tests/*_parity.rs`, included with `#[path]`: one copy of every generator
//! and digest), and `tests/dml_selftest.test.mjs` runs each group in Node and in the browser
//! engines. A group takes `every`: the `every`-th item of each loop of its corpus (1 is the whole
//! corpus, the native test); a subset leaves the other facets of the capture unchecked. Not the
//! binding API of `uo-drawingml`; never in the shipped module.
#![allow(dead_code, unused_imports, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::expect_used)]

use uo_testkit::{Capture, Report};

#[path = "../../uo-drawingml/tests/frontier_a_parity.rs"]
mod frontier_a_parity;
#[path = "../../uo-drawingml/tests/light_bevel_parity.rs"]
mod light_bevel_parity;
#[path = "../../uo-drawingml/tests/preset_parity.rs"]
mod preset_parity;
#[path = "../../uo-drawingml/tests/raster_parity.rs"]
mod raster_parity;

type Replay = fn(&Capture, usize) -> Report;

fn presets(c: &Capture, every: usize) -> Report {
    preset_parity::replay_presets(c, every)
}

fn custom_guides(c: &Capture, every: usize) -> Report {
    preset_parity::replay_custom_guides(c, every)
}

fn arc(c: &Capture, _every: usize) -> Report {
    preset_parity::replay_arc(c)
}

fn colour(c: &Capture, every: usize) -> Report {
    frontier_a_parity::replay_colour(c, every)
}

fn patterns(c: &Capture, _every: usize) -> Report {
    frontier_a_parity::replay_patterns(c)
}

fn adjust(c: &Capture, every: usize) -> Report {
    frontier_a_parity::replay_adjust(c, every)
}

fn blip(c: &Capture, every: usize) -> Report {
    frontier_a_parity::replay_blip(c, every)
}

fn light_tables(c: &Capture, _every: usize) -> Report {
    light_bevel_parity::replay_light_tables(c)
}

fn light_at(c: &Capture, every: usize) -> Report {
    light_bevel_parity::replay_light_at(c, every)
}

fn bevel_phi(c: &Capture, _every: usize) -> Report {
    light_bevel_parity::replay_bevel_phi(c)
}

fn shade_bevel(c: &Capture, every: usize) -> Report {
    light_bevel_parity::replay_shade_bevel(c, every)
}

/// Every group: its name, the capture it replays and the replay.
const GROUPS: [(&str, &str, Replay); 20] = [
    ("presets", preset_parity::CAPTURE, presets),
    ("custom_guides", preset_parity::CAPTURE, custom_guides),
    ("arc_to_seg", preset_parity::CAPTURE, arc),
    ("colour", frontier_a_parity::CAPTURE, colour),
    ("patterns", frontier_a_parity::CAPTURE, patterns),
    ("adjust", frontier_a_parity::CAPTURE, adjust),
    ("blip", frontier_a_parity::CAPTURE, blip),
    ("light_tables", light_bevel_parity::CAPTURE, light_tables),
    ("light_at", light_bevel_parity::CAPTURE, light_at),
    ("bevel_phi", light_bevel_parity::CAPTURE, bevel_phi),
    ("shade_bevel", light_bevel_parity::CAPTURE, shade_bevel),
    ("flatten", raster_parity::CAPTURE, raster_parity::replay_flatten),
    ("fill", raster_parity::CAPTURE, raster_parity::replay_fill),
    ("blur", raster_parity::CAPTURE, raster_parity::replay_blur),
    ("distance", raster_parity::CAPTURE, raster_parity::replay_distance),
    ("morph", raster_parity::CAPTURE, raster_parity::replay_morph),
    ("shift", raster_parity::CAPTURE, raster_parity::replay_shift),
    ("warp", raster_parity::CAPTURE, raster_parity::replay_warp),
    ("affine", raster_parity::CAPTURE, raster_parity::replay_affine),
    ("effects", raster_parity::CAPTURE, raster_parity::replay_effects),
];

fn write_out(text: &str, out: *mut u8, cap: u32) -> u32 {
    let bytes = text.as_bytes();
    if bytes.len() > cap as usize || out.is_null() {
        return 0;
    }
    // SAFETY: `out` is a JS-provided buffer of `cap` bytes (`uo_alloc_bytes(cap)`) and `bytes.len() <= cap`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
    bytes.len() as u32
}

/// Number of groups.
#[unsafe(no_mangle)]
pub extern "C" fn uo_dml_selftest_group_count() -> u32 {
    GROUPS.len() as u32
}

/// The name of group `i`; 0 when there is none or it does not fit.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_dml_selftest_group_name(i: u32, out: *mut u8, cap: u32) -> u32 {
    GROUPS.get(i as usize).map_or(0, |(name, _, _)| write_out(name, out, cap))
}

/// Replays group `i` with `every` (1 = the whole corpus). The text written is
/// `equal=<n> mismatches=<n> accepted=<n> missing=<n>` and, for each of the first ten mismatches,
/// the facet with the captured and the computed value. 0 when the group does not exist or the
/// text does not fit.
///
/// # Safety
/// `out` must be null or point to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_dml_selftest_run(i: u32, every: u32, out: *mut u8, cap: u32) -> u32 {
    let Some((_, text, replay)) = GROUPS.get(i as usize) else { return 0 };
    let Ok(capture) = Capture::parse(text) else { return write_out("capture does not parse", out, cap) };
    let report = replay(&capture, every as usize);
    let summary = format!(
        "equal={} mismatches={} accepted={} missing={}\n{}",
        report.equal,
        report.mismatches.len(),
        report.accepted.len(),
        report.missing.len(),
        report.describe(10)
    );
    write_out(&summary, out, cap)
}
