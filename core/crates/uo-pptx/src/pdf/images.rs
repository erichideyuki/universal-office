//! The pictures of a page: media parts (JPEG as it is, `DCTDecode`; the other raster formats decoded and written
//! as Flate), derived images (the colour effects of `fxImage`, `slides-effects.ts`, over the pixels, or through
//! `/Decode` for the brightness and contrast of a JPEG) and generated bitmaps.

use uo_jsnum::math::{max, round};
use uo_pdf::JpegImage;
use uo_render::slides::{ImageFx, ImageSrc};

use super::css::parse_css;
use super::interp::Interp;
use super::{ImgHandle, PicSrc};

impl Interp<'_, '_> {
    /// The embedded picture of an image source, `None` when it cannot be drawn (counted).
    pub(super) fn image(&mut self, s: &ImageSrc) -> Option<ImgHandle> {
        match s {
            ImageSrc::Media(i) => {
                let m = self.res.media_entries().get(usize::try_from(*i).ok()?)?;
                let part = self.sym(m.part);
                self.media_image(&part, None)
            }
            ImageSrc::Derived(i) => {
                let d = self.res.derived_entries().get(usize::try_from(*i).ok()?)?;
                let m = self.res.media_entries().get(usize::try_from(d.image).ok()?)?;
                let part = self.sym(m.part);
                let fx = d.fx.clone();
                self.media_image(&part, Some(&fx))
            }
            ImageSrc::Bitmap(i) => self.bitmap_image(*i),
        }
    }

    fn bitmap_image(&mut self, i: u32) -> Option<ImgHandle> {
        if let Some(h) = self.bitmaps.get(&i) {
            return h.clone();
        }
        let h = self.make_bitmap(i);
        self.bitmaps.insert(i, h.clone());
        h
    }

    fn make_bitmap(&mut self, i: u32) -> Option<ImgHandle> {
        if self.res.is_recipe(i) {
            // a metafile, PICT or TIFF recorded as canvas calls: played as vector content
            return self.play_recipe(i);
        }
        let b = self.res.bitmap_entries().get(usize::try_from(i).ok()?)?;
        let bytes = self.res.bitmap_bytes(b.data)?;
        let (w, h) = (b.w, b.h);
        let want = usize::try_from(w).ok()?.checked_mul(usize::try_from(h).ok()?)?.checked_mul(4)?;
        if w == 0 || h == 0 || bytes.len() < want {
            return None;
        }
        let doc = self.w.doc.as_mut()?;
        let r = doc.embed_rgba(bytes.get(..want)?, w, h);
        let pdf = self.w.note(r)?;
        self.w.report.images += 1;
        Some(ImgHandle { src: PicSrc::Image(pdf), w: f64::from(w), h: f64::from(h) })
    }

    fn media_image(&mut self, part: &str, fx: Option<&ImageFx>) -> Option<ImgHandle> {
        let key = match fx {
            Some(fx) => format!("{part}|{fx:?}"),
            None => part.to_owned(),
        };
        if let Some(h) = self.w.images.get(&key) {
            return h.clone();
        }
        let h = self.make_media(part, fx);
        self.w.images.insert(key, h.clone());
        h
    }

    fn make_media(&mut self, part: &str, fx: Option<&ImageFx>) -> Option<ImgHandle> {
        let bytes = self.src.part(part)?;
        self.embed_picture(&bytes, fx)
    }

    /// A picture from its file bytes: JPEG as it is, the other raster formats decoded; `None` (counted) when it is
    /// none of them.
    pub(super) fn embed_picture(&mut self, data: &[u8], fx: Option<&ImageFx>) -> Option<ImgHandle> {
        if let Some(j) = uo_image::jpeg_info(data) {
            // brightness and contrast ride on `/Decode`; the other effects need pixels JPEG never decodes here
            let mut decode: Vec<f64> = Vec::new();
            if let Some(fx) = fx {
                let pixel_fx = fx.gray || fx.duotone.is_some() || fx.bi_level.is_some() || fx.clr_change.is_some();
                if pixel_fx || ((fx.bright.is_some() || fx.contrast.is_some()) && (j.components == 4)) {
                    self.count("image:jpeg-effect");
                } else if fx.bright.is_some() || fx.contrast.is_some() {
                    let b = fx.bright.unwrap_or(0.0);
                    let c = fx.contrast.unwrap_or(0.0);
                    let k = if c >= 0.0 { 1.0 / max(0.0001, 1.0 - c) } else { 1.0 + c };
                    let d0 = 0.5 - 0.5 * k + b * (1.0 + k) / 2.0;
                    let pair = [d0, d0 + k];
                    decode = if j.components == 1 { pair.to_vec() } else { vec![pair[0], pair[1], pair[0], pair[1], pair[0], pair[1]] };
                }
            }
            let img = JpegImage {
                bytes: data,
                width_px: j.width,
                height_px: j.height,
                components: j.components,
                adobe_inverted: j.adobe,
                decode: &decode,
                smask: None,
            };
            let doc = self.w.doc.as_mut()?;
            let r = doc.embed_jpeg(&img);
            let pdf = self.w.note(r)?;
            self.w.report.images += 1;
            return Some(ImgHandle { src: PicSrc::Image(pdf), w: f64::from(j.width), h: f64::from(j.height) });
        }
        let Ok(mut r) = uo_image::decode_raster(data, &self.w.opts.image_limits) else {
            self.count("image:unsupported");
            return None;
        };
        if let Some(fx) = fx {
            apply_fx(&mut r.data, fx, |s| String::from_utf16_lossy(self.res.sym_units(s)));
        }
        let doc = self.w.doc.as_mut()?;
        let e = doc.embed_rgba(&r.data, r.width, r.height);
        let pdf = self.w.note(e)?;
        self.w.report.images += 1;
        Some(ImgHandle { src: PicSrc::Image(pdf), w: f64::from(r.width), h: f64::from(r.height) })
    }
}

fn rgb_of(s: &str) -> Option<[f64; 3]> {
    parse_css(s).map(|c| [c.0, c.1, c.2])
}

/// `fxImage` over RGBA pixels (`slides-effects.ts`): the colour change, brightness and contrast, then the
/// bi-level, duotone or gray mapping on the Rec. 709 luminance of the sRGB values.
fn apply_fx(px: &mut [u8], fx: &ImageFx, sym: impl Fn(u32) -> String) {
    use uo_jsnum::conv::to_uint8_clamp;
    let from = fx.clr_change.as_ref().and_then(|c| rgb_of(&sym(c.from)));
    let to = fx.clr_change.as_ref().and_then(|c| rgb_of(&sym(c.to)));
    let to_alpha = fx.clr_change.as_ref().map_or(1.0, |c| c.to_alpha);
    let duo = fx.duotone.as_ref().and_then(|d| Some((rgb_of(&sym(d.first))?, rgb_of(&sym(d.second))?)));
    let b = fx.bright.unwrap_or(0.0);
    let c = fx.contrast.unwrap_or(0.0);
    let k = if c >= 0.0 { 1.0 / max(0.0001, 1.0 - c) } else { 1.0 + c };
    for p in px.as_chunks_mut::<4>().0 {
        let (mut r, mut g, mut bl) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        if let (Some(f), Some(t)) = (from, to)
            && (r - f[0]).abs() <= 2.0
            && (g - f[1]).abs() <= 2.0
            && (bl - f[2]).abs() <= 2.0
        {
            p[0] = to_uint8_clamp(t[0]);
            p[1] = to_uint8_clamp(t[1]);
            p[2] = to_uint8_clamp(t[2]);
            p[3] = to_uint8_clamp(round(f64::from(p[3]) * to_alpha));
            continue;
        }
        if b != 0.0 || c != 0.0 {
            let adj = |v: f64| ((v / 255.0 - 0.5) * k + 0.5 + b * (1.0 + k) / 2.0) * 255.0;
            r = adj(r);
            g = adj(g);
            bl = adj(bl);
        }
        let y = (0.2126 * r + 0.7152 * g + 0.0722 * bl) / 255.0;
        if let Some(bi) = fx.bi_level {
            let v = if y >= bi { 255.0 } else { 0.0 };
            (r, g, bl) = (v, v, v);
        } else if let Some((d0, d1)) = duo {
            r = d0[0] + (d1[0] - d0[0]) * y;
            g = d0[1] + (d1[1] - d0[1]) * y;
            bl = d0[2] + (d1[2] - d0[2]) * y;
        } else if fx.gray {
            let v = y * 255.0;
            (r, g, bl) = (v, v, v);
        }
        p[0] = to_uint8_clamp(r);
        p[1] = to_uint8_clamp(g);
        p[2] = to_uint8_clamp(bl);
    }
}
