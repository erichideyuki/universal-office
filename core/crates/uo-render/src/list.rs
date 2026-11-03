//! A list of items with its side columns: the storage of a page and of every intermediate item
//! array of a producer (cell contents, table rows, static frames), so items move between them
//! by copying their column ranges ("Types (T)", decision T2).

use crate::ids::{FaceId, Sym, TextFxId};
use crate::opt::OptF64;
use crate::types::{Item, PathCmd, RunFlags, Stroke};

/// A range of a column: `start..start + len`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

impl Span {
    /// The `usize` range (`None` when `start + len` overflows).
    #[must_use]
    pub fn range(self) -> Option<std::ops::Range<usize>> {
        let s = self.start as usize;
        s.checked_add(self.len as usize).map(|e| s..e)
    }
}

/// `gid` of a glyph that has none (the TS `gid?: number` absent).
pub const NO_GID: u32 = u32::MAX;

/// The glyphs of every run of the list, column by column (`{ cp, x, gid? }[]`): 12 bytes a glyph,
/// 16 once some glyph of the list has a `gid`.
#[derive(Clone, Debug, Default)]
pub struct GlyphColumns {
    /// code points (`u32`: lone surrogates occur, RQ9)
    pub cp: Vec<u32>,
    /// absolute x
    pub x: Vec<f64>,
    /// empty while no glyph has a `gid`; then as long as `cp`, [`NO_GID`] where absent
    pub gid: Vec<u32>,
}

impl GlyphColumns {
    #[must_use]
    pub fn len(&self) -> usize {
        self.cp.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cp.is_empty()
    }

    /// The `gid` of glyph `i`.
    #[must_use]
    pub fn gid(&self, i: usize) -> Option<u32> {
        self.gid.get(i).copied().filter(|&g| g != NO_GID)
    }

    fn push(&mut self, cp: u32, x: f64, gid: Option<u32>) {
        if let Some(g) = gid {
            if self.gid.len() < self.cp.len() {
                self.gid.resize(self.cp.len(), NO_GID);
            }
            self.gid.push(g);
        } else if !self.gid.is_empty() {
            self.gid.push(NO_GID);
        }
        self.cp.push(cp);
        self.x.push(x);
    }
}

/// The rarely-read fields of a glyph run (`text`, `scaleX`, `alpha`, `fx`), one per run, so the
/// run item stays small.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunSide {
    /// `text` (UTF-16 in `ItemList::text`; RQ7: read by the P1 `Canon` only, not encoded)
    pub text: Span,
    pub scale_x: OptF64,
    pub alpha: OptF64,
    pub fx: Option<TextFxId>,
}

/// A list grew past `u32` entries in one of its columns (the producer's work budget stops long
/// before; this keeps the spans exact instead of truncating).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapacityError;

impl std::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("display list column past u32 entries")
    }
}

impl std::error::Error for CapacityError {}

fn len32(n: usize) -> Result<u32, CapacityError> {
    u32::try_from(n).map_err(|_| CapacityError)
}

/// A glyph run to push: everything of `GlyphRunItem` but the glyphs.
#[derive(Clone, Copy, Debug)]
pub struct GlyphRunHead<'t> {
    pub face: FaceId,
    pub color: Sym,
    pub size_pt: f64,
    pub y: f64,
    pub text: &'t [u16],
    pub scale_x: OptF64,
    pub alpha: OptF64,
    pub fx: Option<TextFxId>,
    pub flags: RunFlags,
}

/// Items in paint order and their columns.
#[derive(Clone, Debug, Default)]
pub struct ItemList {
    pub items: Vec<Item>,
    pub glyphs: GlyphColumns,
    pub runs: Vec<RunSide>,
    pub text: Vec<u16>,
    pub path_cmds: Vec<PathCmd>,
}

impl ItemList {
    #[must_use]
    pub fn new() -> ItemList {
        ItemList::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Appends a glyph run with its glyphs `(cp, x, gid)`.
    pub fn push_glyphs(
        &mut self,
        head: GlyphRunHead<'_>,
        glyphs: impl IntoIterator<Item = (u32, f64, Option<u32>)>,
    ) -> Result<(), CapacityError> {
        let g0 = self.glyphs.len();
        for (cp, x, gid) in glyphs {
            self.glyphs.push(cp, x, gid);
        }
        let glyphs = Span { start: len32(g0)?, len: len32(self.glyphs.len() - g0)? };
        let t0 = self.text.len();
        self.text.extend_from_slice(head.text);
        let text = Span { start: len32(t0)?, len: len32(head.text.len())? };
        let run = len32(self.runs.len())?;
        self.runs.push(RunSide { text, scale_x: head.scale_x, alpha: head.alpha, fx: head.fx });
        self.items.push(Item::Glyphs {
            face: head.face,
            color: head.color,
            size_pt: head.size_pt,
            y: head.y,
            glyphs,
            run,
            flags: head.flags,
        });
        Ok(())
    }

    /// Appends a path item with its commands.
    pub fn push_path(&mut self, cmds: &[PathCmd], fill: Option<Sym>, stroke: Option<Stroke>, even_odd: bool) -> Result<(), CapacityError> {
        let c0 = self.path_cmds.len();
        self.path_cmds.extend_from_slice(cmds);
        let cmds = Span { start: len32(c0)?, len: len32(cmds.len())? };
        self.items.push(Item::Path { cmds, fill, stroke, even_odd });
        Ok(())
    }

    /// Appends an item without column payload (rect, line, image, shape, clip, xform). A glyph
    /// or path item must come through [`ItemList::push_glyphs`] / [`ItemList::push_path`] or
    /// [`ItemList::push_shifted`], since its spans point into the columns of its own list.
    pub fn push_plain(&mut self, item: Item) {
        self.items.push(item);
    }

    /// The glyphs of a run: `(cp, x, gid)`.
    pub fn glyphs_of(&self, glyphs: Span) -> impl Iterator<Item = (u32, f64, Option<u32>)> + '_ {
        let r = glyphs.range().unwrap_or(0..0);
        let s = r.start;
        let cp = self.glyphs.cp.get(r.clone()).unwrap_or(&[]);
        let x = self.glyphs.x.get(r).unwrap_or(&[]);
        cp.iter().zip(x).enumerate().map(move |(k, (&c, &x))| (c, x, self.glyphs.gid(s + k)))
    }

    /// The side fields of run `run`.
    #[must_use]
    pub fn run(&self, run: u32) -> Option<&RunSide> {
        self.runs.get(run as usize)
    }

    /// The text of a span.
    #[must_use]
    pub fn text_of(&self, text: Span) -> &[u16] {
        text.range().and_then(|r| self.text.get(r)).unwrap_or(&[])
    }

    /// The commands of a path span.
    #[must_use]
    pub fn cmds_of(&self, cmds: Span) -> &[PathCmd] {
        cmds.range().and_then(|r| self.path_cmds.get(r)).unwrap_or(&[])
    }

    /// Appends item `i` of `src` moved by `(dx, dy)`: the TS `shiftItem(it, dy, dx)` (table.ts:1392),
    /// field by field in its arithmetic — glyph x move only when `dx` is truthy (non-zero, not
    /// NaN), a clip push without `y` is kept as is, a transform keeps `M'(p + d) = M(p) + d`.
    /// Out-of-range `i` appends nothing.
    pub fn push_shifted(&mut self, src: &ItemList, i: usize, dy: f64, dx: f64) -> Result<(), CapacityError> {
        let Some(it) = src.items.get(i) else { return Ok(()) };
        let moved = match it {
            Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
                let side = src.run(*run).copied().unwrap_or_default();
                let head = GlyphRunHead {
                    face: *face,
                    color: *color,
                    size_pt: *size_pt,
                    y: *y + dy,
                    text: src.text_of(side.text),
                    scale_x: side.scale_x,
                    alpha: side.alpha,
                    fx: side.fx,
                    flags: *flags,
                };
                let truthy = uo_jsnum::conv::truthy(dx);
                return self.push_glyphs(head, src.glyphs_of(*glyphs).map(|(c, x, g)| (c, if truthy { x + dx } else { x }, g)));
            }
            Item::Path { cmds, fill, stroke, even_odd } => {
                let shifted: Vec<PathCmd> = src
                    .cmds_of(*cmds)
                    .iter()
                    .map(|c| match *c {
                        PathCmd::Z => PathCmd::Z,
                        PathCmd::C { x1, y1, x2, y2, x, y } => {
                            PathCmd::C { x1: x1 + dx, y1: y1 + dy, x2: x2 + dx, y2: y2 + dy, x: x + dx, y: y + dy }
                        }
                        PathCmd::M { x, y } => PathCmd::M { x: x + dx, y: y + dy },
                        PathCmd::L { x, y } => PathCmd::L { x: x + dx, y: y + dy },
                    })
                    .collect();
                return self.push_path(&shifted, *fill, *stroke, *even_odd);
            }
            Item::Rect { x, y, w, h, fill, alpha } => Item::Rect { x: *x + dx, y: *y + dy, w: *w, h: *h, fill: *fill, alpha: *alpha },
            Item::Line { x1, y1, x2, y2, color, width_pt, dash } => {
                Item::Line { x1: *x1 + dx, y1: *y1 + dy, x2: *x2 + dx, y2: *y2 + dy, color: *color, width_pt: *width_pt, dash: *dash }
            }
            Item::Image(im) => {
                let mut im = im.clone();
                im.x += dx;
                im.y += dy;
                Item::Image(im)
            }
            Item::Shape(sh) => {
                let mut sh = sh.clone();
                sh.x += dx;
                sh.y += dy;
                Item::Shape(sh)
            }
            Item::ClipPush { x, y, w, h } => match y.get() {
                None => it.clone(),
                Some(yv) => Item::ClipPush { x: OptF64::some(x.or(0.0) + dx), y: OptF64::some(yv + dy), w: *w, h: *h },
            },
            Item::XformPush { m: Some(m) } => {
                let [a, b, c, d, e, f] = **m;
                Item::XformPush { m: Some(Box::new([a, b, c, d, e + dx - (a * dx + c * dy), f + dy - (b * dx + d * dy)])) }
            }
            Item::XformPush { m: None } | Item::ClipPop | Item::XformPop => it.clone(),
        };
        self.items.push(moved);
        Ok(())
    }

    /// Appends every item of `src` moved by `(dx, dy)` (`items.push(...src.map(shift))`).
    pub fn extend_shifted(&mut self, src: &ItemList, dy: f64, dx: f64) -> Result<(), CapacityError> {
        for i in 0..src.items.len() {
            self.push_shifted(src, i, dy, dx)?;
        }
        Ok(())
    }

    /// Heap bytes held (M3 accounting).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let boxed: usize = self
            .items
            .iter()
            .map(|it| match it {
                Item::Image(im) => {
                    size_of::<crate::types::ImageItem>() + im.pic.as_ref().map_or(0, |_| size_of::<crate::types::PictureLook>())
                }
                Item::Shape(_) => size_of::<crate::types::ShapeItem>(),
                Item::XformPush { m: Some(_) } => size_of::<[f64; 6]>(),
                _ => 0,
            })
            .sum();
        self.items.capacity() * size_of::<Item>()
            + self.glyphs.cp.capacity() * 4
            + self.glyphs.x.capacity() * 8
            + self.glyphs.gid.capacity() * 4
            + self.runs.capacity() * size_of::<RunSide>()
            + self.text.capacity() * 2
            + self.path_cmds.capacity() * size_of::<PathCmd>()
            + boxed
    }
}
