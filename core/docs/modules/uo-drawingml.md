# uo-drawingml — presets, colour modifiers, raster effects, image adjustments

Record of the rust-port workflow. Status: **stage 1 plan revised after the analysis skeptic
(stage 2), gate 1 pending** (planner + reuse specialist + skeptic, 2026-11-01). Prerequisite: the
shared crate `uo-jsnum` (question G1, record `core/docs/modules/uo-jsnum.md`) must pass its own
gate 2 before stage 3 of this module starts. Revisions after the skeptic are marked
"(revised after the skeptic, An)"; the skeptic's own section is kept unchanged below the plan.

## Scope

- **TS covered** (behaviour source, frozen on `main` 0.9.x):
  - `shared/drawingml/src`: `preset-defs.ts` (data), `preset-engine.ts` (all exports),
    `geometry.ts` (`presetPath` and the types `Pt`/`PathSeg`/`PresetPath`), `outline.ts`,
    `color.ts`, `raster.ts`, `effects.ts`, `light3d.ts`, `bevel.ts`, `bevel-word.ts` (data),
    `adjust.ts`, `patterns.ts`;
  - `write/packages/docx-layout/src/images.ts`: the Word blip colour effects `effectLine`,
    `applyImageEffects`, `effectDecode`, `applyPictureAdjust` (question G4). The codecs in the same
    file (`decodePng/Gif/Bmp/Tiff`, `jpegInfo`, `imagePixelSize`) are not in scope (G5).
- **C# covered**: `dotnet/src/UniversalOffice.Core/DrawingML/*` and `DmlColorMods.cs` are a subset
  port of the TS (no adjust, contour, shape styles, `presetPath`, blip effects); they are the type
  guide only. Nothing exists only in C#: `TsResource` (reads the TS tables at run time) is replaced
  by generated tables, `JsNumber` by `uo-jsnum`.
- **Moves out of the shared package (Word-only, G3)**: `contour.ts` (`wrapContour`, Word
  `wp:wrapPolygon` in 21600 units) and `shape-styles.ts` (`SHAPE_STYLE_PRESETS`, the `wps:style`
  gallery) go to `uo-docx` (editing API) with its plan; `patternRowsFromImage` (Word VML GIF,
  `docx-layout/src/page-fill.ts`) stays with Write.
- **Left for other modules / later records** (registered, not planned here — G6):
  - DrawingML colour **resolver** (`a:srgbClr/schemeClr/sysClr/prstClr` + mods from a
    `uo_opc::xml::Element`, clrScheme, clrMap): today 4 non-identical copies (`color.ts`,
    `shared/charts/src/dml-color.ts`, `slides-core/src/theme.ts`, C# `DmlColorMods.cs`; Slides and
    charts compute `hueOff` as `(parseInt/1e5·1e5)/21600000` and differ in `touched`); stale
    different algorithms in the converter (`slides-processing/src/theme.ts`, C# `Pptx/Theme.cs`,
    `Converter/.../Theme.cs`) and Write's own `docx-core/src/serialize.ts:shapeColor`. SpreadsheetML
    integer HLS `tint` (`sheet-core/src/theme.ts`) is a different spec and stays in Sheet.
    **First consumer: `uo-docx`** (revised after the skeptic, A9): Write's docx-core reads
    `a:srgbClr/schemeClr/sysClr/prstClr` into a `"RRGGBB|lumMod=…"` string (drawing.ts:259–264)
    that `resolveDrawingColor` parses (`parseInt(v, 10)`, resolve.ts:397–411) before calling
    `applyColorMods`; the resolver (XML element + theme + clrMap) is designed in the uo-docx plan
    and placed in this crate, so that string encoding is not ported as Write-only glue and then
    duplicated at the Slides port. Deferring it does not block the Write switch: this module ports
    `apply_color_mods`, the only piece Write takes from here.
  - DrawingML **fills**: gradient sampling and geometry, pattern rasters, blip tiles
    (`docx-layout/src/geometry.ts`; Slides and charts have canvas-bound copies) — designed with
    `uo-render`, because their output is display-list content.
  - **msospt** (binary shape type → `prst`): three copies (`slides-core/src/ppt/preset-map.ts`,
    `docx-core/src/doc/preset-map.ts`, `sheet-core/src/xls/odraw-geom.ts`, C# `PresetMap.cs`),
    planned with the first legacy-format port.
  - Write's own `presetPathLocal` (twips, arcs as cubic Béziers, `docx-layout/src/geometry.ts`) and
    the canvas effect pipelines (`canvas-picture.ts`/`pdf-picture.ts`/`textfx.ts`) stay with
    `uo-docx-layout`; Slides' canvas `applyBevel`/`edt1d` and its PowerPoint blip model
    (`render.ts` ~452–500) arrive with the Slides port (the blip model as a named variant, G4).
- **Converter and C# stale copies** (`converter/packages/slides-processing/src/geometry.ts`,
  C# `Pptx/Geometry.cs`, `Converter/Processing/Slides/Format/Geometry.cs`, the converter themes) are
  replaced by this crate when those products move; they are not parity references (as uo-opc G7).

### Users (all found)

| User | Functions |
|---|---|
| Write `docx-layout` (`canvas-picture.ts`, `pdf-picture.ts`, `textfx.ts`, `engine.ts`, `geometry.ts`, `resolve.ts`, `picture.ts`, `pdf.ts`) | `presetOutline`, `flattenPath`, `fillPolygons`, `newMask`, `gaussianBlur`, `morph`, the five effect masks, `reflectionAlpha`, `shadeBevel`, `presetTextRect`, `patternRows`, `applyColorMods`; blip effects (`images.ts`) |
| Write `editor` (`chrome-picture-adjust.ts`, `universal-write.ts`, `chrome-picture.ts`, `chrome-page.ts`, `chrome-picture-pane.ts`, `chrome-shape.ts`, `shape-ops.ts`) | `adjust*` on `ImageData`, `presetOutline` (gallery icons), `patternRows`, `PRESET_PATTERNS` (UI list, key order), `wrapContour`, `SHAPE_STYLE_PRESETS` |
| Slides `slides-core` (`render.ts`, `read.ts`, `diagram-layout.ts`, `patterns.ts`, `core.ts`) | `presetPath`, `presetTextRect`, `evalPreset`, `hasPresetDef`, `presetGuides`, `customGuides`, `arcToSeg`, `lightTable`, `lightAt`, `lightFace`, `bevelPhi`, patterns |
| Slides shell `slides/packages/slides/src/chrome-widgets.ts:266` (revised after the skeptic, A10) | `evalPreset` for the shape gallery icons — needs a wasm export when Slides switches (G10) |
| Sheet | nothing directly: shapes go through the `slides-core` reader; charts use `shared/charts` (own colour copy) |
| Converter / C# | stale copies only (above); C# `DocxLayout/*` calls the C# DrawingML port |

## Plan (planner + reuse specialist — proposed, revised after the skeptic; gate 1 pending)

### Crate, dependencies and reuse

- **One crate `uo-drawingml`**, shared by Write, Slides and Sheet; no Word assumptions (G3 moves the
  two Word-only files out). `#![forbid(unsafe_code)]`. Dependencies: **`uo-jsnum` only** (G1); no
  third-party crates. `uo-opc` is not needed by this module (no XML is parsed: the preset
  definitions become generated tables); the later colour resolver and custGeom reader will take
  `uo_opc::xml::Element`.
- **Generated tables, no runtime parsing, no caches**: a generator
  (`core/crates/uo-drawingml/tools/gen-tables.mjs`, run against the frozen TS through `UO_TS_ROOT`,
  using `uo-testkit/js/testkit.mjs`) writes checked-in Rust files from the TS sources:
  `preset-defs.ts` (tokenized exactly like `parseDef`: `split('|')`, `split(';')`, `indexOf('=')`,
  `words()` = `trim().split(/\s+/).filter(Boolean)`, `split('#')`, `split('~')`, `split(',')`),
  `light3d.ts` `TABLES` and `bevel-word.ts` `WORD_BEVEL` (base64 decoded to bytes), `patterns.ts`
  (file order kept), `adjust.ts` `BRIGHT_LUT`/`CONTRAST_LUT`/`TEMP_TABLE` (as numbers), `light3d.ts`
  `PROFILES`, and `bevel.ts` `SRGB_LIN` (256 × f32 bits) / `LIN_SRGB` (4096 × u8) **evaluated in
  Node on the capture platform** (they use `Math.pow`). The generator asserts that no key is
  integer-like (so JS object order = file order). A test (`UO_TS_PARITY=1`) re-runs it and diffs.
  The TS runtime caches (`parsed` in preset-engine, which caches `null` for every unknown name;
  `lutCache` keyed by an arbitrary float; `decoded`; `wordTabs`) are not ported: static data plus
  per-call LUTs give the same results with no unbounded growth (M5).
- **Reuse later**: Slides calls the same API (`preset_path`, `preset_guides`, `custom_guides`,
  `arc_to_seg`, light3d); `uo-docx-layout` builds its picture/text-effect pipeline on `raster` +
  `effects` + `bevel` + `blip`; `uo-charts` and the Slides theme port call `apply_color_mods`
  (their hue/`parseInt` quirks become named variants until stage 5, per the reuse specialist).

### Module layout and TS → Rust mapping (port order)

| # | Rust module | TS source | Notes |
|---|---|---|---|
| 0 | — | — | prerequisite: `uo-jsnum` (G1/G2) |
| 1 | `tables/{presets,light,word_bevel,patterns,adjust,srgb}.rs` (generated) | data of preset-defs, light3d, bevel-word, patterns, adjust, bevel | generator + diff test first |
| 2 | `error.rs`, `limits.rs`, `lib.rs` | — | `Error`, `LimitKind`, `Limits`, `clear_caches()` (no-op, documented) |
| 3 | `path.rs` | geometry.ts types | `Pt`, `PathSeg`, `Subpaths` |
| 4 | `color.rs` | color.ts | |
| 5 | `preset/engine.rs` | preset-engine.ts | builtins, formulas, `Guides`, `eval_preset`, `preset_guides`, `preset_text_rect`, `custom_guides`, `arc_to_seg` |
| 6 | `preset/path.rs` | geometry.ts `presetPath`, `actionGlyph` | hand-drawn shapes |
| 7 | `outline.rs` | outline.ts | |
| 8 | `patterns.rs` | patterns.ts | |
| 9 | `raster.rs` | raster.ts | `Mask`, `Affine`, `Polygons`, flatten, fill, blur, EDT, morph, shift, warp |
| 10 | `effects.rs` | effects.ts | |
| 11 | `light3d.rs` | light3d.ts | |
| 12 | `bevel.rs` | bevel.ts | |
| 13 | `adjust.rs` | adjust.ts | `blurRgb` whole-buffer, a plain translation (revised after the skeptic, A1); the banded form is stage 5 (G9); `blackbody` not ported (G13) |
| 14 | `blip.rs` | docx-layout images.ts (4 functions) | G4 |
| 15 | tests (M1–M5, parity replay), `core/fuzz` targets, `uo-wasm` feature `drawingml-memtest` | | |

### Data model and ownership

Everything is plain owned values; no graphs, no arenas needed, no interior mutability, no global
state. Memory scopes: **per operation** only (every function allocates its outputs and temporaries
and returns; the crate holds nothing between calls). Static tables are `&'static` data.

- `Pt { x: f64, y: f64 }`.
- `enum PathSeg { M{x,y}, L{x,y}, C{x1,y1,x2,y2,x,y}, Q{x1,y1,x,y}, E{cx,cy,rx,ry,a0,a1,ccw: bool}, Z }`
  (TS `ccw?: boolean`: absent ≡ `false` for every consumer and for flatten).
- `Subpaths { segs: Vec<PathSeg>, ends: Vec<u32> }` — all subpaths in one allocation;
  `iter() -> impl Iterator<Item = &[PathSeg]>`, `len()`, `get(i)`, builder `push(seg)` /
  `end_sub()`. Replaces `PathSeg[][]`. **Empty subpaths are kept** (revised after the skeptic,
  A7): `evalPreset` pushes a path's `segs` even when it has no drawable command, with `fill`/
  `stroke` parallel, and `presetPath` tests `g.subs.length`; so `end_sub()` always records an end,
  `len()` counts empty subpaths and `iter()`/`get(i)` yield `&[]` for them.
- `PresetGeometry { subs: Subpaths, fill: Vec<Option<f64>>, stroke: Vec<bool> }` (`fill`: `None` =
  `fill="none"`, else the shade offset; parallel to `subs`).
- `enum PresetPath { Poly(Vec<Pt>), Line{x1,y1,x2,y2}, Pie{start_deg,end_deg}, Rect, Ellipse,
  RoundRect{r}, Path(PathShape) }`, `PathShape { even_odd: bool, subs: Subpaths,
  sub_lum: Option<Vec<Option<f64>>>, sub_stroke: Option<Vec<bool>> }` — the presence of `sub_lum`
  is kept: Slides fills per subpath when present and the whole path with the rule when absent.
- `Outline { subs: Subpaths, fill: Vec<bool>, stroke: Vec<bool> }`; `TextRect { l, t, r, b }`.
- `Guides<'a>` — `HashMap<&'a str, f64>` with the TS `Map` semantics (later `set` overwrites, a
  guide may shadow a builtin, lookups only — no iteration is exposed, so order never matters);
  `get(name) -> Option<f64>`, `val(token) -> f64` (= TS `val`: variable, else
  `uo_jsnum::parse::to_number(token)` if finite, else 0). Preset names are `&'static`; custom-guide names
  borrow the caller's strings.
- Generated preset table: `PresetDef { av: &[Guide], gd: &[Guide], paths: &[PathDef], rect:
  Option<[&str; 4]> }`, `Guide { name: &str, op: Op, args: [&str; 3] }` (missing args are the
  token `"0"`, as the TS default — it goes through `val`, so a custom guide named `"0"` is seen),
  `enum Op { Val, MulDiv, AddSub, AddDiv, IfElse, Abs, At2, Cat2, Sat2, Cos, Sin, Tan, Max, Min,
  Mod, Pin, Sqrt, Unknown }` (`Op::parse(&str)` for custom guides), `PathDef { w: Option<f64>,
  h: Option<f64>, fill: Option<f64>, stroke: bool, cmds: &[Cmd] }`, `Cmd { op: u8, args: &[&str] }`
  (a missing path argument is `val(undefined)` = 0, **not** the token `"0"`). Lookup by binary
  search over a sorted index; `preset_names()` in file order.
- `Mask { w: u32, h: u32, a: Vec<f32> }` — f32 storage, f64 arithmetic (see traps).
- `Affine(pub [f64; 6])` (`a b c d e f`), `Affine::IDENTITY`, `mul(&self, n)` (= `mulAffine(m, n)`,
  n first), `invert()`, `apply(x, y)`.
- `Polygons { coords: Vec<f64>, ends: Vec<u32> }` (pairs; one allocation; replaces `number[][]`).
- `LightTable(&'static [u8])`.
- RGBA buffers are borrowed `&mut [u8]` from the caller; nothing keeps them.

### Public API (Rust; fallible where TS could throw RangeError or allocate unboundedly)

```rust
// path / presets
pub fn has_preset_def(name: Option<&str>) -> bool;
pub fn preset_names() -> impl Iterator<Item = &'static str>;           // file (= JS key) order
pub fn eval_preset(name: &str, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<PresetGeometry>;
pub fn preset_guides(name: &str, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<Guides<'static>>;
pub fn preset_text_rect(name: Option<&str>, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> Option<TextRect>;
pub struct GuideDef<'a> { pub name: &'a str, pub fmla: &'a [&'a str] }  // fmla = [op, args…], tokenized by the caller
pub fn custom_guides<'a>(av: &[GuideDef<'a>], gd: &[GuideDef<'a>], w: f64, h: f64, limits: &Limits) -> Guides<'a>;
pub fn arc_to_seg(cx: f64, cy: f64, rx: f64, ry: f64, st_ang: f64, sw_ang: f64) -> Option<(PathSeg, f64, f64)>;
pub fn preset_path(geom: Option<&str>, w: f64, h: f64, adj: Option<&[f64]>, adj_names: Option<&[&str]>) -> PresetPath;
pub fn preset_outline(prst: Option<&str>, w: f64, h: f64, adj: Option<&[(&str, f64)]>) -> Outline; // caller passes JS object key order
// colour
pub type Rgb = [f64; 3];
pub fn srgb_to_lin(v: f64) -> f64;  pub fn lin_to_srgb(v: f64) -> f64;
pub enum ColorModKind { Gamma, InvGamma, Comp, Inv, Gray, Shade, Tint, LumMod, LumOff, Lum, SatMod, SatOff, Sat, HueMod, HueOff, Hue }
impl ColorModKind { pub fn from_name(name: &str) -> Option<Self> }   // unknown names have no effect in TS → None
pub fn apply_color_mods(c: Rgb, mods: &[(ColorModKind, Option<f64>)]) -> Rgb; // raw value in 1/100000
// patterns
pub fn preset_patterns() -> &'static [(&'static str, [u8; 8])];      // JS key order (UI list)
pub fn pattern_rows(prst: &str) -> Option<[u8; 8]>;
pub fn pattern_density(prst: &str) -> f64;
// raster
impl Mask { pub fn new(w: u32, h: u32, limits: &Limits) -> Result<Mask, Error>; pub fn from_vec(w: u32, h: u32, a: Vec<f32>) -> Result<Mask, Error>; /* w(), h(), data(), data_mut(), into_vec() */ }
pub const DEFAULT_TOL: f64 = 0.35;
pub fn flatten_path(subs: &Subpaths, m: &Affine, tol: f64, limits: &Limits) -> Result<Polygons, Error>;
pub fn fill_polygons(mask: &mut Mask, polys: &Polygons, even_odd: bool);
pub fn gaussian_blur(m: &Mask, sigma: f64, limits: &Limits) -> Result<Mask, Error>;
pub fn distance_to(m: &Mask, inside: bool) -> Vec<f32>;
pub fn morph(m: &Mask, r: f64, limits: &Limits) -> Result<Mask, Error>;
pub fn shift_mask(m: &Mask, dx: f64, dy: f64, size: Option<(u32, u32)>, limits: &Limits) -> Result<Mask, Error>;
pub fn warp_mask(m: &Mask, inv: &Affine, w: u32, h: u32, limits: &Limits) -> Result<Mask, Error>;
// effects
pub enum Align { Tl, T, Tr, L, Ctr, R, Bl, B, Br, Other }   // Align::from_attr(None) = B; unknown string = Other (centre)
pub struct ShadowSpec { pub blur_pt: f64, pub dist_pt: f64, pub dir_deg: f64, pub sx: Option<f64>, pub sy: Option<f64>, pub kx_deg: Option<f64>, pub ky_deg: Option<f64>, pub algn: Align }
pub struct BoxF { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }
pub fn align_point(algn: Align, x: f64, y: f64, w: f64, h: f64) -> (f64, f64);
pub fn shadow_affine(s: &ShadowSpec, b: &BoxF, ppt: f64) -> Affine;
pub fn outer_shadow_mask(sil: &Mask, b: &BoxF, s: &ShadowSpec, ppt: f64, limits: &Limits) -> Result<Mask, Error>;
pub fn inner_shadow_mask(sil: &Mask, blur_pt: f64, dist_pt: f64, dir_deg: f64, ppt: f64, limits: &Limits) -> Result<Mask, Error>;
pub fn glow_mask(sil: &Mask, rad_pt: f64, ppt: f64, limits: &Limits) -> Result<Mask, Error>;
pub fn soft_edge_mask(sil: &Mask, rad_pt: f64, ppt: f64, limits: &Limits) -> Result<Mask, Error>;
pub struct Reflection { pub st_a: f64, pub st_pos: f64, pub end_a: f64, pub end_pos: f64 }
pub fn reflection_alpha(t: f64, r: &Reflection) -> f64;
// 3D light and bevel
pub fn light_table(rig: Option<&str>, mat: Option<&str>) -> LightTable;  // always found (threePt fallback)
pub fn light_at(t: &LightTable, dir: Option<&str>, theta_deg: f64, phi_deg: f64) -> [f32; 4];
pub fn light_face(t: &LightTable) -> [f32; 4];
pub fn bevel_phi(prst: Option<&str>, t: f64, hw: f64) -> f64;
pub struct BevelSpec<'a> { pub w: f64, pub h: f64, pub prst: Option<&'a str>, pub rig: Option<&'a str>, pub dir: Option<&'a str>, pub material: Option<&'a str>, pub word: bool }
pub fn shade_bevel(rgba: &mut [u8], sil: &Mask, b: &BevelSpec, limits: &Limits) -> Result<(), Error>; // rgba.len() == 4·w·h
// image adjustments (a14:imgEffect)
pub fn adjust_saturation(rgba: &mut [u8], s: f64);
pub fn adjust_temperature(rgba: &mut [u8], kelvin: f64);
pub fn adjust_brightness_contrast(rgba: &mut [u8], bright: f64, contrast: f64);
pub fn adjust_sharpen(rgba: &mut [u8], w: u32, h: u32, amount: f64, limits: &Limits) -> Result<(), Error>;
// `blackbody` is not ported: no caller (revised after the skeptic, A10/G13)
// blip colour effects (Word model, G4)
pub struct ImageEffects { pub gray: bool, pub gain: Option<f64>, pub bright: Option<f64>, pub floor: bool, pub bi_level: Option<f64> }
pub fn effect_line(fx: &ImageEffects) -> Option<(f64, f64)>;          // (slope, off)
pub fn apply_image_effects(rgba: &mut [u8], fx: &ImageEffects);
pub fn effect_decode(fx: &ImageEffects, components: u32) -> Option<Vec<f64>>;
pub struct ClrChange<'a> { pub from: &'a str, pub to: &'a str, pub to_alpha: f64 }
pub struct PictureAdjust<'a> { pub alpha: Option<f64>, pub duotone: Option<(&'a str, &'a str)>, pub clr_change: Option<ClrChange<'a>> }
pub fn apply_picture_adjust(rgba: &mut [u8], o: &PictureAdjust);
// limits and errors
pub struct Limits { pub max_mask_pixels: u64, pub max_kernel_radius: u32, pub max_path_points: u64, pub max_fill_work: u64, pub max_custom_guides: u32, pub max_image_pixels: u64 }
pub enum LimitKind { MaskPixels, KernelRadius, PathPoints, FillWork, ImagePixels }
pub enum Error { Limit(LimitKind), BufferSize, InvalidArgument }
pub fn clear_caches();   // no caches; exists for the M1/M2 harness contract
```

`fill_polygons` returns `Result<(), Error>` because of `max_fill_work` (revised after the skeptic,
A14/G14).

**Lib re-exports** (revised after the skeptic, A15): `lib.rs` declares the modules privately and
re-exports exactly the items of the block above, by name (`pub use raster::{Mask, Affine,
Polygons, DEFAULT_TOL, flatten_path, fill_polygons, gaussian_blur, distance_to, morph, shift_mask,
warp_mask};` and so on per module); no glob re-exports.

#### Error variant per operation and condition (revised after the skeptic, A3)

Order of checks = the TS order: the no-op/early-return conditions of the TS come first, then the
buffer check, then the limits; every check runs **before the first allocation** of the call
(`flatten_path` is the exception: it checks before each growth and frees what it built — M1 covers
that path).

| Function | Condition | Result | TS behaviour |
|---|---|---|---|
| `Mask::new` | `w·h > max_mask_pixels` (u64) | `Limit(MaskPixels)` | allocates (RangeError above the engine maximum) |
| `Mask::from_vec` | `a.len() != w·h` | `BufferSize` | n/a (Rust-only constructor) |
| `flatten_path` | points produced `> max_path_points` | `Limit(PathPoints)` | grows without bound |
| `fill_polygons` | no edges | `Ok`, mask unchanged | same |
| `fill_polygons` | `rows × edges > max_fill_work` (rows = `r1 − r0 + 1` from the edge y-range, computed before the row buffer) | `Limit(FillWork)` | runs (effectively hangs) |
| `gaussian_blur` | `sigma < 0.05` | `Ok`, copy | same |
| `gaussian_blur` | NaN σ | `Ok`, all-zero mask of the same size | same (kernel length 0) |
| `gaussian_blur` | `max(1, ceil(3σ)) > max_kernel_radius`, incl. `+∞` | `Limit(KernelRadius)` | `+∞`: RangeError (raster.ts:137); finite huge: OOM or hang |
| `gaussian_blur` | output size over the limit | cannot happen (same size as an existing mask) | — |
| `distance_to` | — | infallible (`w·h == 0` → empty) | same |
| `morph` | `abs(r) < 0.01` | `Ok`, copy | same |
| `morph` | NaN r and `w·h == 0` | `Ok`, empty mask `w×h` | same (no write reaches `pad.a.set`) |
| `morph` | NaN r and `w·h > 0` | `InvalidArgument` | RangeError at `pad.a.set` (raster.ts:212) |
| `morph` | `±∞` r | `Limit(MaskPixels)` | RangeError at the padded allocation |
| `morph` | padded `(w+2p)·(h+2p) > max_mask_pixels` | `Limit(MaskPixels)` | allocates |
| `shift_mask`, `warp_mask` | target `w·h` over the limit | `Limit(MaskPixels)` | allocates |
| effect masks | the errors of the calls they compose (warp/shift/morph, then blur) | same variant | — |
| `shade_bevel` | `!(b.w > 0.5)` (incl. NaN) | `Ok`, no-op | same |
| `shade_bevel` | `rgba.len() != 4·w·h` | `BufferSize` | shorter: NaN → 0 stores, writes past the end ignored; longer: tail untouched |
| `shade_bevel` | padded `(w+2)·(h+2)` over the limit | `Limit(MaskPixels)` | allocates |
| `adjust_sharpen` | `!amount` (0, −0, NaN) or `w < 2` or `h < 2` | `Ok`, no-op | same (before touching `d`) |
| `adjust_sharpen` | `rgba.len() != 4·w·h` | `BufferSize` | A4a: longer → trailing RGB zeroed (soften), shorter → NaN → 0 |
| `adjust_sharpen` | `w·h > max_image_pixels` | `Limit(ImagePixels)` | allocates 24–28 B/px |
| pointwise adjusts, blip, presets, colour, light | — | infallible; a partial trailing pixel is left untouched | partial pixel: R zeroed or untouched (G7) |
| `custom_guides` | more than `max_custom_guides` per list | truncated (Slides' `slice(0, 2000)`), infallible | the Slides caller truncates |

Changes from 0.9 (for `CHANGELOG.md`): `PathSeg[][]`/`number[][]` become `Subpaths`/`Polygons`; out
params of `lightAt`/`lightFace` become return values; allocating functions take `&Limits` and
return `Result`; `presetOutline` takes an ordered slice instead of a `Record`; the .NET public
namespace `UniversalOffice.DrawingML` disappears (G10). The npm package was private: no npm break.

**Crossing to JS/.NET** (designed now, built with the `uo-wasm`/`uo-ffi` switch plans): the layout
path uses this crate internally (results reach the painter through `uo-render`). The editor shell
needs only stateless calls on byte buffers (adjust previews, blip effects), the pattern table and
`preset_outline` for gallery icons (and later Slides' `eval_preset` for its gallery, A10);
buffers are moved through `uo_alloc_bytes`, pointwise adjusts and blip effects run on JS-sized
chunks (any multiple of 4 bytes gives identical results), so for them wasm memory does not grow by
the image size. Sharpen/soften need neighbouring rows (σ ≤ 2.35, so r ≤ 8): with the whole-buffer
port of stage 3 the whole image would sit in wasm memory (4N, 48 MB at 12 Mpx), so the switch plan
adds a banded call with an r-row halo, built on the stage-5 banding (revised after the skeptic,
A11/G9/G10). No handles cross in this module.

### Crate vs own port (reuse specialist, measured where noted)

| Candidate | Decision | Why |
|---|---|---|
| `kurbo` (flatten) | own port | adaptive flattening ≠ uniform `n = ceil(len/tol/4)` capped 256/512 |
| `tiny-skia`, `lyon` (fill) | own port | ours: f32 coverage, 4 sub-scanlines, exact fractional spans, `min(1, a + row)` |
| `image`, `imageproc`, `fast_image_resize` (blur, EDT) | own port | two blurs with different edges (zero padding, non-renormalized kernel; vs clamped edges), kernel stored f32 normalized by an f64 sum, EDT with INF 1e20 and padding |
| `palette` (HSL) | own port | different operation order, platform `powf` |
| `base64` | not needed | tables generated at build time |
| `libm` 0.2.16 / Rust std math | **never directly** | 400k-sample probe vs Node 24 (V8 13.6): libm differs on sin (3,623), cos (3,633), log (732), pow (25k–39k), hypot (143k); std on Windows differs everywhere and differs from wasm — all through `uo-jsnum` (G1, G2) |
| `std::collections::HashMap` | use | `Guides`; no iteration exposed |

## JS semantics: rules and traps (for the stage-3 port and the verifiers)

### Global rules

- **R1** No std float functions except `+ - * / %`, `sqrt`, `abs`, `floor`, `ceil`, `trunc`,
  `is_nan`, `is_finite`, `to_bits`: everything else goes through `uo_jsnum::math` (`sin cos tan
  atan atan2 exp log pow hypot round sign min max`, `max_n`/`min_n` for the 3-argument calls).
  Enforced by the workspace `disallowed-methods` list of the uo-jsnum plan (`core/clippy.toml`,
  section "Clippy enforcement" of `core/docs/modules/uo-jsnum.md`), which already bans the
  transcendentals, `sin_cos`, `exp2`, `exp_m1`, `ln_1p`, `log2`, `log10`, `cbrt`, `asin`/`acos`,
  `sinh`/`cosh`/`tanh`, `powf`/`powi`, `mul_add`, `round`/`round_ties_even`, `max`/`min`/`clamp`,
  `signum`, `rem_euclid`/`div_euclid`. **Requested additions** (revised after the skeptic, A5; to
  be settled at the uo-jsnum gate, not edited here): `f64::to_radians`/`to_degrees` (they multiply
  by a constant `π/180`, while the TS writes `(deg * Math.PI) / 180`), and a review/grep rule
  against `std::f64::consts` other than `PI` (`FRAC_PI_2`, `FRAC_1_SQRT_2`… are not what
  `Math.PI / 2` computes at run time). **Casts**: `#![deny(clippy::cast_possible_truncation,
  clippy::cast_sign_loss)]` in `uo-drawingml` (the uo-jsnum plan's G5 suggests trying it here
  first): every f64 → u8/u32/usize goes through `uo_jsnum::conv::{to_uint8, to_uint8_clamp}` or
  `js_index` (R6); `x as u8` truncates and saturates, which is neither `ToUint8` nor
  `ToUint8Clamp`. Allowed exceptions carry `#[expect(…, reason = "…")]`.
- **R2** f64 arithmetic always; f32 is storage only. Every `Float32Array` write is `as f32` (=
  `Math.fround`); every read is `f64::from(x)`. `x[i] += v` on a Float32Array is
  `x[i] = (f64::from(x[i]) + v) as f32`. Never compute in f32. No FMA (Rust never contracts).
- **R3** Keep the TS expression order and grouping: `(a * b) / c` ≠ `a * (b / c)`; left
  associativity (`u*u*u*cx + 3*u*u*t*x1 + …`); constants the TS computes at run time stay
  computed the same way (`Math.PI * 2`, `(i * 2 * Math.PI) / n`, `(deg * Math.PI) / 180`,
  `-s.distPt * ppt * Math.cos(a)` = `((-d) * ppt) * cos`); `const` evaluation in Rust is IEEE
  identical.
- **R4** Truthiness: `x || 1` replaces `0`, `-0` and `NaN`; `!x` is true for `0/-0/NaN/''/
  undefined`; `x ?? d` only replaces `undefined`/`null` (`NaN` and `''` pass); `a && b` on strings:
  `''` is falsy (`b.rig && …`, `rig && TABLES[…]`, `geom && …`); empty arrays are truthy
  (`adj && adjNames` with `[]`) — `Some(&[])` must not collapse to `None`; `adj?.length` is falsy
  for `[]`.
- **R5** NaN and signed zero: `Math.max/min` propagate NaN and order `-0 < +0` (`uo_jsnum::math::{max, min}`;
  `f64::max(0, NaN)` returns 0 — wrong, e.g. `Math.sqrt(Math.max(0, x()))`); every comparison
  with NaN is false (branches like `v < lo ? lo : v > hi ? hi : v` return NaN); `Math.round`
  rounds ties up and returns `-0` for `[-0.5, 0)` (`uo_jsnum::math::round`); `Math.sign` keeps `±0`/NaN.
  Outputs are compared with the sign of zero.
- **R6** Typed arrays: an out-of-range or NaN index reads `undefined` → NaN in arithmetic
  (`lut[NaN]`, `tab[NaN]`, `t[(NaN·11+p)·4+c]`); out-of-range writes are ignored; `Uint8Array`
  stores `ToUint8` (truncate, modulo 256; NaN → 0); `Uint8ClampedArray` stores `ToUint8Clamp`
  (clamp, **round half to even**; NaN → 0); `new Float32Array(NaN)` has length 0,
  `new Float32Array(Infinity)` throws RangeError (→ `Err(Limit)`). **One helper for every
  f64-derived index** (revised after the skeptic, A6): `js_index(i: f64, len: usize) ->
  Option<usize>` — `None` for NaN, fractional, negative or `≥ len`; a read with `None` gives NaN, a
  write with `None` is a no-op. Requested from `uo-jsnum` (`uo_jsnum::conv::js_index`; if the
  uo-jsnum gate does not take it, a private `pub(crate)` helper in `uo-drawingml` with the same
  contract). Sites: `LIN_SRGB[round(v·4095)]` (bevel.ts:101), `t[…]` in `lightAt` (light3d.ts:186)
  and `tab[…]` in `bevelPhi` (light3d.ts:225), `row[ia] +=` (raster.ts:121 — the write is
  ignored and the `ib` branch still runs), `m.a[…]` in `at()` of shift/warp (raster.ts:235/250),
  the word-bevel `at()`, the adjust LUT lookups.
- **R7** Loops whose bound can be NaN, infinite or fractional keep an f64 counter
  (`let mut i = 1.0; while i <= n { …; i += 1.0 }`): NaN bounds run zero iterations.
- **R8** Lookups: own properties only (G7); TS `Map` overwrite semantics; JS object key order for
  name lists (generator-checked: no integer-like keys).
- **R9** Strings: `Number(t)` = `uo_jsnum::parse::to_number` (accepts `0x1A`, `0b`, `0o`, `Infinity`, JS
  whitespace, `''` → 0; rejects `inf`, `nan`, `1_0`); `parseInt(s, 16)` = `uo_jsnum::parse::parse_int`
  (stops at junk, accepts sign and leading spaces); `s.slice(i, j)` is in UTF-16 units — the only
  use here is the blip hex `parseInt(s.slice(i, i + 2), 16)`, done with a private helper that takes
  UTF-16 code units `[i, i+2)` of `s.encode_utf16()` (never slice `str` at byte offsets of
  non-ASCII text). `split(/\s+/)` tokenization happens in the Slides reader, not here (A13); when
  needed it is `uo_jsnum::text::split_js_space`.
- **R10** No panics on input: sizes in `u64` with checked multiplication against `Limits` before
  any allocation; no indexing that hostile values can push out of range; no `unwrap`; sorting
  only with a total order.

### Per file

| File | Traps |
|---|---|
| preset-engine.ts | `val`: variable first, then `Number(t)` if finite else 0; missing formula args are the token `'0'` (through `val`), missing path args are 0; `z() \|\| 1` (NaN/±0 → 1); `?:` is `x > 0` (NaN → z); `pin` with NaN returns NaN; `mod` = `sqrt((x*x + y*y) + z*z)`; `at2` = `atan2(y, x) / DEG`, `cat2`/`sat2` take `atan2(z, y)`; `DEG = Math.PI / (180 * 60000)`; `adj && adjNames` → by name (`indexOf` = first), else positional `i < adj.length`; `v ?? evalFormula(…)` keeps NaN adj; path `w`/`h`: `p.w ? w / p.w : 1` (0/NaN → 1); arc: `rx <= 0 \|\| ry <= 0 \|\| sw === 0` skips (NaN passes!), `Math.abs(sw) >= 2π - 1e-9`, the `while (p1 <= p0) p1 += 2π` loops (bounded: `atan2` ∈ [−π, π]); `presetTextRect` returns null unless `r > l && b > t` (NaN → null) |
| geometry.ts | `geom && (adj?.length && ADJUSTED_OFFICIAL.has(geom) \|\| geom === 'arc')`; `adj?.[0] ?? d` keeps NaN; `fitToBox`: `maxX - minX \|\| 1`, NaN never assigned to min/max; `subStroke` only when some stroke is false; `subLum` presence matters; action buttons: `lums` 0 for the Document fold; unknown/`''`/undefined → `Rect` |
| outline.ts | `!prst \|\| prst === 'rect'` → rectangle; `Object.keys` order is the caller's duty (integer-like keys first) |
| color.ts | `raw === undefined \|\| !Number.isFinite(raw)` skips valued mods only (gamma/invGamma/comp/inv/gray always apply); shade/tint per channel via the sRGB curve (`pow` 2.4 and 1/2.4); `clamp01` = `max(0, min(1, v))` (`-0` → `+0`); HSL: exact `===` channel tests, `%` is fmod, `((h2 % 1) + 1) % 1`, `seg = floor(h2*6) % 6` compared as **f64** to 0…4 (NaN → the last branch — do not cast to int); `hueOff`/`hue` use `raw / 21600000` |
| raster.ts | flatten: `scale = max(1e-9, hypot(m0,m1), hypot(m2,m3))` (3-arg NaN-propagating max, V8 hypot); `n = max(2, min(256, ceil(len/tol/4)))` (NaN → 0 points: `tol = 0` with a zero-length segment gives `0/0` → NaN → no points, a non-zero length gives `+∞` → 256; `tol = -0` gives `−∞` or NaN → 2 or no points — revised after the skeptic, A8); `M` pushes the pending polygon only if it has ≥ 6 numbers, else discards; `Z` moves the pen without a point; ellipse: `ccw` sweep normalization, `i = 0..=n`, `t = a0 + ((a1 - a0) * i) / n`. fill: skip edges with `y0 === y1` (NaN edges kept); `ymin`/`ymax` NaN → no rows at all (replicable); `y = py + (s + 0.5) / 4`; intersections sorted by x with a stable sort — Rust uses `sort_by(|p, q| p.x.total_cmp(&q.x))` (revised after the skeptic, A8: `total_cmp` orders `−0 < +0` and NaN last, never panics; the JS comparator treats ±0 as equal, which is harmless because equal-x spans are zero width and the positive spans, their order and the f32 row sums are unchanged); any **NaN** x is a named divergence (G7) with the mechanical rule of the parity plan; `xa = max(0, x)`, `xb = min(w, x')`, `xb <= xa` skip; f32 row accumulation (R2); `a = min(1, a + row)`. blur: `sigma < 0.05` copies; NaN sigma → all-zero mask of the same size; `r = max(1, ceil(3σ))`; `sum += v` uses the f64 value, `k[i] /= sum` stores f32; zero padding via `lo/hi`, no renormalization. EDT: `INF = 1e20`; `w·h == 0` → empty result (TS writes into zero-length arrays harmlessly); `k` never drops below 0 (proof: `z[0] = −1e20` and `s > −1e20` for f ∈ {0, ≤ 1e20 + n²}) — use a guarded loop, results identical. morph: `abs(r) < 0.01` copies; `p = ceil(abs(r)) + 2`; NaN/∞ r throws RangeError in TS → `Err`; `r - dd + 0.5` and `dd + r + 0.5` order. shift/warp: `at(xx, yy)` returns 0 outside, **NaN when xx or yy is NaN** (TS reads `a[NaN]`); bilinear order as written. `invertAffine`: `det \|\| 1e-12` |
| effects.ts | `algn ?? 'b'`; unknown align → 0.5/0.5; `tan((kx * π) / 180)`; composition `back ∘ S ∘ K ∘ toA`; inner shadow shifts by `-dist·ppt·cos(a)`; `soft_edge` `min(e, sil)` in f64 then f32; `reflectionAlpha` branch order |
| light3d.ts | `rig && TABLES['rig\|']` (`''` → threePt); `mat && mat !== 'warmMatte' && …`; `DIR[dir ?? 't'] ?? 0` — `''` gives 0, Object.prototype names give NaN in TS (G7: unknown = 0); `th = ((θ + off) % 360 + 360) % 360`, `t0 = floor(ti) % 24`; `PH` search; `fp` clamp; out stored f32 (`q * 0.02`, `q / 255`); `bevelPhi`: `u = max(1e-3, min(1, t))`, profile index NaN → NaN, `angle` → tan 1, circle `s / sqrt(max(1e-6, 1 - s*s))`, result `(atan(tan·hw)·180)/π` |
| bevel.ts | `wordTable` conditions use truthiness (`''` passes as "not set"); `prst ?? 'circle'`; face column `o = 24·25·2 − 2`; `!(b.w > 0.5)` returns (NaN); padded `(W+2)×(H+2)` EDT, `dist` f32, `gaussianBlur(dist, 1.2)`; `a <= 0` skips (NaN does not skip); Sobel order as written; `atan2(-gy, -gx)` with signed zeros (`-0` → −180°); `round(v·4095)` NaN index → `LIN_SRGB[NaN]` undefined → store 0; blend `round(c + (nv − c)·a)` |
| adjust.ts | `abs(s − 1) < 1e-6` early out; its own `rgbToHsl`/`hslToRgb` (not color.ts's — different operations); `ns = s <= 0 ? 0 : s < 1 ? sat·s : min(1, pow(sat, 1/s))` (NaN s → black); `clamp8` = `v <= 0 ? 0 : v >= 255 ? 255 : round(v)` (NaN → NaN → stored 0); temperature: `max(1000, kelvin)` NaN-propagating, mired table computed per call, `DG = -(0.2126·DR + 0.0722·DB) / 0.7152`; LUT curves: `knots(null)` = `min(255, i·8)`, sorted `pts` with the 0 entry, `p1 === p0 ? 0 : …`, `j = min(31, x >> 3)`, `x1 = j === 31 ? 255 : x0 + 8`, contrast `v >= 1` exact step; LUTs recomputed per call (no cache, identical); `blurRgb` clamps edges, `sum += k[i] = exp(…)` sums the f64 value, ported **whole-buffer** in stage 3 with its three f32 store points: `k` (normalized kernel), `tmp` (horizontal pass) and `out` (vertical pass) — soften then reads `clamp8(out[j])`, sharpen reads only `yb[i*3]` (revised after the skeptic, A1); sharpen: `!amount` (0/NaN) or `w < 2 \|\| h < 2` return; luma stored through **Uint8ClampedArray (half-even)**; `dl = abs(x) <= 2.5 ? 0 : k·(x − sign(x)·2.5)`, `if (dl)`; partial trailing pixel (len % 4 ≠ 0) left untouched (G7) |
| images.ts (blip) | `effectLine`: `gain === undefined && !bright` (NaN bright is falsy) → none; `off = 0.5 − 0.5·s + (b/2)·(1+s)`; map `floor(y + 1e-9)` or `round(y)`, NaN → 0; `thr >= 0` (NaN/negative → no bi-level); hex via `parseInt(s.slice(i, i+2), 16)` (NaN → comparisons false, stores 0); `abs(c − from) <= 2`; `round(alpha·255)`; stores of `applyPictureAdjust` outside 0..255 — alpha/`toAlpha` above 1 **or negative**, and hex with a sign (`parseInt('-f', 16)` = −15 stored as `d[i] = tr`) — wrap in the TS PDF path (`Uint8Array`, pdf.ts:108) and clamp in the canvas path (`Uint8ClampedArray`, canvas-picture.ts:133); Rust clamps every such store (G7, revised after the skeptic, A4b) |
| patterns.ts | `PRESET_PATTERNS[prst]` own keys only (`'constructor'` throws in TS); popcount over the 8 bytes; key order kept |

## Parity plan

- **Capture platform**: Linux x64, pinned Node 24 LTS, frozen `main` dist; every capture file
  carries `@platform` (OS, glibc, Node, V8, and the hwcaps mask of G2). Windows captures are not
  references for facets that use `pow`.
- **Comparison**: `uo-testkit` hashed streams, exact bits, `-0` included; **NaN canonicalized** on
  both sides (V8 and wasm NaN payloads differ); f32 buffers hashed as LE bytes (NaN canonicalized),
  RGBA as bytes. Accepted differences are only the named ones of G7, never tolerances.
- **Generated corpus (committed, always replayed in `cargo test`)** — inputs reproducible on both
  sides from the capture line (parameters + `Rng` seeds of `uo-testkit`):
  1. presets: every name of `PRESET_DEFS`, every `presetPath` case and `actionButton*`, unknown
     names, `''`, `undefined`, and Object.prototype names (G7 set, excluded from equality, checked
     as "unknown" in Rust) × sizes {(200,100), (100,200), (100,100), (0,0), (1, 1e6), (21600,21600),
     (13.37, 0.5)} × adj sets {none, `[]`, defaults, each av perturbed by 0, −1, 1e5, 1e9, −1e9,
     NaN, ±∞, 1e308} × names {named, positional, partial, reordered, duplicated} → `presetPath`,
     `evalPreset`, `hasPresetDef`, `presetGuides` (values of every builtin/av/gd name),
     `presetTextRect`, `presetOutline`;
  2. custom guides: 5,000 generated programs (all ops, unknown ops, forward refs, shadowed
     builtins, names `''`/`'0'`, tokens `0x1A`, `1e400`, ` 12`, `Infinity`, `+.5`, `5.`, `0b11`,
     `1_0`, U+00A0/U+FEFF-padded, Arabic digits) + `arcToSeg` grids incl. NaN/∞;
  3. colour: 20,000 random colours × mod sequences (all kinds, raw undefined/NaN/±∞/huge/negative/
     fractional, gamma brackets) + `srgbToLin`/`linToSrgb` grids;
  4. raster: outlines of all presets at 1–400 px under random affines (incl. det 0, NaN skew) and
     `tol` ∈ {0.35, 0, 1e-3, NaN}; fill even/odd; blur σ ∈ {0, 0.04, 0.05, 0.3, 1, 2.5, 10, 33.3,
     NaN}; EDT inside/outside; morph r ∈ {±0.005, ±0.5, ±3.7, ±20}; shift/warp fractional,
     negative, out of range, NaN; affine mul/invert;
  5. effects: every `algn`, scale/skew incl. 0, negative, 89.9°; the five masks; reflection grid;
  6. light/bevel: `lightTable` for all keys and unknowns (byte digest), `lightAt` θ×φ×dir grid,
     `lightFace`, `bevelPhi` (prst × t × hw incl. NaN), `shadeBevel` on generated silhouettes ×
     specs (word on/off, every Word prst, every rig/material, every dir, w ∈ {0.4, 0.6, 3, 12, NaN});
  7. adjust: generated RGBA (gradients, noise, flat, edges) 1×1…512×384 × the panel presets and
     random values (incl. NaN, ±∞, out of range);
  8. blip: the four functions over generated images, effects, hex strings (valid, short, junk,
     non-ASCII), alpha in and out of [0, 1];
  9. patterns: every name + unknowns.
- **Real-document capture (manual, documented; replay with `UO_TS_PARITY=1`)**: an esbuild alias
  replaces `@universal-office/drawingml` (and, for the blip functions, an esbuild `onResolve` plugin
  keyed on the importer redirects the relative `./images.js` imports of `canvas-picture.ts:14`,
  `pdf.ts:11` and `picture.ts:8` — not a package alias) with a recording shim during the Write corpus run (`write/tools/corpus-lab/run-ours.mjs`, canvas
  and PDF paths) and the Slides runs (`slides/tools/ppt-lab/render-deck.mjs`), over the 2,742-sample
  oracle corpus of uo-opc and the slides samples. Small inputs are written in the capture file;
  buffer inputs (masks, RGBA) go to a content-addressed local blob store (not committed; size
  reported); outputs are digests. Chained calls replay from the Rust outputs when the input digest
  was produced earlier in the same document, so a divergence is reported at its first call.
  Requirements (revised after the skeptic, A12): (a) `run-ours.mjs` renders in Playwright
  Chromium/Electron, not in Node — `pow` facets (shade/tint, present in almost every themed shape
  outline) count as references only if that browser runs on Linux with the platform settings of
  the pow decision (G2) passed in its launch environment; otherwise those facets are excluded and
  counted; (b) the importer-keyed plugin above for the blip functions; (c) in-place functions
  (`fillPolygons`, `shadeBevel`, `adjust*`, blip) are recorded from a snapshot taken **before**
  the call, and the typed-array kind (`Uint8Array` / `Uint8ClampedArray`) is part of the input
  (A4b); (d) buffers reach 24 Mpx × 4 B per call: the blob store is capped (digest-only above a
  size set in the capture script, the count of digest-only calls reported).
- **Mechanical rule for the fill NaN case** (revised after the skeptic, A4e): a `fill_polygons`
  call (and anything chained from its output) whose polygons contain a non-finite coordinate is
  excluded from equality and counted in the report; no judgement by the verifier.
- **Process**: the parity verifier records each round sent back to the port in "Process metrics".

## Memory plan (measured gate)

- **M1**: `assert_no_leak` on every public function and on every row of the error table
  (revised after the skeptic, A3): each `Err` row is a test that also asserts that nothing was
  allocated before the check (allocation count 0), except `flatten_path`, whose partial output
  must be released.
- **M2 soak** (tolerance **0 B**: the crate has no caches): per iteration — all 187 presets through
  `eval_preset`/`preset_path`/`preset_guides`/`preset_text_rect`/`preset_outline` at 3 sizes,
  defaults and one hostile adj set; 1,000 colour sequences; 200 custom-guide programs; flatten +
  fill of 20 outlines at 256 px; outer/inner shadow, glow and soft edge on a 512×512 silhouette;
  `shade_bevel` 256×256 (Word and rig); the four adjusts on 1024×768 RGBA (sharpen ±); blip
  effects; patterns. Native: 100 iterations in every `cargo test`, 1,000 in the release CI job,
  sampled every 50. Wasm (`uo-wasm` feature `drawingml-memtest`, never shipped): 200 iterations,
  `memory.buffer.byteLength` flat after the first.
- **M3 budgets** (N = W·H pixels; recorded at 1 Mpx and 4 Mpx with TS heap + ArrayBuffer references
  from the same scenario in Node):

  | Operation | Allocated (faithful) | Peak live temporaries |
  |---|---|---|
  | `Mask::new` | 4N | — |
  | `fill_polygons` | (W+1)·4 + edges·40 + intersections·16 | same |
  | `gaussian_blur` | 8N + kernel | 4N (tmp) |
  | `distance_to` | 12N + 4·max(W,H)·8 | 8N |
  | `morph` | 4N + 4Np + 12Np (Np = padded) | ≈ 16Np |
  | `outer_shadow_mask` | 12N | 8N |
  | `inner_shadow_mask` | 16N | 12N |
  | `glow_mask` / `soft_edge_mask` | ≈ 16Np + 12N | ≈ 16Np + 4N |
  | `shade_bevel` | ≈ 4Np + 12Np + 4N + 8N | ≈ 24N |
  | `adjust_sharpen` (stage 3, whole-buffer, A1) | 28N (luma RGBA 4N + `tmp` 12N + `out` 12N) | 28N; stage 5 (G9): N + (2r+1)·W·4 |
  | soften (`adjust_sharpen` < 0, stage 3) | 24N | 24N; stage 5 (G9): (2r+1)·W·12 |
  | pointwise adjusts, blip | ≤ 3·256 B LUTs | — |
  | presets | one `HashMap` (≤ 300 entries) + outputs | — |

  Phone check (revised after the skeptic, A2): the padding is not negligible. `morph` pads by
  `ceil(|r|)+2` and the callers' layers already include the effect margin: text layers up to
  2.4e7 px (textfx.ts:144) with a glow margin of 1.2·rad (textfx.ts:84) give Np ≤ ≈ 2.0·N ≈ 48 M px;
  pictures (cap 12e6, canvas-picture.ts:148) give Np ≤ ≈ 2.25·N ≈ 27 M px; shapes/PDF (8e6) ≈ 18 M
  px. Peak inside the crate for a glow is ≈ 8N + 16Np: **≈ 0.53 GB for a 12 Mpx picture glow and
  ≈ 0.96 GB for a 24 Mpx text glow**, plus the caller's f32 RGBA (16N, textfx.ts:154). wasm memory
  never shrinks; against the iOS baseline this is not viable for the switch. Consequences: the
  wasm32 `max_mask_pixels` (32 Mi) is **below** a padded text glow (≈ 48 M px), so on wasm32 such
  a glow returns `Err(Limit(MaskPixels))` where TS paints — a listed wasm32 difference (G8), not
  hidden; the phone budget per effect call is G12 (gate-3 / switch requirement). Stage-5
  candidates, all result-identical: pad-free EDT (the column pass reads the silhouette through the
  virtual frame, the column plane in integer form — values are exact squares or `1e20 + k` rounded,
  re-expanded to the same f64 — and the row pass only for interior rows), the banded
  sharpen/soften (G9), the active edge table in `fill_polygons` (order-independence shown above),
  compiled preset programs.
- **M4**: no handles in this module. When the editor-shell exports exist, the uo-wasm boundary test
  pattern applies to every returned buffer (10,000 alloc/process/free cycles).
- **M5**: no caches; `clear_caches()` is a documented no-op. Tests prove no hidden growth: 10,000
  distinct unknown preset names and 10,000 distinct brightness values leave live bytes unchanged
  (the TS caches grew without bound on both).
- **M6**: `#![forbid(unsafe_code)]` — no Miri job needed; `dhat` for investigation.

## Security plan

- **Untrusted inputs**: preset names and adj names/values (any f64), custom guide lists (names,
  tokens), path segments (count and coordinates), mask dimensions and effect parameters derived
  from document EMUs (blur radius, distance, glow radius, skew → `tan` near 90°), RGBA buffers with
  their dimensions, colour mod sequences, bevel strings, hex strings (non-ASCII possible).
- **Limits** (defaults, G8, revised after the skeptic): `max_mask_pixels` 64 Mi native (above every
  padded caller layer, ≤ ≈ 48 M px, so no parity effect) / 32 Mi wasm32 (**below** a padded text
  glow — a listed wasm32 difference, A2), `max_kernel_radius` 4,096 px, `max_path_points` 4 Mi per
  flatten, `max_fill_work` (rows × edges; A14/G14) with the default set by the security
  specialist from measured times, above what the TS callers produce, `max_custom_guides` 2,000 per
  list (Slides' `slice(0, 2000)`, applied as the same truncation), `max_image_pixels` 256 Mi
  native / 32 Mi wasm32. Every size is computed in u64 and checked before allocating. Without
  `max_fill_work`, `fill_polygons` is O(rows × 4 × edges): ~10¹⁵ on hostile custGeom with 4 Mi
  points and a tall mask (the TS hangs the same way; small fuzz `Limits` do not reach it). The
  text reflection blur is bounded by the layer only on the bottom margin (textfx.ts:91), so a
  hostile `blurRad` reaches `max_kernel_radius` → `Err` where TS builds a huge kernel and
  effectively hangs (G7, A4d).
- **Fuzz targets** (`core/fuzz`, CI 60 s each, `-rss_limit_mb=2048`, small `Limits`):
  `dml_preset` (names, sizes, adj incl. NaN/∞/subnormal, name lists), `dml_custom_guides` (programs
  + `arc_to_seg`), `dml_color_mods`, `dml_raster` (arbitrary subpaths, affine, tol, σ, r, shifts on
  ≤ 256² masks), `dml_effects`, `dml_bevel`, `dml_adjust` (lengths not multiple of 4, w/h
  mismatches, any params) and `dml_blip` (hex strings with non-ASCII). Property: no panic, no
  allocation above the limits, no hang above the CI timeout.

## Stage-3 brief (revised after the skeptic, A15)

- Committed and deterministic: the table generator (`tools/gen-tables.mjs`, which copies
  `parseDef` verbatim — it is not exported) and every corpus generator (JS and Rust sides, seeded
  `uo-testkit` `Rng`); a re-run produces byte-identical files.
- `lib.rs` re-exports are the explicit list of the Public API block (no globs).
- If stage 3 is split between agents, **one** of them owns `lib.rs`, `error.rs` and `limits.rs`;
  the others add only their module files and ask that owner for re-exports or new error variants.
- Port order as in the module table; `cargo clippy --all-targets -D warnings` clean (with the R1
  bans and the cast lints) before stage 4.

## Stage 5 handover: banded sharpen/soften (G9; revised after the skeptic, A1)

Not part of stage 3. Done on **Opus** in stage 5, with parity as the net, under these invariants
(verified by the skeptic in adjust.ts):

1. **The three f32 rounding points stay**: the kernel `k` is stored as f32 after normalization;
   the horizontal sum is stored as f32 (`tmp`); the vertical sum is stored as f32 (`out`/`yb`)
   before it is read (soften: `clamp8(out[j])`, adjust.ts:201; sharpen: `yb[i*3]`, 206/208).
   A streamed version keeps these as explicit `as f32` roundings even though no array remains.
2. **Sharpen: the luma plane is complete before the first write to `d`**, built from the original
   pixels through `ToUint8Clamp` (adjust.ts:204–205 vs 210); only channel 0 of the blurred luma is
   read (`yb[i*3]`, 208), so channels 1–2 may be dropped.
3. **Soften: row order** — each horizontal-pass row is computed before its source row of `d` is
   overwritten; output row y needs horizontal rows up to y + r, so rows are written in increasing
   y with a ring of 2r + 1 horizontal rows; clamped edge rows and images with h < 2r + 1 follow the
   TS clamp `min(h−1, max(0, y+i))`.
4. Per-output operation order (the `i = −r..r` sums in f64) is unchanged.

The same banding is the basis of the banded wasm call of G10 (r ≤ 8, σ ≤ 2.35).

## Sonnet readiness (revised after the skeptic)

- **Sonnet for stage 3 of this crate: yes.** With A1 moved to stage 5, nothing in stage 3 requires
  a design decision: the API, data model, error table (A3), generated tables, file mapping, empty
  subpaths (A7), the sort comparator (`total_cmp`, A8), float-index access (`js_index`, A6), the
  casts (A5) and every trap are fixed, and the verifier's fill-NaN rule is mechanical (A4e). The
  generator is mechanical because it copies `parseDef`.
- Open item that does not block the translation: the pow reference (G2, decided at the uo-jsnum
  gate) changes only which capture platform/flags the parity verifier uses.
- Review focus for the verifiers: R2 store points (incl. `blurRgb`'s three), R5 (NaN/±0), both blur
  edge modes, `shade_bevel`'s Sobel and `atan2` signed zeros, the error-check order.
- **Opus**: the prerequisite `uo-jsnum` (its own plan, record and gate) and the stage-5 banding
  (G9) and pad-free EDT (G12).

## Questions for the maintainer (gate 1) — consolidated (revised after the skeptic)

One list, planner and skeptic together; the last column is the final recommendation.

| # | Question | Final recommendation |
|---|---|---|
| G1 | Create the shared prerequisite crate **`uo-jsnum`** (record `core/docs/modules/uo-jsnum.md`): JS numeric semantics for every engine crate — V8 fdlibm port of sin/cos/tan/atan/atan2/exp/log (confirmed as the right source by A13), V8 `Math.hypot`, pow per G2, JS round/sign/min/max, `ToNumber`, `parseInt`, `ToUint8`/`ToUint8Clamp`, the workspace `disallowed-methods` list; on **Opus**, its own gate; uo-drawingml stage 3 starts after its gate 2. Surface this crate needs (A13): `sin cos tan atan atan2 exp log pow hypot`(2) `round sign min max max_n/min_n to_number parse_int`(radix 16) `to_uint8_clamp`, plus the requests of G15. | **Yes** |
| G2 | **Open — decided at the uo-jsnum gate, not here.** Reference for `Math.pow`, which since V8 11 calls the platform C `pow` after the ES special cases. Options on the table: the uo-jsnum planner recommends capturing with `node --no-use-std-math-pow` and porting V8's legacy (fdlibm) pow — platform-independent; this module's planner and skeptic recommended capturing on Linux with glibc's FMA variants masked and porting ARM optimized-routines `pow` (non-FMA path); the fallback is a correctly rounded `pow` (CORE-MATH) with listed differences. Users here: the sRGB curve of `apply_color_mods` (shade/tint — in almost every themed outline), `S^(1/s)` in saturation, and the bevel LUTs (generated, so only at generation time). Whatever is chosen also fixes how the browser-based real-document capture is launched (A12a). | **Follow the uo-jsnum gate decision.** From this module's side any option that makes the capture platform-independent is fine; `--no-use-std-math-pow` is the simplest for the browser run too, if Chromium/Electron accept the flag (`--js-flags`) |
| G3 | Move the Word-only `contour.ts` (`wrapContour`) and `shape-styles.ts` (`SHAPE_STYLE_PRESETS`) into `uo-docx` (editing API), planned with it. | **Yes** |
| G4 | Bring Write's blip colour effects (`effectLine`, `applyImageEffects`, `effectDecode`, `applyPictureAdjust`) into `uo-drawingml::blip` now as the Word model; Slides' PowerPoint model is added as a named variant with the Slides port. | **Yes** |
| G5 | Image codecs (`decodePng/Gif/Bmp/Tiff`, `jpegInfo`; Slides `export/raster.ts`; C# `Pdf/Raster.cs` ×2) go to a new crate `uo-image` (on `uo-opc::flate`) with its own plan, not into `uo-drawingml` (`uo-pdf` should not depend on DrawingML). | **Yes, new crate `uo-image`** |
| G6 | Register the later shared modules of this crate, each planned with its first consumer: the DrawingML colour resolver (XML + theme + clrMap; call-site quirks as named variants until stage 5) — **first consumer `uo-docx`** (A9); fills (gradients, pattern rasters, tiles) with `uo-render`; the msospt map with the first legacy-format port. | **Yes** |
| G7 | Behaviour fixes that are not parity targets (as uo-opc G5), with the skeptic's additions (A4): Object.prototype names (`constructor`, `__proto__`, `toString`…) in `dir`/`prst`/pattern lookups are unknown keys (TS: NaN or a thrown TypeError); NaN intersections in `fill_polygons` sort with `total_cmp` (TS order unspecified) and such calls are excluded from equality by the mechanical rule of the parity plan; **every store of `applyPictureAdjust` outside 0..255 clamps** (alpha/`toAlpha` above 1 or negative, signed hex — TS wraps in the PDF path); a partial trailing pixel is left untouched; buffer-size mismatches of `shade_bevel`/`adjust_sharpen` return `BufferSize` (TS zeroes or ignores, A4a); NaN morph radius returns `InvalidArgument` on a non-empty mask and `Ok` on an empty one, like TS (A4c); ±∞ radius, infinite σ, kernels over `max_kernel_radius` (incl. the hostile text-reflection `blurRad`, A4d) and over-limit sizes return `Err(Limit)` (TS throws RangeError, exhausts memory or hangs). Full list: the error table. | **Approve as a group** |
| G8 | `Limits` defaults: `max_mask_pixels` 64 Mi native / 32 Mi wasm32, `max_kernel_radius` 4,096, `max_path_points` 4 Mi, `max_fill_work` per G14, `max_custom_guides` 2,000, `max_image_pixels` 256 Mi native / 32 Mi wasm32. Native defaults are above every padded caller layer (≤ ≈ 48 M px), so parity is unaffected; **the wasm32 32 Mi is below a padded text glow** (A2) — on wasm32 such a glow returns `Err` where TS paints, a listed wasm32 difference until G12 is met. | **Yes, with the wasm32 difference recorded** |
| G9 | Banded sharpen/soften (1-byte luma plane, channel 0 only; soften with a ring of 2r+1 rows; from 24–28 B/px to ≈ 1 B/px): **not in stage 3** — stage 3 ports `blurRgb` whole-buffer; the banding is done in stage 5 on Opus under the four invariants of "Stage 5 handover" (the three f32 rounding points, luma plane complete first, soften row order, only `yb[i*3]` read). | **Yes, in stage 5 (Opus)** |
| G10 | API surface: drop the .NET public namespace `UniversalOffice.DrawingML` in 0.10 (`CHANGELOG.md`); wasm exports only what the shells need, built with the switch plan — pointwise adjusts/blip on chunks, the pattern table, `preset_outline`, Slides' `eval_preset` later (A10), and a **banded sharpen/soften call with an r-row halo** (r ≤ 8) on top of the G9 banding (A11). | **Yes** |
| G11 | Tables generated from the frozen TS files (not from `presetShapeDefinitions.xml`), checked in, with a deterministic generator and the regeneration diff test. | **Yes** |
| G12 | Phone budget for wasm32 effects (A2): a cap on peak live bytes per effect call (proposal 128 MB) as a gate-3 / switch requirement, met by the stage-5 pad-free EDT/morph (integer column plane, interior rows only — result-identical) plus lower wasm caller caps decided in `uo-docx-layout`; until then the wasm32 differences from the limit are listed. | **Yes** |
| G13 | Drop `blackbody` (no caller in Write, Slides or Sheet; one runtime `pow` user and one corpus group less); `CHANGELOG.md`. | **Yes** |
| G14 | `max_fill_work` (rows × edges) for `fill_polygons` now (`fill_polygons` returns `Result`), default set by the security specialist from measured times above what the TS callers produce; the result-identical active edge table stays a stage-5 item. | **Yes, limit now** |
| G15 | Requests to the uo-jsnum gate (A5, A6), not edited in its record: (a) `uo_jsnum::conv::js_index(i: f64, len: usize) -> Option<usize>` (None for NaN, fractional, negative, ≥ len) — otherwise a private helper with the same contract here; (b) add `f64::to_radians`/`to_degrees` to the banned list and a review rule against `std::f64::consts` other than `PI`; (c) this crate tries `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]` first (uo-jsnum G5). | **Yes** |

## Analysis skeptic (verified in the code)

Stage 2, Opus, 2026-11-01. Every finding was checked in the frozen TS (and the C# where noted);
claims of the plan that held are not repeated. Line numbers are of the `rust-core` worktree.

| # | Finding (evidence) | Blocking | Proposed decision |
|---|---|---|---|
| A1 | **G9 drops the f32 store points that R2 keys on.** Soften: the vertical sum is stored in `out` (Float32Array, adjust.ts:169/184) and only then `clamp8(b[j])` (201); sharpen: the horizontal `tmp` (175) and `yb[i*3]` (206/208) are f32. A streamed version has no arrays left, so "every Float32Array write is `as f32`" no longer triggers. Verified otherwise: the per-output operations are the same — only `yb[i*3]` is read, so channels 1–2 of `y` are dead (208); the luma plane must be complete, from the original pixels, before the first row of `d` is written (204–205 vs 210); in soften each horizontal row is computed before its source row is overwritten (output row y needs rows ≤ y+r). Banding is an algorithm design (ring indexing with clamped rows, h < 2r+1), not a translation. | **yes** (Sonnet readiness) | Stage 3 ports `blurRgb` whole-buffer; the banding moves to stage 5 (Opus, parity as the net) with these four invariants copied into the record — or, if the maintainer keeps it in stage 3, the plan gives the exact pseudo-code with the `as f32` points. |
| A2 | **The wasm32 `max_mask_pixels` (32 Mi) is not above what TS produces, and the phone numbers are low.** `morph` pads by `ceil(\|r\|)+2` (raster.ts:209–211). Text layers go up to 2.4e7 px (textfx.ts:144) with a glow margin of 1.2·rad (textfx.ts:84) → padded ≤ ≈2.0·N ≈ 48 M px; pictures grow by rad (canvas-picture.ts:53, cap 12e6 at 148) → ≤ 2.25·N ≈ 27 M; shapes/PDF 8e6 → 18 M. So on wasm32 a text glow on a layer above ≈16.7 M px returns `Err` where TS paints. Peak inside the crate is 8N + 16Np (sil, morph out, `pad`, `g` f64, `out`): ≈0.53 GB for a 12 Mpx picture glow, ≈0.96 GB for a 24 Mpx text glow, plus the caller's f32 RGBA (16N, textfx.ts:154); the plan's 230/700 MB assume Np ≈ N. wasm memory never shrinks; against the iOS baseline this is not viable. | no for stage 3 (translation unchanged); **yes for plug-in** | G8 states the wasm32 difference; new **G12**: a wasm32 phone budget per effect call as a gate-3/plug-in requirement, met by stage 5 (pad-free EDT: column pass reads the silhouette with the virtual frame, column plane in integer form — values are exact squares or 1e20 + k rounded, re-expanded to the same f64 —, row pass only for interior rows) together with lower wasm caller caps decided in uo-docx-layout. |
| A3 | **Error variants are not mapped.** `Error { Limit, BufferSize, InvalidArgument }` has no table of condition → variant, and the TS is not uniform: `morph` with NaN r does not throw at allocation (`new Float32Array(NaN)` has length 0) but at `pad.a.set` (raster.ts:212) — and returns an empty mask without throwing when the input is empty; infinite σ throws at raster.ts:137. uo-opc rework R2 was exactly a wrong error class. | no | Add a table: function → condition → variant → TS behaviour, and require every check before the first allocation (M1 error paths). |
| A4 | **G7 is incomplete.** (a) `BufferSize`: TS soften with `d.length > 4wh` zeroes the trailing RGB (`b[j]` undefined → `clamp8` → NaN → 0, adjust.ts:201), shorter buffers turn into NaN → 0 — Rust returns `Err`; (b) `applyPictureAdjust` diverges between the canvas (`Uint8ClampedArray`, canvas-picture.ts:133) and PDF (`Uint8Array`, pdf.ts:108) paths also for **negative** alpha/toAlpha and for hex with a sign (`parseInt('-f', 16)` = −15 → `d[i] = tr`, images.ts:579–590), not only "above 1"; (c) NaN morph radius on an empty mask (TS: Ok); (d) text reflection blur is bounded only on the bottom margin (textfx.ts:91), so a hostile `blurRad` can exceed `max_kernel_radius` (TS: huge kernel, effectively a hang); (e) the parity verifier needs a mechanical rule for the fill NaN case, not a judgement. | no | G7 wording: "every store of `applyPictureAdjust` outside 0..255 clamps"; list (a), (c), (d); (e) rule: a `fill_polygons` call whose polygons contain a non-finite coordinate is excluded from equality and counted. |
| A5 | **R1 list misses methods a translator reaches for.** `to_radians`/`to_degrees` (constant `π/180` ≠ `(deg * Math.PI) / 180`, effects.ts:40–41, bevel.ts:89), `sin_cos`, `exp2`, `exp_m1`, `ln_1p`, `log2`, `log10`, `cbrt`, `asin`, `acos`, `sinh/cosh/tanh`, `rem_euclid`/`div_euclid` (color.ts:112/119 use `%`), `round_ties_even`; `std::f64::consts` other than `PI` (TS writes `Math.PI / 2`, `1 / 3`). Casts: `x as u8` truncates and saturates — neither `ToUint8` nor `ToUint8Clamp`. | no | Extend `disallowed-methods`/`disallowed-types` accordingly; `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]` in the crate so every f64 → u8/usize goes through a uo-jsnum helper. |
| A6 | **No helper for typed-array reads with a float index.** NaN indexes reach typed arrays at bevel.ts:101 (`LIN_SRGB[NaN]`), light3d.ts:186 and 225 (`t[NaN]`, `tab[NaN]`), raster.ts:121 (`row[NaN] +=`, write ignored, then `ib` branch still runs), raster.ts:235/250 (`m.a[NaN]`). R6 states the semantics but each site would be solved ad hoc. | no | One helper in uo-jsnum, used at every f64-derived index: `js_index(i: f64, len: usize) -> Option<usize>` (None for NaN, fractional, negative, ≥ len); read → NaN, write → no-op. |
| A7 | **Empty subpaths.** `evalPreset` pushes `segs` even when a path has no drawable command (preset-engine.ts:241), with `fill`/`stroke` parallel (242–243); `presetPath` tests `g.subs.length` (geometry.ts:89 and the default branch). `Subpaths` must keep them. | no | Spec: `end_sub()` always records an end; `len()` counts empty subpaths; `iter()` yields `&[]` for them. |
| A8 | Trap table: `tol = 0` with a zero-length segment gives `0/0` → NaN → no points; `tol = -0` gives `n = 2` (raster.ts:47/56/67) — not "tol = 0 → 256". `Rust` sort: `total_cmp` orders `-0 < +0` (JS comparator treats them equal); harmless by the plan's equal-x argument (verified: equal-x spans are zero width, the positive spans and their order are fixed, so the f32 row sums are identical) — name `total_cmp` as the comparator. | no | Correct the row; fix the comparator in the plan. |
| A9 | **Colour resolver deferral: fine for the Write switch, but G6 names the wrong first consumer.** Write's path is docx-core reading `a:srgbClr/schemeClr/sysClr/prstClr` into a `"RRGGBB\|lumMod=…"` string (drawing.ts:259–264) → `resolveDrawingColor` (`parseInt(v, 10)`, resolve.ts:397–411) → `applyColorMods`, the only thing it takes from this crate; Write does not import `shared/charts`; Slides' copy (`theme.ts`) is needed only at the Slides port. The first consumer of an XML resolver is therefore **uo-docx**. | no | G6: the shared resolver (XML element + theme + clrMap) is designed in the uo-docx plan and placed here, so the TS string encoding is not ported as Write-only glue and duplicated at the Slides port. |
| A10 | **Users and dead code.** `blackbody` has no caller (only the export, index.ts:17); `hasPresetDef` and `PRESET_DEFS` have no caller outside the package. Missed user: Slides shell `slides/packages/slides/src/chrome-widgets.ts:266` (`evalPreset` for gallery icons) — needs a wasm export when Slides switches. | no | New **G13**: drop `blackbody` (one runtime `pow` user and one corpus group less). Add chrome-widgets to the users table and to G10's later list. |
| A11 | **Sharpen/soften at the wasm boundary need the whole image in wasm memory** (4N, 48 MB at 12 Mpx); the plan's "wasm memory does not grow by the image size" holds only for the pointwise adjusts and blip. σ ≤ 2.35 (adjust.ts:198), so r ≤ 8. | no | G10: a banded call with an r-row halo for sharpen/soften in the switch plan (pairs with A1). |
| A12 | **Real-document capture runs in a browser, not in Node.** `run-ours.mjs` bundles with esbuild and renders in Playwright Chromium/Electron (run-ours.mjs:17, 29, 128). (a) `pow` facets — shade/tint, present in almost every themed shape outline — are references only if that run is on Linux with `GLIBC_TUNABLES` passed in the browser launch env (WSL2), otherwise they are excluded; (b) the blip functions are relative imports (`./images.js`: canvas-picture.ts:14, pdf.ts:11, picture.ts:8) — an esbuild `onResolve` plugin keyed on the importer, not a package alias; (c) in-place functions (`fillPolygons`, `shadeBevel`, `adjust*`, blip) need a snapshot before the call, and the typed-array kind is part of the input (A4b); (d) buffers reach 24 Mpx × 4 B per call — cap the blob store (digest-only above a size). | no | Write (a)–(d) into the parity plan; G2 covers the browser run too. |
| A13 | **uo-jsnum dependency: right, and the fdlibm claim holds.** Checked the probe data against correctly rounded values (Python `Decimal`, 6,000 samples each): Node 24 sin/cos are not correctly rounded in ≈3.4% of samples, like the `libm` crate (≈3.2%); the Node/`libm` sin differences all lie at \|x\| > π/4 (range reduction), cos also below (kernel) — consistent with V8's fdlibm 5.3 against musl's newer msun, not with glibc or CORE-MATH. Surface this crate needs: `sin cos tan atan atan2 exp log pow hypot`(2 args) `round sign min max`(2 and 3 args) `to_number`, `parse_int` (radix 16, blip only), `to_uint8_clamp` (sharpen luma), `js_index` (A6). Not needed at run time here: `split_js_whitespace` (custGeom tokenization is in the Slides reader, read.ts:1506), `to_uint8` (only if G7 is rejected). | no | Pass this list to the uo-jsnum planner; G1 unchanged. |
| A14 | **`fill_polygons` has no time bound.** O(rows × 4 × edges) (raster.ts:102–108): with `max_path_points` 4 Mi and a tall mask the work is ~10¹⁵ on hostile custGeom; the TS hangs the same way, and the fuzz targets with small `Limits` will not reach it. | no | New **G14**: a `max_fill_work` (rows × edges) limit with a default above what the TS callers produce, set by the security specialist with measured times — or the active edge table (result-identical) in stage 3. |
| A15 | Process (uo-opc R1, R3): the table generator and every corpus generator are committed and deterministic; `lib.rs` re-exports are listed in the plan; if stage 3 is split between agents, one of them owns `lib.rs`/`error.rs`/`limits.rs`. | no | Add to the stage-3 brief. |

### Sonnet readiness (skeptic)

Closed enough for a mechanical translation **after the revisions above are written into the plan**.
Places where a Sonnet translator would otherwise make a design decision: the G9 banding (A1 — move
to stage 5 or pseudo-code), the error variant per condition (A3), float-index reads/writes and
f64 → integer casts (A5, A6), empty subpaths (A7), the sort comparator (A8), and the parity
verifier's rule for the fill NaN case (A4e). With A1 moved to stage 5 nothing in stage 3 needs
Opus: the generator is mechanical if it copies `parseDef` verbatim (it is not exported), and the
rest is file-by-file translation under R1–R10. The verifiers should look first at R2 store points,
R5, both blur edge modes and `shade_bevel`'s Sobel/`atan2` signed zeros.

## Verification (gate 2 — YYYY-MM-DD)

### Parity

Corpus: … / … items. Mismatches: 0. Differential: …

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native) | | iterations, min/max live |
| M2 soak (wasm) | | memory.buffer high-water |
| M3 budgets | | allocated / peak per operation |
| M4 boundaries | n/a (no handles) | |
| M5 cache caps | | no caches; growth tests |
| M6 Miri / LeakSanitizer | n/a (`forbid(unsafe_code)`) | |

### Security

Findings, fuzz targets (time, executions, crashes).

## Evolution

| Change | Keeps results? | Before | After | Decision |
|---|---|---|---|---|

## Final review and benchmarks (gate 3 — YYYY-MM-DD)

Implementation skeptic findings and decisions. `tools/bench` table (main × rust-core): presets
(all names × sizes), effects at 1 and 4 Mpx, adjusts at 12 Mpx, native and wasm.

## Process metrics

| Stage | Agent (model) | Time | Tool calls | Rounds / rework |
|---|---|---|---|---|
| 1 | planner (Opus) | | | |
| 1 | reuse specialist (Opus) | | | report: math probe `%TEMP%/uo-mathprobe` (Node 24.15 / V8 13.6, Windows) |
| 2 | analysis skeptic (Opus) | | ≈45 | 15 findings (A1 blocking for Sonnet readiness; A2 blocking for the switch) |
