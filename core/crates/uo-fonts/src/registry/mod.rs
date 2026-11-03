//! The font registry (LQ8 of the uo-docx-layout plan): the faces a host pushes and the provider
//! policies that answer for them, the face cache shared by every document, and the budgets that
//! keep a phone alive.
//!
//! - [`FaceCache`]: parsed [`Face`](crate::Face)s deduplicated by content, least recently used
//!   out first over `bytes.len() + heap_bytes()`; faces some handle holds are pinned.
//! - [`FaceLedger`]: what one document may pin; [`FontBudget`] / [`FontProfile`]: the numbers
//!   (desktop, phone), chosen by the binding.
//! - [`FontRegistry`]: pushed faces (`push_family`, `push_asset`) and the missing notes the host
//!   fetches; the providers of the named [policies](crate::policy) are built from it.
//! - [`FontAnswer`] / [`AnswerId`]: what a provider answers, with the identity the layout's
//!   `FaceId` follows.
//! - [`AdvanceTable`]: the empirical advances attached to a pushed face, for the PDF only (PQ1).

mod advance;
mod answer;
pub(crate) mod answers;
mod budget;
mod cache;
mod digest;
mod ledger;
mod store;
mod ttc_face;

pub use advance::{AdvanceTable, AdvanceTableError, MAX_ADVANCE_ENTRIES, MAX_ADVANCE_JSON_BYTES};
pub use answer::{AnswerId, AnswerParts, FaceSource, FontAnswer, FontProvider, SourceFace};
pub use answers::MAX_ANSWER_SLOTS;
pub use budget::{FontBudget, FontProfile};
pub use cache::{CacheStats, CachedFace, ContentKey, FaceCache, MAX_ALIASES, MAX_UNPINNED_FACES};
pub use ledger::{FaceLedger, Ticket};
pub use store::{FontRegistry, FontSource, HostFace, MAX_ATTEMPTED, MAX_MISSING_FAMILIES, MissingFamily, PushError};
pub use ttc_face::ttc_face_by_index;

pub(crate) use store::{Pushed, PushedFace};
