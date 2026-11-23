//! Cell styles: what a caller describes ([`Style`]) and the deduplicated pools of `styles.xml`
//! (number formats, fonts, fills, borders, cell formats).
//!
//! A [`Style`] is interned once ([`StylePool::intern`]) and referenced by its [`StyleId`]. The
//! cell format (`xf`) is resolved when a cell is written, because two things are only known then:
//! the direction of the sheet (`Start`/`End` become `left`/`right`) and the implied date format
//! of a date cell whose style has none. Equal resolved formats share one `xf`.

use std::collections::HashMap;

use uo_opc::xml::write::{Escape, escape_into};

use crate::cell::push_u32;
use crate::error::{Error, Result};
use crate::{MAX_FONTS, MAX_XFS};

/// An sRGB colour `0xRRGGBB` (written opaque, `FFRRGGBB`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u32);

impl Rgb {
    fn push(self, out: &mut String) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        out.push_str("FF");
        for shift in [20, 16, 12, 8, 4, 0] {
            out.push(char::from(HEX[((self.0 >> shift) & 0xF) as usize]));
        }
    }
}

/// The line of a border edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    Thin,
    Medium,
}

/// One side of a cell border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorderEdge {
    pub style: BorderStyle,
    /// `None`: automatic (black)
    pub color: Option<Rgb>,
}

/// The four sides of a cell border (`None`: no line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Borders {
    pub left: Option<BorderEdge>,
    pub right: Option<BorderEdge>,
    pub top: Option<BorderEdge>,
    pub bottom: Option<BorderEdge>,
}

impl Borders {
    /// The same edge on the four sides.
    pub fn all(edge: BorderEdge) -> Self {
        Borders { left: Some(edge), right: Some(edge), top: Some(edge), bottom: Some(edge) }
    }
}

/// Horizontal alignment. `Start`/`End` follow the sheet direction (left/right in a left-to-right
/// sheet, right/left in a right-to-left one); `CenterContinuous` is Excel's "Center Across
/// Selection" (the text centres over the empty cells to its right, without merging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HAlign {
    Left,
    Center,
    Right,
    Start,
    End,
    CenterContinuous,
}

/// What a cell looks like. `Style::default()` is Excel's Normal style.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Style {
    /// number format code (`"0.00%"`, `"dd/mm/yyyy"`, `"[Red]0;[Blue]-0"`…); `None` = General
    /// (date cells then get a date format, see [`crate::CellValue::Date`])
    pub num_fmt: Option<String>,
    pub bold: bool,
    pub italic: bool,
    /// text colour; `None` = automatic
    pub color: Option<Rgb>,
    /// solid background; `None` = no fill
    pub fill: Option<Rgb>,
    pub border: Borders,
    /// `None` = General (text to the start, numbers to the end, as Excel decides)
    pub align: Option<HAlign>,
    /// wrap text
    pub wrap: bool,
}

/// A style interned by [`crate::XlsxWriter::add_style`]. `StyleId::DEFAULT` (0) is Normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleId(pub u32);

impl StyleId {
    pub const DEFAULT: StyleId = StyleId(0);
}

/// The built-in formats whose code is the same in every locale (a custom code equal to one of
/// them is written as the id; the locale-dependent ones, 5-8, 14-22, 37-44, are not).
const INVARIANT_BUILTINS: [u32; 15] = [0, 1, 2, 3, 4, 9, 10, 11, 12, 13, 45, 46, 47, 48, 49];
/// Built-in 14, the short date of the reader's locale (`m/d/yyyy` in en-US, `dd/mm/yyyy` in pt-BR).
pub(crate) const BUILTIN_SHORT_DATE: u32 = 14;
/// Built-in 22, the short date and time of the reader's locale.
pub(crate) const BUILTIN_DATE_TIME: u32 = 22;
const FIRST_CUSTOM_NUMFMT: u32 = 164;
const MAX_NUMFMT_LEN: usize = 255;

/// Physical horizontal alignment as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Horizontal {
    Left,
    Center,
    Right,
    CenterContinuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Font {
    bold: bool,
    italic: bool,
    color: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Xf {
    num_fmt: u32,
    font: u32,
    fill: u32,
    border: u32,
    horizontal: Option<Horizontal>,
    wrap: bool,
}

/// What an interned style resolves to, before the direction and date defaults.
#[derive(Debug, Clone, Copy)]
struct Interned {
    num_fmt: Option<u32>,
    font: u32,
    fill: u32,
    border: u32,
    align: Option<HAlign>,
    wrap: bool,
}

/// The deduplicated pools of `styles.xml`.
pub(crate) struct StylePool {
    styles: HashMap<Style, StyleId>,
    interned: Vec<Interned>,
    num_fmts: Vec<String>,
    num_fmt_ids: HashMap<String, u32>,
    fonts: Vec<Font>,
    font_ids: HashMap<Font, u32>,
    fills: Vec<Rgb>,
    fill_ids: HashMap<Rgb, u32>,
    borders: Vec<Borders>,
    border_ids: HashMap<Borders, u32>,
    xfs: Vec<Xf>,
    xf_ids: HashMap<Xf, u32>,
    /// (style, right-to-left, implied date format) → xf
    resolved: HashMap<(StyleId, bool, u32), u32>,
}

impl StylePool {
    pub(crate) fn new() -> Self {
        let mut p = StylePool {
            styles: HashMap::new(),
            interned: Vec::new(),
            num_fmts: Vec::new(),
            num_fmt_ids: HashMap::new(),
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            fills: Vec::new(),
            fill_ids: HashMap::new(),
            borders: Vec::new(),
            border_ids: HashMap::new(),
            xfs: Vec::new(),
            xf_ids: HashMap::new(),
            resolved: HashMap::new(),
        };
        // the Normal font, no border, xf 0 = Normal; style 0 = Style::default()
        let normal = Font { bold: false, italic: false, color: None };
        p.fonts.push(normal);
        p.font_ids.insert(normal, 0);
        p.borders.push(Borders::default());
        p.border_ids.insert(Borders::default(), 0);
        let xf0 = Xf { num_fmt: 0, font: 0, fill: 0, border: 0, horizontal: None, wrap: false };
        p.xfs.push(xf0);
        p.xf_ids.insert(xf0, 0);
        p.styles.insert(Style::default(), StyleId::DEFAULT);
        p.interned.push(Interned { num_fmt: None, font: 0, fill: 0, border: 0, align: None, wrap: false });
        p
    }

    pub(crate) fn intern(&mut self, s: &Style) -> Result<StyleId> {
        if let Some(&id) = self.styles.get(s) {
            return Ok(id);
        }
        // validate everything before touching a pool
        let num_fmt = match &s.num_fmt {
            None => None,
            Some(code) => Some(self.num_fmt_id(code)?),
        };
        let font = Font { bold: s.bold, italic: s.italic, color: s.color };
        if !self.font_ids.contains_key(&font) && self.fonts.len() >= MAX_FONTS {
            return Err(Error::TooManyStyles);
        }
        let num_fmt = num_fmt.map(|n| n.unwrap_or_else(|code| self.add_num_fmt(code)));
        let font = *self.font_ids.entry(font).or_insert_with(|| {
            self.fonts.push(font);
            (self.fonts.len() - 1) as u32
        });
        let fill = match s.fill {
            None => 0,
            Some(rgb) => *self.fill_ids.entry(rgb).or_insert_with(|| {
                self.fills.push(rgb);
                // 0 = none and 1 = gray125 are the two fills every styles.xml starts with
                (self.fills.len() + 1) as u32
            }),
        };
        let border = *self.border_ids.entry(s.border).or_insert_with(|| {
            self.borders.push(s.border);
            (self.borders.len() - 1) as u32
        });
        let id = StyleId(self.interned.len() as u32);
        self.interned.push(Interned { num_fmt, font, fill, border, align: s.align, wrap: s.wrap });
        self.styles.insert(s.clone(), id);
        Ok(id)
    }

    /// `Ok(Ok(id))` for a built-in or known code, `Ok(Err(code))` for a new custom code.
    fn num_fmt_id<'a>(&self, code: &'a str) -> Result<std::result::Result<u32, &'a str>> {
        let units = code.encode_utf16().count();
        if units == 0 || units > MAX_NUMFMT_LEN {
            return Err(Error::NumFmt);
        }
        if code.eq_ignore_ascii_case("general") {
            return Ok(Ok(0));
        }
        if let Some(&id) = INVARIANT_BUILTINS.iter().find(|&&id| uo_numfmt::builtin_format(id) == Some(code)) {
            return Ok(Ok(id));
        }
        Ok(self.num_fmt_ids.get(code).copied().ok_or(code))
    }

    fn add_num_fmt(&mut self, code: &str) -> u32 {
        let id = FIRST_CUSTOM_NUMFMT + self.num_fmts.len() as u32;
        self.num_fmts.push(code.to_owned());
        self.num_fmt_ids.insert(code.to_owned(), id);
        id
    }

    pub(crate) fn contains(&self, id: StyleId) -> bool {
        (id.0 as usize) < self.interned.len()
    }

    /// Whether the style sets a number format (a date cell without one gets a date format).
    pub(crate) fn has_num_fmt(&self, id: StyleId) -> bool {
        self.interned.get(id.0 as usize).is_some_and(|s| s.num_fmt.is_some())
    }

    /// The `s` attribute of a cell: the xf of `id` in a sheet of this direction, with
    /// `implied_num_fmt` when the style sets none (0 = General).
    pub(crate) fn xf(&mut self, id: StyleId, rtl: bool, implied_num_fmt: u32) -> Result<u32> {
        let key = (id, rtl, implied_num_fmt);
        if let Some(&xf) = self.resolved.get(&key) {
            return Ok(xf);
        }
        let Some(s) = self.interned.get(id.0 as usize).copied() else { return Err(Error::UnknownStyle(id.0)) };
        let horizontal = s.align.map(|a| match a {
            HAlign::Left => Horizontal::Left,
            HAlign::Right => Horizontal::Right,
            HAlign::Center => Horizontal::Center,
            HAlign::CenterContinuous => Horizontal::CenterContinuous,
            HAlign::Start if rtl => Horizontal::Right,
            HAlign::Start => Horizontal::Left,
            HAlign::End if rtl => Horizontal::Left,
            HAlign::End => Horizontal::Right,
        });
        let xf = Xf { num_fmt: s.num_fmt.unwrap_or(implied_num_fmt), font: s.font, fill: s.fill, border: s.border, horizontal, wrap: s.wrap };
        let index = match self.xf_ids.get(&xf) {
            Some(&i) => i,
            None => {
                if self.xfs.len() >= MAX_XFS {
                    return Err(Error::TooManyStyles);
                }
                self.xfs.push(xf);
                let i = (self.xfs.len() - 1) as u32;
                self.xf_ids.insert(xf, i);
                i
            }
        };
        self.resolved.insert(key, index);
        Ok(index)
    }

    /// `xl/styles.xml` with the Normal font `font_name`/`font_size`.
    pub(crate) fn xml(&self, font_name: &str, font_size: &str) -> String {
        let mut o = String::with_capacity(1024 + 64 * self.xfs.len());
        o.push_str(uo_opc::xml::write::XML_DECL_BARE);
        o.push_str("<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">");
        if !self.num_fmts.is_empty() {
            o.push_str("<numFmts count=\"");
            push_u32(&mut o, self.num_fmts.len() as u32);
            o.push_str("\">");
            for (i, code) in self.num_fmts.iter().enumerate() {
                o.push_str("<numFmt numFmtId=\"");
                push_u32(&mut o, FIRST_CUSTOM_NUMFMT + i as u32);
                o.push_str("\" formatCode=\"");
                escape_into(&mut o, code, Escape::ATTR_VALID);
                o.push_str("\"/>");
            }
            o.push_str("</numFmts>");
        }
        o.push_str("<fonts count=\"");
        push_u32(&mut o, self.fonts.len() as u32);
        o.push_str("\">");
        for f in &self.fonts {
            o.push_str("<font>");
            if f.bold {
                o.push_str("<b/>");
            }
            if f.italic {
                o.push_str("<i/>");
            }
            o.push_str("<sz val=\"");
            o.push_str(font_size);
            o.push_str("\"/>");
            if let Some(c) = f.color {
                o.push_str("<color rgb=\"");
                c.push(&mut o);
                o.push_str("\"/>");
            }
            o.push_str("<name val=\"");
            escape_into(&mut o, font_name, Escape::ATTR_VALID);
            o.push_str("\"/><family val=\"2\"/></font>");
        }
        o.push_str("</fonts><fills count=\"");
        push_u32(&mut o, self.fills.len() as u32 + 2);
        o.push_str("\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>");
        for c in &self.fills {
            o.push_str("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"");
            c.push(&mut o);
            o.push_str("\"/><bgColor indexed=\"64\"/></patternFill></fill>");
        }
        o.push_str("</fills><borders count=\"");
        push_u32(&mut o, self.borders.len() as u32);
        o.push_str("\">");
        for b in &self.borders {
            o.push_str("<border>");
            for (name, edge) in [("left", b.left), ("right", b.right), ("top", b.top), ("bottom", b.bottom)] {
                o.push('<');
                o.push_str(name);
                match edge {
                    None => o.push_str("/>"),
                    Some(e) => {
                        o.push_str(match e.style {
                            BorderStyle::Thin => " style=\"thin\">",
                            BorderStyle::Medium => " style=\"medium\">",
                        });
                        match e.color {
                            Some(c) => {
                                o.push_str("<color rgb=\"");
                                c.push(&mut o);
                                o.push_str("\"/>");
                            }
                            None => o.push_str("<color indexed=\"64\"/>"),
                        }
                        o.push_str("</");
                        o.push_str(name);
                        o.push('>');
                    }
                }
            }
            o.push_str("<diagonal/></border>");
        }
        o.push_str("</borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
        o.push_str("<cellXfs count=\"");
        push_u32(&mut o, self.xfs.len() as u32);
        o.push_str("\">");
        for x in &self.xfs {
            o.push_str("<xf numFmtId=\"");
            push_u32(&mut o, x.num_fmt);
            o.push_str("\" fontId=\"");
            push_u32(&mut o, x.font);
            o.push_str("\" fillId=\"");
            push_u32(&mut o, x.fill);
            o.push_str("\" borderId=\"");
            push_u32(&mut o, x.border);
            o.push_str("\" xfId=\"0\"");
            if x.num_fmt != 0 {
                o.push_str(" applyNumberFormat=\"1\"");
            }
            if x.font != 0 {
                o.push_str(" applyFont=\"1\"");
            }
            if x.fill != 0 {
                o.push_str(" applyFill=\"1\"");
            }
            if x.border != 0 {
                o.push_str(" applyBorder=\"1\"");
            }
            if x.horizontal.is_none() && !x.wrap {
                o.push_str("/>");
                continue;
            }
            o.push_str(" applyAlignment=\"1\"><alignment");
            if let Some(h) = x.horizontal {
                o.push_str(match h {
                    Horizontal::Left => " horizontal=\"left\"",
                    Horizontal::Center => " horizontal=\"center\"",
                    Horizontal::Right => " horizontal=\"right\"",
                    Horizontal::CenterContinuous => " horizontal=\"centerContinuous\"",
                });
            }
            if x.wrap {
                o.push_str(" wrapText=\"1\"");
            }
            o.push_str("/></xf>");
        }
        o.push_str("</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>");
        o.push_str("<dxfs count=\"0\"/><tableStyles count=\"0\"/></styleSheet>");
        o
    }

    #[cfg(test)]
    pub(crate) fn counts(&self) -> [usize; 5] {
        [self.num_fmts.len(), self.fonts.len(), self.fills.len(), self.borders.len(), self.xfs.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup() {
        let mut p = StylePool::new();
        assert_eq!(p.intern(&Style::default()).unwrap(), StyleId::DEFAULT);
        let red = Style { fill: Some(Rgb(0xFF0000)), ..Style::default() };
        let a = p.intern(&red).unwrap();
        assert_eq!(p.intern(&red.clone()).unwrap(), a);
        let bold_red = Style { bold: true, ..red.clone() };
        let b = p.intern(&bold_red).unwrap();
        assert_ne!(a, b);
        // same fill shared, new font
        assert_eq!(p.counts(), [0, 2, 1, 1, 1]);
        let xa = p.xf(a, false, 0).unwrap();
        let xb = p.xf(b, false, 0).unwrap();
        assert_eq!((xa, xb), (1, 2));
        assert_eq!(p.xf(a, true, 0).unwrap(), xa, "no direction-dependent alignment: one xf");
        // Start resolves by direction; equal physical alignments share an xf
        let start = p.intern(&Style { align: Some(HAlign::Start), ..Style::default() }).unwrap();
        let right = p.intern(&Style { align: Some(HAlign::Right), ..Style::default() }).unwrap();
        assert_eq!(p.xf(start, true, 0).unwrap(), p.xf(right, false, 0).unwrap());
        assert_ne!(p.xf(start, false, 0).unwrap(), p.xf(start, true, 0).unwrap());
    }

    #[test]
    fn num_fmts() {
        let mut p = StylePool::new();
        let f = |p: &mut StylePool, c: &str| p.intern(&Style { num_fmt: Some(c.into()), ..Style::default() });
        let pct = f(&mut p, "0.00%").unwrap();
        assert_eq!(p.interned[pct.0 as usize].num_fmt, Some(10));
        let general = f(&mut p, "General").unwrap();
        assert_eq!(p.interned[general.0 as usize].num_fmt, Some(0));
        // locale-dependent built-in code: custom
        let d = f(&mut p, "m/d/yyyy").unwrap();
        assert_eq!(p.interned[d.0 as usize].num_fmt, Some(164));
        let d2 = f(&mut p, "dd/mm/yyyy").unwrap();
        assert_eq!(p.interned[d2.0 as usize].num_fmt, Some(165));
        let again = p.intern(&Style { num_fmt: Some("dd/mm/yyyy".into()), bold: true, ..Style::default() }).unwrap();
        assert_eq!(p.interned[again.0 as usize].num_fmt, Some(165));
        assert_eq!(f(&mut p, ""), Err(Error::NumFmt));
        assert_eq!(f(&mut p, &"0".repeat(256)), Err(Error::NumFmt));
        assert!(f(&mut p, &"0".repeat(255)).is_ok());
    }

    #[test]
    fn unknown_style() {
        let mut p = StylePool::new();
        assert_eq!(p.xf(StyleId(9), false, 0), Err(Error::UnknownStyle(9)));
    }
}
