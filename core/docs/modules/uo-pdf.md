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

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (PQ1–PQ12). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.
