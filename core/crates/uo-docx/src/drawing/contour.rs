//! The wrap contour (`wp:wrapPolygon`, units 0..21600) of a picture with transparency — what
//! Word writes when "Tight"/"Through" wrapping is chosen (`shared/drawingml/src/contour.ts`,
//! `wrapContour`, 1:1). A 96-cell grid on the longer side, vertices on the cells, the figure
//! dilated by two cells, the gaps that open upward or downward filled (the outline follows the
//! leftmost and rightmost cell of each row), then Douglas–Peucker with one cell.
//!
//! The TS reads only `rgba[(y * w + x) * 4 + 3]`; the code below is the TS in the same order of
//! operations, every `Math.*` through `uo_jsnum::math` (R1). The TS never touches
//! `uo-drawingml`'s raster or presets (contrary to what the module brief assumed): the only
//! numeric functions it uses are `Math.round`, `Math.floor`, `Math.min`, `Math.max` and
//! `Math.hypot`.

use uo_jsnum::conv::{to_uint32, truthy};
use uo_jsnum::math::{abs, floor, hypot, max, min, round};

/// Why a contour was not computed. The TS has no error path (a short buffer reads as
/// `undefined`, which is not `0`, so the missing pixels count as opaque); here the caller's
/// buffer must cover `w * h` pixels, as `getImageData` always gives (declared in the record,
/// decision C2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContourError {
    /// `rgba` has fewer than `w * h * 4` bytes
    BufferSize,
}

/// A vertex (x, y) in 21600ths of the picture box: always an integer value in `0..=21600`.
pub type Vertex = [f64; 2];

/// `wrapContour(rgba, w, h)`: the closed polygon (the first vertex repeated at the end) in
/// 21600ths, or `Ok(None)` when the picture has no useful transparency (the default rectangle
/// stays): `w` or `h` is 0, no pixel has alpha, or every pixel does, or the dilated figure fills
/// the grid. `rgba` is `w * h` RGBA pixels; only the alpha byte is read. `w` and `h` are
/// integers (the TS takes any JS number; its callers pass the integer size of a bitmap).
///
/// Time is O(`w * h`) and the grid is at most 96 x 96, so no allocation depends on the size.
pub fn wrap_contour(rgba: &[u8], w: u32, h: u32) -> Result<Option<Vec<Vertex>>, ContourError> {
    if w < 1 || h < 1 {
        return Ok(None);
    }
    let bytes = u64::from(w).checked_mul(u64::from(h)).and_then(|n| n.checked_mul(4)).ok_or(ContourError::BufferSize)?;
    if u64::try_from(rgba.len()).map_err(|_| ContourError::BufferSize)? < bytes {
        return Err(ContourError::BufferSize);
    }
    const N: f64 = 96.0;
    let wf = f64::from(w);
    let hf = f64::from(h);
    let gwf = if wf >= hf { N } else { max(1.0, round((N * wf) / hf)) };
    let ghf = if hf >= wf { N } else { max(1.0, round((N * hf) / wf)) };
    let gw = to_uint32(gwf);
    let gh = to_uint32(ghf);
    let (gwu, ghu) = (to_usize(gw), to_usize(gh));
    let mut occ = vec![0u8; gwu * ghu];
    let mut filled = 0usize;
    for y in 0..h {
        let gy = to_uint32(min(ghf - 1.0, floor((f64::from(y) * ghf) / hf)));
        for x in 0..w {
            let at = (u64::from(y) * u64::from(w) + u64::from(x)) * 4 + 3;
            // `at < bytes <= rgba.len()` (checked above), so the byte exists
            if rgba.get(usize::try_from(at).map_err(|_| ContourError::BufferSize)?).copied() == Some(0) {
                continue;
            }
            let gx = to_uint32(min(gwf - 1.0, floor((f64::from(x) * gwf) / wf)));
            let k = to_usize(gy) * gwu + to_usize(gx);
            if let Some(cell) = occ.get_mut(k)
                && *cell == 0
            {
                *cell = 1;
                filled += 1;
            }
        }
    }
    if filled == 0 || filled == gwu * ghu {
        return Ok(None);
    }
    // dilate by 2 cells (a square: the corners of the L stay straight, as in Word)
    const R: i32 = 2;
    let mut dil = vec![0u8; gwu * ghu];
    for y in 0..gh {
        for x in 0..gw {
            if occ.get(to_usize(y) * gwu + to_usize(x)).copied() == Some(0) {
                continue;
            }
            for dy in -R..=R {
                for dx in -R..=R {
                    let (Some(xx), Some(yy)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                        continue;
                    };
                    if xx < gw
                        && yy < gh
                        && let Some(cell) = dil.get_mut(to_usize(yy) * gwu + to_usize(xx))
                    {
                        *cell = 1;
                    }
                }
            }
        }
    }
    if dil.iter().all(|&v| v == 1) {
        return Ok(None);
    }
    // the extremes of each grid row
    let mut left: Vec<Vertex> = Vec::new();
    let mut right: Vec<Vertex> = Vec::new();
    for y in 0..gh {
        let mut l: Option<u32> = None;
        let mut r: Option<u32> = None;
        for x in 0..gw {
            if dil.get(to_usize(y) * gwu + to_usize(x)).copied() == Some(1) {
                if l.is_none() {
                    l = Some(x);
                }
                r = Some(x);
            }
        }
        let (Some(l), Some(r)) = (l, r) else {
            continue;
        };
        left.push([f64::from(l), f64::from(y)]);
        right.push([f64::from(r), f64::from(y)]);
    }
    if left.is_empty() {
        return Ok(None);
    }
    right.reverse();
    let mut ring = left;
    ring.extend(right);
    let simple = douglas_peucker(ring, 1.0);
    let mut pts: Vec<Vertex> = simple.iter().map(|&[x, y]| [round((x * 21600.0) / gwf), round((y * 21600.0) / ghf)]).collect();
    // `pts[0]!`: the simplified ring is never empty (the ring has at least two vertices and both
    // ends are always kept), but an empty one is `None`, not a panic
    let Some(&first) = pts.first() else {
        return Ok(None);
    };
    pts.push(first);
    Ok(Some(pts))
}

fn to_usize(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// `douglasPeucker(pts, eps)` (contour.ts): the vertices of `pts` kept by the iterative
/// Douglas–Peucker, same order.
fn douglas_peucker(pts: Vec<Vertex>, eps: f64) -> Vec<Vertex> {
    if pts.len() < 4 {
        return pts;
    }
    let mut keep = vec![0u8; pts.len()];
    let last = pts.len() - 1;
    keep[0] = 1;
    keep[last] = 1;
    let mut stack: Vec<(usize, usize)> = vec![(0, last)];
    while let Some((a, b)) = stack.pop() {
        let [ax, ay] = pts[a];
        let [bx, by] = pts[b];
        let hyp = hypot(bx - ax, by - ay);
        let len = if truthy(hyp) { hyp } else { 1.0 };
        let mut best = -1.0f64;
        let mut bi: Option<usize> = None;
        for (i, &[px, py]) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = abs((bx - ax) * (ay - py) - (ax - px) * (by - ay)) / len;
            if d > best {
                best = d;
                bi = Some(i);
            }
        }
        // `best > eps && bi > 0`: `bi` is an index above `a`, so above 0 whenever it is set
        if let Some(bi) = bi
            && best > eps
            && bi > 0
        {
            keep[bi] = 1;
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    pts.into_iter().zip(keep).filter(|&(_, k)| k != 0).map(|(p, _)| p).collect()
}
