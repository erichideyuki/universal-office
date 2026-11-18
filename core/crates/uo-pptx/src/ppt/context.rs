//! Shared context of the `.ppt` → `.pptx` conversion: options, the relations (`.rels`) of a part,
//! the media of the package and the state of the part (slide/master) being written
//! (`ppt/context.ts`).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use super::reader::{MasterStyles, PptDocument};
use super::roundtrip::RoundTrip;
use super::text::TextBlock;
use super::textconv::{TextConvCtx, TextHost};
use super::xmlw::{XML_DECL, el, ns, s};

/// Options of the conversion (`ConvertOptions`).
#[derive(Debug, Clone, Copy)]
pub struct ConvertOptions {
    /// Use the round-trip data of PowerPoint 2007+ when present (the original theme and the
    /// appearance of the shapes from the metroBlob) — which is what PowerPoint itself does when it
    /// opens the .ppt. `false` = only the 97-2003 records. Default: true.
    pub use_round_trip: bool,
    /// Today's date (year, month 1..12, day) for the automatic date field. The TS reads the
    /// clock (`new Date()`, local time); a wasm module has none, so the host passes it. `None`
    /// takes the UTC date of the system clock where there is one.
    pub today: Option<(i32, u32, u32)>,
    /// Charts (embedded `Excel.Chart` OLE objects) as the TS converts them: the fallback picture the `.ppt` keeps.
    /// PowerPoint (measured, Slides wave 5: `Shape.Type` = msoChart, `HasChart`) opens them as native charts from the
    /// BIFF8 chart substream of the embedded workbook (`uo_charts::biff`). Default: false (PowerPoint).
    pub ts_ppt_charts: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions { use_round_trip: true, today: None, ts_ppt_charts: false }
    }
}

impl ConvertOptions {
    /// The frozen TS converter (the B2 byte-parity replays and every TS-parity harness that converts a `.ppt`).
    #[must_use]
    pub fn ts_strict() -> ConvertOptions {
        ConvertOptions { ts_ppt_charts: true, ..ConvertOptions::default() }
    }
}

/// A map with the iteration order of a JS `Map`: new keys are appended, a key set again keeps its
/// position.
#[derive(Debug, Clone)]
pub struct OMap<K: Eq + Hash + Clone, V> {
    items: Vec<(K, V)>,
    ix: HashMap<K, usize>,
}

impl<K: Eq + Hash + Clone, V> Default for OMap<K, V> {
    fn default() -> Self {
        OMap { items: Vec::new(), ix: HashMap::new() }
    }
}

impl<K: Eq + Hash + Clone, V> OMap<K, V> {
    #[must_use]
    pub fn new() -> Self {
        OMap::default()
    }

    /// `map.get(key)`.
    pub fn get(&self, k: &K) -> Option<&V> {
        self.ix.get(k).and_then(|&i| self.items.get(i)).map(|(_, v)| v)
    }

    /// `map.has(key)`.
    pub fn has(&self, k: &K) -> bool {
        self.ix.contains_key(k)
    }

    /// `map.set(key, value)`.
    pub fn set(&mut self, k: K, v: V) {
        if let Some(slot) = self.ix.get(&k).and_then(|&i| self.items.get_mut(i)) {
            slot.1 = v;
            return;
        }
        self.ix.insert(k.clone(), self.items.len());
        self.items.push((k, v));
    }

    /// `map.size`.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The entries in `Map` order.
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.items.iter()
    }

    /// Takes the entries out in order.
    pub fn into_vec(self) -> Vec<(K, V)> {
        self.items
    }
}

/// One relationship.
#[derive(Debug, Clone)]
struct Rel {
    id: String,
    type_: String,
    target: String,
    external: bool,
}

/// Relations of a part (generates the `.rels`); short types use the officeDocument namespace.
#[derive(Debug, Default)]
pub struct PartRels {
    list: Vec<Rel>,
    index: HashMap<(String, String, bool), usize>,
}

impl PartRels {
    #[must_use]
    pub fn new() -> Self {
        PartRels::default()
    }

    /// `rels.add(type, target, external)`: the rId of the same relation, or of a new one.
    pub fn add(&mut self, type_: &str, target: &str, external: bool) -> String {
        let key = (type_.to_owned(), target.to_owned(), external);
        if let Some(&i) = self.index.get(&key)
            && let Some(r) = self.list.get(i)
        {
            return r.id.clone();
        }
        let id = format!("rId{}", self.list.len() + 1);
        self.index.insert(key, self.list.len());
        self.list.push(Rel { id: id.clone(), type_: type_.to_owned(), target: target.to_owned(), external });
        id
    }

    /// The `.rels` XML.
    #[must_use]
    pub fn xml(&self) -> String {
        let items: String = self
            .list
            .iter()
            .map(|r| {
                el(
                    "Relationship",
                    vec![
                        ("Id", s(r.id.clone())),
                        ("Type", s(if r.type_.starts_with("http") { r.type_.clone() } else { format!("{}/{}", ns::REL_TYPE, r.type_) })),
                        ("Target", s(r.target.clone())),
                        ("TargetMode", if r.external { s("External") } else { None }),
                    ],
                    "",
                )
            })
            .collect();
        format!("{XML_DECL}{}", el("Relationships", vec![("xmlns", s(ns::REL))], &items))
    }
}

/// State of the whole conversion (document, media, options).
pub struct ConvCtx<'a> {
    pub doc: &'a PptDocument<'a>,
    /// pib → path of the media (`ppt/media/imageN.ext`); a picture bullet has its own key `-1 - ref`
    pub media: OMap<i64, String>,
    /// binary parts generated (media), in order
    pub parts: OMap<String, Vec<u8>>,
    pub media_exts: OMap<String, ()>,
    pub use_round_trip: bool,
    /// content types of extra XML parts (round-trip diagrams), by path
    pub overrides: OMap<String, String>,
    /// number of diagrams (SmartArt) already emitted — numbering of the parts
    pub diagrams: u32,
    /// number of native charts already emitted (`ppt/charts/chartN.xml`)
    pub charts: u32,
    /// `ConvertOptions::ts_ppt_charts`
    pub ts_ppt_charts: bool,
    /// Budget of converted TEXT characters (all the shapes, repeated ones included): a large
    /// block referenced by thousands of shapes (OutlineTextRefAtom) multiplied the output without limit.
    pub text_budget: f64,
    /// images inside the complex data of a property: key → path (`complexMedia`)
    pub complex_media: OMap<String, String>,
    /// recoloured metafiles: key → path (`recolorMedia`)
    pub recolor_media: OMap<String, String>,
    pub rt: RoundTrip,
    pub today: (i32, u32, u32),
    /// the counter of `a:fld` GUIDs (`fieldSeq`): per conversion, starting at 1
    pub field_seq: Rc<Cell<u32>>,
}

/// Cap of converted text per document (characters) — far above any real deck.
pub const TEXT_BUDGET: f64 = 64.0 * 1024.0 * 1024.0;

/// The default colour scheme of PowerPoint 97 (when the .ppt carries none).
#[must_use]
pub fn default_scheme() -> Vec<String> {
    ["FFFFFF", "000000", "808080", "000000", "00CC99", "3333CC", "CCCCFF", "B2B2B2"].iter().map(|x| (*x).to_owned()).collect()
}

/// `DEFAULT_SCHEME[i]`.
#[must_use]
pub fn default_scheme_at(i: usize) -> Option<&'static str> {
    ["FFFFFF", "000000", "808080", "000000", "00CC99", "3333CC", "CCCCFF", "B2B2B2"].get(i).copied()
}

/// Context of a part (slide/master) being written.
pub struct PartCtx<'c, 'a> {
    pub conv: &'c mut ConvCtx<'a>,
    pub rels: PartRels,
    pub text: TextConvCtx<'a>,
    /// the effective colour scheme (8 colours 'RRGGBB')
    pub scheme: Vec<String>,
    /// the effective background colour (for fillType "background")
    pub bg_color: Option<String>,
    /// MASTER part (master/title shapes): the document's default fill applies
    pub is_master: bool,
    /// blocks of the SlideListWithText (OutlineTextRefAtom)
    pub outline: Vec<Rc<TextBlock>>,
    /// the slideId of the slide (key of the outline's PP9 in the document)
    pub slide_id: Option<u32>,
    /// shape ids already used in the part (avoids duplicates)
    pub used_ids: HashSet<i64>,
    /// spids actually emitted (valid targets of the animations)
    pub spids: HashSet<u32>,
    /// text of each shape by spid (character ranges → paragraphs in the animations)
    pub texts: HashMap<u32, Rc<Vec<u16>>>,
    /// ORIGINAL OOXML id (cNvPr of the round-trip shapexml, bits of the number) → emitted id
    pub ooxml_ids: HashMap<u64, f64>,
    /// Origin (group space) of the tile of the cells of a .ppt TABLE: the application converts
    /// the table and the cells share the tile from its origin
    pub tile_origin: Option<(f64, f64)>,
}

/// What a part lends to the text conversion: its relations and the media of the package.
pub struct PartHost<'x, 'a> {
    pub conv: &'x mut ConvCtx<'a>,
    pub rels: &'x mut PartRels,
}

impl TextHost for PartHost<'_, '_> {
    fn link_rel(&mut self, target: &str) -> String {
        self.rels.add("hyperlink", target, true)
    }

    fn bullet_blip_rel(&mut self, bullet_ref: i32) -> Option<String> {
        let key = -1 - i64::from(bullet_ref);
        let path = match self.conv.media.get(&key).filter(|p| !p.is_empty()) {
            Some(p) => p.clone(),
            None => {
                let b = self.conv.doc.bullet_blips.get(usize::try_from(bullet_ref).ok()?)?.as_ref()?;
                let ext = b.kind.ext();
                let path = format!("ppt/media/image{}.{ext}", self.conv.media.len() + 1);
                self.conv.media.set(key, path.clone());
                self.conv.parts.set(path.clone(), b.bytes.to_vec());
                self.conv.media_exts.set(ext.to_owned(), ());
                path
            }
        };
        Some(self.rels.add("image", &format!("../media/{}", path.strip_prefix("ppt/media/").unwrap_or(&path)), false))
    }
}

impl<'c, 'a> PartCtx<'c, 'a> {
    /// A new part context (`newPartCtx`); the hyperlinks of the text enter the part's relations.
    pub fn new(
        conv: &'c mut ConvCtx<'a>,
        scheme: Vec<String>,
        styles: Option<Rc<MasterStyles>>,
        outline: Vec<Rc<TextBlock>>,
        slide_number: Option<f64>,
    ) -> Self {
        let mut text = TextConvCtx::new(conv.doc, scheme.clone(), conv.today, Rc::clone(&conv.field_seq));
        text.styles = styles;
        text.slide_number = slide_number;
        let mut used = HashSet::new();
        used.insert(1);
        PartCtx {
            conv,
            rels: PartRels::new(),
            text,
            scheme,
            bg_color: None,
            is_master: false,
            outline,
            slide_id: None,
            used_ids: used,
            spids: HashSet::new(),
            texts: HashMap::new(),
            ooxml_ids: HashMap::new(),
            tile_origin: None,
        }
    }
}
