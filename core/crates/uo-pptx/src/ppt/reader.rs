//! Reading the PowerPoint 97-2003 binary ([MS-PPT]) into an intermediate model faithful to the
//! records (`ppt/ppt-reader.ts`); `to_pptx` materialises it as PresentationML.
//!
//! Data path: "Current User" (`CurrentUserAtom.offsetToCurrentEdit`) → the newest `UserEditAtom`
//! → … → `offsetLastEdit` (older edits) → the `PersistDirectoryAtom` of each edit (persistId →
//! offset in the stream) → the `DocumentContainer` (`docPersistIdRef`): `DocumentAtom` (sizes),
//! `Environment` (fonts, default styles), `DrawingGroup` (the BStore of the pictures),
//! `SlideListWithText` (masters / slides / notes) → `MainMasterContainer` / `SlideContainer` by
//! persistId.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use uo_drawingml::odraw::{
    Blip, BlipBudget, BlipKind, Bse, Flavor, OdrawLimits, PropTable, decode_blip, read_bstore, read_fopt, rt as ort,
};

use super::error::PptError;
use super::oa::{OaDrawing, Shapes};
use super::pict::fit_pict_frame;
use super::rd::{Rec, RecRead, child, children, read_rec, recs_in, rt, sub, u8_at, u16_at, u32_at, utf16, utf16_units};
use super::text::{
    MasterStyleLevel, TextBlock, TextCF, TextPF, read_cf_exception_atom, read_pf_exception_atom, read_style_text_prop9,
    read_text_master_style, split_text_blocks,
};

/// A font of the FontCollection (`FontEntityAtom`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PptFont {
    pub name: String,
    pub charset: u8,
    pub pitch_family: u8,
}

/// The entry transition (`SSSlideInfoAtom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PptTransition {
    pub effect_type: u8,
    pub effect_direction: u8,
    /// 0 slow, 1 medium, 2 fast
    pub speed: u8,
    /// display time (ms) when it advances by itself
    pub slide_time: i32,
    pub manual_advance: bool,
    pub auto_advance: bool,
    pub hidden: bool,
    pub sound: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideKind {
    Slide,
    MainMaster,
    TitleMaster,
    Notes,
}

/// What to show of the date/number/footer placeholders of the master (`HeadersFootersContainer`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PptHeadersFooters {
    pub has_date: bool,
    pub has_today_date: bool,
    pub has_user_date: bool,
    pub has_slide_number: bool,
    pub has_header: bool,
    pub has_footer: bool,
    pub format_id: i16,
    pub user_date: Option<String>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

/// An external hyperlink (`ExternalHyperlinkContainer`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PptHyperlink {
    pub target: Option<String>,
    pub location: Option<String>,
    pub friendly_name: Option<String>,
}

/// An embedded OLE object (`ExOleEmbedContainer`): its ProgID, its kind and the storage record (`ExOleObjStg`, compressed
/// when its instance is 1).
#[derive(Debug, Clone)]
pub struct PptOle<'a> {
    pub prog_id: Option<String>,
    pub sub_type: u32,
    pub storage: Option<Rec<'a>>,
}

/// The master styles of a master by text type (the instance of the `TextMasterStyleAtom`).
pub type MasterStyles = HashMap<u16, Vec<MasterStyleLevel>>;

/// A slide, main master, title master or notes page, decoded (`PptSlide`).
#[derive(Debug, Clone)]
pub struct PptSlide<'a> {
    pub kind: SlideKind,
    /// notes: the slideId of the slide they belong to (`NotesAtom.slideIdRef`)
    pub slide_id_ref: Option<u32>,
    pub persist_id: u32,
    /// `SlidePersistAtom.slideId` (what `masterIdRef` references)
    pub slide_id: u32,
    /// `SlideAtom.geom` (SlideLayoutType)
    pub geom: u32,
    pub placeholder_types: Vec<u8>,
    pub master_id_ref: u32,
    /// `SlideAtom.slideFlags`: bit 0 master objects, bit 1 master scheme, bit 2 master background
    pub flags: u32,
    /// the colour scheme (8 colours 'RRGGBB')
    pub color_scheme: Option<Vec<String>>,
    pub transition: Option<PptTransition>,
    pub drawing: OaDrawing,
    /// text of the placeholders from the SlideListWithText (`OutlineTextRefAtom`)
    pub outline: Vec<Rc<TextBlock>>,
    pub master_styles: Option<Rc<MasterStyles>>,
    /// binary tags of the shape ProgTags: name → blob
    pub binary_tags: HashMap<String, Rec<'a>>,
    pub name: Option<String>,
    /// `RoundTripTheme12Atom` (zip with the original theme)
    pub round_trip_theme: Option<&'a [u8]>,
    /// `RoundTripAnimationAtom12` (zip with the original `<p:timing>`)
    pub round_trip_timing: Option<&'a [u8]>,
    pub headers_footers: Option<PptHeadersFooters>,
}

type BlipCache<'a> = HashMap<(bool, u64), Option<Rc<Blip<'a>>>>;

/// The BLIP store of the document: decoded on demand (only the pictures shapes use), once per
/// source, with a cap on the total memory (`PptDocument.blip`).
pub struct BlipStore<'a> {
    entries: Vec<Option<Bse<'a>>>,
    pics: Option<&'a [u8]>,
    /// key: (embedded record?, header offset or `foDelay`)
    cache: RefCell<BlipCache<'a>>,
    budget: RefCell<BlipBudget>,
}

/// Cap on the TOTAL bytes inflated from the pictures (zlib metafiles) per document.
const BLIP_BUDGET: u64 = 256 * 1024 * 1024;

/// `decodeBlip` of `ppt/officeart.ts`: odraw's Slides flavor, and the PICT fitted to its frame.
fn decode_slides_blip<'a>(rec: &Rec<'a>, budget: &mut BlipBudget) -> Option<Blip<'a>> {
    let mut b = decode_blip(rec, budget, Flavor::Slides, &OdrawLimits::WORD)?;
    if b.kind == BlipKind::Pict
        && let Some((w, h)) = b.pict_size
    {
        b.bytes = fit_pict_frame(std::mem::take(&mut b.bytes), w, h);
    }
    Some(b)
}

/// `decodeBlip(rec)` without a budget: only the per-picture cap applies.
#[must_use]
pub fn decode_unlimited<'a>(rec: &Rec<'a>) -> Option<Blip<'a>> {
    decode_slides_blip(rec, &mut BlipBudget::unlimited())
}

impl<'a> BlipStore<'a> {
    fn empty() -> BlipStore<'a> {
        BlipStore {
            entries: Vec::new(),
            pics: None,
            cache: RefCell::new(HashMap::new()),
            budget: RefCell::new(BlipBudget { left: BLIP_BUDGET }),
        }
    }

    /// The picture of the BStore entry `pib` (1-based), `None` when there is none.
    #[must_use]
    pub fn blip(&self, pib: u32) -> Option<Rc<Blip<'a>>> {
        let e = self.entries.get(usize::try_from(pib.checked_sub(1)?).ok()?)?.as_ref()?;
        let (rec, key): (Option<Rec<'a>>, (bool, u64)) = if let Some(emb) = e.embedded {
            (Some(emb), (true, emb.hdr))
        } else {
            let pics = self.pics?;
            if e.fo_delay == 0xffff_ffff || u64::from(e.fo_delay) >= pics.len() as u64 {
                return None;
            }
            let key = (false, u64::from(e.fo_delay));
            if let Some(hit) = self.cache.borrow().get(&key) {
                return hit.clone();
            }
            (read_rec(pics, u64::from(e.fo_delay), pics.len() as u64), key)
        };
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let b = rec.and_then(|r| decode_slides_blip(&r, &mut self.budget.borrow_mut())).map(Rc::new);
        self.cache.borrow_mut().insert(key, b.clone());
        b
    }
}

/// The document (`PptDocument`).
pub struct PptDocument<'a> {
    /// slide size in master units (576/inch)
    pub slide_w: i32,
    pub slide_h: i32,
    pub notes_w: i32,
    pub notes_h: i32,
    pub slide_size_type: u16,
    pub first_slide_number: u16,
    /// by `FontEntityAtom.recInstance` (sparse)
    pub fonts: Vec<Option<PptFont>>,
    pub doc_cf: TextCF,
    pub doc_pf: TextPF,
    /// `TextMasterStyleAtom` of the Environment (type "other")
    pub doc_other_style: Option<Vec<MasterStyleLevel>>,
    pub blips: BlipStore<'a>,
    /// the document's DEFAULT properties (FOPT of the DggContainer)
    pub drawing_defaults: PropTable<'a>,
    pub shapes: Shapes<'a>,
    pub masters: Vec<PptSlide<'a>>,
    pub slides: Vec<PptSlide<'a>>,
    /// the notes pages (one per slide that has notes) and the notes master
    pub notes: Vec<PptSlide<'a>>,
    pub notes_master: Option<PptSlide<'a>>,
    /// binary tags of the document ("___PPT9" with OutlineTextProps9)
    pub binary_tags: HashMap<String, Rec<'a>>,
    /// hyperlinks by `exHyperlinkId`
    pub hyperlinks: HashMap<u32, PptHyperlink>,
    /// embedded OLE objects by `exObjId`
    pub ole: HashMap<u32, PptOle<'a>>,
    /// default language of the text (LCID of the `TextSpecialInfoDefaultAtom`)
    pub doc_lcid: Option<u16>,
    pub slide_headers_footers: Option<PptHeadersFooters>,
    /// `DocumentAtom.fOmitTitlePlace`
    pub omit_title_place: bool,
    /// PP9 of the outline text: by `"{slideIdRef}:{textType}"`
    pub outline_pf9: HashMap<String, Rc<Vec<TextPF>>>,
    /// picture bullets (BlipCollection9 of "___PPT9"), by the `bulletBlipRef` of the PP9
    pub bullet_blips: Vec<Option<Rc<Blip<'a>>>>,
}

impl PptDocument<'_> {
    /// `doc.blip(pib)`.
    #[must_use]
    pub fn blip(&self, pib: u32) -> Option<Rc<Blip<'_>>> {
        self.blips.blip(pib)
    }
}

const HEADER_TOKEN_ENCRYPTED: u32 = 0xf3d1_c4df;

fn encrypted() -> PptError {
    PptError::Format("Apresentação protegida por senha (criptografada) — não é possível abri-la.".to_owned())
}

/// Deepest nesting of containers a recursive read follows (a JS engine fails at a depth of some
/// thousands with a `RangeError`, which `convertPptToPptx` reports as a corrupt file).
const MAX_RECURSION: usize = 1000;

fn too_deep() -> PptError {
    PptError::Internal("Maximum call stack size exceeded".to_owned())
}

/// Reads a `.ppt` from its streams: `doc` = "PowerPoint Document", `pics` = "Pictures",
/// `current_user` = "Current User".
pub fn read_ppt_document<'a>(doc: &'a [u8], pics: Option<&'a [u8]>, current_user: Option<&[u8]>) -> Result<PptDocument<'a>, PptError> {
    // CurrentUserAtom → offset of the current edit
    let mut current_edit: Option<u32> = None;
    if let Some(cu) = current_user
        && cu.len() >= 20
        && let Some(rec) = read_rec(cu, 0, cu.len() as u64)
        && rec.rec_type == rt::CURRENT_USER_ATOM
    {
        let token = u32_at(cu, rec.start + 4);
        if token == HEADER_TOKEN_ENCRYPTED {
            return Err(encrypted());
        }
        // an unknown token: the offset is still tried (validated by `is_user_edit`)
        current_edit = Some(u32_at(cu, rec.start + 8));
    }
    let edits = collect_user_edits(doc, current_edit);
    let Some(newest) = edits.first() else {
        return Err(PptError::Format("Estrutura do .ppt não reconhecida (UserEditAtom ausente).".to_owned()));
    };
    if newest.len >= 0x20 && u32_at(doc, newest.start + 28) != 0 {
        return Err(encrypted());
    }
    let persist = build_persist_directory(doc, &edits);
    let doc_persist_id = u32_at(doc, newest.start + 16);
    let doc_rec = rec_at_persist(doc, &persist, doc_persist_id);
    let Some(doc_rec) = doc_rec.filter(|r| r.rec_type == rt::DOCUMENT) else {
        return Err(PptError::Format("DocumentContainer ausente no .ppt.".to_owned()));
    };

    let mut out = PptDocument {
        slide_w: 5760,
        slide_h: 4320,
        notes_w: 4320,
        notes_h: 5760,
        slide_size_type: 0,
        first_slide_number: 1,
        fonts: Vec::new(),
        doc_cf: TextCF::default(),
        doc_pf: TextPF::default(),
        doc_other_style: None,
        blips: BlipStore::empty(),
        drawing_defaults: PropTable::new(),
        shapes: Shapes::default(),
        masters: Vec::new(),
        slides: Vec::new(),
        notes: Vec::new(),
        notes_master: None,
        binary_tags: HashMap::new(),
        hyperlinks: HashMap::new(),
        ole: HashMap::new(),
        doc_lcid: None,
        slide_headers_footers: None,
        omit_title_place: false,
        outline_pf9: HashMap::new(),
        bullet_blips: Vec::new(),
    };
    let mut notes_master_persist = 0u32;

    struct SlideRef<'t> {
        persist_id: u32,
        slide_id: u32,
        texts: Vec<Rc<TextBlock>>,
        _p: std::marker::PhantomData<&'t ()>,
    }
    let mut master_refs: Vec<SlideRef<'_>> = Vec::new();
    let mut slide_refs: Vec<SlideRef<'_>> = Vec::new();
    let mut notes_refs: Vec<SlideRef<'_>> = Vec::new();
    let mut seen_persist: HashSet<(u16, u32)> = HashSet::new();

    for c in children(&doc_rec) {
        match c.rec_type {
            rt::DOCUMENT_ATOM => {
                let o = c.start;
                out.slide_w = c.i32(o);
                out.slide_h = c.i32(o + 4);
                out.notes_w = c.i32(o + 8);
                out.notes_h = c.i32(o + 12);
                notes_master_persist = c.u32(o + 24);
                out.first_slide_number = c.u16(o + 32);
                out.slide_size_type = c.u16(o + 34);
                out.omit_title_place = c.u8(o + 37) != 0;
            }
            rt::HEADERS_FOOTERS => {
                if c.inst == 3 {
                    out.slide_headers_footers = Some(read_headers_footers(&c));
                }
            }
            rt::ENVIRONMENT => read_environment(&c, &mut out),
            rt::DRAWING_GROUP => {
                let dgg = child(&c, ort::DGG_CONTAINER);
                if let Some(dopt) = dgg.and_then(|d| child(&d, ort::FOPT)) {
                    read_fopt(&dopt, &mut out.drawing_defaults, Flavor::Slides);
                }
                let bstore = dgg.and_then(|d| child(&d, ort::BSTORE_CONTAINER));
                if let Some(bs) = bstore {
                    out.blips = BlipStore {
                        entries: read_bstore(&bs),
                        pics,
                        cache: RefCell::new(HashMap::new()),
                        budget: RefCell::new(BlipBudget { left: BLIP_BUDGET }),
                    };
                }
            }
            rt::SLIDE_LIST_WITH_TEXT => {
                // children: SlidePersistAtom followed by the text blocks of that slide
                // (list index, position in the list) of the current slide ref, when it was kept
                let mut cur: Option<(u8, usize)> = None;
                let mut pending: Vec<Rec<'_>> = Vec::new();
                let flush = |cur: Option<(u8, usize)>, pending: &mut Vec<Rec<'_>>, lists: [&mut Vec<SlideRef<'_>>; 3]| {
                    if let Some((which, idx)) = cur {
                        let blocks = split_text_blocks(pending);
                        let [m, s, n] = lists;
                        let list = match which {
                            0 => m,
                            1 => s,
                            _ => n,
                        };
                        if let Some(r) = list.get_mut(idx) {
                            r.texts.extend(blocks.into_iter().map(Rc::new));
                        }
                    }
                    pending.clear();
                };
                for k in children(&c) {
                    if k.rec_type == rt::SLIDE_PERSIST_ATOM {
                        flush(cur, &mut pending, [&mut master_refs, &mut slide_refs, &mut notes_refs]);
                        let persist_id = k.u32(k.start);
                        let slide_id = k.u32(k.start + 12);
                        // the same persistId twice = the same slide: converted ONCE
                        let which: Option<u8> = match c.inst {
                            1 => Some(0),
                            0 => Some(1),
                            2 => Some(2),
                            _ => None,
                        };
                        cur = None;
                        if let Some(w) = which
                            && seen_persist.insert((c.inst, persist_id))
                        {
                            let list = match w {
                                0 => &mut master_refs,
                                1 => &mut slide_refs,
                                _ => &mut notes_refs,
                            };
                            list.push(SlideRef { persist_id, slide_id, texts: Vec::new(), _p: std::marker::PhantomData });
                            cur = Some((w, list.len() - 1));
                        }
                    } else {
                        pending.push(k);
                    }
                }
                flush(cur, &mut pending, [&mut master_refs, &mut slide_refs, &mut notes_refs]);
            }
            rt::LIST => read_prog_tags(&c, &mut out.binary_tags, 0)?,
            rt::EXTERNAL_OBJECT_LIST => {
                for x in children(&c) {
                    // ExOleEmbedContainer: ExOleObjAtom (drawAspect, type, exObjId, subType, persistIdRef) + the ProgID
                    if x.rec_type == rt::EX_OLE_EMBED {
                        let mut id: Option<u32> = None;
                        let mut ole = PptOle { prog_id: None, sub_type: 0, storage: None };
                        for k in children(&x) {
                            if k.rec_type == rt::EX_OLE_OBJ_ATOM {
                                id = Some(k.u32(k.start + 8));
                                ole.sub_type = k.u32(k.start + 12);
                                ole.storage =
                                    rec_at_persist(doc, &persist, k.u32(k.start + 16)).filter(|r| r.rec_type == rt::EX_OLE_OBJ_STG);
                            } else if k.rec_type == rt::CSTRING && k.inst == 2 {
                                ole.prog_id = Some(utf16(k.data, k.start, k.end));
                            }
                        }
                        if let Some(id) = id {
                            out.ole.insert(id, ole);
                        }
                        continue;
                    }
                    if x.rec_type != rt::EXTERNAL_HYPERLINK {
                        continue;
                    }
                    let mut id: Option<u32> = None;
                    let mut link = PptHyperlink::default();
                    for k in children(&x) {
                        if k.rec_type == rt::EXTERNAL_HYPERLINK_ATOM {
                            id = Some(k.u32(k.start));
                        } else if k.rec_type == rt::CSTRING {
                            let s = utf16(k.data, k.start, k.end);
                            match k.inst {
                                0 => link.friendly_name = Some(s),
                                1 => link.target = Some(s),
                                3 => link.location = Some(s),
                                _ => {}
                            }
                        }
                    }
                    if let Some(id) = id {
                        out.hyperlinks.insert(id, link);
                    }
                }
            }
            _ => {}
        }
    }

    // OutlineTextProps9Container (0x0FAE): pairs OutlineTextPropsHeaderExAtom (0x0FAF: slideIdRef,
    // txType) + StyleTextProp9Atom (numbering of the outline)
    if let Some(pp9) = out.binary_tags.get("___PPT9").copied() {
        for c in recs_in(pp9.data, pp9.start, pp9.end) {
            // BlipCollection9Container (0x07F8) → BlipEntity9Atom (0x07F9): type (1) + unused (1) + the blip record
            if c.rec_type == 0x07f8 {
                for e in children(&c) {
                    if e.rec_type != 0x07f9 {
                        continue;
                    }
                    let inner = recs_in(e.data, e.start + 2, e.end).next();
                    let b = inner.and_then(|r| decode_slides_blip(&r, &mut BlipBudget::unlimited())).map(Rc::new);
                    out.bullet_blips.push(b);
                }
                continue;
            }
            if c.rec_type != 0x0fae {
                continue;
            }
            let mut key: Option<String> = None;
            for k in children(&c) {
                if k.rec_type == 0x0faf {
                    key = Some(format!("{}:{}", k.u32(k.start), k.u32(k.start + 4)));
                } else if k.rec_type == rt::STYLE_TEXT_PROP9_ATOM
                    && let Some(kk) = key.take()
                {
                    out.outline_pf9.insert(kk, Rc::new(read_style_text_prop9(&k)));
                }
            }
        }
    }

    for m in &master_refs {
        let Some(rec) = rec_at_persist(doc, &persist, m.persist_id) else { continue };
        let kind = if rec.rec_type == rt::MAIN_MASTER {
            SlideKind::MainMaster
        } else if rec.rec_type == rt::SLIDE {
            SlideKind::TitleMaster
        } else {
            continue;
        };
        let s = read_slide_container(&mut out.shapes, &rec, kind, m.persist_id, m.slide_id, Vec::new())?;
        out.masters.push(s);
    }
    for s in &slide_refs {
        let Some(rec) = rec_at_persist(doc, &persist, s.persist_id).filter(|r| r.rec_type == rt::SLIDE) else { continue };
        let sl = read_slide_container(&mut out.shapes, &rec, SlideKind::Slide, s.persist_id, s.slide_id, s.texts.clone())?;
        out.slides.push(sl);
    }
    for n in &notes_refs {
        let Some(rec) = rec_at_persist(doc, &persist, n.persist_id).filter(|r| r.rec_type == rt::NOTES) else { continue };
        let nt = read_slide_container(&mut out.shapes, &rec, SlideKind::Notes, n.persist_id, n.slide_id, n.texts.clone())?;
        out.notes.push(nt);
    }
    if notes_master_persist != 0
        && let Some(rec) = rec_at_persist(doc, &persist, notes_master_persist).filter(|r| r.rec_type == rt::NOTES)
    {
        out.notes_master = Some(read_slide_container(&mut out.shapes, &rec, SlideKind::Notes, notes_master_persist, 0, Vec::new())?);
    }
    let mut drawings: Vec<OaDrawing> = Vec::new();
    drawings.extend(out.masters.iter().map(|s| s.drawing));
    drawings.extend(out.slides.iter().map(|s| s.drawing));
    drawings.extend(out.notes.iter().map(|s| s.drawing));
    drawings.extend(out.notes_master.iter().map(|s| s.drawing));
    out.shapes.resolve_shape_masters(&drawings, OdrawLimits::WORD.max_master_props.into());
    Ok(out)
}

/// The chain of UserEditAtoms from the newest to the oldest.
fn collect_user_edits<'a>(doc: &'a [u8], current_edit: Option<u32>) -> Vec<Rec<'a>> {
    let mut out = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut off: Option<u64> = current_edit.map(u64::from);
    // without a valid "Current User": the last top-level UserEditAtom
    if off.is_none_or(|o| !is_user_edit(doc, o)) {
        off = last_top_level_user_edit(doc);
    }
    while let Some(o) = off {
        if o == 0 || seen.contains(&o) || !is_user_edit(doc, o) {
            break;
        }
        seen.insert(o);
        let Some(ue) = read_rec(doc, o, doc.len() as u64) else { break };
        out.push(ue);
        off = Some(u64::from(u32_at(doc, ue.start + 8)));
    }
    out
}

fn is_user_edit(doc: &[u8], off: u64) -> bool {
    read_rec(doc, off, doc.len() as u64).is_some_and(|r| r.rec_type == rt::USER_EDIT_ATOM && r.len >= 28)
}

fn last_top_level_user_edit(doc: &[u8]) -> Option<u64> {
    // scans the headers without materialising a record each (a large zeroed stream = millions of
    // empty records of 8 bytes)
    let len = doc.len() as u64;
    let mut last = None;
    let mut off = 0u64;
    while off + 8 <= len {
        if u16_at(doc, off + 2) == rt::USER_EDIT_ATOM {
            last = Some(off);
        }
        let next = off + 8 + u64::from(u32_at(doc, off + 4));
        if next > len {
            break;
        }
        off = next;
    }
    last
}

/// persistId → offset, newer edits over older ones.
fn build_persist_directory(doc: &[u8], edits: &[Rec<'_>]) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    // different edits pointing at the SAME directory: read once (N edits x M entries was N·M
    // insertions — hours in a file of a few MB)
    let mut seen: HashSet<u32> = HashSet::new();
    for ue in edits.iter().rev() {
        let pd_off = u32_at(doc, ue.start + 12);
        if !seen.insert(pd_off) {
            continue;
        }
        let Some(pd) = read_rec(doc, u64::from(pd_off), doc.len() as u64) else { continue };
        if pd.rec_type != rt::PERSIST_DIRECTORY_ATOM {
            continue;
        }
        let mut o = pd.start;
        while o + 4 <= pd.end {
            let v = u32_at(doc, o);
            let id = v & 0xf_ffff;
            let n = v >> 20;
            let mut k = 0u32;
            while k < n && o + 8 + u64::from(k) * 4 <= pd.end {
                map.insert(id + k, u32_at(doc, o + 4 + u64::from(k) * 4));
                k += 1;
            }
            o += 4 + u64::from(n) * 4;
        }
    }
    map
}

fn rec_at_persist<'a>(doc: &'a [u8], persist: &HashMap<u32, u32>, id: u32) -> Option<Rec<'a>> {
    let off = *persist.get(&id)?;
    read_rec(doc, u64::from(off), doc.len() as u64)
}

/// Environment: fonts + the text defaults of the document.
fn read_environment(env: &Rec<'_>, out: &mut PptDocument<'_>) {
    for c in children(env) {
        match c.rec_type {
            rt::FONT_COLLECTION => {
                for f in children(&c) {
                    if f.rec_type != rt::FONT_ENTITY_ATOM {
                        continue;
                    }
                    let mut units = utf16_units(f.data, f.start, f.start + 64);
                    if let Some(z) = units.iter().position(|&u| u == 0) {
                        units.truncate(z);
                    }
                    let name = String::from_utf16_lossy(&units);
                    let idx = usize::from(f.inst);
                    if out.fonts.len() <= idx {
                        out.fonts.resize(idx + 1, None);
                    }
                    if let Some(slot) = out.fonts.get_mut(idx) {
                        *slot = Some(PptFont { name, charset: f.u8(f.start + 64), pitch_family: f.u8(f.start + 67) });
                    }
                }
            }
            rt::TEXT_SPECIAL_INFO_DEFAULT_ATOM => {
                // TextSIException: mask + spellInfo (bit 0) + lid (bit 1)
                let masks = c.u32(c.start);
                if masks & 0x2 != 0 {
                    out.doc_lcid = Some(c.u16(c.start + 4 + if masks & 0x1 != 0 { 2 } else { 0 }));
                }
            }
            rt::TEXT_CHAR_FORMAT_EXCEPTION_ATOM => out.doc_cf = read_cf_exception_atom(&c),
            rt::TEXT_PARAGRAPH_FORMAT_EXCEPTION_ATOM => out.doc_pf = read_pf_exception_atom(&c),
            rt::TEXT_MASTER_STYLE_ATOM => out.doc_other_style = Some(read_text_master_style(&c)),
            _ => {}
        }
    }
}

/// ProgTags → name → BinaryTagDataBlob (recursive into List/ProgTags).
fn read_prog_tags<'a>(rec: &Rec<'a>, into: &mut HashMap<String, Rec<'a>>, depth: usize) -> Result<(), PptError> {
    if depth > MAX_RECURSION {
        return Err(too_deep());
    }
    for c in children(rec) {
        if c.rec_type == rt::PROG_TAGS || c.rec_type == rt::LIST {
            read_prog_tags(&c, into, depth + 1)?;
        } else if c.rec_type == rt::PROG_BINARY_TAG {
            let kids = children(&c);
            let name_rec = kids.iter().find(|k| k.rec_type == rt::CSTRING);
            let blob = kids.iter().find(|k| k.rec_type == rt::BINARY_TAG_DATA_BLOB);
            if let (Some(n), Some(b)) = (name_rec, blob) {
                into.insert(utf16(n.data, n.start, n.end), *b);
            }
        }
    }
    Ok(())
}

/// `HeadersFootersContainer`: `HeadersFootersAtom` (formatId i16, flags u16: date, today's date,
/// fixed date, slide number, header, footer) + CStrings (instance 0 = fixed date, 1 = header,
/// 2 = footer).
fn read_headers_footers(rec: &Rec<'_>) -> PptHeadersFooters {
    let mut hf = PptHeadersFooters::default();
    for k in children(rec) {
        if k.rec_type == rt::HEADERS_FOOTERS_ATOM {
            hf.format_id = k.i16(k.start);
            let f = k.u16(k.start + 2);
            hf.has_date = f & 0x1 != 0;
            hf.has_today_date = f & 0x2 != 0;
            hf.has_user_date = f & 0x4 != 0;
            hf.has_slide_number = f & 0x8 != 0;
            hf.has_header = f & 0x10 != 0;
            hf.has_footer = f & 0x20 != 0;
        } else if k.rec_type == rt::CSTRING {
            let s = utf16(k.data, k.start, k.end);
            match k.inst {
                0 => hf.user_date = Some(s),
                1 => hf.header = Some(s),
                2 => hf.footer = Some(s),
                _ => {}
            }
        }
    }
    hf
}

/// `ColorSchemeAtom` → 8 colours 'RRGGBB'.
fn read_color_scheme(rec: &Rec<'_>) -> Vec<String> {
    (0..8u64)
        .map(|i| {
            let o = rec.start + i * 4;
            format!("{:02X}{:02X}{:02X}", u8_at(rec.data, o), u8_at(rec.data, o + 1), u8_at(rec.data, o + 2))
        })
        .collect()
}

/// `SlideContainer` / `MainMasterContainer` → [`PptSlide`].
fn read_slide_container<'a>(
    shapes: &mut Shapes<'a>,
    rec: &Rec<'a>,
    kind: SlideKind,
    persist_id: u32,
    slide_id: u32,
    outline: Vec<Rc<TextBlock>>,
) -> Result<PptSlide<'a>, PptError> {
    let mut s = PptSlide {
        kind,
        slide_id_ref: None,
        persist_id,
        slide_id,
        geom: 0,
        placeholder_types: Vec::new(),
        master_id_ref: 0,
        flags: 0,
        color_scheme: None,
        transition: None,
        drawing: OaDrawing::default(),
        outline,
        master_styles: None,
        binary_tags: HashMap::new(),
        name: None,
        round_trip_theme: None,
        round_trip_timing: None,
        headers_footers: None,
    };
    let mut styles: Option<MasterStyles> = None;
    for c in children(rec) {
        match c.rec_type {
            rt::SLIDE_ATOM => {
                let o = c.start;
                s.geom = c.u32(o);
                for i in 0..8u64 {
                    s.placeholder_types.push(c.u8(o + 4 + i));
                }
                s.master_id_ref = c.u32(o + 12);
                s.flags = u32::from(c.u16(o + 20));
            }
            rt::NOTES_ATOM => {
                s.slide_id_ref = Some(c.u32(c.start));
                s.flags = u32::from(c.u16(c.start + 4));
            }
            rt::SLIDE_SHOW_SLIDE_INFO_ATOM => {
                let o = c.start;
                let flags = c.u16(o + 10);
                s.transition = Some(PptTransition {
                    slide_time: c.i32(o),
                    effect_direction: c.u8(o + 8),
                    effect_type: c.u8(o + 9),
                    speed: c.u8(o + 12),
                    manual_advance: flags & 0x1 != 0,
                    hidden: flags & 0x4 != 0,
                    sound: flags & 0x10 != 0,
                    auto_advance: flags & 0x400 != 0,
                });
            }
            rt::DRAWING => {
                if let Some(dg) = child(&c, ort::DG_CONTAINER) {
                    s.drawing = shapes.read_dg_container(&dg);
                }
            }
            rt::COLOR_SCHEME_ATOM => {
                // instance 1 = the slide's / master's scheme; 6 = list of default schemes
                if c.inst == 1 {
                    s.color_scheme = Some(read_color_scheme(&c));
                }
            }
            rt::TEXT_MASTER_STYLE_ATOM => {
                styles.get_or_insert_with(HashMap::new).insert(c.inst, read_text_master_style(&c));
            }
            rt::PROG_TAGS => read_prog_tags(&c, &mut s.binary_tags, 0)?,
            rt::CSTRING => {
                if c.inst == 2 || c.inst == 3 {
                    s.name = Some(utf16(c.data, c.start, c.end));
                }
            }
            rt::ROUND_TRIP_THEME12_ATOM => s.round_trip_theme = Some(sub(c.data, c.start, c.end)),
            rt::ROUND_TRIP_ANIMATION_ATOM12_ATOM => s.round_trip_timing = Some(sub(c.data, c.start, c.end)),
            rt::HEADERS_FOOTERS => s.headers_footers = Some(read_headers_footers(&c)),
            _ => {}
        }
    }
    s.master_styles = styles.map(Rc::new);
    Ok(s)
}
