# uo-dml-render — the shared DrawingML shape renderer (Slides + Sheet drawings), plan §4.5

Record of the rust-port workflow (hybrid mode). Status: architecture pass done 2026-11-17 (Opus); **B1a ported 2026-11-17**
(Sonnet: shape model, reader, theme, cascade, table styles, symbol fonts); **B3 ported 2026-11-17** (Opus: text layout and
the measurement protocol, `text`; parity 100 % over 12,396 layout calls, see "B3 — text layout"); B4–B5 follow. Companion records: `uo-pptx.md` (the presentation,
decisions T1/T2/P1/C0/W0, traps, memory) and `uo-render.md` "Slides extensions (plan §4.2) — design" (the
display-list items this crate emits).

Behaviour source: the frozen TS at `D:\Projects\OfficeNext\.claude\worktrees\ts-ref-092` (`b92b60fa`, 0.9.2).
Line counts are raw; line numbers are of `slides/packages/slides-core/src` unless a path is given.

## Why a crate (reuse rule)

`sheet/packages/sheet/src/drawings.ts` (326) is explicitly "not a second DrawingML engine": it calls the
Slides reader and painter (`drawings.ts:14-19`). Under `SKILL.md` "Reuse" these go to a crate used by both
apps (`SKILL.md` crate rule 1). `uo-drawingml` already holds the format-neutral DrawingML pieces (presets,
guides, colour modifiers in RGB linear, raster masks, light/bevel, `odraw`); this crate is the next layer: a
**shape model** read from `p:`/`xdr:` trees, its **text**, and its **emission** as a display list.

## Exact boundary

### What Sheet calls today (drawings.ts) and the Rust replacement

| `drawings.ts` call | Purpose | `uo_dml_render` API |
|---|---|---|
| `readTheme(dml, themePart)` (`drawingTheme`, 107) | DrawingML theme of the workbook; Sheet then fills missing slots from its own theme and the default clrMap | `read_theme(src, part) -> Theme`; the Sheet-specific completion stays in Sheet (it is Sheet's rule) |
| `readDrawingElements(dml, part, [el], theme)` (`shapesAt`, 225) | shapes of one anchored object, read in the space of the anchor | `read_drawing_elements(src, owner, &[ElementRef], &Theme, ShapePrefix::Xdr, Option<FrameOverride>) -> Vec<ShapeList>` |
| `placeElement(el, T)` (72) | rewrites the object's `a:xfrm` (and group `chOff/chExt`) as synthetic XML elements with `String(Math.round(x))` | `FrameOverride { x, y, w, h }` applied by the reader at the xfrm read, reproducing the round trip (`Math.round` → `String` → `parseInt`: `x ≥ 1e21` reads back as `1`, `-0` as `0`) |
| `applyShapeTextCascade(sh, EXCEL_TEXT, theme, [])` (`finishShape`) | Excel text defaults (theme minor font, 11 pt), no master | `apply_shape_text_cascade(&mut ShapeArena, shape, &TextStyleContext, &Theme, keys, master_keys)`; Sheet builds its `TextStyleContext` |
| `paintShapesTo(ctx, batch, {slideW:0, slideH:0, images}, k, {ox, oy, fontStack, clipOverflow: true})` | paint in the anchor rect | `emit_shapes(&ShapeList, &EmitParams { scale: k, ox, oy, clip_overflow: true, … }, &MeasureTable) -> Result<Emitted, NeedMeasures>` |
| the `flush()` batching around chart shapes (306–322) | charts inside drawings are painted by Sheet's chart engine with Excel defaults | `emit_shapes` returns `Segments`: runs of shape items and `HostChart { shape, rect }` holes, in z-order |
| `rasterizeSpecialImage` / `specialImageNeedsPrepare` / `prepareSpecialImage` (`warm`, 255) | WMF/EMF/TIFF/PICT the browser cannot decode | `special_image(bytes, display) -> SpecialImage` (`Layer` sub-list for metafiles/PICT, RGBA for TIFF, `NeedsDecode(embedded blobs)` for EMF+ with PNG/JPEG inside) |
| `picDisplaySize` (132) | metafile raster size | stays in Sheet (anchor math), passed as `display` |
| `paintControl` (148) | form controls as Excel draws them | stays in Sheet (Sheet's own items) |

### Owned by uo-dml-render

| TS | Raw lines | Module (`uo_dml_render::…`) | Frontier |
|---|---|---|---|
| `model.ts` shape types (`Shape`, `Xfrm`, `Fill`, `GradientStop`, `ImageRef`, `ImageFx`, `MediaRef`, `Run`, `Paragraph`, `TextBody`, `Table*`, `Theme`) | ≈ 450 of 558 | `model`, `theme` (skeleton done) | — |
| `read.ts` shape half: `readRels` helper, `readShapes`, `readDrawingElements`, `pruneHidden`, `groupFill`, `readGroup`, `readGraphicFrame`, `cellPaintOf`, `olePreviewFromVml`, `vmlColor`, `chartThemeFor`, `chartImagesFor`, `stripUndefShape`, `renamePrefix`, `readDiagramShapes`, `readTable`, `readShape`, `parseCustGeom`, `diagramStyler`, `themeStyleFill`, `parsePattFill`, `parseGradFill`, `parseXfrm`, `readTxBody`, `readParagraph`, `readBulletStyle`, `readRun`, `readPic`, `readBevel`, `readEffects`, `readPicMedia`, `mediaMime`, `readImageFx`, `resolveBlip`, `applyTile`, `mimeFromPart` (732–2320) | ≈ 1,570 | `read` | B1a |
| `theme.ts` (`readTheme`, `applyClrMap`, `clrMapAttrs`, `colorFromDml` — **the HSL colour-modifier profile**: shade/tint in RGB linear, lum/sat/hue in HSL without ceiling, `gamma`/`invGamma` sRGB mode, "untouched keeps the original spelling") | 232 | `theme` | B1a |
| `text-style.ts` | 375 | `text_style` | B1a |
| `table-style.ts`, `table-styles-builtin.ts` | 291 + 123 | `table_style` | B1a |
| `symbol-fonts.ts` | 73 | `text::symbol` | B1a |
| `text-layout.ts`; `pptMeasure` and the plain measurer (render.ts 2085–2150, 1889) | 686 + ≈ 70 | `text::{layout, measure}` | B3 |
| `diagram-metrics.ts` (`measureApprox`, `lineFactor`: SmartArt's own width tables, no browser) | 96 | `smartart::metrics` | B3 |
| `render.ts` model → canvas calls (everything except the shell half below) | ≈ 2,160 of 2,662 | `emit::{shape, body, text, table, fill, line, picture, chart}` | B4 |
| `raster.ts` | 34 | `special` | B4 |
| `chart-engine.ts` (bridge to `shared/charts`) | 205 | `emit::chart` (calls `uo-charts`) | B4 |
| `diagram-layout.ts`, `diagram-fallback.ts` | 2,497 + 429 | `smartart` | B5a |
| `metafile.ts`, `emfplus.ts`, `pict.ts` | 1,574 + 1,041 + 533 | `metafile`, `pict` | B5b |
| `text-warp.ts` geometry | 594 | `text::warp` — **PDF effects wave** (the viewer runs it in the shell) | after M4 |

Total ≈ **12,000 raw TS lines** (+ 594 later).

### What stays in uo-pptx

The presentation (`uo-pptx.md`): package session, slide/layout/master graph and `readSlide`'s inheritance
passes, deck-level text-style context (`buildTextStyleContext` builds the *PowerPoint* `TextStyleContext`
that this crate's cascade consumes), notes, embedded fonts, timing/animation/playback/slideshow/media,
editor, `.ppt`, exporters. Also the **slide frame** of the emission (`paintSlide`: background fill, pattern,
gradient, `bgImage`, the slide clip) — a dozen lines in `uo_pptx::emit_slide`, around this crate's
`emit_shapes`.

### What stays in the TS shell (painter side)

`render.ts`'s pixel-reading effects and canvas plumbing: `applyBevel` + `edt1d` + the `SRGB_LIN`/`LIN_SRGB`
tables (`getImageData`), `fxImage` (recolour by `getImageData` of the decoded image), the layer pool
(`acquireLayer`/`discardLayer`/`compositeLayer`), `guarded`'s save-depth repair, `scratchCanvas`, the warp
raster (`paintWarp` from the `paintWarpedText` call on, plus `text-warp.ts`), and the `TextRun` fit
(`measureText` at device scale). ≈ 500 lines of `render.ts` + 594 of `text-warp.ts`. They are driven by
`Effect` items and `TextRun`s (`uo-render.md`).

## Dependency direction

```
uo-jsnum ── uo-text ─────────────────────────────────────────────────┐
uo-opc (xml, cfb, zip) ──┬─ uo-drawingml (presets, colour, masks, odraw)│
                          │        │                                    │
                          │   uo-render (display list: Write page + Slides list, codec, painter contract)
                          │        │                     ▲
uo-numfmt ──► uo-charts ──┼────────┤ (charts emit list items; numfmt formats labels)
                          ▼        ▼
                     uo-dml-render (shape model, reader, theme/HSL, cascade, table styles,
                                    text layout + measurement protocol, emit, SmartArt, metafiles, PICT)
                          ▲                         ▲
                     uo-pptx (presentation)    uo-xlsx / Sheet grid (drawings)
                          ▲                         ▲
                     uo-wasm (uo-slides*.wasm) / uo-ffi
```

- `uo-dml-render` depends on `uo-opc`, `uo-drawingml`, `uo-render`, `uo-jsnum`, `uo-text`, `uo-image`
  (TIFF variant, header sizes and dpi for the server path), `uo-charts`, and `uo-fonts` only for the PDF
  measurer (behind a feature). It never depends on `uo-pptx` or on anything of Sheet.
- `uo-pptx` depends on `uo-dml-render` (and transitively the rest); Sheet's crates depend on
  `uo-dml-render` too. Neither app crate depends on the other.
- **Charts** (`uo-charts`, plan §6, not yet created): `emit::chart` calls
  `uo_charts::emit_chart(&ChartSpace, rect, ppt, &ChartTheme, &MeasureTable, sel) -> sub-list` into the
  same Slides list, and `uo_charts::part_bounds` for `chartPartBounds` (animated chart parts). The chart
  theme resolver (`colorFromDml` with the presentation's clrMap) is passed **from this crate** as a trait
  object (`ColorResolver`), as the TS passes it to `parseChartSpace`. Charts measure text (`measureText`,
  `'Hg'` font bounding boxes: `shared/charts/src/render.ts:184, 220, 615, 1059`) through the same
  `MeasureTable` (see below). Sheet's charts call `uo-charts` directly with Excel defaults.
- **Number formats** (`uo-numfmt`, being ported now in its own crate by another agent): used by
  `uo-charts` for labels; this crate does not call it directly.

## Data model (summary; skeleton in `src/model.rs`, `src/theme.rs`, `src/text_style.rs`)

- `Shape` mirrors `model.ts:275` field by field. Hot fields inline (ids, name, placeholder, `xfrm`, geometry,
  fill, line, text body id, image id, flags); **rare payloads boxed** in `ShapeExtras` (effects: shadow,
  glow, soft edge, reflection, inner shadow, fill overlay; 3D: bevel, extrusion, camera; media; chart;
  table id; SmartArt ids; group ids). `size_of::<Shape>()` is a test (≤ 256 bytes).
- `ShapeArena`: flat `bodies`, `paras`, `runs`, `tables`, `rows`, `cells`, `images`, `cust` (see
  `uo-pptx.md` P4). `ShapeList { shapes: Vec<Shape>, arena: ShapeArena }` is what the reader returns and the
  emitter takes; `uo_pptx::SlideModel` embeds one.
- Colours: `CssColor` — the TS keeps colour **strings** (`'#' + val` as written, or the uppercase `hex()` of
  a modified colour, or `rgba(...)` built later). Inline `#RRGGBB` (7 bytes) or boxed for anything else;
  equality and the serialized string are the TS's.
- `Theme`: `colors`/`scheme`/`clr_map` as `JsRecord` (ordered, JS key order), `fonts`, `line_widths`, and the
  three style lists as element ids into the theme part's `Arc<uo_opc::xml::Document>` (the TS stores the
  `XmlElement`s themselves, `model.ts:552-555`).
- `TextStyleLevel`/`StyleLevels` (9 levels, `Box<[TextStyleLevel; 9]>`)/`TextStyleContext` mirror
  `text-style.ts:40-334`; the placeholder maps are ordered small maps with string keys (`uo-pptx.md` P3).
- `ImageRef` → `ImageRef { media: MediaId, mime, fx, tile, fill_rect }`; the bytes are the media table's
  `Arc<[u8]>` (shared with the part store). Sheet's package supplies its own media table through the same
  `MediaSource` trait as `PartSource`.

## Measurement protocol (the advance-table input, decision T1)

Every measurement the TS asks of the browser, found by reading `text-layout.ts`, `render.ts`,
`text-warp.ts`, `metafile.ts`, `pict.ts`, `diagram-*.ts` and `shared/charts/src/render.ts`:

| # | Site | Font string | State | Asked | Used as |
|---|---|---|---|---|---|
| **A1** | `pptMeasure`, per code point of a `PER_CHAR` string (render.ts:2123–2133) | `layoutFontStr({…f, sizePt: 1000}, 1)` = `[italic ]{700\|400} 1000px <stack(family,bold,italic)>` | `fontKerning='none'` (when supported) | `measureText(ch).width` | `em = w / 1000`, then per use `Math.round(em × sizePt × 8) / 8` — **unrounded in the table** |
| **A2** | `pptMeasure` kerning (2137–2146), only if `f.kern && text.length > 1 && 'fontKerning' supported` | same 1000 px string | `none` then `normal` | `w0`, `w1` of the **whole string** | `((w1 − w0) / 1000) × sizePt` added unrounded |
| **A3** | `pptMeasure`, non-`PER_CHAR` string (2104–2111; astral characters fail `PER_CHAR`, see traps) | `layoutFontStr(f, 1)` = the run's size in px | `fontKerning = f.kern ? 'normal' : 'none'` | `measureText(text).width` | width in pt as is |
| **A4** | the plain measurer of `paintWarp` (1889) and its `trailing` widths (1895–1900) | `layoutFontStr(f, 1)` | the context's current kerning (default `auto`) | `measureText(text).width` | the WordArt layout input |
| **A5** | unit animation pieces (1801–1806) | `layoutFontStr(fontOf(r.font, pst), pxPerPt)` = **device px** | current kerning (`auto`) | `measureText(prefix).width`, `measureText(piece).width` | the per-letter/word boxes → `AnimBase` of unit slots |
| **A6** | `drawText` fit (1652–1655), underline/strike of a piece (1695, 1702) | device px | `fontKerning` per run, `letterSpacing` | `measureText(text).width` | **painter side**: the `TextRun` carries the laid width, the painter measures and fits as the TS |
| **A7** | warp `measureLine` (text-warp.ts:333–380) | `font(10)` (10 px/pt) | `letterSpacing = spc × 10` | width, `actualBoundingBox{Left,Right,Ascent,Descent}`, `fontBoundingBox{Ascent,Descent}` (fallback to actual) of runs, prefixes, trimmed runs | **shell** in the viewer (`Effect::Warp`); for the PDF wave from `uo-fonts` |
| **A8** | metafile font probe (metafile.ts:694–701, `fontAvail`) | `40px <family>` vs fallback | — | `measureText(probe).width` | font availability decision |
| **A9** | metafile text (749–776): `'Hg'` metrics and per-char widths when the record has no `dx` | the record's font | — | `fontBoundingBox*`, `actualBoundingBox*`, `width` | text placement in the metafile layer |
| **A10** | PICT underline (pict.ts:275) | the record's font | — | `measureText(str).width` | underline length |
| **A11** | charts (`shared/charts/src/render.ts:184, 220, 615, 1059`) | the chart text font at chart px | — | widths; `'Hg'` `fontBoundingBoxAscent + Descent` | label layout (`uo-charts`) |
| **A12** | image natural size (`imgW`/`imgH`, render.ts:407–420) | — | — | decoded `naturalWidth/Height` (EXIF-oriented, SVG by the browser) | tiles, crops, picture bullets |
| — | SmartArt text fit (`diagram-layout.ts:1756, 2203`) | — | — | **none**: `measureApprox` (own tables) unless a measurer is passed (never, in the TS read path) | ported as tables |

**Table format** (`uo_dml_render::text::measure`, skeleton in `src/measure.rs`):

```rust
pub struct MeasureTable {
    pub caps: Capabilities,             // font_kerning, letter_spacing, segmenter, font_bounding_box (the TS tests each)
    pub generation: u32,                // the shell's font generation (a FontFace loaded → new generation)
    char_em: Map<(FaceKey, u32), f64>,              // A1: raw width at 1000 px (the /1000 is done in Rust, as the TS)
    kern: Map<(FaceKey, Box<str>), (f64, f64)>,      // A2: (w0, w1) at 1000 px
    whole: Map<(FaceKey, SizeBits, Kerning, LsBits, Box<str>), f64>,  // A3, A4, A5, A10: width
    metrics: Map<(FaceKey, SizeBits, Kerning, LsBits, Box<str>), TextMetrics>,  // A7 (PDF wave), A9, A11
    font_available: Map<Box<str>, bool>,            // A8
    images: Map<MediaId, (f64, f64)>,               // A12
}
pub struct FaceKey { pub family: Box<str>, pub bold: bool, pub italic: bool }   // fontStack is fixed per paint
```

- **FaceKey, not the CSS string**: within one paint `fontStack` is a fixed function of `(family, bold,
  italic)` (render.ts:92), so the key is equivalent and the shell builds the identical string (T2: the
  stack, with fonts installed by name, is the shell's).
- **Sizes and spacings are keyed by f64 bits** (`SizeBits(u64)`); the shell answers for exactly the value
  the Rust side will compute (both sides build `sizePt × pxPerPt` the same way: the Rust request carries
  the bits).
- **Two-phase**: `layout_text_body(…, &MeasureTable)` records misses instead of guessing; an emit with
  misses returns `NeedMeasures(Vec<MeasureReq>)`, the shell measures them all (one batch per frame, as
  Write's jobs) and the emit runs again. Round 0 pre-fills A1 for every code point of every run per face
  (one cheap batch), so most slides finish in one round; A2/A3 depend on the line breaks and may need a
  second. Rounds are capped (8) — past the cap the emit fails typed (never a placeholder in output).
- **Server/PDF path**: the same table filled from `uo-fonts` metrics (advance widths × 1000 / upem, the
  kerning of GPOS/`kern` pairs summed per string); results differ from the browser's by design (plan §4.3).
- **Capture**: the TS run records every A1–A12 answer; native replay feeds them, so parity is against the
  true TS without proprietary font files in CI.
- **Capabilities** change results and are part of the table: `'fontKerning' in ctx` (A2 skipped, A3 kerning
  unset), `letterSpacing` support (painter fit, warp), `Intl.Segmenter` (grapheme vs code point breaking,
  `text-layout.ts:217–221`), `fontBoundingBox*` support (charts, warp fallback).

## Emission (B4; the item set is `uo-render.md` "Slides extensions")

```rust
pub struct EmitParams { pub scale: f64, pub ox: f64, pub oy: f64, pub slide_w: f64, pub slide_h: f64,
                        pub clip_overflow: bool, pub suppress_text: Option<u32>, pub anim: Option<AnimInputs> }
pub fn emit_shapes(list: &ShapeList, p: &EmitParams, m: &MeasureTable, out: &mut SlidesList)
    -> Result<Segments, NeedMeasures>;
```

- **One display list per (shapes, scale, origin)**: the TS computes every coordinate as
  `ox + X × scale` in JS doubles and several decisions in px (see `uo-render.md` "Scale-dependent
  decisions"); the emitter reproduces the same arithmetic, so the list is valid for one viewport.
- **Per-shape isolation** (`guarded`): each shape is emitted into a sub-range; an `Err` (hostile data the TS
  would have thrown on) truncates that range back and the next shape goes on — the TS's "skip the shape".
- **The canvas IR rule** (P-DL1): the list is the TS's canvas call sequence, with every value the TS computed
  from model data precomputed here, and every value it computed from canvas state (`getTransform`, device
  `measureText`, `getImageData`, canvas size, capability tests) left as a symbolic operand the painter
  resolves. Only pixel-reading or device-measuring code becomes an `Effect`.

## Fuzz targets

`dml_read` (structure-aware `p:`/`xdr:` shape trees: groups 10⁴ deep, huge `avLst`, NaN/∞ attributes),
`dml_theme_color` (colour elements with every modifier and garbage values), `dml_text_layout` (random bodies
× random `MeasureTable`s incl. NaN, 0, 1e308, missing entries), `dml_emit` (random `ShapeList` → emit: no
panic, item count bounded by a function of the input), `dml_smartart` (data/layout/style parts, depth),
`dml_metafile` (EMF/WMF/EMF+ bytes), `dml_pict` (PICT bytes), `dml_table` (rows × cols at the limit, merges
pointing outside).

## Memory

Caches here: the generated-bitmap caches (`pattCache` 256, `pathGradCache` 64 / 16 MiB) and the chart model
cache (32) — `uo-pptx.md` M5 table. `clear_caches()` empties them (M1). M1 covers `read_shapes`,
`read_drawing_elements`, `read_theme`, `layout_text_body` (both phases), `emit_shapes`, `special_image`.

## Questions for the M4 gate

- DR1: `text-warp.ts` stays in the shell for the viewer and moves to Rust with the PDF effects — accept?
- DR2: `FaceKey` instead of the CSS string as the table key (equivalent within a paint) — accept?
- DR3: `size_of::<Shape>() ≤ 256` with boxed extras — a budget, not a behaviour; recorded at B1a.

## B1a result (2026-11-17, Sonnet)

Ported 1:1 into `core/crates/uo-dml-render`: `theme.rs` (`readTheme`, `applyClrMap`, `clrMapAttrs`, `colorFromDml` — the PowerPoint
profile —, `textFillColor`, `solidFillColor`, `lumHsl`), `text_style.rs` (`parseTxStyleLevels`, `parsePPrLevel`,
`mergeStyleLevels`, `applyTextStyles`, `applyShapeTextCascade`, `readRunExtras`), `table_style.rs` + `table_styles_data.rs` (the 74
embedded styles, generated from the TS file; `readTableStyle`, `applyTableStyle`, `mergeOrigins`, `cellLine`), `symbol.rs` +
`symbol_data.rs` (Symbol/Wingdings/Webdings tables, generated; BSD-2 notice in the header), `read.rs`/`read_dml.rs`/`read_text.rs`
(the shape half of `read.ts`: `readShapes`, `readGroup`, `readGraphicFrame`, `readTable`, `readShape`, `readPic`, `readTxBody`,
`readParagraph`, `readRun`, `readEffects`, `readBevel`, `readPicMedia`, `parseCustGeom`, fills/gradients/patterns, blips and image
effects, OLE preview from VML, chart theme/images/format, SmartArt drawing, `readDrawingElements` with the `xdr:` prefix), `source.rs`
(`PartSource`, `RelMap`, `readRels`, `findRelTarget`, `resolveRelTarget`, `ReadError`, `Limits`, the slides XML preset), `canon.rs`
(feature `canon`).

### Decisions outside the source semantics (declared)

- **A1. Owned trees instead of the arena** (`ShapeArena` and the `BodyId/ImageId/CustId/TableId/Span` ids are gone from `model.rs`;
  `ids.rs` keeps them because the B3 skeleton `layout.rs` and `timing.rs` still name them). Why: the reader clones shapes across
  lists (inherited decorations are read from the master/layout and cloned into each slide, the placeholder `spPr` merge copies
  images and custom geometry, the style cascade copies picture bullets into paragraphs): with arenas every crossing remaps
  indices and the cascade functions need `&mut arena` threaded through; with owned trees each is a `clone()`. Measured cost:
  `Shape` 408 B, `Run` 104 B, `Paragraph` 368 B, `TextStyleLevel` 512 B, retained slide 0.95× the TS heap (`uo-pptx.md`, M3): the
  0.5× target is open, with the compaction list there. DR3 (`size_of::<Shape>() ≤ 256`) is therefore NOT met (408); a test pins 416.
- A2. `Shape.fill` is `Option<Box<Fill>>` (a `Fill` is ~150 B); rare payloads stay in the boxed `ShapeExtras`; `ChartRef.theme` is
  `Arc<Theme>`; `ImageRef` keeps `MediaId` (the host's media table: part name and `Arc` bytes) and the mime string.
- A3. `Theme.styles` holds element ids into the theme part's parsed document (shared `Arc<Document>` of the parse cache), each list
  optional on its own; `JsRecord`/`PhMap` are insertion-ordered vectors with overwrite-in-place (`Map.set`, `obj[k] = v`).
- A4. `xdr:`/`dsp:` renaming (`renamePrefix`) is done by writing the subtree as text with the names renamed (and the hidden shapes
  pruned for sheet drawings) and parsing it again; it is also how SmartArt `dsp:sp` shapes reach `readShape`. The `FrameOverride`
  of the skeleton (Sheet's `placeElement` round trip) is NOT applied by `read_drawing_elements`: the Sheet host places the element
  before the call, as the TS does; the type stays in `source.rs` for the Sheet frontier.
- A5. Caps (the TS has none): group/AlternateContent depth (64), shapes per list (100,000), `rows × columns` of a table (1,000,000).
  Over a cap the read returns `ReadError::Limit`, never a stack overflow or an out-of-memory.
- A6. `Math.pow` of the colour code is `uo_jsnum::math::pow` (V8's fdlibm): the oracle numbers of the TS comments reproduce exactly
  (04617B shade 15% → `#012430`, satMod 320% → `#00abfe`; unit test).
- A7. A SmartArt without a cached drawing: `regenerateDiagram` and `diagramFallbackShapes` are B5a (nothing is generated here);
  `diagramStyler` goes with them. OLE previews from VML are matched by hand-written scanners equal to the TS regexes.

### Parity and memory

The reader is replayed through `uo-pptx` (see "B1b result" there): 1,972 facets equal, 0 mismatches over 228 decks. Unit tests
(`tests/units.rs`, 11): measured colour values, clrMap, merge rules of the levels, relationship helpers and `Map.set` semantics,
the sheet drawing path (prefix, hidden shapes, group ids), OLE preview from VML, chart theme override/images/format, group flip +
rotation arithmetic, table style bands and merges, symbol fonts, M1 of `read_theme`/`read_shapes`/colours (exact, error path
included). `clear_caches()` stays empty: this crate keeps no cache yet (B4 adds the bitmap caches with their caps).

### Gaps left

B3 (`layout.rs`, `measure.rs`), B4 (`emit.rs`; `table_style::tableImages`), B5a/B5b as listed in `uo-pptx.md`; the full
`THIRD-PARTY-NOTICES.md` entry for the dingbats data (BSD-2) should be generated with the crate notices.

## B3 — text layout (2026-11-17, Opus)

Ported 1:1: `text-layout.ts` (686) and the two layout measurers of `render.ts` (`pptMeasure` 2102–2150 and the plain
measurer of `paintWarp` 1889) into `src/text/` (`layout.rs`, `measure.rs`); `cellBody` (render.ts:2160) as
`text::cell_body` for B4 and the replay; the grapheme segmentation the layout relies on (`Intl.Segmenter('grapheme')`,
text-layout.ts:217) as the new `uo_text::grapheme` (see `uo-text.md`), and `\p{Ll}` (small caps) as
`uo_text::props::is_lowercase_letter`. `diagram-metrics.ts` is **not** in B3 any more: the B5a agent, running at the same
time, took `smartart/` (its `metrics.rs` replaced the B3 port, with a `DiagramMeasure` trait); the skeleton row above is
superseded. The arena `ImageId`/`BodyId`/`CustId`/`TableId` of the first design are gone from `ids.rs` (the layout clones
`ImageRef`s, decision A1); `Span` stays (uo-pptx `timing.rs`).

API (`uo_dml_render::text`): `layout_text_body(tb, box_w, box_h, Measurer, &MeasureTable, &LayoutTextOptions) ->
Result<TextLayoutResult, LayoutError>`, `need_measures(…) -> Vec<MeasureReq>` (empty = ready), `layout_text_body_traced`
(also returns every question asked: parity harnesses), `format_auto_num(scheme, n)`, `cell_body(&TableCell)`, the result types
(`PlacedLine` with owned `PlacedRun`s, `PlacedBullet`, `LayoutFont`) and `MeasureTable` with `set_*`/lookup methods, caps and
the font generation. `Canon::layout` (feature `canon`) prints a result as the TS canonical JSON.

### The measurement protocol as built

- `MeasureTable` interns faces (`family → [bold×italic]` index) and keys strings by `&str` in nested maps, so no lookup
  allocates. A1 `(face, char) → width at 1000 px`, A2 `(face, text) → (w0, w1)`, A3/A4 `(face, size bits, kerning, letter
  spacing bits, text) → width`; the metrics, font-availability and image-size maps of the skeleton stay for B4/B5b.
- A1 is measured with `fontKerning = 'none'` where the context has the property and in the current state where it has not
  (the TS sets the state only `if (prevK !== undefined)`); A2 is never asked without it; A3 uses `normal`/`none` by the run's
  kerning, or the current state without the capability; the plain measurer always the current state (`Kerning::Current`).
- A pass with misses counts each missing answer as 0 and goes on, so one pass collects them all; while any A1 advance is
  missing only the A1 questions are returned (the advances decide the breaks that decide the A2/A3 strings), then the rest.
  Measured over the replays: at most **6 rounds** (b3a), 2 on the samples; `MAX_ROUNDS = 8`.
- Caps (M5): 65,536 answers or 8 MiB per table (an insert past either clears the table first; the TS clears per paint, so
  no value depends on it); `set_generation` (a `FontFace` loaded) empties it.

### Decisions outside the source semantics (declared)

- **B3-1. No memo.** `pptMeasure`'s `advCache` and its whole-string memo return what the loop computes again from the same
  answers (the memo key prints `sizePt`, so `-0`/`0` and every `NaN` collide, and they give the same sum); the Rust layout
  recomputes. The TS asks each advance once per paint; Rust looks up the table each time (no allocation).
- **B3-2. Two-phase with zero placeholders** (above): the discarded passes may ask questions the true path never asks
  (answered and kept by the shell, harmless).
- **B3-3. A lone low surrogate is written U+FFFD in measurement keys.** The kerning seam (`text-layout.ts:504`) builds
  `String.fromCodePoint(prev.codePointAt(prev.length - 1))`, which after an astral character is the lone low surrogate; a Rust
  `str` cannot hold it, and a canvas draws U+FFFD for it. U+FFFD is outside `PER_CHAR` like the surrogate, so the measurer
  takes the same branch.
- **B3-4. The endless `displayRuns` recursion throws.** A symbol-area character the Symbol (or `a:sym`) table maps to 0
  (U+F07F–U+F0A0 in Symbol) stays in the area after `symbolText`, and the TS recurses until `RangeError`, which `guarded`
  turns into "skip the shape". Rust recurses with a depth cap of 64 and returns `LayoutError::Throws("RangeError")` (the
  replay compares the error class: 57 such calls in b3a).
- **B3-5. Paragraph levels index an array.** `autoNumCounters[level]` with a negative or `NaN` level (the reader's
  `parseInt(lvl)`) is a property that persists like an element; `Counters` keeps the TS key semantics (`String(level)`: `-0` is
  `0`, every `NaN` one key) and the reset loop `for (l = level + 1; l < 9; l++)` reaches only the keys it would step on (a
  huge negative level, where the TS loop runs billions of steps or forever, resets the same keys at once).
- **B3-6. `constructor` is a symbol font** (fix in B1a's `symbol.rs`): `norm(face) in T` finds `Object.prototype.constructor`;
  the TS then draws the text unchanged in "Segoe UI Symbol". No other prototype key survives `norm`.
- B3-7. The result keeps the TS optional fields as `Option` where the TS tests `!== undefined`, and as `bool` where it only
  ever writes `true` (`strike`, `fillTile`, `kern`); run texts are owned `String`s (the skeleton's arena `Span` was dropped:
  B4 consumes a result per body and drops it).

### Parity

Capture `core/crates/uo-pptx/js/capture-textlayout.mjs` (format and walk in its header, `uo-pptx-captures.md`): the frozen
`layoutTextBody` + `pptMeasure`/plain measurer over a context whose widths are the real advances (hmtx) and GPOS/`kern`
pairs of the open fonts (`site/assets/fonts/liberationsans-*`, `shared/fonts/files/*`; other families by their metric
substitute, else Liberation Sans), read at capture time and never copied. Replay `core/crates/uo-pptx/tests/text_layout.rs`
(the log becomes the `MeasureTable`; inputs, the asked-measurement set and the canonical result are compared exactly):

| Capture | Decks | Layout calls | Facets equal | Mismatches | Two-phase replays (max rounds) |
|---|---|---|---|---|---|
| `textlayout-samples` (tracked `.pptx` samples) | 28 | 1,682 | 5,409 | 0 | 240 (2) |
| `textlayout-b1a` | 200 | 5,100 | 16,200 | 0 | 392 (5) |
| `textlayout-b3a` (new text-heavy group, `lib/gen/b3.mjs`) | 120 | 2,807 | 8,910 | 0 | 249 (6) |
| `textlayout-b3a-nokerning-noseg` (no `fontKerning`, no `Intl.Segmenter`) | 120 | 2,807 | 8,910 | 0 | 163 (6) |

100 % on the first run of the port; the one failing case of the first replay (the no-`fontKerning` A1 key) was the
harness's log→table conversion, not the port. Replay time 0.2–3.4 s per capture (release). The captures are byte-identical
on a second run (apart from `@platform`); committed: decks/b3a 2.8 MB, captures and logs 5.1 MB.

### Memory and robustness

- M1 (`tests/text_layout.rs` of this crate): `layout_text_body` (ok, missing answers, the throwing path), `need_measures`,
  `layout_text_body_traced`, `format_auto_num`, a filled and dropped `MeasureTable`: live bytes back exactly.
- M5: the table filled past 1,000 entries, past a 64 KiB byte cap and past the default 65,536 entries; generation change
  empties it. The layout keeps no cache (`clear_caches()` unchanged).
- No panic, bounded: 1,000 random bodies (NaN/±∞/negative/huge numbers, levels −3…10, hard snippets) × hostile tables
  (NaN, negative, 1e308, ∞, missing answers) through the two-phase loop in 0.56 s (debug); `UO_DML_LONG=1` 20,000 in 2.2 s
  (release). Line count ≤ characters + paragraphs is asserted.
- Fuzz target `text_layout` (`core/fuzz/src/text_layout.rs`, seeds in `seed.rs`): byte-driven bodies and tables; checks
  purity (two layouts equal as canonical text) and the line bound. Fallback driver: 713,191 executions in 60 s, 0 crashes.

### What B4 needs from the text side

1. Call `layout_text_body` with the box `(cx × scale) / scale` after the text rectangle of the geometry, `bodyPr rot`, the
   `vert` swap and the unflip (paintText's order), and the plain measurer + `wrap: 'none'` rule for WordArt; tables use
   `cell_body` and `1e12` for the row growth, as `paintTable`'s `needOf`.
2. Batch `need_measures` over every body of the slide per round (one shell batch per round), cap at `MAX_ROUNDS`; a
   `LayoutError::Throws` is the TS's skipped shape.
3. The `TextRun` fields come from `PlacedRun` (`laid_w = w × scale` when the text is the run's, `rise × scale`, `spc × scale`,
   `kern`); unit animations (A5) and the painter's fit (A6) measure at device scale on the shell side, not in this table.
4. The PDF path fills the same table from `uo-fonts` (advances × 1000 / upem per character, the GPOS/`kern` delta of the
   whole string at 1000 px): the capture's measurer is that function, so a `uo-fonts` filler can be checked against these
   logs.

## B5a — SmartArt (2026-11-17, Sonnet)

Ported 1:1 into `src/smartart/`: `engine.rs` (`diagram-layout.ts` 17–1744: presentation tree, constraints, `lin`, `snake`, `cycle`, `pyra`,
composite, hierarchy, text rules), `emit.rs` (connectors, text fit, the generated `dsp:spTree`, `layoutDiagram`, `supported`), `data.rs`
(data model, axes, `forEach`/`if` iteration), `fallback.rs` (`diagram-fallback.ts`), `styler.rs` (`diagramStyler` of `read.ts`), `metrics.rs` +
`metrics_data.rs` (`diagram-metrics.ts`, tables generated from the TS) and `xt.rs` (the owned `XmlElement` tree, `normNs`, XML text). Hooks
in `read.rs` (`read_graphic_frame`: regenerate, then the fallback; `read_diagram_shapes_in` takes a root). Public API:
`regenerate_diagram`, `layout_diagram(&DiagramInput)`, `diagram_fallback_shapes`, `diagram_styler`, `DiagramMeasure`/`ApproxMeasure`.

**Parity** (`uo-pptx/tests/replay.rs::b5a_smartart_decks_match_the_ts`): generator group `b5a` (seeds 500000–500149, 150 decks, 3.7 MB, `js/lib/gen/b5a.mjs`),
capture `model-b5a.capture` (0.05 MB; the slide model contains the graphic frame's shapes). TS side: 2,431 regenerated shapes, 479 fallback shapes,
3,266 `dgmId`s. Rust replay: 970 facets equal, 0 mismatches, first run. Families (layout definitions authored in the generator): default (snake),
process1 (lin + conn), vList2 (lin, rules `h INF`), cycle2 (cycle, arrows and `curve` arcs), radial1 (`ctrShpMap`), hierarchy (hierRoot/hierChild,
`forEach`), pyramid1 (pyra with accounts), matrix1 (composite `ar`), chevron1, hList1; modes: regenerate, cached drawing, empty drawing, drawing found by
the name guess, unsupported algorithm → fallback (13 uniqueIds), missing layout/data/style/colours; data with levels, `asst`, custT sizes, pPr spacing,
presentation points (custScale, custLinFactNeighbor, presLayoutVars), user `spPr`, duplicate/cyclic/dangling connections. The four committed samples with
SmartArt all carry a cached drawing (covered by `model-samples`). No Office layout file is used: the definitions are authored; parity is Rust vs TS on the same input.

**Decisions outside the source semantics** (declared):
- B5a-1. Object references are indices (`NId` into the node list, `PtId` into the points); the module state of the TS (`FIT`, `FRAME`, `SCALE_BUDGET`) is fields of `Eng`.
- B5a-2. The `{ ...c, constrs: [k] }` shallow copy of `evalKidsOnly` shares `v`, `equ`… with the original and is a distinct identity; reproduced as an `owner_copy` flag: a limit
  (`lte`) whose reference is that copy is stored with no node (its `fontPt` is never set in the TS either).
- B5a-3. The generated `dsp:spTree` goes back to the shape reader as XML text (`xt::to_xml` + `parse_xml`), like `renamePrefix` (A4).
- B5a-4. Bounds the TS does not have (it ends in a `RangeError`/hang that `read.ts` catches): XML nesting over 1,500 levels refuses the part, a data model over 200,000 points
  regenerates nothing, the org-chart fallback stops after 2,000,000 calls or 1,500 levels (a cyclic/shared graph) and gives no shapes, `fitText` starts at most at 1,000,000 pt, the arc search
  of a curved connector has an iteration cap.
- B5a-5. `diagram-metrics.ts` is ported here (96 lines, tables generated into `metrics_data.rs`), not in B3: it needs no `MeasureTable`. The engine takes a `DiagramMeasure` trait
  (`ApproxMeasure` by default, as the TS never passes its own) so a host with real advance tables can plug in without touching the engine. B3 did not need to meet it.
- B5a-6. A TS throw inside the styler (`parseXml` of the built `fillRef` text, a negative style index) is `Err(StyleThrow)` and the fallback returns no shapes, as `read.ts`'s `try`.

**Memory** (`tests/smartart.rs`): M1 exact for `regenerate_diagram` (with and without layout) and the fallback, after warming the width tables. M5: the width tables are `OnceLock`
statics built from constants (the TS caches them per face): nothing grows with the input, so no cap exists; `clear_caches` stays a no-op. Bounded no-panic over cyclic/duplicate/dangling
connections, 5,000 nodes (tree capped at 3,000), NaN/INF/1e400 attributes, a `forEach ref` that recurses into itself, 5,000-level XML. Fuzz target `smartart` (`core/fuzz`, parts split at NUL).
Replay of the 150 decks: 1.2 s (release).

## Process metrics

| Stage | Agent | Time | Notes |
|---|---|---|---|
| Architecture pass | Opus | one session (shared with `uo-pptx.md`) | boundary, measurement protocol, skeleton |
| B1a port (with B1b, one session) | Sonnet | see `uo-pptx.md` | 0 parity rounds; decision A1 (owned trees) and its M3 cost recorded above |
| B3 port + capture + generator group + tests | Opus | about 1 h 10 min wall (reading 15 min; grapheme generator and port 15 min; layout port 25 min; capture, b3a and replay 15 min; tests, fuzz, docs) | 0 port bugs found by the replay (100 % on the first run); 1 harness fix (A1 key without `fontKerning`); rework: the long-word loop rewritten without nested closures, a surrogate `char` literal, an M5 expectation; the concurrent B5a agent replaced the `diagram-metrics` port and broke the crate build for a while (clippy of this crate re-run after it) |
| B5a port + generator group + capture + tests | Sonnet | two sessions; port and hooks about 3 h, generator/capture/replay/tests/fuzz/docs about 1 h after the resume | 0 parity mismatches on the first replay; rework was clippy only (22 findings: negated float comparisons, identical branches, a too-complex type) |
