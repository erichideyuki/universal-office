//! The open deck (`uo-pptx.md` P3): the part store, the parse cache, the media table and the slide
//! cache. Every cache is behaviour-neutral (the TS re-parses on every call) and capped (memory
//! gate M5): the parse cache by bytes (LRU), the slide cache by count (LRU), the media table by
//! the package's media.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_dml_render::ids::MediaId;
use uo_dml_render::source::{PartSource, R, ReadError, parse_xml};
use uo_dml_render::theme::Theme;
use uo_opc::xml::Document;

use crate::error::PptxError;
use crate::fonts::{self, EmbeddedFont};
use crate::limits::Limits;
use crate::model::{Presentation, ReadGen, ReadSet, SlideModel};
use crate::package::{PartGen, PartStore, RawParts};
use crate::read;

/// A parsed part, valid while its generation is the store's.
#[derive(Clone, Debug)]
struct CacheEntry {
    name: Box<str>,
    generation: PartGen,
    doc: Arc<Document>,
    size: usize,
}

/// The parse cache: `parseXml(getPartText(name))` per part version, LRU with a byte cap (M5).
#[derive(Debug, Default)]
pub struct XmlCache {
    entries: Vec<CacheEntry>,
    bytes: usize,
    cap: usize,
}

impl XmlCache {
    fn with_cap(cap: usize) -> XmlCache {
        XmlCache { entries: Vec::new(), bytes: 0, cap }
    }

    fn get(&mut self, name: &str, generation: PartGen) -> Option<Arc<Document>> {
        let pos = self.entries.iter().position(|e| &*e.name == name)?;
        if self.entries.get(pos).is_some_and(|e| e.generation != generation) {
            let old = self.entries.remove(pos);
            self.bytes -= old.size;
            return None;
        }
        let e = self.entries.remove(pos);
        let doc = Arc::clone(&e.doc);
        self.entries.push(e); // most recently used last
        Some(doc)
    }

    fn put(&mut self, name: &str, generation: PartGen, doc: &Arc<Document>) {
        if let Some(pos) = self.entries.iter().position(|e| &*e.name == name) {
            let old = self.entries.remove(pos);
            self.bytes -= old.size;
        }
        let size = doc.heap_bytes();
        if size > self.cap {
            return; // a part bigger than the whole cache is not kept
        }
        while self.bytes + size > self.cap && !self.entries.is_empty() {
            let old = self.entries.remove(0);
            self.bytes -= old.size;
        }
        self.bytes += size;
        self.entries.push(CacheEntry { name: name.into(), generation, doc: Arc::clone(doc), size });
    }

    /// Bytes the cached trees hold (heap estimate).
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Number of cached parts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Nothing cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn clear(&mut self) {
        self.entries = Vec::new();
        self.bytes = 0;
    }
}

/// One media part: the bytes are the part store's `Arc`.
#[derive(Clone, Debug)]
pub struct MediaEntry {
    pub part: Box<str>,
    pub bytes: Arc<[u8]>,
    generation: PartGen,
}

/// The media of the package by part name (bounded by the package itself).
#[derive(Debug, Default)]
pub struct MediaTable {
    entries: Vec<MediaEntry>,
    index: FxHashMap<Box<str>, u32>,
}

impl MediaTable {
    /// The entry of a media id.
    pub fn get(&self, id: MediaId) -> Option<&MediaEntry> {
        self.entries.get(id.0 as usize)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn ensure(&mut self, store: &PartStore, part: &str) -> Option<MediaId> {
        let e = store.get(part)?;
        match self.index.get(part).copied() {
            Some(i) => {
                if let Some(m) = self.entries.get_mut(i as usize)
                    && m.generation != e.generation
                {
                    m.bytes = Arc::clone(&e.bytes);
                    m.generation = e.generation;
                }
                Some(MediaId(i))
            }
            None => {
                let i = u32::try_from(self.entries.len()).ok()?;
                self.entries.push(MediaEntry { part: part.into(), bytes: Arc::clone(&e.bytes), generation: e.generation });
                self.index.insert(part.into(), i);
                Some(MediaId(i))
            }
        }
    }

    fn clear(&mut self) {
        self.entries = Vec::new();
        self.index = FxHashMap::default();
    }
}

/// The resolved themes the deck's slide models share (`PartSource::share_theme`): one copy per distinct
/// theme value instead of one per slide (the TS model keeps no theme in a slide; memory compaction,
/// `uo-pptx.md` "Model memory"). FIFO, capped at [`MAX_SHARED_THEMES`] (M5); an evicted theme stays
/// alive in the models that hold it.
#[derive(Debug, Default)]
pub struct ThemeTable {
    themes: Vec<Arc<Theme>>,
}

/// Distinct themes a deck keeps for sharing (a deck has a theme per master, times the clrMap overrides).
pub const MAX_SHARED_THEMES: usize = 32;

impl ThemeTable {
    /// The held copy of an equal theme, else `theme` itself, held (the oldest dropped past the cap).
    pub fn share(&mut self, theme: Theme) -> Arc<Theme> {
        if let Some(t) = self.themes.iter().find(|t| ***t == theme) {
            return Arc::clone(t);
        }
        if self.themes.len() >= MAX_SHARED_THEMES {
            self.themes.remove(0);
        }
        let t = Arc::new(theme);
        self.themes.push(Arc::clone(&t));
        t
    }

    /// Themes held.
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }
}

/// The package as the shape reader sees it: the part store with the deck's caches, recording every
/// part a read touches (the [`ReadSet`] of a slide).
pub struct DeckSource<'a> {
    parts: &'a PartStore,
    xml: &'a RefCell<XmlCache>,
    media: &'a RefCell<MediaTable>,
    themes: &'a RefCell<ThemeTable>,
    touched: RefCell<Vec<Box<str>>>,
}

impl<'a> DeckSource<'a> {
    /// The parts touched so far, with the generation each had (or `Absent`), deduplicated in order.
    pub fn read_set(&self) -> ReadSet {
        let mut seen: Vec<(Box<str>, ReadGen)> = Vec::new();
        for n in self.touched.borrow().iter() {
            if seen.iter().any(|(s, _)| s == n) {
                continue;
            }
            let g = match self.parts.get(n) {
                Some(e) => ReadGen::Present(e.generation),
                None => ReadGen::Absent,
            };
            seen.push((n.clone(), g));
        }
        seen.shrink_to_fit();
        ReadSet(seen)
    }

    fn note(&self, name: &str) {
        self.touched.borrow_mut().push(name.into());
    }

    /// The part store this source reads.
    pub fn store(&self) -> &PartStore {
        self.parts
    }
}

impl PartSource for DeckSource<'_> {
    fn part(&self, name: &str) -> Option<Arc<[u8]>> {
        self.note(name);
        self.parts.bytes(name).cloned()
    }

    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        self.note(name);
        let Some(entry) = self.parts.get(name) else { return Ok(None) };
        if let Some(d) = self.xml.borrow_mut().get(name, entry.generation) {
            return Ok(Some(d));
        }
        let text = uo_jsnum::text::decode_utf8(&entry.bytes).into_owned();
        let doc = Arc::new(parse_xml(text).map_err(|e| match e {
            ReadError::Xml(m) => ReadError::Xml(format!("{name}: {m}")),
            other => other,
        })?);
        self.xml.borrow_mut().put(name, entry.generation, &doc);
        Ok(Some(doc))
    }

    fn media(&self, part: &str) -> Option<MediaId> {
        self.note(part);
        self.media.borrow_mut().ensure(self.parts, part)
    }

    fn media_entry(&self, id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        self.media.borrow().get(id).map(|m| (m.part.clone(), Arc::clone(&m.bytes)))
    }

    fn share_theme(&self, theme: Theme) -> Arc<Theme> {
        self.themes.borrow_mut().share(theme)
    }
}

/// An open presentation.
#[derive(Debug)]
pub struct Deck {
    parts: PartStore,
    limits: Limits,
    xml: RefCell<XmlCache>,
    media: RefCell<MediaTable>,
    themes: RefCell<ThemeTable>,
    presentation: Option<(Presentation, ReadSet)>,
    /// Slide models by slide index, least recently used first.
    slides: Vec<(u32, Arc<SlideModel>)>,
    /// The PowerPoint-over-TS switches (default = PowerPoint; `SlidesRules::ts_strict()` for TS parity replays).
    rules: uo_dml_render::SlidesRules,
}

impl Deck {
    fn new(parts: PartStore, limits: Limits) -> Deck {
        Deck {
            parts,
            limits,
            xml: RefCell::new(XmlCache::with_cap(limits.max_xml_cache_bytes)),
            media: RefCell::new(MediaTable::default()),
            themes: RefCell::new(ThemeTable::default()),
            presentation: None,
            slides: Vec::new(),
            rules: uo_dml_render::SlidesRules::default(),
        }
    }

    /// `PptxPackage.load`: a ZIP package (a `.ppt` needs the converter of frontier B2).
    pub fn open(bytes: &[u8], limits: Limits) -> Result<Deck, PptxError> {
        Deck::open_with(bytes, limits, &uo_opc::Limits::DEFAULT)
    }

    /// [`Deck::open`] with the container limits given (the fuzz targets and hosts with a smaller budget).
    pub fn open_with(bytes: &[u8], limits: Limits, opc: &uo_opc::Limits) -> Result<Deck, PptxError> {
        let store = crate::package::load(bytes, opc)?;
        Ok(Deck::new(store, limits))
    }

    /// `PptxPackage.fromParts`: a deck from parts (name → bytes), without a ZIP.
    pub fn from_parts(parts: RawParts, limits: Limits) -> Result<Deck, PptxError> {
        let store = PartStore::from_parts(parts);
        if store.get("ppt/presentation.xml").is_none() {
            return Err(PptxError::NoPresentation);
        }
        Ok(Deck::new(store, limits))
    }

    /// A deck over an existing part store.
    pub fn from_store(parts: PartStore, limits: Limits) -> Deck {
        Deck::new(parts, limits)
    }

    /// The part store.
    pub fn parts(&self) -> &PartStore {
        &self.parts
    }

    /// The part store, mutable (the editing session of feature `edit` replaces whole parts through it; the caches
    /// compare part generations, so a changed part makes the slides that read it stale).
    pub fn parts_mut(&mut self) -> &mut PartStore {
        &mut self.parts
    }

    /// The limits the deck was opened with.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The PowerPoint-over-TS switches the reads and emissions of this deck follow.
    pub fn rules(&self) -> uo_dml_render::SlidesRules {
        self.rules
    }

    /// Sets the PowerPoint-over-TS switches; a change empties the caches (models may depend on them).
    pub fn set_rules(&mut self, rules: uo_dml_render::SlidesRules) {
        if self.rules != rules {
            self.rules = rules;
            self.clear_caches();
        }
    }

    /// `setPart`: replaces or appends a part. Slides that read it become stale.
    pub fn set_part(&mut self, name: &str, bytes: Arc<[u8]>) -> PartGen {
        self.parts.set(name, bytes)
    }

    /// `removePart`.
    pub fn remove_part(&mut self, name: &str) -> bool {
        self.parts.remove(name)
    }

    /// A reading view of the package with the deck's caches (for the printers of the parity tests and
    /// the later emission).
    pub fn source(&self) -> DeckSource<'_> {
        DeckSource { parts: &self.parts, xml: &self.xml, media: &self.media, themes: &self.themes, touched: RefCell::new(Vec::new()) }
    }

    fn is_fresh(&self, rs: &ReadSet) -> bool {
        rs.0.iter().all(|(n, g)| match (self.parts.get(n), g) {
            (Some(e), ReadGen::Present(old)) => e.generation == *old,
            (None, ReadGen::Absent) => true,
            _ => false,
        })
    }

    /// `readPresentation`: cached while the parts it read are unchanged.
    pub fn presentation(&mut self) -> Result<&Presentation, PptxError> {
        let stale = self.presentation.as_ref().is_none_or(|(_, rs)| !self.is_fresh(rs));
        if stale {
            let (p, rs) = {
                let src = self.source();
                let p = read::read_presentation(&src)?;
                (p, src.read_set())
            };
            self.presentation = Some((p, rs));
        }
        match &self.presentation {
            Some((p, _)) => Ok(p),
            None => Err(PptxError::NoPresentation),
        }
    }

    /// `readSlide` of slide `index` (0-based): cached (LRU of [`Limits::max_cached_slides`]) and re-read
    /// when a part it read changed.
    pub fn slide(&mut self, index: u32) -> Result<Arc<SlideModel>, PptxError> {
        if let Some(pos) = self.slides.iter().position(|(i, _)| *i == index) {
            let fresh = self.slides.get(pos).is_some_and(|(_, s)| self.is_fresh(&s.read_set));
            let entry = self.slides.remove(pos);
            if fresh {
                let s = Arc::clone(&entry.1);
                self.slides.push(entry);
                return Ok(s);
            }
        }
        let target = {
            let pres = self.presentation()?;
            let r = pres.slides.get(index as usize).ok_or(PptxError::NoSuchSlide(index))?;
            r.target.clone().ok_or(PptxError::NoSuchSlide(index))?
        };
        let model = {
            let src = self.source();
            let mut m = read::read_slide_with(&src, &target, self.limits.dml(), &self.limits)?;
            m.read_set = src.read_set();
            m
        };
        let arc = Arc::new(model);
        self.slides.push((index, Arc::clone(&arc)));
        while self.slides.len() > self.limits.max_cached_slides.max(1) {
            self.slides.remove(0);
        }
        Ok(arc)
    }

    /// `readSlideNotes` of slide `index`.
    pub fn slide_notes(&mut self, index: u32) -> Result<String, PptxError> {
        let target = {
            let pres = self.presentation()?;
            let r = pres.slides.get(index as usize).ok_or(PptxError::NoSuchSlide(index))?;
            r.target.clone().ok_or(PptxError::NoSuchSlide(index))?
        };
        let src = self.source();
        Ok(read::read_slide_notes(&src, &target)?)
    }

    /// `readEmbeddedFonts`: the fonts embedded in the deck.
    pub fn embedded_fonts(&self) -> Vec<EmbeddedFont> {
        fonts::read_embedded_fonts(&self.source()).unwrap_or_default()
    }

    /// `collectDeckTypefaces`: the typefaces left literal in the slides, layouts and masters.
    pub fn deck_typefaces(&self) -> Vec<Box<str>> {
        fonts::collect_deck_typefaces(&self.parts)
    }

    /// `save`: the package as a ZIP (deflate level 6, store order).
    pub fn save(&self) -> Result<Vec<u8>, PptxError> {
        self.parts.save()
    }

    /// The timing model of every slide (`None` for a slide that does not read): the input of the playback
    /// timeline and the slide show (B6). The models are shared, not copied.
    pub fn timing_models(&mut self) -> Vec<Option<Arc<crate::timing::TimingModel>>> {
        let n = self.presentation().map_or(0, |p| p.slides.len());
        (0..n).map(|i| u32::try_from(i).ok().and_then(|i| self.slide(i).ok()).and_then(|m| m.timing.clone())).collect()
    }

    /// Empties every cache (memory gate M1: live bytes return to what the open deck holds).
    pub fn clear_caches(&mut self) {
        self.xml.borrow_mut().clear();
        self.media.borrow_mut().clear();
        self.themes.borrow_mut().themes = Vec::new();
        self.slides = Vec::new();
        self.presentation = None;
    }

    /// Slide models in the cache (for the M5 test).
    pub fn cached_slides(&self) -> usize {
        self.slides.len()
    }

    /// The parse cache (for the M5 test).
    pub fn xml_cache(&self) -> std::cell::Ref<'_, XmlCache> {
        self.xml.borrow()
    }

    /// The shared themes (for the M5 test).
    pub fn theme_table(&self) -> std::cell::Ref<'_, ThemeTable> {
        self.themes.borrow()
    }
}
