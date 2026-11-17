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

## B4 — render (2026-11-17, Sonnet)

Ported 1:1 into `src/emit/` (`render.ts` 13–2662 minus the shell half, `raster.ts`'s role, the `chart-engine.ts` bridge as a gap) and the slide glue
`uo_pptx::emit_slide`. Files: `mod.rs` (API: `emit_shapes` = `paintShapesTo`, `emit_slide` = `paintSlideTo`, `EmitParams`, `Emission`, `SlideBg`),
`cx.rs` (the builder: one method per canvas call, resources, layer and save bookkeeping, `globalAlpha` tracker, picture/derived-image resolution,
the bitmap caches), `shape.rs` (instance transform, glow, soft edge, reflection, fill/shadow/outline/picture body, extrusion, bevel effect, shape
geometry and arrowheads, the shape list), `paint.rs` (gradients with the PowerPoint sub-stops, 8×8 patterns, path-gradient rasters, image tiles, dashes),
`text.rs` (text rectangle, `bodyPr rot`, `vert`, unflip, bullets, runs, WordArt `Warp` effect), `table.rs`, `dpi.rs` (`imageDpi` of
`shared/charts`: PNG/JPEG/BMP resolution), `chart.rs` (the chart gap). Glue: `uo-pptx/src/emit.rs` (`emit_slide(deck, index, View, table, res, caches)`),
`js/emit-uolf.mjs` + `examples/emit_uolf.rs` (the parity driver), `tests/support/stub_measure.rs` (the capture's `stubMeasure` as a `MeasureTable` filler).

**API as built.** `emit_slide(&SlideBg, &[Shape], &EmitParams, &Env, &mut SlidesResources, &mut EmitCaches) -> Result<Emission, NeedMeasures>`;
`Env { src: &dyn PartSource, table: &MeasureTable }`. The call is **one pass**: a pass with measurements the table lacks returns `NeedMeasures` for the
WHOLE slide (every body of every shape and every table cell is laid out in the pass and its missing answers collected: one shell batch per round,
2–6 rounds on the corpora); the shell answers (`MeasureReq::{CharAt1000, KernAt1000, Width, ImageSize}`) and the call is repeated. `Emission { list, chart_frames }`.
One `EmitCaches` per generation of the `SlidesResources` (it maps pattern and path-gradient keys to bitmap ids).

**Parity** (draw form, exact: `emit-uolf.mjs` writes the `.uolf` files, `compare-drawops.mjs` paints them with `paint-slides.ts` over the recorder and compares line by
line with the frozen TS streams; measureText answered by the capture's stub, so no font files):

| Capture | Views | Equal | Different | Cause of the difference |
|---|---|---|---|---|
| `drawops-samples` (53 decks, views a and b) | 1,202 | **1,166** | 36 | all 36 contain a chart frame (`pb-graficos*`, `pb-midia`, `showcase_animado`, `stress-table-chart`): the TS stream has the chart engine's calls |
| `drawops-b1a` (100 decks, view a) | 246 | **246** | 0 | |
| `b5a` SmartArt (150 decks, views a and b; local capture, not committed) | 440 | 440 | 0 | |
| `b3a` text (50 decks, view a; local, 4.7 M full-form lines, not committed) | 107 | 107 | 0 | |

Coverage of what the 1,995 lists contain (`js/dbg-stats.mjs`): 216 k `save`, 207 k text runs (bullets 4,276; outline, shadow, underline, strike, linear/pattern fills), 130 effects (92
WordArt `Warp`, 38 `Bevel`), 256 layers (reflection, soft edge), 12 extrusions (`setTransformReg`), 708 pattern bitmaps, 77 picture patterns (37 tiles), 69 derived
images, shadows (990 off-screen idioms), path gradients, tables. Capability variants of the samples capture: `nokerning,nospacing,nopattern` 1,164 / 1,202 (the 36 charts and 2 views whose
`pat.setTransform`-less fallback `clip + drawImage` is not expressible); `nolayers` 1,162 (the 36 charts and 4 path-gradient/pattern views: with no scratch canvas the TS falls back
to a radial gradient / solid colour, the list's bitmap paint is skipped) — canvases without `OffscreenCanvas` and `<canvas>` do not exist in the targets, so this is a declared gap, not a gate.

**Schema changes** (`core/schema/slides.uos`, regenerated with `core/tools/uo-schema`; **Write's schemas, bytes and the pinned hash test are unchanged**, `uo-schema check`):
`BevelEffect.dir`; `TextRun.bullet` (a bullet is `font`, `fillStyle`, `fillText` only: the TS does not touch the kerning state for it); `PatternRes.tile: TileDef?` with
`TileParams`/`TileDef`/`Edges` (the TS `tilePattern` reads the live transform and the decoded size: the list carries the parameters, the painter evaluates it, so animation
scale reaches the tile); `WarpTile.tile`; `WarpPaint` gained `scaled`, `path`, `fill_to`, `raw` (`RawStop`), `tile: TileParams?`, `fill_rect` (letter fills need the raster box,
known only at warp time: the painter rasterizes a path gradient from the model stops, and builds tiles and stretched pictures); two items: `SetTransformReg(r)`
(`ctx.setTransform(baseM)` of the extrusion) and `Prepare(ImageSrc)` (the TS builds `fxImage` at a point of the sequence: the painter realizes the derived image there, so the order of its
scratch canvases is the TS's). Interpreter (`paint-slides.ts`, `slides-effects.ts`): `tilePattern` + `flippedTile` and `pathGradient` + `gradMode` + `sigmaT` MOVED verbatim, `letterPaint`
rewritten for the new fields (`makeLinearGradient` with `scaled`, path rasters, tiles at `k / 12700`), `bullet`, `prepare`, `setTransformReg`, the `env.tilePattern` hook removed;
`BevelEffect & { dir? }` is plain `BevelEffect`. The CI `tsc` line passes (it already lists the four files); `node --test js/selftest-drawops.mjs` 12/12.
**Recorder change** (`js/lib/record-ctx.mjs`): the recipe hash of an offscreen canvas now skips `set`/`setLineDash`/`gradient`/`stop`/`pattern`/`patternTransform`/`measure`
lines (the `draw` form's drop set: every draw line carries the resolved state). The TS lays text out on the layer's context, so the old hash contained the layout's
`measure` and `set font` probes, which a list that measures from advance tables cannot (and should not) reproduce. `drawops-samples` and `drawops-b1a` were regenerated (the committed
`.ops.gz`/`.capture` changed; the 12 views of `fxImage` order and 104 of layers that differed before this change are the only effect).
`uo-drawingml` gained two accessors (`srgb_lin_f32`, `lin_srgb_u8`) for the extrusion's light table lookups (the tables stayed `pub(crate)`).

**Interpreter rules kept** (R-S3 contract): `fallbackEnd` = end of the else block after the matching `LayerEnd`; `SlotBegin/SlotEnd` = save/restore bracket. A shape with a soft edge
(layer composited at its end) is `LayerBegin .. LayerEnd(Composite) <else block: the shape drawn straight> fallbackEnd`; a shape with a reflection ends its layer with `Keep`, the else
block follows, then (after `fallbackEnd`) `CaptureCtm(0)`, the reflection layer (`TransformByInverseOf(0)`, `DrawLayer`) and the final composite `Save SetTransform(identity) DrawLayer
ReleaseLayer`; the soft-edge mask is a nested layer on the shape's layer. Without layers (the `nolayers` capability) the reflection's last three items still run and add a save/restore pair to the
stream (declared, not a gate). The static emission uses plain `Save`/`Restore` per shape (see B4-3).

### Decisions outside the source semantics (declared)

- **B4-1. Two-phase, one pass per call.** A layout with missing answers returns `Ok(None)` and the pass goes on (other bodies add their questions; the pass's list is discarded). Only the
  TS's `RangeError` (the symbol-font recursion, `LayoutError::Throws`) is `Abort`: the shape is closed (`abort_to`: layers kept-and-released, saves restored) and skipped; the TS
  leaves what it had drawn, the Rust list as well (the items already pushed stay). A media entry first seen in a discarded pass is not registered (a size answer is needed first), so a
  `Media` row keeps the size of its first real use.
- **B4-2. The canvas state is tracked only where the TS reads it back.** `ctx.globalAlpha` (the outline restores `prevAlpha`) by a small tracker in `Cx` with the canvas rule (a set outside
  0..1 is ignored; a layer starts at 1); the transform is NOT tracked: `baseM`, the reflection's inverse and the use-bg-fill window are registers, `tilePattern`'s `hypot(a, b)` is the painter's.
- **B4-3. Static emission.** No slots (`SlotBegin/End`), no paragraph/unit animation hooks, no `MediaFrame`, no `AnimInputs` in `EmitParams` (B6: it re-emits with the slot template; the plain
  `Save`/`Restore` per shape becomes a `Shape` slot there). `fontScale` emphasis, `fillColor` animation overrides and the `suppress_text` overlay: only `suppress_text` is wired.
- **B4-4. Charts** (a gap until C-6; now closed). `paint_chart_slot` paints the chart through `uo-charts`: `emit/chart.rs`, `chart_host.rs` (host, measurement, caches) and `chart_canvas.rs` (the `Canvas` of the
  engine as list items); record `uo-charts.md` "C-6 — Slides host". `Emission::chart_frames` is kept and always empty. `Cx` is exported now.
- **B4-5. Bitmap caches are per generation.** `EmitCaches` maps the TS key of `pattCache` (`prst + fg + bg + alphas`) and `pathGradCache` (path, fill rect, grid, stops) to ids in the resources' bitmap table.
  Caps (M5): 256 patterns; 64 path gradients and 16 MiB of their bytes (the TS clears at `size > 64` before a set, reproduced). A cleared cache sets `overflowed`: the resources' table keeps the old bitmaps,
  so the host starts a new generation (new `SlidesResources` and `EmitCaches`). The TS `pattCache` has no cap. The parity tool makes a fresh `EmitCaches` per slide because the recorded TS ran a fresh
  `render.js` instance per slide (its caches are module state).
- **B4-6. Image size and availability.** A picture is "available" when the package has the media; its decoded size is the table's `ImageSize` answer (A12; 0 × 0 is legal, as the TS's fake image). The parity
  tool answers with the TS recorder's `imageSize` (PNG, GIF, JPEG headers, else 96 × 96). `imageDpi` is a port of `shared/charts` (`emit::image_dpi`, pub): `uo-charts` can reuse or replace it.
- **B4-7. Letter fills with both a pattern and a gradient/picture** keep only the last paint: the TS builds the overridden pattern's canvas as a side effect, the list does not (no corpus case).
- **B4-8. Capability fallbacks that change the paint kind** (no scratch canvas: pattern → solid, path gradient → radial; no `setTransform` on patterns: stretched picture by `clip + drawImage`)
  are not in the list; targets have both. `nolayers`/`nopattern` captures differ only there.
- **B4-9. Sorting** the extrusion quads uses `uo_jsnum::sort::js_sort_by` (V8's decisions); `Math.hypot`, `round`, `pow`, `sin` ... are `uo_jsnum::math`; the light tables are `uo-drawingml`'s.
- **B4-10. A WordArt (`prstTxWarp`) body is an `Effect::Warp`** carrying the placed lines (runs with face ids, `x`/baseline/height in pt, the spc, outline, pattern, alpha, letter fill as a spec) and
  the shadow/tile parameters; the layout runs with the plain measurer inside a `Save`/`Restore` pair (the TS measures on the context there), with the trailing-space widths measured through
  the table (A4). The warp raster itself is the shell's (`slides-warp.ts`, DR1).

**Memory** (`uo-pptx/tests/emit.rs`, `uo-dml-render/tests/emit.rs`): **M1** `emit_slide`/`emit_shapes` over generated decks (b1a, b3a text and tables, b5a SmartArt), the error paths (unknown slide, an
unanswerable table, `NeedMeasures`) and a whole encode: live bytes back exactly after the outputs are dropped and `Deck::clear_caches` (the lazily built engine tables are warmed first and are
not a leak). **M2** 60 emit-all-views-of-a-deck iterations, flat, tolerance 0. **M3** recorded by `m3_the_cost_of_a_slide_is_recorded` (see the process line). **M5**: pattern cache past 256 and path-gradient cache
past 64 entries stay at the cap with `overflowed` set; `EmitCaches::clear` empties both; the measure table keeps its own caps (B3). `clear_caches()` of the crate is still the empty function: every cache lives in a value
the caller owns (`EmitCaches`, `MeasureTable`, the deck's).
**Robustness**: bounded no-panic over (a) 100 random shape lists with NaN/±∞/1e300 geometry, fills, dashes, layers, vertical/rotated text at hostile viewports (`UO_DML_LONG=1`: 2,000), (b) mutated XML parts of generated decks
(slides, layouts, themes: truncation, deletion, byte flips, hostile tokens, numbers replaced by NaN/1e308/−1) emitted at hostile scales and origins, (c) hostile viewports over a real deck: no panic, every list
validates and encodes, items bounded (`< 200 × shapes + 100`). Fuzz target `slides_emit` (`core/fuzz/src/slides_emit.rs`; invariants: a built list validates and encodes, `save`/`restore` balance, two emissions are equal,
the item count is bounded). Stable driver: see the process line.

**What B5b, B6 and B7 need.**
- **B5b** (metafiles, PICT, TIFF, EOT): their result is a `Bitmap` (RGBA) the emitter draws as a picture: in `Cx::media_img` a media whose bytes are a metafile/PICT/TIFF/EOT must resolve to a generated bitmap (new `Cx` method
  next to `image`, cache by `metafile_part + display size`, `EmitCaches`), raster-op effects are `Effect::MetafileRop`; `MeasureReq::{Metrics, FontAvailable}` answers (A8–A10) are already in the table API, the emitter does not
  ask them yet; `Media.has_svg`/`natural` come from the shell for SVG.
- **B6** (timing, playback): the slot template — `SlotBegin/End` around a shape (replaces the shape's `Save/Restore`), `Background`, `Paragraph(i)` (the `pbox` union of line boxes, `boxBase`), `Unit(k)` (`planUnits`, the pieces
  measured at device px = a second measurement kind), `MediaFrame` over the poster; `EmitParams` gets the animation inputs (`AnimInputs`: which shapes animate); the re-emit rule for slot values the painter does not apply
  (`fillColor`, `lineColor`, `textColor`, `fontScale`) calls `emit` for that shape with overrides (`BodyOverrides` of the TS: `fill`, `line`, `frame`, `unshape`, `chartParts`). The static emitter already keeps `base_rot`, the shape's box and
  `Fit` as local values at the places a slot needs them. A tile pattern under an animated scale is already right (the painter evaluates it).
- **B7** (viewer `engine="rust"`): per frame `emit_slide` with the shell's `MeasureTable` (kept per font generation: `set_generation`), the encoder with one `SlidesEncoder` per (view, generation) and one `EmitCaches` per generation,
  `Emission::chart_frames` until `uo-charts` lands, decode in `paint-slides.ts` with `env.media/bitmap/light/patternRows`, `warp` in the shell. A resize re-emits (scale and origin are in the list).
- **uo-charts**: emit at `chart::paint_chart_slot` (it already has the box in px and the shape index); charts need `Cx` pub items (`cx.rs` is crate-private; `Cx` and its helpers can be exported when the crate arrives).

## B5b — metafiles (2026-11-17, Sonnet)

`metafile.ts` (1,574), `emfplus.ts` (1,041), `pict.ts` (533) and the dispatch of `raster.ts` (`rasterizeSpecialImage`) are ported 1:1 into `src/metafile/` (≈ 5,000
Rust lines: `mod.rs` API, detection and sizes; `rec.rs` the recording canvas and the wire form; `gdi.rs` pens, brushes, fonts, the clip stack, `paintPath`, DIB decode and
blit, the text of a metafile; `emf.rs` `playEmf`; `wmf.rs` `playWmf`; `emfplus.rs` the EMF+ renderer; `pict.rs` QuickDraw). The TIFF of the Slides picture is `uo-image`'s
`tiff_slides.rs` (`uo-image.md`); the EOT reader and MicroType Express are `uo-fonts`' `eot::read` + `mtx.rs` (`uo-fonts.md`). A metafile is NOT rasterized in Rust.

**The recipe (what the list carries).** The TS plays a metafile on a canvas it creates at the display size (`raster.ts`: scale ×2, `maxSide` 2,400, `minSide` 600, the
picture's size on the slide for a `<p:pic>`) and uses the canvas as an ordinary image. Rust has no canvas and Chromium's anti-aliasing cannot be matched by a rasterizer
of ours, so the producer RECORDS the canvas calls (`rec::Rec`: one `Op` per call, the canvas it went to, the numbers the TS computed) and the shell replays them
(`uo-wasm/js/meta-player.ts`, `replayRecipe`) on a canvas of the recipe's size: that canvas is the picture. The recipe is a **bitmap of the resources whose bytes are the ops**
(`SlidesResources::recipe_bitmap`, `is_recipe`: no schema change, the `bitmaps` table and `Bitmap { w, h }` are the canvas size; `.uolf` frames flag them in bit 31 of the
length), so `ImageSrc::Bitmap(id)` is the picture and the painter (`paint-slides.ts` `bitmapCanvas`) asks the env: `recipe(index)`, `decodeBlob(data)` (EMF+ PNG/JPEG,
prepared by the shell), `metaCanvas` (the factory of the replays; the parity harness gives them a recorder of their own, as the TS rasterized them outside the slide's
stream). A TIFF is a recipe of one `putImageData`. The wire form: `"UOMF"`, version 1, layer size, a truncated flag, the blobs (decoded DIBs, compressed bitmaps), the ops.
Only the canvas state the TS reads back is tracked in Rust (`lineWidth` for the dash unit, `globalAlpha *=`, `ctx.font` for `measureText`), with the canvas rules (an
invalid `lineWidth`/`globalAlpha` is ignored and not in the stream; a negative radius of `arc`/`ellipse` throws). `xorRect` (the exact PATINVERT on the pixels:
`getTransform`, `canvas.width`, `getImageData`) is the op `XorRect { .. skip }`: the player runs the TS function (`slides-effects.ts`) and, when it answers `true`, skips
the `skip` ops that follow, the approximation the TS draws when it answers `false` — so the `MetafileRop` effect of the schema stays declared and unused.

**The emitter hook (`emit/cx.rs`, `emit/shape.rs`).** `Cx::special_img(media)`: a media whose bytes are a metafile, a PICT or a TIFF resolves to the raster of the picture
(`image_quiet` and `image_plain` try it first; the natural size is the canvas size, as `imgW(canvas)`). A `<p:pic>` sets `Cx::picture_display` (`picDisplaySize`: `ext / 9525 /
(1 − crop)`) around its `image` call; the raster is cached per media in `EmitCaches` (256 pictures / 64 MiB, `overflowed` past it): **the display of the first use decides**,
as the viewer's `imageCache` keeps one canvas per part. The measure protocol is the emitter's: the player asks `measureText` under RAW CSS font strings and the decoded state of
compressed EMF+ bitmaps; an unknown answer is a `Need` (collected for the whole pass, the pass's output discarded), mapped to two new requests of `text/measure.rs`: A13
`MetaText { font, text }` (`MeasureTable::{set,meta}_text`: width and `fontBoundingBoxAscent/Descent`) and A14 `MetaDecode { key, data }` (`set_meta_decoded`: the natural size, or `None`).
`font_avail` (the TS `fontAvail`, module state) is a memo per rasterization: the probe is a state-only sequence of canvas calls (`save`, `font`, `restore`) without a pixel,
so re-emitting it changes nothing the shell draws.

**Parity** (exact, line by line over the whole canvas text: state sets, offscreen canvases `@cvK`, device-space geometry, resolved paints; the `measure` probes are Rust's host
calls and are compared as measure answers):

| Capture | Cases | Equal | Different |
|---|---|---|---|
| `meta-b5b` (873 synthetic files, 1,267 rasterizations, 399 k lines of the TS; ops 3.0 MB gz, inputs 0.8 MB) | EMF 240 + mutated 80, EMF+ random 140 + 46, EMF+ focused 32 + 10, WMF 180 + 60, PICT 70 + 23, TIFF 130 + 43 (RGBA digest), EOT 160 + 53 (sfnt digest) | **1,267** | 0 |
| `drawops-b5b` (40 generated decks, 162 slide views; `capture-drawops --gen b5b --special`) | the draw-op stream of a slide whose pictures are the rasterized canvases (identity: `canvas(WxH:recipe hash)`) | **162** | 0 |

The inputs are generated in code (`js/lib/gen/meta-*.mjs`: EMF with every record the TS handles, EMF+ objects (brushes, pens with custom caps, paths, regions, raw and compressed
bitmaps, nested metafiles, continued objects) and records, WMF, PICT v1/v2 with every bitmap packing, TIFF of every compression (the LZW and PackBits encoders are in the
generator), EOT with MTX (an LZCOMP encoder with the adaptive Huffman, LZ77 copies, DUP symbols and RLE, and the CTF encoding of glyphs); no font, no proprietary byte) and damaged copies
(cut, bit flips). V8 coverage of the TS over them (`js/coverage-meta.mjs`): `metafile.js` 1,453 / 1,592 code lines (the rest: `recolorMetafile`, 160 lines ported in B2, and the async
`prepare`), `emfplus.js` 1,073 / 1,129, `pict.js` 678 / 715 (`fitPictFrame` is B2's), `tiff.js` 179 / 184, `eot.js` 519 / 526 (four `throw` guards).
Tools: `capture-meta.mjs` (TS, 5.5 s), `compare-meta.mjs` (Rust recipes replayed over the recorder, 0.3 s; the Rust side is `examples/meta_recipe.rs`, 0.09 s for all of
them), `diff-meta.mjs` (first difference of one file), `coverage-meta.mjs`. Not compared with the TS: an EMF+ compressed bitmap the shell decodes (Node has no
`createImageBitmap`: the capture covers the not-decoded path, i.e. the GDI fallback of a dual metafile); `a_decoded_bitmap_is_drawn_from_its_blob` pins the Ready path.

### Decisions outside the source semantics (declared)

- **B5b-1. A recipe, not a bitmap or a display sub-list** (the plan's B5b row and the B4 section said a `Bitmap`): the pixels are the shell's (see above). The B4 `MetafileRop`
  effect is unused. A metafile picture's colour effects (`a:duotone`, grayscale, `clrChange`, brightness): **not applied** — the list's `Derived` has a media base, and the picture is a bitmap
  (gap B5b-G1; the TS applies `fxImage` to the canvas). `alpha` (`GlobalAlpha`) works.
- **B5b-2. One raster per media**, the display of the first use (as the viewer's cache per part); a picture shape and a fill of the same part share it.
- **B5b-3. Throws.** A `DataView` read past the end is `Throw`: the play stops, what was drawn stays, the `finally` restores run, as `rasterizeMetafile`'s `catch`. `push(...array)` of more
  than `SPREAD_LIMIT` = 120,000 elements throws (V8's stack limit, whose exact value depends on the depth: the TS fails at about that size; EMF `path.push(...cmds)`, polypolygon
  and EMF+ custom caps).
- **B5b-4. Caps (M5)**, none in the TS (each makes a hostile file bounded, none changes a real one): ops of one recipe 600,000 (`truncated` flag), blob bytes 64 MiB, `MAX_ELEMS` 65,536 for
  the arrays of gradients/dashes read by count, EMF+ path points 4 M per rasterization (`PATH_WORK`), pixels of a decoded DIB / EMF+ bitmap / PICT bitmap / TIFF 16 Mpx **and** at most 128
  pixels per byte of data (an uncompressed row is ≥ 1/8 byte a pixel, PackBits/RLE ≤ 128; TIFF 16,384 pixels a byte), an EMF+ bitmap stride smaller than a row, needs 4,096, saved EMF+ states
  4,096, PICT font ids 4,096, TIFF tag values 4 M, a loop whose bound is a count of the file stops at the length of the data. `rasterizeMetafile`'s canvas: ≤ 2,400 px a side (the TS's own rule); a
  non-finite display is no picture.
- **B5b-5. Strings are UTF-16** (`JsStr`) end to end, so a lone surrogate from an EMF text record survives to `fillText`/`ctx.font` as in the TS; the measure table's keys are lossy
  (U+FFFD, decision B3-3), which the stub (and a canvas) measures like the surrogate.
- **B5b-6. Sorting** uses `uo_jsnum::sort::js_sort_order` (V8's decisions, no Rust `sort_by` with an inconsistent comparator, which can panic); `Math.*` are `uo_jsnum::math`; `toFixed`/`String(n)` are `uo_jsnum::format`.
- **B5b-7.** The module is `#![allow(cast_*, neg_cmp_op_on_partial_ord)]`: the offsets, counts and sizes are JS numbers whose float-to-integer `as` saturates, every use is bounded by the length of the data;
  `!(a > b)` keeps the JS NaN comparison.

### Memory and robustness

**M1** `m1_rasterization_leaves_nothing_behind` (metafiles, PICT, TIFF over a sample of the set at a display size, and the EOT reads; live bytes exactly back after the outputs are dropped), **M2**
`m2_a_soak_of_rasterizations_is_flat` (60 rasterizations, tolerance 0), **M5** the caps above: `m5_a_record_bomb_stops_at_the_op_cap` (300,000 records: truncated, bounded),
`m5_the_canvas_size_is_capped` (huge/NaN/negative displays), `hostile_declared_sizes_are_refused_or_bounded` (a 60,000² TIFF, a 16,384² PICT bitmap, a 16 MiB MTX block from 40 bytes). The raster cache of
the emitter has its own cap (256 pictures / 64 MiB, `EmitCaches::overflowed`); the TS caches have none (`fontAvail` is per rasterization here, `decoded` is the shell's, capped there at 256 as the TS). Bounded no-panic:
`damaged_files_never_panic_and_stay_bounded` (1,500 cut / flipped / hostile-field files of the set, `UO_DML_LONG=1` 20,000; slowest < 10 s asserted, measured < 0.2 s). Fuzz targets `emf_play` (EMF, WMF, EMF+ with a
valid header or raw, optional display), `pict_play`, `tiff_slides`, `eot_read` (`core/fuzz/src/metafile.rs`; seeds are the capture inputs; invariants: the recipe is `UOMF` with a canvas of 1..2,401 px, two passes are equal, an answering host
is never asked, an MTX result is an sfnt whose directory fits): stable driver 40 s each, **0 crashes** (`emf_play` 32,531 executions at 811/s, slowest 150 ms after the caps above — the first run found a 1.4 s input: a 20-byte
record claiming 2 M path points, now bounded by `PATH_WORK`; `pict_play` 5.9 M, `tiff_slides` 5.3 M, `eot_read` 13.0 M).

## Process metrics

| Stage | Agent | Time | Notes |
|---|---|---|---|
| Architecture pass | Opus | one session (shared with `uo-pptx.md`) | boundary, measurement protocol, skeleton |
| B1a port (with B1b, one session) | Sonnet | see `uo-pptx.md` | 0 parity rounds; decision A1 (owned trees) and its M3 cost recorded above |
| B3 port + capture + generator group + tests | Opus | about 1 h 10 min wall (reading 15 min; grapheme generator and port 15 min; layout port 25 min; capture, b3a and replay 15 min; tests, fuzz, docs) | 0 port bugs found by the replay (100 % on the first run); 1 harness fix (A1 key without `fontKerning`); rework: the long-word loop rewritten without nested closures, a surrogate `char` literal, an M5 expectation; the concurrent B5a agent replaced the `diagram-metrics` port and broke the crate build for a while (clippy of this crate re-run after it) |
| B5a port + generator group + capture + tests | Sonnet | two sessions; port and hooks about 3 h, generator/capture/replay/tests/fuzz/docs about 1 h after the resume | 0 parity mismatches on the first replay; rework was clippy only (22 findings: negated float comparisons, identical branches, a too-complex type) |
| B4 port + schema fixes + interpreter edits + parity driver + tests + fuzz + docs | Sonnet | one long session (about 190 tool calls). Wall times on this machine (4 jobs): `uo-dml-render`+`uo-pptx` release build about 20 s; emit of the 53 samples (1,202 views) 1.5 s, of the 100 b1a decks 3.0 s (one process per deck); the compare of either capture 1.7 s; `capture-drawops --samples` 12.7 s; `uo-pptx/tests/emit.rs` 2.4 s, `uo-dml-render/tests/emit.rs` 0.2 s; fuzz `slides_emit` stable driver 166,352 executions in 40 s (4,159/s, slowest 16.6 ms, 0 crashes) | parity 1,166/1,202 + 246/246 on the first full run after the recorder-hash and `Prepare` fixes; rework: 4 (the recorder canvas hash, the lazily built derived image, discarded passes polluting the media table, one-request-per-round tables); the fuzz run found two hangs (an infinite column span in `needOf`, an unbounded arc subdivision) and a NaN false positive in its own determinism check |
| B5b port (metafiles, EMF+, PICT) + TIFF variant + EOT/MTX reader + generators (EMF/EMF+/WMF/PICT/TIFF/EOT, the LZW and LZCOMP encoders) + recipe player + captures + tests + fuzz + docs | Sonnet | one long session (about 330 tool calls). Wall times on this machine (4 jobs): `capture-meta.mjs` over 873 files / 1,267 rasterizations 5.5 s (399 k lines), the Rust side of the whole set 0.09 s (process included 0.5 s), `compare-meta.mjs` replay and compare 0.3 s, `capture-drawops --gen b5b --special` 4.8 s (162 views), `emit-uolf` of the 40 decks 1.1 s, `tests/metafile.rs` 1.2 s; fuzz 40 s a target | parity 1,267 / 1,267 and 162 / 162 after the first complete run with the player fixed; rework: 7 (the replay's `skip` of the XOR fallback read `r.p += r.u32()` before the `u32`; the Mac Roman table lost U+F8FF; V8's spread limit (a 120 k array of subpaths throws in the TS) found by the focused EMF+ group; the TIFF as a recipe instead of RGBA so the page stream does not contain the canvas lines; the per-media raster key; five M5 caps from the fuzz run's slow inputs; clippy casts) |
