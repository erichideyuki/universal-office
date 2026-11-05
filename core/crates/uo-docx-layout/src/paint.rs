//! Painting lines, boxes and borders into display items (`paint.ts`, frontier F5): glyph runs
//! (merged by the TS string key, whose face part is `face.key`), backgrounds, tab leaders as Word
//! places them, equations, check boxes, inline objects, the VML `o:hr`, underlines and strikes,
//! paragraph and page borders (`paintBorder`, `borderWidth`, the composite bands, waves).
//!
//! Emits into [`PassItems`] with the `lineBox` side data (RQ2) and the per-call `lineSeq`
//! ([`LineSeq`], LA3). Ported 1:1 in the TS order of operations; what the TS reads of
//! `face.font`, of the document settings and of the run objects goes through [`PaintEnv`],
//! implemented by the engine's [`Cx`] and by the parity replay of the `p3.paint` facet (record
//! "Stage 3 — F5", F5-1). Colours stay the strings the document wrote, interned (RQ6).

use crate::engine::Cx;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::fonts::FaceMetrics;
use crate::items::{ItemSide, PassItems};
use crate::keys::{LineSeq, RunKey};
use crate::ldoc::{LDoc, RunSrc};
use crate::lines::{BreakSettings, Line, LineEnv, PlacedGlyph, glyph_of};
use crate::math::MathBox;
use crate::model::{BorderSpec, ColorRef, FontSlot, PBdr, PageBorders, RPr, Run, font_of_slot, resolve_color, shading_fill};
use crate::output::{GlyphMeta, LineBox, TabLeader};
use crate::para::{Glyph, GlyphKind, PreparedPara, RunStyle, WORD_UNIT};
use crate::underline::{UlFont, paint_underlines};
use uo_jsnum::math as jm;
use uo_render::types::{LineDash, PathCmd, RunFlags, Stroke};
use uo_render::{FaceId, GlyphRunHead, Item, OptF64, Sym, TextFxId};

/// Where a line is painted: the frame's left edge, the line's top and the page context.
#[derive(Clone, Copy, Debug)]
pub struct LinePlacement {
    pub frame_x: f64,
    pub top: f64,
    /// the line is in a header/footer (not read by `paintLine`; the engine's)
    pub hf: bool,
}

/// The `o:hr` shape of a horizontal-rule glyph (`g.obj.shape`, the fields `paintHr` reads).
#[derive(Clone, Debug)]
pub struct HrShape {
    pub hr_pct: Option<f64>,
    pub width_twips: f64,
    pub height_twips: f64,
    pub hr_align: Option<Box<str>>,
    /// `sh.style.fill ?? 'A0A0A0'`
    pub fill: Box<[u16]>,
    pub hr_no_shade: bool,
}

/// What painting reads outside the line and its prepared paragraph (the TS `ctx`, `face.font` and
/// the run objects): implemented by the engine's [`Cx`] and by the facet replay.
pub trait PaintEnv {
    /// `faceMetrics(face.font)` (F1)
    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics>;
    /// what `ulMetrics` reads of `face.font` at pixel size `em`
    fn ul_font(&mut self, face: FaceId, em: f64) -> LResult<UlFont>;
    /// `(f.advanceWidth(f.glyphId(cp)), f.metrics.unitsPerEm)` (tab leaders)
    fn advance_units(&mut self, face: FaceId, cp: u32) -> LResult<(f64, f64)>;
    /// `!!f.hasColorGlyphs?.() && !!f.colorLayers?.(f.glyphId(cp))`
    fn colr(&mut self, face: FaceId, cp: u32) -> LResult<bool>;
    /// `face.key`, interned
    fn face_key(&mut self, face: FaceId) -> LResult<Sym>;
    /// `(!!face.synthBold, !!face.synthItalic)`
    fn face_synth(&mut self, face: FaceId) -> LResult<(bool, bool)>;
    /// `!!fx.outline || !!fx.glow || !!fx.shadow || !!fx.reflection` of a resolved effect set
    fn fx_draws(&self, fx: TextFxId) -> bool;
    /// interns a colour or name
    fn intern(&mut self, s: &[u16]) -> LResult<Sym>;
    /// the code units of an interned string
    fn sym_text(&self, s: Sym) -> Vec<u16>;
    /// `ctx.fmt.settings` (the compatibility mode)
    fn settings(&self) -> BreakSettings;
    /// `!!ctx.fmt.settings.compat['useWord97LineBreakRules']`
    fn word97_line_breaks(&self) -> bool;
    /// `!!fontOfSlot(rpr.rFonts, 'eastAsia', ctx.fmt.theme)` (resolve.ts, F2)
    fn east_asia_font(&mut self, rpr: &RPr) -> LResult<bool>;
    /// `++lineSeq`
    fn next_line_seq(&mut self) -> LResult<LineSeq>;
    /// `paintInlineObject(ctx, out, g, x0, baseY0)` (paint.ts:196)
    fn inline_object(&mut self, p: &PreparedPara, g: &Glyph, x0: f64, base_y0: f64, out: &mut PassItems) -> LResult<()>;
    /// `g.obj?.shape` of a horizontal-rule glyph (`None`: no shape)
    fn hr_shape(&mut self, p: &PreparedPara, g: &Glyph) -> LResult<Option<HrShape>>;
    /// charges the work budget (loops whose length comes from coordinates)
    fn charge(&mut self, units: u64) -> LResult<()>;
}

// ── PassItems with their side data ──────────────────────────────────────────────────────────

/// `out.push(item)` for an item without columns.
pub(crate) fn push_item(out: &mut PassItems, it: Item) {
    out.list.push_plain(it);
    out.side.push(ItemSide::default());
}

/// `out.push({ t: 'path', … })`.
pub(crate) fn push_path(out: &mut PassItems, cmds: &[PathCmd], fill: Option<Sym>, stroke: Option<Stroke>, even_odd: bool) -> LResult<()> {
    out.list.push_path(cmds, fill, stroke, even_odd)?;
    out.side.push(ItemSide::default());
    Ok(())
}

fn rect(out: &mut PassItems, x: f64, y: f64, w: f64, h: f64, fill: Sym) {
    push_item(out, Item::Rect { x, y, w, h, fill, alpha: OptF64::NONE });
}

fn line_item(out: &mut PassItems, [x1, y1, x2, y2]: [f64; 4], color: Sym, width_pt: f64, dash: Option<LineDash>) {
    push_item(out, Item::Line { x1, y1, x2, y2, color, width_pt, dash });
}

/// `String.fromCodePoint(cp)` appended to `text` (a lone surrogate value stays one unit; above
/// U+10FFFF the TS throws `RangeError`).
fn push_code_point(text: &mut Vec<u16>, cp: u32) -> LResult<()> {
    if cp < 0x1_0000 {
        text.push(u16::try_from(cp).map_err(|_| LayoutError::Internal("paint: code point"))?);
    } else if cp <= 0x10_ffff {
        let v = cp - 0x1_0000;
        text.push(u16::try_from(0xd800 + (v >> 10)).map_err(|_| LayoutError::Internal("paint: code point"))?);
        text.push(u16::try_from(0xdc00 + (v & 0x3ff)).map_err(|_| LayoutError::Internal("paint: code point"))?);
    } else {
        return Err(LayoutError::Internal("paint: String.fromCodePoint RangeError"));
    }
    Ok(())
}

/// A glyph run being built (`GlyphRunItem` before `out.push(run)`).
struct RunBuf {
    face: FaceId,
    size_pt: f64,
    color: Sym,
    y: f64,
    scale_x: OptF64,
    alpha: OptF64,
    fx: Option<TextFxId>,
    flags: RunFlags,
    line_box: Option<LineBox>,
    glyphs: Vec<(u32, f64, Option<u32>)>,
    text: Vec<u16>,
}

impl RunBuf {
    fn new(face: FaceId, size_pt: f64, color: Sym, y: f64) -> RunBuf {
        RunBuf {
            face,
            size_pt,
            color,
            y,
            scale_x: OptF64::NONE,
            alpha: OptF64::NONE,
            fx: None,
            flags: RunFlags::default(),
            line_box: None,
            glyphs: Vec::new(),
            text: Vec::new(),
        }
    }

    /// `out.push(run)`.
    fn push(self, out: &mut PassItems) -> LResult<()> {
        let head = GlyphRunHead {
            face: self.face,
            color: self.color,
            size_pt: self.size_pt,
            y: self.y,
            text: &self.text,
            scale_x: self.scale_x,
            alpha: self.alpha,
            fx: self.fx,
            flags: self.flags,
        };
        out.list.push_glyphs(head, self.glyphs.iter().copied())?;
        let glyph = self.line_box.map(|line_box| GlyphMeta { line_box: Some(line_box), line_num: false });
        out.side.push(ItemSide { glyph, ..ItemSide::default() });
        Ok(())
    }
}

/// The run key of `paintLine` (paint.ts:62), compared as the TS compares the template string:
/// component by component (numbers by their JS string: `-0` and `0` alike, NaN equal to NaN). The
/// string components (face key, colour, `fxKey`) could only make two different tuples print the
/// same string if one of them contained `|`; then the full strings are compared (F5-5).
#[derive(Clone, PartialEq)]
struct RunKeyParts {
    face_key: Sym,
    synth: (bool, bool),
    size: u64,
    color: Sym,
    shift: u64,
    w: u64,
    alpha: u64,
    colr: bool,
    /// `st.fxKey ?? ''`
    fx_key: Box<str>,
    gid: bool,
}

fn num_eq_key(v: f64) -> u64 {
    if v == 0.0 { 0 } else { uo_jsnum::conv::canonicalize_nan(v).to_bits() }
}

impl RunKeyParts {
    /// The full TS string, built only when a string component contains `|`.
    fn text(&self, nums: [f64; 4], color: &[u16], key: &[u16]) -> Vec<u16> {
        let put = |t: &str, s: &mut Vec<u16>| s.extend(t.encode_utf16());
        let mut s: Vec<u16> = key.to_vec();
        put("|", &mut s);
        put(if self.synth.0 { "1" } else { "0" }, &mut s);
        put(if self.synth.1 { "1" } else { "0" }, &mut s);
        put("|", &mut s);
        put(&uo_jsnum::format::to_js_string(nums[0]), &mut s);
        put("|", &mut s);
        s.extend_from_slice(color);
        for v in &nums[1..] {
            put("|", &mut s);
            put(&uo_jsnum::format::to_js_string(*v), &mut s);
        }
        put("|", &mut s);
        put(if self.colr { "c" } else { "" }, &mut s);
        put("|", &mut s);
        put(&self.fx_key, &mut s);
        put("|", &mut s);
        put(if self.gid { "g" } else { "" }, &mut s);
        s
    }
}

/// `paintLine(ctx, out, line, frameX, top)` through the engine's context.
pub fn paint_line(cx: &mut Cx<'_, '_>, p: &PreparedPara, line: &Line, at: LinePlacement, out: &mut PassItems) -> LResult<()> {
    paint_line_in(cx, p, line, at.frame_x, at.top, out)
}

/// `paintLine(ctx, out, line, frameX, top)` (paint.ts:19): a text line → display items (grouped
/// glyphs, underlines, highlights).
pub fn paint_line_in<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    line: &Line,
    frame_x: f64,
    top: f64,
    out: &mut PassItems,
) -> LResult<()> {
    let base_y = top + line.baseline;
    let line_id = env.next_line_seq()?;
    let all: Vec<PlacedGlyph> = line.marker.iter().flatten().chain(line.placed.iter()).copied().collect();
    let glyph = |q: &PlacedGlyph| glyph_of(p, q.g).ok_or(LayoutError::Internal("paint: placed glyph out of range"));
    let style = |g: &Glyph| p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: glyph style"));
    // backgrounds: run shading and highlight (behind the text)
    for pg in &all {
        let g = glyph(pg)?;
        let st = style(&g)?;
        // `st.highlight ?? st.shading`, then `!fill`
        let Some(fill) = st.highlight.as_deref().or(st.shading.as_deref()) else { continue };
        if fill.is_empty() || pg.w <= 0.0 || g.kind == GlyphKind::Lb || g.kind == GlyphKind::Pb {
            continue;
        }
        let m = env.face_metrics(g.face)?;
        let asc = m.ascent * st.size_pt * 20.0;
        let desc = m.descent * st.size_pt * 20.0;
        let fill = env.intern(&fill.encode_utf16().collect::<Vec<u16>>())?;
        rect(out, frame_x + pg.x, base_y - asc, pg.w, asc + desc, fill);
    }
    // glyphs, grouped by face/size/colour/shift
    let mut run: Option<RunBuf> = None;
    let mut run_key: Option<(RunKeyParts, Option<Vec<u16>>)> = None;
    let flush = |run: &mut Option<RunBuf>, run_key: &mut Option<(RunKeyParts, Option<Vec<u16>>)>, out: &mut PassItems| -> LResult<()> {
        if let Some(r) = run.take()
            && !r.glyphs.is_empty()
        {
            r.push(out)?;
        }
        *run_key = None;
        Ok(())
    };
    for (k, pg) in all.iter().enumerate() {
        let g = glyph(pg)?;
        if let Some(mi) = g.math {
            flush(&mut run, &mut run_key, out)?;
            let mb = p.math.get(mi as usize).ok_or(LayoutError::Internal("paint: g.math"))?;
            paint_math(env, p, out, mb, frame_x, pg.x, base_y, line, line_id)?;
            continue;
        }
        if g.kind != GlyphKind::Char && g.kind != GlyphKind::Nbsp && !(g.kind == GlyphKind::Shy && pg.w > 0.0) {
            if g.kind == GlyphKind::Space {
                continue;
            }
            flush(&mut run, &mut run_key, out)?;
            if g.kind == GlyphKind::Tab && pg.leader.is_some() {
                paint_leader(env, p, out, &all, k, frame_x, base_y)?;
            }
            // (a picture bullet's glyph has no model run: the TS one is a run built by
            // `prepareParagraph`, F4 — painted the same way)
            if g.kind == GlyphKind::Obj && (g.obj.is_some() || g.pic_bullet.is_some()) && g.adv > 0.0 {
                env.inline_object(p, &g, frame_x + pg.x, base_y, out)?;
            }
            if g.kind == GlyphKind::Obj
                && g.hr.is_some()
                && let (Some(sh), Some(fr)) = (env.hr_shape(p, &g)?, line.hr_frame)
            {
                paint_hr(env, p, out, &g, &sh, frame_x, fr, base_y)?;
            }
            if g.kind == GlyphKind::Box
                && let Some(b) = g.check_box
            {
                paint_check_box(env, p, out, &g, b, frame_x + pg.x, base_y)?;
            }
            continue;
        }
        // variation selector (U+FE0E/FE0F, emoji): no glyph
        if (0xfe00..=0xfe0f).contains(&g.cp) {
            continue;
        }
        // typographic spaces handled as characters (FORMTEXT result): blank — without the .notdef
        // of a font that lacks them (corpus t43)
        if (0x2000..=0x200a).contains(&g.cp) || g.cp == 0x3000 {
            continue;
        }
        let st = style(&g)?;
        // text WITHOUT fill (w14:noFill / alpha 100%) and without outline: Word neither draws nor
        // writes it to the PDF (oracle hidetext/tf-nofill, tf-alpha100)
        if st.alpha.get() == Some(0.0) && !st.outline && !st.fx.is_some_and(|fx| env.fx_draws(fx)) {
            flush(&mut run, &mut run_key, out)?;
            continue;
        }
        // colour emoji (COLR): Word draws the coloured layers, no extractable text (oracle
        // hidetext/emoji)
        let colr = env.colr(g.face, g.cp)?;
        let face_key = env.face_key(g.face)?;
        let synth = env.face_synth(g.face)?;
        let color = env.intern(&st.color)?;
        // `st.rpr.w ?? 100`
        let w_num = st.rpr.w.and_then(crate::model::JsNumber::as_f64);
        let w_key = w_num.unwrap_or(100.0);
        let alpha_key = st.alpha.or(1.0);
        let parts = RunKeyParts {
            face_key,
            synth,
            size: num_eq_key(g.size_pt),
            color,
            shift: num_eq_key(st.shift),
            w: num_eq_key(w_key),
            alpha: num_eq_key(alpha_key),
            colr,
            fx_key: st.fx_key.clone().unwrap_or_default(),
            gid: g.sub_gid.is_some(),
        };
        let bar = u16::from(b'|');
        let key_units: Vec<u16> = env.sym_text(face_key);
        let ambiguous = key_units.contains(&bar) || st.color.contains(&bar) || st.fx_key.as_deref().is_some_and(|f| f.contains('|'));
        let full = if ambiguous { Some(parts.text([g.size_pt, st.shift, w_key, alpha_key], &st.color, &key_units)) } else { None };
        let same = match &run_key {
            Some((prev, prev_full)) => match (prev_full, &full) {
                (Some(a), Some(b)) => a == b,
                (None, None) => *prev == parts,
                // one side ambiguous: equal strings would need the same components
                _ => false,
            },
            None => false,
        };
        if !same {
            flush(&mut run, &mut run_key, out)?;
            let mut r = RunBuf::new(g.face, g.size_pt, color, base_y - st.shift);
            // `st.rpr.w && st.rpr.w !== 100`
            if let Some(w) = w_num.filter(|&w| uo_jsnum::conv::truthy(w) && w != 100.0) {
                r.scale_x = OptF64::some(w / 100.0);
            }
            r.flags = r.flags.with(RunFlags::SYNTH_BOLD, synth.0).with(RunFlags::SYNTH_ITALIC, synth.1);
            if let Some(a) = st.alpha.get() {
                r.alpha = OptF64::some(a);
                // translucent: Word draws CURVES, not text (oracle hidetext/tf-alpha50)
                if a > 0.0 && a < 1.0 {
                    r.flags = r.flags.with(RunFlags::AS_PATH, true);
                }
            }
            r.line_box =
                Some(LineBox { asc: line.baseline - st.shift, desc: line.height - line.baseline + st.shift, lead: pg.x, line: line_id });
            if colr {
                r.flags = r.flags.with(RunFlags::AS_PATH, true).with(RunFlags::COLR, true);
            }
            r.fx = st.fx;
            // a swapped glyph (OpenType): no character in the PDF — as curves, like WordArt
            if g.sub_gid.is_some() {
                r.flags = r.flags.with(RunFlags::AS_PATH, true);
            }
            run = Some(r);
            run_key = Some((parts, full));
        }
        let cp = if g.kind == GlyphKind::Shy { 0x2d } else { g.cp };
        if let Some(r) = run.as_mut() {
            r.glyphs.push((cp, frame_x + pg.x, g.sub_gid));
            push_code_point(&mut r.text, cp)?;
        }
    }
    flush(&mut run, &mut run_key, out)?;
    // underline / strike — by contiguous stretches with the same style
    paint_decorations(env, p, out, &all, frame_x, base_y)
}

/// `paintLeader(ctx, out, pg, frameX, baseY, all)` (paint.ts:102): a tab leader as Word draws it
/// (oracle tocdots), in 600-DPI pixels: the glyph/face/size/colour/shift of the TAB's run ('heavy'
/// also draws '_'); step = the glyph's advance at the device em, rounded to the pixel (+ the
/// run's `w:spacing`; × the `w:w` scale); an ABSOLUTE grid from the PAGE's left edge; whole glyphs
/// only, the first at k·step ≥ the text end, the last ending ≤ the next text's start.
fn paint_leader<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    all: &[PlacedGlyph],
    i: usize,
    frame_x: f64,
    base_y: f64,
) -> LResult<()> {
    let pg = all.get(i).ok_or(LayoutError::Internal("paint: leader tab"))?;
    if pg.w <= 0.0 {
        return Ok(());
    }
    let g = glyph_of(p, pg.g).ok_or(LayoutError::Internal("paint: leader glyph"))?;
    let st = p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: leader style"))?;
    let ch: u16 = match pg.leader {
        Some(TabLeader::Hyphen) => u16::from(b'-'),
        Some(TabLeader::Underscore | TabLeader::Heavy) => u16::from(b'_'),
        Some(TabLeader::MiddleDot) => 0xb7,
        _ => u16::from(b'.'),
    };
    let cp = u32::from(ch);
    let rpr = &st.rpr;
    let em_px = jm::round((g.size_pt * 600.0) / 72.0);
    let (adv, upm) = env.advance_units(g.face, cp)?;
    let mut step = jm::round((adv * em_px) / upm);
    if let Some(sp) = rpr.spacing.filter(|v| v.truthy()) {
        step += jm::round(sp.to_number() / 2.4);
    }
    let w = rpr.w.filter(|v| v.truthy()).map(crate::model::JsNumber::to_number).filter(|&w| w != 100.0);
    let scale = w.map_or(1.0, |w| w / 100.0);
    step = jm::round(step * scale);
    if step <= 0.0 {
        return Ok(());
    }
    // the frame's px: half a pixel rounds to EVEN (472.5 → 472; 467.5 → 468 — oracle tdhalfpx)
    let fv = frame_x / 2.4;
    let ff = jm::floor(fv);
    let fx = if jm::abs(fv - ff - 0.5) < 1e-6 { ff + jm::rem(ff, 2.0) } else { jm::round(fv) };
    let x0 = display_end(env, p, all, i, fx)?;
    let x1 = fx + jm::round((pg.x + pg.w) / 2.4);
    let color = env.intern(&st.color)?;
    let mut r = RunBuf::new(g.face, g.size_pt, color, base_y - st.shift);
    if scale != 1.0 {
        r.scale_x = OptF64::some(scale);
    }
    let mut k = jm::ceil(x0 / step);
    while (k + 1.0) * step <= x1 {
        env.charge(1)?;
        r.glyphs.push((cp, k * step * 2.4, None));
        r.text.push(ch);
        k += 1.0;
    }
    if !r.glyphs.is_empty() {
        // no `lineBox` on a leader run
        let head = GlyphRunHead {
            face: r.face,
            color: r.color,
            size_pt: r.size_pt,
            y: r.y,
            text: &r.text,
            scale_x: r.scale_x,
            alpha: OptF64::NONE,
            fx: None,
            flags: RunFlags::default(),
        };
        out.list.push_glyphs(head, r.glyphs.iter().copied())?;
        out.side.push(ItemSide::default());
    }
    Ok(())
}

/// `displayEnd(ctx, all, tab, fx)` (paint.ts:140): the end of the text before the tab as Word
/// SHOWS it in the PDF (600-DPI px): widths converted from the internal unit (1/4096 pt) with the
/// 32-bit fixed-point multiplier 8534/2²², the text since the previous tab measured in pieces
/// (the longest prefix whose width × 8534 + 2²¹ still fits in 2³²), each rounded on its own.
fn display_end<E: PaintEnv + ?Sized>(env: &mut E, p: &PreparedPara, all: &[PlacedGlyph], i: usize, fx: f64) -> LResult<f64> {
    let tab = all.get(i).ok_or(LayoutError::Internal("paint: displayEnd tab"))?;
    let mut s = i;
    while s > 0 && all.get(s - 1).and_then(|q| glyph_of(p, q.g)).is_some_and(|g| g.kind != GlyphKind::Tab) {
        s -= 1;
    }
    let st = env.settings();
    if st.compatibility_mode < 15.0 && env.word97_line_breaks() {
        let from = if s < i { all.get(s) } else { Some(tab) };
        let g = from.and_then(|q| glyph_of(p, q.g)).ok_or(LayoutError::Internal("paint: displayEnd glyph"))?;
        let rpr = &p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: displayEnd style"))?.rpr;
        if env.east_asia_font(rpr)? {
            return Ok(fx + jm::round(tab.x / 2.4));
        }
    }
    let seg = |u: f64| jm::floor((u * 8534.0 + 2_097_152.0) / 4_194_304.0);
    let start_x = if s < i { all.get(s).map_or(f64::NAN, |q| q.x) } else { tab.x };
    let mut px = fx + jm::round(start_x / 2.4);
    let mut u = 0.0;
    for q in all.iter().take(i).skip(s) {
        let w = jm::round(q.w / WORD_UNIT);
        if u > 0.0 && (u + w) * 8534.0 + 2_097_152.0 >= 4_294_967_296.0 {
            px += seg(u);
            u = 0.0;
        }
        u += w;
    }
    Ok(px + seg(u))
}

/// `paintMath(out, box, frameX, x, baseY, line, lineId)` (paint.ts:160): an equation's fraction —
/// numerator and denominator, in that order, as Word writes the PDF — and the bar.
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
fn paint_math<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    mb: &MathBox,
    frame_x: f64,
    x: f64,
    base_y: f64,
    line: &Line,
    line_id: LineSeq,
) -> LResult<()> {
    let mut run: Option<RunBuf> = None;
    let mut key: Vec<u16> = Vec::new();
    for it in &mb.items {
        let st = p.styles.get(it.style.0 as usize).ok_or(LayoutError::Internal("paint: math style"))?;
        let face_key = env.face_key(it.face)?;
        let color = env.intern(&st.color)?;
        // `${it.face.key}|${it.sizePt}|${it.style.color}|${it.y}`
        let mut k = env.sym_text(face_key);
        k.extend("|".encode_utf16().chain(uo_jsnum::format::to_js_string(it.size_pt).encode_utf16()).chain("|".encode_utf16()));
        k.extend_from_slice(&st.color);
        k.extend("|".encode_utf16().chain(uo_jsnum::format::to_js_string(it.y).encode_utf16()));
        if run.is_none() || k != key {
            if let Some(r) = run.take() {
                r.push(out)?;
            }
            key = k;
            let mut r = RunBuf::new(it.face, it.size_pt, color, base_y + it.y);
            r.line_box =
                Some(LineBox { asc: line.baseline + it.y, desc: line.height - line.baseline - it.y, lead: x + it.x, line: line_id });
            run = Some(r);
        }
        if let Some(r) = run.as_mut() {
            r.glyphs.push((it.cp, frame_x + x + it.x, None));
            push_code_point(&mut r.text, it.cp)?;
        }
    }
    if let Some(r) = run.take() {
        r.push(out)?;
    }
    for r in &mb.rules {
        let fill = env.intern(&r.color)?;
        rect(out, frame_x + x + r.x, base_y + r.y, r.w, r.h, fill);
    }
    Ok(())
}

/// `paintCheckBox(out, g, x, baseY)` (paint.ts:180).
fn paint_check_box<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    g: &Glyph,
    b: (f64, bool),
    x: f64,
    base_y: f64,
) -> LResult<()> {
    let (side, checked) = b;
    // the box's foot ≈ 0.18 × the body below the baseline (measured)
    let top = base_y + 0.18 * (side / 1.07) - side;
    let w = jm::max(0.5, side / 20.0 / 16.0);
    let st = p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: check box style"))?;
    let c = env.intern(&st.color)?;
    line_item(out, [x, top, x + side, top], c, w, None);
    line_item(out, [x, top + side, x + side, top + side], c, w, None);
    line_item(out, [x, top, x, top + side], c, w, None);
    line_item(out, [x + side, top, x + side, top + side], c, w, None);
    if checked {
        line_item(out, [x, top, x + side, top + side], c, w, None);
        line_item(out, [x + side, top, x, top + side], c, w, None);
    }
    Ok(())
}

/// `paintHr(out, g, frameX, fr, baseY)` (paint.ts:234): the VML horizontal line (o:hr) as Word
/// draws it in the PDF (oracle vmlhr): a rectangle of the fill colour whose foot is at baseline +
/// the run face's descent − 2.5 pt; width = the frame's, or `o:hrpct` aligned by `o:hralign`;
/// without `o:hrnoshade`, the relief: 2 px borders, top and left in the colour, bottom and right
/// E3E3E3.
#[expect(clippy::too_many_arguments, reason = "the TS signature plus the shape")]
fn paint_hr<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    g: &Glyph,
    sh: &HrShape,
    frame_x: f64,
    fr: (f64, f64),
    base_y: f64,
) -> LResult<()> {
    let (left, right) = fr;
    let avail = right - left;
    let mut w = match sh.hr_pct.filter(|&v| uo_jsnum::conv::truthy(v)) {
        Some(pct) => (avail * pct) / 1000.0,
        None if sh.width_twips > 0.0 => jm::min(sh.width_twips, avail),
        None => avail,
    };
    if w <= 0.0 {
        w = avail;
    }
    let x0 = frame_x
        + left
        + match sh.hr_align.as_deref() {
            Some("left") => 0.0,
            Some("right") => avail - w,
            _ => (avail - w) / 2.0,
        };
    let m = env.face_metrics(g.face)?;
    let st = p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: hr style"))?;
    let bottom = base_y - st.shift + m.descent * g.size_pt * 20.0 - 50.0;
    // `g.hr ?? sh.heightTwips`
    let h = g.hr.or(sh.height_twips);
    let fill = env.intern(&sh.fill)?;
    rect(out, x0, bottom - h, w, h, fill);
    if sh.hr_no_shade || h < 10.0 || w < 10.0 {
        return Ok(());
    }
    let e = 4.8; // 2 px at 600 dpi
    let light = env.intern(&"E3E3E3".encode_utf16().collect::<Vec<u16>>())?;
    rect(out, x0, bottom - h, w, e, fill);
    rect(out, x0, bottom - h + e, e, h - 2.0 * e, fill);
    rect(out, x0, bottom - e, w, e, light);
    rect(out, x0 + w - e, bottom - h + e, e, h - 2.0 * e, light);
    Ok(())
}

/// A decoration stretch (`Seg`, paint.ts:254): from `x0` to `x1`, its first glyph.
struct DecoSeg<'p> {
    x0: f64,
    x1: f64,
    g: std::borrow::Cow<'p, Glyph>,
}

/// `paintDecorations(out, all, frameX, baseY, line)` (paint.ts:253).
fn paint_decorations<E: PaintEnv + ?Sized>(
    env: &mut E,
    p: &PreparedPara,
    out: &mut PassItems,
    all: &[PlacedGlyph],
    frame_x: f64,
    base_y: f64,
) -> LResult<()> {
    // underlines: the geometry measured in Word (underline.ts)
    paint_underlines(env, p, out, all, frame_x, base_y)?;
    let style = |g: &Glyph| p.styles.get(g.style.0 as usize);
    // `segs((g) => g.style.strike || g.style.dstrike)`
    let mut res: Vec<DecoSeg<'_>> = Vec::new();
    let mut cur: Option<DecoSeg<'_>> = None;
    // trailing spaces of the line get no decoration
    let mut last_content: Option<usize> = None;
    for (k, q) in all.iter().enumerate() {
        let Some(g) = glyph_of(p, q.g) else { continue };
        if !matches!(g.kind, GlyphKind::Space | GlyphKind::Lb | GlyphKind::Pb) {
            last_content = Some(k);
        }
    }
    for (k, pg) in all.iter().enumerate() {
        let g = glyph_of(p, pg.g).ok_or(LayoutError::Internal("paint: decoration glyph"))?;
        let pred = style(&g).is_some_and(|st| st.strike || st.dstrike);
        let on = last_content.is_some_and(|lc| k <= lc) && pred && pg.w > 0.0 && g.kind != GlyphKind::Lb && g.kind != GlyphKind::Pb;
        // `cur.g.style === g.style`: one style object per run (StyleIx)
        let joins = on && cur.as_ref().is_some_and(|c| c.g.style == g.style && jm::abs(c.x1 - (frame_x + pg.x)) < 1.0);
        if joins {
            if let Some(c) = cur.as_mut() {
                c.x1 = frame_x + pg.x + pg.w;
            }
        } else if on {
            if let Some(c) = cur.take() {
                res.push(c);
            }
            cur = Some(DecoSeg { x0: frame_x + pg.x, x1: frame_x + pg.x + pg.w, g });
        } else if let Some(c) = cur.take() {
            res.push(c);
        }
    }
    if let Some(c) = cur.take() {
        res.push(c);
    }
    for s in &res {
        let st: &RunStyle = style(&s.g).ok_or(LayoutError::Internal("paint: strike style"))?;
        let m = env.face_metrics(s.g.face)?;
        let y = base_y - st.shift - m.ascent * st.size_pt * 20.0 * 0.3;
        let w = jm::max(0.5, m.underline_thick * st.size_pt);
        let color = env.intern(&st.color)?;
        line_item(out, [s.x0, y, s.x1, y], color, w, None);
        if st.dstrike {
            line_item(out, [s.x0, y - w * 40.0, s.x1, y - w * 40.0], color, w, None);
        }
    }
    Ok(())
}

// ── borders ─────────────────────────────────────────────────────────────────────────────────

fn border_on(b: Option<&BorderSpec>) -> Option<&BorderSpec> {
    b.filter(|b| &*b.val != "none" && &*b.val != "nil")
}

/// `b.sz ?? 4` (`null` is nullish).
fn sz_or(b: &BorderSpec, d: f64) -> f64 {
    b.sz.and_then(crate::model::JsNumber::as_f64).unwrap_or(d)
}

/// `borderWidth(b)` (paint.ts:290): the drawn thickness (twips) of an OOXML border (`w:sz` in
/// 1/8 pt).
#[must_use]
pub fn border_width(b: Option<&BorderSpec>) -> f64 {
    let Some(b) = border_on(b) else { return 0.0 };
    // the thickness goes to WHOLE twips, truncated: w:sz 1 → 2 tw, 3 → 7 tw (oracle bdrthin)
    let tw = jm::floor(jm::max(1.0, sz_or(b, 4.0)) * 2.5 + 1e-9);
    match &*b.val {
        "double" => tw * 3.0,
        "thinThickSmallGap" | "thickThinSmallGap" => tw + 40.0,
        // the other composites: the sum of the bands Word draws (table-lab border probe)
        "triple" => tw * 5.0,
        "thinThickThinSmallGap" => tw + 58.0,
        "thinThickMediumGap" | "thickThinMediumGap" => tw * 2.0,
        "thinThickThinMediumGap" => tw * 3.0,
        "thinThickLargeGap" | "thickThinLargeGap" => tw + 43.0,
        "thinThickThinLargeGap" => tw * 2.0 + 58.0,
        "threeDEmboss" | "threeDEngrave" => tw + 2.0 * jm::max(14.0, jm::floor(tw / 2.0)),
        _ => tw,
    }
}

/// A band of a composite border (`Band`, paint.ts:310): thickness (twips) and colour (`None` = gap).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub t: f64,
    pub c: Option<BandInk>,
}

/// `'ink' | 'light' | 'mid'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandInk {
    Ink,
    Light,
    Mid,
}

/// `borderBands(val, w)` (paint.ts:319): the bands of a border, OUTSIDE in, as Word draws them.
#[must_use]
pub fn border_bands(val: &str, w: f64) -> Vec<Band> {
    let q = |x: f64| jm::max(2.4, jm::floor(x / 2.4 + 1e-9) * 2.4);
    let thin = 14.4;
    let half = q(w / 2.0);
    let thick = 28.8;
    let gap = |t: f64| Band { t, c: None };
    let ink = |t: f64| Band { t, c: Some(BandInk::Ink) };
    match val {
        "double" => vec![ink(w), gap(w), ink(w)],
        "triple" => vec![ink(w), gap(w), ink(w), gap(w), ink(w)],
        "thinThickSmallGap" => vec![ink(w), gap(thin), ink(thin)],
        "thickThinSmallGap" => vec![ink(thin), gap(thin), ink(w)],
        "thinThickThinSmallGap" => vec![ink(thin), gap(thin), ink(w), gap(thin), ink(thin)],
        "thinThickMediumGap" => vec![ink(w), gap(half), ink(half)],
        "thickThinMediumGap" => vec![ink(half), gap(half), ink(w)],
        "thinThickThinMediumGap" => vec![ink(half), gap(half), ink(w), gap(half), ink(half)],
        "thinThickLargeGap" => vec![ink(thick), gap(w), ink(thin)],
        "thickThinLargeGap" => vec![ink(thin), gap(w), ink(thick)],
        "thinThickThinLargeGap" => vec![ink(thin), gap(w), ink(thick), gap(w), ink(thin)],
        "threeDEmboss" => {
            let t = jm::max(thin, half);
            vec![Band { t, c: Some(BandInk::Light) }, Band { t: w, c: Some(BandInk::Mid) }, Band { t, c: Some(BandInk::Ink) }]
        }
        "threeDEngrave" => {
            let t = jm::max(thin, half);
            vec![Band { t, c: Some(BandInk::Ink) }, Band { t: w, c: Some(BandInk::Mid) }, Band { t, c: Some(BandInk::Light) }]
        }
        _ => vec![ink(w)],
    }
}

/// `toWhite(hex, f)` (paint.ts:342): mixes with white (3D relief: black → C0C0C0 / 606060).
#[must_use]
pub fn to_white(hex: &[u16], f: f64) -> Vec<u16> {
    let n = uo_jsnum::parse::parse_int_utf16(hex, 16);
    let n = uo_jsnum::conv::to_int32(n);
    let mut out = String::with_capacity(6);
    for v in [(n >> 16) & 255, (n >> 8) & 255, n & 255] {
        let v = f64::from(v);
        let r = jm::round(v + (255.0 - v) * f);
        let s = uo_jsnum::format::to_radix_string(r, 16).unwrap_or_default();
        if s.len() < 2 {
            out.push('0');
        }
        out.push_str(&s);
    }
    out.to_uppercase().encode_utf16().collect()
}

/// `BORDER_DASH[val]` (paint.ts:348): the dash pattern of a border (multiples of w).
fn border_dash(val: &str) -> Option<LineDash> {
    Some(match val {
        "dotted" => LineDash::BdrDot,
        "dashed" => LineDash::BdrDash,
        "dotDash" | "dashDotStroked" => LineDash::BdrDashDot,
        "dotDotDash" => LineDash::BdrDashDotDot,
        "dashSmallGap" => LineDash::BdrDashSmall,
        _ => return None,
    })
}

/// `borderPaintWidth(b)` (paint.ts:357): the thickness (twips) Word WRITES a border with in the
/// PDF (oracle bdrw): `w:sz` (1/8 pt) truncated to 1/600".
#[must_use]
pub fn border_paint_width(b: &BorderSpec) -> f64 {
    jm::max(1.0, jm::floor((jm::max(2.0, sz_or(b, 4.0)) * 25.0) / 24.0 + 1e-9)) * 2.4
}

/// The outside side of a border (`outer`, paint.ts:371).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outer {
    Top,
    Bottom,
    Left,
    Right,
}

/// `resolveColor({ val: b.color, themeColor, themeTint, themeShade }, ctx.fmt.theme)` (resolve.ts,
/// F2's `uo_docx::word`).
fn border_color(cx: &mut Cx<'_, '_>, b: &BorderSpec) -> LResult<Option<Box<[u16]>>> {
    let spec = ColorRef {
        val: b.color.as_deref(),
        theme_color: b.theme_color.as_deref(),
        theme_tint: b.theme_tint.as_deref(),
        theme_shade: b.theme_shade.as_deref(),
    };
    Ok(resolve_color(Some(spec), &cx.ldoc.view.formatting().theme).map(|c| c.encode_utf16().collect()))
}

/// `shadingFill(p.ppr.shd, ctx.fmt.theme)` (resolve.ts, F2's `uo_docx::word`).
fn shading_fill_of(cx: &mut Cx<'_, '_>, p: &PreparedPara) -> LResult<Option<Box<[u16]>>> {
    Ok(shading_fill(p.ppr.shd.as_deref(), &cx.ldoc.view.formatting().theme).map(|c| c.encode_utf16().collect()))
}

/// `paintBorder(ctx, out, b, x1, y1, x2, y2, outer?)` (paint.ts:368): a horizontal/vertical border
/// centred on the segment. Solid ones as filled RECTANGLES of the quantized thickness, as in
/// Word's PDF; dotted/dashed as strokes.
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
pub fn paint_border(
    cx: &mut Cx<'_, '_>,
    out: &mut PassItems,
    b: Option<&BorderSpec>,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    outer: Option<Outer>,
) -> LResult<()> {
    let Some(b) = border_on(b) else { return Ok(()) };
    let color: Vec<u16> = match border_color(cx, b)? {
        Some(c) => c.into(),
        None => "000000".encode_utf16().collect(),
    };
    paint_border_in(cx, out, b, &color, [x1, y1, x2, y2], outer)
}

/// The body of `paintBorder` after the colour is resolved.
pub fn paint_border_in<E: PaintEnv + ?Sized>(
    env: &mut E,
    out: &mut PassItems,
    b: &BorderSpec,
    color: &[u16],
    seg: [f64; 4],
    outer: Option<Outer>,
) -> LResult<()> {
    let [x1, y1, x2, y2] = seg;
    let w = border_paint_width(b);
    let c = env.intern(color)?;
    if let Some(dash) = border_dash(&b.val) {
        line_item(out, [x1, y1, x2, y2], c, w / 20.0, Some(dash));
        return Ok(());
    }
    // diagonal (tl2br/tr2bl of a cell): a stroke
    if x1 != x2 && y1 != y2 {
        line_item(out, [x1, y1, x2, y2], c, w / 20.0, None);
        return Ok(());
    }
    let horizontal = jm::abs(y2 - y1) < jm::abs(x2 - x1);
    if &*b.val == "wave" || &*b.val == "doubleWave" {
        return paint_wave(env, out, &*b.val == "doubleWave", c, w, horizontal, seg);
    }
    let bands = border_bands(&b.val, w);
    let total = bands.iter().fold(0.0, |s, x| s + x.t);
    // outside in: from the top (or the left) when the outside is top/left
    let flip = matches!(outer, Some(Outer::Bottom | Outer::Right));
    let mut pos = -total / 2.0;
    let order: Vec<Band> = if flip { bands.iter().rev().copied().collect() } else { bands };
    for band in order {
        if let Some(ink) = band.c {
            let fill = match ink {
                BandInk::Light => env.intern(&to_white(color, 192.0 / 255.0))?,
                BandInk::Mid => env.intern(&to_white(color, 96.0 / 255.0))?,
                BandInk::Ink => c,
            };
            if horizontal {
                rect(out, jm::min(x1, x2), y1 + pos, jm::abs(x2 - x1), band.t, fill);
            } else {
                rect(out, x1 + pos, jm::min(y1, y2), band.t, jm::abs(y2 - y1), fill);
            }
        }
        pos += band.t;
    }
    Ok(())
}

/// `paintWave(out, double, color, w, horizontal, x1, y1, x2, y2)` (paint.ts:403): a wavy border
/// (Word uses bitmap tiles): a zigzag 3 pt high with a 6 pt period, scaled with the thickness;
/// the double one draws two waves.
fn paint_wave<E: PaintEnv + ?Sized>(
    env: &mut E,
    out: &mut PassItems,
    double: bool,
    color: Sym,
    w: f64,
    horizontal: bool,
    seg: [f64; 4],
) -> LResult<()> {
    let [x1, y1, x2, y2] = seg;
    let h = jm::max(60.0, w * 3.0);
    let period = h * 2.0;
    let lw = jm::max(0.5, w / 20.0 / 1.5);
    let len = if horizontal { jm::abs(x2 - x1) } else { jm::abs(y2 - y1) };
    let waves: &[f64] = if double { &[-h / 4.0, h / 4.0] } else { &[0.0] };
    for &off in waves {
        let mut cmds: Vec<PathCmd> = Vec::new();
        let mut s = 0.0;
        while s <= len + 0.01 {
            env.charge(1)?;
            let r = jm::rem(jm::round(s / (period / 2.0)), 2.0);
            let d = (if uo_jsnum::conv::truthy(r) { 1.0 } else { -1.0 }) * (h / if double { 4.0 } else { 2.0 });
            let a = jm::min(s, len);
            let first = cmds.is_empty();
            let (x, y) = if horizontal { (jm::min(x1, x2) + a, y1 + off + d) } else { (x1 + off + d, jm::min(y1, y2) + a) };
            cmds.push(if first { PathCmd::M { x, y } } else { PathCmd::L { x, y } });
            s += period / 2.0;
        }
        push_path(out, &cmds, None, Some(Stroke { color, width_pt: lw, dash: None }), false)?;
    }
    Ok(())
}

/// `paintParaBox(ctx, out, p, pBdr, left, right, top, bottom, drawTop, drawBottom, between)`
/// (paint.ts:430): the border/shading box of a block of paragraphs with equal borders, as Word
/// writes it (oracle pbdr).
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
pub fn paint_para_box(
    cx: &mut Cx<'_, '_>,
    out: &mut PassItems,
    p: &PreparedPara,
    p_bdr: Option<&PBdr>,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    draw_top: bool,
    draw_bottom: bool,
    between: bool,
) -> LResult<()> {
    let fill = shading_fill_of(cx, p)?;
    const EXT: f64 = 29.0;
    let bl = p_bdr.and_then(|b| b.left.as_deref());
    let br = p_bdr.and_then(|b| b.right.as_deref());
    let space = |b: &BorderSpec| b.space.and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0) * 20.0;
    let pad_l = border_on(bl).map_or(0.0, space);
    let pad_r = border_on(br).map_or(0.0, space);
    let in_l = left - EXT - pad_l;
    let in_r = right + EXT + pad_r;
    // shadow (w:shadow): the reserved top/bottom band is twice the thickness; the top border sits
    // in its LOWER half and the shadow is a copy of the right and bottom borders moved by the
    // thickness right and down (Word's PDF)
    let top_b = p_bdr.and_then(|b| b.top.as_deref());
    let bot_b = p_bdr.and_then(|b| b.bottom.as_deref());
    let sh_t = if draw_top && top_b.is_some_and(|b| b.shadow == Some(true)) { border_width(top_b) } else { 0.0 };
    let sh_b = if draw_bottom && bot_b.is_some_and(|b| b.shadow == Some(true)) { border_width(bot_b) } else { 0.0 };
    let sh_r = if br.is_some_and(|b| b.shadow == Some(true)) { border_width(br) } else { 0.0 };
    let f_top = if uo_jsnum::conv::truthy(sh_t) { top + sh_t + border_width(top_b) } else { top };
    let f_bot = if uo_jsnum::conv::truthy(sh_b) { bottom - sh_b - border_width(bot_b) } else { bottom };
    if let Some(fill) = fill.filter(|f| !f.is_empty()) {
        let c = PaintEnv::intern(cx, &fill)?;
        rect(out, in_l, f_top, in_r - in_l, f_bot - f_top, c);
    }
    let Some(pb) = p_bdr else { return Ok(()) };
    let out_l = in_l - border_width(bl);
    let out_r = in_r + border_width(br);
    if draw_top {
        let y = top + sh_t + border_width(top_b) / 2.0;
        paint_border(cx, out, top_b, out_l, y, out_r, y, None)?;
    }
    if let (true, Some(bt)) = (between, border_on(pb.between.as_deref())) {
        let yb = top + space(bt) + border_width(Some(bt)) / 2.0;
        paint_border(cx, out, Some(bt), out_l, yb, out_r, yb, None)?;
    }
    if draw_bottom {
        let y = bottom - sh_b - border_width(bot_b) / 2.0;
        paint_border(cx, out, bot_b, out_l, y, out_r, y, None)?;
        if uo_jsnum::conv::truthy(sh_b) {
            paint_border(cx, out, bot_b, out_l + sh_b, bottom - sh_b / 2.0, out_r + sh_b, bottom - sh_b / 2.0, None)?;
        }
    }
    paint_border(cx, out, bl, in_l - border_width(bl) / 2.0, top + sh_t, in_l - border_width(bl) / 2.0, bottom - sh_b, None)?;
    paint_border(cx, out, br, in_r + border_width(br) / 2.0, top + sh_t, in_r + border_width(br) / 2.0, bottom - sh_b, None)?;
    if uo_jsnum::conv::truthy(sh_r) {
        paint_border(
            cx,
            out,
            br,
            in_r + border_width(br) + sh_r / 2.0,
            top + sh_t + sh_r,
            in_r + border_width(br) + sh_r / 2.0,
            bottom,
            None,
        )?;
    }
    Ok(())
}

/// The page geometry `pageBorderItems` reads (`g`, paint.ts:481).
#[derive(Clone, Copy, Debug, Default)]
pub struct PageBorderGeom {
    pub page_w: f64,
    pub page_h: f64,
    pub mar_l: f64,
    pub mar_r: f64,
    pub mar_t: f64,
    pub mar_b: f64,
    pub hdr: f64,
    pub ftr: f64,
}

/// `pageBorderItems(ctx, pb, g, surround)` (paint.ts:478): page borders (`w:pgBorders`) as Word
/// writes them in the PDF (oracle pgbdr).
pub fn page_border_items(
    cx: &mut Cx<'_, '_>,
    pb: &PageBorders,
    g: &PageBorderGeom,
    surround_header: bool,
    surround_footer: bool,
) -> LResult<PassItems> {
    let mut colors: [Option<Vec<u16>>; 4] = [None, None, None, None];
    for (k, b) in [pb.top.as_ref(), pb.bottom.as_ref(), pb.left.as_ref(), pb.right.as_ref()].into_iter().enumerate() {
        if let (Some(b), Some(slot)) = (border_on(b), colors.get_mut(k)) {
            *slot = Some(match border_color(cx, b)? {
                Some(c) => c.into(),
                None => "000000".encode_utf16().collect(),
            });
        }
    }
    page_border_items_in(cx, pb, g, surround_header, surround_footer, &colors)
}

/// The body of `pageBorderItems` with the colours of top, bottom, left, right resolved (the TS
/// resolves each when it draws that side; resolution has no side effect).
pub fn page_border_items_in<E: PaintEnv + ?Sized>(
    env: &mut E,
    pb: &PageBorders,
    g: &PageBorderGeom,
    surround_header: bool,
    surround_footer: bool,
    colors: &[Option<Vec<u16>>; 4],
) -> LResult<PassItems> {
    let lw = |b: Option<&BorderSpec>| border_on(b).map_or(0.0, |b| jm::max(0.25, sz_or(b, 4.0) / 8.0) * 20.0);
    let thick = |b: Option<&BorderSpec>| if border_on(b).is_some_and(|b| &*b.val == "double") { 3.0 * lw(b) } else { lw(b) };
    let sp = |b: Option<&BorderSpec>| b.and_then(|b| b.space).and_then(crate::model::JsNumber::as_f64).unwrap_or(0.0) * 20.0;
    let (t, bo, l, r) = (pb.top.as_ref(), pb.bottom.as_ref(), pb.left.as_ref(), pb.right.as_ref());
    let page = pb.offset_from.as_deref() == Some("page");
    // the outer faces of the box
    let top = if page { sp(t) } else { (if surround_header { g.hdr } else { g.mar_t }) - sp(t) - thick(t) };
    let bottom = if page { g.page_h - sp(bo) } else { g.page_h - (if surround_footer { g.ftr } else { g.mar_b }) + sp(bo) + thick(bo) };
    let left = if page { sp(l) } else { g.mar_l - sp(l) - thick(l) };
    let right = if page { g.page_w - sp(r) } else { g.page_w - g.mar_r + sp(r) + thick(r) };
    let mut out = PassItems::new();
    // the bands (from the outer face, inwards) of a border
    let bands = |b: &BorderSpec| -> Vec<(f64, f64)> {
        let w = lw(Some(b));
        if &*b.val == "double" { vec![(0.0, w), (2.0 * w, 3.0 * w)] } else { vec![(0.0, w)] }
    };
    let color = |env: &mut E, k: usize| -> LResult<Sym> {
        let c = colors.get(k).and_then(Option::as_deref).ok_or(LayoutError::Internal("paint: page border colour"))?;
        env.intern(c)
    };
    if let Some(b) = border_on(t) {
        for (a, z) in bands(b) {
            let c = color(env, 0)?;
            rect(&mut out, left, top + a, right - left, z - a, c);
        }
    }
    if let Some(b) = border_on(bo) {
        for (a, z) in bands(b) {
            let c = color(env, 1)?;
            rect(&mut out, left, bottom - z, right - left, z - a, c);
        }
    }
    if let Some(b) = border_on(l) {
        for (a, z) in bands(b) {
            let c = color(env, 2)?;
            rect(&mut out, left + a, top, z - a, bottom - top, c);
        }
    }
    if let Some(b) = border_on(r) {
        for (a, z) in bands(b) {
            let c = color(env, 3)?;
            rect(&mut out, right - z, top, z - a, bottom - top, c);
        }
    }
    Ok(out)
}

// ── inline objects ──────────────────────────────────────────────────────────────────────────

/// `paintInlineObject(ctx, out, g, x0, baseY0)` (paint.ts:196): an inline image (EMF/WMF through
/// the metafile players inside a clip with `src`, else an image item with its `pictureLook`) or an
/// inline shape, inside its effects margin (oracle effext).
///
/// The three ends need what the engine holds (F5-9): the `ImageId` of a model drawing (RQ3), the
/// pass's face lookup and resource writer for the metafile players and `pictureLook`, and the
/// shape item — `engine::objects` (F7b). A picture bullet (`g.pic_bullet`, no model run) paints
/// its numbering picture as a new image object (F4 hand-on).
pub fn paint_inline_object(cx: &mut Cx<'_, '_>, p: &PreparedPara, g: &Glyph, x0: f64, base_y0: f64, out: &mut PassItems) -> LResult<()> {
    let shift = p.styles.get(g.style.0 as usize).ok_or(LayoutError::Internal("paint: inline object style"))?.shift;
    if let Some(pb) = g.pic_bullet.as_deref()
        && g.obj.is_none()
    {
        let key = uo_jsnum::format::to_js_string(pb.id);
        let Some(img) = cx.ldoc.view.formatting().numbering.pic_bullets.get(&key).and_then(|b| b.image.as_ref()) else {
            return Err(LayoutError::Internal("paint: picture bullet image"));
        };
        let (w, h) = (pb.width_twips, pb.height_twips);
        let r = crate::metafile::MetafileFrame { x: x0, y: base_y0 - shift - h, w, h };
        let data = cx.ldoc.view.media(img.data.0).unwrap_or(&[]);
        let id = crate::engine::objects::pic_bullet_id(cx, img, w, h)?;
        if !crate::engine::objects::push_metafile(cx, data, id, r, out)? {
            crate::engine::objects::push_image_item(cx, None, id, r, out)?;
        }
        return Ok(());
    }
    // `g.obj!`
    let run = g.obj.and_then(|k| model_run(cx.ldoc, k)).ok_or(LayoutError::Internal("paint: g.obj"))?;
    let image = run.ext().and_then(|e| e.image.as_deref());
    let shape = run.ext().and_then(|e| e.shape.as_deref());
    // the effects margin: the picture sits inside it (oracle effext);
    // `obj.image?.inlineEffect ?? obj.shape?.inlineEffect`, then `ie?.l ?? 0`
    let ie = image.and_then(|i| i.inline_effect.as_ref()).or_else(|| shape.and_then(|s| s.inline_effect.as_ref()));
    let nullish0 = |v: crate::model::JsNumber| v.as_f64().unwrap_or(0.0);
    let x = x0 + ie.map_or(0.0, |e| nullish0(e.l));
    let base_y = base_y0 - ie.map_or(0.0, |e| nullish0(e.b));
    if let Some(im) = image {
        let data = cx.ldoc.view.media(im.data.0).unwrap_or(&[]);
        let (w, h) = (im.width_twips.to_number(), im.height_twips.to_number());
        let r = crate::metafile::MetafileFrame { x, y: base_y - shift - h, w, h };
        // EMF (`isEmf`) before WMF (`isWmf`, the Equation Editor 3.0 previews…): Word draws and
        // writes the text
        let id = crate::engine::objects::picture_id(cx, im, false)?;
        if !crate::engine::objects::push_metafile(cx, data, id, r, out)? {
            crate::engine::objects::push_image_item(cx, Some(im), id, r, out)?;
        }
        return Ok(());
    }
    if let Some(s) = shape {
        let (w, h) = (s.width_twips.to_number(), s.height_twips.to_number());
        return crate::engine::objects::push_shape_item(cx, &s.style, [x, base_y - shift - h, w, h], None, out);
    }
    Ok(())
}

// ── the engine's context as the paint environment ───────────────────────────────────────────

/// The model run of a run key (a slice `{ ...r, text }` keeps every other field of its base run;
/// the depth bound keeps a malformed chain of keys from recursing).
pub(crate) fn model_run<'l>(ldoc: &'l LDoc<'_>, key: RunKey) -> Option<&'l Run> {
    let mut k = key;
    for _ in 0..64 {
        match ldoc.run(k)? {
            RunSrc::Model { run, .. } => return Some(run),
            RunSrc::Synth(run) => return Some(run),
            RunSrc::Slice { base, .. } => k = *base,
        }
    }
    None
}

/// `faceMetrics(face.font)` of a layout face (computed once per face by F1's table).
pub(crate) fn face_metrics_of(cx: &Cx<'_, '_>, face: FaceId) -> LResult<FaceMetrics> {
    cx.fonts.metrics(face).ok_or(LayoutError::Internal("paint: face not in the layout"))
}

fn font_of<'c>(cx: &'c Cx<'_, '_>, face: FaceId) -> LResult<&'c uo_fonts::Face> {
    cx.fonts.face(face).ok_or(LayoutError::Internal("paint: face not in the layout"))
}

/// `font.vdmx(em)`: a `Map` keyed by the pixel size, so a size that is not a 16-bit integer
/// finds nothing (`-0` finds 0).
fn vdmx_at(f: &uo_fonts::Face, em: f64) -> Option<f64> {
    if !(0.0..=65535.0).contains(&em) || !uo_jsnum::conv::is_integer(em) {
        return None;
    }
    f.vdmx(uo_jsnum::conv::to_uint16(em)).map(|v| f64::from(v.y_min))
}

impl PaintEnv for Cx<'_, '_> {
    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics> {
        face_metrics_of(self, face)
    }

    fn ul_font(&mut self, face: FaceId, em: f64) -> LResult<UlFont> {
        let f = font_of(self, face)?;
        let fm = f.metrics();
        Ok(UlFont {
            units_per_em: f64::from(fm.units_per_em),
            us_win_descent: f64::from(fm.us_win_descent),
            descender: f64::from(fm.descender),
            vdmx_y_min: vdmx_at(f, em),
        })
    }

    fn advance_units(&mut self, face: FaceId, cp: u32) -> LResult<(f64, f64)> {
        let f = font_of(self, face)?;
        Ok((f64::from(f.advance_width(f.glyph_id(cp))), f64::from(f.metrics().units_per_em)))
    }

    fn colr(&mut self, face: FaceId, cp: u32) -> LResult<bool> {
        let f = font_of(self, face)?;
        Ok(f.has_color_glyphs() && f.color_layers(f.glyph_id(cp)).is_some())
    }

    fn face_key(&mut self, face: FaceId) -> LResult<Sym> {
        self.res.face(face).map(|f| f.key).ok_or(LayoutError::Internal("paint: face not in the resources"))
    }

    fn face_synth(&mut self, face: FaceId) -> LResult<(bool, bool)> {
        self.res.face(face).map(|f| (f.synth_bold, f.synth_italic)).ok_or(LayoutError::Internal("paint: face not in the resources"))
    }

    fn fx_draws(&self, fx: TextFxId) -> bool {
        self.res.text_fx(fx).is_some_and(|f| f.outline.is_some() || f.glow.is_some() || f.shadow.is_some() || f.reflection.is_some())
    }

    fn intern(&mut self, s: &[u16]) -> LResult<Sym> {
        self.res.symbols.intern(s).ok_or(LayoutError::Limit(LimitKind::Capacity))
    }

    fn sym_text(&self, s: Sym) -> Vec<u16> {
        self.res.sym(s).to_vec()
    }

    fn settings(&self) -> BreakSettings {
        LineEnv::settings(self)
    }

    fn word97_line_breaks(&self) -> bool {
        self.ldoc.view.formatting().settings.compat.get("useWord97LineBreakRules").copied().unwrap_or(false)
    }

    fn east_asia_font(&mut self, rpr: &RPr) -> LResult<bool> {
        // `!!fontOfSlot(rpr.rFonts, 'eastAsia', theme)`: an empty name is falsy
        let theme = &self.ldoc.view.formatting().theme;
        Ok(font_of_slot(rpr.r_fonts.as_deref(), FontSlot::EastAsia, theme).is_some_and(|s| !s.is_empty()))
    }

    fn next_line_seq(&mut self) -> LResult<LineSeq> {
        *self.line_seq = self.line_seq.checked_add(1).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
        Ok(LineSeq(*self.line_seq))
    }

    fn inline_object(&mut self, p: &PreparedPara, g: &Glyph, x0: f64, base_y0: f64, out: &mut PassItems) -> LResult<()> {
        paint_inline_object(self, p, g, x0, base_y0, out)
    }

    fn hr_shape(&mut self, _p: &PreparedPara, g: &Glyph) -> LResult<Option<HrShape>> {
        let Some(sh) = g.obj.and_then(|k| model_run(self.ldoc, k)).and_then(Run::ext).and_then(|e| e.shape.as_deref()) else {
            return Ok(None);
        };
        let fill: Box<[u16]> = match &sh.style.fill {
            Some(c) => c.0.to_ts_string().encode_utf16().collect(),
            None => "A0A0A0".encode_utf16().collect(),
        };
        Ok(Some(HrShape {
            hr_pct: sh.hr_pct.and_then(crate::model::JsNumber::as_f64),
            width_twips: sh.width_twips.to_number(),
            height_twips: sh.height_twips.to_number(),
            hr_align: sh.hr_align.clone(),
            fill,
            hr_no_shade: sh.hr_no_shade == Some(true),
        }))
    }

    fn charge(&mut self, units: u64) -> LResult<()> {
        self.budget.charge(units)
    }
}
