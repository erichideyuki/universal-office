//! The JS traps of the preset engine, one test each (module record, "JS semantics: rules and
//! traps"), on top of the bit-for-bit replay of `preset_parity.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::f64::consts::PI;

use uo_drawingml::{
    GuideDef, Limits, PathSeg, PresetPath, arc_to_seg, custom_guides, eval_preset, has_preset_def, preset_guides, preset_names,
    preset_outline, preset_path, preset_text_rect,
};

#[test]
fn there_are_187_presets_in_the_ts_key_order() {
    let names: Vec<&str> = preset_names().collect();
    assert_eq!(names.len(), 187);
    assert_eq!(names[0], "accentBorderCallout1");
    assert_eq!(names[186], "wedgeRoundRectCallout");
    assert!(names.iter().all(|n| has_preset_def(Some(n))));
}

#[test]
fn only_own_names_are_presets() {
    for name in ["constructor", "__proto__", "toString", "hasOwnProperty", "valueOf", "", "Rect", "nope"] {
        assert!(!has_preset_def(Some(name)), "{name}");
        assert!(eval_preset(name, 10.0, 10.0, None, None).is_none(), "{name}");
        assert!(preset_guides(name, 10.0, 10.0, None, None).is_none(), "{name}");
        assert_eq!(preset_path(Some(name), 10.0, 10.0, None, None), PresetPath::Rect, "{name}");
    }
    assert!(!has_preset_def(None));
    assert_eq!(preset_path(None, 10.0, 10.0, None, None), PresetPath::Rect);
    assert_eq!(preset_path(Some(""), 10.0, 10.0, None, None), PresetPath::Rect);
}

#[test]
fn an_empty_adjust_list_is_truthy_but_has_no_length() {
    // `adj?.length` is falsy for []: the hand-drawn roundRect; with one value, the official formulas
    let hand = preset_path(Some("roundRect"), 100.0, 100.0, Some(&[]), None);
    assert_eq!(hand, PresetPath::RoundRect { r: 100.0 * (16667.0 / 100000.0) });
    let official = preset_path(Some("roundRect"), 100.0, 100.0, Some(&[10000.0]), None);
    assert!(matches!(official, PresetPath::Path(_)));
    // `adj && adjNames` with [] is "by name": no adjust applies, the defaults stay
    let by_name = preset_guides("roundRect", 100.0, 100.0, Some(&[]), Some(&[])).unwrap();
    assert_eq!(by_name.get("adj"), Some(16667.0));
    // a positional list shorter than the avLst leaves the rest at their defaults
    let short = preset_guides("hexagon", 100.0, 100.0, Some(&[1.0]), None).unwrap();
    assert_eq!(short.get("adj"), Some(1.0));
    assert_eq!(short.get("vf"), Some(115470.0));
}

#[test]
fn adjust_names_use_the_first_match_and_nan_is_kept() {
    let g = preset_guides("hexagon", 100.0, 100.0, Some(&[1.0, 2.0]), Some(&["adj", "adj"])).unwrap();
    assert_eq!(g.get("adj"), Some(1.0));
    let g = preset_guides("roundRect", 100.0, 100.0, Some(&[f64::NAN]), None).unwrap();
    assert!(g.get("adj").unwrap().is_nan());
    // the name of an unknown adjust is ignored
    let g = preset_guides("roundRect", 100.0, 100.0, Some(&[7.0]), Some(&["other"])).unwrap();
    assert_eq!(g.get("adj"), Some(16667.0));
    // a name whose value is missing from `adj` falls back to the default formula
    let g = preset_guides("hexagon", 100.0, 100.0, Some(&[7.0]), Some(&["vf", "adj"])).unwrap();
    assert_eq!(g.get("vf"), Some(7.0));
    assert_eq!(g.get("adj"), Some(25000.0));
}

#[test]
fn custom_guides_follow_val() {
    let av = [GuideDef { name: "a", fmla: &["val", "0x1A"] }, GuideDef { name: "b", fmla: &["*/", "a", "2"] }];
    let gd = [
        GuideDef { name: "c", fmla: &["+/", "a", "b", "0"] },
        GuideDef { name: "d", fmla: &["?:", "nope", "1", "2"] },
        GuideDef { name: "e", fmla: &["pin", "1", "5", "3"] },
        GuideDef { name: "f", fmla: &["sqrt", "-4"] },
        GuideDef { name: "g", fmla: &["bogus"] },
        GuideDef { name: "h", fmla: &[] },
        GuideDef { name: "i", fmla: &["val", "1e400"] },
        GuideDef { name: "j", fmla: &["val", "\u{a0}12\u{feff}"] },
    ];
    let g = custom_guides(&av, &gd, 200.0, 100.0, &Limits::DEFAULT);
    assert_eq!(g.get("a"), Some(26.0));
    // `*/` with a missing third argument: the token "0" gives 0, which `|| 1` turns into 1
    assert_eq!(g.get("b"), Some(52.0));
    // `+/` divides by `z || 1`
    assert_eq!(g.get("c"), Some(78.0));
    // `?:` is `x > 0`: an unknown name is Number('nope') = NaN → 0 → the third
    assert_eq!(g.get("d"), Some(2.0));
    // `pin`: v above hi gives hi
    assert_eq!(g.get("e"), Some(3.0));
    assert_eq!(g.get("f"), Some(0.0));
    assert_eq!(g.get("g"), Some(0.0));
    assert_eq!(g.get("h"), Some(0.0));
    // a token that is not finite is 0
    assert_eq!(g.get("i"), Some(0.0));
    assert_eq!(g.val("1e400"), 0.0);
    assert_eq!(g.val("0x10"), 16.0);
    assert_eq!(g.val(" 7 "), 7.0);
    assert_eq!(g.val("1_0"), 0.0);
    // JS white space (U+00A0, U+FEFF) is trimmed by Number()
    assert_eq!(g.get("j"), Some(12.0));
    // the built-ins
    assert_eq!(g.get("hc"), Some(100.0));
    assert_eq!(g.get("ss"), Some(100.0));
    assert_eq!(g.get("ls"), Some(200.0));
    assert_eq!(g.get("wd32"), Some(6.25));
    assert_eq!(g.get("cd4"), Some(5_400_000.0));
}

#[test]
fn a_guide_may_shadow_a_builtin_and_the_last_definition_wins() {
    let av = [
        GuideDef { name: "w", fmla: &["val", "5"] },
        GuideDef { name: "x", fmla: &["+-", "w", "w", "0"] },
        GuideDef { name: "w", fmla: &["val", "7"] },
    ];
    let g = custom_guides(&av, &[], 200.0, 100.0, &Limits::DEFAULT);
    assert_eq!(g.get("w"), Some(7.0));
    // x was evaluated between the two definitions
    assert_eq!(g.get("x"), Some(10.0));
    assert_eq!(g.get("wd2"), Some(100.0));
}

#[test]
fn custom_guides_stop_at_the_limit_per_list() {
    let av: Vec<GuideDef<'_>> = ["a", "b", "c"].iter().map(|n| GuideDef { name: n, fmla: &["val", "1"] }).collect();
    let gd: Vec<GuideDef<'_>> = ["d", "e", "f"].iter().map(|n| GuideDef { name: n, fmla: &["val", "1"] }).collect();
    let g = custom_guides(&av, &gd, 10.0, 10.0, &Limits { max_custom_guides: 2, ..Limits::DEFAULT });
    for (name, want) in [("a", true), ("b", true), ("c", false), ("d", true), ("e", true), ("f", false)] {
        assert_eq!(g.get(name).is_some(), want, "{name}");
    }
    let none = custom_guides(&av, &gd, 10.0, 10.0, &Limits { max_custom_guides: 0, ..Limits::DEFAULT });
    assert!(none.get("a").is_none());
    assert_eq!(none.len(), 17 + 30);
}

#[test]
fn the_arc_guard_lets_nan_through() {
    assert!(arc_to_seg(0.0, 0.0, 0.0, 5.0, 0.0, 5_400_000.0).is_none());
    assert!(arc_to_seg(0.0, 0.0, 5.0, -1.0, 0.0, 5_400_000.0).is_none());
    assert!(arc_to_seg(0.0, 0.0, 5.0, 5.0, 0.0, 0.0).is_none());
    assert!(arc_to_seg(0.0, 0.0, 5.0, 5.0, 0.0, -0.0).is_none());
    assert!(arc_to_seg(0.0, 0.0, f64::NAN, 5.0, 0.0, 5_400_000.0).is_some());
    assert!(arc_to_seg(0.0, 0.0, 5.0, 5.0, 0.0, f64::NAN).is_some());
    // a full turn: a1 = a0 + sign * 2 pi
    let (seg, _, _) = arc_to_seg(10.0, 10.0, 5.0, 5.0, 0.0, 21_600_000.0).unwrap();
    let PathSeg::E { a0, a1, ccw, .. } = seg else { panic!("not an ellipse") };
    assert_eq!(a1, a0 + 2.0 * PI);
    assert!(!ccw);
    let (seg, _, _) = arc_to_seg(10.0, 10.0, 5.0, 5.0, 0.0, -21_600_000.0).unwrap();
    let PathSeg::E { a0, a1, ccw, .. } = seg else { panic!("not an ellipse") };
    assert_eq!(a1, a0 - 2.0 * PI);
    assert!(ccw);
}

#[test]
fn text_rect_is_none_unless_it_has_area() {
    let r = preset_text_rect(Some("rect"), 200.0, 100.0, None, None).unwrap();
    assert_eq!((r.l, r.t, r.r, r.b), (0.0, 0.0, 200.0, 100.0));
    assert!(preset_text_rect(Some("rect"), 0.0, 0.0, None, None).is_none());
    assert!(preset_text_rect(Some("rect"), f64::NAN, 1.0, None, None).is_none());
    assert!(preset_text_rect(Some("line"), 10.0, 10.0, None, None).is_none());
    assert!(preset_text_rect(None, 10.0, 10.0, None, None).is_none());
    assert!(preset_text_rect(Some(""), 10.0, 10.0, None, None).is_none());
    let e = preset_text_rect(Some("ellipse"), 100.0, 100.0, None, None).unwrap();
    assert!((e.l - 100.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) / 2.0).abs() < 0.5);
}

#[test]
fn path_scaling_uses_the_path_coordinate_system() {
    // cloud has `w=43200 h=43200` paths: the coordinates scale to the rectangle
    let g = eval_preset("cloud", 100.0, 50.0, None, None).unwrap();
    assert!(g.subs.len() >= 2);
    assert_eq!(g.fill.len(), g.subs.len());
    assert_eq!(g.stroke.len(), g.subs.len());
}

#[test]
fn outline_falls_back_to_the_rectangle() {
    for prst in [None, Some(""), Some("rect"), Some("nope"), Some("constructor")] {
        let o = preset_outline(prst, 30.0, 20.0, None);
        assert_eq!(o.subs.len(), 1);
        assert_eq!((o.fill.as_slice(), o.stroke.as_slice()), (&[true][..], &[true][..]));
        assert_eq!(o.subs.get(0).unwrap().len(), 5);
    }
    let o = preset_outline(Some("accentBorderCallout1"), 100.0, 100.0, Some(&[("adj1", 50000.0)]));
    assert_eq!(o.subs.len(), 3);
    // fill = "none" is a stroke-only subpath
    assert_eq!(o.fill, [true, false, false]);
}

#[test]
fn hand_drawn_and_official_paths_agree_on_shape_kinds() {
    assert!(matches!(preset_path(Some("ellipse"), 10.0, 10.0, None, None), PresetPath::Ellipse));
    assert!(matches!(preset_path(Some("line"), 10.0, 20.0, None, None), PresetPath::Line { x1: 0.0, y1: 0.0, x2: 10.0, y2: 20.0 }));
    let PresetPath::Pie { start_deg, end_deg } = preset_path(Some("pie"), 10.0, 10.0, None, None) else { panic!("pie") };
    assert_eq!((start_deg, end_deg), (0.0, 270.0));
    let PresetPath::Poly(p) = preset_path(Some("pentagon"), 100.0, 100.0, None, None) else { panic!("pentagon") };
    assert_eq!(p.len(), 5);
    // fitted to the box: the extremes are exactly 0 and the size
    assert_eq!(p.iter().map(|q| q.x.to_bits()).min(), Some(0.0f64.to_bits()));
    let PresetPath::Path(shape) = preset_path(Some("can"), 100.0, 100.0, None, None) else { panic!("can") };
    assert_eq!(shape.sub_lum, Some(vec![Some(0.0), Some(0.4)]));
    assert!(shape.sub_stroke.is_none());
    let PresetPath::Path(shape) = preset_path(Some("actionButtonDocument"), 100.0, 100.0, None, None) else { panic!("document") };
    assert_eq!(shape.sub_lum, Some(vec![Some(0.0), Some(-0.4), Some(0.0)]));
    // a preset with a no-stroke layer carries sub_stroke
    let PresetPath::Path(shape) = preset_path(Some("cube"), 100.0, 100.0, Some(&[30000.0]), None) else { panic!("cube") };
    assert!(shape.sub_lum.is_some());
}
