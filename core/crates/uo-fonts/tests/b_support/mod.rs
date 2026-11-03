//! Fonts built in code for the tests of frontier B (face, cmap, VDMX, COLR, outlines, program,
//! info, `.ttc`): tables as the TS reads them, on top of `uo_testkit::generate::sfnt`. No
//! proprietary font bytes anywhere.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

pub fn head(units_per_em: u16, loca_long: i16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).u32(0x0001_0000).u32(0xDEAD_BEEF).u32(0x5F0F_3CF5).u16(0).u16(units_per_em);
    w.zeros(16);
    w.i16(-11).i16(-22).i16(33).i16(44);
    w.u16(0).u16(8).i16(2).i16(loca_long).i16(0);
    let b = w.into_bytes();
    assert_eq!(b.len(), 54);
    b
}

pub fn hhea(ascender: i16, descender: i16, line_gap: i16, number_of_h_metrics: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0001_0000).i16(ascender).i16(descender).i16(line_gap);
    w.zeros(34 - 10);
    w.u16(number_of_h_metrics);
    w.into_bytes()
}

pub fn maxp(num_glyphs: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0000_5000).u16(num_glyphs);
    w.into_bytes()
}

/// An OS/2 table of `len` bytes (the fields past `len` are cut), with recognisable values:
/// xAvgCharWidth 555, weight 700, fsType `fs_type`, ySuperscriptYOffset 140, panose 1..=10,
/// unicode ranges `0x1111_1111 * (1..=4)`, fsSelection `fs_selection`, typo 760/-240/80,
/// win 900/300, code pages `0x8000_0001` / `0x0000_0002`.
pub fn os2(version: u16, len: usize, fs_type: u16, fs_selection: u16) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(version).i16(555).u16(700).u16(5).u16(fs_type);
    w.zeros(14);
    w.i16(140);
    w.zeros(6);
    assert_eq!(w.len(), 32);
    w.bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    w.u32(0x1111_1111).u32(0x2222_2222).u32(0x3333_3333).u32(0x4444_4444);
    w.u32(0x5555_5555);
    w.u16(fs_selection).u16(0x20).u16(0x7E);
    w.i16(760).i16(-240).i16(80).u16(900).u16(300);
    w.u32(0x8000_0001).u32(0x0000_0002);
    let mut b = w.into_bytes();
    assert_eq!(b.len(), 86);
    b.resize(96.max(len), 0);
    b.truncate(len);
    b
}

/// hmtx: `(advance, lsb)` records, then lsb-only entries up to the glyph count.
pub fn hmtx(metrics: &[(u16, i16)], extra_lsb: usize) -> Vec<u8> {
    let mut w = BeWriter::new();
    for &(adv, lsb) in metrics {
        w.u16(adv).i16(lsb);
    }
    for _ in 0..extra_lsb {
        w.i16(7);
    }
    w.into_bytes()
}

pub fn post(position: i16, thickness: i16, len: usize) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u32(0x0003_0000).i32(-0x0003_4000).i16(position).i16(thickness);
    let mut b = w.into_bytes();
    b.resize(32.max(len), 0);
    b.truncate(len);
    b
}

#[derive(Clone, Copy, Debug)]
pub enum Ro {
    Zero,
    /// the first glyph of the segment is `glyph_id_array[i]`
    Array(u16),
    Raw(u16),
}

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub start: u16,
    pub end: u16,
    pub delta: i16,
    pub ro: Ro,
}

pub fn seg(start: u16, end: u16, delta: i16) -> Seg {
    Seg { start, end, delta, ro: Ro::Zero }
}

pub fn seg_array(start: u16, end: u16, delta: i16, index: u16) -> Seg {
    Seg { start, end, delta, ro: Ro::Array(index) }
}

/// A format-4 subtable; `seg_x2` overrides the declared `segCountX2` (an odd value runs the TS
/// loop `ceil(segX2 / 2)` times over arrays laid out with the exact `segX2`).
pub fn format4(segs: &[Seg], glyph_ids: &[u16], seg_x2: Option<u16>) -> Vec<u8> {
    let n = segs.len();
    let declared = seg_x2.unwrap_or((n * 2) as u16);
    let mut w = BeWriter::new();
    w.u16(4).u16(0).u16(0).u16(declared).u16(0).u16(0).u16(0);
    for s in segs {
        w.u16(s.end);
    }
    w.u16(0);
    for s in segs {
        w.u16(s.start);
    }
    for s in segs {
        w.i16(s.delta);
    }
    for (i, s) in segs.iter().enumerate() {
        let ro = match s.ro {
            Ro::Zero => 0,
            Ro::Array(idx) => ((n - i) * 2) as u16 + idx * 2,
            Ro::Raw(v) => v,
        };
        w.u16(ro);
    }
    for &g in glyph_ids {
        w.u16(g);
    }
    let len = w.len() as u16;
    w.set_u16(2, len);
    w.into_bytes()
}

pub fn format12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(12).u16(0).u32(16 + 12 * groups.len() as u32).u32(0).u32(groups.len() as u32);
    for &(s, e, g) in groups {
        w.u32(s).u32(e).u32(g);
    }
    w.into_bytes()
}

/// A cmap table from `(platform, encoding, subtable)` records.
pub fn cmap(subs: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(subs.len() as u16);
    let mut off = 4 + 8 * subs.len() as u32;
    for (p, e, data) in subs {
        w.u16(*p).u16(*e).u32(off);
        off += data.len() as u32;
    }
    for (_, _, data) in subs {
        w.bytes(data);
    }
    w.into_bytes()
}

/// The default cmap: Windows BMP, `A..=C` to glyphs 1..=3 by delta, `0x4E00..=0x4E01` through the
/// glyph id array to 4 and 3.
pub fn basic_cmap() -> Vec<u8> {
    cmap(&[(3, 1, format4(&[seg(0x41, 0x43, -0x40), seg_array(0x4E00, 0x4E01, 0, 0), seg(0xFFFF, 0xFFFF, 1)], &[4, 3], None))])
}

#[derive(Clone)]
pub struct Font {
    pub tables: Vec<SfntTable>,
    pub options: SfntOptions,
}

impl Font {
    /// 5 glyphs, 3 hMetrics (advances 500, 600, 700; glyphs 3 and 4 have lsb only), unitsPerEm 1000,
    /// OS/2 version 4 of 96 bytes, a 32-byte post, no glyf.
    pub fn basic() -> Font {
        let t = |tag: &[u8; 4], data: Vec<u8>| SfntTable::new(tag, data);
        Font {
            tables: vec![
                t(b"head", head(1000, 0)),
                t(b"hhea", hhea(800, -200, 90, 3)),
                t(b"maxp", maxp(5)),
                t(b"OS/2", os2(4, 96, 0, 0x41)),
                t(b"hmtx", hmtx(&[(500, 10), (600, 20), (700, 30)], 2)),
                t(b"cmap", basic_cmap()),
                t(b"post", post(-100, 50, 32)),
            ],
            options: SfntOptions::default(),
        }
    }

    pub fn set(mut self, tag: &[u8; 4], data: Vec<u8>) -> Font {
        let t = SfntTable::new(tag, data);
        match self.tables.iter().rposition(|x| &x.tag == tag) {
            Some(i) => self.tables[i] = t,
            None => self.tables.push(t),
        }
        self
    }

    pub fn add(mut self, tag: &[u8; 4], data: Vec<u8>) -> Font {
        self.tables.push(SfntTable::new(tag, data));
        self
    }

    pub fn without(mut self, tag: &[u8; 4]) -> Font {
        self.tables.retain(|t| &t.tag != tag);
        self
    }

    pub fn table(&self, tag: &[u8; 4]) -> &SfntTable {
        self.tables.iter().rev().find(|t| &t.tag == tag).unwrap()
    }

    pub fn build(&self) -> Vec<u8> {
        build_sfnt(&self.tables, &self.options)
    }
}

/// A simple glyph from contours of absolute `(x, y, on_curve)` points, every delta as an `i16`.
pub fn simple_glyph(contours: &[&[(i16, i16, bool)]]) -> Vec<u8> {
    let mut ends = Vec::new();
    let mut total = 0u16;
    for c in contours {
        total += c.len() as u16;
        ends.push(total - 1);
    }
    simple_glyph_ends(&ends, contours.iter().flat_map(|c| c.iter().copied()).collect::<Vec<_>>().as_slice())
}

/// A simple glyph with explicit end points (they may be unsorted or past the last point).
pub fn simple_glyph_ends(ends: &[u16], points: &[(i16, i16, bool)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(ends.len() as i16).i16(0).i16(0).i16(0).i16(0);
    for &e in ends {
        w.u16(e);
    }
    w.u16(0);
    for &(_, _, on) in points {
        w.u8(u8::from(on));
    }
    let mut x = 0i32;
    for &(px, _, _) in points {
        w.i16((i32::from(px) - x) as i16);
        x = i32::from(px);
    }
    let mut y = 0i32;
    for &(_, py, _) in points {
        w.i16((i32::from(py) - y) as i16);
        y = i32::from(py);
    }
    w.into_bytes()
}

#[derive(Clone, Copy, Debug)]
pub enum Scale {
    None,
    Uniform(i16),
    Xy(i16, i16),
    Matrix(i16, i16, i16, i16),
}

#[derive(Clone, Copy, Debug)]
pub struct Comp {
    pub gid: u16,
    pub dx: i16,
    pub dy: i16,
    pub words: bool,
    pub xy: bool,
    pub scale: Scale,
}

pub fn comp(gid: u16, dx: i16, dy: i16) -> Comp {
    Comp { gid, dx, dy, words: true, xy: true, scale: Scale::None }
}

/// A composite glyph; `more` is set on every component but the last.
pub fn composite(components: &[Comp]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(-1).i16(0).i16(0).i16(0).i16(0);
    for (i, c) in components.iter().enumerate() {
        let mut flags = 0u16;
        if c.words {
            flags |= 1;
        }
        if c.xy {
            flags |= 2;
        }
        match c.scale {
            Scale::None => {}
            Scale::Uniform(_) => flags |= 8,
            Scale::Xy(..) => flags |= 0x40,
            Scale::Matrix(..) => flags |= 0x80,
        }
        if i + 1 < components.len() {
            flags |= 0x20;
        }
        w.u16(flags).u16(c.gid);
        if c.words {
            w.i16(c.dx).i16(c.dy);
        } else {
            w.i8(c.dx as i8).i8(c.dy as i8);
        }
        match c.scale {
            Scale::None => {}
            Scale::Uniform(a) => {
                w.i16(a);
            }
            Scale::Xy(a, d) => {
                w.i16(a).i16(d);
            }
            Scale::Matrix(a, b, c2, d) => {
                w.i16(a).i16(b).i16(c2).i16(d);
            }
        }
    }
    w.into_bytes()
}

/// `glyf` and `loca` from glyph data (each padded to 2 or 4 bytes).
pub fn glyf_loca(glyphs: &[Vec<u8>], long: bool) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = BeWriter::new();
    let mut loca = BeWriter::new();
    let mut offsets = vec![0u32];
    for g in glyphs {
        glyf.bytes(g);
        let pad = if long { (4 - g.len() % 4) % 4 } else { g.len() % 2 };
        glyf.zeros(pad);
        offsets.push(glyf.len() as u32);
    }
    for o in offsets {
        if long {
            loca.u32(o);
        } else {
            loca.u16((o / 2) as u16);
        }
    }
    (glyf.into_bytes(), loca.into_bytes())
}

/// A font with `glyf`/`loca` for the glyphs (head `indexToLocFormat` and maxp match).
pub fn font_with_glyphs(glyphs: &[Vec<u8>], long: bool) -> Font {
    let (glyf, loca) = glyf_loca(glyphs, long);
    Font::basic().set(b"head", head(1000, i16::from(long))).set(b"maxp", maxp(glyphs.len() as u16)).add(b"glyf", glyf).add(b"loca", loca)
}

/// A name table of `(platform, encoding, language, name id, bytes)` records.
pub fn name_table(records: &[(u16, u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(records.len() as u16).u16(6 + 12 * records.len() as u16);
    let mut off = 0u16;
    for (p, e, l, id, s) in records {
        w.u16(*p).u16(*e).u16(*l).u16(*id).u16(s.len() as u16).u16(off);
        off += s.len() as u16;
    }
    for (_, _, _, _, s) in records {
        w.bytes(s);
    }
    w.into_bytes()
}

pub fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// A VDMX table: ratio records `(charset, x, y0, y1, group)` and the groups of `(ppem, yMax, yMin)`
/// each ratio points to.
pub fn vdmx(ratios: &[(u8, u8, u8, u8, usize)], groups: &[Vec<(u16, i16, i16)>]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(1).u16(groups.len() as u16).u16(ratios.len() as u16);
    for &(c, x, y0, y1, _) in ratios {
        w.u8(c).u8(x).u8(y0).u8(y1);
    }
    let first = 6 + 4 * ratios.len() + 2 * ratios.len();
    let mut starts = Vec::new();
    let mut at = first;
    for g in groups {
        starts.push(at as u16);
        at += 4 + 6 * g.len();
    }
    for &(.., group) in ratios {
        w.u16(starts[group]);
    }
    for g in groups {
        w.u16(g.len() as u16).u8(1).u8(2);
        for &(p, a, b) in g {
            w.u16(p).i16(a).i16(b);
        }
    }
    w.into_bytes()
}

/// COLR v0: base records `(gid, first, count)` and layers `(gid, palette index)`.
pub fn colr(bases: &[(u16, u16, u16)], layers: &[(u16, u16)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(bases.len() as u16).u32(14).u32(14 + 6 * bases.len() as u32).u16(layers.len() as u16);
    for &(g, f, n) in bases {
        w.u16(g).u16(f).u16(n);
    }
    for &(g, p) in layers {
        w.u16(g).u16(p);
    }
    w.into_bytes()
}

/// CPAL: one palette of BGRA colours.
pub fn cpal(colors: &[[u8; 4]]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(0).u16(colors.len() as u16).u16(1).u16(colors.len() as u16).u32(14).u16(0);
    for c in colors {
        w.bytes(c);
    }
    w.into_bytes()
}

/// A legacy `kern` table: version 0 with the subtables given as `(coverage, pairs)`; `length` of
/// each subtable is written as the real one unless overridden by `lengths`.
pub type KernSubtable = (u16, Vec<(u16, u16, i16)>);

pub fn kern_table(version: u16, subtables: &[KernSubtable], lengths: &[Option<u16>]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.u16(version).u16(subtables.len() as u16);
    for (i, (coverage, pairs)) in subtables.iter().enumerate() {
        let real = 14 + 6 * pairs.len() as u16;
        let length = lengths.get(i).copied().flatten().unwrap_or(real);
        w.u16(0).u16(length).u16(*coverage).u16(pairs.len() as u16).u16(0).u16(0).u16(0);
        for &(l, r, v) in pairs {
            w.u16(l).u16(r).i16(v);
        }
    }
    w.into_bytes()
}

/// Deterministic mutations of a file: `rounds` times, flip, set or zero a few bytes, sometimes
/// truncate.
pub fn mutate(bytes: &[u8], seed: u32, rounds: usize) -> Vec<Vec<u8>> {
    let mut rng = uo_testkit::generate::Rng::new(seed);
    let mut out = Vec::new();
    for _ in 0..rounds {
        let mut b = bytes.to_vec();
        for _ in 0..1 + rng.below(6) {
            let i = rng.below(b.len().max(1));
            if let Some(x) = b.get_mut(i) {
                *x = match rng.below(4) {
                    0 => 0,
                    1 => 0xFF,
                    2 => x.wrapping_add(1),
                    _ => (rng.next_u32() & 0xFF) as u8,
                };
            }
        }
        if rng.one_in(8) {
            let keep = rng.below(b.len().max(1));
            b.truncate(keep);
        }
        out.push(b);
    }
    out
}

/// Generated fonts that exercise the quirks of every reader of frontier B (the TS differential
/// and the no-panic loop start from these).
pub fn quirk_fonts() -> Vec<(String, Vec<u8>)> {
    let f4 = |g: i16| format4(&[seg(0x41, 0x41, g - 0x41)], &[], None);
    let mut fonts: Vec<(String, Vec<u8>)> = vec![("basic".into(), Font::basic().build())];
    let mut add = |name: &str, f: Font| fonts.push((name.into(), f.build()));
    add("scores", Font::basic().set(b"cmap", cmap(&[(1, 0, f4(30)), (3, 0, f4(31)), (0, 3, f4(32))])));
    add(
        "symbol",
        Font::basic().set(
            b"cmap",
            cmap(&[(3, 0, format4(&[seg(0xF041, 0xF042, 7 - 0xF041_u32 as i16), seg(0xF020, 0xF020, 1 - 0xF020_u32 as i16)], &[], None))]),
        ),
    );
    add(
        "format12",
        Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&[(0x41, 0x5A, 1), (0x1_0000, 0x1_0010, 0xFFFF_FFF8), (0x30, 0x2F, 9)]))])),
    );
    let mut odd = format4(&[seg(0x41, 0x43, -0x40), seg(0x50, 0x52, -0x4F), seg(0x60, 0x62, 1)], &[], None);
    odd[6..8].copy_from_slice(&5u16.to_be_bytes());
    add("odd_segx2", Font::basic().set(b"cmap", cmap(&[(3, 1, odd)])));
    add(
        "overlap",
        Font::basic().set(
            b"cmap",
            cmap(&[(3, 1, format4(&[seg(0x60, 0x6F, 1), seg(0x41, 0x7F, 2), seg(0x50, 0x5F, 3), seg(0x41, 0x41, 4)], &[], None))]),
        ),
    );
    add(
        "arrays",
        Font::basic().set(
            b"cmap",
            cmap(&[(
                3,
                1,
                format4(
                    &[
                        seg_array(0x10, 0x12, 5, 0),
                        seg_array(0x20, 0x20, 0, 3),
                        Seg { start: 0x30, end: 0x31, delta: 0, ro: Ro::Raw(0x7000) },
                    ],
                    &[1, 0, 9, 4],
                    None,
                ),
            )]),
        ),
    );
    add("os2_v0_short", Font::basic().set(b"OS/2", os2(0, 60, 2, 0)));
    add("os2_v1", Font::basic().set(b"OS/2", os2(1, 86, 0x200, 0x40)));
    add("hmtx_zero", Font::basic().set(b"hhea", hhea(800, -200, 90, 0)));
    add(
        "vdmx",
        Font::basic()
            .add(b"VDMX", vdmx(&[(1, 5, 1, 2, 0), (1, 1, 1, 1, 1)], &[vec![(10, 1, 1)], vec![(10, 100, -20), (12, 120, -22), (10, 3, 3)]])),
    );
    let bases = [(5u16, 0u16, 2u16), (9, 2, 1), (12, 3, 3), (20, 4, 10)];
    let layers = [(100u16, 0u16), (101, 1), (102, 0xFFFF), (103, 7), (104, 1), (105, 0)];
    add("colr", Font::basic().add(b"COLR", colr(&bases, &layers)).add(b"CPAL", cpal(&[[0x10, 0x20, 0x30, 0xFF], [0, 0x80, 0xFF, 0x80]])));
    let sq: [(i16, i16, bool); 4] = [(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)];
    let glyphs = vec![
        vec![],
        simple_glyph(&[&sq]),
        simple_glyph(&[&[(0, 0, true), (50, 100, false), (100, 0, true)]]),
        simple_glyph(&[&[(0, 0, false), (100, 0, false), (100, 100, false)]]),
        simple_glyph(&[&[(5, 5, true)], &[(0, 0, true), (10, 0, true)]]),
        composite(&[
            Comp { scale: Scale::Matrix(0x4000, 0x2000, 0x1000, 0x7FFF), ..comp(1, 10, 20) },
            Comp { scale: Scale::Uniform(0x2000), ..comp(2, -5, 7) },
        ]),
        composite(&[Comp { words: false, ..comp(2, -3, 4) }, Comp { xy: false, ..comp(3, 99, 99) }]),
        simple_glyph_ends(&[1, 0, 1], &[(0, 0, true), (10, 0, true)]),
        composite(&[comp(7, 0, 0), comp(99, 0, 0)]),
    ];
    fonts.push(("glyphs_short".into(), font_with_glyphs(&glyphs, false).build()));
    fonts.push(("glyphs_long".into(), font_with_glyphs(&glyphs, true).build()));
    fonts.push((
        "glyph_end_past_npts".into(),
        font_with_glyphs(&[vec![], simple_glyph_ends(&[5, 1], &[(0, 0, true), (10, 0, true)])], false).build(),
    ));
    fonts.push((
        "names".into(),
        Font::basic()
            .add(
                b"name",
                name_table(&[
                    (3, 1, 0x407, 1, utf16be("Familie")),
                    (3, 1, 0x409, 1, utf16be("Family")),
                    (3, 1, 0x409, 2, vec![0xD8, 0, 0, 0x41, 0xDC, 0, 0x7F]),
                    (1, 0, 0, 4, b"Mac".to_vec()),
                    (3, 10, 0x407, 4, utf16be("Voller Name")),
                ]),
            )
            .build(),
    ));
    fonts.push(("no_post".into(), Font::basic().without(b"post").build()));
    fonts.push(("no_maxp".into(), Font::basic().without(b"maxp").build()));
    fonts.push(("no_os2".into(), Font::basic().without(b"OS/2").build()));
    fonts.push(("fs_restricted".into(), Font::basic().set(b"OS/2", os2(4, 96, 0x2, 0)).build()));
    fonts
}
