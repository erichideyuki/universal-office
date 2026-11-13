//! One `impl Session` module per command family (record "D2a — session (architecture)": the
//! `Command` skeleton). D2a owns `text`, `context` and `settings`; every other module is one D2b
//! frontier's, which adds its family enum's variants and arms there and nowhere else.

pub mod bookmarks;
pub mod clipboard;
pub mod comments;
/// wave 7 (A2): the read-only answers of the editor switch's last families
pub mod completion;
pub mod context;
pub mod drawing;
pub mod find;
pub mod format;
pub mod header_footer;
/// wave 7 (B, HF-T1): the flat editing view of a header/footer part with a table (`hfParts.paras`)
pub mod hf_flat;
pub mod lists;
pub mod navigation;
pub mod notes;
pub mod page;
pub mod paragraph;
pub mod pictures;
pub mod previews;
pub mod references;
pub mod revision_cmds;
pub mod sections;
pub mod settings;
/// wave 7 (A2): the edits of the shell's gestures outside `Command`
pub mod shell_edits;
pub mod styles;
pub mod tables;
pub mod text;
pub mod text_layout;
