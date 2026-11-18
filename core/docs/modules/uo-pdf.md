# uo-pdf — the PDF writer and the display-list PDF painter (viewer, editor and server PDF)

Record of the rust-port workflow. Status: **milestone-1 gate approved 2026-11-03** (hybrid
mode, `SKILL.md` "Hybrid mode"). Planner and reuse analysis together (one Opus agent, 2026-11-03),
with the uo-render plan (`uo-render.md`, RQ1–RQ13) and after the uo-docx-layout architecture (K1–K20,
LQ1–LQ22; this record decides its **LQ8(e)**). No separate analysis skeptic under the hybrid mode:
the milestone-1 skeptic and security review cover it. Milestone 1 ends here: uo-docx D1a →
uo-docx-layout → uo-render → **uo-pdf** (viewer/editor PDF export and the clickCompliance server PDF).

Line counts are non-test source lines on `main` 0.9.x (frozen), counted in this worktree. Line
numbers are of `write/packages/docx-layout/src` unless a path is given.

## Scope

### TS covered (behaviour source)

| TS | Lines | Rust (`uo-pdf` unless noted) |
|---|---|---|
| `pdf-writer/bytes.ts` (`ByteBuilder`, `utf16beHex`), `writer.ts` (`PdfWriter`: alloc/define, `addStream` with the `> 64` bytes and "only if smaller" deflate rule, xref, trailer) | 143 | `writer` |
| `pdf-writer/document.ts`: `PdfLink`, `PdfOutlineItem`, `safePdfUri`, `rgb255`, `hexColor`, `fmt`, `PdfPage` (`withAlpha`, `fillRect`, `line`, `glyphs` both modes, `path`, `fillShaded`, clips, transforms, images), `PdfDocument` (`embedFont`, `embedJpeg`, `embedRgba`, `addShading`, `addPage`, `addOutline`, `build`, `writeOutline`) | 515 | `document`, `page`, `num` |
| `pdf-writer/font.ts` (`EmbeddedFont`: Type0/CIDFontType2 Identity-H, `/W`, ToUnicode, the Symbol/Wingdings ToUnicode tables), `index.ts`, `subset.ts` (re-export) | 182 | `font` (subsetting is `uo_fonts::subset_true_type`) |
| `pdf.ts` (`renderPdf`, `fontFor` with the `#pua` copy, `imageFor`, `rgbaOf`, `pictureEnv`, `bgImage`, `outlineInto`, `runPathCmds`, `paintGlyphsFxPdf`, `glyphsAsPath`, `paintItem`, `rotatePath`/`linePath` → uo-render `geom`, `paintFillPdf`) | 540 | `render` |
| `pdf-picture.ts` (`paintShapeEffectsPdf`, `paintPicturePdf`) | 326 | `render::picture` (shared helpers from `uo_render::paint::picture`, uo-render RQ10) |
| `pdf-word-spaces.ts` (`withWordSpaces`) + `typography/font/sfnt.ts` `advanceTwips` with the advance table (386–403) | 41 + 18 | `word_spaces` (LQ8(e), PQ1) |

Total ≈ **1,765 lines** (the uo-docx estimate table said 1,790).

### C# (type guide; what exists only in C# moves or is dropped by name)

`UniversalOffice.Core/Pdf/**` (1,882 lines) and `UniversalOffice.Docx/DocxLayout/{Pdf,PdfPicture,
PdfNavigation,PdfWordSpaces}.cs` (1,192) are ports of the TS. Only in C#:

| C# | Fate |
|---|---|
| parallel `Build` (`Parallel.For` over font subset + deflate and content deflate, `PreparedStream`, `PrepareRgba`, `AddPage(PdfPage)` for pages drawn outside the document; Core/Pdf/PdfDocument.cs ≈ 480–600) | not in milestone 1 (results identical, numbering deterministic); a native-only stage-5 candidate (PQ10) |
| `ZLibStream(CompressionLevel.Fastest)` (Core/Pdf/PdfWriter.cs:140) | replaced by uo-opc deflate (G2): the server PDFs change from C# deflate to miniz_oxide (decompressed bytes equal) |
| `PdfRenderOptions.DecodeImage` host callback (TS has the option too; no host passes it: editor 22401, viewer 1349) | **dropped** (PQ6) |
| `PdfFontSource` + `DocxPdf.FixedFontProvider` (the v1 "fonts for every family" API, Export/DocxPdf.cs) | dropped or wrapped by the facade (layout LQ8(d)) |
| `Raster.cs` (`DecodePng`, `JpegSize`; port of slides `export/raster.ts`, used by the converter's xlsx/pptx PDFs) | **uo-image** (PQ3) |
| `SubstituteFontProvider` + `SystemFontProvider`/`SystemFontIndex` (the clickCompliance font policy) | uo-fonts `registry` policy `server-substitute` + `uo_fonts::system` (layout frontier P, LQ8(a, c)); used by the server path here (§9) |
| `TtfProgram` (converter PDF font program) | `uo_fonts::FontProgram` (done) |
| `PdfParityTests` (structural: page count, MediaBox, `Tj` count, image count) | superseded by PP0/PP1 (§6), much stricter |

### Left for other modules

| Part | Goes to |
|---|---|
| `pdf-navigation.ts` (links from `runBoxes`, outline from `documentOutline`) | uo-docx-layout F8 → a `uo_render::Navigation` value in points; uo-pdf only writes it |
| item types, shape geometry evaluation, fills, page-fill tiles, text-fx layers, picture helpers | uo-render (`types`, `geom`, `paint`) |
| image codecs (`images.ts` `decodePng/Gif/Bmp/Tiff`, `jpegInfo`, `imagePixelSize`; C# `Raster.cs`) | **uo-image** (uo-drawingml G5) — planned in brief here (§10, PQ3) |
| blip/picture pixel effects (`applyImageEffects`, `applyPictureAdjust`, `effectDecode`) | uo-drawingml `blip` (done) |
| subsetting, faces, outlines, COLR, symbol-font test | uo-fonts (done) |
| zlib deflate | uo-opc `flate` (done; PQ5 asks a reusable compressor) |
| the converter's PDFs (`converter/.../pptx/pdf.ts` 533, `xlsx/pdf.ts` 303, C# `PptxPdf`/`XlsxPdf` ×2) and its stale `pdf-writer` copy (842) and v1 `docx/pdf.ts` (74) | Slides/Sheet ports: they become uo-render producers or direct users of this crate's writer API; the stale copies are replaced, not ported (G7-style) |
| reading PDFs (`converter/.../pdf/markdown.ts` over pdf.js, C# `PdfMarkdown.cs` over PdfPig) | the converter plan (input side, not a writer) |

## Plan (milestone-1 gate — pending)

### 1. Boundaries and reuse

```
uo-jsnum ── uo-opc (flate) ── uo-image (codecs) ──┐
uo-fonts (Face, subset, symbol test, AdvanceTable) ┼── uo-pdf ◄── uo-render (Page, Resources, Navigation, geom, paint)
uo-drawingml (masks, blip effects) ────────────────┘     ▲
                    bindings: uo-wasm (viewer/editor export), uo-ffi + C# facade (server PDF)
```

- **Independent of uo-docx and uo-docx-layout** (layout verdict, K12/K14): the input is
  `&[uo_render::Page]` + `&uo_render::Resources` (faces with `Arc<Face>`, key, family, flags and the
  optional advance table; images by `ImageId` with bytes, ext, `ImageEffects`; text effects; page
  fills; custom paths) + `Option<&Navigation>`. Everything Word-specific was decided by the producer.
- **Reuse**: the writer (`PdfDocument`/`PdfPage`) is app-neutral and public, with both glyph modes
  (Write's `runs: true` corridas; the converter's per-glyph `Tm … Tj`), because Slides and Sheet
  export through it later; the painter (`render_pdf`) is generic over uo-render pages, so a Slides
  page painted by uo-render goes through the same code.

### 2. Data model and ownership

```rust
// writer — app-neutral, mirrors pdf-writer/*
pub struct PdfWriter<S: PdfSink> { sink: S, next: u32, written: u32, offsets: Vec<u64>,
    pending: BTreeMap<u32, Vec<u8>>, compress: bool, deflater: uo_opc::flate::ZlibCompressor /* PQ5 */ }
pub struct PdfDocument<S: PdfSink> { w: PdfWriter<S>, fonts: Vec<EmbeddedFont>, images: Vec<PdfImage>,
    shadings: Vec<PdfShading>, alpha_states: IndexMap<(Box<str>, Box<str>), u32> /* "fmt(ca)|fmt(CA)" */,
    pages: Vec<PageRec>, outline: Vec<OutlineEntry> }
pub struct PageRec { width_pt: f64, height_pt: f64, content: StreamBody /* compressed at page end, PQ4 */, links: Vec<PdfLink> }
pub struct PageWriter<'d, S> { doc: &'d mut PdfDocument<S>, page: u32, content: Vec<u8> }  // TS PdfPage; `with_alpha(fa, sa, |p| …)`
pub struct EmbeddedFont { index: u32, base_name: Box<str>, face: Arc<uo_fonts::Face>, used: FxHashMap<u16, u32> /* gid → first cp */,
    symbol: Option<&'static [(u8, u32)]>, scale: f64 /* 1000 / upem */ }
// painter — one call
struct RenderState<'r> { pages: &'r [Page], res: &'r Resources,
    fonts: IndexMap<FontKey /* face key, pua */, Option<u32>>, images: FxHashMap<ImageId, Option<PdfImage>>,
    adjusted: FxHashMap<(ImageId, JsonKey /* [alpha, duotone, clrChange] */), Option<PdfImage>>,
    bg_images: IndexMap<Box<str> /* the TS JSON key */, PdfImage>, spaces: WordSpaceCursor }
```

- **Memory scopes**: everything above lives for one `render_pdf` call; nothing crosses calls
  (`clear_caches()` no-op). Faces and image bytes are borrowed through the `Arc`s of `Resources`.
- **Keys follow the TS** (they decide object numbering, so they are parity-relevant): fonts by
  `face.key` (+ `#pua`, pdf.ts:39), images by **`ImageId` = TS object identity** (pdf.ts:33
  `WeakMap`; uo-render RQ3), adjusted pictures by (image, `JSON.stringify([alpha, duotone,
  clrChange])`, 119–121, a JSON key with the JS rules), page-fill tiles by their JSON/string key
  (523, 528), ExtGStates by `fmt(ca)|fmt(CA)` (document.ts:416).
- **Object numbering = the TS allocation order**, kept exactly: paint-time objects (SMask before its
  image, shading function before its dict) in paint order; at `build`, fonts (FontFile2, descriptor,
  CIDFont, ToUnicode, Type0, per font in creation order), catalog, pages node, all page objects, then
  per page its content stream and link annotations, then the outline root and items.

### 3. Public API

```rust
pub struct PdfOptions { pub compress: bool /* true */, pub limits: PdfLimits }
pub fn render_pdf(pages: &[Page], res: &Resources, nav: Option<&Navigation>, o: &PdfOptions) -> Result<Vec<u8>, PdfError>;
pub fn render_pdf_to<S: PdfSink>(pages: &[Page], res: &Resources, nav: Option<&Navigation>, o: &PdfOptions, sink: S) -> Result<S, PdfError>;
pub trait PdfSink { fn write(&mut self, bytes: &[u8]) -> Result<(), PdfError>; }   // Vec<u8>; chunked (wasm); Write (native)
pub enum PdfError { Limit(LimitKind), Sink }
// the app-neutral writer for Sheet/Slides (and tests): PdfDocument::new(sink, compress), embed_font, embed_jpeg,
// embed_rgba, add_shading, add_page -> PageWriter, add_outline, build
```

- **Changed from 0.9** (CHANGELOG): `renderPdf(result, {compress, decodeImage, navigation})` becomes
  `render_pdf(pages, resources, navigation, options)`; `decodeImage` is gone (PQ6); faces and
  images are ids; output can stream to a sink.
- **What crosses**: wasm — one export per document view, `export_pdf(view) → bytes` (the binding
  calls `Layout::navigation` then `render_pdf_to` with a chunked sink; JS assembles a `Blob` from
  ≥ 1 MiB chunks, PQ4); .NET — `docx_to_pdf(bytes, options) → bytes` through uo-ffi (§9).

### 4. Content streams and numbers

- **Numbers** go through uo-jsnum only: `fmt(n)` = `if NaN → "0"; r = round(max(-1e9, min(1e9, n))·1000)/1000;
  -0 → "0"; else to_js_string(r)` (document.ts:86–92; never an exponent: |r| ≥ 0.001 or 0, ≤ 1e9);
  `pdfSize = round(sizePt/0.12)·0.12` (pdf.ts:315); TJ adjustments `round(((pen − dx/sx)·1000)/size)`
  with the pen advanced by `widthThousandths` (document.ts:177–196); `gid.toString(16).padStart(4, '0')`
  (`to_radix_string`); integers through `to_js_string` (`String(-0)` is `"0"`); xref offsets
  `padStart(10, '0')`.
- **Colours**: `hexColor` with JS `parseInt(…, 16)` over UTF-16 code-unit pairs after the first
  `#` (`'auto'` → `[10, NaN, NaN]`), per the uo-render colour table (RQ6): where the TS PDF tests
  `HEX6` the Rust does, where it does not, neither does Rust.
- **Strings**: `safePdfUri` (trim with JS `\s` over UTF-16 — uo-text; `^(https?://|mailto:)` case-
  insensitive by a hand matcher; **UTF-8 by `TextEncoder`**, which turns lone surrogates into
  `EF BF BD`; `%XX` for bytes ≤ 0x20, ≥ 0x7F, `"`, `<`, `>`; `\` before `(`, `)`, `\`); outline
  titles `<FEFF…>` from UTF-16 code units, upper case; base font names
  `family.replace(/\s+/g, '') + -Bold/-Italic`, then `psName` keeps `[A-Za-z0-9+-]` (no injection
  into names or dictionaries: every dictionary is built from numbers, generated names and these
  sanitized strings).
- **Glyphs**: `fontFor` (`face.key`, symbol fallback `glyphId(cp − 0xF000)` for symbol fonts,
  pdf.ts:50–53), the `#pua` copy for F0xx codes missing from the cmap (320–328), `gid` glyphs dropped
  from text (322), word spaces (§5), corridas (`Tm` per same-baseline increasing-x run + `TJ`),
  `asPath` and COLR runs as outlines (Q → C at 2/3, skew 0.2, pdf.ts:251–295), text effects
  (`paintGlyphsFxPdf`: raster layer at 200 dpi from `uo_render::paint::fx_layers`, gradient fill
  as clip + shading, outline stroked), `withAlpha` wrapping.
- **Shapes and pictures**: geometry from `uo_render::geom` **in points** (uo-render RQ1), shadows,
  pattern/blip rasters clipped by the path, rectangular gradients as rasters, axial/radial shadings
  (function type 0, 512/256 samples), shape and picture effects as RGBA layers (`pdf-picture.ts`).

### 5. Fonts: embedding, subsetting, advance tables (LQ8(e) decided)

- `EmbeddedFont` writes the TS object graph: FontFile2 = `uo_fonts::subset_true_type(bytes, gids
  sorted ascending, keep_layout = false)` (a failure gives the whole font back, as the TS catch),
  `/Length1` = subset length, descriptor with `/Flags 32`, bbox `[-200 desc 1200 asc]`, `/W` with
  `round(adv·1000/upem)`, ToUnicode in chunks of 100 with the Symbol/Wingdings tables (font.ts:152–161;
  name test by hand matchers for `^[A-Z]{6}\+`, `[\s,]+`, `^Symbol(MT)?(-|$)`/i, `^Wingdings(-|$)`/i).
- **LQ8(e) — advance tables: keep, per face, for the PDF only (PQ1).** `SfntFont.setAdvanceTable`
  (sfnt.ts:386–403) never reaches the layout's width model (para.ts:378–405); its only engine reader
  is the PDF word-space insertion (pdf-word-spaces.ts:24 → `font.advanceTwips`). Who loads one: the
  viewer for its hosted Calibri/Aptos assets (viewer fonts.ts:64–67, `site/assets/fonts/*-advances.json`),
  the editor from `FontAsset.advances` (plugins.ts:9, universal-write.ts:1822); C# never does
  (`SfntFont.SetAdvanceTable` has no caller), so the server PDF has none. Rust:
  `uo_fonts::registry::AdvanceTable` (parsed once when the host pushes the face: size keys kept as the
  JSON strings and matched against `to_js_string(sizePt)`, character keys kept only when they are
  exactly one code point), attached to the face in the registry and carried in `FaceRes.advances`;
  `word_spaces::advance_twips(face, table, cp, size)` ports `SfntFont.advanceTwips` (table hit, else
  `advanceWidth·deviceEmPt·20/upem` with `deviceEmPt = round(size·600/72)·72/600`). Cost: a few KB per
  face. The alternative (drop the tables) changes only which spaces are written for text extraction
  in documents using those hosted faces — accepted by name if the maintainer prefers.

### 6. Parity

The TS PDF is deterministic (no dates, no `/ID`, fflate's deflate is deterministic), so the target is
strict.

| Capture | How | Compared |
|---|---|---|
| **PP0 isolated** (the uo-pdf gate before the layout is done) | the frozen TS engine lays out the corpus; its `LayoutResult` + `pdfNavigation` go through the uo-render TS adapter (uo-render R-P2c) with the face bytes, advance tables and image bytes dumped; Rust decodes → `render_pdf`; the TS runs `renderPdf` on the same `LayoutResult` | (a) `compress: false`: **byte-exact**; (b) `compress: true`: **structural** (below) |
| **PP1 end to end** (after layout P1) | Rust read → layout → `navigation` → PDF vs TS read → layout → `pdfNavigation` → `renderPdf`, same face transcript | structural, plus byte-exact with `compress: false` |
| facets | `fmt` (random f64 incl. NaN, ±0, ±∞, ties, 1e9 bounds), `hexColor`, `safePdfUri` (lone surrogates, controls), `utf16beHex`, `withWordSpaces` (with and without tables), `writeOutline` (level sequences), ToUnicode tables, `widthThousandths`, the TJ algorithm, `glyphsAsPath`, `drawImageTransformed`, `addShading`, `paintFillPdf` tiling, `pdf-picture` layers | exact |

- **Structural comparison** (own test-only parser, ≈ 300 lines; our PDFs are classic-xref, no object
  streams): same object count and numbering; per object the dictionary text equal after removing
  `/Filter /FlateDecode` and `/Length n`; stream payloads equal after inflating FlateDecode (deflate
  parity on decompressed bytes, uo-opc G2); trailer `/Size`, `/Root` equal; xref and `startxref`
  validated against the Rust bytes (each offset hits `N 0 obj`). The "compress only if smaller" rule
  may flip for a stream near 64 bytes or incompressible: the comparator treats stored and deflated
  alike, and the run counts flips.
- **Validity**: every CI output passes `qpdf --check` (a CI tool, not a dependency); pdf.js text
  extraction of TS and Rust PDFs equal on a sample.
- **Corpus**: the 2,742 oracle docx locally with Office fonts (PDFs embed subsets of proprietary
  fonts, so they never enter the repository — CLAUDE.md; the TS capture is regenerated in the gate
  run, LQ21); the 7 .NET synthetic inputs (pictures, page, page-pattern, page-frame, page-tile,
  textboxes, texteffects); 2,000 generated documents with open fonts in CI; stress: 500 pages,
  100 × 12 Mpx PNGs, 50 embedded fonts, 10,000 links, a 9-level 5,000-entry outline.
- **Migration report (not a gate)**: clickCompliance receives C#-generated PDFs today; a structural
  C# vs Rust diff over the synthetic inputs and a sample of generated documents lists where the
  server output changes (C# Fastest deflate, any C#-only divergence), for the release notes.

### 7. Memory and streaming (measured gate)

- **Streaming in TS order** (PQ4): objects are emitted to the sink as soon as every lower number is
  written, so paint-time objects (images, SMasks, shadings) leave memory as they are made; a page's
  content stream is compressed when the page ends and held **compressed** until `build` (its object
  number comes after the fonts and the page objects); fonts are subset at `build` (they need every
  used gid). Peak ≈ Σ compressed page contents + the largest image working set (RGBA 4N + RGB 3N +
  alpha N + its deflate) + the build-phase dictionaries; the TS holds every body plus the final
  concatenation (≈ 2 × output). With the chunked sink the wasm high-water no longer grows with the
  output size (wasm memory never shrinks).
- **Compressor reuse** (PQ5): one zlib compressor per render, reset per stream (uo-opc measured
  ≈ 254 KB of miniz_oxide state per deflated entry).
- **M1**: `render_pdf` (ok; each `Limit`), every `PageWriter`/`PdfDocument` method, `embed_jpeg`/
  `embed_rgba` with and without alpha, `build` with fonts.
- **M2 soak**: 1,000 × `render_pdf` over 20 PP0 captures incl. the 500-page and image stress, sampled
  every 50; flat, **0 B** after the call; wasm `memory.buffer` flat after 200 with the chunked sink.
- **M3 budgets** (first numbers, replaced by measurements): peak ≤ Σ compressed contents + 9N of the
  largest image + 4 MiB; allocated ≤ 3 × output + 9 × Σ decoded image pixels; vs the TS heap peak of
  the same render (Node `--expose-gc`); phone: a 100-page text document ≤ 32 MiB wasm high-water.
- **M4**: the output buffer crossing to JS (10,000 × create/free and the finalizer path) and to .NET
  (copied into `byte[]`, native freed; LeakSanitizer on Linux CI).
- **M5**: no cross-call cache; per-call maps bounded by the input (declared, covered by M1).
- **M6**: no `unsafe`.

### 8. Security plan

Inputs from untrusted documents: coordinates (NaN/±∞/1e300: `fmt` clamps), item counts, glyph
counts, font bytes (uo-fonts limits; subset failure → whole font), image bytes (uo-image limits:
dimensions, inflate bombs through uo-opc limits, GIF/TIFF LZW expected sizes), URIs, outline titles
and levels, page-fill tile sizes.

| Risk (TS behaviour) | `PdfLimits` | Outcome |
|---|---|---|
| page-fill tiling `for (y < H; y += th)` with a tiny tile (pdf.ts:538): millions of `Do` (TS hangs or exhausts memory) | `max_ops_per_page` (e.g. 2·10⁶) | `Err(Limit(Ops))` |
| huge decoded images (12 Mpx → ≈ 100 MB working set) | uo-image `max_image_px` (e.g. 1.2·10⁸) | image skipped as the TS `catch` does (pdf.ts:87) + warning |
| huge content per page | `max_content_bytes` per page (e.g. 256 MiB) | `Err(Limit(Content))` |
| output beyond the wasm32 address space | `max_output_bytes` (desktop 1 GiB, phone 256 MiB; native unlimited) | `Err(Limit(Output))` |
| blip tiles | TS cap 4,000 (geometry.ts:325), kept | — |
| URI schemes (`javascript:`, `file:`) | `safePdfUri` whitelist (http, https, mailto) | link dropped, as the TS |
| panics (`panic = "abort"` kills every document in the instance) | `deny(clippy::indexing_slicing)`, checked arithmetic on offsets and counts | — |

**Fuzz targets**: `pdf_render` (structure-aware random pages + resources: the output parses with
the test parser and its xref is valid), `pdf_fmt` (differential against a JS-generated table),
`pdf_uri`, `pdf_image` (hostile image bytes through `embed_*`), `pdf_font` (hostile font bytes through
embedding + subset at build), `pdf_outline` (random levels and titles).

### 9. The server PDF path (clickCompliance)

Today: `UniversalOfficeWordToPdfConversionService.ConvertToPdf` (clickCompliance
`src/clickCompliance.Infrastructure/Services/Word/UniversalOfficeWordToPdfConversionService.cs:29–40`)
calls `DocxLayoutPipeline.ToPdf(docx, SubstituteFontProvider.Shared, new LayoutOptions {
HyphenationLanguages = ["pt-BR", "en", "es"] })` under a `PdfConversionQueue` (concurrency
`max(1, cores/2)`, 30 s wait, `Task.Run`). The pipeline: `DocxPackage.Load` → `ReadFullDocument` →
embedded fonts (`WithEmbeddedFonts`, unreadable → none) → layout → `PdfRenderer.RenderPdf(layout,
null, PdfNavigation.From(full, layout))`.

After the switch:

```
C# facade  DocxLayoutPipeline.ToPdf(docx, FontPolicy.ServerSubstitute, options)  (shape kept; CHANGELOG for IFontProvider)
  └─ uo-ffi  uo_docx_to_pdf(bytes, len, &PdfJobOptions) -> OwnedBuffer      (SafeHandle; copied to byte[]; freed)
       └─ uo-docx read (D1a, DocxLimits) → embedded fonts → registry policy `server-substitute`
          over the process-wide `uo_fonts::system` index (Windows, Linux, macOS, EXTRA_FONT_DIRS)
          → uo-docx-layout (hyphenation_loaded from HyphenationLanguages ∩ the document walk, LQ16; now)
          → Layout::navigation → uo_pdf::render_pdf_to(Vec sink)
```

- **Font policy**: `server-substitute` reproduces `SubstituteFontProvider` (system face; else
  `FontCatalog.MetricSubstitute` — Calibri → Carlito, Cambria → Caladea, Arial/Helvetica →
  Liberation Sans, Times → Liberation Serif, Courier → Liberation Mono — as a **fresh stand-in answer
  per call**, `StandIn = true`, which matters for kerning across runs, K2a), with C# as its parity
  reference (layout frontier P). The PDF embeds by `face.key`, so a stand-in and the real face of the
  same file share one embedded font, as today.
- **Threads**: clickCompliance runs several conversions at once. The system index and the
  `FaceCache` are process-wide behind `RwLock`/`Mutex` (native only; wasm stays single-threaded);
  everything else is per call. M4 + LeakSanitizer cover the FFI; a soak runs 8 threads × 500
  conversions.
- **Time**: a hostile document can hang the C# engine forever inside the queue; Rust is bounded by
  the layout `max_work` and these limits. A cancellation flag checked by the work budget (PQ9) would
  let the 30 s request timeout stop a running conversion too.
- **No advance tables** on the server (as C#), no `decodeImage`.

### 10. uo-image (dependency, planned in brief here — PQ3)

uo-drawingml G5 created the crate without a plan; uo-pdf is its first consumer and it is on the
milestone-1 critical path (the PDF embeds PNG/GIF/BMP/TIFF as RGBA and JPEG directly). Scope:
`images.ts` `jpegInfo`, `imagePixelSize`, `decodePng` (on `uo_opc::flate` instead of fflate's
`unzlibSync`), `decodeGif` + `lzw`, `decodeBmp`, `tiffLzw`, `packBits`, `decodeTiff`, `decodeRaster`
(≈ 400 lines); C# `Raster.cs` is a separate older port (Adam7 → null) used by the converter's PDFs:
kept as a named variant only if the converter port needs it. `ImageLimits { max_px, max_dim }`;
fuzz `image_decode`; parity: TS outputs (RGBA bytes, sizes, `null`s) over every image of the corpus
plus generated hostile files. Sonnet, closed scope, ≈ 2–4 h incl. facets.

### 11. JS semantics and risks

`parseInt` (hex pairs, leading whitespace and sign accepted), `String(n)`/`toString(16)`/`padStart`
(uo-jsnum), `Math.round` ties toward +∞, `-0` (`fmt` and `String`), `TextEncoder` on lone surrogates,
`String.fromCodePoint` on lone surrogates in ToUnicode (kept as code units, uo-render RQ9), JS `\s`
in `replace`/`trim` (uo-text tables, layout LQ11), `JSON.stringify` keys of the picture and page-fill
caches (JS rules: `undefined` omitted, NaN → `null`, −0 → `0`; the uo-docx-layout `json_key` helper),
`Map` insertion order (fonts, ExtGStates: `IndexMap`), `WeakMap` identity (images: `ImageId`), the
stable sort of gids, `Uint8Array` stores of `Math.round` results (`uo_jsnum::conv`), the `try/catch`
around image embedding (pdf.ts:69–89: a failing decoder leaves the image out).

### 12. Frontiers, models and order

| # | Frontier | Content | Lines | Model | Depends on |
|---|---|---|---|---|---|
| I | uo-image | §10 | ≈ 400 | **Sonnet** | uo-opc |
| W | writer | `bytes`, `writer` (with sink, pending map, compressor), `document`, `page`, `num`, `font` | ≈ 840 | **Sonnet** | uo-fonts, uo-opc, uo-jsnum |
| H | harness | the PP0 capture script (TS adapter of uo-render C + dumps), byte-exact and structural comparators, the test parser, facets, `qpdf` job | new | **Sonnet** | uo-render C (TS adapter) |
| P | painter | `pdf.ts`, `pdf-picture.ts`, `pdf-word-spaces.ts` + `advance_twips` | ≈ 925 | **Sonnet** | W, I, uo-render T/G |
| S | server path | the `uo_docx_to_pdf` export, the C# facade call, the thread soak — designed in the bindings plan | — | FFI shape **Opus** (with uo-ffi), facade Sonnet | layout, P |
| — | security review, milestone skeptic | once for milestone 1 | — | **Opus** | — |

Order: **I ∥ W** (no dependency on the layout) → **H** (as soon as uo-render's TS adapter exists)
→ **P** → **PP0** on TS layouts (parallel with layout waves 1–3) → **PP1** after layout P1 → **S**.
Every Sonnet frontier runs with the parity and memory verifiers on top and compiles clippy-clean
before verification.

### 13. Estimate

| Part | Hours (agent time, through gate 2) | Measured or assumed |
|---|---|---|
| I uo-image port + facets + fuzz | 3–5 | assumed (≈ 400 lines) |
| W writer port (≈ 840) + facets | 4–6 | **measured basis**: uo-opc Opus ≈ 1,300 lines/h port-only; Sonnet 0 parity rounds in uo-fonts/uo-opc/uo-drawingml; assumed 400–600 lines/h + facets |
| P painter port (≈ 925) | 2–4 | same basis |
| H harness (capture, two comparators, test parser, `qpdf`, pdf.js sample) | 6–9 | assumed |
| parity rounds PP0 + PP1 | 4–8 | assumed; the PDF is deterministic and its inputs are already parity-tested (fonts, drawingml), so rounds should come from plan gaps |
| memory (sink, compressor reuse, M1–M5) | 3–5 | **measured basis**: uo-fonts stage 4 ≈ 160 min / 2.7k lines |
| security, fuzz | 3–5 | assumed |
| server path integration test (beyond the bindings plan) | 2–3 | assumed |
| **working band** | **≈ 27–45 h (midpoint ≈ 36)** | |

Critical path for PP0 ≈ 15–22 h (W → P → H → rounds), **independent of the layout port**; PP1 waits
on layout P1. Not included: uo-ffi/C# facade and uo-wasm plumbing (bindings plan), the Slides/Sheet
converter PDFs, stage-5 work (parallel build, content dedup), real-Safari runs, maintainer latency.

## Milestone-1 gate — questions (PQ1–PQ12)

★ = needed before its port starts.

| # | Question | Recommendation | When |
|---|---|---|---|
| **PQ1** ★ | LQ8(e): `SfntFont` advance tables | **Keep, for the PDF only**: `uo_fonts::registry::AdvanceTable` attached per face when the host pushes it, carried in `FaceRes.advances`, read only by `word_spaces::advance_twips`; server none (as C#). Alternative: drop, with the word-space insertion change accepted by name | ★ before P (and the uo-fonts registry, frontier P of the layout) |
| **PQ2** ★ | Parity method | Byte-exact with `compress: false`; structural with compression (inflated streams equal, dictionaries equal minus `/Length`/`/Filter`, same numbering, xref valid); PP0 on TS layouts through the uo-render adapter (independent of the layout port), PP1 end to end | ★ before H |
| **PQ3** ★ | uo-image has no plan (G5) and is on this critical path | Accept §10 as its plan (Sonnet, ≈ 400 lines, limits, fuzz, TS facets); the converter's `Raster.cs` variant only with the converter port | ★ before I |
| **PQ4** ★ | Streaming and object order | Keep the TS numbering **and** file order; emit objects eagerly in number order; hold page contents compressed until `build`; chunked sink for wasm. Out-of-order emission (contents written early) is a stage-5 option that keeps structure but not byte order | ★ before W |
| PQ5 | One compressor per render | Ask uo-opc for a reusable zlib compressor (`reset` between streams) — ≈ 254 KB of state per stream otherwise | W |
| PQ6 | Drop `decodeImage` (no host passes it) and the v1 `PdfFontSource` path | **Drop**, CHANGELOG (layout LQ8(d) for the fonts) | W |
| PQ7 | Public writer API for Sheet/Slides | Keep `PdfDocument`/`PageWriter` public with both glyph modes; the converter's stale `pdf-writer` copy and v1 `docx/pdf.ts` are replaced, not ported | W |
| PQ8 | Image identity in the PDF | `ImageId` (uo-render RQ3): one XObject per TS object identity; content dedup = stage 5 (changes results: smaller PDFs) | P |
| PQ9 | Server cancellation | Add a cancellation flag to the job options, checked by the layout work budget and per page here; the facade maps the request timeout to it | S |
| PQ10 | C#'s parallel build | Not in milestone 1; native-only stage-5 candidate measured at gate 3 against the clickCompliance queue (which already runs conversions in parallel) | gate 3 |
| PQ11 | Limits and outcomes (§8) | `PdfLimits` as listed; a limit → `Err(Limit)` where the TS hangs or exhausts memory; an undecodable or oversized image is skipped, as the TS `catch` | W/P |
| PQ12 | Server PDFs change from C# to TS semantics at the switch (deflate, any C#-only divergence) | Publish the migration report (§6) with the release; no gate | release |

## Requests to other records (not edited there)

- **uo-fonts**: `registry::AdvanceTable` and its attachment per pushed face (PQ1); the
  `server-substitute` policy's "fresh answer per call" identity (already in layout K6).
- **uo-opc**: a reusable zlib compressor (PQ5).
- **uo-render**: RQ1 (geometry in points for the PDF), RQ3 (image identity), RQ6 (colour table), the
  TS adapter (R-P2c) before H.
- **uo-docx-layout**: `Layout::navigation` returns `uo_render::Navigation` in points exactly as
  pdf-navigation.ts computes it (`b.x / T`, first line of the bookmarked block); `FaceRes.key` is the
  provider's `key`.
- **Bindings** (uo-wasm/uo-ffi plans): `export_pdf` with a chunked sink; `uo_docx_to_pdf` with
  owned output, thread-safe registry, cancellation flag (PQ9).

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

Pending. Candidates: image dedup by content (PQ8), parallel build on native (PQ10), out-of-order
streaming (PQ4), font subsetting during painting.

## Final review and benchmarks (gate 3)

Pending. `tools/bench`: PDF export of the oracle corpus and of the 500-page and image stress, main
(TS and C#) × rust-core, Node, WebKit and .NET (the clickCompliance shape, 1 and N threads).

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner + reuse, architecture plan for uo-render and uo-pdf together (Opus) | (orchestrator) | ≈ 60 |
| 3 | frontier W: writer port + PP0 differential + M1/streaming/no-panic tests + record (Sonnet, hybrid mode) | (not recorded; one session) | ≈ 125 (own tally) |

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (PQ1–PQ12). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.

## Milestone 1 — frontier W (the writer)

Hybrid mode, one Sonnet session, 2026-11-03. Scope: the app-neutral public writer of §3 — `PdfDocument`/`PageWriter`
with both glyph modes, the object numbering and file order of the TS, streaming emission to a sink, fonts through
uo-fonts, `PdfLimits`. The painter (`render_pdf`, frontier P) and uo-image (frontier I) are not here. Crate
`core/crates/uo-pdf` (no dependency on uo-render or uo-docx; it depends on uo-jsnum, uo-opc, uo-fonts and `rustc-hash`).

### Ported

| TS (`write/packages/docx-layout/src/pdf-writer/`) | Lines | Rust (`uo-pdf/src/`) | Lines |
|---|---|---|---|
| `bytes.ts` (`ByteBuilder` is a `Vec<u8>`; `utf16beHex`) | 51 | `num.rs` (`fmt`, `Display` helpers, `latin1`, `utf16be_hex`) | 115 |
| `writer.ts` (`PdfWriter`) | 92 | `writer.rs` (`PdfWriter<S>`, `StreamBody`), `sink.rs` | 310 + 81 |
| `document.ts` `PdfPage` | ≈ 190 | `page.rs` (`PageWriter`) | 403 |
| `document.ts` `PdfDocument`, `safePdfUri`, `rgb255`, `hexColor`, `fmt` and the types | ≈ 325 | `document.rs`, `uri.rs`, `color.rs`, `types.rs` | 471 + 39 + 26 + 158 |
| `font.ts` (`EmbeddedFont`, the Symbol/Wingdings tables) | 161 | `font.rs` (`FontProgram`, `EmbeddedFont`) | 338 |
| (limits, errors: new, PQ11) | — | `error.rs` | 88 |

840 TS lines → 2,077 Rust lines (doc comments, the `Display` helpers and the sink layer included). Also: `ZlibDeflater` in
`uo-opc/src/flate.rs` (PQ5, 30 lines + a test; every uo-opc test binary stays green).

**What is the TS, kept**: every dictionary and content string is built in the TS order with the TS spacing (a stream with an
empty dictionary is `<<  /Length n >>`, two spaces); numbers go through `fmt` (`Math.round` ties toward +Infinity, clamp to
±1e9, NaN and -0 as `0`) and the JS `String(n)` of uo-jsnum (`Js`); `Math.round/min/max/cos/sin` come from uo_jsnum; colours
are `parseInt(…, 16)` over UTF-16 code units; the TJ adjustments stay JS numbers and are written with `String(n)` (NaN,
Infinity and exponents come out as in the TS); `fontFor`'s `ps_name`, the `/^[A-Z]{6}\+/` strip and the Symbol/Wingdings test
are hand matchers with the JS `\s` and ASCII-only `i` semantics; the ToUnicode `String.fromCodePoint` throws (a code point past
U+10FFFF) become `PdfError::Invalid`.

**Numbering and order** (parity-relevant, PQ4): paint-time objects (soft mask before its image, shading function before its
dictionary) take the next number when made and go out at once; at `build` the fonts (FontFile2, descriptor, CIDFont, ToUnicode,
Type0 per font), then catalog, pages node, one number per page, then per page its content stream, its link annotations and the
page object, then the outline root and items. The catalog is defined last, so everything after it waits in memory — the page
contents, **compressed when their page ends** (the "more than 64 bytes and smaller" rule is decided there, with the writer's
single reusable compressor).

### Decisions outside the plan (each one declared)

1. **API shape.** `PageWriter<'d, S>` borrows the document and is pushed to it at `finish()` (a page dropped without `finish`
   is finished by `Drop`, an error is kept for `build`); resources created while a page is painted go through
   `page.document()` (the TS `embedJpeg` during `paintItem`), which keeps the TS object numbers. Fonts are `FontId` handles on
   documents that own the `EmbeddedFont`; `page.encode(font, cp)` is `font.encode`. `PdfImage`/`PdfShading` are `Copy` handles.
   `with_alpha` takes a closure over the page.
2. **`FontProgram`** is a trait with JS-number (`f64`) fields and optional `font_bbox`/`ascender`/`descender`/`italic_angle`
   (the TS interface); implemented for `uo_fonts::Face` (what `fontFor` hands over: hhea ascent/descent, no box or angle) and
   `uo_fonts::FontProgram` (the converter's). The painter implements it on its own wrapper for the symbol-font glyph fallback.
   `embed_font` takes `impl FontProgram + Send + 'static` and the font bytes as `Arc<[u8]>` (shared, never copied).
3. **Glyph ids are written with `{:04x}` of a `u32`**, not `to_radix_string` (no allocation per glyph): identical to
   `gid.toString(16).padStart(4, '0')` for every `u32` — tested against `to_radix_string` over 0..=0xFFFF and the edges.
4. **Arithmetic.** All geometry is `f64` with the JS operators in the TS order. Counters, indexes and sizes the TS holds as
   doubles are integers (`u32` image sizes, `i64` page indices, `u32` outline levels) where the TS only ever holds integers
   there; what a fraction or NaN there would write (`undefined 0 R`) is not supported. Rust `&str` has no lone surrogates: a
   UTF-16 host converts lossily first, which is exactly what the TS `TextEncoder` does to a URI (`EF BF BD`); an outline title
   with a lone surrogate would differ (the TS writes the unit, here U+FFFD).
5. **`PdfError::Invalid(&'static str)`** exists for the places the TS throws or is silent: an object left undefined at
   `build`, an object defined twice after it was written or before it was allocated (the TS overwrites or extends a sparse
   array), and the ToUnicode `RangeError`. `LimitKind::Objects` guards the `u32` object number.
6. **Limits (PQ11).** `max_ops_per_page` (a draw call counts one, `glyphs` counts its glyphs, a link counts one),
   `max_content_bytes` per page, `max_output_bytes` (written plus waiting objects, ±25 bytes of framing per object, and the xref
   at `build`), and `font: FontLimits` for the subsetter. The page limits are **sticky**: past the limit the page stops writing,
   `PageWriter::failure()` tells a painter loop to stop, and `finish()` and `build` return the error. Defaults: 2·10⁶ ops, 256 MiB
   of content, 1 GiB native; wasm32 64 MiB and 256 MiB.
7. **`embed_rgba` refuses a width × height that claims more pixels than the data holds** (`Invalid`); the TS zero-pads. It
   builds the soft mask first and drops it before the RGB (the TS keeps both arrays), and never allocates an alpha array for an
   opaque image; the "any pixel not 255" scan runs over all of `rgba`, extra and cut-short pixels included, as the TS loop does.
8. **`ChunkedSink`** hands over chunks of exactly `chunk` bytes (the last smaller) from one buffer allocated once per chunk;
   `IoSink` wraps an `io::Write`; `&mut S` is a sink.
9. Not ported: the standalone `PdfPage` without a document (no `gsFor`; opacity then draws opaque), the C# parallel `Build` and
   `AddPage(PdfPage)` (PQ10), `decodeImage` and `PdfFontSource` (PQ6: they lived in the painter).

### Parity (PP0 of the writer layer)

`tests/parity_support/capture_writer.mjs` (committed, deterministic: the xorshift32 of uo-testkit, the open fonts of
`shared/fonts/files`, pseudo-random image bytes) drives the frozen TS API from `UO_TS_ROOT` with 160 generated op scripts:
fonts of three program kinds (face, symbol fallback, custom with every optional field, `NaN`/`0` units per em), RGBA and JPEG
images with and without masks and decode arrays, axial and radial shadings, the whole `PdfPage` surface (rectangles, lines,
glyphs in both modes with scale, synthetic bold and italic, paths with both fill rules, clips, transforms, images, nested
`withAlpha`, links of every kind), resources created in the middle of a page, outlines (levels 0 to 7, blank titles, indices out of
range), special values (NaN, ±Infinity, -0, 5e-324, 1e300, rounding ties) in every numeric slot. Frequencies in the capture:
708 glyph calls, 805 paths, 404 links, 527 outline entries, 355 images, 172 shadings, 296 alpha scopes, 208 fonts on 322 pages.

| Check | Result |
|---|---|
| `compress: false`, byte for byte (length and SHA-256) | **155 of 155 scripts identical** (3,428 objects) on the first run, no port bug found by it |
| the 5 other scripts | the TS throws (`String.fromCodePoint` past U+10FFFF in ToUnicode); Rust returns `Invalid` for both modes |
| `compress: true`, structural (same numbering, dictionaries equal apart from `/Filter`/`/Length`, inflated streams equal, xref/trailer valid) | **155 of 155 identical** |
| streams deflated: TS (fflate) 812, Rust (miniz_oxide) 809 | the "only if smaller" rule flipped in 3 scripts (a stream near its edge); invisible to the comparison, counted and printed by the test |
| `fmt` over 65,000 values (random bit patterns, ties, ±1e9 bounds, NaN, -0, subnormals), `hexColor` (3,000 strings with `#`, white space, signs, non-digits, astral characters), `safePdfUri` and `utf16beHex` (4,000 strings with the JS white space set, controls, `ſ`, BOM) | all four digests equal |
| pdf.js 6.4 opens every Rust file (`check_pdfjs.mjs`) | 155 of 155 open, 309 pages, 3,520 text characters extracted |

Not available here: `qpdf` (the CI validity job of §6 stays to be wired; the test parser validates every offset, object frame
and trailer of each file, and pdf.js loads them). The corpus is synthetic by design; the oracle documents come with frontier H/P.

### Memory (measured, `uo-alloc`)

| Check | Result | Numbers |
|---|---|---|
| M1 per operation | green | scalars, writer (out-of-order define, streams, every error), sinks (broken, chunked), every `PdfDocument`/`PageWriter` method in both modes with fonts, images, shadings, links, outline, opacity and a dropped page, `embed_*` variants, each limit and the half-written build error, and **all 160 scripts × 2 modes**: live bytes return to the start exactly |
| M2 soak | green | 1,000 documents (the scripts in turn, both modes), live bytes sampled every 50: **0 B** above the start (5,455,152 B at every sample: the test fonts and scripts) |
| M3 streaming (PQ4) | green | 256×256 noisy images, chunked sink that drops the chunks: **peak 1.82 MB at 2, 8, 32 and 128 images** (files of 0.5, 2.1, 8.4 and 33.6 MB); the same render into a `Vec` peaks at 1.3, 2.9, 9.2 and 34.4 MB. The peak is one chunk buffer (1 MiB) plus the working set of one image |
| M3 pages | green | 400-rectangle pages (12 KB of content) with a dropping sink: 10 / 100 / 500 pages → file 32 KB / 320 KB / 1.6 MB, peak 1.4 / 1.7 / 3.0 MB (compressed contents wait for the catalog); with `compress: false` the file is 0.12 / 1.2 / 6.2 MB and the peak 1.2 / 2.3 / 8.2 MB |
| M3 one big image | green | 12 Mpx (4000×3000) noise, release: opaque peak 102 MB (8.53 N), allocated 203 MB; with alpha peak 80 MB (6.70 N), allocated 171 MB; file 36 MB. Within the plan's 9N + 4 MiB. The opaque case is higher because the deflate output of noise grows by doubling up to the input size: stopping the deflate once it passes the input size (the "only if smaller" rule would store it anyway) is a stage-5 candidate that also saves the time |
| M4 / M5 / M6 | not applicable here | no handle crosses a boundary in this crate; no cross-call cache (`clear_caches` is a no-op); no `unsafe` (`#![forbid(unsafe_code)]`), so no Miri |
| no-panic loops | green | 3,000 random hostile documents (NaN/Infinity/1e300 in every slot, garbage and mutated font bytes, handles of another document, tiny limits, failing sinks): 1,941 written, 639 stopped by a limit, 129 by the sink, 291 refused as `Invalid`, no panic; 1,000 files of the loop parse with a valid xref; 500 pages × 80 glyphs + 10,000 links + a 5,000-entry 9-level outline: 16,008 objects, 2.75 MB, valid; a chain of 20,000 outline levels does not recurse |

Checks: `cargo fmt --check`, `cargo clippy -p uo-pdf -p uo-opc --all-targets -- -D warnings` (plus `deny(clippy::indexing_slicing)`
in the library), `core/tests/grep-bans.sh`, `core/tests/float-casts.sh` clean; `cargo test -p uo-pdf` (4 test binaries, 47
tests, one `#[ignore]`d) and `-p uo-opc` green; `cargo build -p uo-pdf --target wasm32-unknown-unknown --profile wasm` builds.
Commands: `UO_TS_ROOT=… node core/crates/uo-pdf/tests/parity_support/capture_writer.mjs` regenerates `tests/parity/writer.cap`;
`cargo test --release -p uo-pdf --test writer_memory -- --nocapture --test-threads=1 --include-ignored` prints the numbers.

### Not done here

The painter and uo-image (frontiers P and I); the fuzz targets `pdf_*` of §8 (the no-panic loops are the stable-Rust form of the
writer ones); the wasm `memory.buffer` soak and the WebKit run of the chunked sink; `qpdf --check` in CI; the cancellation flag
(PQ9, with the painter); the writer is not yet in `rust-core.yml` (the workspace-wide `lint` job already covers it). Worktree
note: during this work other frontiers were editing uo-fonts and the workspace manifest, so the final runs were made in
the real workspace once it built, and the intermediate ones in a snapshot of HEAD with this crate overlaid.

## Milestone 1 — frontier P (the painter)

Hybrid mode, one Sonnet session, 2026-11-05. Scope: `render_pdf` / `render_pdf_to`, the display-list PDF painter over `uo_render` pages (`pdf.ts`,
`pdf-picture.ts`, `pdf-word-spaces.ts` + `advanceTwips`). New files in `core/crates/uo-pdf`: `src/render.rs` (1,235 lines with the doc comments),
`src/picture.rs` (553), `src/word_spaces.rs` (86), the tests `tests/painter_{parity,memory,nopanic}.rs`, `tests/support/{pages,hostile}.rs`,
`tests/parity_support/capture_painter.mjs`, `tests/parity/painter.cap` (60 generated pages, 1.1 MB) and `tests/parity/painter-docs.cap` (6 documents laid
out by the frozen TS engine, 1.2 MB).

### Ported (TS to Rust)

| TS (`write/packages/docx-layout/src/`) | Rust (`uo-pdf/src/`) |
|---|---|
| `pdf.ts` `renderPdf`, `fontFor` (`#pua` copy, symbol-font fallback `glyphId(cp - 0xF000)`), `imageFor`, `rgbaOf`, `pictureEnv` (`image`, `masked`, the `adjustedFor` cache), `bgImage`, `paintFillPdf` | `render.rs` `render_pdf_to`, `Painter::{font_for, image_for, rgba_of, pic_image, pic_masked, bg_image, paint_fill}` |
| `pdf.ts` `paintItem` (glyphs with word spaces and the pua runs, rect, line with the dash table, image, shape: shadow, pattern/blip clip, rectangular gradient raster, shaded gradient, plain; path, xform, clip) | `Painter::{paint_item, paint_run, paint_image, paint_shape, paint_shape_body}` |
| `pdf.ts` `outlineInto`, `runPathCmds`, `glyphsAsPath` (COLR layers, skew 0.2, Q to C at 2/3), `paintGlyphsFxPdf` | `Painter::{outline_into, run_path_cmds, glyphs_as_path, paint_run_fx}` |
| `pdf-picture.ts` `paintShapeEffectsPdf`, `paintPicturePdf` (shadow, glow, reflection, inner shadow, bevel, soft edges as `/SMask`, outline outside) | `picture.rs` `Painter::{paint_shape_effects, paint_picture}` |
| `pdf-word-spaces.ts` `withWordSpaces`, `sfnt.ts` `advanceTwips` with the advance table (PQ1) | `word_spaces.rs` `with_word_spaces`, `advance_twips`, `WordSpaceCursor` |
| (new) options, cancellation (PQ9) | `PdfOptions { compress, limits, paint, image, cancel }`, `PdfError::Cancelled`, `hex_color_units` (colour over UTF-16 units) |

From the other crates, as the hand-off of uo-render G asked: `geom::shape_parts(LineUnit::PdfPoints)`, `Painter::Pdf` bounds, `ChannelStore::Wrap`,
`gradient_samples` 512 (256 for `path`), `pattern_raster(0.75, 4)`, `blip_tiles(1/12700)`, `rect_gradient_raster(ppu 2)`, `gradient_tile_rgba(.., pdf)`,
`fx_layers(200/72)`, `dash_pattern`, `picture::dash`, the uo-image decoders and the uo-drawingml masks. The PDF's own as-path outline code (skew 0.2) is here.

### Parity evidence (PP0 of the painter)

`capture_painter.mjs` drives the frozen TS `renderPdf` (UO_TS_ROOT d0f45bc5, built `dist`) and writes, per input, the `spec` (the LayoutResult in the shape of
`tree.ts`, faces and images by index), `raw` (length:sha256 of the `compress: false` file), `norm` (objects:digest of the `compress: true` file, the structural
form of section 6) and `deflated`. The Rust side decodes the spec into uo-render types (`tests/support/pages.rs`) and calls `render_pdf`. Decision: the planned
TS adapter of uo-render C (`encodePage` then Rust decode) was **not used**: the Rust conversion from the generated wire types to the hand-written types is
frontier E's and does not exist yet, so the capture carries its own small JSON spec (same data; the Rust decode is test-only).

| Corpus | Inputs | Result |
|---|---|---|
| generated pages, committed (`painter.cap`) | 60 specs, 3,392 objects | **60/60 byte-exact (`raw`) and structurally equal (`norm`)** |
| generated pages, 400 specs (local) | 21,759 objects; 11,090 glyph runs (1,143 with text effects), 1,832 shapes, 1,860 images (950 with a picture look) | **400/400 equal** |
| laid-out documents, committed (`painter-docs.cap`) | 6 generated documents (open fonts): engine layouts with headers, footers, tables, page numbers | 6/6 equal |
| laid-out documents, 12 generated (local) | 55 pages, 934 objects | 12/12 equal |
| the 8 .NET synthetic inputs (pictures, page, page-pattern/frame/tile, textboxes, texteffects, synthetic), installed Office fonts | 14 pages, 196 objects | 8/8 equal |
| the Word oracle, every 9th document, installed Office fonts | **300 documents, 519 pages, 3,508 objects** | **300/300 equal** |

The generated corpus covers: word spaces inside and between runs with and without advance tables, pua codes, `scaleX`, synthetic styles, `asPath`, text effects
(all fills, outline with dashes, shadow, glow, reflection), every line dash, images (PNG RGBA/RGB/gray, BMP, JPEG shells with 1/3/4 components and Adobe,
undecodable bytes) with effects, crops, rotations, flips, picture looks (preset geometry, outline with dashes, outer/inner shadow, glow, soft edges, bevel,
reflection, alpha, duotone, colour change), shapes (29 preset names incl. unknown and `custom`, flips, rotation, adjust values, solid/linear/radial/rect
gradients, patterns, stretched and tiled blips, shadow, shape effects), paths, nested clips and transforms, page backgrounds and the five page fills, links
(uri, destination, rejected schemes), outlines. Hostile numbers (NaN, infinities, -0, 1e300, 5e-324, rounding ties) in rect/line/path/clip/glyph coordinates.
Not covered by a font: **COLR layers** (no open colour font; the flag runs the non-colour path only). `deflated` streams: the "only if smaller" rule flipped
for 3 of 60 and 11 of 400 specs (invisible to the comparison, counted by the test). `qpdf` is not installed here: validity is the test parser (every xref
offset, object frame, trailer) over every compared file.

Parity rounds: **0 port bugs found by the corpus.** One generator round: the first runs showed specs where the TS writes a text-effects layer of `NaN` width (a
NaN `scaleX` or glyph x on a run with effects: the TS emits an invalid `/Width NaN` image; Rust refuses it, decision G-D9 of uo-render); the generator no longer
combines them.

### Memory and no-panic (measured, `uo-alloc`; `tests/painter_memory.rs`, `painter_nopanic.rs`)

| Check | Result | Numbers |
|---|---|---|
| M1 | green | `render_pdf` over 24 specs x 2 modes returns the live bytes to the start (outline caches of the faces warmed first); the ops / content / output limits, the cancellation flag, a failing sink, tight helper limits and 30 hostile pages with navigation: no leak |
| M2 soak | green | 200 renders over 20 specs: live bytes **38,327,520 B at every sample (0 B above the start)** |
| M3 images | green | 256x256 noise, chunked sink that drops the file: **peak 2.14 MB at 2, 8, 32 and 128 images** (files 0.4 / 1.5 / 6.1 / 24.5 MB; allocated 3.5 / 9.2 / 32 / 127 MB) |
| M3 text | green | 40 lines x 8 words per page: 10 / 100 / 500 pages give files of 0.16 / 0.75 / 3.41 MB, peak 1.58 / 2.16 / 4.80 MB (dropping sink), 0.69 / 1.68 / 5.99 MB (Vec sink) |
| M3 one 12 Mpx image | green | through the painter (PNG decode + embed): peak 96 MB (8.0 N), allocated 153 MB; the writer's own figures (8.5 N opaque, 6.7 N with alpha) hold |
| M3 picture effects | green | 450x300 pt picture with shadow, glow and soft edges (layer capped at 8 Mpx): peak 24.8 MB, allocated 106 MB |
| M5 / M6 | n/a | no cross-call cache (`clear_caches` stays a no-op); no `unsafe` |
| no-panic | green | 600 hostile documents (NaN/inf/1e300 everywhere, resource ids that do not exist, faces without bytes or font, garbage and 60,000x60,000-declared PNGs, crops over 100 %, tiny limits, failing sinks, cancellation): 286 written (all parse), 148 stopped by a limit, 60 by the sink, 42 cancelled, 64 refused as invalid; no panic. A page-fill tile of 1/1000 px over a 1000 pt page stops at the operations limit in 0.48 s (2 million draws; the TS hangs). Cancelling from another thread after 30 ms stops a 400-page render in 31 ms |

Slow hostile inputs (bounded, not hangs): a blur radius of thousands of points on an 8 Mpx layer takes up to 17 s at uo-drawingml's default
`max_kernel_radius` 4,096 (about 3.7 s at 64, which the CI loop uses); a host that renders untrusted documents should lower
`PaintLimits.draw.max_kernel_radius` / `max_mask_pixels` (the wasm32 defaults already halve the mask).

Checks: `cargo clippy -p uo-pdf --all-features --tests --no-deps -- -D warnings` clean; `--lib --target wasm32-unknown-unknown` clean;
`core/tests/float-casts.sh` and `grep-bans.sh` exit 0; `rustfmt --check` on the files above clean; no `unwrap`/`expect`/`panic!`/panicking index in `src/`;
every `Math.*` through `uo_jsnum::math`/`conv`; `cargo test --release -p uo-pdf -- --include-ignored` all green. Commands:
`UO_TS_ROOT=.. node tests/parity_support/capture_painter.mjs [--pages N]` regenerates `painter.cap`; `.. --docs <dir> --open-fonts --inline-spec --max 6 --dump <d>
--out tests/parity/painter-docs.cap` the document fixture; `.. --docs <dir> --dump <d> --out <f>` plus
`UO_PAINTER_CAPTURE=<f> UO_PAINTER_DUMP=<d> cargo test --release -p uo-pdf --test painter_parity real` the local real-document runs (`UO_PAINTER_TS=<dump>`
prints the first differing byte).

### Declarations (every decision outside the TS semantics)

1. **API**: `PdfOptions { compress, limits: PdfLimits, paint: PaintLimits, image: ImageLimits, cancel: Option<Arc<AtomicBool>> }`; `render_pdf(pages, res, nav, o) -> Result<Vec<u8>>`, `render_pdf_to(.., sink)`. `PdfError::Cancelled` is new (PQ9; checked before each page and every 256 items, and since the milestone-1 gate fixes (2026-11-10) also before each raster effect: text effects, shape effects and picture effects, via `Painter::stop_for_cancel`, so one 600-run glow page (60 s) stops 40-45 ms after the flag instead of at the next 256-item check; the layout side of the flag is the layout's).
2. **Resources the TS keys by object identity are keyed by the resource ids**: fonts by `(face.key Sym, pua)` (two `FaceId`s with the same key share one embedded font, the first wins, as the TS `Map` by key), images by `ImageId`, adjusted pictures by `(ImageId, JSON key)`, page-fill tiles by a JSON-style string built with `uo_jsnum::json` (the same equalities as the TS `JSON.stringify` keys).
3. **Failure policy**: an image the decoders refuse (`Unsupported`, `Malformed`, a limit of `ImageLimits`) is left out (the TS `catch`, PQ11); a writer `Limit`/`Sink` error met while a resource is made is kept and the render ends with it after the page; `Invalid` from the writer is an absent resource. A mask/raster limit met inside a picture or shape effect (`PaintError`) ends the composition of that item (what was written stays; the TS has no limit and exhausts memory). Errors of `rect_gradient_raster` / `pattern_raster` / `blip_tiles` skip that fill part.
4. **Text-effect layers with a NaN size are not drawn** (uo-render G-D9; the TS writes `/Width NaN`). `run_box` of an empty effect run is skipped (the TS throws a `TypeError`). `advance_twips` with a code point past U+10FFFF and a table reads nothing (the TS `String.fromCodePoint` throws).
5. **`apply_picture_adjust` clamps** its stores (uo-drawingml G7); the TS PDF path stores into a `Uint8Array` and wraps. They differ only for a malformed colour string (negative or NaN channels). Colour strings reach uo-drawingml through `String::from_utf16_lossy` (a lone surrogate cannot come from XML).
6. **Outline titles** still go through `String` (W decision 4: a lone-surrogate title differs); link URIs through `from_utf16_lossy` (equal to the TS `TextEncoder`).
7. **`GeomEval::Drawingml` shapes are painted with the compact evaluator** (uo-render G-D13; Slides wires `uo_drawingml::preset_path` later). A face with bytes but no parsed `Face`, or a missing resource id, is "not embeddable"/absent. `PathItem.even_odd` is ignored (the TS `pdf.ts` ignores `evenOdd` too).
8. **Reflection loops** of the shape effects index with `js_index`: a NaN index skips the pixel (the TS stores `undefined`); `iw x ih` of the soft mask/reflection of a picture is bounded by `PaintLimits.max_raster_px`.
9. Numeric sites: `Math.round/min/max/floor/ceil/sqrt/cos/sin` through `uo_jsnum::math`; `Uint8Array` stores through `conv::to_uint8`; `Float32Array` stores through a local `f32_store` (`as f32` under `#[allow]` for the float-casts script); the constants `2/3`, `200/72`, `0.12`, `1/12700` written as the TS expressions; `Math.round(x/0.12)*0.12` for the PDF size.
10. **Outside the painter files**: `uo-render`, `uo-image`, `uo-drawingml` added to `uo-pdf/Cargo.toml` (`core/Cargo.lock` gained the three lines); `tests/writer_nopanic.rs` got the `Cancelled` arm; `hex_color_units` added to `color.rs`. Nothing outside `core/crates/uo-pdf/**`, `Cargo.lock` and this record was touched.

### Not done

- **COLR layer parity** (no open colour font; the code is a transcription), EMF/WMF glyph-index runs beyond the generated `gid` runs; `qpdf --check` (not installed) and the pdf.js text-extraction sample of section 6 for the painter files.
- The fuzz target `pdf_render` (the no-panic loop is its stable-Rust form), the wasm `memory.buffer` soak and the WebKit run of the chunked sink, the 2,000 generated documents of the CI corpus (the committed fixtures are 60 + 6), PP1 (end to end with the Rust layout), the C# vs Rust migration report.
- The oracle: 300 of 2,742 documents replayed (every 9th, 519 pages); the rest is the same command on a larger directory (local machine, fonts never committed).
- Cancellation inside the layout work budget and the facade mapping of the request timeout (S).

### What S and the bindings need next

- **S (server path)**: build `Resources` from the layout (faces with `Arc<[u8]>` bytes and `uo_fonts::Face`, `advances: None`), a `Navigation` from `Layout::navigation`, then `render_pdf_to(.., Vec)`; share a `cancel` flag with the layout work budget; set `PdfOptions.paint.draw` (`max_kernel_radius`, `max_mask_pixels`) and `image` limits for untrusted documents. The wasm export wraps `render_pdf_to` with a `ChunkedSink` (1 MiB or more); `PdfLimits::DEFAULT` and `ImageLimits::DEFAULT` already switch on the target, `PaintLimits::DEFAULT` does not (set `max_fx_px` for a phone).
- **Bindings**: `PdfError` maps to the facade error (`Limit(kind)`, `Sink`, `Cancelled`, `Invalid`); an image skipped for a limit is silent today, a warning list in the result would help the host (not built).

### Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| P | painter port + capture script + loader + PP0 (generated, laid-out, .NET, oracle sample) + M1-M3 + no-panic + cancellation + record (Sonnet, hybrid mode) | about 3 h wall, one session (the 400-spec capture takes 3 min, its replay 55 s in release) | about 120 (own tally) |

Parity rounds: 2 replays with mismatches (4 of 60 specs, then 1 of 300), both the NaN layer of the TS (a generator change, **no port bug**); every later run equal. Memory/no-panic: 3 test-design fixes (the `Usage` field types, a closure kept by the sink, the kernel limit of the loop), none in the library.

## Slides PDF (R-S5)

Status: **implemented and measured against the Slides display list, effects and font fallback included (wave 2, below); not yet compared
with PowerPoint's PDF** (the oracle is a
later measured step, see "The PowerPoint comparison" below). Plan: `slides-sheet-port-plan.md` §9 (M4, scope P1) and the
"Slides extensions" design of `uo-render.md` (R-S5). One Sonnet session, 2026-11-17, sub-frontier R-S5.

### Scope and where the code is

| Part | Where |
|---|---|
| `MeasureTable` filler from `uo-fonts` (the server/PDF path of decision T1) | `uo-pptx/src/pdf/measure.rs` (`fill_measures`, `text_width`, `FontSource`, `FaceSet`) — a new file in `uo-pptx`, so it does not touch B5b's `uo-fonts` EOT/MTX files nor `uo-dml-render` |
| Interpreter of the Slides list (state, paths, paints, layers, text, pictures, recipes) | `uo-pptx/src/pdf/{interp,paint,text,images,recipe,geom,css}.rs`; SVG pictures `svg.rs` (PowerPoint PDF fixes, below) |
| Entry points | `uo_pptx::pdf(&mut Deck, &PdfOptions) -> Result<PdfOutput, PdfFail>` (feature `pdf`; `PdfOutput { bytes, report }`) and `uo_pptx::pdf::pdf_from_lists(&[(&SlidesList, &SlidesResources)], &dyn PartSource, &PdfOptions)` (the interpreter alone: tests, hostile lists, lists emitted elsewhere) |
| Effects that need pixels (wave 2) | `uo-pptx/src/pdf/raster.rs` (the canvas interpreter over pixels: `Raster`, `Surface`, paints, the bevel), `effects.rs` (shadow images, raster layers, the bevel on the page, warps on the page, `Kept`/`KeptRange`), `warp.rs` (WordArt as outlines) |
| Writer additions (additive, used only by this painter) | `uo-pdf/src/document.rs`, `page.rs`: `PdfDocument::{add_ext_gstate, add_form, form_object, add_pattern, add_user_shading, embed_color_mask}`, `PageWriter::raw`; `uo-pdf/src/raster.rs` (`uo_pdf::raster`: coverage fill, stroker, premultiplied `Pixmap` with the canvas compositing modes, blur, `edt1d`; re-exports the `uo-drawingml` masks, light tables and sRGB curves) |
| Tests | `uo-pptx/tests/{pdf_measure,pdf_slides,pdf_effects}.rs`, `tests/support/{open_fonts,pdf_view}.rs` (they reuse `uo-pdf/tests/support/pdf_parse.rs`, the frame check of the Write PDF tests) |

**No `PdfProfile`** (plan §4.4 finding S9 said `render_pdf` hard-wires Word rules): the Slides list is a canvas call sequence, not Word
pages, so it does not go through `render_pdf` at all. The shared part is the app-neutral writer (`PdfDocument`/`PageWriter`, fonts with
subsetting and ToUnicode, images, shadings), which stays as it was; the interpreter lives with the producer (`uo-pptx`), the way
Write's painter lives with Write's page. Nothing in `render_pdf`/`render.rs` changed.

### How a list becomes a page

- **1 px = 1 pt.** The slide is emitted at `scale = 1/12700` px per EMU, origin 0; the page is `sw x sh` pt (= the slide size) and its content
  starts with `q 1 0 0 -1 0 H cm` (the canvas is y down). The interpreter keeps the canvas state (CTM, alpha, composite, fill/stroke paints,
  line style, dash, shadow, blur, text alignment and baseline, the font) and writes PDF operators in the canvas's own user space:
  `save/restore` = `q/Q`, `transform/translate/rotate/scale` = `cm`, `setTransform` = the `cm` that takes the current matrix to it,
  registers keep the CTM. A path is stored with the transform it was added under and written, at paint time, in the user space of the paint
  (the canvas keeps device space; PDF forbids `cm` between construction and painting). Arcs, `arcTo`, ellipses and `roundRect` become cubics.
  Matrix entries are written with 6 decimals, coordinates with 4 (3 decimals lose a rotation).
- **Colours and alpha.** CSS colours (`#rgb[a]`, `#rrggbb[aa]`, `rgb()`, `rgba()`, `transparent`, names) are parsed in `css.rs`; an unreadable
  one is ignored as the canvas does. `globalAlpha * colour alpha` is an ExtGState (`/ca /CA`, with `/BM`), written only when it changes.
- **Blend modes**: `source-over` Normal, `multiply`, `screen`, `darken`, `lighten` (the recipes' `difference` too). **Not PDF**: `destination-in`,
  `destination-out`, `source-in` (see the missing list).
- **Layers** (`LayerBegin/End`, `DrawLayer`, `ReleaseLayer`) are transparency groups: a form XObject (`/Group /S /Transparency /I true`) in canvas
  space, composited under the identity transform with the context's alpha/blend mode (`Composite`) or kept for `DrawLayer`
  (drawn under the current transform). With a layer, `LayerEnd` continues at `fallbackEnd` (the else block is skipped); the layer right after
  a shape slot's `SlotBegin` is the soft-reveal layer and, in the static state of a PDF, is not acquired (the TS interpreter's rule). A layer in
  which a `destination-in/out` *fill* had to be skipped (the reflection's fade) is **dropped** instead of composited wrong (`layer:dropped`).
- **Gradients**: linear and radial are sampled type-0 shadings (256 samples, canvas RGBA interpolation); stops that carry alpha add a
  luminosity soft mask of the alpha ramp; one stop is a two-stop ramp; **conic** is its average colour (`gradient:conic`); a gradient
  *stroke* is its first stop (`gradient:stroke`). Text with a gradient fill is the text as a clip (`7 Tr`, kept until `ET`) and the shading; a run that shows no glyph paints nothing (see "PowerPoint PDF fixes").
- **Pictures**: JPEG is passed through (`DCTDecode`; brightness/contrast of a derived image ride on `/Decode`); the other raster formats are
  decoded by `uo-image` and written as Flate with an `/SMask` for alpha; the colour effects of `fxImage` (duotone, gray, bi-level, clrChange,
  brightness/contrast) are applied to the pixels (a JPEG with a pixel effect keeps the original and is counted: `image:jpeg-effect`). The 9-argument
  `drawImage` is a clip and a placement. Generated bitmaps (pattern tiles, path-gradient rasters) are RGBA images. An **SVG** picture (`asvg:svgBlip`
  with no raster beside it) is vector content, a form like a metafile recipe (`svg.rs`, see "PowerPoint PDF fixes").
- **Patterns** are tiling patterns of the image cell; the matrix is `pattern transform * CTM * base` where `base` is the page flip on the
  page and the identity inside a form (a pattern's default space is the space of the content stream that uses it); `a:tile` follows
  `tilePattern` (alignment, the device-snapping of tiles smaller than a device pixel, flips as mirrored copies in the cell; `TileDef.nat` of the
  chart engine); `no-repeat` is one clipped copy.
- **Text** (`TextRun`, `FillText` of the chart engine): the face is the host's answer for the family (`FontSource`), embedded **once per font file**
  (subset and ToUnicode by `uo-pdf`); a file whose OS/2 `fsType` forbids embedding is never put in the PDF (`font:not-embeddable`) — proprietary
  fonts are only ever in the PDF if the host provides the files, and the Write rule holds: nothing proprietary is bundled. Glyphs come from the
  cmap, pair kerning is `TJ` adjustments, letter spacing and the *fit* (the TS spreads `laidW - natural` over the characters when more than a
  quarter px off) are `Tc`, a glyph the face lacks advances by `.notdef` and is counted (`glyph:missing`). Fill: colour, gradient (clip), pattern
  (`scn`); outline = a second stroked pass (`1 Tr`, round joins); underline/strike = lines; bullets draw as the TS does (no fit/spacing).
  Alignment and baseline of `FillText` follow the canvas (`middle`, `top`... from the face's hhea metrics).
- **Metafile recipes** (B5b: EMF, EMF+, WMF, PICT, TIFF arrive as bitmaps whose bytes are canvas calls): played as **vector content**. Each canvas of
  the recipe is a form in canvas space; the picture is the root canvas placed in the destination rectangle; paths, fills, strokes (dash with phase),
  clips, transforms, gradients, patterns of a canvas, text under the raw CSS font, `drawImage` of a canvas or of a compressed bitmap (PNG/JPEG
  embedded), `putImageData` (an image at device coordinates) all go through the same interpreter code; `globalCompositeOperation` `difference`,
  `multiply`, `lighten` are blend modes. The **exact XOR** (`xorRect`, a pixel operation) has no PDF form: counted (`meta:xor-rect`) and the
  approximation the TS draws when it cannot XOR is the content that follows, which is played. A cut or damaged recipe keeps what it drew.
- **Hyperlinks** (wave 4, `pdf/links.rs`, `pdf/mod.rs` `Writer::page`): the display list carries none (it is the canvas calls); the emission
  hands them beside it. (a) **Run marks**: `Emission::links` (`uo-dml-render.md` "Links (wave 4)") names every `TextRun` written for a
  linked placed run with its box in list user space; when the vector interpreter draws that `SItem::Text` — or a raster pass of a layer draws
  it (soft edges, reflections: the run is marked once, by whichever draws it first) — the box goes through the current transform to its
  device AABB (rotated and flipped shapes included). The pieces of one link that touch (the layout places runs word by word; vertical and
  right-to-left lines too) are joined per line fragment, as PowerPoint writes its rectangles; each is cut to the page (a piece wholly off the
  page — text overflowing the slide — gets none: `PdfReport::links_off_page`) and placed as a `/Link` with `safe_pdf_uri` (http, https,
  mailto; others `hyperlink:scheme`). (b) **Shape clicks and jumps** from `uo_pptx::links::slide_links` (`ShapeClick`: `a:hlinkClick` on
  `p:cNvPr`, a group's click for its children; `RunJump`: a run's slide/show jump, shape-wide) over the shape's box turned by its `rot`: an
  address as a URI, a slide jump as an internal `/Dest` to its page, next/previous/first/last slide to the page computed from this one;
  "last viewed" and "end show" have no PDF form (`hyperlink:action`). `PdfReport::internal_links` counts the page destinations. (c) A shape
  whose text drew no linked piece at all (WordArt warps draw outlines, not runs) gets its run links over its turned box — the TS viewer's
  hit-test geometry (`PdfReport::shape_wide_links`). `hyperlink:unplaced` = a model link run (with text) whose shape drew no piece with its
  address and had no shape-wide fallback (matched by address, not text: caps, symbol fonts and tabs change the drawn text). A shape with both a
  shape address and a run jump gets both rectangles (the same box). `pdf_from_lists_with_links` takes the marks for lists emitted elsewhere
  (`pdf_from_lists` is unchanged: no links).
- **Limits and failure policy**: `PdfLimits` apply (ops and content bytes per page, output bytes); a limit stops the document with the typed error;
  a slide that cannot be read or emitted is a blank page of the slide's size and is listed in `PdfReport::failed_slides`; an inconsistent list
  (`validate`) is refused; a picture the decoders refuse is left out (counted); save depth 512 per target, 12 nested layers, 1 M path segments,
  256 canvases and 65 k objects per recipe. No `unwrap`, no panicking index.

### What is not drawn (the missing-effects report, per document in `PdfReport::missing`)

"Complete = nothing disabled": every key below is a count of something the PDF did not draw as the list says. Before → after is wave 1 →
wave 2 over the same 538 decks / 1,357 pages (open fonts only).

| Key | Meaning | Before | After |
|---|---|---|---|
| `shadow` | canvas shadow not drawn (counted per paint) | 10,306 | **0** (drawn: 3,765 shadow images; the old count was per paint, and the rest cast nothing: text whose glyphs are all missing, shadows outside the page, or paints inside rasterized layers, whose shadows are in the layer's pixels) |
| `blur`, `composite:destination-in` / `-out` / `source-in`, `layer:dropped` | soft edges and reflections | 154, 275 / 121 / 0, 154 | **0** (278 layers rasterized). Still counted outside a layer (no emitter path; hostile lists only) |
| `effect:warp` | WordArt text absent | 227 | **0** (218 warps drawn as outlines on the page; the other 9 are inside rasterized layers, drawn there, or have no visible glyph) |
| `effect:bevel` | bevel not shaded | 89 | **0** (81 shaded on the page; the other 8 are shaded inside rasterized layers or are below the TS's 0.5-px width, which it skips too) |
| `bevel:underlay` | a bevel shaded over a page whose JPEG/metafile under the shape was left out of the raster (shading exact where the shape's own fill is opaque) | — | 5 |
| `raster:fallback` | a layer needing raster compositing that holds a JPEG or a metafile (no decoder here): drawn as vector, its modes/blurs counted as before | — | 0 |
| `warp:picture-fill` | WordArt letters with a picture fill (drawn with the run colour) | — | 0 |
| `glyph:missing` | a character no face has, after the fallback chain | 327,000 | **299,280** (27,516 glyphs now drawn by a fallback face). With this machine's installed fonts added (`UO_PDF_SYSTEM_FONTS=1`, read in place): **48,637** |
| `meta:xor-rect` | exact XOR of a metafile (recipes are vector forms; the XOR needs the recipe's pixels) | 5 | 5 |
| `image:unsupported` | an image no decoder reads (an SVG, until the PowerPoint PDF fixes: now 0, the SVG is vector content; `svg:*` keys for its undrawn parts) | 1 | 1 |
| `transform:singular` | painting under a singular transform skipped (the canvas draws nothing either) | 1 | 1 |
| `hyperlink:unplaced` | links not placed (wave 4: **0**, see the hyperlink paragraph) | 686 (265 placed) | 690 (261 placed: 4 linked runs are now inside rasterized soft-edge layers) |
| `gradient:conic`, `gradient:stroke`, `pattern:stroke`, `pattern:repeat-axis`, `clear-rect`, `media-frame`, `image:jpeg-effect`, `image:recipe-truncated`, `font:missing`, `font:not-embeddable`, `chart:frame`, `effect:metafile-rop` | approximations and rare cases | 0 | 0 |

Still disabled, in order of count: 299,280 characters without any face among the open fonts (CJK, Arabic, emoji of the generated `b3a`
decks; a host with system fonts drops it to 48,637), 5 metafile XORs, 5 approximate bevel underlays, 1 SVG picture,
and the classes that have no case in the corpus but would count if met: JPEG/metafile content inside a soft edge or reflection
(`raster:fallback`, no JPEG decoder in the core), picture-filled WordArt, conic gradients and gradient/pattern strokes.

### Numbers

- **Measure filler against the B3 capture logs** (the capture's measurer is the same function: advances plus the Office kerning pairs,
  `units * px / upem`, over the open fonts): **456,505 measurements, 0 mismatches, bit-equal** — `textlayout-samples` 6,280, `-b1a` 47,558,
  `-b3a` 265,093, `-b3a-nokerning-noseg` 137,574 (248,022 of them kerned strings of two or more characters). The catalog substitution
  (`FaceSet`) answers the same face as the capture's own rule for the 20 families in the logs (80 family/variant pairs). The `drawops-*` logs hold the
  deterministic stub's answers, not font widths, so they are not comparable (the stub is a different measurer by design).
- **PDFs**: every committed sample (`slides/samples`, incl. `playback`) and the generated groups `b1a` (200), `b3a` (120), `b5a` (150), `b5b` (40):
  **538 decks, 1,357 pages, 0 failed slides**; wave 1: 117,064,289 bytes, 1,521,573 glyphs; wave 2 (effects and fallback): **167,046,998 bytes,
  1,543,684 glyphs**; each PDF passes the frame check of the Write tests (header,
  every xref offset hits `N 0 obj`, every object closed, trailer), `page count = slide count` (also the `/Count`), `page size = slide size`
  (EMU/12,700), and the text read back through each font's ToUnicode equals the glyphs drawn (`report.glyphs`); on 53 decks the characters read
  back are a subset (as a multiset) of the characters of the list's runs (36,368 of at most 37,209 reached the PDF; the difference is text
  skipped under the shadow idiom or at alpha 0). 243 metafile recipes were played.
- **Speed**: wave 1: 538 decks in 24.9 s (release, whole test), 18 ms per page, slowest deck 0.28 s; wave 2: 67.7 s alone (76 s beside the
  other PDF tests), 50 ms per page, slowest deck `b3a/0061` 2.7 s (text shadows and bevel underlays: each bevel re-rasterizes the page under its box).
- **Hostile input**: 10,000 random lists (NaN, infinities, 1e308, unbalanced saves, layers, composites, paints, indices in range) in 1.6 s, 0
  panics, every PDF valid; 600 flipped/cut/extended recipes, 569 played, 0 panics.
- **Memory**: M1 `assert_no_leak` over a deck export (`reference.pptx`) and over a metafile deck (`b5b/0000`): live bytes return exactly. M2: 24
  exports, live bytes flat (7,511,646 B on every sample). M3 (`reference.pptx`): PDF 55,446 B, allocated 2,655,718 B, peak above start 840,909 B.
- **Write's PDF is unchanged** (`cargo test -p uo-pdf --release`, all green after the additions): `writer_parity` 3 and `painter_parity` 3 (the
  TS captures, byte/structural), `writer_unit` 30, `writer_memory` 9, `painter_memory` 5, `writer_nopanic` 5, `painter_nopanic` 4. The page
  `/Resources` dictionary is written exactly as before unless a Slides-only resource (extra state, form, pattern) was registered.
- Checks: `cargo clippy -p uo-pptx -p uo-pdf --all-targets --all-features -- -D warnings` and the `wasm32-unknown-unknown` lib clippy clean;
  `rustfmt --check` clean; `core/tests/grep-bans.sh` 0; `float-casts.sh` shows hits only in B5b's `uo-fonts/src/mtx.rs`, none in `uo-pdf`/`uo-pptx`.

### Decisions outside the plan (each declared)

1. The Slides painter builds whole content streams itself (`Vec<u8>` per target) and hands each page over with `PageWriter::raw`; the
   writer keeps its page-oriented API for Write. Forms and patterns are written at once (their `/Resources` is one object, defined at `build`).
2. Layers are forms drawn at the identity transform with the *context's* alpha, as the canvas composites them; a tainted layer is dropped, not
   approximated (a mirrored, unfaded reflection is worse than none).
3. (wave 1) Shadows and blurs were counted, not approximated. Superseded in wave 2: they are rasterized as Word rasterizes its effects (see
   "Effects and font fallback").
4. `FaceSet`'s substitution is the catalog's (`metric_substitute`, a superset of the capture's); a host that wants the server policy
   (`SubstituteFontProvider`, `SystemFontIndex`) implements `FontSource` over it.
5. The shared `SlidesResources` getters (`face_entries`, `media_entries`, `derived_entries`, `bitmap_entries`) were added to `uo-render/src/slides.rs`
   (read-only accessors; the PDF reads the producer's tables directly, no codec round trip).
6. A new `SlidesResources` per slide: media pictures are cached across slides by part name (a logo of the master is embedded once), generated
   bitmaps and gradients per page, so a long deck does not keep every raster of every slide.

### Differences from the vendor converter (`converter/packages/converter/src/pptx/pdf.ts`, 533 lines; `slides-processing` is its reader)

Read from the source (the package that produced Slides PDFs before; nothing was run):

| Vendor converter | This painter |
|---|---|
| Its own drawing code over the *model* (`drawShape`, `presetPath`, `drawTable`, `drawTextBody`), a second renderer | Interprets the same display list the viewer paints, so shapes, tables, SmartArt, charts and pictures match the viewer by construction |
| `opts.fonts` mandatory TTFs, one `TtfProgram` per variant, widths without kerning | A `FontSource` policy (open fonts + substitutes), advances and Office kerning pairs identical to the layout's measurer, subsetting and ToUnicode per font file |
| Gradients: 24 interpolated strips clipped by the path | Sampled axial/radial shadings with alpha soft masks |
| Shadows (`outerShdw`), `fillAlpha`, `alphaModFix` ignored (opaque drawing) | Alpha, blend modes, layers honoured; shadows counted as missing |
| Charts: a gray placeholder | Charts are in the list (chart engine) and are vector content |
| Images: JPEG direct, PNG decoded; gif/svg/webp/bmp a gray placeholder, silently | JPEG direct; PNG/GIF/BMP/TIFF decoded; metafiles/PICT as vector content; undecodable pictures are left out and counted (never a silent placeholder) |
| No effects pipeline, no 3D, no WordArt warp, no reflections or soft edges | Not drawn either, but **listed** per document (`effect:*`, `composite:*`, `layer:dropped`) |
| Tables: simplified (1 run per line); rot/flip via a local `cm` | Whatever the emitter lays out (the viewer's tables, rotation, flips, groups, cameras) |
| No hyperlinks, no outline | Link annotations over the linked runs (see above); no outline yet |
| No failure report: unsupported things disappear | `PdfReport` (counters, failed slides, per-page missing counts) |

### The PowerPoint comparison (a later measured step; nothing of it is done here)

The committed scripts (`slides/tools/ppt-lab/`, read-only here) already do the COM part for PNG: `export-deck.ps1` opens the deck read-only
**without a window** (`Presentations.Open(path, ReadOnly, Untitled, WithWindow=false)`), exports a warm-up slide and waits 3 s (embedded fonts load
asynchronously), never touches the user's window and only closes what it opened (the `no-focus-steal` rule); `.ps1` files need a UTF-8 BOM and
`-ExecutionPolicy Bypass`. A PDF comparison needs, in order: (1) a script like `export-deck.ps1` that calls `$pres.SaveAs($pdf, 32)`
(`ppSaveAsPDF`) or `ExportAsFixedFormat($pdf, 2)` (`ppFixedFormatTypePDF`, with `ppPrintOutputSlides`, no hidden slides) over each
`samples/*.pptx` and the generated decks, with the fonts synced by `npm run fonts:sync` so PowerPoint and the Rust PDF use the same files
(and a second run with open fonts only, to separate font effects from layout); (2) page size and count; (3) a text comparison through
pdf.js `getTextContent` (per-item x, y, size and font name: the exact measure, as the Write corpus does) and a per-slide raster comparison
with the same renderer at the same dpi (pdf.js in headless Chromium, as `render-deck.mjs`/`viewer-check.mjs`); (4) the PowerPoint facts to
measure first, in this order, because the missing list depends on them: shadow geometry (offset, blur, alpha) and how PowerPoint writes it in the
PDF (a raster with an SMask), soft edge and reflection (rasters), WordArt warp (outlines), bevel (raster), the conic gradient (a mesh), and
whether PowerPoint embeds fonts as TrueType subsets (OFL replacement fonts differ in name only). Facts recorded in the oracle memory
(`playback-oracle-facts`, `pptx-render-rules-measured`) already cover the cascade, `spcBef`, linear-RGB gradients and charts.

### Effects and font fallback (wave 2, 2026-11-17)

One Opus session (frontier "PDF effects + font fallback"). The TS viewer gets these effects from the browser canvas (shadows, `filter`,
compositing modes, `getImageData` for the bevel, raster strips for WordArt); a PDF has none of them, so the painter does what Word does in its PDF
(memory fact: Word writes glow/shadow/reflection as images with alpha behind vector text): the pixel-only parts become images, everything else
stays vector. PowerPoint's own PDF was not consulted (no TS × PowerPoint divergence was in question; the comparison stays the later step below).

- **The raster interpreter** (`raster.rs`): the canvas of `paint-slides.ts` over premultiplied pixels (`uo_pdf::raster::Pixmap`) at 2 raster px per
  pt (144 dpi; regions above 4 M px get a lower resolution). Same item rules as the vector interpreter (slots = save/restore, the soft-reveal layer
  after a shape slot not acquired, `fallbackEnd`); fills, strokes (own stroker: joins, caps, miter limit, dashes), text (glyph outlines placed by the
  same fallback-aware placement as the PDF text), pictures (bilinear), linear/radial (two-circle)/conic gradients, patterns with `a:tile`, layers,
  clips (coverage masks), `clearRect`, every `CompositeOp` (unbounded modes act over the whole clip, as the canvas), `filter: blur` (Gaussian,
  sigma = the CSS length), canvas shadows (sigma = blur / 2, offset in device px; the off-screen idiom works because the shadow is the coverage of
  the polygons moved by the offset), the bevel and the warp. A first pass in measure mode gives the device bounds of a range, so a layer's
  surface covers only what it paints. Coverage: `uo_pdf::raster::fill_coverage` (4 sub-scanlines, exact horizontal coverage, sorted edges with an
  active list — the `uo-drawingml` fill rescans every edge per row, too slow for a slide).
- **Shadows on the vector page** (`effects.rs`): every paint under an active shadow first draws the shadow as a 1 x 1 image of the shadow colour
  whose `/SMask` is the blurred alpha (`PdfDocument::embed_color_mask`; a soft mask need not have the image's size, ISO 32000-1 11.6.5.3), at 2 px per
  pt (fewer for wide blurs, `3 / sigma`), placed in device space under the clip in force; the shape is then drawn as vector (or not at all under the
  off-screen idiom). The source alpha is the coverage times the paint's alpha (gradients per pixel; picture patterns count as opaque; a drawn picture
  uses its decoded alpha when it is not a JPEG). Text shadows (`TextRun.shadow`) are the glyph outlines' shadow; the text stays text.
- **Raster layers**: a layer whose items hold `destination-in/out`, `source-in`, a blur, a bevel or `clearRect` (soft edges, reflections) is run
  through the raster interpreter and placed as one image (composited at the identity with the context's alpha; or kept, for `DrawLayer`). A kept
  vector layer that a raster layer draws (the reflection draws the shape's layer) is rasterized from its recorded items (`KeptRange`).
- **Bevel** on the page: the page up to the effect is rasterized over the shape's box (lenient: a JPEG or metafile under it is left out and counted
  `bevel:underlay`), `applyBevel` (the distance transform `edt1d`, `bevel_phi`, `light_at`/`light_face` of `uo-drawingml`, linear-light `colour * D +
  S`) gives the lit colour `nv` of each silhouette pixel, and the image of `nv` with the silhouette coverage as alpha is placed over the page: the
  PDF's compositing then gives `px + (nv - px) a`, the TS's own mix.
- **WordArt** (`warp.rs`): the geometry of `slides-warp.ts` (all presets with their defaults, arc-length parametrization, `pathAt`, parallel curves,
  `arcLift`, the line groups, ink extents from the outlines) applied to the glyph outlines: envelope presets map every outline point through
  `P(u, v)` (pieces of at most 2 output px, curves to 0.15 px), single-path presets place each glyph rigidly on its cell. Letter fills: colour with
  the run alpha, gradients whose end points follow the warp (the gradient itself is not bent), preset patterns as the colour mixture of their
  density, outlines (`width * pxPerPt`); the layer shadow is the outlines' shadow; the tiled picture is a tiling pattern clipped to the outlines.
- **Font fallback** (`measure.rs`, `text.rs`): `FontSource::fallback(cp, bold, italic, primary)` (provided method, default `fallback_chain`: the
  families of the Write layout's `fallbackOrder` — Leelawadee UI for Thai, MS Mincho / Malgun Gothic for CJK / Hangul, the class fallback and Cambria
  Math for arrows and math, then Segoe UI Symbol, Segoe UI Emoji, Arial Unicode MS, MS Gothic, Cambria Math, Segoe UI, Arial — each through the
  source's own substitution, so Arial is Liberation Sans in `FaceSet`); `FaceSet` then scans its own faces ("the open fonts available", same
  variant first). The measure (`text_width_in`, used by `fill_measures`) and the drawing (`place_glyphs`) use the same per-character faces, so the
  layout and the PDF agree; a run its face covers measures bit-for-bit as before (the B3 logs still replay with 0 mismatches). In the PDF every glyph
  is positioned at its layout pen position through `TJ` adjustments, switching `Tf` to the fallback font's embedding inside the text object.
- **Cancellation**: `PdfOptions::cancel: Option<Arc<AtomicBool>>` (default `None`), checked before every slide and every raster effect; a raised flag
  fails the export with `PdfFail::Pdf(uo_pdf::PdfError::Cancelled)` (the Write PDF's error).
- **Write's PDF unchanged**: the `uo-pdf` additions are a new module and a new method; `cargo test -p uo-pdf --release` green (`writer_parity` 3,
  `painter_parity` 3, `writer_unit` 30, memory 9 + 5, no-panic 5 + 4). `uo-fonts` was not touched.
- **Tests** (`tests/pdf_effects.rs`, uncompressed PDFs read back): shadow colour, peak alpha and order (under the shape); the off-screen idiom draws
  only the shadow; a destination-in layer's pixels (half alpha kept, the rest cleared); a reflection faded downwards with the kept layer still vector;
  a bevel lit differently at top and bottom; five warp presets as outlines inside the box; a character Caladea lacks drawn by Liberation Sans (two
  fonts, ToUnicode round trip, the measure follows the fallback, off = missing); text shadow; hostile effect lists; cancellation. The 538-deck test
  (all valid, page count/size, ToUnicode text = glyphs drawn, the text subset check now counting fallback-drawable characters) and 3,000 hostile
  lists (`UO_PDF_HOSTILE_ROUNDS=3`) pass; memory M1/M2 unchanged in shape (`memory_is_flat_over_a_deck` green).
- **Size**: 117 MB → 167 MB over the corpus. Measured contributions before the warp flattening was tuned: shadows 10 MB, raster layers 3.4 MB;
  warps were 94 MB with 1-pt pieces and became small with tolerance-based flattening.
- **Decisions declared**: (W1) pixels at 144 dpi (Word's effects are about 200 dpi; a region over 4 M px goes lower); (W2) warp letters as outlines,
  not raster strips (exact mapping of the strips' piecewise-affine approximation; smaller and sharper), gradients not bent; (W3) a shadow's source
  alpha for picture patterns is 1; (W4) the bevel underlay omits JPEG/metafile pictures (counted); (W5) the fallback face's advance is the measure
  (the browser measures and draws with its own fallback; the old `.notdef` advance is kept only when no face has the character).

### Hyperlinks (wave 4, 2026-11-17)

Over the same 538 decks (`tests/pdf_slides.rs`, open fonts): before, 261 links placed by text matching and 690 `hyperlink:unplaced`; now
**994 link rectangles** — 941 run fragments placed through their marks (the 4 runs inside rasterized soft-edge layers included) and 53
shape-wide (runs in WordArt warps) — **0 unplaced**, 2,089 linked fragments of text that overflows the slide left without a rectangle
(`links_off_page`), 0 refused schemes. The corpus has no shape clicks or jumps (its decks write run addresses only); the constructed slide of
`tests/links.rs` places a slide jump (on a shape turned 90 degrees), "next" and "last slide" as `/Dest` (3 internal), a shape address and a
run address as URIs, refuses a `file:` address (`hyperlink:scheme`) and counts "end show" (`hyperlink:action`). Before the fragment join the
same corpus gave 11,797 rectangles (one per placed word or letter). Write's PDF is unchanged (`cargo test -p uo-pdf`: uo-pdf untouched).
The PDFs of the decks with links changed, so `uo-ffi/tests/parity/slides-native.tsv` (32 of 93 digests) needs `UO_BLESS=1`.

### Next steps

1. A JPEG decoder (or the host's) for raster layers and bevel underlays that hold photos (`raster:fallback`, `bevel:underlay`), then the recipe
   played onto pixels for the exact metafile XOR (`meta:xor-rect`).
2. Speed: the bevel underlay re-rasterizes the page prefix per bevel and every text shadow is its own image; batching the shadows of a
   paragraph and caching the underlay would bring the 50 ms per page back towards 20.
3. An outline from the slide titles, `/PageLabels`, notes pages (hyperlinks are done: wave 4 below).
4. Conic gradient (a type 4/7 mesh or a pre-rasterized image), gradient strokes (clip-stroke is not available: a stroked outline path would
   be needed), `repeat-x/y` patterns.
5. The PowerPoint PDF oracle above; then `FontSource` over `SubstituteFontProvider` for the server/.NET facade and the `uo-wasm` export
   (`export_pdf` for the Slides viewer, chunked sink).

### Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| R-S5 wave 2 | raster interpreter, shadows, raster layers, bevel, warp outlines, font fallback, cancellation, tests, record (Opus) | one session; a first corpus run took 136 s (raster shadows painted over whole surfaces: fixed by bounding each shadow and blur to its region, 10 s → 0.9 s on `b1a/0075`) and 229 MB (warps flattened at 1 pt: fixed with tolerance-based flattening, 167 MB) | about 120 |
| R-S5 | filler + equality test, writer additions, interpreter, recipes, hyperlinks, tests, record (Sonnet) | one session; the 538-deck export test runs in 25 s, the filler equality in 0.5 s, the hostile 10,000 lists in 1.6 s, `cargo test -p uo-pdf --release` in 86 s; time lost to the concurrent chart-engine edit of `uo-dml-render` (it broke the build for a while: the `TileDef.nat`, `PathOp::RoundRect`, `SItem::ChartFont/FillText` additions arrived mid-session and were handled) | about 170 (own tally) |

Rework: the extraction test's expectation (outline passes and chart `FillText` draw glyphs the runs list once or not at all: fixed in the test, not in the
painter), a pattern base matrix that was wrong inside forms (found by reasoning about the PDF default space before any test, fixed with `Target::base`),
four `\n` literals the shell turned into newlines while patching (found by the compiler/grep, fixed with the editor).

## PowerPoint agreement (M4 prep)

Status: **measured; the two PDF-only items fixed (1 gradient text, 10 SVG picture, 2026-11-18: "PDF fixes" below), the TS-legacy items
still to measure.** The M4 gate's "PowerPoint agreement" item (`slides-sheet-port-plan.md` §9, §11): the
Rust Slides PDF (`uo_pptx::pdf`) against PowerPoint 365 pt-BR's own PDF of the same deck, page by page, with the frozen TS viewer's canvas
as the third witness so that each difference is either **TS legacy** (Rust = TS ≠ PowerPoint: a rule to measure, where Office decides) or
**PDF only** (Rust ≠ TS = PowerPoint: the PDF interpreter). One Opus session, 2026-11-17. Harness, data and how to rerun:
`core/crates/uo-pptx/tests/oracle_ppt/README.md`.

### Method

- **Decks** (`oracle_ppt/decks.tsv`, 627): every tracked `slides/samples` file (53: 28 `.pptx`, 25 `.ppt`) and the generated groups
  (`b1a` 200, `b5a` 150, `b5b` 40, `b6a` 100, `c1d` 60 all; `b3a` every 5th, 24). **PowerPoint refused 488 of the 574 generated decks**
  (318 "file corrupt", 62 "could not open", 83 `0x80CB8002`, 25 opened but failed `SaveAs` PDF; `results/ppt-pdfs.tsv`): b1a 38/200,
  b3a 24/24, b5a 0/150, b5b 4/40, b6a 9/100, c1d 11/60 open. The generators target the TS reader (hostile values, lenient structure), not
  schema validity. Compared: **139 decks, 736 pages** (601 from the committed samples, 135 generated). Page count and page size agree on
  all 139 (0 differences); 0 Rust failures.
- **PowerPoint** (`slides/tools/ppt-lab/export-pdf.ps1`, driver `pdf-oracle.mjs ppt`): COM, powershell with `windowsHide`, each deck opened
  read-only `WithWindow = msoFalse`, hidden slides shown in memory, `SaveAs(ppSaveAsPDF)` (PowerShell's late binding cannot pass
  `ExportAsFixedFormat`'s optional arguments). PDFs cached by deck sha256 under `core/target/w4-oracle/ppt-oracle/ppt` (88.5 MB, not
  committed; digests in `results/ppt-pdfs.tsv`). 484 s of PowerPoint time for the 139 decks (`b3a` decks ≈ 20 s each: CJK font linking).
- **Rust** (`oracle_ppt/main.rs`, `cargo test -p uo-pptx --release --features ppt --test oracle_ppt`, opt-in by `UO_PPT_ORACLE_OUT`): two
  font policies: `system` = the installed fonts **read in place** (Windows fonts, user fonts, Office `CloudFonts` — the files PowerPoint
  draws with, so text differences are layout, not substitution), `open` = Liberation/Carlito/Caladea only (the shipped server default).
  Built from the working tree of 2026-11-17 evening, **including the in-progress hyperlink frontier** of another agent (links are placed);
  the "after" numbers from the tree of 2026-11-18 (the PDF fixes, the hyperlink wave as committed).
- **TS** (`pdf-oracle-ts.mjs`): the frozen 0.9.2 viewer (`.claude/worktrees/ts-ref-092`), `renderSlideToCanvas` at 96 dpi in headless
  Chromium. Caveat: the browser cannot see the Office `CloudFonts` cache (memory fact), so the TS draws **Aptos** with a substitute while
  PowerPoint and Rust `system` use Aptos — the samples are mostly Aptos (the playback decks), which lowers TS × PowerPoint there.
- **Comparison** (`pdf-oracle-compare.py`, pypdfium2 for both PDFs): pixels at 96 dpi on white; a pixel **differs** when no pixel of the
  other image within 1 px is within 40 on every channel, both ways (antialiasing/hinting do not count); agreement = 1 − differing/all. Each
  differing Rust × PowerPoint pixel is split by the TS canvas (legacy / PDF only / all three differ) and attributed to the topmost page object
  of each PDF. Text: characters in content order (similarity), each PowerPoint glyph against the nearest Rust glyph of the same character
  (within 1 pt / 3 pt), signed offsets, font names. Links: count, URI, rectangle IoU. Then `pdf-oracle-classify.py` tags each page with known
  causes (detectors in its header) and attributes each page's differing pixels to the first matching cause (priority order of the table
  below); `pdf-oracle-baseline.py` gives the vertical glyph offset per font.

### Numbers

| Pages | Rust `system` × PowerPoint | Rust `open` × PowerPoint | TS × PowerPoint | Rust × TS | text similarity | glyphs within 1 pt / 3 pt |
|---|---|---|---|---|---|---|
| committed samples (601) | mean **0.9947**, median 0.9961, **98.2% of pages ≥ 0.99** | 0.9908, 0.9914, 67.6% | 0.9905, 0.9882, 44.8% | 0.9908 | 0.982 (93.5% of pages ≥ 0.99) | 21% / 93% |
| generated (135) | 0.7207, 0.9071, 3.7% | 0.7207, 0.9028, 3.0% | 0.8057, 0.9288 | 0.8751 | 0.752 | 16% / 24% |
| all (736) | 0.9445, 0.9951, 80.8% | 0.9413, 0.9904, 55.7% | 0.9566, 0.9870 | 0.9696 | 0.940 | 20% / 80% |
| **after the PDF fixes** (2026-11-18): samples | mean **0.9964**, median 0.9961, **98.5%** | 0.9925, 0.9914, 67.9% | (unchanged) | 0.9925 | (unchanged) | (unchanged) |
| generated | **0.8070**, 0.9319, 5.2% | 0.8069, 0.9297, 4.4% | | 0.9843 | | |
| all | **0.9616**, 0.9951, 81.4% | 0.9585, 0.9904, 56.2% | | **0.9910** | | |

Without the two whole-page causes (gradient text, `sysClr`) the 650 remaining pages agree at mean 0.9881, median 0.9955 (after the
fixes: without `sysClr`, 656 pages at 0.9877, 0.9955; on the generated pages Rust now agrees with PowerPoint as well as the TS does,
0.8070 against 0.8057). On the samples
the Rust PDF agrees with PowerPoint **better than the TS viewer does** (Aptos read in place); with the open fonts it is about the TS's level.
Horizontal glyph placement agrees (median |dx| ≤ 0.08 pt for every font); the 21% "within 1 pt" is the vertical offset of item 3 below.
Differing pixels over all pages: 30.86 M of 642 M (4.8%): **TS legacy 18.7 M (61%)**, **PDF only 9.8 M (32%)**, all three 2.4 M. By Rust
object under the pixel: shading 13.5 M (the gradient text), path 9.2 M, text 5.2 M, form 1.6 M, image 0.7 M, shadow image 0.6 M.
**After the PDF fixes**: 21.82 M (3.4%): TS legacy 20.7 M (95%; it grew by the legacy differences the gradient floods had hidden),
**PDF only 0.86 M (4%)**, all three 0.23 M; by Rust object: path 11.0 M, text 5.3 M, shading 3.1 M (gradients that are really there),
image 1.0 M, form 0.8 M, shadow 0.5 M. The largest PDF-only pages left are `b3a` text (glyphs of other fallback faces than the
browser's) and `b6a_0073` p1 (0.28, a mostly legacy page).

### Differences, ranked by impact (`results/classes.tsv`: exclusive differing pixels, first matching cause owns a page)

| # | Difference | Pages / decks | Exclusive px | Split | Who decides |
|---|---|---|---|---|---|
| 1 | **Fixed 2026-11-18.** **Gradient-filled text** was written as a text clip (`7 Tr … ET`) followed by `sh`; pdfium (this harness, and the Chrome/Edge PDF viewers) ignored the text clip and painted the shading over the whole clip region — **the slide disappears under a gradient** (`b1a_0067` p2, `s_stress-text` p4 at 0.03, `b6a_0073`, `c1d_0027`). Acrobat not checked. | 29 / 22 (2,940 such fills over the 627 Rust PDFs, 206 decks) → **0** | 12.4 M (40%) → 0 | PDF only 8.4 M → 0 | **Rust PDF** (fixed: the cause was the `0 Tr` reset before `ET`, and empty clips; see "PDF fixes". `s_stress-text` p4 0.031 → 0.999, `b1a_0067` p2 0.002 → 0.965, `c1d_0027` p2 0.13 → 0.95) |
| 2 | **`a:sysClr`**: the TS (and Rust) take `lastClr`; PowerPoint takes the system colour (`windowText` black, `highlight` the Windows accent) — a whole background black instead of light (`b6a_0021` 0.07), gradients of another hue (`b3a_0090`) | 80 / 51 (all generated: random `lastClr`) | 12.6 M | legacy 11.7 M | **PowerPoint** (candidate: resolve `sysClr` to the system value; measure the table; real decks usually carry the system value in `lastClr`, so the real-world effect is small) |
| 3 | **First-line baseline per font**: Rust (the TS layout rule) puts text lower than PowerPoint by **0.036–0.038 em for Aptos**, 0.017 em Calibri, 0.019 Calibri Bold, 0.014 Cambria Bold; ≈ 0 for Liberation Serif, Caladea, Times New Roman (`results/baseline.tsv`; ≈ 1.07 pt on 28-pt Aptos titles, ≈ 4 pt on the 100-pt numbers of the playback decks) | 665 pages carry an offset > 0.3 pt | 2.3 M | legacy 1.8 M (TS = Rust by construction: same layout, same measures) | **PowerPoint** (candidate: the ascent metric PowerPoint uses for the first line — hhea vs OS/2 typo/win; measure with the synthetic font groups) |
| 4 | **Characters the run's face lacks** (CJK, Thai, Devanagari, Arabic, emoji): PowerPoint links Yu Gothic, MS Gothic, Malgun Gothic, Angsana New, Mangal; Rust uses the Word fallback chain (MS Mincho, Leelawadee UI, …) → other widths, other line breaks (`b3a`) | 88 / 53 | 2.1 M | legacy 1.8 M | **PowerPoint** (candidate: a per-script fallback table of PowerPoint — the theme's `a:font script=…` list and Office's defaults — instead of Word's) |
| 5 | **WordArt warps**: arch/curve geometry differs (`s_legacy_lo-wordart-test` p3) | 33 / 28 | 0.75 M | legacy 0.62 M, PDF only 0.12 M (outline flattening vs the TS raster strips) | PowerPoint for the geometry; the PDF part is small |
| 6 | **Date/time fields** in the system locale: PowerPoint writes `17/11/2026` (pt-BR), Rust/TS `11/17/2026` (`b1a_0186`) | 90 / 59 | 0.28 M | legacy | **PowerPoint** (candidate: the field's format follows the run language / PowerPoint's locale rule; measure) |
| 7 | **Charts**: plot-area insets and doughnut radius/hole (`s_showcase_animado` p5 0.96, `pb-graficos*` 3 px plot-area shift) | 31 / 16 | 0.18 M | legacy (uo-charts = TS) | **PowerPoint** (chart layout facts) |
| 8 | **`.ppt` charts drawn from the fallback picture** the converter keeps (PowerPoint draws the embedded chart natively: sharper text, lighter gridlines) | 12 / 3 | 73 k | legacy (B2 converter = TS) | PowerPoint (convert the OLE chart) |
| 9 | **Hyperlinks**: on the 82 pages with links PowerPoint writes 4,043 link rectangles, **2,741 of them wholly outside the page** (text overflowing the slide: PowerPoint keeps the annotation beyond the media box) and 1,302 on it. First measured with the in-progress frontier: Rust 2,296, IoU > 0.3 for 331. **Now (wave 4 as committed, 2026-11-18): Rust 181, all on the page** (pieces joined per line fragment, off-page pieces dropped by design: `links_off_page`); 15 pages where PowerPoint has links and Rust none (`b1a_0003`, `b1a_0146` p3–4, `b3a_0045`, `b3a_0090`…: their linked text is off the page); same URIs (152); 35 of PowerPoint's rectangles matched at IoU > 0.3 (36 of its 1,302 on-page ones) | 82 | (no pixels) | — | Rust: PowerPoint writes about 7 on-page rectangles per Rust one (per run or word, not per joined fragment) and keeps off-page ones — measure its rule before changing the join |
| 10 | **Fixed 2026-11-18.** **SVG picture** not drawn (`image:unsupported`, `pb-365.pptx` p1: the check icon missing; the deck has only the SVG, no PNG fallback) | 1 / 1 → 0 | 40 k → 0 | PDF only (the browser draws the SVG) | Rust PDF (fixed: the SVG is played as vector content, `pdf/svg.rs`; p1 0.957 → 0.9998) |

Smaller, not in the pixel ranking: **hidden slides** (5 decks) are pages of the Rust PDF but PowerPoint's Save as PDF leaves them out by
default (the harness shows them to keep page i = slide i) — a policy choice; **PDF font names**: Rust writes family names (`Arial`,
`Caladea`) and `Fallback<N>` for fallback faces where PowerPoint writes PostScript names (`ArialMT`, `Calibri-Bold`) — cosmetic (preflight,
accessibility); 43 pages with no detected cause hold 0.24 M px; the TS × PowerPoint gap on Aptos text is the browser's font visibility, not
an engine rule. Gradients themselves agree (the linear-RGB interpolation of the TS matches PowerPoint within 3 levels on `gaps-smoke`).

**Candidate fixes where PowerPoint should win (TS legacy, behind `ts_*` switches as the Write port did):** 2 (`sysClr`), 3 (first-line
baseline metric), 4 (PowerPoint's per-script fallback), 6 (field date format), 7 (chart layout), 8 (`.ppt` charts), 5 (warp geometry) —
each needs its rule measured first in PowerPoint with synthetic oracle decks. **Rust PDF fixes (no TS question):** 1 (gradient text: the
biggest single item, 40% of all differing pixels — done), 10 (SVG — done), 9 (link rectangles: PowerPoint's rule to measure first). For the gate, a PowerPoint-valid generator mode (decks
PowerPoint opens) would let the generated groups count; today only 15% of them do.

### PDF fixes (items 1 and 10, 2026-11-18)

One Opus session. Rust side only: the PowerPoint PDFs and the TS canvases are the cached ones; the Rust PDFs of both font policies and the
comparison were rerun over the same 627 decks / 736 pages (numbers above).

- **Gradient text** (`pdf/text.rs` `glyph_block`, `fill_glyphs`). The cause was not the text clip itself: pdfium's content parser adds
  the accumulated text clip at `ET` **only when the text render mode in force there is a clip mode** (`Handle_EndText`), and the painter
  reset `0 Tr` just before `ET` (legal PDF: the mode that counts is the one at `TJ`; pdf.js and Acrobat apply the clip). A clip block now
  keeps its mode until `ET` (the `Q` of `fill_spec` restores it). Second case, found by the rerun: a gradient run whose glyphs no face has
  wrote an **empty** text clip (`BT 7 Tr … ET` without `TJ`), which pdfium also takes as no clip — the shading flooded the page (17 runs,
  6 decks, `b6a_0073` p1); such a run now paints nothing (its characters are counted `glyph:missing` as before). The text stays real text
  (extractable: pypdfium2's text page and the ToUnicode test). The shading-pattern form (`/Pattern cs` + `PatternType 2`) was not needed:
  the clip form is honoured by pdfium, pdf.js and Acrobat, and keeps the stop-alpha soft mask and the shading cache as they were. Over the
  627 oracle PDFs: 2,940 gradient fills with the reset → 0 (2,923 fills left, the 17 empty ones dropped). `pdf-oracle-classify.py`'s
  `gradient-text` detector now tags only the broken form (none left).
- **SVG pictures** (`pdf/svg.rs`, new; `images.rs` `embed_picture` sends SVG bytes there). The reader already takes the PNG of
  `r:embed` when PowerPoint wrote one beside the `asvg:svgBlip` (2016+ files do); `pb-365.pptx` has **only** the SVG, so no raster fallback
  existed and the SVG is now played as vector content: a form XObject (transparency group) of the picture's natural size, placed by
  `drawImage` like a metafile recipe. Subset (the static SVG of Office icons and the usual producers): `svg`/`g`/`a`/`switch`/`use`/`symbol`,
  nested viewports, the seven shapes and the whole path grammar (arcs as cubics), presentation attributes, `style` and `<style>` sheets
  (type, class and id selectors by specificity), fill/stroke with opacity, rules, caps, joins, miter, dashes, `opacity` as a group,
  transforms, `clip-path`, linear/radial gradients (`href` chains, both units, `gradientTransform`, stop opacity as a luminosity soft mask),
  the CSS colour names. Counted, not drawn: `svg:text`, `svg:image`, `svg:pattern`, `svg:marker`; drawn without the effect: `svg:filter`,
  `svg:mask`, `svg:spread`, `svg:effect` (an `fxImage` recolour); `svg:limit` (1 M segments, 64 MB of content, depth 64, 200,000
  elements). The raster interpreter has no SVG player: a soft-edge/reflection layer holding an SVG is drawn as vector (`raster:fallback`) and
  an SVG picture's canvas shadow is its box (`svg:shadow`). `PdfReport::svgs` counts the pictures played. Declared: the natural size is
  `width`/`height`, else the viewBox, else 300 x 150, and `drawImage` stretches that box (a browser fits an SVG without intrinsic size into
  the destination: the two differ only when the ratios differ). `pb-365` p1: 0.957 → 0.9998 (0 PDF-only pixels); `image:unsupported` 1 → 0.
- **Write PDF**: unaffected. `uo-pdf` was not edited; Write's writer never emits a text clip (`2 Tr` is its faux bold; its shadings are path
  clips, `W n /Sh sh`). `cargo test -p uo-pdf --release` green (parity, unit, memory, no-panic).
- **Tests** (`tests/pdf_slides.rs`): gradient text keeps `7 Tr` until `ET`, is followed by the shading and extracts; a run without
  glyphs writes no clip and no shading; a constructed SVG (sheet class and id rules, a gradient with a fading stop, a clip, `use`, group
  opacity, a named colour, `text` counted) reaches the PDF as vector content; hostile SVGs (unparsable, 200 nested transforms, NaN/1e308
  arcs, `use` and `href` cycles, bad dashes and points) never panic and stay valid; the `pb-365` icon is drawn; 5 unit tests in `svg.rs`
  (sniffing, path grammar, transforms, aspect ratio, colours and sheets). The 538-deck test is unchanged (994 links, 0 failed slides). The
  Slides digests of `uo-ffi/tests/parity/slides-native.tsv` change (19 of 93 differed on the tree of 2026-11-18, other frontiers' edits
  included): `UO_BLESS=1`.

### Files

`core/crates/uo-pptx/tests/oracle_ppt/` (`main.rs`, `decks.tsv`, `README.md`, `results/{pages,decks,classes,features,baseline}.tsv`,
`results/ppt-pdfs.tsv`, `results/summary.json`); `slides/tools/ppt-lab/` (`export-pdf.ps1`, `pdf-oracle.mjs`, `pdf-oracle-ts.mjs`,
`pdf-oracle-compare.py`, `pdf-oracle-classify.py`, `pdf-oracle-baseline.py`). Work files (PDFs, PNGs, the side-by-side diff pictures
`diff/<id>-<page>.png`: PowerPoint | Rust / TS | differing pixels, red = TS legacy, blue = PDF only, magenta = all three) stay in
`core/target/w4-oracle/ppt-oracle`. Run time: PowerPoint ≈ 25 min for the 627 decks (most refusals are fast), Rust 108 s, TS ≈ 3 min,
comparison ≈ 50 min (736 pages).
