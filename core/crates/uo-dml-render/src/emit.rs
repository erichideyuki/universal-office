//! The emission parameters and result shape (frontier B4). The items themselves are the Slides list
//! of `uo-render` (`uo-render.md` "Slides extensions (plan §4.2) — design"), added in the next wave;
//! until then this module only fixes what the callers pass and get back.

/// One viewport of an emission (`paintSlideTo`/`paintShapesTo` arguments and options). The list is
/// valid for exactly these numbers: every coordinate is `ox + X × scale` in JS doubles and several
/// decisions are taken in px (`uo-render.md` S7).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct EmitParams {
    /// px per EMU.
    pub scale: f64,
    pub ox: f64,
    pub oy: f64,
    /// Slide size in EMU (0 for Sheet drawings, as `paintShapesTo` is called there).
    pub slide_w: f64,
    pub slide_h: f64,
    /// `vertOverflow/horzOverflow="clip"` honoured (Excel: yes, measured; Slides: off).
    pub clip_overflow: bool,
    /// The shape whose text the editor's inline overlay covers (`suppressTextShapeIdx`).
    pub suppress_text: Option<u32>,
}

/// A piece of the emitted output in z-order: a range of display-list items, or a chart the host
/// paints itself (Sheet's charts inside drawings use Sheet's chart engine with Excel defaults:
/// the `flush()` batching of drawings.ts:306–322).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Segment {
    Items { start: u32, end: u32 },
    HostChart { shape: u32, rect: [f64; 4] },
}

/// The segments of one emission.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Segments(pub Vec<Segment>);
