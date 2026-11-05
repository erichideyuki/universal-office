# uo-render — the display list (items, resources, per-page codec, painter helpers)

Record of the rust-port workflow. Status: **milestone-1 gate approved 2026-11-03** (hybrid
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
| 3 — T | types, list container, resources, navigation, the small `geom` functions, `render.uos` + `write.uos` (Opus, inside uo-docx-layout F0) | (F0 session) | in the layout record's F0 row (≈ 165 for F0 + T) |
| C | codec: generator `uo-schema`, runtime `uo-codec`, IDL grammar, R-P2 suites, fuzz targets, CI check (Sonnet, hybrid mode) | (orchestrator) | ≈ 165 |

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (RQ1–RQ13). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.

## Milestone 1 — frontier C (codec)

Built 2026-11-03 (Sonnet, hybrid mode) from G15/RQ4. Files: `core/tools/uo-schema` (the generator, a workspace
bin+lib crate, `publish = false`), `core/crates/uo-codec` (the Rust runtime, `js/runtime.ts` its TypeScript twin),
`core/schema/manifest.txt` (what is generated from what), the generated files (committed), `core/fuzz/src/codec.rs`
(two fuzz targets), the `codec` job of `.github/workflows/rust-core.yml`.

### Generator design

- **A Rust crate, not a Node script.** One toolchain for everyone who regenerates (the Rust and .NET developers need
  no Node), deterministic by construction (no clock, no hash-map order, LF output), tested with the workspace's
  `cargo test`, and under the same lints as the rest. No dependencies (the SHA-256 of the schema hash is 40 lines
  with the NIST vectors in its tests; an independent Node `crypto` hash of the pinned canonical text agrees).
  Node is needed only to *run* the TypeScript side of the tests.
- **Pipeline**: `parse.rs` (lexer + recursive descent, errors with the line) → `model.rs` (names resolved, every rule
  that keeps the emitted code valid and the decoder bounded checked, minimum wire sizes, recursion flags, canonical
  text and hash) → four emitters: `rust.rs` (types + `encode`/`decode` + tables + messages), `ts.rs`,
  and the test-support emitters `rust_support.rs` (canonical dump + seeded random messages) and `ts_support.rs`
  (canonical dump). The emitters assume the model is valid, so adding a rule means adding it in `model.rs` once.
- **Manifest** `core/schema/manifest.txt`: one line per schema, `schema/x.uos rust=… ts=… [rust_support=…] [ts_support=…]`,
  paths relative to `core/`. `cargo run -p uo-schema -- generate` writes, `-- check` fails when a committed file differs
  (CR ignored for Windows checkouts), `-- hash file.uos` prints the hash. CI runs `check`; `tests/generator.rs` runs the
  same comparison, so a plain `cargo test` also fails on a stale file.
- **Generated Rust types are the wire types**, separate from uo-render's hand-written `Page`/`Resources` (ranges into
  side buffers, `OptF64`, `Sym` newtypes). Frontier E maps one to the other. A `columns<S>` is generated as a
  struct-of-arrays `SColumns { cp: Vec<u32>, x: Vec<f64>, gid: Vec<Option<u32>> }` (decoded column by column straight
  into the output vectors; no row struct and no temporary); in TypeScript it is an array of plain objects.

### IDL grammar (`core/schema/*.uos`)

```
file     := doc* "schema" NAME "magic" STRING ";" item*
item     := table | struct | message | enum
table    := doc* "table" NAME ":" TYPE ";"                       -- entries are `TYPE` (a plain struct); ids are positions
struct   := doc* "struct"  TYPE "{" [field ("," field)* [","]] "}"
message  := doc* "message" TYPE "{" ... "}"                      -- a struct that is also a root: gets encode_/decode_ functions and an id
field    := doc* NAME ":" type ["?"] ["ts" STRING]                -- `?` optional; `ts` overrides the TypeScript property name
enum     := doc* "enum" TYPE ["tag" STRING] "{" variant ("," variant)* [","] "}"
variant  := doc* TYPE ["=" STRING] ["(" type ")"]                 -- `= "x"` the TypeScript name; a payload is any type but optional
type     := "bool" | "u8" | "u16" | "u32" | "i32" | "f64"
          | "utf16"                                               -- UTF-16 code units (lone surrogates kept); TS string
          | "sym"                                                 -- interned string: an id into the implicit symbol table; TS string
          | "ref" "<" NAME ">"                                    -- an id into a declared table; TS number
          | "bytes_ref"                                           -- a handle (varint id) to bytes the shell pulls separately; TS number
          | "bytes" | "app_section"                               -- inline bytes (the producer's section is another schema's message); TS Uint8Array
          | "list" "<" type ">" | "columns" "<" TYPE ">"          -- columns: a list of a struct stored column by column
          | TYPE                                                  -- a struct or an enum
NAME  := [a-z][a-z0-9_]*        TYPE := [A-Z][A-Za-z0-9]*        doc := "///" text        "//" is a comment
```

Rules checked by the generator (each has a test in `tests/generator.rs`): the magic is 4 printable ASCII characters;
names are unique and not reserved by the generated code (`Tables`, `Delta`, `Vec`, `Option`, …); a Rust keyword as a
field name becomes a raw identifier (`ref` → `r#ref`, `box` → `r#box`; `self`/`super`/`crate`/`Self` are refused);
a struct has at least one field and at most 64 `bool`/optional fields (one bitmap); an enum has at least one variant;
a type may contain itself **only through `list<>`** (a `Vec` is the indirection; the decoder counts depth only for
these types); a message cannot be nested; `columns<>` needs a plain struct; at most 16 tables (the symbol table
counts); TypeScript names are unique per struct/enum, printable ASCII, and reached with `["0"]` when not identifiers;
a flattened payload struct must not have a field named like the tag property.

TypeScript mapping: field names default to camelCase of the IDL name (`flip_h` → `flipH`), overridden by `ts`; a unit-only
enum is a string union (`"dotted" | "dashDot"`); a payload enum is an object with the tag property (`tag "t"`, default
`t`): `{ t: "glyphs" } & GlyphRun` when the payload is a struct (flattened, the `tree.ts` shape) and `{ t: "clip", v: number }`
otherwise; an optional field is an absent property (`undefined`, never `null`); a `bool` is always `true`/`false` when
decoded and `!!x` when encoded. The schema `hash` is the first 8 bytes of SHA-256 over a normalized text (names, types,
order, optionality, magic; not docs, `ts` names or formatting; the form is pinned by a test, so changing it is a deliberate
wire version bump).

### Wire format as built

Little-endian; **canonical** (the decoder refuses every other encoding, so `decode` then `encode` gives the input bytes).

| Value | Encoding |
|---|---|
| `u32`, `u16`, counts, ids, enum tags, `bytes_ref` | LEB128, shortest form, at most 5 bytes, nothing over `u32::MAX` (`u16`: over 65535 refused) |
| `i32` | zigzag, then LEB128 |
| `u8`, `bool` as a payload | one byte (`bool` only 0 or 1) |
| `f64` | 8 raw bytes; `-0` and the infinities kept; **every NaN is written as `0x7FF8000000000000`** and any other NaN is refused on decode |
| `utf16` | number of code units, then 2 bytes each |
| `bytes`, `list<u8>` | length, then the bytes |
| struct | a bitmap of `ceil(n/8)` bytes with one bit per `bool` field and per optional field, in field order, bit 0 first, unused bits zero; then every field in order (a `bool` has nothing more, an absent optional has nothing more). No bitmap for a struct without such fields |
| `list<T>` | count, then the elements; `list<bool>` is bit-packed after the count |
| `columns<S>` | count, then column by column: a plain column is its values, a `bool` column is bit-packed, an optional column is its presence bit-packed and then the present values only |
| enum | the tag as a varint, then the payload of that variant |
| message | `magic[4]`, `hash[8]`, message id (varint), generation (varint); for each table `from` and `count` (varints); the entries of every table in table order (the symbol table first); then the body |

Tables: a message carries only the entries the decoder has not seen (`from` = the entries before it); an id in a body or
in an entry may refer to any entry up to `from + count`. The decoder requires `from` to equal its table's length
(`TableGap` otherwise), a new generation drops the tables first, and a message that fails leaves the tables as they
were (appended entries truncated, or the old tables restored after a reset). Pages may be pulled in any order as long
as each view's encoder advances its own marks, as the plan says.

Rust API (generated, per message `Page`): `encode_page(out: &mut Vec<u8>, delta: &Delta, msg: &Page) -> Result<(), EncodeError>`
(appends; on error `out` is as it was), `decode_page(buf, tables: &mut Tables, limits: &Limits) -> Result<Page, DecodeError>`,
`peek_header`, `Tables { generation, sym, <table>… }` (+ `lens`, `truncate`, `delta_since`, `sym_units`), `Delta { generation, <table>: TableDelta }`.
Errors are typed (`BadMagic`, `VersionMismatch`, `WrongMessage`, `Truncated`, `BadVarint`, `BadTag`, `BadBool`, `BadRef`, `TableGap`,
`NonCanonicalNaN`, `NonZeroPadding`, `Limit(what)`, `TrailingBytes`; encode: `TooLarge`, `ColumnLengths`).
TypeScript: `decodePage(buf, tables, limits?)`, `encodePage(msg, state)` with `Tables` (`sym` is a `SymTable`: strings resolved
on decode, interned on encode; other tables are arrays of decoded objects), `EncodeState { generation, tables, sent }`,
`newTables`, `newEncodeState`, `tableLens`, `DecodeError` with the same `code`s.

**Limits** (`Limits`, defaults in the runtime): depth 64 (recursive types only), list length 2^24, total elements 2^26,
and **allocation 128 MiB** — counted before each `Vec` is reserved from the Rust size of its elements. The last one was added
after a test showed the first three do not bound memory: a 10 MB message of one-byte `Item::Pop` asks for 1.84 GB (184 B
per element in Rust). Every count is also checked against the bytes that remain (`count × min bits ≤ remaining × 8`, the
minimum computed per type by the generator), so nothing is allocated for a count the message cannot back. The TypeScript decoder checks
depth, list length and total elements but not bytes (it reads what the wasm module produced).

### Verification and numbers

| Check | Where | Result |
|---|---|---|
| R-P2a Rust encode → Rust decode: same values (dump: every `f64` by bits, every string by code units) and same bytes on re-encode | `tests/codec_fixture.rs`, `tests/codec_real.rs` (the suite is one macro run over the fixture's `Frame` and `Command`, `render.uos` `Page` and `write.uos` `WritePage`) | 400 random streams each (1–6 messages, tables, generations): fixture 270,266 + 82,810 B, render 1,078,419 B, write 2,401,057 B. Plus 10^5 glyphs in columns, empty page, `-0`/±∞/subnormal/NaN, lone surrogates, depth, lying counts |
| R-P2b Rust encode → **TypeScript decode → TypeScript encode** identical bytes, and the TypeScript dump equals the Rust dump | the same macro, Node 24 (`tests/ts/cross.ts`) | 360 messages per schema, all identical: fixture 69,041 + 21,839 B, render 285,536 B, write 549,599 B |
| Canonical form: every prefix of a message fails and leaves the tables; every single-byte change fails or decodes to a value that encodes back to exactly the changed bytes | the same macro | render: 35,189 refused / 59,463 canonical; write: 32,458 / 62,298; frame: 36,221 / 41,947; command: 18,709 / 11,355; no violation |
| Runtime primitives (both languages): varints at every boundary, overlong/6-byte/over-`u32` refused, zigzag, NaN, strings, bitmaps and padding, limits, header | `crates/uo-codec/tests/runtime.rs` (12), `tests/ts/runtime.test.ts` (11, run under `node --test` by `tests/ts_runtime.rs`) | pass |
| Generator: the committed files are current; deterministic; every rule above; hash pinned (`ebe79f884b66eb70` for a two-field schema, equal to an independent Node SHA-256) and insensitive to comments/docs/ts names | `tests/generator.rs` (12) | pass |
| Generated code under uo-render's deny lints (`cast_possible_truncation`, `cast_sign_loss`, `indexing_slicing`), workspace clippy `-D warnings` (no unwrap/expect/panic), `cargo fmt`, ban lists (`clippy-ban`, `grep-bans`), float-cast scan | clippy over `--all-targets`, `tests/*.sh` | clean |
| Generated TypeScript under `tsc 5.9 --strict --noUncheckedIndexedAccess --exactOptionalPropertyTypes --noUnusedLocals --noUnusedParameters --erasableSyntaxOnly --verbatimModuleSyntax` | CI `codec` job | clean |
| wasm32 | `cargo build -p uo-codec --target wasm32-unknown-unknown --profile wasm`; the generated fixture/render/write codecs and the tests compile for wasm32 (`cargo check -p uo-schema --tests --target wasm32-unknown-unknown`) | builds. Not executed in wasm (the codec has no `unsafe`, no platform call, no hash-map; the TypeScript side is what runs in the browser) |

Memory (M1–M3, `tests/memory.rs`, `crates/uo-codec/tests/runtime.rs`): **M1** encode + decode return live bytes exactly to the start on the
fixture and on 20 render messages, and on several hundred error paths (every 7th prefix, 200 flipped bits, bad hash, trailing byte, a failure after the tables were
appended to); the runtime's every operation and error path too. **M2** soak of 2,000 encode+decode iterations
across generations, sampled every 100: flat, tolerance 0 (a codec keeps no state or cache; `clear_caches()` is a no-op). **M3** (fixture page of 10^5 glyphs
`cp, x, gid?, joins, note?`): wire 1,055,337 B (10 B/glyph; the render glyph, cp + x only, is 9.1 B against 12 B in memory); encode allocates 4,194,296 B in
total for a 2,097,152 B peak (the output doubles from empty: callers should reserve the previous page's size); decode allocates 4,500,364 B, exactly the
columns, no temporary. A lying count allocates under 4 KB. M4/M5: no handles and no caches in this crate.

Throughput (release, this Windows x64 machine, `tests/perf.rs`, not a gate): 100,000 glyphs in one run (912,577 B): encode 184 µs (≈ 5.0 GB/s), decode 334 µs (≈ 2.7 GB/s);
a text page of 3,000 glyphs in 60 runs (28,793 B): encode 7.8 µs, decode 42.6 µs. The TypeScript side was not timed.

Fuzz (`core/fuzz`, targets `codec_decode`, `codec_roundtrip`; the stable driver, libFuzzer runs in CI): `codec_decode` feeds arbitrary bytes, raw or behind the valid
magic and hash, to four decoders and asserts no panic, no call over 1 s, an error leaves the tables empty, and **decode ∘ encode = identity** on whatever decodes;
`codec_roundtrip` seeds a random valid message and checks encode/decode/re-encode and that every cut fails. 120 s each: 133,044,119 and 767,329 executions
(1.1 M/s and 6.4 k/s), no crash, slowest call 11 ms and 37 ms; again after the allocation limit was added, 90 s each: 90,163,533 and 572,125 executions, no crash, slowest 7.7 ms and 45 ms. The decoders were fuzzed with the small `CODEC_LIMITS` (depth 16, 16 Ki elements, 64 Ki total, 16 MiB allocation).

### Decisions outside the plan (declared)

1. Runtime crate `uo-codec` (used by uo-render and the uo-docx session: more than one user), TypeScript twin in `crates/uo-codec/js/runtime.ts`; generated TypeScript imports it by a relative path with the `.ts` extension (erasable syntax only, so Node 24 runs it and a bundler needs no transform).
2. Generated Rust types are wire types; the hand-written uo-render types stay (frontier E converts). Generated files are `generated.rs`: **`gen` is a reserved keyword in edition 2024**, so the plan's `codec/gen.rs` cannot be a module name.
3. Canonical decoding as a rule (not in the plan), which makes `decode ∘ encode` a fuzz oracle and R-P2b exact; NaN canonicalization is done by the codec itself (no `uo-jsnum` dependency); a NaN with a payload does not survive a round trip by design.
4. Integers: `u8` raw, `u16`/`u32` varint, `i32` zigzag; no 64-bit integers (a JS number cannot carry them; add with `bigint` if the session needs one).
5. `sym` decodes to a string in TypeScript and to an id in Rust; `ref<T>` stays a number in both; `bytes_ref` is a varint handle; `app_section` is inline length-prefixed bytes holding another schema's complete message (own magic and hash).
6. Table deltas are generic (`table` declarations), not render-specific: header `from`/`count` per table up front, entries after, atomic commit/rollback, the generation in every header.
7. Recursion only through `list<>`; depth counted only for recursive types; the allocation limit (above) is an addition to the plan's "limits on decoded sizes".
8. The generator crate allows `clippy::disallowed_methods` crate-wide (the ban list keeps JavaScript text semantics in the engines; this tool reads ASCII); the runtime and all generated code are clean under it.
9. Generated Rust carries `#![cfg_attr(rustfmt, rustfmt::skip)]` (the compiler never sees it), so `cargo fmt --check` ignores the files wherever they are included (checked in the workspace and in the fuzz crate); generated test support (`*_support.rs`: dump, random) is a separate sibling module for `#[cfg(test)]`.
10. `Box` is allowed as a type name (the generated code never uses `Box`); `render.uos` has a `Box` struct and a `box` field.

### Not done / hand-offs

- uo-render and uo-docx-layout do not declare the generated modules yet (their `lib.rs` are the Opus agent's): add `uo-codec` to their `Cargo.toml`, `pub mod codec;` with `codec/mod.rs` = `mod generated; pub use generated::*; #[cfg(test)] mod generated_support;`. The files are already in `crates/uo-render/src/codec/` and `crates/uo-docx-layout/src/codec/`, compiled and tested through `#[path]` in `core/tools/uo-schema/tests` and in the fuzz crate.
- R-P2c (the frozen TS `LayoutResult` → generated TS encoder → Rust decode) is frontier D; the encoder it needs exists (`encodePage(msg, state)`; the adapter interns colour strings through `state.tables.sym` and fills `faces`, `images`, … itself).
- The session schema of uo-docx D2a is not written; the generator covers what the plan lists for it (payload enums, nested lists, recursion through lists, optional fields).
- Not run: the generated codec inside wasm (compiled only), in WebKit/real Safari, a TypeScript decode timing, `libFuzzer` itself (the stable driver ran; CI runs libFuzzer), a long fuzz run (minutes, not hours).
- Open for the maintainer: the TypeScript decoder has no byte limit; if it ever reads bytes that did not come from the wasm module, add `maxAlloc` as in Rust.

## Milestone 1 — frontier G (geometry and painter helpers)

Hybrid mode, one Sonnet session, 2026-11-05. Scope: §5 `geom` and `paint::*` with their R-P3 facets. Pure functions on the
types of frontier T; no painter, encoder or wasm export here (frontiers E, D, V; uo-pdf P). Files: `src/geom.rs`,
`src/paint/{mod,fill,page_fill,text_fx,picture}.rs`, `tests/g_*.rs` (+ `tests/g_support/`, `tests/captures/g_*.capture`),
`tools/capture-g.mjs`. `geom::PENDING` is empty (a test asserts it).

### Ported (TS → Rust)

| TS (`write/packages/docx-layout/src/`) | Rust (`uo-render/src/`) |
|---|---|
| `geometry.ts` 14–150 `K`, `arcTo`, `poly`, `pin`, `presetPathLocal` (21 names, `default` = rectangle) | `geom.rs` `arc_to`, `preset_path_local` |
| `geometry.ts` 472–520 `custPaths` (M/L/C/Q/A/Z, `p.w \|\| 1`, flips, rotation) | `geom::cust_paths` |
| `canvas.ts` 439–451 / `pdf.ts` 386–400 the shape-part assembly (line, `custom`, preset + `OPEN_PRESETS`) | `geom::shape_parts(unit, &ShapeGeom, &Resources, frame, line)` → `ShapePart { cmds, fill, stroke, even_odd }` |
| `geometry.ts` 199–352 `phi`, `hexRgb`, `gradientSamples`, `gradientGeometry`, `patternRaster`, `blipTiles`, `isRectGradient`, `rectGradientRaster` | `paint/fill.rs` (+ `paint::hex_rgb`, `GradBox`, `Tile`, `PatternRaster`, `RectRaster`, `stops_of`) |
| `page-fill.ts` 61–175 `tri`, `fract`, `near`, `fillParam`, `ramp`, `gradientTilePx`, `gradientTileRgba`, `patternRgba`, `hexRgb` (with `\|\| 0`), `pageFillTileTw` | `paint/page_fill.rs` |
| `textfx.ts` 77–231 `gradOf`, `fxMarginPt`, `runBox`, `runOutlineSegs`, `fxLayers`, `dilate`, `dashPattern` | `paint/text_fx.rs` (+ `RunView`, `RunView::of(list, item, res)`, `FxLayer`) |
| `canvas-picture.ts` / `pdf-picture.ts` `bounds`, `shapeSubs`, `maskToCanvas`/`colored`, `DASH` | `paint/picture.rs` `effect_bounds(Painter, …)`, `shape_subs`, `colored_mask(…, ChannelStore)`, `dash` |
| (new) limits and errors of the helpers | `paint::{PaintLimits, PaintError, LimitKind}` |

`geom.rs` 161 → 516 lines, `paint/` 20 → 949 lines (doc comments included); the T functions (`map_path`, `path_bounds`,
`rotate_path`, `line_path`, `preset_path`, `is_open_preset`) are unchanged and now under facets.

### Parity evidence (R-P3)

`tools/capture-g.mjs` runs the frozen TS (`UO_TS_ROOT` = the clickcompliance-docx checkout, d0f45bc5, built `dist`) and writes
five captures; `tests/g_*.rs` regenerate every input with the testkit `Rng` and the same pools (hostile values — NaN, ±∞, −0, 1e308 —
in every number pool, malformed colour strings, unknown preset names) and compare digest by digest (exact f64 bits, −0
distinguished, NaN canonicalized, pixel buffers as bytes). The functions that are private in the TS (`arcTo`, `rotatePath`,
`shapePath('line')`, `linePath`, `dilate`, the picture `bounds`/`shapeSubs`/`maskToCanvas`/`colored`/`DASH`) are **run from the
source text of the built `dist`** (`extractFn`), not copied.

| Capture | Facets | Calls compared | Result |
|---|---|---|---|
| `g_geom` | 40: `preset/<29 names>` (local + placed with flips/rotation), `cust`, `arc`, `rotate.{canvas,pdf}`, `line.{canvas,pdf}`, `bounds`, `map`, `open`, `parts.{canvas,pdf}` | 8,700 presets, 3,000 cust sets, 3,000 arcs, 8,000 rotate/line, 4,000 bounds/map, 6,000 shape parts | 40/40 equal |
| `g_fill` | 5: `samples`, `geometry`, `pattern`, `tiles`, `rect` | 1,200 ramps (two-stop S curve and linear), 3,000 geometries, 400 pattern rasters (all 54 presets plus unknown names, 5 pixel sizes, 4 samples per cell), 1,500 tile sets, 300 rect gradients up to 2,000² | 5/5 equal |
| `g_page_fill` | 5: `param`, `tile_px`, `tile_rgba`, `pattern_rgba`, `tile_tw` | 25,200 `fillParam` values (600 fills × 21 points × 2 modes), 600 tile sizes, 160 tiles × screen/PDF, 400 patterns, 600 tile sizes in twips | 5/5 equal |
| `g_text_fx` | 7: `margin`, `box`, `outline`, `gradof`, `dilate`, `dash`, `layers` | 1,500 margins, 1,500 boxes, 600 outlines (4 open fonts), 300 dilations, 260 `fxLayers` (177 produced a layer; pixel bytes compared) | 7/7 equal |
| `g_picture` | 6: `bounds.{canvas,pdf}`, `subs`, `colored.{canvas,pdf}`, `dash` | 6,000 bounds, 800 subpath sets (17 shapes incl. arcs, all flips), 1,200 coloured masks | 6/6 equal |

63 facets, **0 mismatches, 0 accepted, 0 not replayed**. The committed captures were taken in WSL (Node v24.15.0 linux-x64, glibc 2.39,
`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2): the replay requires that platform because the ramp uses `**`
(`gradientSamples`: `g_fill`, `g_page_fill`). A second WSL run is byte-identical to the committed files and a **Windows Node
v24.15.0 win32-x64** run has the same records line for line below `@platform` (so the pow-dependent facets did not separate the
platforms on this corpus). Capture time 5 s; replay 3.9 s release, 45 s debug (`g_fill` 27 s of it).

Two things the capture settles about the TS itself: (1) the picture helpers are **not** equal copies — `bounds` differs
(the canvas adds `h` to the shadow margin for a negative `sy`; `bounds.canvas` ≠ `bounds.pdf` digests) and `maskToCanvas`
clamps its colour channels where `colored` wraps (they differ only for a malformed colour string) — while `shapeSubs` and `DASH`
are equal (the script throws if they ever differ) and are one function each; (2) `canvas.ts` and `pdf.ts` have the same
`rotatePath` (one function, both facets equal).

### Memory and no-panic

- **M1** (`tests/g_memory.rs`): every `geom`/`paint` function, ok and each refusal, returns the live bytes to the starting value
  (`assert_no_leak`, outputs `black_box`ed, the face's outline cache warmed first); the size checks come before the allocation they
  guard (`measure`: 0 allocated bytes for the `max_samples` and `max_raster_px` refusals, < 256 B for the pattern ones, < 64 KiB for
  the over-cap effects layer, < 1 MiB — the disc offsets — for `dilate`). **M2**: a soak of the pipeline (paths, ramps, pattern,
  rect gradient, page tile, `fx_layers`) is flat with a tolerance of 0 B. **M3** (release, bytes allocated / peak above start):
  `pattern_raster` Letter PDF (0.75 pt, 4 px per cell) 55.1 MB output, 55.1 / 55.1 MB; `rect_gradient_raster` 2,000² 16.0 MB,
  16.0 / 16.0 MB; `gradient_tile_rgba` 1024×768 3.1 MB, 3.15 / 3.15 MB; `fx_layers` 24 pt run at 2.78 px/pt (54,810 px) 4.7 / 2.1 MB,
  at 8.33 px/pt (489,936 px) 40.4 / 18.6 MB (**82 B and 37 B per pixel**: float RGBA 16, silhouette, reflection, blur and effect masks,
  output 4); `dilate` 1000² r = 6 4.0 / 4.0 MB. Budgets asserted: output + 8 KiB for the rasters, 64 B/px + 1 MiB peak for the layers.
- **No-panic** (`tests/g_nopanic.rs`): 20,000 hostile geometries (random f64 bit patterns, NaN, ±∞, ±1e300, −0), custom paths of
  commands without points, 3,000 hostile fills, 3,000 page fills, 600 hostile runs (glyph ids and code points past the face, columns of
  different lengths, hostile `scaleX`/size/ppt/effect parameters, tight mask limits), 4,000 pictures; colour strings of random UTF-16
  (lone surrogates); the cases the TS never returns from end with an error (blip tiles under the precision of the coordinate, a
  sweep of 1e300°, a 10⁹ pattern). 4 s debug.

Gates run: `cargo test -p uo-render --all-features` (debug and release) green; `cargo check -p uo-docx-layout --all-features --tests`
green; `cargo clippy -p uo-render --all-features --tests --no-deps -- -D warnings` native and `--lib --target wasm32-unknown-unknown`
clean; `core/tests/float-casts.sh` exit 0; `core/tests/grep-bans.sh` clean; `rustfmt --check` on the files above clean. No `unwrap`,
`expect`, `panic!` or panicking index in `src/`.

### Declarations (every decision outside the TS semantics)

Numeric sites (all `Math.*` through `uo_jsnum`): `math::{cos, sin, tan}` (arcs, rotations, shadow margins, gradient axis), `math::exp`
(`phi`), `math::pow` (`**` 2.2 and 1/2.2 of the ramp), `math::hypot` (gradient geometry), `math::{round, ceil, floor}` (rasters,
tiles, bounds), `math::{min, max, min_n, max_n}` (every `Math.min/max`, NaN wins), `math::rem` (`%` of the pattern and angle
folds), `math::abs`; `conv::{truthy}` (`p.w || 1`, `if (st.rot)`, `Math.hypot(...) || 1`), `conv::{to_uint8, to_uint8_clamp}` (typed-array
stores: `Uint8Array` wraps, `Uint8ClampedArray` clamps), `conv::to_int32`/`to_uint32` (`>>` of a pattern row and its count),
`conv::js_index` (reads `ta[i]` with a JS number index: NaN out of range); `parse::parse_int_utf16` (`parseInt(h.slice(a, b), 16)`
over the UTF-16 units: a malformed colour gives NaN/negative channels and the store decides); `paint::f32_store` (`Float32Array`
store = `as f32`) for the RGBA accumulation and the masks. Constants written as the TS does at run time: `4/3`, `2/3`, `1/2.2`,
`PI/3`, `(deg * PI) / 180`; `K = 0.5522847498`.

| # | Decision | Why |
|---|---|---|
| G-D1 | `arc_to` emits nothing for a sweep of more than 16,384 quarter turns (it returns the current point); a NaN sweep emits nothing as in the TS | the TS loops `ceil(\|sweep\|/90)` times (hangs / exhausts memory on 1e300) |
| G-D2 | Colours and names arrive as interned `Sym`s through a `&SymbolTable` (or `&[u16]`); `hex_rgb` is the TS `parseInt` over UTF-16 | RQ6; no lossy conversion of colour strings. Preset and pattern names do `String::from_utf16` (lone surrogates cannot name a preset) |
| G-D3 | `PaintLimits` (`max_raster_px` 2^25, `max_tiles` 2^20, `max_samples` 2^20, `max_dilate_offsets` 2^16, `max_dilate_work` 2^33, `max_fx_px` 2.4·10⁷ = the TS cap, the uo-drawingml `Limits`) and `PaintError { Limit, Invalid, Draw }` | the TS has only the three caps (4,000 tiles, 2,000² rect gradient, 2.4·10⁷ px layer) and none of the others; a host short of memory lowers `max_fx_px` (the layer peaks at 37 B/px: 0.9 GB at the TS cap) |
| G-D4 | `pattern_raster`: a raster size that is not a whole number (NaN, fraction: `px` 0, `k` 0.5 on an odd frame) is `Invalid`; over the limit `Limit(RasterPx)`; no allocation before the check | the TS builds a zero-length array / throws a `RangeError` |
| G-D5 | `blip_tiles`: counts every loop turn (rows and tiles) against `max_tiles`; the TS check `out.length < 4000` stays; `algn` absent is the caller's `"tl"` | the TS hangs when the step is under the precision of the coordinate, and one row can hold millions of tiles |
| G-D6 | `rect_gradient_raster`: a NaN side is `Invalid` (the TS returns an empty raster of NaN size) | |
| G-D7 | `gradient_samples(stops, n)`: `n` over `max_samples` is `Limit(Samples)`; `gradient_tile_rgba` over `max_raster_px` is `Limit(RasterPx)` | |
| G-D8 | `dilate`: the disc is capped (`max_dilate_offsets`, work) and an infinite radius refused, **before** the output mask is allocated | the TS loops (2R+1)² offsets per pixel |
| G-D9 | `fx_layers` returns `Ok(None)` for a run with no glyphs (the TS throws a `TypeError` in `runBox`) and for a layer whose size is NaN (the TS then returns a degenerate layer of NaN size — a NaN `scaleX`; the parity corpus replaces a NaN `scaleX` by 1 for this facet only, `box`/`outline` keep it); mask limit errors are `Draw` | |
| G-D10 | `run_box` is `Option` (empty run); `run_outline_segs` returns `Subpaths`, never "null": a `uo_fonts::Face` always has the outline method (no `glyf` = no subpaths, as every glyph answering `null`) | the TS `LayoutFont.glyphOutline` is optional, `Face` is not |
| G-D11 | `RunView` borrows the glyph columns (`cps`, `xs`, `gids` with `NO_GID`); a column shorter than `cps` is tolerated (the glyph is skipped / the box is `None`) | no `GlyphRunItem` is rebuilt |
| G-D12 | `picture`: **two** `effect_bounds` (`Painter::Canvas` keeps the extra `h`), **one** `shape_subs`, **one** `dash`, `colored_mask` with `ChannelStore::{Clamp, Wrap}` | the capture proves what is equal and what is not (above) |
| G-D13 | `shape_parts` is the one assembly for both painters (`LineUnit` picks the unit: `line` points `/20` for the PDF); `GeomEval::Drawingml` shapes are not evaluated here (the painter calls uo-drawingml's `preset_path`) | RQ1; the TS evaluates the compact subset only |
| G-D14 | `gradient_geometry` reads `GradFill.rot_with_shape` as a `bool` (`rotWithShape !== false`: the decoder sets `true` for absent; `grad_of` sets `true`); `adj` is looked up as a JS object (last value of a repeated name); `GradBox.rot` 0 stands for absent (`!box.rot`) | the types of T |
| G-D15 | `pattern_raster` takes the preset name and looks the 8 rows up with `uo_drawingml::pattern_rows` (own names only, G7 of uo-drawingml) | |
| G-D16 | The shape-part **assembly** facet (`parts.{canvas,pdf}`) compares with a transcription of the `paintShape` / PDF `shape` lines in the script (built from the real `custPaths`, `presetPath`, `OPEN_PRESETS` and the extracted private `rotatePath`/`shapePath('line')`/`linePath`); the assembly itself is not callable in the TS | it is inline code of two large functions |

### Not done

- **No corpus-driven inputs.** Every facet uses generated inputs (pools with hostile values, real open fonts for the text effects, the
  real preset tables); no capture of the `ShapeItem`/`GlyphRunItem` values of real layouts. The item fields themselves are covered by the
  layout adapter (2,755 layouts); the helpers' use of them comes with frontier E/V (R-P4) and the PDF parity.
- The canvas-only precomputation of frontier E (glyph batch joins, the `F0xx` symbol remap, `gid` outlines of canvas.ts 357–371, the 64-sample
  gradient call) and the PDF's own as-path outline code (`pdf.ts` 177–292, outline skew 0.2) are not here.
- The wasm paint exports (`paint_fx_layers`, `paint_pattern`, …) of §5 and the uo-drawingml mask exports of RQ12.
- `GeomEval::Drawingml` evaluation (Slides).
- Cargo: `uo-testkit` added to `[dev-dependencies]` of `uo-render` (and the matching line of `core/Cargo.lock`) for the replay. No other file
  outside `geom.rs`, `paint/`, `tests/g_*`, `tools/` and this record was touched.

### What the next frontiers need

- **E (canvas encoder)**: `shape_parts(CanvasTwips, …)` for the `parts` hint, `gradient_geometry` + `gradient_samples(…, 64)` for
  `grad_geom`/`grad_samples`, `RunView::of` + `run_box` for fx runs, `run_outline_segs`/`Face::glyph_outline` for the `outlines` hint; the wasm
  exports call `fx_layers(ppt)`, `pattern_raster(px 15, k 1)`, `rect_gradient_raster(ppu 2)`, `gradient_tile_rgba(…, false)`, `blip_tiles(emu 1/635)`,
  `page_fill_tile_tw`, `picture::*` with `Painter::Canvas` / `ChannelStore::Clamp`. The wasm host sets `PaintLimits::max_fx_px` for a phone.
- **uo-pdf P**: the same functions in points with `LineUnit::PdfPoints`, `Painter::Pdf`, `ChannelStore::Wrap`, `gradient_samples` 512 (256 for `path`),
  `pattern_raster(px 0.75, k 4)`, `blip_tiles(emu 1/12700)`, `gradient_tile_rgba(…, true)`, `text_fx::dash_pattern`, `picture::dash`, `fx_layers(ppt 200/72)`.
  `RunView.synth_italic` here is the textfx skew 0.21; the PDF's own outline paths use 0.2.

### Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| G | geometry + painter helpers + R-P3 + M1–M3 + no-panic + record (Sonnet, hybrid mode) | one session (wall time not recorded) | ≈ 100 (own tally) |

Parity rounds: capture 1 of each file; `g_geom`, `g_fill`, `g_picture` equal on the first replay; `g_page_fill` 2 mismatches (a **port bug**:
a clippy rewrite of `fo >= 75 || fo < 0` into a negated range changed the NaN focus; fixed, equal on round 2); `g_text_fx` 1 mismatch (not a
port bug: a NaN-size layer, decision G-D9, a corpus change). Memory/no-panic: 2 test-design fixes (a `Vec` kept across `assert_no_leak`, an
off-by-integer `k = 0.5` case), none in the library.

## Types (T)

Frontier T, Opus, 2026-11-03, inside uo-docx-layout F0 (record there: "Milestone 1 — F0").

### What exists (`core/crates/uo-render`)

`forbid(unsafe_code)`, `deny(cast_possible_truncation, cast_sign_loss, indexing_slicing)`; fmt,
clippy `-D warnings` (native and wasm32) and the ban scripts clean.

- `ids`: `Sym`, `FaceId`, `ImageId`, `TextFxId`, `PageFillId`, `CustId` — dense, first-use order,
  `NonZeroU32` (`index + 1`) so `Option<Id>` is 4 bytes; `from_index`/`index`/`raw`/`from_raw` for
  the codec.
- `sym::SymbolTable`: UTF-16 interning, lone surrogates kept, `since(n)` for the per-generation delta.
- `opt::OptF64`: an optional number in 8 bytes (below).
- `types`: `Page`, `Item` and every payload of RA2 field by field — `RunFlags`, `LineDash` (RQ13, with
  `BORDER_DASHES`), `Stroke`, `PathCmd`, `Ltrb`, `ImageItem`, `PictureLook` (+ `PresetSpec`, `PicLine`,
  `PicShadow`, `PicGlow`, `PicBevel`, `PicReflection`, `ClrChange`), `ShapeItem` with the `ShapeGeom`
  spec and `GeomEval` (RQ1), `GradFill`/`GradStop`, `ShapeShadow`, `Pattern`, `Blip`/`BlipTile`,
  `ShapeFx`, `CustPath`/`CustCmd`, `ResolvedFill`/`FxStop`/`ResolvedTextFx` (+ outline, shadow, glow,
  reflection, `LineCap`/`LineJoin`), `PageFill`/`PageFillKind`.
- `list`: `ItemList` (items + `GlyphColumns` `cp`/`x`/`gid`, `RunSide` per glyph run, UTF-16 `text`,
  `path_cmds`), `Span`, `GlyphRunHead`, `push_glyphs`/`push_path`/`push_plain`, `push_shifted`/
  `extend_shifted` (the TS `shiftItem`), `heap_bytes` (M3), `CapacityError`.
- `resources`: `Resources` (symbols, faces, images, text effects, page fills, custom paths; `add_*`
  and lookups), `FaceRes`, `ImageRes`.
- `nav`: `Navigation`, `PageLink`, `PdfLink`, `LinkTarget`, `OutlineEntry` (points, as
  pdf-navigation.ts).
- `geom` (RQ1): `is_open_preset`, `map_path`, `path_bounds`, `rotate_path`, `line_path` (the canvas
  twips and PDF points variants, `LineUnit`), `preset_path` (placement), `ShapeXf`, `CustPart`;
  `preset_path_local`, `cust_paths`, `arc_to` are frontier G's, listed in `geom::PENDING`.
- `paint`: module plan for frontier G and the shared `GradGeom`.
- `codec`: the generated render codec (frontier C), wired here.
- IDL (in the frontier-C grammar): `core/schema/render.uos` (the canvas page message: tables
  `faces`, `images`, `text_fx`, `page_fills`, `cust`; `Page`, the `Item` union with the tree.ts names
  through `ts`, the canvas hints of §3.3 — `parts`, `grad_geom`, `grad_samples`, `joins`, `remap`,
  `outlines`, `box` — and `app: app_section`) and `core/schema/write.uos` (the Write section:
  `WritePage` with `section`, `number`, `hfItems`, `lines` with `chars` as `columns`, `pfmts` by value,
  `objects`, `cells`, `clipSrc`).

Tests (`tests/types.rs`): `size_of::<Item>()` = 48 (≤ 48), `OptF64` against NaN/±0/±∞/the marker
bits, the symbol table, and `push_shifted` against the `shiftItem` rules. The layout adapter check
(uo-docx-layout `tests/adapter.rs`) decodes 2,755 frozen-TS layouts (the whole Word oracle but one
embedded-font document, the 8 .NET synthetic inputs, generated documents) into these types and
re-encodes them bit for bit through the P1 walker: every item field the TS produced on the corpus has
a representation here.

### Decisions beyond the plan (declared)

| # | Decision | Why |
|---|---|---|
| T1 | `Item` has **inline struct variants** for the small items and boxes `Image`/`Shape`; `XformPush` boxes its matrix | RA2's `Item::Glyphs(GlyphRun)` with payload structs measures 56 B (the tag cannot use the payload's padding); inline variants measure 48 |
| T2 | A glyph run keeps `text`, `scaleX`, `alpha`, `fx` in a side record `RunSide` (`list.runs[run]`) | the run item stays 40 B; those fields are rare or (RQ7) read only by the canonical stream |
| T3 | `OptF64` for every optional number of an item (absent = a signalling-NaN payload JS never produces; values are NaN-canonicalized on entry, as the codec does) | `Option<f64>` is 16 B; RA1.2 needs absent apart from NaN |
| T4 | `ItemList` is the storage of a page **and** of the producers' intermediate arrays (uo-docx-layout `PassItems`): moving an item copies its column ranges; `push_shifted` is the TS `shiftItem` (app-neutral arithmetic) | one container; the layout shifts items between arrays all the time |
| T5 | `FaceRes` = `face: Option<uo_fonts::Face>` (TS `font`) + `bytes: Option<Arc<[u8]>>` (TS `bytes`, absent = not embeddable) + `advances: Option<Arc<AdvanceTable>>` (PQ1); manual `Debug` | RA2 merged `font` and `bytes`; the TS keeps them apart |
| T6 | `ImageRes` gains `svg` and `width_twips`/`height_twips` | the viewer paints the SVG version (universal-document-viewer.ts:962); page fills build images from the twips sizes |
| T7 | `CustPath`/`CustCmd` are defined here until uo-drawingml hosts them (§12 request open), then become re-exports | uo-drawingml had no `CustPath` when T was written |
| T8 | `Clip` and `Xform` are split into push/pop variants (`ClipPush { x, y, w, h: OptF64 }`, `ClipPop`, `XformPush`, `XformPop`); the wire keeps the tree.ts `op` field | the push fields are optional in the TS; pops carry nothing |
| T9 | The small pure functions of `geom` are ported now (with their TS arithmetic, `uo_jsnum` trig); the preset evaluator and custom paths stay frontier G's | RQ1 moves them here; F0 needed `map_path`/`rotate_path` shapes to fix the signatures |
| T10 | IDL conventions: Rust field names with `ts "<tree.ts name>"`; numbers `f64`, optional numbers `f64?`, optional booleans `bool`; two-element arrays (`duotone`, `focusPosition`) as structs with fields `"0"`/`"1"`; `PictureLook.geom.adj` and `ShapeGeom.adj` as `list<AdjValue>` in the TS key order | the decoder materializes tree.ts objects (RQ5) |

### Process

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| T | types + IDL, inside layout F0 (Opus) | (F0 session) | counted in the layout record's F0 row |
