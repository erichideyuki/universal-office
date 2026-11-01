//! The OPC package, lazy (gate 1, G4): one type for Write, Sheet, Slides and ODF, unifying
//! `DocxPackage` (`write/packages/docx-core/src/package.ts`), `XlsxPackage`
//! (`sheet/packages/sheet-core/src/package.ts`), `PptxPackage`
//! (`slides/packages/slides-core/src/package.ts`), `OdfPackage`/`writeZip`
//! (`write/packages/docx-core/src/odf/zip.ts`) and the C# `OpcPackage`.
//!
//! The TS packages are a `Map<name, bytes>` filled by `unzipSync` (every part inflated at load)
//! and saved by `zipSync(…, { level: 6 })`. Here the names, their order and the duplicate rules
//! are the same (see `zip`), but nothing is inflated until it is read.
//!
//! Memory scopes:
//! - the input archive is retained for the life of the package (owned, `open` copies it and
//!   `open_owned` takes it);
//! - a stored entry is read as a slice of the input (no copy);
//! - a deflated entry is inflated on its first read into a per-entry cache, kept until
//!   [`Package::release`], [`Package::release_all`], a `set_part`/`remove_part` of that name, or
//!   the drop of the package; except through [`Package::xml_part`], whose tree takes the
//!   inflated text as its source (no copy) and the package keeps nothing;
//! - parts given by `set_part`/`from_parts` are owned by the package until replaced or removed;
//! - the case-insensitive indexes (one for the [`CaseFold`] policy, one ASCII index for
//!   [`Package::resolve_target`]) are built on the first fallback lookup and dropped on any
//!   change of the part set; each holds one folded name per part;
//! - what is known of an entry's stream (its output length, encoded length and CRC-32) is kept
//!   per entry, a few bytes, so a save verifies each entry once (D5).
//!
//! Limits: each raw read checks `Limits::part_cap(name)` on the bytes it produces, and the
//! running total of produced bytes (each entry counted once, the first time it is read) against
//! `max_total_bytes`. Declared sizes never reserve memory. Errors of
//! a damaged entry surface when that entry is read; [`Package::verify`] reproduces the TS
//! behaviour of failing at load.
//!
//! The format crates and hosts go through two entry points (gate 2, D3 and D6), so that "the
//! part checked" and "the part opened" are the same by construction:
//! - [`Package::xml_part`] is the only way to parse a part as XML: the XML part cap applies
//!   whatever the name (a main part may be `word/document.bin`; the content type decides), and
//!   the package's `max_xml_depth` and `max_xml_elements` bound the tree; relationships and
//!   content types are read from its result ([`crate::rels::from_document`],
//!   [`crate::content_types::ContentTypes::from_document`]);
//! - [`Package::resolve_target`] turns a relationship target into the exact name of the part the
//!   package opens (the OPC rule: [`part_name::resolve_relationship_target`], then ASCII case
//!   folding); a validator checks that name with [`Package::part`] or [`Package::xml_part`].
//!
//! Saving (gate 2, D5): an untouched entry is copied raw only when it is verified: the data the
//! reader locates through the local header inflates (or, stored, is) exactly to the declared
//! size, its CRC-32 is the declared one, and the declared compressed range lies inside the
//! archive. Anything else is written again from the bytes the reader produces (deflated at the
//! save level, or stored if it was stored), and an entry that cannot be read fails the save with
//! the read's error. Entries never read are verified by [`flate::check_raw`], which keeps no
//! output, so the save of a package whose media were never read holds none of them. That check
//! holds no memory, so it is not counted toward the total; its work is bounded instead by
//! excluding overlaps: an entry whose compressed range overlaps another entry's (a crafted
//! archive can point thousands of entries at one stream) is never verified nor copied raw, it is
//! read under the normal limits; every other entry is checked once with the most deflate can
//! produce from its bytes ([`MAX_DEFLATE_RATIO`]), so a save inflates at most that ratio times
//! the archive.
//!
//! `Package` is `Send`, not `Sync` (the caches are `OnceCell`s): reads borrow `&self`, so several
//! parts can be held at once.

use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

use crate::error::{Error, LimitKind, Result};
use crate::flate;
use crate::limits::Limits;
use crate::part_name;
use crate::xml::{self, ParseOptions, Source};
use crate::zip::{self, RawEntry, ZipEntry, ZipWriter};

/// The most output a save checks per compressed byte of an entry (with 1 MiB at least): deflate
/// cannot exceed 1,032:1, and Slides uses the same bound for its "impossible ratio" filter.
pub const MAX_DEFLATE_RATIO: u64 = 1100;

/// Options of a save: level, STORED names, timestamp (see [`zip::WriteOptions`]).
pub use crate::zip::WriteOptions as SaveOptions;

/// How [`Package::find`] folds names for its fallback lookup when the exact name is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseFold {
    /// exact names only (Write `DocxPackage`, Slides `PptxPackage`, ODF)
    #[default]
    None,
    /// JS `toLowerCase` (full Unicode lowercase): Sheet `XlsxPackage.caseless`
    Unicode,
    /// ASCII only, the OPC rule (C# `OpcPartName`)
    Ascii,
}

/// Options of [`Package::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenOptions {
    /// fallback lookup policy of [`Package::find`]
    pub case_fold: CaseFold,
    /// Slides: skip entries whose declared size is impossible for deflate,
    /// `originalSize > max(1 MiB, compressed × 1100)` (`slides-core/src/package.ts:26`)
    pub skip_impossible_ratio: bool,
    /// decode parts starting with a UTF-16 BOM as UTF-16 in [`Package::part_text`]; off, the
    /// TS behaviour (`TextDecoder` is UTF-8 only)
    pub utf16_text: bool,
}

/// What is known of an entry's stream once it was inflated or checked (D5).
#[derive(Debug, Clone, Copy)]
struct Known {
    /// the bytes the reader returns
    len: u64,
    /// the bytes the stream encodes (equal to `len` unless the declared size truncated it)
    stream_len: u64,
    /// CRC-32 of the bytes the reader returns (computed by a save or `verify`)
    crc32: Option<u32>,
}

enum Content {
    /// an entry of the input archive, inflated on demand
    Entry { entry: ZipEntry, inflated: OnceCell<Vec<u8>>, known: Cell<Option<Known>> },
    /// bytes given by the caller
    Owned(Vec<u8>),
}

struct Part {
    name: String,
    content: Content,
    /// the bytes of this part were added to the running total
    counted: Cell<bool>,
    /// its compressed range overlaps another entry's: a save reads it, never copies it raw
    shared: bool,
}

/// An OPC (or ODF) package: ordered named parts. See the module comment.
pub struct Package {
    source: Vec<u8>,
    parts: Vec<Part>,
    index: HashMap<String, usize>,
    caseless: OnceCell<HashMap<String, usize>>,
    /// ASCII-folded names for [`Package::resolve_target`] (the `caseless` index serves when the
    /// policy is `CaseFold::Ascii`)
    ascii: OnceCell<HashMap<String, usize>>,
    limits: Limits,
    options: OpenOptions,
    produced: Cell<u64>,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package").field("parts", &self.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()).finish()
    }
}

fn fold(policy: CaseFold, name: &str) -> Option<String> {
    match policy {
        CaseFold::None => None,
        CaseFold::Unicode => Some(name.to_lowercase()),
        CaseFold::Ascii => Some(name.to_ascii_lowercase()),
    }
}

/// Decodes part bytes like `new TextDecoder().decode(bytes)`, optionally honouring a UTF-16 BOM.
pub fn decode_text(bytes: &[u8], utf16: bool) -> Cow<'_, str> {
    if utf16 {
        let le = match bytes {
            [0xFF, 0xFE, ..] => Some(true),
            [0xFE, 0xFF, ..] => Some(false),
            _ => None,
        };
        if let Some(le) = le {
            let body = bytes.get(2..).unwrap_or(&[]);
            let units = body.chunks(2).map(|c| match c {
                [a, b] if le => u16::from_le_bytes([*a, *b]),
                [a, b] => u16::from_be_bytes([*a, *b]),
                // a trailing odd byte: replaced, as the WHATWG UTF-16 decoder does
                _ => 0xFFFD,
            });
            return Cow::Owned(char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect());
        }
    }
    zip::decode_utf8(bytes)
}

/// [`decode_text`] of bytes the caller gives up: UTF-8 keeps their buffer (the BOM removed in
/// place), so only invalid UTF-8 and UTF-16 are copied.
fn decode_text_owned(mut bytes: Vec<u8>, utf16: bool) -> String {
    if utf16 && matches!(bytes.as_slice(), [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..]) {
        return decode_text(&bytes, true).into_owned();
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// What [`Package::read`] found.
enum Read<'a> {
    /// bytes the package holds
    Held(&'a [u8]),
    /// bytes inflated by this read, not kept
    Inflated(Vec<u8>),
}

impl Package {
    fn with(source: Vec<u8>, limits: Limits, options: OpenOptions) -> Self {
        Package {
            source,
            parts: Vec::new(),
            index: HashMap::new(),
            caseless: OnceCell::new(),
            ascii: OnceCell::new(),
            limits,
            options,
            produced: Cell::new(0),
        }
    }

    /// Opens a zip package (copies `bytes`; see [`Package::open_owned`]).
    pub fn open(bytes: &[u8], limits: &Limits, options: &OpenOptions) -> Result<Package> {
        Package::open_owned(bytes.to_vec(), limits, options)
    }

    /// Opens a zip package, keeping `bytes` as the source of its parts.
    pub fn open_owned(bytes: Vec<u8>, limits: &Limits, options: &OpenOptions) -> Result<Package> {
        let entries = if options.skip_impossible_ratio {
            zip::read_filtered(&bytes, limits.max_entries, &mut |e| e.size <= (1u64 << 20).max(e.compressed_size.saturating_mul(1100)))?
        } else {
            zip::read(&bytes, limits.max_entries)?
        };
        let shared = overlapping(&entries, bytes.len() as u64);
        let mut pkg = Package::with(bytes, limits.clone(), *options);
        pkg.parts.reserve_exact(entries.len());
        pkg.index.reserve(entries.len());
        for (entry, shared) in entries.into_iter().zip(shared) {
            pkg.index.insert(entry.name.clone(), pkg.parts.len());
            pkg.parts.push(Part {
                name: entry.name.clone(),
                content: Content::Entry { entry, inflated: OnceCell::new(), known: Cell::new(None) },
                counted: Cell::new(false),
                shared,
            });
        }
        Ok(pkg)
    }

    /// A package without parts (the format crates add their templates).
    pub fn new_empty(limits: &Limits, options: &OpenOptions) -> Package {
        Package::with(Vec::new(), limits.clone(), *options)
    }

    /// A package from parts in memory, in the given order; a repeated name keeps its first
    /// position and its last bytes (`new Map(parts)`). Slides' `fromParts` takes a JS object:
    /// pass the parts through [`zip::js_object_order`] for its order.
    pub fn from_parts(parts: impl IntoIterator<Item = (String, Vec<u8>)>, limits: &Limits, options: &OpenOptions) -> Package {
        let mut pkg = Package::with(Vec::new(), limits.clone(), *options);
        for (name, data) in parts {
            pkg.set_part(name, data);
        }
        pkg
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn options(&self) -> &OpenOptions {
        &self.options
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Bytes produced by reads so far (each entry counted once), checked against `max_total_bytes`.
    pub fn produced_bytes(&self) -> u64 {
        self.produced.get()
    }

    /// Part names in package order (the file order of `unzipSync`, then parts added later).
    pub fn part_names(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.parts.iter().map(|p| p.name.as_str())
    }

    /// The exact name is a part.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// The real name of a part: the exact name, else the first part (in order) whose folded name
    /// matches under the [`CaseFold`] policy (`XlsxPackage.caseless`).
    pub fn find_name(&self, name: &str) -> Option<&str> {
        self.find_index(name).and_then(|i| self.parts.get(i)).map(|p| p.name.as_str())
    }

    fn find_index(&self, name: &str) -> Option<usize> {
        if let Some(&i) = self.index.get(name) {
            return Some(i);
        }
        let key = fold(self.options.case_fold, name)?;
        self.folded(&self.caseless, self.options.case_fold).get(&key).copied()
    }

    /// The index of folded names under `policy`, built on first use: the first part (in order)
    /// of each folded name.
    fn folded<'a>(&'a self, cell: &'a OnceCell<HashMap<String, usize>>, policy: CaseFold) -> &'a HashMap<String, usize> {
        cell.get_or_init(|| {
            let mut m = HashMap::with_capacity(self.parts.len());
            for (i, p) in self.parts.iter().enumerate() {
                if let Some(k) = fold(policy, &p.name) {
                    m.entry(k).or_insert(i);
                }
            }
            m
        })
    }

    /// The part a relationship `target` of `source_part` names (gate 2, D6): the OPC rule,
    /// [`part_name::resolve_relationship_target`], then the exact name, else the first part (in
    /// order) equal under ASCII case folding, whatever the [`CaseFold`] policy. Returns the name
    /// as stored, the one [`Package::part`] and [`Package::xml_part`] open; `None` for a target
    /// that names no part (external, malformed) or a part that does not exist.
    ///
    /// The one resolution path of format crates and hosts: a validator that checks the returned
    /// part checks exactly what the library opens. (Until stage 5 the apps' legacy target rules
    /// stay as parity policies in the tests.)
    pub fn resolve_target(&self, source_part: &str, target: &str) -> Option<&str> {
        let name = part_name::resolve_relationship_target(source_part, target)?;
        let i = match self.index.get(&name) {
            Some(&i) => i,
            None => {
                let cell = if self.options.case_fold == CaseFold::Ascii { &self.caseless } else { &self.ascii };
                *self.folded(cell, CaseFold::Ascii).get(&part_name::fold_ascii(&name))?
            }
        };
        self.parts.get(i).map(|p| p.name.as_str())
    }

    /// The bytes of a part by exact name (`getPart` of Write/Slides). Inflates on first read.
    pub fn part(&self, name: &str) -> Result<Option<&[u8]>> {
        match self.index.get(name) {
            Some(&i) => self.load(i).map(Some),
            None => Ok(None),
        }
    }

    /// The bytes of a part by name with the fallback of the [`CaseFold`] policy (`getPart` of Sheet).
    pub fn find(&self, name: &str) -> Result<Option<&[u8]>> {
        match self.find_index(name) {
            Some(i) => self.load(i).map(Some),
            None => Ok(None),
        }
    }

    /// Like [`Package::find`], but a missing part is `Error::MissingPart`.
    pub fn require(&self, name: &str) -> Result<&[u8]> {
        self.find(name)?.ok_or_else(|| Error::MissingPart(name.to_string()))
    }

    /// A part as text, by exact name: UTF-8 with U+FFFD for invalid sequences and the UTF-8 BOM
    /// removed (`new TextDecoder().decode`); UTF-16 only with `OpenOptions::utf16_text`.
    pub fn part_text(&self, name: &str) -> Result<Option<Cow<'_, str>>> {
        Ok(self.part(name)?.map(|b| decode_text(b, self.options.utf16_text)))
    }

    /// [`Package::part_text`] with the fallback lookup (`XlsxPackage.getPartText`).
    pub fn find_text(&self, name: &str) -> Result<Option<Cow<'_, str>>> {
        Ok(self.find(name)?.map(|b| decode_text(b, self.options.utf16_text)))
    }

    fn limit_error(&self, name: &str, part_cap: u64, cap: u64) -> Error {
        if cap < part_cap {
            Error::Limit { kind: LimitKind::TotalBytes, max: self.limits.max_total_bytes, part: None }
        } else {
            Error::Limit { kind: LimitKind::PartBytes, max: part_cap, part: Some(name.to_string()) }
        }
    }

    /// The cap of the next read of `part` with `part_cap`: the part cap, and the rest of the
    /// total if it was never counted.
    fn cap_for(&self, part: &Part, part_cap: u64) -> (u64, u64) {
        if part.counted.get() {
            return (part_cap, part_cap);
        }
        let left = self.limits.max_total_bytes.saturating_sub(self.produced.get());
        (part_cap, part_cap.min(left))
    }

    fn count(&self, part: &Part, n: usize) {
        if !part.counted.replace(true) {
            self.produced.set(self.produced.get().saturating_add(n as u64));
        }
    }

    fn load(&self, i: usize) -> Result<&[u8]> {
        let Some(part) = self.parts.get(i) else { return Err(Error::MissingPart(String::new())) };
        self.load_capped(part, self.limits.part_cap(&part.name))
    }

    /// Reads `part`, inflating it (under `part_cap` and the total) if it was never read; the
    /// inflated bytes are kept for the next reads.
    fn load_capped<'a>(&'a self, part: &'a Part, part_cap: u64) -> Result<&'a [u8]> {
        match self.read(part, part_cap)? {
            Read::Held(b) => Ok(b),
            Read::Inflated(bytes) => match &part.content {
                Content::Entry { inflated, .. } => Ok(inflated.get_or_init(|| bytes)),
                Content::Owned(v) => Ok(v),
            },
        }
    }

    /// Reads `part` without keeping what it inflates: the bytes the package already holds (set
    /// by the caller, inflated before, or a stored entry, a slice of the input), or the output
    /// of an inflate that the caller takes. The part counts toward the total on its first read.
    fn read<'a>(&'a self, part: &'a Part, part_cap: u64) -> Result<Read<'a>> {
        let (entry, inflated, known) = match &part.content {
            Content::Owned(v) => return Ok(Read::Held(v)),
            Content::Entry { entry, inflated, known } => (entry, inflated, known),
        };
        if let Some(v) = inflated.get() {
            return Ok(Read::Held(v));
        }
        let (part_cap, cap) = self.cap_for(part, part_cap);
        let raw = entry.compressed(&self.source);
        if entry.method == 0 {
            // `slc(data, b, b + sc)`: the declared uncompressed size is ignored (browser.js:2682)
            if raw.len() as u64 > cap {
                return Err(self.limit_error(&part.name, part_cap, cap));
            }
            self.count(part, raw.len());
            return Ok(Read::Held(raw));
        }
        let (bytes, stream_len) = match flate::inflate_raw_counted(raw, Some(entry.size), cap) {
            Ok(b) => b,
            Err(Error::Limit { .. }) => return Err(self.limit_error(&part.name, part_cap, cap)),
            Err(e) => return Err(e),
        };
        self.count(part, bytes.len());
        if known.get().is_none() {
            known.set(Some(Known { len: bytes.len() as u64, stream_len, crc32: None }));
        }
        Ok(Read::Inflated(bytes))
    }

    /// Parses a part as XML (gate 2, D3): the one way the format crates parse a part. The part
    /// is found like [`Package::find`] (exact name, then the [`CaseFold`] policy) and read with
    /// `max_xml_part_bytes` whatever its name; its text is decoded like [`Package::part_text`]
    /// and parsed with `options`, whose depth and element limits are lowered to the package's
    /// `max_xml_depth` and `max_xml_elements`. A missing part is `Error::MissingPart`; a limit
    /// error names the part. A part inflated by this call is not kept by the package (the tree
    /// owns its text; a later read inflates it again, without counting it again toward the
    /// total); bytes the package already holds are copied into the tree.
    pub fn xml_part(&self, name: &str, options: &ParseOptions<'_>) -> Result<xml::Document> {
        let part = self.find_index(name).and_then(|i| self.parts.get(i)).ok_or_else(|| Error::MissingPart(name.to_string()))?;
        let cap = self.limits.max_xml_part_bytes;
        // an entry inflated here is not kept by the package: the tree takes its bytes as its
        // source without a copy (the text is valid UTF-8 almost always)
        let text: Source = match self.read(part, cap)? {
            Read::Inflated(bytes) => decode_text_owned(bytes, self.options.utf16_text).into(),
            Read::Held(bytes) => {
                // read before through `part` with the binary cap of its name, or set by the caller
                if bytes.len() as u64 > cap {
                    return Err(Error::Limit { kind: LimitKind::PartBytes, max: cap, part: Some(part.name.clone()) });
                }
                decode_text(bytes, self.options.utf16_text).into()
            }
        };
        let lower = |own: Option<u32>, package: u32| Some(own.map_or(package, |o| o.min(package)));
        let options = ParseOptions {
            max_depth: lower(options.max_depth, self.limits.max_xml_depth),
            max_elements: lower(options.max_elements, self.limits.max_xml_elements),
            ..*options
        };
        xml::parse(text, &options).map_err(|e| match e {
            Error::Limit { kind, max, part: None } => Error::Limit { kind, max, part: Some(part.name.clone()) },
            e => e,
        })
    }

    /// Whether a save may copy the entry raw (D5): the stream is verified once and the result
    /// kept. An entry whose range overlaps another's is not (the save reads it under the
    /// limits). Verifying a never-read entry inflates it without keeping the output
    /// ([`flate::check_raw`]), up to [`MAX_DEFLATE_RATIO`] times its compressed bytes and not
    /// counted toward the total (it holds no memory); a stream that would go past that ratio is
    /// not intact, and the read that follows reports the limit. Other errors are the read's.
    fn intact(&self, part: &Part) -> Result<bool> {
        let Content::Entry { entry, inflated, known } = &part.content else { return Ok(false) };
        if part.shared {
            return Ok(false);
        }
        let raw = entry.compressed(&self.source);
        let k = match known.get() {
            Some(k @ Known { crc32: Some(_), .. }) => k,
            Some(k) if let Some(out) = inflated.get() => Known { crc32: Some(flate::crc32(out)), ..k },
            // a stored entry is read as its compressed bytes (see `load_capped`)
            _ if entry.method == 0 => Known { len: raw.len() as u64, stream_len: raw.len() as u64, crc32: Some(flate::crc32(raw)) },
            _ => {
                let cap = (raw.len() as u64).saturating_mul(MAX_DEFLATE_RATIO).max(1 << 20);
                match flate::check_raw(raw, Some(entry.size), cap) {
                    Ok(c) => Known { len: c.len, stream_len: c.stream_len, crc32: Some(c.crc32) },
                    Err(Error::Limit { .. }) => return Ok(false),
                    Err(e) => return Err(e),
                }
            }
        };
        known.set(Some(k));
        // the local header only gives where the data starts (its name and extra lengths): data
        // found at a wrong place fails the size or CRC check; a compressed range cut by the end
        // of the archive means the central record lies about it
        let in_archive = entry.data_start.checked_add(entry.compressed_size).is_some_and(|end| end <= self.source.len() as u64);
        Ok(in_archive && k.len == entry.size && k.stream_len == entry.size && k.crc32 == Some(entry.crc32))
    }

    /// Inflates every entry not read yet without keeping the output ([`flate::check_raw`]), with
    /// the part caps and the total: the TS inflates everything at load, so a damaged entry fails
    /// the whole package there. What it finds serves a later save (D5).
    pub fn verify(&self) -> Result<()> {
        let mut total = 0u64;
        for part in &self.parts {
            let Content::Entry { entry, inflated, known } = &part.content else { continue };
            let part_cap = self.limits.part_cap(&part.name);
            let left = self.limits.max_total_bytes.saturating_sub(total);
            let cap = part_cap.min(left);
            let raw = entry.compressed(&self.source);
            let n = if let Some(v) = inflated.get() {
                v.len() as u64
            } else if entry.method == 0 {
                raw.len() as u64
            } else if let Some(k) = known.get() {
                k.len
            } else {
                // inflated without keeping the output; what it found serves a later save (D5)
                match flate::check_raw(raw, Some(entry.size), cap) {
                    Ok(c) => {
                        known.set(Some(Known { len: c.len, stream_len: c.stream_len, crc32: Some(c.crc32) }));
                        c.len
                    }
                    Err(Error::Limit { .. }) => return Err(self.limit_error(&part.name, part_cap, cap)),
                    Err(e) => return Err(e),
                }
            };
            if n > cap {
                return Err(self.limit_error(&part.name, part_cap, cap));
            }
            total += n;
        }
        Ok(())
    }

    /// Drops the inflated copy of a part (it is inflated again on the next read, without
    /// counting again toward the total). Returns whether a copy was dropped.
    pub fn release(&mut self, name: &str) -> bool {
        let Some(&i) = self.index.get(name) else { return false };
        match self.parts.get_mut(i).map(|p| &mut p.content) {
            Some(Content::Entry { inflated, .. }) => inflated.take().is_some(),
            _ => false,
        }
    }

    /// Drops every inflated copy.
    pub fn release_all(&mut self) {
        for p in &mut self.parts {
            if let Content::Entry { inflated, .. } = &mut p.content {
                inflated.take();
            }
        }
    }

    /// Sets a part: a new name goes last, an existing one keeps its position (`Map.set`).
    pub fn set_part(&mut self, name: impl Into<String>, data: Vec<u8>) {
        let name = name.into();
        match self.index.get(&name) {
            Some(&i) => {
                if let Some(p) = self.parts.get_mut(i) {
                    p.content = Content::Owned(data);
                }
            }
            None => {
                self.index.insert(name.clone(), self.parts.len());
                self.parts.push(Part { name, content: Content::Owned(data), counted: Cell::new(true), shared: false });
                self.caseless.take();
                self.ascii.take();
            }
        }
    }

    /// Sets a part as UTF-8 text (`new TextEncoder().encode(text)`, no BOM).
    pub fn set_part_text(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.set_part(name, text.into().into_bytes());
    }

    /// Removes a part by exact name; the others keep their order. Returns whether it existed.
    /// (Sheet's `deletePart` is `remove_part(find_name(name))`.)
    pub fn remove_part(&mut self, name: &str) -> bool {
        let Some(i) = self.index.remove(name) else { return false };
        if i < self.parts.len() {
            self.parts.remove(i);
        }
        for v in self.index.values_mut() {
            if *v > i {
                *v -= 1;
            }
        }
        self.caseless.take();
        self.ascii.take();
        true
    }

    /// Keeps only the parts for which `keep(name)` is true.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        let before = self.parts.len();
        self.parts.retain(|p| keep(&p.name));
        if self.parts.len() != before {
            self.index.clear();
            for (i, p) in self.parts.iter().enumerate() {
                self.index.insert(p.name.clone(), i);
            }
            self.caseless.take();
            self.ascii.take();
        }
    }

    /// Writes the package as a zip like `zipSync(parts, { level: 6 })`: parts in JS object
    /// order (array-index names first, `zip::js_object_order`); parts set by the caller are
    /// compressed; entries never set are copied raw (same method, bytes, CRC and declared size)
    /// when they are verified (D5, see the module comment), else written again from the bytes
    /// the reader produces (deflated, or stored if they were stored); `stored` names held
    /// deflated are inflated and stored. An entry that cannot be read fails the save with the
    /// read's error.
    pub fn save(&self, options: &SaveOptions) -> Result<Vec<u8>> {
        let order = zip::js_object_order(self.parts.iter().map(|p| (p.name.clone(), p)).collect());
        // what each entry writes, decided (and compressed) first so that the archive is reserved
        // at its exact length: no growth by doubling, no unused capacity in the result
        let mut deflater = flate::Deflater::new(options.level);
        let mut deflated: Vec<u8> = Vec::new();
        let mut plan: Vec<(String, Planned<'_>)> = Vec::with_capacity(order.len());
        for (name, part) in order {
            let planned = match &part.content {
                Content::Owned(data) => self.plan_bytes(&name, data, options, &mut deflater, &mut deflated)?,
                Content::Entry { entry, .. } => {
                    let restore = entry.method != 0 && options.stored.contains(&name);
                    if !restore && self.intact(part)? {
                        let raw = entry.compressed(&self.source);
                        Planned::Raw(RawEntry { method: entry.method, crc32: entry.crc32, size: entry.size, data: raw })
                    } else if entry.method == 0 {
                        let data = self.load_capped(part, self.limits.part_cap(&part.name))?;
                        Planned::Raw(RawEntry { method: 0, crc32: flate::crc32(data), size: data.len() as u64, data })
                    } else {
                        let data = self.load_capped(part, self.limits.part_cap(&part.name))?;
                        self.plan_bytes(&name, data, options, &mut deflater, &mut deflated)?
                    }
                }
            };
            if name.len() > 65_535 {
                return Err(Error::NameTooLong { len: name.len() });
            }
            plan.push((name, planned));
        }
        drop(deflater);
        let compressed = |p: &Planned<'_>| match p {
            Planned::Raw(r) => r.data.len() as u64,
            Planned::Deflated { range, .. } => range.len() as u64,
        };
        let len = zip::archive_len(plan.iter().map(|(n, p)| {
            let (method, size) = match p {
                Planned::Raw(r) => (r.method, r.size),
                Planned::Deflated { size, .. } => (8, *size),
            };
            (n.len(), method, compressed(p), size)
        }));
        let mut w = ZipWriter::with_capacity(options.clone(), len);
        for (name, p) in &plan {
            let raw = match p {
                Planned::Raw(r) => *r,
                Planned::Deflated { range, crc32, size } => {
                    RawEntry { method: 8, crc32: *crc32, size: *size, data: deflated.get(range.clone()).unwrap_or(&[]) }
                }
            };
            w.add_raw(name, raw)?;
        }
        Ok(w.finish())
    }

    /// The entry of bytes written again: deflated into `deflated` at the save level, or stored
    /// (level 0 or a `stored` name), as `ZipWriter::add` writes them.
    fn plan_bytes<'a>(
        &self,
        name: &str,
        data: &'a [u8],
        options: &SaveOptions,
        deflater: &mut flate::Deflater,
        deflated: &mut Vec<u8>,
    ) -> Result<Planned<'a>> {
        let crc32 = flate::crc32(data);
        let size = data.len() as u64;
        if options.level == 0 || options.stored.iter().any(|s| s == name) {
            return Ok(Planned::Raw(RawEntry { method: 0, crc32, size, data }));
        }
        let start = deflated.len();
        deflater.deflate_into(data, deflated)?;
        Ok(Planned::Deflated { range: start..deflated.len(), crc32, size })
    }
}

/// What [`Package::save`] writes for an entry: compressed bytes held elsewhere (copied raw, or
/// stored), or a range of the save's deflate output.
enum Planned<'a> {
    Raw(RawEntry<'a>),
    Deflated { range: std::ops::Range<usize>, crc32: u32, size: u64 },
}

/// Which entries have a compressed range that overlaps another entry's (empty ranges never do):
/// one sweep over the ranges sorted by start, clamped to the archive as the reader clamps them.
fn overlapping(entries: &[ZipEntry], len: u64) -> Vec<bool> {
    let range = |e: &ZipEntry| {
        let start = e.data_start.min(len);
        (start, e.data_start.saturating_add(e.compressed_size).min(len).max(start))
    };
    let mut order: Vec<(u64, u64, usize)> =
        entries.iter().enumerate().map(|(i, e)| (range(e), i)).filter(|((s, e), _)| s < e).map(|((s, e), i)| (s, e, i)).collect();
    order.sort_unstable();
    let mut shared = vec![false; entries.len()];
    // the entry reaching furthest so far, and where it ends
    let mut furthest: Option<(usize, u64)> = None;
    for (start, end, i) in order {
        match furthest {
            Some((j, reach)) if start < reach => {
                for k in [i, j] {
                    if let Some(s) = shared.get_mut(k) {
                        *s = true;
                    }
                }
                if end > reach {
                    furthest = Some((i, end));
                }
            }
            _ => furthest = Some((i, end)),
        }
    }
    shared
}
