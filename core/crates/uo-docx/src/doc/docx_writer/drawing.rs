//! Drawings: inline pictures, floating shapes (anchors, `wps:wsp`, `wpg:grpSp`, `pic:pic`), fills,
//! lines and arrows, WordArt, the page background. The OfficeArt layer is `uo_drawingml::odraw`.

use uo_drawingml::odraw::msospt::{PresetVariant, is_connector_type, is_word_art_type, preset_for, raw_adjust};
use uo_drawingml::odraw::{Shape, fsp};
use uo_jsnum::conv::to_int32;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{max, min, rem, round};

use super::{DocxWriter, Kind, PartRels, Story};
use crate::doc::bytes::ByteReader;
use crate::doc::numeric::int_of;
use crate::doc::ooxml_props::hex;
use crate::doc::pictures::{Crop, Hr, PicFx, PicInfo, pic_fx_of, shape_blip};
use crate::doc::reader::Fspa;
use crate::doc::xmlw::{AttrVal, el_of, esc_text_units};
use crate::doc::{DocError, DocLimitKind};

const EMU_PER_TWIP: i64 = 635;

/// A box in EMU (`{ x, y, cx, cy }`).
#[derive(Debug, Clone, Copy)]
struct BoxEmu {
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
}

fn int(v: i64) -> Option<AttrVal<'static>> {
    Some(AttrVal::Int(v))
}

fn num(v: f64) -> Option<AttrVal<'static>> {
    Some(AttrVal::Num(v))
}

/// `oaFlag`: a Boolean of OfficeArt (the sets 0x?3F/0x?7F/0x?BF/0x?FF): the bit `bit` counts only when
/// its `fUse*` (bit + 16) is on; else `None` (the default).
fn oa_flag(v: Option<u32>, bit: u32) -> Option<bool> {
    let v = v?;
    if (v >> (bit + 16)) & 1 == 0 {
        return None;
    }
    Some((v >> bit) & 1 == 1)
}

/// `oaColor` as the colour value: the RGB the property holds (a BGR `u32` with a zero top byte), or
/// `def`.
fn oa_color_val(v: Option<u32>, def: i32) -> i32 {
    match v {
        Some(v) if v >> 24 == 0 => i32::from_le_bytes((((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff)).to_le_bytes()),
        _ => def,
    }
}

fn oa_color(v: Option<u32>, def: i32) -> String {
    hex(oa_color_val(v, def))
}

/// `oaString`: the UTF-16LE string, zero terminated, of a complex property.
fn oa_string(shape: &Shape<'_>, id: u16) -> Option<Vec<u16>> {
    let c = shape.props.get_complex(id)?;
    if c.len() < 2 {
        return None;
    }
    let mut s = Vec::new();
    for pair in c.as_chunks::<2>().0 {
        let ch = u16::from(pair[0]) | (u16::from(pair[1]) << 8);
        if ch == 0 {
            break;
        }
        s.push(ch);
    }
    Some(s)
}

/// `(v | 0) / 65536` of a property value.
fn fixed16(v: u32) -> f64 {
    f64::from(i32::from_le_bytes(v.to_le_bytes())) / 65536.0
}

/// `cropOf`.
fn crop_of(shape: &Shape<'_>) -> Option<Crop> {
    let f = |id: u16| fixed16(shape.props.value(id).unwrap_or(0));
    let c = Crop { t: f(0x100), b: f(0x101), l: f(0x102), r: f(0x103) };
    (c.t != 0.0 || c.b != 0.0 || c.l != 0.0 || c.r != 0.0).then_some(c)
}

/// `xfrmXml`: `a:xfrm` with the rotation and flips of the shape (and the child space, for groups).
fn xfrm_xml(name: &str, b: BoxEmu, shape: &Shape<'_>, ch: Option<BoxEmu>, extra_flip_v: bool) -> String {
    let rot_raw = shape.props.value(0x0004).unwrap_or(0);
    let rot = round(fixed16(rot_raw) * 60000.0);
    let rot_attr = if rot != 0.0 && !rot.is_nan() { num(rem(rem(rot, 21_600_000.0) + 21_600_000.0, 21_600_000.0)) } else { None };
    // the preset can be the vertical mirror of the binary geometry (preset-map)
    let flip_v = shape.has_flag(fsp::FLIP_V) != extra_flip_v;
    let a = [
        ("rot", rot_attr),
        ("flipH", shape.has_flag(fsp::FLIP_H).then_some(AttrVal::Int(1))),
        ("flipV", flip_v.then_some(AttrVal::Int(1))),
    ];
    let mut c = format!("<a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>", b.x, b.y, b.cx.max(0), b.cy.max(0));
    if let Some(ch) = ch {
        c += &format!("<a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/>", ch.x, ch.y, ch.cx.max(0), ch.cy.max(0));
    }
    el_of(name, &a, &c)
}

/// `picGraphic`: the `a:graphic` of a picture, with its colour adjustments and crop.
fn pic_graphic(name: &[u16], rid: &str, cx: i64, cy: i64, crop: Option<Crop>, fx: Option<PicFx>) -> String {
    // the colour adjustments of the picture (a "faded" watermark…) inside the a:blip
    let mut blip_kids = String::new();
    if let Some(fx) = fx {
        if fx.gray {
            blip_kids += "<a:grayscl/>";
        }
        if fx.bi_level {
            blip_kids += "<a:biLevel thresh=\"50000\"/>";
        }
        let bright = fx.bright.filter(|&b| b != 0.0);
        let contrast = fx.contrast.filter(|&c| c != 0.0);
        if bright.is_some() || contrast.is_some() {
            blip_kids += &el_of("a:lum", &[("bright", bright.and_then(num)), ("contrast", contrast.and_then(num))], "");
        }
    }
    let mut src = String::new();
    if let Some(c) = crop {
        let k = |v: f64| {
            let r = round(v * 100_000.0);
            if r != 0.0 && !r.is_nan() { num(r) } else { None }
        };
        src = el_of("a:srcRect", &[("l", k(c.l)), ("t", k(c.t)), ("r", k(c.r)), ("b", k(c.b))], "");
    }
    let blip = el_of("a:blip", &[("r:embed", Some(AttrVal::Str(rid)))], &blip_kids);
    format!(
        "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr>{}<pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>{blip}{src}<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>",
        el_of("pic:cNvPr", &[("id", int(0)), ("name", Some(AttrVal::Units(name)))], "")
    )
}

/// `hrVml`: a horizontal rule in line (`o:hr`): DrawingML has no equivalent; the Word writes VML.
pub(super) fn hr_vml(pic: &PicInfo<'_>, hr: &Hr) -> String {
    let pt = |emu: f64| to_js_string(round((emu / 12700.0) * 10.0) / 10.0);
    let style = format!("width:{}pt;height:{}pt", pt(pic.cx), pt(pic.cy));
    let fill = hr.fill.map(|f| format!("#{}", hex(f)));
    let a = [
        ("style", Some(AttrVal::Str(&style))),
        ("o:hrpct", hr.pct.map(|p| AttrVal::Int(i64::from(p)))),
        ("o:hralign", Some(AttrVal::Str(hr.align))),
        ("o:hrstd", Some(AttrVal::Str("t"))),
        ("o:hrnoshade", hr.noshade.then_some(AttrVal::Str("t"))),
        ("o:hr", Some(AttrVal::Str("t"))),
        ("fillcolor", fill.as_deref().map(AttrVal::Str)),
        ("stroked", Some(AttrVal::Str("f"))),
    ];
    format!("<w:pict>{}</w:pict>", el_of("v:rect", &a, ""))
}

/// `oaFillXml`: the fill of an OfficeArt shape: solid (0x181) or a linear gradient (`fillType` 0x180 =
/// 4 shade / 7 shadeScale). Measured in the Word (.docx → .doc → .docx and the PDF of the equivalent
/// VML): the angle 0x18B (degrees 16.16) 0 = top to bottom → `a:lin` 90° − α, a negative one is α + 180°
/// (−45 = 135); colours 0x181 → 0x183 or the list 0x197 (colour, position 16.16), the second one can be
/// the fill colour lightened/darkened (index 0xF0); focus 0x18C f ≥ 0: the second colour at f% and back
/// to the first; f < 0: the first at 100 + f%, with the second at the ends (−100 = 100).
fn oa_fill_xml(shape: &Shape<'_>) -> String {
    let pv = |k: u16| shape.props.value(k);
    let c1_val = oa_color_val(pv(0x181), 0x00ff_ffff);
    let c1 = hex(c1_val);
    let ty = pv(0x180).unwrap_or(0);
    if ty != 4 && ty != 7 {
        return format!("<a:solidFill><a:srgbClr val=\"{c1}\"/></a:solidFill>");
    }
    // the system index 0xF0 (the colour of the fill) with function 1 = darken (c·p/255) or 2 = lighten
    // (255 − (255 − c)·p/255): the "fill lighten(p)" of the VML (fixture degrades; corpus t66)
    let col = |v: Option<u32>| -> String {
        match v {
            Some(v) if v >> 24 == 0x10 && (v & 0xff) == 0xf0 => {
                let func = (v >> 8) & 0x0f;
                let p = f64::from((v >> 16) & 0xff);
                let n = c1_val;
                let ch = |c: i32| -> i32 {
                    let c = f64::from(c);
                    to_int32(round(match func {
                        1 => (c * p) / 255.0,
                        2 => 255.0 - ((255.0 - c) * p) / 255.0,
                        _ => c,
                    }))
                };
                hex((ch((n >> 16) & 0xff) << 16) | (ch((n >> 8) & 0xff) << 8) | ch(n & 0xff))
            }
            _ => oa_color(v, 0x00ff_ffff),
        }
    };
    let mut base: Vec<(f64, String)> = vec![(0.0, c1.clone()), (1.0, col(pv(0x183)))];
    if let Some(arr) = shape.props.get_complex(0x197)
        && arr.len() >= 6
    {
        let r = ByteReader::new(arr);
        let n = u64::from(r.u16(0));
        let cb = u64::from(r.u16(4));
        let mut list: Vec<(f64, String)> = Vec::new();
        let mut i = 0u64;
        while i < n && cb >= 8 && 6 + (i + 1) * cb <= arr.len() as u64 {
            list.push((f64::from(r.i32(6 + i * cb + 4)) / 65536.0, col(Some(r.u32(6 + i * cb)))));
            i += 1;
        }
        if list.len() >= 2 {
            base = list;
        }
    }
    let focus = i32::from_le_bytes(pv(0x18c).unwrap_or(0).to_le_bytes());
    let f = f64::from(focus.clamp(-100, 100)) / 100.0;
    let rev: Vec<(f64, String)> = base.iter().rev().map(|(q, c)| (1.0 - q, c.clone())).collect();
    // f >= 0: from the first to the second in [0, f] and back in [f, 1]; f < 0: the reverse in [0, 1 + f] and [1 + f, 1]
    let (a, b, mid) = if f >= 0.0 { (&base, &rev, f) } else { (&rev, &base, 1.0 + f) };
    let mut stops: Vec<(f64, &str)> = Vec::new();
    if mid > 0.0 {
        for (q, c) in a {
            stops.push((q * mid, c));
        }
    }
    if mid < 1.0 {
        for (q, c) in b {
            if !(mid > 0.0 && *q == 0.0) {
                stops.push((mid + q * (1.0 - mid), c));
            }
        }
    }
    let mut alpha = fixed16(pv(0x18b).unwrap_or(0));
    while alpha < 0.0 {
        alpha += 180.0;
    }
    let ang = rem(rem(rem(90.0 - alpha, 360.0) + 360.0, 360.0), 360.0) * 60000.0;
    // (position 16.16 → 0.01%: 26214/65536 = 40%)
    let gs: String = stops
        .iter()
        .map(|(q, c)| {
            format!("<a:gs pos=\"{}\"><a:srgbClr val=\"{c}\"/></a:gs>", to_js_string(round(max(0.0, min(1.0, *q)) * 10000.0) * 10.0))
        })
        .collect();
    format!(
        "<a:gradFill rotWithShape=\"1\"><a:gsLst>{gs}</a:gsLst><a:lin ang=\"{}\" scaled=\"{}\"/></a:gradFill>",
        to_js_string(round(ang)),
        i32::from(ty == 7)
    )
}

/// `wordArtXml`: classic WordArt (msosptTextPlainText…: geoText): the text (gtextUNICODE, the font
/// gtextFont, the size gtextSize) drawn with the fill of the shape; the box has no fill or border. It
/// becomes a WordArt text box of DrawingML (bodyPr fromWordArt + prstTxWarp textPlain) with the text in the
/// colour of the fill: the typical watermark (the Word writes v:textpath in the VML).
fn word_art_xml(shape: &Shape<'_>, b: BoxEmu, child: bool, id: Option<i64>) -> Option<String> {
    let text = oa_string(shape, 0xc0).filter(|t| !t.is_empty())?;
    let pv = |k: u16| shape.props.value(k);
    let font = oa_string(shape, 0xc5).filter(|f| !f.is_empty());
    let size = match pv(0xc3) {
        Some(raw) => max(1.0, round((f64::from(raw) / 65536.0) * 2.0)),
        None => 72.0,
    };
    let filled = oa_flag(pv(0x1bf), 4).unwrap_or(true);
    let mut r_pr = String::new();
    if let Some(font) = &font {
        let f = Some(AttrVal::Units(font));
        r_pr += &el_of("w:rFonts", &[("w:ascii", f), ("w:hAnsi", f), ("w:cs", f)], "");
    }
    if filled {
        r_pr += &el_of("w:color", &[("w:val", Some(AttrVal::Str(&oa_color(pv(0x181), 0x00ff_ffff))))], "");
    }
    r_pr += &el_of("w:sz", &[("w:val", num(size))], "");
    r_pr += &el_of("w:szCs", &[("w:val", num(size))], "");
    // text.split(/\r\n|\r|\n/)
    let mut lines: Vec<&[u16]> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        if text[i] == 0x0d && text.get(i + 1) == Some(&0x0a) {
            lines.push(&text[start..i]);
            i += 2;
            start = i;
        } else if text[i] == 0x0d || text[i] == 0x0a {
            lines.push(&text[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    lines.push(&text[start..]);
    let paras: String = lines
        .iter()
        .map(|line| {
            format!(
                "<w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"center\"/></w:pPr><w:r><w:rPr>{r_pr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                esc_text_units(line)
            )
        })
        .collect();
    let sp_pr =
        xfrm_xml("a:xfrm", b, shape, None, false) + "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln>";
    let body_pr = el_of(
        "wps:bodyPr",
        &[
            ("rot", int(0)),
            ("vert", Some(AttrVal::Str("horz"))),
            ("wrap", Some(AttrVal::Str("none"))),
            ("lIns", int(0)),
            ("tIns", int(0)),
            ("rIns", int(0)),
            ("bIns", int(0)),
            ("fromWordArt", int(1)),
            ("anchor", Some(AttrVal::Str("ctr"))),
            ("anchorCtr", int(0)),
        ],
        "<a:prstTxWarp prst=\"textPlain\"><a:avLst/></a:prstTxWarp><a:noAutofit/>",
    );
    let nv = match (child, id) {
        (true, Some(id)) => el_of("wps:cNvPr", &[("id", int(id)), ("name", Some(AttrVal::Str(&format!("WordArt {id}"))))], ""),
        _ => String::new(),
    };
    Some(format!(
        "<wps:wsp>{nv}<wps:cNvSpPr txBox=\"1\"/><wps:spPr>{sp_pr}</wps:spPr><wps:txbx><w:txbxContent>{paras}</w:txbxContent></wps:txbx>{body_pr}</wps:wsp>"
    ))
}

impl<'a> DocxWriter<'a> {
    /// `backgroundXml`: the page background: the background shape of the main drawing with a solid
    /// fill (`fillType` 0x180 = 0, `fFilled` in 0x1BF) → `w:background` with the colour (0x181).
    pub(super) fn background_xml(&self) -> String {
        let Some(shape) = self.r.drawings.background_shape() else { return String::new() };
        let pv = |k: u16| shape.props.value(k);
        if pv(0x180).unwrap_or(0) != 0 || oa_flag(pv(0x1bf), 4) == Some(false) {
            return String::new();
        }
        let Some(c) = pv(0x181).filter(|c| c >> 24 == 0) else { return String::new() };
        let color = oa_color(Some(c), 0x00ff_ffff);
        if color == "FFFFFF" { String::new() } else { el_of("w:background", &[("w:color", Some(AttrVal::Str(&color)))], "") }
    }

    /// `inlineDrawing`: an inline picture (the 0x01 character).
    pub(super) fn inline_drawing(&mut self, pic: PicInfo<'a>, rels: &mut PartRels) -> String {
        let PicInfo { blip, cx, cy, name, crop, fx, .. } = pic;
        let Some(blip) = blip else { return String::new() };
        let target = self.media_target(blip);
        let rid = rels.add_str("image", &target, false);
        self.doc_pr_id += 1;
        let id = self.doc_pr_id;
        let cx = int_of(max(1.0, round(cx)));
        let cy = int_of(max(1.0, round(cy)));
        let name: Vec<u16> = name.unwrap_or_else(|| format!("Imagem {id}").encode_utf16().collect());
        let inner = el_of("wp:extent", &[("cx", int(cx)), ("cy", int(cy))], "")
            + &el_of("wp:effectExtent", &[("l", int(0)), ("t", int(0)), ("r", int(0)), ("b", int(0))], "")
            + &el_of("wp:docPr", &[("id", int(id)), ("name", Some(AttrVal::Units(&name)))], "")
            + "<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>"
            + &pic_graphic(&name, &rid, cx, cy, crop, fx);
        format!(
            "<w:drawing>{}</w:drawing>",
            el_of("wp:inline", &[("distT", int(0)), ("distB", int(0)), ("distL", int(0)), ("distR", int(0))], &inner)
        )
    }

    /// `anchorDrawing`: a floating shape (the 0x08 character).
    pub(super) fn anchor_drawing(&mut self, f: &Fspa, ctx: Story, rels: &mut PartRels) -> Result<String, DocError> {
        let drawings = std::rc::Rc::clone(&self.r.drawings);
        // a spid is read as `u32`; the FSPA's is an `i32` (a negative one is never in the map)
        let Some(shape) = u32::try_from(f.spid).ok().and_then(|s| drawings.shape(s, ctx.hdr)) else { return Ok(String::new()) };
        // the master shape (hspMaster of another): the Word does not draw it or write it in the .docx
        if drawings.masters.contains(&shape.spid) {
            return Ok(String::new());
        }
        let pv = |id: u16| shape.props.value(id);
        let cx = ((i64::from(f.xa_right) - i64::from(f.xa_left)) * EMU_PER_TWIP).max(1);
        let cy = ((i64::from(f.ya_bottom) - i64::from(f.ya_top)) * EMU_PER_TWIP).max(1);
        // position: the properties posh/posrelh/posv/posrelv (tertiary FOPT) or the bx/by of the FSPA
        let rel_h = ["margin", "page", "column"].get(usize::from(f.bx)).copied().unwrap_or("column");
        let rel_v = ["margin", "page", "paragraph"].get(usize::from(f.by)).copied().unwrap_or("paragraph");
        let (posrelh, posrelv, posh, posv) = (pv(0x390), pv(0x392), pv(0x38f), pv(0x391));
        // MSOPRH/MSOPRV (0 margin, 1 page, 2 text, 3 character/line…): checked against the VML of the Word
        let pick = |table: &[&'static str], v: Option<u32>, default: &'static str| -> &'static str {
            match v {
                Some(v) => usize::try_from(v).ok().and_then(|i| table.get(i)).copied().unwrap_or(default),
                None => default,
            }
        };
        let rel_h_name =
            pick(&["margin", "page", "column", "character", "leftMargin", "rightMargin", "insideMargin", "outsideMargin"], posrelh, rel_h);
        let rel_v_name =
            pick(&["margin", "page", "paragraph", "line", "topMargin", "bottomMargin", "insideMargin", "outsideMargin"], posrelv, rel_v);
        let align_of = |table: &[&'static str], v: Option<u32>| -> &'static str {
            match v {
                Some(v) if v != 0 => usize::try_from(v).ok().and_then(|i| table.get(i)).copied().unwrap_or(""),
                _ => "",
            }
        };
        let h_align = align_of(&["", "left", "center", "right", "inside", "outside"], posh);
        let v_align = align_of(&["", "top", "center", "bottom", "inside", "outside"], posv);
        let position_h = format!(
            "<wp:positionH relativeFrom=\"{rel_h_name}\">{}</wp:positionH>",
            if h_align.is_empty() {
                format!("<wp:posOffset>{}</wp:posOffset>", i64::from(f.xa_left) * EMU_PER_TWIP)
            } else {
                format!("<wp:align>{h_align}</wp:align>")
            }
        );
        let position_v = format!(
            "<wp:positionV relativeFrom=\"{rel_v_name}\">{}</wp:positionV>",
            if v_align.is_empty() {
                format!("<wp:posOffset>{}</wp:posOffset>", i64::from(f.ya_top) * EMU_PER_TWIP)
            } else {
                format!("<wp:align>{v_align}</wp:align>")
            }
        );
        let wrap_text = ["bothSides", "left", "right", "largest"].get(usize::from(f.wrk)).copied().unwrap_or("bothSides");
        const POLYGON: &str = "<wp:wrapPolygon edited=\"0\"><wp:start x=\"0\" y=\"0\"/><wp:lineTo x=\"0\" y=\"21600\"/><wp:lineTo x=\"21600\" y=\"21600\"/><wp:lineTo x=\"21600\" y=\"0\"/><wp:lineTo x=\"0\" y=\"0\"/></wp:wrapPolygon>";
        let wrap = match f.wr {
            1 => "<wp:wrapTopAndBottom/>".to_owned(),
            2 | 0 => format!("<wp:wrapSquare wrapText=\"{wrap_text}\"/>"),
            4 => format!("<wp:wrapTight wrapText=\"{wrap_text}\">{POLYGON}</wp:wrapTight>"),
            5 => format!("<wp:wrapThrough wrapText=\"{wrap_text}\">{POLYGON}</wp:wrapThrough>"),
            _ => "<wp:wrapNone/>".to_owned(),
        };
        let dist = |id: u16, def: u32| i64::from(pv(id).unwrap_or(def));
        self.doc_pr_id += 1;
        let id = self.doc_pr_id;
        let name = format!("Forma {id}");
        let graphic = self.shape_graphic(shape, f, ctx, rels, &name, cx, cy)?;
        if graphic.is_empty() {
            return Ok(String::new());
        }
        // a shape "in line with the text" (the Word 2000+ writes it as an anchor at the character/line,
        // offset 0): the Word makes it an inline object
        if posrelh == Some(3)
            && posrelv == Some(3)
            && posh.is_none_or(|v| v == 0)
            && posv.is_none_or(|v| v == 0)
            && f.xa_left == 0
            && f.ya_top == 0
        {
            let inner = el_of("wp:extent", &[("cx", int(cx)), ("cy", int(cy))], "")
                + &el_of("wp:effectExtent", &[("l", int(0)), ("t", int(0)), ("r", int(0)), ("b", int(0))], "")
                + &el_of("wp:docPr", &[("id", int(id)), ("name", Some(AttrVal::Str(&name)))], "")
                + "<wp:cNvGraphicFramePr/>"
                + &graphic;
            return Ok(format!(
                "<w:drawing>{}</w:drawing>",
                el_of("wp:inline", &[("distT", int(0)), ("distB", int(0)), ("distL", int(0)), ("distR", int(0))], &inner)
            ));
        }
        // GroupShapeBooleanProperties (0x3BF): fBehindDocument (5), fLayoutInCell (15), fAllowOverlap (9),
        // fHidden (1): FSPA.fBelowText only counts without the property (Word 97); checked against the
        // VML of the Word (negative z-index, o:allowincell, o:allowoverlap)
        let g = pv(0x3bf);
        let behind = oa_flag(g, 5).unwrap_or(f.wr == 3 && f.below_text);
        // the Z order = the order of the shapes in the drawing (DgContainer), as the z-index of the Word's VML
        let z = i64::from(shape.z.unwrap_or_else(|| u32::try_from(id).unwrap_or(u32::MAX)));
        let a = [
            ("distT", int(dist(0x385, 0))),
            ("distB", int(dist(0x387, 0))),
            ("distL", int(dist(0x384, 114_300))),
            ("distR", int(dist(0x386, 114_300))),
            ("simplePos", int(0)),
            ("relativeHeight", int(251_658_240 + z * 1024)),
            ("behindDoc", int(i64::from(behind))),
            ("locked", int(i64::from(f.anchor_lock))),
            ("layoutInCell", int(i64::from(oa_flag(g, 15).unwrap_or(true)))),
            ("allowOverlap", int(i64::from(oa_flag(g, 9).unwrap_or(true)))),
        ];
        let inner = "<wp:simplePos x=\"0\" y=\"0\"/>".to_owned()
            + &position_h
            + &position_v
            + &el_of("wp:extent", &[("cx", int(cx)), ("cy", int(cy))], "")
            + &el_of("wp:effectExtent", &[("l", int(0)), ("t", int(0)), ("r", int(0)), ("b", int(0))], "")
            + &wrap
            + &el_of(
                "wp:docPr",
                &[
                    ("id", int(id)),
                    ("name", Some(AttrVal::Str(&name))),
                    ("hidden", (oa_flag(g, 1) == Some(true)).then_some(AttrVal::Int(1))),
                ],
                "",
            )
            + "<wp:cNvGraphicFramePr/>"
            + &graphic;
        Ok(format!("<w:drawing>{}</w:drawing>", el_of("wp:anchor", &a, &inner)))
    }

    /// `shapeGraphic`: the `a:graphic` of a floating shape: a picture, a shape or a text box, or a group.
    #[allow(clippy::too_many_arguments)]
    fn shape_graphic(
        &mut self,
        shape: &Shape<'a>,
        f: &Fspa,
        ctx: Story,
        rels: &mut PartRels,
        name: &str,
        cx: i64,
        cy: i64,
    ) -> Result<String, DocError> {
        if let Some(children) = &shape.children
            && shape.has_flag(fsp::GROUP)
        {
            let g = shape.group_rect().map_or(BoxEmu { x: 0, y: 0, cx, cy }, |g| BoxEmu {
                x: i64::from(g.l),
                y: i64::from(g.t),
                cx: i64::from(g.r) - i64::from(g.l),
                cy: i64::from(g.b) - i64::from(g.t),
            });
            let xfrm = xfrm_xml("a:xfrm", BoxEmu { x: 0, y: 0, cx, cy }, shape, Some(g), false);
            let mut kids = String::new();
            for c in children {
                kids += &self.child_shape_xml(c, ctx, rels)?;
            }
            return Ok(format!(
                "<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\"><wpg:wgp><wpg:cNvGrpSpPr/><wpg:grpSpPr>{xfrm}</wpg:grpSpPr>{kids}</wpg:wgp></a:graphicData></a:graphic>"
            ));
        }
        let blip = shape_blip(&self.r, shape);
        let stories = if ctx.hdr { &self.txbx_hdr } else { &self.txbx_main };
        let story = stories.get(&i64::from(shape.spid)).or_else(|| stories.get(&i64::from(f.spid))).copied();
        if let Some(blip) = blip
            && story.is_none()
        {
            let target = self.media_target(blip);
            let rid = rels.add_str("image", &target, false);
            let units: Vec<u16> = name.encode_utf16().collect();
            return Ok(pic_graphic(&units, &rid, cx, cy, crop_of(shape), pic_fx_of(Some(shape))));
        }
        let wsp = self.wsp_xml(shape, ctx, rels, BoxEmu { x: 0, y: 0, cx, cy }, story, false, None)?;
        Ok(format!(
            "<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">{wsp}</a:graphicData></a:graphic>"
        ))
    }

    /// `childShapeXml`: a shape inside a group (coordinates in the child space of the group).
    fn child_shape_xml(&mut self, shape: &Shape<'a>, ctx: Story, rels: &mut PartRels) -> Result<String, DocError> {
        let a = shape.child_rect().unwrap_or_default();
        let b = BoxEmu {
            x: i64::from(a.l),
            y: i64::from(a.t),
            cx: (i64::from(a.r) - i64::from(a.l)).max(0),
            cy: (i64::from(a.b) - i64::from(a.t)).max(0),
        };
        if let Some(children) = &shape.children
            && shape.has_flag(fsp::GROUP)
        {
            let g = shape.group_rect().map_or(
                BoxEmu { x: i64::from(a.l), y: i64::from(a.t), cx: i64::from(a.r) - i64::from(a.l), cy: i64::from(a.b) - i64::from(a.t) },
                |g| BoxEmu {
                    x: i64::from(g.l),
                    y: i64::from(g.t),
                    cx: i64::from(g.r) - i64::from(g.l),
                    cy: i64::from(g.b) - i64::from(g.t),
                },
            );
            let xfrm = xfrm_xml("a:xfrm", b, shape, Some(g), false);
            let mut kids = String::new();
            for c in children {
                kids += &self.child_shape_xml(c, ctx, rels)?;
            }
            return Ok(format!("<wpg:grpSp><wpg:cNvGrpSpPr/><wpg:grpSpPr>{xfrm}</wpg:grpSpPr>{kids}</wpg:grpSp>"));
        }
        self.doc_pr_id += 1;
        let id = self.doc_pr_id;
        let blip = shape_blip(&self.r, shape);
        let story = if ctx.hdr { &self.txbx_hdr } else { &self.txbx_main }.get(&i64::from(shape.spid)).copied();
        if let Some(blip) = blip
            && story.is_none()
        {
            let target = self.media_target(blip);
            let rid = rels.add_str("image", &target, false);
            return Ok(format!(
                "<pic:pic><pic:nvPicPr>{}<pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>{}<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>",
                el_of("pic:cNvPr", &[("id", int(id)), ("name", Some(AttrVal::Str(&format!("Imagem {id}"))))], ""),
                el_of("a:blip", &[("r:embed", Some(AttrVal::Str(&rid)))], ""),
                xfrm_xml("a:xfrm", b, shape, None, false)
            ));
        }
        self.wsp_xml(shape, ctx, rels, b, story, true, Some(id))
    }

    /// `wspXml`: `wps:wsp`: a preset geometry, fill, line and (optionally) a text box.
    #[allow(clippy::too_many_arguments)]
    fn wsp_xml(
        &mut self,
        shape: &Shape<'a>,
        ctx: Story,
        rels: &mut PartRels,
        b: BoxEmu,
        story: Option<(i64, i64)>,
        child: bool,
        id: Option<i64>,
    ) -> Result<String, DocError> {
        let pv = |k: u16| shape.props.value(k);
        if is_word_art_type(shape.shape_type)
            && story.is_none()
            && let Some(art) = word_art_xml(shape, b, child, id)
        {
            return Ok(art);
        }
        let adjust = raw_adjust(&shape.props);
        // `box.cx || 1`
        let w = if b.cx == 0 { 1.0 } else { i64_to_f64(b.cx) };
        let h = if b.cy == 0 { 1.0 } else { i64_to_f64(b.cy) };
        let geo = preset_for(shape.shape_type, &adjust, w, h, PresetVariant::WordSlides);
        let (prst, flip_v) = geo.as_ref().map_or(("rect", false), |g| (g.prst, g.flip_v));
        let is_line = shape.shape_type == 20 || is_connector_type(shape.shape_type);
        // the Booleans of fill (0x1BF: fFilled, bit 4) and line (0x1FF: fLine, bit 3); without the fUse*
        // the default holds (lines and connectors without fill)
        let filled = oa_flag(pv(0x1bf), 4).unwrap_or(!is_line);
        let lined = oa_flag(pv(0x1ff), 3).unwrap_or(true);
        let line_width = pv(0x1cb).unwrap_or(9525);
        let av: String = geo
            .iter()
            .flat_map(|g| g.av())
            .map(|g| {
                el_of(
                    "a:gd",
                    &[("name", Some(AttrVal::Str(g.name))), ("fmla", Some(AttrVal::Str(&format!("val {}", to_js_string(g.val)))))],
                    "",
                )
            })
            .collect();
        let mut sp_pr = xfrm_xml("a:xfrm", b, shape, None, flip_v)
            + &format!(
                "<a:prstGeom prst=\"{prst}\">{}</a:prstGeom>",
                if av.is_empty() { "<a:avLst/>".to_owned() } else { format!("<a:avLst>{av}</a:avLst>") }
            );
        if !is_line {
            sp_pr += &if filled { oa_fill_xml(shape) } else { "<a:noFill/>".to_owned() };
        }
        if lined {
            let dash = [
                "solid",
                "sysDash",
                "sysDot",
                "sysDashDot",
                "sysDashDotDot",
                "dot",
                "dash",
                "lgDash",
                "dashDot",
                "lgDashDot",
                "lgDashDotDot",
            ]
            .get(usize::try_from(pv(0x1ce).unwrap_or(0)).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or("solid");
            let arrow = |v: Option<u32>| -> Option<&'static str> {
                let v = v.filter(|&v| v != 0)?;
                Some(
                    ["none", "triangle", "stealth", "diamond", "oval", "arrow"]
                        .get(usize::try_from(v).unwrap_or(usize::MAX))
                        .copied()
                        .unwrap_or("triangle"),
                )
            };
            let (head, tail) = (arrow(pv(0x1d0)), arrow(pv(0x1d1)));
            sp_pr += &format!("<a:ln w=\"{line_width}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", oa_color(pv(0x1c0), 0));
            if dash != "solid" {
                sp_pr += &format!("<a:prstDash val=\"{dash}\"/>");
            }
            if let Some(head) = head {
                sp_pr += &format!("<a:headEnd type=\"{head}\"/>");
            }
            if let Some(tail) = tail {
                sp_pr += &format!("<a:tailEnd type=\"{tail}\"/>");
            }
            sp_pr += "</a:ln>";
        } else {
            sp_pr += "<a:ln><a:noFill/></a:ln>";
        }
        let mut txbx = String::new();
        if let Some(story) = story {
            if self.nest >= self.limits.max_nesting {
                return Err(DocError::Limit { kind: DocLimitKind::Nesting, max: u64::from(self.limits.max_nesting) });
            }
            self.nest += 1;
            let inner = self.txbx_content(story, ctx, rels);
            self.nest -= 1;
            let inner = inner?;
            txbx = format!("<wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx>", if inner.is_empty() { "<w:p/>" } else { &inner });
        }
        let l = pv(0x81).unwrap_or(91440);
        let t = pv(0x82).unwrap_or(45720);
        let rr = pv(0x83).unwrap_or(91440);
        let bb = pv(0x84).unwrap_or(45720);
        let anchor =
            ["t", "ctr", "b", "t", "ctr", "b"].get(usize::try_from(pv(0x87).unwrap_or(0)).unwrap_or(usize::MAX)).copied().unwrap_or("t");
        // the Booleans of text (0xBF): fFitShapeToText (bit 1) with fUsefFitShapeToText (bit 17)
        let text_bits = pv(0xbf).unwrap_or(0);
        let autofit = if text_bits & (1 << 17) != 0 { text_bits & 2 != 0 } else { false };
        let body_pr = el_of(
            "wps:bodyPr",
            &[
                ("rot", int(0)),
                ("vert", Some(AttrVal::Str("horz"))),
                ("wrap", Some(AttrVal::Str("square"))),
                ("lIns", int(i64::from(l))),
                ("tIns", int(i64::from(t))),
                ("rIns", int(i64::from(rr))),
                ("bIns", int(i64::from(bb))),
                ("anchor", Some(AttrVal::Str(anchor))),
                ("anchorCtr", int(0)),
            ],
            if autofit { "<a:spAutoFit/>" } else { "<a:noAutofit/>" },
        );
        let nv = match (child, id) {
            (true, Some(id)) => el_of("wps:cNvPr", &[("id", int(id)), ("name", Some(AttrVal::Str(&format!("Forma {id}"))))], ""),
            _ => String::new(),
        };
        let c_nv = if is_line {
            "<wps:cNvCnPr/>".to_owned()
        } else {
            format!("<wps:cNvSpPr{}/>", if story.is_some() { " txBox=\"1\"" } else { "" })
        };
        Ok(format!("<wps:wsp>{nv}{c_nv}<wps:spPr>{sp_pr}</wps:spPr>{txbx}{body_pr}</wps:wsp>"))
    }

    /// The content of the story of a text box (`wspXml`'s `txbx`): its paragraphs and tables.
    fn txbx_content(&mut self, story: (i64, i64), ctx: Story, rels: &mut PartRels) -> Result<String, DocError> {
        let mut paras = self.collect_paras(story.0, story.1)?;
        // the story of the box ends with an empty guard paragraph
        if paras.len() > 1
            && let Some(last) = paras.last()
            && last.cp_end - last.cp_start == 1
        {
            paras.pop();
        }
        let saved = std::mem::take(&mut self.field_stack);
        let len = paras.len();
        let inner = self.render_blocks(&mut paras, 0, len, 0, Story { kind: Kind::Txbx, hdr: ctx.hdr }, rels, None);
        self.field_stack = saved;
        inner
    }
}

/// An `i64` below 2^53 as the JS number it is.
fn i64_to_f64(v: i64) -> f64 {
    v as f64
}
