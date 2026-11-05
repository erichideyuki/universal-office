//! The clipboard model (D2a): the Office Clipboard of the TS (`OFFICE_CLIPBOARD`: the current
//! rich copy and the last 24 items, shared by every element of the page) as one
//! [`ClipboardRing`] per wasm instance (G7), held by the binding and lent to each command
//! ([`super::ExecCx::clipboard`]). A [`Clip`] holds values, never references into a document: the
//! TS document identity (`RichClip.doc`) and the source `WeakRef` (`src.el`) become the
//! [`ClipOrigin`] (session id + generation, U3). The paste rules that read a clip are the
//! clipboard family's (D2b-F8).
//!
//! Besides the TS count cap (24, `rememberClip`), the ring has a byte cap (M5): past it the
//! oldest items go (the current clip stays).

use std::collections::VecDeque;
use std::sync::Arc;

use crate::js::Str;
use crate::model::Story;
use crate::model::run::Run;
use crate::model::table::Table;
use crate::text::Utf16;

use super::env::LocalNow;
use super::text::DocRange;

/// The TS ring length (`if (ring.length > 24) ring.length = 24`).
pub const MAX_CLIPS: usize = 24;
/// The byte cap of the ring (M5): 32 MiB.
pub const DEFAULT_MAX_CLIP_BYTES: usize = 32 << 20;

/// Where a clip was copied (`RichClip.doc`, `src`): the session and its document generation (a
/// session that loads another document bumps the generation, the TS `docToken`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipOrigin {
    pub session: u32,
    pub generation: u32,
}

/// Selected cells copied as a new table (`cells`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipCells {
    pub table: Arc<Table>,
    pub rows: bool,
    pub whole: bool,
}

/// A rich copy (`RichClip`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Clip {
    /// the plain text (`text`)
    pub text: Utf16,
    /// the copied paragraphs with the tables between them (`paras`, `tables` with `before`)
    pub content: Story,
    /// the list marker text of each paragraph (`ClipPara.marker`, "Keep Text Only")
    pub markers: Vec<Option<Str>>,
    pub origin: Option<ClipOrigin>,
    /// the document's path in the host (`path`)
    pub path: Option<Str>,
    /// lists used: numId → raw `w:abstractNum` (`lists`)
    pub lists: Vec<(f64, Arc<str>)>,
    /// paragraph styles used: styleId → `w:name` (`styleNames`)
    pub style_names: Vec<(Str, Str)>,
    /// where it came from (`src`, Paste Link)
    pub src: Option<(ClipOrigin, DocRange)>,
    pub cells: Option<ClipCells>,
    /// a selected drawing object (`drawing`)
    pub drawing: Option<Run>,
}

impl Clip {
    /// An estimate of the bytes the clip keeps alive (for the cap).
    #[must_use]
    pub fn bytes(&self) -> usize {
        let mut n = std::mem::size_of::<Clip>() + self.text.heap_bytes();
        for p in self.content.paras.iter() {
            n += std::mem::size_of_val(p);
            for r in &p.runs {
                n += std::mem::size_of_val(r) + r.text.heap_bytes();
            }
        }
        n += self.content.tables.len() * 256;
        n += self.lists.iter().map(|(_, s)| s.len() + 16).sum::<usize>();
        n += self.style_names.iter().map(|(a, b)| a.len() + b.len()).sum::<usize>();
        n
    }
}

/// One item of the ring (`{ id, clip, at }`).
#[derive(Debug, Clone, PartialEq)]
pub struct RingItem {
    pub id: u32,
    pub clip: Arc<Clip>,
    pub at: LocalNow,
}

/// The Office Clipboard of one wasm instance (see the module comment).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardRing {
    current: Option<Arc<Clip>>,
    ring: VecDeque<RingItem>,
    seq: u32,
    max_bytes: usize,
    bytes: usize,
    /// bumped on every change (the TS `listeners`: the shell polls it)
    version: u32,
}

impl Default for ClipboardRing {
    fn default() -> Self {
        ClipboardRing::new(DEFAULT_MAX_CLIP_BYTES)
    }
}

impl ClipboardRing {
    #[must_use]
    pub fn new(max_bytes: usize) -> ClipboardRing {
        ClipboardRing { current: None, ring: VecDeque::new(), seq: 0, max_bytes, bytes: 0, version: 0 }
    }

    /// The current rich copy (`richClip`).
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Clip>> {
        self.current.as_ref()
    }

    pub fn set_current(&mut self, clip: Option<Arc<Clip>>) {
        self.current = clip;
        self.version = self.version.wrapping_add(1);
    }

    /// The items, newest first.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &RingItem> + '_ {
        self.ring.iter()
    }

    /// The item with an id.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&RingItem> {
        self.ring.iter().find(|x| x.id == id)
    }

    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// `rememberClip(clip)`: the copy goes first; an equal item (the same clip, or the same
    /// non-empty text without a drawing on either side) leaves its old place; at most 24 items
    /// and the byte cap.
    pub fn remember(&mut self, clip: Arc<Clip>, at: LocalNow) -> u32 {
        let dup = self.ring.iter().position(|x| {
            Arc::ptr_eq(&x.clip, &clip)
                || (x.clip.text == clip.text && clip.drawing.is_none() && x.clip.drawing.is_none() && !x.clip.text.is_empty())
        });
        if let Some(i) = dup
            && let Some(old) = self.ring.remove(i)
        {
            self.bytes = self.bytes.saturating_sub(old.clip.bytes());
        }
        self.seq = self.seq.wrapping_add(1);
        self.bytes = self.bytes.saturating_add(clip.bytes());
        self.ring.push_front(RingItem { id: self.seq, clip, at });
        while self.ring.len() > MAX_CLIPS || (self.bytes > self.max_bytes && self.ring.len() > 1) {
            match self.ring.pop_back() {
                Some(old) => self.bytes = self.bytes.saturating_sub(old.clip.bytes()),
                None => break,
            }
        }
        self.version = self.version.wrapping_add(1);
        self.seq
    }

    /// Removes one item (`deleteClipboardItem`).
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(i) = self.ring.iter().position(|x| x.id == id) else { return false };
        if let Some(old) = self.ring.remove(i) {
            self.bytes = self.bytes.saturating_sub(old.clip.bytes());
        }
        self.version = self.version.wrapping_add(1);
        true
    }

    /// Empties the ring (`clearClipboard`).
    pub fn clear(&mut self) {
        self.ring.clear();
        self.bytes = 0;
        self.version = self.version.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(t: &str) -> Arc<Clip> {
        Arc::new(Clip { text: Utf16::from(t), ..Clip::default() })
    }

    #[test]
    fn ring_semantics() {
        let mut r = ClipboardRing::default();
        for i in 0..30 {
            r.remember(clip(&format!("c{i}")), LocalNow::default());
        }
        assert_eq!(r.len(), MAX_CLIPS);
        // the same text moves to the front
        let first = r.items().last().map(|x| x.clip.text.clone()).unwrap_or_default();
        r.remember(Arc::new(Clip { text: first.clone(), ..Clip::default() }), LocalNow::default());
        assert_eq!(r.len(), MAX_CLIPS);
        assert_eq!(r.items().next().map(|x| x.clip.text.clone()), Some(first));
        // empty texts never dedupe
        r.remember(clip(""), LocalNow::default());
        r.remember(clip(""), LocalNow::default());
        assert_eq!(r.items().take(2).filter(|x| x.clip.text.is_empty()).count(), 2);
    }

    #[test]
    fn byte_cap() {
        let mut r = ClipboardRing::new(4096);
        for i in 0..20 {
            r.remember(clip(&"x".repeat(1000 + i)), LocalNow::default());
        }
        assert!(r.bytes() <= 4096 || r.len() == 1);
        assert!(r.len() < 20);
    }
}
