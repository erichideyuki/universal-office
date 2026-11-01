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
//!   the drop of the package;
//! - parts given by `set_part`/`from_parts` are owned by the package until replaced or removed;
//! - the case-insensitive index (only with a [`CaseFold`] policy) is built on the first fallback
//!   lookup and dropped on any change of the part set; it holds one folded name per part.
//!
//! Limits: each read checks `Limits::part_cap(name)` on the bytes it produces, and the running
//! total of produced bytes (each entry counted once, the first time it is read) against
//! `max_total_bytes`. Declared sizes never reserve memory. Errors of a damaged entry surface
//! when that entry is read; [`Package::verify`] reproduces the TS behaviour of failing at load.
//!
//! `Package` is `Send`, not `Sync` (the caches are `OnceCell`s): reads borrow `&self`, so several
//! parts can be held at once.

use std::borrow::Cow;
use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

use crate::error::{Error, LimitKind, Result};
use crate::flate;
use crate::limits::Limits;
use crate::zip::{self, RawEntry, ZipEntry, ZipWriter};

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

enum Content {
    /// an entry of the input archive, inflated on demand
    Entry { entry: ZipEntry, inflated: OnceCell<Vec<u8>> },
    /// bytes given by the caller
    Owned(Vec<u8>),
}

struct Part {
    name: String,
    content: Content,
    /// the bytes of this part were added to the running total
    counted: Cell<bool>,
}

/// An OPC (or ODF) package: ordered named parts. See the module comment.
pub struct Package {
    source: Vec<u8>,
    parts: Vec<Part>,
    index: HashMap<String, usize>,
    caseless: OnceCell<HashMap<String, usize>>,
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

impl Package {
    fn with(source: Vec<u8>, limits: Limits, options: OpenOptions) -> Self {
        Package { source, parts: Vec::new(), index: HashMap::new(), caseless: OnceCell::new(), limits, options, produced: Cell::new(0) }
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
        let mut pkg = Package::with(bytes, limits.clone(), *options);
        pkg.parts.reserve_exact(entries.len());
        pkg.index.reserve(entries.len());
        for entry in entries {
            pkg.index.insert(entry.name.clone(), pkg.parts.len());
            pkg.parts.push(Part {
                name: entry.name.clone(),
                content: Content::Entry { entry, inflated: OnceCell::new() },
                counted: Cell::new(false),
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
        let lower = self.caseless.get_or_init(|| {
            let mut m = HashMap::with_capacity(self.parts.len());
            for (i, p) in self.parts.iter().enumerate() {
                if let Some(k) = fold(self.options.case_fold, &p.name) {
                    m.entry(k).or_insert(i);
                }
            }
            m
        });
        lower.get(&key).copied()
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

    /// The cap of the next read of `part`: its part cap, and the rest of the total if it was
    /// never counted.
    fn cap_for(&self, part: &Part) -> (u64, u64) {
        let part_cap = self.limits.part_cap(&part.name);
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
        let (entry, inflated) = match &part.content {
            Content::Owned(v) => return Ok(v),
            Content::Entry { entry, inflated } => (entry, inflated),
        };
        if let Some(v) = inflated.get() {
            return Ok(v);
        }
        let (part_cap, cap) = self.cap_for(part);
        let raw = entry.compressed(&self.source);
        if entry.method == 0 {
            // `slc(data, b, b + sc)`: the declared uncompressed size is ignored (browser.js:2682)
            if raw.len() as u64 > cap {
                return Err(self.limit_error(&part.name, part_cap, cap));
            }
            self.count(part, raw.len());
            return Ok(raw);
        }
        let bytes = match flate::inflate_raw(raw, Some(entry.size), cap) {
            Ok(b) => b,
            Err(Error::Limit { .. }) => return Err(self.limit_error(&part.name, part_cap, cap)),
            Err(e) => return Err(e),
        };
        self.count(part, bytes.len());
        Ok(inflated.get_or_init(|| bytes))
    }

    /// Inflates every entry not read yet and drops the output, with the part caps and the total:
    /// the TS inflates everything at load, so a damaged entry fails the whole package there.
    pub fn verify(&self) -> Result<()> {
        let mut total = 0u64;
        for part in &self.parts {
            let Content::Entry { entry, inflated } = &part.content else { continue };
            let part_cap = self.limits.part_cap(&part.name);
            let left = self.limits.max_total_bytes.saturating_sub(total);
            let cap = part_cap.min(left);
            let raw = entry.compressed(&self.source);
            let n = if let Some(v) = inflated.get() {
                v.len() as u64
            } else if entry.method == 0 {
                raw.len() as u64
            } else {
                match flate::inflate_raw(raw, Some(entry.size), cap) {
                    Ok(b) => b.len() as u64,
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
                self.parts.push(Part { name, content: Content::Owned(data), counted: Cell::new(true) });
                self.caseless.take();
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
        }
    }

    /// Writes the package as a zip like `zipSync(parts, { level: 6 })`: parts in JS object
    /// order (array-index names first, `zip::js_object_order`); parts set by the caller are
    /// compressed, entries never set are copied raw (same method, bytes, CRC and declared size),
    /// except `stored` names held deflated, which are inflated and stored.
    pub fn save(&self, options: &SaveOptions) -> Result<Vec<u8>> {
        let order = zip::js_object_order(self.parts.iter().map(|p| (p.name.clone(), p)).collect());
        let mut w = ZipWriter::new(options.clone());
        for (name, part) in order {
            match &part.content {
                Content::Owned(data) => w.add(&name, data)?,
                Content::Entry { entry, .. } => {
                    if entry.method != 0 && options.stored.contains(&name) {
                        let i = self.index.get(&name).copied().unwrap_or(usize::MAX);
                        w.add(&name, self.load(i)?)?;
                    } else {
                        let raw = entry.compressed(&self.source);
                        w.add_raw(&name, RawEntry { method: entry.method, crc32: entry.crc32, size: entry.size, data: raw })?;
                    }
                }
            }
        }
        Ok(w.finish())
    }
}
