//! The handles of the Editing ABI. None holds a pointer to another handle: every call that needs
//! the document receives it as well, so any free order is safe (finalizers run in any order).

use std::sync::atomic::{AtomicU32, Ordering};

use indexmap::IndexSet;
use uo_docx::api::body_writer::{BodyWriter, ParagraphProperties};
use uo_docx::api::row_template::RowTemplate;
use uo_docx::api::sanitizer::SanitizeStage;
use uo_docx::api::{ApiError, DocxDocument, NodeId};

use super::wire::drop_box;

/// What a view is (it decides which table of the document holds its node).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewKind {
    /// a `DocxShape` (the run holding the shape)
    Shape,
    /// a `DocxCustomInline` (the inline sdt)
    Inline,
}

/// Bits of a view value: kind (1) | document salt (8) | index (23).
const INDEX_BITS: u32 = 23;
const SALT_BITS: u32 = 8;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
const SALT_MASK: u32 = (1 << SALT_BITS) - 1;

/// Source of the per-document salt (wraps: 256 documents).
static NEXT_SALT: AtomicU32 = AtomicU32::new(1);

/// An opened document.
pub struct UoDocx {
    pub(super) doc: DocxDocument,
    /// the view tables: the nodes the host holds as views, one per kind (a shape run and an inline
    /// sdt never share an index space); a view crosses the ABI as `kind | salt | index`, so `NodeId`
    /// never leaves Rust (a clone starts empty and gets its own salt)
    shapes: IndexSet<NodeId>,
    inlines: IndexSet<NodeId>,
    /// the salt of this document (a best-effort tag, see `node`)
    salt: u32,
    pub(super) poisoned: bool,
    /// the size of the bytes it was opened from (for the host's GC pressure)
    pub(super) input_len: usize,
}

impl UoDocx {
    pub(super) fn new(doc: DocxDocument, input_len: usize) -> UoDocx {
        let salt = NEXT_SALT.fetch_add(1, Ordering::Relaxed) & SALT_MASK;
        UoDocx { doc, shapes: IndexSet::new(), inlines: IndexSet::new(), salt, poisoned: false, input_len }
    }

    /// The view value of a node (the table inserts it once).
    pub(super) fn view(&mut self, kind: ViewKind, id: NodeId) -> Result<u32, ApiError> {
        let table = match kind {
            ViewKind::Shape => &mut self.shapes,
            ViewKind::Inline => &mut self.inlines,
        };
        let (i, _) = table.insert_full(id);
        let i = u32::try_from(i)
            .ok()
            .filter(|i| *i <= INDEX_MASK)
            .ok_or_else(|| ApiError::invalid_operation("too many views of one document"))?;
        let k = u32::from(kind == ViewKind::Inline);
        Ok((k << 31) | (self.salt << INDEX_BITS) | i)
    }

    /// The node of a view value. The kind is exact (a shape view is never an inline). A view of
    /// ANOTHER document is refused when its salt differs (255 of 256 pairs; two documents 256 opens
    /// apart share a salt) or its index is past this table; the tables never shrink, so a stale
    /// view cannot be told from a live one: `InvalidOperationException` for what is detectable.
    pub(super) fn node(&self, kind: ViewKind, view: u32) -> Result<NodeId, ApiError> {
        let bad = || ApiError::invalid_operation("stale or foreign view");
        if (view >> 31 == 1) != (kind == ViewKind::Inline) || (view >> INDEX_BITS) & SALT_MASK != self.salt {
            return Err(bad());
        }
        let table = match kind {
            ViewKind::Shape => &self.shapes,
            ViewKind::Inline => &self.inlines,
        };
        table.get_index((view & INDEX_MASK) as usize).copied().ok_or_else(bad)
    }
}

/// A body writer.
pub struct UoBodyWriter(pub(super) BodyWriter);
/// A taken table row.
pub struct UoRowTemplate(pub(super) RowTemplate);
/// The first phase of the two-phase sanitizer (`finish` consumes it).
pub struct UoSanitizeStage(pub(super) Option<SanitizeStage>);
/// The sample paragraphs extracted from a template.
pub struct UoSampleSet(pub(super) Vec<(String, ParagraphProperties)>);
/// A paragraph's properties.
pub struct UoParagraphProps(pub(super) ParagraphProperties);

const _: () = {
    fn is_send<T: Send>() {}
    let _ = is_send::<UoDocx>;
    let _ = is_send::<UoBodyWriter>;
    let _ = is_send::<UoRowTemplate>;
    let _ = is_send::<UoSanitizeStage>;
    let _ = is_send::<UoSampleSet>;
    let _ = is_send::<UoParagraphProps>;
};

macro_rules! free_fn {
    ($name:ident, $t:ty) => {
        /// Releases the handle (null is a no-op; no error slot is touched: it may run on a finalizer thread).
        ///
        /// # Safety
        /// `p` is null or came from an export of this library and is released once.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(p: *mut $t) {
            // SAFETY: the caller's guarantee (# Safety).
            unsafe { drop_box(p) };
        }
    };
}

free_fn!(uo_docx_free, UoDocx);
free_fn!(uo_body_writer_free, UoBodyWriter);
free_fn!(uo_row_template_free, UoRowTemplate);
free_fn!(uo_stage_free, UoSanitizeStage);
free_fn!(uo_sample_set_free, UoSampleSet);
free_fn!(uo_paragraph_props_free, UoParagraphProps);
