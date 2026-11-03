//! The F5 facets (record "Wave 1 — F5"): `p3.lines` (every `LineBreaker.next`/`reset`/
//! `peekWordWidth`) and `p3.paint` (every `paintLine`), replayed from the inputs the TS capture
//! dumped (`tools/lib/facets-f5.mjs`, `<key>.f5.bin`): the Rust `LineBreaker` and `paint_line_in`
//! run on the decoded paragraphs, with the TS font answers, and every call's raw output must equal
//! the TS bytes; the per-document facet digests are rebuilt and compared with the capture.
//!
//! Runs the committed fixture (`tests/captures/f5`) always; a fresh capture with
//! `UO_F5_CAPTURE=<capture> UO_F5_DUMP=<dump dir>` (written by `tools/capture-layout.mjs --dump` in
//! the same run, LQ21). Calls that need another frontier's stage (`LayoutError::Unported`: inline
//! objects F3, theme fonts F2) are counted as pending per stage, never as equal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // helpers outside #[test] functions

mod common;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::StreamSink;
use uo_docx_layout::canon::{CanonSink, canon_layout};
use uo_docx_layout::error::{LResult, LayoutError};
use uo_docx_layout::fonts::FaceMetrics;
use uo_docx_layout::items::PassItems;
use uo_docx_layout::keys::{LParaId, LineSeq, RunKey};
use uo_docx_layout::lines::{
    BreakMark, BreakSettings, GlyphSrc, Line, LineBreaker, LineEnv, LineFrame, PlacedGlyph, PtabAlign, PtabSpec, glyph_of, leader_from_ts,
};
use uo_docx_layout::math::{MathBox, MathItem, MathRule};
use uo_docx_layout::model::{JsNumber, RPr, TextShadow};
use uo_docx_layout::output::{Layout, LayoutPage, TabAlign, TabLeader, TabStop};
use uo_docx_layout::paint::{HrShape, PaintEnv, paint_line_in};
use uo_docx_layout::para::{Ch, Glyph, GlyphFlags, GlyphKind, LineRule, MarkerSuffix, ParaMarker, PreparedPara, RunStyle, StyleIx};
use uo_docx_layout::underline::UlFont;
use uo_render::types::{FxGlow, FxOutline, FxReflection, FxShadow, FxStop, LineCap, LineJoin, ResolvedFill, ResolvedTextFx};
use uo_render::{FaceId, FaceRes, OptF64, Resources, Sym, TextFxId};
use uo_testkit::{Capture, Digest, Replay, Stream};

// ── the raw Sink encoding (sink.mjs) ────────────────────────────────────────────────────────

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

type R<T> = Result<T, String>;

impl Rd<'_> {
    fn take(&mut self, n: usize) -> R<&[u8]> {
        let s = self.b.get(self.at..self.at + n).ok_or("truncated f5 dump")?;
        self.at += n;
        Ok(s)
    }
    fn tag(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn num(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bool(&mut self) -> R<bool> {
        Ok(self.tag()? != 0)
    }
    fn bytes(&mut self) -> R<Vec<u8>> {
        let n = u64::from_le_bytes(self.take(8)?.try_into().unwrap());
        Ok(self.take(usize::try_from(n).unwrap())?.to_vec())
    }
    fn u16s(&mut self) -> R<Vec<u16>> {
        Ok(self.bytes()?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
    fn string(&mut self) -> R<String> {
        Ok(String::from_utf16_lossy(&self.u16s()?))
    }
    fn opt_num(&mut self) -> R<Option<f64>> {
        if self.bool()? { Ok(Some(self.num()?)) } else { Ok(None) }
    }
    fn opt_string(&mut self) -> R<Option<String>> {
        if self.bool()? { Ok(Some(self.string()?)) } else { Ok(None) }
    }
}

/// The raw writer (the same bytes as sink.mjs with `raw: true`).
#[derive(Default)]
struct Raw(Vec<u8>);

impl Raw {
    fn num(&mut self, v: f64) -> &mut Self {
        let v = if v.is_nan() { f64::NAN } else { v };
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32v(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn tagc(&mut self, c: u8) -> &mut Self {
        self.0.push(c);
        self
    }
    fn boolv(&mut self, b: bool) -> &mut Self {
        self.0.push(u8::from(b));
        self
    }
    fn u16s(&mut self, s: &[u16]) -> &mut Self {
        let b: Vec<u8> = s.iter().flat_map(|u| u.to_le_bytes()).collect();
        self.bytesv(&b)
    }
    fn bytesv(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(b);
        self
    }
    fn opt_num(&mut self, v: Option<f64>) -> &mut Self {
        self.boolv(v.is_some());
        if let Some(v) = v {
            self.num(v);
        }
        self
    }
    fn opt_str(&mut self, s: Option<&str>) -> &mut Self {
        self.boolv(s.is_some());
        if let Some(s) = s {
            self.u16s(&s.encode_utf16().collect::<Vec<_>>());
        }
        self
    }
}

impl CanonSink for Raw {
    fn tag(&mut self, c: u8) {
        self.tagc(c);
    }
    fn u32(&mut self, v: u32) {
        self.u32v(v);
    }
    fn f64(&mut self, v: f64) {
        self.num(v);
    }
    fn bool(&mut self, b: bool) {
        self.boolv(b);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.bytesv(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(&Digest::of(b).0);
    }
}

// ── the decoded inputs ──────────────────────────────────────────────────────────────────────

struct FaceRec {
    m: FaceMetrics,
    upm: f64,
    win_descent: f64,
    descender: f64,
}

/// What a replayed glyph carries outside `Glyph` (indexed by `g.ptab` / `g.obj`).
#[derive(Default)]
struct Extras {
    ptabs: Vec<PtabSpec>,
    shapes: Vec<Option<HrShape>>,
}

struct Para {
    p: PreparedPara,
    extras: Extras,
}

#[derive(Default)]
struct Env {
    faces: Vec<FaceRec>,
    styles: Vec<RunStyle>,
    style_ea: Vec<bool>,
    paras: Vec<Para>,
    adv: HashMap<(u32, u32, u64), f64>,
    vdmx: HashMap<(u32, u64), Option<f64>>,
    lead: HashMap<(u32, u32), (f64, f64)>,
    colr: HashMap<(u32, u32), bool>,
    res: Resources,
    settings: BreakSettings,
    word97: bool,
    cur_para: usize,
    line_seq: u32,
}

fn bits(v: f64) -> u64 {
    if v == 0.0 {
        0
    } else if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        v.to_bits()
    }
}

fn face_ix(face: FaceId) -> u32 {
    u32::try_from(face.index()).unwrap()
}

fn miss(what: &'static str) -> LayoutError {
    LayoutError::Internal(what)
}

impl LineEnv for Env {
    fn settings(&self) -> BreakSettings {
        self.settings
    }
    fn advance_twips(&mut self, face: FaceId, cp: u32, size_pt: f64) -> LResult<f64> {
        self.adv.get(&(face_ix(face), cp, bits(size_pt))).copied().ok_or(miss("replay: advance not captured"))
    }
    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics> {
        self.faces.get(face.index()).map(|f| f.m).ok_or(miss("replay: face"))
    }
    fn ptab(&self, p: &PreparedPara, g: &Glyph) -> Option<PtabSpec> {
        let _ = p;
        g.ptab.and_then(|k| self.paras.get(self.cur_para)?.extras.ptabs.get(k as usize).copied())
    }
    fn charge(&mut self, _units: u64) -> LResult<()> {
        Ok(())
    }
}

impl PaintEnv for Env {
    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics> {
        LineEnv::face_metrics(self, face)
    }
    fn ul_font(&mut self, face: FaceId, em: f64) -> LResult<UlFont> {
        let f = self.faces.get(face.index()).ok_or(miss("replay: face"))?;
        let vd = *self.vdmx.get(&(face_ix(face), bits(em))).ok_or(miss("replay: vdmx not captured"))?;
        Ok(UlFont { units_per_em: f.upm, us_win_descent: f.win_descent, descender: f.descender, vdmx_y_min: vd })
    }
    fn advance_units(&mut self, face: FaceId, cp: u32) -> LResult<(f64, f64)> {
        self.lead.get(&(face_ix(face), cp)).copied().ok_or(miss("replay: leader advance not captured"))
    }
    fn colr(&mut self, face: FaceId, cp: u32) -> LResult<bool> {
        self.colr.get(&(face_ix(face), cp)).copied().ok_or(miss("replay: colr not captured"))
    }
    fn face_key(&mut self, face: FaceId) -> LResult<Sym> {
        self.res.face(face).map(|f| f.key).ok_or(miss("replay: face"))
    }
    fn face_synth(&mut self, face: FaceId) -> LResult<(bool, bool)> {
        self.res.face(face).map(|f| (f.synth_bold, f.synth_italic)).ok_or(miss("replay: face"))
    }
    fn fx_draws(&self, fx: TextFxId) -> bool {
        self.res.text_fx(fx).is_some_and(|f| f.outline.is_some() || f.glow.is_some() || f.shadow.is_some() || f.reflection.is_some())
    }
    fn intern(&mut self, s: &[u16]) -> LResult<Sym> {
        self.res.symbols.intern(s).ok_or(miss("replay: symbols"))
    }
    fn sym_text(&self, s: Sym) -> Vec<u16> {
        self.res.sym(s).to_vec()
    }
    fn settings(&self) -> BreakSettings {
        self.settings
    }
    fn word97_line_breaks(&self) -> bool {
        self.word97
    }
    fn east_asia_font(&mut self, rpr: &RPr) -> LResult<bool> {
        let k = self.styles.iter().position(|s| std::ptr::eq(&*s.rpr, rpr)).ok_or(miss("replay: style of rpr"))?;
        Ok(self.style_ea[k])
    }
    fn next_line_seq(&mut self) -> LResult<LineSeq> {
        self.line_seq += 1;
        Ok(LineSeq(self.line_seq))
    }
    fn inline_object(&mut self, _p: &PreparedPara, _g: &Glyph, _x0: f64, _b: f64, _out: &mut PassItems) -> LResult<()> {
        // the Rust side of `paintInlineObject` needs the engine's image table (F7b): pending
        Err(LayoutError::Unported(uo_docx_layout::error::Frontier::F7b, "paint::paint_inline_object (image table, F7b)"))
    }
    fn hr_shape(&mut self, _p: &PreparedPara, g: &Glyph) -> LResult<Option<HrShape>> {
        let para = self.paras.get(self.cur_para).ok_or(miss("replay: para"))?;
        Ok(g.obj.and_then(|k| para.extras.shapes.get(k.ix()).cloned().flatten()))
    }
    fn charge(&mut self, _units: u64) -> LResult<()> {
        Ok(())
    }
}

fn kind_of(s: &str) -> R<GlyphKind> {
    Ok(match s {
        "char" => GlyphKind::Char,
        "space" => GlyphKind::Space,
        "nbsp" => GlyphKind::Nbsp,
        "tab" => GlyphKind::Tab,
        "lb" => GlyphKind::Lb,
        "pb" => GlyphKind::Pb,
        "cb" => GlyphKind::Cb,
        "obj" => GlyphKind::Obj,
        "shy" => GlyphKind::Shy,
        "box" => GlyphKind::Box,
        k => return Err(format!("glyph kind {k}")),
    })
}

fn align_of(s: &str) -> TabAlign {
    match s {
        "center" => TabAlign::Center,
        "right" => TabAlign::Right,
        "decimal" => TabAlign::Decimal,
        "bar" => TabAlign::Bar,
        "clear" => TabAlign::Clear,
        "num" => TabAlign::Num,
        _ => TabAlign::Left,
    }
}

fn sym(res: &mut Resources, s: &[u16]) -> Sym {
    res.symbols.intern(s).unwrap()
}

fn text_fx(r: &mut Rd<'_>, res: &mut Resources) -> R<ResolvedTextFx> {
    fn fill(r: &mut Rd<'_>, res: &mut Resources) -> R<ResolvedFill> {
        Ok(match r.tag()? {
            b'n' => ResolvedFill::None,
            b's' => {
                let hex = r.u16s()?;
                ResolvedFill::Solid { hex: sym(res, &hex), alpha: r.num()? }
            }
            _ => {
                let n = r.u32()?;
                let mut stops = Vec::new();
                for _ in 0..n {
                    let pos = r.num()?;
                    let hex = r.u16s()?;
                    stops.push(FxStop { pos, hex: sym(res, &hex), alpha: r.num()? });
                }
                let angle_deg = r.num()?;
                let scaled = r.bool()?;
                let path = r.opt_string()?.map(|s| sym(res, &s.encode_utf16().collect::<Vec<_>>()));
                let fill_to =
                    if r.bool()? { Some(uo_render::types::Ltrb { l: r.num()?, t: r.num()?, r: r.num()?, b: r.num()? }) } else { None };
                ResolvedFill::Grad { stops, angle_deg, scaled, path, fill_to }
            }
        })
    }
    let mut fx = ResolvedTextFx::default();
    if r.bool()? {
        fx.fill = Some(fill(r, res)?);
    }
    if r.bool()? {
        let width_pt = r.num()?;
        let f = fill(r, res)?;
        let dash = r.opt_string()?.map(|s| sym(res, &s.encode_utf16().collect::<Vec<_>>()));
        let cap = match r.string()?.as_str() {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        };
        let join = match r.string()?.as_str() {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        fx.outline = Some(FxOutline { width_pt, fill: f, dash, cap, join });
    }
    if r.bool()? {
        let (blur_pt, dist_pt, dir_deg, sx, sy, kx_deg, ky_deg) = (r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?);
        let algn = r.u16s()?;
        let hex = r.u16s()?;
        fx.shadow = Some(FxShadow {
            blur_pt,
            dist_pt,
            dir_deg,
            sx,
            sy,
            kx_deg,
            ky_deg,
            algn: sym(res, &algn),
            hex: sym(res, &hex),
            alpha: r.num()?,
        });
    }
    if r.bool()? {
        let rad_pt = r.num()?;
        let hex = r.u16s()?;
        fx.glow = Some(FxGlow { rad_pt, hex: sym(res, &hex), alpha: r.num()? });
    }
    if r.bool()? {
        let (blur_pt, st_a, st_pos, end_a, end_pos, dist_pt, dir_deg) =
            (r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?);
        let algn = r.u16s()?;
        fx.reflection = Some(FxReflection { blur_pt, st_a, st_pos, end_a, end_pos, dist_pt, dir_deg, algn: sym(res, &algn) });
    }
    Ok(fx)
}

fn style(r: &mut Rd<'_>, env: &mut Env) -> R<()> {
    let size_pt = r.num()?;
    let color: Box<[u16]> = r.u16s()?.into();
    let underline = if r.bool()? { Some((r.string()?.into_boxed_str(), r.opt_string()?.map(String::into_boxed_str))) } else { None };
    let strike = r.bool()?;
    let dstrike = r.bool()?;
    let highlight = r.opt_string()?.map(String::into_boxed_str);
    let shading = r.opt_string()?.map(String::into_boxed_str);
    let shift = r.num()?;
    let alpha = OptF64::from(r.opt_num()?);
    let outline = r.bool()?;
    let fx = if r.bool()? {
        let f = text_fx(r, &mut env.res)?;
        Some(env.res.add_text_fx(f).unwrap())
    } else {
        None
    };
    let fx_key = r.opt_string()?.map(String::into_boxed_str);
    let mut rpr = RPr {
        w: r.opt_num()?.map(JsNumber::Num),
        spacing: r.opt_num()?.map(JsNumber::Num),
        vert_align: r.opt_string()?.map(String::into_boxed_str),
        sz: r.opt_num()?.map(JsNumber::Num),
        ..RPr::default()
    };
    if r.bool()? {
        rpr.text_shadow = Some(Box::new(TextShadow {
            blur: JsNumber::Num(r.num()?),
            dist: JsNumber::Num(r.num()?),
            dir: JsNumber::Num(r.num()?),
            ..TextShadow::default()
        }));
    }
    env.style_ea.push(r.bool()?);
    env.styles.push(RunStyle {
        rpr: Arc::new(rpr),
        size_pt,
        color,
        underline,
        strike,
        dstrike,
        highlight,
        shading,
        shift,
        alpha,
        outline,
        fx,
        fx_key,
        ..RunStyle::default()
    });
    Ok(())
}

/// Maps the dump's global style labels to the paragraph's own `styles` (identity kept: one entry
/// per label).
struct LocalStyles<'e> {
    env: &'e Env,
    labels: Vec<u32>,
}

impl LocalStyles<'_> {
    fn ix(&mut self, label: u32) -> StyleIx {
        let k = self.labels.iter().position(|&l| l == label).unwrap_or_else(|| {
            self.labels.push(label);
            self.labels.len() - 1
        });
        StyleIx(u32::try_from(k).unwrap())
    }
    fn styles(&self) -> R<Vec<RunStyle>> {
        self.labels.iter().map(|&l| self.env.styles.get(l as usize).cloned().ok_or_else(|| format!("style {l}"))).collect()
    }
}

fn glyph(r: &mut Rd<'_>, ls: &mut LocalStyles<'_>, extras: &mut Extras, math: &mut Vec<MathBox>) -> R<Glyph> {
    let kind = kind_of(&r.string()?)?;
    let ch_units = r.u16s()?;
    if ch_units.len() > 2 {
        return Err("glyph ch longer than one code point".into());
    }
    let mut ch = Ch::default();
    for (k, u) in ch_units.iter().enumerate() {
        ch.units[k] = *u;
    }
    ch.len = u8::try_from(ch_units.len()).unwrap();
    let cp = r.num()? as u32;
    let (adv, asc, desc) = (r.num()?, r.num()?, r.num()?);
    let (n_asc, n_desc) = (OptF64::from(r.opt_num()?), OptF64::from(r.opt_num()?));
    let offset = r.num()? as u32;
    let run_idx = r.num()? as u32;
    let mut flags = 0u16;
    for f in [GlyphFlags::BREAK_AFTER, GlyphFlags::GLUE, GlyphFlags::CLEAR, GlyphFlags::FIXED, GlyphFlags::HANG, GlyphFlags::HYPH] {
        if r.bool()? {
            flags |= f;
        }
    }
    let hr = OptF64::from(r.opt_num()?);
    let fe_shrink = OptF64::from(r.opt_num()?);
    let size_pt = r.num()?;
    let face = FaceId::from_index(r.u32()? as usize).unwrap();
    let style = ls.ix(r.u32()?);
    let ptab = if r.bool()? {
        let alignment = match r.string()?.as_str() {
            "center" => PtabAlign::Center,
            "right" => PtabAlign::Right,
            _ => PtabAlign::Left,
        };
        let leader = r.opt_string()?.filter(|l| !l.is_empty() && l != "none").map(|l| leader_from_ts(&l));
        extras.ptabs.push(PtabSpec { alignment, leader });
        Some(u32::try_from(extras.ptabs.len() - 1).unwrap())
    } else {
        None
    };
    let obj = if r.bool()? {
        let sh = if r.bool()? {
            Some(HrShape {
                hr_pct: r.opt_num()?,
                width_twips: r.num()?,
                height_twips: r.num()?,
                hr_align: r.opt_string()?.map(String::into_boxed_str),
                fill: r.u16s()?.into(),
                hr_no_shade: r.bool()?,
            })
        } else {
            None
        };
        extras.shapes.push(sh);
        Some(RunKey(u32::try_from(extras.shapes.len() - 1).unwrap()))
    } else {
        None
    };
    let check_box = if r.bool()? { Some((r.num()?, r.bool()?)) } else { None };
    let sub_gid = r.opt_num()?.map(|v| v as u32);
    let m = if r.bool()? {
        let mut mb = MathBox { w: r.num()?, ..MathBox::default() };
        for _ in 0..r.u32()? {
            let (cp, x, y) = (r.num()? as u32, r.num()?, r.num()?);
            let face = FaceId::from_index(r.u32()? as usize).unwrap();
            let size_pt = r.num()?;
            let style = ls.ix(r.u32()?);
            mb.items.push(MathItem { cp, x, y, face, size_pt, style });
        }
        for _ in 0..r.u32()? {
            mb.rules.push(MathRule { x: r.num()?, y: r.num()?, w: r.num()?, h: r.num()?, color: r.u16s()?.into() });
        }
        math.push(mb);
        Some(u32::try_from(math.len() - 1).unwrap())
    } else {
        None
    };
    Ok(Glyph {
        kind,
        ch,
        cp,
        face,
        style,
        adv,
        asc,
        desc,
        n_asc,
        n_desc,
        offset,
        run_idx,
        flags: GlyphFlags(flags),
        obj,
        hr,
        ptab,
        size_pt,
        check_box,
        note_id: None,
        fe_shrink,
        kern_adj: OptF64::NONE,
        lig_start: None,
        lig_end: None,
        sub_gid,
        math: m,
    })
}

fn para(r: &mut Rd<'_>, env: &Env) -> R<Para> {
    let (ind_left, ind_right, first_line) = (r.num()?, r.num()?, r.num()?);
    let jc = r.string()?.into_boxed_str();
    let line_rule = match r.string()?.as_str() {
        "exact" => LineRule::Exact,
        "atLeast" => LineRule::AtLeast,
        _ => LineRule::Auto,
    };
    let line = r.num()?;
    let mut tabs = Vec::new();
    for _ in 0..r.u32()? {
        let val = align_of(&r.string()?);
        let pos = r.num()?;
        let leader = r.opt_string()?.map(|l| leader_from_ts(&l));
        tabs.push(TabStop { val, pos, leader });
    }
    let mut ls = LocalStyles { env, labels: Vec::new() };
    let mark_style = ls.ix(r.u32()?);
    let mark_face = FaceId::from_index(r.u32()? as usize).unwrap();
    let mut extras = Extras::default();
    let mut math = Vec::new();
    let marker = if r.bool()? {
        let width = r.num()?;
        let suff = match r.string()?.as_str() {
            "tab" => MarkerSuffix::Tab,
            "space" => MarkerSuffix::Space,
            _ => MarkerSuffix::Nothing,
        };
        let mjc = r.string()?.into_boxed_str();
        let mut glyphs = Vec::new();
        for _ in 0..r.u32()? {
            glyphs.push(glyph(r, &mut ls, &mut extras, &mut math)?);
        }
        Some(ParaMarker { text: Box::default(), glyphs, width, level: 0, suff, jc: mjc, pic_bullet: None, chapter: None })
    } else {
        None
    };
    let mut glyphs = Vec::new();
    for _ in 0..r.u32()? {
        glyphs.push(glyph(r, &mut ls, &mut extras, &mut math)?);
    }
    let styles = ls.styles()?;
    Ok(Para {
        p: PreparedPara {
            para: LParaId(0),
            ppr: Arc::default(),
            style_id: None,
            styles,
            mark_style,
            mark_face,
            glyphs,
            marker,
            ind_left,
            ind_right,
            first_line,
            jc,
            tabs,
            space_before: 0.0,
            space_after: 0.0,
            before_auto: false,
            after_auto: false,
            line_rule,
            line,
            contextual: false,
            keep_next: false,
            keep_lines: false,
            widow_control: false,
            page_break_before: false,
            empty: false,
            rev_bars: None,
            math,
        },
        extras,
    })
}

fn placed_list(r: &mut Rd<'_>, p: &PreparedPara) -> R<Option<Vec<PlacedGlyph>>> {
    let n = r.u32()?;
    let mut out = Vec::new();
    let mut ok = true;
    for _ in 0..n {
        let g = match r.tag()? {
            b'p' => Some(GlyphSrc::Para(r.u32()?)),
            b'm' => Some(GlyphSrc::Marker(r.u32()?)),
            b'h' => {
                let off = r.num()?;
                p.glyphs
                    .iter()
                    .position(|g| f64::from(g.offset) + f64::from(g.ch.len) == off)
                    .map(|k| GlyphSrc::AutoHyphen(u32::try_from(k).unwrap()))
            }
            _ => None,
        };
        let (x, w) = (r.num()?, r.num()?);
        let leader = r.opt_string()?.map(|l| leader_from_ts(&l));
        match g {
            Some(g) => out.push(PlacedGlyph { g, x, w, leader }),
            None => ok = false,
        }
    }
    Ok(ok.then_some(out))
}

// ── the Rust side of each call ──────────────────────────────────────────────────────────────

fn write_placed(w: &mut Raw, p: &PreparedPara, list: &[PlacedGlyph]) {
    w.u32v(u32::try_from(list.len()).unwrap());
    for q in list {
        match q.g {
            GlyphSrc::Para(i) => w.tagc(b'p').u32v(i),
            GlyphSrc::Marker(j) => w.tagc(b'm').u32v(j),
            GlyphSrc::AutoHyphen(_) => {
                let off = glyph_of(p, q.g).map_or(f64::NAN, |g| f64::from(g.offset));
                w.tagc(b'h').num(off)
            }
        };
        w.num(q.x).num(q.w).opt_str(q.leader.map(TabLeader::name));
    }
}

fn write_frame(w: &mut Raw, f: &LineFrame) {
    w.num(f.left).num(f.right).boolv(f.gap).opt_num(f.next.map(|n| n.0)).opt_num(f.next.map(|n| n.1));
}

fn write_line(w: &mut Raw, id: u32, f: &LineFrame, l: &Line, p: &PreparedPara) {
    w.tagc(b'n').u32v(id);
    write_frame(w, f);
    w.num(f64::from(l.start))
        .num(f64::from(l.end))
        .num(l.width)
        .num(l.x_start)
        .num(l.asc)
        .num(l.desc)
        .num(l.height)
        .num(l.baseline)
        .boolv(l.last);
    w.opt_str(l.break_after.map(|b| match b {
        uo_docx_layout::lines::BreakKind::Page => "page",
        uo_docx_layout::lines::BreakKind::Column => "column",
    }));
    w.boolv(l.clear);
    w.opt_str(l.gap_tab.map(|g| match g {
        uo_docx_layout::lines::GapTab::Break => "break",
        uo_docx_layout::lines::GapTab::Start => "start",
    }));
    w.boolv(l.spanned).boolv(l.hr_frame.is_some());
    if let Some((a, b)) = l.hr_frame {
        w.num(a).num(b);
    }
    write_placed(w, p, &l.placed);
    w.boolv(l.marker.is_some());
    if let Some(m) = &l.marker {
        write_placed(w, p, m);
    }
}

/// The P1 canonical stream of a page holding `out` (as facets-f5.mjs `paintOut`).
fn canon_items(out: PassItems, res: &Resources) -> Vec<u8> {
    let mut page = LayoutPage::default();
    for (i, side) in out.side.iter().enumerate() {
        if let Some(m) = side.glyph {
            page.item_meta.push((u32::try_from(i).unwrap(), m));
        }
        if let Some(src) = side.clip_src {
            page.clip_src.push((u32::try_from(i).unwrap(), src));
        }
    }
    page.page.list = out.list;
    let layout = Layout { pages: vec![page], resources: res.clone(), ..Layout::default() };
    let mut w = Raw::default();
    canon_layout(&layout, &|_| None, &mut w);
    w.0
}

#[derive(Default, Debug)]
struct Tally {
    lines_equal: usize,
    lines_mismatch: usize,
    paint_equal: usize,
    paint_mismatch: usize,
    skipped: usize,
    pending: BTreeMap<String, usize>,
    errors: BTreeMap<String, usize>,
    first: Vec<String>,
}

/// Replays one document's dump; returns the rebuilt facet values (`p3.lines`, `p3.paint`).
fn replay_doc(key: &str, dump: &[u8], t: &mut Tally) -> R<(String, String)> {
    let mut r = Rd { b: dump, at: 0 };
    let mut env = Env::default();
    let mut breakers: HashMap<u32, (usize, LineBreaker, BreakSettings, bool)> = HashMap::new();
    let mut lines = StreamSink(Stream::new());
    let mut paint = StreamSink(Stream::new());
    let note = |t: &mut Tally, s: String| {
        if t.first.len() < 12 {
            t.first.push(s);
        }
    };
    loop {
        let tag = r.tag()?;
        match tag {
            b'E' => break,
            b'F' => {
                let l = r.u32()? as usize;
                assert_eq!(l, env.faces.len(), "face labels in order");
                let key_s = r.u16s()?;
                let fam = r.u16s()?;
                let (bold, italic, synth_bold, synth_italic, stand_in) = (r.bool()?, r.bool()?, r.bool()?, r.bool()?, r.bool()?);
                let m = FaceMetrics {
                    ascent: r.num()?,
                    descent: r.num()?,
                    underline_pos: r.num()?,
                    underline_thick: r.num()?,
                    ..FaceMetrics::default()
                };
                let (upm, win_descent, descender) = (r.num()?, r.num()?, r.num()?);
                let (k, f) = (sym(&mut env.res, &key_s), sym(&mut env.res, &fam));
                env.res.add_face(FaceRes {
                    key: k,
                    family: f,
                    bold,
                    italic,
                    synth_bold,
                    synth_italic,
                    stand_in,
                    face: None,
                    bytes: None,
                    advances: None,
                });
                env.faces.push(FaceRec { m, upm, win_descent, descender });
            }
            b'S' => {
                let l = r.u32()? as usize;
                assert_eq!(l, env.styles.len(), "style labels in order");
                style(&mut r, &mut env)?;
            }
            b'P' => {
                let l = r.u32()? as usize;
                assert_eq!(l, env.paras.len(), "paragraph labels in order");
                let p = para(&mut r, &env)?;
                env.paras.push(p);
            }
            b'A' => {
                let (f, cp, size, v) = (r.u32()?, r.num()? as u32, r.num()?, r.num()?);
                env.adv.insert((f, cp, bits(size)), v);
            }
            b'V' => {
                let (f, em, v) = (r.u32()?, r.num()?, r.opt_num()?);
                env.vdmx.insert((f, bits(em)), v);
            }
            b'W' => {
                let (f, cp, a, u) = (r.u32()?, r.num()? as u32, r.num()?, r.num()?);
                env.lead.insert((f, cp), (a, u));
            }
            b'C' => {
                let (f, cp, c) = (r.u32()?, r.num()? as u32, r.bool()?);
                env.colr.insert((f, cp), c);
            }
            b'B' => {
                let id = r.u32()?;
                let pl = r.u32()? as usize;
                let grid = r.opt_num()?;
                let s = BreakSettings {
                    default_tab_stop: r.num()?,
                    compatibility_mode: r.num()?,
                    no_indent_as_numbering_tab_stop: r.bool()?,
                    split_pg_break_and_para_mark: r.bool()?,
                    ..BreakSettings::default()
                };
                let word97 = r.bool()?;
                let s = BreakSettings { consecutive_hyphen_limit: r.opt_num()?.unwrap_or(0.0), hyphenation_zone: r.opt_num()?, ..s };
                let p = &env.paras.get(pl).ok_or("breaker of an unknown paragraph")?.p;
                breakers.insert(id, (pl, LineBreaker::new(p, grid), s, word97));
            }
            b'N' => {
                let id = r.u32()?;
                let frame = LineFrame {
                    left: r.num()?,
                    right: r.num()?,
                    gap: r.bool()?,
                    next: match (r.opt_num()?, r.opt_num()?) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => None,
                    },
                };
                let ts = r.bytes()?;
                let (pl, br, s, w97) = breakers.get_mut(&id).ok_or("next of an unknown breaker")?;
                env.settings = *s;
                env.word97 = *w97;
                env.cur_para = *pl;
                let para = env.paras.get(*pl).ok_or("para")?;
                // the replay env answers from the dump; the paragraph is read through a clone so
                // the env can be borrowed mutably
                let p = para.p.clone();
                let out = br.next_in(&mut env, &p, frame);
                match out {
                    Ok(l) => {
                        let mut w = Raw::default();
                        write_line(&mut w, id, &frame, &l, &p);
                        if w.0 == ts {
                            t.lines_equal += 1;
                        } else {
                            t.lines_mismatch += 1;
                            note(t, format!("{key}: next of breaker {id} differs (byte {})", first_diff(&w.0, &ts)));
                        }
                        lines.0.bytes(&w.0);
                    }
                    Err(e) => {
                        *t.errors.entry(format!("lines: {e}")).or_default() += 1;
                        lines.0.bytes(&ts);
                    }
                }
            }
            b'R' => {
                let id = r.u32()?;
                let m = BreakMark { pos: r.num()? as u32, line_idx: r.num()? as u32, trailing_break: r.bool()?, hyphen_run: r.num()? };
                let ts = r.bytes()?;
                let (_, br, _, _) = breakers.get_mut(&id).ok_or("reset of an unknown breaker")?;
                br.reset(m);
                let mut w = Raw::default();
                w.tagc(b'r').u32v(id).num(f64::from(m.pos)).num(f64::from(m.line_idx)).boolv(m.trailing_break).num(m.hyphen_run);
                if w.0 == ts {
                    t.lines_equal += 1
                } else {
                    t.lines_mismatch += 1
                }
                lines.0.bytes(&w.0);
            }
            b'K' => {
                let id = r.u32()?;
                let ts = r.bytes()?;
                let (pl, br, _, _) = breakers.get(&id).ok_or("peek of an unknown breaker")?;
                let p = &env.paras.get(*pl).ok_or("para")?.p;
                let mut w = Raw::default();
                w.tagc(b'k').u32v(id).num(br.peek_word_width(p));
                if w.0 == ts {
                    t.lines_equal += 1;
                } else {
                    t.lines_mismatch += 1;
                    note(t, format!("{key}: peekWordWidth of breaker {id} differs"));
                }
                lines.0.bytes(&w.0);
            }
            b'D' => {
                let pl = r.u32()? as usize;
                let (frame_x, top, baseline, height) = (r.num()?, r.num()?, r.num()?, r.num()?);
                let hr_frame = if r.bool()? { Some((r.num()?, r.num()?)) } else { None };
                let p = env.paras.get(pl).ok_or("paint of an unknown paragraph")?.p.clone();
                let placed = placed_list(&mut r, &p)?;
                let marker = if r.bool()? { Some(placed_list(&mut r, &p)?) } else { None };
                let ts = r.bytes()?;
                let (Some(placed), Some(marker)) = (placed, marker.map_or(Some(None), |m| m.map(Some))) else {
                    t.skipped += 1;
                    paint.0.bytes(&ts);
                    continue;
                };
                env.cur_para = pl;
                let line = Line {
                    start: 0,
                    end: 0,
                    placed,
                    marker,
                    width: 0.0,
                    x_start: 0.0,
                    asc: 0.0,
                    desc: 0.0,
                    height,
                    baseline,
                    break_after: None,
                    clear: false,
                    last: false,
                    gap_tab: None,
                    spanned: false,
                    hr_frame,
                };
                let mut out = PassItems::new();
                match paint_line_in(&mut env, &p, &line, frame_x, top, &mut out) {
                    Ok(()) => {
                        let mine = canon_items(out, &env.res);
                        if mine == ts {
                            t.paint_equal += 1;
                        } else {
                            t.paint_mismatch += 1;
                            note(t, format!("{key}: paintLine of paragraph {pl} differs (byte {})", first_diff(&mine, &ts)));
                        }
                        paint.0.bytes(&mine);
                    }
                    Err(LayoutError::Unported(fr, what)) => {
                        *t.pending.entry(format!("{fr:?} {what}")).or_default() += 1;
                        paint.0.bytes(&ts);
                    }
                    Err(e) => {
                        *t.errors.entry(format!("paint: {e}")).or_default() += 1;
                        paint.0.bytes(&ts);
                    }
                }
            }
            b'X' => {
                let why = r.string()?;
                let ts = r.bytes()?;
                t.skipped += 1;
                note(t, format!("{key}: paint call not replayable ({why})"));
                paint.0.bytes(&ts);
            }
            other => return Err(format!("{key}: unknown record {:?} at {}", other as char, r.at - 1)),
        }
    }
    Ok((lines.value(), paint.value()))
}

fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

fn safe(key: &str) -> String {
    key.replace(['/', '\\', ':'], "__")
}

fn run(cap_path: &Path, dump: &Path) -> Tally {
    let cap = Capture::read(cap_path).unwrap();
    let mut replay = Replay::new(&cap);
    let mut t = Tally::default();
    for rec in cap.records() {
        for (f, _) in &rec.facets {
            if f != "p3.lines" && f != "p3.paint" {
                replay.skip(&rec.key, f);
            }
        }
        let path = dump.join(format!("{}.f5.bin", safe(&rec.key)));
        let Ok(bytes) = std::fs::read(&path) else {
            replay.skip(&rec.key, "p3.lines");
            replay.skip(&rec.key, "p3.paint");
            continue;
        };
        let before = (t.pending.values().sum::<usize>(), t.errors.values().sum::<usize>(), t.skipped);
        match replay_doc(&rec.key, &bytes, &mut t) {
            Ok((l, p)) => {
                let clean = before == (t.pending.values().sum::<usize>(), t.errors.values().sum::<usize>(), t.skipped);
                if clean {
                    replay.check(&rec.key, "p3.lines", &l);
                    replay.check(&rec.key, "p3.paint", &p);
                } else {
                    // pending or skipped calls carry the TS bytes: the call counts above are the
                    // result; the digest equality would be circular
                    replay.skip(&rec.key, "p3.lines");
                    replay.skip(&rec.key, "p3.paint");
                }
            }
            Err(e) => panic!("{}: {e}", rec.key),
        }
    }
    let report = replay.finish();
    eprintln!("f5 facets: {}; {t:?}", report.summary());
    report.assert_clean();
    t
}

#[test]
fn f5_facets_committed_fixture() {
    let dir = common::manifest().join("tests/captures/f5");
    let t = run(&dir.join("layout-ci.capture"), &dir);
    assert!(t.lines_equal > 0 && t.paint_equal > 0, "{t:?}");
    assert_eq!((t.lines_mismatch, t.paint_mismatch), (0, 0), "{t:?}");
    assert!(t.errors.is_empty(), "{t:?}");
}

#[test]
fn f5_facets_against_a_fresh_capture() {
    let (Ok(cap), Ok(dump)) = (std::env::var("UO_F5_CAPTURE"), std::env::var("UO_F5_DUMP")) else {
        eprintln!("f5 facets: set UO_F5_CAPTURE and UO_F5_DUMP to replay a fresh capture");
        return;
    };
    let t = run(&PathBuf::from(cap), &PathBuf::from(dump));
    assert_eq!((t.lines_mismatch, t.paint_mismatch), (0, 0), "{:?}", t.first);
    assert!(t.errors.is_empty(), "{t:?}");
}
