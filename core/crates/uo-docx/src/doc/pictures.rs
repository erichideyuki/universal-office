//! Pictures of the `.doc` (`pictures.ts`):
//!  - inline: the character 0x01 with `sprmCPicLocation` points at a `PICFAndOfficeArtData` in the
//!    `Data` stream ([MS-DOC] 2.9.192): a PICF (68 bytes: target size, scale, crop) + [name] + an
//!    `OfficeArtInlineSpContainer` (a `SpContainer` and `FBSE`s with the BLIP inside);
//!  - floating: the shape of the drawing with `pib` → the BLIP store → the BLIP (inside the FBSE or
//!    in the `WordDocument` stream at `BSE.foDelay`).
//!
//! The BLIPs borrow the streams; one [`DocReader::blip_budget`] bounds what the document inflates,
//! in the order the TS decodes the pictures.

use uo_drawingml::odraw::{Blip, Flavor, OdrawLimits, Rec, Shape, decode_blip, read_fbse, read_rec, read_sp_container, recs_in, rt};
use uo_jsnum::math::{abs, max, round};

use super::bytes::ByteReader;
use super::reader::DocReader;

/// The crop of a picture in fractions (0..1) of each side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// A horizontal rule (a "HR" shape without a picture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hr {
    pub align: &'static str,
    pub pct: Option<u32>,
    pub fill: Option<i32>,
    pub noshade: bool,
}

/// Colour adjustments of a picture in the DrawingML model (`a:lum` in thousandths of a percent,
/// `a:grayscl`, `a:biLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PicFx {
    pub bright: Option<f64>,
    pub contrast: Option<f64>,
    pub gray: bool,
    pub bi_level: bool,
}

/// An inline picture.
#[derive(Debug, Clone, PartialEq)]
pub struct PicInfo<'a> {
    pub blip: Option<Blip<'a>>,
    /// the displayed size in EMU
    pub cx: f64,
    pub cy: f64,
    /// the name (Latin-1 units), when the PICF has one: an empty name is still a name
    pub name: Option<Vec<u16>>,
    pub crop: Option<Crop>,
    pub hr: Option<Hr>,
    pub fx: Option<PicFx>,
}

const EMU_PER_TWIP: f64 = 635.0;

/// `picFxOf`: `pictureContrast` (0x108, 16.16: the gain of the VML) and `pictureBrightness` (0x109,
/// /65536: the blacklevel of the VML) become `a:lum` (bright = 2·blacklevel; contrast = 1 − 1/gain
/// or gain − 1); the Blip Boolean (0x13F): `fPictureBiLevel` (bit 1), `fPictureGray` (bit 2).
#[must_use]
pub fn pic_fx_of(shape: Option<&Shape<'_>>) -> Option<PicFx> {
    let shape = shape?;
    let mut fx = PicFx::default();
    let gain = shape.props.value(0x108).map_or(1.0, |ct| f64::from(ct) / 65536.0);
    let bl = shape.props.value(0x109).map_or(0.0, |br| f64::from(i32::from_le_bytes(br.to_le_bytes())) / 65536.0);
    if abs(gain - 1.0) > 1e-6 {
        fx.contrast = Some(round((if gain >= 1.0 { 1.0 - 1.0 / gain } else { gain - 1.0 }) * 100_000.0));
    }
    if bl != 0.0 {
        fx.bright = Some(round(2.0 * bl * 100_000.0));
    }
    let bb = shape.props.value(0x13f);
    let flag = |bit: u32| bb.is_some_and(|v| (v >> (bit + 16)) & 1 == 1 && (v >> bit) & 1 == 1);
    if flag(2) {
        fx.gray = true;
    }
    if flag(1) {
        fx.bi_level = true;
    }
    (fx.contrast.is_some() || fx.bright.is_some() || fx.gray || fx.bi_level).then_some(fx)
}

/// `decodeBlip(rec, r.blipBudget)` with the document's budget.
fn decode<'a>(r: &DocReader<'a>, rec: &Rec<'a>) -> Option<Blip<'a>> {
    let mut budget = r.blip_budget.get();
    let blip = decode_blip(rec, &mut budget, Flavor::Word, &OdrawLimits::WORD);
    r.blip_budget.set(budget);
    blip
}

/// `blipAt`: a BLIP written straight into a stream (`WordDocument` at `foDelay`).
fn blip_at<'a>(r: &DocReader<'a>, stream: &'a [u8], off: u32) -> Option<Blip<'a>> {
    let off = u64::from(off);
    if off + 8 > stream.len() as u64 {
        return None;
    }
    let rec = read_rec(stream, off, stream.len() as u64)?;
    if !rec.is_blip() {
        return None;
    }
    decode(r, &rec)
}

/// `readInlinePicture`: the picture at `fc` of the `Data` stream.
#[must_use]
pub fn read_inline_picture<'a>(r: &DocReader<'a>, fc: i32) -> Option<PicInfo<'a>> {
    let data: &'a [u8] = r.data?;
    let fc = u64::try_from(fc).ok()?;
    if fc + 0x44 > data.len() as u64 {
        return None;
    }
    let d = ByteReader::new(data);
    let lcb = i64::from(d.i32(fc));
    let cb_header = d.u16(fc + 4);
    if lcb < i64::from(cb_header) || cb_header < 0x44 {
        return None;
    }
    let end = (data.len() as u64).min(fc + u64::try_from(lcb).unwrap_or(0));
    let mm = d.i16(fc + 6);
    let dxa_goal = i64::from(d.i16(fc + 28));
    let dya_goal = i64::from(d.i16(fc + 30));
    let mx = i64::from(match d.u16(fc + 32) {
        0 => 1000,
        v => v,
    });
    let my = i64::from(match d.u16(fc + 34) {
        0 => 1000,
        v => v,
    });
    let crop_l = i64::from(d.i16(fc + 36));
    let crop_t = i64::from(d.i16(fc + 38));
    let crop_r = i64::from(d.i16(fc + 40));
    let crop_b = i64::from(d.i16(fc + 42));
    let mut o = fc + u64::from(cb_header);
    let mut name = None;
    if mm == 0x66 {
        let cch = u64::from(d.u8(o));
        name = Some((0..cch).map(|i| u16::from(d.u8(o + 1 + i))).collect::<Vec<u16>>());
        o += 1 + cch;
    }
    let w_tw = ((dxa_goal - crop_l - crop_r) * mx) as f64 / 1000.0;
    let h_tw = ((dya_goal - crop_t - crop_b) * my) as f64 / 1000.0;
    let mut info =
        PicInfo { blip: None, cx: max(1.0, w_tw) * EMU_PER_TWIP, cy: max(1.0, h_tw) * EMU_PER_TWIP, name, crop: None, hr: None, fx: None };
    // OfficeArtInlineSpContainer: a SpContainer and the FBSEs
    let mut shape: Option<Shape<'a>> = None;
    let mut bses: Vec<Blip<'a>> = Vec::new();
    for rec in recs_in(data, o, end) {
        if rec.rec_type == rt::SP_CONTAINER && shape.is_none() {
            shape = Some(read_sp_container(&rec, Flavor::Word));
        } else if rec.rec_type == rt::FBSE {
            let bse = read_fbse(&rec);
            let blip = if let Some(emb) = &bse.embedded {
                decode(r, emb)
            } else if bse.fo_delay != 0 && bse.fo_delay != 0xffff_ffff {
                blip_at(r, r.word, bse.fo_delay)
            } else {
                None
            };
            if let Some(blip) = blip {
                bses.push(blip);
            }
        } else if rec.is_blip()
            && let Some(blip) = decode(r, &rec)
        {
            bses.push(blip);
        }
    }
    let pib = shape.as_ref().and_then(|s| s.props.value(0x104)).unwrap_or(1);
    // `bses[pib - 1] ?? bses[0]`: pib 0 is `bses[-1]`, undefined
    let pick = pib.checked_sub(1).and_then(|i| usize::try_from(i).ok());
    let chosen = match pick {
        Some(i) if i < bses.len() => Some(i),
        _ if bses.is_empty() => None,
        _ => Some(0),
    };
    info.blip = chosen.map(|i| bses.swap_remove(i));
    if let Some(fx) = pic_fx_of(shape.as_ref()) {
        info.fx = Some(fx);
    }
    if let Some(shape) = &shape {
        let f = |id: u16| f64::from(i32::from_le_bytes(shape.props.value(id).unwrap_or(0).to_le_bytes())) / 65536.0;
        let c = Crop { t: f(0x100), b: f(0x101), l: f(0x102), r: f(0x103) };
        if c.t != 0.0 || c.b != 0.0 || c.l != 0.0 || c.r != 0.0 {
            info.crop = Some(c);
        }
    }
    if let Some(shape) = &shape
        && info.blip.is_none()
        && (shape.props.contains(0x393) || shape.props.contains(0x394) || shape.props.contains(0x396))
    {
        let fill = shape.props.value(0x181);
        let align =
            ["left", "center", "right"].get(shape.props.value(0x394).map_or(1, |v| usize::try_from(v).unwrap_or(usize::MAX))).copied();
        info.hr = Some(Hr {
            align: align.unwrap_or("center"),
            pct: shape.props.value(0x393),
            fill: fill.filter(|f| f >> 24 == 0).map(|f| (((f & 0xff) << 16) | (f & 0xff00) | ((f >> 16) & 0xff)) as i32),
            noshade: true,
        });
    }
    if info.crop.is_none() && dxa_goal > 0 && dya_goal > 0 && (crop_l != 0 || crop_t != 0 || crop_r != 0 || crop_b != 0) {
        info.crop = Some(Crop {
            l: crop_l as f64 / dxa_goal as f64,
            r: crop_r as f64 / dxa_goal as f64,
            t: crop_t as f64 / dya_goal as f64,
            b: crop_b as f64 / dya_goal as f64,
        });
    }
    Some(info)
}

/// `shapeBlip`: the picture of a floating shape (`pib` → the BLIP store).
#[must_use]
pub fn shape_blip<'a>(r: &DocReader<'a>, shape: &Shape<'a>) -> Option<Blip<'a>> {
    let pib = shape.props.value(0x104).filter(|&v| v != 0)?;
    let bse = r.drawings.bse(pib)?;
    if let Some(emb) = &bse.embedded {
        return decode(r, emb);
    }
    if bse.fo_delay != 0xffff_ffff {
        return blip_at(r, r.word, bse.fo_delay);
    }
    None
}
