//! Text of the binary PowerPoint ([MS-PPT] 2.9): paragraph and character format exceptions, the
//! runs of the StyleTextPropAtom, the ruler, the master styles and the PP9 extension (`ppt/text.ts`).
//!
//! Every structure is "mask + optional fields in spec order": each parser returns the number of
//! bytes it consumed. Text is UTF-16 code units (`Vec<u16>`): the TS indexes it by code unit.

use std::rc::Rc;

use super::rd::{Rec, RecRead, children, i16_at, latin1_units, rt, u8_at, u16_at, u32_at, utf16_units};

/// Text or bullet colour: direct RGB (index 0xFE) or a scheme index (0..7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorIndex {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub index: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStop {
    pub pos: i32,
    pub type_: u16,
}

/// Paragraph properties (`TextPFException`); `None` = inherited.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextPF {
    pub has_bullet: Option<bool>,
    pub bullet_has_font: Option<bool>,
    pub bullet_has_color: Option<bool>,
    pub bullet_has_size: Option<bool>,
    pub bullet_char: Option<i32>,
    pub bullet_font: Option<i32>,
    /// 25..400 = % of the text; negative = absolute points (−N)
    pub bullet_size: Option<i32>,
    pub bullet_color: Option<ColorIndex>,
    pub align: Option<i32>,
    pub line_spacing: Option<i32>,
    pub space_before: Option<i32>,
    pub space_after: Option<i32>,
    pub left_margin: Option<i32>,
    pub indent: Option<i32>,
    pub default_tab_size: Option<i32>,
    pub tab_stops: Option<Vec<TabStop>>,
    pub font_align: Option<i32>,
    pub wrap_flags: Option<i32>,
    pub text_direction: Option<i32>,
    pub auto_num_scheme: Option<i32>,
    pub auto_num_start: Option<i32>,
    pub bullet_blip_ref: Option<i32>,
    pub has_auto_num: Option<bool>,
}

/// Character properties (`TextCFException`); `None` = inherited.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextCF {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub shadow: Option<bool>,
    pub emboss: Option<bool>,
    pub font: Option<i32>,
    pub ea_font: Option<i32>,
    pub ansi_font: Option<i32>,
    pub symbol_font: Option<i32>,
    pub size: Option<i32>,
    pub color: Option<ColorIndex>,
    pub position: Option<i32>,
    pub pp9rt: Option<i32>,
}

macro_rules! overlay {
    ($dst:expr, $src:expr; $($f:ident),* $(,)?) => {
        $( if $src.$f.is_some() { $dst.$f = $src.$f.clone(); } )*
    };
}

impl TextPF {
    /// `Object.assign(self, o)`: the fields `o` has override.
    pub fn assign(&mut self, o: &TextPF) {
        overlay!(self, o; has_bullet, bullet_has_font, bullet_has_color, bullet_has_size, bullet_char, bullet_font, bullet_size,
            bullet_color, align, line_spacing, space_before, space_after, left_margin, indent, default_tab_size, tab_stops,
            font_align, wrap_flags, text_direction, auto_num_scheme, auto_num_start, bullet_blip_ref, has_auto_num);
    }
}

impl TextCF {
    /// `Object.assign(self, o)`.
    pub fn assign(&mut self, o: &TextCF) {
        overlay!(self, o; bold, italic, underline, shadow, emboss, font, ea_font, ansi_font, symbol_font, size, color, position, pp9rt);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PFRun {
    pub count: u32,
    pub indent_level: u16,
    pub pf: TextPF,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CFRun {
    pub count: u32,
    pub cf: TextCF,
}

/// Text ruler (`TextRulerAtom`): margins per level (master units).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextRuler {
    pub default_tab_size: Option<i32>,
    pub tab_stops: Option<Vec<TabStop>>,
    pub left_margin: [Option<i32>; 5],
    pub indent: [Option<i32>; 5],
}

/// A field (slide number, date…) at a character position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaChar {
    pub pos: u32,
    pub rec_type: u16,
}

/// A click action on a text range `[start, end)` in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLink {
    pub start: u32,
    pub end: u32,
    /// `InteractiveInfoAtom.exHyperlinkIdRef`
    pub hyperlink_id: u32,
    /// 4 = hyperlink; 3 = jump
    pub action: u8,
    pub jump: u8,
}

/// The text block of a shape (ClientTextbox) or of the SlideListWithText. The big parts are
/// shared (`Rc`): the TS copies a block with an object spread, which shares them.
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// `TextHeaderAtom.textType`
    pub text_type: u32,
    pub text: Rc<Vec<u16>>,
    pub pf_runs: Rc<Vec<PFRun>>,
    pub cf_runs: Rc<Vec<CFRun>>,
    pub ruler: Option<Rc<TextRuler>>,
    /// PP9 exceptions per paragraph
    pub pf9: Option<Rc<Vec<TextPF>>>,
    pub meta_chars: Rc<Vec<MetaChar>>,
    pub links: Rc<Vec<TextLink>>,
}

/// One level of a master style (`TextMasterStyleLevel`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MasterStyleLevel {
    pub level: Option<u16>,
    pub pf: TextPF,
    pub cf: TextCF,
}

fn read_color_index(d: &[u8], o: u64) -> ColorIndex {
    ColorIndex { r: u8_at(d, o), g: u8_at(d, o + 1), b: u8_at(d, o + 2), index: u8_at(d, o + 3) }
}

/// Most tab stops accepted (corrupt counts would multiply the output).
const MAX_TABS: u64 = 64;

fn read_tab_stops(d: &[u8], o: u64) -> (Vec<TabStop>, u64) {
    let n = u64::from(u16_at(d, o));
    let mut tabs = Vec::new();
    for i in 0..n.min(MAX_TABS) {
        tabs.push(TabStop { pos: i32::from(i16_at(d, o + 2 + i * 4)), type_: u16_at(d, o + 4 + i * 4) });
    }
    (tabs, 2 + n * 4)
}

/// `TextPFException` from `o` (the mask already read in `masks`): the properties and the bytes used.
#[must_use]
pub fn read_pf_fields(d: &[u8], o: u64, masks: u32) -> (TextPF, u64) {
    let start = o;
    let mut o = o;
    let mut pf = TextPF::default();
    if masks & 0x0f != 0 {
        let f = u16_at(d, o);
        o += 2;
        if masks & 0x01 != 0 {
            pf.has_bullet = Some(f & 1 != 0);
        }
        if masks & 0x02 != 0 {
            pf.bullet_has_font = Some(f & 2 != 0);
        }
        if masks & 0x04 != 0 {
            pf.bullet_has_color = Some(f & 4 != 0);
        }
        if masks & 0x08 != 0 {
            pf.bullet_has_size = Some(f & 8 != 0);
        }
    }
    if masks & 0x80 != 0 {
        pf.bullet_char = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x10 != 0 {
        pf.bullet_font = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x40 != 0 {
        pf.bullet_size = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x20 != 0 {
        pf.bullet_color = Some(read_color_index(d, o));
        o += 4;
    }
    if masks & 0x800 != 0 {
        pf.align = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x1000 != 0 {
        pf.line_spacing = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x2000 != 0 {
        pf.space_before = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x4000 != 0 {
        pf.space_after = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x100 != 0 {
        pf.left_margin = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x400 != 0 {
        pf.indent = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x8000 != 0 {
        pf.default_tab_size = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x100000 != 0 {
        let (tabs, size) = read_tab_stops(d, o);
        pf.tab_stops = Some(tabs);
        o += size;
    }
    if masks & 0x10000 != 0 {
        pf.font_align = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0xe0000 != 0 {
        pf.wrap_flags = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x200000 != 0 {
        pf.text_direction = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    (pf, o - start)
}

/// The CFMasks bits that need the `fontStyle` field.
const CF_STYLE_BITS: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0010 | 0x0020 | 0x0080 | 0x0200 | 0x3c00;

/// `TextCFException` from `o` (the mask already read in `masks`): the properties and the bytes used.
#[must_use]
pub fn read_cf_fields(d: &[u8], o: u64, masks: u32) -> (TextCF, u64) {
    let start = o;
    let mut o = o;
    let mut cf = TextCF::default();
    if masks & CF_STYLE_BITS != 0 {
        let s = u16_at(d, o);
        o += 2;
        if masks & 0x0001 != 0 {
            cf.bold = Some(s & 0x0001 != 0);
        }
        if masks & 0x0002 != 0 {
            cf.italic = Some(s & 0x0002 != 0);
        }
        if masks & 0x0004 != 0 {
            cf.underline = Some(s & 0x0004 != 0);
        }
        if masks & 0x0010 != 0 {
            cf.shadow = Some(s & 0x0010 != 0);
        }
        if masks & 0x0200 != 0 {
            cf.emboss = Some(s & 0x0200 != 0);
        }
        if masks & 0x3c00 != 0 {
            cf.pp9rt = Some(i32::from((s >> 10) & 0xf));
        }
    }
    if masks & 0x10000 != 0 {
        cf.font = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x200000 != 0 {
        cf.ea_font = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x400000 != 0 {
        cf.ansi_font = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x800000 != 0 {
        cf.symbol_font = Some(i32::from(u16_at(d, o)));
        o += 2;
    }
    if masks & 0x20000 != 0 {
        cf.size = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x40000 != 0 {
        cf.color = Some(read_color_index(d, o));
        o += 4;
    }
    if masks & 0x80000 != 0 {
        cf.position = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    (cf, o - start)
}

/// The runs of a StyleTextPropAtom: paragraph runs covering `text.length + 1` (the implicit final
/// mark) followed by the character runs.
#[must_use]
pub fn read_style_text_prop(rec: &Rec<'_>, text_len: u64) -> (Vec<PFRun>, Vec<CFRun>) {
    let d = rec.data;
    let mut o = rec.start;
    let end = rec.end;
    let mut pf = Vec::new();
    let mut cf = Vec::new();
    let mut covered = 0u64;
    while covered < text_len + 1 && o + 10 <= end {
        let count = u32_at(d, o);
        let indent_level = u16_at(d, o + 4);
        let masks = u32_at(d, o + 6);
        let (p, size) = read_pf_fields(d, o + 10, masks);
        o += 10 + size;
        pf.push(PFRun { count, indent_level, pf: p });
        covered += u64::from(count);
        if count == 0 {
            break;
        }
    }
    covered = 0;
    while covered < text_len + 1 && o + 8 <= end {
        let count = u32_at(d, o);
        let masks = u32_at(d, o + 4);
        let (c, size) = read_cf_fields(d, o + 8, masks);
        o += 8 + size;
        cf.push(CFRun { count, cf: c });
        covered += u64::from(count);
        if count == 0 {
            break;
        }
    }
    (pf, cf)
}

/// `TextRulerAtom` → ruler.
#[must_use]
pub fn read_text_ruler(rec: &Rec<'_>) -> TextRuler {
    let d = rec.data;
    let mut o = rec.start;
    let masks = u32_at(d, o);
    o += 4;
    let mut ruler = TextRuler::default();
    if masks & 0x2 != 0 {
        o += 2;
    }
    if masks & 0x1 != 0 {
        ruler.default_tab_size = Some(i32::from(i16_at(d, o)));
        o += 2;
    }
    if masks & 0x4 != 0 {
        let (tabs, size) = read_tab_stops(d, o);
        ruler.tab_stops = Some(tabs);
        o += size;
    }
    for lvl in 0..5usize {
        if masks & (0x8 << lvl) != 0 {
            if let Some(slot) = ruler.left_margin.get_mut(lvl) {
                *slot = Some(i32::from(i16_at(d, o)));
            }
            o += 2;
        }
        if masks & (0x100 << lvl) != 0 {
            if let Some(slot) = ruler.indent.get_mut(lvl) {
                *slot = Some(i32::from(i16_at(d, o)));
            }
            o += 2;
        }
    }
    ruler
}

/// `TextMasterStyleAtom` → levels (pf + cf). Instances ≥ 5 carry the level number before each.
#[must_use]
pub fn read_text_master_style(rec: &Rec<'_>) -> Vec<MasterStyleLevel> {
    let d = rec.data;
    let mut o = rec.start;
    let n = u16_at(d, o);
    o += 2;
    let mut out = Vec::new();
    let mut i = 0u16;
    while i < n && o < rec.end {
        let mut level = None;
        if rec.inst >= 5 {
            level = Some(u16_at(d, o));
            o += 2;
        }
        let pm = u32_at(d, o);
        o += 4;
        let (pf, psize) = read_pf_fields(d, o, pm);
        o += psize;
        let cm = u32_at(d, o);
        o += 4;
        let (cf, csize) = read_cf_fields(d, o, cm);
        o += csize;
        out.push(MasterStyleLevel { level, pf, cf });
        i += 1;
    }
    out
}

/// `TextCFExceptionAtom` (the document's character defaults).
#[must_use]
pub fn read_cf_exception_atom(rec: &Rec<'_>) -> TextCF {
    read_cf_fields(rec.data, rec.start + 4, rec.u32(rec.start)).0
}

/// `TextPFExceptionAtom` (the document's paragraph defaults): 2 reserved bytes + the exception.
#[must_use]
pub fn read_pf_exception_atom(rec: &Rec<'_>) -> TextPF {
    read_pf_fields(rec.data, rec.start + 6, rec.u32(rec.start + 2)).0
}

/// `StyleTextProp9Atom` (PP9 extension): per paragraph, pf9 + cf9 + si; of the pf9 only the
/// automatic numbering and the picture bullet matter.
#[must_use]
pub fn read_style_text_prop9(rec: &Rec<'_>) -> Vec<TextPF> {
    let d = rec.data;
    let mut o = rec.start;
    let mut out = Vec::new();
    while o + 12 <= rec.end {
        let pm = u32_at(d, o);
        o += 4;
        let mut pf = TextPF::default();
        if pm & 0x80_0000 != 0 {
            pf.bullet_blip_ref = Some(i32::from(i16_at(d, o)));
            o += 2;
        }
        if pm & 0x200_0000 != 0 {
            pf.has_auto_num = Some(u16_at(d, o) != 0);
            o += 2;
        }
        if pm & 0x100_0000 != 0 {
            pf.auto_num_scheme = Some(i32::from(u16_at(d, o)));
            pf.auto_num_start = Some(i32::from(i16_at(d, o + 2)));
            o += 4;
        }
        let cm = u32_at(d, o);
        o += 4;
        // TextCFException9: pp10runid/unused (4 bits + 28) only with the pp10ext bit
        if cm & 0x10_0000 != 0 {
            o += 4;
        }
        let sm = u32_at(d, o);
        o += 4;
        o += text_si_exception_size(d, o, sm);
        out.push(pf);
    }
    out
}

/// Size of the fields of a `TextSIException` (after the mask), in spec order.
fn text_si_exception_size(d: &[u8], o: u64, masks: u32) -> u64 {
    let mut n = 0u64;
    if masks & 0x1 != 0 {
        n += 2;
    }
    if masks & 0x2 != 0 {
        n += 2;
    }
    if masks & 0x4 != 0 {
        n += 2;
    }
    if masks & 0x40 != 0 {
        n += 2;
    }
    if masks & 0x20 != 0 {
        n += 4;
    }
    if masks & 0x200 != 0 {
        n += 4 + 4 * u64::from(u32_at(d, o + n));
    }
    n
}

/// A text block from the consecutive text records (children of a ClientTextbox or a stretch of
/// the SlideListWithText); `recs` starts at the TextHeaderAtom.
#[must_use]
pub fn read_text_block(recs: &[Rec<'_>]) -> Option<TextBlock> {
    let header = recs.first()?;
    if header.rec_type != rt::TEXT_HEADER_ATOM {
        return None;
    }
    let text_type = header.u32(header.start);
    let mut text: Vec<u16> = Vec::new();
    let mut ruler = None;
    let mut meta_chars = Vec::new();
    let mut links = Vec::new();
    let mut style: Option<Rec<'_>> = None;
    // (hyperlinkId, action, jump) of the last InteractiveInfo
    let mut last_action: Option<(u32, u8, u8)> = None;
    for rec in recs.iter().skip(1) {
        match rec.rec_type {
            rt::INTERACTIVE_INFO => {
                let a = children(rec).into_iter().find(|k| k.rec_type == rt::INTERACTIVE_INFO_ATOM);
                last_action = match a {
                    Some(a) if rec.inst == 0 => Some((a.u32(a.start + 4), a.u8(a.start + 8), a.u8(a.start + 10))),
                    _ => None,
                };
            }
            rt::TEXT_INTERACTIVE_INFO_ATOM => {
                if rec.inst == 0
                    && let Some((hyperlink_id, action, jump)) = last_action
                {
                    links.push(TextLink { start: rec.u32(rec.start), end: rec.u32(rec.start + 4), hyperlink_id, action, jump });
                }
                last_action = None;
            }
            rt::TEXT_CHARS_ATOM => text = utf16_units(rec.data, rec.start, rec.end),
            rt::TEXT_BYTES_ATOM => text = latin1_units(rec.data, rec.start, rec.end),
            rt::STYLE_TEXT_PROP_ATOM => style = Some(*rec),
            rt::TEXT_RULER_ATOM => ruler = Some(Rc::new(read_text_ruler(rec))),
            rt::SLIDE_NUMBER_META_CHAR_ATOM
            | rt::DATE_TIME_META_CHAR_ATOM
            | rt::GENERIC_DATE_META_CHAR_ATOM
            | rt::HEADER_META_CHAR_ATOM
            | rt::FOOTER_META_CHAR_ATOM
            | rt::RTF_DATE_TIME_META_CHAR_ATOM => meta_chars.push(MetaChar { pos: rec.u32(rec.start), rec_type: rec.rec_type }),
            _ => {}
        }
    }
    let (pf_runs, cf_runs) = match style {
        Some(st) => read_style_text_prop(&st, text.len() as u64),
        None => (Vec::new(), Vec::new()),
    };
    Some(TextBlock {
        text_type,
        text: Rc::new(text),
        pf_runs: Rc::new(pf_runs),
        cf_runs: Rc::new(cf_runs),
        ruler,
        pf9: None,
        meta_chars: Rc::new(meta_chars),
        links: Rc::new(links),
    })
}

/// Splits a list of records into blocks that start at a TextHeaderAtom (`splitTextBlocks`).
#[must_use]
pub fn split_text_blocks(recs: &[Rec<'_>]) -> Vec<TextBlock> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<Rec<'_>>> = None;
    for rec in recs {
        if rec.rec_type == rt::TEXT_HEADER_ATOM {
            if let Some(c) = cur.take()
                && let Some(b) = read_text_block(&c)
            {
                out.push(b);
            }
            cur = Some(vec![*rec]);
        } else if let Some(c) = cur.as_mut() {
            c.push(*rec);
        }
    }
    if let Some(c) = cur
        && let Some(b) = read_text_block(&c)
    {
        out.push(b);
    }
    out
}

/// What a ClientTextbox holds: a block of its own, or a reference to the outline (with the ruler
/// the shape may carry).
#[derive(Debug, Clone, Default)]
pub struct ClientTextbox {
    pub block: Option<TextBlock>,
    pub outline_ref: Option<u32>,
    pub ruler: Option<TextRuler>,
}

/// The text children of a ClientTextbox → block (or reference to the outline).
#[must_use]
pub fn read_client_textbox(rec: &Rec<'_>) -> ClientTextbox {
    let kids = children(rec);
    if let Some(r) = kids.iter().find(|k| k.rec_type == rt::OUTLINE_TEXT_REF_ATOM) {
        let outline_ref = Some(r.u32(r.start));
        let ruler = kids.iter().find(|k| k.rec_type == rt::TEXT_RULER_ATOM).map(read_text_ruler);
        return ClientTextbox { block: None, outline_ref, ruler };
    }
    ClientTextbox { block: split_text_blocks(&kids).into_iter().next(), outline_ref: None, ruler: None }
}
