//! Porta de docx-layout/src/typography/font/sfnt.ts — a parte que o layout usa para medir
//! texto: tabelas, métricas, cmap (formatos 4 e 12), advances e kerning (GPOS e 'kern'
//! legada). Fica de fora o que não entra na medição: VDMX, GSUB, COLR e contornos.
use std::cell::RefCell;

use crate::fx::FxMap;
use crate::gpos::{gpos_kern_in_latin, kern_key, parse_gpos_kern, KernClass0, KernMap};
use crate::read::{js_round, OutOfRange, View, R};

#[derive(Debug, Clone, Default)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub s_typo_ascender: i16,
    pub s_typo_descender: i16,
    pub s_typo_line_gap: i16,
    pub us_win_ascent: u16,
    pub us_win_descent: u16,
    pub fs_selection: u16,
    pub underline_position: i16,
    pub underline_thickness: i16,
}

#[derive(Clone, Copy)]
enum Mapper {
    /// (c + delta) & 0xffff
    Delta(i32),
    /// glyphIdArray: idP + rangeOffset + (c - start) * 2
    Range { id_p: usize, range_offset: usize, delta: i32 },
    /// cmap 12: startGlyph + (c - start)
    Group(u32),
}

#[derive(Clone, Copy)]
struct CmapRange {
    start: u32,
    end: u32,
    mapper: Mapper,
}

pub struct SfntFont {
    bytes: Vec<u8>,
    tables: FxMap<[u8; 4], (usize, usize)>,
    pub metrics: FontMetrics,
    number_of_h_metrics: usize,
    hmtx_offset: usize,
    cmap_ranges: Vec<CmapRange>,
    symbol_cmap: bool,
    advance_cache: RefCell<FxMap<u32, u16>>,
    pub kern_in_latin: bool,
    kern_map: KernMap,
    kern_class0: KernClass0,
}

#[derive(Debug)]
pub enum FontError {
    MissingTable(&'static str),
    Cmap(&'static str),
    OutOfRange,
}

impl From<OutOfRange> for FontError {
    fn from(_: OutOfRange) -> Self {
        FontError::OutOfRange
    }
}

impl SfntFont {
    pub fn new(bytes: Vec<u8>) -> Result<SfntFont, FontError> {
        let mut f = SfntFont {
            bytes,
            tables: FxMap::default(),
            metrics: FontMetrics::default(),
            number_of_h_metrics: 0,
            hmtx_offset: 0,
            cmap_ranges: Vec::new(),
            symbol_cmap: false,
            advance_cache: RefCell::new(FxMap::default()),
            kern_in_latin: true,
            kern_map: KernMap::default(),
            kern_class0: KernClass0::default(),
        };
        let v = View(&f.bytes);
        let num_tables = v.u16(4)? as usize;
        let mut tables = FxMap::default();
        for i in 0..num_tables {
            let p = 12 + i * 16;
            tables.insert(v.tag(p)?, (v.u32(p + 8)? as usize, v.u32(p + 12)? as usize));
        }
        let req = |tag: &'static str| -> Result<(usize, usize), FontError> {
            let t: [u8; 4] = tag.as_bytes().try_into().unwrap();
            tables.get(&t).copied().ok_or(FontError::MissingTable(tag))
        };
        let head = req("head")?;
        let hhea = req("hhea")?;
        let maxp = req("maxp")?;
        let os2 = req("OS/2")?;
        let _num_glyphs = v.u16(maxp.0 + 4)?;
        let number_of_h_metrics = v.u16(hhea.0 + 34)? as usize;
        let hmtx_offset = req("hmtx")?.0;
        let mut metrics = FontMetrics {
            units_per_em: v.u16(head.0 + 18)?,
            ascender: v.i16(hhea.0 + 4)?,
            descender: v.i16(hhea.0 + 6)?,
            line_gap: v.i16(hhea.0 + 8)?,
            s_typo_ascender: v.i16(os2.0 + 68)?,
            s_typo_descender: v.i16(os2.0 + 70)?,
            s_typo_line_gap: v.i16(os2.0 + 72)?,
            us_win_ascent: v.u16(os2.0 + 74)?,
            us_win_descent: v.u16(os2.0 + 76)?,
            fs_selection: v.u16(os2.0 + 62)?,
            underline_position: 0,
            underline_thickness: 0,
        };
        if let Some(&(off, len)) = tables.get(b"post") {
            if len >= 12 {
                metrics.underline_position = v.i16(off + 8)?;
                metrics.underline_thickness = v.i16(off + 10)?;
            }
        }
        let (ranges, symbol) = parse_cmap(v, req("cmap")?.0)?;

        let mut class0 = KernClass0::default();
        let mut kern_in_latin = true;
        let mut kern_map = match tables.get(b"GPOS") {
            Some(&(gpos, _)) => match parse_gpos_kern(v, gpos, &mut class0) {
                Ok(map) => match gpos_kern_in_latin(v, gpos) {
                    Ok(latin) => {
                        kern_in_latin = latin;
                        map
                    }
                    Err(_) => {
                        class0.clear();
                        KernMap::default()
                    }
                },
                Err(_) => {
                    class0.clear();
                    KernMap::default()
                }
            },
            None => KernMap::default(),
        };
        if kern_map.is_empty() {
            if let Some(&(off, _)) = tables.get(b"kern") {
                let _ = parse_legacy_kern(v, off, &mut kern_map);
            }
        }

        f.tables = tables;
        f.metrics = metrics;
        f.number_of_h_metrics = number_of_h_metrics;
        f.hmtx_offset = hmtx_offset;
        f.cmap_ranges = ranges;
        f.symbol_cmap = symbol;
        f.kern_in_latin = kern_in_latin;
        f.kern_map = kern_map;
        f.kern_class0 = class0;
        Ok(f)
    }

    pub fn kern_pairs(&self) -> usize {
        self.kern_map.len()
    }

    pub fn kern_class0_len(&self) -> usize {
        self.kern_class0.len()
    }

    pub fn has_table(&self, tag: &[u8; 4]) -> bool {
        self.tables.contains_key(tag)
    }

    /// Kerning do par de GLIFOS (g1→g2) em twips no em de dispositivo.
    pub fn kern_glyphs_twips(&self, g1: u32, g2: u32, size_pt: f64) -> f64 {
        if g1 > 0xffff || g2 > 0xffff {
            return 0.0;
        }
        let units = match self.kern_map.get(&kern_key(g1, g2)) {
            Some(&u) => u,
            None => match self.kern_class0.get(&g1) {
                Some(c0) if !c0.cd2.contains_key(&g2) => c0.adv,
                _ => 0,
            },
        };
        if units == 0 {
            return 0.0;
        }
        let device_em_pt = (js_round((size_pt * 600.0) / 72.0) * 72.0) / 600.0;
        (units as f64 * device_em_pt * 20.0) / self.metrics.units_per_em as f64
    }

    pub fn glyph_id(&self, code_point: u32) -> u32 {
        let g = self.lookup_glyph(code_point);
        if g == 0 && self.symbol_cmap && code_point < 0x100 {
            return self.lookup_glyph(code_point + 0xf000);
        }
        g
    }

    fn lookup_glyph(&self, cp: u32) -> u32 {
        let (mut lo, mut hi) = (0isize, self.cmap_ranges.len() as isize - 1);
        while lo <= hi {
            let mid = (lo + hi) >> 1;
            let r = self.cmap_ranges[mid as usize];
            if cp < r.start {
                hi = mid - 1;
            } else if cp > r.end {
                lo = mid + 1;
            } else {
                return match r.mapper {
                    Mapper::Delta(delta) => ((cp as i64 + delta as i64) & 0xffff) as u32,
                    Mapper::Range { id_p, range_offset, delta } => {
                        let gp = id_p + range_offset + (cp - r.start) as usize * 2;
                        match View(&self.bytes).u16(gp) {
                            Ok(0) | Err(_) => 0,
                            Ok(g) => ((g as i64 + delta as i64) & 0xffff) as u32,
                        }
                    }
                    Mapper::Group(start_glyph) => start_glyph.wrapping_add(cp - r.start),
                };
            }
        }
        0
    }

    /// Advance width do glifo em unidades da fonte (com cache, como no TS).
    pub fn advance_width(&self, glyph_id: u32) -> u16 {
        if let Some(&w) = self.advance_cache.borrow().get(&glyph_id) {
            return w;
        }
        let i = (glyph_id as usize).min(self.number_of_h_metrics.saturating_sub(1));
        let w = View(&self.bytes).u16(self.hmtx_offset + i * 4).unwrap_or(0);
        self.advance_cache.borrow_mut().insert(glyph_id, w);
        w
    }

    /// Advance de um code point em twips (escala linear no em arredondado a 600 DPI).
    pub fn advance_twips(&self, code_point: u32, size_pt: f64) -> f64 {
        let units = self.advance_width(self.glyph_id(code_point)) as f64;
        let device_em_pt = (js_round((size_pt * 600.0) / 72.0) * 72.0) / 600.0;
        (units * device_em_pt * 20.0) / self.metrics.units_per_em as f64
    }
}

fn parse_cmap(v: View, cmap: usize) -> Result<(Vec<CmapRange>, bool), FontError> {
    let n = v.u16(cmap + 2)? as usize;
    let mut best: Option<usize> = None;
    let mut best_score = -1;
    for i in 0..n {
        let p = cmap + 4 + i * 8;
        let platform = v.u16(p)?;
        let encoding = v.u16(p + 2)?;
        let offset = v.u32(p + 4)? as usize;
        let score = match (platform, encoding) {
            (3, 10) => 4,
            (3, 1) => 3,
            (0, _) => 2,
            (3, 0) => 1,
            _ => 0,
        };
        if score > best_score {
            best_score = score;
            best = Some(cmap + offset);
        }
    }
    let best = best.ok_or(FontError::Cmap("cmap sem subtabela utilizável"))?;
    let symbol = best_score == 1;
    let mut ranges = Vec::new();
    match v.u16(best)? {
        4 => {
            let seg_count = v.u16(best + 6)? as usize / 2;
            let end_p = best + 14;
            let start_p = end_p + seg_count * 2 + 2;
            let delta_p = start_p + seg_count * 2;
            let range_p = delta_p + seg_count * 2;
            for s in 0..seg_count {
                let end = v.u16(end_p + s * 2)? as u32;
                let start = v.u16(start_p + s * 2)? as u32;
                let delta = v.i16(delta_p + s * 2)? as i32;
                let range_offset = v.u16(range_p + s * 2)? as usize;
                let id_p = range_p + s * 2;
                let mapper = if range_offset == 0 { Mapper::Delta(delta) } else { Mapper::Range { id_p, range_offset, delta } };
                ranges.push(CmapRange { start, end, mapper });
            }
        }
        12 => {
            let n_groups = v.u32(best + 12)? as usize;
            for g in 0..n_groups {
                let p = best + 16 + g * 12;
                ranges.push(CmapRange { start: v.u32(p)?, end: v.u32(p + 4)?, mapper: Mapper::Group(v.u32(p + 8)?) });
            }
        }
        _ => return Err(FontError::Cmap("cmap format não suportado")),
    }
    ranges.sort_by_key(|r| r.start);
    Ok((ranges, symbol))
}

/// Tabela 'kern' (versão 0, subtabelas formato 0 horizontais); erro no meio mantém o já lido.
fn parse_legacy_kern(v: View, off: usize, kern: &mut KernMap) -> R<()> {
    if v.u16(off)? != 0 {
        return Ok(());
    }
    let n = v.u16(off + 2)? as usize;
    let mut p = off + 4;
    for _ in 0..n {
        let length = v.u16(p + 2)? as usize;
        let coverage = v.u16(p + 4)?;
        let format = coverage >> 8;
        let horizontal = coverage & 1 != 0;
        let cross = coverage & 4 != 0;
        if format == 0 && horizontal && !cross {
            let pairs = v.u16(p + 6)? as usize;
            for i in 0..pairs {
                let r = p + 14 + i * 6;
                let key = kern_key(v.u16(r)? as u32, v.u16(r + 2)? as u32);
                let adj = v.i16(r + 4)? as i32;
                kern.entry(key).or_insert(adj);
            }
        }
        p += length;
    }
    Ok(())
}
