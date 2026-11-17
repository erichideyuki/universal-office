//! The DrawingML property readers of `read.ts` (fills, gradients, patterns, `a:xfrm`, effects, 3D
//! bevel, blips and image effects, media, custom geometry): the parts of the shape half that do not
//! walk the shape tree. Same order of operations as the TS.

use uo_drawingml::{GuideDef, PathSeg, Subpaths, arc_to_seg, custom_guides, pattern_density};
use uo_jsnum::conv::to_int32;
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_jsnum::sort::js_sort_by;
use uo_jsnum::text::trim;
use uo_opc::xml::Element;

use crate::color::CssColor;
use crate::model::*;
use crate::source::{PartSource, R, RelMap, int_attr, int_or, is_int_text, pint, read_rels, rels_path_for, ta};
use crate::theme::{Theme, color_from_dml, solid_fill_color};

/// Reader context: the package and the caps.
#[derive(Clone, Copy)]
pub struct Rd<'a> {
    pub src: &'a dyn PartSource,
    pub limits: crate::source::Limits,
}

/// `fillAlphaEl(fill)?.attrs['val']` (`xml.ts`): the LAST `<a:alpha>` of the fill's colour.
pub(crate) fn fill_alpha_val<'d>(fill: Element<'d>) -> Option<&'d str> {
    fill.child_elements().next().and_then(|c| c.last_child("a:alpha")).and_then(|a| a.attr("val"))
}

/// `/val (-?\d+)/.exec(s)?.[1]`: the first number written after `val `.
pub(crate) fn val_after(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(i) = s.get(from..).and_then(|r| r.find("val ")) {
        let start = from + i + 4;
        let mut j = start;
        if b.get(j) == Some(&b'-') {
            j += 1;
        }
        let d0 = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > d0 {
            return s.get(start..j);
        }
        from += i + 1;
    }
    None
}

/// `{ type: 'solid', color }`.
pub(crate) fn solid_fill(color: CssColor) -> Fill {
    let mut f = Fill::of_kind(FillKind::Solid);
    f.color = Some(color);
    f
}

/// `parseXfrm` (`read.ts:1799`).
pub fn parse_xfrm(el: Element<'_>) -> Xfrm {
    let off = el.first_child("a:off");
    let ext = el.first_child("a:ext");
    let mut x = Xfrm {
        x: int_or(off, "x", 0.0),
        y: int_or(off, "y", 0.0),
        cx: int_or(ext, "cx", 0.0),
        cy: int_or(ext, "cy", 0.0),
        rot: None,
        flip_h: None,
        flip_v: None,
    };
    if let Some(r) = ta(el, "rot") {
        x.rot = Some(pint(r));
    }
    if el.attr("flipH") == Some("1") {
        x.flip_h = Some(true);
    }
    if el.attr("flipV") == Some("1") {
        x.flip_v = Some(true);
    }
    x
}

/// `<a:pattFill prst>` → a solid fill of the AVERAGE colour plus the pattern (`parsePattFill`).
pub fn parse_patt_fill(el: Element<'_>, theme: &Theme) -> Fill {
    let fg_el = el.first_child("a:fgClr");
    let bg_el = el.first_child("a:bgClr");
    let fg = fg_el.and_then(|e| color_from_dml(e.child_elements().next(), theme)).unwrap_or_else(|| CssColor::new("#000000"));
    let bg = bg_el.and_then(|e| color_from_dml(e.child_elements().next(), theme)).unwrap_or_else(|| CssColor::new("#FFFFFF"));
    let prst = el.attr("prst").unwrap_or("pct5");
    let d = pattern_density(prst);
    let n = |h: &str| -> [i32; 3] {
        let s = h.replacen('#', "", 1);
        let v = to_int32(parse_int(&s, 16));
        [(v >> 16) & 255, (v >> 8) & 255, v & 255]
    };
    let (a, b) = (n(fg.as_str()), n(bg.as_str()));
    let mut avg = String::from("#");
    for i in 0..3 {
        let (ai, bi) = (f64::from(a.get(i).copied().unwrap_or(0)), f64::from(b.get(i).copied().unwrap_or(0)));
        let v = math::round(ai * d + bi * (1.0 - d));
        avg.push_str(&format!("{:02x}", uo_jsnum::conv::to_uint8(v)));
    }
    // alpha of each colour of the pattern (measured, syn-pattalpha)
    let alpha_of = |e: Option<Element<'_>>| -> Option<f64> {
        let v = e.and_then(fill_alpha_val)?;
        Some(math::max(0.0, math::min(1.0, pint(v) / 100000.0)))
    };
    let (fg_a, bg_a) = (alpha_of(fg_el), alpha_of(bg_el));
    let mut f = Fill::of_kind(FillKind::Solid);
    f.color = Some(CssColor::new(&avg));
    f.pattern = Some(Box::new(PatternFill { prst: prst.into(), fg, bg, fg_a: fg_a.filter(|a| *a < 1.0), bg_a: bg_a.filter(|a| *a < 1.0) }));
    f
}

/// `parseGradFill`: a gradient with 2+ stops, `<a:lin ang>` in degrees; `None` otherwise.
pub fn parse_grad_fill(grad: Element<'_>, theme: &Theme) -> Option<Fill> {
    let gs_lst = grad.first_child("a:gsLst")?;
    let mut stops: Vec<GradientStop> = Vec::new();
    for gs in gs_lst.children_named("a:gs") {
        let pos = gs.attr("pos").map_or(0.0, |p| pint(p) / 100000.0);
        let color_el = gs.child_elements().next();
        let Some(color) = color_from_dml(color_el, theme) else { continue };
        let mut stop = GradientStop { pos: math::max(0.0, math::min(1.0, pos)), color, alpha: None };
        // alpha per stop (template overlays use transparent → dark)
        if let Some(a) = color_el.and_then(|c| c.last_child("a:alpha")).and_then(|a| ta(a, "val")) {
            stop.alpha = Some(pint(a) / 100000.0);
        }
        stops.push(stop);
    }
    if stops.len() < 2 {
        return None;
    }
    js_sort_by(&mut stops, |a, b| a.pos - b.pos);
    let lin = grad.first_child("a:lin");
    let angle = lin.and_then(|l| l.attr("ang")).map_or(0.0, |a| pint(a) / 60000.0);
    let mut out = Fill::of_kind(FillKind::Gradient);
    out.stops = Some(stops.into_boxed_slice());
    out.angle_deg = Some(angle);
    if lin.and_then(|l| l.attr("scaled")) == Some("1") {
        out.scaled = Some(true);
    }
    // radial gradient: <a:path path="rect|circle|shape"> (pos 0 = centre)
    if let Some(path_el) = grad.first_child("a:path") {
        out.path = Some(path_el.attr("path").unwrap_or("shape").into());
        if let Some(ftr) = path_el.first_child("a:fillToRect") {
            let n = |k: &str| ftr.attr(k).map_or(0.0, |v| pint(v) / 100000.0);
            out.fill_to = Some(Inset { l: n("l"), t: n("t"), r: n("r"), b: n("b") });
        }
    }
    Some(out)
}

/// A group's own fill (`groupFill`, `read.ts:841`): `None` = none; `<a:grpFill/>` = the group above's.
pub fn group_fill(grp_sp_pr: Element<'_>, theme: &Theme) -> Option<(Fill, Option<f64>)> {
    if grp_sp_pr.first_child("a:noFill").is_some() {
        return Some((Fill::of_kind(FillKind::None), None));
    }
    if let Some(sf) = grp_sp_pr.first_child("a:solidFill") {
        let c = color_from_dml(sf.child_elements().next(), theme)?;
        let alpha = fill_alpha_val(sf).filter(|a| !a.is_empty()).map(|a| pint(a) / 100000.0);
        return Some((solid_fill(c), alpha));
    }
    let g = grp_sp_pr.first_child("a:gradFill").and_then(|g| parse_grad_fill(g, theme))?;
    Some((g, None))
}

/// The mime of an image part by extension (`mimeFromPart`, `read.ts:2297`).
pub fn mime_from_part(part: &str) -> &'static str {
    let ext = match part.rfind('.') {
        Some(d) => part.get(d + 1..).unwrap_or(""),
        None => "",
    };
    match uo_jsnum::text::to_lowercase(ext).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}

/// `readImageFx` (`read.ts:2186`): colour effects of a `<a:blip>`.
pub fn read_image_fx(blip: Element<'_>, theme: &Theme) -> Option<Box<ImageFx>> {
    let mut fx = ImageFx::default();
    let mut any = false;
    for e in blip.child_elements() {
        match e.name() {
            "a:duotone" => {
                let cs: Vec<CssColor> = e.child_elements().filter_map(|c| color_from_dml(Some(c), theme)).collect();
                if let [a, b] = cs.as_slice() {
                    fx.duotone = Some([a.clone(), b.clone()]);
                    any = true;
                }
            }
            "a:alphaModFix" => {
                fx.alpha = Some(pint(e.attr("amt").unwrap_or("100000")) / 100000.0);
                any = true;
            }
            "a:grayscl" => {
                fx.gray = Some(true);
                any = true;
            }
            "a:biLevel" => {
                fx.bi_level = Some(pint(e.attr("thresh").unwrap_or("50000")) / 100000.0);
                any = true;
            }
            "a:lum" => {
                let b = pint(e.attr("bright").unwrap_or("0")) / 100000.0;
                let c = pint(e.attr("contrast").unwrap_or("0")) / 100000.0;
                if b != 0.0 && !b.is_nan() {
                    fx.bright = Some(b);
                    any = true;
                }
                if c != 0.0 && !c.is_nan() {
                    fx.contrast = Some(c);
                    any = true;
                }
            }
            "a:clrChange" => {
                let from = e.first_child("a:clrFrom");
                let to = e.first_child("a:clrTo");
                let fc = from.and_then(|f| color_from_dml(f.child_elements().next(), theme));
                let to_el = to.and_then(|t| t.child_elements().next());
                let tc = color_from_dml(to_el, theme);
                if let (Some(from), Some(to)) = (fc, tc) {
                    let a = to_el.and_then(|t| t.last_child("a:alpha")).and_then(|a| a.attr("val"));
                    fx.clr_change = Some(ClrChange { from, to, to_alpha: a.map_or(1.0, |a| pint(a) / 100000.0) });
                    any = true;
                }
            }
            _ => {}
        }
    }
    any.then(|| Box::new(fx))
}

/// `resolveBlip` (`read.ts:2227`): `<a:blip r:embed>` → the image through the rels (`None` when it
/// does not resolve).
pub fn resolve_blip(blip: Element<'_>, src: &dyn PartSource, rels: &RelMap, theme: Option<&Theme>) -> Option<ImageRef> {
    // SVG icon (2016+): the PNG when it exists; the app sometimes writes ONLY the SVG
    let mut rid: Option<&str> = blip.attr("r:embed").or_else(|| blip.attr("embed"));
    if rid.is_none_or(str::is_empty)
        && let Some(ext) = blip.first_child("a:extLst")
    {
        for e in ext.children_named("a:ext") {
            let svg = e.child_elements().find(|x| x.name().ends_with(":svgBlip"));
            if let Some(v) = svg.and_then(|s| ta(s, "r:embed")) {
                rid = Some(v);
            }
        }
    }
    let rid = rid.filter(|r| !r.is_empty())?;
    let part = rels.get(rid)?;
    src.part(part)?;
    let media = src.media(part)?;
    let mut r = ImageRef { media, mime: mime_from_part(part).into(), fx: None, tile: None, fill_rect: None };
    if let Some(t) = theme {
        r.fx = read_image_fx(blip, t);
    }
    Some(r)
}

/// `applyTile` (`read.ts:2268`): `<a:tile>`/`<a:stretch><a:fillRect>` of a `<a:blipFill>` → the image's
/// tile/fillRect; returns whether the image tiles.
pub fn apply_tile(r: &mut ImageRef, blip_fill: Option<Element<'_>>) -> bool {
    let tile = blip_fill.and_then(|b| b.first_child("a:tile"));
    let fr = blip_fill.and_then(|b| b.first_child("a:stretch")).and_then(|s| s.first_child("a:fillRect"));
    if let Some(fr) = fr
        && ["l", "t", "r", "b"].iter().any(|k| fr.attr(k).is_some_and(|v| !v.is_empty() && v != "0"))
    {
        let v = |k: &str| ta(fr, k).map_or(0.0, |x| pint(x) / 100000.0);
        r.fill_rect = Some(Inset { l: v("l"), t: v("t"), r: v("r"), b: v("b") });
    }
    // neither <a:stretch> nor <a:tile>: PowerPoint TILES at natural size (tdf#128596, measured)
    if tile.is_none()
        && let Some(bf) = blip_fill
        && bf.first_child("a:stretch").is_none()
    {
        r.tile = Some(Box::new(Tile { sx: 1.0, sy: 1.0, tx: 0.0, ty: 0.0, algn: None, flip: None }));
        return true;
    }
    let Some(tile) = tile else { return false };
    let n = |k: &str, d: f64| tile.attr(k).map_or(d, pint);
    let mut t = Tile {
        sx: n("sx", 100000.0) / 100000.0,
        sy: n("sy", 100000.0) / 100000.0,
        tx: n("tx", 0.0),
        ty: n("ty", 0.0),
        algn: None,
        flip: None,
    };
    if let Some(a) = ta(tile, "algn").filter(|a| *a != "tl") {
        t.algn = Some(a.into());
    }
    if let Some(f @ ("x" | "y" | "xy")) = tile.attr("flip") {
        t.flip = Some(f.into());
    }
    r.tile = Some(Box::new(t));
    true
}

/// `mediaMime` (`read.ts:2161`): the mime of a media file by extension.
pub fn media_mime(path: &str, kind: MediaKind) -> &'static str {
    let ext = uo_jsnum::text::to_lowercase(path_ext(path));
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => {
            if kind == MediaKind::Audio {
                "audio/webm"
            } else {
                "video/webm"
            }
        }
        "ogv" => "video/ogg",
        "wmv" => "video/x-ms-wmv",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "wma" => "audio/x-ms-wma",
        "ogg" | "oga" => "audio/ogg",
        "mid" | "midi" => "audio/midi",
        _ => {
            if kind == MediaKind::Audio {
                "audio/*"
            } else {
                "video/*"
            }
        }
    }
}

/// `/\.([a-z0-9]+)(?:[?#].*)?$/i.exec(path)?.[1] ?? ''`.
fn path_ext(path: &str) -> &str {
    let b = path.as_bytes();
    let mut from = 0;
    while let Some(i) = path.get(from..).and_then(|r| r.find('.')) {
        let dot = from + i;
        let s = dot + 1;
        let mut e = s;
        while b.get(e).is_some_and(u8::is_ascii_alphanumeric) {
            e += 1;
        }
        if e > s {
            let rest = path.get(e..).unwrap_or("");
            let ok = rest.is_empty()
                || ((rest.starts_with('?') || rest.starts_with('#'))
                    && !rest.chars().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')));
            if ok {
                return path.get(s..e).unwrap_or("");
            }
        }
        from = dot + 1;
    }
    ""
}

/// `readPicMedia` (`read.ts:2095`): the media of a `<p:pic>`: `<a:videoFile r:link>` /
/// `<a:audioFile r:link>` in `<p:nvPr>` and, in 2010+, the EMBEDDED part in `<p14:media r:embed>`,
/// which has priority.
pub fn read_pic_media(nv_pic_pr: Element<'_>, src: &dyn PartSource, rels: &RelMap) -> Option<MediaRef> {
    let nv_pr = nv_pic_pr.first_child("p:nvPr")?;
    let mut kind: Option<MediaKind> = None;
    let mut link: Option<&str> = None;
    for c in nv_pr.child_elements() {
        match c.name() {
            "a:videoFile" | "a:quickTimeFile" => {
                kind = Some(MediaKind::Video);
                link = c.attr("r:link");
            }
            "a:audioFile" | "a:wavAudioFile" => {
                kind = Some(MediaKind::Audio);
                link = c.attr("r:link").or_else(|| c.attr("r:embed"));
            }
            _ => {}
        }
    }
    let kind = kind?;
    let mut embed: Option<&str> = None;
    let mut p14: Option<Element<'_>> = None;
    if let Some(ext) = nv_pr.first_child("p:extLst") {
        for e in ext.children_named("p:ext") {
            let m = e.child_elements().find(|x| x.name().ends_with(":media"));
            if m.is_some() {
                p14 = m;
            }
            if let Some(v) = m.and_then(|m| ta(m, "r:embed")) {
                embed = Some(v);
            }
        }
    }
    let mut media =
        MediaRef { kind, part: None, url: None, mime: "application/octet-stream".into(), trim: None, fade: None, bookmarks: None };
    if let Some(p14) = p14 {
        // trim, fade and bookmarks (PowerPoint 2010+)
        let ms = |v: Option<&str>| -> f64 {
            match v {
                Some(s) if !s.is_empty() => {
                    let n = uo_jsnum::parse::parse_float(s);
                    if n.is_nan() || n == 0.0 { 0.0 } else { n }
                }
                _ => 0.0,
            }
        };
        for c in p14.child_elements() {
            let name = c.name();
            let tag = name.get(name.find(':').map_or(0, |i| i + 1)..).unwrap_or(name);
            match tag {
                "trim" => media.trim = Some((ms(c.attr("st")), ms(c.attr("end")))),
                "fade" => media.fade = Some((ms(c.attr("in")), ms(c.attr("out")))),
                "bmkLst" => {
                    media.bookmarks = Some(
                        c.child_elements()
                            .filter(|b| b.name().ends_with(":bmk"))
                            .map(|b| MediaBookmark { name: b.attr("name").unwrap_or("").into(), time_ms: ms(b.attr("time")) })
                            .collect(),
                    )
                }
                _ => {}
            }
        }
    }
    for rid in [embed, link] {
        let Some(target) = rid.filter(|r| !r.is_empty()).and_then(|r| rels.get(r)).filter(|t| !t.is_empty()) else { continue };
        if src.part(target).is_some() {
            media.part = Some(target.into());
            break;
        }
        if has_scheme(target) && media.url.is_none() {
            media.url = Some(target.into());
        }
    }
    if media.part.is_none() && media.url.is_none() {
        return None;
    }
    let path = media.part.as_deref().or(media.url.as_deref()).unwrap_or("");
    media.mime = media_mime(path, kind).into();
    Some(media)
}

/// `/^[a-z][a-z0-9+.-]*:/i.test(s)`.
fn has_scheme(s: &str) -> bool {
    let b = s.as_bytes();
    if !b.first().is_some_and(u8::is_ascii_alphabetic) {
        return false;
    }
    let mut i = 1;
    while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-')) {
        i += 1;
    }
    b.get(i) == Some(&b':')
}

/// The effects of a shape (`readEffects`, `read.ts:2038`).
#[derive(Default)]
pub struct Effects {
    pub shadow: Option<OuterShadow>,
    pub inner_shadow: Option<InnerShadow>,
    pub glow: Option<Glow>,
    pub soft_edge: Option<f64>,
    pub fill_overlay: Option<FillOverlay>,
    pub reflection: Option<Reflection>,
}

/// `readEffects`: the spPr `<a:effectLst>` (empty = none) or, without its own, the theme's
/// `<a:effectStyle>` pointed at by the effectRef of `<p:style>` (colour `phClr` = the reference's).
pub fn read_effects(sp_pr: Option<Element<'_>>, style_el: Option<Element<'_>>, theme: &Theme) -> Effects {
    let mut out = Effects::default();
    let mut eff = sp_pr.and_then(|s| s.first_child("a:effectLst"));
    let mut th: std::borrow::Cow<'_, Theme> = std::borrow::Cow::Borrowed(theme);
    if eff.is_none()
        && !sp_pr.is_some_and(|s| s.first_child("a:effectDag").is_some())
        && let Some(style_el) = style_el
    {
        let r = style_el.first_child("a:effectRef");
        let idx = r.map_or(0.0, |r| pint(r.attr("idx").unwrap_or("0")));
        let st = if idx > 0.0 { theme.styles.as_ref().and_then(|s| s.effect_style(usize_of(idx - 1.0)?)) } else { None };
        eff = st.and_then(|s| s.first_child("a:effectLst"));
        let ph = r.and_then(|r| color_from_dml(r.child_elements().next(), theme));
        if let Some(ph) = ph {
            let mut t = theme.clone();
            t.colors.set("phClr", ph);
            th = std::borrow::Cow::Owned(t);
        }
    }
    let Some(eff) = eff else { return out };
    let th: &Theme = &th;
    let num = |e: Element<'_>, k: &str, d: f64| e.attr(k).map_or(d, pint);
    let color_of = |e: Element<'_>| -> (CssColor, f64) {
        let c = e.child_elements().next();
        let a = c.and_then(|c| c.last_child("a:alpha")).and_then(|a| a.attr("val"));
        (c.and_then(|c| color_from_dml(Some(c), th)).unwrap_or_else(|| CssColor::new("#000000")), a.map_or(1.0, |a| pint(a) / 100000.0))
    };
    // preset shadow (prstShdw) ≈ outer without blur
    if let Some(shd) = eff.first_child("a:outerShdw").or_else(|| eff.first_child("a:prstShdw")) {
        let (color, alpha) = color_of(shd);
        let mut s = OuterShadow {
            color,
            blur_rad: num(shd, "blurRad", 0.0),
            dist: num(shd, "dist", 0.0),
            dir_deg: num(shd, "dir", 5400000.0) / 60000.0,
            alpha,
            sx: None,
            sy: None,
            algn: None,
        };
        // SCALED shadow (sx/sy) anchored (algn): e.g. 93% at the top = only the base shows
        let ssx = num(shd, "sx", 100000.0) / 100000.0;
        let ssy = num(shd, "sy", 100000.0) / 100000.0;
        if ssx != 1.0 || ssy != 1.0 {
            s.sx = Some(ssx);
            s.sy = Some(ssy);
            s.algn = Some(shd.attr("algn").unwrap_or("b").into());
        }
        out.shadow = Some(s);
    }
    if let Some(inner) = eff.first_child("a:innerShdw") {
        let (color, alpha) = color_of(inner);
        out.inner_shadow = Some(InnerShadow {
            color,
            alpha,
            blur_rad: num(inner, "blurRad", 0.0),
            dist: num(inner, "dist", 0.0),
            dir_deg: num(inner, "dir", 0.0) / 60000.0,
        });
    }
    if let Some(glow) = eff.first_child("a:glow") {
        let (color, alpha) = color_of(glow);
        if num(glow, "rad", 0.0) > 0.0 {
            out.glow = Some(Glow { rad: num(glow, "rad", 0.0), color, alpha });
        }
    }
    if let Some(soft) = eff.first_child("a:softEdge")
        && num(soft, "rad", 0.0) > 0.0
    {
        out.soft_edge = Some(num(soft, "rad", 0.0));
    }
    // fill overlay (2007 themes: texture × gradient in multiplication)
    if let Some(ov) = eff.first_child("a:fillOverlay") {
        let g = ov.first_child("a:gradFill").and_then(|g| parse_grad_fill(g, th));
        let fill = match g {
            Some(g) => Some(g),
            None => solid_fill_color(ov, th).map(solid_fill),
        };
        if let Some(fill) = fill {
            out.fill_overlay = Some(FillOverlay { blend: ov.attr("blend").unwrap_or("over").into(), fill });
        }
    }
    if let Some(refl) = eff.first_child("a:reflection") {
        out.reflection = Some(Reflection {
            blur_rad: num(refl, "blurRad", 0.0),
            st_a: num(refl, "stA", 100000.0) / 100000.0,
            end_a: num(refl, "endA", 0.0) / 100000.0,
            st_pos: num(refl, "stPos", 0.0) / 100000.0,
            end_pos: num(refl, "endPos", 100000.0) / 100000.0,
            dist: num(refl, "dist", 0.0),
            dir_deg: num(refl, "dir", 5400000.0) / 60000.0,
            sy: num(refl, "sy", 100000.0) / 100000.0,
            fade_dir_deg: num(refl, "fadeDir", 5400000.0) / 60000.0,
        });
    }
    out
}

/// A non-negative integral f64 as an index.
pub fn usize_of(x: f64) -> Option<usize> {
    uo_jsnum::conv::js_index(x, usize::MAX)
}

/// `readBevel` (`read.ts:1995`): 3D bevel `<a:sp3d><a:bevelT>` + `<a:scene3d><a:lightRig>` of the spPr
/// or, without its own scene/sp3d, of the theme's effectStyle pointed at by the effectRef.
pub fn read_bevel(sp_pr: Option<Element<'_>>, style_el: Option<Element<'_>>, theme: &Theme, shape: &mut Shape) {
    let mut sp3d = sp_pr.and_then(|s| s.first_child("a:sp3d"));
    let mut scene = sp_pr.and_then(|s| s.first_child("a:scene3d"));
    if sp3d.is_none()
        && scene.is_none()
        && let Some(style_el) = style_el
    {
        let r = style_el.first_child("a:effectRef");
        let idx = r.map_or(0.0, |r| pint(r.attr("idx").unwrap_or("0")));
        let st = if idx > 0.0 { theme.styles.as_ref().and_then(|s| s.effect_style(usize_of(idx - 1.0)?)) } else { None };
        if let Some(st) = st {
            sp3d = st.first_child("a:sp3d");
            scene = st.first_child("a:scene3d");
        }
    }
    let rig = scene.and_then(|s| s.first_child("a:lightRig"));
    let cam = scene.and_then(|s| s.first_child("a:camera"));
    if let Some(p) = cam.and_then(|c| ta(c, "prst")).filter(|p| *p != "orthographicFront") {
        shape.extras_mut().cam_prst = Some(p.into());
    }
    let rig_rig = rig.and_then(|r| ta(r, "rig")).map(Box::<str>::from);
    let rig_dir = rig.and_then(|r| ta(r, "dir")).map(Box::<str>::from);
    let mat = sp3d.and_then(|s| ta(s, "prstMaterial")).map(Box::<str>::from);
    // extrusion: sides in their own colour (or the fill's), with the scene light
    let eh = sp3d.and_then(|s| int_attr(s, "extrusionH")).unwrap_or(0.0);
    if eh > 0.0
        && let Some(sp3d) = sp3d
    {
        let color = sp3d.first_child("a:extrusionClr").and_then(|ec| color_from_dml(ec.child_elements().next(), theme));
        shape.extras_mut().extrusion = Some(Extrusion { h: eh, color, rig: rig_rig.clone(), dir: rig_dir.clone(), mat: mat.clone() });
    }
    let Some(bt) = sp3d.and_then(|s| s.first_child("a:bevelT")) else { return };
    let w = int_attr(bt, "w").unwrap_or(76200.0);
    let h = int_attr(bt, "h").unwrap_or(76200.0);
    if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
        return;
    }
    shape.extras_mut().bevel = Some(Bevel { w, h, prst: ta(bt, "prst").map(Into::into), rig: rig_rig, dir: rig_dir, mat });
}

impl Shape {
    /// The boxed rare payloads, created on first use.
    pub fn extras_mut(&mut self) -> &mut ShapeExtras {
        self.extras.get_or_insert_with(Default::default)
    }

    /// The effects `readEffects` found, written into the shape.
    pub fn set_effects(&mut self, e: Effects) {
        if e.shadow.is_some()
            || e.inner_shadow.is_some()
            || e.glow.is_some()
            || e.soft_edge.is_some()
            || e.fill_overlay.is_some()
            || e.reflection.is_some()
        {
            let x = self.extras_mut();
            if e.shadow.is_some() {
                x.shadow = e.shadow;
            }
            if e.inner_shadow.is_some() {
                x.inner_shadow = e.inner_shadow;
            }
            if e.glow.is_some() {
                x.glow = e.glow;
            }
            if e.soft_edge.is_some() {
                x.soft_edge = e.soft_edge;
            }
            if e.fill_overlay.is_some() {
                x.fill_overlay = e.fill_overlay;
            }
            if e.reflection.is_some() {
                x.reflection = e.reflection;
            }
        }
    }
}

/// `readRels` for a part's own relationships.
pub(crate) fn rels_of(src: &dyn PartSource, part: &str) -> R<RelMap> {
    read_rels(src, &rels_path_for(part), part)
}

/// `parseCustGeom` (`read.ts:1484`): `<a:custGeom>` → scalable paths. Coordinates stay in the units of
/// the path's own `<a:path w h>`; the renderer scales them to the shape's xfrm.
pub fn parse_cust_geom(cust: Element<'_>, shape_w: Option<f64>, shape_h: Option<f64>) -> Option<CustGeom> {
    let path_lst = cust.first_child("a:pathLst")?;
    let mut subs: Vec<Vec<PathSeg>> = Vec::new();
    let mut no_fill: Vec<bool> = Vec::new();
    let mut no_stroke: Vec<bool> = Vec::new();
    // fill="darken|darkenLess|lighten|lightenLess": darker/lighter face (as in the presets)
    let mut lum: Vec<f64> = Vec::new();
    let shade = |f: &str| -> f64 {
        match f {
            "darken" => -0.4,
            "darkenLess" => -0.2,
            "lighten" => 0.4,
            "lightenLess" => 0.2,
            _ => 0.0,
        }
    };
    let mut sizes: Vec<(f64, f64)> = Vec::new();
    let (mut pw, mut ph) = (0.0_f64, 0.0_f64);
    // formula guides (avLst/gdLst — LibreOffice writes presets this way): the points cite guides by
    // name, evaluated at the shape's size (EMU); a path without w/h is in that same system
    let sw = shape_w.filter(|w| *w > 0.0).unwrap_or(0.0);
    let sh = shape_h.filter(|h| *h > 0.0).unwrap_or(0.0);
    let collect = |el: Option<Element<'_>>| -> Vec<(String, Vec<String>)> {
        el.map(|e| {
            e.children_named("a:gd")
                .take(2000)
                .map(|g| {
                    let f = trim(g.attr("fmla").unwrap_or("")).to_string();
                    (g.attr("name").unwrap_or("").to_string(), uo_jsnum::text::split_js_space(&f).map(str::to_string).collect())
                })
                .collect()
        })
        .unwrap_or_default()
    };
    let av = collect(cust.first_child("a:avLst"));
    let gd = collect(cust.first_child("a:gdLst"));
    fn fmla_of(v: &[(String, Vec<String>)]) -> Vec<Vec<&str>> {
        v.iter().map(|(_, f)| f.iter().map(String::as_str).collect()).collect()
    }
    let (av_f, gd_f) = (fmla_of(&av), fmla_of(&gd));
    let av_defs: Vec<GuideDef<'_>> = av.iter().zip(av_f.iter()).map(|((n, _), f)| GuideDef { name: n, fmla: f }).collect();
    let gd_defs: Vec<GuideDef<'_>> = gd.iter().zip(gd_f.iter()).map(|((n, _), f)| GuideDef { name: n, fmla: f }).collect();
    let guides = custom_guides(
        &av_defs,
        &gd_defs,
        if sw != 0.0 { sw } else { 21600.0 },
        if sh != 0.0 { sh } else { 21600.0 },
        &uo_drawingml::Limits::DEFAULT,
    );
    let num = |t: Option<&str>| -> f64 {
        match t {
            None => 0.0,
            Some(t) => {
                if is_int_text(t) {
                    pint(t)
                } else {
                    guides.val(t)
                }
            }
        }
    };
    let pt = |el: Option<Element<'_>>| -> (f64, f64) { (num(el.and_then(|e| e.attr("x"))), num(el.and_then(|e| e.attr("y")))) };
    for path in path_lst.children_named("a:path") {
        if let Some(w) = ta(path, "w") {
            pw = math::max(pw, pint(w));
        }
        if let Some(h) = ta(path, "h") {
            ph = math::max(ph, pint(h));
        }
        let mut segs: Vec<PathSeg> = Vec::new();
        let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
        for el in path.child_elements() {
            let pts: Vec<Element<'_>> = el.children_named("a:pt").collect();
            match el.name() {
                "a:moveTo" => {
                    let p = pt(pts.first().copied());
                    segs.push(PathSeg::M { x: p.0, y: p.1 });
                    (cx, cy) = p;
                }
                "a:lnTo" => {
                    let p = pt(pts.first().copied());
                    segs.push(PathSeg::L { x: p.0, y: p.1 });
                    (cx, cy) = p;
                }
                "a:cubicBezTo" => {
                    let p1 = pt(pts.first().copied());
                    let p2 = pt(pts.get(1).copied());
                    let p3 = pt(pts.get(2).copied());
                    segs.push(PathSeg::C { x1: p1.0, y1: p1.1, x2: p2.0, y2: p2.1, x: p3.0, y: p3.1 });
                    (cx, cy) = p3;
                }
                "a:quadBezTo" => {
                    let p1 = pt(pts.first().copied());
                    let p2 = pt(pts.get(1).copied());
                    segs.push(PathSeg::Q { x1: p1.0, y1: p1.1, x: p2.0, y: p2.1 });
                    (cx, cy) = p2;
                }
                "a:arcTo" => {
                    let a = arc_to_seg(cx, cy, num(el.attr("wR")), num(el.attr("hR")), num(el.attr("stAng")), num(el.attr("swAng")));
                    if let Some((seg, x, y)) = a {
                        segs.push(seg);
                        (cx, cy) = (x, y);
                    }
                }
                "a:close" => segs.push(PathSeg::Z),
                _ => {}
            }
        }
        if !segs.is_empty() {
            subs.push(segs);
            let own_w = path.attr("w").map_or(0.0, |w| nz(pint(w)));
            let own_h = path.attr("h").map_or(0.0, |h| nz(pint(h)));
            // a path without w/h: coordinates in the shape's size
            if own_w == 0.0 && sw != 0.0 {
                pw = math::max(pw, sw);
            }
            if own_h == 0.0 && sh != 0.0 {
                ph = math::max(ph, sh);
            }
            sizes.push((if own_w != 0.0 { own_w } else { sw }, if own_h != 0.0 { own_h } else { sh }));
            no_fill.push(path.attr("fill") == Some("none"));
            lum.push(shade(path.attr("fill").unwrap_or("")));
            no_stroke.push(matches!(path.attr("stroke"), Some("0" | "false")));
        }
    }
    if subs.is_empty() {
        return None;
    }
    // text rectangle (<a:rect l t r b>, guides at the shape's size) as a fraction of the box
    let mut text_rect: Option<Inset> = None;
    if let Some(rc) = cust.first_child("a:rect")
        && sw != 0.0
        && sh != 0.0
    {
        let g = |k: &str, d: &str| num(Some(rc.attr(k).unwrap_or(d)));
        let (l, t, r, b) = (g("l", "0"), g("t", "0"), g("r", "r"), g("b", "b"));
        if r > l && b > t && (l > 1.0 || t > 1.0 || r < sw - 1.0 || b < sh - 1.0) {
            text_rect = Some(Inset { l: l / sw, t: t / sh, r: r / sw, b: b / sh });
        }
    }
    // each <a:path> has its own w×h system: take all of them to the largest (what the shape stretches)
    for (segs, size) in subs.iter_mut().zip(sizes.iter()) {
        let (wi, hi) = *size;
        let kx = if wi > 0.0 && pw > 0.0 && wi != pw { pw / wi } else { 1.0 };
        let ky = if hi > 0.0 && ph > 0.0 && hi != ph { ph / hi } else { 1.0 };
        if kx == 1.0 && ky == 1.0 {
            continue;
        }
        for g in segs.iter_mut() {
            match g {
                PathSeg::M { x, y } | PathSeg::L { x, y } => {
                    *x *= kx;
                    *y *= ky;
                }
                PathSeg::C { x1, y1, x2, y2, x, y } => {
                    *x *= kx;
                    *y *= ky;
                    *x1 *= kx;
                    *y1 *= ky;
                    *x2 *= kx;
                    *y2 *= ky;
                }
                PathSeg::Q { x1, y1, x, y } => {
                    *x *= kx;
                    *y *= ky;
                    *x1 *= kx;
                    *y1 *= ky;
                }
                PathSeg::E { .. } | PathSeg::Z => {}
            }
        }
    }
    let mut sp = Subpaths::new();
    for segs in subs {
        for s in segs {
            sp.push(s);
        }
        sp.end_sub();
    }
    Some(CustGeom {
        w: if pw != 0.0 { pw } else { 1.0 },
        h: if ph != 0.0 { ph } else { 1.0 },
        subs: sp,
        no_fill: no_fill.iter().any(|b| *b).then(|| no_fill.into_boxed_slice()),
        no_stroke: no_stroke.iter().any(|b| *b).then(|| no_stroke.into_boxed_slice()),
        lum: lum.iter().any(|v| *v != 0.0).then(|| lum.into_boxed_slice()),
        text_rect,
    })
}

/// `x || 0`: NaN becomes 0.
fn nz(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}
