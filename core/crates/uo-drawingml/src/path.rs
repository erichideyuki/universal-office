//! Path types shared by the preset engine, the outlines and the rasterizer: `Pt`, `PathSeg` and
//! `Subpaths` (the TS `Pt`, `PathSeg` and `PathSeg[][]` of `geometry.ts`).

/// A point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

/// A segment of a subpath, in absolute coordinates of the `w x h` rectangle.
///
/// `E` is an ellipse arc (`ctx.ellipse`): angles in RADIANS, `a0` to `a1`; the TS `ccw?` that is
/// absent counts as `false` for every consumer and for flatten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSeg {
    M { x: f64, y: f64 },
    L { x: f64, y: f64 },
    C { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Q { x1: f64, y1: f64, x: f64, y: f64 },
    E { cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64, ccw: bool },
    Z,
}

/// All the subpaths of a shape in one allocation (the TS `PathSeg[][]`).
///
/// **Empty subpaths are kept** (A7): `eval_preset` records the segments of a path even when it
/// has no drawable command, with its fill and stroke parallel, and `preset_path` tests the
/// number of subpaths. So [`Subpaths::end_sub`] always records an end, [`Subpaths::len`] counts
/// empty subpaths and [`Subpaths::iter`] and [`Subpaths::get`] yield `&[]` for them. Segments
/// pushed after the last `end_sub` belong to a subpath that is not finished yet and are not
/// visible through `len`, `iter` or `get`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subpaths {
    segs: Vec<PathSeg>,
    ends: Vec<u32>,
}

impl Subpaths {
    /// No subpaths.
    pub fn new() -> Subpaths {
        Subpaths::default()
    }

    /// Appends a segment to the subpath being built.
    pub fn push(&mut self, seg: PathSeg) {
        self.segs.push(seg);
    }

    /// Finishes the subpath being built (possibly empty) and starts the next.
    pub fn end_sub(&mut self) {
        self.ends.push(u32::try_from(self.segs.len()).unwrap_or(u32::MAX));
    }

    /// Number of finished subpaths, empty ones included.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// True when no subpath is finished.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The segments of subpath `i`, `None` past the last.
    pub fn get(&self, i: usize) -> Option<&[PathSeg]> {
        let end = usize::try_from(*self.ends.get(i)?).unwrap_or(usize::MAX).min(self.segs.len());
        let start = match i.checked_sub(1).and_then(|p| self.ends.get(p)) {
            Some(&s) => usize::try_from(s).unwrap_or(usize::MAX).min(end),
            None => 0,
        };
        self.segs.get(start..end)
    }

    /// The finished subpaths in order.
    pub fn iter(&self) -> impl Iterator<Item = &[PathSeg]> + '_ {
        (0..self.ends.len()).map(|i| self.get(i).unwrap_or(&[]))
    }

    /// Every segment of every subpath (and of the unfinished one), in order.
    pub fn segs(&self) -> &[PathSeg] {
        &self.segs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_subpaths_are_kept() {
        let mut s = Subpaths::new();
        assert!(s.is_empty());
        s.end_sub();
        s.push(PathSeg::M { x: 1.0, y: 2.0 });
        s.push(PathSeg::Z);
        s.end_sub();
        s.end_sub();
        s.push(PathSeg::L { x: 3.0, y: 4.0 });
        assert_eq!(s.len(), 3);
        assert_eq!(s.get(0), Some(&[][..]));
        assert_eq!(s.get(1), Some(&[PathSeg::M { x: 1.0, y: 2.0 }, PathSeg::Z][..]));
        assert_eq!(s.get(2), Some(&[][..]));
        assert_eq!(s.get(3), None);
        let lens: Vec<usize> = s.iter().map(<[PathSeg]>::len).collect();
        assert_eq!(lens, [0, 2, 0]);
        assert_eq!(s.segs().len(), 3);
    }
}
