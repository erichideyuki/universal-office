//! Indices into the host's tables (media, themes) and the generic range type. The shape model itself is owned trees
//! (`uo-dml-render.md` decision A1); the arena ids of the first design are gone (B3 replaced the last users).

/// A range of an arena vector (`start .. start + len`): the paragraphs of a body, the runs of a
/// paragraph, the cells of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

impl Span {
    pub const EMPTY: Span = Span { start: 0, len: 0 };

    pub const fn new(start: u32, len: u32) -> Span {
        Span { start, len }
    }
}

/// A media part of the package (bytes shared with the part store, mime, header size and dpi),
/// in the host's media table (`uo_pptx` deck or the Sheet package).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct MediaId(pub u32);

/// An effective theme (theme part + clrMap) in the host's theme table (hash-consed).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ThemeId(pub u32);
