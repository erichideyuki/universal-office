//! What the shared reader needs from a host package (`PartSource`, `package.ts:8`:
//! `getPart | getPartText | resolveRelTarget`): implemented by `uo_pptx`'s part store and by the
//! Sheet package, each with its own parse cache. Also the pieces every reader function shares:
//! the typed error, `readRels` (`read.ts:28`), the lenient XML preset of slides-core and the
//! attribute helpers that reproduce the TS truthiness tests.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_opc::xml::{CharRefPolicy, ContentPi, Document, Element, ParseOptions, Source};

use crate::ids::MediaId;

/// Why a read failed. The TS throws (`parseXml` on a malformed part, a missing part); `readSlide`
/// callers turn it into the slide's error, so the variants are the classes, not the text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ReadError {
    /// `XML invalido @…` of `xml.ts`.
    Xml(String),
    /// A part the read needs is missing (`Pacote sem …`).
    Missing(Box<str>),
    /// A [`crate::Limits`] value was exceeded (a declared refusal of hostile input).
    Limit(&'static str),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Xml(m) => write!(f, "XML invalido: {m}"),
            ReadError::Missing(p) => write!(f, "Pacote sem {p}"),
            ReadError::Limit(w) => write!(f, "limite excedido: {w}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// The reader's result type.
pub type R<T> = Result<T, ReadError>;

/// Caps of the reader on untrusted input (the TS has none: a deep group raises a `RangeError`, a
/// huge table exhausts the heap). Defaults mirror `uo_pptx::Limits`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// `readGroup` nesting (the TS recursion).
    pub max_group_depth: u32,
    /// Shapes of one list (flattened groups included).
    pub max_shapes: u32,
    /// Rows × columns of one table (the border grids are `rows × cols`).
    pub max_table_cells: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_group_depth: 64, max_shapes: 100_000, max_table_cells: 1_000_000 }
    }
}

/// A package the shape reader can read parts from.
pub trait PartSource {
    /// The part's bytes (`getPart`), shared with the store.
    fn part(&self, name: &str) -> Option<Arc<[u8]>>;
    /// The part parsed with the lenient Slides preset (`parseXml(getPartText(…))`), through the
    /// host's parse cache. `Ok(None)` = the part does not exist.
    fn xml(&self, name: &str) -> R<Option<Arc<Document>>>;
    /// `getPartText` (`TextDecoder`: BOM stripped, U+FFFD for bad bytes).
    fn text(&self, name: &str) -> Option<String> {
        self.part(name).map(|b| uo_jsnum::text::decode_utf8(&b).into_owned())
    }
    /// The media table entry of a part (bytes, mime, header size and dpi), created on first use.
    fn media(&self, part: &str) -> Option<MediaId>;
    /// The part name and bytes of a media entry (what `ImageRef.part` and `ImageRef.bytes` were).
    fn media_entry(&self, id: MediaId) -> Option<(Box<str>, Arc<[u8]>)>;
    /// A resolved theme as the model keeps it (a slide's theme, a chart's theme). A host with a
    /// deck-scope table returns the copy it already holds when an EQUAL theme was resolved before, so
    /// the slides of a master share one theme (memory compaction, `uo-pptx.md` "Model memory"); the
    /// default allocates. Values are the same either way.
    fn share_theme(&self, theme: crate::theme::Theme) -> Arc<crate::theme::Theme> {
        Arc::new(theme)
    }
}

/// The element-name prefix of the shape tree: `p:` (slides) or `xdr:` (spreadsheet drawings). The
/// TS renames the prefix (`renamePrefix`, read.ts:1161); the reader matches against the prefix
/// instead (same names, no tree copy).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapePrefix {
    P,
    Xdr,
}

/// An element of a document the host already parsed (a drawing object of a sheet).
#[derive(Clone, Debug)]
pub struct ElementRef {
    pub doc: Arc<Document>,
    pub element: uo_opc::xml::ElementId,
}

/// Sheet's anchor rectangle in EMU, applied where the reader reads the object's own `a:xfrm`
/// (`placeElement`, drawings.ts:72). The TS writes synthetic `a:off`/`a:ext` (and the group's
/// `chOff`/`chExt` defaults) with `String(Math.round(v))` and reads them back with `parseInt`; the
/// reader reproduces that round trip exactly (`v ≥ 1e21` prints `1e+21` and reads back as 1, `-0`
/// as 0).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameOverride {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The XML options of slides-core's `parseXml` (`xml.ts`): lenient, processing instructions inside
/// content skipped, a numeric reference outside Unicode replaced by U+FFFD, no depth limit.
pub fn xml_options() -> ParseOptions<'static> {
    ParseOptions { content_pi: ContentPi::Skip, bad_char_ref: CharRefPolicy::Replace, ..ParseOptions::LENIENT }
}

/// `parseXml(text)` with the Slides options.
pub fn parse_xml(text: impl Into<Source>) -> R<Document> {
    uo_opc::xml::parse(text, &xml_options()).map_err(|e| ReadError::Xml(e.to_string()))
}

/// `resolveRelTarget` (`package.ts:63`): `/abs` → `abs`; otherwise the owner's folder with every
/// leading `../` of the target walked up.
pub fn resolve_rel_target(from_part: &str, target: &str) -> String {
    if let Some(rest) = target.strip_prefix('/') {
        return rest.to_string();
    }
    let mut dir: &str = match from_part.rfind('/') {
        Some(i) => from_part.get(..i).unwrap_or(""),
        None => "",
    };
    let mut t = target;
    while let Some(rest) = t.strip_prefix("../") {
        t = rest;
        dir = match dir.rfind('/') {
            Some(i) => dir.get(..i).unwrap_or(""),
            None => "",
        };
    }
    if dir.is_empty() { t.to_string() } else { format!("{dir}/{t}") }
}

/// `relsPathFor` (`read.ts:46`): `dir/_rels/file.rels` (a part without a folder keeps `dir` = the
/// whole name before the last slash, which for `-1` is the name minus its last character — the TS
/// `slice(0, -1)` quirk, kept).
pub fn rels_path_for(part: &str) -> String {
    match part.rfind('/') {
        Some(slash) => {
            let dir = part.get(..slash).unwrap_or("");
            let file = part.get(slash + 1..).unwrap_or("");
            format!("{dir}/_rels/{file}.rels")
        }
        None => {
            // lastIndexOf = -1: slice(0, -1) drops the last UTF-16 unit, slice(0) is the whole name
            let mut chars = part.chars();
            chars.next_back();
            format!("{}/_rels/{part}.rels", chars.as_str())
        }
    }
}

/// `Map<string, string>` of `readRels`: insertion order with `Map.set` semantics (a repeated id
/// keeps its first position and takes the last target).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RelMap {
    entries: Vec<(Box<str>, Box<str>)>,
    index: FxHashMap<Box<str>, u32>,
}

impl RelMap {
    /// An empty map (`new Map()`).
    pub fn new() -> RelMap {
        RelMap::default()
    }

    /// `map.set(id, target)`.
    pub fn set(&mut self, id: &str, target: &str) {
        match self.index.get(id).and_then(|&i| self.entries.get_mut(i as usize)) {
            Some(e) => e.1 = target.into(),
            None => {
                self.index.insert(id.into(), u32::try_from(self.entries.len()).unwrap_or(u32::MAX));
                self.entries.push((id.into(), target.into()));
            }
        }
    }

    /// `map.get(id)`.
    pub fn get(&self, id: &str) -> Option<&str> {
        let i = *self.index.get(id)?;
        self.entries.get(i as usize).map(|e| &*e.1)
    }

    /// `map.values()`.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| &*e.1)
    }

    /// `[...map]`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|e| (&*e.0, &*e.1))
    }

    /// `map.size`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `map.size === 0`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `readRels` (`read.ts:28`): the relationships of `rels_part` (owned by `owner`) as id → target;
/// internal targets are resolved to package paths, `External` ones (hyperlinks) stay raw. An absent
/// part is an empty map.
pub fn read_rels(src: &dyn PartSource, rels_part: &str, owner: &str) -> R<RelMap> {
    let mut map = RelMap::new();
    let Some(doc) = src.xml(rels_part)? else { return Ok(map) };
    for rel in doc.root().children_named("Relationship") {
        let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else { continue };
        if rel.attr("TargetMode") == Some("External") {
            map.set(id, target);
        } else {
            map.set(id, &resolve_rel_target(owner, target));
        }
    }
    Ok(map)
}

/// `findRelTarget` (`read.ts:152`): the first relationship of `part` whose `Type` ends with
/// `/<suffix>`, resolved.
pub fn find_rel_target(src: &dyn PartSource, part: &str, suffix: &str) -> R<Option<String>> {
    if part.is_empty() {
        return Ok(None);
    }
    let Some(doc) = src.xml(&rels_path_for(part))? else { return Ok(None) };
    let want = format!("/{suffix}");
    for rel in doc.root().children_named("Relationship") {
        if rel.attr("Type").unwrap_or("").ends_with(&want) {
            return Ok(Some(resolve_rel_target(part, rel.attr("Target").unwrap_or(""))));
        }
    }
    Ok(None)
}

/// An attribute that is defined **and** non-empty: the TS `if (el.attrs['k'])` test.
pub fn ta<'d>(el: Element<'d>, k: &str) -> Option<&'d str> {
    el.attr(k).filter(|v| !v.is_empty())
}

/// `parseInt(s, 10)` of a defined attribute (NaN when it holds no digits).
pub fn int_attr(el: Element<'_>, k: &str) -> Option<f64> {
    el.attr(k).map(|v| uo_jsnum::parse::parse_int(v, 10))
}

/// `parseInt(attr ?? dflt, 10)`.
pub fn int_or(el: Option<Element<'_>>, k: &str, dflt: f64) -> f64 {
    match el.and_then(|e| e.attr(k)) {
        Some(v) => uo_jsnum::parse::parse_int(v, 10),
        None => dflt,
    }
}

/// `parseInt(v, 10)`.
pub fn pint(v: &str) -> f64 {
    uo_jsnum::parse::parse_int(v, 10)
}

/// `/^-?\d+$/.test(v)`.
pub fn is_int_text(v: &str) -> bool {
    let b = v.as_bytes();
    let d = b.strip_prefix(b"-").unwrap_or(b);
    !d.is_empty() && d.iter().all(u8::is_ascii_digit)
}

/// `x || undefined` for a number: 0 and NaN become absent.
pub fn nonzero(x: f64) -> Option<f64> {
    (x != 0.0 && !x.is_nan()).then_some(x)
}
