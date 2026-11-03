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
