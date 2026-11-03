//! Fonts built in code for the tests of subset, EOT and the embedding plan (frontier D): a
//! TrueType font with `glyf` outlines (simple and composite), the layout tables, a format-4
//! `cmap` and a `name` table, assembled with `uo_testkit::generate::sfnt`. No proprietary font
//! bytes anywhere.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

pub mod subset_cases;

use uo_testkit::generate::Rng;
use uo_testkit::generate::sfnt::{BeWriter, SfntOptions, SfntTable, build_sfnt};

/// A simple glyph entry: `numberOfContours` 1, a box, and `payload` bytes after the header (the
/// subsetter only reads the sign of the first word).
pub fn simple_glyph(payload: usize, seed: u8) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(1).i16(0).i16(0).i16(500).i16(700);
    for i in 0..payload {
        w.u8(seed.wrapping_add(i as u8));
    }
    w.into_bytes()
}

pub const ARGS_ARE_WORDS: u16 = 0x0001;
pub const WE_HAVE_A_SCALE: u16 = 0x0008;
pub const MORE_COMPONENTS: u16 = 0x0020;
pub const X_AND_Y_SCALE: u16 = 0x0040;
pub const TWO_BY_TWO: u16 = 0x0080;

/// A composite glyph entry: `numberOfContours` -1 and one component record per `(flags, gid)`;
/// `MORE_COMPONENTS` is set on every record but the last, and the argument and transform bytes
/// follow the flags (`ARGS_ARE_WORDS`, the three transform forms).
pub fn composite_glyph(components: &[(u16, u16)]) -> Vec<u8> {
    let mut w = BeWriter::new();
    w.i16(-1).i16(0).i16(0).i16(500).i16(700);
    for (i, &(flags, gid)) in components.iter().enumerate() {
        let more = if i + 1 < components.len() { MORE_COMPONENTS } else { 0 };
        let flags = (flags & !MORE_COMPONENTS) | more;
        w.u16(flags).u16(gid);
        if flags & ARGS_ARE_WORDS != 0 {
            w.i16(10).i16(-20);
        } else {
            w.i8(10).i8(-20);
        }
        if flags & WE_HAVE_A_SCALE != 0 {
            w.i16(0x4000);
        } else if flags & X_AND_Y_SCALE != 0 {
            w.i16(0x4000).i16(0x2000);
        } else if flags & TWO_BY_TWO != 0 {
            w.i16(0x4000).i16(0).i16(0).i16(0x4000);
        }
    }
    w.into_bytes()
}

/// What [`TestFont::build`] assembles.
#[derive(Debug, Clone)]
pub struct TestFont {
    /// one `glyf` entry per glyph, empty for a glyph without an outline
    pub glyphs: Vec<Vec<u8>>,
    pub long_loca: bool,
    /// GPOS, GSUB, GDEF, kern, vhea, vmtx (dummy bytes)
    pub layout_tables: bool,
    /// the segments `(start, end, delta)` of the format-4 cmap (the 0xFFFF terminator is added)
    pub cmap_segments: Vec<(u16, u16, i16)>,
    pub fs_type: u16,
    pub weight: u16,
    pub italic: bool,
    pub family: String,
    pub version: u32,
    /// extra tables at the end of the directory (`DSIG` and the like)
    pub extra: Vec<([u8; 4], Vec<u8>)>,
    pub panose: [u8; 10],
}

impl Default for TestFont {
    fn default() -> Self {
        TestFont {
            glyphs: vec![
                simple_glyph(20, 1),
                simple_glyph(30, 2),
                simple_glyph(7, 3),
                simple_glyph(40, 4),
                Vec::new(),
                simple_glyph(11, 5),
            ],
            long_loca: false,
            layout_tables: true,
            cmap_segments: vec![(0x41, 0x44, -0x40)],
            fs_type: 0,
            weight: 400,
            italic: false,
            family: "Test Sans".to_owned(),
            version: 0x0001_0000,
            extra: vec![(*b"DSIG", vec![1, 2, 3, 4, 5, 6, 7, 8])],
            panose: [2, 11, 5, 2, 4, 2, 4, 2, 2, 3],
        }
    }
}

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

impl TestFont {
    pub fn with_glyphs(glyphs: Vec<Vec<u8>>) -> TestFont {
        TestFont { glyphs, ..TestFont::default() }
    }

    pub fn num_glyphs(&self) -> u16 {
        self.glyphs.len() as u16
    }

    /// `(glyf bytes, loca bytes)`: entries padded to 2 (short loca) or 4 bytes as real fonts are.
    pub fn glyf_and_loca(&self) -> (Vec<u8>, Vec<u8>) {
        let align = if self.long_loca { 4 } else { 2 };
        let mut glyf = Vec::new();
        let mut offsets = vec![0u32];
        for g in &self.glyphs {
            glyf.extend_from_slice(g);
            while glyf.len() % align != 0 {
                glyf.push(0);
            }
            offsets.push(glyf.len() as u32);
        }
        let mut loca = BeWriter::new();
        for o in offsets {
            if self.long_loca {
                loca.u32(o);
            } else {
                loca.u16((o / 2) as u16);
            }
        }
        (glyf, loca.into_bytes())
    }

    pub fn head(&self) -> Vec<u8> {
        let mut w = BeWriter::new();
        w.u32(0x0001_0000).u32(0x0001_0000).u32(0x1234_5678).u32(0x5F0F_3CF5).u16(0x000B).u16(1000);
        w.zeros(16);
        w.i16(0).i16(-200).i16(1000).i16(900).u16(0).u16(8).i16(2);
        w.i16(i16::from(self.long_loca)).i16(0);
        w.into_bytes()
    }

    fn cmap(&self) -> Vec<u8> {
        let mut segs = self.cmap_segments.clone();
        segs.push((0xFFFF, 0xFFFF, 1));
        let n = segs.len() as u16;
        let mut sub = BeWriter::new();
        sub.u16(4).u16(16 + 8 * n).u16(0).u16(n * 2).u16(0).u16(0).u16(0);
        for s in &segs {
            sub.u16(s.1);
        }
        sub.u16(0);
        for s in &segs {
            sub.u16(s.0);
        }
        for s in &segs {
            sub.i16(s.2);
        }
        for _ in &segs {
            sub.u16(0);
        }
        let mut w = BeWriter::new();
        w.u16(0).u16(1).u16(3).u16(1).u32(12).bytes(sub.as_bytes());
        w.into_bytes()
    }

    fn name_table(&self) -> Vec<u8> {
        let records = [
            (1u16, self.family.clone()),
            (2, "Regular".to_owned()),
            (4, format!("{} Regular", self.family)),
            (5, "Version 1.000".to_owned()),
        ];
        let mut strings = Vec::new();
        let mut recs = BeWriter::new();
        for (id, text) in &records {
            let s = utf16be(text);
            recs.u16(3).u16(1).u16(0x409).u16(*id).u16(s.len() as u16).u16(strings.len() as u16);
            strings.extend_from_slice(&s);
        }
        let mut w = BeWriter::new();
        w.u16(0).u16(records.len() as u16).u16(6 + 12 * records.len() as u16).bytes(recs.as_bytes()).bytes(&strings);
        w.into_bytes()
    }

    fn os2(&self) -> Vec<u8> {
        let mut b = vec![0u8; 96];
        let mut put = |at: usize, bytes: &[u8]| b[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, &3u16.to_be_bytes());
        put(2, &500i16.to_be_bytes());
        put(4, &self.weight.to_be_bytes());
        put(6, &5u16.to_be_bytes());
        put(8, &self.fs_type.to_be_bytes());
        put(32, &self.panose);
        for (i, r) in [0xA000_02FFu32, 0x5000_ECFB, 1, 0].into_iter().enumerate() {
            put(42 + 4 * i, &r.to_be_bytes());
        }
        let fs_selection: u16 = if self.italic { 0x0001 } else { 0x0040 };
        put(62, &fs_selection.to_be_bytes());
        put(78, &0x6000_019Fu32.to_be_bytes());
        put(82, &0xDFD7_0000u32.to_be_bytes());
        b
    }

    pub fn tables(&self) -> Vec<SfntTable> {
        let (glyf, loca) = self.glyf_and_loca();
        let n = self.num_glyphs();
        let mut hhea = BeWriter::new();
        hhea.u32(0x0001_0000).i16(900).i16(-200).i16(0).zeros(24).u16(n);
        let mut maxp = BeWriter::new();
        maxp.u32(0x0001_0000).u16(n).zeros(26);
        let mut hmtx = BeWriter::new();
        for i in 0..n {
            hmtx.u16(500 + i).i16(0);
        }
        let mut t = vec![
            SfntTable::new(b"head", self.head()),
            SfntTable::new(b"hhea", hhea.into_bytes()),
            SfntTable::new(b"maxp", maxp.into_bytes()),
            SfntTable::new(b"hmtx", hmtx.into_bytes()),
            SfntTable::new(b"cmap", self.cmap()),
            SfntTable::new(b"loca", loca),
            SfntTable::new(b"glyf", glyf),
            SfntTable::new(b"OS/2", self.os2()),
            SfntTable::new(b"post", vec![0, 3, 0, 0, 0, 0, 0, 0, 0xFF, 0x38, 0, 50, 0, 0, 0, 0]),
            SfntTable::new(b"name", self.name_table()),
            SfntTable::new(b"cvt ", vec![9; 8]),
            SfntTable::new(b"fpgm", vec![8; 6]),
            SfntTable::new(b"prep", vec![7; 5]),
            SfntTable::new(b"gasp", vec![0, 1, 0, 1, 0xFF, 0xFF, 0, 3]),
        ];
        if self.layout_tables {
            for (tag, fill) in [(b"GPOS", 1u8), (b"GSUB", 2), (b"GDEF", 3), (b"kern", 4), (b"vhea", 5), (b"vmtx", 6)] {
                t.push(SfntTable::new(tag, vec![fill; 14 + usize::from(fill)]));
            }
        }
        for (tag, data) in &self.extra {
            t.push(SfntTable::new(tag, data.clone()));
        }
        t
    }

    pub fn build(&self) -> Vec<u8> {
        build_sfnt(&self.tables(), &SfntOptions { version: self.version, ..SfntOptions::default() })
    }
}

/// Reads a directory the way a font reader does, for checking what the subsetter wrote:
/// `(tag, offset, length, checksum)` in file order.
pub fn directory(font: &[u8]) -> Vec<([u8; 4], usize, usize, u32)> {
    let u16_at = |o: usize| usize::from(u16::from_be_bytes([font[o], font[o + 1]]));
    let u32_at = |o: usize| u32::from_be_bytes([font[o], font[o + 1], font[o + 2], font[o + 3]]);
    (0..u16_at(4))
        .map(|i| {
            let p = 12 + 16 * i;
            ([font[p], font[p + 1], font[p + 2], font[p + 3]], u32_at(p + 8) as usize, u32_at(p + 12) as usize, u32_at(p + 4))
        })
        .collect()
}

pub fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    directory(font).into_iter().find(|t| &t.0 == tag).map(|t| &font[t.1..t.1 + t.2])
}

/// A copy of the font with `count` bytes of the region `[from, to)` replaced by seeded values.
pub fn mutate(font: &[u8], rng: &mut Rng, from: usize, to: usize, count: usize) -> Vec<u8> {
    let mut out = font.to_vec();
    let to = to.min(out.len());
    if from >= to {
        return out;
    }
    for _ in 0..count {
        let at = from + rng.below(to - from);
        out[at] = match rng.below(4) {
            0 => 0,
            1 => 0xFF,
            2 => 0x80,
            _ => rng.next_u32() as u8,
        };
    }
    out
}
