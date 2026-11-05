# uo-drawingml — presets, colour modifiers, raster effects, image adjustments

Record of the rust-port workflow. Status: **gate 1 approved 2026-11-01** (G1–G15 as the final recommendations; G2 pow follows the uo-jsnum gate: option (a)); stage 3 phase 1 (foundation, the preset engine) done 2026-11-03; phase 2 (three frontiers) is next.

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
    planned with the first legacy-format port. Done with the OfficeArt records of uo-docx G11:
    `odraw::msospt` (section "odraw — OfficeArt").
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

## Questions for the maintainer (gate 1)

**Approved 2026-11-01.** Maintainer, verbatim: "Eu aprovo tudo" — every final recommendation accepted. G2 (pow) = uo-jsnum G1 option (a). — consolidated (revised after the skeptic)

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

## Stage 3 — port

### Phase 1 — foundation (Sonnet, `crates/uo-drawingml`, 2026-11-03)

**What exists.** The crate `core/crates/uo-drawingml` (`Cargo.toml`: `uo-jsnum` by path; dev-deps `uo-alloc` with `global` and `uo-testkit`; workspace lints), `lib.rs` (`#![forbid(unsafe_code)]`, `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]`, every module declared privately, the re-exports of the Public API block by name, no globs, plus the `Result` alias), `error.rs` and `limits.rs` complete, the shared types, the table generator with its tests, and the preset engine with `preset_path` and the outlines ported and verified bit for bit against the TS.

| Part | State |
|---|---|
| `error.rs` | **complete**: `Error { Limit(LimitKind), BufferSize, InvalidArgument, NotPorted }`, `LimitKind { MaskPixels, KernelRadius, PathPoints, FillWork, ImagePixels }`, `Display`, `std::error::Error`, `Result<T>`. `NotPorted` is the stage-3 placeholder (decision 1). |
| `limits.rs` | **complete**: `Limits` with the six fields of the plan, `NATIVE` (`max_mask_pixels` 2^26, `max_kernel_radius` 4,096, `max_path_points` 2^22, `max_fill_work` 2^30 (provisional, decision 2), `max_custom_guides` 2,000, `max_image_pixels` 2^28), `WASM32` (`max_mask_pixels` and `max_image_pixels` 2^25), `DEFAULT` by `target_pointer_width = "32"`. The module comment states the wasm32 text-glow difference (G8, A2). |
| `path.rs` | **ported**: `Pt`, `PathSeg` (`E.ccw` is a `bool`), `Subpaths { segs, ends }` keeping empty subpaths (A7: `end_sub` always records an end; `len`, `iter`, `get`, `push`, `end_sub`, `segs`). |
| `preset/def.rs`, `tables/presets.rs` | **generated table + lookup**: `PresetDef`, `Guide`, `Op` (+ `Op::parse`), `PathDef`, `Cmd`; 187 presets in the JS key order, a sorted `PRESET_INDEX` for the binary search; own names only (G7). |
| `preset/engine.rs` | **ported**: `Guides` (`HashMap`, `get`, `val`, `len`, `is_empty`), builtins, `evalFormula` (all 17 ops), `preset_vars` (by name / by position / NaN kept), `has_preset_def`, `preset_names`, `GuideDef` + `custom_guides` (truncation at `max_custom_guides` per list), `arc_to_seg`, `preset_guides`, `TextRect` + `preset_text_rect`, `PresetGeometry` + `eval_preset`. |
| `preset/path.rs` | **ported**: `PresetPath`, `PathShape`, `preset_path` (every hand-drawn shape, the action-button glyphs, the official-formula fallback). |
| `outline.rs` | **ported**: `Outline`, `preset_outline`. |
| `raster.rs` | **types ported, functions stubbed**: `Mask` (private fields, `new` with the `max_mask_pixels` check before the allocation, `from_vec` with `BufferSize`, accessors), `Affine` (`IDENTITY`, `mul` = `mulAffine`, `invert` = `invertAffine` with `det \|\| 1e-12`, `apply`), `Polygons` (builder: `push_point`, `end_poly`, `pending_numbers`, `discard_pending`, `get`, `iter`), `DEFAULT_TOL`. The seven functions return the placeholder. |
| `num.rs` | `f32_store(f64) -> f32`, the one audited `Float32Array` store (decision 9). |
| `tables/{light,word_bevel,patterns,adjust,srgb}.rs` | **generated, not used yet** (phase 2): light tables and bevel profiles (23 + 4), Word bevel tables (12), patterns (54), brightness/contrast curves and the temperature table, `SRGB_LIN` (as f32 bits) and `LIN_SRGB`. |
| `color.rs`, `patterns.rs`, `adjust.rs`, `blip.rs`, `effects.rs`, `light3d.rs`, `bevel.rs` | **stubs with the final signatures** of the plan (types included); placeholder policy below. |
| `tools/gen-tables.mjs` | the generator (G11), committed and deterministic; reads the frozen TS sources through `--ts-root`/`UO_TS_ROOT`. |
| `tools/capture-presets.mjs` + `tests/captures/presets.capture` | the TS differential of the preset engine. |

**Placeholder policy of the stubs** (one line at the top of each file names the frontier that ports it; no `todo!`, no `panic!`): functions returning `Result` return `Err(Error::NotPorted)`; `f64` results are NaN; `Option` results `None`; collections empty; in-place mutators do nothing; `Affine` results NaN-filled. A phase-2 port replaces every body and removes its use of `NotPorted`.

**Frontier split for phase 2** (by the plan's port order and the data dependencies; each frontier owns only its files plus the re-exports already in `lib.rs`, which need no change; new error variants or limits go through the owner of `error.rs`/`limits.rs`):

| Frontier | Files (TS source) | Needs | Notes |
|---|---|---|---|
| **A: colour, patterns, adjust, blip** | `color.rs` (color.ts), `patterns.rs` (patterns.ts, table done), `adjust.rs` (adjust.ts; `BRIGHT_LUT`, `CONTRAST_LUT`, `TEMP_TABLE` done; `blurRgb` whole-buffer, A1), `blip.rs` (images.ts, G4) | `uo-jsnum` only (`pow` in `color.rs` shade/tint and saturation; `parse_int`, `to_uint8_clamp`, `to_uint8` in adjust and blip) | self-contained; `pow` facets need the WSL capture; no dependency on B or C |
| **B: raster, effects** | `raster.rs` (the seven functions; types done), `effects.rs` (effects.ts) | `path.rs` types, `Limits`, `f32_store` | the biggest numeric surface (flatten, fill with `max_fill_work`, both blur edge modes, EDT, morph); `effects.rs` calls `raster.rs` |
| **C: light3d, bevel** | `light3d.rs` (light3d.ts; `LIGHT_TABLES`, `PROFILES` done), `bevel.rs` (bevel.ts; `WORD_BEVEL`, `SRGB_LIN_BITS`, `LIN_SRGB` done) | `bevel.rs` calls `distance_to` and `gaussian_blur` of B: it compiles against the phase-1 signatures, but its parity tests need B's port | `light3d.rs` is independent and can be tested first; `bevel.rs` last |

**Tests (all from `core/`, `cargo test --release -p uo-drawingml`).**

| File | Tests | What |
|---|---|---|
| unit (`src/`) | 14 | `error` 1, `limits` 1, `path` 1 (empty subpaths), `raster` 3 (mask limits and `BufferSize`, affine formulas incl. `det \|\| 1e-12`, polygons), `num` 1, `tables::tests` 7 (below) |
| `tables::tests` | 7 (+1 opt-in) | **re-parse of the TS while it exists** (`shared/drawingml/src`, a second, independent tokenizer): presets (187 names in order, every guide, command and path attribute, text rectangle, the index is a sorted permutation), light tables and profiles, Word bevel, patterns, adjust curves; **`SRGB_LIN`/`LIN_SRGB` recomputed with `uo_jsnum::math::pow`** (512 values, equal to the loops evaluated by Node 24.15.0 linux-x64 with the FMA variant of glibc's pow masked: a check of the pow port as well); with `UO_TS_PARITY=1` the generator runs again (`--no-srgb`, Node) and its files equal the checked-in ones byte for byte |
| `tests/preset_parity.rs` | 3 | the TS differential (below) |
| `tests/engine.rs` | 12 | one test per JS trap: own names only, `Some(&[])` truthy with no length, `??` keeps NaN, `indexOf` first match, `val` rules (`0x1A`, `1e400`, U+00A0/U+FEFF, `1_0`), `z \|\| 1`, `?:` is `x > 0`, shadowed builtins and last-definition-wins, `max_custom_guides` per list, the arc guard lets NaN through and a full turn, text rectangle `None` rules, path scaling, outline fallback, hand-drawn kinds and shading |
| `tests/memory.rs` | 8 | M1 on every public operation (the engine over 4 sizes and 6 adjust cases, custom guides, arcs, shared types, masks, and every stub), the error paths of `Mask::new`/`from_vec` (nothing allocated before the check: `usage.allocated == 0`), M5 (10,000 distinct unknown preset names: live delta 0), M2 soak (100 iterations in release, 6 in debug, sampled every 2, **tolerance 0 B**), M3 budgets |
| `tests/nopanic.rs` | 5 | every preset x 60 hostile size/adjust/name combinations (NaN, ±∞, `MAX`, `MIN`, subnormals, `-0`, random bit patterns, lists of 0–8 values and names, `Some(&[])`), 6,000 hostile custom guide programs (one in 500 with 3,000 formulas over the limit), 400,000 arcs from hostile values (the `while p1 <= p0` loops end), builders in any call order, masks of any dimension |

**TS differential (the preset engine).** `tools/capture-presets.mjs` runs the frozen `shared/drawingml/dist` (`presetPath`, `evalPreset`, `hasPresetDef`, `presetGuides`, `presetTextRect`, `presetOutline`, `customGuides`, `arcToSeg`) and writes `tests/captures/presets.capture`; `tests/preset_parity.rs` regenerates every input with the same code (the Rust mirror of the corpus), hashes the outputs with the uo-testkit stream (exact f64 bits, `-0` distinguished, NaN canonicalized with `uo_jsnum::conv::canonicalize_nan`) and compares digest by digest.

- Corpus: all **187 presets** + 13 names without definition (`undefined`, `''`, unknown, `constructor`, `__proto__`, `toString`, `hasOwnProperty`, `valueOf`, wrong case, a bare `actionButton`, a leading space…) x **7 sizes** {(200,100), (100,200), (100,100), (0,0), (1,1e6), (21600,21600), (13.37,0.5)} x **the adjust sets** of `adjSets` (none, `[]`, `[]` with `[]` names, defaults positional and named, each av perturbed by 0, −1, 1e5, 1e9, −1e9, NaN, ±∞, 1e308 as positional, named, partial, reordered and duplicated lists, long lists, unknown names; 21,470 sets over the whole corpus) x the five functions = **751,450 calls**, as 7,201 digests (facet per preset, size and function) plus `has/all`; **5,000 custom guide programs** (all ops, unknown ops, forward references, builtins shadowed, names `''`/`'0'`, tokens `0x1A`, `1e400`, ` 12`, `Infinity`, `+.5`, `5.`, `0b11`, `1_0`, U+00A0/U+FEFF padding, an Arabic digit) with 87 queried names each = 435,000 values; an `arcToSeg` grid of **24,948** cases (NaN, ±∞, zero and negative radii and sweeps, 1e308).
- **Result: 7,222 facets equal, 0 mismatches, 0 accepted differences, 0 not replayed.** No accepted difference was needed. A mutation check showed the replay bites: changing the octagon constant 0.29289 to 0.29290 broke 6 facets (reverted).
- **Which facets touch `pow`: none.** The ported code calls only `sin`, `cos`, `tan`, `atan2` and `sqrt` (fdlibm in V8, identical on Windows and Linux x64). `pow` appears in this phase only in `SRGB_LIN`/`LIN_SRGB` (generated, see above). The committed `presets.capture` was nevertheless taken in WSL (Node v24.15.0 linux-x64, glibc 2.39, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`); a capture on Node v24.15.0 win32-x64 has the same content line for line. The replay requires V8 13.6.233.17, x64 and Node v24.15.0 only. Capture runtime: 4.4 s.
- Replay runtime: 2.8 s release, 36 s debug.

**Measured (M1–M5, phase-1 scope).** M1: green on every public function and every stub, outputs `black_box`ed; error paths of `Mask::new` allocate 0 bytes. M5: no cache exists; 10,000 distinct unknown names leave the live bytes unchanged (the TS cached `null` for each). M2: flat with tolerance 0 B. M3 (200x100, per call): `eval_preset` and `preset_path` allocate at most **29,425 B** (gear9, peak 19,232 B), typical presets 3.5–10 KB (`rect` 3,921 B, `star5` 4,817 B, `cloud` 10,210 B); `preset_guides` and `preset_text_rect` at most **22,448 B** (circularArrow; the `HashMap` of ~200 guides), 3,216 B for a preset with few guides; `preset_outline` = `eval_preset` + 1–2 bytes-sized flag vectors; `custom_guides` 3,216 B for a small program. The test asserts ceilings of 44 KiB (eval/path) and 33 KiB (guides, text rectangle), ≈ 1.5x the measured worst case.

**Gate commands (from `core/`, `CARGO_TARGET_DIR=core/target/dml`).** `cargo fmt -p uo-drawingml -- --check` clean; `cargo test --release -p uo-drawingml` 14 + 12 + 8 + 5 + 3 tests pass (the debug `cargo test` passes too); `bash core/tests/grep-bans.sh` clean; `bash core/tests/float-casts.sh` clean (after decision 10); `cargo build -p uo-drawingml --target wasm32-unknown-unknown` (dev and release) builds, and `cargo clippy --target wasm32-unknown-unknown` is clean. `cargo clippy -p uo-drawingml --all-targets --all-features -- -D warnings` is clean, with the workspace ban list and the two cast lints (for a while during the work it stopped in `uo-jsnum`, `src/dtoa/cached_powers.rs:117`, `disallowed_names: foo`, increment B of another session; I linted with `--no-deps` meanwhile and re-ran without it at the end: clean).

**Decisions the plan did not settle** (everything else is the plan as written):

1. *`Error::NotPorted`* is a fourth variant besides the plan's `Limit`, `BufferSize`, `InvalidArgument` (the brief asks for it); it is stage-3 only.
2. *`max_fill_work` = 2^30 (native and wasm32)* is a provisional number: the plan leaves the value to the security specialist. Reasoning: the scan costs about 4 x rows x edges simple operations, so 2^30 is a few seconds; a real outline has at most about 10^4 edges over at most about 5 x 10^3 rows (5 x 10^7), a factor 20 below. `max_image_pixels` and `max_mask_pixels` are the plan's numbers.
3. *`Guides::len()` and `is_empty()`* are not in the plan's API: the differential compares `Map.size` (the TS `presetGuides` returns the `Map`), and no iteration is exposed, as planned.
4. *`custom_guides` truncation* applies `max_custom_guides` to each list (`av` and `gd`) separately, as Slides' `slice(0, 2000)` is applied per list in the caller.
5. *Representation of the generated tables* (the plan fixes the content, not the types): `include!`d `.rs` files (not formatted by rustfmt, like `uo-fonts`' `catalog_tables.rs`); the formula args keep the three tokens the TS destructures (a fourth is dropped, as in `[op, a = '0', b = '0', c = '0']`); a path command is a byte (`M L Q C A Z`, or 0 for a command the TS `switch` ignores) with all its tokens; brightness/contrast knots are `[u8; 33]` (integers 0..255, asserted), the temperature table `(f64, f64, f64)`, `SRGB_LIN` is stored as the bits of the f32 values, patterns as `[u8; 8]` decoded at generation time; `PRESET_INDEX` is a `[u16]` of positions sorted by name. The generator asserts: no integer-like key, ASCII names and tokens without quotes or backslashes, every guide op known, every fill in `{'', none, darken, darkenLess, lighten, lightenLess}`, every table length.
6. *The stubs' placeholder values* (NaN, `None`, empty, no-op, `Err(NotPorted)`) are my choice; the brief says "neutral". `Align::from_attr` returns `Other`, `LightTable` is empty.
7. *Types and builders of `raster.rs` beyond the plan's listing*: `Mask` fields are private with `w()`, `h()`, `data()`, `data_mut()`, `into_vec()` (the plan lists these accessors); `Polygons` has the builder `push_point`/`end_poly`/`pending_numbers`/`discard_pending`/`len`/`is_empty`/`get`/`iter` (the plan gives only its layout); `Subpaths::segs()`. The affine arithmetic (`mulAffine`, `invertAffine`, `apply`) and `Mask::new`/`from_vec` are already ported in phase 1 (they are tiny and the shared types need them); the frontier-B port may extend `Polygons`.
8. *Saturating counts* (declared as the brief asks): `Subpaths::end_sub` and `Polygons::end_poly` store the end offset as `u32` and **saturate** at `u32::MAX` (`u32::try_from(len).unwrap_or(u32::MAX)`), and the reads clamp offsets to the length. Unreachable for the data of this crate (a preset has at most a few hundred segments; `flatten_path` stops at `max_path_points` = 2^22), and no result changes below 2^32 elements; it only keeps the builders panic-free. No other conversion of arithmetic semantics exists: no `as` between floats and integers in the library (the cast lints are `deny` for the whole crate), integer `usize`/`u32`/`u64` conversions are `try_from`.
9. *`num::f32_store(f64) -> f32`* is the single `Float32Array` store of the crate (R2: `x as f32` is `Math.fround`): the deny of `cast_possible_truncation` also fires on `f64 as f32`, so the one `#[expect(clippy::cast_possible_truncation)]` lives in this helper and phase-2 ports use it (`#[allow(dead_code)]` until they do; it is exercised by a unit test and by the `SRGB_LIN` check).
10. *`core/tests/float-casts.sh` now runs `cargo clippy ... --no-deps`.* The script lints each crate with the cast warnings on, and when the crate depends on `uo-jsnum` (this crate is the first) it reported `uo-jsnum`'s own deliberate `f64 as i32` casts, which the script is meant to exclude ("uo-jsnum holds the conversions"). One-line change; the result for the other crates is unchanged.
11. *`''` as `None`*: `preset_path`, `preset_outline` and `preset_text_rect` treat `Some("")` like the TS falsy `geom`/`name` (the hand-drawn `switch` and `geom && ...` give the same result either way), `has_preset_def(Some(""))` is false.
12. *Test and capture layout*: facets at (preset, size, function) granularity (`path.s3`, `eval.s0`…) so a mismatch names the first diverging case; the capture records `av`, `avd` (defaults at 100x100) and `guides` per preset because the Rust mirror cannot derive them without the TS; `UO_TS_PARITY=1` re-runs the generator without `srgb.rs` (Node on any OS); `srgb.rs` is regenerated in WSL (`tools/gen-tables.mjs` refuses to write it elsewhere) and its values are re-derived in `cargo test` with the pow port.

**Not done in phase 1 (by the brief or by order):** the modules of the three frontiers; the fuzz targets (`core/fuzz` `dml_*`) and the `uo-wasm` feature `drawingml-memtest` (stage 4); the CI soak with 1,000 iterations and the wasm soak; the real-document capture; the TS differential of colour, raster, effects, bevel, adjust and blip (each frontier writes its own, as the uo-fonts frontiers did); the `CHANGELOG.md` lines of the list "Changes from 0.9" (they go with the switch).

**Notes for the orchestrator.** (a) The decision about `max_fill_work` and the wasm32 limits is still the security specialist's. (b) A frontier that needs `Subpaths` or `Mask` constructors not listed should add them in its own file, not in `path.rs`. (c) The generated `presets.rs` has lines up to 7.8 KB; it is meant to be read through `tests/`, not by hand. (d) The three frontiers can start now: A has no dependency, B and C compile against the signatures here.

### Phase 2, frontier C — light3d and bevel (Sonnet, 2026-11-03)

**Ported (1:1, no stub left in the two files).** `light3d.rs` (`light_table`, `light_at`, `light_face`, `bevel_phi`, plus `LightTable::bytes`, decision 1) and `bevel.rs` (`shade_bevel`, with the Word table selection `word_table`, the Word light `word_light`, the Sobel gradient and the linear-light blend). Own files only: `lib.rs`, `error.rs`, `limits.rs` and `num.rs` are untouched (the light/bevel code uses `num::f32_store` for every `Float32Array` store: the light samples, the Word light, the Word face). Every table is the generated one of phase 1 (`LIGHT_TABLES`, `PROFILES`, `WORD_BEVEL`, `SRGB_LIN_BITS`, `LIN_SRGB`); every float-derived table index goes through `js_index` (`light_at`'s `t[...]`, `bevel_phi`'s `tab[...]`, the Word `at()`, `LIN_SRGB[round(v * 4095)]`); no std float math besides `+ - * / % sqrt abs`-free paths (`uo_jsnum::math::{atan, atan2, tan, sqrt, floor, max, min, round}` are used); `shade_bevel` calls frontier B's `distance_to` and `gaussian_blur` (real since B landed).

**Tests (all from `core/`, `cargo test --release -p uo-drawingml`; mine are the five files below, all green in release and in debug).**

| File | Tests | What |
|---|---|---|
| unit (`light3d.rs`) | 2 | a short or empty table reads `undefined` as NaN with no panic; the template-string key comparison (`concat_eq`) |
| `tests/light_bevel.rs` | 13 | the JS traps one by one: table sizes and fallbacks (unknown rig = `threePt`, `''` = unset, material of another rig), Object.prototype names are unknown keys for `dir`/`prst` (G7), table knots, azimuth wrap and polar clamp, direction rotation (`''`/unknown = 0), NaN/±∞ angles, `bevel_phi` profiles, clamps, `angle` never reads `t`, `-0` through `atan`; `shade_bevel`: no-op `!(w > 0.5)` before the buffer check, the error order and variants, empty/huge-dimension masks, `a <= 0` skips (and `-0`), NaN coverage stores 0, `a >= 1` is any value ≥ 1, the blend `round(c + (nv - c) * a)` checked against `uo_jsnum::math::round`, alpha and outside pixels untouched, every Word table condition, `''` as unset |
| `tests/light_bevel_parity.rs` | 4 | the TS differential (below) |
| `tests/memory_light_bevel.rs` | 5 | M1 on every public operation and every error path (outputs `black_box`ed; the light functions allocate 0 bytes; the no-op and every error path of `shade_bevel` allocate 0 bytes before their check), M5 (10,000 distinct unknown names through every function: live delta 0), M2 soak (100 iterations release / 6 debug, sampled every 2, **tolerance 0 B**), M3 |
| `tests/nopanic_light_bevel.rs` | 3 | 400,000 hostile `light_table`/`light_at`/`light_face`/`bevel_phi` calls (NaN payloads, ±∞, subnormals, `MAX`, `-0`, odd and Unicode names); 6,000 hostile `shade_bevel` calls (sizes 0–40, NaN/∞/negative coverage, buffers of the wrong length, tiny limits: 4,421 Ok and 1,579 Err in the committed seed); one 300×200 hostile silhouette |

**TS differential.** `tools/capture-light-bevel.mjs` runs the frozen `shared/drawingml/dist` (`lightTable`, `lightAt`, `lightFace`, `bevelPhi`, `shadeBevel`) and writes `tests/captures/light_bevel.capture`; `tests/light_bevel_parity.rs` regenerates every input (a Rust mirror of the corpus: silhouettes from analytic coverage with `+ - * / sqrt` only, noise from the seeded `Rng`, RGBA from the same `Rng`), hashes the outputs with the uo-testkit stream (f64 bits with `-0`, NaN canonicalized; RGBA as bytes) and compares digest by digest.

- Corpus: `lightTable` over 20 rigs × 14 materials (unknown names, `''`, `undefined`: 280 tables, byte digests); `lightAt` over the **23 table keys × 12 directions × 140 azimuths × 64 polar angles** (NaN, ±∞, `-0`, 1e300, 359.99999, subnormals, off-grid and exact knots) = 2,472,960 calls (as 4 values each, per-key digest), `lightFace` of every table; `bevelPhi` over 16 presets × 96 `t` × 11 `hw` (NaN, ±∞, `-1`, `-0`, 0 to 1.2 in steps of 0.02 and the 24 band centres); `shadeBevel` over **12,664 calls** on generated silhouettes (ellipses incl. 1×1, 2×2, 1-pixel lines and columns, a rounded rectangle touching the raster edge, a ring, three random blobs, full, empty, a noise mask with coverage −1/0/0.25/0.5/1/1.5, and 256×256 / 200×150 / 300×120 large ones) with random RGBA: **every Word preset and every condition that selects or rejects the Word table** (16 presets incl. `undefined`/`''`/unknown/wrong case × 6 rig/dir/material conditions × `w` ∈ {0.4, 0.6, 3, 12, 80, NaN} × 2 height ratios × 6 masks), **every rig × material × preset** (20 × 14 × 16, directions cycling), every direction × preset × rig, larger masks with Word on and off, and a silhouette with NaN coverage. Every call is made on a `Uint8ClampedArray` **and** on a `Uint8Array` and the script fails unless the two results are identical (12,556 comparisons, all equal; the 108 large calls only on the clamped array).
- **Result: 110 facets equal, 0 mismatches, 0 accepted differences, 0 not replayed**, first run (4 tests). Mutation check (each reverted): the Sobel `2 * g(cr, r)` term, the `D` step `0.02`, the Word `u * NT - 0.5` offset, the sign of `gy` in `atan2(-gy, -gx)` and `atan2(0 - gy, 0 - gx)` (the signed-zero trap: `-0` becomes `+0` and 48 of 50 bevel facets break) were each caught; one mutation survived: the `1e-6` guard of `max(1e-6, 1 - s * s)` in `bevel_phi`, which is **unreachable** (`u >= 1e-3` keeps `1 - s * s >= 0.002`), kept as the TS writes it.
- **Where each facet was captured.** The committed file was captured in **WSL, Node v24.15.0 linux-x64 (V8 13.6.233.17-node.48), glibc 2.39, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`** (the pow reference) for the whole file: the light facets (`light/*`) call no `pow` (sin/cos/tan/atan/atan2/sqrt only), the bevel facets (`bevel/*`) read `SRGB_LIN`/`LIN_SRGB`, which `bevel.js` evaluates with `Math.pow` at load time, so they count only from this environment (the replay requires linux, glibc 2.39, the tunables, V8 13.6, x64 and no Node flags). A capture on **Node v24.15.0 win32-x64** is the same file line for line except `@platform` (checked with `diff`), and a second WSL run is byte-identical (the script is deterministic). The replay runs 2.1 s release, 18 s debug.

**Measured (M1–M5, frontier C).** M1: green on every public function and every `shade_bevel` error path; the light functions allocate **0 bytes**; the no-op (`w <= 0.5`, NaN) and the `BufferSize`/`Limit(MaskPixels)` errors (padded size over the limit, or `w + 2` overflowing `u32`) allocate **0 bytes** before their check. M5: no cache; 10,000 distinct unknown names (the TS kept a decoded Word table per preset and a decoded light table per key) leave the live bytes unchanged. M2: flat with tolerance 0 B. M3 (`shade_bevel`, one call, circle bevel, `w` = size/8; Word on and off the same): **256×256: 1,860,772 B allocated (28.4 B/px), peak above start 1,072,256 B (16.4 B/px); 1024×1024: 29,462,692 B (28.1 B/px), peak 16,871,552 B (16.1 B/px)**. The plan's estimate (≈ 4Np + 12Np + 4N + 8N ≈ 28 B/px allocated, ≈ 24 B/px peak) holds for the allocation; the peak is lower because the padded plane is dropped before the blur. The test asserts ceilings 10% over the measurement.

**Gate commands (from `core/`, `CARGO_TARGET_DIR=$PWD/target/dml-c`).** `cargo test --release -p uo-drawingml --no-fail-fast`: the five files above pass (25 + 2 unit); `bash tests/grep-bans.sh` clean; `bash tests/float-casts.sh` clean; `cargo build -p uo-drawingml --target wasm32-unknown-unknown` (dev and release) builds; `rustfmt --check` clean on my files; `cargo clippy -p uo-drawingml --all-targets --all-features -- -D warnings` is clean for my files (at the time of the last run it stopped on 4 errors in `tests/raster_effects.rs`, frontier B's file in progress, and on nothing else).

**Decisions the plan did not settle** (everything else is the plan as written):

1. *`LightTable::bytes()`* is a public accessor that the plan's API block does not list. The corpus of the plan (item 6: "`lightTable` for all keys and unknowns (byte digest)") cannot be replayed from an integration test without it (the field is `pub(crate)`). It returns the `&'static [u8]` of the table (1,060 bytes).
2. *`shade_bevel` order of checks* follows the error table (no-op `!(w > 0.5)` first, then `BufferSize`, then the padded size) and adds one case the table does not name: **`w + 2` or `h + 2` overflowing `u32` returns `Limit(MaskPixels)`** (a mask whose other side is 0 can have any `u32` width), before any allocation.
3. *Storing the blend*: the TS writes into a `Uint8ClampedArray` or a `Uint8Array` (the callers use both). The stored values are `LIN_SRGB[...]` (a byte) or `Math.round(c + (nv - c) * a)` for `0 < a < 1`, always an integer in 0..=255 (or NaN → 0), so both array kinds give the same bytes; Rust stores `nv` directly and `to_uint8_clamp(round(...))` for the blend (declared here because the clamp would differ from the wrap for values outside 0..=255, which cannot occur). The differential checks the two kinds on 12,556 calls.
4. *Reads past the end are NaN, never a panic* (R10): a `distance_to` plane shorter than `(w + 2) * (h + 2)` (impossible with the real port) reads NaN where the TS reads `undefined`; a table shorter than the generated ones (not constructible outside the crate, covered by a unit test) gives NaN.
5. *G7 applied as the plan says, nothing new*: `dir`, `prst` (profile and Word table) are own-name lookups, so `constructor`, `__proto__`, `toString`… are unknown (the TS reads `DIR['constructor']` → NaN azimuth, `PROFILES['constructor']` → the `Object` function (a NaN profile), `WORD_BEVEL['constructor']` → `atob` of the function's source text, which throws). `rig` and `material` are not affected (the TS key is `rig|material`, never a prototype name). A test shows each of these names behaves as an unknown name.
6. *`Some("")` for `prst`* is not "unset" in the Word table (`b.prst ?? 'circle'` keeps `''` and `WORD_BEVEL['']` is absent, so the light model is used); for `rig`, `dir` and `material` it is falsy as in the TS (`b.rig && …`). Both are tested.
7. *No `(!tab && !wt)` branch*: `lightTable` never returns `undefined` (an unknown rig is `threePt`), so the TS condition reduces to the width test; the Rust function does not carry the dead half.
8. *Frontier A/B files*: I ran `cargo fmt -p uo-drawingml` twice at the start (before the other frontiers had changed anything on disk, by `git status`, the first time; the second time files of A and B were already modified), which rustfmt-formats the whole crate; after noticing it I formatted only my own files (`rustfmt --edition 2024` per file). If a formatting-only change shows up in `color.rs`, `adjust.rs`, `blip.rs`, `patterns.rs`, `raster.rs` or `effects.rs`, it is that.

**Not done:** the fuzz target `dml_bevel`, the `uo-wasm` memtest, the 1,000-iteration CI soak and the real-document capture (stage 4, as in phase 1); no `CHANGELOG.md` line (the list goes with the switch); the Word tables' `prst` coverage beyond the 12 generated ones is by construction (the table is data).

### Phase 2, frontier A — colour, patterns, adjust, blip (Sonnet, 2026-11-03)

**Ported (1:1 from `color.ts`, `patterns.ts`, `adjust.ts` and the four blip functions of `docx-layout/src/images.ts`).** `color.rs`: `srgb_to_lin`, `lin_to_srgb`, `ColorModKind::from_name`, `apply_color_mods` and the HSL helper (`hsl`: `%` is fmod, the hue segment is compared as `f64` so NaN falls in the last branch, `clamp01` = `max(0, min(1, v))`, `pow` through `uo_jsnum::math::pow`). `patterns.rs`: the table lookups, own names only, and the popcount density. `adjust.rs`: `adjust_saturation`, `adjust_temperature`, `adjust_brightness_contrast` (the curves rebuilt per call, on the stack) and `adjust_sharpen` with `blur_rgb` as the plain whole-buffer translation (the three `f32` store points: the kernel after `/= sum`, the horizontal pass and the vertical pass; the kernel sum accumulates the `f64` value, as the TS assignment expression does; the luma plane goes through `to_uint8_clamp`, half to even; all three channels are blurred as in the TS, 28 and 24 bytes per pixel as budgeted). `blip.rs`: `effect_line`, `apply_image_effects`, `effect_decode`, `apply_picture_adjust` (the hex pairs are `parseInt(s.slice(i, i + 2), 16)` over UTF-16 units through `uo_jsnum::text::utf16_slice` and `parse_int`). No `Error::NotPorted` is used any more by these files (the variant stays: the other frontiers still use it). The error rows of the plan hold: `adjust_sharpen` no-op (`!amount`, `w < 2`, `h < 2`) first, then `BufferSize`, then `Limit(ImagePixels)`, all before the first allocation.

**Tests (all from `core/`, `cargo test --release -p uo-drawingml`; the frontier-A files are their own, so the shared `memory.rs`/`nopanic.rs` are untouched).**

| File | Tests | What |
|---|---|---|
| `tests/frontier_a.rs` | 23 | one test per trap, numbers read from the frozen TS where given: every modifier name and unknown names, valued modifiers need a finite raw while `comp`/`inv`/`gray`/`gamma` always apply, `-0` and NaN in the curve (`clamp01`), NaN blackens the colour channels in saturation/temperature/brightness/contrast and keeps alpha, early outs (1e-6 thresholds), contrast +100% is the exact step at 128, a trailing partial pixel untouched (G7), sharpen/soften values, `!amount` and tiny images return before the buffer check, `BufferSize` before `Limit`, a tiny amount (kernel NaN: sharpen unchanged, soften black), images narrower than the kernel, `effectLine` truthiness, `effectDecode` rules, grey/line/bi-level order, floor and its `1e-9` guard, every store of `apply_picture_adjust` clamped (alpha above 1, negative, NaN, signed hex, `0x`, surrogate halves), an unparsable `from` matches nothing, the pattern table key order and own names (`constructor`, `__proto__`… are unknown, G7) |
| `tests/frontier_a_parity.rs` | 4 | the TS differential (below) |
| `tests/frontier_a_memory.rs` | 6 | M1 on every public operation and every error row (`measure(...).allocated == 0` for `BufferSize`, `Limit(ImagePixels)` and the no-op rows), M5 (10,000 distinct brightness values and 10,000 distinct unknown pattern names leave the live bytes unchanged), M2 soak of every operation (100 iterations release, 10 debug, sampled every 5, **tolerance 0 B**), M3 budgets |
| `tests/frontier_a_nopanic.rs` | 4 | 60,000 colour sequences over random bit patterns and all 16 kinds with raw NaN/inf/subnormal/random; 30,000 buffers of 0–69 bytes (lengths not a multiple of 4) through the three pointwise adjusts and sharpen/soften with random dimensions and `max_image_pixels = 400`, with the invariants "alpha never changes", "a trailing partial pixel never changes", "an error leaves the pixels as they were"; sharpen over `u32::MAX` dimensions and limits 0 / `u64::MAX`; 40,000 blip calls over random effects and hex text with lone-surrogate halves, signs, NUL and 10,000-character strings. They also run in debug, where overflow checks are on |

**TS differential.** `tools/capture-frontier-a.mjs` (committed, deterministic: two runs are byte-identical) runs the frozen `shared/drawingml/dist` and `write/packages/docx-layout/dist/images.js` of the checkout in `UO_TS_ROOT` (`D:/Projects/OfficeNext/.claude/worktrees/clickcompliance-docx`) and writes `tests/captures/frontier-a.capture`; `tests/frontier_a_parity.rs` regenerates every input with the testkit `Rng` and exact arithmetic, hashes the outputs (exact f64 bits, `-0` distinguished, NaN canonicalized, pixel buffers as bytes) and compares digest by digest. The committed capture is from WSL (Node v24.15.0 linux-x64, glibc 2.39, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`, no flags), and the replay requires exactly that platform, because `pow` is in 3 of its facets. Corpus:

- colour: **20,000 modifier sequences** (0–6 modifiers of the 16 kinds plus unknown names, raw undefined/NaN/±inf/±0/huge/negative/fractional/random, channels uniform/palette/grey/out of range/NaN) in 40 facets; `srgb_to_lin`/`lin_to_srgb` over **41,031 inputs** (a 0…1 grid, ±4 ulp around both knees, edge values, 20,000 uniform, 20,000 random bit patterns) in 9 facets;
- patterns: all 54 names with rows, density and hex, the key order, unknown names (3 facets);
- adjust: **57 generated images** (gradient, noise, flat, edges, smooth; 1×1 up to 130×70, plus two of 300×200; several narrower than the kernel) × 17 saturation values, 23 temperatures and 21 sharpen/soften amounts (171 facets), and the 25 × 25 brightness × contrast grid on an image with every level in every channel (25 facets). **Every adjust call ran in the TS on a `Uint8Array` and on a `Uint8ClampedArray` and the script fails when they differ: 4,102 calls, identical** (so one `&mut [u8]` serves both, as the module comment says);
- blip: `effectLine` 72 cases, `effectDecode` 3,072 cases, `applyImageEffects` 2,304 effect sets × 3 images, `applyPictureAdjust` 11 alphas × 12 duotones × 12 colour changes × 3 images = 4,752 combinations (hex valid, short, signed, junk, `0x`, non-ASCII, split surrogate pairs, alpha and `toAlpha` outside 0..1), compared with the `Uint8ClampedArray` run. The `Uint8Array` (PDF path) run differs from it in **1,933 of the 4,752 combinations**: those are the accepted differences of G7 (Rust clamps what the PDF path wraps); the script fails if an in-range combination (alpha and `toAlpha` in 0..1, hex in 0..255 or NaN) differs, and **1,584 in-range combinations were equal**.
- **Result: 256 facets equal (49 colour, 3 pattern, 196 adjust, 8 blip), 0 mismatches, 0 accepted in the replay (the G7 differences are outside the compared run), 0 not replayed.** Replay 0.5 s release, 1.1 s debug; capture 2.2 s.
- **Mutation check** (30 one-line mutants of `color.rs`/`adjust.rs`/`blip.rs`, files restored after each): 26 killed (gray coefficient, curve constants, the sRGB knees `<=` to `<`, tint in linear light, the `hue`/`hueOff` divisors, the `satMod` max, the saturation `pow` argument, `store8` half-even vs half-up, the luma rounding, the kernel normalization, the f64 vs f32 kernel sum, the clamped vs wrapped blur edge, the 255 identity knot, the `x1` of the last interval, the contrast step, the temperature curve and `DG`, the bi-level `>=`, the `1e-9` floor guard, the colour-change tolerance and its alpha rounding, the hex offsets, `effectLine`, `effectDecode`'s CMYK case, duotone `round` to `floor`). The survivors are equivalent mutants: `% 6` on `h` when `max === rf` (the quotient is within ±1), `% 6` on `floor(h2 * 6)` (`h2 < 1`, so `h2 * 6` rounds to at most `6 - 2^-50`), the literal `0.4166666666666667` equal to `1 / 2.4`, and `<` for `<=` against the 2.5 threshold (the delta is 0 at equality); one weak mutant (`val / 216` for `raw / 21600000`) is invisible after `(h % 1 + 1) % 1`, and a larger change of the same line was killed.
- **Which facets touch `pow`:** `color/mods` (shade and tint outside gamma mode), `color/lin`, `adjust/sat` (`sat^(1/s)`). Measured on Windows Node 24.15.0 x64 (not a reference): 2 of the 40 `color/mods` blocks and 5 of the 9 `color/lin` blocks differ from the WSL capture, and nothing else (`adjust/sat` included): the evidence for the WSL requirement.
- **"Both blur edge modes":** this frontier has only the clamped-edge `blurRgb`; the zero-padding blur is `raster.ts` `gaussianBlur`, frontier B. The clamped edge is exercised by the sharpen and soften grids on 1×1 … 9×9 images (radius up to 8 against a width of 1–9).

**Measured (M1–M5, this scope; release, 1024×768).** M1 green on every public operation and error row, outputs `black_box`ed; the error and no-op rows allocate **0 bytes**. M3: `adjust_sharpen` allocates **22,020,132 B = 28 B/px** (peak above start the same), 65 ms; soften **18,874,404 B = 24 B/px**, 69 ms; the three pointwise adjusts and the blip functions allocate **0 B** (the curves and their interpolation points live on the stack; the plan allowed 3 × 256 B). M5: the TS `lutCache` grew one curve per distinct value, here 10,000 distinct values leave the live bytes at a 0 delta. M2: flat with tolerance 0 B.

**Gate commands (from `core/`, `CARGO_TARGET_DIR` an absolute path, `D:/Projects/OfficeNext/.claude/worktrees/rust-core/core/target/dml-a`).** `rustfmt --check` clean on the frontier-A files (`cargo fmt -p uo-drawingml -- --check` lists only frontier-B files at the time of the run: `raster.rs`, `memory_raster.rs`, `nopanic_raster.rs`, `raster_effects.rs`); `cargo clippy -p uo-drawingml --all-targets --all-features -- -D warnings` clean (the whole crate at the time of the run); `cargo test --release -p uo-drawingml` all pass (the whole crate at the time of the run, the other frontiers included; the frontier-A files also pass in debug); `bash tests/float-casts.sh` clean; `bash tests/grep-bans.sh` reports one hit, in frontier B's `tests/raster_effects.rs:134` (`std::f64::consts::FRAC_PI_2`), none in the frontier-A files; `cargo build -p uo-drawingml --target wasm32-unknown-unknown` (dev and release) and `cargo clippy --target wasm32-unknown-unknown -- -D warnings` clean.

**Decisions the plan did not settle (everything else is the plan as written).**

1. *No change of arithmetic semantics.* No saturating, wrapping or truncating conversion was introduced: every `f64` to byte goes through `to_uint8` / `to_uint8_clamp` (the typed-array stores), every `f64` to index through `js_index`, every integer size through `try_from` or `checked_mul`. `store8(v) = to_uint8_clamp(clamp8(v))` in `adjust.rs` (an integer in 0..=255 or NaN, so it is both typed-array kinds); `apply_image_effects` stores through `to_uint8` (its `Uint8Array`, in range anyway); `apply_picture_adjust` through `to_uint8_clamp` (G7). A bug of my own, found by the debug run of the no-panic test: `n * 4` for `w = h = u32::MAX` overflowed `u64` and wrapped in release (the `BufferSize` check then compared against a wrong size); it is `checked_mul` now, and the test that found it stays.
2. *Curves on the stack.* `curve_for` builds the interpolation points in a `[(f64, [f64; 33]); 16]` array and sorts them with `sort_unstable_by(total_cmp)` instead of the TS stable `sort`: the keys are distinct, so the order is the same; 16 is room for the 15 points of the tables (a longer static table would be cut: unreachable). The brightness/contrast LUT is a `[u8; 256]` (the TS cache is not ported, as planned). An earlier version used a `Vec` (8,160 B per call); the M3 measurement showed it and it was removed.
3. *Unreachable fallbacks are explicit.* `blur_rgb` returns `Err(InvalidArgument)` if its radius is not an index below 65,536 (cannot happen: the callers' sigma is at most 2.35, so the radius is at most 8); `clamp_index` and the table reads use `unwrap_or` where the TS would read `undefined` (never reached). `blur_rgb` stays private.
4. *Lookups.* `pattern_rows` scans the 54 entries linearly (not the sorted index of the presets), in the TS key order of the generated table; the density is `popcount / 64`.
5. *Partial trailing pixel* (G7, as planned): `chunks_exact_mut(4)` in every function; the TS would zero or read `undefined` there (and for `clrChange` leave it untouched). Tested explicitly.
6. *`apply_image_effects` models `map[NaN]`* with `js_index` returning NaN (then 0 when stored, false in the bi-level comparison), although the index is always an integer in 0..=255 there.
7. *No fuzz targets, no wasm memtest, no CHANGELOG lines* (stage 4 and the switch, as for phase 1); the real-document capture is not done.
8. *Tooling note.* `CARGO_TARGET_DIR=core/target/dml-a` taken relative from `core/` creates `core/core/target`; I used the absolute path (and removed the stray directory I created).

**Not done in this frontier:** the 3-channel blur is not reduced to channel 0 and not banded (stage 5, G9); the real-document capture; the zero-padding blur (frontier B).

### Phase 2, frontier B — raster and effects (Sonnet, 2026-11-03)

**Ported (1:1 from `raster.ts` and `effects.ts`).** `raster.rs`: `flatten_path`, `fill_polygons`, `gaussian_blur`, `distance_to`, `morph`, `shift_mask`, `warp_mask` (the types, `Affine` and `Polygons` of phase 1 are untouched). `effects.rs`: `Align::from_attr`, `align_point`, `shadow_affine`, `outer_shadow_mask`, `inner_shadow_mask`, `glow_mask`, `soft_edge_mask`, `reflection_alpha`. The phase-1 signatures are kept (frontier C's bevel compiles against `distance_to` and `gaussian_blur` and its parity run passes on them) with one exception, decision 1. No use of `Error::NotPorted` is left in the two files. No std float math: `exp`, `sin`, `cos`, `tan`, `hypot`, `max`, `min`, `max_n`, `floor`, `ceil`, `sqrt` come from `uo_jsnum::math`, every `Float32Array` store is `f32_store`, every f64 index is `js_index`.

| File | Tests | What |
|---|---|---|
| `src/raster.rs` (unit) | 3 (phase 1) | unchanged |
| `tests/raster_effects.rs` | 30 | one test per rule: polygons of 3 points or more kept and shorter ones dropped, `Z` returns the pen, the tolerance traps (`tol` = 0 / −0 / NaN with and without a zero-length curve, singular matrix gives `scale` 1e-9), the `ccw` sweep normalization and its point counts, `max_path_points` at the exact bound; fill on and off the pixel grid, even-odd against non-zero, opposite windings, `min(1, a + row)` incl. NaN cells, the row of `ceil(ymax)` clamped and the rows past it untouched, no edges or a NaN y range leave the mask unchanged, `max_fill_work` at the exact bound (`rows × edges`) and "mask untouched on `Err`"; the blur against a **naive second implementation** (the TS loops written out) on 6 sizes × 7 sigmas, small sigma = copy, NaN = all-zero (+0 bits), zero padding without renormalization, the radius limit at the exact bound (3.3 gives 10, 3.4 gives 11) and on empty masks; EDT values, threshold 0.5, NaN not inside, all-background = 1e10, empty; morph values (`r − dd + 0.5`, the f32-stored distance), erosion at the border, copy under 0.01 (0.01 itself transforms), every row of the morph error table; shift/warp (integer, fractional, size, NaN gives NaN, ±∞ gives NaN weights, singular, limits); the align table, `shadow_affine` identity bit for bit, offset, scale about the anchor, skew; outer/inner/glow/soft semantics (which side the inner shadow shades), error pass-through, `reflection_alpha` branches incl. NaN |
| `tests/memory_raster.rs` | 8 | M1 on every public operation and every error row (outputs `black_box`ed; the error paths allocate **0 bytes**: `usage.allocated == 0` for the limit rows of fill, blur, morph NaN/±∞/padded, shift, warp, glow, soft edge; `flatten_path` over its limit releases its partial output: `live_delta == 0`), M2 soak (per iteration: 8 outlines flattened + filled + EDT at 256 px, outer/inner shadow, glow, soft edge on 512²; 100 iterations sampled every 5 in release, 4 in debug; `UO_SOAK=1000` for the CI job; **tolerance 0 B, flat**), M3 budgets at 1 and 4 Mpx |
| `tests/nopanic_raster.rs` | 6 | 60,000 hostile subpath lists × affines × tolerances × path limits; 60,000 hostile polygon sets (NaN, ±∞, `MAX`, `MIN`, subnormals, −0, random bit patterns) filled on masks 0×0 to 40×3 with hostile `max_fill_work`; flattened hostile paths filled; blur/EDT/morph on hostile masks (NaN, ∞, −1, 2) with hostile sigmas, radii and limits; shift/warp/the four effects with hostile values, sizes and limits; `u32::MAX × 0` and `0 × u32::MAX` masks through every function; sizes over the limits (`u32::MAX²`, 1e9, `f64::MAX`) and all-zero limits. Run in debug too (overflow checks on) |
| `tests/raster_parity.rs` + `tools/capture-raster.mjs` + `tests/captures/raster.capture` | 9 | the TS differential, below |
| `tests/memory.rs` | 8 | one existing call updated for decision 1 |

**TS differential.** `tools/capture-raster.mjs` runs the frozen `shared/drawingml/dist` (`UO_TS_ROOT` = the `clickcompliance-docx` checkout) and writes `tests/captures/raster.capture` (1,865 facets); `tests/raster_parity.rs` mirrors every generator (the seeded `uo-testkit` `Rng`, which is the xorshift32 of the JS side) and compares digest by digest: exact f64 bits with −0 distinguished for polygons and affines, f32 buffers hashed as LE bits with NaN canonicalized (`0x7fc00000`), a TS `RangeError` as the tag `E` (any `Err` here).

- Corpus: **flatten** — the outline of every one of the 187 presets × 5 sizes (1×1 to 400×150) × 10 affines (identity, scale, 1e6, rotation, skew, flip, determinant 0, NaN skew …) × 5 tolerances (0.35, 0, 1e-3, NaN, −0) = 46,750 calls, plus 4,000 generated subpath lists with every segment kind and hostile values (NaN, ±∞, 1e308, −0); **fill** — 3,000 generated polygon sets (rectangles on and off the grid, triangles, pentagrams, opposite windings, horizontal edges, repeated vertices, ±1e6 slivers) on 8 mask sizes (0×5 and 5×0 included), empty and pre-filled (values up to 1.5), both rules, and every preset outline flattened and filled on 2 sizes × both rules (748); **blur** — 17 sigmas (−1, 0, 0.04, 0.0499, 0.05, 0.3 to 33.3, 1000, NaN, ±∞) × 8 sizes (0×0 included) × 4 mask kinds (random, discs, binary, hostile values with NaN/−0/2/−1); **EDT** both directions × 4 kinds × 8 sizes × 3 seeds; **morph** 17 radii (±0.005, ±0.0099, ±0.01, ±0.5 to ±20, NaN, ±∞) × 7 sizes × 4 kinds; **shift** 14 offsets (NaN, ±∞, 1e308, −0 …) × 8 sizes × 4 kinds × 4 target sizes; **warp** 11 inverse affines (singular, NaN, ∞, 1e308) × 8 × 4 × 4; `mulAffine`/`invertAffine` over a value pool (6,000); **effects** — `alignPoint` (every algn, `undefined`, `''`, unknown), `shadowAffine` over 2,000 generated specs, the four masks over 5 silhouettes (2 discs, `star5`, `heart`, a binary mask) × 40 generated specs (outer/inner) and × 8 radii (incl. NaN, ∞, negative) × 3 pixel scales (glow/soft), `reflectionAlpha` over an 8,100-point grid with NaN. A polygon with a non-finite coordinate is not in the fill corpus (the mechanical rule of the plan); the nopanic test covers it.
- **Result: 1,865 facets equal, 0 mismatches, 0 accepted differences, 0 not replayed**; every group passed on its first run. Mutation check (each reverted): the sub-scanline offset 0.5 to 0.4 (fill, effects fail), the sort order reversed (fill, effects), the kernel value + 1e-9 (blur, effects), `INF` 1e20 to 1e19 (EDT), `r − dd + 0.5` to `+ 0.501` (morph, effects), `ceil(len/tol/2)` to `/2.1` (flatten, fill), the soft-edge factor 0.86 to 0.85 and the skew divisor 180 to 180.1 (effects): every one is caught. Frontier C's `light_bevel_parity` (bevel uses `distance_to` and `gaussian_blur`) passes on this port as a second witness.
- **Which facets touch `pow`: none.** The ported code calls `exp` (the blur kernel), `sin`, `cos`, `tan` (fdlibm in V8, the same on every Node x64) and `hypot` (the V8 Torque builtin). Where each capture was taken: the committed file in **WSL, Node v24.15.0 linux-x64, glibc 2.39, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`** (the pow reference environment, although no facet needs it); a **Windows Node v24.15.0 win32-x64** capture has the same records line for line (a `diff` of everything below the `@platform` line is empty); a second WSL run is byte-identical to the committed file. The replay requires V8 13.6.233.17, x64 and Node v24.15.0 only. Capture: 1.4 s; replay: 1.5 s release, 15 s debug.

**Memory (M1–M3, measured; `cargo test --release -p uo-drawingml --test memory_raster -- --nocapture`).** M1 green on everything above, error paths 0 B allocated. M2 flat, tolerance 0 B, 100 iterations (`UO_SOAK` raises it). M3, bytes allocated and peak above start, as multiples of N (pixels) and Np (the padded plane), against the plan's table (the asserted ceiling is the plan's formula + 2 % + 64 KiB):

| Operation | N = 1,048,576 (1 Mpx) | N = 4,194,304 (4 Mpx) | Plan |
|---|---|---|---|
| `Mask::new` | 4.00 N | 4.00 N | 4N |
| `fill_polygons` (star5, 10 edges) | 5.3 KB | 9.4 KB | `(W+1)·4 + edges·40 + intersections·16` |
| `gaussian_blur` σ 4 | 8.01 N, peak 8.01 N | 8.00 N, peak 8.00 N | 8N + kernel, peak 4N (the TS holds `tmp` and `out` together too: 8N) |
| `distance_to` | 12.03 N, peak 12.03 N | 12.01 N, peak 12.01 N | 12N + 4·max(W,H)·8 |
| `morph` r 4 (Np = 1.0235 N / 1.0117 N) | 20.40 N, peak 16.40 N | 20.20 N, peak 16.20 N | 16Np + 4N, peak ≈ 16Np |
| `outer_shadow_mask` | 12.01 N, peak 12.01 N | 12.00 N, peak 12.00 N | 12N |
| `inner_shadow_mask` | **12.01 N**, peak 12.01 N | **12.00 N**, peak 12.00 N | 16N (decision 6) |
| `glow_mask` rad 6 (p = 5) | 28.35 N, peak 16.34 N | 28.17 N, peak 16.17 N | ≈ 16Np + 12N, peak ≈ 16Np + 4N |
| `soft_edge_mask` rad 6 (p = 8) | 28.54 N, peak 16.53 N | 28.27 N, peak 16.26 N | same |

Peaks inside the crate for a glow are 17.1 MB at 1 Mpx and 67.8 MB at 4 Mpx (above the silhouette, which the caller holds); the A2/G8/G12 phone numbers of the plan stand (the stage-5 pad-free EDT is what lowers them).

**Gate commands (from `core/`, `CARGO_TARGET_DIR=target/dml-b`).** `cargo fmt -p uo-drawingml -- --check` clean; `cargo clippy -p uo-drawingml --all-targets --all-features -- -D warnings` clean for the library and for my test targets; `cargo test --release -p uo-drawingml` (all frontiers' tests) green, debug green for my tests; `bash tests/grep-bans.sh` and `bash tests/float-casts.sh` clean; `cargo build -p uo-drawingml --target wasm32-unknown-unknown --release` builds and `cargo clippy --target wasm32-unknown-unknown --lib` is clean.

**Decisions the plan did not settle** (everything else is the plan as written):

1. **`fill_polygons` takes `limits: &Limits` as a fourth parameter.** The plan's API block lists `fill_polygons(mask, polys, even_odd)` and the revised text says it returns `Result` because of `max_fill_work`, but the limit has no way in; every other allocating function takes `&Limits`, and the fuzz targets need a small one. `lib.rs` needs no change (re-exports are by name); the only caller outside the module, the stub call in `tests/memory.rs`, was updated. Anyone who wrote against the phase-1 three-argument signature (frontier C does not call it) must add the argument.
2. **`max_path_points` counts every point pushed during the call**, also those of a polygon dropped later (fewer than 3 points), and is checked before each push; on `Err` what was built is dropped. The plan said "points produced"; this is the strict reading (it bounds the work as well as the memory).
3. **`fill_polygons` order of checks**: a pre-pass over the edges (a lazy iterator, no allocation) counts them and takes `ymin`/`ymax` with the NaN-propagating `min`/`max`; then no edges gives `Ok`; no rows (`rows` NaN or ≤ 0, i.e. a NaN y range, or the polygons outside the mask) gives `Ok`; then `rows × edges` against `max_fill_work` (`checked_mul`) gives `Err(FillWork)`; only then are the edge list, the row buffer and the intersection list allocated. So the `Err` path allocates 0 bytes (the plan's table says "before the row buffer"). A mask `w × 0` with a huge `w` (legal through `Mask::from_vec`) has no rows and never allocates the `(w + 1)` row the TS would.
4. **Loop order changed, order of floating-point operations not**: the vertical pass of `gaussian_blur` runs row by row with a per-row f64 accumulator (`acc`, `8·W` bytes) instead of the TS column order (cache behaviour at 24 Mpx); each output pixel still sums `i = lo … hi` in increasing order starting from 0 with `s += tmp * k` in f64, so every bit is the same (the naive second implementation in the unit tests and the differential agree). The horizontal pass iterates slices. No arithmetic-semantics change otherwise: no saturating, wrapping or truncating conversion exists in the two files (the cast lints are `deny`); integer counters are `usize`/`i64`/`u64`, and f64 loop counters stay f64 where the TS bound can be NaN or fractional (R7).
5. **`gaussian_blur`**: the kernel is allocated with `try_reserve_exact` and `2r + 1` is `checked_mul`/`checked_add`, so a huge `max_kernel_radius` (or wasm32) gives `Limit(KernelRadius)` instead of an abort. The order of the table is kept (copy, NaN gives zeros, radius limit); the radius limit is also reported on an empty mask (the TS builds the kernel first).
6. **Memory-only differences from the TS, results identical**: `inner_shadow_mask` writes `sil × (1 − b)` into the blurred plane instead of a third plane (12N allocated against the plan's 16N); `soft_edge_mask` frees the eroded plane before the loop; `morph` builds its output only after the EDT and drops the padded copy as soon as the distance is computed (the peak is the same 16Np: `pad` is alive inside `distance_to`).
7. **`morph` limit arithmetic**: the padded size is converted with `js_index(…, usize::MAX)` (NaN, ±∞ and anything past `usize` are `None`, so `Limit(MaskPixels)`), the pixel count is a `u64` `checked_mul` against `max_mask_pixels`, and the padded plane is created with `Mask::new` (a second check). `±∞` is `Limit(MaskPixels)` for an empty mask too, as the TS throws at the allocation.
8. **`distance_to`/`edt_pass`** index the work arrays directly (the bounds are `size ≥ n`, `z` of `size + 1` and the `INF` sentinel at `z[k + 1]`; the nopanic tests drive it with hostile masks); `edt_pass` takes `n: u32` so `q` converts with `f64::from`. The `while s <= z[k]` loop has the guard `k == 0` then `break` (unreachable, plan).
9. **Sorting**: `total_cmp` as the plan fixes (the JS comparator `p.x − q.x` treats ±0 as equal; harmless, shown by the 3,748 fills). A non-finite coordinate in a polygon is a named divergence (G7); nothing in Rust panics on it (the sort is total, `js_index` ignores the NaN writes) and the nopanic test covers it.
10. **Tests**: the test crates allow `clippy::cast_precision_loss`/`cast_possible_truncation` (the generators mirror JS number arithmetic on indices; the two lints are `deny` only for the library), and one `PI / 2.0` replaces `FRAC_PI_2` (grep-bans). The capture was taken in WSL although no facet needs the pow environment (caution, the same as phase 1).

**Not done (by the brief or by order):** the fuzz targets `dml_raster` and `dml_effects` and the wasm soak (stage 4); the 1,000-iteration CI soak (the test reads `UO_SOAK`); the real-document capture; `CHANGELOG.md` lines (the list goes with the switch); `max_fill_work` is still the provisional 2^30 (a fill of 4 Mpx with 10⁴ edges is far below it; the active edge table is stage 5); the wasm32 `max_mask_pixels` difference for padded text glows (A2/G8) stands as in the plan; no test runs under wasm32 (the build and clippy do).

## Continuous verification (hybrid mode, Sonnet, 2026-11-03)

Hybrid mode (`.claude/skills/rust-port/SKILL.md`): parity, the memory gate and fuzzing run continuously in CI and point at what to fix; the Opus security review comes once, at the end of milestone 1. This section is what was added for `uo-drawingml` after the three frontiers: the `dml_*` fuzz targets, the wasm memory test and the wasm parity self-test, their CI steps and what they measured. Nothing in `crates/uo-drawingml/src` was touched. The only edits outside new files are in the four parity tests of the crate (the replay loops became callable functions, below), `uo-wasm` (`Cargo.toml` features, `lib.rs`, `tests/browser/run.mjs` forwards three more variables), `core/fuzz` (`Cargo.toml`, `src/lib.rs`, `src/bin/seed.rs`) and `.github/workflows/rust-core.yml`.

### Fuzz targets (`core/fuzz`, `src/drawingml.rs`, `fuzz_targets/dml_*.rs`)

Twelve targets, plain `fn(&[u8])` like the other crates', run by libFuzzer (`cargo fuzz`) and by the stable `fallback` driver; `TARGETS` lists them, so the CI fuzz job (it loops over every file of `fuzz_targets`) runs them for 60 s each with no change to its loop. Input decoding (`Input`): a number is a tag byte (0..24 a special value: ±0, NaN, ±∞, `f64::MAX/MIN`, the smallest normal and subnormal, ±1e9, ±1e300, 21,600, 100,000…; 24..96 an `i16`; 96..160 an `i32 / 16`; the rest 8 raw bytes, every bit pattern), so hostile values are one or two bytes away. Limits are small (`DML_LIMITS`: 2^16 mask pixels, kernel radius 48, 2^12 path points, 2^14 fill work, 24 guides per list, 2^12 image pixels) and `TIGHT_LIMITS` (selector bit 7: 600 px, radius 3, 40 points, 200 work, 3 guides, 64 px), so every limit row of the error table is reachable from a few hundred bytes. Every public call is wrapped in a 1 s bound (`timed`). The invariants are the ones the plan and the error table give, checked independently of the code:

| Target | Feeds | Invariants beyond no panic and the 1 s bound |
|---|---|---|
| `dml_preset` | preset name (defined, prototype names, `""`, `undefined`, strings), `w`, `h`, adjust lists (none, `[]`, values, values + names, names of another length) | `eval_preset`, `preset_guides` and `has_preset_def` agree on which names are defined; the geometry's `fill`/`stroke` are parallel to its subpaths; `Subpaths` `len`/`iter`/`get` agree; the built-in variables are always there; a text rectangle is never empty; a repeated call gives the same result (the crate keeps no state) |
| `dml_preset_path` | the same arguments | `PathShape.sub_lum` and `sub_stroke` are parallel to the subpaths; an outline always has at least one subpath and parallel flags; same result on a repeated call; with selector bit 0 the outline is also flattened |
| `dml_custom_guides` | `w`, `h`, an `av` and a `gd` list (operators incl. unknown ones, tokens like `0x1A`, `1e400`, U+00A0, names `""`/`"0"`), then the six numbers of an `arc_to_seg` | guides defined ≤ built-ins + 2 × `max_custom_guides`; the lists cut at the limit by the caller give the same value for every name and token; `arc_to_seg` is `None` exactly for `rx <= 0`, `ry <= 0` or a zero sweep |
| `dml_color` | a colour, up to 16 modifiers (all 16 kinds, unknown names, raw absent/NaN/±∞/huge) | unknown names have no kind; deterministic; when the last modifier that changes the colour is an HSL one every channel is NaN or in 0..=1; `lin_to_srgb(srgb_to_lin(v))` returns `v` within 1e-9 on 0..=1 |
| `dml_patterns` | a pattern by index or any string | the lookup equals the table, the density equals popcount / 64 (0.5 for an unknown name), no name listed twice |
| `dml_adjust` | one of the four adjusts, `w`, `h`, a buffer of exactly `4wh`, longer, shorter or of any length, two numbers | alpha and a trailing partial pixel are never written; `adjust_sharpen` follows the table (no-op, then `BufferSize`, then `Limit(ImagePixels)`) and an `Err` or a no-op leaves the buffer as it was |
| `dml_blip` | the effects, the picture adjust (alpha, duotone, colour change; hex strings with signs, `0x`, short, non-ASCII), pixels of any length | length unchanged; alpha never written by the effects, nor by the picture adjust without alpha or colour change; a trailing partial pixel untouched; `effect_decode` has 2 or 6 numbers and only when `effect_line` is `Some` |
| `dml_flatten` | up to 47 segments of every kind (M L C Q E Z, any coordinates), an affine, `tol`, a mask, even-odd or non-zero | `flatten_path`: `Err` is `Limit(PathPoints)`, polygons have ≥ 3 points and together at most `max_path_points`; `fill_polygons`: `Err` is `Limit(FillWork)` and leaves the mask as it was; with finite polygons and a clean mask the coverage stays in 0..=1 and never goes down |
| `dml_raster` | a mask up to 96 px (blocky regions, specials as coverage: 1.5, -1, NaN, ∞, -0), then blur, distance, morph, shift, warp or the `Mask::new`/`from_vec` limits | the expected variant of every `Err` is recomputed from the parameters (kernel over the limit incl. ∞, padded plane or target over `max_mask_pixels`, NaN radius on a non-empty mask); `sigma < 0.05` and `\|r\| < 0.01` are copies; NaN sigma is an all-zero mask; the distance plane is never negative or NaN and is 0 on the set; dilation never lowers and erosion never raises; a clean mask stays in 0..=1 |
| `dml_effects` | the four effect masks, the shadow transform, `align_point`, the reflection gradient | the mask has the size of the silhouette; only the errors of the calls it composes; outer and inner shadow over a silhouette past `max_mask_pixels` are `Limit(MaskPixels)`; for a clean silhouette and tame parameters the mask stays in 0..=1 |
| `dml_light` | rig, material, direction, angles; preset, `t`, `hw` | every table has the size of the default one; `light_at`/`light_face` are finite in 0..=6 for finite angles; `bevel_phi` is NaN or within ±90°; same bits on a repeated call |
| `dml_bevel` | a silhouette, a buffer of the right or a wrong length, `w`, `h`, preset, rig, direction, material, the Word flag | the table of the record: `!(w > 0.5)` is a no-op, then `BufferSize`, then `Limit(MaskPixels)` (padded plane, `w + 2` overflowing `u32`) or `Limit(KernelRadius)`; an `Err` or a no-op leaves the pixels as they were; alpha never changes; pixels where coverage is ≤ 0 keep their colour |

**Seeds** (`dml_seeds` in `core/fuzz/src/drawingml.rs`, written by the `seed` bin like the others, deterministic; `corpus/` is not versioned): 1,553 valid inputs of the layouts (every one of the 187 presets at a size and adjust set rotating through 6 sizes and 5 hostile sets, positional and by name, for the two preset targets; a program per guide operator and arcs with hostile sweeps; every modifier with raw values in and out of range and a sequence of all 16; every pattern and unknown names; every adjust on 7 shapes and lengths; the blip effects × picture adjusts; masks with every operation and limit; every effect; every light rig and bevel preset; the outlines of 63 presets flattened and filled) plus the empty input per target. A unit test of the fuzz crate runs every seed (`cargo test --lib --no-default-features --features fallback`).

**Runs** (libFuzzer: WSL Ubuntu 24.04, cargo-fuzz 0.13.2 on nightly, AddressSanitizer, overflow checks and debug assertions on, `-timeout=10 -rss_limit_mb=2048`, the 12 targets in parallel; the stable driver on Windows). The libFuzzer column is two runs of 660 s each (the second continues from the first's corpus, on the final build); the stable driver's is its last run (90 s each, mutations of the seeds, `--max-len 2048`; a call slower than 2 s would be saved as `slow-*`).

| Target | libFuzzer execs (2 × 660 s) | Coverage and corpus after | Peak RSS | Stable driver execs (90 s) | Slowest call (driver) |
|---|---|---|---|---|---|
| `dml_preset` | 2,914,398 | cov 1,017, 328 inputs | 419 MB | 4,339,867 | 12.7 ms |
| `dml_preset_path` | 2,704,033 | cov 1,584, 521 inputs | 447 MB | 2,388,554 | 8.4 ms |
| `dml_custom_guides` | 3,591,873 | cov 949, 1,132 inputs | 453 MB | 3,132,259 | 0.4 ms |
| `dml_color` | 65,186,328 | cov 764, 847 inputs | 549 MB | 66,181,508 | 2.3 ms |
| `dml_patterns` | 30,360,576 | cov 141, 41 inputs | 415 MB | 29,353,528 | 8.0 ms |
| `dml_adjust` | 1,821,412 | cov 584, 422 inputs | 412 MB | 3,514,743 | 6.9 ms |
| `dml_blip` | 43,941,244 | cov 582, 423 inputs | 533 MB | 9,085,125 | 0.4 ms |
| `dml_flatten` | 1,814,782 | cov 1,009, 704 inputs | 442 MB | 4,261,415 | 20.4 ms |
| `dml_raster` | 11,776,420 | cov 851, 475 inputs | 444 MB | 5,367,963 | 10.8 ms |
| `dml_effects` | 8,629,859 | cov 869, 494 inputs | 442 MB | 7,569,809 | 3.6 ms |
| `dml_light` | 120,256,405 | cov 321, 129 inputs | 616 MB | 80,001,824 | 1.0 ms |
| `dml_bevel` | 10,163,239 | cov 822, 582 inputs | 444 MB | 4,079,799 | 9.4 ms |
| **Total** | **303,160,569** | | | **219,276,394** | |

**Result: 0 crashes, 0 hangs, 0 out-of-memory in `crates/uo-drawingml/src`.** libFuzzer's slowest unit is under 1 s in every run (the stable driver's slowest call is 20.4 ms). A first stable-driver pass of 60 s each (236 M executions, before the final build) gave the same: no crash in any target but `dml_effects`, whose hits were false invariants of my own target (next paragraph). The ASan build can find little in a `#![forbid(unsafe_code)]` crate; what it adds here are the overflow checks and the debug assertions, which are on.

**Two false invariants of the target, found by the first pass and fixed in the target (the crate is not at fault, and the TS does the same).** (1) `outer_shadow_mask` returned NaN coverage for a scale `sx`/`sy` that is subnormal but not zero: the determinant of the shadow transform underflows, `invertAffine` divides by it (`det || 1e-12` only replaces an exact 0) and the inverse overflows to ∞, so the warp samples NaN (2,459 hits in 60 s). (2) The same for a box with coordinates around 1e303: the anchor translation cancels catastrophically in `back ∘ S ∘ K ∘ toA`. The target's "tame parameters" now exclude both (|value| < 1e6, scale 0 or ≥ 1e-30); NaN coverage out of a hostile shadow is an f32 store, not a panic, but whether the effect layer should refuse it is for the review. Neither value can come from the XML (`sx` is an integer percentage).

**For the security review (observations, not crashes; none is reachable with the fuzz limits).** Measured natively, release, while other jobs ran: `gaussian_blur` costs O(N × min(2r + 1, w)), so `max_kernel_radius` (4,096) is not a time bound: σ = 1,365 (r = 4,096) takes 0.13 s on 512², 1.0 s on 1024² and 9.1 s on 2048², about 5 minutes extrapolated to the 64 Mi pixel limit (the TS has no bound at all); `fill_polygons` at the provisional `max_fill_work` of 2^30 takes 4.1 s (8,192 rows × 131,072 edges) to 13.8 s (1,024 rows × 1,048,576 points) before it returns `Ok`; `morph` with r = 3,000 on a 1 Mpx mask pads to 49 Mpx: 2.1 s and 0.8 GB. In wasm, `flatten_path` into its 2^22-point limit peaks at 64 MB live and takes the module's memory from 28 MB to 124 MB (the polygon vector doubles: the old and the new block coexist), and a padded-plane `morph` at the wasm32 `max_mask_pixels` is 512 MB live (below). `max_fill_work`, `max_kernel_radius` × mask size and the wasm32 `max_mask_pixels` are the numbers the review decides.

### wasm memory test (`uo-wasm` feature `dml-memtest`, `src/dml_memtest.rs`, `tests/dml_memory.test.mjs`)

Test-only exports (never shipped), run in Node and in WebKit by the same file (`browser/run.mjs` now also forwards `UO_DML_SOAK`, `UO_DML_EVERY` and `UO_DML_BIG`). The soak is the scenario of the plan's M2: per round all 187 presets through `eval_preset`/`preset_path`/`preset_guides`/`preset_text_rect`/`preset_outline` at 3 sizes with defaults and a hostile adjust set, 1,000 colour sequences, 200 custom-guide programs, flatten + fill + distance of 20 outlines at 256 px, the four effect masks on a 512 × 512 silhouette (and the two error rows), `shade_bevel` on 256 × 256 (Word and rig), the four adjusts on a 1024 × 768 RGBA image (sharpen and soften), the blip effects and the patterns. The big effects run one call each on a disc of 1,024² and 2,048² pixels (and, with `UO_DML_BIG=1`, 2,896² and 3,464²: 8.4 and 12 Mpx) with the wasm32 limits (`Limits::DEFAULT`), measuring the peak live bytes above the start of the call (`uo_alloc`), the bytes allocated and `memory.buffer.byteLength` afterwards. The phone baseline is the proposal of G12: 128 MiB of live bytes per effect call (the iOS tab limit is not published; this is the record's number, not a measurement on a device).

| Check | Result |
|---|---|
| M2 soak, 200 rounds (Node 24.15, 221 ms per round on a quiet machine) | live bytes flat at **0 B** (sampled every 5 rounds); `byteLength` 28.25 MB after round 0 and unchanged to round 200; the long-lived inputs (discs, RGBA image) are 4.25 MB. WebKit 26.5, 60 rounds: the same numbers (470 ms per round) |
| Per round (ms, Node) | presets 11.5, colour 0.2, custom guides 0.3, flatten and fill 26, effects 512² 29, `shade_bevel` 6, adjusts 1024×768 146, blip and patterns 2 |
| Return to the start | `uo_dml_memtest_reset` brings the live bytes back to the start exactly; every big effect and worst case leaves 0 live bytes |
| Big effects, peak live bytes per call (the same in Node and WebKit, equal to the native M3 numbers) | **1.0 Mpx**: outer/inner shadow 12.0 MB, glow 16.4, soft edge 16.5, morph 16.4, blur 8.0, `shade_bevel` 16.1, sharpen 28.0, soften 24.0, distance 12.0; **4.2 Mpx**: 48.0, 48.0, 64.8, 65.1, 64.8, 32.0, 64.2, **112.0**, 96.0, 48.1. All inside the M3 budgets (asserted at +5%) and inside the 128 MiB phone budget |
| Over the phone budget (`UO_DML_BIG=1`, Node) | **8.4 Mpx**: glow 129.1 MB, soft edge 129.5, morph 129.1, `shade_bevel` 128.2, sharpen 224.0, soften 192.0 (outer/inner shadow 96.0, blur 64.0 and distance 96.1 are under); **12.0 Mpx**: every effect but blur (91.6) is over: shadows 137.4, glow 184.5, soft edge 184.9, `shade_bevel` 183.4, sharpen 320.4, soften 274.6, distance 137.4. The caller's own buffers come on top (the 12 Mpx RGBA is 48 MB) |
| `memory.buffer.byteLength` high-water, big effects alone (never shrinks) | after the 1.0 Mpx calls **37.9 MB**; after the 4.2 Mpx calls **150.4 MB** (sharpen: 112 MB peak + the 16 MB image + the disc); with `UO_DML_BIG=1` 8.4 Mpx **310.8 MB**, 12 Mpx **448.1 MB** |
| wasm32 limit rows | `Limits::DEFAULT` is the WASM32 set (2^25 mask and image pixels, radius 4,096, 2^22 points, 2^30 work, 2,000 guides). `Mask::new(8192, 8192)`, blur σ = 2,000, `morph` r = 1e5, `shade_bevel` on a mask of width `u32::MAX` and a fill of 70,000 edges over 65,536 rows return `Limit(MaskPixels)`, `KernelRadius`, `MaskPixels`, `MaskPixels` and `FillWork`, with 0 live bytes left and `byteLength` unchanged |
| Worst case per limit (wasm32 limits) | `flatten_path` into the 2^22-point limit: refused, **64.0 MB peak**, 128 MB allocated in total, the module grows 28.25 → **124.4 MB**; blur at the radius limit on 64 × 64: 0.06 MB; with `UO_DML_BIG=1`, `morph` to the 2^25-pixel padded plane (512 × 512 silhouette, r = 2,638): **512.05 MB peak**, the module grows to **636.4 MB** (the A2 case: the wasm32 `max_mask_pixels` is not phone-safe, G12) |

### wasm parity self-test (`uo-wasm` feature `dml-selftest`, `src/dml_selftest.rs`, `tests/dml_selftest.test.mjs`)

The replay loops of the four parity tests of the crate (`preset_parity.rs`, `frontier_a_parity.rs`, `light_bevel_parity.rs`, `raster_parity.rs`) became public functions that return the `Report` (`replay_presets(cap, every)`, `replay_custom_guides`, `replay_arc`, `replay_colour`, `replay_patterns`, `replay_adjust`, `replay_blip`, `replay_light_tables`, `replay_light_at`, `replay_bevel_phi`, `replay_shade_bevel`, `replay_flatten`, `replay_fill`, `replay_blur`, `replay_distance`, `replay_morph`, `replay_shift`, `replay_warp`, `replay_affine`, `replay_effects`); the `#[test]` functions now call them on the whole corpus (`every = 1`). No generator, digest or capture changed, and the 20 native tests pass as before. `uo-wasm` includes the four files with `#[path]` (one copy of every generator and digest, as the fonts self-test does), embeds the four captures with `include_str!` and exports `uo_dml_selftest_group_count/name/run`: the module recomputes the digests itself and the JS test only reads the counts. `every = n` replays the n-th item of every loop of a corpus (presets, blocks of custom guides and colour, images of the adjust grid, light keys, bevel groups (a group left out still counts its calls, because the call number seeds the pixels), raster parameters, silhouettes of the effects); the facets not replayed are not compared. The module is excluded from `cfg(test)` builds of `uo-wasm` (the included `#[test]`s would run a second time).

| Run | Result |
|---|---|
| Node 24.15, whole corpus (`UO_DML_EVERY=1`), 20 groups | **9,453 facets equal, 0 mismatches**, 15.5 s (presets 7,201 facets 6.3 s, `shade_bevel` 50 facets 3.9 s, flatten 975 2.9 s, adjust 196 1.0 s, light_at 24 0.65 s, the rest under 0.2 s) |
| Node 24.15, subset (`every = 4`, the default) | 2,773 facets equal, 4.5 s |
| WebKit 26.5 (Playwright 1.61), whole corpus | 9,453 facets equal, 20.7 s; subset: 2,773 equal, 6.4 s |
| Does it bite? | a throwaway copy of the crates with `0.29289` changed to `0.29290` in the octagon and `0.7152` to `0.7153` in `adjust_temperature`: the subset run fails in `presets` (6 mismatches) and `adjust` and names the facets; the copy was deleted |

Native, wasm and the frozen TS therefore agree bit for bit on the same inputs for the whole committed corpus. The facets that call `pow` (colour `shade`/`tint`, the sRGB curves, `adjust_saturation`) are computed in the module with the same `uo_jsnum::math::pow` port; the captures were taken on Linux with the pow settings of G2.

### CI (`.github/workflows/rust-core.yml`)

`wasm` job: builds `dml-memtest` and runs `UO_DML_SOAK=200 node --test crates/uo-wasm/tests/dml_memory.test.mjs`, builds `dml-selftest` and runs `UO_DML_EVERY=1 node --test crates/uo-wasm/tests/dml_selftest.test.mjs`; the WebKit step builds and runs both through `browser/run.mjs` (memory test with `UO_DML_SOAK=100`, self-test with the default subset). About 1 minute of tests in Node and 1 minute in WebKit on top of the builds. `fuzz` job: no change to the loop (it runs every file of `fuzz_targets`: 12 more targets at 60 s each, 12 minutes); the job name and the comment list the targets. The `lint` job already covers the new code (`--all-features`, native and wasm32); `core/fuzz` is not in that workspace and was linted on its own.

**Gate commands, from `core/`** (`CARGO_TARGET_DIR=$PWD/target/dml-ver`): `cargo fmt -p uo-drawingml -p uo-wasm -- --check` and, in `fuzz/`, `cargo fmt --check` clean; `cargo clippy -p uo-drawingml -p uo-wasm --all-targets --all-features -- -D warnings` (also with `--target wasm32-unknown-unknown`) and, in `fuzz/`, `cargo clippy --all-targets -- -D warnings` (default features, and `--no-default-features --features fallback`) clean; `cargo test --release -p uo-drawingml -p uo-wasm --all-features` green (the 20 parity tests among them); `bash tests/grep-bans.sh` and `bash tests/float-casts.sh` clean; `cargo build -p uo-wasm --features dml-memtest|dml-selftest --target wasm32-unknown-unknown --profile wasm` builds, and `node --test crates/uo-wasm/tests/dml_memory.test.mjs` (7 tests) and `dml_selftest.test.mjs` (22 tests) pass in Node and in WebKit (`UO_PLAYWRIGHT=… UO_WASM_PATH=… node crates/uo-wasm/tests/browser/run.mjs --browser webkit <file>`; `UO_WASM_PATH` must be a `.wasm` inside the repository). Fuzz: `cargo run --release --no-default-features --features fallback --bin seed`, then `… --bin fallback -- --secs 90 dml_preset …`, or `cargo +nightly fuzz run dml_preset` (in WSL: `cargo fuzz build`, then the binaries of the target directory with `-max_total_time=660 -timeout=10 -rss_limit_mb=2048`; a clock jump of the WSL VM at its start can make libFuzzer stop at once with "Done N runs in -2 second(s)": restart those).

**Not done.** The security review (the `max_*` values above are its decision); real Safari / iOS (gate 3); the wasm run of `fill_polygons` at the work cap and of `gaussian_blur` at the radius cap on a large mask (seconds to minutes: native numbers only); Chromium and Firefox runs; the whole-corpus self-test in WebKit in CI (a local run took 20.7 s; only the subset runs there); a mutation check of the fuzz invariants beyond the NaN finding above.

## odraw — OfficeArt (Opus brief, 2026-11-03)

uo-docx plan G11 (approved): the OfficeArt records of the binary formats ([MS-ODRAW]) live in
`uo_drawingml::odraw`, behind the feature `legacy` (`[features] legacy = []`; the D4 `.doc`
converter, and later the `.xls`/`.ppt` readers, enable it), next to the msospt → preset map
(this record's G6), which is `odraw::msospt`. Hybrid mode: one Opus agent wrote this brief and the
1:1 port of the Write side; the Slides and Sheet differences are named variants, implemented where
they are a few lines and verified against their own TS, declared (not implemented) where they are
reader logic of their own.

### Sources and placement

| TS (frozen `main`) | Lines | Rust | State |
|---|---|---|---|
| `write/packages/docx-core/src/doc/officeart.ts` | 280 | `odraw/{fopt,shape,bstore}.rs` | ported, verified |
| `doc/bytes.ts`, the record half (`readRec`, `recsIn`, `children`, `child`, `OA`) | ≈ 50 | `odraw/record.rs` | ported, verified; the `ByteReader` half stays with D4 |
| `doc/reader.ts` `loadDrawings`, `shape`, `backgroundShape`, `bse` | ≈ 40 | `odraw/word.rs` | ported, verified (it is OfficeArt, not `.doc`) |
| `doc/preset-map.ts` | 267 | `odraw/msospt.rs` | ported, verified with the Slides and Sheet variants |
| `slides-core/src/ppt/officeart.ts`, `ppt/preset-map.ts` | 376 + 274 | same modules, `Flavor::Slides`, `PresetVariant::WordSlides` | FOPT and BLIP variants implemented and verified; tree/masters/anchor declared below |
| `sheet-core/src/xls/officeart.ts`, `xls/odraw-geom.ts` (`presetGeomXml`, `hasPreset`, `rawAdjust`) | 400 + ≈ 150 | `Flavor::Sheet`, `PresetVariant::Sheet`, `OdrawLimits::SHEET` | FOPT, BLIP and preset variants implemented and verified; tree declared below |
| C# `DocBinary/OfficeArt.cs`, `PresetMap.cs` | 375 + 294 | type guide | `PresetMap.cs` reviewed: same table, defaults and `JsNumber.Round` |

Dependencies: `uo-opc` (`flate::inflate_capped`, the `inflateCapped` of the three readers, already
ported and verified there) and `uo-jsnum` (`math::{round, min, max}`, `conv::{to_int16, to_uint16,
truthy}`). `#![forbid(unsafe_code)]` and the crate's cast lints apply.

### API

```rust
// uo_drawingml::odraw (feature "legacy")
pub enum Flavor { Word, Slides, Sheet }                       // which reader's rules, where the copies differ
pub struct OdrawLimits { max_blip_bytes: u64, max_group_depth: u32, max_word_drawings: u32, max_shapes: u32, max_master_props: u32 }
impl OdrawLimits { const WORD; const SHEET; }                 // Default = WORD

// records (record.rs): everything borrows the stream ('a)
pub mod rt { DGG_CONTAINER, BSTORE_CONTAINER, DG_CONTAINER, SPGR_CONTAINER, SP_CONTAINER, FBSE, FSPGR, FSP, FOPT,
             CLIENT_TEXTBOX, CHILD_ANCHOR, CLIENT_ANCHOR, CLIENT_DATA, BLIP_*, SECONDARY_FOPT, TERTIARY_FOPT, … }
pub struct Rec<'a> { ver: u8, inst: u16, rec_type: u16, len: u32, hdr: u64, start: u64, end: u64, data: &'a [u8] }
impl Rec { is_container, bytes, children -> Recs, child(type), is_blip }
pub fn read_rec(data: &[u8], off: u64, limit: u64) -> Option<Rec<'_>>;
pub fn recs_in(data: &[u8], start: u64, end: u64) -> Recs<'_>;  // iterator, the sequence of recsIn

// properties (fopt.rs)
pub struct Prop<'a> { id: u16, value: u32, bid: bool, complex: Option<&'a [u8]> }
pub struct PropTable<'a>;   // JS Map order: get, value, contains, get_complex, set, iter, iter_complex, len
pub fn read_fopt<'a>(rec: &Rec<'a>, into: &mut PropTable<'a>, flavor: Flavor);
pub fn is_boolean_prop(id: u16) -> bool;  pub fn merge_booleans(base: u32, over: u32) -> u32;

// shapes (shape.rs)
pub mod fsp { GROUP, CHILD, PATRIARCH, DELETED, …, BACKGROUND, HAVE_SPT }  pub const PROP_HSP_MASTER: u16 = 0x301;
pub struct Shape<'a> { spid, shape_type: u16, flags, props: PropTable<'a>, fspgr, child_anchor, client_anchor,
                       client_data, client_textbox: Option<Rec<'a>>, last_anchor: Option<AnchorKind>,
                       children: Option<Vec<Shape<'a>>>, z: Option<u32> }
impl Shape { group_rect() -> Option<RectI32>, child_rect(), has_flag(f) }      // the Word/Slides `readRect`
pub fn read_sp_container<'a>(rec: &Rec<'a>, flavor: Flavor) -> Shape<'a>;
pub fn read_spgr_container<'a>(rec: &Rec<'a>, flavor: Flavor, l: &OdrawLimits) -> Shape<'a>;
pub struct ShapeMap<'a>;    // spid → shape, JS Map semantics (a repeated spid replaces in place)
pub fn index_drawing<'a>(dg: &Rec<'a>, into: &mut ShapeMap<'a>, l: &OdrawLimits);          // Word
pub fn apply_masters(m: &mut ShapeMap<'_>, masters: &mut BTreeSet<u32>, l: &OdrawLimits);   // Word

// Word's OfficeArtContent (word.rs): what DocReader.loadDrawings built
pub struct WordDrawings<'a> { bstore: Vec<Option<Bse<'a>>>, main: ShapeMap<'a>, hdr: ShapeMap<'a>, masters: BTreeSet<u32> }
impl WordDrawings { shape(spid, hdr), background_shape(), bse(pib) }
pub fn read_word_drawings<'a>(table: &'a [u8], fc: u32, lcb: u32, l: &OdrawLimits) -> WordDrawings<'a>;

// BLIP store and BLIPs (bstore.rs)
pub struct Bse<'a> { bt_win32: u8, size: u32, c_ref: u32, fo_delay: u32, embedded: Option<Rec<'a>> }
pub fn read_fbse<'a>(c: &Rec<'a>) -> Bse<'a>;  pub fn read_bstore<'a>(rec: &Rec<'a>) -> Vec<Option<Bse<'a>>>;
pub enum BlipKind { Png, Jpeg, Tiff, Bmp, Emf, Wmf, Pict }  // ext(), mime(): the TS strings
pub struct Blip<'a> { kind: BlipKind, bytes: Cow<'a, [u8]>, pict_size: Option<(i32, i32)> }
pub struct BlipBudget { left: u64 }  // document() = 512 MiB, unlimited()
pub fn decode_blip<'a>(rec: &Rec<'a>, budget: &mut BlipBudget, flavor: Flavor, l: &OdrawLimits) -> Option<Blip<'a>>;

// msospt → DrawingML preset (msospt.rs)
pub enum PresetVariant { WordSlides, Sheet }
pub struct PresetResult { prst: &'static str, flip_v: bool, /* av(): at most 2 */ }  pub struct PresetAdjust { name: &'static str, val: f64 }
pub fn preset_for(spt: u16, adjust: &[Option<f64>], w: f64, h: f64, v: PresetVariant) -> Option<PresetResult>;
pub fn is_word_art_type(spt: u16) -> bool;  pub fn is_connector_type(spt: u16) -> bool;  pub fn has_preset(spt: u16) -> bool;
pub fn raw_adjust(props: &PropTable<'_>) -> [Option<f64>; 10];   // `v | 0` of 0x147..0x150
```

Shape of the data: nothing is copied out of the stream (property data, records and raster BLIPs are
slices; a BLIP is owned only when inflated, prefixed (BMP, WMF) or cut by `cbSize` after an
inflate). The TS reads every field relative to the whole buffer, so a field past the end of its
record reads the next record's bytes and past the buffer reads 0: the Rust reads are the same
(`Rec.data` is the whole buffer). `PropTable` keeps `Map` order (append; replace in place) with a
linear search up to 16 entries and an index above (a FOPT holds up to 4,095 entries). D4 owns the
streams (`Vec<u8>` per CFB stream) and borrows `WordDrawings<'a>` from the table stream for the
conversion.

### The three copies, compared (named variants)

| Area | Word (`doc/officeart.ts`) | Slides (`ppt/officeart.ts`) | Sheet (`xls/officeart.ts`) | In the Rust API |
|---|---|---|---|---|
| `readRec`, `recsIn` | — | same | same rule written as `rec.end - rec.start < len` | one implementation |
| FOPT entries, `IMsoArray` size without the 6-byte header (LibreOffice), `0xFFF0` cells | — | same | same | one implementation |
| boolean set read twice (FOPT + TertiaryFOPT) | `mergeBooleans(prev, new)` whenever the new one is not complex (no `fUse` bit → previous value); the new entry (its `bid`) replaces | per-bit, only if neither is complex and the new value has an `fUse` bit; keeps the previous entry's `bid`; else replaced | as Slides | `read_fopt(.., Flavor)`: implemented, verified |
| where complex data goes | same table | same table | separate `complex` map; complex never replaces a simple value | `PropTable` keeps Sheet's complex entries apart (`get_complex`, `iter_complex`): implemented, verified |
| `SpContainer` | FSP, FSPGR, FOPTs, ChildAnchor (last wins) | + ClientAnchor (8-byte `i16` or 16-byte `i32` rect), ClientData, ClientTextbox; one `anchor` + `anchorKind` (last of the two) | FSPGR and ChildAnchor only when ≥ 16 bytes (a short one does not replace); ClientAnchor = cell anchor (≥ 18 bytes); `clientDataAt`; a deleted shape is `undefined` | `Shape` keeps the raw records and `last_anchor`; `group_rect`/`child_rect` are the Word/Slides read. **To add at the ports**: `client_rect_ppt`, `cell_anchor`, the Sheet ≥ 16 rule (accessors over the same records, no API change) |
| group tree | `readSpgrContainer` (no header → empty group with `fGroup`; deleted dropped; depth 64) | same | `readGroup`: no header → no group; header deleted → no group; depth 32; `readShapes` walks every `SpgrContainer` of every `DgContainer` of the concatenated MSODRAWING, header skipped | Word/Slides: `read_spgr_container`. **To add at the Sheet port**: `read_sheet_shapes` over `recs_in` + `read_sp_container` with `OdrawLimits::SHEET` |
| drawing and masters | `indexDrawing` (spid map, `z`) + `applyMasters`: one level, regardless of `fHaveMaster`, every property but `0x301`, type inherited, booleans `mergeBooleans(master, own)` | `readDgContainer` (first `SpgrContainer` as root, `fBackground` shape) + `resolveShapeMasters`: chain to depth 8, needs `fHaveMaster`, never `0x80/0x301/0x380/0x381/0x3A9`, per-bit with the master's `fUse` | `readDrawingDefaults` (the DggContainer FOPT) | Word: `index_drawing`, `apply_masters`, `read_word_drawings`. **To add at the Slides port**: `read_dg_container`, `resolve_shape_masters` (planned names, same types) |
| FBSE | `btWin32`, `size`, `foDelay`, embedded BLIP | + `cRef` | embedded only (by `pib`) | `Bse` has all fields |
| BLIP: DIB header | missing fields 40/24/0/0 | same | also a zero `biSize`/`biBitCount` → 40/24 (`\|\|`) | `decode_blip(.., Flavor)`: implemented, verified |
| BLIP: WMF | keeps an existing placeable header; units per inch from `ptSize` (`round((r − l) · 914400 / ptX)`, clamped to 1..65535) | always adds one, 1,440 | as Slides | same function: implemented, verified |
| BLIP: PICT | raw | `fitPictFrame(bytes, ptSize)` (Slides' `pict.ts`) | raw | `Blip.pict_size` carries `ptSize`; the fit stays in Slides (not compared: the Slides facet skips PICT) |
| budget | optional (`DocReader.blipBudget` 512 MiB) | optional | always 512 MiB per store; errors swallowed (`try/catch`); decoded BLIPs cached by `pib` | `BlipBudget`; Sheet's cache is the app's (M5 cap at its port) |
| group depth | 64 | 64 | 32 | `OdrawLimits::{WORD, SHEET}` |

**msospt, entry by entry** (the script evaluates every type 0..=255 and 4,095 over 40 cases of
adjusts and box sizes, 10,280 evaluations per copy, and compares): Write and Slides `presetFor` are
the same code (the files differ only in comments): **0 differences**. Sheet `presetGeomXml` differs
only in (a) the classic WordArt types (24–31, 136–175): `rect` without adjusts in Write/Slides,
no preset in Sheet (1,920 evaluations); (b) `w || 1`, `h || 1` applied inside Sheet, by the callers
in Write/Slides (68 evaluations with a 0, −0 or NaN side); (c) `flipV: false` vs absent (same
meaning); everything else equal (8,292 identical, 0 other). The tables (`P`, `DML_DEFAULTS`,
`binDefaults`) are identical in the three TS copies and in C# `PresetMap.cs`. Hence
`PresetVariant::{WordSlides, Sheet}`; Sheet's XML (`<a:prstGeom prst><a:avLst>…</a:avLst>`, always
with `avLst` open and closed) is the caller's (the parity builds it with `uo_jsnum::format::Js`).

**Next shared candidates** (not ported here; each with the first port that needs it): the freeform
geometry (`pVertices` 0x145 / `pSegmentInfo` 0x146 → custom geometry: Sheet `custGeomXml`, Slides
`shapeconv.ts`, Write's own in `docx-writer.ts`), the legacy arc (`arcGeomXml`), the WordArt warp
adjusts (`warpAdjust`) and the OfficeArt colour (`0x08xxxxxx` scheme/system indices) of the three
writers. They should land as `odraw::geom` and `odraw::color` with flavors, not in D4.

### Limits

| Limit | Value | Source | Effect |
|---|---|---|---|
| `max_blip_bytes` | 64 MiB | TS `MAX_BLIP` | a metafile inflating past it is dropped (`None`) |
| `BlipBudget::document()` | 512 MiB | TS `blipBudget` | cumulative per document; a refused BLIP spends nothing |
| `max_group_depth` | 64 (Sheet 32) | TS `MAX_GROUP_DEPTH` | deeper groups dropped; also the recursion bound |
| `max_word_drawings` | 16 | TS `guard++ < 16` | |
| `max_shapes` | 65,536 | **new** | shape containers past it are skipped (in one `read_word_drawings`, or one tree). An empty 8-byte `SpContainer` costs a 440-byte `Shape`: without it a 2 MB table stream builds ≈ 110 MB |
| `max_master_props` | 1,048,576 | **new** | `apply_masters` stops once it has added that many properties (one 4,095-property master named by 10,000 shapes would copy 41 M entries, ≈ 1 GB) |

Loops: every record step advances by ≥ 8 bytes; a FOPT has at most 4,095 entries (12-bit
instance); `read_word_drawings` reads at most 16 drawings; offsets are `u64` (no overflow, nothing
truncated). Nothing allocates from a size in the file before the data is there, except the inflate,
which is capped (uo-opc). The two new limits change results only on hostile input; they are the
listed differences of this module.

### Decisions outside the TS semantics (none changes a result on the corpus)

1. `max_shapes` and `max_master_props` (above).
2. BLIP bytes borrowed (`Cow`) where the TS copies (`slice()`): same bytes.
3. `recs_in` is an iterator; the `children(rec)` arrays are not built.
4. `spt` is `u16` (the 12-bit `recInstance`), the adjusts `f64` (`raw_adjust` gives the callers'
   `v | 0`).
5. `Blip.pict_size` (the Slides input) is returned for every flavor.
6. Sheet's separate complex map lives inside `PropTable` (boxed, created only by `Flavor::Sheet`).
7. When `max_shapes` runs out exactly at a group header the group is an empty shape (only at the
   limit).

### Parity (Node 24.15.0 x64, frozen TS `UO_TS_ROOT` = the `clickcompliance-docx` worktree)

`tools/capture-odraw.mjs` (deterministic: testkit xorshift32, `node:zlib` at fixed levels; run twice,
byte-identical) writes `tests/captures/odraw.capture` (571 KB), replayed by `tests/odraw_parity.rs`:

| Corpus | Records | Facets |
|---|---|---|
| the 11 `.doc` fixtures: `OfficeArtContent` at `fcDggInfo` through the real `DocReader` (the script checks that its copy of `loadDrawings` gives the same digest), every BStore entry decoded as `shapeBlip` does (embedded or at `foDelay` in WordDocument), the PICF walk of the Data stream (inline pictures: `readSpContainer`, `readFbse`, `decodeBlip`); Rust reads the streams with `uo_opc::cfb` and checks their SHA-256 | 11 | `streams`, `drawings`, `blips`, `inline` |
| generated contents (nested and deleted groups, duplicate spids, masters, backgrounds, FOPT/Secondary/Tertiary with booleans and arrays, every BLIP type and instance, stored/zlib/raw/garbage/truncated metafiles, short and long `cbSize`, placeable WMFs, overflowing records, a delay stream and a Data stream of PICFs, budgets from 0 to 512 MiB) | 100 | `drawings`, `blips`, `inline` |
| byte edits and cuts of the fixtures (24 each) and of the generated contents (2 each), in the table, delay or Data stream | 464 | same |
| FOPT blocks under Word `readFopt`, Slides `readFopt`, Sheet `readFoptBlock` | 250 | `word`, `slides`, `sheet` |
| single BLIPs under Word `decodeBlip` (budgets 0, 50, 300, 512 MiB), Slides `decodeBlip` (not PICT), Sheet `BlipStore` | 250 | `word`, `slides` (233), `sheet` |
| msospt 0..=255 and 4,095 × 40 cases | 257 + 40 | `word`, `slides`, `sheet` |

**Result: 3,990 facets equal, 0 mismatches, 0 accepted, nothing unreplayed, on the first run of
the replay.** It bites: six deliberate changes in the port, one at a time (units per inch × 2, the
Word merge rule dropped, the master type not inherited, `ss || 2` in the preset ratio, Sheet's
`biSize` default 41, Word's placeable test inverted) gave 50, 153, 3, 16, 4 and 13 mismatches; a
change of 1 in 914,400 is absorbed by `Math.round` on this corpus (no mismatch).

Commands (from `core/`, `CARGO_TARGET_DIR=$PWD/target/odraw`):
`UO_TS_ROOT=<frozen checkout> node crates/uo-drawingml/tools/capture-odraw.mjs`;
`cargo test --release -p uo-drawingml --features legacy --test odraw_parity`.

### Memory (measured, `tests/odraw_memory.rs`)

| Check | Result |
|---|---|
| M1 | every entry point returns live bytes exactly: `read_word_drawings` and the decode of every BLIP of the 111 documents, 250 BLIPs × 3 flavors × 3 budgets, 250 FOPT blocks × 3 flavors, every preset type × 2 variants, the cap and budget refusals |
| M2 | 200 rounds × (111 documents read and decoded + one BLIP): flat at 0 B after the first sample |
| M3 | fixtures: 2.0 KB allocated / 1.9 KB peak for the 219-byte contents; `formas.doc` (14,431 bytes, 8 + 3 shapes, masters) 18.9 KB / 10.1 KB; `degrades.doc` 14.2 KB / 8.9 KB; worst allocated / `lcb` over the generated corpus 28.1. A 3 MiB EMF inflate: 12.6 MB allocated, 4.7 MB peak (uo-opc's output growth); refused at a 1 MiB cap: 2.2 MB / 1.05 MB |
| M5 | no cache (`clear_caches` unchanged) |
| bounds | 100,000 nested `SpgrContainer`s read to exactly 64 levels; `max_shapes` and `max_master_props` tested at their edge |

### Security (continuous)

- No panic (`tests/odraw_nopanic.rs`, overflow checks on in debug): 120 rounds (600 in release) of
  edited and cut contents of all 111 documents at random `fc`/`lcb`, with the Word and small limits,
  every record re-read as FOPT, shape, group, FBSE, BStore and BLIP under the three flavors; 2,000
  random buffers; edited BLIPs and FOPT blocks; every `u16` type through the preset map with NaN,
  ±∞, ±0 and huge boxes and adjusts; each input under 2 s.
- Fuzz target `odraw_records` (`core/fuzz/src/odraw.rs`, in `TARGETS`, seeds from the fixtures and
  the capture through `odraw_seeds`): invariants: shapes ≤ `max_shapes`, deterministic, a refused
  BLIP spends no budget, an inflate spends ≤ `max_blip_bytes`, at most 2 adjusts, FOPT tables ≤
  `inst`, an embedded BLIP inside its FBSE. Stable driver 120 s: 12,706,027 executions, 0 crashes,
  slowest 10.3 ms. libFuzzer (WSL Ubuntu 24.04, nightly, `-s none`, overflow checks and debug
  assertions on) 300 s: 2,548,137 executions, 0 crashes.
- For the security review: the 512 MiB per-document inflate budget is cumulative and each BLIP may
  be 64 MiB at once (a phone budget question for D4's caller); the two new limits above.

### For the Slides and Sheet ports

Add (same types, no API change): Slides `read_dg_container`, `resolve_shape_masters`,
`client_rect_ppt`, its `fitPictFrame` on `Blip.pict_size`; Sheet `read_sheet_shapes`, `cell_anchor`,
the ≥ 16-byte rule for FSPGR/ChildAnchor, `read_drawing_defaults`, a capped `pib` cache. The parity
script already loads both TS readers; extend it with their `.ppt` samples (`slides/samples/legacy`,
`slides/samples/playback`, tracked) and generated `.xls` drawings.

### Not done

The Slides/Sheet tree and master functions above; a wasm run of the odraw tests (no wasm export
uses odraw yet: D4 adds the convert entry point); the Opus security review (milestone).

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
| 1 | planner (Opus) | 20.4 min | 64 | |
| 1 | reuse specialist (Opus) | 8.8 min | 58 | report: math probe `%TEMP%/uo-mathprobe` (Node 24.15 / V8 13.6, Windows) |
| 2 | analysis skeptic (Opus) | 15.4 min | 60 | 15 findings (A1 blocking for Sonnet readiness; A2 blocking for the switch) |
| 1 (revision) | planner (Opus): A1–A15 written in | 4.5 min | 38 | |
| 3 (phase 1) | porter: foundation (Sonnet) | not timed | ≈ 150 (estimate) | 0 rounds from a verifier yet; 12 decisions outside the plan (listed in "Stage 3 — port"); 3 own bugs found by my own tests/diff and fixed before the report: the custom-guide token list of the Rust corpus mirror had one token fewer than the script (caught by the differential), a wrong `pin` expectation in a trap test, `raster.rs` written with CRLF by a Windows text-mode write |
| 3 (phase 2, C) | porter: light3d + bevel (Sonnet) | not timed | ≈ 70 (estimate) | 0 rounds from a verifier yet; 8 decisions outside the plan (listed in "Phase 2, frontier C"); no bug of my own found by the differential (4 tests equal on the first run, mutation-checked); 2 test-side fixes before the report: a wrong assumption of mine in a unit test (`bevel_phi` with `angle` and NaN `t` is 45, not NaN, as in the TS) and `clippy::unit_arg` in two test files |
| 3 (phase 2, A) | porter: color + patterns + adjust + blip (Sonnet) | not timed | ≈ 75 (estimate) | 0 rounds from a verifier yet; 8 decisions outside the plan (listed in "Phase 2, frontier A"); 1 bug of my own (a `u64` overflow of `4 * w * h` that wrapped in release, found by the debug run of the no-panic test) and 1 design defect of my own (a per-call `Vec` in the brightness curves, found by the M3 measurement) fixed before the report; 7 wrong expectations of my own in the unit tests corrected against the TS (arithmetic slips, not port bugs) |
| 3 (phase 2, B) | porter: raster + effects (Sonnet) | not timed | ≈ 60 (estimate) | 0 rounds from a verifier yet; 10 decisions outside the plan (listed in "Phase 2, frontier B"; the one that touches an API is the `limits` parameter of `fill_polygons`); no bug of my own found by the differential (9 tests, 1,865 facets equal on the first run; a mutation check on 8 constants shows the replay bites); 1 test of mine wrong on the first run (the `flatten` error-path test used `star5`, a 10-point polygon, so the limit did not trigger: fixed in the test, not in the code); 7 hand-computed expectations of mine corrected after reasoning from the TS (e.g. the row of `ceil(ymax)` is merged and clamped, the 1-row blur is scaled by the centre tap) |
| 4 (continuous verification) | verifier: fuzz targets, wasm memtest and self-test, CI (Sonnet) | not timed | ≈ 165 (estimate) | no round from the Opus review yet; 0 findings in `src` (303 M libFuzzer and 219 M stable-driver executions of 12 targets, wasm soak flat at 0 B, 9,453 facets equal in wasm); 2 false invariants of my own `dml_effects` target found by its first pass (a subnormal shadow scale and a 1e303 box make NaN coverage in the TS too) and fixed in the target; 1 failure of my own wasm test (a NaN checksum of the guides part, fixed in the export); a WSL clock jump stopped three libFuzzer runs at once (restarted); the parity tests of the crate were refactored (loops into callable functions) and the 20 tests pass as before; clippy `manual_is_multiple_of` and a banned `f64::round` in the new code fixed |
| odraw (uo-docx G11): Opus brief + 1:1 port of the Write side, Slides/Sheet FOPT/BLIP/preset variants, TS differential (`tools/capture-odraw.mjs`), M1–M3, no-panic, fuzz target `odraw_records`, D4 frontier briefs in uo-docx.md | Opus (hybrid mode) | one session, ≈ 2 h | ≈ 95 | 0 rounds (no verifier yet); 0 mismatches on the first replay (3,990 facets); 3 own slips caught by my own tests before the record: a wrong expectation (chevron 13800 → 49,996 in the TS, the comment's 50,000 is the .pptx twin), out-of-range edits in the replay (a typed-array store past the end is ignored in JS), `&mut CONST` budget warnings (budget consts became `const fn`); 2 limits added beyond the TS (`max_shapes`, `max_master_props`) |
