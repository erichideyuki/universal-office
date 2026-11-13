//! The Write editing session for the browser and Node (milestone 2, feature `session`; record
//! `core/docs/modules/uo-bindings.md`, "Milestone 2: the session in wasm").
//!
//! One handle per open document, [`handle::UoWriteSession`]: it owns uo-docx's `Session`, the fonts the
//! document embeds, the host's font environment (for the layout oracle and the session's layouts), the
//! font embedding settings, the painter of the last `CopyFormat` query and the lines of the last layout
//! the caret exports read. JS frees it (`free()` + `FinalizationRegistry`, `js/session.mjs`).
//!
//! - **Hot paths** are dedicated exports (typing, Enter, Backspace, Delete, the selection, caret moves,
//!   hit test, caret and selection rectangles); every one answers an `ExecResult` message (or a `Hit`,
//!   `Caret`, `SelRects` message).
//! - **Everything else** goes through `uo_session_exec` / `uo_session_query` / `uo_session_complete`
//!   with the messages of `schema/session.uos` (codec and the conversions to uo-docx's types generated
//!   by core/tools/uo-schema: `codec/generated.rs`, `codec/bind.rs`).
//! - **Layouts**: `uo_session_layout` lays the session's view out and answers an owned `UoLayout` (the
//!   viewer's handle: every `uo_layout_*` and `uo_render_*` export works on it); the caret exports take
//!   it back (the shell decides when to lay out again, as the TS does).
//! - **The layout oracle** of the layout family (fields, TOC, hyphenation, page numbers) is built on
//!   the first command that asks it ([`oracle::LazyOracle`]): typing never builds an engine.
//! - **One Office Clipboard per wasm instance** ([`ring`], G7).
//!
//! Conventions as `write.rs`: input buffers come from `uo_alloc_bytes` and are consumed by the call; a
//! call that creates a handle returns null on failure and sets the last error (`uo_last_error`); a call
//! that answers bytes returns a `UoBuf` with the statuses 0 ok, 1 none, 2 no pending font save, 5 a
//! message that does not decode or bind (UTF-8 detail), 11 null handle.

pub mod caret;
pub mod codec;
pub mod env;
pub mod handle;
pub mod layout;
pub mod oracle;
pub mod query;
pub mod ring;
// wave 5, frontier R (the editor switch: review and document)
pub mod review;
// the editor switch (record "Milestone 2: the editor switch"): the shell's incremental relayout and
// story queries, and its toolbar state
pub mod shell;
pub mod shell_state;
// wave 5, frontier R
pub mod shell_review;
// wave 5, front O (objects and the page): object geometry and section facts of the shell
pub mod shell_objects;
// wave 5, frontier S (stories: tables, headers/footers, notes, the caret in a text box)
pub mod shell_stories;
// wave 7, frontier A1 (File, View, Font and Paragraph): the reads, native-only commands and exports of its families
pub mod shell_format;
// wave 7 (A2): lists, paste, tables, objects and the page
pub mod shell_completion;
pub mod wire;

#[cfg(all(feature = "session-selftest", not(test)))]
pub mod selftest;
