//! `pdf-writer/font.ts`: a TrueType font embedded as Type0/CIDFontType2 with Identity-H (CID =
//! glyph id), the used glyphs recorded for `/W` and the ToUnicode CMap (searchable, copyable text).

use std::fmt::Write as _;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_fonts::subset_true_type;
use uo_jsnum::format::Js;
use uo_jsnum::math;
use uo_jsnum::text::is_js_space;

use crate::error::{PdfError, PdfLimits, Result};
use crate::num::{latin1, utf16be_hex_units};
use crate::sink::PdfSink;
use crate::writer::PdfWriter;

/// What the writer needs to know of a TrueType font program: the TS `FontProgram`. The numbers
/// are JS numbers, so a host program that gives a fraction, NaN or a huge value gets what the TS
/// would write.
pub trait FontProgram {
    /// `unitsPerEm` of `head`.
    fn units_per_em(&self) -> f64;
    /// The glyph of a Unicode code point (0 when absent).
    fn glyph_id(&self, code_point: u32) -> u32;
    /// The advance width of a glyph, in font units.
    fn advance_width(&self, glyph_id: u32) -> f64;
    /// The font box in font units (`head.xMin..yMax`); `None` derives one from the ascent and descent.
    fn font_bbox(&self) -> Option<[f64; 4]> {
        None
    }
    /// Ascent in font units; `None` is 0.8 em.
    fn ascender(&self) -> Option<f64> {
        None
    }
    /// Descent in font units (negative); `None` is -0.2 em.
    fn descender(&self) -> Option<f64> {
        None
    }
    /// `post.italicAngle` in degrees; `None` is 0.
    fn italic_angle(&self) -> Option<f64> {
        None
    }
}

/// The face of the layout (hhea ascender and descender, no bounding box and no angle: what the
/// TS `fontFor` hands over).
impl FontProgram for uo_fonts::Face {
    fn units_per_em(&self) -> f64 {
        f64::from(self.metrics().units_per_em)
    }

    fn glyph_id(&self, code_point: u32) -> u32 {
        uo_fonts::Face::glyph_id(self, code_point)
    }

    fn advance_width(&self, glyph_id: u32) -> f64 {
        f64::from(uo_fonts::Face::advance_width(self, glyph_id))
    }

    fn ascender(&self) -> Option<f64> {
        Some(f64::from(self.metrics().ascender))
    }

    fn descender(&self) -> Option<f64> {
        Some(f64::from(self.metrics().descender))
    }
}

/// The converter's font program (head bounding box, hhea ascent and descent, the `post` angle).
impl FontProgram for uo_fonts::FontProgram {
    fn units_per_em(&self) -> f64 {
        f64::from(uo_fonts::FontProgram::units_per_em(self))
    }

    fn glyph_id(&self, code_point: u32) -> u32 {
        uo_fonts::FontProgram::glyph_id(self, code_point)
    }

    fn advance_width(&self, glyph_id: u32) -> f64 {
        f64::from(uo_fonts::FontProgram::advance_width(self, glyph_id))
    }

    fn font_bbox(&self) -> Option<[f64; 4]> {
        let [a, b, c, d] = uo_fonts::FontProgram::font_bbox(self);
        Some([f64::from(a), f64::from(b), f64::from(c), f64::from(d)])
    }

    fn ascender(&self) -> Option<f64> {
        Some(f64::from(uo_fonts::FontProgram::ascender(self)))
    }

    fn descender(&self) -> Option<f64> {
        Some(f64::from(uo_fonts::FontProgram::descender(self)))
    }

    fn italic_angle(&self) -> Option<f64> {
        Some(uo_fonts::FontProgram::italic_angle(self))
    }
}

/// The glyph and advance [`EmbeddedFont::encode`] found for a code point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Encoded {
    pub gid: u32,
    /// the advance in font units
    pub advance_width: f64,
}

/// An embedded TrueType font. The document creates it ([`crate::PdfDocument::embed_font`]) and
/// writes its object graph at `build`, with the subset of the glyphs used.
pub struct EmbeddedFont {
    index: u32,
    base_name: Box<str>,
    bytes: Arc<[u8]>,
    program: Box<dyn FontProgram + Send>,
    /// glyph id → the first code point that used it (for ToUnicode)
    used: FxHashMap<u32, u32>,
    scale: f64,
}

impl EmbeddedFont {
    pub(crate) fn new(index: u32, base_name: &str, bytes: Arc<[u8]>, program: Box<dyn FontProgram + Send>) -> Self {
        let scale = 1000.0 / program.units_per_em();
        EmbeddedFont { index, base_name: base_name.into(), bytes, program, used: FxHashMap::default(), scale }
    }

    /// The resource name in the content stream, `F<index>`.
    pub fn resource_name(&self) -> String {
        format!("F{}", self.index)
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    /// The glyphs recorded so far (the ones `/W` and ToUnicode will list).
    pub fn used_glyphs(&self) -> usize {
        self.used.len()
    }

    /// Maps a code point to its glyph and advance and records the use. Identity-H has 2 bytes per
    /// glyph: an id outside 0..=0xFFFF (an invalid font or cmap) becomes `.notdef`.
    pub fn encode(&mut self, code_point: u32) -> Encoded {
        let g0 = self.program.glyph_id(code_point);
        let gid = if g0 <= 0xffff { g0 } else { 0 };
        self.used.entry(gid).or_insert(code_point);
        Encoded { gid, advance_width: self.program.advance_width(gid) }
    }

    /// The advance of a glyph in thousandths of an em (the text space of the PDF), as a JS number.
    pub fn width_thousandths(&self, gid: u32) -> f64 {
        math::round(self.program.advance_width(gid) * self.scale)
    }

    /// `BaseFont` cannot hold spaces: only `[A-Za-z0-9+-]` is kept.
    fn ps_name(&self) -> String {
        self.base_name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-').collect()
    }

    /// The Symbol/Wingdings equivalents the Word writes in ToUnicode, by the font name
    /// (`replace(/^[A-Z]{6}\+/, '')`, `replace(/[\s,]+/g, '')`, then `/^Symbol(?:MT)?(?:-|$)/i` or
    /// `/^Wingdings(?:-|$)/i`; the `i` flag folds ASCII only).
    fn symbol_table(&self) -> Option<&'static [(u32, u32)]> {
        let name = &*self.base_name;
        let b = name.as_bytes();
        let untagged = match (b.get(..6), b.get(6)) {
            (Some(tag), Some(b'+')) if tag.iter().all(u8::is_ascii_uppercase) => name.get(7..)?,
            _ => name,
        };
        let squeezed: String = untagged.chars().filter(|&c| !is_js_space(c) && c != ',').collect();
        let ends = |s: &str| s.is_empty() || s.starts_with('-');
        if let Some(rest) = strip_prefix_ignore_ascii_case(&squeezed, "symbol") {
            let after_mt = strip_prefix_ignore_ascii_case(rest, "mt");
            if ends(rest) || after_mt.is_some_and(ends) {
                return Some(WORD_SYMBOL_UNICODE);
            }
        }
        if let Some(rest) = strip_prefix_ignore_ascii_case(&squeezed, "wingdings")
            && ends(rest)
        {
            return Some(WORD_WINGDINGS_UNICODE);
        }
        None
    }

    /// Writes the object graph of the font (FontFile2 with the subset of the used glyphs, the
    /// descriptor, the CIDFont, ToUnicode, the Type0 font) and returns the Type0 object number.
    pub(crate) fn write<S: PdfSink>(&self, w: &mut PdfWriter<S>, limits: &PdfLimits) -> Result<u32> {
        let name = self.ps_name();
        let um = self.program.units_per_em();
        let asc = math::round(self.program.ascender().unwrap_or(um * 0.8) * self.scale);
        let desc = math::round(self.program.descender().unwrap_or(-um * 0.2) * self.scale);
        let bbox = match self.program.font_bbox() {
            Some(b) => b.map(|v| math::round(v * self.scale)),
            None => [-200.0, desc, 1200.0, asc],
        };
        let italic = self.program.italic_angle().unwrap_or(0.0);
        // 32 = non-symbolic (Latin text), bit 7 (64) when italic
        let flags: u32 = 32 | if italic != 0.0 { 64 } else { 0 };

        // FontFile2: the TrueType program with only the used glyphs (tens of KB instead of ~1 MB);
        // /Length1 is the uncompressed length of the subset
        let mut gids: Vec<u32> = self.used.keys().copied().collect();
        gids.sort_unstable();
        let subset = subset_true_type(&self.bytes, gids.iter().copied(), false, &limits.font);
        let mut dict = String::new();
        let _ = write!(dict, "/Length1 {}", subset.len());
        let font_file = w.add_stream(&dict, &subset, true)?;
        drop(subset);

        dict.clear();
        let _ = write!(
            dict,
            "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {} \
             /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {font_file} 0 R >>",
            Js(bbox[0]),
            Js(bbox[1]),
            Js(bbox[2]),
            Js(bbox[3]),
            Js(italic),
            Js(asc),
            Js(desc),
            Js(asc),
        );
        let descriptor = w.add_dict(&dict)?;

        // /W: the width of each used glyph in thousandths of an em
        dict.clear();
        let _ = write!(
            dict,
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor {descriptor} 0 R /CIDToGIDMap /Identity /DW 1000 /W ["
        );
        for (i, gid) in gids.iter().enumerate() {
            if i > 0 {
                dict.push(' ');
            }
            let _ = write!(dict, "{gid} [{}]", Js(self.width_thousandths(*gid)));
        }
        dict.push_str("] >>");
        let cid_font = w.add_dict(&dict)?;

        let to_unicode = self.build_to_unicode(&gids)?;
        let to_unicode = w.add_stream("", &to_unicode, true)?;

        dict.clear();
        let _ = write!(
            dict,
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
             /DescendantFonts [{cid_font} 0 R] /ToUnicode {to_unicode} 0 R >>"
        );
        w.add_dict(&dict)
    }

    /// The ToUnicode CMap: each CID (= glyph id) used to its code point, in chunks of 100.
    fn build_to_unicode(&self, gids: &[u32]) -> Result<Vec<u8>> {
        let mut b = Vec::new();
        latin1(
            &mut b,
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let table = self.symbol_table();
        let mut line = String::new();
        for chunk in gids.chunks(100) {
            line.clear();
            let _ = writeln!(line, "{} beginbfchar", chunk.len());
            for gid in chunk {
                let cp0 = self.used.get(gid).copied().unwrap_or(0);
                // symbol font (private area F020-F0FF): the Word writes the Unicode equivalent of
                // some codes only (the bullet, the minus, the arrows...); the rest keep the private code
                let sym = table.filter(|_| (0xf020..=0xf0ff).contains(&cp0)).and_then(|t| lookup(t, cp0 - 0xf000));
                let cp = sym.unwrap_or(cp0);
                let hex = if let Ok(unit) = u16::try_from(cp) {
                    utf16be_hex_units([unit])
                } else if cp <= 0x10ffff {
                    let v = cp - 0x10000;
                    utf16be_hex_units([0xd800 + (v >> 10) as u16, 0xdc00 + (v & 0x3ff) as u16])
                } else {
                    // String.fromCodePoint throws a RangeError
                    return Err(PdfError::Invalid("code point past U+10FFFF in a ToUnicode map"));
                };
                let _ = writeln!(line, "<{gid:04x}> <{hex}>");
            }
            line.push_str("endbfchar\n");
            latin1(&mut b, &line);
        }
        latin1(&mut b, "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        Ok(b)
    }
}

fn lookup(table: &[(u32, u32)], key: u32) -> Option<u32> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) { s.get(prefix.len()..) } else { None }
}

/// Codes of the Symbol font (0x20-0xFF) that the Word writes as Unicode in the ToUnicode of its
/// PDFs (measured: the 224 codes through w:sym); the others, Greek letters included, keep the
/// private code F0xx.
const WORD_SYMBOL_UNICODE: &[(u32, u32)] = &[
    (0x28, 0x28),
    (0x29, 0x29),
    (0x2b, 0x2b),
    (0x2d, 0x2212),
    (0x3d, 0x3d),
    (0x5e, 0x22a5),
    (0xae, 0x2192),
    (0xb7, 0x2022),
    (0xbe, 0x23af),
    (0xd3, 0xa9),
    (0xe3, 0xa9),
];

/// The same for Wingdings (Wingdings 2/3 and Webdings: none, everything stays in the private code).
const WORD_WINGDINGS_UNICODE: &[(u32, u32)] = &[
    (0x4a, 0x263a),
    (0x6c, 0x26ab),
    (0x6e, 0x25fc),
    (0x71, 0x2751),
    (0x73, 0x2b27),
    (0x74, 0x29eb),
    (0x75, 0x25c6),
    (0x76, 0x2756),
    (0xa7, 0x25aa),
    (0xd8, 0x27a2),
    (0xe0, 0x2192),
    (0xe8, 0x2794),
    (0xfc, 0x2713),
];
