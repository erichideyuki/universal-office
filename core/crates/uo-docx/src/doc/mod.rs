//! `.doc` (Word 97-2003 binary, [MS-DOC]) to `.docx` conversion in memory (D4). The normal docx
//! reader opens the result. Port of `write/packages/docx-core/src/doc/**` (frozen `main`), minus
//! what moved earlier: the container (`uo_opc::cfb`) and OfficeArt (`uo_drawingml::odraw`).
//! Brief and declarations: `core/docs/modules/uo-docx.md`, "D4 — .doc (frontier briefs)" and
//! "Stage 3 — D4".
//!
//! Frontiers (files of this module): F0 [`bytes`], [`xmlw`], this file; F1 [`fib`], [`text`],
//! [`fkp`], [`sprm`], [`fonts`]; F2 [`props`]; F3 [`stsh`], [`lists`]; F4 [`reader`], [`pictures`],
//! [`formfields`]; F5 [`ooxml_props`], [`styles_xml`], [`numbering`], [`settings`]; F6
//! [`docx_writer`]. [`numeric`] holds the float to integer helper of the writer.
//!
//! Everything borrows the input; offsets are `u64`; nothing panics on document data.

pub mod bytes;
pub mod docx_writer;
pub mod fib;
pub mod fkp;
pub mod fonts;
pub mod formfields;
pub mod lists;
pub mod numbering;
pub mod numeric;
pub mod ooxml_props;
pub mod open;
pub mod pictures;
pub mod props;
pub mod reader;
pub mod settings;
pub mod sprm;
pub mod stsh;
pub mod styles_xml;
pub mod text;
pub mod xmlw;

use std::fmt;

use uo_opc::cfb::{is_cfb, read_cfb};

pub use docx_writer::{ConvertOptions, DocxWriter};
use open::DocStreams;
use reader::DocReader;

/// Which limit of [`DocLimits`] was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLimitKind {
    /// code units of the decoded text (all stories)
    TextUnits,
    /// string units read from the table stream (style names…)
    StringUnits,
    /// the work budget of one conversion (string units read, runs, cells…)
    Work,
    /// nesting of tables and text-box stories past `DocLimits::max_nesting`
    Nesting,
}

/// Why a `.doc` was not converted: the 14 `DocFormatError` throw sites of the TS by class
/// (parity compares the class, never the text; the TS messages are Portuguese, these are
/// English), the catch-all of `convertDocToDocx`, and the limits of the Rust port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    /// no Compound File signature (`index.ts`)
    NotCfb,
    /// the Compound File is damaged (`reader.ts`, `readCfb` threw)
    Cfb(uo_opc::Error),
    /// `EncryptionInfo`/`EncryptedPackage` streams, or `fEncrypted` without obfuscation
    Encrypted,
    /// `fEncrypted` with `fObfuscated` (XOR obfuscation)
    Obfuscated,
    /// no `WordDocument` stream
    NoWordDocument,
    /// Word 6/95 (`nFib` below 0xC1)
    PreWord97,
    /// the FIB is shorter than 0x9A bytes
    FibTruncated,
    /// `wIdent` is neither 0xA5EC nor 0xA5DC
    FibSignature { ident: u16 },
    /// the table stream (`0Table` or `1Table`) is missing; `one` says which
    NoTableStream { one: bool },
    /// no Clx (`lcb` 0 or past the table)
    NoClx,
    /// the Clx has no Pcdt (`0x02`)
    NoPcdt,
    /// the piece table has no piece
    EmptyPieceTable,
    /// the catch-all of `convertDocToDocx`: any other throw of the TS reader or writer ("corrupt
    /// or not supported"); `what` names it for diagnostics
    Corrupt(&'static str),
    /// a limit of [`DocLimits`] (the TS has none: its failure is `Corrupt`, a RangeError or an
    /// out-of-memory, or it succeeds with a huge result)
    Limit { kind: DocLimitKind, max: u64 },
    /// the frontier that produces this result is not ported yet
    NotYet(&'static str),
}

impl DocError {
    /// The class name the parity capture uses (`error` facet).
    #[must_use]
    pub fn class(&self) -> &'static str {
        match self {
            DocError::NotCfb => "not-cfb",
            DocError::Cfb(_) => "cfb",
            DocError::Encrypted => "encrypted",
            DocError::Obfuscated => "obfuscated",
            DocError::NoWordDocument => "no-word-document",
            DocError::PreWord97 => "pre-word97",
            DocError::FibTruncated => "fib-truncated",
            DocError::FibSignature { .. } => "fib-signature",
            DocError::NoTableStream { .. } => "no-table-stream",
            DocError::NoClx => "no-clx",
            DocError::NoPcdt => "no-pcdt",
            DocError::EmptyPieceTable => "empty-pieces",
            DocError::Corrupt(_) => "corrupt",
            DocError::Limit { .. } => "limit",
            DocError::NotYet(_) => "not-yet",
        }
    }
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::NotCfb => f.write_str("not a Word 97-2003 .doc (no Compound File signature)"),
            DocError::Cfb(e) => write!(f, "corrupt .doc: {e}"),
            DocError::Encrypted => f.write_str("the document is password protected (encrypted) and cannot be opened"),
            DocError::Obfuscated => f.write_str("the document is password protected (XOR obfuscation) and cannot be opened"),
            DocError::NoWordDocument => f.write_str("not a Word document: the WordDocument stream is missing"),
            DocError::PreWord97 => f.write_str("a document from a version before Word 97 (Word 6/95) is not supported"),
            DocError::FibTruncated => f.write_str("corrupt .doc: truncated FIB"),
            DocError::FibSignature { ident } => write!(f, "corrupt .doc: invalid FIB signature (0x{ident:x})"),
            DocError::NoTableStream { one } => write!(f, "corrupt .doc: the {}Table stream is missing", u8::from(*one)),
            DocError::NoClx => f.write_str("corrupt .doc: the piece table (Clx) is missing"),
            DocError::NoPcdt => f.write_str("corrupt .doc: no Pcdt in the Clx"),
            DocError::EmptyPieceTable => f.write_str("corrupt .doc: the piece table is empty"),
            DocError::Corrupt(what) => write!(f, "corrupt or unsupported .doc: {what}"),
            DocError::Limit { kind, max } => write!(f, "limit {kind:?} ({max}) exceeded"),
            DocError::NotYet(what) => write!(f, "not ported yet: {what}"),
        }
    }
}

impl std::error::Error for DocError {}

/// Limits of one conversion of untrusted input (D4 declaration 8): counted on what is actually
/// produced, never trusted from sizes in the file. A limit hit is a [`DocError::Limit`], never a
/// panic or an allocation from a declared size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocLimits {
    /// code units of the decoded text, all stories (`decodeText` builds `totalCp` units, and
    /// `totalCp` is a sum of `i32` counts of the FIB: the TS allocates it, up to V8's string limit
    /// of 2^29 - 24, then throws a RangeError)
    pub max_text_units: u64,
    /// string units read from the table stream (style names, font names, list text, bookmark
    /// names…), summed over one conversion: a name read at an offset is not bounded by its own
    /// record in the TS, so overlapping records can multiply a small input
    pub max_string_units: u64,
    /// the work budget of what no other limit bounds (cells of a table row built by
    /// `sprmTInsert`, runs…), in elements
    pub max_work: u64,
    /// the bytes an input of this size may inflate string reads to, as a multiple of the input
    /// length (`max_string_units` is `max(floor, per_byte × input)`)
    pub string_units_per_byte: u64,
    /// the deepest nesting of tables and of text-box stories the body writer follows (the TS recurses
    /// until the JavaScript stack overflows, which is the catch-all `Corrupt` there)
    pub max_nesting: u32,
}

impl DocLimits {
    /// The native defaults: 256 Mi units of text (the V8 limit is 512 Mi), 64 Mi string units,
    /// 64 Mi elements of work.
    pub const NATIVE: DocLimits =
        DocLimits { max_text_units: 1 << 28, max_string_units: 1 << 26, max_work: 1 << 26, string_units_per_byte: 8, max_nesting: 64 };

    /// The wasm32 defaults: a quarter of the native text and work (a phone kills the tab first).
    pub const WASM32: DocLimits =
        DocLimits { max_text_units: 1 << 26, max_string_units: 1 << 24, max_work: 1 << 24, string_units_per_byte: 8, max_nesting: 32 };

    /// The defaults of the target.
    #[cfg(target_arch = "wasm32")]
    pub const DEFAULT: DocLimits = DocLimits::WASM32;

    /// The defaults of the target.
    #[cfg(not(target_arch = "wasm32"))]
    pub const DEFAULT: DocLimits = DocLimits::NATIVE;
}

impl DocLimits {
    /// The string units one conversion may read from a table stream of `stream_len` bytes:
    /// `string_units_per_byte` times the stream, at least 2^20, at most `max_string_units`.
    #[must_use]
    pub fn string_budget(&self, stream_len: u64) -> u64 {
        self.max_string_units.min((1u64 << 20).max(self.string_units_per_byte.saturating_mul(stream_len)))
    }
}

impl Default for DocLimits {
    fn default() -> Self {
        DocLimits::DEFAULT
    }
}

/// A Compound File with a `WordDocument` stream (`isWordBinary`): the Word 97-2003 document?
#[must_use]
pub fn is_word_binary(bytes: &[u8]) -> bool {
    is_cfb(bytes) && read_cfb(bytes).is_ok_and(|c| c.has("WordDocument"))
}

/// Converts a `.doc` (the bytes of the file) into a `.docx` (zip bytes), with the default options
/// ([`ConvertOptions::default`]).
///
/// # Errors
/// [`DocError`]: not a Compound File, encrypted, pre-Word 97 or corrupt files, or a limit of `limits`.
pub fn convert_doc_to_docx(bytes: &[u8], limits: &DocLimits) -> Result<Vec<u8>, DocError> {
    convert_doc_to_docx_with(bytes, limits, &ConvertOptions::default())
}

/// [`convert_doc_to_docx`] with options (`convertDocToDocx(bytes, options)`).
///
/// # Errors
/// As [`convert_doc_to_docx`].
pub fn convert_doc_to_docx_with(bytes: &[u8], limits: &DocLimits, options: &ConvertOptions) -> Result<Vec<u8>, DocError> {
    if !is_cfb(bytes) {
        return Err(DocError::NotCfb);
    }
    let streams = DocStreams::open(bytes)?;
    let reader = DocReader::new(&streams, limits)?;
    DocxWriter::new(reader, limits)?.build(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_compound_file() {
        assert!(!is_word_binary(b"PK\x03\x04"));
        assert_eq!(convert_doc_to_docx(b"hello", &DocLimits::DEFAULT), Err(DocError::NotCfb));
    }

    #[test]
    fn a_compound_file_signature_alone_is_not_a_word_document() {
        let sig = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
        assert!(!is_word_binary(&sig));
        assert_eq!(convert_doc_to_docx(&sig, &DocLimits::DEFAULT).unwrap_err().class(), "cfb");
    }

    #[test]
    fn classes_are_distinct() {
        let all = [
            DocError::NotCfb,
            DocError::Cfb(uo_opc::Error::NotZip),
            DocError::Encrypted,
            DocError::Obfuscated,
            DocError::NoWordDocument,
            DocError::PreWord97,
            DocError::FibTruncated,
            DocError::FibSignature { ident: 1 },
            DocError::NoTableStream { one: true },
            DocError::NoClx,
            DocError::NoPcdt,
            DocError::EmptyPieceTable,
            DocError::Corrupt("x"),
            DocError::Limit { kind: DocLimitKind::Work, max: 1 },
            DocError::NotYet("x"),
        ];
        let mut names: Vec<_> = all.iter().map(DocError::class).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        assert!(all.iter().all(|e| !e.to_string().is_empty()));
    }
}
