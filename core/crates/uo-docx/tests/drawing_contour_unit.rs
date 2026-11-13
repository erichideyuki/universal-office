//! Unit tests of `uo_docx::drawing` (the contour and the Shape Styles gallery moved in from
//! `shared/drawingml`, uo-drawingml gate decision G3). The three cases of the TS test
//! (`shared/drawingml/test/contour.test.ts`) plus the decisions of the port; the parity with V8
//! over thousands of cases is `parity_contour.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::drawing::{ContourError, SHAPE_STYLE_PRESETS, find_shape_style, shape_style_preset, wrap_contour};

/// RGBA `w x h` with `inside(x, y)` opaque and the rest transparent (the TS test's `img`).
fn img(w: u32, h: u32, inside: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let mut d = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            if inside(x, y) {
                d[((y * w + x) * 4 + 3) as usize] = 255;
            }
        }
    }
    d
}

fn contour(d: &[u8], w: u32, h: u32) -> Option<Vec<[f64; 2]>> {
    wrap_contour(d, w, h).unwrap()
}

#[test]
fn opaque_image_has_no_contour() {
    assert_eq!(contour(&img(48, 36, |_, _| true), 48, 36), None);
}

#[test]
fn circle_extremes_are_cells_7_and_88_of_the_96_grid() {
    let c = contour(
        &img(240, 240, |x, y| {
            let (dx, dy) = (f64::from(x) + 0.5 - 120.0, f64::from(y) + 0.5 - 120.0);
            (dx * dx + dy * dy).sqrt() <= 96.0
        }),
        240,
        240,
    )
    .unwrap();
    let xs: Vec<f64> = c.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = c.iter().map(|p| p[1]).collect();
    assert_eq!(xs.iter().copied().fold(f64::INFINITY, uo_jsnum::math::min), 1575.0);
    assert_eq!(xs.iter().copied().fold(f64::NEG_INFINITY, uo_jsnum::math::max), 19800.0);
    assert_eq!(ys.iter().copied().fold(f64::INFINITY, uo_jsnum::math::min), 1575.0);
    assert_eq!(ys.iter().copied().fold(f64::NEG_INFINITY, uo_jsnum::math::max), 19800.0);
    assert_eq!(c.first(), c.last());
}

#[test]
fn l_shape_keeps_straight_corners() {
    let c = contour(&img(240, 240, |x, y| (24..216).contains(&x) && (24..216).contains(&y) && (x < 96 || y >= 144)), 240, 240).unwrap();
    let cells: Vec<(f64, f64)> = c.iter().map(|p| (p[0] / 225.0, p[1] / 225.0)).collect();
    assert!(cells.contains(&(7.0, 7.0)));
    assert!(cells.contains(&(7.0, 88.0)));
    assert!(cells.contains(&(88.0, 88.0)));
}

#[test]
fn one_pixel_dilates_to_a_five_cell_square() {
    let c = contour(&img(96, 96, |x, y| x == 48 && y == 48), 96, 96).unwrap();
    assert_eq!(c, vec![[10350.0, 10350.0], [10350.0, 11250.0], [11250.0, 11250.0], [11250.0, 10350.0], [10350.0, 10350.0]]);
}

#[test]
fn nothing_to_wrap_is_none() {
    assert_eq!(contour(&[], 0, 0), None);
    assert_eq!(contour(&[], 0, 5), None);
    assert_eq!(contour(&[], 5, 0), None);
    assert_eq!(contour(&vec![0u8; 4 * 30 * 30], 30, 30), None);
    // every grid cell filled, or the dilated figure fills it
    assert_eq!(contour(&img(96, 96, |_, _| true), 96, 96), None);
    assert_eq!(contour(&img(96, 96, |x, y| (2..94).contains(&x) && (2..94).contains(&y)), 96, 96), None);
    assert!(contour(&img(96, 96, |x, y| (3..93).contains(&x) && (3..93).contains(&y)), 96, 96).is_some());
}

#[test]
fn only_the_alpha_byte_is_read() {
    let mut d = img(64, 64, |x, y| x > 10 && x < 50 && y > 5 && y < 40);
    let a = contour(&d, 64, 64);
    for px in d.as_chunks_mut::<4>().0 {
        px[0] = 0xff;
        px[1] = 0x13;
        px[2] = 0x77;
        if px[3] != 0 {
            px[3] = 1;
        }
    }
    assert_eq!(contour(&d, 64, 64), a);
}

#[test]
fn the_grid_follows_the_longer_side() {
    // 192 x 96: 96 x 48 cells, so a full-height box spans the whole 0..21600 range in y
    let c = contour(&img(192, 96, |x, _| (60..100).contains(&x)), 192, 96).unwrap();
    assert!(c.iter().all(|p| p[1] == 0.0 || p[1] == 21600.0 || (0.0..=21600.0).contains(&p[1])));
    let ys: Vec<f64> = c.iter().map(|p| p[1]).collect();
    assert_eq!(ys.iter().copied().fold(f64::INFINITY, uo_jsnum::math::min), 0.0);
    // the last grid row is row 47 of 48: 47 * 21600 / 48
    assert_eq!(ys.iter().copied().fold(f64::NEG_INFINITY, uo_jsnum::math::max), 21150.0);
    // a 3 x 500 picture has a single grid row (round(96 * 3 / 500) = 1)
    let thin = contour(&img(500, 3, |x, _| (100..200).contains(&x)), 500, 3).unwrap();
    assert!(thin.iter().all(|p| p[1] == 0.0));
}

#[test]
fn a_short_buffer_is_refused_and_a_long_one_is_fine() {
    let d = img(40, 40, |x, y| x < 20 && y < 20);
    assert_eq!(wrap_contour(&d[..d.len() - 1], 40, 40), Err(ContourError::BufferSize));
    assert_eq!(wrap_contour(&[], 40, 40), Err(ContourError::BufferSize));
    assert_eq!(wrap_contour(&[0; 7], u32::MAX, u32::MAX), Err(ContourError::BufferSize));
    let mut long = d.clone();
    long.extend_from_slice(&[255; 33]);
    assert_eq!(wrap_contour(&long, 40, 40), wrap_contour(&d, 40, 40));
    // zero sizes are a plain `None`, whatever the buffer
    assert_eq!(wrap_contour(&[], 0, u32::MAX), Ok(None));
}

#[test]
fn a_big_picture_costs_its_pixels_only() {
    let (w, h) = (2000u32, 1500u32);
    let d = img(w, h, |x, y| x > 100 && x < 1900 && y > 200 && y < 1300 && (x + y) % 7 != 0);
    let c = contour(&d, w, h).unwrap();
    assert!(c.len() >= 5);
    assert!(c.iter().all(|p| (0.0..=21600.0).contains(&p[0]) && (0.0..=21600.0).contains(&p[1])));
}

#[test]
fn shape_styles_table_and_lookups() {
    assert_eq!(SHAPE_STYLE_PRESETS.len(), 77);
    assert_eq!(shape_style_preset(1.0), SHAPE_STYLE_PRESETS.first());
    assert_eq!(shape_style_preset(77.0), SHAPE_STYLE_PRESETS.last());
    for bad in [0.0, -0.0, 78.0, 1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 4_294_967_297.0, 1.0000000000000002] {
        assert_eq!(shape_style_preset(bad), None, "{bad}");
    }
    assert_eq!(SHAPE_STYLE_PRESETS[0].sp_pr, "<a:ln/>");
    assert!(SHAPE_STYLE_PRESETS[0].style.starts_with("<wps:style><a:lnRef idx=\"2\"><a:schemeClr val=\"dk1\"/>"));
    // 1..=42 are the theme styles (the empty ln of the theme styles), 43.. the presets
    assert!((0..42).all(|i| SHAPE_STYLE_PRESETS[i].style.contains("schemeClr")));
    for (i, p) in SHAPE_STYLE_PRESETS.iter().enumerate() {
        assert!(p.style.starts_with("<wps:style>") && p.style.ends_with("</wps:style>"), "{i}");
        assert_eq!(find_shape_style(p.sp_pr, p.style), Some(u32::try_from(i + 1).unwrap()));
    }
    assert_eq!(find_shape_style("<a:ln/>", ""), None);
    assert_eq!(find_shape_style("", SHAPE_STYLE_PRESETS[0].style), None);
}

#[test]
fn shape_styles_source_unchanged() {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(crate_dir.join("src/data/shape_styles.rs")).unwrap();
    let line = text.lines().find_map(|l| l.strip_prefix("// source: ")).unwrap();
    let (path, hash) = line.split_once(" sha256 ").unwrap();
    assert_eq!(path, "shared/drawingml/src/shape-styles.ts");
    let bytes = std::fs::read(repo.join(path)).unwrap();
    let got = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(&bytes));
    assert_eq!(got, hash, "{path} changed since src/data/shape_styles.rs was generated: run tools/gen-shape-styles.mjs");
}

#[test]
fn shape_styles_regenerate_identically() {
    if !uo_testkit::opt_in("UO_TS_PARITY", "uo-docx shape styles regeneration (node tools/gen-shape-styles.mjs --check)") {
        return;
    }
    let status = std::process::Command::new("node")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/gen-shape-styles.mjs"))
        .arg("--check")
        .status()
        .unwrap();
    assert!(status.success(), "src/data/shape_styles.rs is stale against UO_TS_ROOT");
}
