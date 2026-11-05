# uo-bindings — milestone 1: the read path in the browser (uo-wasm) and in .NET (uo-ffi + C# facade)

Record of the rust-port workflow (hybrid mode, `SKILL.md` "Hybrid mode"). One Opus session, 2026-11-05, as the
architect and implementer of the milestone-1 bindings, including uo-pdf's **frontier S (server path)**. Status:
**designed and implemented to a tested boundary; maintainer gate pending** (questions in "For the milestone-1 gate").

The read path exposed here: docx bytes → uo-docx model (D1a) → uo-docx-layout (F0–F8) → uo-render pages → uo-pdf
(P). Nothing of the editing session (milestone 2) or of the D3 Editing API crosses yet; the ABI conventions are built
so D3's exports join them.

## Scope

| Binding | Consumer | What crosses | Code |
|---|---|---|---|
| `uo-wasm` feature `write` | the TS viewer (`<universal-document-viewer>`), later the editor shell | environment (fonts, hyphenation), document, layout handles; page messages for the canvas painter; navigation, run boxes, block lines; PDF in chunks | `crates/uo-wasm/src/write.rs`, `js/write.mjs`, `js/write.d.ts` |
| shared read path | both bindings | open, layout (+ embedded fonts, view transform, navigation, run boxes), pages out of a layout, PDF options per trust, error codes, the last-error slot | `crates/uo-ffi/src/pipeline.rs` (uo-wasm includes it with `#[path]`) |
| `uo-ffi` | the .NET facade | converter, output buffer, cancellation flag; `uo_docx_to_pdf` | `crates/uo-ffi/src/{lib,server}.rs` |
| C# facade | clickCompliance, .NET hosts | `DocxPdfConverter`, `DocxLayoutPipeline.ToPdf`, `DocxPdf.ToPdf`, exceptions | `core/dotnet/UniversalOffice.Docx` (+ `.Tests`, `.Smoke`) |

Inputs read: `uo-pdf.md` (§3, §7, §9, frontier P "What S and the bindings need next"), `uo-render.md` (frontier E: the
render view and paint exports, "the layout handle does not exist yet"), `uo-docx-layout.md` (LA5, LA6, §3, F8 and its
`PENDING = ["encode_page"]`), `uo-fonts.md` (registry, policies), `uo-docx-d3.md` §4 (C ABI design), the frozen C#
`DocxLayoutPipeline`, `SubstituteFontProvider`, `SystemFontProvider`, `DocxPdf`, `PdfRenderOptions`, `LayoutOptions`.

## 1. The shared read path (`pipeline.rs`)

```rust
pub fn open(bytes: Vec<u8>, read: &ReadOptions) -> Result<Opened /* doc, embedded fonts */, JobError>;
pub struct LayoutJob<'a> { provider: Arc<dyn FontProvider>, cache: &FaceCache, ledger: Option<&Arc<FaceLedger>>,
    hyphenation: &Arc<HyphenationRegistry>, options: &LayoutOptions, cancel: Option<&AtomicBool>, run_boxes: bool }
pub fn lay_out(doc: &Opened, job: &LayoutJob) -> Result<LaidOut /* layout, navigation, links, inlines */, JobError>;
pub fn take_pages(layout: &mut Layout) -> Vec<Page>;          // the display list leaves; the Write geometry stays
pub fn pdf_options(compress: bool, trust: Trust, cancel: Option<Arc<AtomicBool>>) -> PdfOptions;
pub enum JobError { Read(DocxError), Layout(LayoutError), Pdf(PdfError), Cancelled, Argument(&str), Other { .. } }
```

- `lay_out` = `withEmbeddedFonts(provider, embedded)` (uo-docx-layout `EmbeddedFontProvider::wrap`, unreadable
  embedded fonts are none — the TS/C# `catch`) → `view_transform::transformed` → `derive::build` → `engine::layout_document`,
  then `Layout::navigation` (and `run_boxes` for the viewer) **over the same view**, so the result owns no borrow of the
  model: a layout outlives its document handle.
- `Trust::Untrusted` lowers the caps uo-pdf frontier P asked for: blur kernel radius 256 (default 4,096), masks and
  text-effect layers 16 Mpx, decoded images 40 Mpx. The server converts user uploads: untrusted is its default.

## 2. The wasm API (feature `write`)

### Handles and exports

| Handle | Created by | Released by | Owns |
|---|---|---|---|
| `UoWriteEnv` | `uo_write_env_new(profile)` (0 desktop, 1 phone) | `uo_write_env_free` | `FontRegistry` (profile budget, the instance `FaceCache`), `HyphenationRegistry` |
| `UoWriteDocument` | `uo_write_open(ptr, len)` | `uo_write_document_free` | model + embedded fonts |
| `UoLayout` | `uo_write_layout(doc, env, now_ms, tz_offset_min, flags, web_width_tw, lang, hyph_langs, max_pages, generation)` | `uo_layout_free` | a uo-render `RenderView` (pages + resources), the Write geometry (`Layout` minus its pages), navigation, run boxes |
| (borrowed) `UoRenderView` | `uo_layout_view(layout)` | never (dies with the layout) | — |
| `UoBuf` (render.rs) | every query | `uo_buf_free` | bytes + status |

Environment: `uo_write_env_push_family(env, family, source, embeddable, faces)` (the viewer's
`resolver.getWithFile`: source host/localAccess/bundled/page/installed/substitute/default, licence flag, faces packed as
`variant u8, ttc i32, len u32, bytes`), `uo_write_env_push_asset(env, family, variant, bytes, advances_json)` (hosted
assets; the `*-advances.json` table for the PDF, PQ1), `uo_write_env_clear_fonts`, `uo_write_env_take_missing` (the TS
`loadMissing` selection), `uo_write_env_register_hyphenation(env, text)`.

Document: `uo_write_hyphenation_patterns(doc, env, app_langs)` = the viewer's `hyphenationLangs(full)`
(`documentHyphenationLanguages(full)` filtered by the app list, prefix match) → pattern ids **not yet registered**, so
the host fetches only those.

Layout: `uo_layout_page_count`, `uo_layout_page_info` (w, h, section, number, hf_items), `uo_layout_view` (then every
`uo_render_*` paint export of frontier E: `encode_page`, `fx_layers`, `pattern`, `rect_gradient`, `blip_tiles`,
`page_fill_*`, `picture_*`, `set_max_fx_px`), `uo_layout_navigation` (links in points + outline = `pdfNavigation`),
`uo_layout_run_boxes(filter)` (links / plugin inlines = `runBoxes`, twips), `uo_layout_block_line(block)` (the
viewer's outline jump), `uo_layout_warnings`, `uo_layout_missing_faces`, `uo_layout_face_bytes(face)` (the document's
embedded fonts exist only in wasm: the painter makes its `FontFace` from these), `uo_layout_image_bytes(image)`,
`uo_layout_pdf(layout, flags, chunk)`. Result layouts are in the module doc of `src/write.rs`.

Layout flags: hyphenation on/off/force (bits 0–1), view print/web/draft (2–3), revisions default/final/simple/all/
original (4–6), `hyphenationLanguages` given (8). `generation` stamps the page messages (0 = a module counter); the
tests pass 1 so the message bytes are reproducible.

### Font bytes flow (`viewer` policy)

The host decides where bytes come from, exactly as the TS viewer's `FontResolver` (`shared/fonts`): Local Font Access
(Chromium, with permission), the page's `@font-face` files, the open substitutes of `@universal-office/font-files`, the
hosted assets. It pushes them; the `viewer` policy (uo-fonts, port of `fonts.ts:90–137`) answers the engine per layout
call; a non-embeddable licence answers with the metric substitute. After a layout, `takeMissing()` lists what the engine
asked for and nobody had; the host fetches and pushes them and lays out again (the TS loop, at most 4 passes). Embedded
document fonts never leave wasm except through `faceBytes(i)` for the canvas. The face cache is the instance's
(`FaceCache::shared()`); the env's ledger is the document budget (desktop/phone profile, uo-fonts D4).

### Errors and limits

A call that creates a handle returns null and sets the **last error** (`uo_last_error(UoErrorInfo*)` →
`{code, stage, max, message_len}` written unaligned, `uo_last_error_message(buf, cap)`, `uo_last_error_clear()`); a query
returns a `UoBuf` status (render.rs space: 0 ok, 1 none, 5 bad argument, 6 no page/item, 8 missing resource, 11 null
handle); `uo_layout_pdf` returns the byte count or `-code`. Limits: uo-opc/uo-docx reader limits (`ReadOptions::default`,
wasm32 defaults), `LayoutLimits` (`max_pages` by argument), `PdfLimits::WASM32` (256 MiB output), `PaintLimits` per view
(phone: fx layers capped at 2^22 px), untrusted caps with `PDF_UNTRUSTED`.

### The PDF in chunks

`uo_layout_pdf` writes through uo-pdf's `ChunkedSink`: every chunk (1 MiB by default, ≥ 1 KiB) goes to the host import
`uo_host.pdf_chunk(ptr, len) -> i32`, which copies and answers 0 (anything else stops the file with code 13). The module
holds at most one chunk of output, so its memory (which never shrinks) does not grow with the file. The native build
replaces the import by a per-thread callback (`write::host::set_pdf_sink`, tests).

### The JS wrapper (`js/write.mjs`, `js/write.d.ts`)

`writeImports()` → `{ imports, host }` for `WebAssembly.instantiate`; `bindWrite(exports, host)` → `newEnv()`, `open()`;
classes `WriteEnv`, `WriteDocument`, `WriteLayout` with `free()` and a `FinalizationRegistry` each; `WriteError { code,
stage, max, detail }`; `layout.paint` = the frontier-E operations on the borrowed view; `layout.pdf()` →
`{ chunks, length }` (`new Blob(chunks)` in a browser). It reuses `render.mjs` (`Buf`, `put`).

### The TS/Rust switch of the viewer (plan; frontier D builds it)

- One module per page (`write` build), one `WriteEnv` per viewer element. A viewer option (`engine="rust"`, default
  `ts` until the milestone gate) picks the path; the TS path stays intact behind it until 0.10 ships ("plug in at once":
  the whole `layoutDocument` path switches, not stage by stage).
- `relayout()`: `fonts.loadAssets` → `pushAsset`; `doc.hyphenationPatterns(env)` → fetch + `registerHyphenation`; up to
  four passes of `doc.layout(env, LAYOUT_OPTS)` + `env.takeMissing()` → resolver → `pushFamily`.
- Painting: `layout.encodePage(i)` → generated `js/gen/render.ts` `decodePage` → the TS painter of frontier D (hints,
  `paint.*` rasters), `faceBytes(i)` → `FontFace`; link overlays from `runBoxes('links')`, outline from
  `navigation()`/`blockLine()`; print and download from `layout.pdf()`.
- Missing before the switch: the **Write geometry section** of the page message (`Layout::encode_page`, the text layer,
  find, `collectAnnotations`) — gate item G8; frontier D (the TS shell painting decoded pages) and V (painter parity).

## 3. The C ABI (`uo-ffi`)

- **Handles**: `UoConverter` (`uo_converter_new(const UoConverterOptions*, UoConverter**)`, `uo_converter_free`,
  `uo_converter_families`), `UoBuf` (`uo_buf_ptr`, `uo_buf_len`, `uo_buf_free`), `UoCancel` (`uo_cancel_new`,
  `uo_cancel_raise`, `uo_cancel_free`). Released only by their `*_free`; null is the `ARGUMENT` code, `*_free(null)` a no-op.
- **Status codes** (`pipeline::code`, shared with wasm): 0 ok; 1–9 = D3 §4 (1 argument, 2 invalid operation, 3 invalid
  data, 4 XML, 5 limit exceeded, 6 format, 7 overflow, 8 JSON, 9 out of range); **10 cancelled, 11 no font, 12 internal
  (a caught panic, a broken invariant), 13 sink**. Details per thread: `uo_last_error`, `uo_last_error_message`,
  `uo_last_error_clear` (every export clears the slot first; the message is dropped, not kept, so a read slot holds no memory).
- **Options structs** start with their size (`UoConverterOptions { size, flags, font_dirs, font_dirs_len }`, `UoPdfJob
  { size, flags, now_ms, tz_offset_min, revisions, max_pages, reserved, lang, lang_len, hyph_langs, hyph_langs_len }`): a
  newer facade may pass a longer struct, a shorter one is refused. Strings are UTF-8 `(ptr, len)`, lists `'\n'`-separated.
- **Buffers**: inputs are borrowed for the call only; the PDF comes back as a `UoBuf` the facade copies into a `byte[]`
  and frees at once (the SafeHandle's `Dispose`).
- **Threads**: `UoConverter` is `Send + Sync` and shared by every thread (the system index and the face cache are locked
  inside; a font registry and its ledger are made per conversion, so documents do not share a font budget). `UoCancel`
  is raised from any thread; a job holds its own `Arc` of the flag.
- **Cancellation**: checked before the work, between read, layout and PDF, and by uo-pdf before each page and every 256
  items (PQ9). Inside the layout the work budget bounds the time (G4).
- **Panics**: `catch_unwind` in every export → code 12. Native only: on wasm32 the crate is empty (`#![cfg]`).
- `uo_docx_to_pdf(conv, data, len, job, cancel, &out)`: `.doc` (Word 97-2003, uo-docx D4) is converted first, as the C#
  `DocxPdf.ToPdf` did; then `server-substitute` over the converter's index, all seven hyphenation pattern sets compiled in
  (`hyphen-data`, parsed once per process) filtered by `HyphenationLanguages`, navigation, PDF (`Vec` sink).
- Diagnostics: `uo_version`, `uo_memory_counting`, `uo_memory_live_bytes`, `uo_clear_caches`.

## 4. The C# facade (`core/dotnet/UniversalOffice.Docx`)

```csharp
namespace UniversalOffice.DocxLayout;
public sealed class DocxPdfConverter : IDisposable {               // one per host, thread-safe
    public static DocxPdfConverter Shared { get; }                  // machine fonts + substitutes, untrusted
    public DocxPdfConverter(DocxPdfConverterOptions o);             // SystemFonts, FontDirectories, TrustedInput
    public IReadOnlyList<string> Families { get; }
    public byte[] ToPdf(byte[] docx, LayoutOptions? opts = null, PdfRenderOptions? pdf = null, CancellationToken ct = default);
}
public static class DocxLayoutPipeline { public static byte[] ToPdf(byte[] docx, DocxPdfConverter fonts, LayoutOptions?, PdfRenderOptions?, CancellationToken); }
public static class SubstituteFontProvider { public static DocxPdfConverter Shared { get; } }   // the 0.9 name
public sealed class LayoutOptions { Lang, Now, UtcOffset, Hyphenation, HyphenationLanguages, Revisions, MaxPages }
public sealed class RevisionView { string Mode }   public sealed class PdfRenderOptions { bool? Compress }
namespace UniversalOffice.Export;  public static class DocxPdf { ToPdf(byte[]); ToPdf(byte[], DocxPdfConverter, …) }
namespace UniversalOffice;  UniversalOfficeException { Code, Stage }, OfficeLimitExceededException { Maximum }
namespace UniversalOffice.Native;  public static class NativeCore { Version, IsCountingMemory, LiveBytes, ClearCaches() }
```

- clickCompliance's call `DocxLayoutPipeline.ToPdf(docx, SubstituteFontProvider.Shared, new LayoutOptions {
  HyphenationLanguages = ["pt-BR", "en", "es"] })` compiles unchanged (tested); its `PdfConversionQueue` can pass its
  request-timeout token.
- Interop: `LibraryImport` source-generated P/Invoke; `ConverterHandle`, `BufferHandle`, `CancelHandle` are
  `SafeHandle`s (`ReleaseHandle` → the `*_free`); the token registration raises the native flag and is disposed before
  the flag. Error mapping: 1 → `ArgumentException`, 3/4 → `InvalidDataException`, 5 → `OfficeLimitExceededException`
  (stage, maximum), 10 → `OperationCanceledException`, others → `UniversalOfficeException`.
- `Now`: a `Utc` value is shown in local time; a `Local`/`Unspecified` value is the local wall clock; the offset is the
  machine's at that instant or `UtcOffset` (a fixed offset per conversion, as the TS reads local getters of one `Date`).
- **Packaging**: `runtimes/<rid>/native/{uo_ffi.dll | libuo_ffi.so | libuo_ffi.dylib}` from `UoNativeRoot`
  (default `core/target/native-pack/<rid>/`), gathered by a CI matrix before `dotnet pack` (not built yet: G14). A
  development build copies the library cargo built (`UoNativeLib`, or the test/smoke projects' build target).

### API breaks (for `CHANGELOG.md`, .NET)

1. `DocxLayoutPipeline.ToPdf(byte[], IFontProvider, LayoutOptions?, PdfRenderOptions?)` → `ToPdf(byte[], DocxPdfConverter,
   LayoutOptions?, PdfRenderOptions?, CancellationToken)`; `SubstituteFontProvider.Shared` is now a `DocxPdfConverter`
   (the clickCompliance call is source compatible).
2. Removed: `IFontProvider`, `FontFace`, `SystemFontProvider` (and its `Shared`, `Index`, `HasCoreFonts`), the public
   `SystemFontIndex`, `PdfFontSource`, `DocxPdf.FixedFontProvider`, `DocxPdf.ToPdf(byte[], List<PdfFontSource>)`,
   `DocxPdf.ToPdf(byte[], IFontProvider, …)`. Font folders are `DocxPdfConverterOptions.FontDirectories`.
3. Not exposed in 0.10 milestone 1: `DocxLayoutPipeline.Layout`, `LayoutResult`, `LayoutEngine`, `PdfRenderer`,
   `PdfNavigation`, `RunBoxes` (no layout crosses the ABI).
4. `LayoutOptions`: `AdvanceEm`, `AdvanceRound`, `JustifyShrink`, `OnWordFit` removed (lab only, LQ12); new
   `UtcOffset`, `MaxPages`; `Revisions` keeps `Mode` only (`InsDel`, `Formatting`, `Authors`, `Colors`, `Order` do not
   cross yet).
5. `PdfRenderOptions.DecodeImage` (PQ6) and `.Navigation` removed: links and the outline always come from the document.
6. Exceptions: `UniversalOffice.OfficeLimitExceededException` now derives from the new `UniversalOfficeException` and
   carries `Stage` and `Maximum` (no `OfficeLimit` enum, no `PartName`), for every limit (package, reader, layout, PDF);
   damaged input is `InvalidDataException`; cancellation is `OperationCanceledException`.
7. New: `DocxPdfConverter` (thread-safe, `IDisposable`), `DocxPdfConverterOptions` (`TrustedInput` false by default:
   lower effect and image caps), `CancellationToken` everywhere, `NativeCore`.
8. The package carries native libraries per RID: it is no longer "pure .NET".
9. Output: the server PDFs equal the TS `renderPdf` (not the C# port): deflate is miniz_oxide (C# `ZLibStream` Fastest
   before), and wherever the C# port diverged from the TS the bytes change.

wasm/npm: new API with no 0.9 counterpart (it replaces, at the switch, the viewer's direct calls): `layoutDocument(full,
provider, opts)` → `doc.layout(env, opts)` with fonts pushed to an environment instead of a provider callback;
`renderPdf(layout, { navigation })` → `layout.pdf()` (chunks); `runBoxes(full, layout, pick)` → `layout.runBoxes(kind)`;
`pdfNavigation` → `layout.navigation()`; `LayoutResult` pages → encoded messages.

## 5. Parity

| Comparison | Corpus | Result |
|---|---|---|
| **Rust server path = frozen TS** (`renderPdf(layoutDocument(full, withEmbeddedFonts(nodeFonts)), { compress: false, navigation: pdfNavigation })`), byte for byte | the 68 committed layout fixtures (`uo-docx-layout/tests/captures/f7a..f8/docs`, open fonts), TS d0f45bc5 captured with uo-pdf's harness (`capture_painter.mjs --docs --open-fonts`) into `uo-ffi/tests/parity/server-<family>.cap` | **68/68 with the `oracle` policy and 68/68 with the shipped `server-substitute`** (43.4 MB of uncompressed PDF) |
| same, installed fonts (local, opt-in `UO_SERVER_ORACLE_DIR`) | the Word oracle (`write/packages/docx-layout/test/oracle`), every 13th group: 19 groups, **211 documents**, Office fonts | **211/211** equal |
| C# facade = Rust native, compressed (shipped options) | the 68 fixtures (`server-native.tsv`, blessed by the Rust test) | **68/68** |
| C# facade = TS, uncompressed | the 68 fixtures | **68/68** |
| C# facade = Rust native, installed fonts (opt-in) | the 211 oracle documents | **211/211** |
| wasm = native | 3 fixture documents: page messages, navigation, link boxes, PDF compressed and not (FNV-1a pinned by `write_native.rs`, checked by `write_api.test.mjs`) | equal in **Node 24.15 and WebKit 26.5** |

This is the first **end-to-end** PDF parity (PP1 of uo-pdf §6): Rust read → layout → navigation → PDF against the TS
pipeline, not the painter alone. Zero mismatches on the first run; no port bug found.

**Why not a byte comparison with the frozen C# PDF**: the C# is a second port, not the behaviour source; it deflates with
`ZLibStream(Fastest)` (uo-opc G2: decompressed parity only) and its engine may diverge from the TS where its parity was
never closed. The record of uo-pdf already lists a structural C#-vs-Rust **migration report** (not a gate) as the way to
tell clickCompliance what changes; what is compared instead is the TS (the reference) byte for byte, and the facade
against the native library byte for byte. The server-substitute policy itself has its C# parity facet in uo-fonts.

## 6. Memory (measured)

| Check | Where | Result |
|---|---|---|
| M1 native, a whole viewer session through the wasm exports (env + 5 families + open + layout + every query + PDF) | `uo-wasm/tests/write_native.rs` | 76,618,438 B allocated, peak 14,986,201 B, **0 B left** |
| M1 native, one server conversion through `uo_docx_to_pdf` (ok / invalid / cancelled / page limit) | `uo-ffi/tests/ffi_memory.rs` | ok 39,465,851 B allocated (peak 1,139,625 B, file 245,286 B); invalid 46 B; cancelled 7,984 B; limit 21,083,844 B; **0 B left each** |
| M3 server conversion (release) | same | f8__7__0 7,970 B docx → 245,286 B PDF: 39.5 MB allocated, peak 1.14 MB; f7b__7__0 → 187,564 B: 87.5 MB / 3.59 MB; f7c__7__1 → 335,884 B: 69.1 MB / 2.27 MB; f8__7__2 → 323,120 B: 136.2 MB / 3.42 MB |
| M4 C ABI (native) | same | 10,000 × (converter + family buffer + cancel flag): 2,640,000 B allocated, **0 B left** |
| M4 wasm, `free()` | `write_api.test.mjs` (Node, WebKit) | 10,000 environments, 10,000 documents, 10,000 layouts: live bytes back **exactly** |
| M4 wasm, `FinalizationRegistry` | same (Node `--expose-gc`) | the same 3 × 10,000 abandoned: counted while alive, back **exactly** |
| M2 wasm | same | 30 viewer sessions: live 5,180,964 B after each (the instance face cache), `memory.buffer` flat (19.5 MB in WebKit; 93 MB in Node after the finalizer test's 30,000 live handles) |
| M4 .NET, `Dispose()` and the finalizer | `UniversalOffice.Docx.Tests` | 10,000 converters each way: **0 B**; 200 conversions + 10,000 refused conversions with a cancellable token (a `UoCancel` + registration each): **0 B** |
| thread soak native | `ffi_memory.rs` | 4 rounds × 4 threads × 6 conversions in 2.1 s: outputs equal to the single-thread bytes, global live 10,629,883 B after every round |
| thread soak .NET | `ThreadSoakTests` | 3 rounds × 8 threads × 8 conversions in 3.6 s (`Parallel.For`): equal bytes, native live 9,061,846 B after every round |
| M5 | — | no new cache: the face cache, system index and hyphenation registry are the existing capped ones (uo-fonts); the per-call registry/ledger dies with the call |
| M6 Miri | `cargo +nightly miri test --target x86_64-unknown-linux-gnu`: `-p uo-ffi --lib` (4 tests: null handles, caught panic, refused input, cancellation) and `-p uo-wasm --features write --test write_unsafe` (every consumed buffer, null handles, the error info written to an unaligned buffer, a short message copy) | **clean, no leak reported** |
| LeakSanitizer | the existing CI job runs `cargo test --workspace --all-features` on Linux, which now includes uo-ffi's tests (sampled parity, M1/M4, soak) | CI (not run here: Windows) |

Sizes (`--profile wasm`, wasm32-unknown-unknown): default module **15,528 B** (6,553 B gzip, unchanged); the viewer
module `--features write` **5,915,444 B (1,450,639 B gzip)**; both pass `wasm-tools validate
--features=-all,wasm1,mutable-global,saturating-float-to-int,sign-extension,reference-types,multi-value,bulk-memory`
(Safari 15). The write module imports one function, `uo_host.pdf_chunk`.

Timings (this machine, contended): 136 server conversions of the fixtures in 23 s (release); the smoke console indexes
the machine's 357 families in 53 ms and converts `f8__7__1` (316,892 B PDF) in 151 ms; 211 oracle documents (2 policies,
two converters) in 3.6 s.

## 7. Security

- Every export catches panics (native) or relies on no-panic code (wasm, `panic = "abort"`); no `unwrap`, `expect`,
  `panic!` or panicking index in `src/`; inputs are length-checked (`parse_faces`, option sizes, UTF-8).
- Untrusted caps (§1) on the server by default; the reader, layout and PDF limits are the crates'.
- The host import receives a pointer into wasm memory and must copy before returning (documented; the JS wrapper does).
- No new fuzz target: the exports are thin over fuzzed crates; a `ffi_docx_to_pdf` target over the whole pipeline is a
  gate item (G21).

## For the milestone-1 gate

| # | Decision taken here (outside the existing plans) | Alternative |
|---|---|---|
| G1 | The read path is **one file** `uo-ffi/src/pipeline.rs`, included by uo-wasm with `#[path]` (no new crate; no `#[no_mangle]` export leaks from one cdylib into the other) | move it into uo-docx-layout (`pipeline` module) |
| G2 | The last error is **per thread** (`uo_last_error(UoErrorInfo*)`), not per handle as D3 §4 sketched; D3's exports adopt it | per-handle slot |
| G3 | Codes 10 cancelled, 11 no font, 12 internal (D3 said a panic is code 2), 13 sink added to D3's 1–9 | — |
| G4 | **Cancellation inside the layout is not wired**: it needs `LayoutOptions.cancel` + a `LayoutError::Cancelled` checked in `Budget::charge` (a uo-docx-layout change); milestone 1 checks between stages and inside the PDF, and the work budget bounds a layout | wire it now |
| G5 | Untrusted caps: kernel radius 256, masks/effect layers 16 Mpx, images 40 Mpx (first numbers); the facade defaults to untrusted | measure on the corpus first |
| G6 | The wasm PDF leaves through an **import** (`uo_host.pdf_chunk`): the viewer module cannot be instantiated without imports | pull the chunks (the output then stays in wasm memory, which never shrinks) |
| G7 | `write` is **not a default feature** (the default module stays the 15 KB foundation build the other tests use); the shipped viewer module is `--features write`, **5.9 MB (1.45 MB gzip)** — a size budget, `wasm-opt`, and trimming uo-docx are open | make `write` default |
| G8 | **The Write geometry section of the page message is not built** (`Layout::encode_page`, uo-docx-layout `PENDING`): the viewer's text layer, find and annotations need it; this binding exports run boxes, navigation and block lines | build it before the switch (frontier E of the layout) |
| G9 | `uo_layout_view` lends the layout's view (valid until the layout is freed) to the frontier-E paint exports | a second handle with its own copy |
| G10 | `generation` is an argument of `uo_write_layout` (0 = counter) so message bytes are reproducible | — |
| G11 | .NET `Now` + `UtcOffset`: one fixed offset per conversion (a DST change between now and a CREATEDATE is not modelled) | a time-zone table (`TimeZoneRule::Table`) from `TimeZoneInfo` |
| G12 | The facade drops the 0.9 surfaces listed in "API breaks" (items 2–6) | keep wrappers for one release |
| G13 | `DocxPdfConverter.Shared` = machine fonts, untrusted; `SubstituteFontProvider.Shared` returns it | — |
| G14 | NuGet native packaging: `runtimes/<rid>/native` from `target/native-pack/<rid>`; the RID matrix (win-x64, win-arm64, linux-x64, linux-arm64, linux-musl-x64, osx-arm64, osx-x64?) and its CI job are **not built** | Wasmtime (one `.wasm` for every RID) |
| G15 | The foundation placeholder projects `core/dotnet/UniversalOffice.Native(.Tests)` are replaced by `UniversalOffice.Docx` (named after the NuGet; D3's Editing API joins this assembly), `.Tests`, `.Smoke`; the CI `dotnet` job line changed accordingly | — |
| G16 | Phone profile: effect layers capped at 2^22 px | — |
| G17 | Server hyphenation: the seven pattern sets compiled into the native library, all "loaded", filtered by `HyphenationLanguages` (the C# behaviour) | load per document |
| G18 | `.doc` on the server path through uo-docx D4 (`doc` feature), as the C# `DocxPdf.ToPdf` | `.docx` only |
| G19 | LeakSanitizer covers the Rust tests of uo-ffi (existing job); the C# facade tests under LSan would need `liblsan` preloaded into `dotnet` | add it |
| G20 | The default debug test job samples every 7th fixture document (`UO_SERVER_PARITY_EVERY`); a new release job `server-pdf` runs all 68 | — |
| G21 | No fuzz target for the exports yet (`ffi_docx_to_pdf`, `wasm_write_session`) | add to `core/fuzz` |

## Files touched outside the owned crates (each additive)

- `crates/uo-render/src/view.rs`: `RenderView::pages(&self) -> &[Page]` (the PDF of a view reads `&[Page]`).
- `.github/workflows/rust-core.yml`: wasm job (build + Node test + validation of the write module + WebKit run of
  `write_api.test.mjs`), new `server-pdf` job, `dotnet` job runs `UniversalOffice.Docx.Tests` and the smoke (replaces the
  removed placeholder project's line), Miri: `-p uo-ffi --lib` (was the whole package: the integration tests read
  fonts) and `-p uo-wasm --features write --test write_unsafe`.
- `Cargo.lock` (new dependencies of uo-ffi and uo-wasm).
- Within owned crates, shared test infrastructure: `uo-wasm/tests/browser/run.mjs` forwards `UO_WRITE_M4`,
  `UO_WRITE_SOAK`; `uo-wasm/tests/browser/shims/assert.mjs` accepts a validation function in `throws` (Node semantics);
  `uo-wasm/src/render.rs` helpers made `pub(crate)` (`buf`, `Out`, `take_bytes`, view accessors).
- Read only: the frozen TS checkout `D:\Projects\OfficeNext\.claude\worktrees\clickcompliance-docx` (d0f45bc5, built
  `dist`) for the captures; the main checkout's `dist` is older (no `pdf-navigation.js`).

## Not done (handed on)

- The Write geometry section (G8), the TS shell (frontier D) and the switch itself; real Safari 15 / iOS (BrowserStack).
- The NuGet RID matrix and `dotnet pack` with native assets (G14); LSan on the facade tests (G19); fuzz targets (G21).
- Cancellation inside the layout (G4); a `ToPdf(Stream)` overload (uo-pdf's `IoSink` exists) for large outputs.
- The structural C# vs Rust migration report for clickCompliance (uo-pdf §6, not a gate).
- The rest of the oracle corpus with installed fonts (211 of 2,742 run; the same command over every group).
- The editor's queries (hit test, caret, selection: F8's `EditLines`) — milestone 2 with the session; the plugin-inline
  face (`custom_inline_face`) needs the page items, which the layout handle moved into the render view.

## Commands

```
export CARGO_TARGET_DIR=<target>
cargo test -p uo-ffi --release --all-features                      # UO_SERVER_PARITY_EVERY=1 for all 68; UO_FFI_SOAK=<n>
UO_SERVER_ORACLE_DIR=<dir> cargo test -p uo-ffi --release --test server_parity the_oracle -- --nocapture
cargo test -p uo-wasm --features write,count --test write_native -- --nocapture   # prints the pinned checksums
cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm
UO_WASM_PATH=<.wasm> node --expose-gc --test crates/uo-wasm/tests/write_api.test.mjs
UO_WASM_PATH=<.wasm> UO_PLAYWRIGHT=<playwright> node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/write_api.test.mjs
dotnet test dotnet/UniversalOffice.Docx.Tests -m:2 [--artifacts-path <dir>]       # UO_DOTNET_M4, UO_DOTNET_CONVERSIONS, UO_DOTNET_THREADS, UO_DOTNET_SOAK
dotnet run --project dotnet/UniversalOffice.Docx.Smoke -- <in.docx> <out.pdf> [--font-dir <d>]... [--no-system-fonts]
UO_TS_ROOT=<built d0f45bc5> node crates/uo-pdf/tests/parity_support/capture_painter.mjs --docs <dir> [--open-fonts] --dump <tmp> --out <cap>
```

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| design + wasm + uo-ffi + C# facade + parity + memory + record | one Opus session, 2026-11-05 | about 2 h 45 min wall (reading the records ≈ 30 min, builds and test runs ≈ 1 h of it) | about 120 |

Parity rounds: **0** — every comparison (68 + 211 TS, 68 + 211 facade, wasm vs native) equal on its first run.
Rework inside the session (none found by a verifier): the page-message checksum depended on a module counter (→ the
`generation` argument, G10); the last-error message kept its allocation after a failed call (→ dropped on clear, and the
wrappers clear after reading; found by M1 and by the .NET M4); the memory tests read global counters while other tests ran
(→ serialized); uo-ffi did not build for wasm32 under the workspace clippy (→ native-only `#![cfg]`); the WebKit
`assert.throws` shim lacked Node's validation-function form; two XML comments with `--` in the csproj files; one edit
script that missed a CRLF file (`uo-wasm/src/lib.rs`).
