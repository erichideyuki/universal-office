//! What a font provider answers: the engine's `FontFace` (TS) / `FontFace` (C#), with an
//! **identity**.
//!
//! The layout merges glyph runs, kerns and ligates only across the *same face object*
//! (para.ts:488, 524, 1016; K2a of the uo-docx-layout plan), so which requests share an answer is
//! observable. Each policy reproduces the keying of the provider it ports, and the layout's
//! `FaceId` follows [`AnswerId`], never the bytes: two answers over the same file are two faces
//! when the TS provider hands out two objects.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::advance::AdvanceTable;
use super::cache::CachedFace;
use super::ledger::Ticket;
use crate::face::Face;

/// The identity of an answer: equal ids are the same object of the TS provider. Unique in the
/// process, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnswerId(u64);

impl AnswerId {
    pub(crate) fn next() -> AnswerId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        AnswerId(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// One answer of a provider. While an `Arc<FontAnswer>` lives, its face is pinned in the cache and
/// charged to the document's ledger.
#[derive(Clone)]
pub struct FontAnswer {
    id: AnswerId,
    /// unique key of the effective face (family and variant), identifies it in the PDF
    pub key: String,
    /// the effective family after substitution
    pub family: String,
    face: CachedFace,
    /// the variant ASKED for
    pub bold: bool,
    pub italic: bool,
    /// bold or italic is synthetic: the family has no such variant
    pub synth_bold: bool,
    pub synth_italic: bool,
    /// the family asked for is absent and this is the host's metric-compatible substitute (the
    /// layout first tries what Word puts in its place)
    pub stand_in: bool,
    /// the empirical advances loaded with the face, read only by the PDF (PQ1)
    pub advances: Option<Arc<AdvanceTable>>,
    ticket: Option<Ticket>,
}

/// The ingredients of an answer; [`FontAnswer::new`] gives it a fresh identity.
pub struct AnswerParts {
    pub key: String,
    pub family: String,
    pub face: CachedFace,
    pub bold: bool,
    pub italic: bool,
    pub synth_bold: bool,
    pub synth_italic: bool,
    pub stand_in: bool,
    pub advances: Option<Arc<AdvanceTable>>,
    pub ticket: Option<Ticket>,
}

impl FontAnswer {
    /// A new answer, a new identity.
    pub fn new(parts: AnswerParts) -> Arc<FontAnswer> {
        Arc::new(FontAnswer {
            id: AnswerId::next(),
            key: parts.key,
            family: parts.family,
            face: parts.face,
            bold: parts.bold,
            italic: parts.italic,
            synth_bold: parts.synth_bold,
            synth_italic: parts.synth_italic,
            stand_in: parts.stand_in,
            advances: parts.advances,
            ticket: parts.ticket,
        })
    }

    /// A copy of this answer with a fresh identity and `stand_in` set: the C# `SubstituteFontProvider`
    /// marks a copy because the face of its inner provider may serve its own family. The face, its
    /// pin and its ledger charge are shared.
    pub(crate) fn marked_stand_in(&self) -> Arc<FontAnswer> {
        let mut copy = self.clone();
        copy.id = AnswerId::next();
        copy.stand_in = true;
        Arc::new(copy)
    }

    pub fn id(&self) -> AnswerId {
        self.id
    }

    pub fn face(&self) -> &Face {
        self.face.face()
    }

    /// The font file bytes, shared with the cache (the PDF embeds them).
    pub fn bytes(&self) -> &Arc<[u8]> {
        self.face.bytes()
    }

    pub fn cached_face(&self) -> &CachedFace {
        &self.face
    }

    pub fn ticket(&self) -> Option<&Ticket> {
        self.ticket.as_ref()
    }
}

impl std::fmt::Debug for FontAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontAnswer")
            .field("id", &self.id)
            .field("key", &self.key)
            .field("family", &self.family)
            .field("bold", &self.bold)
            .field("italic", &self.italic)
            .field("synth_bold", &self.synth_bold)
            .field("synth_italic", &self.synth_italic)
            .field("stand_in", &self.stand_in)
            .finish_non_exhaustive()
    }
}

/// The engine's `FontProvider`: the face for a family (as written in the document) and a
/// bold/italic request, or `None` when the family does not exist (the layout then follows its own
/// substitution chain). Synchronous and callable from several threads.
pub trait FontProvider: Send + Sync {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>>;
}

/// A face picked from a font index for a request, before a policy gives it an identity.
pub struct SourceFace {
    /// `family|bi|path#index` of the picked entry (`bi`: `b`, `i`, both or none, of the ENTRY)
    pub key: String,
    /// the family of the picked entry
    pub family: String,
    pub face: CachedFace,
    /// the request asked for bold and the picked entry is not
    pub synth_bold: bool,
    pub synth_italic: bool,
}

/// Where the `oracle` and `server-substitute` policies get faces from: an index of the machine's
/// fonts (the native `system::SystemFontIndex`), or any host-built list. Stateless about
/// identity: the policies keep the answers.
pub trait FaceSource: Send + Sync {
    /// The pick of node-fonts' `face()` for one request: the family is looked up in lower case,
    /// first among the names the fonts declare (name ID 1), then among the typographic names
    /// (ID 16); the entry with the lowest score wins, the first in index order on a tie; its bytes
    /// are loaded (a face of a `.ttc` as a standalone sfnt). `None` when the family is unknown or
    /// the face cannot be loaded.
    fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace>;
}
