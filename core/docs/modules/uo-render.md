# uo-render — the display list (items, resources, per-page codec, painter helpers)

Record of the rust-port workflow. Status: **architecture plan, milestone-1 gate pending** (hybrid
mode, `SKILL.md` "Hybrid mode"). Planner and reuse analysis together (one Opus agent, 2026-11-03),
written after the uo-docx-layout architecture (its K1–K20 and LQ1–LQ22) and the uo-docx G15. Under
the hybrid mode there is no separate analysis skeptic for this module: the milestone-1 skeptic and
security review cover it. Milestone 1 is the read path: uo-docx D1a → uo-docx-layout → **uo-render**
→ uo-pdf (viewer and server PDF).

Line counts are non-test source lines on `main` 0.9.x (frozen), counted in this worktree. Line
numbers are of `write/packages/docx-layout/src` unless a path is given.

## Scope

uo-render is the contract between every producer of pages (Write's layout now, Slides and Sheet
later) and every painter (the TS canvas painter of the shell, uo-pdf, later XPS/raster). It owns
three things: the **item vocabulary** (no Word concepts), the **per-page binary encoding** to the
TS shell (G15), and the **painter helpers** that both painters need and that are pure arithmetic or
rasters (no browser canvas).

### TS covered (behaviour source)

| TS | Lines | Rust (`uo-render`) |
|---|---|---|
| `tree.ts` items: `GlyphRunItem`, `RectItem`, `LineItem`, `ImageItem`, `PictureLook`, `ShapeItem`, `GradFill`, `PathCmdTw`, `PathItem`, `ClipItem`, `XformItem`, `BORDER_DASHES`; `LayoutPage` `widthTw/heightTw/background/backgroundFill/backgroundShow` | ≈ 190 of 275 | `types` (the Write-only rest of `tree.ts` — `LaidOutLine`, `LineParaFormat`, `PlacedCell`, `PlacedObject`, `ObjectFrame`, `section`, `number`, `hfItems` — stays in uo-docx-layout `output.rs`, LA5) |
| *(new, RQ1)* `geometry.ts` shape evaluation: `arcTo`, `poly`, `pin`, `presetPathLocal`, `OPEN_PRESETS`, `mapPath`, `presetPath`, `pathBounds`, `custPaths` (14–200, 476–520); `canvas.ts` `rotatePath`/`shapePath('line')` (553–573) and `pdf.ts` `rotatePath`/`linePath` (483–503) | ≈ 280 | `geom` |
| `geometry.ts` fills: `phi`, `hexRgb`, `gradientSamples`, `gradientGeometry`, `patternRaster`, `blipTiles`, `isRectGradient`, `rectGradientRaster` (202–354) | ≈ 150 | `paint::fill` (uo-drawingml G6 "fills … designed with uo-render") |
| `page-fill.ts` painter half: `tri`, `fract`, `near`, `fillParam`, `ramp`, `gradientTilePx`, `gradientTileRgba`, `patternRgba`, `hexRgb`, `pageFillTileTw` (61–175), and the `PageFillPaint` type (44–59) | ≈ 130 | `paint::page_fill` |
| `textfx.ts` painter half: `ResolvedFill`/`ResolvedTextFx` types, `gradOf`, `fxMarginPt`, `runBox`, `runOutlineSegs`, `fxLayers`, `dilate`, `dashPattern` (14–27, 77–231) | ≈ 165 | `paint::text_fx` (font outlines through `uo_fonts::Face`) |
| `canvas-picture.ts` / `pdf-picture.ts` near-duplicates: `bounds`, `shapeSubs`, `maskToCanvas`/`colored`, `DASH` (RQ10) | ≈ 80 | `paint::picture` — one copy only where a differential test proves the two TS copies equal |
| *(new)* canvas-side precomputation that needs a font or geometry: glyph batch joins (`batchable` + the advance test, canvas.ts:350–385), the F0xx symbol remap (375), outlines of `gid` glyphs (357–371), shape parts in twips (439–451), gradient geometry + 64 samples (506–510), run boxes of fx runs | ≈ 70 | `encode::canvas` |

Total ported ≈ **1,065 lines** of behaviour (the "275" of the uo-docx estimate table counted only
`tree.ts`), plus new code: the schema-driven codec generator (shared with the uo-docx session, G15)
and the TS decoder adapter.

### C# (type guide only)

`DocxLayout/Tree.cs`, `Geometry.cs`, `PageFill.cs`, `TextFx.cs` (ports of the same files; useful for
the optional fields). Nothing of the display list exists only in C#.

### Left for other modules

| Part | Goes to | Why |
|---|---|---|
| producing items: `paint.ts`, `geometry.ts` 356–475 (`substPhClr`, `withThemeStyles`, `shapeExtras`, `groupChild`), `picture.ts` `pictureLook`, `page-fill.ts` `resolvePageFill`/`imagePixelInfo`/`patternRowsFromImage`, `textfx.ts` `fxColor`/`fillOf`/`resolveTextFx`, `emf.ts`/`wmf.ts`, `cullOffPage` | uo-docx-layout (F3, F5, F7a) | model → items |
| the Write geometry section (lines, chars, pfmts, objects, cells, `hfItems`, `section`, `number`, the `ClipItem.src` map) | uo-docx-layout `output.rs` (K14), encoded by this crate's generator as an app section | Word-only |
| `pdf-navigation.ts` (computing links and outline) | uo-docx-layout F8 (`Layout::navigation`) | needs the model; the `Navigation` *type* is here |
| the PDF painter | uo-pdf | |
| masks, blur, bevel, presets, colour modifiers | uo-drawingml (done) | used as is |
| image codecs (`imagePixelSize` for blip tiles) | uo-image (uo-drawingml G5; uo-pdf PQ3) | the natural size reaches this crate as a number |

### Stays in the TS shell

`canvas.ts` (601) becomes a painter of **decoded** pages: same drawing code, fed by the generated
decoder (RQ5) instead of engine objects; `fontOf`/`image`/`color` of `EnginePaintEnv` keyed by
`FaceId`/`ImageId`; the font-dependent branches read precomputed hints. `canvas-picture.ts` (391)
keeps the canvas compositing (browser `drawImage` with smoothing, dashed strokes,
`destination-out`) and calls the mask functions through wasm exports (RQ12). `inline-renderer.ts`,
the dark-theme recolouring (`env.color`, `rgbaCanvasThemed`), `paintPageBackground`'s tile cache
(`fillTiles`, cap 32) and the browser `FontFace`s stay in the shell.

## Plan (milestone-1 gate — pending)

### 1. Boundaries and reuse

```
uo-jsnum ─┬─ uo-fonts (Face, outlines, COLR) ──┐
          ├─ uo-drawingml (masks, CustPath, ImageEffects) ─┼── uo-render (types, geom, paint, codec)
          └─ (no uo-docx, no uo-opc, no uo-image) ──────────┘        ▲            ▲
                                    uo-docx-layout (produces) ───────┘            │
                                    uo-pdf (paints from Rust values) ─────────────┤
                                    uo-wasm (encodes pages + paint exports) ──────┘ ──► TS decoder + canvas painter
```

- **Same in Sheet and Slides**: everything in this crate. The item set is what a slide or a sheet
  page draws too (glyph runs with absolute x, rects, lines, images with crop/rotation/picture look,
  DrawingML shapes, paths, clips, transforms). No item carries a ref, a run, a paragraph, a line
  number or a header/footer flag; those travel in the producer's app section (§3.4).
- **Not shared, and not placed here**: Word's line culling (`lineBox`, `lineNum`: RQ2), the
  header/footer layer split (`hfItems`), the EMF source of a clip (`ClipItem.src`).
- **Named variant inside a shared crate** (RQ1): `geom::compact_preset_path` is Write's own preset
  subset (`presetPathLocal`, ≈ 105 lines, arcs as cubic Béziers), different from uo-drawingml's 187
  presets that Slides uses. It is not a Word *document* concept, only a different evaluator, so it
  sits here as a named variant (`GeomEval::Compact` vs `GeomEval::Drawingml` for Slides later), as
  uo-drawingml keeps call-site quirks as named variants (its G4/G7).

### 2. Data model and ownership

#### RA1. Principles

1. **Plain data, no borrows of the model** (layout LA1.5): a `Page` and its `Resources` outlive
   edits and cross to JS only by encoding.
2. **Exact numbers**: every coordinate `f64` (LQ5), in the TS unit (twips, page top-left). Optional
   numbers stay `Option<f64>` wherever a painter distinguishes `undefined` from a value (`alpha`,
   `scaleX` — `it.scaleX ? …` in pdf.ts:334 and `it.scaleX || 1` in pdf.ts:323 treat 0 as absent,
   canvas `?? 1` does not —, `rot`, `fillAlpha`, `line.alpha`, crop). Optional booleans are `bool`
   (absent ≡ false: every painter tests them by truthiness; declared for the P1 canonical stream).
3. **Small items, side buffers** (SoA): a glyph is 12 bytes (`cp: u32`, `x: f64`; `gid` in a side
   column only when the page has one); big or rare payloads (`ShapeItem`, `PictureLook`, text effects,
   page fills, custom paths) are boxed or in per-layout tables. `size_of::<Item>() ≤ 48` is a test.
4. **Identity by id, bytes shared underneath**: faces by `FaceId` (the provider answer's identity,
   K2a), images by `ImageId` (the TS object identity, RQ3), strings by `Sym` (interned per layout).

#### RA2. Types (`uo_render::types`)

```rust
pub struct Page {
    pub w: f64, pub h: f64,                                  // widthTw, heightTw
    pub items: Vec<Item>,                                    // paint order (the TS `items`)
    pub glyphs: GlyphColumns,                                // cp: Vec<u32>, x: Vec<f64>, gid: Option<Vec<u32>> (NO_GID sentinel)
    pub path_cmds: Vec<PathCmd>,                             // PathItem ranges
    pub text: Vec<u16>,                                      // GlyphRun.text (P1 Canon / debug only, RQ7)
    pub background: Option<Sym>, pub background_fill: Option<PageFillId>, pub background_show: bool,
}
pub enum Item { Glyphs(GlyphRun), Rect(Rect), Line(Line), Image(Box<ImageItem>), Shape(Box<ShapeItem>),
                Path(PathItem), Clip(Clip), Xform(Xform) }
pub struct GlyphRun { pub face: FaceId, pub size_pt: f64, pub color: Sym, pub y: f64, pub glyphs: Range<u32>,
    pub scale_x: Option<f64>, pub text: Range<u32>, pub flags: RunFlags /* synth_bold, synth_italic, as_path, colr */,
    pub alpha: Option<f64>, pub fx: Option<TextFxId> }
pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub fill: Sym, pub alpha: Option<f64> }
pub struct Line { pub x1: f64, pub y1: f64, pub x2: f64, pub y2: f64, pub color: Sym, pub width_pt: f64, pub dash: Option<LineDash> }
pub enum LineDash { Dotted, Dashed, DashDot, DashDotDot, BdrDot, BdrDash, BdrDashDot, BdrDashDotDot, BdrDashSmall } // RQ13
pub struct ImageItem { pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub image: ImageId,
    pub crop: Option<Crop /* l t r b */>, pub rot: Option<f64>, pub flip_h: bool, pub flip_v: bool, pub pic: Option<Box<PictureLook>> }
pub struct ShapeItem { pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub geom: ShapeGeom,   // RQ1: the spec, not a path
    pub fill: Option<Sym>, pub stroke: Option<Stroke>, pub line: Option<[f64; 4]>, pub grad: Option<GradFill>,
    pub shadow: Option<ShapeShadow>, pub pattern: Option<Pattern>, pub blip: Option<Blip>, pub fx: Option<ShapeFx> }
pub struct ShapeGeom { pub geom: Sym, pub adj: SmallVec<[(Sym, f64); 2]> /* TS insertion order */, pub cust: Option<CustId>,
    pub flip_h: bool, pub flip_v: bool, pub rot: Option<f64>, pub fill_alpha: Option<f64>, pub line_alpha: Option<f64>,
    pub eval: GeomEval /* Compact (Write) | Drawingml (Slides, later) */ }
pub struct PathItem { pub cmds: Range<u32>, pub fill: Option<Sym>, pub stroke: Option<Stroke>, pub even_odd: bool }
pub enum Clip { Push { x: Option<f64>, y: Option<f64>, w: Option<f64>, h: Option<f64> }, Pop }
pub enum Xform { Push(Option<[f64; 6]>), Pop }   // a Push without `m` acts as a pop in both painters (canvas.ts:245, pdf.ts:469)
pub struct Resources {                            // per Layout; ids are dense, in first-use order
    pub symbols: SymbolTable,                     // UTF-16 strings: colours as the document wrote them, preset names, ext
    pub faces: Vec<FaceRes>, pub images: Vec<ImageRes>, pub text_fx: Vec<ResolvedTextFx>,
    pub page_fills: Vec<PageFill>, pub cust: Vec<Arc<[uo_drawingml::CustPath]>>,
}
pub struct FaceRes { pub key: Sym, pub family: Sym, pub bold: bool, pub italic: bool, pub stand_in: bool,
    pub face: Option<Arc<uo_fonts::Face>> /* TS `face.bytes` may be absent */, pub advances: Option<Arc<uo_fonts::AdvanceTable>> /* uo-pdf PQ1 */ }
pub struct ImageRes { pub bytes: Arc<[u8]> /* shared by content */, pub ext: Sym,
    pub effects: Option<uo_drawingml::ImageEffects> /* InlineImage.effects */, pub natural_px: Option<(u32, u32)> /* imagePixelSize */ }
pub struct Navigation { pub links: Vec<PageLink>, pub outline: Vec<OutlineEntry> }   // pdf-navigation.ts values, pt, as TS computes them
```

`PictureLook`, `GradFill`, `ResolvedTextFx`, `PageFill`, `Stroke`, `ShapeShadow`, `Pattern`, `Blip`
(`image: ImageId`, `tile`, `alpha`), `ShapeFx` mirror `tree.ts`/`textfx.ts`/`page-fill.ts` field by
field with `Sym` for strings and `Option` per optional field. `CustPath` (custGeom numeric paths) is
a DrawingML value: one type in `uo_drawingml` used by the uo-docx model and here (request, §12).

#### RA3. Ownership and memory scopes

| Scope | Holds |
|---|---|
| per Layout (retained with the layout) | `Vec<Page>`, `Resources` (faces as `Arc<Face>` pinned while referenced; image bytes `Arc<[u8]>` shared with the model's media) |
| per encode call | the output `Vec<u8>` (moves to the binding; JS copies and frees) |
| per binding view | the **delta watermark**: how many symbols/faces/images/fx/fills/cust entries this view already sent for the current layout generation (5 integers + generation) |
| per paint export call | the RGBA/mask buffer returned (moves out) |

No caches in the crate (`clear_caches()` is a no-op, kept for M1).

### 3. Codec (G15)

#### 3.1 Generator

**Own schema-driven generator** (G15 recommendation, confirmed here): one schema language →
Rust types' encode/decode and a TS encoder/decoder. Shared with the uo-docx session `exec`/`query`
(so it is designed for both now, RQ4).

- **Schema files** `core/schema/*.uos` (a small IDL: `struct`, `enum` with payload variants,
  `option`, `list`, `columns` (struct-of-arrays), `sym` (interned string), `utf16`, `bytes_ref`
  (handle, never inline), `f64`, `u32`/`i32` (varint/zigzag), `bool`, `app_section`).
- **Generator** `core/tools/uo-schema` (workspace bin crate, `publish = false`, never shipped): emits
  `crates/uo-render/src/codec/gen.rs` and `core/crates/uo-wasm/js/gen/render.ts` (and later
  `session.ts`). Generated files are committed; CI regenerates and fails on a diff (the uo-text
  regeneration-diff pattern).
- **Wire format**: little-endian; LEB128 varints for counts, ids, enum tags (zigzag for signed);
  `f64` as 8 raw bytes (bits preserved: `-0`, NaN payloads canonicalized by uo-jsnum `conv` before
  encoding, since JS cannot keep them anyway); `bool`s and option presence packed in one bitmap per
  struct; strings as UTF-16 code-unit count + `u16`s (lone surrogates kept); `columns` encoded
  column by column (all `cp`, then all `x`); lists bounded by the remaining bytes before allocation.
- **Versioning**: `SCHEMA_HASH` (SHA-256 of the normalized schema, first 8 bytes) in every message
  header and compiled into both the wasm and the TS decoder; a mismatch is a typed `VersionMismatch`
  error (a stale CDN pair), never a misread. No in-place evolution in 0.10: the wasm and the decoder
  ship in one npm package.

#### 3.2 Per-page message

```
header   magic "UORP", SCHEMA_HASH, generation (layout id in the view), page index
delta    new symbols | new faces (key, family, bold, italic, stand_in, has_bytes) | new images
         (ext, natural_px, byte length — bytes pulled separately, §4) | new text fx | new page fills
render   page (w, h, background, fill id, show) + items + columns, canvas-oriented (§4.2)
app      producer section (Write: lines, chars, pfmts, objects, cells, hfItems, section, number,
         clip-src map — defined in uo-docx-layout, encoded by the same generator)
```

- **Resource-table deltas**: a page carries only the resources the view has not sent in this
  generation; the JS tables grow in pull order (pages may be pulled in any order — scrolling). A new
  `Layout` bumps the generation; the decoder drops its tables on a new generation. Ids are dense, so
  the delta is "entries from index n".
- **Encoded on demand** (layout LA5): only the pages being painted; the Rust `Page` stays the
  source; encodings are not cached in Rust (the shell may keep decoded pages; that cache is the
  shell's, capped).

#### 3.3 What is encoded for the canvas (the encoder is painter-specific)

The PDF painter reads the Rust `Page` directly (no codec). The encoding exists for the canvas, so
the encoder precomputes, **in the canvas units and the canvas arithmetic**, what the canvas would
have asked a font or a geometry function for (the shell has neither after the switch):

| Canvas use today | Encoded instead |
|---|---|
| `presetPath`/`custPaths`/`shapePath('line')`+`rotatePath` in twips (439–451) | shape parts `{cmds, fill, stroke, evenOdd}` in twips, computed by `geom` with the canvas's arguments |
| `pathBounds` + `gradientGeometry` + `gradientSamples(stops, 64)` (506–510) | `GradGeom` + 64 RGB samples |
| `font.glyphId`/`advanceWidth` for batching (350–385) | one "joins previous glyph" bit per glyph of plain runs (computed with the TS rule; used only when `'fontKerning' in ctx`, as today) |
| F0xx symbol remap (375) | the drawn code point, as a sparse list of (glyph index, cp) |
| `font.glyphOutline(gid)` (357–371) | outlines of `gid` glyphs in font units + `upem` (rare: EMF) |
| `runBox(it)` for fx runs (289) | the run box |
| `fxLayers`, `patternRaster(…, 15)`, `rectGradientRaster`, page-fill tiles | **not encoded**: zoom-dependent rasters through item-addressed wasm exports at paint time (§5) |

Everything else is the TS value as is (colours as interned strings, RQ6).

#### 3.4 App sections

The envelope carries one opaque, schema-typed producer section per page. Write's section is defined
by uo-docx-layout (LA5, K14) with this generator; the shell's Write code reads it, the generic
painter never does.

### 4. TS decoder contract and painter boundary

#### 4.1 Contract (RQ5)

- `decodePage(buf, tables) → { page: LayoutPageLike, write: WriteSection }`: plain JS objects with
  the **`tree.ts` field names** (`t: 'glyphs'`, `glyphs: {cp, x, gid?}[]`, …), materialized for the
  pages being painted and dropped after (the canvas painter changes stay small: font and image by
  id, the hints of §3.3, shape parts instead of `style`). `tables` holds the resource deltas.
- `EnginePaintEnv` becomes `{ fontOf(faceId), image(imageId), color(hex) }`. The shell pulls image
  bytes once per `ImageId` (`image_bytes(view, id)`, copied, freed after the browser decode) and font
  bytes once per face it must register as a `FontFace`.
- Errors are typed (`DecodeError`: version, truncated, bad id); the painter keeps its
  `try { paintItems } catch` in the viewer (universal-document-viewer.ts:931).
- Shell caches keyed by object identity today (`fxCache` and `adjusted`/`cache` `WeakMap`s of
  canvas.ts:268 and canvas-picture.ts:35, 121) move to keys by (`ImageId` or page+item, ppt) with
  caps, or every repaint rebuilds the layers (performance, not results).

#### 4.2 Colours (K14, answered)

The display list keeps **the raw document strings** (interned); there is no "reference painter":
each painter keeps its own parse, per field, exactly as today. The table is the contract both new
painters follow (verified in canvas.ts and pdf.ts):

| Field | Canvas (TS shell) | PDF (uo-pdf) |
|---|---|---|
| page `background` | `#${bg}` assigned without a test (invalid CSS is ignored by the canvas) | `hexColor` if non-empty (pdf.ts:166) |
| `rect.fill` | skip unless `HEX6` (131) | `hexColor`, no test (345) |
| `line.color` | skip unless `HEX6` (159) | `hexColor`, no test (351) |
| glyph run `color` | skip unless `HEX6` (320), except fx modes | `hexColor`, no test (333, 291) |
| COLR layer colour | — (browser draws colour fonts) | `hexColor(L.color ?? it.color)` (269) |
| `shape.fill`, `shape.stroke.color` | `HEX6` (435–436) | `HEX6` (379–380) |
| `shape.shadow.color` | `HEX6` (454) | `hexColor`, no test (405) |
| `path.fill` / `path.stroke.color` | `HEX6` / `HEX6` (223–224) | `HEX6` / `hexColor`, no test (460–461) |

`hexColor` is `parseInt(h.slice(i, i+2), 16)` per pair after removing the first `#`: `'auto'` gives
`[10, NaN, NaN]/255` (`'au'` parses as `a`), written as `0.039 0 0` by `fmt` (NaN → `0`). uo-pdf
reproduces it with `uo_jsnum::parse` over UTF-16 code units.

#### 4.3 Painter-specific constants (kept per painter, not unified)

Synthetic italic: canvas transform skew −0.21 and outline skew 0.21 (canvas.ts:344, 361); PDF text
matrix `0.21` (pdf-writer/document.ts:171) but outline skew **0.2** (pdf.ts:203, 256). Synthetic
bold: canvas stroke `sizePt/35·20`, PDF `Tr 2` with `sizePt/35`, as-path stroke `sizePt·0.03`
(pdf.ts:292). Pattern raster: canvas `px = 15, k = 1`, PDF `px = 0.75 pt, k = 4`; blip tiles: canvas
`emu = 1/635`, PDF `1/12700`; rect gradient: both `w/20` and `ppu = 2`; shape gradient samples:
canvas 64, PDF 512 (256 for `path`). These are painter decisions with their own oracle evidence; the
helpers take them as parameters.

### 5. Painter helpers (`uo_render::geom`, `uo_render::paint`)

- `geom` (RQ1): `compact_preset_local(prst, w, h, adj)`, `preset_path(spec, x, y, w, h)`,
  `cust_paths(cust, x, y, w, h, xf)`, `line_path`, `rotate_path`, `map_path`, `path_bounds`,
  `is_open_preset`, `shape_parts(geom, x, y, w, h, line, unit)` — each painter calls them in its own
  unit (canvas twips via the encoder, PDF points), so both get the TS bits.
- `paint::fill`: `gradient_samples`, `gradient_geometry`, `pattern_raster` (limited, §8),
  `blip_tiles` (TS cap 4,000 kept), `is_rect_gradient`, `rect_gradient_raster` (TS cap 2,000² kept).
- `paint::page_fill`: `fill_param`, `gradient_tile_px`, `gradient_tile_rgba`, `pattern_rgba`,
  `page_fill_tile_tw`.
- `paint::text_fx`: `grad_of`, `fx_margin_pt`, `run_box(face, run, glyphs)`, `run_outline_segs`,
  `fx_layers` (TS cap 2.4·10⁷ px kept, `Float32Array` accumulation as `f32` storage), `dilate`,
  `dash_pattern`.
- `paint::picture`: `effect_bounds`, `shape_subpaths`, `colored_mask` (RQ10).
- **Wasm exports for the canvas** (uo-wasm, designed here): item-addressed, so no item is marshalled
  back: `paint_fx_layers(view, page, item, ppt)`, `paint_pattern(view, page, item, px)`,
  `paint_rect_gradient(view, page, item, ppu)`, `paint_page_fill_tile(view, page, w, h)`,
  `paint_pattern_tile(view, page)`; plus the uo-drawingml mask calls `canvas-picture.ts` makes
  (`presetOutline`, `flattenPath`, `fillPolygons`, shadows, glow, soft edge, `reflectionAlpha`,
  `gaussianBlur`, `shadeBevel`, `morph`) over `Float32Array`/`Uint8Array` buffers (RQ12). Buffers
  move through `uo_alloc_bytes`.

### 6. Public API

```rust
// uo_render
pub mod types;  pub mod geom;  pub mod paint;  pub mod codec;
pub fn encode_page_for_canvas(page: &Page, res: &Resources, app: &dyn AppSection, sent: &mut DeltaMark, out: &mut Vec<u8>);
pub fn decode_page(buf: &[u8], tables: &mut DecodeTables) -> Result<DecodedPage, DecodeError>;  // tests, fuzz, tools
pub fn clear_caches() {}
```

- **Changed from 0.9** (CHANGELOG at the switch): `LayoutResult` items reference faces, images,
  text effects and page fills by id; `ShapeItem.style` (a docx-core `ShapeStyle`) becomes
  `ShapeGeom`; `ImageItem.image`/`ShapeItem.blip.image`/`ClipItem.src` (model objects) become ids;
  `lineBox`/`lineNum` leave the item (RQ2); `ClipItem.src` moves to the Write section; the painter
  env takes ids; `paintItems` takes decoded pages.

### 7. Crate vs own port

| Need | Candidates | Recommendation |
|---|---|---|
| Codec | `flatbuffers` (Apache-2.0, zero-copy, external `flatc`, vtable overhead), `capnp`, `prost` + `protobufjs` (JS runtime ≈ 70 KB), `postcard`/`bincode` (no TS side) | **own generator** (G15): exact control of f64 bits, UTF-16, columns and symbol deltas; no runtime in the shell; one schema for the session too |
| Path geometry | `kurbo`, `lyon` | **own** (TS arithmetic order; uo-drawingml decided the same) |
| Interning | `string-interner` | **own** `SymbolTable` (UTF-16 keys, dense ids in first-use order, needed by the deltas) |
| Small vectors | `smallvec` (MIT/Apache) | use, as uo-docx |

`#![forbid(unsafe_code)]`, `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]`.

### 8. Parity

| Capture | Where | Compared |
|---|---|---|
| **R-P1 items** | the layout's P1 (uo-docx-layout §5) compares every item field through `Canon` | exact bits |
| **R-P2 codec** | (a) Rust encode → Rust decode = input (property tests: NaN/−0/±∞, empty and lone-surrogate strings, empty pages, 10⁵ glyphs); (b) Rust encode → TS decode → TS encode → **identical bytes** (corpus + random); (c) the **TS adapter**: the frozen TS `LayoutResult` → generated TS encoder → Rust decode → `Canon` = the TS walker stream | exact |
| **R-P3 helpers** | TS facets of every moved function, inputs and outputs (as uo-drawingml did): `presetPath`/`custPaths`/`linePath`/`rotatePath` in **both units**, `gradientSamples`, `gradientGeometry`, `patternRaster` (both parameter sets), `blipTiles`, `rectGradientRaster`, page-fill tiles, `fxLayers` (with the face bytes), `runBox`, `runOutlineSegs`, `dashPattern`, the picture helpers | exact (RGBA bytes, f64 bits) |
| **R-P4 canvas painter** (LQ6) | (a) Node: the frozen `paintItems`/`paintPageBackground` over the TS items **vs** the new painter over decoded pages, with a recording `CanvasRenderingContext2D` (every call and property set, f64 args by bits; a transform stack so `getTransform` answers as a real context; `OffscreenCanvas`, `DOMMatrix`, `createImageData`/`putImageData` stubbed and digested; `image()` returns a token per image identity; `fontOf` fixed; `'fontKerning' in ctx` run both ways; shell caches cleared per page); (b) **Chromium pixel-exact** (Playwright) old vs new for the paths that read pixels back (`canvas-picture.ts`, fx layers, page fills) over the pictures/texteffects/page fixtures and a corpus sample | log equality; pixel equality |

- **The TS adapter (R-P2c) decouples** uo-render and uo-pdf from the layout port: both reach
  parity on TS-produced layouts before uo-docx-layout passes P1.
- Corpus: the layout corpus (2,742 oracle docx locally with Office fonts, regenerated in the gate run
  — LQ21; the 7 .NET synthetic inputs; 2,000 generated documents with open fonts in CI).

### 9. Memory plan (measured gate)

- **M1**: `encode_page_for_canvas`, `decode_page` (ok and each `DecodeError`), every `geom`/`paint`
  function and every paint export (ok and each limit error), `SymbolTable` growth.
- **M2 soak**: 1,000 × (encode every page of 20 corpus layouts incl. the 500-page stress → decode →
  drop), sampled every 50, flat, **0 B** after drop; wasm: `memory.buffer.byteLength` flat after 200
  iterations of encode + paint exports.
- **M3 budgets** (first record, replaced by measurements): retained `Page` ≤ 0.5 × the TS
  `LayoutPage` heap delta (glyph 12 B vs ≈ 50 B as a JS object in an array); encoded page ≤ 1.5 × its
  in-memory size; per-page decode on the JS side ≤ the TS page objects it replaces (transient);
  `fx_layers` and rasters as the TS (their caps kept).
- **M4**: encoded page buffers and paint-export buffers, 10,000 × create/free and the finalizer path.
- **M5**: no Rust cache; the delta watermark is bounded by the layout; the shell's caches (decoded
  pages, `fillTiles` 32, `shapeCache` 32, picture layers 8 per image, the new fx layer map) each get
  a cap test in the shell suite.
- **M6**: no `unsafe`.

### 10. Security plan

The encoded bytes are produced by our own wasm, but the decoders must not trust them (version skew,
corruption, a future second producer):

- **Decoders** (Rust and TS): every count checked against the remaining bytes before allocation;
  ids checked against the tables (`BadId`); no recursion in the render schema; version check first.
- **Helpers on hostile values** (coordinates from untrusted documents): `pattern_raster` has no cap
  in the TS (a 10⁹-twip shape allocates W·H·4 bytes and the TS runs out of memory) → `PaintLimits`
  `max_raster_px` (default 2.4·10⁷, the `fxLayers` cap) → `Err(Limit)`, accepted by name;
  `blip_tiles` 4,000 and `rect_gradient_raster` 2,000² as the TS; page-fill tiles bounded by the tile
  size floor (uo-pdf PQ11).
- **Fuzz targets**: `render_decode` (arbitrary bytes, Rust decoder), `render_roundtrip`
  (structure-aware random `Page` + `Resources`, encode/decode equality), `render_geom` (hostile specs:
  NaN/∞ sizes, huge adj, 10⁵-command custom paths), `render_paint` (rasters with hostile sizes),
  plus the TS decoder under `fast-check` (never a non-`DecodeError` throw, never an allocation above
  16 × the input).

### 11. JS semantics and risks

`parseInt` hex pairs (NaN → `0` via `|| 0` in page-fill.ts:155, NaN kept in geometry.ts:208 and
reaching `Math.round` → typed-array store), `Uint8Array` (wrap) vs `Uint8ClampedArray` (clamp) stores
through `uo_jsnum::conv`, `Float32Array` accumulation in `fxLayers` (`f32` storage), `Math.*` via
uo-jsnum, `Record` key order for `adj` (insertion order; integer-like names first, none in practice),
`String.fromCodePoint` of lone surrogates (RQ9: `u32`, not `char`), UTF-16 code units in `slice`, `-0` in coordinates
(kept by the codec), `JSON.stringify` cache keys (shell only).

### 12. Frontiers, models and order

| # | Frontier | Content | Lines | Model | Depends on |
|---|---|---|---|---|---|
| T | types | `types` (RA2), `Resources`, `Navigation`, `SymbolTable` | ≈ 190 + new | **Opus**, inside uo-docx-layout F0 (it fixes the item types with the layout planner) | uo-fonts, uo-drawingml |
| C | codec | schema IDL, generator (Rust + TS emitters, runtime helpers), render schema, R-P2, fuzz | new ≈ 1.5–2k | **Sonnet** (the wire format and IDL are fixed here and in RQ4); the session schema is added by uo-docx D2a | T |
| G | geometry + paint helpers | §5 (`geom`, `paint::*`) + R-P3 facets | ≈ 805 | **Sonnet** (closed scope, pure functions) | T |
| E | canvas encoder | §3.3 precomputation, app-section hook, wasm paint exports | ≈ 70 + new | **Sonnet** | C, G |
| D | TS shell side | decoder adapter, `canvas.ts` on decoded pages, `canvas-picture.ts` on exports, caches by id, the TS adapter encoder of R-P2c | TS ≈ 600 touched | **Sonnet** (TS) | C, E |
| V | painter parity | recording context, log comparison, Chromium pixel check | new | **Sonnet** | D |

Order: **T** (with layout F0) → **C ∥ G** → **E** → **D** → **V**. C's TS adapter (R-P2c) is what
uo-pdf's isolated parity needs, so C goes first after T. Security review and the final skeptic run
once for milestone 1 (Opus). Sonnet frontiers run with the parity and memory verifiers on top; every
round sent back is recorded in "Process metrics".

### 13. Estimate

| Part | Hours (agent time, through gate 2) | Measured or assumed |
|---|---|---|
| T types (beyond what layout F0 already counts: `Resources`, `Navigation`, codec annotations) | 1–2 | assumed |
| C generator + render schema + round-trip/cross-language tests + fuzz | 8–12 | assumed (new code, ≈ 1.5–2k lines, two emitters) |
| G helpers port (≈ 805 lines) + facets | 5–7 | **measured basis**: uo-opc Opus port ≈ 1,300 lines/h port-only; Sonnet ports elsewhere had 0 parity rounds (uo-fonts, uo-opc, uo-drawingml); assumed 400–600 lines/h for Sonnet plus the facet harness |
| E canvas encoder + paint exports | 4–6 | assumed |
| D TS decoder, painter adaptation, TS adapter encoder | 6–9 | assumed |
| V recording context, logs, Chromium pixel check, rounds | 6–9 | assumed |
| memory and security verification | 3–5 | **measured basis**: uo-fonts stage 4 ≈ 160 min for 2.7k lines; scaled |
| **working band** | **≈ 33–50 h (midpoint ≈ 41)** | the uo-docx table's "275 lines" counted only `tree.ts`; this adds the codec, the painter boundary and its harness |

Critical path ≈ 20–28 h (T → C → E → D → V); not on the layout's critical path once T is done (the
TS adapter feeds parity). Not included: uo-wasm binding plumbing beyond the paint exports, the
editor's (milestone 2) use of the decoded pages, real-Safari runs, maintainer latency.

## Milestone-1 gate — questions (RQ1–RQ13)

★ = needed before its port starts (T is inside layout F0, so RQ1–RQ3 are needed before F0 closes).

| # | Question | Recommendation | When |
|---|---|---|---|
| **RQ1** ★ | Shape geometry is **unit-dependent**: the TS canvas evaluates `presetPath`/`custPaths` in twips (canvas.ts:448–450), the TS PDF in points (pdf.ts:397–399), and `(w/20)·a` is not `(w·a)/20` in bits. They are used **only by the painters**, not by the layout. Revises layout LA5/LQ6 ("`ShapeItem.style` becomes resolved geometry") | Items carry the **spec** `ShapeGeom`; `presetPathLocal`, `presetPath`, `custPaths`, `mapPath`, `pathBounds`, `OPEN_PRESETS`, `arcTo`, `linePath`/`rotatePath` (≈ 280 lines) move from layout F3 to `uo_render::geom` as the named variant `GeomEval::Compact`; each painter evaluates in its unit (the canvas through the encoder). Layout F3 keeps `withThemeStyles`/`shapeExtras`/`groupChild` | ★ before layout F0 closes |
| **RQ2** ★ | `lineBox` (cull group) and `lineNum` are read **only** by `cullOffPage` (engine.ts:4000–4038), never by a painter or the shell | They leave the shared item and the encoding: a layout side table (`ItemMeta`) compared by the layout `Canon` only. `hfItems` and `ClipItem.src` go to the Write section (as K14) | ★ before F0 closes |
| **RQ3** ★ | Image identity: the TS PDF embeds per `InlineImage` **object** (pdf.ts:33 `WeakMap`; one per drawing, even for the same media part; derived EMF bitmaps per creation), while layout K12 dedupes derived images by content | `ImageId` = the TS object identity (one per model drawing image, one per derived creation, one per pass for the page fill); the **bytes** are shared by content underneath (K12's memory win kept). uo-pdf keys on `ImageId` → strict structural parity. Content dedup in the PDF = stage-5 candidate (changes results) | ★ before F0 closes |
| **RQ4** ★ | G15 codec details | Own generator `core/tools/uo-schema`, IDL `core/schema/*.uos`, generated Rust + TS committed with a CI regeneration diff; LE, LEB128/zigzag, raw f64, UTF-16, presence bitmaps, `columns`, `sym` with per-generation deltas, `bytes_ref` handles; `SCHEMA_HASH` check; v1 covers what the uo-docx session needs (payload enums, nested lists) | ★ before C (and before uo-docx D2a) |
| **RQ5** ★ | TS decoder shape | Materialize per-page plain objects with the `tree.ts` field names (decode on paint, dropped after), so `canvas.ts` keeps its code; the alternative (zero-copy views over the buffer) saves GC but rewrites the painter and its parity | ★ before D |
| RQ6 | Colours (K14) | Raw strings interned; **each painter keeps its own parse per field** (table §4.2); no reference painter | T |
| RQ7 | `GlyphRun.text` has no painter or shell reader (searched viewer and editor) | Kept in Rust (UTF-16 side buffer, ≈ 2 B/char) for the P1 `Canon`; not encoded unless a debug flag asks | T |
| RQ8 | Painter parity method (LQ6) | Node recorded draw-call log (all of `canvas.ts`) + Chromium pixel-exact for the pixel-reading paths; the log is the gate, pixels a second facet | V |
| RQ9 | Glyph code points: they come from `codePointAt` over the run text (`w:sym` is stored through `String.fromCharCode`, 16 bits, docx-core document.ts:1967–1976; EMF text is 16-bit), so they are always ≤ 0x10FFFF, but **lone surrogates occur** and `String.fromCodePoint` accepts them (canvas.ts:376, ToUnicode font.ts:138) | `cp: u32`, never `char`; the codec and the ToUnicode writer keep lone surrogates as code units; a value > 0x10FFFF (only from a buggy producer) is a debug assertion and U+FFFD in release | T |
| RQ10 | `canvas-picture.ts`/`pdf-picture.ts` near-duplicates (`bounds`, `shapeSubs`, mask colouring) | One copy in `paint::picture` only where a differential test proves the two TS copies equal; otherwise two named variants | G |
| RQ11 | Budgets | Retained page ≤ 0.5 × TS; encoded ≤ 1.5 × in-memory; first numbers measured at gate 2 | gate 2 |
| RQ12 | `canvas-picture.ts` masks: wasm exports (one implementation) or keep the frozen TS `shared/drawingml` in the shell for milestone 1 | **Wasm exports** (fixed decision: one implementation), behind a thin TS facade with the same signatures; the TS package as the fallback only if the exports slip the milestone | D |
| RQ13 | Border dash kinds: K14 proposed arrays in the shared item and the `bdr*` name in the Write section | **Enum `LineDash`** (9 kinds, 1 byte) in the shared item: both painters expand it with the same arithmetic (`BORDER_DASHES`, then ×20 on the canvas), so the arrays would cost memory without changing a bit; the kinds are dash patterns, not Word objects | T |

## Requests to other records (not edited there)

- **uo-docx-layout**: RQ1 (F3 shrinks by ≈ 280 lines; `drawing.rs` builds `ShapeGeom`), RQ2, RQ3
  (`ImageRef` gains an identity separate from the bytes), RQ13; `Layout::navigation` returns
  `uo_render::Navigation` in points as pdf-navigation.ts computes them.
- **uo-docx**: `CustPath` (custGeom numeric paths) is a `uo_drawingml` type used by the model; the
  session schema uses this crate's generator (G15).
- **uo-drawingml**: host `CustPath`; buffer exports for the mask calls of `canvas-picture.ts` (RQ12).
- **uo-wasm**: page pull with deltas, `image_bytes`/font bytes pulls, the item-addressed paint
  exports (§5).

## Analysis skeptic

Not run separately (hybrid mode); the milestone-1 skeptic covers this record.

## Verification (gate 2)

Pending.

### Parity

Corpus: … Mismatches: … Differential: …

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native) | | |
| M2 soak (wasm) | | |
| M3 budgets | | |
| M4 boundaries | | |
| M5 cache caps | | |
| M6 Miri / LeakSanitizer | | |

### Security

Pending.

## Evolution

Pending. Candidates: `f32` coordinates in the encoding (LQ5, pixel compare), zero-copy decoding
(RQ5 alternative), delta-of-x glyph columns (only with an exactness proof).

## Final review and benchmarks (gate 3)

Pending. `tools/bench`: encode + decode + paint per page (main: paint from objects), wasm and Node.

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner + reuse, architecture plan for uo-render and uo-pdf together (Opus) | (orchestrator) | ≈ 60 |
