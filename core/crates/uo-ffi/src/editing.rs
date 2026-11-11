//! The C ABI of the Editing API of uo-docx D3 (feature `editing-api`), for the .NET facade
//! (`core/dotnet/UniversalOffice.Docx/Editing`). Record: `core/docs/modules/uo-docx-d3.md` §4.
//!
//! Conventions (the milestone-1 ones of [`crate`], plus):
//! - every export returns an `int32` status (`pipeline::code`, the `ApiError::code` numbers 1-9),
//!   except the `*_free`s (no-ops on null, no error slot touched: they run on finalizer threads),
//!   pure getters and counters;
//! - the details of a failure: `uo_last_error` / `uo_last_error_message` (shared with the PDF path)
//!   plus `uo_last_error_limit` / `uo_last_error_part` for the `OfficeLimit` and the part name;
//!   `uo_editing_error_clear` clears both;
//! - strings come in as UTF-8 `(ptr, len)`, `len == usize::MAX` meaning null; lists and records go
//!   in and out as the packed format of [`wire`]; owned results are `UoBuf`s (`uo_buf_free`);
//! - a document handle ([`UoDocx`]) is used by ONE thread at a time (the facade locks it); the
//!   other handles hold node ids of their document and take it on every call, so any free order
//!   is safe; a view (shape, inline) is an index into the document's view table;
//! - a panic inside an export on a document poisons that handle (`INVALID_OPERATION` after it);
//! - the exports are `unsafe extern "C" fn` (as the milestone-1 ones): the caller promises the contract
//!   of each `# Safety` section: every `(ptr, len)` is null or readable for `len` bytes during the call,
//!   every handle is null or live and used by one thread at a time, every output slot is null or
//!   writable. [`wire`] holds `unsafe fn` helpers that rely on it; each export wraps its body in ONE
//!   `unsafe` block with the single SAFETY line saying so (pure exports with no pointer stay safe);
//! - a view is `kind | salt | index` (1, 8 and 23 bits): the kind is exact, a view of another
//!   document is refused unless the salts collide (best effort), see [`handles::UoDocx::node`].

mod document;
mod error;
mod handles;
mod ops;
mod options;
mod pure;
#[cfg(test)]
mod tests;
mod views;
mod wire;
mod writer;

pub use document::*;
pub use error::{UoLimitInfo, uo_editing_error_clear, uo_last_error_limit, uo_last_error_part};
pub use handles::*;
pub use ops::*;
pub use options::*;
pub use pure::*;
pub use views::*;
pub use writer::*;
