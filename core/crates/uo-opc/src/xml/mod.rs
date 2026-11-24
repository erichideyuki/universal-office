//! XML: the reader ([`read`], one iterative parser with per-format options and a strict mode),
//! the compact tree ([`tree`]), writing and escapes ([`write`]) and raw surgery on XML text
//! ([`raw`]).
mod chars;
mod entities;
pub mod pull;
pub mod raw;
pub mod read;
pub mod tree;
pub mod write;

pub use entities::CharRefPolicy;
pub use read::{CanonicalPrefixes, ContentPi, Mode, ParseOptions, parse};
pub use tree::{Attr, Document, Element, ElementId, NameId, Node, Positions, Source};
