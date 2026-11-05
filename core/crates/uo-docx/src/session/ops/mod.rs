//! One `impl Session` module per command family (record "D2a — session (architecture)": the
//! `Command` skeleton). D2a owns `text`, `context` and `settings`; every other module is one D2b
//! frontier's, which adds its family enum's variants and arms there and nowhere else.

pub mod bookmarks;
pub mod clipboard;
pub mod comments;
pub mod context;
pub mod drawing;
pub mod find;
pub mod format;
pub mod header_footer;
pub mod lists;
pub mod navigation;
pub mod notes;
pub mod page;
pub mod paragraph;
pub mod pictures;
pub mod references;
pub mod revision_cmds;
pub mod sections;
pub mod settings;
pub mod styles;
pub mod tables;
pub mod text;
pub mod text_layout;
