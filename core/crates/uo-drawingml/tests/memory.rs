//! Memory gate of phase 1 (M1, M2, M5): every public operation and every error path returns the
//! live bytes exactly to their starting value, outputs `black_box`ed; the error paths allocate
//! nothing before their check; the crate holds nothing between calls (10,000 distinct unknown
//! preset names leave the live bytes unchanged); a soak of the preset engine is flat with a
//! tolerance of 0 B. The phase-2 modules are covered here too (and in their own memory tests).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::*;

const SIZES: [(f64, f64); 4] = [(200.0, 100.0), (0.0, 0.0), (21600.0, 21600.0), (13.37, 0.5)];

type AdjCase = (Option<Vec<f64>>, Option<Vec<&'static str>>);

fn adj_cases() -> Vec<AdjCase> {
    vec![
        (None, None),
        (Some(vec![]), None),
        (Some(vec![]), Some(vec![])),
        (Some(vec![50000.0, 25000.0]), None),
        (Some(vec![f64::NAN, 1e9]), Some(vec!["adj1", "adj2"])),
        (Some(vec![-1e9, f64::INFINITY, 7.0]), Some(vec!["adj2", "adj1", "adj3"])),
    ]
}

fn names() -> Vec<&'static str> {
    preset_names().collect()
}

#[test]
fn m1_preset_engine() {
    clear_caches();
    let all = names();
    assert_no_leak(|| black_box(preset_names().count()));
    for name in [Some("rect"), Some(""), None, Some("nope"), Some("star5"), Some("actionButtonReturn"), Some("constructor")] {
        assert_no_leak(|| black_box(has_preset_def(name)));
    }
    for name in all.iter().copied().step_by(7).chain(["rect", "nope", "heart", "frame", "can", "cube", "sun"]) {
        for (w, h) in SIZES {
            for (adj, nm) in adj_cases() {
                let adj = adj.as_deref();
                let nm = nm.as_deref();
                assert_no_leak(|| black_box(eval_preset(name, w, h, adj, nm)));
                assert_no_leak(|| black_box(preset_guides(name, w, h, adj, nm)));
                assert_no_leak(|| black_box(preset_text_rect(Some(name), w, h, adj, nm)));
                assert_no_leak(|| black_box(preset_path(Some(name), w, h, adj, nm)));
            }
            assert_no_leak(|| black_box(preset_outline(Some(name), w, h, None)));
            assert_no_leak(|| black_box(preset_outline(Some(name), w, h, Some(&[("adj1", 1.0), ("adj2", f64::NAN)]))));
        }
    }
    assert_no_leak(|| black_box(preset_outline(None, 10.0, 10.0, None)));
    assert_no_leak(|| black_box(preset_path(None, 10.0, 10.0, None, None)));
    assert_no_leak(|| black_box(preset_text_rect(None, 10.0, 10.0, None, None)));
}

#[test]
fn m1_custom_guides_and_arcs() {
    let av = [GuideDef { name: "a", fmla: &["val", "5"] }, GuideDef { name: "b", fmla: &["*/", "a", "w", "3"] }];
    let gd = [GuideDef { name: "c", fmla: &["at2", "b", "a"] }, GuideDef { name: "d", fmla: &[] }, GuideDef { name: "", fmla: &["bogus"] }];
    for limits in [Limits::DEFAULT, Limits { max_custom_guides: 1, ..Limits::DEFAULT }, Limits { max_custom_guides: 0, ..Limits::DEFAULT }]
    {
        assert_no_leak(|| {
            let g = custom_guides(&av, &gd, 200.0, 100.0, &limits);
            black_box((g.get("c"), g.val("b"), g.len(), g.is_empty()))
        });
    }
    assert_no_leak(|| black_box(custom_guides(&[], &[], f64::NAN, f64::INFINITY, &Limits::DEFAULT)));
    for (rx, ry, st, sw) in
        [(5.0, 5.0, 0.0, 5_400_000.0), (0.0, 1.0, 0.0, 1.0), (f64::NAN, f64::NAN, f64::NAN, f64::NAN), (1.0, 1.0, 1e9, -21_600_000.0)]
    {
        assert_no_leak(|| black_box(arc_to_seg(1.0, 2.0, rx, ry, st, sw)));
    }
}

#[test]
fn m1_shared_types() {
    assert_no_leak(|| {
        let mut s = Subpaths::new();
        s.end_sub();
        s.push(PathSeg::M { x: 0.0, y: 0.0 });
        s.push(PathSeg::Z);
        s.end_sub();
        black_box((s.len(), s.is_empty(), s.get(1).map(<[PathSeg]>::len), s.iter().count(), s.segs().len(), s.clone()))
    });
    assert_no_leak(|| {
        let mut p = Polygons::new();
        p.push_point(1.0, 2.0);
        p.end_poly();
        p.push_point(3.0, 4.0);
        p.discard_pending();
        black_box((p.len(), p.get(0).map(<[f64]>::len), p.iter().count(), p.clone()))
    });
    assert_no_leak(|| black_box((Affine::IDENTITY.mul(&Affine::IDENTITY), Affine::IDENTITY.invert(), Affine::IDENTITY.apply(1.0, 2.0))));
    assert_no_leak(|| black_box((Limits::DEFAULT, Limits::NATIVE, Limits::WASM32, Limits::default())));
    assert_no_leak(|| black_box(Pt::default()));
}

#[test]
fn m1_masks_and_their_error_paths() {
    let limits = Limits { max_mask_pixels: 10_000, ..Limits::NATIVE };
    assert_no_leak(|| black_box(Mask::new(100, 100, &limits).unwrap().into_vec()));
    assert_no_leak(|| {
        let mut m = Mask::new(10, 10, &limits).unwrap();
        m.data_mut()[5] = 1.0;
        black_box((m.w(), m.h(), m.data().len(), m.clone()))
    });
    assert_no_leak(|| black_box(Mask::from_vec(3, 3, vec![0.0; 9]).unwrap()));
    // the error paths allocate nothing before their check
    let (r, usage) = measure(|| Mask::new(101, 100, &limits));
    assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(usage.allocated, 0);
    let (r, usage) = measure(|| Mask::new(u32::MAX, u32::MAX, &limits));
    assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    assert_eq!(usage.allocated, 0);
    assert_no_leak(|| black_box(Mask::new(101, 100, &limits)));
    assert_no_leak(|| black_box(Mask::from_vec(3, 3, vec![0.0; 8])));
    assert_no_leak(|| black_box(Mask::from_vec(u32::MAX, u32::MAX, Vec::new())));
}

#[test]
fn m1_modules_not_ported_yet() {
    let limits = Limits::DEFAULT;
    let mask = Mask::new(4, 4, &limits).unwrap();
    let identity = Affine::IDENTITY;
    let mut rgba = vec![7u8; 4 * 4 * 4];
    assert_no_leak(|| {
        black_box((srgb_to_lin(0.5), lin_to_srgb(0.5), apply_color_mods([0.1, 0.2, 0.3], &[(ColorModKind::LumMod, Some(80_000.0))])))
    });
    assert_no_leak(|| black_box(ColorModKind::from_name("lumMod")));
    assert_no_leak(|| black_box((preset_patterns().len(), pattern_rows("pct5"), pattern_density("pct5"))));
    assert_no_leak(|| black_box(flatten_path(&Subpaths::new(), &identity, DEFAULT_TOL, &limits)));
    assert_no_leak(|| black_box(gaussian_blur(&mask, 1.0, &limits)));
    assert_no_leak(|| black_box(distance_to(&mask, true)));
    assert_no_leak(|| black_box(morph(&mask, 1.0, &limits)));
    assert_no_leak(|| black_box(shift_mask(&mask, 1.0, 1.0, Some((5, 5)), &limits)));
    assert_no_leak(|| black_box(warp_mask(&mask, &identity, 4, 4, &limits)));
    let mut target = mask.clone();
    assert_no_leak(|| black_box(fill_polygons(&mut target, &Polygons::new(), false, &limits)));
    let spec = ShadowSpec {
        blur_pt: 1.0,
        dist_pt: 1.0,
        dir_deg: 0.0,
        sx: None,
        sy: None,
        kx_deg: None,
        ky_deg: None,
        algn: Align::from_attr(None),
    };
    let bx = BoxF { x: 0.0, y: 0.0, w: 4.0, h: 4.0 };
    assert_no_leak(|| black_box((align_point(Align::B, 0.0, 0.0, 4.0, 4.0), shadow_affine(&spec, &bx, 1.0))));
    assert_no_leak(|| black_box(outer_shadow_mask(&mask, &bx, &spec, 1.0, &limits)));
    assert_no_leak(|| black_box(inner_shadow_mask(&mask, 1.0, 1.0, 0.0, 1.0, &limits)));
    assert_no_leak(|| black_box(glow_mask(&mask, 1.0, 1.0, &limits)));
    assert_no_leak(|| black_box(soft_edge_mask(&mask, 1.0, 1.0, &limits)));
    assert_no_leak(|| black_box(reflection_alpha(0.5, &Reflection { st_a: 1.0, st_pos: 0.0, end_a: 0.0, end_pos: 1.0 })));
    let table = light_table(Some("threePt"), None);
    assert_no_leak(|| black_box((light_at(&table, Some("t"), 0.0, 0.0), light_face(&table), bevel_phi(Some("circle"), 0.5, 1.0))));
    let bevel = BevelSpec { w: 3.0, h: 3.0, prst: None, rig: None, dir: None, material: None, word: false };
    assert_no_leak(|| black_box(shade_bevel(&mut rgba, &mask, &bevel, &limits)));
    assert_no_leak(|| {
        adjust_saturation(&mut rgba, 2.0);
        adjust_temperature(&mut rgba, 5000.0);
        adjust_brightness_contrast(&mut rgba, 0.5, -0.5);
        black_box(adjust_sharpen(&mut rgba, 4, 4, 0.5, &limits))
    });
    let fx = ImageEffects { gray: true, gain: Some(1.5), bright: None, floor: false, bi_level: None };
    assert_no_leak(|| {
        apply_image_effects(&mut rgba, &fx);
        apply_picture_adjust(&mut rgba, &PictureAdjust { alpha: Some(0.5), duotone: None, clr_change: None });
        black_box((effect_line(&fx), effect_decode(&fx, 3)))
    });
}

#[test]
fn m5_no_hidden_growth() {
    // the TS cached `null` for every unknown preset name (and a LUT per brightness value): here
    // nothing is kept
    clear_caches();
    let (_, usage) = measure(|| {
        for i in 0..10_000u32 {
            let name = format!("unknown{i}");
            black_box(has_preset_def(Some(&name)));
            black_box(eval_preset(&name, 100.0, 100.0, None, None));
            black_box(preset_path(Some(&name), 100.0, 100.0, None, None));
            black_box(preset_outline(Some(&name), 100.0, 100.0, None));
        }
    });
    assert_eq!(usage.live_delta, 0);
    assert_no_leak(clear_caches);
}

/// M2: the preset engine, flat with a tolerance of 0 B (the crate has no caches).
#[test]
fn m2_soak_of_the_preset_engine() {
    let all = names();
    let hostile = [f64::NAN, 1e9, -1e9, f64::INFINITY];
    let iterations = if cfg!(debug_assertions) { 6 } else { 100 };
    let series = soak(iterations, 2, |_| {
        for name in &all {
            for (w, h) in [(200.0, 100.0), (21600.0, 21600.0), (0.0, 1e6)] {
                black_box(eval_preset(name, w, h, None, None));
                black_box(preset_path(Some(name), w, h, None, None));
                black_box(preset_guides(name, w, h, None, None));
                black_box(preset_text_rect(Some(name), w, h, None, None));
                black_box(preset_outline(Some(name), w, h, None));
                black_box(eval_preset(name, w, h, Some(&hostile), None));
                black_box(preset_path(Some(name), w, h, Some(&hostile), Some(&["adj", "adj1", "adj2", "adj3"])));
            }
        }
        let av = [GuideDef { name: "a", fmla: &["val", "1"] }];
        black_box(custom_guides(&av, &av, 10.0, 10.0, &Limits::DEFAULT));
    });
    assert_flat(&series, 1, 0);
}

/// M3: allocated and peak bytes of one call of the preset engine (the numbers of the module
/// record); the asserted ceilings are the budget a regression of more than 10% must not pass.
#[test]
fn m3_budgets_of_the_preset_engine() {
    let mut worst_eval = (0u64, "");
    let mut worst_path = (0u64, "");
    let mut worst_peak = 0i64;
    let mut worst_guides = 0u64;
    for name in names() {
        let (_, usage) = measure(|| black_box(eval_preset(name, 200.0, 100.0, None, None)));
        if usage.allocated > worst_eval.0 {
            worst_eval = (usage.allocated, name);
        }
        worst_peak = worst_peak.max(usage.peak_above_start);
        let (_, usage) = measure(|| black_box(preset_path(Some(name), 200.0, 100.0, None, None)));
        if usage.allocated > worst_path.0 {
            worst_path = (usage.allocated, name);
        }
        let (_, usage) = measure(|| black_box(preset_guides(name, 200.0, 100.0, None, None)));
        worst_guides = worst_guides.max(usage.allocated);
        assert!(usage.allocated <= 33_792, "{name}: guides allocated {}", usage.allocated);
        let (_, usage) = measure(|| black_box(preset_text_rect(Some(name), 200.0, 100.0, None, None)));
        assert!(usage.allocated <= 33_792, "{name}: text rect allocated {}", usage.allocated);
    }
    eprintln!(
        "worst eval_preset: {} B ({}), peak {worst_peak} B; worst preset_path: {} B ({}); worst preset_guides: {worst_guides} B",
        worst_eval.0, worst_eval.1, worst_path.0, worst_path.1
    );
    assert!(worst_eval.0 <= 45_056, "eval_preset allocated {} ({})", worst_eval.0, worst_eval.1);
    assert!(worst_path.0 <= 45_056, "preset_path allocated {} ({})", worst_path.0, worst_path.1);
}
