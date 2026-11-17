//! The helpers at the end of `render.ts` (2723–2862) and the small conversions every family painter shares: formats
//! (`pickFmt`), the 3D light (`scaleRgb`), `mixWhite` of the render module, `minorLog`, `quartile`, `squarify`, `boxCats`,
//! `rank`, and the label merges (`mergeDLbls`, `mergePointLabel`).

#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::conv::{to_int32, to_uint8};
use uo_jsnum::math::{max, max_n, min, min_n, round};
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::utf16_slice;

use crate::emit::Rect;
use crate::error::{ChartError, JsErrorKind};
use crate::layout::Scale;
use crate::model::{DLbl, DLbls, DLblsCommon, FmtCode, Group, GroupKind, NumFmt, ShapeProps};

/// "General" as UTF-16 (the code `pickFmt` falls back to).
pub(crate) const GENERAL: [u16; 7] = [71, 101, 110, 101, 114, 97, 108];
/// "0%".
pub(crate) const PERCENT: [u16; 2] = [48, 37];

/// `pickFmt(nf, src)`: the recorded format (even linked: it is the cache of the source's format), except a linked
/// "General" → the series'.
pub(crate) fn pick_fmt<'f>(nf: Option<&'f NumFmt>, src: Option<&'f [u16]>) -> &'f [u16] {
    if let Some(n) = nf
        && (!n.linked || n.code.units() != GENERAL.as_slice())
    {
        return n.code.units();
    }
    src.or_else(|| nf.map(|n| n.code.units())).unwrap_or(&GENERAL)
}

/// `s.fmt` of an optional series format as the unit slice `pickFmt` takes.
pub(crate) fn units(f: Option<&FmtCode>) -> Option<&[u16]> {
    f.map(FmtCode::units)
}

/// `Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')` (lower case).
fn hex_channel(v: f64) -> String {
    let c = max(0.0, min(255.0, round(v)));
    if c.is_nan() { "NaN".to_owned() } else { format!("{:02x}", to_uint8(c)) }
}

/// `parseInt(hex.slice(1, 7), 16)` as the int32 the `>>` operators see.
fn hex_int(hex: &str) -> i32 {
    to_int32(parse_int(&utf16_slice(hex, 1, 7), 16))
}

/// `scaleRgb(hex, k)`: the channels multiplied by `k` (the light of the 3D faces); lower case `#rrggbb`.
pub(crate) fn scale_rgb(hex: &str, k: f64) -> String {
    let n = hex_int(hex);
    let ch = |s: i32| hex_channel(f64::from((n >> s) & 255) * k);
    format!("#{}{}{}", ch(16), ch(8), ch(0))
}

/// `mixWhite(hex, k)` of render.ts (not color.ts): mix with white, `k` = the fraction of white; lower case.
pub(crate) fn mix_white_hex(hex: &str, k: f64) -> String {
    let n = hex_int(hex);
    let ch = |s: i32| hex_channel(f64::from((n >> s) & 255) * (1.0 - k) + 255.0 * k);
    format!("#{}{}{}", ch(16), ch(8), ch(0))
}

/// `minorLog(sc)`: the minor ticks of a log scale.
pub(crate) fn minor_log(sc: &Scale) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let b = sc.log.unwrap_or(10.0);
    let mut v = sc.lo;
    while v < sc.hi * (1.0 + 1e-9) && out.len() < 2000 {
        let mut k = 1.0;
        while k < b {
            let x = v * k;
            if x <= sc.hi * (1.0 + 1e-9) {
                out.push(x);
            }
            k += 1.0;
        }
        v *= b;
    }
    out
}

/// `rank(g)`: paint order in one plot (areas, bars, then lines and scatter).
pub(crate) fn rank(g: &Group) -> f64 {
    match g.kind {
        GroupKind::Area => 0.0,
        GroupKind::Bar => 1.0,
        _ => 2.0,
    }
}

/// `boxCats(g)`: the distinct categories of a box & whisker group in order of appearance (no category = "1").
pub(crate) fn box_cats(g: &Group) -> Vec<Box<str>> {
    let mut out: Vec<Box<str>> = Vec::new();
    for s in &g.series {
        for i in 0..s.values.len() {
            let c: &str = s.cats.as_ref().and_then(|c| c.get(i)).map_or("1", |c| c);
            if !out.iter().any(|o| &**o == c) {
                out.push(c.into());
            }
        }
    }
    if out.is_empty() { vec!["1".into()] } else { out }
}

/// `quartile(v, p, exclusive)`: QUARTIL.EXC (position p(n+1)) or QUARTIL.INC (p(n-1)+1), interpolated.
pub(crate) fn quartile(v: &[f64], p: f64, exclusive: bool) -> f64 {
    let n = v.len() as f64;
    if v.len() == 1 {
        return v.first().copied().unwrap_or(f64::NAN);
    }
    let mut h = if exclusive { p * (n + 1.0) } else { p * (n - 1.0) + 1.0 };
    h = max(1.0, min(n, h));
    let k = uo_jsnum::math::floor(h);
    let f = h - k;
    let at = |i: f64| v.get(to_ix(i - 1.0)).copied().unwrap_or(f64::NAN);
    at(k) + if k < n { f * (at(k + 1.0) - at(k)) } else { 0.0 }
}

/// `squarify(sizes, r)`: "squarified" rectangles (Bruls, Huizing and van Wijk) for sizes in descending order.
pub(crate) fn squarify(sizes: &[f64], r: Rect) -> Vec<Rect> {
    let total = sizes.iter().fold(0.0, |a, b| a + b);
    if !(total > 0.0) || r.w <= 0.0 || r.h <= 0.0 {
        return sizes.iter().map(|_| Rect { x: r.x, y: r.y, w: 0.0, h: 0.0 }).collect();
    }
    let scale = (r.w * r.h) / total;
    let areas: Vec<f64> = sizes.iter().map(|s| s * scale).collect();
    let mut out: Vec<Rect> = Vec::new();
    let mut rect = r;
    let mut i = 0;
    let worst = |row: &[f64], side: f64| -> f64 {
        let s = row.iter().fold(0.0, |a, b| a + b);
        let mx = max_n(row);
        let mn = min_n(row);
        max((side * side * mx) / (s * s), (s * s) / (side * side * mn))
    };
    while i < areas.len() {
        let side = min(rect.w, rect.h);
        let mut row: Vec<f64> = vec![areas.get(i).copied().unwrap_or(f64::NAN)];
        let mut j = i + 1;
        while let Some(&aj) = areas.get(j) {
            let mut with = row.clone();
            with.push(aj);
            if worst(&with, side) <= worst(&row, side) {
                row.push(aj);
                j += 1;
            } else {
                break;
            }
        }
        let s = row.iter().fold(0.0, |a, b| a + b);
        if rect.w >= rect.h {
            let cw = s / rect.h;
            let mut y = rect.y;
            for a in &row {
                let hh = a / cw;
                out.push(Rect { x: rect.x, y, w: cw, h: hh });
                y += hh;
            }
            rect = Rect { x: rect.x + cw, y: rect.y, w: rect.w - cw, h: rect.h };
        } else {
            let rh = s / rect.w;
            let mut x = rect.x;
            for a in &row {
                let ww = a / rh;
                out.push(Rect { x, y: rect.y, w: ww, h: rh });
                x += ww;
            }
            rect = Rect { x: rect.x, y: rect.y + rh, w: rect.w, h: rect.h - rh };
        }
        i = j;
    }
    out
}

/// A float index that checks have proved to be a non-negative integer in range, as a `usize` (NaN, negative: 0; capped).
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=2^31 first")]
pub(crate) fn to_ix(x: f64) -> usize {
    if x.is_nan() || x < 0.0 { 0 } else { min(x, 2_147_483_648.0) as usize }
}

/// A `usize` as the `i32` the data selection takes.
pub(crate) fn i32_of(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(i32::MAX)
}

/// `new Array(n)` / `Array.from({ length: n })` of a float length: the RangeError of V8 for a length that is not an
/// array length; the engine's own cap (10,000,000, declared) also answers it.
pub(crate) fn array_len(n: f64) -> Result<usize, ChartError> {
    if n >= 0.0 && n == uo_jsnum::math::floor(n) && n <= 10_000_000.0 {
        Ok(to_ix(n))
    } else if n.is_nan() || n < 0.0 {
        // `Array.from({ length: NaN or negative })`: ToLength = 0
        Ok(0)
    } else {
        Err(ChartError::Throws(JsErrorKind::RangeError, "Invalid array length"))
    }
}

/// `{ ...a.sp, ...b.sp }` of two shapes that never hold an `undefined` key.
fn merge_sp(a: &ShapeProps, b: &ShapeProps) -> ShapeProps {
    ShapeProps {
        fill: b.fill.clone().or_else(|| a.fill.clone()),
        line: b.line.clone().or_else(|| a.line.clone()),
        shadow: b.shadow.clone().or_else(|| a.shadow.clone()),
    }
}

/// `{ ...a, ...stripUndef(b) }` of the label fields shared by `c:dLbls` and `c:dLbl`, with `txPr` and `sp` merged key by key.
fn merge_common(a: &DLblsCommon, b: &DLblsCommon) -> DLblsCommon {
    DLblsCommon {
        delete: b.delete.or(a.delete),
        show_val: b.show_val.or(a.show_val),
        show_cat: b.show_cat.or(a.show_cat),
        show_ser: b.show_ser.or(a.show_ser),
        show_pct: b.show_pct.or(a.show_pct),
        show_bubble: b.show_bubble.or(a.show_bubble),
        show_legend_key: b.show_legend_key.or(a.show_legend_key),
        pos: b.pos.clone().or_else(|| a.pos.clone()),
        sep: b.sep.clone().or_else(|| a.sep.clone()),
        num_fmt: b.num_fmt.clone().or_else(|| a.num_fmt.clone()),
        tx_pr: b.tx_pr.clone().over(a.tx_pr.clone()),
        sp: merge_sp(&a.sp, &b.sp),
        leader_lines: b.leader_lines.clone().or_else(|| a.leader_lines.clone()),
    }
}

/// `mergeDLbls(a, b)`: the series' labels over the group's.
pub(crate) fn merge_dlbls(a: Option<&DLbls>, b: Option<&DLbls>) -> Option<DLbls> {
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(b.clone()),
        (Some(a), None) => Some(a.clone()),
        (Some(a), Some(b)) => {
            Some(DLbls { common: merge_common(&a.common, &b.common), points: b.points.clone().or_else(|| a.points.clone()) })
        }
    }
}

/// `mergePointLabel(dl, pd)`: the point's label over the series' (defined fields win; `spPr`/`txPr` merge key by key).
pub(crate) fn merge_point_label(dl: &DLbls, pd: &DLbl) -> DLblsCommon {
    merge_common(&dl.common, &pd.common)
}
