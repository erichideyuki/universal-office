//! `Face`: one parsed font file, the Rust form of the TS `SfntFont` (and of the `LayoutFont`
//! contract). Cheap to clone (`Arc`), `Send + Sync`; the parts that cost something (kerning,
//! ligatures, VDMX, COLR, outlines) are read on first use and kept with the face.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::cmap::{Cmap, CmapScoring};
use crate::colr::{self, ColorLayer, ColrInfo};
use crate::error::{FontError, Result};
use crate::gpos::Kerning;
use crate::gsub::{GsubLigatures, LigGroup};
use crate::limits::FontLimits;
use crate::outline::{self, OutlineCache, OutlineCmd};
use crate::read::{Tag, View};
use crate::sfnt::{TableDir, TableRecord};
use crate::vdmx::{self, VdmxEntry, VdmxTable};

/// The metrics `SfntFont` reads eagerly (hhea, OS/2 with its optional fields, post). Optional
/// fields exist by the OS/2 length and version exactly as in the TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub us_win_ascent: u16,
    pub us_win_descent: u16,
    pub s_typo_ascender: i16,
    pub s_typo_descender: i16,
    pub s_typo_line_gap: i16,
    pub fs_selection: u16,
    /// OS/2 length >= 86 and version >= 1
    pub code_page_range1: Option<u32>,
    /// OS/2 length >= 26
    pub y_superscript_y_offset: Option<i16>,
    /// OS/2 length >= 4
    pub x_avg_char_width: Option<i16>,
    /// OS/2 length >= 42
    pub panose: Option<[u8; 10]>,
    /// 0 unless the post table is at least 12 bytes
    pub underline_position: i16,
    pub underline_thickness: i16,
}

/// A parsed font. Clones share the same data.
#[derive(Clone)]
pub struct Face(Arc<FaceData>);

struct FaceData {
    bytes: Arc<[u8]>,
    dir: TableDir,
    metrics: FontMetrics,
    num_glyphs: u16,
    number_of_h_metrics: u16,
    hmtx_offset: u32,
    cmap: Cmap,
    kerning: OnceLock<Kerning>,
    gsub: OnceLock<Option<GsubLigatures>>,
    vdmx: OnceLock<Option<VdmxTable>>,
    colr: OnceLock<Option<ColrInfo>>,
    outlines: Mutex<OutlineCache>,
    limits: FontLimits,
}

/// `advanceWidth` of `SfntFont`: `i = min(glyphId, numberOfHMetrics - 1)` in **signed** arithmetic
/// (with no metrics the index is -1 and the read lands 4 bytes before `hmtx`, in the table before
/// it), then the `u16` at `hmtx + i * 4`. Only a read outside the input falls back, to 0 (G4).
/// Shared with the converter's program.
pub(crate) fn advance_width_at(view: View<'_>, hmtx_offset: u32, number_of_h_metrics: u16, glyph_id: u32) -> u16 {
    let i = i64::from(glyph_id).min(i64::from(number_of_h_metrics) - 1);
    View::signed_offset(i64::from(hmtx_offset) + i * 4).and_then(|o| view.u16(o)).unwrap_or(0)
}

impl Face {
    /// Parses the font in `bytes` (the bytes are shared, never copied). Fails like the TS
    /// constructor, with the same class and the same order of checks (R-READS): every directory
    /// record, then the required tables `head`, `hhea`, `maxp`, `OS/2` and `hmtx`, the eager reads,
    /// the `post` pair and the cmap. Kerning, ligatures, VDMX, COLR and outlines are read on first
    /// use; none of them can fail the parse.
    pub fn parse(bytes: Arc<[u8]>, limits: &FontLimits) -> Result<Face> {
        let view = View::new(&bytes);
        let dir = TableDir::parse(view)?;
        let req = |tag: Tag| dir.last(tag).ok_or(FontError::MissingTable(tag));
        let head = req(Tag::HEAD)?;
        let hhea = req(Tag::HHEA)?;
        let maxp = req(Tag::MAXP)?;
        let os2 = req(Tag::OS2)?;
        let num_glyphs = view.u16(u64::from(maxp.offset) + 4)?;
        let number_of_h_metrics = view.u16(u64::from(hhea.offset) + 34)?;
        let hmtx_offset = req(Tag::HMTX)?.offset;
        let (h, hh, o) = (u64::from(head.offset), u64::from(hhea.offset), u64::from(os2.offset));
        let mut metrics = FontMetrics {
            units_per_em: view.u16(h + 18)?,
            ascender: view.i16(hh + 4)?,
            descender: view.i16(hh + 6)?,
            line_gap: view.i16(hh + 8)?,
            s_typo_ascender: view.i16(o + 68)?,
            s_typo_descender: view.i16(o + 70)?,
            s_typo_line_gap: view.i16(o + 72)?,
            us_win_ascent: view.u16(o + 74)?,
            us_win_descent: view.u16(o + 76)?,
            fs_selection: view.u16(o + 62)?,
            ..FontMetrics::default()
        };
        if os2.length >= 86 && view.u16(o)? >= 1 {
            metrics.code_page_range1 = Some(view.u32(o + 78)?);
        }
        if os2.length >= 26 {
            metrics.y_superscript_y_offset = Some(view.i16(o + 24)?);
        }
        if os2.length >= 4 {
            metrics.x_avg_char_width = Some(view.i16(o + 2)?);
        }
        if os2.length >= 42 {
            metrics.panose = Some(<[u8; 10]>::try_from(view.slice(o + 32, 10)?).map_err(|_| FontError::OutOfBounds)?);
        }
        if let Some(post) = dir.last(Tag::POST)
            && post.length >= 12
        {
            let p = u64::from(post.offset);
            metrics.underline_position = view.i16(p + 8)?;
            metrics.underline_thickness = view.i16(p + 10)?;
        }
        let cmap_table = req(Tag::CMAP)?;
        let cmap = Cmap::parse(view, u64::from(cmap_table.offset), CmapScoring::Face, limits)?;
        Ok(Face(Arc::new(FaceData {
            bytes,
            dir,
            metrics,
            num_glyphs,
            number_of_h_metrics,
            hmtx_offset,
            cmap,
            kerning: OnceLock::new(),
            gsub: OnceLock::new(),
            vdmx: OnceLock::new(),
            colr: OnceLock::new(),
            outlines: Mutex::new(OutlineCache::default()),
            limits: *limits,
        })))
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.0.bytes
    }

    pub fn metrics(&self) -> &FontMetrics {
        &self.0.metrics
    }

    pub fn num_glyphs(&self) -> u16 {
        self.0.num_glyphs
    }

    /// The glyph of a code point (0 = `.notdef`); a format-12 start glyph plus offset is saturated
    /// to `u32::MAX` (every consumer treats any id above 0xFFFF alike).
    pub fn glyph_id(&self, code_point: u32) -> u32 {
        self.0.cmap.glyph_id(View::new(&self.0.bytes), code_point)
    }

    /// The advance of a glyph in font units; a read past the end of the input is 0 (G4).
    pub fn advance_width(&self, glyph_id: u32) -> u16 {
        advance_width_at(View::new(&self.0.bytes), self.0.hmtx_offset, self.0.number_of_h_metrics, glyph_id)
    }

    /// The kerning data, read on the first kerning query (GPOS, else the legacy `kern`; never fails).
    fn kerning(&self) -> &Kerning {
        self.0.kerning.get_or_init(|| Kerning::of_font(&self.0.bytes, &self.0.dir, &self.0.limits))
    }

    /// The Office kerning emulation (S19): union of the `kern` features of every script, first
    /// value wins across lookups, first-glyph XAdvance only, lookup flags ignored, the class-0
    /// rule, legacy `kern` when GPOS gave no pair. Not OpenType shaping. 0 = none; either glyph
    /// above 0xFFFF gives 0.
    pub fn kern_units(&self, g1: u32, g2: u32) -> i32 {
        self.kerning().units(g1, g2)
    }

    /// GPOS registers `kern` in the Latin (or default) script; true without GPOS.
    pub fn kern_in_latin(&self) -> bool {
        self.kerning().kern_in_latin
    }

    /// Applies the requested GSUB features to a run of glyph ids; `None` when the font has none of
    /// them, or when the call passes the face's work budget (S4-3).
    pub fn ligate(&self, gids: &[u32], features: &[Tag]) -> Option<Vec<LigGroup>> {
        let gsub = self.0.gsub.get_or_init(|| GsubLigatures::of_font(&self.0.bytes, &self.0.dir, &self.0.limits));
        gsub.as_ref()?.ligate_within(gids, features, &mut self.0.limits.work())
    }

    /// Code page bit 31 (symbol) or a glyph for U+F020 (`isSymbolFont`).
    pub fn is_symbol_font(&self) -> bool {
        self.0.metrics.code_page_range1.unwrap_or(0) >> 31 == 1 || self.glyph_id(0xF020) != 0
    }

    /// The VDMX entry of a pixel size (the group of the first ratio that serves a square device,
    /// read once); `None` without a readable VDMX or for a size it does not list.
    pub fn vdmx(&self, ppem: u16) -> Option<VdmxEntry> {
        let table = self.0.vdmx.get_or_init(|| {
            let record = self.0.dir.last(Tag::VDMX)?;
            vdmx::parse(View::new(&self.0.bytes), record)
        });
        table.as_ref()?.entries.get(&ppem).copied()
    }

    fn colr_info(&self) -> Option<&ColrInfo> {
        self.0.colr.get_or_init(|| colr::info(View::new(&self.0.bytes), self.0.dir.last(Tag::COLR), self.0.dir.last(Tag::CPAL))).as_ref()
    }

    /// COLR version 0 base records with a CPAL palette are present and readable.
    pub fn has_color_glyphs(&self) -> bool {
        self.colr_info().is_some()
    }

    /// The colour layers of a glyph; `None` without colour data, for a glyph with no layers, or on
    /// a read past the end of the input (G4).
    pub fn color_layers(&self, glyph_id: u32) -> Option<Vec<ColorLayer>> {
        let info = self.colr_info()?;
        colr::layers(View::new(&self.0.bytes), info, glyph_id)
    }

    /// The outline of a glyph (`glyf`, simple and composite), y up; `None` for a font without
    /// `glyf`, a glyph that does not exist or cannot be read, or one past a limit of
    /// [`FontLimits`]. An empty glyph is `Some` of no commands. Kept per face up to
    /// `outline_cache_entries` glyphs and 4 MiB of commands (the cache is cleared whole when it is full; 0 turns
    /// it off; a glyph of more than 256 KiB of commands is not kept).
    pub fn glyph_outline(&self, glyph_id: u32) -> Option<Arc<[OutlineCmd]>> {
        if glyph_id >= u32::from(self.0.num_glyphs) {
            return None;
        }
        if let Some(hit) = self.0.outlines.lock().unwrap_or_else(PoisonError::into_inner).map.get(&glyph_id) {
            return hit.clone();
        }
        let view = View::new(&self.0.bytes);
        let computed: Option<Arc<[OutlineCmd]>> =
            outline::glyph_outline(view, &self.0.dir, self.0.num_glyphs, &self.0.limits, glyph_id).map(Arc::from);
        let cap = usize::try_from(self.0.limits.outline_cache_entries).unwrap_or(usize::MAX);
        let size = computed.as_ref().map_or(0, |o| o.len().saturating_mul(size_of::<OutlineCmd>()));
        if cap > 0 && size <= outline::OUTLINE_CACHE_MAX_ENTRY_BYTES {
            let mut cache = self.0.outlines.lock().unwrap_or_else(PoisonError::into_inner);
            if cache.map.len() >= cap || cache.bytes.saturating_add(size) > outline::OUTLINE_CACHE_MAX_BYTES {
                cache.map.clear();
                cache.bytes = 0;
            }
            if cache.map.insert(glyph_id, computed.clone()).is_none() {
                cache.bytes += size;
            }
        }
        computed
    }

    /// The last directory record with the tag.
    pub fn table(&self, tag: Tag) -> Option<TableRecord> {
        self.0.dir.last(tag)
    }
}
