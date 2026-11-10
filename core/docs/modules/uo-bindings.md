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
`PENDING = []` (`encode_page` landed with G8)), `uo-fonts.md` (registry, policies), `uo-docx-d3.md` §4 (C ABI design), the frozen C#
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
| `UoLayout` | `uo_write_layout(doc, env, now_ms, tz_offset_min, flags, web_width_tw, lang, hyph_langs, max_pages, generation)` | `uo_layout_free` | a uo-render `RenderView` (pages + resources), the Write geometry (`Layout` minus its pages), navigation, run boxes, the `SectionModel` and the annotations (G8) |
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

Layout: `uo_layout_page_count`, `uo_layout_page_info` (w, h, section, number, hf_items), `uo_layout_encode_page(page)` (the page message with the Write section as its `app`, G8), `uo_layout_annotations` (`collectAnnotations`), `uo_layout_view` (then every
`uo_render_*` paint export of frontier E: `encode_page`, `fx_layers`, `pattern`, `rect_gradient`, `blip_tiles`,
`page_fill_*`, `picture_*`, `set_max_fx_px`, and the picture-layer masks of frontier D: `picture_polys`, `picture_silhouette`, `picture_bevel`,
`picture_adjust`, `outer_shadow`, `reflection_alphas`, `inner_shadow`, `glow`, `soft_edge`, `gaussian_blur` — layouts in the last section of `uo-wasm/src/render.rs`), `uo_layout_navigation` (links in points + outline = `pdfNavigation`),
`uo_layout_run_boxes(filter)` (links / plugin inlines = `runBoxes`, twips), `uo_layout_block_line(block)` (the
viewer's outline jump), `uo_layout_warnings`, `uo_layout_missing_faces`, `uo_layout_face_bytes(face)` (the document's
embedded fonts exist only in wasm: the painter makes its `FontFace` from these), `uo_layout_image_bytes(image)`,
`uo_layout_pdf(layout, flags, chunk)`. Result layouts are in the module doc of `src/write.rs`.

Layout flags: hyphenation on/off/force (bits 0–1), view print/web/draft (2–3), revisions default/final/simple/all/
original (4–6), `hyphenationLanguages` given (8), `cancelCheck` given (9, `LAYOUT_CANCEL_POLL`: the layout asks the host import `uo_host.layout_poll`, G4-W). `generation` stamps the page messages (0 = a module counter); the
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
stage, max, detail }`; `layout.paint` = the frontier-E and -D operations on the borrowed view (`Buf` results); `layout.rasters()` = their decoded facade (render.mjs `rasters`: `null` for status 1 and 4, a `RenderError` otherwise), the `RasterSource` of the TS painter `js/paint.ts`; `layout.pdf()` →
`{ chunks, length }` (`new Blob(chunks)` in a browser). It reuses `render.mjs` (`Buf`, `put`).

### The TS/Rust switch of the viewer (done 2026-11-10; plan below, built and measured after it)

- One module per page (`write` build), one `WriteEnv` per viewer element. A viewer option (`engine="rust"`, default
  `ts` until the milestone gate) picks the path; the TS path stays intact behind it until 0.10 ships ("plug in at once":
  the whole `layoutDocument` path switches, not stage by stage).
- `relayout()`: `fonts.loadAssets` → `pushAsset`; `doc.hyphenationPatterns(env)` → fetch + `registerHyphenation`; up to
  four passes of `doc.layout(env, LAYOUT_OPTS)` + `env.takeMissing()` → resolver → `pushFamily`.
- Painting: `layout.encodePage(i)` (now carrying the Write section; `paint.encodePage` is the bare render message; `layout.annotations()`) → generated `js/gen/render.ts` `decodePage` → the TS painter of frontier D (hints,
  `paint.*` rasters), `faceBytes(i)` → `FontFace`; link overlays from `runBoxes('links')`, outline from
  `navigation()`/`blockLine()`; print and download from `layout.pdf()`.
- **Frontier D and V are done** (2026-11-09; `uo-render.md` "Milestone 1 — frontier D" and "… frontier V"): `js/paint.ts` + `js/picture.ts` paint a decoded page with the same draw calls as the frozen `paintItems`/`paintPageBackground` (the glue is the snippet in the header of `paint.ts` and of `write.d.ts`: `decodePage(layout.encodePage(i), tables)`, `layout.rasters()`, `paintPageBackground` + `paintItems`). Parity: 0 mismatches in the draw-call log over the 74 committed fixture documents (replay of `v_paint.capture`, platform-independent: it passed on win32 Node 24 and on Linux Node 22 under WSL; the CI line is not in the workflow yet), 200 generated pages (every painter branch) and the whole Word oracle (2,742 documents, `UO_V_ORACLE_STRIDE=1`; the default stride 13 is 347 documents); the export bytes equal the frozen functions; Chromium: 390 renders, 1,030,732,444 pixels, 0 differing; M4 (10,000 pages) and M5 (caps) green. **Still missing before the gate:** the painter in WebKit/real Safari 15 (only the handle layer ran in WebKit), the CI lines of D/V and of the switch (listed below, not in the workflow: another frontier owns it), and the gaps of the Rust path listed under "Remaining gaps" below. (The `engine="rust"` option and its wiring are done, below; G4 cancellation is done, `cancelCheck` aborts a relayout.)
- **Content Security Policy (carried into the viewer README at the switch; the editor README when `<universal-write>` switches).** After the switch the
  engine itself is WebAssembly, so a host whose CSP restricts `script-src` must allow `'wasm-unsafe-eval'` (Chrome/Edge
  97, Firefox 102, Safari/iOS 16.0 — caniuse, checked 2026-11-06); **Safari 15, our baseline, does not know it** and
  would need `'unsafe-eval'`. Today only the optional spell checker needs it (documented in the `write` and
  `@universal-office/spellcheck` READMEs). The answer for hosts that cannot allow it is the plain-JS `wasm2js` build of
  the same module (one implementation, slower and larger): measured in "No-WebAssembly fallback (wasm2js) and wasm-opt".
  To confirm on a real Safari 15 (BrowserStack) before the gate: whether its CSP blocks WebAssembly at all.

#### As built (frontier SW, 2026-11-10)

Files: `write/packages/viewer/src/rust-engine.ts` (the viewer side: `RustSession`, one per element; dynamic-imported only for
`engine="rust"`), `src/image-decode.ts` (the bitmap decode both engines share: the old `bitmapFor` body, extracted), the element
(`universal-document-viewer.ts`: early branches; the TS lines are untouched except the type widening and the `!this.full` guards),
`src/write-engine.d.ts` (ambient declaration of the engine surface) and `core/crates/uo-wasm/js/viewer.ts` (the engine side:
`loadViewerModule` = ONE module per page, `ViewerEnv`/`ViewerDocument`/`DecodedLayout` over `write.mjs`, the painter `paint.ts`,
every page decoded once: render message + Write section), `viewer.check.ts` (assigns the real module to the declaration: drift
fails tsc), `hyphen-patterns.mjs`, `write.d.mts`, `render.d.mts` (TS7016: `.mjs` modules typed for `.ts` callers). New wasm
export `uo_layout_image_svg` (+ `WriteLayout.imageSvg`): SVG images paint as vectors as in TS (0 of the 74 fixtures have SVG).
`tools/build-pkg.mjs`: alias `ENGINE` (outside `INTERNAL`: the licence loop reads `<pkg>/package.json`), and `chunks/uo-write.wasm`
copied from `UO_WRITE_WASM` or `core/target/wasm32-unknown-unknown/wasm/uo_wasm.wasm` (warning when missing). Alias also in the
viewer's `vitest.config.ts` and `build:demo`. README: attribute `engine`, `configureEngine`, "Content Security Policy"; `CHANGELOG.md` Unreleased.

Behaviour: `engine` absent or any value but `rust` is the TS path, byte for byte (the vitest "no .wasm" test and Chromium phase 0
prove no `.wasm`, no engine chunk and no `stats` request; the entry's static closure holds none of the engine's code). With `rust`:
`relayout()` = fetch `ASSET_FILES` (cached per base) -> `pushAsset`; `doc.hyphenationPatterns` -> pattern text from the TS pattern
modules -> `registerHyphenation`; up to 5 passes of `doc.layout(env, { cancelCheck })` + `env.takeMissing()` -> the shared
`FontResolver` (`getWithFile`, else `resolveWithFile` + `loadFace`) -> `pushFamily`; painting `decodePage` -> `paintBackground` +
`paintItems` (the D painter), `faceBytes` -> one `FontFace` per file content; text layer from the Write section (`paras`, `lines`);
link boxes from `runBoxes('links')`; comments and headings from `annotations()`; print and download from `layout.pdf()`; every
handle freed on disconnect, new document, replaced layout (`setLayout` frees the previous unless it is the cached print layout).
`cancelCheck` (G4) stops a layout when the document changes or the element disconnects.

Declarations (outside the plan):

| # | Decision | Alternative |
|---|---|---|
| SW-1 | The viewer reaches the engine JS through the alias `@universal-office/write-engine` (`core/crates/uo-wasm/js/viewer.ts`), typed by an ambient `.d.ts` plus a drift check; no `paths` for the types (the ambient module serves them) | `paths` + `allowImportingTsExtensions` (breaks the viewer's emit) |
| SW-2 | Hyphenation patterns come from the TS pattern modules through a literal-import loader (same chunk as the TS path, no duplicated data); the text is the `parse_patterns` format. Checked: the 7 ids register, and 8 fixtures lay out to the same page bytes with them as with `uo-fonts/data/hyphen/*.txt` | ship the `.txt` files (about 450 KB more) |
| SW-3 | `.doc` on the Rust path is converted by the TS `convertDocToDocx`, then opened in wasm | the `doc` feature in the module (size, G7) |
| SW-4 | Up to 5 layout passes instead of 4: the first may be a discovery pass for families the resolver already answers (the Rust policy cannot ask the resolver synchronously) | a synchronous resolver callback |
| SW-5 | `getLinks()`, `getCustomInlines()`, the public `goToBookmark(name)` and link-anchor clicks read the document with the TS reader lazily (`readFullDocument`, only when called); anchors then use `blockLine(block)` | new wasm exports (bookmark -> block, links, inlines) |
| SW-6 | `udv-link-click` `detail.text` is rebuilt from the characters inside the link box; the TS gives the whole run text (differs when a link run wraps) | run boxes carrying the run text (layout change) |
| SW-7 | Plugin inline renderers are not painted and `udv-inline-click` is not fired with `engine="rust"` (`runBoxes('inlines')` carries only the type) | sdt tag data from the wasm (layout change) |
| SW-8 | Disconnect frees every handle; reconnect reopens from the kept bytes (the scroll position resets; moving the element in the DOM does the same) | keep the session alive across a move |
| SW-9 | `build-pkg` copies `chunks/uo-write.wasm` and only warns when it is missing; which build ships (opt-level, `wasm-opt -Oz`, G7) is open | fail the build on rust-core |
| SW-10 | `configureEngine` is static, first load wins (one module per page) | a per-element `engine-wasm` attribute |
| SW-11 | Desktop font profile (`newEnv()` without `phone`); PDF trusted caps, as the TS `renderPdf` | phone/untrusted for viewer documents (product decision) |
| SW-12 | Painter caches are the global `defaultCaches`, as the TS module-level caches | per-element caches |
| SW-13 | New export `uo_layout_image_svg` (status 8 when the image has no SVG, 6 unknown index, 11 null) | none |
| SW-14 | Default stays `ts` until the gate; the CI lines are not added to `rust-core.yml` (another frontier owns it) | - |
| SW-15 | A `FontFace` per file CONTENT (FNV-1a of the bytes + length) shared by every element and document of the page; each session repaints once when a face it uses finishes loading | one per layout |
| SW-16 | The browser test can restart the page every n documents (`UO_SW_BATCH`, default 0 = never): one page accumulates canvases and FontFaces of both engines; restarting made the runs slower here, so it is off | one page, always |

Measured (2026-11-10, Chromium headless-shell 1248 on win32, `viewer_switch.mjs`, `UO_TS_ROOT` = the frozen checkout for tools only):

```
bundle: entry static closure 2 output(s) 868 KB; reachable only by import("./rust-engine"): 2 output(s) 66 KB + uo-write.wasm 3875 KB
phase 0: 3 documents with engine="ts" (1p 122 ms, 12p 198 ms, 5p 33 ms); requests 68, for the wasm / the engine chunk: 0
parity: 13 documents, 108 pages in 64 s
  page count + sizes   0 documents differ (108 pages compared)
  headings / comments / link boxes  0 differ (49 headings, 0 comments, 39 link boxes)
  text layer           0 differ (119856 characters)
  pixels               0 documents differ (216 renders, 519194547 px, 30515092 ink px, 0 unsettled renders), tolerance 0
  PDF (print layout)   0 differ (8607684 B)
  exceptions           0
  loadDocx (open + layout, incl. fonts): TS 1766 ms total, median 88 ms; Rust 3802 ms total, median 288 ms
(UO_SW_STRIDE=6: 13 of the 74 fixture documents, one per 6, every group; an earlier run at UO_SW_STRIDE=10 gave 8 documents, 52 pages, the same 0 differences.
The full 74 did not finish in the implementer's run; the reviewer then ran it.)

**Full run (reviewer, `UO_SW_STRIDE=1 UO_SW_BATCH=10 viewer_switch.mjs --mode parity`, Chromium headless, 457 s): all 74
documents, 579 pages, 0 differences** — page count/sizes, headings (144), comments, link boxes (838), text layer
(693,279 chars), find hits (414 queries, 22,137 hits), pixels (1,158 renders, 2,625,879,594 px, 212,919,649 ink px,
tolerance 0), uncompressed PDF (45,358,755 B). loadDocx totals: TS 16,892 ms (median 122 ms), Rust 27,373 ms (median
251 ms) — the Rust path is slower to open in the browser today; to be measured and explained before the gate.
```

Memory (`--mode memory`, 200 open/close cycles of 10 fixture documents with `engine="rust"`, `count,write` build): live bytes
2,422,548 after the first close, then exactly 4,850,392 at every later sample (190 samples, 1 distinct value); `memory.buffer` 17,039,360
B at every sample after the warm-up (1 distinct value); handles alive after every close 0 (3 while a document is open: environment,
document, layout). Vitest: the 21 existing viewer tests + 4 engine-switch tests pass with the default engine (25 passed, 1 skipped: the
wasm one), and 26/26 with `UO_WASM_PATH` (5 documents: page count, sizes, headings, comments, link boxes, `getLinks`, `goToBookmark`,
`exportPdf`, `handles === 0` after remove). Rust: `write_native` 7/7 (M1: 78,696,053 B allocated, 0 B left, with the svg export called for
every image index, a null layout and an unknown index), `write_api` 6/6 and `write_cancel` 7/7 (Node), clippy `-D warnings` native and wasm32 clean for
`uo-wasm`, grep-bans / float-casts / clippy-ban 0. Sizes: entry static closure 868 KB minified (the TS viewer as before), the Rust path's JS 66 KB
(minified, loaded only for `engine="rust"`), `uo-write.wasm` `--features write`: 3,967,023 B raw, 1,409,254 B gzip (before the svg export 3,966,580 /
1,409,092: +443 B / +162 B).

Remaining gaps of the Rust path against TS: plugin inline renderers (SW-7); `getLinks`/`getCustomInlines`/bookmarks through the TS reader
(SW-5); `udv-link-click` text (SW-6); `.doc` through the TS converter (SW-3); the Layout da Web (`view=web`) and Modo de Leitura run
through the same code but are not in the parity test (print layout only); no WebKit/Safari run of the painter; the first layout of a
document takes one extra pass (SW-4) and costs more than the TS engine in the browser on the fixtures (see the load times above;
the TS engine caches fonts and metrics between documents in the page, the Rust one measures again).

CI lines to add (not added): the core tsc line + `viewer.ts` `viewer.check.ts` `../write/packages/viewer/src/write-engine.d.ts`; the viewer's vitest
with `UO_WASM_PATH` (`count,write` build); `UO_WASM_PATH=<..> UO_TS_ROOT=<frozen> node crates/uo-wasm/tests/browser/viewer_switch.mjs --mode all`.

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
  items (PQ9), and now inside the layout: `LayoutOptions.cancel` (the same `Arc<AtomicBool>` as `PdfOptions.cancel` and
  `UoCancel`) is read at every `Budget::charge`, `LayoutError::Cancelled` becomes code 10 (G4). On wasm, `cancel_poll`
  asks the host import `uo_host.layout_poll` (flag bit 9; G4-W).
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
(Safari 15). The write module imports two functions, `uo_host.pdf_chunk` and (G4) `uo_host.layout_poll`.

Timings (this machine, contended): 136 server conversions of the fixtures in 23 s (release); the smoke console indexes
the machine's 357 families in 53 ms and converts `f8__7__1` (316,892 B PDF) in 151 ms; 211 oracle documents (2 policies,
two converters) in 3.6 s.

### Size analysis of the viewer module (G7, measured 2026-11-06)

Same commit, `--features write`, `wasm32-unknown-unknown`, `--profile wasm` (LTO, one codegen unit, `panic=abort`),
only `opt-level` changed through `CARGO_PROFILE_WASM_OPT_LEVEL`; gzip `-9`, brotli quality 11. The TS reference is the
published `@universal-office/document-viewer` 0.9.0 (`publish/`): `index.js` (reader, layout, canvas painter) is
loaded at start, the PDF chunk only for print/export, the hyphenation and built-in table-style chunks on demand.

| Build | Raw | gzip | brotli |
|---|---|---|---|
| Rust, `opt-level = 3` (today) | 5,959,992 | 1,465,433 | 1,055,236 |
| Rust, `opt-level = "s"` | 4,091,275 | 1,234,978 | 908,502 |
| Rust, `opt-level = "z"` | 3,525,048 | 1,090,792 | 816,431 |
| TS `index.js` (start) | 860,102 | 300,375 | 252,586 |
| TS PDF chunk (on demand) | 481,994 | 146,092 | 123,094 |
| TS all chunks | 2,121,993 | 655,067 | — |

Where the bytes are (`opt-level = 3`, names kept, `target/size/wasm-size.mjs`): code 5.52 MB, data 0.42 MB (Unicode,
font catalogue and other tables). Code by crate: generic code instantiated from `core` 2.11 MB, `alloc` 0.28,
`hashbrown` 0.20, `indexmap` 0.08 (≈ 2.7 MB of monomorphised std, inlined by `opt-level = 3`); ours: `uo_docx`
1.32 MB (the reader), `uo_docx_layout` 0.80, `uo_render` 0.19, `uo_pdf` 0.09, `uo_fonts` 0.08, `uo_opc` 0.08,
`uo_drawingml` 0.06, `uo_jsnum` 0.05, `uo_image` 0.04, `miniz_oxide` 0.02.

**Corrected attribution and results (2026-11-07, plan and numbers in `size-optimization.md`).** The per-function
attribution first used for this analysis (the uncommitted `target/size/wasm-size.mjs`) skipped the import section with an
off-by-one, so every function body carried the name of the next one: single-function claims were wrong ("`FlowState::clone`
31 KB" is `Engine::layout_static_in`, `FlowState::clone` is 1.8 KB; "`uo_pdf::num::Out::write_fmt` 24.7 KB" is
`LineBreaker::next_in`; all of `core::fmt` is 14.6 KB, not 84 KB). The family totals above were roughly right. The
committed tools `core/tools/size/` (`wasm-size`, `wasm-share`, `wasm-ab`) read the module correctly. With them, by owner:
`core` 2.04 MB, of which `core::slice::sort` is **1.83 MB in 497 functions** (127 copies of the stable sort inside the
signature writer alone: 1.69 MB), `uo_docx` 1.37 MB, `uo_docx_layout` 0.93, `uo_render` 0.18, `hashbrown` 0.17.

Raw bytes are not transfer bytes: gzip and brotli already fold byte-identical copies, so removing the duplicated code
takes a third off the raw module but only a few percent off the transfer. Measured (`--features write`, `--profile wasm`,
identical output digest on the 12 fixtures, speed within noise: layout +1.3%, total +0.9%):

| Step | Raw | gzip -9 | brotli 11 |
|---|---|---|---|
| before | 5,960,000 | 1,465,192 | 1,050,575 |
| one `sort_members` for the signature members (item 1) | 4,281,155 | 1,445,744 | 1,046,845 |
| + one JSON walker over `dyn JsObjectDyn` (item 2) | 4,014,011 | 1,422,858 | 1,034,548 |
| + one index sort core for the cold sorts (item 3a) | 3,960,432 (-33.5%) | 1,405,088 (-4.1%) | 1,023,890 (-2.5%) |

So the "trimming monomorphisation" row below (a band of 0.6-0.8 MB) was too high for the transfer: the work that exists
(items 1-3a) is -4.1% gzip. What is left is a speed trade (`opt-level = "s"` for `uo-docx`: about -68 KB gzip for layout
+3%, open +14%) and the splits listed in `size-optimization.md`, section 7 (PDF in a second module: about -60 to -70 KB
gzip; the data section: 161 KB gzip).

Why it is larger than the JS: in the browser the engine supplies the standard library (strings, `Map`, regex, JSON,
number formatting, `Intl`, inflate through `DecompressionStream`) for free, and minified JS is dense; the wasm module
ships its own (hash maps, UTF-16 strings, `uo_jsnum`, hand regex matchers, inflate, Unicode tables), and Rust
generics are compiled once per type, then inlined.

What optimisation buys (rows 1–2 measured; see the wasm2js section below for the `wasm-opt` numbers):

| Step | gzip | Basis |
|---|---|---|
| `opt-level = "z"` | 1.09 MB (−26%) | measured; `z` is 1.5–1.8× slower than `3` here (layout + pages + PDF, wasm2js section): per-crate `opt-level` (layout at 3, cold crates `z`) — to measure |
| + `wasm-opt -Oz` | 1.12 MB (+2.3%) | **measured: no transfer gain** — raw −16.6%, gzip +2.3%, brotli +5.3%; worth it only for raw size/parse and ≈ 10% speed (the first estimate, 0.9–1.0 MB, was wrong) |
| + PDF, inflate and image encoders in a second module loaded on export | ≈ −0.1 MB at start | `uo_pdf` + `miniz_oxide` + encoders ≈ 6% of the code |
| + trimming monomorphisation (`dyn` at cold generic sites, one sort, one map type) and moving the reader's cold paths out of the start module | -0.06 MB gzip measured for the sort and `dyn` work (see above); the rest is a split | work in the crates, not a flag; the first estimate (0.6–0.8 MB) was wrong for the transfer |

So ≈ 45–60% off today's 1.45 MB gzip at start is still the realistic band, but it comes from code changes (split
modules, less monomorphisation), not from `wasm-opt`; matching the TS start (≈ 0.30 MB gzip, ≈ 0.45 MB with the PDF) is
not expected. Budget decision: G7.

**Benchmark main × rust-core (memory, 2026-11-06)**: the wasm viewer path against the frozen TS engine and the TS viewer
0.9.0 is recorded in `uo-docx-layout.md`, "Benchmarks main × rust-core — memory (measured)" (harness
`core/tools/bench/memory/`: `run.mjs` for Node, `browser.mjs` for Chromium/WebKit). Through these exports a 100-page
document keeps an 18.4 MiB `UoLayout` (48.2 B per laid-out character) and the module's `memory.buffer` reaches 45.8 MiB
(46.9 MiB with the PDF; 192.4 MiB for 500 pages); freeing every handle returns the live bytes to 2.1 MiB (the face cache)
but not the buffer — dropping the instance does (an asynchronous major GC in Node), and a new one starts at 1.4 MiB. In a
Chromium tab the Rust engine side (no painter yet) is +67 MiB of renderer private bytes against +127–150 MiB for the TS
viewer.

### No-WebAssembly fallback (wasm2js) and wasm-opt (measured)

Measured 2026-11-06 on the same sources as the G7 table (`--features write`, `--profile wasm`), binaryen **132.0.0** from
the npm package `binaryen` (it ships `wasm-opt` and `wasm2js` as node scripts around a wasm build; installed into
`core/target/tools/binaryen`, not a dependency of the repository; 15 files, **108,542,397 B unpacked**; the registry
sends no content-length, so the tarball size was not read). Sizes: raw bytes, `gzip -9` (CLI), brotli 11; JS output
minified with the repository's esbuild 0.28.1 (`minify`, target es2018; binaryen has no JS minifier).

**Why.** A page whose CSP `script-src` has neither `'wasm-unsafe-eval'` nor `'unsafe-eval'` cannot compile WebAssembly.
`'wasm-unsafe-eval'` exists from Chrome/Edge 97, Firefox 102, Safari/iOS 16.0; **Safari 15 (our baseline) has no such
keyword**, so there a strict CSP leaves only `'unsafe-eval'` (which also allows `eval`) or no wasm at all. The wasm2js
build is plain JavaScript and needs neither. (Safari 15 itself was not available; WebKit 26.5 from Playwright was.)

**Sizes (viewer module).**

| Build | Raw | gzip | brotli | Output correct? |
|---|---|---|---|---|
| wasm, `opt-level = 3` | 5,960,000 | 1,465,190 | 1,050,575 | (reference) |
| wasm, 3 + `wasm-opt -O3` | 5,376,501 (-9.8%) | 1,462,528 (-0.2%) | 1,078,183 (+2.6%) | yes (`write_api` pinned checksums) |
| wasm, `opt-level = "z"` | 3,525,048 | 1,090,787 | 816,431 | (reference) |
| wasm, z + `wasm-opt -Oz` | 2,940,079 (-16.6%) | 1,115,496 (+2.3%) | 860,133 (+5.3%) | yes (same) |
| wasm2js, z, no `wasm-opt`, no `wasm2js -O` | 9,444,914 | 1,903,039 | 1,285,411 | not run |
| wasm2js, z + `wasm-opt -Oz`, no `wasm2js -O` | 8,416,037 | 2,043,952 | 1,430,384 | yes (12 fixtures) |
| **wasm2js, z + `wasm-opt -Oz --alignment-lowering` + `wasm2js -Oz` (the script's default)** | **5,862,537** | **1,574,692** | **1,146,143** | **yes (12 fixtures)** |
| wasm2js, same, unminified (`.raw.js`) | 29,479,388 | 3,067,149 | 1,962,285 | — |
| wasm2js, 3 + `wasm-opt -O3`, no `wasm2js -O` | 15,603,620 | 3,062,033 | 2,003,586 | **no** (page messages differ) |
| wasm2js, 3 + `wasm-opt -O3 --alignment-lowering` + `wasm2js -O3` | 10,644,633 | 2,170,654 | 1,496,728 | **no** (same) |

Reading: `wasm-opt` shrinks the raw module 10-17% but **does not shrink what is transferred** (gzip -0.2% / +2.3%, brotli
+2.6% / +5.3%: it unrolls and inlines what the compressor already folded); its value is parse and instantiate time and
memory, not bytes on the wire (and `z` + `-Oz` ran 10% faster than plain `z`: 227 vs 257 ms on the 33-page fixture in
Node). `wasm2js` text is 2.0x (z) to 7x of the wasm in raw terms and 1.4x-1.9x in gzip; its own `-Oz` is worth 30% of the
minified size and needs the `wasm-opt` step first (`wasm2js -O` on a raw rustc module aborts on an unaligned store).
`--alignment-lowering` (rustc emits many `align=1` loads and stores) costs 0.3% on the Oz pipeline and keeps the output
valid if the optimiser ever leaves an unaligned access. Build time on this machine: `wasm-opt -Oz` 35 s, `-O3` 55 s,
`wasm2js` 10-35 s plus 1-1.5 min for the whole script (all single-threaded node).

**The opt-level 3 build is not safe to convert (finding).** wasm2js of the `3` module gives byte-identical PDFs, navigation
and link boxes but **different page messages** (f8__7__0: 51,757 B in wasm, 53,200 B in JS; the same with or without
`wasm-opt`, `--alignment-lowering`, `--llvm-nontrapping-fptoint-lowering`). A `--log-execution` trace of both engines
diverges at the first `encode_page` after the (identical) layout, in the function that tests `OptF64`
(`uo-render/src/opt.rs`): its "absent" marker is the signalling NaN `0x7ff4_0000_0000_0001`, and in the JS build the
marker arrives as the canonical `0x7ff80000_00000000` (V8 does not keep NaN payloads across all of its paths; patching the
marker to a quiet payload did not help). The `z` build converts correctly on the 12 fixtures, but only because its
code passes `OptF64` as an integer; **the hazard is in the Rust source, not in wasm2js's arithmetic**. Before any wasm2js
build ships: give `OptF64` an explicit presence bit (or any representation that is not a NaN payload) and run
`write_wasm2js.test.mjs` over a bigger corpus than 12 fixtures. Not done here (the crate is outside this task's scope).

**The loader (`js/write.mjs`, `write.d.ts`).** `loadWrite({ engine, wasm, js })` -> `{ uo, engine, exports }`; `engine`
`'wasm'`, `'js'` or `'auto'` (default). `auto` probes with `canCompileWasm()` (an 8-byte module through `new
WebAssembly.Module`, which is subject to the same CSP check) and uses the JS build if the probe fails; if the probe passes
but the compile or instantiate still throws a `CompileError`/`EvalError` and `js` was given, it falls back too. `wasm`
is bytes, a URL, a `Response` or a promise of one; `js` is the module URL (dynamic `import()`, so it needs `script-src
'self'` or the URL's origin), the namespace, the factory, or a thunk returning either. The JS module exports `default
function (imports) -> exports` (the build script rewrites wasm2js's ES-module output into that factory, so several
instances are possible and no import map is needed). `engine: 'js'` never touches `WebAssembly`.

**Correctness (z, final pipeline).** `tests/write_wasm2js.test.mjs` (Node) and `tests/browser/wasm2js_run.mjs --mode
correct` (Chromium 156, WebKit 26.5): over the 12 fixtures of `uo-docx-layout/tests/captures/f8` every render page
message, Write section, annotations, navigation, link boxes, page info, first embedded face, the PDF (compressed) and the
PDF (uncompressed, 4 KiB chunks) are **byte-identical** between the WebAssembly engine (the unoptimised `z` module) and the
wasm2js engine (built from the `wasm-opt`-ed module); the three pinned native PDF checksums hold in the JS engine; errors
keep their code and stage; `memory.buffer` is flat over repeated sessions. `write_api.test.mjs` also passes on the
`wasm-opt`-ed modules (`z`+`-Oz` and `3`+`-O3`).

**CSP proof.** The test page is served with `Content-Security-Policy: script-src 'self'` (page script external, no inline):

| | Chromium 156 | WebKit 26.5 |
|---|---|---|
| `canCompileWasm()` | false | false |
| wasm engine | refused: `CompileError`, "violates ... CSP directive because 'unsafe-eval' is not an allowed source of script" | refused: `CompileError`, "Refused to create a WebAssembly object because 'unsafe-eval' or 'wasm-unsafe-eval' is not an allowed source" |
| `securitypolicyviolation` | `script-src: wasm-eval` (x3) | — (none fired for wasm) |
| wasm2js engine | loads in 99 ms, runs; PDFs of f8__7__0 / 3 / 10 = 245,286 / 144,349 / 240,299 B, fnv `c28b3c53` / `a2cf1bd6` / `88d31b5b` = the native checksums | loads in 111 ms, same bytes and checksums |
| `loadWrite()` (`auto`) | `js` | `js` |

Control page with `script-src 'self' 'wasm-unsafe-eval'`: `canCompileWasm()` true, the wasm engine loads, `auto` picks
`wasm` in both browsers (and `eval`/`new Function` stays refused in both pages).

**Speed** (`layout + every page message and Write section + PDF`, median of 5 after one warm-up, ms; env and document
prepared outside; Windows 11, loaded machine; fixtures: f8__7__0 3 pages, f8__7__10 5, f8__7__9 24, f8__7__7 33):

| Engine | f8__7__0 | f8__7__10 | f8__7__9 | f8__7__7 |
|---|---|---|---|---|
| Node 24.15, wasm `3` | 30 | 26 | 97 | 145 |
| Node, wasm `z` | 48 | 40 | 167 | 257 |
| Node, wasm `z` + `-Oz` | 43 | 37 | 148 | 227 |
| Node, **wasm2js** (z) | 218 | 165 | 1,240 | 1,955 |
| Chromium 156, wasm `3` | 31 | 26 | 104 | 146 |
| Chromium, wasm `z` | 46 | 39 | 155 | 235 |
| Chromium, **wasm2js** | 229-236 | 174-186 | 798-1,398 | 1,245-2,161 |
| WebKit 26.5, wasm `3` | 41 | 34 | 123 | 190 |
| WebKit, wasm `z` | 56 | 47 | 185 | 285 |
| WebKit, **wasm2js** | 243-246 | 179-327 | 795-1,442 | 1,243-1,271 |

wasm2js is **4-8x slower than the `z` module and 6-14x slower than the `3` module** (V8, the same ratios in JavaScriptCore);
the JS columns are two runs each in the browsers because the spread is large (tier-up and GC; a single run varies up to
1.8x). Cold costs: loading the module (parse, no instantiation) 216-237 ms in Node, 100-104 ms in Chromium, 106-111 ms in
WebKit vs 6-32 ms for `WebAssembly.instantiate`; in Node the first layout + PDF of the 3-page fixture takes 465 ms on
wasm2js (env and open 147 ms) vs 81-100 ms on wasm (41-46 ms).

**Memory.** The linear memory (`memory.buffer`) is the same 25,034,752 B high-water in both engines for the four fixtures
(same code, same allocations). The process cost differs: Node maxRSS 302 MB for wasm2js vs 123 MB (z) / 127 MB (3) for wasm,
`heapUsed` 53 MB vs 5-6 MB; Chromium `usedJSHeapSize` after loading the module and the runs 342 MB vs 18 MB (z) / 27 MB (3)
(9.4 MB of that is the parsed module). WebKit exposes no JS heap figure.

**Recommendation.**
1. Do not ship wasm2js as a default or in the start bundle: +5.9 MB raw / **+1.57 MB gzip** on top of nothing (a host that
   may compile wasm never downloads it), a 4-8x slowdown and ~2.5x the memory. Ship it, if at all, as an **opt-in extra
   package** next to `font-files`/`spellcheck` (a data-like artefact the host registers: `js: () => import('...')`), loaded
   only when `canCompileWasm()` is false. It does not change the budget of the wasm module itself.
2. While the TS engine exists behind the viewer switch (plan above, `engine="ts"`), **it is the cheaper no-wasm fallback**
   (0.30 MB gzip at start, plain JS, no conversion hazard); wasm2js becomes interesting only when the TS path is removed
   and a strict-CSP/Safari 15 host still has to work. That decision belongs to G7/the 0.10 gate.
3. If it is shipped: build only from the `z` module, keep `wasm-opt -Oz --alignment-lowering` + `wasm2js -Oz`, put
   `write_wasm2js.test.mjs` (and the browser check) in the gate, and fix `OptF64` first (finding above).
4. `wasm-opt` on the wasm module: worth running for the 10-17% smaller raw module and ~10% speed, not for transfer size
   (gzip/brotli do not improve).

Commands (all outputs under `core/target/`, never committed):

```
npm install --prefix core/target/tools/binaryen binaryen                                  # once; wasm-opt, wasm2js (not a repo dependency)
export CARGO_TARGET_DIR=<target>; cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm -j 4
CARGO_PROFILE_WASM_OPT_LEVEL=z cargo build ...                                            # the z module
node core/target/tools/binaryen/node_modules/binaryen/bin/wasm-opt -Oz --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --enable-multivalue --enable-reference-types in.wasm -o out.wasm
node core/crates/uo-wasm/tools/build-wasm2js.mjs --in <uo_wasm.wasm> --out core/target/wasm2js/uo_wasm.js [--wasm-opt Oz|O3|none] [--wasm2js-opt Oz|O3|none] [--no-minify] [--esbuild <dir>]
UO_WASM_PATH=<.wasm> UO_WASM2JS_PATH=<.js> node --test crates/uo-wasm/tests/write_wasm2js.test.mjs
UO_PLAYWRIGHT=<playwright dir> [UO_CHROMIUM=<chrome.exe>] node crates/uo-wasm/tests/browser/wasm2js_run.mjs --wasm <.wasm> --js <.js> [--browser chromium|webkit] [--mode csp|correct|speed]
node core/crates/uo-wasm/tools/bench-wasm2js.mjs --engine wasm --wasm <.wasm>             # or --engine js --js <.js>; one engine per process (maxRSS)
```

Files: `js/write.mjs` + `write.d.ts` (`loadWrite`, `canCompileWasm`), `tools/build-wasm2js.mjs`, `tools/bench-wasm2js.mjs`,
`tests/wasm2js_common.mjs`, `tests/write_wasm2js.test.mjs`, `tests/browser/wasm2js_page.mjs`, `tests/browser/wasm2js_run.mjs`.
Not done: Safari 15 itself, Firefox, a corpus beyond the 12 fixtures, the `OptF64` fix, wasm2js of the `count` build
(`uo_memory_live_bytes` returns an i64, which the JS build would give as its low half).

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
| G4 | **Done (2026-11-09).** `LayoutOptions.cancel` (shared flag, Relaxed load at every `Budget::charge`) + `cancel_poll` (every `CANCEL_POLL_UNITS` = 4,096 work units, first at the first charge) + `LayoutError::Cancelled` (latched, wins over `Limit(Work)`); uo-ffi: code 10, stage none, the server passes the same Arc to layout and PDF; C# facade unchanged (token -> `UoCancel` -> `OperationCanceledException`); wasm: opt-in host import `uo_host.layout_poll` (flag bit 9, JS `LayoutOptions.cancelCheck`, **G4-W**), a throwing callback counts as cancel. Latency from raising the flag mid-layout (650-paragraph document, release): native 0.16-1.4 ms, C ABI 0.7-1.4 ms, .NET facade 0.6-1.5 ms (ranges over runs) (before: the rest of the layout, hundreds of ms); M1 0 B left (native layout, C ABI, wasm 20 cancelled layouts cancelled mid-layout at the k-th poll, .NET 5 + 1); cost within noise (B/A -2.9% to +0.4%, C/A -0.3% to +1.5% over median/min on pages100 and pages500, one +17.5% outlier min, noisy machine); layout parity unchanged (layout_parity 5, f8_parity 2, g8 4, f7a 7, server_parity 3; wasm-ab digest cca8d8794ec4bfe0 equal, layout min 366.1 -> 367.9 ms). Record: uo-docx-layout.md "Stage 3 — G4" | the PDF on wasm and the `.doc`/reader stay uncancellable inside (G4-4) |
| G5 | Untrusted caps: kernel radius 256, masks/effect layers 16 Mpx, images 40 Mpx (first numbers); the facade defaults to untrusted | measure on the corpus first |
| G6 | The wasm PDF leaves through an **import** (`uo_host.pdf_chunk`): the viewer module cannot be instantiated without imports | pull the chunks (the output then stays in wasm memory, which never shrinks) |
| G7 | `write` is **not a default feature** (the default module stays the 15 KB foundation build the other tests use); the shipped viewer module is `--features write`, **5.9 MB (1.45 MB gzip)** — a size budget, `wasm-opt`, and trimming uo-docx are open | make `write` default |
| G8 | **Done (2026-11-07).** The Write geometry section of the page message (`Layout::encode_page`, `uo_layout_encode_page`, `uo_layout_annotations`): parity against the frozen TS 100%, 8 corpora (committed 52 docs, ci 100, `--gen f8` 100, oracle 2,742, 10 real documents with 35 comments / 55 rects), 0 mismatches; 0 B left (M1); no-panic; Node 8/8, native 6/6; the section is 1.9–2.2× the render message on the wire (maintainer decision G8-W); G8 code in the module: 26 KB raw / 9.4 KB gzip of 3.95 MB / 1.40 MB. Record: uo-docx-layout.md "Stage 3 — G8" | wire size, lazy section (G8-2) |
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
| G22 | `uo-render` `OptF64` marks "absent" with a signalling-NaN payload (`0x7ff4_0000_0000_0001`); NaN payloads are not guaranteed to survive outside wasm loads/stores (V8 canonicalises them on some paths — the `opt-level = 3` wasm2js build gives different page messages because of it, see the wasm2js section) | give `OptF64` an explicit presence bit (or any non-NaN representation) before any JS/wasm2js build ships; audit other NaN-payload markers |

## Files touched outside the owned crates (each additive)

- `crates/uo-render/src/view.rs`: `RenderView::pages(&self) -> &[Page]` (the PDF of a view reads `&[Page]`).
- `write/packages/viewer/**` (new `rust-engine.ts`, `image-decode.ts`, `write-engine.d.ts`, `engine-switch.test.ts`; the element, `fonts.ts`, `text-layer.ts`, `views.ts`, README, `vitest.config.ts`, `package.json`), `tools/build-pkg.mjs` (alias `ENGINE`, `chunks/uo-write.wasm`), `CHANGELOG.md`: the viewer switch.
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

- The switch for the editor `<universal-write>` (milestone 2) and the gaps of the viewer's Rust path ("Remaining gaps" in section 2); the painter in WebKit / real Safari 15 / iOS (BrowserStack); the CI lines of the switch and of D/V; HANDOFF queue item 4 is done by this frontier (HANDOFF.md is updated by the orchestrator).
- The NuGet RID matrix and `dotnet pack` with native assets (G14); LSan on the facade tests (G19); fuzz targets (G21).
- A `ToPdf(Stream)` overload (uo-pdf's `IoSink` exists) for large outputs.
- The structural C# vs Rust migration report for clickCompliance (uo-pdf §6, not a gate).
- The rest of the oracle corpus with installed fonts (211 of 2,742 run; the same command over every group).
- The editor's queries (hit test, caret, selection: F8's `EditLines`) — milestone 2 with the session; the plugin-inline
  face (`custom_inline_face`) needs the page items, which the layout handle moved into the render view.

## Commands

```
UO_TS_ROOT=<frozen> UO_WASM_PATH=<count,write .wasm> node crates/uo-wasm/tests/browser/viewer_switch.mjs [--mode parity|memory|all|phase0]   # UO_SW_STRIDE, UO_SW_SOAK, UO_SW_ONLY=<regex>, UO_SW_BATCH, UO_SW_VERBOSE
export CARGO_TARGET_DIR=<target>
cargo test -p uo-ffi --release --all-features                      # UO_SERVER_PARITY_EVERY=1 for all 68; UO_FFI_SOAK=<n>
UO_SERVER_ORACLE_DIR=<dir> cargo test -p uo-ffi --release --test server_parity the_oracle -- --nocapture
cargo test -p uo-wasm --features write,count --test write_native -- --nocapture   # prints the pinned checksums
cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm
UO_WASM_PATH=<.wasm> node --expose-gc --test crates/uo-wasm/tests/write_api.test.mjs
UO_WASM_PATH=<.wasm> node --expose-gc --test crates/uo-wasm/tests/write_cancel.test.mjs      # G4
cargo test -p uo-docx-layout --release --test g4_cancel -- --nocapture     # G4; UO_G4_BENCH=<runs> UO_G4_DOC=<docx> for the cost
cargo test -p uo-ffi --release --all-features --test cancel_layout -- --nocapture   # G4
UO_WASM_PATH=<.wasm> UO_PLAYWRIGHT=<playwright> node crates/uo-wasm/tests/browser/run.mjs --browser webkit crates/uo-wasm/tests/write_api.test.mjs
dotnet test dotnet/UniversalOffice.Docx.Tests -m:2 [--artifacts-path <dir>]       # UO_DOTNET_M4, UO_DOTNET_CONVERSIONS, UO_DOTNET_THREADS, UO_DOTNET_SOAK
dotnet run --project dotnet/UniversalOffice.Docx.Smoke -- <in.docx> <out.pdf> [--font-dir <d>]... [--no-system-fonts]
UO_TS_ROOT=<built d0f45bc5> node crates/uo-pdf/tests/parity_support/capture_painter.mjs --docs <dir> [--open-fonts] --dump <tmp> --out <cap>
```

## Process metrics

SW (the viewer TS/Rust switch, 2026-11-10): planner Opus, implementer Sonnet 5.5; wall time and tool-call counts were not recorded per step (estimates: about 4 h wall, a large part of it waiting for Chromium runs on a loaded machine; about 150 tool calls). Parity rounds: 0 mismatches on every comparison, first run. Rework: the `.wasm` default-source bug in the memory phase (the driver asked for the module before any viewer loaded it), a test timer that paused the page after 150 s (cleared per document now).

G4 (cancellation inside the layout): planner Opus, implementer Sonnet 5.5 (the maintainer's request; SKILL.md assigns the layout crate to Opus), see uo-docx-layout.md "Stage 3 — G4".

G8 (the Write section of the page message): Opus WIP 6471786d + Sonnet 5.5 finish (the maintainer's request; SKILL.md assigns the layout crate to Opus), see uo-docx-layout.md "Stage 3 — G8".

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
