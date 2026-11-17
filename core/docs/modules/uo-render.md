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

The Write section is the `write.uos` message `WritePage` (Rust `uo-docx-layout/src/codec/generated.rs`, TS `uo-wasm/js/gen/write.ts` `decodeWritePage`), produced by `uo-docx-layout` `Layout::write_section` / `encode_page` (in the wasm layout handle: `write_section` + `RenderView::set_app`) and carried as the `app` of `encode_page_for_canvas`. G8 record: uo-docx-layout.md "Stage 3 — G8".

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

## Milestone 1 — frontier E (canvas encoder and wasm paint exports)

Hybrid mode, one Sonnet session, 2026-11-05. Scope: §3.3 precomputation, the app-section hook, the page encoder, and the wasm paint
exports (§5), with parity facets against the frozen TS where a TS counterpart exists. Files: `uo-render/src/encode/{mod,hints,to_wire,
decode}.rs`, `src/view.rs`, `tests/e_{hints,roundtrip,memory,nopanic}.rs` (+ `tests/e_support/`, `tests/captures/e_hints.capture`),
`tools/capture-e.mjs`; `uo-wasm/src/{render,render_fixture}.rs`, `js/render.mjs`, `tests/render_{native,unsafe}.rs`,
`tests/render_{paint,boundary,ts_codec}.test.mjs`; `core/schema/render.uos` and its generated files (E-D1). The `codec` module of
uo-render was already declared when this session began.

### Built (ported / new)

| What | Where | Source |
|---|---|---|
| Page encoder `encode_page_for_canvas(page, res, app, sent, out)` (+ `_with(CanvasOptions)`): items, resources and the hints into the generated message; per-view `DeltaMark`; `AppSection` hook; strict errors (`BadId`, `BadSpan`, `Codec`), `out` and the mark untouched on error | `encode/mod.rs`, `to_wire.rs` | new (§3.2, §3.4, §6) |
| Glyph hints: `batchable`, the batch `joins` (the `paintGlyphs` loop simulated, outline glyphs `continue`, NaN does not break), the F0xx `remap`, `outlines` of `gid` glyphs (font units, empty outlines kept) | `encode/hints.rs::glyph_hints` | canvas.ts 315–392 (ported) |
| Fx-run hints: `box` (`run_box`) and the gradient geometry of a gradient text fill / outline fill (`fx_fill_geom`, `fx_outline_geom`) | `hints.rs::fx_hints` | canvas.ts 289–300 (ported) |
| Shape hints: `parts` in canvas twips (`shape_parts(CanvasTwips)`), `grad_geom` (`path_bounds` of all parts + `gradient_geometry`), `grad_samples` (`gradient_samples(…, 64)`), the same branch conditions as `paintShape` | `hints.rs::shape_hints` | canvas.ts 427–510 (ported) |
| Inverse `decode_page(buf, &mut DecodeTables, limits) -> DecodedPage` (Rust `Page` + `Resources` from a message; symbol dedup map; atomic on error) | `encode/decode.rs` | new (§6 "tests, fuzz, tools"; R-P2c bridge) |
| `RenderView`: pages + resources + encoder mark + app sections; `push_message`, `encode_page`, `set_face_bytes`, `set_image_bytes`; item-addressed `fx_layers(ppt)`, `pattern_raster` (px 15, k 1), `rect_gradient` (ppu 2), `blip_tiles` (emu 1/635), `page_fill_tile_tw`, `page_fill_raster`, `picture_bounds` (`Painter::Canvas`), `picture_subs` | `view.rs` | new (§5) |
| Wasm exports (C ABI, feature `render`): `UoRenderView` and `UoBuf` handles, 20 `uo_render_*`/`uo_buf_*` functions, `colored_mask` (`ChannelStore::Clamp`) and `dash` over raw buffers; result layouts and status codes in the module doc of `src/render.rs` | `uo-wasm/src/render.rs`, `js/render.mjs` (JS handles with `free()` + `FinalizationRegistry`, decoders) | new |
| Test fixtures built inside the module (feature `render-fixture`, never shipped): `uo_render_fixture(0)` one rectangle, `(1)` a 24 pt fx run, `a:pattFill`, rectangular gradient, tiled blip, picture look, shape with effects, axial gradient, two page fills | `uo-wasm/src/render_fixture.rs` | new |

### Evidence

**Parity (R-P4 slice, the frozen `paintItems` as the oracle).** `tools/capture-e.mjs` runs the real `paintItems` of the built `dist`
(`UO_TS_ROOT`) with a recording context; `tests/e_hints.rs` encodes the same generated items (the testkit `Rng`, mirrored pools),
decodes the message with the generated codec and replays the painter's decisions from the hints alone through
`tests/e_support/shell.rs` — a transcription of the part of `paintGlyphs`/`paintShape` that reads the hints, i.e. **the contract the TS shell
(frontier D) implements**. Digest of every drawing call (name, f64 bits, strings by code point):

| Facet | Cases | Result |
|---|---|---|
| `glyphs.kern` (`'fontKerning' in ctx`, batches) | 1,500 runs (4 open faces, scaleX / synthetic italic / bold, F0xx symbols, ZWNJ, CJK, astral, gid outlines, NaN jitter) | equal; 6,901 `fillText` of which 1,545 batched, 9,169 `quadraticCurveTo` |
| `glyphs.nokern` (no `fontKerning`: every glyph alone, remap and outlines still apply) | the same 1,500 | equal; 9,650 `fillText` |
| `fxrun` (`runBox` + `gradientGeometry(gradOf(fill), box, …)` for fill and outline fill) | 800 fx runs | equal; 395 gradient geometries |
| `shapes` (parts, gradient geometry, 64 samples, shadow, stroke dash) | 2,007 shapes of 2,500 (13 geoms incl. custom paths with arcs, flips, rotation, lines, NaN/∞ coordinates) | equal; 1,967 of the 2,500 paint something, 809 with a gradient |
| `shapes.fx` (the local item of the effects path) | the other 493 shapes | equal |

**9 facets equal, 0 mismatches, 0 accepted, 0 not replayed**; the test is sensitive (changing the `0.25` join tolerance to `0.3` gives 2 mismatches).
Platform: captured with Node v24.15.0 (V8 13.6) on win32-x64 — see E-D8.

**Round trip (R-P2).** `tests/e_roundtrip.rs`: 60 seeds × a page that uses every item kind and every optional field with hostile numbers
(`-0`, NaN, ±∞, 1e308): Rust page → `encode_page_for_canvas` → `decode_page` → `Debug`-equal page and equal resources (symbols, text effects, page
fills, custom paths, face and image metadata); pages pulled in the order 3, 0, 4, 1, 2 send exactly the entries up to the highest id used so far
(symbol watermark 4, 4, 5, 5, 5); a new generation restarts the tables; the app section travels opaque (also 259 arbitrary bytes); `debug_text` off
sends no text; errors leave the buffer and the mark as they were; a stale schema hash is `VersionMismatch`. **R-P2b for the encoder**
(`uo-wasm/tests/render_ts_codec.test.mjs`, Node): the Rust-encoded first message of the rich fixture → generated TS `decodePage` → generated TS
`encodePage` is **byte-identical**, and the hints arrive as plain objects with the tree.ts names. `cargo test -p uo-schema` (generator check,
codec suites, TS cross-checks after the schema change) green.

**Wasm exports vs native.** `uo-wasm/tests/render_native.rs` calls the exports natively over the fixtures, checks the pattern result against the
helper called directly and prints the FNV-1a 32 checksums that `render_paint.test.mjs` pins: the same 12 checksums in Node and in WebKit —
**the wasm bytes equal the native bytes** for `fx_layers` (64,024 B), `pattern` (53,640 B), `rect_gradient` (57,608 B), `blip_tiles`,
`page_fill_tile_tw`, `page_fill_raster` (gradient 3,145,736 B, pattern 264 B), `picture_bounds` ×2, `picture_subs`, `encode_page` ×2. Refusal codes
(no page, no item, wrong item, missing font, not a font, stale hash, truncated message, null handle) are tested at both ends. A message pushed into
an empty view (the TS-adapter path) paints the same bytes once the font is attached.

**Wasm validation.** `wasm-tools validate --features=-all,wasm1,mutable-global,saturating-float-to-int,sign-extension,reference-types,multi-value,bulk-memory`
(the CI line): valid for the default build (15,528 B, unchanged — `render` is a feature, E-D7), `--features render` (578,821 B) and
`--features count,render-fixture` (983,005 B). **WebKit 26.5 (Playwright) runs both `.test.mjs` files of this frontier: 13 ok, 1 skipped (the
finalizer test needs `gc()`), 0 failed** on this machine; Node: 16 of 16 (paint 7, boundary 7, ts codec 2).

### Memory and no-panic

- **M1** (`tests/e_memory.rs`, `uo_alloc::assert_no_leak`): encode + decode of the full page (font face attached, `debug_text` on and off), of a 40-run
  text page with hints, of 60 shapes with parts and samples, every error path (bad id, one in seven prefixes and seven flipped bytes of a real message)
  return the live bytes exactly. `uo-wasm/tests/render_native.rs::m1_every_export_returns_the_live_bytes`: every export, ok and refused, including the
  consumed input buffers (`push_message` ×3 with a table gap, `set_app`, `set_face_bytes`, `colored_mask`).
- **M2**: native soak 1,000 × (encode + decode of a 30-run + 30-shape page), sampled every 50, flat with tolerance 0 B; 500 iterations across
  generations (3 pages each) flat at 0 B. Wasm (Node and WebKit): 200 iterations of `encode_page` + every paint export on the rich fixture —
  `memory.buffer.byteLength` identical after the 2nd round (12.8 MiB in Node, 9.2 MiB in WebKit), live bytes identical every round
  (799,422 B while the rich view is open, **0 B** after `free()`).
- **M3** (release, bytes allocated / peak above start): text page of 3,000 glyphs in 60 runs — in memory 54,272 B, **encoded 29,473 B = 0.54 ×** (budget
  1.5 ×), encode 225,722 / 144,554 B, decode 219,130 / 164,366 B; one run of 10^5 glyphs — 1,573,184 B, encoded 912,777 B (**0.58 ×**), encode
  4,301,098 / 3,052,242 B, decode 5,149,586 / 3,575,910 B. The rasters' budgets are frontier G's (output + 8 KiB; 64 B/px for the layers).
- **M4** (`render_boundary.test.mjs`, Node and WebKit): 10,000 × (create view, encode a page, free result, free view) with `free()`; 10,000 views
  and results abandoned and released by the `FinalizationRegistry` (Node `--expose-gc`; native live bytes return **exactly**); 10,000 × (new view,
  push a good message, `set_app`, three refused inputs) and 1,000 buffers given to a null handle: consumed, 0 B left; 1,000 × the raw helpers. A
  second `free()` is a no-op.
- **M6**: the new `unsafe` is in `uo-wasm/src/render.rs` (SAFETY comments; `take_bytes` is the one place that rebuilds a `Vec` from a JS buffer).
  `tests/render_unsafe.rs` (no font, no raster) passes under **Miri** (`cargo +nightly miri test -p uo-wasm --features render-fixture --test render_unsafe
  --target x86_64-unknown-linux-gnu`, which also reports leaks). uo-render has no `unsafe`.
- **No-panic** (`tests/e_nopanic.rs`, 9 s debug): 400 pages with random f64 bit patterns in every number (hostile shapes, custom paths with arcs,
  gradients, runs with code points up to `u32::MAX`, gids from `u32`), unregistered faces, malformed spans and ids (all refused), 6 × 1,500 mutated real
  messages through `decode_page` and 3,000 arbitrary byte strings, 60 hostile views through every view operation (caps lowered to 2^22 px), one
  run of 10^5 glyphs with hints.

Gates run: `cargo test -p uo-render --all-features` (debug and release) and `cargo test -p uo-wasm --all-features` green; `cargo clippy -p uo-render
-p uo-wasm --all-features --tests --no-deps -- -D warnings` clean natively and `--lib --target wasm32-unknown-unknown` (also with
`count,render-fixture`); `core/tests/float-casts.sh` exit 0; `core/tests/grep-bans.sh` clean; `rustfmt --check` on the files above clean; `cargo run -p
uo-schema -- check` up to date. No `unwrap`, `expect`, `panic!` or panicking index in `src/` (the generated files aside).

### Declarations (every decision outside the plan or the TS semantics)

| # | Decision | Why |
|---|---|---|
| E-D1 | **Schema change**: `GlyphRun` gains `fx_fill_geom` / `fx_outline_geom` (`GradGeom?`, TS `fxFillGeom`/`fxOutlineGeom`); `render.uos` regenerated (`SCHEMA_HASH` changes; `js/gen/render*.ts` and `generated*.rs` are the regenerated files); `core/tools/uo-schema/tests/perf.rs` gets the two `None` fields (outside the allowed paths, additive) | §3.3 lists only the run box, but `paintGlyphsFx` calls `gradientGeometry(gradOf(f), box, …)` for a gradient text fill and outline, and the shell has no `geometry.ts` after the switch |
| E-D2 | A shape with effects (`fx` set and geom not `line`) gets the hints of the **local item** the canvas builds (`x = y = 0`, `rot = 0`, no `fx`/`shadow`, canvas.ts:430); `RenderView::pattern_raster`/`blip_tiles` use the same local frame | the canvas paints the body through `paintShapeEffects(…, drawBody)` → `paintShape(local)`; the parts of the placed item are never drawn. The shell must not draw a shape's own `shadow` in that path (the TS drops it) |
| E-D3 | Shape hints are skipped (empty `parts`) when `paintShape` returns early (no valid fill/stroke and no gradient/pattern/blip) unless the shape has effects; `GeomEval::Drawingml` shapes get no parts (the Slides evaluator is uo-drawingml's, run by the painter that needs it) | bytes; G-D13 |
| E-D4 | Resource deltas go **up to the highest id the page uses** (plus what those entries use), not "all unsent": pages pulled in any order are decodable, a page carries only what it needs | §3.2 "a page carries only the resources the view has not sent" with out-of-order pulls |
| E-D5 | `AppSection` is `fn write(&self, out: &mut Vec<u8>)` (impls for `[u8]`, `Vec<u8>`, `NoApp`) and the generation lives in the `DeltaMark`; the page index is not in the header (the shell knows what it pulled) | the codec header has no page index; the simplest hook that lets the layout encode its section on demand |
| E-D6 | Added beyond the plan: `encode::decode` (wire → Rust types), `RenderView`, and `push_message`/`set_face_bytes`/`set_image_bytes` | the paint exports need a view; the TS adapter (R-P2c) and V need to paint TS-produced layouts through the same exports. The wire loses `has_bytes`, `upem`, `byte_len`, `has_svg`, the hints and the glyph text (re-encoding a decoded view differs in those) |
| E-D7 | The wasm exports are behind the `render` feature (not default: the shipped module enables it together with the layout bindings); `render-fixture` (implies `count`) is test only | the default module stays 15,528 B for the existing tests; the render exports add 563 KB (fonts + drawingml code) |
| E-D8 | `capture-e` was taken on **win32-x64, Node v24.15.0 (V8 13.6)**, not in WSL like G's: WSL here has only Node 22 and installing Node is a download. `capture_e()` requires V8 13.6 / x64 / Node v24.15.0 but no OS or glibc. The pow-dependent facets (`gradientSamples`) compared equal in G between win32 and linux (V8 runs its own `ieee754::pow`) | no new download; to be re-taken in WSL when Node 24 is there |
| E-D9 | Hints are computed only for a face with a parsed font (`FaceRes.face`); `joins` only for plain runs (`scaleX === 1`, no synthetic italic) and only when some glyph joins; `remap` and `outlines` do not depend on batching (the shell's fallback without `fontKerning` needs them) | the TS reads `it.face.font`, which a face without bytes (the TS adapter, decoders) does not have; same bytes the shell would draw |
| E-D10 | `upem` of a face is `units_per_em || 1000` (1000 for a face without font) | canvas.ts:333 |
| E-D11 | The path span of a `Path` item is checked strictly (`BadSpan`), where `ItemList::cmds_of` would silently give an empty path | a corrupt producer list must not paint as empty |
| E-D12 | `RenderView.limits` (`PaintLimits`) is per view; `uo_render_view_set_max_fx_px` lowers the effect-layer cap for a phone (a cap of 1,000 px answers none or status 4 without allocating: tested) | G-D3 |
| E-D13 | The shell-side contract for batching: `joins[i]` true = glyph `i` is appended (ZWNJ + its raw `cp`) to the batch begun at the nearest earlier glyph with `joins` false; the first glyph of a batch is drawn with its `remap`ped cp and must be `batchable`; outline glyphs are drawn from `outlines` and are never batch starts | what `e_support/shell.rs` implements and the facets prove |

### Not done

- ~~**The uo-drawingml mask exports of RQ12**~~ **Done in frontier D** (milestone 1, below): `picture_polys`, `picture_silhouette`, `picture_bevel`, `picture_adjust`, `outer_shadow`, `reflection_alphas`, `inner_shadow`, `glow`, `soft_edge`, `blur`; `morph` and `presetOutline` are not exported (the canvas never calls the first; `picture_subs`/`picture_polys` cover the second).
- The layout handle exists now (G8 and the binding): the wasm `UoLayout` owns a `RenderView` and encodes with `set_app` for the Write section (`uo_layout_encode_page`).
- A TS oracle comparison of the *exports* (the frozen `patternRaster`/`rectGradientRaster`/`blipTiles`/`fxLayers`/page-fill functions run on the
  same items in Node against the wasm bytes): the exports are thin calls of G's helpers, which G proved against the TS (63 facets), and the wasm
  bytes equal the native bytes here; a direct wasm × TS test is frontier V's.
- CI: no job runs the new tests yet (`.github/workflows/rust-core.yml` is being edited by other frontiers). Lines to add: `cargo build -p uo-wasm
  --features count,render-fixture --target wasm32-unknown-unknown --profile wasm`, then `node --expose-gc --test crates/uo-wasm/tests/render_{paint,boundary,ts_codec}.test.mjs`,
  the WebKit runner on the first two, `cargo test -p uo-wasm --features render-fixture`, and the Miri line of `render_unsafe`.
- Real Safari 15 / iOS 15 (BrowserStack) is not run; only the feature-set validation and WebKit 26.5.
- No fuzz target was added (`render_decode`/`render_roundtrip` of §10 are covered by the codec fuzz targets and the mutation tests of `e_nopanic.rs`).

### What D and V need

**D (TS shell).**
- Decode with the regenerated `js/gen/render.ts` (`decodePage(buf, tables)`); `tables.faces[i].upem` is the font's units per em; `sym` fields are strings already.
- `paintGlyphs` on a decoded run: `outlines` (by `index`, font units: `x' = g.x + c.x·gx + k·c.y·s`, `y' = it.y − c.y·s`, `s = sizePt·20/upem`, `gx = s·sx`, `k = synthItalic ? 0.21 : 0`),
  `remap` (the cp for the glyph at `index`; applies to batch starts only), `joins` (only when `'fontKerning' in ctx`; a run without `joins` draws glyph by glyph). `e_support/shell.rs` is
  the reference transcription (line by line); its draw-call log equals the frozen painter's.
- `paintShape`: draw `parts` (`cmds`, `fill`, `stroke`, `evenOdd`) instead of calling the geometry functions; for a gradient use `gradGeom`
  (`axial {x0,y0,x1,y1}` | `radial {x0,y0,r}`) and the 64 `gradSamples` RGB triples (`addColorStop(i / 63, env.color(hex2 hex2 hex2))`); the pattern, rectangular-gradient
  and blip branches call the `pattern` / `rectGradient` / `blipTiles` exports with `(page, item)`. For a shape with `fx`, `paintShapeEffects` must paint the body with the
  LOCAL item: the hints are already the local ones and the shape's own `shadow` is not drawn.
- `paintGlyphsFx`: `box` and `fxFillGeom` / `fxOutlineGeom` replace `runBox`/`gradientGeometry`; the layer is `view.fxLayers(page, item, ppt)` (RGBA, straight alpha, `x0`,`y0` in twips,
  `ppt` as the TS computes it; `null` for status 1; cache by (page, item, ppt) with a cap).
- `paintPageBackground`: `pageFillTileTw(page, devicePxTw)`, then `pageFillRaster(page)` for gradient and pattern fills (null for image fills); cache by fill key in the shell (cap 32).
- `js/render.mjs` is the handle layer (free + `FinalizationRegistry`, result decoders); port it to TS or wrap it. Image bytes: `image_bytes` is not in this frontier
  (the layout handle will own it); `setImageBytes(image, bytes, naturalW, naturalH)` is what a decoded view needs for tiled blips.
- Statuses: 0 ok, 1 nothing to paint (a valid answer), 4 limit (paint nothing, keep going), the rest are bugs of the caller.
- The picture layer of `canvas-picture.ts` still needs the drawingml mask exports (Not done, first item).

**V (painter parity).**
- Use `e_support/shell.rs` and the recording context of `tools/capture-e.mjs` (same log encoding: name, args as `n`/`s`/`a`) as the starting point of the Node log comparison
  for the whole `paintItems`; the glyph and shape slices are already equal on 1,500 + 2,500 generated items.
- The TS-adapter path is ready: frozen TS `LayoutResult` → generated TS encoder (`encodePage(msg, state)` with `tables.faces/images/…` filled by the adapter) → `newView(g).pushMessage` →
  `setFaceBytes`/`setImageBytes` → every export; then compare the export bytes with the frozen `patternRaster`, `rectGradientRaster`, `blipTiles`, `fxLayers`, `gradientTileRgba`,
  `patternRgba` on the same items (Node), and Chromium pixels for the paths that read pixels back.
- The wasm fixtures (`uo_render_fixture`) are the smoke inputs; the pinned checksums are in `render_paint.test.mjs`.

### Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| E | encoder + hints + decoder + view + wasm exports + parity capture + R-P2/M1–M4/no-panic + record (Sonnet, hybrid mode) | one session (wall time not recorded) | ≈ 170 (own tally) |

Parity rounds: `e_hints` equal on the first replay (9 facets). Rework in the session: a 40-byte leak in my own test (a result handle not freed; none in the library),
a `Path` span leniency found by my no-panic test (E-D11), `assert.throws` added to the WebKit shim, two test expectations (a NaN scale answers `none`; `set_face_bytes` parses before it looks the face up).
No port bug was sent back by a verifier.

## Milestone 1 — frontier D (TS painter on decoded pages)

Sonnet (implementer, 2026-11-09), plan by the planner of the same workflow run. Closed scope: the frozen `canvas.ts` + `canvas-picture.ts` ported line by line over the decoded page message, with the wasm exports as the only source of geometry, rasters and masks.

### Built

| File | What |
|---|---|
| `crates/uo-wasm/js/paint.ts` (new, erasable TS) | `paintItems`, `paintPageBackground`, `paintShape`, `snapThin`, `PaintCaches`/`defaultCaches`, `PaintEnv`, `RasterSource`, `PaintPage`; the usage snippet is in its header and in `write.d.ts` |
| `crates/uo-wasm/js/picture.ts` (new) | `canvas-picture.ts`: `needsPicturePainter`, `paintPicture`, `buildLayer`, `adjustedBitmap`, `paintShapeEffects`, `maskToCanvas`, `reflect` |
| `crates/uo-wasm/js/render.mjs` | `paintOps(view)` (every item-addressed export as a `Buf`), `RenderView.paint`, `rasters(ops)` (decoded facade, every `Buf` taken and freed), the decoders `mask`/`polygons`/`f64s` |
| `crates/uo-wasm/js/write.mjs`, `write.d.ts` | `layout.paint` = `paintOps` of the borrowed view; `layout.rasters()`; `PaintOps` and `RasterSource` typed |
| `crates/uo-render/src/view.rs` | `picture_polys`, `picture_silhouette`, `picture_bevel`, `picture_adjust`, `outer_shadow`, `reflection_alphas`, `inner_shadow`, `glow`, `soft_edge`, `blur` (all on `self.limits.draw`) |
| `crates/uo-render/src/paint/picture.rs` | the look → spec conversions, moved from uo-pdf: `shadow_spec`, `BevelParts`, `AdjustParts` |
| `crates/uo-wasm/src/render.rs` | the ten new exports (layouts in the file's last section and the module header) |
| `crates/uo-wasm/src/render_fixture.rs` | fixture kind 2 (an image with the whole look, an image with outer shadow + glow, a shape with inner shadow + soft edges, a shape with outer shadow + reflection) |

The TS → wasm call mapping of the picture layer (frozen `canvas-picture.ts` → `RasterSource`):

| Frozen call | Here |
|---|---|
| `bounds(look, w, h)` | `pictureBounds(page, item)` (image look or the shape's `{...fx, line?}`) |
| `shapeSubs` + `flattenPath(subs, [ppt,0,0,ppt,ox,oy], 0.35)` | `picturePolys(page, item, ppt, ox, oy)` → `Float64Array[]` |
| `fillPolygons(newMask(W, H), polys)` | `pictureSilhouette(page, item, ppt, ox, oy, W, H)` → `Float32Array` |
| `shadeBevel(img.data, sil, bevel)` | `pictureBevel(page, item, ppt, ox, oy, W, H, rgba)` → shaded RGBA |
| `applyImageEffects` + `applyPictureAdjust` | `pictureAdjust(page, item, rgba)` (null = nothing applies, the TS returns the bitmap) |
| `outerShadowMask(sil, box, spec, ppt)` | `outerShadow(page, item, sil, W, H, box, dirDeg, ppt)` (the `rotWithShape` rule stays in TS) |
| `reflectionAlpha(t, r)` row loop | `reflectionAlphas(page, item, top, hPpt, rows)` |
| `innerShadowMask`, `glowMask`, `softEdgeMask`, `gaussianBlur` | `innerShadow`, `glow`, `softEdge`, `gaussianBlur` (generic: no item) |
| `maskToCanvas` colours, `DASH[name]` | `coloredMask`, `dash` (existing exports; a small dash cache in the facade) |
| `patternRaster`, `rectGradientRaster`, `blipTiles`, `fxLayers`, `pageFillTileTw`, `gradientTileRgba`, `patternRgba` | `pattern`, `rectGradient`, `blipTiles`, `fxLayer`, `pageFillTileTw`, `pageFillRaster` (frontier E) |
| `presetOutline`, `morph` | not exported: `picture_subs`/`picture_polys` cover the outline; `canvas-picture.ts` never calls `morph` |

This closes the first item of frontier E's "Not done" (the drawingml mask exports).

### Evidence

- `tsc` (the CI line of frontier E plus `paint.ts picture.ts`): exit 0.
- Native: `render_native` 5/5 (the new `the_picture_layer_exports`, `the_picture_layer_exports_refuse_hostile_arguments`, M1 extended with the picture pass); `render_unsafe` (consumed buffers of the mask exports) ok natively and under **Miri** (`cargo +nightly miri test -p uo-wasm --features render-fixture --test render_unsafe --target x86_64-unknown-linux-gnu`: 1 passed, no leak); `uo-render` all features green (e_hints 1/1 = 9 facets equal, e_memory 3, e_nopanic 5, g_* all); `uo-pdf` `painter_parity` 3/3 after the helper move (60 generated specs, 6 laid-out documents, real documents: same counts as before).
- The wasm results equal the native ones (pinned FNV checksums in `render_paint.test.mjs`, Node and WebKit 26.5): `polys` cdbaaacd, `silhouette` a9af21dc, `bevel` 691efc5e, `adjust` 8d05a3e5, `outer_shadow` b51bc9e8, `reflection` cf5e2cca, `inner_shadow` 5a49b4ca, `glow` b1420741, `soft_edge` e34f4565, `blur` 4b9441ad.
- The three old render test files now instantiate with `writeImports().imports`, so ONE build (`count,write,render-fixture`) runs every render test (they failed with "Imports argument must be present" on that build before).
- Parity of the painter: see frontier V below.

### Memory

- **M1 (native)**: `m1_every_export_returns_the_live_bytes` runs `picture_pass` (20 calls: ok, refused, consumed inputs, null handles) twice and asserts the live bytes return exactly.
- **M4 (10,000 pages, `UO_PAINT_M4=10000`)**: 10,000 pages cycled through `encodePage` → `decodePage` → background + items over 6 layouts (46 distinct pages): 8,407,239 draw calls, live bytes 14,991,615 → 14,991,615 (identical), `memory.buffer` 19,988,480 → 19,988,480 (flat after the 200-page warm-up); after `layout.free()`/`doc.free()` the live bytes equal the pre-layout value. The finalizer path: 10,000 rounds of abandoned `Buf`s (never taken or freed) return to the baseline exactly (7,721,330 B). The picture layer (fixture 2, 16 + 48 paints at 8 scales through the mask exports): live 6,925,640 → 6,925,640, `memory.buffer` flat.
- **M5 (caps)**: every cache is filled past its cap through constructed pages and never exceeds it (`paint_memory.test.mjs`, caps 8/8/16/8/4/4): page-fill tiles 8, shape layers ≤ 9 (cap + 1 for an instant: the TS rule clears when over the cap, after the insert), effect layers 16, images with picture layers 8, layers per image ≤ 5 (cap 4 + 1), adjusted bitmaps 4. Defaults: 32 / 32 / 256 / 64 / 8 / 16.
- What stays after `env.free()` (6.9 MB) is process-wide (hyphenation and font tables of the languages the documents used, built on first use): not the painter's, reported in the test output.
- A defect found by the hostile test and fixed: `picture_bevel` computed `w * h * 4` in unchecked `u64` (a debug-build overflow panic for `w = h = u32::MAX`): now `checked_mul`.

### Declarations

| # | Decision | Why |
|---|---|---|
| D-1 | `shadow_spec`, the bevel spec and the `PictureAdjust` build moved from uo-pdf into `uo_render::paint::picture` (`shadow_spec`, `BevelParts`, `AdjustParts`); uo-pdf calls them (no duplicate). Arithmetic untouched | two painters, one conversion; `painter_parity` 3/3 guards it |
| D-2 | The painter lives in `crates/uo-wasm/js/` next to `write.mjs`/`render.mjs`, erasable TS with `import type` only from `gen/render.ts`; the handle layer comes in through the structural `RasterSource`/`PaintEnv` interfaces, not by importing `write.mjs` | TS cannot type `./write.mjs` through `write.d.ts` (TS7016); at the switch `write.d.ts` may become `write.d.mts` (maintainer) |
| D-3 | Mixed ABI for the masks: item-addressed where the look supplies the parameters (polys, silhouette, bevel, adjust, outer shadow, reflection rows), raw parameters for the generic ones (inner shadow, glow, soft edge, blur). A mask is `w*h` f32 LE, a layer `w*h*4` bytes, a length that does not match is status 5 | RQ12 said "a thin TS facade with the same signatures"; the item-addressed form keeps the look inside wasm |
| D-4 | `outer_shadow` computes the shadow of `look.shadow` whatever its kind; the `kind === 'outer'` test and the `rotWithShape` direction rule stay in TS | those are the TS's own `if`s |
| D-5 | Status 4 (a limit) and 1 on a mask export make the painter skip THAT effect (a missing silhouette is a zero mask, a missing layer paints nothing) and keep going | the TS has no limits (it exhausts memory); record E: 4 = paint nothing, keep going |
| D-6 | `rasters().rectGradient` answers null for status 5 (a NaN side, G-D6) instead of the frozen NaN-size canvas; no corpus exercised it | an invalid size must not throw in a paint loop |
| D-7 | Caches (`PaintCaches`): page-fill tiles and shape-effect layers are global like the TS module caches (cap 32, clear when over); effect layers are per layout (`WeakMap` on `owner`) keyed `page:item` with a cap of 256 (the TS kept a `WeakMap` per item: no cap); picture layers per layout and image id (cap 8 per image as the TS, plus 64 images); adjusted bitmaps per image id (cap 16, the TS had none). The shape-layer key replaces `style.cust` by the decoded geometry `tables.cust[id]` and the blip's `data.length` by `tables.images[id].byteLength` | a custom geometry's id is per layout, the TS key inlined the object; the equivalence classes stay the TS's |
| D-8 | `grad && !gradGeom` is the rectangular gradient (hints.rs: `grad_geom` exists only for a gradient that is not rectangular and has no pattern or blip); a fill gradient without its geometry hint paints no fill in the text-effect path (`paintOf` returns null) | the hints are all-or-nothing per item |
| D-9 | Glyph hints as `e_support/shell.rs`: `outlines` and `remap` by index (a `Map` built only when present), `joins` only when `'fontKerning' in ctx`, the ZWNJ written as the escape `\u200C`, `upem` from `tables.faces[face].upem`; a run whose face is missing from the tables paints nothing | the contract of E-D13 |
| D-10 | The shape of an effects item is painted in `local` mode: the hints are already the local item's (E-D2), so only the shadow and `fx` are skipped and `x`, `y`, `rot` read as 0 | canvas.ts:430 |
| D-11 | The page background reads the source-tile size from the export's raster (`pageFillRaster` is `gradientTilePx`) and, on a cache hit, from the cached canvas; an image fill uses `env.image(id)` with `imagePx` | the TS computed `gradientTilePx` itself |
| D-12 | CI: the new test files, the build line and the tsc line are listed below and NOT added to `.github/workflows/rust-core.yml` (another frontier is editing it) | orchestrator's call |

CI lines to add: `tsc … crates/uo-wasm/js/paint.ts crates/uo-wasm/js/picture.ts`; `cargo build -p uo-wasm --features count,write,render-fixture --target wasm32-unknown-unknown --profile wasm` then `node --expose-gc --test --test-concurrency=1 crates/uo-wasm/tests/{render_paint,render_boundary,render_ts_codec,write_section,write_api,paint_parity,paint_memory}.test.mjs`; the WebKit runner on `render_paint`/`render_boundary`; `cargo test -p uo-wasm --features render-fixture,write --test render_native --test write_native`; the Miri line of `render_unsafe`.

### Not done

- The wiring in the viewer (`engine="rust"`, queue item 4): the snippet in `paint.ts`'s header is the glue; nothing in `universal-document-viewer.ts` changed.
- The painter has not run in WebKit/Safari (the handle layer and the mask exports have, WebKit 26.5); Chromium only for the pixels. Real Safari 15 is the gate's.
- `write_section.test.mjs` still passes the 3-language OPTS (the plan proposed registering the 7 pattern files and dropping `hyphenationLanguages`; V's own harness does, the existing test was left alone).
- The size of the new exports: see "Size" below; the G7 budget is the maintainer's.

### Size

`--features write --profile wasm`: raw 3,966,580 B, gzip -9 1,409,092 B, against the last recorded 3,960,432 / 1,405,088 (`size-optimization.md` item 3a): **+6,148 B raw / +4,004 B gzip**, an upper bound for the ten exports (other frontiers landed code since). `wasm-tools validate` with the Safari-15 feature set: valid. The test build (`count,write,render-fixture`) is 4,371,733 B.

### Process metrics (D)

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| D + V | one implementer session (Sonnet), planner's plan followed | ≈ 55 min wall (20:43–21:40, 2026-11-09), machine shared with two agents | ≈ 170 |

Rework: clippy (`too_many_arguments` on two view methods, `f64::max` ban in a test, import order by rustfmt); the hostile-argument test found the `picture_bevel` overflow (above); the M4 test needed a warm-up cycle (the environment and process-wide tables are built on first use) and per-item cache watching for M5. No port bug was found by a verifier in the painter; frontier V found one adapter bug (below).

## Milestone 1 — frontier V (painter parity)

Sonnet, same session. The draw calls of the TS painter of decoded pages (frontier D) against the frozen `paintItems`/`paintPageBackground`, on a recording context, over the committed fixtures, generated pages and the whole Word oracle, plus the export bytes and Chromium pixels.

### Built

| File | What |
|---|---|
| `crates/uo-wasm/tests/support/recording-context.mjs` | the recording context: logs every method and property set (capture-e's n/s/a encoding), a real affine stack, tokens for gradients/patterns/canvases, `OffscreenCanvas`/`DOMMatrix` stubs with a creation-ordered token and a deterministic pseudo-random RGBA backing; `putImageData` writes it, `drawImage` logs the source token and the FNV of its backing |
| `crates/uo-wasm/tests/support/paint-common.mjs`, `paint-d.mjs` | the four configurations (kerning × scale 1/15 and 2.5/15, with alpha 0.6 on the second scale), the fixture list, the open-font environment, the D-side env and page decoding |
| `crates/uo-render/tools/capture-v.mjs` | the frozen side: TS layout of the 74 fixture documents, frozen painter on the recording context, one digest per document and configuration → `tests/captures/v_paint.capture` |
| `crates/uo-render/tools/lib/ts-adapter.mjs` | R-P2c: a frozen `LayoutResult` → render.uos messages through the generated encoder (the TS twin of `to_wire.rs`), `pushInto(view, adapted)` |
| `crates/uo-wasm/tests/paint_parity.test.mjs` | Part A (CI, no TS root), Part B (generated + oracle, call-by-call diff + R-P2c), Part C (export bytes) |
| `crates/uo-wasm/tests/browser/paint_pixels.mjs` | Chromium: the frozen `canvas.js` and `paint.ts` bundled by the frozen esbuild into `$CARGO_TARGET_DIR/v-pixels/`, `getImageData` compared byte for byte |
| `crates/uo-wasm/tests/paint_memory.test.mjs` | M4 / M5 of the painter |
| `crates/uo-testkit/js/testkit.mjs` | `readCapture(path)` (additive) |

### Evidence (platform: win32, Node v24.15.0, V8 13.6, x64)

| Corpus | Layouts / pages / items | Draw calls | Mismatches |
|---|---|---|---|
| **Part A**: 74 committed fixture documents, laid out by the wasm engine (open fonts, the 7 hyphenation patterns, no `hyphenationLanguages`), decode → D painter, against `v_paint.capture` | 74 / 579 / 55,322 (glyphs 47,334 · rect 6,372 · line 939 · shape 377 · clip 300) | 456,698 per configuration (× 4 configurations, 296 digests) | **0** (and a perturbed painter fails 288 of them: the digests are sensitive) |
| **Part B, generated**: 200 `genSpec` seeds (every painter branch: images PNG/BMP/fake JPEG, picture looks, shapes with presets/custom/gradients/patterns/blips/shadows/effects, paths, clips, xforms, page fills, text effects, hostile numbers), frozen painter vs adapter → Rust view → `encodePage` → `decodePage` → D painter | 200 / 405 / 10,659 (image 952 · glyphs 5,821 · shape 951 · xform 617 · line 535 · rect 587 · clip 802 · path 394) | 1,577,904 (4 configurations) | **0** log, **0** exception mismatches |
| **Part B, oracle**: the WHOLE Word oracle with `UO_V_ORACLE=1 UO_V_ORACLE_STRIDE=1` (259 groups, 2,742 documents, laid out by the frozen engine with this machine's installed fonts), same path. The default stride 13 (`UO_V_ORACLE=1` only) is 347 layouts / 505 pages / 11,689 items / 597,358 frozen calls (x 4 configurations), 0 mismatches, re-run 2026-11-09 after the fixes | 2,742 / 4,629 / 140,089 (glyphs 115,043 · rect 22,107 · image 604 · shape 410 · line 429 · clip 1,302 · xform 156 · path 38) | 9,880,928 (4 configurations) | **0** |

- **R-P2c** (the adapter): for every page of the three corpora the Rust view re-encodes what the adapter pushed; decoded by the TS decoder and with the hint fields stripped it equals the adapter's own message and the five resource tables are equal: **0 differences** on 2,942 layouts (200 generated + 2,742 oracle). Rust refused nothing.
- **Export bytes** (Part C, frozen function vs export, byte equality; generated corpus unless said): `patternRaster` 135 equal (+4 both null) · `rectGradientRaster` 147 · `blipTiles` 126 · `fxLayers` at ppt 1.5: 424 equal (+169 both null) and at 3.3333: 424 (+169) · `pageFillTileTw` 59 · `gradientTileRgba` 22 · `patternRgba` 16; oracle: `fxLayers` 196 equal at each ppt (+2 both null) (the oracle has no pattern/blip/rect-gradient shape facets). **0 mismatches, 0 refusals, 0 one-sided results**: the test asserts `mismatch + refused + frozenOnly + rustOnly = 0`.
- **Chromium pixels** (`UO_V_PIXEL_SEEDS=300 node crates/uo-wasm/tests/browser/paint_pixels.mjs`; the default is 60 seeds: 37 layouts, 78 renders, 204,663,507 px, 0 differing, and every category has renders with ink (pixels of the frozen render that are not the paper; the run fails if a category has renders but none with ink): pic 48/48, shapeFx 16/16, textFx 40/40, pageFill 12/12, shapeRaster 36/36, text 76/76 renders with ink, 64,515,535 ink px in all (the ink of a category counts the whole page, so text on the same page is included); re-run 2026-11-09 with the same result as below; chromium-1248 headless shell): 185 generated layouts, 390 renders (two scales), **1,030,732,444 pixels, 0 differing**. By path: picture layers 196 renders / 520,531,947 px; shape effects 82 / 217,781,747; text-effect layers 176 / 468,497,714; page fills 50 / 126,509,204; pattern/gradient/blip shapes 184 / 478,088,546; text 374 / 980,081,569.
- Handle layer in WebKit 26.5: `render_paint` 9/9 (including the picture-layer exports equal to native) and `render_boundary` 6/6 (the gc test skipped).

### Declarations

| # | Decision | Why |
|---|---|---|
| V-1 | `v_paint.capture` was taken once, on win32 Node v24.15.0 (`@platform` is recorded in it). Part A does NOT key on the platform: the replay side takes its gradient samples, geometry and rasters from the Rust hints and exports (wasm) and the D painter only calls `Math.round`/`Math.hypot` and fdlibm `sin`/`cos`, none OS-dependent. Verified: Part A passes (0 mismatches, 74 / 579 / 55,322 / 456,698) on win32 Node 24 and on Linux x64 Node 22.21 (WSL Ubuntu 24.04) | the earlier version skipped on any other platform, so on the Linux CI it would have checked nothing; the `**` caveat (E-D8) concerns the capture side only. If a future V8 changed the last bit, recapture with `tools/capture-v.mjs` |
| V-2 | The frozen painter keeps module-level caches; capture and replay paint the same pages in the same order in one process, the replay with fresh `PaintCaches`; the canvas tokens count across the whole run on both sides | otherwise a cache hit (no `createImageData`/`putImageData`) would differ for reasons unrelated to the painter |
| V-3 | The recording context does not rasterise: `drawImage` logs the source's token and the FNV of its stub backing; the pixels of the picture layer, shape effects and text-effect layers are compared by Chromium (`paint_pixels.mjs`), not by the Node log | stated limitation; both are needed for the parity claim |
| V-4 | `fontOf` of both sides keys on family/bold/italic/synthBold/synthItalic, never on `key` (a provider path in the TS, `Family|variant` here); an image's bitmap token is the sha256 of its bytes | the keys differ by design |
| V-5 | The end-to-end fixture corpus needs the 7 hyphenation pattern files registered and NO `hyphenationLanguages` (the 3-language option set of `write_section.test.mjs` makes 8 of the 74 documents lay out differently): a layout/font-policy difference, not a painter one | measured by the planner; the harness registers all patterns |
| V-6 | The oracle corpus runs through the ADAPTER (frozen layout with installed fonts → view), not end to end: the TS node provider over this machine's fonts and the wasm viewer policy may pick different faces. It covers all 259 groups (the plan's subset was ≈ 395 documents). It depends on the machine's fonts: local only | |
| V-7 | Adapter decisions: tables in first-use order across the layout (faces/images/text effects/page fills/custom geometries by object identity); `hasBytes = !!face.bytes`; the font file handed to the view is `face.bytes`, or the sfnt the TS font was parsed from (`font.view`) for a non-embeddable face, so the view computes the glyph hints the frozen painter computed from `face.font` (E-D9); `GradFill.rotWithShape` is `rotWithShape !== false` (the frozen `gradientGeometry` treats absent as true) | the first run of V found the last one: a radial gradient radius differed (1707.27 vs 2263.00) until the adapter mapped it as the TS reads it; an adapter bug, not a painter bug |
| V-8 | Chromium harness: pages whose numbers are not plain (non-finite or beyond 40,000) are skipped (the canvases would be hostile-sized); images the browser cannot decode (the generated fake JPEGs) paint nothing on both sides; the bundle is a build output under `$CARGO_TARGET_DIR/v-pixels/`; a harness bug (the table id of an image is its first use, not its spec index) was fixed before the numbers above | |
| V-9 | **Accepted differences: none exercised.** 0 unexplained mismatches in every row above. The one declared behavioural difference is D-6 (a NaN rectangular-gradient side: the frozen painter throws from `createImageData`, D skips the fill); no corpus item reaches it | |

### Not done

- Chromium covers 185 generated layouts at two scales; the oracle documents (few images) were compared by call log only, not by pixels, and the corpus of real documents with pictures (the Word corpus outside the oracle) was not run.
- The painter and the pixel comparison run in Chromium only: no Firefox, no WebKit, no real Safari.
- `paint_parity.test.mjs` Parts B/C and `paint_pixels.mjs` need the frozen TS root (and, for the oracle, this machine's fonts): they are local runs, not CI steps. Part A and `paint_memory` need no TS root and no fonts and are the CI gate (not yet in the workflow, D-12); Part A is platform-independent (V-1).
- Hostile-number pages are compared in Node only (0 exception mismatches; they are not rendered).

### Process metrics (V)

Parity rounds: Part A equal on the first run; Part B found one adapter bug (V-7, one round, 4 mismatches in 20 seeds → 0 in 200); the Chromium harness needed one fix of its own (V-8) before 0 differing pixels. Model Sonnet; time and tool calls are shared with frontier D above (one combined row: the per-step split of D and V was not recorded). Fix round 1 (reviewer findings): Part A made platform-independent, one-sided/refused export results counted as failures, ink check in the Chromium harness, record corrections; ≈ 30 min. The new `RenderView` methods of D are covered by uo-wasm's `render_native` (M1 + hostile arguments) and `render_unsafe` instead of `e_memory`/`e_nopanic` test files of uo-render itself.

## Milestone 1 — Shipping, CI and WebKit (painter)

Frontier "Shipping and CI" (2026-11-10, Sonnet); the licence and CI parts are in uo-bindings.md section 8. Here: what the painter (D) and its parity (V) got.

**CI lines added** (`.github/workflows/rust-core.yml`, none could run on GitHub from here; each command was run locally, see "Commands"):

- job `wasm`: one `count,write,render-fixture` build runs write_api, write_cancel, write_section, `render_paint`, `render_boundary`, `render_ts_codec`, `paint_parity` (Part A only: no `UO_TS_ROOT`), `paint_memory` and the new `browser/shims.test.mjs`; the WebKit step builds the same module and runs render_paint, render_boundary, write_section and paint_parity (Part A) in WebKit.
- job `painter` (new): checkout of `main` as the frozen engine (built as in layout-parity), then the **same-run Linux recapture of the frozen side** diffed against the committed `v_paint.capture` without its `@platform` line, `paint_parity` with `UO_TS_ROOT` (Part A + Part B at `UO_V_SEEDS=50` + Part C facets), and `paint_pixels.mjs` in Chromium and in WebKit (`UO_V_PIXEL_SEEDS=30`).
- job `miri`: `render_unsafe` (`--features render-fixture`). The `codec` job's strict `tsc` now also covers `paint.ts`, `picture.ts`, `viewer.ts`, `viewer.check.ts` and `write-engine.d.ts`.

**Decision (Part A platform).** The brief said Part A was pinned to win32, Node v24.15.0 and V8 13.6. That pin was already gone: since V-1 the replay depends only on the Rust exports and on `Math.round/hypot/sin/cos` through the recorded frozen digests, and the test has no platform skip (it passed on WSL Node 22.21.1). What really varies is the frozen side's capture, so the CI proves *that* instead of pinning: the `painter` job recaptures the frozen painter on Linux and Node 24.15 and diffs it with the committed capture (equal except `@platform`; measured here on Windows Node 24.15, 19 s, and on Linux WSL Node 22.21.1: byte-equal, 74 / 579 / 55,322 / 456,698). The Math-override approach (`d2b-math-override.mjs`) was not needed for Part A in Node; `run.mjs` gained an optional `UO_MATH_OVERRIDE=1` only to attribute a difference in another JavaScript engine (it was not needed in WebKit, below).

**Parts B/C in CI at 50 seeds** (200 locally): measured 50 layouts, 102 pages, 2,556 items, 393,434 frozen draw calls x 4 configurations: 0 log mismatches, 0 exception mismatches, 0 wire differences; every facet equal (blipTiles 35, fxLayers 97+97, patternRaster 34, rectGradientRaster 49, pageFillTileTw 12, gradientTileRgba 4, patternRgba 1; refused/frozen-only/export-only 0); 67 s.

**WebKit (Playwright 1.61, WebKit 26.5 on Windows, headless).**

- Handle layer: render_paint 9/9, render_boundary 6 ok + 1 skip (needs `--expose-gc`), write_section 2/2.
- **Part A in WebKit: 74 documents, 579 pages, 55,322 items, 456,698 draw calls x 4 configurations, 0 mismatches** (22 s; Node: 11 s). Sensitivity: `UO_V_NEGATIVE=1` gives 288 mismatches and exit 1, the same count as Node. No named difference: JavaScriptCore's `Math.sin/cos/hypot/pow` gave the frozen digests unchanged, so `UO_MATH_OVERRIDE` is not needed and the CI line does not set it.
- **Pixels (`paint_pixels.mjs --browser webkit`)**: 60 seeds = 37 generated layouts, 78 renders at two scales, 204,663,507 px; 300 seeds = 185 layouts, 390 renders, 1,030,732,444 px; **0 differing pixels, 0 errors, every category with ink** (pic 196 renders, shapeFx 82, textFx 176, pageFill 50, shapeRaster 184, text 374). Chromium 156 with 60 seeds: 0 differing (the 300-seed Chromium record above is unchanged).
- **Paths exercised.** WebKit on Windows has no `OffscreenCanvas` and no `ctx.fontKerning`: both painters took the `HTMLCanvasElement` fallback (the Safari-15-like path) and never the kerning-joins path (`fontKerning` present). On the Linux CI WebKit (GTK/WPE) `OffscreenCanvas` may exist and the run then takes the other path; the harness prints which one it used. Real Safari 15 / iOS (BrowserStack) is still the gate item.

**Browser runner changes** (`tests/browser/run.mjs`, shims): `.ts` served through `node:module` `stripTypeScriptTypes`, `?__fs=stat|list`, shims for `node:crypto` (synchronous SHA-256), `Buffer` subset, `node:os`, `readdirSync/existsSync/statSync`, `path.resolve/relative/basename/sep`, `url.pathToFileURL`; `UO_CHROMIUM`, `UO_MATH_OVERRIDE`, `UO_V_NEGATIVE`, `UO_PAINT_M4` forwarded; `shims.test.mjs` proves Buffer and SHA-256 against Node on 200 random inputs each (edge lengths 55/56/63/64/119/120 included). `paint_pixels.mjs` takes `--browser chromium|webkit` and `UO_PLAYWRIGHT`.

**Commands** (core/, `UO_WASM_PATH` = a `count,write,render-fixture` build; `PW` = a Playwright 1.61 package folder):
`UO_WASM_PATH=$W UO_PLAYWRIGHT=$PW node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/paint_parity.test.mjs` ·
`UO_TS_ROOT=$F UO_WASM_PATH=$W UO_V_PIXEL_SEEDS=60 node crates/uo-wasm/tests/browser/paint_pixels.mjs --browser webkit` ·
`UO_TS_ROOT=$F node --max-old-space-size=8192 crates/uo-render/tools/capture-v.mjs --out-dir <existing dir>` (the directory must exist) and `diff` without `@platform` ·
`UO_TS_ROOT=$F UO_V_SEEDS=50 UO_WASM_PATH=$W node --expose-gc --test --test-concurrency=1 crates/uo-wasm/tests/paint_parity.test.mjs`.

**Not covered / for the maintainer.** The painter CI job depends on `main` staying at the frozen commit (a later push to `main` changes the same-run recapture). Chromium in the `painter` job is the Playwright-bundled revision, not the 1248 used here. The `viewer` job and the Playwright install lines are untested on GitHub.

### Process metrics (Shipping/CI/WebKit)

About 95 tool calls; rework: 3 (a regex mangled by the shell's backslash halving in `build-pkg.mjs`, `capture-v.mjs --out-dir` needs an existing directory so the CI step does `mkdir -p`, a relative `CARGO_TARGET_DIR` breaks `viewer_switch.mjs`'s esbuild paths: use an absolute one); WebKit Part A passed on the first run after the shims.

## Types (T)

Frontier T, Opus, 2026-11-03, inside uo-docx-layout F0 (record there: "Milestone 1 — F0").

### What exists (`core/crates/uo-render`)

`forbid(unsafe_code)`, `deny(cast_possible_truncation, cast_sign_loss, indexing_slicing)`; fmt,
clippy `-D warnings` (native and wasm32) and the ban scripts clean.

- `ids`: `Sym`, `FaceId`, `ImageId`, `TextFxId`, `PageFillId`, `CustId` — dense, first-use order,
  `NonZeroU32` (`index + 1`) so `Option<Id>` is 4 bytes; `from_index`/`index`/`raw`/`from_raw` for
  the codec.
- `sym::SymbolTable`: UTF-16 interning, lone surrogates kept, `since(n)` for the per-generation delta.
- `opt::OptF64`: an optional number in 8 bytes (below). Since 2026-11-10 "absent" is the integer 0 and a value is its canonical bits + 1, no NaN payload (G22, uo-bindings.md).
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
| T3 | `OptF64` for every optional number of an item (absent = a signalling-NaN payload JS never produces; **2026-11-10: now the integer 0, a value is its bits + 1, G22**; values are NaN-canonicalized on entry, as the codec does) | `Option<f64>` is 16 B; RA1.2 needs absent apart from NaN |
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

## Slides extensions (plan §4.2) — design

*(Opus architecture pass, 2026-11-17; design only — no uo-render code changes in this pass, the
implementation is the next wave, sub-frontiers R-S1–R-S5 below.)* Producers: `uo-dml-render` (shapes, for
Slides and Sheet drawings), `uo-pptx` (slide frame, slots), `uo-charts`. Decisions applied (T1, T2, P1, W0)
and the Slides model are in `uo-pptx.md`; the shape boundary and the measurement protocol in
`uo-dml-render.md`. TS line numbers are of `slides/packages/slides-core/src` in the frozen
`ts-ref-092` (`b92b60fa`).

### S1. Why a second list type (and not new `Item` variants)

Write's `Page`/`Item` are *semantic* page items (glyph runs with absolute glyph x, rects, shapes as specs)
because Write's painters were written against them. The Slides painter is not: `render.ts` is 2,662 lines of
canvas calls whose results depend on canvas state (`getTransform` in 14 places, canvas shadows used through
an off-screen offset, `ctx.filter = blur(…)`, composite modes, full-canvas layers, `getImageData`,
`measureText` at device scale). Rebuilding that as semantic items would be a rewrite with its own parity
risk. So:

- **P-DL1 (the canvas IR rule).** The Slides list (`SlidesList`) is the TS's canvas call sequence. Every value
  the TS computed from **model data** is precomputed by the producer with the TS arithmetic (bits); every
  value it computed from **canvas state** (current transform, device `measureText`, pixels, canvas size,
  capability tests) is a **symbolic operand** the painter resolves; only pixel-reading or device-measuring
  code is an **`Effect`** executed by the shell with the TS code. Parity of the draw-op stream (plan §4.3,
  the primary gate) then holds by construction: the painter issues the same calls with the same arguments.
- **W0 kept**: `SlidesList` is its own schema (`core/schema/slides.uos`, magic `UORS`, its own
  `SCHEMA_HASH`); `render.uos`, Write's bytes and hash do not change. Shared: the generator
  (`core/tools/uo-schema`), the runtime (`uo-codec`), `SymbolTable`, the delta mechanism (§3.2), `ImageId`
  semantics. Sheet's viewport list (C4) uses `SlidesList` too (grid items are a small subset: rects, lines,
  text runs, clips).
- `GeomEval::Drawingml` (plan §4.2 last bullet) is **not needed** by Slides or Sheet drawings: the emitter
  evaluates presets through `uo-drawingml` and emits paths. The variant stays for a future Write use.

### S2. Types (`uo_render::slides`, sketch for R-S1)

```rust
pub struct SlidesList {
    pub view: ViewBox,                   // ox, oy, scale, sw, sh (the TS `SlidePx`): slot math and shell checks
    pub items: Vec<SItem>,               // paint order
    pub paths: Vec<PathOp>,              // path arena; items hold Span
    pub stops: Vec<(f64, Sym)>,          // gradient stops (offset, CSS colour string as the TS built it)
    pub texts: String,                   // run texts; items hold byte Span
    pub slots: SlotTemplate,             // S6
}
pub enum PathOp { MoveTo(f64, f64), LineTo(f64, f64), BezierTo([f64; 6]), QuadTo([f64; 4]),
                  Arc { cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool },
                  ArcTo([f64; 5]), Ellipse([f64; 7], bool), Rect([f64; 4]), Close }
pub enum SItem {
    // state and transforms: the TS primitive sequence is kept (the recorder and Skia compose them in order)
    Save, Restore, Translate(f64, f64), Rotate(f64), Scale(f64, f64), Transform([f64; 6]),
    SetTransform([f64; 6]), ResetTransform,
    CaptureCtm(CtmReg),                       // reg := current transform (`const baseM = ctx.getTransform()`)
    TransformByInverseOf(CtmReg),             // `rl.transform(inv.a, …)` of a captured CTM (reflection)
    TransformUnshape { base: CtmReg },        // `ctx.getTransform().inverse().multiply(baseM)` (useBgFill, 577)
    // paint state
    GlobalAlpha(f64), GlobalAlphaMul(f64),    // `ctx.globalAlpha = ctx.globalAlpha * a` keeps the TS operand order
    Composite(CompositeOp),                   // source-over, destination-in/out, source-in, multiply, screen, darken, lighten
    Filter(Option<Sym>),                      // `blur(1.23px)` exactly as the TS string (toFixed(2) in the producer)
    FillStyle(Paint), StrokeStyle(Paint),
    LineWidth(f64), LineJoin(Join), LineCap(Cap), MiterLimit(f64), LineDash(Span),
    ShadowColor(Sym), ShadowBlur(f64), ShadowOffset(CtmOperand, CtmOperand),
    Font(FontRef), TextAlign(Align), TextBaseline(Baseline), ImageSmoothing(bool),
    // geometry
    BeginPath, AppendPath(Span), Fill(FillRule), Stroke, Clip(FillRule),
    FillRect([f64; 4]), StrokeRect([f64; 4]), ClearRect([f64; 4]),
    DrawImage { src: ImageSrc, s: Option<[f64; 4]>, d: [f64; 4] },
    // text
    Text(Box<TextRun>),                       // S4
    // offscreen layers (acquireLayer/compositeLayer): S3
    LayerBegin { layer: LayerId, fallback_end: u32 },  // no layer available → jump to `fallback_end` (the TS else-branch)
    LayerEnd { layer: LayerId, then: LayerThen },       // Composite { soft: Span<SoftReveal> } | Keep (drawn later)
    DrawLayer(LayerId), ReleaseLayer(LayerId),
    // shell-executed code
    Effect(Box<Effect>),                      // S5
    // animation
    SlotBegin(SlotRef), SlotEnd,              // S6
    MediaFrame { shape: u32, d: [f64; 4], s: Option<[f64; 4]> },  // `mediaFrame` over the cover (S6)
}
pub enum Paint { Css(Sym), Linear(GradId), Radial(GradId), Conic(GradId), Pattern(PatternId) }
pub struct Gradient { pub geom: [f64; 6], pub stops: Span }        // createLinear/Radial/ConicGradient arguments
pub struct PatternRes { pub src: PatternSrc /* Bitmap(BitmapId) | Image(ImageSrc) */, pub repeat: Repeat,
                        pub transform: Option<[f64; 6]> /* setTransform when supported, else ignored (TS test) */ }
pub struct BitmapRes { pub w: u32, pub h: u32, pub rgba: Arc<[u8]> }  // generated: pattern tiles, path gradients, TIFF
pub enum ImageSrc { Media(ImageId), Derived(ImageId, FxId) /* Effect::ImageFx result */, Bitmap(BitmapId) }
pub struct CtmOperand { pub add: f64, pub k: f64, pub axis: CtmAxis /* A | B */ }   // value = add + ctm.axis * k
pub struct FontRef { pub face: FaceIdS /* (family, bold, italic) */, pub size_px: f64 }
```

- **CTM operands and registers.** The off-screen shadow idiom (`shadowOffsetX = sx + m.a * K` with
  `m = ctx.getTransform()`, `K = 20000`, then `translate(-K, 0)`: glow 661, shape shadow 908–931, inner
  shadow 1121, table shadow 2329) becomes `ShadowOffset(CtmOperand { add: sx, k: K, axis: A }, …)`; floating
  addition is commutative, so `m.a * K - dx` is `add = -dx` exactly. Captured matrices (`baseM`, the
  reflection's `T.inverse()`, the unshape product) use ≤ 4 registers per list. The painter resolves them from
  its own context; uo-pdf from its CTM stack. No pixel is read for any of these — they are **not** Effects.
- **Paint strings**: CSS colour strings are the TS's (`'#'+val`, `rgba(r,g,b,a)` with JS number→string, the
  `rgb(…)` of `shadeRgb`), produced with `uo_jsnum::format`; the painter assigns them unparsed (§4.2's
  per-painter parse rule; uo-pdf parses as the canvas would, a table like §4.2's is part of R-S5).
- **Capability branches** stay in the list: no layer (`acquireLayer` → `null`: tests, old browsers) jumps to
  the TS fallback items; `pat.setTransform` absent ignores the pattern transform; `'fontKerning'`/
  `letterSpacing` absent skip their state changes (TextRun, S4). The recorder runs both ways.

### S3. Layers (acquireLayer / compositeLayer / discardLayer)

A layer is an offscreen canvas of the **current target's size** with the target's transform (render.ts:154).
`LayerBegin` switches the target (stack); `LayerEnd { Composite }` applies the soft reveals (`applySoftReveal`,
shell) and draws it onto the previous target under `setTransform(1,0,0,1,0,0)` (`compositeLayer`, 193);
`LayerEnd { Keep }` + `DrawLayer` cover the soft-edge mask (`mk` drawn with `destination-in` and a blur
filter into the shape layer, 681–707) and the reflection (709–757). Pool and eviction are the painter's
(`layerPool`, M5). Uses: shape soft-reveal (`paintShapeList` 352–369), reflection/soft edge (`paintShape`
589–604), WordArt shadow (inside `Effect::Warp`), animated chart parts (790).

### S4. `TextRun` (decision T1)

`Text(TextRun)` is one `drawText` call (render.ts:1636–1707) with its model-derived values precomputed:

```rust
pub struct TextRun {
    pub text: Span, pub font: FontRef, pub x: f64, pub y: f64,          // y = baseline incl. `rise × scale`
    pub laid_w: Option<f64>,       // `r.w × scale` when text === r.text (fit, decorations); None for unit pieces
    pub fit_allowed: bool,         // `!ls && text === r.text && !st` (the painter adds its `prevLs !== undefined`)
    pub letter_spacing_px: f64,    // `r.spc × scale` (0 = none)
    pub kern: bool,                // `fontKerning = kern ? 'normal' : 'none'` when supported
    pub fill: Paint, pub alpha_mul: Option<f64>,                         // `fillA < 1` → globalAlpha × fillA
    pub shadow: Option<TextShadow>, // color `rgba(…)`, blur `blurRad × scale × 0.8`, offsets (save/restore around fillText)
    pub fill_text: bool,           // false when fillA ≤ 0 (outline only)
    pub outline: Option<(Sym, f64)>,                                     // strokeText, width `max(0.5, w × scale)`, join round
    pub underline: Option<Decoration>, pub strike: Option<Decoration>,  // color, width `max(1, sizePx/16)`, y `round(…)+0.5`
}
```

The painter replays `drawText` in the TS order, including the fit: `n = [...text].length; d = laid_w −
measureText(text).width; |d| > 0.25 → fit = d / n`, `letterSpacing = ${ls || fit}px`, the restore of
`letterSpacing`/`fontKerning` at the end, and decoration widths `laid_w ?? measureText(text).width`. The
CSS font string is built by the painter from `FontRef` with the shell's `fontStack` (T2):
`[italic ]{700|400} <size_px>px <stack>` with `size_px` printed by JS (the producer only passes the f64).
PDF (R-S5): glyphs through `uo-fonts`, advances from the `MeasureTable` filled by `uo-fonts`, the fit as a
character spacing (`Tc`) — measured against PowerPoint PDF in its own oracle gate (P1).

### S5. `Effect` items (shell-executed; PDF lists them missing, P1)

| Effect | TS code moved to the painter | Parameters precomputed |
|---|---|---|
| `Bevel` | `applyBevel` + `edt1d` + `SRGB_LIN`/`LIN_SRGB` (1295–1387; `getImageData` of the device rect) | silhouette path span + rule, box, `BevelSpec`, light table id (uo-drawingml `light3d`, as a wasm export or the TS table) |
| `ImageFx` | `fxImage` (448–503; recolour of the decoded pixels) | image, `ImageFx` (duotone, alpha, gray, biLevel, lum, clrChange) → derived image id, cached by the shell |
| `Warp` | `paintWarp` from the layout on (1868–1984) + `text-warp.ts` (594: ink metrics at 10 px/pt, line rasters at `k ≤ 4096`, strips/cells) | `WarpLine`s (runs with `FontRef`s, x, colours, strokes, pattern, spc, alpha, baseline, sizePt, lnPct, align, trailing), prst, adj, `fromWordArt`, box, `pxPerPt`, shadow parameters |
| `MetafileRop` | the raster-op path of `metafile.ts` (312–330, `getImageData`) | ROP code, rect, operand bitmap |
| *(slot-driven)* `Reveal` / `SoftReveal` | `reveal.ts` (`applyReveal`, `applySoftReveal`) | filter, transition, progress, box — from the slot value (S6) |
| *(between slides)* transition | `transition.ts` `paintTransition` | type, attrs, progress, direction — from the `SlideShow` frame, not a list item |

### S6. Animation slots

The TS paint calls lazy hooks with geometry it computes while painting (`anim-engine.ts:83`
`ShapeAnimHooks`). The list keeps that structure as **slot markers** around the items each hook governs, and
a per-frame **slot table** supplies the hook results.

| Slot kind | TS site | Base geometry (computed by the producer, in px, normalized by `boxBase`) | Value applied by the painter |
|---|---|---|---|
| `Shape` (one per animated or opacity-faded shape) | `paintShapeList` 336–371: `shapeOpacity`, `hooks.all(base)` | xfrm box × scale + origin; `rot` | `op` (≤ 0 skips, < 1 multiplies alpha), `all: PartState` → `applyPart` (alpha, translate/rotate/scale about the box centre, reveal filters; soft reveals → the shape goes through a layer, S3) |
| `Background` | `paintShape` 571: `hooks.bg(base)` | the shape base | `applyPart` on the body only |
| `Paragraph(i)` | `paintText` 1712–1734, 1759–1764: `hooks.para(i, base)` | union box of the paragraph's runs/bullet per line (`pbox`), line top/bottom from baseline ± 0.8/0.2 height | `applyPart` around the paragraph's lines; `!visible` skips them |
| `Unit(k)` (letter/word) | 1767–1833: `unit.state(ref, base, word)` | pieces measured at **device px** (measurement A5, two-phase): union per unit, `size = max sizePx`, `y = by − size × 0.8` | `applyPart` around the pieces; the bullet follows the paragraph's first unit |
| `ChartPart(j)` | 776–797: `chartParts[j].state(base)` | `chartPartBounds` (uo-charts, measurement A11) or the frame for the background part | layered chart part drawn with `applyPart` |
| `Media` | `mediaFrame(shape)` 343–345 | the picture rect | the current video/GIF frame and its alpha over the cover (`MediaFrame` item) |

- **Slot table** (one per frame): `SlotTable { values: Vec<SlotValue> }`, `SlotValue = Static | Hidden |
  State(PartStateWire)`: `visible, opacity, x, y, w, h, rot, sx, sy, mx, my, filters: [(filter, transition,
  p)]` — exactly the fields `applyPart` reads (render.ts:116–140). `op` for shape slots. Encoded per frame in
  a tiny `SlotFrame` message (`UORF`), no resources.
- **Slots are per viewport**: `AnimBase` divides px geometry by the slide's px size (`(x + w/2 − ox) / sw`),
  so the template (bases) belongs to one (scale, origin) list and `evaluate_slots` reads it; a resize
  re-emits the list and the template.
- **Re-emit rule.** When a slot value carries a field `applyPart` does *not* apply — `fillColor`, `lineColor`,
  `textColor`, `fontWeight`, `fontStyle`, `underline`, `fontFamily`, or `fontScale ≠ 1` (which re-runs the
  layout with `sizeScale`, 1634) — the producer **re-emits that shape's sub-list** for the frame with the
  values applied (only that shape crosses; the frame message carries `(shape range, replacement items)`).
  Colours are re-emitted rather than patched because `ov.fill` changes the paint *kind* in `paintBody`
  (a gradient fill becomes the animated solid colour).
- **Who evaluates**: `uo_pptx::evaluate_slots` (B6) — the anim engine port runs over the template's bases;
  playback/show state gives the time. The painter never evaluates SMIL.

### S7. Scale-dependent decisions (why the list is per scale × origin)

Found in `render.ts`, `raster.ts`, `patterns.ts` (a re-export of `uo-drawingml`'s tables), `text-warp.ts`:

1. Every coordinate is `ox + X × scale` in JS doubles (541–544 and every paint function).
2. The text layout box is `(cx × scale) / scale` and text rectangles/vertical swaps are derived in px
   (1575–1634) → layout per scale (`uo-pptx.md` correction 2). Table cells lay out in EMU (2196) — scale-free.
3. Minimum widths: outline `max(1, (lineWidth ?? 9525) × scale)` (876, 1144), text outline `max(0.5, w × scale)`
   (1687), decorations `max(1, sizePx / 16)` with `round(by ± …) + 0.5` (1693–1701), table borders
   `w × scale < 1.5 ? 1 : w × scale` (2370).
4. Effect thresholds in px: glow `r > 0.3` (659), soft edge `r > 0.5` (684), bevel `bw > 0.5` *device* px
   (1326, Effect), blur strings `toFixed(2)` of px (701, 724), shadow blurs `blurRad × scale × K` (32–33).
5. Pattern fills: `setTransform([9525 × scale, 0, 0, 9525 × scale, ox, oy])` (2481); the 8×8 tiles are
   scale-free (cache by colours/alphas).
6. Path gradients: the raster grid is `k = 192 / max(w, h)` of the **px** box, `gw = max(2, round(w × k))`
   (2506–2507) → a generated bitmap per scale (cache, M5).
7. Image tiles: natural size × (914400/dpi or 9525) × scale, offsets × scale (526–528, the image natural size
   from the shell, measurement A12).
8. Extrusion depth `E = h × scale` (1232); camera matrices are scale-free.
9. Device measurements: unit pieces (A5, two-phase) and the run fit (painter).
10. Warp raster resolution and strip counts (text-warp.ts, Effect), layer sizes = canvas size (painter).
11. Metafile/PICT layers are rasterized at the picture's display size at 96 dpi (raster.ts:16), not at the
    zoom — scale-free, cached per picture.

### S8. Codec, painter and PDF without touching Write

- **Schema** `core/schema/slides.uos`: `SlidesList`, `SlotFrame`, the resource deltas (symbols, faces as
  `(family, bold, italic)`, images by `MediaId` + natural size, generated bitmaps by bytes handle, gradients,
  patterns), generated into `uo-render/src/codec/slides_generated.rs` and `uo-wasm/js/gen/slides.ts`. Same
  wire rules as §3.1 (f64 bits, varints, UTF-16 strings, bounded lists, `SCHEMA_HASH` check).
- **TS painter**: a new interpreter `uo-wasm/js/paint-slides.ts` over decoded items plus the moved shell code
  (effects, `reveal.ts`, `transition.ts`, `text-warp.ts`, layer pool, `guarded`'s save-depth repair applied
  per shape range) — Write's `paint.ts`/`picture.ts` untouched.
- **PDF (R-S5, M4 scope P1)**: uo-pdf interprets the same list with a `PdfProfile` for Slides (plan §4.4):
  state/transforms (`q`/`Q`/`cm`, registers from its CTM stack), paths, fills (solid; linear/radial as shadings;
  conic and patterns per measurement), images, clips, `TextRun` as glyphs, composite `multiply`/`screen`/
  `darken`/`lighten` as blend modes; canvas shadows, `Filter`, `destination-*` layers and every `Effect` are
  **listed missing** until measured against PowerPoint PDF.
- **Editor overlay** (selection outlines, handles): a separate small `SlidesList` per selection change, built
  from the same shape paths (`uo-dml-render` exposes `shape_outline(shape, view)`); hit testing and link/find
  boxes travel in the producer's app section, as Write's (§3.4).

### S9. Advance-table input format

Specified with the protocol in `uo-dml-render.md` "Measurement protocol": per `(family, bold, italic)` and
code point the **raw** width at 1000 px (kerning off; the 1/8-pt rounding is per use in Rust), whole-string
kerning pairs `(w0, w1)` at 1000 px, whole-string widths at a given size/kerning/letter spacing (non-
`PER_CHAR` text, WordArt layout, unit pieces, PICT), full `TextMetrics` (metafile text, charts; warp for the
PDF wave), font-availability probes, image natural sizes, and the capability bits. The table is an input of
the emit (two-phase), never a cache of results.

### S10. Sub-frontiers of the implementation wave

| # | Content | Size | Model |
|---|---|---|---|
| R-S1 | `uo_render::slides` types, `slides.uos`, generator additions (none expected beyond new message types) | ≈ 600 new | Sonnet |
| R-S2 | encoder/decoder, round trip (Rust → Rust, Rust → TS → identical bytes), fuzz `slides_decode`, `slides_roundtrip` | ≈ 800 new | Sonnet |
| R-S3 | TS interpreter `paint-slides.ts` + the moved shell code (≈ 1,900 raw TS lines, mostly moved verbatim) | TS ≈ 1,000 new + moved | Sonnet |
| R-S4 | recording context and canonical op stream (plan §4.3, shared with the capture agent) | new | Sonnet |
| R-S5 | uo-pdf interpretation, `PdfProfile` | ≈ 800 | Sonnet, after B4 |

M5 for the painter: layer pool, derived images, pattern objects, decoded lists (each capped, shell tests);
the Rust side keeps no cache in uo-render (`clear_caches` stays a no-op).

## Slides extensions — R-S1/R-S2 implementation

Sonnet, 2026-11-17, one session, inside the design above (S1–S10). Built: the Slides list types, its two schemas,
the generated Rust/TypeScript codecs, the resources/encoder/decoder/validator wrapper, tests, fuzz targets. Not touched: Write's
schema, bytes and generated files (`render.uos`, `write.uos`, their generated Rust and TypeScript are byte-identical to
HEAD: `git status` lists none of them), `uo-dml-render`, `uo-pptx`, the TS interpreter (R-S3), the recording context (R-S4), PDF (R-S5).

### What was built

| File | Content |
|---|---|
| `core/schema/slides.uos` (magic `UORS`, hash `5115aabc928a2e71`) | `SlidesList` and every type under it: 4 persistent tables (`faces`, `media`, `derived`, `bitmaps`; the symbol table is implicit), `SItem` (48 variants: the S2 sketch, one canvas call or state assignment each), `PathOp`, `Paint`, `ImageSrc`, `TextRun`, `Effect` (`Bevel`, `Warp`, `MetafileRop`), `SlotDef`, with the arenas `paths`, `stops`, `dashes`, `texts`, `gradients`, `patterns`, `runs`, `effects`, `slots` |
| `core/schema/slots.uos` (magic `UORF`, hash `c2afa6fecfbddca4`) | `SlotFrame`: per-frame `SlotValue`s (`Static`, `Hidden`, `State`, `Shape`), `PartState` (the fields `applyPart` reads), `AnimFilter`, `Replacement` (the re-emit rule: a complete `SlidesList` message as bytes) |
| `core/schema/manifest.txt` | two lines; generated `uo-render/src/codec/{slides,slots}_generated{,_support}.rs`, `uo-wasm/js/gen/{slides,slots}{,.support}.ts` |
| `uo-render/src/slides.rs` | `uo_render::slides`: re-exports of the generated types, `SlidesResources` (interning, de-duplication, id checks, bitmap bytes by handle), `SlidesEncoder` (generation + delta mark), `SlidesDecoder` (tables, limits, rollback), `validate`, `check_frame`, `SlidesError`, constants (`SHADOW_OFFSET_K`, `CTM_REGISTERS` = 4, `MAX_LAYERS` = 16) |
| `uo-render/src/codec/mod.rs`, `lib.rs` | `pub mod slides_generated`, `slots_generated` (+ cfg(test) support), `pub mod slides` |
| `core/tools/uo-schema` | one generator fix: the TypeScript import of `tagIndex` only when the schema has a unit-only enum (`slots.uos` has none; `noUnusedLocals` failed). No other generator change: the grammar covered everything |
| `tools/uo-schema/tests/{slides,slots}_codec`, `codec_real.rs` | the full R-P2 property suite (Rust round trip, TypeScript decode + re-encode identical bytes, every prefix, mutations) over both schemas |
| `uo-render/tests/s_slides.rs`, `s_nopanic.rs`, `s_memory.rs`, `s_support/` | semantic tests, hostile bytes, M1–M3; `s_support` has the sample slide and the hostile-list generator |
| `core/fuzz` (`src/slides.rs`, `src/slides_wire.rs`, `fuzz_targets/slides_{decode,roundtrip}.rs`, `Cargo.toml`, `lib.rs`, `bin/seed.rs`; `codec.rs` exports its `target!` macro) | the two targets, seeds in `seed` |

### Decisions (the design was ambiguous or silent; each recorded)

1. **The generated wire types ARE the types.** The S2 sketch had hand-written `SlidesList`/`SItem`; but a Slides item is the canvas call itself, there is
   no side-column layout to protect (Write converts because of its 40-byte items and glyph columns), so a hand-written copy would only be a conversion
   layer with its own bugs. `uo_render::slides` re-exports the generated types and adds the stateful parts. Consequence: ids are plain `u32` (symbol, face,
   media, derived, bitmap) and the structs are `Clone`, not `Copy` (the generator derives `Clone` only; adding `Copy` would change Write's generated files,
   so it was not done: producers write `.clone()` for a `Span` they reuse).
2. **Big payloads live in arenas, not in items.** The generator never boxes (recursion only through `list<>`), so `SItem`'s size is its largest variant.
   `Text(u32)` and `Effect(u32)` index `runs` and `effects`; `SItem` is **88 B** (`DrawImage`), `TextRun` 224 B stays out of it. `SlotBegin(u32)` indexes `slots` (the S2 `SlotRef` became the slot position).
3. **`texts` is UTF-16** (`utf16`, spans count code units), not a `String` with byte spans: the TS strings can hold lone surrogates and the wire already carries UTF-16.
4. **Gradients, patterns, stops, runs, effects are per list; faces, media, derived images, bitmaps are tables** (per-generation deltas). Everything scale-dependent is in the list, so a resize re-sends only the list.
   `Derived` is a table of `(media, ImageFx)`, so the S5 `ImageFx` effect is not an `Effect` variant: `ImageSrc::Derived(id)` carries it and the shell caches by the id.
5. **`Filter(Option<Sym>)` is two variants** (`FilterSet(sym)`, `FilterNone`): an enum payload cannot be optional. `LayerThen::Composite` has no soft-reveal span: the reveals come from the enclosing shape slot's value at paint time (they are animation state, not list data).
6. **A separate schema for the frame** (`slots.uos`, magic `UORF`), as the design said: magic is per schema. It carries the implicit symbol table (filter names) and no resources; the re-emitted sub-lists are complete `UORS` messages as `bytes`.
   `AnimFilter.out` replaces the transition string (the painter only tests `=== 'out'`).
7. **Gradient geometry is six generic numbers** (`g0..g5`), the kind is the `Paint` variant (`Linear`/`Radial`/`Conic`); `Effect` payloads (`Bevel`, `Warp` with `WarpPaint`/`WarpTile`, `MetafileRop`) are a best reading of S5 and the TS signatures
   (`applyBevel`, `paintWarp`, `WarpLine`); **R-S3 will refine them** (a schema change before the first release only changes the hash, no compatibility is owed).
8. **`validate` is part of both ends** (encode refuses; decode validates after the codec): arena spans (checked in `u64`), arena indices, register < 4, layer id < 16, `fallback_end` in `(at, len]`, and, on the encode side, every table id (the decoder's `ref_in` already checks those on decode).
   It checks neither save/restore balance nor layer pairing (the painter repairs per shape range: S8) nor any number.
9. **Decode failure restores the tables** even when the codec committed (the list was invalid): the decoder takes the old tables aside on a generation change and puts them back.
10. The `generated` support (`random_*`, `dump_*`) generates random lists with indices bounded by the *tables* only, not by the arenas, so they exercise the codec, not `validate`; `s_support::hostile_list` is the arena-aware generator.

### Evidence (platform win32, Rust release unless noted)

- **Write unchanged**: `render.uos`, `write.uos`, `codec/generated*.rs`, `uo-wasm/js/gen/{render,write}*.ts` have no diff (the generator's `check`: 28 generated files up to date). `uo-render` suite (the 41 existing tests across `e_*`, `g_*`, `types` and the lib) passes; `uo-schema` suite (codec_fixture 23, codec_real now 20 with the 10 new, codec_session 60, generator 12, bind 6, memory 4, ts_runtime 1, lib 1) passes; test `write_schema_identity_is_unchanged_and_slides_has_its_own` pins `UORP`/`0836c72ae39d3a59` and that each decoder refuses the other's bytes.
- **R-P2 on the Slides schemas** (`codec_real`): Rust round trip, 400 random streams each: list 3,047,082 B, frame 368,547 B; Rust encode → **TypeScript decode → TypeScript encode** identical bytes, 360 messages each: list 851,945 B, frame 105,785 B (Node 24); mutations: list 34,234 refused / 61,766 canonical, frame 19,152 / 27,064; every prefix refused. Generated TypeScript passes the CI `tsc 5.9.3` flags (`--strict --noUncheckedIndexedAccess --exactOptionalPropertyTypes --erasableSyntaxOnly …`) over `gen/*.ts`.
- **`s_slides.rs`** (9): the sample slide round trips with exact bytes, delta messages carry only new table entries (the second message of the same list is at least 150 B smaller, asserted), a new generation resends everything, 25 mutations each make the encoder refuse with `out` and mark untouched, the decoder rolls the tables back on a codec error and on a validation error, every prefix of the sample is refused and 4,164 single-byte flips give 524 refused / 3,640 canonical, 3,000 hostile lists (NaN payloads, infinities, wild indices): 379 encoded and decoded back to identical bytes, 2,621 refused by `validate`, resources checks, frame round trips.
- **Size of the sample list** (a 1280 × 720 slide: background, gradient card with the off-screen shadow idiom and outline, pattern fill, two pictures (one derived), a layer, an animated two-run paragraph, a warp effect; **48 items**): **1,388 B** on the wire, of which 322 B are the header and the tables (faces, media, derived, bitmap pointer, 13 symbols) and 1,066 B the list = **22.2 B per item** (the arenas and the runs included). In memory `size_of::<SItem>()` = 88, `SlidesList` = 280 (+ heap). A `SlotFrame` with one animated part and a re-emitted 1,388 B list: 1,524 B (the frame itself is the 136 B left over).
- **No panic** (`s_nopanic.rs`, 4 tests, 0.02 s): 20,000 random inputs raw and behind a valid header, 8,000 mutated valid messages (1,104 decode, 6,896 refused; byte set, bit flip, huge count, truncation, duplicated block), a lying 2^24 count allocates under 8 KB, 20,000 frame inputs; every call under 1 s, every error leaves the tables empty (`UO_SLIDES_NOPANIC_ITERS` for longer).
- **Memory** (`s_memory.rs`): **M1** encode + decode and every error path (the encoder's refusal, every 11th prefix, every 13th flipped byte, a table-gap rejection) return the live bytes exactly; resources with a generated bitmap drop clean. **M2** soak of 2,000 encode + decode iterations, flat, tolerance 0. **M3** the sample: 1,388 B message, **12,056 B allocated** in encode + decode together, peak 9,112 B above start (the output doubles from empty: producers should reserve the previous message's size). M4/M5: no handles, no caches (the bitmap bytes belong to `SlidesResources`).
- **Fuzz** (`slides_decode`, `slides_roundtrip`, the stable driver, 45 s each, 49 + 31 seeds): 39,323,956 and 641,312 executions (874 k/s and 14.3 k/s), no crash, slowest call 2.0 ms and 6.0 ms. `slides_decode` also runs the validating `SlidesDecoder` and asserts accepted ⇒ `validate` passes ⇒ re-encodes to the input; `slides_roundtrip` selector 2 builds random VALID lists through `SlidesResources`/`SlidesEncoder`/`SlidesDecoder`. libFuzzer itself runs in CI (the workflow's fuzz job lists targets from `TARGETS`; not edited here).
- **Lints**: `cargo clippy -p uo-render --all-targets -- -D warnings` native, `--lib --target wasm32-unknown-unknown` clean; clippy `--all-targets --all-features` native and wasm32 (lib) clean on every dependent (`uo-docx-layout`, `uo-pdf`, `uo-wasm`, `uo-ffi` native) and `uo-schema`; `cargo fmt` clean on the changed files (the pre-existing unformatted `uo-render/tests/types.rs` was left alone); `grep-bans.sh` clean. `check-changed.mjs` was not used: the other agent's uncommitted `Cargo.lock` change selects the whole workspace; the dependents above were checked by hand (the list: `uo-render uo-docx-layout uo-ffi uo-pdf uo-wasm`).

### What R-S3 and R-S4 need

- **R-S3** (`paint-slides.ts`): decode with the generated `decodeSlidesList(buf, tables)` (the `Tables` object holds `sym` as resolved strings, `faces`, `media`, `derived`, `bitmaps` as arrays); item objects are `{ op: "fillRect", x, y, w, h }`-style (payload structs flattened, scalar payloads as `{ op, v }`); `Paint`/`ImageSrc`/`Effect`/`LayerThen` tag `kind`. Interpret `items` in order; `LayerBegin.fallbackEnd` is the item index to jump to when no layer is available; `SlotBegin(i)` reads the frame value at `slots[i]`; registers 0–3 hold captured matrices; `CtmOperand` = `add + ctm[axis] * k`. The bitmap bytes come from the wasm side by `Bitmap.data` handle (`SlidesResources::bitmap_bytes`; the wasm export is R-S3/wasm-view work, not built here). Lists are validated on the Rust side; the interpreter still bounds-checks indices it reads from a TS-built list. Refine `Effect` payloads against the TS code as it moves.
- **R-S4** (recording context): the canonical op stream is one record per `SItem` plus the context calls the painter issues for `Text` (fit, decorations), `Effect` and layers, so the recorder runs behind the same interpreter; `SItem` names map 1:1 to canvas calls, which keeps the stream comparable with the TS capture. The sample list in `s_support::sample_list` is a fixture to start from.
- Producers (`uo-dml-render`, `uo-pptx`) build `SlidesList` directly with `SlidesResources` ids, call `SlidesEncoder::encode` (it validates), and use one `SlidesEncoder` per (view, generation).

### Process metrics

About 90 tool calls (reading the record, the schema grammar and the generator, then write–check cycles). Rework: 5 small (a shell heredoc broken by an apostrophe → Write tool; `Span` is not `Copy`; a banned `core::f64::consts` use and then clippy's `approx_constant` on the literal replacing it; `tagIndex` unused import found only by the CI tsc flags; clippy `type_complexity` in a test). Wall times on this machine (4 jobs, shared with two other agents): schema generation 2.5 s, `uo-render` clippy native 3 s cold-incremental, full `uo-render` test suite (release) 39 s, `uo-schema` suite 47 s, fuzz crate release build 66 s, fuzz runs 45 s per target; the whole session about an hour of wall time. Parity rounds with a verifier: none (new code, no TS counterpart yet).

## Slides extensions — R-S3/R-S4 implementation

Sonnet, 2026-11-17, one session, TypeScript/JavaScript only (no Rust, schema or generator change; `uo-pptx/src` and
`uo-dml-render` untouched). Built: the TS interpreter of the Slides list with the shell code moved from the frozen TS
(`ts-ref-092` `b92b60fa`, `slides/packages/slides-core/src`), the recording context and canonical op stream, the draw-op
captures of the frozen TS, a self-test, and the Rust-list-vs-TS comparison script (waits for the B4 producer).

### Files

| File | Content |
|---|---|
| `uo-wasm/js/paint-slides.ts` | `SlidesPainter` (`new SlidesPainter(env).paint(ctx, list, tables, frame?)`): the interpreter of all 48 `SItem`s, the layer pool, the guarded ranges, slot application (`applyPart`), text (`drawText`), paints/patterns/images, the three `Effect`s; caches (derived images ≤ 256, bitmap canvases ≤ 128, `clear()`) |
| `uo-wasm/js/slides-effects.ts` | MOVED: `fxImage`, `applyBevel` + `edt1d` + sRGB tables, `xorRect`, `imgW/imgH/scratchCanvas` |
| `uo-wasm/js/slides-warp.ts` | MOVED: `text-warp.ts` (594 lines) |
| `uo-wasm/js/slides-reveal.ts` | MOVED: `reveal.ts` (`applyReveal`, `applySoftReveal`, `softRevealFilter`, `WIPE_SOFT`) |
| `uo-pptx/js/lib/record-ctx.mjs` | the recording `CanvasRenderingContext2D` (+ `RecDOMMatrix`, recording `OffscreenCanvas`, `stubMeasure`) |
| `uo-pptx/js/lib/opstream.mjs` | forms (`full`, `draw`, `loose`), `compare`, `digest`, `parseOps` |
| `uo-pptx/js/lib/ts-paint.mjs` | the frozen `paintSlideTo` / `paintShapesTo` over the recorder (a FRESH `render.js` instance per run: its pattern and path-gradient canvases are process-wide caches, so a shared instance would make a stream depend on the slides painted before it) |
| `uo-pptx/js/lib/rust-paint.mjs` | `paint-slides.ts` over the recorder |
| `uo-pptx/js/lib/views.mjs` | the two views and the capability sets |
| `uo-pptx/js/capture-drawops.mjs` | the capture CLI (see below) |
| `uo-pptx/js/compare-drawops.mjs` | Rust-list-vs-TS comparison (the `.uolf` input format is in its header) |
| `uo-pptx/js/selftest-drawops.mjs` | 12 tests: `node --test core/crates/uo-pptx/js/selftest-drawops.mjs` (0.3 s) |
| `uo-pptx/tests/captures/drawops-{samples,b1a}.{capture,ops.gz,measures.tsv.gz}` | the captures |

Type check: the new TS passes the CI `tsc 5.9.3` line (`--strict --noUncheckedIndexedAccess --noUnusedLocals --noUnusedParameters --noImplicitReturns
--exactOptionalPropertyTypes --erasableSyntaxOnly --verbatimModuleSyntax`) together with `gen/{slides,slots}.ts` and the codec runtime. **The workflow's tsc
line (`.github/workflows/rust-core.yml`, `lint` job) does not list the new files yet** (the file has another agent's uncommitted edit): add
`crates/uo-wasm/js/paint-slides.ts crates/uo-wasm/js/slides-effects.ts crates/uo-wasm/js/slides-warp.ts crates/uo-wasm/js/slides-reveal.ts`
(`gen/*.ts` is already globbed), and `node --test core/crates/uo-pptx/js/selftest-drawops.mjs` as a step.

### The op stream (record-ctx.mjs)

One text line per call or property set, LF-terminated, `@cvK ` prefix for a line issued on the K-th offscreen canvas (layers, scratch canvases; K counts
creation order in the run). Numbers are JS's shortest round trip, `-0` prints as `0`; strings are JSON-quoted.

- `set <name> <value>` (every property: `fillStyle`, `font`, `letterSpacing`, ...; a gradient/pattern value is `@gN`/`@pN`); `save`, `restore`; `ctm a b c d e f`
  restated after every transform call and after a `restore`; `setLineDash`;
- path geometry in **device space** (the transform at the call applied): `moveTo X Y`, `lineTo`, `bezierCurveTo`, `quadraticCurveTo`, `rect` (4 corners),
  `arc`/`ellipse` as centre + the device image of the two axis vectors + angles + ccw (exact for any affine transform), `arcTo` with the device points,
  `r` and the linear part; `beginPath`, `closePath`;
- draw calls with the **resolved state** appended after ` | `: `fill RULE | fs=<paint> alpha= comp= [filter=] [shadow=color,blur,ox,oy] [clip=N]`; `stroke | ss= lw= cap=
  join= miter= [dash=a,b@offset]` (width and dash scaled by the transform); `fillRect`/`strokeRect`/`clearRect` (4 corners); `fillText "t" X Y | m=a,b,c,d font= align=
  base= ls= kern= fs= ...`; `strokeText`; `drawImage SRC [s=x,y,w,h] P0..P3 | smooth= ...`; `clip RULE`; paints: a colour string raw (never parsed), `lin(x0,y0,x1,y1|off:color;..)`,
  `rad(...)`, `conic(angle,cx,cy|...)` in device space, `pat(SRC,repeat,a,b,c,d,e,f)` with the device matrix (CTM x pattern transform); image sources `img(<sha1/12 of
  the bytes>)` or `canvas(WxH:<recipe hash>)` (the running hash of the lines recorded on that canvas, including `putImageData` data hashes);
- definitions `gradient @gN kind args`, `stop @gN off "color"`, `pattern @pN SRC repeat`, `patternTransform`; reads `getImageData x y w h` (returns zeros),
  `putImageData x y w h <sha>`; `measure "font" "text"` (the width is NOT in the stream).

Forms (`opstream.mjs`): `full` = everything; **`draw`** (the comparison form, what the captures store) drops `set`, `setLineDash`, `gradient`, `stop`, `pattern`,
`patternTransform`, `measure` (the draw lines already carry the resolved state; this removes 69 % of the lines of a full stream, mostly the layout's measureText probing); `loose` also
drops `save`/`restore`/`ctm` (two painters that arrange the transform stack differently still compare equal). `compare(a, b, { which, eps })` is exact by default,
`eps` compares numeric tokens relatively.

Capability set pinned (`DEFAULT_CAPS`; `--caps nolayers,nopattern,nospacing,nokerning` turn one off): `OffscreenCanvas` exists (layers and scratch canvases are real recording
canvases), `DOMMatrix` + `CanvasPattern.setTransform`, `letterSpacing`, `fontKerning`. The recorder mimics the canvas where the painters depend on it: `arc`/`ellipse`/`arcTo` with a
negative radius throw (the TS `guarded` relies on it), non-finite transform arguments are ignored, `globalAlpha` outside 0..1 is ignored, `addColorStop` outside 0..1 throws.
`getImageData` returns zeros: bevel, recolour and PATINVERT record their reads and writes but the pixel arithmetic is not exercised by the streams (it is pixel code, gated by
Chromium pixels, plan 4.3 second gate).

### Moved code (copy, adapt only inputs) — every adaptation

- `slides-reveal.ts` = `reveal.ts` **verbatim** (no imports, no change).
- `slides-warp.ts` = `text-warp.ts`: (1) `import { patternRows } from './patterns.js'` became the injected `PatternRowsOf` parameter of `rasterLine` (new 4th parameter) and
  `paintWarpedText` (new last parameter). Nothing else.
- `slides-effects.ts`: `imgW`/`imgH` (render.ts 406-420) verbatim, exported; `scratchCanvas` (440-446) verbatim, exported; `fxImage` (448-501): the canvas factory is a parameter
  (`scratch`), the `ImageFx` is the TS model shape (`toTsFx` converts the wire `ImageFx`), cache unchanged (module WeakMap by image + key); `applyBevel` (1295-1382) with `edt1d`:
  `sh.bevel` -> the effect payload (`w`, `h`, `prst`, `rig`, `dir`, `mat`), `x y w h` -> `e.bx`, `scale` -> `e.scale`, `shapePath(m2, geom ...)` ->
  `beginPath` + the replay of the silhouette path ops (`replayPath`) + `fill(e.rule)` (the TS `rule ? fill(rule) : fill()`), the light tables (`lightTable`, `lightAt`, `lightFace`,
  `bevelPhi`) injected as `LightTables`; `xorRect` (metafile.ts 311-330): the brush colour is an `[r, g, b]` triple (the operand bitmap's first pixel) instead of `#RRGGBB`.
- `paint-slides.ts` copies `acquireLayer`/`discardLayer`/`compositeLayer` (render.ts 154-200; the canvas factory is the env's, default = the TS's `OffscreenCanvas` else `<canvas>`),
  `applyPart` (116-140; wire `PartState`, the filter's `out` flag for the `transition === 'out'` test), `drawText` (1638-1707; the producer-side values arrive in `TextRun`), and the paintWarp
  tail (1931-1956: layer, tiled picture `source-in`, shadow composite) and `patBlt` (metafile.ts 1372-1385).

Not moved, with the reason: `pptMeasure`/`advCache` and `layoutTextBody` (producer side: the layout is measured from advance tables, S9); `transition.ts` `paintTransition` (between slides, driven by the
`SlideShow` frame, not a list item: a later wave); the metafile/PICT/TIFF rasterisers (producer side: their result is a `Bitmap`); **the pattern and path-gradient bitmap generators
(`presetPattern`, `pathGradient`, `gradMode`)**: the design makes them producer output (`bitmaps` table, sized by scale), so the interpreter only turns the RGBA bytes into a canvas
(`env.bitmap(i)`, cached, `createImageData` + `putImageData`) and `createPattern`s it — the brief listed them as shell code, the schema (S2: `PatternSrc::Bitmap`) decides otherwise.

### Interpretation rules the schema left open (each is a B4 contract)

1. **Slots**: `SlotBegin` = `save()` + evaluation, `SlotEnd` = the matching `restore()`; the producer emits no `Save`/`Restore` of its own around a slotted part. Value `Hidden`, `Shape.op <= 0` or
   `applyPart` false: restore and skip to after the matching `SlotEnd` (the TS skips without a save/restore pair; the stream has one extra pair). Shape value: `op < 1` multiplies the alpha, then `applyPart(all)`
   with the soft-reveal collector. `Static`/no frame: paint as the template. Re-emit replacements (`SlotFrame.replacements`) are not applied by the painter (the caller swaps the list).
2. **`LayerBegin.fallbackEnd` = the end of the else block that follows the matching `LayerEnd`.** With a layer the painter runs `[LayerBegin+1, LayerEnd]` and `LayerEnd` continues at `fallbackEnd` (the else block is skipped);
   with no layer the painter continues just after the matching `LayerEnd` (the first item with the same layer id) and runs the else block up to `fallbackEnd`. An empty else block has `fallbackEnd = LayerEnd + 1`. The
   `LayerBegin` right after the `SlotBegin` of a *shape* slot is the **soft-reveal layer**: taken only when the slot value has a pending soft reveal; otherwise (or without a layer) the else block runs, and with soft reveals
   pending but no layer the painter applies `globalAlpha *= applyReveal(...)` for each, like the TS fallback. `LayerEnd(Composite)` = `compositeLayer(previous target, layer, soft of the enclosing shape slot, slot box)`;
   `Keep` leaves the layer for `DrawLayer` (`target.drawImage(layer.canvas, 0, 0)` under whatever state the items set) and `ReleaseLayer`.
3. **Guarded ranges** (the TS `guarded`, per shape): a `save`/`SlotBegin` at relative depth 1 (a shape under the slide clip, or under `paintShapesTo`'s save) opens a range; a throw inside restores the pending saves of the main
   context, discards the layers acquired in it, and resumes after the range's closing `restore`/`SlotEnd`. A throw at depth <= 1 repairs the stack and propagates. Indices out of range (arenas, tables, registers) throw `RangeError`
   (hostile lists are contained the same way).
4. `MediaFrame`: the host's frame, alpha >= 1 drawn opaque (over the cover already painted: same pixels as the TS, which skips the poster; one extra `drawImage` in the stream), 0 < alpha < 1 under `save`/`globalAlpha *=`/`restore`.
   A picture the host cannot give (still decoding, missing) skips its `drawImage`, and a pattern fill from it skips the `fillStyle` assignment.
5. `TransformUnshape(reg)` applies `ctx.transform(inverse(current) x reg)` (the TS `ctx.transform(ov.unshape...)`; same CTM as `setTransform(reg)`); `ShadowOffset` = `add + ctm[axis] * k` evaluated on the current target.

### Schema gaps found (for the orchestrator / B4; no schema change was made)

- `BevelEffect` has no `dir` (the TS `lightAt(tab, b.dir, ...)`): the interpreter reads an optional `dir` from the payload (`BevelEffect & { dir?: string }`); add `dir: sym?` before B4 emits bevels.
- `MetafileRopEffect.operand` is read as the brush: first RGBA pixel; `rop >>> 16 & 0xff` 0x5A = exact XOR (or `difference` when the transform/clip forbid it), 0x00/0xFF/0x55/0xF0 as the TS `patBlt`.
  The clip state `xorRect` needs is tracked by the painter from `Clip` items per save level.
- `WarpPaint`/`WarpTile` carry less than `letterFill`/`imageTile` read: no `scaled`, `fillTo`, stop alphas for path gradients (`kind: path` falls back to the TS's circular radial gradient, no raster), no image tile
  parameters (`sx sy tx ty algn flip`, dpi). Tiled letter fills and the tiled warp picture need `env.tilePattern` (the charts engine's `tilePattern`) and are skipped without it; stretched picture fills work.
- Host hooks the interpreter needs and `uo-wasm` does not export yet: `light` (uo-drawingml `light3d`) and `patternRows` (the tests import the TS tables of `shared/drawingml`), `tilePattern`.

### Evidence (win32, Node v24.15.0, V8 13.6, `tsc 5.9.3`)

- Self-test (`selftest-drawops.mjs`, 12/12): the recorder run twice over the frozen TS (a sample deck) and over a hand list gives identical streams; every item family under the generated codec (encode -> decode): transforms,
  registers and the CTM operand (`shadowOffsetX = 3 + 2 * 20000`), gradients/conic/patterns with the device pattern matrix, layers (with, without — `nolayers` caps — soft-reveal rule, keep/draw/release), slots (hidden, wipe at q = 0.5
  clips the lower half, shape opacity, the soft wipe takes the layer and skips the else block), text (the fit `d / n` from the stub's `measureText`, spacing, shadow, outline, decorations, alpha, restores), images (derived through the moved
  `fxImage`, bitmap, media frame, a missing picture skipped), effects (bevel mask built on a scratch canvas and written back, XOR rectangle, WordArt `textArchUp` with a layer and shadow), guards (a negative-radius arc contained in its shape,
  a hostile index repairs the saves), caches capped (bitmaps = 128 after 158 distinct) and the layer pool reused (60 paints, 1 layer canvas). **TS vs interpreter**: the frozen `paintSlideTo` on a small slide (background,
  filled and stroked rectangle, underlined text) and the interpreter over the list the producer will emit give equal `draw` and `loose` streams (eps 1e-9: the producer-side `laidW` is re-derived from the text).
- Captures (`capture-drawops.mjs`, run twice: byte-identical `ops.gz`, `measures.tsv.gz` and `capture` apart from `@platform`):

  | Capture | Decks | Slide views | Draw-form lines (full) | Files |
  |---|---|---|---|---|
  | `drawops-samples` (53 tracked samples, views a and b, plus `paintShapesTo` digests per slide) | 53 | 1,202 | 112,074 (365,482) | `ops.gz` 189,220 B, `measures.tsv.gz` 51,919 B, `capture` 52,673 B |
  | `drawops-b1a` (first 100 generated decks, view a) | 100 | 246 | 592,851 (1,340,804) | `ops.gz` 5,981,704 B, `measures.tsv.gz` 250,522 B, `capture` 18,026 B |

  Total committed 6.5 MB (the 100-deck cut keeps the CI subset under 10 MB; 150 decks was 10.0 MB for `ops.gz` alone; all 200 decks, both views: 27.5 MB, regenerate with `--max`). 0 TS errors in the 1,448 recorded views;
  the 54 `@cv` lines of the samples are the layer/scratch canvases of pattern fills and effects. Views: `a` = 1280 px wide at the origin, `b` = 640 px wide at (17.5, 9.25) (a non-zero origin and another scale).
- **measureText log** (`*.measures.tsv.gz`: `"font"<TAB>"text"<TAB>width<TAB>count`, sorted, summed over the capture): samples 5,296 distinct (font, string) pairs over 158 CSS font strings (4,747 multi-character strings: the layout's
  whole-string and kerning probes); b1a 29,403. The widths are the **stub's** (`stubMeasure`: a deterministic em table, 6 % wider bold, a few kerning pairs unless `fontKerning` is `none`), so the files give WHAT the TS asks the
  canvas for, never real advances; the advance tables of B3 still need a real canvas (Chromium) or `uo-fonts`.

### What B4 needs

1. Emit one `SlidesList` stream per deck (generation per deck, view per (slide, view a/b): `ox, oy, scale` from `lib/views.mjs`), in the **`.uolf`** frame file `compare-drawops.mjs` documents (list message + the bitmaps added by each message,
   RGBA in table order) and run `node core/crates/uo-pptx/js/compare-drawops.mjs --ts core/crates/uo-pptx/tests/captures/drawops-samples.ops.gz --rust <dir>`; the b1a capture needs `--ts ...drawops-b1a.ops.gz`.
   The TS stream starts at the first call of `paintSlideTo` (the slide background) and has no wrapper; the list must reproduce the TS's own `save`/`restore` pairs (or compare with `--form loose`).
2. The `draw` stream is exact by default: producers must use the TS arithmetic for every precomputed value (`Math.fround` is not involved; `eps` is only for diagnosing). The pictures are looked up in the package by `Media.part`,
   so `ImageRef.part` and the table entry must agree.
3. The TS records `paintShapesTo` digests per slide (`s<N>.shapes` in the capture) for Sheet drawings; its stream is not stored (the same code path with the slide clip and background removed).
4. Resolve the schema gaps above (bevel `dir`; letter fill and warp tile parameters) before the producer emits those effects; decide rule 2 (`fallbackEnd`) and rule 1 (slot save) as written, or change the interpreter, not both ends.

### Process metrics

About 85 tool calls (reading the design, `render.ts` — 1,300 of its 2,662 lines — `reveal.ts`, `text-warp.ts` and the capture harness; then write–check cycles). Rework: 6 small (a Bash heredoc halved `\n` in a patch script, so the fix went through the
Edit tool; the capture's measure-key parsing; a stale `fallbackEnd` in the slot test; unused-parameter errors found by the CI tsc flags; the draw form kept `setLineDash` until the first capture was regenerated; the compare script first imported the
capture CLI, which runs it). Wall times: captures 13.0 s (samples, 1,202 views plus 1,202 `paintShapesTo` runs; the TS paint is a few ms per slide) and 11.0 s (b1a, 100 decks), self-test 0.3 s, `tsc` over the new files about 4 s. Parity rounds with a
verifier: none (the frozen TS is the oracle; the first producer comparison is B4).
