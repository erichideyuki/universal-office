//! The result of a layout (LA5): uo-render pages and resources plus the Write geometry section
//! (K14, RQ2) — lines and their characters, paragraph formats, placed objects and cells, the
//! header/footer item count, the section and displayed number, and the Word-only item metadata
//! (`lineBox`, `lineNum`, `ClipItem.src`) that leaves the shared items. Plain data: no borrow of
//! the model, so a `Layout` outlives edits and crosses to JS by encoding (LA1.5).

use std::fmt::Write as _;
use std::sync::Arc;

use indexmap::IndexSet;

use crate::keys::TbKey;
pub use crate::model::HfWhich;
use crate::model::{DrawingId, HfPartId, MediaId, NoteKind, NumKey, RunRef, Utf16};
use uo_render::{ImageId, Page, Resources, Span};

/// The result of one `layout()` call.
#[derive(Debug, Default)]
pub struct Layout {
    pub pages: Vec<LayoutPage>,
    /// faces (by provider answer), images (by TS object identity, RQ3), text effects, page fills,
    /// custom paths, interned strings
    pub resources: Resources,
    /// where each image of `resources.images` comes from (same index)
    pub image_origins: Vec<ImageOrigin>,
    /// TS `warnings` (a `Set<string>`: insertion order)
    pub warnings: IndexSet<Box<str>>,
    /// provider misses (the shell loads them and lays out again, as the viewer does)
    pub missing_faces: Vec<FaceRequest>,
    /// `onField`: the last value of each field run, first-set order (`collect_fields`)
    pub fields: Option<Vec<(RunRef, Utf16)>>,
    pub stamp: LayoutStamp,
}

/// What a layout was computed from (the binding reuses its last `Layout` while unchanged, G21).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutStamp {
    pub formatting_version: u64,
}

/// A face the provider did not have.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FaceRequest {
    pub family: Box<str>,
    pub bold: bool,
    pub italic: bool,
}

/// Where an image of the layout comes from (K12, RQ3): the identity is the `ImageId`; the bytes
/// are shared by content underneath.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageOrigin {
    /// a model drawing's media (one id per drawing object, as the TS `InlineImage` objects)
    Media { media: MediaId, drawing: Option<DrawingId> },
    /// built by the layout: an EMF bitmap record (BMP bytes), an EMF+ image, a page-fill image
    Derived,
}

/// One page.
#[derive(Debug, Default)]
pub struct LayoutPage {
    /// size, items, background (uo-render)
    pub page: Page,
    /// `section` (index)
    pub section: u32,
    /// `number` (PAGE as shown: any JS number)
    pub number: f64,
    /// `hfItems`: how many items at the start of `items` are the header/footer layers
    pub hf_items: Option<u32>,
    /// `lines`
    pub lines: Vec<LineGeom>,
    /// the lines of textbox contents (`PlacedObject.content`), sharing `chars` with `lines`
    pub content_lines: Vec<LineGeom>,
    /// every line's `chars`, column by column
    pub chars: CharsSoA,
    /// the paragraph formats the lines point at (`pfmt`), by value (K20: the TS sharing of one
    /// object per paragraph is never observed)
    pub pfmts: Vec<LineParaFormat>,
    /// `objects` (absent and empty differ in the TS object; the canonical stream keeps it)
    pub objects: Option<Vec<PlacedObject>>,
    /// `cells`
    pub cells: Option<Vec<PlacedCell>>,
    /// the Word-only fields of glyph items (RQ2): `lineBox`, `lineNum`; sparse, by item index
    pub item_meta: Vec<(u32, GlyphMeta)>,
    /// `ClipItem.src` (K14): the vector image drawn inside a clip, by item index
    pub clip_src: Vec<(u32, ImageId)>,
}

/// `lineBox` and `lineNum` of a glyph run (RQ2): read only by `cullOffPage` (engine.ts:4005).
#[derive(Clone, Copy, Debug)]
pub struct GlyphMeta {
    /// `{ asc, desc, lead, line }`
    pub line_box: Option<LineBox>,
    pub line_num: bool,
}

/// `lineBox`.
#[derive(Clone, Copy, Debug)]
pub struct LineBox {
    pub asc: f64,
    pub desc: f64,
    pub lead: f64,
    /// the cull group (`lineSeq`, per call)
    pub line: crate::keys::LineSeq,
}

/// `chars` of every line: `{ o, x, w }` column by column.
#[derive(Clone, Debug, Default)]
pub struct CharsSoA {
    /// offset in the paragraph (UTF-16)
    pub o: Vec<u32>,
    pub x: Vec<f64>,
    pub w: Vec<f64>,
}

/// `LaidOutLine` (tree.ts:171).
#[derive(Clone, Debug)]
pub struct LineGeom {
    pub at: LineRef,
    /// `[start, end)` in the paragraph text (UTF-16)
    pub start: u32,
    pub end: u32,
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub baseline: f64,
    pub width: f64,
    /// `chars` (a range of the page's `chars`)
    pub chars: Option<Span>,
    /// `hyph`: the offset where an automatic hyphen broke the word
    pub hyph: Option<u32>,
    /// `rev`: `{ before, after }`
    pub rev: Option<(f64, f64)>,
    /// `frame`: `{ l, r }`
    pub frame: Option<(f64, f64)>,
    /// `pfmt`: index into the page's `pfmts`
    pub pfmt: Option<u32>,
}

/// The TS line ref (`"b12"`, `"b3/r0/c1/b0"`, `"hdr:default/b0"`, `"fn2/b0"`, `"txbx/b3"`): the
/// story and the path inside it **as the TS writes it** — a table cell by its row and its GRID
/// column (`c${slot.lcol}`, table.ts:1060), which the shell resolves with `cellAtGrid` (refs.ts),
/// not by the cell's index in the row ("Milestone 1 — F0", decision F0-4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LineRef {
    pub story: LineStory,
    /// segments: block, then (row, grid column, block) per table level
    pub path: RefPath,
}

/// The segments of a TS ref path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RefPath(pub Box<[u32]>);

/// The story of a line ref.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineStory {
    Body,
    /// `fn<id>` / `en<id>` (the note id as the TS number, printed with `String(id)`)
    Note(NoteKind, NumKey),
    /// `txbx`: the content of a textbox (in the first box of its chain)
    Textbox(TbKey),
    /// `hdr:<slot>` / `ftr:<slot>` (K13: the slot, and the part that serves it)
    HeaderFooter {
        which: HfWhich,
        slot: HfSlot,
        part: HfPartId,
    },
    /// `preview`: the table the engine lays out to measure what a keep-with-next heading needs
    /// (engine.ts:2550, `nextBlockLead`); its path starts at the row (`preview/r0/c1/b0`; added
    /// by F6)
    Preview,
    /// `ensep`: the line of the endnote separator (`placeEndnotes`, engine.ts:2484), taken off
    /// the page right after it is placed — never in a layout (added by F7c)
    EndnoteSep,
    /// `itb`: the content of an inline textbox (`paintInlineDrawings`; added by F7b)
    InlineTextbox,
    /// `grp`: the content of a textbox in a drawing group (`paintObject`; added by F7b)
    GroupTextbox,
}

/// The header/footer slot (`default | first | even`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HfSlot {
    Default,
    First,
    Even,
}

impl LineRef {
    /// The TS string of the ref.
    #[must_use]
    pub fn to_ts(&self) -> String {
        let mut s = String::new();
        match self.story {
            LineStory::Body => {}
            LineStory::Note(kind, id) => {
                s.push_str(if kind == NoteKind::Footnote { "fn" } else { "en" });
                s.push_str(&uo_jsnum::format::to_js_string(id.get()));
            }
            LineStory::Textbox(_) => s.push_str("txbx"),
            LineStory::Preview => s.push_str("preview"),
            LineStory::EndnoteSep => s.push_str("ensep"),
            LineStory::InlineTextbox => s.push_str("itb"),
            LineStory::GroupTextbox => s.push_str("grp"),
            LineStory::HeaderFooter { which, slot, .. } => {
                s.push_str(match which {
                    HfWhich::Header => "hdr:",
                    HfWhich::Footer => "ftr:",
                });
                s.push_str(match slot {
                    HfSlot::Default => "default",
                    HfSlot::First => "first",
                    HfSlot::Even => "even",
                });
            }
        }
        // (a preview's path starts at the row)
        let first = usize::from(self.story == LineStory::Preview);
        for (i, seg) in self.path.0.iter().enumerate() {
            if !s.is_empty() {
                s.push('/');
            }
            let tag = match (i + first) % 3 {
                0 => 'b',
                1 => 'r',
                _ => 'c',
            };
            let _ = write!(s, "{tag}{seg}");
        }
        s
    }
}

/// `LineParaFormat` (tree.ts:201): effective indents and tabs of the paragraph (the ruler).
#[derive(Clone, Debug)]
pub struct LineParaFormat {
    pub left: f64,
    pub right: f64,
    pub first_line: f64,
    /// without the `clear` stops
    pub tabs: Arc<[TabStop]>,
}

/// A tab stop (`TabSpec`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub val: TabAlign,
    pub pos: f64,
    pub leader: Option<TabLeader>,
}

/// `TabAlign`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
    Bar,
    Clear,
    Num,
}

impl TabAlign {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TabAlign::Left => "left",
            TabAlign::Center => "center",
            TabAlign::Right => "right",
            TabAlign::Decimal => "decimal",
            TabAlign::Bar => "bar",
            TabAlign::Clear => "clear",
            TabAlign::Num => "num",
        }
    }
}

/// `TabSpec.leader`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabLeader {
    None,
    Dot,
    Hyphen,
    Underscore,
    Heavy,
    MiddleDot,
}

impl TabLeader {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TabLeader::None => "none",
            TabLeader::Dot => "dot",
            TabLeader::Hyphen => "hyphen",
            TabLeader::Underscore => "underscore",
            TabLeader::Heavy => "heavy",
            TabLeader::MiddleDot => "middleDot",
        }
    }
}

/// `'behind' | 'front'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Behind,
    Front,
}

/// `PlacedObject` (tree.ts:242): a floating object on the page.
#[derive(Clone, Debug)]
pub struct PlacedObject {
    /// the TS `run` when it is a model run (also inside copied paragraphs, K1)
    pub run: Option<RunRef>,
    pub drawing: Option<DrawingId>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub layer: Layer,
    pub hf: bool,
    pub frame: Option<ObjectFrame>,
    /// a textbox: its content lines (a range of the page's `content_lines`) and the text → page
    /// matrix
    pub content: Option<(std::ops::Range<u32>, [f64; 6])>,
}

/// `ObjectFrame` (tree.ts:258).
#[derive(Clone, Copy, Debug)]
pub struct ObjectFrame {
    pub col_x: f64,
    pub col_w: f64,
    pub para_top: f64,
    pub page_w: f64,
    pub page_h: f64,
    pub mar_l: f64,
    pub mar_r: f64,
    pub mar_t: f64,
    pub mar_b: f64,
    pub body_top: f64,
}

/// `PlacedCell` (tree.ts:234): a body table cell's box (`ref` = `b3/r0/c1`, grid column).
#[derive(Clone, Debug)]
pub struct PlacedCell {
    pub at: LineRef,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// grid columns covered
    pub span: f64,
}

impl Layout {
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}
