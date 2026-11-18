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

Document: `uo_write_hyphenation_patterns(doc, env, app_langs, flags)` = the viewer's `hyphenationLangs(full)`
(`documentHyphenationLanguages(full)` filtered by the app list, prefix match) → pattern ids **not yet registered**, so
the host fetches only those; English is `en-us-word` (Word's data) unless `flags` has `LAYOUT_TS_HYPHENATION` (bit 16:
the frozen TS's `en-us`) — WordCraft study, fields D1 (record uo-docx-layout.md "WordCraft study — fields").

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
  **Confirmed on a real Safari 15.6.1 (macOS Monterey, BrowserStack, 2026-11-10):** `script-src 'self'` refuses
  WebAssembly (`EvalError`), and so does `script-src 'self' 'wasm-unsafe-eval'` (Safari 15 ignores the keyword); only
  `'unsafe-eval'` lets it compile. iOS 26.5 compiles with `'wasm-unsafe-eval'` and refuses with `'self'` alone
  (`CompileError`). See "Real Safari 15 / iOS (BrowserStack, 2026-11-10)".

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

#### Open speed (2026-11-10)

**The question.** In the full SW run the Rust engine opened the 74 fixtures slower than TS in Chromium (loadDocx total 27.4 s against
16.9 s, median 251 ms against 122 ms) although the wasm layout alone is as fast as the TS one natively. Where does the time go, and
which part is avoidable inside the approved design?

**Root cause.** One `WriteEnv` per viewer ELEMENT (SW section 2), and a page that shows several documents makes a new element for each.
The TS engine keeps fonts and hyphenation for the whole page (`FontResolver.shared()`), so after the first document it lays out in one
pass. A fresh Rust environment paid again for: (a) the 4 Liberation Sans assets (`pushAsset`, SHA-256 over 1.65 MB, about 11 ms);
(b) a discovery pass that always ran (pass 0 has nothing pushed, so `takeMissing` listed families the resolver already answers): 2 layout
passes per document, 148 for 74; (c) `pushFamily` of the document's families (SHA-256 over every face, about 10 ms for the 4 Carlito faces);
(d) SHA-256 a second time at first use of the face (`FaceCache::get_or_parse`, uo-fonts, NOT changed here). Eager encode+decode of
every page of every pass was a smaller cost (about 4 % of the total).

**Measured** (`viewer_switch.mjs --mode speed`, Chromium headless, 74 fixtures, TS and Rust interleaved per document, one fresh page per
round, round 1 not counted; "S1" = a new element per document, "S2" = one element per engine for all documents, "S0" = cold first load).

```
                                   before (HEAD ec66f1a1)            after (this frontier)
S1 Rust loadDocx total / median    23.2-23.3 s / 172-173 ms          9.4-9.5 s / 44-47 ms
S1 TS   loadDocx total / median     9.3 s /  70-75 ms                 8.9 s / 62-72 ms (TS code unchanged: machine noise)
S1 Rust layout passes (74 docs)     148                                77
S1 median per-document Rust/TS      2.62-2.79                          0.73-0.80  (3 runs x 2 rounds)
S1 Rust stages (sum, ms)            wasmLayout 18095-18215, supply 3188-3221, pushFamily 3088-3125 (544 calls),
                                    prepare 1155-1167, pushAsset 1132-1147 (296 calls), open 125-129
                                    -> wasmLayout 9029, fetchLater 155, open 125, pushFamily 62 (14 calls), prepare 10
S1 Rust first page (drawPage 1/15)  median 21-22 ms                    median 19-21 ms (decode now happens here)
S2 Rust median / TS median          50 / 49 ms (ratio 1.24-1.27)       47-48 / 47-53 ms (ratio 1.14-1.18)
S2 Rust total / TS total            9.9 s / 7.7-7.9 s                  9.4 s / 7.6 s
S0 cold first document (rust / ts)  not measured                       131-173 ms (median 131-146) / 68-97 ms (median 70-80)
```

Node cross-check (`core/target/agent-speed/stages.mjs`, the viewer's own JS, same wasm, eager decode as before): fresh env per document
sum 24.5 s, median 191 ms, p90 523 ms (layout pass 0 = discovery 8.6 s, pass 1 11.0 s, push 3.4 s, assets 1.2 s); one env shared by all
documents sum 10.0 s, median 54.8 ms, p90 212 ms (1 pass each, push 4 ms). The fresh-env Node sum matches the browser total of the SW run.

Full parity after all changes (`--mode parity`, `UO_SW_STRIDE=1 UO_SW_BATCH=10`): 74 documents, 579 pages, 0 differences in page
count/sizes, headings/comments/link boxes (144/0/838), text layer (693,279 chars), find hits (22,137), pixels (1,158 renders,
tolerance 0), PDF (45,358,755 B), 0 exceptions. Its own loadDocx line (a pixel-compare run, element pairs, page restarted every 10
documents, so every batch pays the first load again): TS 16,377 ms total / median 123 ms, Rust 16,036 ms / median 133 ms. Memory mode
(count build, 200 open/close cycles): handles 0 after every close, page envs 1, live bytes 1 distinct value after warm-up, buffer 1
distinct value. Phase 0: 0 requests for the wasm or the engine chunk.

| # | Declaration |
|---|---|
| SPEED-1 | ONE environment per PAGE and fonts base (`rust-engine.ts` `PageFonts`, `newEnv({ page: true })`), never freed by a viewer: fonts, assets and hyphenation patterns pushed once for all elements, like the TS `FontResolver.shared()`. It counts in the new `stats().pageEnvs`, not in `handles`. It replaces the "one `WriteEnv` per viewer element" line of section 2. Alternative kept out: per-element env with pre-pushed families (still pays the SHA-256 of every face for each new viewer, about 20 ms, and stays near the TS median). Known consequences: the FaceLedger budget (WASM_DESKTOP, 128 MiB) is shared by every viewer of the page, so many documents with big embedded fonts could meet `OverBudget` where a per-element env would not (the fixtures cannot); `takeMissing` reports each (family, variant) once per environment, so with several elements loading at the same time one may stop without a family the other already took, until the resolver's `onChange` makes it relayout (the harness loads one at a time and does not exercise this); hyphenation patterns accumulate page-wide; `takeMissing`'s `attempted` set is per page, not per element as the TS `ViewerFonts.attempted`, so a family whose fetch failed in `fetchLater` (transient network or Local Font Access error) is not asked again by a later viewer of the page, where TS retries it per element; one environment per distinct fonts base lives (and keeps its pushed faces charged to the budget) until the page ends, also after the `fonts` attribute changes. Concurrent sessions await ONE assets push and ONE registration per hyphenation id (a promise in `PageFonts`; a failed one is dropped so the next session retries). |
| SPEED-2 | A layout pass whose missing fonts are all answered synchronously by the resolver is freed without being handed to `onLayout` (not shown, not decoded) and the next pass starts at once; the last pass is always handed over. The decision "relayout or stop" is unchanged. (`pushNow` / `fetchLater` split of the old `supply`.) |
| SPEED-3 | Pages are decoded ON DEMAND: new export `uo_layout_page_heads` (w, h, background of every page, because `applyZoom` reads every background), `WriteLayout.pageHeads()`, lazy page objects in `DecodedLayout` that encode+decode page `i` (together, so the tables see messages in encoder order; the encoder serves any order) the first time `items`/`lines`/`paras` or a paint call needs it. A decode failure now shows at paint/read time: one `console.error` per layout and an empty page (before: the layout constructor threw). The `view=web` layout reads lines of every page for its height, so that view still decodes all pages. |
| SPEED-4 | `faceKey` keeps the bytes it fetched only until the caller decides: `fontOf` takes them through `faceBytes` on a cache miss (one copy out of the wasm memory) and calls `dropFaceBytes(i)` when the page already has an entry for the key, so no font file stays held in JS for the life of the layout. `FontFace` gets the exact buffer (one copy instead of three). A page whose lazy decode failed is remembered (`#failed`) and never encoded again; `free()` clears the decoded lines too. Out-of-order decode is tested (`write_api.test.mjs`, reverse and shuffled equal in-order). |
| SPEED-5 | NOT done: `WebAssembly.instantiateStreaming` for URL/Response sources. Tried and measured: S0 cold Rust median 146 / 145 / 129 ms without against 149 / 148 / 140 ms with it (three fresh pages each), no gain in a fresh-profile headless Chromium (the code cache it enables needs a persistent profile), so reverted. |

**What this supersedes in the SW text.** SW-4's "the first layout takes one extra pass" is gone (1 pass per document, 77 passes for 74
documents; the extra ones are real missing fonts), and the "Remaining gaps" sentence that the Rust path costs more than TS on load: with
a page environment the median open in S1 is 44-47 ms against 62-72 ms for TS (ratio 0.73-0.80). SW's own lines are left as they were.

**Remaining gap (S1 and S2 totals, S2 median).** The Rust MEDIAN is at or below TS in the "new element per document" case (the case the
harness and a page showing several documents hit). The total is not: S1 Rust 9.4 s against TS 8.9 s, S2 Rust 9.4 s against TS 7.6 s,
and the S2 median ratio is 1.14-1.18 (47-48 ms against 47-53 ms in separate rounds; same-round ratio 1.16). Everything left is the wasm
layout itself in Chromium: `rust.wasmLayout` 9.0-9.1 s for 77 passes, against the TS layout passes 7.1-7.2 s (the heaviest document,
f7c__7__13: 2.35 s against 1.6 s). In Node the layout stage is at parity with TS on the 68-capture sum (native 0.96, wasm 0.98 of TS; 20 of 68 documents slower natively and 34 in wasm; the wasm layout of the
500-page document is 1.40x TS: `uo-docx-layout.md` "Benchmarks main x rust-core - speed"), so the likely cause of the Chromium gap is how Chromium runs the 3.9 MB
module (Liftoff baseline code, then TurboFan tier-up; a hypothesis, NOT measured here: no flag run, no code cache, see SPEED-5), not something in the viewer. It is not touched by this
frontier. The first call of a new tab pays the compile + instantiate (S0: about 60 ms more than TS for a small document, the module
is memoized per page afterwards). Not fixed here: uo-fonts SHA-256 twice per face (keyed `get_or_parse` would halve it; after SPEED-1 it is paid once per page per face, so
low priority).

**Commands** (from `core/`, `UO_TS_ROOT` = the frozen checkout):
`cargo build -j 4 -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm` (and `--features count,write` for memory);
`UO_WASM_PATH=<wasm> UO_SW_ROUNDS=3 node crates/uo-wasm/tests/browser/viewer_switch.mjs --mode speed` (`UO_SW_SCENARIOS=S0,S1,S2`);
`... --mode parity` with `UO_SW_STRIDE=1 UO_SW_BATCH=10`; `... --mode memory` (count build; now also fails unless `pageEnvs === 1`);
`node tools/size/wasm-ab.mjs --runs 5 <old.wasm> <new.wasm>` (digests equal: cca8d8794ec4bfe0 both, no change of page messages, sections
or PDF); `cargo test -j 4 -p uo-wasm --features write,count --test write_native --test write_unsafe -- --test-threads=3` (M1 0 B left including
`uo_layout_page_heads`); `node --expose-gc --test crates/uo-wasm/tests/write_api.test.mjs ...write_cancel... ...write_section...` (17/17: `write_api` 8, `write_cancel` 7, `write_section` 2; `pageHeads` equals the eager decode of every page of the three fixtures); `node tools/check-changed.mjs` (clean).
Files touched: `uo-wasm/src/write.rs` (+ `uo_layout_page_heads`), `js/{viewer.ts,write.mjs,write.d.ts}`, `viewer/src/rust-engine.ts`,
`viewer/src/write-engine.d.ts` (additive: `newEnv(o?)`, `pageEnvs`; not in the brief's owned list), `tests/browser/viewer_switch.mjs` (`--mode speed`,
memory checks `pageEnvs`), `tests/write_native.rs`, `tests/write_unsafe.rs`, `tests/write_api.test.mjs`.

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
- **Threads**: each `uo_docx_to_pdf` conversion runs on its own thread of `CONVERSION_STACK_BYTES` (16 MiB, 64 MiB in debug: nested tables use about 19 KB of release stack per level and an overflow aborts the process); the caller reads the result and the per-thread last error on its own thread. `UoConverter` is `Send + Sync` and shared by every thread (the system index and the face cache are locked
  inside; a font registry and its ledger are made per conversion, so documents do not share a font budget). `UoCancel`
  is raised from any thread; a job holds its own `Arc` of the flag.
- **Cancellation**: checked before the work, between read, layout and PDF, and by uo-pdf before each page, every 256
  items (PQ9) and before each raster effect (text, shape and picture effects), and now inside the layout: `LayoutOptions.cancel` (the same `Arc<AtomicBool>` as `PdfOptions.cancel` and
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

Fix of 2026-11-17 (orchestrator): since 8f53340f (Word rules by default) the native `server_parity` replays with
`LayoutOptions::ts_strict()`, but the facade had no way to ask for it, so the two C# parity tests failed (3/68). The job
flag `JOB_TS_STRICT = 8` (no struct change) and the internal `LayoutOptions.TsStrict` (visible to the tests only) restore
**43/43** in `UniversalOffice.Docx.Tests`; no product path sets it.

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
| M1 native, one server conversion through `uo_docx_to_pdf` (ok / invalid / cancelled / page limit) | `uo-ffi/tests/ffi_memory.rs` | global counters (the conversion runs on its own thread): ok 39,468,390 B allocated (file 245,286 B); invalid 761 B; cancelled 8,699 B; limit 21,084,607 B; **0 B left each**, three runs equal |
| M3 server conversion (release; `Converter::docx_to_pdf` inline since the C ABI converts on a worker thread) | same | f8__7__0 7,970 B docx → 245,286 B PDF: 39.2 MB allocated, peak 1.14 MB; f7b__7__0 → 187,564 B: 87.3 MB / 3.59 MB; f7c__7__1 → 335,884 B: 68.7 MB / 2.27 MB; f8__7__2 → 323,092 B: 135.8 MB / 3.42 MB |
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
module `--features write` **3,962,975 B (about 1.405 MB gzip, after the size work: `size-optimization.md`)**; both pass `wasm-tools validate
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
build is plain JavaScript and needs neither. (Confirmed on a real Safari 15.6.1 on 2026-11-10: `'wasm-unsafe-eval'` does not
let it compile wasm, `'unsafe-eval'` does; "Real Safari 15 / iOS (BrowserStack, 2026-11-10)".)

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
| wasm2js, 3 + `wasm-opt -O3`, no `wasm2js -O` | 15,603,620 | 3,062,033 | 2,003,586 | **no** before G22 (page messages differ); not rebuilt after |
| wasm2js, 3 + `wasm-opt -O3 --alignment-lowering` + `wasm2js -O3` | 10,644,633 | 2,170,654 | 1,496,728 | **no** before G22 (same); not rebuilt after |
| wasm2js, 3 (G22 fix) + the script's default (`wasm-opt -Oz --alignment-lowering`, `wasm2js -Oz`), minified (2026-11-10) | 7,290,722 | not measured | not measured | **yes after G22** (12 fixtures, Node 5/5; Chromium and WebKit) |

Reading: `wasm-opt` shrinks the raw module 10-17% but **does not shrink what is transferred** (gzip -0.2% / +2.3%, brotli
+2.6% / +5.3%: it unrolls and inlines what the compressor already folded); its value is parse and instantiate time and
memory, not bytes on the wire (and `z` + `-Oz` ran 10% faster than plain `z`: 227 vs 257 ms on the 33-page fixture in
Node). `wasm2js` text is 2.0x (z) to 7x of the wasm in raw terms and 1.4x-1.9x in gzip; its own `-Oz` is worth 30% of the
minified size and needs the `wasm-opt` step first (`wasm2js -O` on a raw rustc module aborts on an unaligned store).
`--alignment-lowering` (rustc emits many `align=1` loads and stores) costs 0.3% on the Oz pipeline and keeps the output
valid if the optimiser ever leaves an unaligned access. Build time on this machine: `wasm-opt -Oz` 35 s, `-O3` 55 s,
`wasm2js` 10-35 s plus 1-1.5 min for the whole script (all single-threaded node).

**The opt-level 3 build was not safe to convert; resolved (G22, 2026-11-10).** Before the fix, wasm2js of the `3` module
gave byte-identical PDFs, navigation and link boxes but **different page messages** (f8__7__0: 51,757 B in wasm, 53,200 B
in JS; the same with or without `wasm-opt`, `--alignment-lowering`, `--llvm-nontrapping-fptoint-lowering`). The 2026-11-06
reading (V8 canonicalising the payload) was wrong; the cause is **wasm2js**, found by a census of the NaN constants in the
disassembly (`wasm-dis`): the `3` module held **22 `f64.const nan:0x4000000000001`** in 9 functions (that is
`OptF64`'s absent marker `0x7ff4_0000_0000_0001`: `some()` was a bitcast of a double, so at opt-level 3 LLVM kept the value in
f64-typed select/phi nodes and materialised the marker as an f64 constant), plus 1,083 canonical `f64.const nan`, 15
canonical f32 and **1 `f64.const nan:0x8deadbeef0001`** (the second marker, `UNDEF_BITS` of uo-image `tiff.rs`). The `z`
modules had none of the `OptF64` marker (it stayed an `i64.const`) but held the TIFF one (`nan:0x8deadbeef0001`). wasm2js prints every NaN constant as the JS literal
`NaN` (the raw JS of the `3` module has 1,099 `NaN` and no payload), and JavaScript cannot carry a payload, so `get()`
saw a present NaN instead of "absent". **The hazard was in the Rust source, not in wasm2js's arithmetic or V8.** There was
no regression: the `3` path never passed (`ba3e15b9`, the commit that added the test, already recorded it); the G4 reviewer's
"fails on the previous base too" is the same failure, and the z build passed all along.

**Fix (2026-11-10).** (1) `OptF64` is an integer, **0 = absent, a value = its canonical bits + 1** (never a NaN payload; a
canonical NaN is never `u64::MAX`, so the sum cannot wrap to 0; still 8 B, `Item` still 48 B). The wire is unchanged
(`OptF64` never reaches it: `to_wire` goes through `Option<f64>`), so no schema and no codec change; this is G22's own
alternative "any non-NaN representation" (a presence bit would make it 16 B and break the `Item` budget). (2) uo-image
`UNDEF_BITS` is now a plain NaN with `is_nan` (exact: the only producers push u16/u32 values and rationals with a
denominator of at least 1, so no other NaN can be stored in a tag). (3) `build-wasm2js.mjs` runs a **NaN-constant census**
on the optimised module (`wasm-dis`) and exits 2 on anything but the canonical `f64.const nan:0x8000000000000` /
`f32.const nan:0x400000` (`--allow-nan-payload` skips it). Measured: the guard on the old `3` module exits 2 listing the 22
`nan:0x4000000000001` and the 1 `nan:0x8deadbeef0001`; on the new `3` module the census is 1,061 canonical f64 + 15 canonical
f32, on the new `z` module 135 + 4.

Evidence after the fix (HEAD ec66f1a1 + this change, `--features write`): modules `3` 3,967,023 -> 3,962,975 B, `z`
3,125,747 -> 3,123,638 B; wasm2js (default pipeline, minified) `3` 7,293,236 -> **7,290,722 B**, `z` 5,240,248 ->
**5,235,996 B** (the old `3` JS was built with `--allow-nan-payload`). `write_wasm2js.test.mjs`: **5/5 on both** (`3`: 38.5 s;
`z`: 45.8 s; before, `3` failed after 2.2 s with the f8__7__0 lengths above). `core/tools/size/wasm-ab.mjs --runs 9` on
base 3 / new 3 / base z / new z: the **four digests are equal** (`cca8d8794ec4bfe0`, everything `collect` reads over the 12
fixtures); min total ms 496.2 / 493.7 / 734.8 / 734.2 (medians 508.1 / 507.7 / 746.3 / 747.5): within noise. Browser, headless
(`wasm2js_run.mjs --mode correct --mode csp`, `new 3` module + its JS): Chromium 156 (chromium-1248) and WebKit 26.5: all 12
fixtures `diff=null`, the three pinned PDFs equal to native (c28b3c53 / a2cf1bd6 / 88d31b5b), the strict CSP (`script-src
'self'`) falls back to the js engine, `'wasm-unsafe-eval'` picks wasm. Parity nets unchanged: uo-render, uo-pdf, uo-docx-layout
(adapter, f3, f5, f6, f8, g8, layout parity), uo-wasm, uo-ffi, uo-image (`image_parity`) all green; `write_api` (pinned
checksums), `write_section`, `paint_parity` (Parts A-C) green on the `count` O3 build. Not covered by the census: a NaN
payload built at run time (`f64::from_bits` of a computed integer); the source has none (grep).

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
3. If it is shipped: keep `wasm-opt -Oz --alignment-lowering` + `wasm2js -Oz` and put `write_wasm2js.test.mjs` (and the
   browser check) in the gate. Since G22 (2026-11-10) the JS engine may be built from the shipped opt-level 3 module too
   (not only from `z`); the NaN-constant census in `build-wasm2js.mjs` is a hard build guard, so a NaN-payload marker
   that comes back in the Rust source fails the build instead of the output.
4. `wasm-opt` on the wasm module: worth running for the 10-17% smaller raw module and ~10% speed, not for transfer size
   (gzip/brotli do not improve).

Commands (all outputs under `core/target/`, never committed):

```
npm install --prefix core/target/tools/binaryen binaryen                                  # once; wasm-opt, wasm2js (not a repo dependency)
export CARGO_TARGET_DIR=<target>; cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm -j 4
CARGO_PROFILE_WASM_OPT_LEVEL=z cargo build ...                                            # the z module
node core/target/tools/binaryen/node_modules/binaryen/bin/wasm-opt -Oz --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --enable-multivalue --enable-reference-types in.wasm -o out.wasm
node core/crates/uo-wasm/tools/build-wasm2js.mjs --in <uo_wasm.wasm> --out core/target/wasm2js/uo_wasm.js [--wasm-opt Oz|O3|none] [--wasm2js-opt Oz|O3|none] [--no-minify] [--allow-nan-payload] [--esbuild <dir>]   # prints the NaN-constant census, exits 2 on payload constants
UO_WASM_PATH=<.wasm> UO_WASM2JS_PATH=<.js> node --test crates/uo-wasm/tests/write_wasm2js.test.mjs
UO_PLAYWRIGHT=<playwright dir> [UO_CHROMIUM=<chrome.exe>] node crates/uo-wasm/tests/browser/wasm2js_run.mjs --wasm <.wasm> --js <.js> [--browser chromium|webkit] [--mode csp|correct|speed]
node core/crates/uo-wasm/tools/bench-wasm2js.mjs --engine wasm --wasm <.wasm>             # or --engine js --js <.js>; one engine per process (maxRSS)
```

Files: `js/write.mjs` + `write.d.ts` (`loadWrite`, `canCompileWasm`), `tools/build-wasm2js.mjs`, `tools/bench-wasm2js.mjs`,
`tests/wasm2js_common.mjs`, `tests/write_wasm2js.test.mjs`, `tests/browser/wasm2js_page.mjs`, `tests/browser/wasm2js_run.mjs`.
Not done: Safari 15 itself, Firefox, a corpus beyond the 12 fixtures, wasm2js of the `count` build
(`uo_memory_live_bytes` returns an i64, which the JS build would give as its low half).

## 7. Security

- Every export catches panics (native) or relies on no-panic code (wasm, `panic = "abort"`); no `unwrap`, `expect`,
  `panic!` or panicking index in `src/`; inputs are length-checked (`parse_faces`, option sizes, UTF-8).
- Untrusted caps (§1) on the server by default; the reader, layout and PDF limits are the crates'.
- The host import receives a pointer into wasm memory and must copy before returning (documented; the JS wrapper does).
- Fuzz: `core/fuzz` target `ffi_docx_to_pdf` runs the whole server pipeline through the C ABI (G21, done); the conversion runs on a 16 MiB thread; `Trust::Untrusted` opens
  with the C# package limits (256 MiB per part, 512 MiB in total); the cancel flag is checked before each raster effect. Open: the paint work budget (G24).

## 8. Shipping, CI and WebKit (2026-11-10)

**Licence notices of what ships.** `chunks/uo-write.wasm` (inside `@universal-office/write` and `@universal-office/document-viewer` since `build-pkg` step 1c) and `uo_ffi` (the native library of the NuGet `UniversalOffice.Docx`) contain compiled third-party Rust. CLAUDE.md requires the licence texts inside the package, so:

- `core/tools/licenses/gen-notices.mjs` builds `core/licenses/` (committed: `THIRD-PARTY-NOTICES.md`, `notices.json`, `texts/`) from `cargo tree` of the two artifacts (`-p uo-wasm --features write --target wasm32-unknown-unknown`; `-p uo-ffi --target all`), the licence files at the root of each registry crate in `~/.cargo/registry/src` (no network), the texts kept in workspace crates and the toolchain's own notices. `node tools/licenses/gen-notices.mjs --check` (CI job `deny`) rebuilds in memory and fails on any difference (toolchain files are checked for presence only, since they vary with the CI's rustc). An SPDX expression must have one OR-alternative whose AND-terms are all in the `[licenses] allow` list of `core/deny.toml` (MIT, Apache-2.0, BSD-3-Clause; override for tests: `UO_LICENSE_ALLOW`); otherwise the generator exits 1 naming the crates (checked with an allow list of BSD-3-Clause only).
- `tools/build-pkg.mjs` `shipEngineWasm(id, dist)` (exported; the former step 1c): copies the wasm, then `verifyNotices` (notices present, `lock` map equal to the registry packages of `core/Cargo.lock`, every listed file present) and copies the notices to `licenses/uo-write-wasm/`; any failure is rethrown as "leva chunks/uo-write.wasm sem os avisos de licença" and fails the build (a missing wasm still only warns, SW-9). Checked: ok; empty notices dir throws; a notices.json with one edited lock version throws. The `wasm` CI job runs it on the `--features write` build.
- NuGet: `UniversalOffice.Docx.csproj` packs `core/licenses/**` under `licenses/rust/` (property `UoRustNotices`) and the target `UoCheckRustNotices` fails the pack (with a native library) when the index is missing. Checked with `dotnet pack` over a fake `uo_ffi.dll`: 27 files under `licenses/rust/`; with an empty `UoRustNotices` the pack fails. The same edit fixed an existing defect: `PackagePath="runtimes/%(RecursiveDir)native/"` made NuGet append the relative path again (`runtimes/win-x64/native/win-x64/uo_ffi.dll`); it is now `.../native/%(Filename)%(Extension)` (`runtimes/win-x64/native/uo_ffi.dll`).

| Component | Version | Licence | wasm | native |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | yes | yes |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | yes | yes |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | yes | yes |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | yes | yes |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | yes | yes |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | yes | yes |
| ryu-js | 1.0.3 | Apache-2.0 OR BSL-1.0 | yes | yes |
| V8 + ARM optimized-routines (uo-jsnum math) | - | BSD-3-Clause AND MIT | yes | yes |
| fflate 0.8.3 (inflate of uo-opc; new `crates/uo-opc/LICENSE-fflate`) | - | MIT | yes | yes |
| Unicode tables of uo-text (ICU of Node 24.15) | - | Unicode-3.0 | yes | yes |
| Rust standard library (COPYRIGHT-library.html, MIT, Apache-2.0) | toolchain | MIT OR Apache-2.0 | yes | yes |
| hyph-utf8 patterns (uo-fonts `hyphen-data`) | - | see the notice file | no | yes |

Size: 401,593 B (392 KiB, 27 files; 279 KB of it the Rust std copyright page) uncompressed in each package that ships the wasm and in the NuGet.

**CI lines for G4 and the switch** (workflow `rust-core.yml`): G4: `cargo test --workspace --all-features` (test job) already runs `g4_cancel` (debug bound 2 s) and uo-ffi `cancel_layout`; `server-pdf` now adds `cargo test -p uo-docx-layout --release --test g4_cancel` (bound 250 ms; measured 0.3-0.7 ms) and its `-p uo-ffi --release` line already runs `cancel_layout` (50 ms bound); `write_cancel.test.mjs` was already in the `wasm` job. Switch: new job `viewer` (JS install and build as ci.yml, `count,write` wasm, `npx vitest run test/engine-switch.test.ts` with `UO_WASM_PATH`, Chromium, `viewer_switch.mjs --mode all` with `UO_SW_STRIDE=6`, `UO_SW_SOAK=50`, `UO_TS_ROOT` = the same checkout: the script takes only tools from it and compares the two engines of the checkout; open fonts only). Measured locally with the frozen root: phase 0 requests for wasm/chunk 0, 13 documents / 108 pages, 0 differences in every row (sizes, headings/comments/links, text layer 119,856 characters, find 4,302 hits, pixels 216 renders 519,194,547 px tolerance 0, PDF 8,607,684 B, exceptions 0), memory 50 cycles flat (live 16,577,181 B, buffer 24,969,216 B, 0 handles after close). uo-docx: no per-file `--test` line remains; its tests are the single `all` binary covered by `--workspace`. WebKit and the painter job: uo-render.md "Milestone 1 — Shipping, CI and WebKit".

**Not runnable locally** (CI is the first run): `cargo deny check bans licenses` (cargo-deny not installed; the `[licenses]` keys may need a fix-up on first run), `npx vitest run test/engine-switch.test.ts` and the whole `viewer` job (no `node_modules` in this worktree), `playwright install --with-deps`, `node tools/build-pkg.mjs write` end to end (only `shipEngineWasm` was run).

**Decisions for the maintainer.** (1) The generated notices are committed (reviewable; no cargo, registry or rustc needed at pack time) and checked in CI. (2) Conservative scope: fflate, Unicode-3.0 and the whole Rust std copyright page are shipped; each can be dropped. (3) `core/deny.toml` gained a `[licenses]` allow list and the `deny` job runs `bans licenses`; until CI runs it, the list is enforced locally only by the generator. (4) `build-pkg` still only warns when the wasm is missing (SW-9) but fails when it ships without current notices. (5) The painter job costs about a frozen build plus 6 minutes per push; the `viewer` job a full JS install and build: both could be limited to `pull_request`/`workflow_dispatch`. (6) Whether a JavaScript engine's Math difference would be overridden in CI is moot today (WebKit had none).

## Milestone-1 gate fixes (2026-11-10)

The blocker and major findings of the milestone-1 security review, the skeptic and the benchmarks that are code, test or record defects
inside the approved design. Whatever needs a maintainer decision (an API shape, a result that differs from the frozen TS, a budget) is NOT
implemented: it is a row G23-G30 of "For the milestone-1 gate" with its evidence. Frozen TS: `UO_TS_ROOT` = `clickcompliance-docx` at
`d0f45bc5`; `CARGO_TARGET_DIR` = `core/target/agent-gatefix`; cargo `-j 4`, tests `--test-threads=3`.

| Finding | Done | Evidence |
|---|---|---|
| Security review, blocker: native stack overflow in the layout of nested tables (about 19 KB of release stack per level; `nest_tables_64.docx`, 1,508 B, kills the process on a 1 MiB thread) | **Fixed.** `uo_docx_to_pdf` runs the conversion on its own thread (`std::thread::scope` + `Builder::stack_size(CONVERSION_STACK_BYTES)`: 16 MiB, 64 MiB in debug); raw pointers and the job are read on the caller thread, the worker returns plain data (`LastError`, which `pipeline::set_last` records on the caller thread, where P/Invoke reads it); a worker panic becomes `INTERNAL`, a spawn failure too. The Rust API `Converter::docx_to_pdf` stays inline (documented: needs a large stack) | Reproduced first with the prebuilt release `bench_speed.exe` (main thread, 1 MiB): `nt_52` passes, `nt_56` and `nest_tables_64` abort ("has overflowed its stack", exit 127). New `uo-ffi/tests/hostile_inputs.rs` (fixtures `tests/hostile/*.docx`, byte-equal to the generators, `UO_BLESS=1` rewrites): 64 and 200 nested tables, 64 nested VML boxes and a 64-level mix convert through the C ABI **from a 1 MiB caller thread**, release (41 / 8 / 7 / 107 ms) and debug (245 / 47 / 45 / 623 ms: the 64 MiB debug constant is enough). Wasm: new `uo-wasm/tests/write_hostile.test.mjs` lays out and exports the same four (64 / 200 tables 2 / 4 pages) |
| Security review, major: the server opened documents with the 2 GiB per part / 4 GiB total limits | **Fixed.** `Trust::Untrusted` opens with the frozen C# `OpcPackageLimits` (256 MiB per part, 512 MiB in total: `UNTRUSTED_PACKAGE_PART_BYTES`, `UNTRUSTED_PACKAGE_TOTAL_BYTES`, `server::read_limits`); trusted keeps the target defaults | Release-only tests (ignored in debug, they inflate up to 512 MiB): three 200 MiB picture parts in a 2.87 MB file give `LIMIT_EXCEEDED`, stage READ, `max` 536,870,912; one 257 MiB part in a 1.23 MB file gives `max` 268,435,456. `server_parity`: 68 documents, oracle policy 68 equal, server-substitute 68 equal, the committed untrusted compressed PDFs unchanged |
| Security review, major: text-effect rasters have no budget; the PDF cannot be cancelled inside them (`glow_dense_600`: 61 s) | **Cancellation fixed, budget NOT done (G24).** Later the same day the PDF took the Word's resolution rule for large blurs ("Effect resolution as the Word"): this page now takes about 1 s. uo-pdf checks the cancel flag before every text effect (`paint_run_fx`), shape effect (`paint_shape_effects`) and picture effect (`make_image`, the adjust paths): `Painter::stop_for_cancel` marks `Cancelled`, the item loop stops after the item. The wasm PDF still has no poll (G4-4, G24) | `glow_dense_600.docx` (4.6 KB, one page, 600 runs of a 78.7 pt glow): 59.5 s uncancelled (release, `bench_speed`, 18.4 MB PDF; the 100-run page 0.46 s, so the cost is superlinear). `uo-ffi/tests/pdf_cancel_effects.rs` raises the flag 300 ms in: `Err(Cancelled)` in **40-45 ms** (before: the next 256-item check, about 26 s apart on this page). uo-pdf `painter_parity` 3, `painter_nopanic` 4, `painter_memory` 5, `writer_*` all green: no output change |
| Security review, major: a trap leaves the viewer's single module usable | **Fixed.** `viewer.ts` keeps an `Instance` (dead flag, handles, page environments) per loaded module; every call goes through `call(inst, f)`: on `WebAssembly.RuntimeError` the instance is **dead**, `loading`/`loaded` are reset and the next `loadViewerModule` instantiates a fresh module; objects of a dead instance read as freed, their `free()` is a no-op, any other call throws "the engine instance stopped after a trap"; `ViewerModule.dead`; `rust-engine.ts` drops a dead module in `open()` and `pageFontsFor` replaces the freed page environment. Count build only: export `uo_test_trap` (`unreachable`) and the test hook `__trapForTest`. **Not detected: the wasm2js engine** (its traps are plain JS errors; G26) | `viewer_trap.test.mjs`: trap throws `RuntimeError`, `mod.dead`, frees do not throw, `open`/`newEnv`/`pdf` refuse, a second `loadViewerModule` is a different live module whose PDF is byte-equal (245,286 B), 0 handles after the frees, a second trap on the fresh module works the same way. `viewer_switch --mode faults` (Chromium): element A (12 pages) survives the trap, reloads another document (`udv-loaded`, 12 pages = TS), a new element B opens, 0 handles after removal |
| Security review, major: a full page environment fails every later document | **Fixed (fallback, G27).** `RustSession.push()` catches `WriteError` (OverBudget, an unreadable face): the family stays on the substitute, one `console.warn`, the answer is not retried; anything else (a trap) still rises. The page environment stays full until reload | `write_hostile.test.mjs` M5: 8 MiB faces with distinct content keys fill the 128 MiB ledger, **the push of face #15 fails with `WriteError` code 5**; the environment still lays the fixture out to a PDF byte-equal to a fresh environment's; after `free()` live bytes are back at the baseline. `viewer_switch --mode faults`: `pushFamily` made to throw `WriteError(5)` 10 times; the document still loads (`udv-loaded`, no `udv-error`; 18 pages against the TS's 17, because every family is the substitute), and the next document loads |
| G21 / security review: no fuzz target over the pipeline | **Done.** `core/fuzz` target `ffi_docx_to_pdf` (`fuzz/src/pipeline.rs`): the whole server pipeline through the C ABI `uo_docx_to_pdf` (selector: trusted, `max_pages`), a 3 s watchdog raises the cancel flag, invariants: no `INTERNAL`, `uo_last_error` equals the status, an OK output is a PDF. Seeds: the 6 smallest layout fixtures and the hostile fixtures | Stable driver, 120 s: **96,040 executions (796/s), 0 crashes**, slowest 924 ms; fuzz crate `clippy -D warnings` clean (`Cargo.lock` gained uo-ffi's dependency set, offline). CI runs every file of `fuzz_targets` (`cargo fuzz run`, 60 s, `-timeout=10`) |
| F2-20 (S4): four captures stamped with the stale engine `b92b60fa` | **Done.** `f0`, `f1` and `f5` `layout-ci` and `f1/resolver` recaptured with the frozen `d0f45bc5` (`capture-layout.mjs --mode ci --count 7 --seed 1 --accept-stale typography/font/sfnt.ts`, `capture-f1-resolver.mjs --count 40 --seed 1`). The digests of **every** record differed from the old ones (the old p1, p2, p2first and p3 values came from the older engine; the facet set per key was kept): the record lines, the dump files (`p1.bin`, `f1.tsv`, `f5.bin`, `transcript.tsv`) and the headers were replaced | `adapter` 2 equal; `f1_parity` committed 10 equal, resolver sweep 40 documents / 80 equal; `f5_parity` 2 equal (lines 21, paint 21); `layout_parity` 5 tests (f7a 48, f7b 36, f7c 48, f7d 36 equal): **0 mismatches**. `grep -rl b92b60fa core/crates/*/tests` is empty. CI: the `layout-parity` and `painter` jobs now pin `ref: d0f45bc5` instead of `main` (S10) |
| M3 (retained memory after close) of the page environment | Measured, **approval is G29** | See below |
| Speed sentence (S3) | **Fixed** in "Open speed" (the rows slower than TS are G30) | Text only |
| Record and code minors (S5 and the skeptic's list) | Applied here: "16/16" (now 17/17), the `z` modules sentence, the licence size, the `pageHeads` sentence, the sizes and the G7 row, `NO_PAGE` frozen, the `build-wasm2js` usage string, the stage `rust.layout+decode` renamed `rust.layout`. The other minors are the orchestrator's | |

**M3 of the page environment (finding 11, reconciled).** The two earlier numbers came from different documents and from different histories of
the page: the SW run (`UO_SW_STRIDE=1`, `--mode memory`) and the section-8 run (`UO_SW_STRIDE=6`, `--mode all`, where the 13-document parity phase fills the
page environment with fonts before the memory phase). Same machine, count build of this HEAD, 200 open/close cycles of 10 documents:

| Run | live bytes after warm-up | `memory.buffer` | handles after close | page envs | Before SPEED-1 (SW, stride 1) |
|---|---|---|---|---|---|
| `--mode memory`, stride 1 (the SW's 10 documents) | **13,610,330 B** (1 distinct value) | 20,643,840 B | 0 | 1 | live 4,850,392 B, buffer 17,039,360 B: **+8,759,938 B (+181 %) live, +3,604,480 B buffer** |
| `--mode memory`, stride 6 (other 10 documents) | 12,569,061 B | 20,447,232 B | 0 | 1 | not measured before SPEED-1 |
| `--mode all`, stride 6, soak 50 (parity first, the section-8 run) | 16,577,181 B | 25,362,432 B | 0 | 1 | the section-8 line (16,577,181 B, 24,969,216 B) is this run: live equal to the byte |

Flat in all three (one distinct live value, one buffer size, 0 handles, 1 page environment). The growth is the page environment (the fonts every
viewer of the page shares, SPEED-1) that now lives as long as the page: over the M3 10 % rule, so it needs the maintainer's approval (G29).
Retained memory per document, after close, is still 0 (handles) and flat.

**Process notes.** `assert_no_leak` (thread-local counters) cannot see a worker thread: `uo_alloc::check_no_leak_global` / `assert_no_leak_global` compare the
global live bytes (valid while no other thread allocates; the callers hold a serial lock). The first run of `cancel_layout` with them failed by -24 B: the
raiser thread frees its spawn bookkeeping when it starts, inside the window; the test waits 20 ms for it to start. After that, `ffi_memory` M1 (global): 0 B left
for ok, refused, cancelled and page limit, **three consecutive runs equal** (allocated 39,468,390 / 761 / 8,699 / 21,084,607 B); `cancel_layout` M1 0 B left
(about 72 MB allocated), latency 0.6-1.4 ms. M3 now measures `Converter::docx_to_pdf` inline (the C ABI converts on a worker): numbers in §6, comparable to the old ones
(allocated 39.2 / 87.3 / 68.7 / 135.8 MB, peaks equal); the f8__7__2 PDF is 323,092 B (the old line said 323,120 B; `server_parity` pins the current bytes).
Verification, all green: `uo-ffi` release (`UO_SERVER_PARITY_EVERY=1`: 4 unit + cancel_layout 2 + ffi_memory 4 + hostile_inputs 4 + pdf_cancel_effects 1 + server_parity 3) and debug (hostile bombs ignored);
`uo-pdf` release (all 8 files); `uo-alloc --features global` 8; `write_native` 7, `write_unsafe` 1, `g4_cancel` 6; Node `write_api` 8, `write_cancel` 7, `write_section` 2 (17), `write_hostile` 5, `viewer_trap` 1;
`viewer_switch --mode all` (stride 6, soak 50): 13 documents / 108 pages, 0 differences in every row (text layer 119,856 characters, 4,302 find hits, 216 renders 519,194,547 px at tolerance 0, PDF 8,607,684 B),
and `--mode faults` ok; `dotnet test UniversalOffice.Docx.Tests` 14/14 (the facade over the worker thread: M4 global, thread soak, cancellation); Miri `-p uo-ffi --lib` 4/4;
`node tools/check-changed.mjs` clean (clippy native + wasm32 with `-D warnings`, float-cast and grep bans); the codec-job `tsc` line clean (viewer.ts, viewer.check.ts, write-engine.d.ts);
`rust-engine.ts` type-checked with a temporary paths tsconfig: 0 errors in it (8 unrelated "cannot find module fflate / converter" errors of the worktree, which has no `node_modules`).
Not run here: the vitest engine-switch test and `rust-core.yml` itself (the branch was never pushed, G30-CI).

**Files touched.** `uo-ffi/src/{lib,pipeline,server}.rs`, `uo-ffi/tests/{ffi_memory,cancel_layout}.rs` (global counters) and new `hostile_inputs.rs`, `pdf_cancel_effects.rs`, `tests/hostile/`;
`uo-alloc/src/lib.rs` + `tests/gate_global.rs`; `uo-pdf/src/{render,picture}.rs`; `uo-wasm/src/lib.rs` (`uo_test_trap`), `js/viewer.ts`, `tests/{viewer_trap,write_hostile}.test.mjs`,
`tests/browser/viewer_switch.mjs` (`--mode faults`, stage rename), `tools/build-wasm2js.mjs` (usage); `write/packages/viewer/src/{rust-engine.ts,write-engine.d.ts}`; `fuzz/**`;
`uo-docx-layout/tests/captures/{f0,f1,f5}`; `.github/workflows/rust-core.yml`.

## Effect resolution as the Word (2026-11-10, Rust only)

Decided by the maintainer: the PDF of a text effect with a large blur follows the Word, in Rust only (the frozen TS keeps
200 dpi). Measured in Word 365 with `ExportAsFixedFormat` (one run per radius; lab scripts in `core/target/pdf-time/`,
outside git), the effect layers of the PDF are an RGB image with an SMask, `/Interpolate false`, **one image per effect**
(the shadow first, then the glow; one per line when the runs are equal) and their resolution **falls with the blur**:
`200 / k` dpi, `k = 1` under the step, else `max(2, ceil(x / step))`, step = 20 pt of glow radius and 10 pt of shadow
blur (the same Gaussian sigma, 10/3 pt). Glow: 19 pt 200 dpi, 20-40 pt 100, 41-60 pt 66.7, 61-80 pt 50, 81-100 pt 40,
150 pt 25. Shadow: 2 pt 200, 10-20 pt 100, 21-30 pt 66.7, 40 pt 50, 41 pt 40, 60 pt 33.3, 61 pt 28.6, 80 pt 25; the
distance does not count. The reflection steps near 17.5 pt and changes regime past about 46 pt (an image of about 125 pt at
12 dpi): **not ported**, it stays at 200 dpi.

| | Done | Evidence |
|---|---|---|
| uo-pdf `fx_downscale` / `fx_downscales` / `paint_fx_layer` (`render.rs`) | When the glow and the shadow are both under their step, the one layer of the frozen TS (byte-equal). Otherwise a layer per effect, each `fx_layers` call with only that effect (and the outline, which grows the silhouette): the reflection at 200 dpi, the shadow at `FX_PPT / k_shadow`, the glow at `FX_PPT / k_glow`, the cancel flag checked before each | Unit test `the_downscale_follows_the_word` (the measured table, also through EMU). `uo-ffi/tests/pdf_cancel_effects.rs` `a_large_blur_goes_at_the_words_resolution`: `glow_dense_600` writes 600 layers at 50 dpi; glow 5 + shadow 4 one layer at 200 dpi; glow 5 + shadow 40 the shadow at 50 dpi then the glow at 200 dpi |
| `glow_dense` fixtures | The generator gives each run its own colour, as the SEC-M1-2 record says (the committed files had one colour, so the runs merged into 16 line layers and the page took 2.7 s, not the recorded 61 s). Re-blessed | `committed_hostile_fixtures_are_the_generator_output` |
| The cancel test | No longer on `glow_dense_600` (now about 1 s in release): one run of 60,000 characters with a 19 pt glow (200 dpi, about 500 line layers) | Release latency 6 ms, debug 1.4 s (bound 1 s / 10 s) |

Times, the same machine (`glow79pt_600c` = `glow_dense_600`; the others are one colour, so a layer per line):

| Document | TS PDF (Chromium) | Rust PDF before | Rust PDF now | Word |
|---|---|---|---|---|
| 600 runs, 78.7 pt glow, 600 colours | 156 s, 18.8 MB | 44.1 s wasm, 59.5 s native | **1.29 s** wasm, 3.7 MB; native 1.08 s with the layout | 1.22 s, 1.7 MB |
| 600 runs, 79 pt glow, one colour | 22.8 s | 5.9 s wasm, 2.7 s native | **0.10 s** wasm | 0.17 s |
| 600 runs, 18 pt glow (the gallery's largest) | 0.82 s | 0.29 s | 0.29 s (unchanged) | 0.34 s |

`viewer_switch --mode parity`, every fixture (stride 1): 74 documents, 579 pages, **0 differences** in pages, text layer,
find, pixels and the PDF (no fixture has a glow of 20 pt or a shadow blur of 10 pt). uo-pdf all tests and uo-ffi all
tests (debug and release) green; clippy native clean.

**The Word's screen, measured after** (PrintWindow of Word's own window off-screen, not activated, zooms 100-500 %,
`core/target/pdf-time/screen-glow.ps1`; compatibility mode 15 and the mode of a file without settings give the same PDF):
- **Fast.** `ScreenRefresh` of the 600-colour page: 333 ms at 100 %, 150 ms at 200 %, 81 ms at 400 % (fewer runs in view),
  the same on a second refresh (no cache seen); the one-colour page 11-36 ms, the page without glow 8-50 ms.
- **Smooth at every zoom**, unlike its PDF (whose 50 dpi image, `/Interpolate false`, shows steps in a nearest-neighbour
  rasterizer): the screen profile is piecewise linear in places (slopes constant over 2-3 px at 100-400 % for an 18 pt
  glow, 3-7 px for a 79 pt one), so a raster below screen resolution scaled up bilinearly, but the step could not be
  measured reliably (long nearly linear ramps): **the screen resolution rule is not known**.
- **The same size at every zoom** (79 pt glow: 50-52 pt wide on screen with 17 pt of text; 150 pt: 81-85 pt; 18 pt: 21-23
  pt), 10-20 % wider than the visible part of its own PDF image (79 pt: 70 px against 57 px at 96 dpi) and far smaller
  than ours.

**The screen too (maintainer: "the PDF rule, smoothed").** uo-render `fx_plan(fx, ppt)` measures each blur in pixels of
`ppt` against those of 200 dpi, so the screen lowers a blur that spans as many pixels as one the PDF lowers (at 96 dpi
the first step is a 41.7 pt glow, at 192 dpi 20.8 pt); `fx_layer_set` paints the plan and is shared by the PDF and the
new export `uo_render_fx_layer_set` (`n`, then per layer `ppt, x0, y0, w, h, RGBA`; render.mjs `fxLayerSet`).
`paint.ts` takes it when the source has it (else `fxLayer`), caches the layers per run and draws a coarser layer with
`imageSmoothingEnabled = true` (bilinear, smooth as the Word's screen); a layer at the asked resolution is drawn exactly
as before, without touching the smoothing (the frozen painter's call log). A coarse layer **pools its silhouette** from a
grid `k` times finer (`fx_layers_pooled`): by the maximum for a glow, by the mean for a shadow or a reflection. Without
it a 150 pt glow at 25 dpi vanished (alpha at most 1/255: the 12 pt letters covered no pixel centre).

| 600 runs, 78.7 pt glow, 600 colours (wasm, Chromium) | 96 dpi | 192 dpi | PDF |
|---|---|---|---|
| before | 6.2 s | 39.7 s | 44.1 s |
| now | **1.5 s** | **2.6 s** | **1.4 s** |
| the Word | 0.33 s (`ScreenRefresh`) | 0.15 s | 1.2 s |

`viewer_switch --mode parity` (every fixture): 0 differences; `paint_parity` Part B (200 generated pages, every
painter branch, 4 configurations): 0 log mismatches; `render_paint`, `paint_memory`, uo-render/uo-pdf/uo-ffi/uo-wasm
native suites, the shell's `tsc --strict` and clippy native + wasm32: green. New tests: `fx_plan` scaling (uo-render),
`the_layer_set_follows_the_plan` (render_native: byte-equal to `fx_layers` under the step, 3 layers at 10, 5 and 10
px/pt past it). Side by side with the Word (`core/target/pdf-time/compare/comparacao-zoom{100,200}.png`, outside git):
our screen before and now look the same; **our glow was much larger than the Word's** (the cause and the fix: "Text effects scale with the font size").

Open, not done here:
- ~~The glow is larger than the Word's~~: fixed, "Text effects scale with the font size".
- **The glow grows the line in the Word**: 17 pt between lines without it, 19.5 / 26 / 56 pt with 5 / 18 / 79 pt (so the
  79 pt document is 2 pages in the Word and 1 in ours). The layout does not do it (TS too).
- The reflection's rule, the picture and shape effects (`paint_shape_effects`, `make_image`) and the TS: unchanged.

## Text effects scale with the font size (2026-11-10, Rust only)

**What was wrong.** Side by side with the Word (`core/target/pdf-time/compare*/`), our glow was far larger than the
Word's on 12 pt text (79 pt glow: about 79 pt past the text against the Word's 18-28 pt). The glow's own profile was
not the cause: on a straight edge (three full blocks U+2588, Arial 72 pt, the effect image's SMask read from the Word's
PDF) the Word's glow is our model exactly, `alpha = Phi((rad/2 - d) / (rad/6))` (half alpha at `rad/2`, sigma
`rad/6`, within the measurement). The model came from the picture effects ("medidos no Word" in uo-drawingml), where
there is no font size, and the TS reused it for text unchanged. **The Word sizes a text run's effects by its font
size**: sweeping 6-200 pt text with glows of 5-150 pt, the sigma is `rad·f/6` with `f = (size/72)^p`, the fit giving
p = 0.692 and 72.7 pt (within 2 % over every point; we use `ln 2` and 72). The shadow's blur (`blur·f/3`, 12-144 pt)
and distance (a blur-free 20 pt shadow moves 5.8 / 20.2 / 32.4 pt at 12 / 72 / 144 pt) scale the same way. So the
frozen TS (and we until now) are right at 72 pt only: 3.5 times too large at 12 pt, 2 times too small at 200 pt.

The same scale explains the resolution rule's odd step: in scaled lengths both sizes have one step (0.96 pt of sigma),
so the PDF's `k` comes 3.5 times sooner on 72 pt text (glow 5 pt 200 dpi, 10 pt 100, 18 pt 50, 30 pt 33.3, 60 pt
18.2, 100 pt 11.1, 150 pt 7.4). `GLOW_STEP_PT` / `SHADOW_STEP_PT` are now in scaled lengths (20 and 10 pt at 12 pt
text); at 72 pt `ceil` gives 26 where the Word wrote 27 for the 150 pt glow.

**Done.** uo-render `word_fx_scale` / `fx_for_word` (glow radius, shadow blur and distance; the reflection stays
unscaled, not measured); `fx_layer_set` (the screen) and uo-pdf `paint_run_fx` scale first, then plan. `fx_layers` and
the export `uo_render_fx_layers` stay the frozen TS's (unscaled): `paint_parity` gives the D painter that path
(`fxLayerSet` removed from its source) and still finds 0 mismatches; the PDF has `PdfOptions::frozen_text_fx` (default
false) for uo-pdf `painter_parity`, which compares with the TS's files and is still equal. `viewer_switch` (74
fixtures, none with text effects): 0 differences. Our glow against the Word's on the straight edge
now (sigma, the Word's / ours): 12 pt text 0.96/1.07, 3.79/3.94, 7.09/7.12 (glow 20, 80, 150); 24 pt 6.09/6.25; 48 pt
9.85/10.17; 72 pt 3.32/3.45, 13.10/13.61, 24.62/25.48: ours 1-4 % wider (before: up to 3.5 times); half alpha ours
0-10 % farther (the Word's dilation looks nearer `0.46·rad` than `rad/2`, inside the method's noise: the Word's top
and left edges disagree up to 25 % on some sizes, ours do not). Tests: `effects_scale_with_the_font_size` (the measured
sigmas within 2 %, the shadow), `the_plan_scales_with_the_resolution` (the 12 pt and 72 pt tables), render_native
(the 24 pt fixture's layers smaller than the frozen TS's, 3 layers at 10 / 5 / 5 px/pt past the steps), uo-ffi
`pdf_cancel_effects` (the cancel page is now 72 pt text with a 5 pt glow: still seconds of 200 dpi layers).

**Found on the way (not fixed here):**
- **The reflection's distance**: a 10 pt `w14:dist` drew no gap in the Word at 72 and 144 pt (the reflection starts at
  the text's bottom); ours moves it. Its blur and whether it scales were not measured.
- **A blur-free shadow** is vector in the Word's PDF (a copy of the text), an image in ours.
- **The glow grows the line** in the Word (above), not in ours.
- **A document without a styles part**: the Word uses Aptos 12 pt, ours a smaller serif (seen in every comparison).
- **The Slides** (`shared/drawingml` model, the same text-effect raster): whether PowerPoint scales text effects by the
  font size too was not measured (PowerPoint's COM opens windows).
- The silhouette of a coarse layer (fixed above, `fx_layers_pooled`).

## For the milestone-1 gate

| # | Decision taken here (outside the existing plans) | Alternative |
|---|---|---|
| G1 | The read path is **one file** `uo-ffi/src/pipeline.rs`, included by uo-wasm with `#[path]` (no new crate; no `#[no_mangle]` export leaks from one cdylib into the other) | move it into uo-docx-layout (`pipeline` module) |
| G2 | The last error is **per thread** (`uo_last_error(UoErrorInfo*)`), not per handle as D3 §4 sketched; D3's exports adopt it | per-handle slot |
| G3 | Codes 10 cancelled, 11 no font, 12 internal (D3 said a panic is code 2), 13 sink added to D3's 1–9 | — |
| G4 | **Done (2026-11-09).** `LayoutOptions.cancel` (shared flag, Relaxed load at every `Budget::charge`) + `cancel_poll` (every `CANCEL_POLL_UNITS` = 4,096 work units, first at the first charge) + `LayoutError::Cancelled` (latched, wins over `Limit(Work)`); uo-ffi: code 10, stage none, the server passes the same Arc to layout and PDF; C# facade unchanged (token -> `UoCancel` -> `OperationCanceledException`); wasm: opt-in host import `uo_host.layout_poll` (flag bit 9, JS `LayoutOptions.cancelCheck`, **G4-W**), a throwing callback counts as cancel. Latency from raising the flag mid-layout (650-paragraph document, release): native 0.16-1.4 ms, C ABI 0.7-1.4 ms, .NET facade 0.6-1.5 ms (ranges over runs) (before: the rest of the layout, hundreds of ms); M1 0 B left (native layout, C ABI, wasm 20 cancelled layouts cancelled mid-layout at the k-th poll, .NET 5 + 1); cost within noise (B/A -2.9% to +0.4%, C/A -0.3% to +1.5% over median/min on pages100 and pages500, one +17.5% outlier min, noisy machine); layout parity unchanged (layout_parity 5, f8_parity 2, g8 4, f7a 7, server_parity 3; wasm-ab digest cca8d8794ec4bfe0 equal, layout min 366.1 -> 367.9 ms). Record: uo-docx-layout.md "Stage 3 — G4" | the PDF on wasm and the `.doc`/reader stay uncancellable inside (G4-4) |
| G5 | Untrusted caps: kernel radius 256, masks/effect layers 16 Mpx, images 40 Mpx (first numbers); the facade defaults to untrusted | measure on the corpus first |
| G6 | The wasm PDF leaves through an **import** (`uo_host.pdf_chunk`): the viewer module cannot be instantiated without imports | pull the chunks (the output then stays in wasm memory, which never shrinks) |
| G7 | `write` is **not a default feature** (the default module stays the 15 KB foundation build the other tests use); the shipped viewer module is `--features write`, **3.96 MB (about 1.405 MB gzip) after the size work (`size-optimization.md`)** — a size budget, `wasm-opt`, and trimming uo-docx are open | make `write` default |
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
| G21 | **Done (2026-11-10)** for `ffi_docx_to_pdf` (whole pipeline through the C ABI; 96,040 stable-driver executions in 120 s, 0 crashes); `wasm_write_session` is covered by `write_hostile.test.mjs` and the no-panic tests, not by a fuzz target | a wasm session target |
| G22 | **Done (2026-11-10).** `OptF64` is an integer (0 = absent, value = canonical bits + 1; 8 B, `Item` still 48 B; no schema/codec change, it never reaches the wire); the second NaN-payload marker (uo-image `tiff.rs` `UNDEF_BITS`) is a plain NaN with `is_nan` (exact: no other NaN can be stored); `build-wasm2js.mjs` fails on NaN-payload constants; wasm2js of the opt-level 3 module now equals wasm on the 12 fixtures (Node 5/5, Chromium, WebKit), output digests equal before/after. Record: "No-WebAssembly fallback" above. Was: `uo-render` `OptF64` marks "absent" with a signalling-NaN payload (`0x7ff4_0000_0000_0001`); NaN payloads are not guaranteed to survive outside wasm loads/stores (V8 canonicalises them on some paths — the `opt-level = 3` wasm2js build gives different page messages because of it, see the wasm2js section) | give `OptF64` an explicit presence bit (or any non-NaN representation) before any JS/wasm2js build ships; audit other NaN-payload markers |
| G23 | **Stack depth.** Every `uo_docx_to_pdf` conversion runs on its own thread of `CONVERSION_STACK_BYTES` (16 MiB; 64 MiB in debug). Evidence: release LTO uses about 19 KB of stack per nested-table level; on a 1 MiB thread `nt_52` passes and `nt_56` / `nest_tables_64` overflow (the reader allows 64 levels, the layout depth guard is 192), and an overflow aborts the process; with the thread, 64 and 200 levels convert from a 1 MiB caller thread (release and debug) | Keep the thread; also shrink the layout's per-level frames; or lower the layout's effective depth below the reader's `max_nesting` 64 (a result change against the TS only past that depth). The Rust API `Converter` still runs on the caller's stack |
| G24 | **Paint work budget for raster effects: not implemented** (the PDF side is much smaller since "Effect resolution as the Word": `glow_dense_600` 44 s → 1.3 s wasm, as the Word's 1.2 s; the screen painter is unchanged, 6.2 s at 96 dpi and 39.7 s at 192 dpi on that page; the budget stays open for the screen and for effects under the steps) (a budget, and it changes results against the frozen TS, which does the work uncapped). Evidence: `glow_dense_600.docx` (4,632 B, one page, 600 runs of a 78.7 pt glow) takes 61.3 s native, 69.8 s wasm trusted and 71.7 s wasm untrusted; every layer is under the per-call caps; the cost grows faster than the runs (100 runs 0.46 s, 600 runs 59.5 s). Now cancellable natively (40-45 ms from the flag, per-effect check). Open: the wasm `uo_layout_pdf` has no poll (G4-4) and the viewer's print and download run it synchronously on the main thread; the screen painter's `fxLayer` per run was not measured | A per-page and per-document budget (layer pixels x kernel work, like the layout's `Budget`) that drops the effects with a warning or fails with Limit, with its numbers; an opt-in poll for wasm `uo_layout_pdf` (reverses G4-4) or a deadline |
| G25 | **Package limits by trust (implemented).** `Trust::Untrusted` opens with the C# limits (256 MiB per part, 512 MiB in total); trusted keeps the target defaults (2 GiB per binary part, 4 GiB total natively). Evidence: with deflate's 1,032:1 a 4 MB upload could hold up to 4 GiB per conversion; the new release tests return `LIMIT_EXCEEDED` for 600 MiB over three parts and for one 257 MiB part | The C# limits for both trust levels (the frozen C# applies them to every caller), with an explicit facade option to raise them |
| G26 | **A trap kills the instance (implemented).** After a `WebAssembly.RuntimeError` the viewer's instance is dead and the next use loads a fresh module. Not covered: the wasm2js engine (`engine: 'js'`), whose traps are plain JS errors; V8 stack exhaustion inside a wasm call (a `RangeError`, not a `RuntimeError`), which `call()` does not treat as fatal (unreachable today: nesting is capped at 64 and 200-level documents pass in wasm); an element that is not the one that trapped keeps reading its old layouts as freed until it loads again | Accept, or detect traps of the js engine (which means telling its errors from the ordinary `WriteError`s) |
| G27 | **A full page environment (SPEED-1 with S7, implemented as a fallback).** A refused `pushFamily` (OverBudget on the shared 128 MiB ledger) keeps the substitute instead of failing the document; the environment stays full until reload (face #15 of 8 MiB is refused) | Rebuild the page environment when it is full (needs reference counting of the sessions that use it) |
| G28 | **The viewer's wasm memory never shrinks (S8).** One module per page (SW-10) and a page environment that is never freed (SPEED-1) keep `memory.buffer` near its high-water mark (about 192 MiB after a 500-page document) until reload, where the TS memory is collected; the viewer always uses the desktop font profile (SW-11). Nothing in the records stated this for iOS 15 | Drop and re-instantiate the module when no viewer holds a document; the phone profile on touch devices; or accept and document it |
| G29 | **M3 baseline of the page environment (S6).** Retained after close, 200 cycles of the SW's 10 documents: 13,610,330 B live and 20,643,840 B buffer against 4,850,392 B and 17,039,360 B before SPEED-1 (+181 % live, over the 10 % M3 rule); the section-8 line (16,577,181 B) was the `--mode all` run, whose parity phase fills the environment first; stride 6 alone 12,569,061 B. Flat, 0 handles, 1 page environment | Approve the page-lifetime baseline, or ask for a reduction (fewer faces kept, free the environment when no viewer holds a document: G28) |
| G30 | **Speed rows slower than TS, and CI.** (1) The wasm layout of the 500-page document is 1.40x TS in Node (5.9 s against 4.2 s; native 0.97x); per-page cost +45 % from 100 to 500 pages (native +9 %, TS +2 %); not tier-up (`--no-liftoff` 6.98 s against 7.12 s), not interference. Profile the allocator and `memory.grow` at the 192 MiB high-water, page faults and locality, or accept it as the stress case. (2) Chromium: S2 total 1.12-1.20x TS, S0 cold about 1.8x, wasm layout 9.0 s against 7.1 s, while the same module in Node is 0.94x TS; the tier-up explanation is unmeasured (Liftoff against TurboFan, code cache). (3) The Rust server PDF against the frozen C# was never timed. **CI (blocker, S1):** `rust-core.yml` has never run (the branch was never pushed; agents may not push), so the painter, viewer and fuzz jobs, `cargo deny bans licenses`, the dotnet matrix, LeakSanitizer and the Linux WebKit painter are unproven, as is everything in these records that says "CI covers it" | Measure the Chromium tier-up and accept or reject a Chromium-total gap above TS for the gate; push the branch (or a bundle to a private remote) and get one green run before the gate |
| G31 | **Security review items still open.** G5's untrusted caps (kernel radius 256, masks and layers 16 Mpx, images 40 Mpx) were never measured on the corpus beyond the untrusted equality of `server_parity`; the uo-drawingml blur/fill/morph time bounds; uo-docx F21/I2 (`max_numbering_levels` 9); real Safari 15 / iOS (BrowserStack needs `/mcp` re-authorisation) | Decide the numbers; run the sweeps |

**Benchmarks main × rust-core (2026-11-10; full tables in uo-docx-layout.md, "Benchmarks main × rust-core — speed" and "— memory").** Node 24, warm, TS / wasm / native
interleaved in one process, 68 oracle capture documents (568 pages) plus five generated classes up to 500 pages, byte-identical raw PDFs from all three engines
on all 73 documents. Total open + layout + PDF against the frozen TS: **native 0.90 and wasm 0.94 on the captures** (native 0.64–0.90, wasm 0.70–0.96 up to 100
pages); open 0.56 / 0.61 and PDF 0.67 / 0.77 always faster; **layout at parity only** (0.96 / 0.98 on the sum, 20 / 34 of 68 documents slower on that stage, worst 1.19).
Gaps: **wasm layout of the 500-page document is 1.40 × TS** (per-page cost +45 % from 100 to 500 pages, native +9 %, not tier-up; cause open), and the **Chromium** viewer
numbers of "Open speed" (S2 total 1.12–1.20 × TS, S0 cold about 1.8 ×, cited, not re-run). Memory re-measured and unchanged (Rust layout 0.23–0.41 × TS, 47.0 B/char
wasm on 500 pages). Not measured: the Rust server PDF against the frozen C#, WebKit timings, the phone profile. The earlier "as fast or faster than TS natively"
sentence of "Open speed" is only backed as "as fast on the corpus sum" (skeptic S3 answered; the benchmark harness is `core/tools/bench/speed/`).

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

SPEED (open speed of the Rust viewer, 2026-11-10): planner Opus, implementer Sonnet 5.5; wall time about 1 h 15 min (harness `--mode speed` and baseline 20 min, fixes A/B/C/D 25 min, full parity 6.5 min, three speed runs 9 min, record 10 min), about 95 tool calls. Parity rounds: 2 quick (stride 6, 0 differences) + 1 full (0 differences). Rework: a Python edit script truncated `viewer_switch.mjs` (cp1252 default; restored from git and rerun with UTF-8); one tsc error in the `FontFace` buffer cast. SPEED-5 (streaming compile) tried and reverted.

SW (the viewer TS/Rust switch, 2026-11-10): planner Opus, implementer Sonnet 5.5; wall time and tool-call counts were not recorded per step (estimates: about 4 h wall, a large part of it waiting for Chromium runs on a loaded machine; about 150 tool calls). Parity rounds: 0 mismatches on every comparison, first run. Rework: the `.wasm` default-source bug in the memory phase (the driver asked for the module before any viewer loaded it), a test timer that paused the page after 150 s (cleared per document now).

G4 (cancellation inside the layout): planner Opus, implementer Sonnet 5.5 (the maintainer's request; SKILL.md assigns the layout crate to Opus), see uo-docx-layout.md "Stage 3 — G4".

G8 (the Write section of the page message): Opus WIP 6471786d + Sonnet 5.5 finish (the maintainer's request; SKILL.md assigns the layout crate to Opus), see uo-docx-layout.md "Stage 3 — G8".

GATEFIX (milestone-1 gate fixes, 2026-11-10): planner (measured the code), implementer Sonnet 5.5; wall time and tool-call counts were not recorded per step (estimates: about 3 h wall, much of it release builds and the 600 MiB bomb runs on a shared machine; about 150 tool calls). Parity rounds: 0 (every parity comparison equal on the first run). Rework: the leak window of `cancel_layout` (-24 B from the raiser thread, found by the first global M1 run), the first M5 baseline taken before the module warmed up, the first PDF-cancel fixture too light (the 100-run page takes 0.46 s: the 600-run one is the 60 s case), the first M3 version used the untrusted converter (changed bytes by 28), and two edit scripts that wrote CRLF into LF files or were eaten by the shell.

G22 / wasm2js fallback parity (2026-11-10): planner Opus (measured the cause, reproduced the failure, built the z module), implementer Sonnet 5.5; about 1 h 30 min wall (the O3 wasm2js build is 1-3 min per run, four builds, plus the uo-ffi and uo-docx-layout release test compiles), about 45 tool calls. Rounds: 0 parity mismatches after the change (digests equal on the first run). Rework: the census regex lost its backslashes through the shell tool (the first guard run printed "none" for the old module; found because the old module is known to fail, fixed by editing the file); no other.

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

## Milestone-1 skeptic review (2026-11)

Opus implementation skeptic, 2026-11-10, over HEAD `ec66f1a1` plus the uncommitted stage-1 work of the SPEED, Shipping/CI/
WebKit and G22 (wasm2js) frontiers. The claim attacked: "milestone 1 is ready for the maintainer's gate". Read-only on source.

**Verdict: not ready yet.** The code of the read path is in good shape. Parity is exact wherever it was measured, every
binding handle has M1/M4 tests, and the TS default path is intact. The gate is still blocked by process items: CI has never
run, the milestone security review has not been done, the main × rust-core speed benchmarks are missing, the F2-20 recapture
is open and the stage-1 work is uncommitted. There are also two memory findings caused by SPEED-1 that the record does not
state.

### What I re-ran

- `node tools/licenses/gen-notices.mjs --check`: current, 27 files, 401,593 B.
- `node tools/check-changed.mjs` (agent-skep target, 6 crates): native clippy, float-casts and grep-bans pass. The wasm32
  `--all-targets` leg **fails**. The cause is the untracked `crates/uo-ffi/examples/bench_speed.rs`, which appeared during this
  review (another agent's benchmark work, with `tools/bench/speed/`). It imports `uo_ffi::pipeline` and `uo_fonts::system`,
  and both are native-only. Re-run without examples (`--lib --tests --benches`, wasm32): clean. The stage-1 changes are
  therefore lint-clean, but the tree is not, until that example is gated (`required-features` or `#![cfg(not(target_family =
  "wasm"))]`).
- `cargo test -p uo-render --lib` (the new `OptF64` test) and `cargo test -p uo-image --release`: green.
- Read through: the OptF64 and TIFF changes, the `rust-engine.ts` and `viewer.ts` diffs, the switch commit's TS diff
  (`1b585623`), the `rust-core.yml` diff, `build-pkg.mjs`, `deny.toml` and the `Cargo.lock` registry set.

### Findings

| # | Severity | Finding | Fix / owner |
|---|---|---|---|
| S1 | blocker (process) | The branch has never been pushed, so **no CI job has ever run**. That includes the new `painter` and `viewer` jobs, `cargo deny check bans licenses`, the `dotnet` matrix (the facade has only been tested on Windows), LeakSanitizer (G19) and the Linux WebKit painter. Every "CI covers it" in this record is still unproven. | Push (or bundle and push), get one green run, and record it before the gate. |
| S2 | blocker (process) | **The milestone security review has not been done.** SKILL "Hybrid mode" step 3 requires it once per milestone, and HANDOFF queue item 5 lists it. The records carry items deferred to it: uo-drawingml blur, fill and morph time bounds; uo-docx limits F21/I2; layout F2-21; G5 caps that were never measured on the corpus. | Run the review (Opus). It decides G5 and G21. |
| S3 | blocker (measurement) | **Speed benchmarks main × rust-core are missing.** The `uo-docx-layout.md` "Final review and benchmarks" section says "Pending" (oracle corpus, the 500-page document, Node and WebKit), and there are no server-PDF timings of the Rust facade against the frozen C#. The SPEED section also claims that "natively and in Node the same Rust layout is as fast or faster than TS", but no record contains a TS-vs-Rust layout timing that supports it. What *is* measured is worse than TS: the wasm layout in Chromium takes 9.0–9.1 s against 7.1–7.2 s (+27%), the S2 total is 9.4 s against 7.6 s, and the S0 cold open is about 2× (median 131–146 ms against 70–80 ms). | Finish the `tools/bench/speed` work now in flight. Either back the "as fast natively" sentence with numbers or delete it. Present every row slower than TS to the maintainer. |
| S4 | blocker (process) | **F2-20 is still open.** Four committed captures are stamped with the stale TS engine `b92b60fa`: `uo-docx-layout/tests/captures/f0/layout-ci.capture`, `f1/layout-ci.capture`, `f1/resolver.capture` and `f5/layout-ci.capture`. | Recapture them with the frozen `d0f45bc5` build (HANDOFF queue 5). |
| S5 | blocker (process) | The stage-1 work (G22, SPEED, shipping/licences) is uncommitted, and none of the 15 minor items its reviewers raised has been applied. Examples: line 290 still says "16/16, one new test", line 702 still says "457 KB", line 706 still reports a pageHeads failure that no longer reproduces, line 550 still says "z modules had 0 such constants", `NO_PAGE` is not frozen, and the build-wasm2js usage string still omits the new flag. | The orchestrator applies the record fixes, verifies and commits. |
| S6 | major (memory, unrecorded) | **SPEED-1 raised the retained memory after close, and M3 does not record it.** The SW memory mode showed 4,850,392 B live and a 17,039,360 B buffer after every close. The section-8 viewer run (tree with SPEED-1) shows 16,577,181 B live and a 24,969,216 B buffer. The SPEED record says "1 distinct value" but gives no number. That is over the 10% M3 rule unless approved (it may be partly a different document set, which nobody reconciled). | Re-run `--mode memory` on the same 10 documents and record before/after. The maintainer approves the new page-lifetime baseline (Liberation assets, pushed families, patterns). |
| S7 | major (robustness) | **A page-lifetime environment that fills up stays full.** SPEED-1 notes that the 128 MiB FaceLedger is shared, but not the consequence. `RustSession.push()` calls `pushFamily` without a `catch`, in `pushNow` and `fetchLater` and in the re-push loop of `relayout`. An `OverBudget` (`WriteError` 5) therefore escapes `relayout`, and the document fails to load (`udv-error`). Nothing is ever evicted from the page env, so every later document on the page that needs a new family fails the same way, until the page reloads. With Local Font Access, a few CJK families are enough. There is no M5 test that fills the page env past its cap. | Catch the error per family (fall back to the substitute, as TS does when a face is missing), or rebuild the page env when it is full. Add an M5 test. |
| S8 | major (maintainer decision) | **The viewer's wasm memory never shrinks for the life of the page.** There is one memoized module per page (SW-10), and the page env is never freed (SPEED-1). After one 500-page document the buffer stays near 192 MiB until reload, where TS memory is collected. The viewer also always uses the desktop font profile (SW-11). The baseline includes iOS 15, whose tabs die far below desktop limits. No record states this as a consequence. | Decide: drop and re-instantiate the module when no viewer holds a document, or the phone profile on touch devices, or accept and document it. |
| S9 | minor (reuse) | **Word-only rules are hard-wired into the shared `uo_pdf::render_pdf`.** Space glyphs are always inserted (`word_spaces`, called with `Some(&mut spaces)` for every top-level item, `render.rs:115-120`), the font size is quantized to 1/600" (0.12 pt, `render.rs:825`), and the text-effect layer is fixed at 200 dpi (`FX_PPT`). These are measured Word behaviours. A Slides or Sheet producer of uo-render pages gets them with no option. Twips as the display-list unit and the `page_fill` effects are Word-flavoured too, but they are documented and generic enough. | Add a `PdfProfile` (or per-page flags) in `PdfOptions` before Slides or Sheet become producers. Record it as a reuse constraint in `uo-pdf.md`. Not for the M1 gate. |
| S10 | minor (CI) | The `layout-parity` and `painter` jobs check out `ref: main` as the frozen reference. Locally `main` is the stale `b92b60fa`. On GitHub `origin/main` is `d0f45bc5`. If the security fixes listed for the published product (S1, S6 of D3) land on `main`, the parity reference moves silently. | Pin `ref: d0f45bc54c05fb026475d7b229efa498af8423c6` in both jobs. |
| S11 | minor (shipping) | `shipEngineWasm` ships whatever module sits at `core/target/wasm32-unknown-unknown/wasm/uo_wasm.wasm`. Locally that is often a `count,write,render-fixture` build, which has the counting allocator and test exports. Only the notices are verified, not the module. | Fail the build when the module exports fixture or self-test symbols or reports counting, or have build-pkg build the module itself. |
| S12 | minor (coverage) | `uo_write_env_push_asset` (the success path with an advance table) and `uo_write_env_clear_fonts` are not in the native M1 test `write_native.rs`. They are covered only at crate level (uo-fonts `registry_memory`) and on the error path in `write_api`. `uo_layout_page_heads` has the null-handle case only, and its empty case is not declared. | Add both exports to `write_native` M1, or state that the crate-level M1 covers them. |
| S13 | minor (record) | Stale numbers. Section 6 "Sizes" (5,915,444 B, 1.45 MB gzip) and the G7 row ("5.9 MB") predate the size work. The module is now 3,962,975 B raw and about 1.405 MB gzip (`size-optimization.md`). | Update both lines. |
| S14 | minor (record) | The §4 .NET API breaks (items 1–9) and the wasm/npm new API are not in `CHANGELOG.md`. Only the viewer switch is. SKILL says every break is listed there. HANDOFF defers this to the milestone-2 gate. | Keep as deferred, but say so in the gate list. |

### Checked and found fine

- **TS default path.** The switch commit changes the TS lines only by type widening, `hasDocument()` guards, a shared
  `fillTextLayer`/`webPageHeight` shape and the `advancesFile` extraction. The engine import is type-only. Phase 0 shows 0
  requests for the wasm or the engine chunk. The 21 existing vitest tests passed (SW). This round's changes touch only
  `rust-engine.ts` (Rust path) and `write-engine.d.ts`.
- **G22 fix.** `OptF64` uses 0 for absent and canonical bits + 1 for a value; it cannot wrap. The TIFF `is_nan` change is
  exact: types 3, 4 and 5 only, with denominators `max(1)`. Digests are equal before and after.
- **Licences.** The 7 registry crates of `Cargo.lock` are all MIT/Apache-compatible, the notices match `cargo tree`, and
  `gen-notices.mjs` has no side effects on import.
- **Viewer concurrency.** `takeMissing` runs synchronously after each layout, so sessions sharing the env cannot interleave
  between layout and take. The shared `FontResolver.shared()` makes keying the page env by fonts base sound. The link scheme
  filter (`SAFE_LINK`) is shared by both engines.
- **Unsafe code.** All engine crates on the path are `forbid(unsafe_code)`. Unsafe exists only in uo-alloc, uo-wasm and
  uo-ffi, and all three are in the Miri job (write_unsafe, render_unsafe, ffi lib).
- **Memory coverage per crate.** M1 and M2 tests exist for every crate on the path: uo-opc, uo-docx, uo-docx-layout (soak in
  f7a–d), uo-render, uo-pdf, uo-fonts, uo-image, uo-drawingml, uo-text, uo-jsnum, uo-ffi and uo-wasm. uo-codec has M1 only
  (no soak). M4 covers every handle: env, document, layout, buf, converter, cancel, and the .NET SafeHandles.
- **Reuse.** uo-opc has no Word assumptions. uo-fonts policies are named, with Word substitution confined to the policy
  layer.

### Parity claims and how they were measured

- **Layout.** Rust equals TS on the Word-oracle corpus: 2,742/2,742 documents, run locally with Office fonts. CI runs
  generated documents and 68 committed fixtures.
- **Server PDF.** Byte-equal to TS on 68/68 fixtures (open fonts, in CI) and on 211 of the 2,742 oracle documents (local,
  opt-in, every 13th group). The rest of the oracle corpus has not been run.
- **Viewer switch.** Chromium only, print layout only, open fonts only, pixel tolerance 0, 74 fixtures locally (stride 6 in
  CI). Not exercised: the web view and Reading Mode, Local Font Access or installed Office fonts, the `.doc` path (through the
  TS converter, SW-3), and plugin inlines (SW-7, a gap). Accepted difference: SW-6 (the `udv-link-click` text).
- **Painter.** Part A replay (74 docs), Part B with 200 generated seeds (50 in CI), and pixels in Chromium and in Windows
  WebKit 26.5 (no OffscreenCanvas, so the kerning-joins path was not exercised). The `viewer_switch` comparison (which
  paints through the same painter) later ran on real Safari 15.6.1, iOS 15.5 and iOS 26.5 ("Real Safari 15 / iOS
  (BrowserStack, 2026-11-10)").
- **wasm vs native.** 3 fixtures pinned. wasm2js: 12 fixtures.

### Real Safari 15 / iOS (BrowserStack, 2026-11-10)

`viewer_switch.mjs --mode serve` (new): no browser is launched; the server serves the same bundle and a page that runs the
same `compare` (page count and sizes, headings, comments, link boxes, text layer, find hits, canvas pixels at 1/15 and
2/15 with tolerance 0, the uncompressed PDF of the print layout) on every 4th fixture (19 documents, 82 pages), then a
30-cycle soak of 4 documents with `engine="rust"`, then a CSP probe, and POSTs every result. The page fixes the clock to
`NOW` (as `page.clock.setFixedTime` does in Chromium), releases each comparison canvas after reading it, and resumes
after a tab crash (the document that was running is recorded with its last step). Only git-tracked (open) fonts are
served. BrowserStack cannot reach localhost: the port was exposed for the run with a temporary anonymous dev tunnel,
deleted after. Count build (`count,write,render-fixture`, 4,266 KB), the HEAD `e2b52827` tree.

| Browser (device) | wasm SIMD | OffscreenCanvas | Documents / pages | Differences | Soak (30 cycles) | CSP `'self'` | `+'wasm-unsafe-eval'` | `+'unsafe-eval'` |
|---|---|---|---|---|---|---|---|---|
| Safari 15.6.1, macOS Monterey (dpr 1) | no | no | 19 / 82 | **0** | live 12,681,089 B flat, buffer flat, 0 handles, 1 page env | refused | **refused** | compiles |
| Safari 15.5, iPhone 13, iOS 15.5 (dpr 3) | no | no | 16 / 56 compared, 3 not (below) | **0** | not reached | not reached | not reached | not reached |
| Safari 26.5, iPhone 17, iOS 26.5 (dpr 3) | yes | yes | 19 / 82 | **0** | same numbers as Safari 15.6 | refused | compiles | compiles |

Per-browser totals of the two full runs: 164 renders, 377,619,748 pixels, 103,422 text-layer characters, 3,692 find
hits, 10,551,603 PDF bytes, all equal TS vs Rust. Open time (loadDocx, median / sum over 19 documents; the TS engine is
measured first, cold, so the medians favour Rust and are not a speed comparison): Safari 15.6 TS 1,202 ms / 27.1 s, Rust
77 ms / 4.9 s; iOS 26.5 TS 1,104 ms / 20.4 s, Rust 21 ms / 2.2 s.

- **iPhone 13, iOS 15.5.** The tab is killed and reloaded (OS memory) at the `settle` step of the three largest documents of
  the stride (d5 `f7a__7__8` 8 pages, d12 9 pages, d18 9 pages): there the harness holds two full viewers (TS and Rust)
  of the document side by side at dpr 3. Opened ALONE (`/?solo=ts|rust&docs=…`: one viewer, open, scroll through every
  page, close), each engine handles all three without a crash: Rust 8.9 / 5.8 / 5.8 s, TS 7.6 / 6.7 / 6.7 s. So the crash
  is the two-viewer harness, not the Rust engine; the pixel comparison of those three documents on iOS 15 is not
  covered (it is covered on Safari 15.6 and iOS 26.5), nor the soak and the CSP probe on iOS 15.
- **First run artefacts (fixed in the harness, not engine findings).** Without the fixed clock, d0 (`f2-3-0`, a
  `TIME \@ "HH:mm:ss"` field) differed in every browser (54–350 pixels and the PDF), because the TS and Rust loads were
  seconds apart; with `NOW` it is equal. Without releasing the comparison canvases, iOS 15 reached its canvas memory
  budget and `getContext('2d')` returned `null` (exceptions on d4, d5, d7).
- **Not covered:** iPad, low-RAM devices, Lockdown Mode (disables WebAssembly), the web view and Reading Mode, the
  wasm2js engine in Safari, Firefox.

### Classification of G1–G22 and the other open items

| Item | Class |
|---|---|
| G4, G8, G22 | done (G22 still uncommitted, S5) |
| G5 (untrusted caps not measured), G21 (no fuzz target over the pipeline; uo-docx-layout and uo-pdf have none at all, `core/fuzz/fuzz_targets`) | **must-fix before the gate**, through the security review (S2) |
| G7 size budget (1.405 MB gzip against the TS start of 0.30 MB), G8-W, G8-2, G1 (the `#[path]` file shared across two cdylibs), G2, G3, G6, G9–G13, G15–G18, G20 | maintainer decision at the gate (recommend accepting G2, G3, G6, G9, G10, G13, G15–G18, G20 as built) |
| G14 NuGet RID matrix, G19 LSan on the facade, the facade `CHANGELOG` breaks (S14), the wasm2js CI job and package (only if wasm2js ships) | later: before any publish of 0.10, not for the code gate |
| SW-1 to SW-16 and SPEED-1 to SPEED-5 | maintainer decisions. Notably SW-6/SW-7 (behaviour gaps), SW-11 with S8 (phone profile, memory never returned), and SPEED-1 with S6/S7 |
| Real Safari 15 / iOS (BrowserStack; the MCP needs `/mcp` re-authorisation), and whether Safari 15's CSP blocks wasm | **done 2026-11-10**: 0 differences on Safari 15.6.1 and iOS 26.5 (19 documents), iOS 15.5 16 of 19 (the other 3 crash the two-viewer harness, not one viewer); Safari 15 needs `'unsafe-eval'` |

### Gate checklist

| Criterion | Status |
|---|---|
| Parity 100% on the captured corpora (layout, PDF, painter, switch) | met where measured; scope limits above |
| Parity reference fresh (F2-20) | met (recaptured against `d0f45bc5`, commit `e2b52827`) |
| Memory gate M1–M5, every crate on the path | met, except the page-env M5 and M3 baseline (S6, S7) |
| M4, every boundary handle (wasm, C ABI, .NET) | met |
| M6: Miri on all unsafe | met locally; LSan CI never run |
| Lint: clippy native + wasm32, bans | met (`check-changed.mjs --all` green at `e2b52827`) |
| CI green on GitHub | **never run** (S1) |
| Security review of the milestone | done (`core/docs/security.md`); its blocker and two majors fixed, the rest in G23–G31 |
| Benchmarks main × rust-core (speed) accepted | measured (`uo-docx-layout.md`); acceptance is the maintainer's (G30: Chromium total and the 500-page wasm layout slower than TS) |
| Real Safari 15 / iOS | met with the iOS 15 limit above ("Real Safari 15 / iOS (BrowserStack, 2026-11-10)") |
| Licence notices for what ships | met (generator + check); `cargo deny licenses` never run |
| TS default path untouched | met |
| Reuse: no Word-only rules in shared crates | minor finding S9 (uo-pdf) |
| Records honest and complete (numbers, decisions, process metrics) | partly: S5, S6, S13, the unsupported speed sentence (S3); the ship frontier has no wall time; SW wall time is an estimate |
| Stage-1 work committed | yes (`e2b52827`) |

Process metrics (this review): one Opus agent, about 1 h 10 min wall time (estimated), about 45 tool calls. Builds: clippy of
6 crates native + wasm32 (about 30 s each, warm) and two small test runs. Rework: none.

## Milestone 2: the session in wasm

*(2026-11-11, milestone 2 wave 2, frontier WS: queue item 8, the wasm half, plus `fontUsage` LQ15; planner → implementer.
Nothing committed by the frontier.)* One `UoWriteSession` handle owns uo-docx's `Session`; the hot paths are dedicated
exports; everything else goes through `uo_session_exec` / `uo_session_query` / `uo_session_complete` with a
schema-generated binary codec whose conversions to uo-docx's own types are generated too (no hand-written mapping per
command). Code: `crates/uo-wasm/src/session.rs` and `src/session/` (feature `session`), `js/session.mjs` +
`js/session.d.ts`, `js/gen/session*.ts`, `schema/session.uos` + `schema/session.bind`; the generator's new emitter
`tools/uo-schema/src/rust_bind.rs`; `uo-docx/src/session/font_usage.rs`.

### API

Every export is `extern "C"`; input buffers come from `uo_alloc_bytes` and are consumed (also on failure). `UoBuf`
statuses: 0 ok, 1 none, 2 no pending font save, 3 save failed (last error set), 5 a message that does not decode or bind
or an argument out of its domain (UTF-8 detail in the buffer), 11 null handle. A handle-creating call answers null and
sets `uo_last_error` (codes of `pipeline::code`).

| Export | Arguments | Result |
|---|---|---|
| `uo_session_open` | env (nullable `UoWriteEnv`, borrowed), `SessionOpen` bytes, docx bytes | handle / null |
| `uo_session_new_blank` | env, `SessionOpen` bytes | handle / null |
| `uo_session_free` | handle | — |
| `uo_session_set_env` | handle, env (nullable) | code (the session keeps the registry `Arc`s, not the env handle) |
| `uo_session_exec` | handle, `Exec` | `ExecResult` (status 0; a command error is the `error` outcome) |
| `uo_session_complete` | handle, `Complete` (job id + `JobResult`) | `ExecResult` |
| `uo_session_query` | handle, `Query` | `QueryResult` |
| `uo_session_type` | handle, UTF-16LE units | `ExecResult` |
| `uo_session_enter` / `_backspace` / `_delete` | handle | `ExecResult` |
| `uo_session_set_selection` | handle, anchor para/offset, focus para/offset | `ExecResult` |
| `uo_session_move_caret` | handle, layout (nullable for left/right/word), dir 0–7, extend | `ExecResult`; 1 nothing moved; 5 bad dir or a line move without layout |
| `uo_session_hit_test` | handle, layout, page, x, y (twips) | `Hit`; 1 no lines (textbox) |
| `uo_session_caret_rect` | handle, layout | `Caret`; 1 no line |
| `uo_session_selection_rects` | handle, layout, page | `SelRects` |
| `uo_session_layout` | handle, env, now ms, tz offset, flags (as `uo_write_layout`; revision bits 0 = the session's `revisionView()`), lang, hyphenation languages, max pages | an OWNED `UoLayout` (every `uo_layout_*` / `uo_render_*` export reads it; `uo_layout_free`) / null |
| `uo_session_save` | handle | docx bytes; 3 |
| `uo_session_font_requests` | handle | `FontRequests` (phase 1: saves, keeps the package); 3 |
| `uo_session_save_with_fonts` | handle, `FontFaces` (one answer per request, 16 GUID bytes per request) | `SaveResult` (bytes + `uw-font-embedding`); 2 without phase 1; 5 mismatched answers |
| `uo_session_clear_caches` | handle | — (M1) |
| `uo_session_clipboard_clear` | — | empties the instance's Office Clipboard |
| test only (`session-selftest`) | `uo_session_test_open/_hook/_next/_facets/_random_exec/_forget` | the capture replay and the fuzz (below) |

JS (`js/session.mjs`, `bindSession(exports, bindWrite(...))`): `open(bytes, o)`, `newBlank(o)`, `clipboardClear()`,
`SessionError`, `codec`; a `WriteSession` (private handle, `FinalizationRegistry`) has `type`, `enter`, `backspace`,
`delete`, `setSelection`, `moveCaret`, `hitTest`, `caretRect`, `selectionRects`, `exec`, `command`, `undo`, `redo`,
`cancelPending`, `setDirty`, `applyPainter`, `setFontEmbedding`, `setClock`, `setUiLang`, `complete`, `query`,
`layout(env, o)` (a `WriteLayout` of write.mjs through the new `uo.wrapLayout`), `save`, `fontRequests`,
`saveWithFonts`, `clearCaches`, `free`. Results are the generated TS objects: a variant whose payload is a struct
carries the struct's fields next to its tag (`{ t: 'done', stories, selection, events, dirty, canUndo, canRedo }`),
other payloads are `v`. Decode, bind and null failures throw `SessionError { code, detail }`; a
`WebAssembly.RuntimeError` marks the binding dead and throws `SessionError('trap')` (no unwinding through the module;
the shell reloads it as the viewer does). session.mjs imports the generated `.ts` directly (Node strips the types,
bundlers compile it); a page without a bundler needs a compiled copy; no `session.d.mts` (WS-D9).

### The codec

- `schema/session.uos` (5,027 lines, schema `write_session`, magic `UOWS`, hash `d4bfc82666d659d1`): 12 messages
  (`Exec`, `Complete`, `ExecResult`, `Query`, `QueryResult`, `SessionOpen`, `FontRequests`, `FontFaces`,
  `SaveResult`, `Caret`, `Hit`, `SelRects`), 661 types. `Command` with its 22 family enums and all 300 variants (298 on
  the wire; `FormatCmd::ApplyPainter` and `ClipboardCmd::RenderPictures` are native-only: the painter stays in the
  handle and `ExecOp::ApplyPainter` uses it); every argument and model type they reach; `ChangeSet`, `Event` (variants
  named by their DOM names, `{ t: 'uw-change' }`, so the shell dispatches `new CustomEvent(ev.t, …)`), `Job`,
  `JobResult`, `CommandError`, `LocalNow`; `QueryOp` / `QueryAnswer` with 82 accessors plus 4 binding-level answers
  (`ClipboardItems`, `CopyFormat`, `FontUsage`, `FontEmbedding`); `ExecOp` = `Command | Undo | Redo | CancelPending |
  SetDirty | ApplyPainter | SetFontEmbedding | SetClock | SetUiLang`.
- `schema/session.bind` (330 bind lines): which Rust type each wire type is (127 enums, 165 structs, 21 tuples, 16
  options, 2 newtypes; 97 `into_only`, the engine's outputs). The type part of both files was drafted by a throwaway
  extractor over the uo-docx sources (kept outside git, `core/target/m2-wsession/plan/`) and finished by hand (the
  head with the messages and the binding-level types is hand-written).
- Generated (`uo-schema generate`, one manifest line): `crates/uo-wasm/src/session/codec/generated.rs` (23.8k lines),
  `generated_support.rs` (canonical dump, random messages; built only with `session-selftest`), `bind.rs` (6.9k
  lines), `js/gen/session.ts`, `js/gen/session.support.ts`. `uo-schema check`: 20 generated files up to date; the
  render/write outputs are byte-identical.
- **The bind emitter** (`rust_bind.rs`, manifest keys `rust_bind=` + `bind=`; the IDL grammar and hash untouched): per
  bound type `impl FromWire<w::T> for Native` and `impl IntoWire<w::T> for Native` against the consumer's traits (the
  `runtime` line: `crate::session::wire`), every field through the traits so the leaf impls are picked by inference;
  kinds `struct [default] [empty]`, `enum [native_only=…]`, `tuple`, `option`, `newtype`, `manual`, modifier
  `into_only`; a payload struct without a bind line is a Rust struct variant, a `tuple` payload a spread tuple variant,
  anything else `V(x)`; matches are exhaustive both ways, so a new variant or field of uo-docx fails to compile until
  the schema has it. Leaf impls (`session/wire.rs`): primitives, `Option`, `Vec`, `Box`, `Arc`, `Arc<[T]>`,
  `VecDeque`, `BTreeSet`, arrays, `IndexMap`/`JsMap` and `JsRecord` as lists of pairs, ids, `JsNumber`, `NumKey`,
  strings.
- Tags: the default `t`; `$t` on `PictureCmd`, `DrawingCmd` and every enum whose payload struct has a field `t`
  (`uo-schema` refuses a clash).
- Decode limits (WS-D4): nesting 64, 16 Mi elements per list, 64 Mi elements and code units per message, 256 MiB of
  decoded values (`codec::limits()`; the codec default is 128 MiB, a picture command carries its bytes).

### Bytes ownership and the two-phase protocols

Inputs move into the module (`uo_alloc_bytes`, consumed); outputs are `UoBuf`s JS copies and frees (`Buf.take`).
**Jobs**: a two-phase command answers the `jobs` outcome; the shell runs each job (decode or encode an image, read the
clipboard, render a clip) and answers it with `complete(job, result)`; while a command is pending every other command
answers `CommandError::Busy` (tested). **Paste**: `ClipboardCmd.pasteData.html` and `JobResult.clipboard.htmlTree`
carry the bytes of `encodePaste()` (core/crates/uo-docx/js/paste-tree.mjs). **Fonts on save**: phase 1
`fontRequests()` saves, reads what the package embeds and answers the faces the plan wants (uo-fonts
`embedding_requests` with Word's policy; none when `embed` is off); the shell gathers the bytes; phase 2
`saveWithFonts(faces, guids)` plans (`plan_font_embedding`; the document's own embedded face of a request first, the TS
`documentFace ?? resolver`), writes the fonts (`write_docx_embedding`) and answers the bytes and the event. The glue
is in uo-docx (`font_usage.rs`: `initial_embedding_settings`, `set_font_embedding` — no undo step, tested —,
`begin_save_with_fonts`, `finish_save_with_fonts`), reusable by uo-ffi.

### Layouts, the oracle, the caret

- `uo_session_layout` lays out `Session::view()` (the edit context's model) with the viewer policy over the embedded
  fonts and answers a `UoLayout` built by the new `write::layout_handle` (a copy of `uo_write_layout`'s success
  branch: dedupe later). Print view only (WS-D6).
- The `LayoutOracle` of the layout family is `uo_docx_layout::oracle::EngineOracle` built on the first call of a
  command (`session/oracle.rs`, `LazyOracle`): typing never builds an engine. Options: the editor's `layoutOpts()`
  (hyphenation languages pt-BR/en/es, `revisionView()` ported from universal-write.ts 7986, the host clock, the loaded
  pattern sets). Without an environment every oracle call answers a layout error (`CommandError::Layout`, tested).
- Hit, caret and selection rectangles wrap uo-docx-layout's `EditLines`/`LineIndex` over the edit scope of the
  context (body; cell: the anchor's body block and the hops through `cell_at_grid`, as f8_parity.rs; header/footer;
  note; a textbox has no scope here because uo-docx does not expose the edited chain, WS-D6), cached per (layout
  generation, scope). Used from uo-docx-layout: `EditLines::new/caret/hit/selection_rects/line_of/caret_x/hit_line_x`,
  `LineIndex::new`, `Locator::new`, `EngineOracle::new`, `derive::build`, `engine::layout_document`,
  `HyphenSessions::new`, `EmbeddedFontProvider::wrap`, `Layout::navigation/run_boxes`.
- `moveCaret`/`moveWord`/`lineEndOffset` ported (`session/caret.rs`, universal-write.ts 9915–9989, 18624): left/right
  skip the characters the markup view hides (`hidden_char`, `canon_pos` made `pub` in uo-docx), up/down through the
  lines in `allLines()` order, home/end, word boundaries with the JS `\s`. A move only sets the selection (no step, no
  event, the dirty flag untouched: unit test). `tableNavigate`/`moveCell` are not ported (gate item); the moves have no
  capture of their own.

### The environment, the ring

`HostEnv` = the session's clock (`Rc<Cell<LocalNow>>` shared with the handle, `SetClock`; WS-D5: one cell of `Copy`
data in a single-threaded module) and the testkit xorshift32 (equal to `FixedEnv`: unit test). One `ClipboardRing` per
instance in a `thread_local` (G7), lent to each command.

### fontUsage (LQ15)

`Session::font_usage()` ports universal-write.ts 2317–2335 literally, with `buildSpans` (10859) reduced to the family,
the variant and the text: bold/italic of the run or the document style, `fontAscii ?? style ?? docDefaults`, textbox
runs give '', image runs `text || U+FFFC`, custom inlines U+FFFC (no `renderer.measure`: its result never reaches the
usage), all caps upper-cased, small caps through `smallCapsSpans` (lower/not-lower runs per code unit, upper-cased
unless the length changes). The editor's built-in `STYLES` table is left out (no font, italic or caps, `bold: false`:
it cannot change the output). Order: the paragraphs of `this.model` — the body's or a cell's top-level paragraphs; a
note's, a header/footer part's and a textbox's flattened with their cells, as the TS swaps `this.model` in the contexts
— each followed by its textbox runs' paragraphs (the textbox's TS `paras` view); then `this.tableAnchors` (the body
tables in the body, cell and note contexts; `[]` in a header/footer or textbox) row → cell → paragraphs → nested
tables; then every section's header and footer slots default/first/even (the flattened part paragraphs). Parity:
`js/capture-font-usage.mjs` (frozen element, Chromium 156, 100 corpus documents, four points: open; select all + bold +
typing at the end; inside the header/footer part of page 0; inside the first body cell) →
`tests/parity/font-usage.capture`; `session_font_usage::font_usage_matches_the_element`: **100 documents, 631 facets
equal, 0 mismatches** (`fu` digest of `JSON.stringify(fontUsage())` plus `n`; in this sample only 3 documents have a
header part the TS enters without creating one and 27 have a body table). M1 of the usage and of both save phases (14
corpus documents + the blank one): 0 B; no panic over 50 corpus documents with the save (145 ms).

### Numbers

| Check | Result |
|---|---|
| (a) capture replay through the wasm exports (Node 24.15, `session_replay.test.mjs`, session-test module) | **d2a** 120 documents, 49,800 facets equal, 0 accepted, 0 mismatches (9,600 actions: 7,320 hot path, 2,280 exec); **f1** 96 docs, 39,840 equal, 0 mismatches (8,478 actions); **f2** 120 docs, 37,559 equal, 1 accepted (F2's "throws after the step", as natively), 0 mismatches (7,200 actions); **f6** 60 docs, 18,780 equal, 0 mismatches (3,600 actions); every action re-encoded by the TS codec byte-identical (28,878 / 28,878); 1.9 s / 2.1 s / 11.5 s / 0.6 s |
| (b) M4 (`session_api.test.mjs`, count build) | 10,000 × open(tiny docx) + type + free, 10,000 × newBlank + free, 10,000 dropped and collected by the FinalizationRegistry (`--expose-gc`, a gc every 500): live bytes back exactly to 5,180,964 B; 45 s |
| M2 wasm | 200 sessions × 50 edits + undo/redo + save: live bytes flat (5,180,964 B at every sample), `memory.buffer` grew 0 B |
| M1 native (`session_native`) | an editor session through every export (open, hot paths, exec/query/complete with a picture job, layout, caret/hit/rects, 8 moves, save, both font phases, free) + a blank one: 0 B left (65.3 MB allocated, 3.6 MB peak) |
| M2 native | 1,000 × (open → 100 hot-path edits → 50 undo / 50 redo → save → free) over 10 corpus documents: 0 B drift (`uo_alloc::soak`), 4.3 s |
| M3 per typed key (native, 200 keys, release) | 15,000 paragraphs: `uo_session_type` 15,652 B mean / 21,982 B worst allocated, 209 µs; `Session::exec` 15,058 / 21,387 B, 210 µs (the export adds about 600 B: the change set's wire form). f8__7__3: 12,351 / 18,715 B, 8.0 µs vs 11,794 / 18,123 B, 7.1 µs |
| bind completeness (native) | 2,838 commands of the hostile generators (F1, F2, F6, F9, D2a) native → wire → bytes → wire → native equal; 67 refused by the declared i64 → i32 narrowing (`i64::MIN/MAX` indices); 0 failures |
| (c) hostile, native | 4,015 cases (random bytes, prefixes, byte flips into exec/query/complete/open): no panic, worst 50 µs; 3,000 random `Exec` messages run on a session: 0 leak on a second run |
| (c) hostile, wasm (`session_hostile.test.mjs`, 10 s) | 1,300,092 calls in two identical passes (random 0–4 KiB strings, prefixes and bit flips of 6 valid messages, 325,022 random `Exec` messages of the generated support run on the session): no trap, worst 14.0 ms per call, a valid exec works afterwards, live bytes after pass 2 = after pass 1 (2,086 B: tables built lazily in pass 1) |
| R-P2 of the 12 messages (`uo-schema` `codec_session.rs`) | 60 tests pass (Rust round trip, TS decode/encode, canonical form); bind fixture: 5,000 random messages wire → native → wire identical, every binding kind |
| (d) size, `--profile wasm`, `gzip -9` | viewer (`write`) 4,024,661 B raw / 1,433,113 B gzip at the end of the wave (HEAD 3,977,238 / 1,414,509; the +18.6 KB gzip is not this frontier's: the viewer build contains none of its code — uo-docx-layout changed in parallel); **session (`session`) 9,230,855 B raw / 3,029,327 B gzip: +1,596,214 B gzip over the viewer** (not optimized; `wasm-tools validate` with the default feature set: valid) |
| regressions | `write_native` 7/7, `write_api.test.mjs` 8/8 on the viewer-count module after the write.rs/write.mjs additions |

### Decisions outside the TS semantics (declared)

- **WS-D1** the bind mechanism: a separate bind map read by a new emitter (no IDL change); the codec lives in uo-wasm.
- **WS-D2** narrowings: i64/i8/i16 → i32 checked, usize/u64 → u32 checked; `JsNumber::Null` goes out as NaN and never
  comes back as `null`; `String`/`Str` arguments are built from UTF-16 with a lone surrogate → U+FFFD; the js_object
  `cleared`/`keys` (explicit `undefined`, key order) are not carried (structs bound `default`); the font usage's text
  goes to uo-fonts as UTF-8 (a lone surrogate → U+FFFD in the subset set); the skip reasons of `uw-font-embedding` are
  dropped by `Event::FontEmbedding`'s shape.
- **WS-D3** the into-only answers drop what the IDL cannot carry: `RunProps.fmt`/`ParaProps.pfmt` (a format change
  inside a format change, recursion without a list) in `RevisionEntries`.
- **WS-D4** the decode limits (above). **WS-D5** the shared clock cell. **WS-D6** print view, viewer policy, no textbox
  scope.
- **WS-D7** caret moves without `tableNavigate`/`moveCell`. **WS-D8** `ExecOp::SetUiLang` added (the session's
  `set_ui_lang` had no command; the element's `lang`). **WS-D9** session.mjs imports the generated `.ts`.
- **WS-D10** statuses 2 (no pending font save) and 3 (save failed) join the `UoBuf` status space.
- **WS-D11** a typed key in a protected session answers `done` with no change (the TS `locked()` returns silently),
  not `CommandError::Locked`.
- **WS-D12** the self-test copies the op-code → command mappings of the D2a, F1 (`step`), F2 (`command`) and F6
  (`step`) native replays and their `paras_digest`/`anc_json`/`docxml` (one copy each), and includes uo-docx's test
  support modules with `#[path]` (`support`, `d2a_support`, `d2b_f1/f2/f6_support`; also in `tests/session_native.rs`
  with `d2b_f9_support`); `big_document()` of memory_d2a.rs is copied (without its tables and fields) into
  session_native.rs.

### Queries left out

The model/state accessors (`document`, `view`, `package`, `context`, `options`, `story`, `paras`, `history`, `pending`,
`pending_props`, the version stores, `bookmark_paras`, `section_state`, `settings`, `policy`, `resolved_policy`,
`history_bytes`, `clip_identity`, `locked`), `copied`/`copy_format` (the painter stays in the handle),
`note_settings` (private entries; `note_options` answers), `render_clip_request` (the F8 continuation),
`count_matches` (its result is a private alias `Found<T>`), `hidden_char` (takes a paragraph), `font_usage` (the
binding-level `FontUsage`). Not in uo-docx at all (queue item 9 prep): `getSelectionState`, `getTableState`, the
spellchecker's paragraph texts and languages.

### Files touched outside the owned crates (additive)

- `tools/uo-schema`: `src/rust_bind.rs` (new), `src/lib.rs` (`pub mod rust_bind`, `Target.rust_bind/bind`, the two
  manifest keys, `generate_target_with_bind`, the bind map read in `generate_all`); tests `bind.rs`,
  `bind_generator.rs`, `codec_session.rs`, `session_codec/mod.rs`, `schemas/bind_test.{uos,bind}`,
  `fixtures/bind_native.rs` and the generated `fixtures/bind_test{,_support,_bind}.rs`.
- `schema/manifest.txt` (two lines), `schema/session.uos`, `schema/session.bind`.
- `crates/uo-wasm/src/write.rs`: `layout_handle`, `layout_geom`, `layout_generation`, `session_layout_options`,
  `env_parts` (all `#[cfg(feature = "session")]`, appended). `js/write.mjs`/`write.d.ts`: `wrapLayout` and
  `lastError` in the binding object, `WriteLayout.ptr`.
- `crates/uo-docx`: `session/mod.rs` (`pub mod font_usage;`), `session/ops/text.rs` (`hidden_char`, `canon_pos` →
  `pub`), `tests/all.rs` (`mod session_font_usage;`), `js/capture-font-usage.mjs`, `tests/parity/font-usage.capture`.
- `Cargo.lock` (uo-wasm's optional `indexmap` and its new dev-dependencies).

### Wanted CI lines (for the orchestrator)

`cargo run -p uo-schema -- check` (already) and `cargo test -p uo-schema` (now with the session suite and the bind
tests); `cargo clippy -p uo-wasm --features session[,count|,session-selftest]` native and wasm32;
`cargo test -p uo-wasm --features session,count,session-selftest --test session_native`;
`cargo build -p uo-wasm --features session --target wasm32-unknown-unknown --profile wasm` + `wasm-tools validate`;
the session-test module + `node --expose-gc --test` of `session_api`, `session_replay` (`UO_WS_FAMILIES=d2a,f1,f2,f6`)
and `session_hostile` (`UO_WS_FUZZ_MS=10000`); tsc over `js/gen/*.ts` with `js/session.d.ts`. No Miri file was added.

### For the milestone-2 gate

1. The bind mechanism (WS-D1) and where the session codec lives (uo-wasm now; uo-docx behind a `session-codec`
   feature, or a shared crate, when uo-ffi carries the session).
2. Session-produced values never on the wire (the painter, `RenderPictures`): accepted as implemented?
3. Layout ownership: the owned `UoLayout` + (session, layout) caret exports; R's incremental relayout becomes
   `uo_session_relayout(h, prev)`.
4. Keyboard navigation: `tableNavigate`/`moveCell` not ported; the moves have no capture (queue item 9, with a
   Chromium capture).
5. Toolbar queries missing from uo-docx (`getSelectionState`, `getTableState`, the spellchecker's texts/languages).
6. The narrowings of WS-D2/WS-D3 (notably `JsNumber::Null`, explicit-`undefined` masks and key order for
   order-sensitive signatures, D2a-U6).
7. `uw-font-embedding` skip reasons: extend `Event::FontEmbedding` in uo-docx.
8. Font policy (`Editor` policy not built) and print view only.
9. `.doc` in the wasm session: off.
10. Module packaging: the editor as its own module (+1.60 MB gzip over the viewer); size work deferred.
11. Replay coverage: d2a, f1, f2, f6 done through the wasm exports; **f3, f4, f5, f7, f8, f9, f10 not replayed in wasm
    in this wave** (their mappings were not ported into the self-test); family-specific facets (pend, sv/nv, rev,
    cm, …) are compared natively only.
12. Not done: `session_unsafe.rs` (Miri), the WebKit run of the session tests, a hostile test pairing a stale layout
    with an edited session (the hit/caret code reads with `get()`; not measured), the skipped queries, the generator
    wart (a list of a unit-only enum emits a redundant `.copied()`, allowed in the uo-schema include of the session
    codec).
13. The viewer module grew 18.6 KB gzip during the wave from changes outside this frontier: re-measure at the merge.

### Process metrics

One implementer agent (Opus), one session of about 4 h wall time, about 200 tool calls. Rework: the extractor's first
drafts (js_object! bodies parsed twice, CR line ends, generic and boxed `Nullable`, recursion through `FormatChange`);
the bind emitter's tuple bounds named unqualified wire types (now `w::`), a too strict payload check, the bind file
hard-coded `super::generated` (now the rust file's stem); the JS wrapper passed struct payloads as `v` (the generated
TS flattens them); M2's sample vector was counted as growth (now `uo_alloc::soak`); the wasm hostile baseline counted
lazily built tables (now two identical passes); M4's finalizer loop let 10,000 sessions live at once and pushed the
memory past 2 GiB (now a gc every 500; render.mjs reads pointers as signed `i32`, a pre-existing limit at 2 GiB);
clippy findings in the new code (6). Builds: four wasm modules per round (about 2–2.5 min each, `-j 4`), one target
dir (`core/target/m2-wsession`).

### Wave-2 follow-ups

The wave-2 reviewer accepted the session in wasm with eight minor findings and asked for CI lines. Measured at HEAD
27f395db plus this frontier, one target dir (`core/target/m2-wsfollow`), `-j 4`, tests at most 3 threads.

1. **Bind exhaustiveness (uo-schema `rust_bind.rs`).** The `default` struct modifier is gone (a parse error that names
   the replacement); a struct bind now lists its Rust-only fields with `skip=a,b` (raw identifiers as `r#type`; repeatable).
   `FromWire` sets each skipped field with `Default::default()` and never uses `..Default::default()`; `IntoWire` of EVERY
   struct bind (the 57 `default` lines, and also the 108 that had no skip list and the 35 `into_only` ones) first
   destructures the native struct with no rest pattern (`let a::P { x: x_x, n: x_n, cache: _ } = self;`), so a new real
   field of a native type fails to compile until the schema or the `skip=` list has it. `emit()` refuses a `skip` name
   that is on the wire, `skip` with `empty`, and a duplicate. The bind map `#` comment rule now starts a comment only at
   a word start (a raw identifier `r#type` is not a comment). `schema/session.bind`: 55 lines `skip=cleared,keys`,
   `RunProps` `skip=fmt,cleared,keys`, `ParaProps` `skip=pfmt,cleared,keys` (the skip lists were measured by a throwaway
   extractor over the 165 struct binds: the only Rust-only fields are the `js_object!` macro's `cleared`/`keys` and WS-D3's
   `fmt`/`pfmt`). Regenerated: `cargo run -p uo-schema -- check` gives "20 generated files are up to date"; the emitted
   `bind.rs` has 0 `..Default::default()` and 57 `cleared: _, keys: _` patterns. Proof of the claim: adding `pub extra: u8`
   to the fixture struct `Tagged` makes `cargo test -p uo-schema --test bind --no-run` fail with E0063 (missing field in the
   initializer) twice and E0027 (the destructure does not mention it); reverted. uo-wasm builds natively and for
   wasm32 with all 165 struct binds. Tests: `bind_generator` 4 pass (grammar with `skip`, the `default` refusal, skip on
   the wire, skip with `empty`, the emitted shapes), `bind` 2 pass.
2. **Hostile `Query` fuzz.** `uo_session_test_random_query(seed)` (selftest, test-only) is the sibling of
   `uo_session_test_random_exec` over the generated `w::Query::random`. Native: `random_exec_messages_leave_nothing` and the
   new `random_query_messages_leave_nothing` share one helper; each runs 3,000 messages (`UO_WS_RANDOM`) on a session
   without an environment and on one with the open fonts: 0 leaks on a second run in all four runs. wasm
   (`session_hostile.test.mjs`, kinds `i % 5`): 10 s first pass, the second pass repeats the same calls. **One final run
   (fix round 2, `UO_WS_FUZZ_MS=10000`):** 725,220 calls in two passes of 362,610 iterations, 145,044 random `Exec` and
   145,044 random `Query` run, no trap, live bytes 3,492 -> 3,492, worst 179.9 ms. The test now prints WHICH call sets the
   worst time: an `exec` of 20 bytes, i.e. a random `Exec` message that decoded into a real command (a layout or a
   whole-document operation); the wave-2 figure of 9.6 ms came before the random `Exec` messages existed. The call count
   varies by run (a 5 s run gave 640,746 calls, worst 55.1 ms) because the first pass is time-bounded; the earlier figures
   (726,080 / 725,980 calls, 132.6 / 229.9 ms) were other runs of the same test.
3. **Stale layouts (fix round 1: real contexts).** Native `stale_layouts_never_panic` (`UO_WS_STALE`, 300 per document, three
   documents, two passes, the second leak-checked under `count`): one layout, then random backspace/delete bursts, select-all +
   delete, Enter, typing, **`EnterCell` of the fixture's tables, `EnterNote` with the real note ids (3/4 of f8__7__0, 7/9 of
   f8__7__10), `EnterHeaderFooter` of the real parts**, the exit commands, undo/redo, fresh layouts now and then (the lines
   cache of a non-body scope is rebuilt against new geometry) and random selections, each followed by `hit_test` (page 0,
   last, last+1, `u32::MAX`; x/y NaN, +-inf, -1, 0, 1500, 300, 5000, +-1e300), `caret_rect`, `selection_rects` and
   `move_caret` (directions 0..8) against the STALE layout; every context is also activated once before the random part.
   The activations are counted from the `uw-cellmode` / `uw-notemode` / `uw-hfmode` events and asserted > 0: 3,600 calls,
   activations cell/note/hf 1 / 21 / 37, statuses only 0/1/5, no panic, second pass 0 B left. **Fix round 2 (cells):** the
   reviewer found that no stale query ran while the session was in a CELL (the one pre-loop activation was exited at once and
   the random indices hit a real cell of `f8__7__3` 6 times in 27). Now each pre-loop `Enter*` runs the four queries before its
   `Exit*`, a probe finds the cells the document really has (anchor 0, rows and columns 0..3: `f8__7__3` only), the random
   `EnterCell` picks from them, and the number of queries made while in a cell (from the `uw-cellmode` events) is asserted
   > 0: native 3,648 calls, 36 of them in a cell, activations cell/note/hf 5 / 29 / 35; wasm 4,896 calls in 2 rounds, 184
   in a cell, `{uw-cellmode: 11, uw-notemode: 31, uw-hfmode: 54}`, live bytes 5,180,964 both rounds. The random part
   rarely stays in a cell because the select-all + delete of the loop destroys the fixture's table early: most cell queries
   are the pre-loop ones (this is the limit of the test). (the first version's
   `EnterNote` used footnote id 1, which the fixtures do not have, and its `Exit*` commands ran in the body: no-ops, found by
   the reviewer). wasm (`session_api.test.mjs`, `UO_WS_STALE`, 200 per document, two rounds): no trap (`!ss.dead`), live bytes
   back. No panic or trap was found: nothing to report to uo-docx-layout.
4. **Protected sessions.** `session_api.test.mjs` asserts that typing in a protected session answers `done`, the saved
   bytes are equal and `isDirty` is false, and runs bold, `setAlignment`, Enter, Backspace and a comment through `exec`: each
   answers `done` (never `error`, never `locked`) and the document is unchanged; the native `refusals_and_errors` now
   requires `Done` and equal `save` bytes (it accepted `Locked | Done`). **WS-D11b:** `CommandError::Locked` is never
   produced anywhere in uo-docx (every locked path returns silently, as the TS `locked()`), so "a command the session
   refuses with Locked through exec" cannot be asserted; the silent refusal is asserted instead. **Finding:**
   `pasteText` is not locked in a protected session and changes the document, as the TS (`pasteTextOnly` checks only the
   read view, not `protectedMode`); the test prints it and does not assert it (gate item).
5. **Phase 2 of the font save.** `uo_session_save_with_fonts` decodes and checks the `FontFaces` message (decode error ->
   5, wrong face count or fewer than 16 GUID bytes per request -> 5) BEFORE it takes the pending `FontSave`, so a
   malformed phase 2 leaves phase 1 alive (**WS-D13**); a status-3 failure still consumes it. Native
   `a_malformed_phase_2_keeps_phase_1` (junk, one answer too many, one GUID byte short -> 5 each; then the right message
   -> 0; then again -> 2) and the same sequence in `session_api.test.mjs` through `s.saveWithFonts` (codes 5, 5, ok, 2).
6. **Replay through the production open.** `session_replay.test.mjs` opens every document with `uo_session_open`
   and a `SessionOpen` message (author "A", seed 1, or the second xorshift value for F2 and F10 as the harness draws one
   `Math.random()`, clock 2026-01-02 03:04 local = UTC, session id 0 or the document's index + 1 for F8; F2 and F10 then
   `setUiLang('pt')`): `uo_session_test_open` and `UoWriteSession::for_tests` are removed. d2a 120 documents, 49,800 facets
   equal; f1 96, 39,840; f2 120, 37,559 + 1 accepted; f6 60, 18,780; **f3** (tables) 70, 40,390, 4,762 actions; 0
   mismatches, TS re-encode byte-identical everywhere. **Fix round 1 adds f5, f8, f9 and f10** (the native replays' mappers
   and facet functions are COPIED into `src/session/selftest/{fam,f10,f8}.rs`, each with a source header; declared, the gate
   list asks to share them with the native support modules), all at 0 mismatches and with the SAME accepted counts as the
   native replays (re-run natively in this round, f8 with `uo-docx/export` as the native replay needs it):
   - **f5** (notes): 60 documents, 44,940 facets equal (the native 48,600 also counts the 3,660 `op` checks), 3,600 actions;
     facets `ctx`, `no`, `np`, `ref`, `nts`, `sect`, `csel`, `parts`; the list-rule hook (F2) as f1/f6.
   - **f9** (find, navigation): 80 documents, 54,803 equal, 10 accepted (F9-A2, as native), 6 steps where the TS threw (the
     script ends), 5,245 actions; `cm` (`countMatches`, a direct call of the session that is not on the wire) is answered by
     the facets export; `res`, `cursel`, `ext`, `ctx`, `hd`.
   - **f10** (references): 77 documents, 76,346 equal, 53 accepted (31 F10-A3 + 22 F10-A4, as native), 2 steps where the TS
     threw, 4,593 actions. The handle has a **test-only `test_oracle`** (`ReplayOracle` of `d2b_f10_support`, field cfg
     `session-selftest`): the production `run()` is unchanged without it. `uo_session_test_f10_setup` (reference seed from the
     capture's `i-seed`, the hyphenation registry of pt/en/es from the uo-fonts data, as the native replay),
     `_begin` (the TS's `i-fv`/`i-pg`/`i-mh` script of the step) and `_record` (how the session asked: `fvs`, `pgc`, `pgv`,
     `mhc`, the values the TS named that the model lacks); the DIVERGENCES by name (`fsep-top716.docx` from step 30).
   - **f8** (clipboard): 54 documents, 43,132 equal, 122 accepted (61 F8-D14 + 61 G7/F8-O2, as native: 46,480 equal, 122
     accepted), 3,491 actions (the `@setup` commands through `exec` since fix round 2), 0 command errors. Per group the Office Clipboard (items AND current rich copy: `clear()` leaves
     the current, found by the first run) and the carried paste defaults, path and policy start over
     (`uo_session_test_f8_group`); `uo_session_test_f8_open` registers the session and the carried state (paste defaults, path, policy, the extended group's
     inline types) goes in as EXEC ACTIONS of the pseudo-code `@setup` (fix round 2: it was applied with `h.command`, bypassing
     the codec and `exec`; now the four commands are re-encoded and counted like every action); the shell's answers
     (`cbt`, `pt`, `fx`) are the JS side's: `readClipboard` answered with the text and the paste tree, `renderClip` with the
     PNGs in order, as the native `drive`. Two additions outside the replay files, both test-only: the self-test build turns on
     `uo-docx/export` (the HTML a copy writes, F8-D5: without it 84 `cbw` facets differ, the native replay needs it too; the
     shipped `session` feature is unchanged), and `env.rs` counts the session's random draws behind
     `cfg(all(feature = "session-selftest", not(test)))` (`draws`/`reset_draws`: the `rc` facet; additive, declared).
   - **f7** (drawing objects, text boxes, pictures; **fix round 2**): 40 documents, 2,400 steps, 27,207 facets equal, 8
     accepted (5 F7-D1 compress `enc` that differ, 3 F7-D6 multi-selection `cnt`: the native replay's own count; merge fix: the
     `cmp:` `enc` facet is compared first and accepted only when it differs, as `Replay::accept`; it was accepted unread),
     2,418 actions, 0 refused by the session, 0 mismatches, TS re-encode 2,418 byte-identical. The earlier record deferred f7
     to a maintainer decision about the draw counter; that was wrong: the test-only counter (`env::draws`) was already there
     and `Session::env_mut()` is public, so `SharedEnv::advance_to(rc)` is reproduced by `uo_session_test_f7_begin`, which
     advances the session's environment through `env_mut().random_u32()` until the cumulative counter (reset by
     `uo_session_test_f7_reset` right before the open, so the open's own draws count as in the native `SharedEnv`) equals the
     capture's `rc` of the previous step. `src/session/selftest/f7.rs` COPIES the native `step` mapper (rewritten as "action
     k of a code": `iw` and `cdt` have a second action, `PlaceAt`, which exists only when the first was not refused: the JS
     loop stops the step on a refusal) and the facets (`paras sel anc flags fs tb pane st cnt n rc docxml`; `ev` comes from the
     TS-decoded change set). The facts the commands take (`sf`, `sfs`, `fr`, `eo`) are parsed module-side from the step's `fx`;
     the job half of the native `Run` is the JS side's: `decodeImage` answered from `gid` (pixels) or `bmp` (size only),
     `encodeImage` answered from `blob` and hashed (`enc`, FNV of the RGBA), `gid` dropped for `ins`/`chg`; the images
     (`img#k`) are handed over once (`uo_session_test_f7_image`). `rc` is compared at every step. Nothing of the family is
     declared as not replayed.
   `uo_session_test_next` answers status 5 with the error when a native command has no wire form (it answered "no more
   actions" before, which the JS loop could not tell from the end of the list). **Not replayed in wasm:** f4 only (no wire
   op for `undo_reentering`/`redo_reentering`): the CI list is `d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10`.
7. **fontUsage parity with notes and text boxes.** `capture-font-usage.mjs --docs 160 --pick notes,textboxes` takes the
   100 stride documents plus the first 30 corpus documents with a note reference and 30 text-box documents (a mini zip
   reader in the script; **fix round 2:** the 30 are no longer "the first 30": the whole corpus is scanned, 24 documents
   whose `w:txbxContent` is NOT inside a table are spread evenly over the 147 that qualify, plus the first 6 of the 81 whose
   boxes are only in cells, the declared divergence), and adds two points: #4 the first note (`noteRefs()[0]`, `openNote`, typing, exit)
   and #5 the first text box (`floatSel = contextDrawingRuns(model)[ix]`, `editSelectedTextbox()`, typing, exit); facets
   `nk`/`nid`, `ix`/`pg`/`nd`/`ce`. The frozen bundle hash stays `6802c7b45d367dd1234f71d9f4ba2b99`. The native replay
   mirrors both (`NoteCmd::Open`; `SelectObject` + `EditSelectedTextbox`). Result (fix round 2, recaptured): 160
   documents, 1,379 facets equal, 0 mismatches. Coverage per context (documents whose point has a real usage): open 160,
   body + bold 160, header/footer 6, cell 44, note 32 (all equal to the element), **text box 33 in the capture, 22
   reproduced** (it was 1 of 33 with the first-30 pick), 11 not opened. The summary line now prints the skipped text-box
   facets ("11 text-box points whose fu/n are NOT replayed"), not only "0 not replayed".
   **Finding (first sample: 32 documents, now 11):** in the documents the element lists one drawing run (`nd=1`, in the body, `ce=0`) and opens its box, while
   the session's `drawing_paths()` of the body story is empty after the same sequence (`SelectObject{0}` leaves
   `float_selection()` = None, context stays Body). The documents are the oracle `cellout`/`cellnote` ones whose box
   anchor sits in a table cell paragraph (reproducer: `write/packages/docx-layout/test/oracle/cellout/co11-in.docx`,
   `UO_FU_DUMP`-style run of `session_font_usage`); the test declares them (prints the 32 names, skips their facets)
   instead of loosening a comparison. It is either a divergence of `loc::walk_story` (`Mode::Drawing`) on boxes inside
   tables or of the context the capture is in: for the owner of `session/ops/drawing` (outside this frontier).
   **Pinned (fix round 1):** `session_font_usage` asserts that exactly 11 text boxes are not opened and that at least 20 are
   reproduced (`covered[5] >= 20`), for the full 160-document run: a fix of the divergence makes the count drop and a regression
   makes it grow, either one fails the test and asks for this record to be updated. Reproducer for the owner of
   `session/ops/drawing`: `write/packages/docx-layout/test/oracle/cellout/co11-in.docx` (a box whose anchor sits in a table
   cell paragraph; `SelectObject{0}` leaves `float_selection()` None). Listed in the gate list below; the HANDOFF queue is not
   an owned file of this frontier: **for the orchestrator**, add the reproducer there.

8. **Dedupe in `handle.rs`.** `variant_name` is `uo_fonts::Variant::name()`; the clock arithmetic uses
   `uo_docx::session::ops::tables::sort::days_from_civil` (raised from `pub(crate)` to `pub` with `#[must_use]`, additive,
   declared; **WS-D14:** a hostile `SetClock` with a month above 14 now follows the session's DATE-field arithmetic, the
   old copy differed there; neither reproduces the JS `Date` overflow: validate month/day at the gate);
   `uo_session_layout` calls `layout::editor_options` and restores an explicit revision mode of `flags` (**WS-D15**).
   Results stay bit-identical: `session_native` 11 pass, `session_api` and the replay unchanged.

**CI** (`.github/workflows/rust-core.yml`, the YAML parses, 16 jobs): `server-pdf` gains `cargo test -p uo-docx-layout
--release --test la7` (full bounds; the debug `test` job runs the reduced ones) and a comment; `soak` gains
`cargo test -p uo-wasm --release --features session,count,session-selftest --test session_native -- --test-threads=2`;
`wasm` gains the session-selftest build and three node steps (`session_api`, `session_hostile` with
`UO_WS_FUZZ_MS=10000`, `session_replay` with `UO_WS_FAMILIES=d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10`, each with `UO_WASM_PATH`), and the Safari-15
step builds and validates the shipped `--features session` module and prints its size (**fix round 1:** these three lines now
come AFTER the shipping lines, which copy the `--features write` module as `uo-write.wasm`; before, the session module
overwrote it first and the shipping check silently exercised another module); the `codec` job's `tsc` list gains
`crates/uo-wasm/js/session.d.ts` (checked locally with typescript 5.9.3: exit 0). **Not added:** the requested
`cargo test -p uo-ffi --features editing-api,count` is invalid (uo-ffi has no `editing-api` feature; it is always on through
the uo-docx dependency, `cargo` answers "does not contain this feature"); the `editing_*` tests already run in the `test`
job (debug, `--workspace --all-features`) and in `server-pdf` (`-p uo-ffi --release --all-features`), named in a comment. If a
visible line is wanted: `cargo test -p uo-ffi --features count --test editing_abi --test editing_memory --test editing_nopanic`.

**Files touched outside the frontier's own files (each additive or test-only, declared):**
`core/tools/uo-schema/tests/bind_generator.rs`, `tests/schemas/bind_test.bind` and the regenerated
`tests/fixtures/bind_test_bind.rs` (the grammar change); `core/crates/uo-docx/src/session/ops/tables/sort.rs`
(`days_from_civil` `pub`); `core/crates/uo-docx/tests/d2b_f3_support/session_codes.rs` (`pub fn commands`);
`core/crates/uo-docx/tests/session_font_usage.rs`, `js/capture-font-usage.mjs`, `tests/parity/font-usage.capture` (item 7);
`core/crates/uo-wasm/src/lib.rs` (the `#[path]` mods of `d2b_f3_support`, `d2b_f9_support`, `d2b_f10_support`, selftest only; rustfmt
also reordered the mod lines); `core/crates/uo-wasm/Cargo.toml` (`session-selftest` gains `uo-docx/export`);
`core/crates/uo-wasm/src/session/env.rs` (the test-only draw counter); new `src/session/selftest/{fam,f10,f8,f7}.rs`;
`core/crates/uo-wasm/tests/session_support.mjs` (new: the shared module loader, fixtures, open fonts). The
`bind_native.rs` experiment of item 1 was reverted. No `schema/session.uos` change, no new wire op.

#### For the milestone-2 gate

1. `CommandError::Locked` is a dead variant (never built): remove it from uo-docx and the schema, or make protected-mode
   commands answer it (the TS answers silently; 0.10 keeps that). `pasteText` in a protected session is not guarded, as the
   TS: keep or fix at the TS.
2. Replay in wasm covers d2a, f1, f2, f3, f5, f6, f7, f8, f9, f10. **Not replayed:** f4 only (needs `undo_reentering` /
   `redo_reentering` with a shell-chosen re-entering, no wire op: recommended additive `ExecOp::UndoReentering(OptHfReenter)` /
   `RedoReentering`). The mappers and facet functions of d2a, f1, f2, f6, f5, f7, f8, f9 and f10 are copies of the native
   replays' (f3 reuses the support function): recommended for the gate, move every family's "code -> actions" mapper into its
   `tests/d2b_fX_support` so the native replay and the wasm self-test share one copy.
3. Native-only facets stay native-only: pend, sv/nv of f1-f6, rev, enc, n (the F8 `nv`/`sv`/`rc`, F9 `cm` and the F10
   `fvs`/`pgc`/`pgv`/`mhc` are compared in wasm now).
4. The 11 text boxes of item 7 (fix round 2; 32 in the first sample) that the element opens and the session does not: owner of `session/ops/drawing`.
   Also declared: the F8 `@setup` commands and the F7 `PlaceAt` second halves go through the codec and `exec`, but the F7
   `rc` advance (`env_mut().random_u32()`) is a test-only side channel into the session's environment.
5. A hostile `SetClock` should be validated at the gate (`LocalNow::from_wire`): month 1-12, day 1-31, minute 0-59.
6. The `uo-ffi` CI line (above): keep the comment, or add the explicit `--test` line.
7. The shipped session module (`--features session`, validated against the Safari 15 feature list) is 9,538,764 B raw /
   3,126,149 B gzip -9 against 9,230,855 / 3,029,327 in the wave-2 record: the growth comes from the editor-switch frontier's
   shell code in the same worktree, this frontier's shipped changes (handle.rs dedupe, WS-D13) are a few hundred bytes.

#### Process metrics

One implementer agent, about 3 h wall time, about 120 tool calls. Builds: the session-selftest module 4 times
(first cold 2 min 19 s by the planner, then 55 s-2 min incrementals), uo-wasm native test builds 2, the uo-docx
`all` test binary 5 incremental rebuilds (48 s the first, 4 s the rest), `check-changed` once. Rework: the Bash tool
halved backslashes and refused a heredoc with a quote, so scripts with escapes went through the Write tool; the bind
comment rule swallowed `r#type` (found by the grammar test); `rustfmt lib.rs` reordered `mod` lines and a use in
`write.rs` (reverted); the first font-usage native run failed because the element leaves the cell before the note point
even when the document has no note (mirrored); the protected-session assertions assumed `pasteText` is locked (it is not
in the TS). Disk: D: 65 GB free throughout.

**Fix round 2** (second review: f7 not replayed, stale queries never in a cell, f7-less record, text-box sampling, inconsistent hostile
numbers, F8 setup bypass): f7 ported (above), the stale tests query in cells, the text-box pick recaptured, the hostile test
names its worst call, the F8 setup runs as exec actions. Final numbers: replay 10 families (d2a 120 docs / 49,800 equal, f1 96 /
39,840, f2 120 / 37,559 + 1, f3 70 / 40,390, f5 60 / 44,940, f6 60 / 18,780, f7 40 / 27,151 + 64, f8 54 / 43,132 + 122, f9 80 /
54,803 + 10, f10 77 / 76,346 + 53), 0 mismatches, 0 re-encode differences; `session_native` 11 pass (feature `count`);
`session_api` 5 pass; `session_hostile` 725,220 calls; `session_font_usage` 4 pass (160 documents, 1,379 equal);
`check-changed` clean (clippy native and wasm32, float-cast and ban scripts); rustfmt on `selftest.rs` and `session_native.rs`.
A `python open(p,'w')` with the default Windows encoding truncated `session_replay.test.mjs` to 0 bytes during the round (a
non-cp1252 character); it was rewritten from the earlier read: use `encoding='utf-8'` in every script.

**Fix round 1** (reviewer findings): about 2 h, the session-selftest module built 4 times (2 min 20 s each, one cold),
`check-changed` once more (clean), the native f5/f8/f9/f10 replays re-run for the equal accepted counts. Rework: a relative
`CARGO_TARGET_DIR` typed from `crates/` created `core/crates/target` (an empty workspace member that broke every cargo
command; removed); `rustfmt` on `lib.rs` reformatted `write.rs` again (reverted); the first f8 run had 1,183 mismatches
(the ring's `clear()` keeps the current rich copy; the HTML of a copy needs `uo-docx/export`). Final numbers: replay 9
families, 0 mismatches; `session_native` 11 pass; `session_api` 5 pass; `session_hostile` 725,980 calls, live bytes 3,492
-> 3,492; `uo-schema check` 20 files up to date; D: 54 GB free.

## Milestone 2: the editor switch

Queue item 9 (wave 3, frontier E, 2026-11-11): an opt-in `engine="rust"` on `<universal-write>` under which the element's
document model, editing, layout and page painting come from the wasm `UoWriteSession`; the TS keeps only what
uo-docx.md "Stays in the TS shell" lists. The default (`engine` absent or any value but `rust`) is the TS engine,
unchanged: no wasm request, no engine code in the entry's static closure (phase 0). Plan by the planner of the wave
(Opus); implementation and verification in this section.

### Inventory of `universal-write.ts` (measured at 27f395db, unchanged since)

23,473 lines; class `UniversalWriteElement` at 1231–23295: **511 public methods** (+ 11 getters/setters, 3 statics),
≈ 600 private ones. References: `this.model` 596, `this.selection` 298, `this.fullDoc` 153, `this.eng` 110,
`this.cellEdit` 119, `snapshot()` 141 call sites, `locked()` 162, `dispatchEvent` 65 (35 distinct `uw-*` types).
Model/layout references per concern (line range: count): open/new/load 1797–2460: 34; save/dirty/print 8530–8910: 32;
getSelection/getSelectionState 2960–3320: 11; format commands 3320–4760: 105; clipboard 4757–5700: 32; undo/redo
(string snapshots) 9520–9735: 29; typing 9736–9914: 20; caret moves: 10; DOM listeners (bindEvents) 9995–10900: 39;
header/footer and the legacy layout 12179–13310: 34; hit test: 14; engine layout and fast paths 16144–16507: 44;
engine paint 16580–16760: 22; screen paint 21399–22080: 15; PDF/print 22365: 10. The bars register their commands in
`chrome-commands.ts` (`supports: typeof ed[m] === 'function'`) and read the toolbar state from `getSelectionState()`.

### Routing design (decision D1: instance shadowing, not an interface refactor)

The editor does not fork its 23k lines and the TS path is not refactored (it is the frozen behaviour and stays the
default until 0.10). `write/packages/editor/src/engine-routes.ts` (static data, no engine import) classifies the
element's methods; `rust-engine.ts` (reached only by a dynamic `import()`) installs them on the INSTANCE with
`Object.defineProperty` at `initialize()`, before the first load:

- **ROUTED** (68 public + 18 private since fix round 2): handled by `RustEditor` over the session (table below). The private ones are
  the TS methods that SHELL code calls into (`refreshView`, `refreshSelection`, `rebuildCanvases`, `paint`,
  `positionProxy`, `scrollCaretIntoView`, `type`, `moveCaret`, `popHistory`, `hitTest`…): the TS shell code (scroll
  virtualization `ensureVisiblePainted`, `setZoom`, the caret blink, the bars) keeps running and reaches the session.
- **SHELL** (26 since fix round 2): run their TS code unchanged (zoom, view getters, theme, plugins, notices, fonts list,
  side panels). `getFontEmbedding`/`setFontEmbedding` were SHELL until fix round 2, but they are document state (the TS
  reads them from the package at load): routed since (review finding).
  `currentPage`, `stats` and `pageWidthZoom` were SHELL in the first round and read TS state that is never filled
  under `rust` (page count 1, words 0, the zoom unchanged): routed since fix round 1 (ED-20).
- **Everything else is closed by default** (fail-closed): a mutator (by name, `isMutator`) is a no-op warned once
  (an `async` one answers a resolved promise, the bars chain `.then` on them); a query runs its TS code over the EMPTY
  TS document (the TS model is never loaded under `engine="rust"`), and a query that throws answers `neutralOf(name)`
  ([] for plural names, `null`, the markup defaults the bars destructure). 292 disabled mutators, 125 disabled queries
  (fix round 2). Four of those queries answer the neutral value WITHOUT running the TS code (`NEUTRAL_QUERIES`, ED-24):
  over the empty TS model they would answer a false value of the document.
  `engine-public.ts` is the generated list of the 511 public names; `engine-switch.test.ts` fails when the source's
  public methods drift from it, so a new public method is disabled until classified.
- **Input**: capture-phase listeners on the hidden textarea (`keydown`, `input`, composition, `copy`/`cut`/`paste`) and
  on the host for events whose path holds the pages container or the hidden textarea (`mousedown`, `dblclick`,
  `contextmenu`; the 2nd and 3rd press of a double/triple click at the caret land on the 1 px textarea, which sits on the
  caret) call `stopImmediatePropagation()`, so the TS bubble listeners of `bindEvents` never see them; the bars, side
  panels and the scroll listener are untouched. A key the shell does not handle itself is forwarded to the element (ED-21),
  so the bars' shortcuts keep working with the focus in the document.
- **Bars**: in `chrome.ts`, right after `registerWriteCommands`, every command whose id is not in
  `ROUTED_COMMAND_IDS` (57 ids, each read in its `run`; Cut, Copy, Paste and Paste as text left out in fix round 1:
  their `run` calls `cut()`/`copy()`/`paste()` without a clipboard event, ED-16) gets `isEnabled: () => false` —
  visible, grayed out, never run on the TS model (D7). Widgets that call the API directly (font, size, the paragraph
  style dropdown) reach routed methods; the styles gallery and pane list nothing under `rust` (ED-24).
- `universal-write.ts` changes: 34 lines (the `engine` getter and observed attribute, `configureEngine`,
  `engineStats`, the `rust` field, `startRust()` after `installFontAssets`, `park`/`unpark` in disconnected/connected).

Why not an `EditorEngine` interface with a TS implementation wrapping today's code: that refactor touches every one of
the ~1,100 methods of the frozen path for a structure that disappears when the TS engine is removed at 0.10; the route
table becomes the shell's call map at the gate instead.

### What is routed

| Element method / input | Session |
|---|---|
| `loadDocx`, `load('docx')` (`.doc` converted first) | `bindSession(...).open(bytes, { protected, author, clock, seed: 1 })` + `ShellEngine` |
| `newDocument()` | `newBlank({ lang: spellcheck-default-lang ?? 'pt-BR' })` |
| `saveDocx()` / `save('docx')`, `saveDocxWithFonts()` | `save()` / `fontRequests()` → `FontResolver` bytes → `saveWithFonts()` (events re-dispatched) |
| `save('pdf')`, `exportPdf()` | `editsSinceLoad === 0` → the original bytes laid out by a `ViewerDocument` (D12), else the current layout's `pdf()` |
| `isDirty`, `setDirty`, `isProtected`, `enableEditing`, `isEmpty` | `ChangeSet.dirty`; `setDirty`; `settings.setProtected { on: false }`; `storyInfo()` |
| typing (`input`, `compositionend`), `uw-before-input` | `type(text)` |
| Enter, Backspace, Delete, Ctrl+Backspace/Delete | `enter()`, `backspace()`, `delete()`; by word: `moveCaret(word)` + `setSelection` + backspace/delete (ED-4) |
| arrows (+Shift), Home/End, Ctrl+Left/Right, Ctrl+Home/End, PageUp/Down, Tab, Shift+Tab | `moveCaret(dir, extend, layout)`; `storyInfo()` + `setSelection`; scroll + 20 line moves; `list.changeListLevel(±1)` at a list item's start, else `type('\t')` (ED-26) |
| mouse down / Shift+click / drag / double / triple click | `hitTest(layout, page, x, y)` + `setSelection`; `paraText()` for the word and the paragraph |
| `setSelection`, `selectAll`, `selectedText` | `setSelection`, `navigation.selectAll`, `query selectedText` |
| `undo`, `redo` (Ctrl+Z/Y, Ctrl+Shift+Z) | `undo()`, `redo()` |
| `getSelectionState()` | `selectionState()` (shell export, below) + `view`, `marks`, `table: null` |
| `toggleRunProp`, `toggleUnderline`, `setUnderline`, `setFontFamily`, `setFontSize`, `growFont`, `setColor`, `setHighlight`, `clearFormatting`, `changeCase`, `toggleVertAlign`, `setCharSpacing`, `clearCharacterFormatting`; Ctrl+B/I/U, Ctrl+Shift+D/W/H/K/A, Shift+F3 | `format.*`, `style.clearCharacterFormatting` (the Underline button style is shell state, as the TS `ulButton`) |
| `setAlignment`, `indent`, `setLineSpacing`, `setSpaceBefore`, `setSpaceAfter`, `setShading`, `clearParagraphFormatting`; Ctrl+E/L/R/J | `paragraph.*` |
| `toggleList`, `removeList`, `changeListLevel`, `applyStyle` | `list.*`, `style.applyStyle` |
| Ctrl+C / Ctrl+X (clipboard events) | `clipboard.copy`/`cut` + `copied()` → `text/plain` (+ `text/html` when the build has it) |
| Ctrl+V / Ctrl+Shift+V (the `paste` event; fix round 2) | `clipboard.pasteData { text, html: encodePaste(text/html), rich: 'withRich', mode: 'text' after Ctrl+Shift+V, layout: {} }` (the TS `bindEvents` paste listener; ED-16) |
| `uw-pasted`, `pasteOptions()`, `repaste()`, `pasteButtonRect()` (the Paste Options button and the Ctrl menu) | the event's detail and `pasteOptions()` from `query pasteOptions` (`noMerge` → `nomerge`); `clipboard.repaste`; `query lastPasteRange` + `caretRectAt` |
| `getFontEmbedding()`, `setFontEmbedding(patch)` (fix round 2) | `query fontEmbedding`; `setFontEmbedding(patch)` (nothing when nothing changes, as the TS) |
| `currentStyles()` (styles gallery and pane, fix round 2) | `selectionState()`: `para.styleId ?? 'Normal'`, `run.charStyle` (ED-23) |
| `exportOdt()`, `exportXps()`, `exportHtml()`, `exportMarkdown()` (fix round 2) | throw a clear error (ED-18): Save As .odt fails instead of writing `undefined` |
| paint, scroll, zoom, thumbnails, caret, `caretRectAt(focus)` | decoded pages (`DecodedLayout` of js/viewer.ts), `caretRect`, `selectionRects` |
| `refresh()` | a new clock epoch + relayout |
| `currentPage()`, `stats()`, `pageWidthZoom()` (bottom bar, word count dialog, mobile fit width) | the decoded pages and the canvases; `storyInfo()` + `paraText(i)` (cached per document change, ED-20) |
| any other key (Ctrl+S, Ctrl+O, Ctrl+], Ctrl+[, Ctrl+Shift+N, F-keys, Esc, Alt+…, AltGr/Option characters, printable keys) | not run by the shell: forwarded to the element for the bars' shortcuts; the browser's default prevented only for the keys the TS `bindEvents` always prevents (ED-21) |

Each outcome is applied the same way: the selection mirrored into the element (`getSelection()` stays SHELL), dirty
and undo flags, a relayout only when `stories` or `formatting` say the document changed (a pure selection change
repaints the selection and the caret), the session's events re-dispatched in order as `CustomEvent(t, { detail })`
(no `detail` for payload-free events, as the TS), the caret scrolled into view, `onSelectionChange` and the bars
synced. `{ t: 'error' }` warns once; `{ t: 'jobs' }` (a two-phase command this wave does not answer) is cancelled.

**Layout and paint.** Every edit relays out synchronously through the shell's `ShellEngine` (LA7 incremental) and
the result is adopted as a `DecodedLayout` (pages decoded lazily). The page base (background + items, the
header/footer layer at `HF_DIM_ALPHA`, as the TS `engineBase`) is cached per page and invalidated only for the
report's `relaid` pages (all on a full relayout or a page-count change); the selection rectangles and the caret are
drawn over it. `rebuildCanvases` mirrors the TS (same canvases, buffers released, the visible ones reallocated by
`ensureVisiblePainted`). Fonts, hyphenation, the page environment and the paint sources are the viewer's
`RustSession` (`write/packages/viewer/src/rust-engine.ts`) reused through `adoptDocument()` — the single font
resolution place of CLAUDE.md; its font loop (`relayout`, up to 5 passes) runs after open and whenever a relayout
reports missing faces or the resolver changes.

### Shell exports (uo-wasm `src/session/shell.rs`, `shell_state.rs`; js/session.mjs)

| Export | JS | Answer |
|---|---|---|
| `uo_shell_engine_new/free/clear` | `ss.newShellEngine()`, `free()` (+ FinalizationRegistry), `clear()` | an incremental `LayoutEngine` per edited document |
| `uo_shell_relayout(e, h, env, now, tz, flags, lang, hyph, max_pages)` | `engine.relayout(session, env, o)` | an owned `UoLayout` (the options exactly as `uo_session_layout`) |
| `uo_shell_relayout_report(e)` | `engine.report()` | `{ path, reason, relaid: [a, b), passes, lineShift, memoHits, memoMisses }` (26 bytes) |
| `uo_shell_story_info(h)` | `storyInfo()` | `{ paras, lastLength }` |
| `uo_shell_para_text(h, i)` | `paraText(i)` | UTF-16 text or null |
| `uo_shell_hyphenation_patterns(h, env, langs, flags)` | `hyphenationPatterns(env, langs, { tsHyphenation })` | the pattern ids the document needs (`uo_write_hyphenation_patterns` over the session) |
| `uo_shell_copied(h)` | `copied()` | `{ text, html }` of the last copy/cut, or null |
| `uo_shell_selection_state(h)` | `selectionState()` | `{ run, para, pending }`: the generated wire `RunProps`/`ParaProps` structs (no header) decoded with the generated `readRunProps`/`readParaProps` |

`selection_state` ports `getSelectionState()` (universal-write.ts 2992–3011): `pendingProps ?? currentProps()`
(`props_at` of the focus), the paragraph props with the style's alignment, the font and size filled through the style
cascade (`styleLook`: `StyleResolver` para/char style, `cascade_rpr`, `font_of_slot` with the theme, the TS fallbacks
`Liberation Sans` and 22).

### Declarations (decisions outside the TS semantics)

- **ED-1** routing by instance shadowing + fail-closed table + capture listeners (D1 above).
- **ED-2** `uo_shell_relayout` repeats ≈ 20 lines of the option code of `handle.rs` `uo_session_layout` (not this
  frontier's file); the engine's `Layout` is **cloned** into the handle after every relayout (`Clone` derived on
  uo-docx-layout `Layout`/`LayoutPage`, declared edit (a)): the handle owns its pages. Gate: fold the engine into the
  session handle with a borrowed or incremental layout handle (removes the clone and the duplicate).
- **ED-3** the shell engine keeps its font provider while (session, registry pointer, registry generation, embedded
  font count) holds: a provider built per call answered new face identities and every keystroke fell back to
  `Provider` (measured: 0/20 incremental before, 20/20 after).
- **ED-4** Ctrl+Backspace/Delete = the TS `deleteWord` composed from session calls (no selection: move by word without
  extending, select from the old focus, backspace/delete). Fix round 1: when the move does not move (the story's start
  or end) nothing happens, as the TS (the first round ran a backspace there and emitted the snapshot/discard dirty pair,
  found by the new Ctrl+Backspace step of the harness, f7d__7__1).
- **ED-5** `caretRectAt(pos)` answers only for the selection's focus (other positions: null; the session has no caret
  query by position).
- **ED-6** `uw-change` and `uw-dirty-change` are dispatched after the mutation (the TS fires them inside `snapshot()`,
  before it): listeners see the new state. Measured: the event SEQUENCES are equal (below), only the timing moved.
- **ED-7** clicks resolve in the body scope: a click on a table cell, a header/footer, a note, a text box or a floating
  object does not enter that context (the TS emits `uw-cellmode`/`uw-tbmode`, or `snapshot()` + `discardSnapshot()` for
  a click on a floating object: `uw-dirty-change true, uw-change, uw-dirty-change false`); `tableNavigate` is absent
  (WS-D7).
- **ED-8** TS screen overlays not painted: field shading and comment highlight, formatting marks, bookmarks, spelling
  squiggles, search hits, the TOC control, table selection and adorners, multi-selection, plugin inline renderers,
  touch handles; print view only (web/draft/read disabled); no spellcheck under `rust`.
- **ED-9** layouts of the editor are not cancellable (the viewer's `cancelCheck` is ignored by the adapter).
- **ED-10** the clock is fixed per "refresh epoch" (open or `refresh()`), so LA7 stays incremental (D-LA7-24); date
  fields do not tick while typing (the TS fast path does not either between full layouts).
- **ED-11** one module per page with the session build (`chunks/uo-write-editor.wasm`, loaded through
  `loadViewerModule`): a viewer-only module loaded first on the page makes `engine="rust"` fail with a clear error.
- **ED-12** `engine` is read at `initialize()`; changing it later has no effect until the next initialization
  (D6: no live switch, which would lose the undo history).
- **ED-13** `loadDocx` returns before the first layout when the page environment has no fonts yet: the pages appear
  after the font loop's first pass (`el.rust.idle()` awaits it); later documents of the page lay out synchronously.
- **ED-14** `selection_state`: `withLatent` (a built-in style the document never defined) and the editor's built-in
  `STYLES` table (documents without `styles.xml`) are not ported (such ids resolve as undefined styles); the default
  paragraph style is the sheet's `defaults.paragraph` (TS: the index's first `w:default` paragraph style, else `Normal`).
- **ED-15** the wire `src` of the run and paragraph props travels as an option of an option (`{ value }`); the shell
  flattens it to the TS number in `getSelectionState()`.
- **ED-16** (rewritten in fix round 2) the keyboard paste (the `paste` event of Ctrl+V and Ctrl+Shift+V) is the TS
  `bindEvents` paste listener over the session: `text/plain`, the `text/html` item encoded by `encodePaste()` (the
  DOMParser tree of the D2b-F8 boundary, core/crates/uo-docx/js/paste-tree.mjs, re-exported by js/session.mjs) with the
  editor's own `fragmentOf`, `rich: 'withRich'` (the session's internal copy when the system text is still its own) and
  `mode: 'text'` after Ctrl+Shift+V (the TS `plainPasteNext`), sent as `clipboard.pasteData` (uo-docx paste.rs
  `paste_data`: the formatting policy, the ring, `Event::Pasted`). The first two rounds called `clipboard.pasteText` (the
  port of the public `pasteText` API method) and lost `uw-pasted` and the TS paste's formatting: review finding of fix
  round 1, fixed. `uw-pasted` travels without detail (F8-D3); the shell dispatches it with `pasteOptions()` (routed:
  the session's query in the TS form), so the Paste Options button (`pasteButtonRect()`, only while the caret is at the
  paste's end, ED-5) and the Ctrl-alone menu (`repaste`) work. Not routed in this wave: `layout.contentWidthTwips` is
  absent (the session uses the body section's print width, F8-D3; the TS `contentWidthTwips()` is the caret's section,
  equal for single-section documents and in print view); an image-only clipboard item (a two-phase decode job: warned,
  nothing pasted); `copy()`/`cut()`/`paste()`/`pastePlain()` called WITHOUT a clipboard event (the bars' Cut, Copy,
  Paste and Paste-as-text commands stay DISABLED, D7; the keyboard paths work), Paste Special and the ring pane; the
  element's `pasteDefaults` field (the bars' saved Set Default Paste preferences) does not reach the session, which
  keeps its own defaults (the TS defaults: within/between `keep`, other `default`). `copied().html` is empty in the
  shipped session build (uo-docx `export` is not in it, F8-D5): the system clipboard gets `text/plain` only, but a
  Ctrl+V in the same editor still pastes the session's own rich copy through `withRich` (as the TS). The shell's
  `text/plain` has LF between paragraphs where the TS's has CRLF (the parity facet `clipboard text`).
- **ED-17** without a `fonts` base the element's installed font assets are pushed to the page environment as assets
  (no advance tables).
- **ED-18** `.odt` load and the html/markdown/odt/xps exports are not routed: `loadOdt` warns; `exportOdt`,
  `exportXps`, `exportHtml` and `exportMarkdown` throw a clear error (fix round 2: they were disabled mutators answering
  `undefined`, so the bar's Save As with the ODT format handed `undefined` bytes to `host.saveFile` or the download —
  review finding). The bar's save catches the error and reports a failed save (the file-picker and host paths); `save()`
  with another format than docx/pdf rejects.
- **ED-19** a reconnected element reopens the document saved at disconnect (the undo history is not kept).
- **ED-20** (fix round 1) `stats()` counts words and characters over the session's `paraText` of the body paragraphs
  (the TS `plainTextSlice` skips note reference marks and counts a plugin inline as its `fallbackText`; the session's
  text holds the run text of both); inside a note or a text box the TS counts the saved body, the shell always the
  body (it never enters those contexts, ED-7). `pageWidthZoom()` uses the width of the caret's page (the focus
  section's page in print view, the only view of this wave) instead of `sect().pageWidthTwips`. `currentPage()` is the
  TS algorithm capped by the layout's page count.
- **ED-21** (fix round 1, corrected in fix round 2) a key the shell does not run itself (a Ctrl or Alt combination
  outside its own table, AltGr and Option characters, the F-keys, Esc, printable keys) stops on the textarea (the TS
  `bindEvents` listeners never see it) and a synthetic copy (`key`, `code`, modifiers, `repeat`, `location`; bubbling,
  composed, cancelable) is dispatched on the element, where the bars' shortcut listener (`bindDocumentFeatures`) and host
  listeners see it; when a listener prevents the copy, the original is prevented too. Otherwise the browser's default
  runs, as in the TS — EXCEPT for the keys the TS `bindEvents` always prevents (`tsOwned`: the navigation keys
  with any modifier, Enter, Backspace, Delete, Tab, PageUp/PageDown, F9, Shift+F3, Ctrl+Alt+F/D, Ctrl+Alt+V, Ctrl+hyphen,
  the Ctrl letters of its map and the Ctrl+Shift letters D/W/H/K/A), whose default stays prevented although their TS
  function is disabled here (Alt+ArrowLeft does not go back in history, Shift+Tab does not move the focus out, Ctrl+Enter
  inserts nothing). The copy is untrusted (`isTrusted` false). Ctrl+C/X/V and Ctrl+Shift+V keep their native default
  (the clipboard events). Fix round 1 prevented every Ctrl/Alt/F-key/Esc original on the premise that they insert
  nothing, which is false: on Windows AltGr reports `ctrlKey` and `altKey` together, so @ (AltGr+2 or AltGr+Q), the euro
  sign (AltGr+E), `[ ] { } \ | ~` on es-ES and the Mac Option characters typed nothing, and F5/F11 lost their browser
  default (review finding of fix round 1, reproduced with CDP: TS offsets 0 → 1 → 2 → 3, Rust 0). `--mode keys` checks
  AltGr+2, AltGr+E and Option+2 and the default of ten keys against the TS. Remaining difference: the TS runs some of
  those owned keys with Alt (Alt+Arrow moves the caret, Alt+Enter is Enter, Ctrl+Alt+B is bold); the shell prevents them
  and runs nothing.
- **ED-22** (fix round 1) PageUp/PageDown scroll by 0.9 of the view as the TS `bindEvents` (the first round scrolled
  by the whole view), then 20 line moves through the layout.

- **ED-23** (fix round 2) `currentStyles()` is routed (the styles gallery's checked item, the styles pane, Modify
  Style): the paragraph's own `styleId` from `selectionState()`, else `Normal` (the TS `docStyles.defaultParagraph()`, the
  document's `w:default` paragraph style: equal for the documents whose default style id is `Normal`; the session does
  not expose that id), and the run's `charStyle` (`pendingProps ?? currentProps()` in both).
- **ED-24** (fix round 2) four disabled queries answer the neutral value without running the TS code
  (`NEUTRAL_QUERIES`): over the empty TS model they answered a false value of the document (review finding).
  Visible effects: `listStyles()` → [] (the styles gallery and the styles pane are EMPTY; the Find dialog's style list
  and the TOC dialog's styles too); `paragraphStyles()` → [] (the paragraph style dropdown falls back to its built-in
  Normal / Title / Heading 1–3 list, whose ids reach the routed `applyStyle`); `listStylesInDocument()` → [] (the list
  gallery shows no document list styles); `isTrackingChanges()` → false (the session reads `w:trackRevisions` at open,
  but no query exposes it; the Track Changes command is disabled, D7, so the bar's flash text never asks). Still
  answered by the TS code over the empty model: `knownFontFamilies()` (SHELL) lists the fonts installed through
  `initialize({ fonts })`/the `fonts` base but NOT the document's embedded families (the TS adds them at load:
  `registerEmbeddedFontMetrics`; the session reads them for layout, no query lists them), so the font menus lack them.
- **ED-25** — **retired in wave 7c (S)** ("Wave 7c — S", item 3: a detached element keeps its session, shell engine, layout
  and every front's state, as the TS; freed when the element is collected). Was: (fix round 2) `engineStats().editors`
  counts the CONNECTED `engine="rust"` elements: a disconnect parks the editor (document saved, session freed) and leaves
  the count, a reconnect re-enters it. The first rounds had a
  `dispose()` that nothing called (the count only grew; review finding): removed. `--mode memory` asserts 1 while open
  and 0 after each remove.
- **ED-26** (fix round 2) Tab / Shift+Tab are the TS `bindEvents` rule: at the start of a list item or with several
  paragraphs selected, `list.changeListLevel(±1)`; when the session answers no change (no selected paragraph in a list),
  Tab types a tab and Shift+Tab does nothing. The first rounds always typed a tab (Shift+Tab: nothing), hidden in the
  parity runs behind f7a__7__8's earlier clipboard divergence until the paste was routed. Difference: the TS asks whether
  the FOCUS paragraph is a list item (with the style's numbering), the session whether ANY selected paragraph is; they
  differ only for a multi-paragraph selection whose focus paragraph is not in a list. The TS's `true` without a change
  (a formatting policy refusing the level) types nothing; the shell then types a tab.
- **ED-27** (merge, orchestrator) the TS model stays empty under `rust` also when the host calls `load*()` before
  `uw-ready`: the routes exist only once `startRust()` returns, so such a call runs the TS load. `startRust()` now resets
  `model`, `pkg` and `fullDoc` to null afterwards (the safety net of the disabled queries and the TS hover listeners is
  the null TS model). The document of that early call is not shown (`initialize()` opens the configured source or a new
  document through the session, as the TS `initialize()` also overwrites); hosts call `load*()` after `uw-ready`. A TS
  load still in flight when the session starts is not awaited (not exercised by the harness).
- **ED-28** (merge, orchestrator) no context menu on the pages under `rust`: the shell's capture `contextmenu` listener
  moves nothing and prevents both the bars' menu (chrome.ts) and the browser's. Declared in the README; routing the menu
  (its disabled items grayed, D7) is gate item 17.
- **Classification (merge).** `paragraphBorderPreset` is a mutator whose name matched neither `MUTATOR` nor `QUERY`, so it
  ran its TS body as a "disabled query" (inert only through the null TS model): added to `MUTATOR`. `engine-switch.test.ts`
  now fails when any disabled query's TS body takes an undo step or applies formatting (`this.snapshot(`/`applyPara(`/
  `applyRun(`/`pushHistory(`).
- **Builds (merge).** `build:offline-demo` and `build:site-live` (single-file bundles, the public site's live editor) did
  not resolve after `rust-engine.ts` arrived: they gained the `write-engine`/`write-session` aliases (the engine JS is
  then inlined, about 389 KB, run only with `engine="rust"`). `tools/build-pkg.mjs` `ENGINE` gained
  `@universal-office/write-session`: without it `build:pkg write` fails. Shipping `chunks/uo-write-editor.wasm` stays D5.

### Numbers

Fixtures: the 74 committed documents of `core/crates/uo-docx-layout/tests/captures/*/docs`, Chromium headless
(chromium-headless-shell 1228 of the frozen Playwright), 1280×900, dpr 1, UTC, a fixed clock, `Math.random` seeded,
reduced motion.

**Parity, full run, fix round 1** (`UO_ES_STRIDE=1 UO_ES_STEPS=12`, seed 1; 888 scripted steps of real mouse and
keyboard events, the TS run first, the Rust run replaying the same events). The scripts changed in fix round 1: each
fixture's script is seeded by its file name (the same script at any stride or `UO_ES_ONLY`; the first round seeded by
the fixture's index, so a stride-6 fixture ran another script than at stride 1), and the steps now cover every routed
input path of the table above: Shift+click, drag, double and triple click, IME composition (CDP
`Input.imeSetComposition` + `Input.insertText`), ArrowUp/Down, Shift+ArrowDown, PageUp/PageDown, Ctrl+Backspace/Delete,
Tab, Ctrl+Home/End, and clipboard steps (Ctrl+Shift+Left, Ctrl+C or Ctrl+X, the copied `text/plain` read as a facet,
then a fixed plain text `colado` put on the clipboard and pasted with Ctrl+V or Ctrl+Shift+V). **The mode FAILS on any
difference that its `DECLARED` table does not cover** (review finding, fixed: the first round only tallied):

Fix round 2 (the keyboard paste through `pasteData`, Tab/Shift+Tab, the key defaults; the same scripts, so the
numbers compare with fix round 1's in brackets):

| Facet | Equal | Different (all declared) |
|---|---|---|
| page count after load | 74 | 0 |
| selection after each step | 799 | 89 (94) |
| caret rectangle (relative to its page canvas) | 791 | 97 (105) |
| `uw-*` event sequence (type + detail) | 866 | 22 (70) |
| toolbar state: run props | 818 | 70 (70) |
| toolbar state: paragraph props | 822 | 66 (71) |
| `isDirty()` | 888 | 0 |
| clipboard `text/plain` after a copy/cut | 876 | 12 (12) |
| page count and sizes after `refresh()` | 72 | 2 (2) |
| page thumbnails, ×1 and ×2.5, pixels at tolerance 0 | 1,050 | 100 (106) |
| screen canvases, caret hidden, TS overlays off | 146 | 29 (31) |
| screen canvases, TS overlays on (ED-8, informative, never fails) | 72 | 103 |
| saved `.docx` part names | 73 | 1 (1) |
| saved `.docx` part bytes | 647 | 14 (17) |
| page errors | 74 fixtures | 0 |

Two full runs in fix round 2: the first (paste routed, before the Tab fix) exit 0 with 27 entries used and 15 entries
stale; the second (Tab fixed, `DECLARED` regenerated) exit 0, `editor_switch: ok`, 5 min 27 s, every entry used, none
stale. **48 of the 74 fixtures are equal in every facet** (fix round 1: 32). The other 26 each have one `DECLARED`
entry (the first differing step, the step facets that may differ from there on, the end facets, the declaration).
Entries by cause:

- **ED-16, the copied `text/plain` only (3: f7a__7__14, f7c__7__5, f8__7__5)**: a copy or cut over a paragraph mark;
  the TS joins paragraphs with CRLF, the shell writes the session's `copied().text` with LF. Every paste of the scripts
  is now equal (selection, events with the `uw-pasted` detail, toolbar state, saved bytes): the 23 ED-16 entries of fix
  round 1 were the paste through `pasteText` (review finding). f8__7__5 also diverges at step 11 (ED-7 below) and
  f7c__7__5 has other pages at the end on the same saved bytes (gate item 10).
- **ED-7, table cells by line moves (7: f7b__7__1/4/5/6/7/9, f7d__7__8)**: PageUp/PageDown/ArrowUp; the TS `moveCaret`
  calls `tableNavigate` and enters the cells (`uw-cellmode`), the session's `moveCaret` stays in the body story (WS-D7).
  f7b__7__5 was hidden behind its clipboard entry until fix round 2.
- **ED-7, a click on a floating object (5: f7b__7__10, f7d__7__6/7/10/11)**: the TS selects it (`uw-change`; in f7d__7__7
  the `snapshot()` + `discardSnapshot()` dirty pair), the shell resolves the click in the body.
- **ED-7, a double click on a note reference or a click in a text box (2: f7c__7__13 `uw-notemode`; f8__7__5 step 11
  `uw-tbmode`, and the TS saves from inside the text box: the saved `document.xml` differs)**.
- **Gate item 1, the caret/hit x mapping (7: f2-3-2, f2-3-3, f7a__7__1/2/3/4/13)**: a click at the TS caret x hits
  another offset (f2-3-3, new in fix round 2 behind its former clipboard entry: paragraph 16 offset 162 in the TS, 159 in
  the shell), and the line moves of ArrowDown/PageUp, whose goal x is the caret x, land elsewhere (f7a__7__1, f7a__7__3).
- **Gate item 10, other pages on the same saved bytes (2: f2-3-1, f7c__7__1)**: equal at every step; f2-3-1 and
  f7c__7__1 were declared ED-16 in fix round 1 (their paste step), now the end pages are their only difference.
- **BUG-1 (1: f7d__7__9)**: equal at every step and in the saved package; the screen of the end differs because the
  final selection is BACKWARD and `uo_session_selection_rects` answers no rectangle for it. Fix requested above
  (frontier W's file).
- f7a__7__8 (fix round 1: ED-16) is equal since the Tab fix (ED-26: its step 3, Tab at the start of a list item, changed
  the list level in the TS and typed a tab in the shell).

Fix round 1's numbers (same scripts, before the paste was routed): selection 794 / 94, caret 783 / 105, events 818 /
70, run 818 / 70, paragraph 817 / 71, clipboard 876 / 12, pages 72 / 2, thumbnails 1,044 / 106, screen 144 / 31,
overlays on 71 / 104, part names 73 / 1, part bytes 644 / 17; 32 fixtures equal, 42 declared (23 of them ED-16: the
TS emitted `uw-pasted` and the shell did not, and five pastes differed in content).

The first round's End-on-a-wrapped-line (gate item 2) and caret-after-typing (f8__7__7) divergences are not hit by the
fix-round-1 scripts; both items stay open. Fixed by this round's harness: the dblclick and triple click of the shell
(the 2nd and 3rd press at the caret land on the 1 px textarea, which the shell did not count as the pages: the shell
kept the caret where the TS selected the word or paragraph; `inPages` now accepts the textarea); Ctrl+Backspace/Delete
at the story's start or end (the shell ran a backspace with nothing selected, emitting the TS's snapshot/discard
dirty pair; the TS `deleteWord` returns without an edit: ED-4 now does too); PageUp/PageDown scrolled by the whole view
instead of 0.9 (ED-22).

**Stride 6 (13 fixtures, 156 steps, `--mode all` with the count build)**: phase 0 (no `.wasm` request, no engine chunk,
no engine code in the static closure), parity exit 0 with 4 declared entries used (f7a__7__14 ED-16, f7b__7__10 ED-7,
f7b__7__6 ED-7, f7d__7__8 ED-7), memory and chrome below, `editor_switch: ok`. Fix round 2 (`--mode all`, count build,
which now includes `keys`): phase 0 the same (0 `.wasm` requests, 0 engine chunk requests, 0 leaks; the entry's
static closure is 11 outputs, 2,207 KB: `paste-html.ts`, now imported by the engine chunk too, became a shared chunk),
parity exit 0 with the same 4 entries used (f7a__7__14 is now its copied CRLF only), memory, keys and chrome below,
`editor_switch: ok` (76 s for the whole mode).

**Keys** (`--mode keys`, fix round 2): AltGr+2 (CDP modifiers 3, `@`), AltGr+E (`€`) and Option+2 (modifiers 1, `™`)
on the same document: TS focus offsets 1, 2, 3 and text `@€™`; Rust the same (fix round 1: offset 0, nothing typed).
The browser's default of ten synthetic keydowns, TS / Rust: F5, F11, Esc, Ctrl+P, Alt+F default / default; Shift+Tab,
Alt+ArrowLeft, F9, Ctrl+Enter, Ctrl+Alt+V prevented / prevented.

**Chrome** (`--mode chrome`, compact and ribbon): the routed buttons change the session state; a non-routed command
(`link`, `pagebreak`) is disabled; with the focus in the document Ctrl+S reaches the bar's save (one
`uw-save-request`), Ctrl+] grows the font (22 → 24 half-points), Ctrl+K runs nothing (no dialog); the ribbon's Cut, Copy
and Paste buttons are disabled (the compact bar shows none); 0 page errors. Fix round 2: the same results (Ctrl+S and
Ctrl+K are no longer prevented by the shell: the bar's listener prevents them, as in the TS).

**Memory** (`--mode memory`, count build, 100 cycles of create `engine="rust"` → load → 20 typed characters →
`saveDocx` → remove, 5 documents): after every remove sessions 0, shell engines 0, handles 0 (1 while open), page
environments 1; live bytes at each cycle's end one value after warm-up (16,986,450), `memory.buffer` one value
(30,736,384 B). Fix round 2: the same values, and `engineStats().editors` is 1 while open and 0 after every remove
(ED-25). Native (`tests/shell_native.rs`): M1 every shell export returns the live bytes exactly (42,961,397 B
allocated, peak 4,325,375 B, 0 B left); M4 10,000 engines (0 B left); wasm M4 (`tests/shell_api.test.mjs`) 10,000 ×
(newShellEngine + relayout + free) in 576 ms and 10,000 dropped through the FinalizationRegistry, live bytes back to
12,782,879 exactly.

**Relayout = full layout** (`shell_native` A): 30 seeded hot-path steps on a 120-paragraph document, every page message
(layout id zeroed) and the page heads byte-equal to `uo_session_layout` after each step; mid-document typing 20/20
incremental (pages 2–4 relaid), Enter falls back to a full relayout with reason `document`.

**Keystroke latency, 50 pages** (`--mode latency`, `gen_docs.mjs` seed 14, 310 paragraphs, caret in paragraph 155;
synchronous duration of one `input` / `keydown` event; Playwright's fixed clock rounds `performance.now()` to ms):

| | TS (fast path) | Rust | fix round 1: TS | fix round 1: Rust | fix round 2: TS | fix round 2: Rust |
|---|---|---|---|---|---|---|
| type, median / p95 | 7 / 11 ms | 27 / 32 ms | 7 / 8 ms | 26 / 30 ms | 7 / 8 ms | 26 / 29 ms |
| Enter | 111 / 130 ms | 139 / 169 ms | 113 / 131 ms | 136 / 142 ms | 111 / 133 ms | 136 / 148 ms |
| Backspace (merging paragraphs) | 109 / 142 ms | 137 / 153 ms | 110 / 133 ms | 135 / 142 ms | 109 / 122 ms | 136 / 142 ms |

Rust per typed key (median / p95; fix round 1 in brackets: 0 / 1, 13 / 14, 0 / 0, 12 / 16, 200/200; fix round 2: 0 / 1,
13 / 14, 0 / 0, 12 / 16, 200/200): session exec 0 / 1 ms; relayout (incremental + `Layout` clone + `layout_handle`)
14 / 15 ms; adopt (handle + page heads) 0 / 0 ms; install + paint (page cache, the relaid page decoded and painted,
caret EditLines, proxy) 13 / 17 ms; incremental 200/200. Native, debug build (`shell_native` E): a keystroke's
relayout + clone + handle 115 ms median, 15.9 MB allocated, 7.6 MB peak; a full `uo_session_layout` 2,230 ms. The
TS's own full relayout runs 1.5 s after the last key and is not in its column.

**Sizes**: the session build (`--features session`) 9,538,764 B raw / 3,126,154 B gzip -9 (the wave's planning base
9,329,985 / 3,063,663, which predates both frontiers of this wave); the viewer build (`--features write`) 4,122,173 /
1,466,506. The editor's engine chunk (rust-engine + the viewer's RustSession + js/viewer, session, paint, codecs): 382 KB
minified; the entry's static closure is unchanged (10 outputs, 2,207 KB, no engine code).

**Regressions**: the editor's vitest suite 30 files / 313 tests (baseline 29 / 310 + `engine-switch.test.ts`); the
viewer's 25 passed + 1 skipped (26/26 with `UO_WASM_PATH`); `write_api.test.mjs` 8/8; `viewer_switch --mode parity`
(stride 6) 0 differences with the session build AND with a `--features write` build (13 documents, 108 pages, pixels at
tolerance 0, PDF, text layer, find, links); editor and viewer tsc (path-mapped configs) exit 0, no output; the core tsc
line with `editor.check.ts` 0 errors; `check-changed` clean (clippy native + wasm32, float casts, grep bans). Fix
round 2: the editor's vitest 30 files / 313 tests (`engine-switch.test.ts` now also checks the four neutral queries and
the routed font embedding and `currentStyles`; it prints 511 public methods: routed 68, shell 26, disabled mutators
292, disabled queries 125, private routed 18, bar commands routed 57); editor and viewer tsc exit 0, no output; the
core tsc line plus `editor.check.ts` and `write-session.d.ts` 0 errors (the new `encodePaste` line included);
`shell_api.test.mjs` 2/2 on the count build (`UO_ES_M4=2000`); `check-changed` clean (no Rust file changed this round).

### Files touched outside the frontier's own files (each declared)

- (a) `core/crates/uo-docx-layout/src/output.rs`: `Clone` added to the derives of `Layout` and `LayoutPage` (purely
  additive; every field was already `Clone`).
- (b) `core/crates/uo-wasm/js/viewer.ts`: four additive `ViewerModule` members (`exports`, `binding`, `adopt`, `kill`);
  `write/packages/viewer/src/write-engine.d.ts`: those four, plus `inner` on `ViewerEnv`/`DecodedLayout` (opaque
  `EngineEnv`/`EngineLayout`).
- (c) `write/packages/viewer/src/rust-engine.ts`: `EngineDocument` (the `doc` field's type; `ViewerDocument` satisfies
  it) and `adoptDocument()`; behaviour-neutral (viewer parity 0 differences on both builds).
- (d) not needed: `current_props` was rebuilt from the public `props_at`.
- (e) new tests `core/crates/uo-wasm/tests/shell_native.rs`, `shell_api.test.mjs`; (f) new `core/crates/uo-wasm/js/editor.check.ts`.
- (g) fix round 2, this frontier's own JS files, additive: `js/session.mjs` re-exports `encodePaste` from
  core/crates/uo-docx/js/paste-tree.mjs (unchanged; a relative import, so the existing `@universal-office/write-session`
  alias of the bundlers carries it and no new alias is needed), declared in `js/session.d.ts` and in the editor's
  `write-session.d.ts` (plus `setFontEmbedding` and `charStyle`, already in the real binding), checked by a new line of
  `editor.check.ts`.
- Not edited (requests below): `CHANGELOG.md`, `tools/build-pkg.mjs`, `.github/workflows/rust-core.yml`.

### Wanted (for the orchestrator)

- `tools/build-pkg.mjs`: ship `chunks/uo-write-editor.wasm` (the `--features session` build) with the write package when
  the `rust-engine` chunk of the editor is present, with the licence notices (as `shipEngineWasm`), and the alias
  `@universal-office/write-session` → `core/crates/uo-wasm/js/session.mjs`; decide D5 (one module for editor and viewer).
- `CHANGELOG.md`: `engine="rust"` on `<universal-write>` (preview), `UniversalWriteElement.configureEngine()`,
  `engineStats()`; the declared differences ED-6 (event timing) and ED-7/ED-8.
- **BUG-1 (fix round 1, owner: frontier W, `src/session/handle.rs`)**: `uo_session_selection_rects` passes
  `s.session.range()` as is to `EditLines::selection_rects`, which walks from `start` to `end`: a backward selection
  (focus before anchor: Shift+Left, a drag or Shift+click upwards) answers NO rectangle, so the shell paints no
  highlight (measured: f7d__7__9 paragraph 6 offsets 10→106 answers 2 rectangles, 106→10 answers 0). The TS paints
  `normalizeRange(range)`. The fix is one line, `let r = uo_docx::session::text::normalize_range(s.session.range());`
  (not made here: not this frontier's file). Declared as BUG-1 in the harness (f7d__7__9, end screen); remove the entry
  when fixed.
- CI (`rust-core.yml`): `cargo test -p uo-wasm --features session,count --test shell_native`; `node --expose-gc --test
  crates/uo-wasm/tests/shell_api.test.mjs` on the count build; `editor.check.ts` and
  `write/packages/editor/src/write-session.d.ts` on the core tsc line; `editor_switch.mjs --mode phase0`, `--mode
  parity` (stride 6; it FAILS on any difference outside its `DECLARED` table since fix round 1), `--mode memory` (count
  build), `--mode chrome`; `editor_switch_tsc.mjs`; the editor's vitest
  through `editor_switch_vitest.mjs`.

### For the milestone-2 gate

1. **Caret/hit x past a run boundary on a mixed-size line** (the first divergence of most fixtures above): the caret x
   of the session (uo-wasm `session/caret.rs`, which calls `EditLines::caret_x` → `caret_x_in` and `hit_line_x` of
   uo-docx-layout `src/hit.rs`, the port of the TS `caretXIn`/`hitLineX` of engine-doc.ts) differs from the TS `caretX`
   (f7a__7__3 paragraph 0 offset 186: TS 3181.7 tw — its `chars[o]` — Rust 3892.7 tw; the line holds runs of two
   sizes, the offset is one past the boundary at 185), so a click at the TS position hits another offset. Pixels of the
   same layout are equal: the mapping, not the layout. The place to look is the `chars` table `EditLines` builds per
   line in uo-docx-layout `hit.rs` (the per-character x/offset arrays), not this frontier's crate.
2. **End on a wrapped line** (f2-3-2, seed 1, step 2): TS lands at offset 88 (the end of the line's range, drawn at the
   start of the next line), the session's `line_end_offset` at 84 (before the trailing white space): `lineEndOffset`
   vs `move_caret` in uo-wasm `session/caret.rs`.
3. Fold the shell engine into `UoWriteSession` (`uo_session_relayout(h, prev)` with a borrowed or incremental layout
   handle): removes the `Layout` clone (ED-2), the duplicated option code, and most of the 14 ms per key.
4. The caret's `EditLines` are rebuilt for every new layout generation (part of the 13 ms install + paint): an
   incremental `EditLines` over the relaid pages.
5. Move `selection_state` into uo-docx (`Session::selection_state()` and a `QueryOp`), with `getTableState` (WS gate
   item 5), `withLatent` and the built-in `STYLES` (ED-14).
6. A caret query by position (ED-5) and the hit test in context scopes (cells, headers/footers, notes, text boxes:
   ED-7), `tableNavigate` (WS-D7).
7. The overlays of ED-8, the views, the spellcheck, then the rich paste and the two-phase jobs (ED-16), one family per
   later wave; the 425 disabled public methods are the remaining surface (count printed by `engine-switch.test.ts`).
8. D5 packaging (one session module per page, viewers included) and the build-pkg change above.
9. The latency numbers are from Playwright's fixed clock (ms resolution): re-measure without it before the gate.
10. The end-of-run page differences of f2-3-1 / f7c__7__1 / f7c__7__11 (same saved bytes, an automatic hyphen in the
    TS only): find whether the TS editor's page-global hyphenator or the session's pattern registration is right
    (the viewer parity, where both sides lay out the same bytes without edits, is 0 differences). Not reproduced by the
    fix-round-1 scripts (f7c__7__11 is equal in every facet; f2-3-1 and f7c__7__1 diverge earlier at a clipboard
    step); kept open.
11. (fix round 1) BUG-1 above: the backward selection's rectangles (one line in `handle.rs`, frontier W).
12. (fix round 1; fix round 2) The keyboard paste is routed through `clipboard.pasteData` since fix round 2 (ED-16):
    the `uw-pasted` differences of the first rounds are gone (they needed no new work: the session already had the port).
    Left: the clipboard items of two-phase jobs (an image-only item, `copy()`/`cut()`/`paste()` without an event, which
    need the `ReadClipboard`/decode/encode/render jobs answered by the shell), Paste Special, the ring pane, the
    `pasteDefaults` field (a session setter), `contentWidthTwips` of the caret's section, and the CRLF of the copied
    `text/plain` (the shell writes the session's `copied().text`; the TS joins paragraphs with CRLF).
13. (fix round 1) The first-round f2-3-3 click divergence (round-1 script, step 7: the TS hit paragraph 23 offset 152 at
    caret y 333.6 px, the Rust paragraph 22 offset 62 at y 322.9: another LINE, not a mixed-size line nor a text box)
    was left unclassified in the first round. With the fix-round-1 scripts (seeded by the fixture's name) f2-3-3's clicks
    are equal and it diverges first at a clipboard step; the round-1 case is not reproducible by the current harness and
    stays open (a candidate for gate item 1's line choice in `EditLines::hit`).
14. (fix round 2) Session queries for what ED-24 answers neutral: the styles list (`listStyles(filter)` with
    `withLatent` and the built-in `STYLES`, then `paragraphStyles`/`listStylesInDocument`), the tracking state
    (`isTrackingChanges`: `SessionOptions.tracking`, read at open), and the document's embedded font families
    (`knownFontFamilies`). Each is a `QueryOp` in uo-wasm `query.rs` (frontier W's files) or a shell export.
15. (fix round 2) The document's default paragraph style id (`docStyles.defaultParagraph()`) in `selectionState()` (ED-23).
16. (fix round 2) The TS-owned keys with Alt that the TS runs (Alt+Arrow, Alt+Enter, Alt+Backspace/Delete, Ctrl+Alt+letter
    of the Ctrl map): the shell prevents them and runs nothing (ED-21).
17. (merge) The context menu on the pages (ED-28): move the selection, then open the bars' menu with its disabled items
    grayed (D7).
18. (merge) Packaging before 0.10: ship `chunks/uo-write-editor.wasm` with the write package (D5). The `write-session`
    alias in `build-pkg.mjs` is in since the merge (a prerequisite: `build:pkg write` did not resolve without it).

### Process metrics

- Wall time: one implementer session (Opus), 09:40–10:55 local (≈ 1 h 15 min, every run included) after the planner's
  measurements; a full parity run (74 fixtures × 12 steps × 2 engines) takes 5 min 40 s, stride 6 45 s.
- Rounds: none with a reviewer yet. Rework inside the session: the shell engine's provider per call (all relayouts
  fell back to `Provider`; fixed by keeping it, ED-3); the first `tsc` of the viewer through a path-mapped config
  EMITTED `.js`/`.d.ts`/`.js.map` next to 564 sources of shared/ and write/ (and a `viewer/dist/`) because the viewer's
  own config emits — removed file by file (only files created by that run, checked by mtime, no junctions), the helper
  now forces `noEmit` and `rootDir`; the harness's click targets drawn on positions without a line moved the TS
  selection without the Rust run seeing it (fixed: redraw, restore); screen captures raced the scroll virtualization
  and the caret blink (fixed: settle + stop the timer); the Rust `rebuildCanvases` kept far buffers (fixed: mirror the
  TS release); disabled `async` methods answered `undefined` where the ribbon chains `.catch` (fixed: a resolved
  promise).
- **Fix round 1** (reviewer round 1: 2 major, 6 minor findings; all fixed or declared): ≈ 2 h 30 min wall time, 6 full
  parity runs (≈ 9 min each with the new steps) plus focused runs. Fixed: the parity mode fails on undeclared
  differences (`DECLARED` table, per fixture / facet / step); the shell forwards the keys it does not run to the element
  (ED-21: Ctrl+S, Ctrl+Shift+S, Ctrl+O, Ctrl+], Ctrl+[, Ctrl+Shift+N work with the focus in the document; `--mode
  chrome` checks it); Cut/Copy/Paste/Paste-as-text disabled in the bars, Ctrl+Shift+V reaches the native paste
  (ED-16); `currentPage`, `stats`, `pageWidthZoom` routed (ED-20); `WriteSession.layout` uses `layoutArgs`; the README
  states the packaging state and the module order; f2-3-3 and gate item 1 named (gate items 1 and 13). The new input
  steps found three shell defects, fixed (double/triple click on the textarea, Ctrl+Backspace at the story's start,
  PageUp/Down scroll) and one session defect, not this frontier's file (BUG-1, backward selection rectangles). Rework:
  the first clipboard steps pasted with End in between (mixing in gate item 2) and read the facets before the TS's
  asynchronous paste landed (flaky selections between runs); fixed by pasting in place, waiting 300 ms, and pasting a
  fixed plain text so that both engines take the plain-text path.
- **Fix round 2** (reviewer round 2: 3 major, 4 minor findings; all fixed or declared): ≈ 1 h 25 min wall time, two
  full parity runs (≈ 5 min 30 s each), one stride-6 `--mode all`, two focused runs. Fixed: the key forwarding prevents
  only what the TS prevents (ED-21: AltGr/Option characters type, F5/F11 keep their default; `--mode keys`); the keyboard
  paste goes through `clipboard.pasteData` with the HTML tree and `withRich` (ED-16), the `uw-pasted` detail and the
  Paste Options (`pasteOptions`, `repaste`, `pasteButtonRect`) routed; `getFontEmbedding`/`setFontEmbedding` routed;
  `currentStyles` routed (ED-23) and four document queries answer neutral (ED-24); `exportOdt`/`exportXps`/
  `exportHtml`/`exportMarkdown` throw (ED-18); the `editors` counter follows connect/disconnect, `dispose()` removed
  (ED-25); the record's garbled ED-16 count and wrong ED-21 premise corrected. Found by the rerouted paste: the shell's
  Tab typed a tab at the start of a list item where the TS changes the list level (ED-26, fixed; f7a__7__8 equal now),
  and two first divergences that the clipboard entries hid (f2-3-3 gate item 1, f7b__7__5 ED-7). Rework: the first
  parity run of the round was started before the `uw-pasted` detail fix and was stopped and rerun.

## Milestone 2: the editor switch, wave 5

Three frontiers in parallel on the same editor package. Each routes its families through its own files
(`write/packages/editor/src/engine-routes-<front>.ts` for the names, `rust-engine-<front>.ts` for the handlers; both merged
by `engine-routes.ts` and `installRoutes`, where a routed name without a handler is a warned no-op, never the TS code) and
writes only its own subsection below. ED numbers continue from ED-28: S uses ED-100.., O ED-200.., R ED-300...

### Wave 5 — S: stories (tables, headers/footers, notes, the caret in a text box)

Frontier S (implementer session, 2026-11-11, on the planner's measured plan). Under `engine="rust"` the caret now lives in
the other stories of the document — table cells, header/footer parts, footnotes/endnotes and the text of a text box — with
the TS editor's input rules: the click, double click, arrows, Tab and Esc of each context, the table commands and the
contextual tabs (Table Design, Table Layout, Header & Footer). The session already had every command (D2a contexts,
D2b-F3 tables, F4 headers/footers, F5 notes); what was missing were the TS decisions that read the LAYOUT (`eng.lines`,
`page.cells`, `page.objects`): they are ported as shell exports over the session's last layout (decision D-S1: in Rust,
not in TS over the decoded pages).

**What is routed** (87 public methods, 116 bar command ids — since fix round 2 also the 27 template ids of
`chrome-table.ts`, `cell:align:{tl…br}` (9), `table:look:{header,total,bandedRows,first,last,bandedCols}` (6),
`table:border:{bottom…tr2bl}` (12), and the 7 ids of ED-109; handlers in `write/packages/editor/src/rust-engine-stories.ts`,
input and hooks in `rust-stories.ts` / `rust-stories-input.ts`):

| Element method / input | Session |
|---|---|
| `insertTable`, row/column/cell insert and delete, `deleteTable`, `mergeCells`, `splitCell`, `splitTable`, `select{Cell,Row,Column,Table}`, `distributeRows/Columns`, `setRowHeight`, `setColumnWidth`, `autoFitTable`, `setCellAlignment`, `setTextDirection`, `cycleTextDirection`, `toggleRepeatHeaderRows`, `setTableCellMargins`, `sort`, `convertTableToText`, `convertTextToTable`, `setTableStyle`, `setTableStyleOptions`, `setCellShading`, `setTableBorderSides`, `setCellBorders`, `setTableProperties`, `insertFormula`, `resizeTableColumn/Row` | `TableCmd` (the `TableLayoutIn.rowHeights` from `uo_shell_table_state`: the displayed height of every row of the target table; `contentWidthTwips` absent, ED-16) |
| `getTableState`, `getSelectionState().table` (the contextual tabs read it), `sortInfo`, `textToTableGuess`, `suggestFormula`, `previewFormula`, `editingTableCell` | `uo_shell_table_state`, `uo_shell_sort_info`, `uo_shell_text_to_table_guess`, queries `suggestFormula`/`previewFormula`, `uo_shell_story_context` |
| Tab / Shift+Tab in a cell (Tab in the last cell adds a row), the arrows between cells and between the body and its tables (also PageUp/PageDown's 20 line moves), Shift+arrow out of a cell, drag and Shift+click across cells, the keys of a selection of cells (arrows, Shift+arrows, Delete, Backspace, Esc), Ctrl+Shift+Enter, Alt+Shift+Up/Down, Ctrl+Home/End in a cell, Esc in a cell | `uo_shell_move_cell` (`moveCell`), `uo_shell_table_navigate` (`tableNavigate` + `enterRowAt` + `leaveTable`, one `ExecResult` merging the steps' change sets), `uo_shell_drag_cell_sel` (`dragCellSel`), `TableCmd::{TableSelArrow, TableSelDelete, ClearTableSelection, SplitTable, MoveTableRow}`, `ContextCmd::ExitCell` |
| a click on a cell, a note line or the body (`routeEngineClick`) | `uo_shell_locate_at` (`engLineAt` + `locateRef`) → `ContextCmd::{EnterCell, ExitCell, EnterNote, ExitNote}` |
| `enterHeaderFooter`, `exitHeaderFooter`, `editingHeaderFooter`, double click in the top/bottom margin zone (protected: `uw-blocked`), double click in the body to leave, click/double click in the other region (`switchHfFocus`), Esc, `goToHeaderFooter`, `next/previousHeaderFooter`, `showDocumentText`, `documentTextShown` | `uo_shell_hf_page` (`hfPartOfPage`, `hfBand`, `hfTypeOfPage`, `hfBandLabels`, `hfContext`) → `HeaderFooterCmd::CreateHeaderFooterPart` when the page shows no part yet, then `ContextCmd::EnterHeaderFooter`; the dim layer, the bands and their labels painted by the shell |
| `setDifferentFirstPage`, `setDifferentOddEven`, `setHeaderFooterDistance`, `setLinkToPrevious`, `removeHeaderFooter`, `removePageNumbers`, `setPageNumberFormat`, `getHeaderFooterOptions`, `getPageNumberFormat`, `insertHeaderFooterTemplate` | `HeaderFooterCmd` with `HfAt` from `uo_shell_hf_page` of the caret's (or the edited) page; queries `headerFooterOptions`/`pageNumberFormat` (ED-104) |
| `insertFootnote`, `insertEndnote` (Ctrl+Alt+F/D), `openNote`, `goToNoteRef`, `nextNote`, `showNotes`, `setNoteOptions`, `getNoteOptions`, `convertNotes`, `noteCount`, `canInsertNote`, `editingNote`, double click on a reference / on the note's number, click in the notes area, Esc in a note | `NoteCmd`, queries `noteOptions`/`noteCount`/`canInsertNote`/`lastConvertedNotes`, `uo_shell_note_at` (`noteRunAt` + `isNoteMark`), `uo_shell_locate_at` |
| a click (one) or double click in a text box enters it (the caret at the point), outside it leaves, Esc, `exitTextbox`, `editingTextbox` | `uo_shell_textbox_at` (`hitTextbox`) → `ContextCmd::EnterTextbox { para, run, host, focus, page }` (the head of the chain); the caret, hit test, selection rectangles and line moves in the box through the textbox edit scope (WS-D6, below) |
| shell state through the TS code (ED-105): `getBorderPen`, `setBorderPen`, `getTableTool` (null), `tableGridlinesShown`, `showTableGridlines` (the gridlines painted from `uo_shell_cell_boxes`), `tableStyleListNow`, `renderTableStyleThumb`, `headerFooterTemplates` | the element's prototype method on the element (they read and write element state only; a throw is warned once) |
| `stylesVersion` | the session's `stylesVersion` query (frontier R's; ED-105) |

**Shell exports** (`core/crates/uo-wasm/src/session/shell_stories.rs`; JSON answers, `UoBuf` statuses 0/1/5/11; JS
`bindStories(exports, uo)` in `js/session-stories.mjs`, re-exported by `js/session.mjs`, typed by `js/session-stories.d.ts`):

| Export | Port of (universal-write.ts) | Answer |
|---|---|---|
| `uo_shell_story_context(h)` | `cellEdit`/`noteEdit`/`hfEdit`+`hfActivePage`/`tbEdit`/`tableSel` | `{ ctx: { kind, … }, tableSel, locked }` |
| `uo_shell_locate_at(h, l, page, x, y)` | `engLineAt` 18726 + `locateRef` | `{ kind: body/cell/hf/note/other, line, para / anchor+hops+block / which+slot+part / footnote+id }`, 1 = no line |
| `uo_shell_cell_at(h, l, page, x, y)` | `cellBoxAtPoint` 20949 | `{ anchor, path, r, c, x, y, w, h, span }` |
| `uo_shell_table_state(h, l)` | `getTableState` 21305, `tableTarget`, `tableAvail`, `rowHeights` | the TS `TableState` + `anchor`, `path`, `layoutIn.rowHeights` (1 = null) |
| `uo_shell_table_sel_rects(h, l, page)`, `uo_shell_cell_boxes(l, page)` | `paintTableSel` 18628, `paintGridlines` | f64 quads |
| `uo_shell_table_navigate(h, l, dir, extend)` | `tableNavigate` 21100, `enterRowAt` 21178, `leaveTable` 21195, `enterCellAt` 19363 | `ExecResult` (the TS `true`) or 1 (the default move) |
| `uo_shell_move_cell(h, l, dir)` | `moveCell` 19272 (+ `tabNewRow`) | `ExecResult` or 1 |
| `uo_shell_drag_cell_sel(h, l, page, x, y)` | `dragCellSel` 21013 | `ExecResult` when the selection of cells changed, else 1 |
| `uo_shell_hf_page(h, l, page)` | `hfPartOfPage` 18686, `hfBand` 18708, `hfTypeOfPage` 11222, `hfBandLabels` 11202 | `{ page, section, sections, number, evenOdd, type, width, height, margins, header: { slot, section, part, top, bottom, linked }, footer }` |
| `uo_shell_note_at(h, para, offset)` | `noteRunAt` 18934 + `isNoteMark` | `{ ref: { footnote, id } | null, mark }` |
| `uo_shell_textbox_at(h, l, page, x, y, tol)` | `hitTextbox` 13375, `contextTextboxRuns`, `textboxChain` | `{ para, run, host, focus, page, rect, rot, onBorder, hf }` |
| `uo_shell_sort_info(h, sep)`, `uo_shell_text_to_table_guess(h, sep, cols)` | `sortInfo` 19765 (+ `guessSortType`), `textToTableGuess` 21257 | the TS objects |
| `textbox_chain(session)` (Rust fn, used by `handle.rs`) | `textboxChain` (uo-docx `drawing.rs` 722, crate-private there) | the head's drawing + the linked boxes by `seq`, the header/footer page |

**The textbox edit scope (WS-D6, closed).** `handle.rs` `lines()` builds `EditScope::Textbox { chain, page }` (borrowed
during `EditLines::new`) when the context is a text box: `caretRect`, `hitTest`, `selectionRects` and the line moves now
answer inside a box (native test F: 18 of the 19 boxes of the f8/f7d/f7b fixtures entered with a caret; f7d__7__10 is
S-G3).

**Shell hooks** (`rust-engine.ts`, additive, each line marked `wave 5 (S): hooks`; frontier O had added the registry
`hooks { key, pointer, paint, adopt }` and `frontApi()` first, so S extended it): `pointerLate` (after the fronts' pointer
hooks), `dbl` (top of `onDblClick`), `move` (top of `moveCaret`: `tableNavigate`), `state` (`getSelectionState` before
caching: `table`), `overlay` (over the selection, before the caret: the selection of cells, the header/footer bands),
`hideSelection` (a selection of cells hides the text selection and the caret, as `paintEngine`), `baseKey` + `layers`
(the page base: in header/footer mode the part at alpha 1 and the body dimmed, or hidden with Show Document Text off),
and `frontApi().invalidateBase()`/`hitAt(e)`. Order (ED-107): S's key hook (selection of cells, cell shortcuts, Tab,
Ctrl+Alt+F/D) and its text box pointer hook register first; S's Esc hook registers at the first `adopt`, after O's and
R's key hooks (the TS checks a selected object before the contexts' Esc); `pointerLate` runs after every `pointer` hook.

**Declarations (ED-100..):**
- **ED-100** routing by the session's context: a context change (enter/exit) has no story change, so the shell does not
  relay out; the page base cache key carries the header/footer mode (`baseKey`), and the session's `lines()` cache is
  keyed by the scope.
- **ED-101** the hit routing of the click and of the zones runs in Rust over the session's last layout (`LineIndex`,
  `cell_at`, the lines' `LineStory` parts; D-S1); the line index is rebuilt per call (a click), not cached. The layout's
  page section indexes the model's sections (as `Locator` assumes; no `sectionMap`); the page size of `hf_page` is the
  section's `sectPr` size (the layout handle keeps the page sizes in its render view, not reachable from `session/`).
  Tested on a two-section document (harness H2: double click in section 2's header, Link to Previous off/on, Different
  First Page on section 2, previous/next across the sections, removals and the options in header mode — every step equal).
  `uo_shell_table_state` answers no table in a header/footer or a text box, and in a note only the body's selection of
  cells (the TS `tableTarget` excludes `hfEdit`/`tbEdit` only, and keeps `tableSel` when a note is entered from it: found
  by the chrome mode, fixed).
- **ED-102** a text box inside a table cell or inside another text box is not entered (the session enters the boxes of
  the outer story's top-level paragraphs: `EnterTextbox` takes `(para, run)` of the outer story) — S-G2; Esc in a box
  leaves it without selecting the shape (the TS selects it: frontier O's object selection). The `onBorder` of
  `uo_shell_textbox_at` is the TS `onObjectBorder` band (fix round 2: `tol` on BOTH sides of the outline; it was the outside
  half only): a click there is never entered by S (frontier O's hit test runs first and selects the shape). Measured in
  harness X4 (f7b__7__10): 3 px inside and 3 px outside the left edge select a shape and enter nothing in both engines.
- **ED-103** the answers are JSON (D-S4: also the pointer-frequency ones; packed binary only for the quads); the
  `uw-notemode` detail the session emits (`{ active, footnote, id }`) is mapped by the shell to the TS form
  (`{ active: true, kind, id }`, `{ active: false }`) on the S paths (an undo that leaves a note re-dispatches the session
  form: RustEditor's `apply`); `uw-hfmode` gains `kind` on entering (the session's has `active` only).
- **ED-104** a header/footer option taken in header mode: the session's step (no `reenter`), a relayout, then — when the
  edited page shows another part (or none of the variant yet) — exit + enter (creating the part), and one
  `uw-hfmode { active, kind }` as the TS `hfSectionOp` (D-S5). Equal in the harness (H1: Different First Page creates the
  first-page header, saved parts equal).
- **ED-105** shell-through table UI state (pen, gridlines, the styles gallery: built-in styles only, the default Office
  theme colours through `documentThemeColors` over the empty TS model; document-defined table styles not listed) — D-S8;
  the gridlines are painted into the page base over the background and under the content, as the TS `engineBase`
  (fix round 2: a new `under` hook; they were painted over the content before — now pixel-equal, harness T3 step 37).
  `setCellBorders(kind)` without a spec (the Borders menu, the Borders split button, Home's borders in a cell) sends the
  pen read through `getBorderPen` (fix round 2: it sent no spec, i.e. no border — found by the new `table:border:outside`
  step). Declared difference: the Border Styles menu (`borderStylesMenu`, opened by the routed `table:borderStyles`) sets
  the pen but does NOT arm the Border Painter as the TS does (`setTableTool('painter')` is a disabled no-op: the tools are
  the D7 remainder). A TS body that throws there is warned once
  (`shell:<name>`) and answers `undefined`, never silently. `stylesVersion` is NOT shell-through: it answers the
  session's style-sheet counter (frontier R's `stylesVersion` query; 0 when the query is absent), so the galleries keyed
  on it refresh when R's routed style commands change the styles; its values are not the TS `docStyles.version` (a cache
  key only, compared nowhere).
- **ED-106** the header/footer gallery thumbnails are not painted (`paintHeaderFooterTemplate` answers false: no
  session preview layout); the TS adornments of the edited text box (its handles) and the shape selection after Esc are
  frontier O's (declared in both harnesses).
- **ED-107** the hooks' order (above).
- **ED-108** (a TS defect found) saving with the caret inside a text box, the frozen TS writes a document.xml of 8,180
  bytes for f8__7__5 (most of the body missing; the session saves 82,578): declared in `editor_switch.mjs` (f8__7__5) and
  the stories harness (X2). The frozen TS also throws `Cannot read properties of undefined (reading 'runs')` typing in
  the linked box of f7d__7__1 (S-G4): that fixture is out of the X1 scenarios.
- **ED-109** (fix round 2) bar ids of `chrome-hf.ts`/`chrome-table.ts` whose `run` reaches only frontier R's routed
  methods are routed by S (they were off with nothing declared): `fields:update` (`updateFields`), `dlg:dateTime` and
  `dlg:alignTab` (their dialogs `dateTimeDialog`/`alignTabDialog`: `dateTimeFormats`, `insertDateTime`,
  `insertAlignmentTab`), `docinfo:{author,title,fileName,filePath}` (`insertDocumentInfo`). The behaviour is R's routes;
  harness steps T3 `fields:update`, H2 `docinfo:author`, `dlg:dateTime`, `dlg:alignTab` (header mode) equal. The ids of
  the two files that stay off are exactly the declared remainder (vitest (b3) and `--mode chrome` fail on any other):
  `table:excel`, `table:style:new`, `table:style:modify`, `table:painter`, `table:draw`, `table:eraser`,
  `paste:cells:{overwrite,rows,nest,text}`, `dlg:borders`, `dlg:pageBorders` (frontier O's page borders). The Quick
  Tables submenu of the Table menu (a menu entry, not a registry command) stays visible: its list is the TS
  `quickTables()` over the empty TS model and picking one is a no-op (`insertQuickTable` not routed; D7 remainder).

**The f7b__7__10 parity entry (ED-216, frontier O's) — measured cause (fix round 2).** The reviewer read the step-6
divergence of `editor_switch.mjs` as S's text box hit entering a text-bearing ellipse. Measured (`UO_ES_TRACE`, the
stories harness `--mode tbhit`, scenario X3): NEITHER engine enters a text box at step 6. The click target is the TS caret
point of body position (2, 256), which lies on the plain ellipse at (7205, 3164) tw (no text); the TS `hitTextbox` answers
null and `hitFloat` the shape, and the shell's O hit test selects the same shape: both runs emit one `uw-change` and
`editingTextbox()` is false in both. The selections differ because the parity harness's `clickTarget` computes the target
by `setSelection` in the TS run only: a click that lands on an object does not move the caret, so the TS keeps the
harness-placed (2, 256) and the shell keeps (5, 100) from step 5. Steps 7–11 then differ through keys with a shape
selected (frontier O's). With the same preset selection in both runs (X3: `setSelection(2, 256)`, the click, then a click
inside the text ellipse, typing, Esc) every step facet is equal; the end screen differs by the TS shape handles after Esc
(ED-106). So the ED-216 entry's `why` ("the shell enters its text") does not hold; its blanket ('*' from step 6) covers a
harness artifact plus O's object keys — for frontier O / the coordinator (S does not edit O's entry; gate list below).

**Files touched outside S's own** (each additive): `src/session.rs` (one `mod` line); `src/session/handle.rs` (`lines()`
builds the textbox scope, its cache key = the chain's drawing ids + the header/footer page — drawing ids are unique in the
document, so a body box and a header box at the same `(para, run)`, or one header box entered on two pages, never share
lines; `scope_key`'s `t{para}.{run}` is not used for a box; `outcome` and `exec_result` `pub(crate)`; one import);
`write/packages/editor/src/rust-engine.ts` (the hooks above, 9 dispatch lines + the registry entries + 5 `frontApi`
members: `invalidateBase`, `hitAt`, `defaultClick(e, moveFirst?)` — the tail of `onMouseDown` moved into a private
`clickAt` that both use, with an optional drag step of the front, so S keeps no copy of the default click and no drag
record of its own — and the getters `module`/`gen`, so S reads no RustEditor private through a cast; fix round 2: the
registry entry `under` + one dispatch line in `baseOf`, painting into the page base under the content); `js/session.mjs` (one re-export line);
`js/session.d.ts` (one re-export line); `js/editor.check.ts` (one line); `write/packages/editor/src/engine-switch.test.ts`
(the disabled-mutator example `insertTable` → `setTableTool`, now routed); `tests/browser/editor_switch.mjs` (`DECLARED`:
8 S entries removed, f8__7__5 rewritten, a marked block). New: `write-session-stories.d.ts` (the editor's declaration of
the binding, an augmentation as front O's). No uo-docx or uo-docx-layout source edited; no codec/schema change.

**Numbers, fix round 2** (2026-11-11 evening; wasm builds of `$T/es-stories` rebuilt with the `onBorder` band, which also
hold O's and R's in-progress code; the round-1 numbers below stay as the history):
- What the review found and what changed: (1) the 27 template ids were off under rust with nothing declared → routed;
  vitest (b3) now registers `registerTableCommands` + `registerHfCommands` with a recording Proxy editor and fails on an id
  that is neither routed nor in the declared remainder, on a routed id whose `run` calls an unrouted method, and on an off
  id whose `run` only reaches routed methods (it found the 7 ids of ED-109 too); (b4) scans the WHOLE of `chrome-table.ts`
  and `chrome-hf.ts` (menus, galleries, widgets: `borderStylesMenu`, `tableShadingMenu`, `bordersMenu`, `pageNumberMenu`,
  `templateTiles`, `tableMenu`…) and fails unless every unrouted `ed.X(` is in the declared list (`setTableTool`,
  `quickTables`, `insertQuickTable`, `canEmbedSpreadsheet`, `insertSpreadsheet`, `hasCellsClip`, `pasteCells`); (b2) also
  reads `dateTimeDialog`/`alignTabDialog`. (2) `--mode chrome` now compares EVERY id the chrome registers from those two
  files (the batches caught by the harness registry patch) plus S's routed list, TS against Rust: 97 registered ids, 116
  routed ids × 5 contexts 580 equal, 0 different; the 12 remainder ids supported as the TS and disabled under rust in all 5
  contexts (60); a registered id missing from both lists, or a stale remainder id, fails. (3) New harness steps
  `cell:align:cc`, `table:look:bandedRows`, `table:border:outside` (found the pen bug of ED-105, fixed), `fields:update`,
  `docinfo:author`, `dlg:dateTime`, `dlg:alignTab`; and mid-scenario screen captures (`{ k: 'screen' }`, facet `screen
  pixels (step)`): T1 a selection of cells by drag and a row selected, H1 header mode (part at alpha 1, body dimmed, the
  bands and labels), H2 section 2's header and Show Document Text off (2 pages), T3 gridlines on — 8 page captures, all
  equal (the gridlines one only after the `under` fix). (4) X3/X4 on f7b__7__10 (above and ED-102).
- Chromium `--mode stories` (2 min 20 s): 21 scenarios, 386 steps: selection, events, run/para/table state, contexts,
  dirty 386 equal each; caret 383 + 3 declared (S-G1, T1 steps 18, 19, 49); API answers 380 equal; mid-scenario screens 8
  equal; pages 21; thumbnails 172 equal; end screens 28 + 6 declared (ED-106 ×6: X1 ×3, X2, X3, X4; ED-108 with X2);
  saved part names 21 equal, part bytes 165 + 1 declared (ED-108); page errors 0; `editor_switch_stories: ok`. Screen
  pages one engine did not allocate: 4 (informative, as round 1; the pixels are compared where both painted).
- `--mode memory` (count build `session_count.wasm`): 30 cycles, live bytes one value from cycle 5 (6,682,346 B),
  `memory.buffer` 12,582,912 B; sessions/engines/handles 0 after every remove. The mode now FAILS with a build without the
  counters (`live` = -1 passed vacuously before).
- Native `shell_stories_native` (8 tests, `--features session,count`, 22 s): pass (F: 36 documents, 18 boxes entered, 1
  without: S-G3).
- `editor_switch.mjs --mode phase0` ok; `--mode parity` (`UO_ES_STRIDE=6`) ok, unchanged: 13 documents, every difference
  declared (f7a__7__14 ED-16, f7b__7__10 ED-216 — see the measured cause above), 0 page errors.
- Editor vitest 33 files / 329 tests, viewer 25 + 1 skipped; editor and viewer tsc exit 0; `check-changed` all steps
  passed (clippy native + wasm32 on uo-docx, uo-docx-layout, uo-ffi, uo-wasm; float casts; bans); rustfmt clean on
  `shell_stories.rs`.

**Numbers** (2026-11-11, fix round 1; wasm builds of `$T/es-stories`, which also hold O's and R's in-progress code):
- Native (`tests/shell_stories_native.rs`, 8 tests, 25 s): A–H pass; M1 1,000 rounds of the 11 read exports: 17,174,600 B
  allocated, peak 3,357 B above the start, 0 B left; F: 36 documents scanned, 18 text boxes entered with a caret, 1
  without (S-G3).
- Chromium (`tests/browser/editor_switch_stories.mjs --mode stories`, 2 min 8 s): 19 scenarios, 360 steps. T1 a 3×3 table
  from a new document (51 steps); T2 the 7 table fixtures; H1 headers/footers; N1/N2 notes; X1/X2 text boxes; and, new in
  this round, BAR COMMANDS through the chrome's registry (a `{ k: 'cmd', id, dlg }` step: the registry is caught by a
  harness-only patch of `CommandRegistry.prototype.add` in the bundle entry — the element's code is unchanged — and run
  with `reg.run(id)`; the dialog it opens is snapshotted (title, every field, the actions), filled by field name and
  accepted; the step's answer is supported/enabled/checked + that snapshot, so the dialogs' TS reads — `getTableState`,
  `sortInfo`, `textToTableGuess`, `suggestFormula`, `getNoteOptions`, `getPageNumberFormat`, `getHeaderFooterOptions` —
  are compared too):
  - **T3** (49 steps, ribbon): Insert Table dialog, Sort dialog, select row, Merge, Split Cells dialog, rows/columns
    above/below/left/right, Insert Cells and Delete Cells dialogs, Table Properties dialog (alignment), Table Options
    dialog (margins + spacing), distribute rows/columns, the three AutoFits, Repeat Header Rows, Text Direction ×3, Clear
    style, gridlines ×2, Formula dialog (formula + number format), select cell/column/table, Convert to Text dialog
    (commas), Convert Text to Table dialog, Split Table, Delete Table, the Insert Table dialog cancelled.
  - **T4** (48 steps, element API): the mutators no command reached with these arguments — `setTableStyle` (Grid Table 4
    Accent 1), `setTableStyleOptions`, `setTableBorderSides` (table and cell), `setCellBorders` (outside, tl2br, none),
    `setTableProperties` (table align, exact row height, header row, cell vAlign; pct table width, column width),
    `setTableCellMargins`, `setCellAlignment`, `setTextDirection`, `setRowHeight`, `setColumnWidth`, distribute,
    `insertCells`/`deleteCells`, `sortInfo`, `sort` (number, descending), `suggestFormula`, `previewFormula`,
    `splitTable`, `convertTableToText`, `textToTableGuess`, `convertTextToTable`, column/row insert and delete.
  - **H2** (36 steps, ribbon; a TWO-SECTION document built by `prep` in a TS run and loaded from the same bytes in both
    engines): double click in section 2's header, `hf:link` off, typing, `hf:prev`/`hf:next` across the sections,
    `hf:firstPage` on/off on section 2, `hf:link` on, `hf:gotoFooter`, the Page Number Format dialog (upper Roman),
    `hf:removePageNumbers`, `setLinkToPrevious(false, 'footer')`, `removeHeaderFooter`, `removePageNumbers`,
    `hf:removeHeader`, the Header & Footer Options dialog (odd/even), `hf:showText` ×2, previous/next,
    `setHeaderFooterDistance`, `hf:close`, `header`.
  - **N3** (26 steps, ribbon): `footnote`, `note:goRef`, `endnote`, `note:next/prev/nextEnd/prevEnd`, `note:show`, the
    Footnote and Endnote dialog (format + start, Apply), the Convert Notes dialog, `openNote`, `note:toFootnote`,
    `goToNoteRef` of a converted id, `showNotes`.

  Result: selection 360 equal, caret 357 + 3 declared (S-G1, now by exact steps 18, 19, 47 — a declaration may list
  `steps`; `from` still means every later step), events/run/para/table/contexts/dirty/API answers 360 equal each, pages
  19, thumbnails 170 equal, screens 28 + 4 declared (ED-106/108), saved part names 19 equal, part bytes 153 + 1 declared
  (ED-108), 0 page errors, `editor_switch_stories: ok`. Found and fixed by the new steps: `goToNoteRef` of a missing id
  answered true (the TS false): the shell now checks the session's `noteRefs` first and leaves the note as the TS does.
- `--mode chrome`: TS against Rust, both with the ribbon, in five contexts (body, cell, row selected, header, footnote
  entered from the selected row): supported/enabled/checked of the 82 routed command ids 410 equal; the visible tabs equal
  (cell: … Table Design | Table Layout; header: … Header & Footer); the ribbon buttons `row:above`, `cells:merge`,
  `table:headerRows`, `table:deleteMenu`, `hf:close`, `hf:link`, `hf:firstPage` present and equal (a missing button
  fails); `table:draw`/`table:eraser` present and disabled (declared remainder). Found and fixed: in a note entered from
  a selection of cells the TS still has a table target (ED-101).
- `--mode memory` (count build, 30 cycles of create → new document → 2×2 table, cell typing, Tab, header, Esc, footnote,
  Esc → save → remove): sessions/shell engines/handles 0 after every remove, live bytes one value from cycle 5
  (6,682,346 B), `memory.buffer` one value (12,582,912 B).
- `editor_switch.mjs`: `--mode phase0` ok (0 `.wasm`, 0 engine chunk requests, 0 leaks; engine chunk 504 KB);
  `--mode parity` with `UO_ES_STRIDE=6` ok (13 documents; every difference declared: f7a__7__14 ED-16, f7b__7__10
  ED-216; 0 page errors).
- Editor vitest 33 files / 325 tests (`engine-stories.test.ts` now also scans the dialogs the routed commands open:
  `chrome-table-dialogs.ts` whole, `notesDialog`/`convertNotesDialog`/`showNotesDialog`,
  `pageNumberFormatDialog`/`hfOptionsDialog` — every `ed.X(` routed or shell); viewer 25 + 1 skipped; editor and viewer
  tsc exit 0 (`editor_switch_tsc.mjs`); `check-changed` all steps passed (clippy native + wasm32, float casts, bans);
  rustfmt clean on the touched `.rs`.
- The strict core tsc line exits 0 only with BOTH editor augmentations (`editor.check.ts` reads
  `Declared.StoriesBinding` and frontier O's `Declared.ObjectsBinding`): the m2-verify-c.sh `tsc_strict` line plus
  `crates/uo-wasm/js/session-stories.d.ts ../write/packages/editor/src/write-session-stories.d.ts
  ../write/packages/editor/src/write-session-objects.d.ts` (with the stories files only: TS2694, no `ObjectsBinding`);
  `core/target/m2-verify-c.sh` updated so.
- Size: session build 9,686,607 B raw / 3,178,967 B gzip -9 (wave 4: 9,538,764 / 3,126,154; all three frontiers).
- Observed in both engines (the frozen TS chrome in headless Chromium, not investigated): after the Insert Table and
  Split Cells dialogs (number fields) typed keys reach the proxy as keydown/keypress but produce no beforeinput/input, in
  the TS as in the shell; the harness types there through the proxy's `input` event (an `input` step). Not a parity
  difference; listed for the gate.

**For the milestone-2 gate**
- **S-G1** (core: uo-docx session / uo-docx-layout derive) after `TableCmd::InsertTable` on the empty paragraph of a blank
  document (body = table, empty paragraph), the live session's layout has no line for the paragraph after the table
  (`uo_layout_block_line(l, 1)` answers none); the same document saved and reopened has it. No caret there (T1 steps 18,
  19, 49 since fix round 2 — 47 before the two screen steps; regression check in `shell_stories_native` C).
- **S-G2** the session cannot enter a text box inside a table cell (or inside another box): `EnterTextbox` is allowed
  from Body/HeaderFooter only and addresses a top-level paragraph run (the TS enters it from the body).
- **S-G3** f7d__7__10: entering the linked box (head para 1 run 0, focus 6) answers no caret at the start (`EditLines`
  textbox scope finds no line of the head's first paragraph).
- **S-G4** frozen-TS defects found (reference, not fixed): save inside a text box (ED-108), the `runs` TypeError typing in
  f7d__7__1's linked box.
- Move the stories exports into uo-docx (`Session::table_state`, `hf_page`…) or `QueryOp`s with frontier R's schema
  ownership (D-S3, D-S4); the `uw-notemode`/`uw-hfmode` details in the session's events (ED-103).
- Not routed (disabled, declared remainder D7): Draw Table/Eraser/Border Painter tools, edge-drag resize and the table
  adorners (move handle, + buttons), Quick Tables (the submenu stays visible, its items are no-ops: ED-109), the Excel
  spreadsheet, `pasteCells`/`hasCellsClip`, New/Modify Table Style, document-defined table styles in the gallery,
  header/footer gallery thumbnails, `dlg:borders` (needs `setParagraphBorders`), `dlg:pageBorders` (frontier O's page
  borders), the Border Styles menu arming the painter (ED-105), touch handles, the Ctrl+click multi-selection, Esc in a
  text box selecting the shape (frontier O). (`insertDateTime`/`insertDocumentInfo`/`insertAlignmentTab`/`updateFields`
  left this list in fix round 2: R routes them, S routes their bar ids — ED-109.)
- **For frontier O / the coordinator**: the `editor_switch.mjs` DECLARED entry f7b__7__10 (ED-216, `facets: '*'`,
  `end: '*'` from step 6) names "the shell enters its text" as the cause; measured, no text box is entered there (the
  f7b__7__10 paragraph above): the step-6 divergence is the parity harness's TS-only `setSelection` in `clickTarget` when
  the click lands on an object, then O's keys with a shape selected. A narrower entry (or a `clickTarget` that restores
  the TS selection / presets the same selection in the Rust run) is O's/the coordinator's call; S did not edit it.
- Harness page helpers copy-adapted in `editor_switch_stories.mjs` (as frontier R did; declared against the plan's
  D-S7): move them into `editor_switch_lib.mjs` once the three frontiers have landed; the registry catch (`entryExtra`)
  and the dialog driver (`cmd`) would move with them.
- The typing after the Insert Table / Split Cells dialogs (Numbers, last item): find whether a real browser loses the
  typed text too (a frozen-TS chrome defect would be S-G4's).
- Not driven in the Chromium harness: the context-menu-only ids (`ctxTable*`, `tableFmt*`) and the gallery/menu openers
  (`table:styles`, `table:shading`, `table:borders`, `table:borderStyles`, `hf:*Menu`) — their state is compared in
  `--mode chrome` and their bodies scanned by the routing test (since fix round 2 the whole of `chrome-table.ts` and
  `chrome-hf.ts`); the menus they open are not driven (their items that are registry commands are: the Borders menu's
  `table:border:*`, the alignment submenus' `cell:align:*`).
- No native test yet of the textbox lines key (two boxes at the same `(para, run)` in two stories within one layout
  generation): the fix is structural (key = the chain's drawing ids + the page).

**Process metrics.** Fix round 2 (≈ 19:10–19:50 local): wasm builds 2 (session + count), native runs 1 (+1 that ran 0
tests without `count`), Chromium stories runs 2 full + 5 focused/probe, chrome 2, memory 2, `editor_switch` phase 0 +
parity 1 + 1 traced single-document parity, vitest 4 focused + 1 full editor + 1 viewer, tsc 1, check-changed 1. Rework:
a heredoc dropped a backslash in a harness string (fixed with the Edit tool); `--mode all` with the non-count build ran
the memory soak vacuously (guard added). Fix round 1 (≈ 18:20–19:05 local): wasm builds 3, native runs 1, Chromium stories runs 9 (focused)
and 2 full, chrome 2, memory 1, `editor_switch` phase 0 + parity 1 each. Rework: the first T3 lost every later table
command after Delete Cells (the table left) and typed nothing after the dialogs (the same in both engines): reordered,
`input` steps. First implementer session (≈ 16:55–18:20 local, ≈ 1 h 25 min). Native test runs 6, wasm builds 4,
Chromium stories runs 8 (focused) + 2 full, `editor_switch` runs 5. Rework: `hf_page` read the page size from the layout's
`Page` (moved into the render view: 0 × 0), so every body double click entered the footer — fixed with the section's
`sectPr`; the harness first targeted a text box inside a table (S-G2) and f7d__7__1 (a TS throw); the `uw-notemode` detail
and the Different-First-Page re-entry (ED-103/104) found by the harness and fixed; non-finite coordinates answered the
first line (now none); shell scripts edited through `node -e` lost backquoted text twice (the comments restored).

### Wave 5 — O: objects and the page (pictures, shapes and text boxes as objects, page setup, views, context menu)

Frontier O of wave 5 (2026-11-11, implementer Opus, plan by the wave's planner): pictures, shapes and text boxes as
objects, the page (setup, columns, breaks, line numbers, hyphenation, page color/fill, watermark, cover page, page borders),
the Read view and the context menu of `<universal-write engine="rust">` routed to the session (uo-docx's F7 drawing and
picture commands, F4 sections, the page and text-layout commands, all ported in milestone 2 waves 1–2). The TS editor at
`d0f45bc5` is the behaviour reference; every difference below is declared.

#### Routes (`engine-routes-objects.ts`, handlers in `rust-engine-objects.ts`)

- **105 public methods** (+ the two 4-space-indented ones, ED-212): the page family (31: `setPageSetup`, `setColumns`,
  `insertPageBreak`, `insertColumnBreak`, `insertSectionBreak`, `insertBlankPage`, `setSectionStart`, `setVerticalAlignment`,
  `setLineNumbering`, `setLineNumberingOptions`, `toggleLineNumberSuppression`, `lineNumbersSuppressed`, `getLineNumbering`,
  `getSection`, `setPageBorders`, `getPageBorders`, `setHyphenation`, `setHyphenationOptions`, `getHyphenation`, `setPageColor`,
  `setPageFill`, `pageBackground`, `setWatermark`, `removeWatermark`, `watermark`, `watermarkTexts`, `watermarkGallery`,
  `insertCoverPage`, `removeCoverPage`, `hasCoverPage`, `coverPages`), pictures (35, `insertImage` … `compressImages`, the
  crop mode), shapes and text boxes as objects (33: format, arrange, size, wrap, align, `insertTextbox`, Draw Text Box, the
  link mode, Set as Default), the selection pane (6) and `setView` (print and read).
- **161 bar command ids** (`ROUTES_OBJECTS.commands`), computed — not hand-listed — by running each command's `run` against
  a recording `Proxy` editor (`engine-objects.test.ts` re-checks it on every run): every `ed.*` call is routed or SHELL, every
  `ctl.*` call is one of the dialogs (`pageDialog`, `columnsDialog`, `pageBgDialog`, `textLayoutDialog`, `pictureDialog`,
  `image`). The test whitelists those dialogs wholesale: their OK paths are NOT checked automatically, they were read by
  hand in chrome-dialogs.ts, chrome-page.ts, chrome-picture-dialogs.ts, chrome-picture-pane.ts, chrome-textlayout.ts (the
  page dialog's header/footer part reaches frontier S's routed methods; the hyphenation dialog's Manual button reaches the
  disabled manual-hyphenation methods: a warned no-op, as `hyph:manual`).
  Not routed (stay disabled, D7): `pic:wrap:editPoints`, `pic:transparentColor`, `pic:change:clip`, `shp:editText`,
  `textbox:save`, `textbox:remove`, `hyph:manual`, `view:web`, `view:draft`, `view:outline`.
- **Stay disabled (declared)**: `editSelectedTextbox`, `exitTextbox`, `editingTextbox` (S: the box context), the manual
  hyphenation family, `paintCoverTemplate`, `paintWatermarkTemplate` (the galleries' previews are blank, ED-208),
  `insertSpreadsheet`, `editEmbeddedObject`, `canEmbedSpreadsheet`, `setSelectObjectsMode`/`isSelectObjectsMode` (no lasso,
  ED-211), `start/endEditWrapPoints`/`editingWrapPoints`, `startTransparentColorPick`, `changeImageFromClipboard`,
  `saveSelectionToTextboxGallery`, `removeTextboxGalleryItem`, `goToPage`.
- Modules (own files): `rust-objects-core.ts` (per-editor state in a `WeakMap`, the objects cache per layout and editor
  generation, the section facts, the TS ⇄ wire conversions), `rust-objects-page.ts`, `rust-objects-pictures.ts`,
  `rust-objects-pointer.ts` (hit tests, handles, drags, crop, Draw Text Box, link mode, keys, paint), `rust-objects-jobs.ts`
  (the two-phase jobs), `write-session-objects.d.ts` (the binding's types, merged into the ambient module of
  `write-session.d.ts` — no edit of the shared declaration).

#### Exports (uo-wasm `src/session/shell_objects.rs`; js/session.mjs `bindObjects`)

| Export | JS | Answer (JSON text, UTF-8) |
|---|---|---|
| `uo_shell_objects(h, l, active_page, flags)` | `bindObjects(w, uo).objects(session, layout, activePage)` | `{ scope: 'body'\|'hf'\|'tb', box, fs, sel: [drawing id…], floats: [{ i, kind, id, page, x, y, w, h, rot, behind, editable, content, key, image, frame }], inline: [{ para, run, id, page, x, y, w, h, rot, image }] }` |
| `uo_shell_section_info(h, hf_section)` | `.sectionInfo(session, hfSection)` | `{ index, count, touched: [sectPr…], pageBorderSection, firstSect, suppressed, titlePg: [bool…], background, lastSect, bgShown, evenOdd }` (the model objects as their `JSON.stringify`, uo-docx `JsonWriter`) |

- `objects` is the port of `enumFloats` (engine branch) and `enumInlinePics`: the layout's `PlacedObject.drawing` and
  `ImageOrigin::Media { drawing }` joined with the session's drawing runs (`drawing_objects()`, `inline_pictures()`); `i` is the
  `SelectObject { index }` of the object, `(para, run)` the `SelectInline` arguments, `key` the selection pane key, `frame` the
  `ObjFacts` frame (the session's `ObjFrame` field names), `image` the layout image (`imageBytes`: the natural size, the crop
  overlay, the previews). The image items are read from the layout's render view (`layout_handle` moves the display lists
  out of the geometry: the first cut read the empty geometry lists and found no inline picture, caught by the native test).
  The floats are listed in a text box too (`box`: the box in edit, its dashed outline and handles; the hit tests skip them
  there, as `enumFloats` is empty in `tbEdit`).
- `sectionInfo` gives what `getSection()`, `getLineNumbering()`, `getPageBorders()`, `lineNumbersSuppressed()`,
  `pageBackground()`, `hfTypeOfPage` and the TS `sect()` (the body's FINAL sectPr: the Read mode zoom and `insertImage`'s
  maximum width) read; the shell computes the TS answers with the SAME pure docx-core / text-layout helpers the TS calls
  (`columnWidths`, `columnsPresetOf`, `maxColumns`, `contentWidth`, `COLUMN_SPACE_DEFAULT`, `lineNumberingInfo`).
- Native test `tests/shell_objects_native.rs` (count build): A every float of f7b__7__0/1, f7d__7__3, f8__7__11 (14
  objects) is selected by its index; B a picture inserted through `PictureCmd::Insert` + a synthetic decode answer is one
  inline entry selected through `SelectInline`; C the section facts of f7a__7__1; D (M1) both exports return the live bytes
  exactly (56,767,391 B allocated, peak 2,485,457 B, 0 B left).

#### Hooks and shared edits (each additive, commented `wave 5 (O)` / `front O`)

- `rust-engine.ts`: the import line; `objectsPointerFirst(this, e)` as the first line of `onMouseDown` (the handles, the crop
  handles, Draw Text Box and the link mode before the text hit — the rotation handle sits off the text and `hitAt` would
  answer null there); `objectsAfterCanvases(this)` in `install()` and `refreshView()` (the Read mode spread, as the TS
  `refresh`/`refreshView`); `objectsJobs(this, o, what)` first in `apply()`'s jobs branch (the two-phase jobs reaching the
  shell by another path, the HTML paste with pictures); the image-only paste branch of `onPaste` now calls
  `objectsPasteImage` (`clipboard.pasteData` with `image`, answered by the job runner); the `contextmenu` capture listener is
  removed (ED-210). The other hooks are the fronts' shared `hooks` (S's): `pointer` (unshifted: O before S, the TS order
  10186–10240), `key`, `overlay` (the handles over the selection, as `paintFloatAndCaret`), `hideSelection` (an inline picture
  selected hides the text highlight: the TS `picSel`), `adopt`; plus a `mousemove` listener on the element for the handle
  cursors (the TS's own listeners return on the null TS model).
- `uo-wasm/src/session.rs` one `pub mod shell_objects;` line; `uo-wasm/src/write.rs` one accessor `layout_view(l)` (the
  render view of a layout; additive); `js/session.mjs` + `js/session.d.ts` an appended `bindObjects` block;
  `js/editor.check.ts` one drift line for `bindObjects`.
- uo-docx (ED-201): `Session::drawing_objects()`, `inline_pictures()`, `selected_drawings()`, `edited_box_drawing()`,
  `drawing_scope()` (drawing.rs, the shell queries block; `DrawingObject`), `shell_section_now()` (sections.rs;
  `ShellSection`). Read-only wrappers over the pub(crate) enumerations (`drawing_paths`, `pane_runs`, `sel_path`,
  `focus_path`, `body_section_range`, `body_sect_pr_at`, `page_border_targets`, `line_numbers_suppressed`); no new behaviour.
- `engine-switch.test.ts`: the disabled-mutator probe moved from `insertPageBreak` (routed now) to `startEditWrapPoints`;
  the drift check accepts the two 4-space-indented public methods as routed (ED-212).

#### Declarations (ED-200..ED-222)

- **ED-200** the front reads the `RustEditor` internals through S's `frontApi()` only (session, layout, `apply`, `paint`,
  `caret`, `locked`, `bumpGen`, `module` for `bindObjects`, `gen` for the objects cache; the typed cast of the first cut is
  gone), and frontier S's stories binding read-only (`bindStories` of js/session.mjs: `storyContext` for the edit
  context, `hfPage` for the section of the header/footer page in edit, `textboxAt` for the text box hit, `cellBoxes` for
  the table edge guard of ED-219).
- **ED-201** the uo-docx accessors above (additive, declared).
- **ED-202** a drag (move, resize, rotate) and the arrow nudge commit ONE session command composed from `PictureCmd::SetLayout`
  (`posH`/`posV` in the object's own references, `widthPt`/`heightPt`, `relWidth`/`relHeight: null` when the source had a
  relative size) or `SetRotation`: the session has no port of `commitPictureDrag`/`commitDrawingDrag`. The position
  arithmetic is the TS `posFor`/`refRect`/`applyResize` (ported in `rust-objects-pointer.ts`). The session writes
  `round(offsetPt × 12700)` for the TS `round((xy − o) × 635)` and `round(pct × 1000)` / `round((o + pct/100 × span) × 635)`
  for `round((xy − o)/span × 100000)` / `round(xy × 635)`: the same value in another floating-point order, so it can differ
  by 1 EMU (or 1 thousandth of a percent) when a product falls on a .5 boundary. `engine-objects.test.ts` compares against
  the REAL private `posFor` of the TS element (the prototype method, not a copy) on 4,000 random cases with integer twips
  (0 differences, asserted) and 4,000 with the fractional rectangles and references of a real layout (at most 1, asserted;
  measured 0/4,000). Measured differences: a moved
  shape keeps its `effectExtent` (the TS `applyDrawingXml(…, true)` recomputes it on any drag; `SetLayout` with a position
  only does not), a group's children are not rescaled from the drag start (the TS resets the group size first).
- **ED-202b** a click on an object without a move: the TS takes and discards an undo step (`uw-dirty-change true`,
  `uw-change`, `uw-dirty-change false` on a clean document, `uw-change` alone on a dirty one); the shell dispatches the same
  events without touching the state.
- **ED-203** the drag preview is a dashed outline (rotated with the object) painted by the overlay hook; the TS re-lays out
  the document live on every frame.
- **ED-204** dragging an inline picture (the TS `startInlineDrag`, a move in the text) does nothing.
- **ED-205** the job runner (`runJobs`): `decodeImage` with `createImageBitmap` (+ `OffscreenCanvas.getImageData` when the
  command needs pixels), `encodeImage` with `OffscreenCanvas.convertToBlob(mime, quality)` (the TS `encodePng` and
  `compressImages` API), the jobs answered in order through `WriteSession.complete`; input on the pages and keys is ignored
  while a runner is active; other job kinds answer `failed` unless registered in `jobKinds` (the seam for frontier R's
  `readClipboard`/`renderClip`). The runner holds the session that issued the jobs: a document loaded or created while a
  codec runs ends the run without applying anything (the old ids mean nothing to the new session); a throwing `complete`
  or handler cancels the pending command (`cancelPending`, applied) so the session does not answer `busy` afterwards.
- **ED-206** pixels are decoded only for the commands that read them (adjust, compress, the Tight/Through contour of
  `setWrap`/`setLayout`); `insert`, `change`, `setShapePictureFill` and the image paste ask for the size only. A 12 MP photo
  costs ≈ 96 MB in JS and the same in wasm during an adjust or a compress (the TS cost as well).
- **ED-207** `insertTextbox` passes the caret's page before the insert as the page of the new box (the session needs it to
  enter the box; equal unless the anchor moves the box to another page).
- **ED-208** the watermark and cover page galleries' previews are blank (`paintWatermarkTemplate`/`paintCoverTemplate` lay
  out a TS preview document: disabled); the custom text watermark with an explicit size sends no `measuredBox` (the TS
  measures the text with the live font provider): the session uses the TS no-font fallback box.
- **ED-209** views: Print and Read only. Read mode = the TS shell (`readPagesPerView`, `applyReadSpread`, `nudgeRead`, the
  wheel and keys) + `settings.setReadView`; Web, Draft and Outline stay disabled (decision D2 of the plan: the TS views are
  continuous tiles, Rust's ViewTransform pages; Outline is `LayoutError::Unported`). The Read mode keys: the shell stops the
  arrows and PageUp/PageDown (they also move the caret, as the TS input listener does), so front O nudges for them; Space
  is forwarded to the element and the TS Read-mode listener (not shadowed) nudges once (the first cut nudged twice).
- **ED-210** the context menu on the pages is the bars' menu (chrome.ts) as in the TS: the shell's capture listener of
  wave 4 is gone (ED-28 superseded); the right button selects a picture or a shape under the pointer first (the TS 10247);
  the items of unrouted commands are grayed by the registry (D7).
- **ED-211** Delete with several objects selected runs one command per object (the TS deletes all in one undo step); no
  lasso (`setSelectObjectsMode`) and no Shift/Ctrl+click multi-selection by pointer; the arrow nudge moves the selected object
  only (the TS `moveSelectedFloatsBy` moves all).
- **ED-212** `setImageLockAnchor` and `setImageEffect` are indented with 4 spaces in universal-write.ts, so they are missing
  from engine-public.ts and ran their TS bodies unshadowed under `engine="rust"` (wave 4); routed now. The drift regex and
  engine-public.ts stay as they are (orchestrator).
- **ED-213** `selectedShape().format` has no `fill`, `body`, `story`, `textbox` (the wire `ObjectState.format` is a
  `PictureFormat`); the shell drops the `blip` and `srcRect` the wire's picture base always carries (the TS `ShapeFormat`
  has neither — except a group's: the TS reads `blip: {}` and a zero `srcRect` for Grupo 7 of f7d__7__10, objects step 4,
  which the shell drops as well). Visible: Shape Fill's checked state, the Format Shape pane's fill page and the text box options read nothing.
- **ED-214** inline pictures in table cells are not listed (the TS `enumInlinePics` walks `model.paragraphs` only; the same).
- **ED-215** the crop-mode drag commits `setCrop` (the source rectangle and the size); the TS also moves the position when
  a left/top handle is dragged (two TS changes in one step; the session has one command).
- **ED-216** the text box hit of the object branch is the TS `hitTextbox` (13338, engine branch) through the session's
  port that frontier S exports (`uo_shell_textbox_at`: the boxes with laid-out content of the context's story, ±5 px
  outside the border in the box's turned frame, the top one by front layer then `relativeHeight`), on the pages whose
  canvas holds the point; its border (`onObjectBorder`, ±5 px both sides) selects the float with the same page and placed
  rectangle and starts the move, its inside goes on to S (the entry). The first cut used `hitFloat(e, 'textbox')`
  (no content test, no tolerance, last-painted instead of z order). Where S's export answers none for a box the TS lists
  (S ED-102: a box inside a group, or a linked box whose chain head is inside one), a content-bearing text-box float under
  the point is still treated as a box (never selected as a shape from its inside) — the TS then enters it, the shell places
  the caret (f7d__7__3). The link mode's target is `hitTextbox(e) ?? hitFloat(e, 'textbox')` as in the TS (14947). The shell
  does not exit a text box and select another object in the same click (frontier S's box context first).
- **ED-217** random ids: the TS draws `wp:docPr` ids, `w14:anchorId` and the content control's `w:id` from `Math.random`,
  the session from its xorshift32 seeded 1 at open (the same generator as the harness page). The harness seeds the page's
  `Math.random` again right after the load, so both draw the same values as long as each command takes the same count
  (the pictures mode is then equal byte for byte).
- **ED-218** (measured in fix round 1; the first cut's "cause not isolated" is withdrawn) the TS behaviours the first cut
  attributed to a stale float selection were traced in the TS run (the harness's `UO_EO_TRACE` wraps the TS element's private
  methods; its `__diag` facet showed `floatSel` valid and in `enumFloats()` at every step): (a) the "ignored nw resize"
  (f7b__7__0 step 6), the "no event after a resize" (f7b__7__1 steps 7, 9) and the rotation of f8__7__11 (step 8) are
  mousedowns 4 px from a table cell edge: the TS `tableMouseDown` (10141) runs before the objects and starts the table edge
  drag — ED-219; (b) "Ctrl+ArrowDown moves the caret" is the TS `nav.ArrowDown` (no object selected, or the caret in a
  box: `moveCaret(down)` traced), which the shell's `onKey` (wave 4, rust-engine.ts) drops: it handles Ctrl only with
  Left/Right — gate-O4, not front O. Neither is a TS quirk of the float selection.
- **ED-219** a left mousedown 4 px around a table cell's left, right or bottom edge (the TS `tableEdgeAt` over the layout's
  `page.cells`, under `tableMouseDown`'s conditions: not locked, Print view, outside a header/footer and a text box) is a
  warned no-op in the shell (`tableEdgeDrag`, preventDefault, focus): the TS starts the table edge drag there BEFORE the
  objects, frontier S does not route the drag (its declared remainder), so front O must not run an object handle at that
  point. The TS then moves the column or row (one `uw-change`, the saved table differs); the shell changes nothing. Not
  guarded: the table move/size handles (`tableHandleAt`, which needs the hover state), the edges of header/footer tables
  (the TS keeps body cells only through `locateRef`; the export lists every placed cell).
- **ED-220** the break keys follow the TS order: Ctrl+Enter / Ctrl+Shift+Enter insert a page / column break in the
  body (and through the routed `insertPageBreak`/`insertColumnBreak`, which refuse like the TS where the TS refuses), but
  in a note (also with a text box open inside one) nothing happens (`ctrl && this.noteEdit ? undefined`: the shell forwards
  the owned key), and in a cell Ctrl+Shift+Enter (not locked) is `splitTable`, frontier S's key hook (front O's hook returns
  false; the first cut ran `insertColumnBreak` in the cell).
- **ED-221** in a header/footer context the objects export gets the page in edit (`hfActivePage`: only that page's
  header/footer objects, as `enumFloats`) and the section queries (`getSection`, `getLineNumbering`, `getPageBorders`,
  `lineNumbersSuppressed`, `pageBackground`) the section of that page (`pageSections[hfActivePage]`, through S's `hfPage`);
  the first cut passed −1 (section 0 / the caret's section).
- **ED-222** `documentThemeColors()` is not routed (an unrouted query: its TS body runs on the null TS model and answers the
  default Office palette): the color pickers of every front (the watermark dialog's of front O, chrome-page.ts:66, but also
  Home's font color, borders, effects) show the default theme, not the document's. One cross-front query for the
  orchestrator (gate list).

#### Numbers

Harness `tests/browser/editor_switch_objects.mjs` (own file; it imports `setup` of editor_switch_lib.mjs, the page helpers
are copy-adapted from editor_switch_review.mjs / editor_switch.mjs — declared duplicate): Chromium headless, 1100×850,
`author="Ana"`, `lang="pt"`, the ribbon; TS first, then Rust, the same script of element calls, real keys and mouse
events (pointer targets computed once on the TS element from its `enumFloats`/`enumInlinePics`/`handleCenters` and replayed at
the same client point), bar buttons by id, synthetic paste events; after each step the facets (selection, `uw-*` events,
dirty, page count, `selectedImage`, `selectedShape` with `fill`/`body` split into the always-declared `shapeFillBody` facet,
`selectedObjectCount`, `selectionPaneItems`, `getSection`, `getView`, `getZoom`, `pageBackground`, `watermark`,
`hasCoverPage`, `getLineNumbering`, `lineNumbersSuppressed`, `getHyphenation`, `getPageBorders`, the three modes; canonical
JSON: keys sorted, null/undefined/false dropped — the wire drops false booleans); at the end page count and sizes,
thumbnails (two passes: the Rust painter decodes pictures asynchronously), the screen with the caret hidden (the TS overlays
no Rust front paints yet off), the saved package part by part. Fix round 1 (session build 9,686,613 B, the same front-O
Rust as the first cut; `--mode all` 8 min 34 s, then `objects`, `views`, `keys` again after the intermittent entries below):

| Mode | Documents | Steps | Facets equal | Different (all declared) | Declarations |
|---|---|---|---|---|---|
| page | f7a__7__0/1/5/10, f7c__7__1/2/8, blank | 33 | 3,760 | 44 | gate-O1 (8), gate-O2 (f7a__7__1) |
| objects | f7b__7__0/1/3/8/10, f7d__7__0/3/10, f8__7__5/11 | 35 | 4,537 | 464 (the `shapeFillBody` facet of ED-213 included) | ED-202, ED-213, ED-216 (S ED-102), ED-219, gate-O4, gate-O5 (intermittent) |
| pictures | blank, f2-3-0 | 30 | 853 | 2 | gate-O3 |
| menu | f7b__7__0, f7d__7__3 | 8 | 217 | 2 (`shapeFillBody`) | — the opened menus' items and grayed states are equal |
| views | f7a__7__1, f2-3-0 | 14 | 356 | 6 | ED-209, gate-O3, gate item 10 of wave 4, gate-O5 (intermittent) |
| keys | blank | 12 | 170 | 1 | the note mark's shading (frontier S or R paint) |

The objects declarations are exact since fix round 1: `at: { facet: steps }` per document, measured with
`UO_EO_MEASURE=1 UO_EO_COVERED=2` and the TS traced with `UO_EO_TRACE`; a declared step that no longer differs fails the
run (`declared but equal now`), so no `'*'` remains. What differs, per cause: the moved shape's `effectExtent` (ED-202:
f7b__7__0/3 steps 2–3, f7d__7__10 to the end), the table edge under a handle (ED-219: f7b__7__0 step 6, f8__7__11 step 8;
f7b__7__1, whose steps 7 and 9 are table edges too, is now equal in every step facet), the shell's Ctrl+ArrowDown (gate-O4:
step 11 and the caret after it on five documents), a group's `blip`/`srcRect` (ED-213, one step), the linked box of
f7d__7__3 (ED-216 via S ED-102: every step after the click). Intermittent (gate-O5, `intermittent: true`, not
stale-checked): end pixels of f7b__7__1/3 in about one run of two (the bold runs in table cells laid out narrower by the
Rust engine — the bold face measured before it loaded; every step facet and the saved package equal) and PageUp's caret in
the views mode of f7a__7__1 (one run of two). The `keys` mode (new): Ctrl+Shift+Enter in a cell splits the table (S), Ctrl+Enter
and Ctrl+Shift+Enter in a note do nothing, in the body after the split they break — every step facet and the saved package
equal to the TS. The views mode now presses Space and PageDown (one nudge each, equal). 0 page errors in every mode. The
pictures mode is equal in every step facet and in
the saved package byte for byte (insert through `insertImage` — the bar's file chooser path calls it, not driven here —, wrap
square/tight with the contour job,
position, arrange, rotate, flip, size, crop fill/aspect/shape, border, effect, format, adjust (decode + encode jobs),
transparency, recolor, alt text, reset, reset size, compress (jobs), crop mode, Delete, the image-only paste and the HTML
paste with a data-URI picture through the jobs, Ctrl+Z). The page mode is equal in every step facet (setup, columns, breaks
by keys and bar, line numbers, hyphenation, page color/fill, watermark gallery and custom, cover page, blank page, vertical
alignment, section start, page borders, undo) and in the saved package; its differences are the cover page's box paint
(gate-O1) and one document's column layout (gate-O2). `--mode probe` (first cut: the Rust-saved cover page document
reopened in both engines): 30 facets equal, 0 different.

Memory (`--mode memory`, count build 3 min 23 s, 40 cycles of create → load f7b__7__1 → `insertImage` PNG → wrap square →
size → `setImageAdjust` → `compressImages` → the queries → `saveDocx` → remove): sessions, shell engines and handles 0
after every remove; live bytes flat after warm-up at 9,189,992 B (as the first cut); `memory.buffer` 18,087,936 B over the
last third. Native (count build, first cut): the exports return the live bytes exactly (D above).

Unit (`engine-objects.test.ts`, 5 tests): the route names exist and do not collide; a handler per name, none throws
without a session; the 161 bar commands reach only routed/SHELL methods and the dialogs (their OK paths read by hand); the
drag arithmetic against the TS element's own `posFor` (4,000 integer cases: 0 differences; 4,000 fractional: at most 1
allowed, 0 measured); the wire conversions. Editor vitest 33 files / 329 tests (all fronts' files), `engine-switch.test.ts`
prints 511 public methods: routed 388, shell 26, disabled mutators 65, disabled queries 41; private routed 21; bar commands
routed 434 (the three fronts together). Editor and viewer tsc exit 0, no output. `check-changed` (clippy native + wasm32,
float casts, bans; the crates the three fronts changed): all steps passed (1 min 11 s). The editor's engine chunk 507 KB
minified (the three fronts together).

Wave-4 suites with this build: `editor_switch.mjs --mode phase0` ok (0 `.wasm` and 0 engine chunk requests, static closure
11 outputs / 2,208 KB, no leak); `--mode parity` (stride 6) ok with 2 declared entries used (f7a__7__14 ED-16, f7b__7__10
ED-216, narrowed in fix round 1 from `'*'` to the six facets and three end facets that differ: the TS places the caret
at para 2 offset 256 on the click of step 6, the shell selects the text-bearing ellipse; block commented `front O`).
First cut, not re-run in fix round 1: `--mode memory` (count build, 40 cycles) ok, live bytes one value (16,986,450);
`--mode chrome` ok; `viewer_switch.mjs --mode parity` (stride 6) ok, 0 differences; native `shell_native` 5/5,
`shell_objects_native` 4/4 (no Rust change of front O since).

#### Process metrics (front O)

One implementer session (Opus), ≈ 16:55–19:15 local, with two other frontiers building and running Chromium on the
machine. Session wasm builds: 8 (2 min 20 s – 4 min each; two waited on the others' cargo); one rebuild was forced by
frontier R's schema regeneration (the harness hung on a codec version mismatch until the module was rebuilt). Rework: the
first inline-picture export read the geometry's display lists, which `layout_handle` had moved into the render view (the
native test caught it: 0 items on every page); the Read mode zoom used the caret's section where the TS `sect()` is the
body's final sectPr (the views mode caught it: zoom 1.061 vs 0.75); the random ids aligned by seeding the page's
`Math.random` after the load; three harness artifacts fixed (the menu closer's mousedown inside the element deselected the
TS's shape, the border target 1 twip inside the edge, the thumbnails captured before the pictures decoded); shell edits
through `node -e` lost escaped text twice (repaired with the Edit tool).

Fix round 1 (same day, ≈ 19:20–20:30, after the review): one session wasm build (1 min 58 s, no Rust change of front O:
the build only picks up the other fronts' sources). Findings fixed: the break keys' order (ED-220), Space in Read mode
nudging twice (ED-209), the text box hit (ED-216), the drag family's declarations (ED-218 measured, ED-219), the
arithmetic test against the real TS `posFor` (ED-202), the job runner's session capture and cancel (ED-205), the
header/footer section (ED-221), the `priv` cast, the duplicated `omit`/`locked`, `documentThemeColors` declared (ED-222),
the dialogs' claim. The harness gained `UO_EO_MEASURE`, `UO_EO_COVERED`, `UO_EO_TRACE` (the TS private methods traced into
a non-compared `__diag` facet), the exact `at` declarations (every declared step must still differ, else the run fails)
and the `keys` mode. The tracing found the table edge order in four minutes of runs where the first cut had guessed a
stale selection.

#### For the milestone-2 gate (front O)

1. **A drag commit in the session** (`PictureCmd::CommitDrag { mode, rect, rot, facts }`, the port of
   `commitPictureDrag`/`commitDrawingDrag`/`startCropDrag`/`startInlineDrag`): the XML of a drag equal by construction
   (`effectExtent`, the group's children, the crop's position), the inline-picture drag in the text (ED-202, ED-204, ED-215).
2. **The cover page's text box is not painted after `insertCoverPage`** (gate-O1): the saved package is equal to the TS's
   (byte for byte once the random ids align, ED-217), the screen and the thumbnail of page 0 lack the colored text box of
   the first cover of the gallery. Repro: `editor_switch_objects.mjs --mode page`, any document, step 24. `--mode probe` with
   `UO_EO_PROBE=<the Rust-saved .docx>` (the saved file reopened in both engines) is equal in every facet, the thumbnails
   included: the in-memory model the session builds in `PageCmd::InsertCoverPage` (uo-docx `page` family) lacks what the
   reader builds from the same XML (the box's fill), not the layout.
3. **Three columns with a separator on f7a__7__1** (gate-O2): after `setColumns({ count: 3, sep: true, spaceTwips: 500 })` the
   session's layout has 11 pages, the TS's 10, on equal section facts (`getSection()` equal); the page count stays different
   until the page color step. The layout (uo-docx-layout columns), not the routing.
4. **A theme color without a theme part** (gate-O3, f2-3-0): the character style Strong has `w:color w:val="C00000"
   w:themeColor="accent2"` and the package has no theme: the TS editor paints its default theme's accent2 (orange), the
   session's layout the C00000 (red); the saved packages are equal. Which one Word paints decides the fix (the editor's default
   theme in the session, or the TS).
5. The shape `fill`, text `body`, `story` and `textbox` of `ObjectState.format` (ED-213): a `ShapeFormat` on the wire.
6. **gate-O4, the shell's Ctrl+ArrowUp/Down** (rust-engine.ts `onKey`, wave 4, not a front file): the TS moves the caret
   as with ArrowUp/Down (`nav`, also with Shift), the shell drops the key. Measured on f7b__7__8/10, f7d__7__0/10, f8__7__5
   (objects mode step 11; the later steps then act at another caret). Fix: `else this.moveCaret(arrows[k], e.shiftKey)` for
   Ctrl+Up/Down (orchestrator; a behaviour edit of a shared file, so not done by front O).
7. A text box hit inside groups and for linked boxes whose chain head is inside a group (S ED-102 in `uo_shell_textbox_at`:
   f7d__7__3's Caixa 4, the TS enters it, the shell places the caret) and the exit-and-select click (ED-216), with
   frontier S.
7a. The table edge drag and the table move/size handles (ED-219): routed by frontier S, the guard of front O becomes S's
   hook (the TS order: tables, then objects); the header/footer table edges then follow `locateRef`.
7b. `documentThemeColors()` (ED-222): one query of the document's theme colors (a shell export or a settings query) for
   every front's color pickers.
7c. **gate-O5, intermittent end pixels** (objects mode, f7b__7__1/3, about one run of two under the machine's load): the
   Rust engine lays out the bold runs of table cells narrower (another line break) — the bold face measured before its file
   loaded; the shell's font loading should relayout once the face arrives (wave-4 shell, not front O). Repro:
   `UO_EO_ONLY=f7b__7__3 UO_EO_DUMP=1 … --mode objects` twice; the dumps (`es-o/dump/f7b__7__3-objects-thumb0-*.png`) show
   the cells of column 1. The views mode's PageUp caret (f7a__7__1, one run of two) is listed under the same id, cause not
   isolated.
7d. The parity click of f7b__7__10 step 6 (editor_switch.mjs): the TS places the caret at para 2 offset 256 with one
   `uw-change` where the shell selects the text-bearing ellipse; trace the TS (`hitTextbox`, `hitFloat`, `routeEngineClick`)
   as the objects harness does before deciding.
8. The galleries' previews (ED-208): a preview session (`newBlank` + the gallery command + the layout of page 0) in the shell.
9. Web and Draft views (ED-209): a `web_width_tw` for `uo_shell_relayout` (shell.rs) and the tile-vs-page difference accepted.
10. The drift regex of engine-switch.test.ts matches 2-space methods only; engine-public.ts lacks the two 4-space ones
    (ED-212): regenerate it (orchestrator).
11. `--mode chrome` of editor_switch.mjs picks its "not routed" button from a fixed list that the fronts keep routing
    (pagebreak and image are O's now; R moved it to painter/color): derive it from `ROUTED_COMMAND_IDS` instead.
12. The corpus coverage of real pictures (EMF clips, cropped JPEG, VML images): no committed fixture has a picture; the
    picture parity rests on the pictures generated in the harness.

### Wave 5 — R: review and document (comments, revisions, fields, find, spelling, styles, clipboard buttons, Alt keys)

Frontier R of wave 5 (2026-11-11, implementer Opus, plan by the wave's planner): the review and document families of
`<universal-write engine="rust">` routed to the session — tracked changes, comments, find/replace and the Navigation pane,
fields and references (TOC, captions, cross-references, table of figures), bookmarks and hyperlinks, styles (list, look,
Inspector, create/modify/rename/delete), spelling, the clipboard WITHOUT a clipboard event (the bars' Cut/Copy/Paste, Paste
Special, the Office Clipboard pane, `pasteDefaults`) and the TS keys with Alt (gate items 12, 14, 15, 16). The TS default
path is unchanged (phase 0 below).

#### Routes

Files of the frontier: `engine-routes-review.ts` (names), `rust-engine-review.ts` (the handler map and the hook
registration), `rust-review-core.ts` (session/layout access through `RustEditor.frontApi()`, the line geometry caches),
`rust-review-revisions.ts` (tracked changes, comments, `runsOf`, the author sync), `rust-review-find.ts` (find, navigation,
fields, references, bookmarks, links), `rust-review-styles.ts`, `rust-review-spell.ts`, `rust-review-clipboard.ts` (buttons,
the `readClipboard` job and the key hook), `rust-review-overlays.ts`. 128 public methods, 3 private, 100 bar commands
(`engine-review.test.ts` prints them).

| Element method / input | Session |
|---|---|
| `toggleTrackChanges`, `isTrackingChanges` | `settings.setTracking`; query `tracking` (new) |
| the element's `author` (the TS reads it live for every new revision and comment) | `settings.setAuthor` (not an edit: no undo step, no dirty flag), pushed when the name changed: after each open (decision 6), from a `MutationObserver` on the `author` and `lang` attributes (the default name is per interface language), from an own accessor over the prototype's `author` property, and before each key and click (a hook first in the key and pointer lists: the other fronts' keys edit too). ED-318 |
| `setMarkupMode`/`getMarkupMode`, `setMarkupOptions`/`getMarkupOptions` | `settings.setMarkup`, `revision.setMarkupOptions` (insDel ↔ insertionsDeletions, `authors: null` ↔ absent); queries `markupMode` (new), `markupOptions`; `uw-markup` gets the TS `{ mode }` detail |
| `getRevisions`, `revisionSpans`, `revisionAuthors`, `authorColor` | query `revisionEntries` mapped to `WriteRevision` (the balloon text by the TS `formatDescription`, style names from `paragraphStyles()`), `revisionAuthorsNow`, `authorColorOf` |
| `accept`/`rejectRevisionAt`, `accept`/`rejectChange`, `accept`/`rejectAllChanges` | `revision.acceptAt/rejectAt/accept/reject/acceptAll/rejectAll`; counts from `revisionCount` before/after |
| `nextChange`, `previousChange`, `selectRevision`, `revisionAtCaret`, `revisionAnchors` | `revision.nextChange/previousChange/selectRevision` + scroll to 1/3 of the view (TS `scrollToSelection`); `revisionAt` of docx-core ported over the body entries; anchors from the paragraph's lines (`uo_shell_para_lines`, the engine line's visible range) |
| `getComments`, `addComment`, `removeComment`, `goToComment`, `commentAnchorPx`; a click on commented text (`uw-comment-click`) | query `comments`; `comment.add/remove` (the new id = the new entry); the first run carrying the id (`paraRuns`, new) + `setSelection` + scroll (`#para:N` rule) |
| `findNextMatch`, `replaceMatch`, `replaceAllMatches`, `highlightMatches`, `selectAllMatches`, `goTo` | `find.*`, outcomes from `navigationResult` (F9Result), the Find-box messages of the TS `findMsg`; `goTo('page'/'line')` in the shell over the layout (the session has none) |
| `search`, `clearSearch`, `goToSearchResult`, headings (`getHeadings`, `currentHeading`, `goTo/select/promote/demote/move/insert/deleteHeading`), `getSelectionRanges`/`setSelectionRanges`, `goToPage` | `navigation.*`, queries `searchResults` (+ page from the lines), `headings`, `currentHeading`, `selectionRanges` |
| fields, TOC, captions, cross-references, table of figures | `reference.*` (field kinds PAGE/NUMPAGES/DATE → page/numPages/date), queries `canInsertField`, `dateTimeFormats`, `tocTemplates`, `tocLevel`, `has/inTableOfContents`, `tableOfContentsOptions`, `captionLabels`, `captionNumbering`, `autoCaption` (`{ table: { label, position } }` ↔ the wire tuple), `captionTarget`, `nextCaptionNumber`, `crossReferenceKinds/Items`, `tableOfFiguresOptions`; booleans and counts from `referencesResult` |
| bookmarks, hyperlinks (`insertBookmark`, `deleteBookmark`, `getBookmarks`, `hasBookmark`, `goToBookmark`, `insertHyperlink`, `setLink`, `removeLink`, `getLink`, `getLinkTargets`, `navigateTo`), Ctrl+click | `bookmark.*`, `find.goToBookmark`, query `bookmarks` (sorted by name in the shell with the TS collation); `getLink` = the TS `linkGroupAt` over `paraRuns`; Ctrl+click (pointer hook) → `openExternalLink`, else `navigateTo`; `navigateTo('#_top')` leaves the note, text box, header/footer and cell first (the TS `toBody()`: `context.exit*` by frontier S's `storyContext`), `#para:N` and a bookmark leave the cell (the TS `exitCell()`) |
| `listStyles(filter)`, `paragraphStyles`, `styleFormatting`, `currentStyles`, `inspectStyle`, `selectionStyleFormat`, `stylesKey`, `knownFontFamilies` | queries `listStyles` (new: the `'all'` list with the use flags), `styleLook` (new: `lookOf` over `withLatent`), `defaultParagraphStyle`/`defaultCharacterStyle` (new, gate 15), `runPropsAt`/`paraPropsAt` (new), `stylesVersion` (new); the filter, the sort and the formatting policy in the shell (styles.ts tail, `policyStyleOk`); `embeddedFamilies()` (new export, gate 14) |
| `createStyle`, `modifyStyle`, `renameStyle`, `deleteStyle`, `setStyleInGallery`, `updateStyleFromSelection`, `clearParagraphStyle`, `clearCharacterStyle`, `setDefaultFont` | `style.*` (the TS `StyleFormat` value/null/absent ↔ the wire `Ch*`); `createStyle` answers the new id |
| spelling (`toggleSpellCheck`, `spellingAtCaret`, `spellingSuggestions`, `replaceSpelling`, `ignoreSpelling`, `addToDictionary`, `nextMisspelling`) | shared/spell's `SpellChecker.shared` loaded by the TS `import()`; the spans from `paraRuns` (language and `noProof` per run, what is not checked). On disconnect (the shell's `park` hook) the cache's listener and the shared checker's reference are released as the TS `disconnectedCallback` does (the Hunspell worker ends with the last editor); a released cache sends no further check (a call after the last `dispose` would start a new engine) |
| `copy`, `cut`, `paste(mode)`, `pastePlain`, `pasteSpecialFormats`, `pasteSpecial`, `clipboardItems`, `pasteClipboardItem`, `pasteAllClipboardItems`, `removeClipboardItem`, `clearClipboardItems`, `onClipboardChange`, the `pasteDefaults` field | `clipboard.copy` / `clipboard.cut`: the session decides what is copied (a drawing object, selected cells, the ranges of a non-contiguous selection, the text, a note reference alone; the cut's cell rules), `copied()` null = nothing copied, nothing written; else the listeners, then `navigator.clipboard` (the TS `writeClipboard`). `navigator.clipboard.read()` → `clipboard.pasteData` (`encodePaste` tree, `withRich`, an image as the session's decode job through the shell's `apply` and frontier O's runner; read denied → the session's own copy). `pasteSpecialFormats`/`pasteSpecial` are two-phase: the session's `readClipboard` job is answered by R's handler registered in O's `jobKinds` (the system clipboard as the TS reads it; denied: `failed`, the session's own copy). Ring commands and query `clipboardItems`; a KEYBOARD copy/cut (the clipboard event of the shell) notifies the listeners too (the shell's `copied` hook; the TS `rememberClip`). `pasteDefaults` is an accessor on the element that pushes `clipboard.setPasteDefaults`; the bars change the object in place (`Object.assign`), so the defaults are pushed again (when changed) before every button paste, ring paste and Ctrl+V |
| keys: Alt+Arrow/Home/End (+Shift), Alt+Enter, Alt+Backspace/Delete, Ctrl+Alt+Left/Right (word), Ctrl+Alt+Up/Down (line), Ctrl+Alt+Backspace/Delete, Ctrl+Alt+letter of the TS Ctrl map (b i u z y a e l r j; c and x = the buttons), Ctrl+Alt+V (`uw-paste-special`), Ctrl(+Alt)+Home/End (the document's edges; Shift extends), PageUp/PageDown with Ctrl or Alt (0.9 of the view + 20 line moves), Ctrl+Alt+Enter / Ctrl+Alt+Shift+Enter (page / column break; nothing in a note), Tab with Ctrl or Alt (the TS Tab rule), F9 (`uw-toc-update` first when the session's `inTableOfContents`, ED-317), Ctrl+hyphen / Ctrl+Shift+hyphen | the key hook, the TS `bindEvents` rules (universal-write.ts 10516–10790) in the TS order over routed methods; the TS rules for Home/End, PageUp/PageDown, Enter and Tab have no Alt check, so with Alt they run as without it. Ctrl+Alt+F/D (notes) and the cell keys are frontier S's hook (it runs first: in a cell Ctrl+Home leaves the cell, then this hook goes to the document's start); the breaks call frontier O's routed `insertPageBreak`/`insertColumnBreak` |
| screen overlays (`paintEngineMarks` field shading + comment highlight, `paintEngineSpelling`, `paintSearchHits`) | routed under the TS private names (a harness's `el[m] = () => {}` turns them off on both engines); painted by the paint hook under the selection, over `uo_shell_page_lines` (body scope) |
| the extra ranges of a non-contiguous selection | `paintExtraRanges` from the shell's `overlay` hook (with the selection, over it: the TS `extras()` in `paintEngineSelection`); NOT under the TS name `paintMultiSelection`, which is the floating objects' multi-selection and lasso (frontier O's) |

Bar commands routed (100): cut, copy, paste, pastePlain, find, replace, find:advanced, goto, goto:f5, hl:all, hl:clear, in:sel,
in:main, paste:strip, paste:special, paste:defaults, clipboard:pane, clip:paste, clip:delete, view:navpane, navigation, the 13
nav:* ids, comments, link, comment, field:page/numpages/date, bookmark, crossref, caption, tof, tof:update, toc, toc:custom,
toc:remove, toc:mark, toc:addText, styles, styles:pane, style:inspector, style:new, style:modifyCurrent, style:updateCurrent,
style:h1–h3, style:create/update/modify/rename/ungallery/togallery/delete, comment:prev/next/delete/deleteAll, spelling,
spellAsYouType, trackChanges, markup:simple/all/none/original, show:insdel, show:formatting, balloons:revisions/inline/mixed,
people:all, reviewPane (+ vertical, horizontal), the 12 change:* accept/reject ids, change:prev, change:next.

#### Session access added (schema, frontier R owns it)

`core/schema/session.uos` (regenerated, `cargo run -p uo-schema -- check` up to date; three regenerations, all additive at the
end of `QueryOp`/`QueryAnswer`, announced): QueryOps `Tracking`, `MarkupMode`, `ParaRuns(QParaRuns { para, fallback })` →
`list<RunSpan { len, link?, link_tip?, field, comment_ids?, del, spell, lang, char_style? }>`, `ListStyles` →
`list<StyleListItem>` (with `link_name`), `StyleLook(QStyleLook)` → `AStyleLook { value: StyleLookW? }`,
`DefaultParagraphStyle`, `DefaultCharacterStyle`, `RunPropsAt(QRunPropsAt)` → `RunProps`, `ParaPropsAt(QParaPropsAt)` →
`ParaProps`, `StylesVersion` → u32; message `ShellLines { lines: list<ShellLine> }`. Arms in `query.rs`, bodies in
`src/session/review.rs` (the spelling half of `LangResolver` with `noProof`, `skipRun`, `lookOf`/`borderCss`). Shell exports
(`src/session/shell_review.rs`): `uo_shell_page_lines(h, layout, page)`, `uo_shell_para_lines(h, layout, para)` (the cached
`EditLines` of the layout, chars as o/x/w columns), `uo_shell_range_rects(h, layout, page, ap, ao, fp, fo)` (normalised:
backward = forward), `uo_shell_embedded_families(h)`; JS `pageLines`, `paraLines`, `rangeRects`, `embeddedFamilies` (session.mjs
R block, session.d.ts, the editor's write-session.d.ts).

#### Declarations (ED-301..ED-319)

- **ED-301** uo-docx additive accessors (decision 1, option A without widening the private types): a new
  `session/ops/styles/listing.rs` (`pub mod listing;` in styles.rs) with `Session::style_list_entries()`,
  `default_paragraph_style()`, `default_character_style()`, `styles_in_use()`, `style_look_formatting(id)` (`withLatent` +
  `resolveStyleId`), read-only over `&Session` (the memoised index when fresh, else the part read again);
  `captions.rs` `all_paragraphs` is `pub(crate)` (was private). No behaviour change (uo-docx styles tests 20/20).
- **ED-302** `listStyles`: the session answers definition order + the latent built-ins; the filter, the sort (priority, then
  `Intl.Collator(uiLocale(lang))`) and the policy filter run in the shell exactly as the TS (decision 2).
- **ED-303** `stylesKey` = `${stylesVersion}:${editsSinceLoad}:${canUndo}:${canRedo}:${hfEdit}:${uiLang}` (the TS uses its
  stacks' lengths; the key only has to change when the list can). `hfEdit` (fix round 1) is 1 while a header/footer is
  edited, also with one of its text boxes open, read from frontier S's `storyContext` (the TS `this.hfEdit`).
- **ED-304** the overlays, revision anchors and search pages use the BODY scope's lines: squiggles and field shading in
  headers/footers, notes and text boxes (f8__7__0, measured) and the page of a search hit in a table cell (f2-3-2: -1 vs 0)
  are not answered; the other stories' scopes are frontier S's. Also `selectRevision(i)` of an entry outside the body (a
  header/footer, note or text box revision): `false` and no scroll, where the TS scrolls to its anchor.
- **ED-305** the Office Clipboard ring is the session's (per instance, G7); the TS ring is page-global. `clipboardItems()`
  answers kind `text` for every item.
- **ED-306** `replaceSpelling` = the session's `delete` then `type` (two undo steps; the TS takes one). A single
  `find.replaceMatch` or a `type` over the selection gives the same text but splits the run (measured: the saved
  `document.xml` differed), so the two-step form was kept: equal saved bytes, one more undo step.
- **ED-307** the spelling cache (`SessionSpell`) repeats ~60 lines of spell.ts's `ParagraphSpell` keyed by the spans'
  content instead of the `Paragraph` object (decision 4: spell.ts untouched).
- **ED-308** the Alt keys read `key` as the TS does: AltGr characters whose key is not a letter of the Ctrl map still type
  (ED-21 stays true; `--mode keys` of the main harness: AltGr+2, AltGr+E, Option+2 unchanged).
- **ED-309** formatting marks (the showMarks half of `paintEngineMarks`) are not painted (O / remainder).
- **ED-310** bookmark brackets (`paintBookmarkMarks`) are not painted and `showBookmarks`/`view:bookmarks` stay disabled (no
  session answer of the bookmark positions per paragraph).
- **ED-311** not routed (disabled or TS-on-empty-model as before): `countMatches` (the session's count is a private alias),
  `getLinks`, `hasExternalFields`, `tocEntrySuggestion`, `paintTocTemplate`, `ensureBuiltinStyle` (a mutation without a
  command), list styles (`listStylesInDocument` neutral, create/apply/modify list style), `selectSimilarFormatting`, mail merge,
  the Paste Special link variants, image clipboard items (two-phase), the `\h` REF Ctrl+click target, `stylesVersion` (routed
  by frontier S through its shell path; R's `stylesKey` reads the session query).
- **ED-312** `revisionSpans()` answers the session's wire entries (`{ span, story, loc }`): the TS entries hold model objects.
- **ED-313** `selectionStyleFormat()` composes the TS `selectionFormat('linked')` from `runPropsAt`/the pending props and
  `paraPropsAt` (not a session command).
- **ED-314 (core)** a document WITHOUT a theme part: the TS editor resolves theme colors over `emptyFormatting().theme` (the
  Office default palette), the session over an empty theme (the `w:val` fallback). Visible on f2-3-0/f2-3-2 (Strong =
  accent2: TS ED7D31, session C00000) on the screen from the first step, and in `styleFormatting` of a latent text1-tinted
  style (Subtle Reference 595959 vs 5A5A5A). The main harness's thumbnails do not show it (its TS thumbnail path agrees);
  gate R-1.
- **ED-315** (shared hook, `rust-engine.ts` `apply`) a ChangeSet with `formatting` drops the shell engine's incremental
  state (`shell.clear()`): measured, after `style.updateStyleFromSelection`/`modifyStyle` the incremental relayout answered
  path `incremental`, relaid `[0, 0)` (stale lines). The TS refreshes fully after a style edit. Gate R-3.
- **ED-316** `uw-markup` from the session has no detail: the shell adds the TS `{ mode }`.
- **ED-317** (fix round 1) F9 asks the TOC question (`uw-toc-update`) when the session's `inTableOfContents` is true: a TOC
  table (not a table of figures) intersecting the selection, or the caret in the TOC content control. The TS asks when ANY
  `TOC` field intersects the selection (a table of figures, `TOC c`, included) and never for the content control alone.
  Both agree on the harness documents; a session query "a TOC field intersects the selection" (uo-docx `toc_fields` +
  `toc_in_selection`, both `pub(crate)`) is gate R-4.
- **ED-318** (fix round 1) the author is pushed from an attribute observer (a microtask) and before each key and click: a
  programmatic mutator called in the SAME task right after `setAttribute('author', …)` (no key, no click in between) records
  the previous name; the property setter pushes at once. The harness's mid-script changes (attribute, then the property)
  are equal in `getRevisions` and the saved `document.xml`.
- **ED-319** (fix round 1) the TS rules for PageUp/PageDown and Tab take any modifier: with Ctrl (no Alt), which the wave-4
  shell forwarded as an owned key without running it, they run here too (Ctrl+PageDown = the scroll and 20 line moves,
  Ctrl+Tab = the Tab rule), not only with Alt.

#### Files touched outside R's own files

- uo-docx: `session/ops/styles.rs` (+1 line `pub mod listing;`), `session/ops/references/captions.rs` (`all_paragraphs`
  `pub(crate)`), new `session/ops/styles/listing.rs` (ED-301).
- uo-wasm: `src/session.rs` (two mod lines, R-commented), `query.rs` (the R arms), schema + generated codec (R owns them),
  `js/session.mjs` and `js/session.d.ts` (R blocks).
- editor: `rust-engine.ts` — the shared hook block (`hooks`, `FrontApi`, `frontApi()`, the four dispatch lines; S and O
  extended it), and ED-315's two lines in `apply`; fix round 1: two hook lists in the block (`park`: the element
  disconnected, dispatched first in `park()`; `copied`: a keyboard copy/cut put an item in the ring, dispatched at the end of
  `clip()`), one dispatch line each; `write-session.d.ts` (R block: the four methods, `ShellLine`).
- editor, read only (imports, no edit): frontier S's `bindStories(...).storyContext` (the edit context: `hfEdit`, the note
  for Ctrl+Alt+Enter, `toBody()`), frontier O's `jobKinds`/`runJobs` (`rust-objects-jobs.ts`: R registers the
  `readClipboard` job there, as O's file announces).
- harness: `editor_switch_lib.mjs` (`entryExtra`), `editor_switch.mjs` (R blocks: the shell's overlays off in the "TS overlays
  off" capture; `--mode chrome`: Ctrl+K opens the hyperlink dialog, Cut/Copy/Paste enabled, the not-routed probe is
  `painter`/`color` — decision 3 taken as recommended).

#### Numbers

Harness `core/crates/uo-wasm/tests/browser/editor_switch_review.mjs` (new; TS first, then Rust, the same script of element
calls, real keys and bar buttons; author "Ana", `lang="pt"`, the ribbon; d3 review documents + f2-3-0, f2-3-2, f7a__7__0,
f8__7__0; facets: answer, selection, uw-* events, dirty, toolbar run/para, the family's facets, end screen with R's overlays on
and off, saved parts; FAILS on anything undeclared or a page error). `--mode all`: exit 0, 120 s:

| Family (documents) | Facets equal | Different (all declared) |
|---|---|---|
| revisions (revisions, f7a__7__0; 27 steps) | 703 | 0 |
| comments (comments, f2-3-0; 8) | 151 | 2 (ED-314) |
| styles (styles, f2-3-0, headers; 12) | 396 | 42 (ED-314 looks of one latent style per step; end screens: ED-314, gate R-2) |
| find (f2-3-0, f2-3-2, headers; 17) | 494 | 5 (ED-314, ED-304) |
| fields (fields, many-fields, hyperlinks, f2-3-0; 16) | 759 | 2 (ED-314) |
| clipboard (f2-3-0; 11) | 89 | 2 (ED-314) |
| keys (f2-3-0; 21 Alt/F9/hyphen keys) | 138 | 2 (ED-314) |
| spelling (f2-3-0 91 misspellings, f8__7__0 273) | lists, F7 path (next, at caret, suggestions, replace, ignore), saved parts equal | squiggle screens: ED-314 (f2-3-0), ED-304 (f8__7__0 header/footer/notes) |

`--mode memory` (count build, 30 cycles of create → load revisions.docx → comment, accept all, replace all, list styles,
search, getRevisions, paint → remove): sessions/shellEngines/handles 0 after every remove, live bytes flat at 6,420,155 after
warm-up. Native `tests/shell_review_native.rs` 3/3 (A queries on comments/styles/hyperlinks; B lines vs rects, backward =
forward, hostile handles; C M1: 2,055,985 B allocated, 0 B left). `shell_native` 5/5, `shell_api.test.mjs` 2/2 (M4 10,000
engines, live bytes back to 12,782,879). uo-codec/uo-schema all pass; uo-docx `styles` 20/20. Editor vitest 33 files / 324
(engine-switch unchanged and green; new `engine-review.test.ts`); editor and viewer tsc clean; the core strict tsc line
(with the three `write-session*.d.ts`) clean; `check-changed` green (clippy native + wasm32 + bans over uo-docx, uo-docx-layout,
uo-ffi, uo-wasm). Main harness: `--mode phase0` ok (0 `.wasm`, 0 engine chunk, 0 leaks), `--mode chrome` ok (both bars),
`--mode memory` ok (live 16,986,450 flat); `--mode parity` stride 6: every R-related facet equal, ONE fixture fails —
f7b__7__10 from step 6 (a click on a floating object: the selection now differs, beyond its ED-7 entry's facets): a click on
an object is frontier O's (`objectsPointerFirst`), not this frontier's (R's pointer hook runs only on Ctrl+click and a
commented run); left for O / the orchestrator.

**Fix round 1** (the reviewer's 4 major and 8 minor findings). The harness gained: an author change mid-script (attribute,
then the property without the attribute) with tracking on; a real click on commented text (`uw-comment-click` in the events
facet) and a real Ctrl+click on a hyperlink (`window.open` stubbed: `https://example.com/x _blank` on both); a keyboard
Ctrl+C / Ctrl+X with an `onClipboardChange` listener (notification counts and ring sizes compared), the bars' in-place
`Object.assign(el.pasteDefaults, …)` before a button paste and a Ctrl+V; the keys Ctrl+Alt+Home/End (+Shift),
Alt+PageDown/PageUp, Ctrl+PageDown, Ctrl+Alt+Shift+PageUp, Ctrl+Alt+Enter, Ctrl+Alt+Shift+Enter (page counts), Ctrl+Tab and
Alt+Tab; an `overlay mask` end facet (the pixels R's overlays change, between the top and bottom margins: compared even where
the page base differs by ED-314; the header/footer bands are ED-304's), and the `looks` facet declared per style id (only
`RefernciaSutil` is left out) instead of the whole facet. The keys mode also found a defect: Ctrl+Alt+V left the session's
`pasteSpecialFormats` job unanswered and every later command was held (a `setSelection` did nothing); the `readClipboard` job
is answered now. `--mode all`: exit 0, 122 s, every family's step facets equal or declared:

| Family (documents) | Facets equal | Different (all declared) |
|---|---|---|
| revisions (revisions, f7a__7__0; 32 steps) | 837 | 0 |
| comments (comments, f2-3-0; 10) | 185 | 2 (ED-314 end screens) |
| styles (styles, f2-3-0, headers; 12) | 398 | 43 (ED-314: the look of `RefernciaSutil` only; end screens ED-314, gate R-2; headers' overlay mask R-2) |
| find (f2-3-0, f2-3-2, headers; 17) | 496 | 6 (ED-314, ED-304; f2-3-2's overlay mask R-6) |
| fields (fields, many-fields, hyperlinks, f2-3-0; 19) | 907 | 2 (ED-314) |
| clipboard (f2-3-0; 20) | 153 | 2 (ED-314) |
| keys (f2-3-0; 38) | 241 | 2 (ED-314) |
| spelling (f2-3-0 91, f8__7__0 273 misspellings) | lists, F7 path, saved parts, f2-3-0's overlay mask equal | screens ED-314 / ED-304; f8__7__0's overlay mask ED-304 (notes) |

`--mode memory` (count build, 30 cycles; the spell checker now loaded, a dictionary in, before each remove): sessions,
shell engines and handles 0 after every remove; the shared checker's references 1 → 0, listeners 1 → 0, its worker
terminated (no transport), the cache released, in every cycle; live bytes flat at 7,925,251 after warm-up. The TS reference
cycle in the same page: references 1 → 0, listeners 1 → 0, no transport. Editor vitest 33 files / 329 (`engine-review.test.ts`
+2: a released cache sends no check and drops its listener; the author push per change); editor and viewer tsc clean; main
harness `--mode phase0` ok, `--mode parity` (stride 6, 13 fixtures) ok; `check-changed` green (no Rust change in this round).

#### For the milestone-2 gate

- R-1 (ED-314) the session's theme for a document without `theme1.xml` (the TS default palette).
- R-2 styles.docx: select paragraph 0 offsets 0–2, italic, Update Style to Match Selection: the saved `.docx` is
  byte-equal to the TS's, but the session lays the paragraph out with the runs after the first without Heading 1's
  bold italic (reloading the saved bytes renders like the TS) — the session's model after `update_style_from_selection` is
  not what it saves (uo-docx).
- R-3 (ED-315) LA7's incremental relayout ignores a style sheet change (path incremental, relaid [0, 0)); the shell clears the
  engine on `formatting` (and an undo of a style edit relies on the undo ChangeSet carrying `formatting`).
- R-4 the other stories' lines for the overlays (ED-304) once S's scopes land; bookmark brackets and formatting marks
  (ED-309/310); a session answer for `countMatches`, `getLinks`, `hasExternalFields`, `tocEntrySuggestion`,
  `ensureBuiltinStyle` (ED-311); a page-level Office Clipboard ring (ED-305); a one-step replace for `replaceSpelling`
  (ED-306).
- R-5 the harness page helpers are copied from editor_switch.mjs (move them into the lib); the f7b__7__10 failure of the
  main parity run (above; not reproduced in fix round 1: the stride-6 parity run is ok).
- R-6 (wave-4 gate item 10, reproduced) f2-3-2 right after `loadDocx`: the TS editor sets automatic hyphens (dia-grama,
  responsabili-dade, in-ternacionalização on page 1) and the session's layout does not; the saved bytes are equal. Visible
  in R's harness as the overlay mask of `f2-3-2|find` (the squiggles on differently broken lines), declared.
- R-7 shared/spell: `SpellChecker.dispose()` during the engine's startup (before the init answered) leaves the released
  instance's worker (or a main-thread engine) alive: `startEngine` goes on after `dispose` and sets a new transport. Same on
  the TS engine; the soak waits for a dictionary before each remove. Not this frontier's file.
- R-8 ED-301: confirm option B (the style listing ported into uo-docx, `styles/listing.rs`, ~210 lines, read-only, tested)
  or move it back behind narrow accessors.
- R-9 a one-step `replaceSpelling` (ED-306: one undo step in the TS, two here; a session command "replace a range keeping
  the props" or an undo group) and a session query for the F9 TOC question (ED-317).

#### Process metrics

One implementer session (Opus), ≈ 3 h wall time after the plan. Rust builds: 4 session wasm builds (2 min 20 s – 2 min 40 s
each, warm) + 1 count build; the schema was regenerated three times (DefaultCharacterStyle, then ParaPropsAt/link_name, found
by the harness's Inspector facets). Rework: two shell-quoting accidents (backquotes inside `node -e` lost text in the schema
and two TS files; repaired with the Edit tool); a stale wasm (schema hash mismatch) hung the first browser run (the harness
now has a 90 s deadline per page call); the screen captures raced the spell checker (the capture now waits for two equal
paints). Defects found and fixed by the harness: `revisionAtCaret` (the docx-core `revisionAt` rule), revision anchors in
markup views (the engine line's visible range), `goTo('page'/'line')` (layout rules in the shell), the Inspector's names and
direct paragraph props (two queries added), `replaceSpelling`'s run split, the stale incremental layout after style edits.
D: 38 GB free at the end.

Fix round 1: ≈ 1 h 15 min. Two session wasm builds (2 min 07 s, 1 min 56 s; S's and O's Rust had moved since the last one),
no Rust change. Rework: an inline `node -e` replacement failed on its own escaping (redone with the Edit tool and patch
scripts written by the Write tool); the first click steps answered "no point" on Rust (the shell's `caretRectAt` answers the
focus only: the harness moves the caret there and back on both engines); the first soak still showed a live transport (the
released cache's in-flight batch called the checker again: fixed; and shared/spell's startup race, R-7). Defects the new
steps found besides the reviewer's: the unanswered `pasteSpecialFormats` job, the coarse f2-3-2 declaration hiding R-6.
D: 38 GB free at the end.

### Wave 6 — P: packaging, CI and docs of the Rust editor

**D5 answered: two modules, one rule.** `@universal-office/write` ships `chunks/uo-write.wasm` (`--features write`, 4,122,173 B,
1.47 MB gzip -9) for viewer-only use and `chunks/uo-write-editor.wasm` (`--features session`, 9,686,613 B, 3.18 MB gzip -9) for
`engine="rust"` editors; `@universal-office/document-viewer` ships only `uo-write.wasm`. `tools/build-pkg.mjs` picks the module by the
CONTENT of the bundle, not by the output name: step 1c reads each output's metafile inputs and ships the viewer module when
`write/packages/viewer/src/rust-engine.ts` is in it and the editor module when `write/packages/editor/src/rust-engine.ts` is. The old
name regex `/chunks\/rust-engine-/` matched two outputs of the write package (the editor's engine chunk and an empty facade) and
would have shipped only the viewer module. It also asserts that the holding chunk sits in `chunks/` (so `new URL('uo-write-editor.wasm',
import.meta.url)` resolves next to it) and that the static closure of `index.js` carries neither a `rust-engine.ts` nor anything of
`core/crates/uo-wasm/js/` (phase 0 at package level). **Module order rule (measured by `pkg_smoke.mjs`):** one engine module per page,
first load wins. Inside one package the editor and the viewer entries share the engine module (the viewer `rust-engine.ts` and the
`js/viewer.ts` memo live in a shared chunk): the editor module serves a viewer loaded after it (1 request for
`uo-write-editor.wasm`, 0 for `uo-write.wasm`); a viewer loaded FIRST fetches `uo-write.wasm` and the editor `initialize()` then
rejects with "the engine module of this page has no session exports (load the editor build, uo-write-editor.wasm, before any viewer)".
The standalone `document-viewer` package carries its own copy of `viewer.ts`, so a page with both packages instantiates two modules
(two memories): documented, not a conflict.

**S11 (milestone-1 finding) closed.** `checkEngineWasm(file, kind)` (exported by build-pkg, cargo-free, `WebAssembly.Module` in Node)
refuses: a file that does not compile; any export matching `/(^|_)(test|selftest|memtest|bench|fixture)_|_fixture$/` (the trailing `_`
keeps `uo_session_hit_test`); an instance (stub imports that throw) answering `uo_memory_live_bytes() !== -1` (the counting
allocator); the editor module without `uo_session_open`; the viewer module WITH it (the session build must not ship as
`uo-write.wasm`). Measured on existing builds, each refused: `session_count.wasm` (`uo_test_trap`), `selftest.wasm` (18
`uo_session_test_*`), `viewer.wasm` count+write (`uo_test_trap`), the cargo default `uo_wasm.wasm` of 2026-11-03 (11
`uo_fonts_memtest_*`: S11 reproduced, it was what `shipEngineWasm` would have copied), `session.wasm` as viewer, `viewer.wasm` as
editor. The check runs BEFORE the copy. The default source is no longer the cargo output but
`core/target/pkg-wasm/uo-write.wasm` / `uo-write-editor.wasm` (`UO_WRITE_WASM` / `UO_WRITE_EDITOR_WASM` override). A missing module still
warns (SW-9) unless `UO_REQUIRE_ENGINE_WASM=1` (CI), which throws. `shipEngineWasm(id, dist, kind = 'viewer')` keeps the old
two-argument form working (the wasm job line).

**Licence notices.** `gen-notices.mjs` has a third artifact `wasm-editor` (`cargo tree -p uo-wasm --features session`); the markdown
has one column per artifact. `cargo tree -e normal` gives the SAME 7 registry crates for `--features write` and `--features session`
(adler2, equivalent, hashbrown, indexmap, miniz_oxide, rustc-hash, ryu-js); `session` adds only workspace code (`uo-text/collation`, an
own port of `sheet/packages/calc/src/collation.ts`). So `core/deny.toml` is unchanged, `core/licenses` differs only in `notices.json`
(+103 lines, the artifact) and `THIRD-PARTY-NOTICES.md` (the column), still 27 files, `--check` current. Both modules copy into ONE
`licenses/uo-write-wasm/` (the union; identical today).

**`writeTypes` defect (pre-existing since milestone 1) and fix.** `build:pkg write` and `document-viewer` failed at the types step on
rust-core: dts-bundle-generator builds its program from the ENTRY files only, so the ambient `declare module
'@universal-office/write-engine'` / `'@universal-office/write-session'` (`src/*.d.ts`) were never seen. Measured (`dts-probe.mjs plain`):
index.d.ts 51 error lines (TS2307 on write-session), viewer.d.ts 15 (write-engine). Fix: per entry a wrapper
`<pkg>/.pkg-entry-<name>.ts` with `/// <reference path>` to every `src/*.d.ts` of the package and of each extra entry plus `export *`
of the real entry (neither `index.ts` has a default export), deleted in the existing `finally`. Result: `index.d.ts` 327,841 B
(`class UniversalWriteElement`), `viewer.d.ts` 36,114 B (`class UniversalDocumentViewerElement`), no reference or write-session
text in the output. Side finding: the published `configureEngine(source: NonNullable<typeof UniversalWriteElement.engineSource>)`
declares `private static engineSource;` untyped, so the public parameter type is `any` (Wanted).

**`UO_TOOLS_ROOT`** (dev only, inert when unset): esbuild and dts-bundle-generator are required from that checkout, `nodePaths` become
every existing `<root>/{write,converter,sheet,slides,shared/*}/node_modules`, and the temporary tsconfig gets `typeRoots` and a `paths['*']`
fallback. It is how the worktree (no node_modules) ran the real build.

**CI (`.github/workflows/rust-core.yml`, parses with PyYAML; 17 jobs, 130 `run` blocks pass `bash -n`).** `pull_request.paths` +=
`write/packages/editor/**`, `write/packages/viewer/**`, `tools/build-pkg.mjs`. `codec`: the strict tsc list gains
`editor.check.ts`, `write-session.d.ts`, `session-stories.d.ts`, `write-session-stories.d.ts`, `write-session-objects.d.ts` (what
`m2-verify-d.sh` checks). The `deny` comment names the editor wasm. `wasm`: (1) after the `count,write,render-fixture` build a negative
S11 step written as `if UO_WRITE_WASM=... node ...; then echo ...; exit 1; fi`, NOT `! cmd`: under `bash -e` a negated command never
fails a step unless it is the last line (the old shipping line worked only for that reason); (2) after the session build the editor module is
copied and shipped with `shipEngineWasm('write', ..., 'editor')` and `test -f`. New job `editor` (ubuntu, 90 min; the viewer job Install,
Build and Chromium steps verbatim): `editor_switch_tsc.mjs` (editor and viewer tsc), the `session,count` build, editor and viewer
vitest through `editor_switch_vitest.mjs` (the verified harness config, `UO_TS_ROOT` = the checkout), `shell_api.test.mjs`
(`UO_ES_M4=2000`), the package step (both modules built and copied, `UO_REQUIRE_ENGINE_WASM=1 node tools/build-pkg.mjs font-files
document-viewer write` plus `test -f`/`grep` of the outputs), Chromium, `pkg_smoke.mjs`, `editor_switch.mjs --mode all`
(`UO_ES_STRIDE=6`, `UO_ES_SOAK=50`), and the three front harnesses (`UO_ES_SOAK=50`, `UO_EO_SOAK=30`, `UO_ER_SOAK=50`) with `if:
github.event_name != 'push'` (about 13 min together by the wave-5 logs). The stories, objects and review harnesses take no stride variable.

**README and CHANGELOG.** The editor README "The Rust engine (preview)" section is rewritten for waves 5-6: the package ships the module,
`configureEngine`/`engineStats`, the module order rule, CSP, what is routed by family, what stays disabled (from `routes-probe.mjs` at
HEAD: 511 public methods = 381 routed + 26 shell + 65 disabled mutators + 41 disabled queries; 21 private routed; 434 bar commands
routed) and the declared host-visible behaviours (ED-6, ED-16, ED-18, ED-19, ED-27, ED-310). `CHANGELOG.md` "Unreleased" gets "Editor:
engine switch (`engine="rust"`, preview)"; the viewer entry no longer says the editor is outside the switch. No version bump.

**`pkg_smoke.mjs`** (new, `core/crates/uo-wasm/tests/browser/`): the PUBLISHED `write/packages/editor/publish` + `font-files/publish` in
headless Chromium, no esbuild: (a) default engine 0 `.wasm` and 0 `rust-engine-` requests, `engineStats()` null; (b) editor first,
document loaded, one character typed then dirty, `engineStats()` not null, then the rust viewer of `viewer.js` loads a document: 1
`uo-write-editor.wasm`, 0 `uo-write.wasm`; (c) viewer first, then the rust editor rejects naming `uo-write-editor.wasm`. Tooling is
copy-adapted (declared P-3) from `editor_switch_lib.mjs`.

**Verification (all run in this worktree, `CARGO_TARGET_DIR=core/target/m2-pkg`).**
- Three wasm builds (`write`, `session`, `session,count`, `--profile wasm`, -j 3) then `cmp` against the copies in `publish/`.
- `node tools/build-pkg.mjs font-files document-viewer write` with `UO_TOOLS_ROOT` = the frozen root, `UO_REQUIRE_ENGINE_WASM=1`: exit 0;
  it RAN END TO END (not just the shipping function). `write/packages/editor/publish/chunks/` has `uo-write.wasm` (4026 KB) and
  `uo-write-editor.wasm` (9460 KB), both `cmp`-equal to the sources; `document-viewer/publish/chunks` has `uo-write.wasm` only; the
  notices folder is in both; exactly one `.js` mentions `uo-write-editor.wasm` (`chunks/rust-engine-*.js`); `files` includes `licenses`;
  no stray `.pkg-entry-*` or `.tsconfig.pkg.json`. `npm pack --dry-run` of the editor package: 6,735,967 B tarball, 20,005,743 B
  unpacked, 79 files.
- Negatives (each throws with the build-pkg message): the six measured builds above and a missing file with
  `UO_REQUIRE_ENGINE_WASM=1`. Positives: the two freshly built modules ship.
- `pkg_smoke.mjs`: `pkg_smoke: ok` (all checks above). `editor_switch.mjs --mode phase0` (stride 6): ok, 0 `.wasm` requests, static
  closure leaks 0, engine chunk 507 KB.
- `yaml-check.py`: safe_load ok, `editor` job present, every step has `run`/`uses`, `bash -n` of every run block. `node --check` of
  build-pkg, gen-notices, pkg_smoke. `check-changed.mjs`: 0 crates (no .rs changed). `editor_switch_tsc.mjs`: editor and viewer exit 0, no
  output. The strict `tsc_strict` line of `m2-verify-d.sh`: exit 0.
- NOT run here: the CI job itself (never run on GitHub), the vitest/shell_api/parity/front-harness lines of the `editor` job (the
  orchestrator merge run covers them; they are the verified `m2-verify-d.sh` commands), `wasm-tools validate` (CI wasm job).

**Declarations.** P-1 the default source of both modules is `core/target/pkg-wasm/` (not the cargo output). P-2 the `uo-write.wasm` guard
(no `uo_session_open`) assumes decision 1(a); drop it if the maintainer ships the session build for both. P-3 `pkg_smoke.mjs` copies the
server and browser launch of `editor_switch_lib.mjs` instead of importing it (the lib is not P's). P-4 `UO_TOOLS_ROOT` is a dev
override, documented only in the build-pkg header. P-5 the README counts are those of HEAD d4279c16; fronts C and L may route more.
P-6 `pkg_smoke.mjs` types through `insertText` when the element exposes it, else a real click and the key `x`; either path ends in
`isDirty() === true`.

**Files touched:** `tools/build-pkg.mjs`, `core/tools/licenses/gen-notices.mjs`, `core/licenses/notices.json`,
`core/licenses/THIRD-PARTY-NOTICES.md`, `.github/workflows/rust-core.yml`, `write/packages/editor/README.md`, `CHANGELOG.md`,
`core/crates/uo-wasm/tests/browser/pkg_smoke.mjs` (new), this record. No .rs/.ts edited; `core/deny.toml` unchanged.

**Fix round 1 (reviewer).** README/CHANGELOG: text boxes are claimed only for the page's top-level boxes (S ED-102: boxes in cells,
inside boxes and in groups are not entered at HEAD d4279c16; the claim comes back at the merge only if front C's work lands with the harness
proof, a C-dependent line of Wanted (3)); the disabled lists are rebuilt from `routes-probe.mjs` (re-run: 511 = 381 + 26 + 65 + 41) and now name
the Font dialog, the ruler indent drags, gridlines, list styles, Change List Level, Select Similar Formatting, the gallery previews, and say that
the public `findNext`/`replaceAll`/`countMatches`/`getLinks` do nothing (the Find pane's `findNextMatch`/`replaceAllMatches` work); the
CHANGELOG bullet splits open/save formats and qualifies image paste (system clipboard; no ring image items, no Paste Special link variants).
CI: the wasm-job S11 negative captures the output and requires the "build de teste" refusal; `editor` job timeout 90 min until the first
GitHub run gives a duration. `pkg_smoke.mjs` polls `layoutPageCount() > 0` (up to 20 s) instead of a fixed 1500 ms sleep (typing stays P-6).
Re-checked: yaml-check.py ok (17 jobs, editor 18 steps, timeout 90), `node --check` pkg_smoke/build-pkg; pkg_smoke itself and the CI job were
not re-run (no wasm rebuild; docs/CI/test-wait edits only).


**Fix round 2 (reviewer).** The README and CHANGELOG disabled lists were checked only against the PUBLIC METHOD tables; the bar grays out by
`ROUTED_COMMAND_IDS` (`chrome.ts`, engine="rust" loop). New probe `core/target/m2-pkg/probe/cmds-probe.mjs` (esbuild bundle of
`registerWriteCommands` + `engine-routes.ts`, a Proxy registry and universal stubs for ed/app/ctl, no DOM) registers the bar commands
and diffs them against the table. Measured at HEAD d4279c16 plus the uncommitted fronts: **462 commands declared by
`registerWriteCommands`, 381 routed, 81 NOT routed** (`ROUTED_COMMAND_IDS` has 434 ids; the other 53 are registered by the panes: style,
navigation, table menus, underline styles). The 81: `fontsInFile`, `export:odt/html/md/xps`, `print`, `compare`, `painter`,
`view:web/outline/draft/ruler/gridlines/bookmarks`, `color`, `dlg:font`, `borders`, `dlg:paragraph`, `dlg:tabs`, `enableEditing`,
`wordCount`, `mailmerge`, `mergefield`, `fontAccess`, `table:excel/style:new/style:modify/painter/draw/eraser`, `paste:cells:*` (4),
`dlg:borders`, `dlg:pageBorders`, `pic:wrap:editPoints`, `shp:editText`, `pic:change:clip`, `pic:transparentColor`, `underline:more`,
`home:border:*` (11), `home:borders`, `home:hrule`, `list:defineBullet/defineNumber/setValue/defineMultilevel/defineStyle/indents/restart/continue`,
`paste:keep/merge/picture/text`, `fx:pane`, `select:objects`, `select:similar`, `toc:level0..9`, `hyph:manual`. Also not routed (declared
only by the ribbon when `setTextGradient` exists): `color:gradient`. The README "What stays disabled" and the CHANGELOG bullet now list
these by user-facing name and carry the counts; "character formatting" notes the Font Color button is grayed (`setColor` is routed, the
button needs `documentThemeColors()`, ED-222); "page design" says page borders work through `setPageBorders` only. Two items of the review
checked and not as stated: `styleInspector` is a pane id (the command `style:inspector` IS routed), and `pasteCells` is not a declared
command (the four `paste:cells:*` are, all unrouted, listed). ED-16: the README/CHANGELOG no longer state LF or CRLF; they point to the engine
record (front C's `copy()`/`cut()` path says "ED-16 closed" in an uncommitted edit; re-read C's final record at the merge). CI paragraph above now
says 90 min.
Re-run: `node tools/build-pkg.mjs font-files document-viewer write` (modules from copies of the earlier measured builds, because
`core/target/pkg-wasm` is not there and a rebuild was not due: .rs changes of C/L are not P's): `document-viewer` builds (checked module
shipped, no stray files); `write` still stops at `writeTypes` on front C's TS2339 (`rust-engine.ts`: `copy`/`cut` missing on
`RustHostElement`), after the shipping step; any front's TS error breaks `build:pkg` (the CI `editor` tsc step catches it first). To redo at
the wave-6 merge: the three wasm builds, this probe, and `build:pkg` end to end.
**Wanted (orchestrator / maintainer).** (1) Export a named `WriteEngineSource` type from `universal-write.ts`/`index.ts` so the
published `configureEngine` parameter is not `any`. (2) `docs/publishing.md`: the two `cargo build --profile wasm` commands and the
`UO_WRITE_WASM`/`UO_WRITE_EDITOR_WASM` variables before `npm run build:pkg`; `publish-packages.yml` must build both modules before 0.10
(it has no Rust toolchain; today it would warn, or fail with `UO_REQUIRE_ENGINE_WASM=1`). (3) Refresh the README/CHANGELOG lists and
the sizes at the merge (re-run `routes-probe.mjs` and the session build). (4) Decide D5 (two modules, recommended) and SW-9 (fatal at
0.10).

**For the milestone-2 gate.** The first GitHub run of the `editor` job (npm install, build, vitest, the Chromium harnesses on Linux;
the DECLARED tables were measured on Windows with chromium-headless-shell 1228: a platform-only difference must be declared, not
hidden); SW-9 becoming fatal at 0.10; one `build:pkg` baseline from a real checkout with node_modules.

**Process metrics.** Cold wasm builds of three feature sets in `core/target/m2-pkg` (-j 3): about 25 min in total; D: 61 GB free at the
end. Gotchas: a Bash heredoc with backticks and apostrophes died twice (redone with the Write tool plus a splice script); `bash -n` on a
Windows temp path lost its backslashes (the check pipes the script on stdin as bytes instead).

### Wave 6 — L: typing latency of the Rust editor

Goal: bring a typed key of `<universal-write engine="rust">` under the TS fast path WITHOUT changing a result (layout
bit-equal to a full layout, the same pixels, the same saved file). Measured first (the planner's profile, at d4279c16),
then steps 0–8 and, in the review's fix round, step 10 (D-L4, the band repaint) and ED-315 removed. Every change is exact
by construction or checked against the unchanged path after every step (below).

**What a key cost and why (baseline, d4279c16, real clock).** Of 54 ms per typed key on 50 pages, 13–14 ms were the O(i)
`Story::block_at` (`blocks().nth(i)`) called once per LINE by the whole-layout walks (Write section, annotations, run
boxes, navigation, `EditLines::new`) — quadratic, 93% of the 3.1 s key on 500 pages —; the rest was the per-key
navigation/run boxes/annotations/`SectionModel` of the whole layout, the `Layout` clone (ED-2), `EditLines` rebuilt for
every layout, every face file copied out of wasm and hashed again per layout, every visible canvas released and
allocated again, and the incremental relayout itself (~7.7 ms in wasm). Enter/Backspace had lost the paragraph memo since
wave 5 (ED-315's `shell.clear()` on every `formatting` ChangeSet; Enter sets `tx.sections` → `sections.sync` →
`formatting`): a memo-less full layout per Enter (438 ms vs TS 194 ms under load).

#### What changed (per step)

0. **Harness** (`editor_switch.mjs --mode latency`, L's): a REAL clock (its own `BrowserContext` without
   `clock.setFixedTime`: Playwright's clock is per context, so the old second page was still faked), pages50 and pages500
   (`gen_docs` seed 14, 3,100 paragraphs, generated next to pages50 when absent), TS and Rust in the same invocation,
   median/p95 of type (200 / 30 keys), Enter and Backspace (20 / 5), per-stage medians (instance wrappers: caret, paint,
   baseOf, paintItems, decode, faceKey, canvases, proxy; plus the editor's marks), the Enter/Backspace report
   path/reason/memo hits, `memory.buffer` and live bytes per phase. `UO_ES_ROUNDS`, `UO_ES_NTYPE`, `UO_ES_NKEY`,
   `UO_ES_ENGINES`, `UO_ES_NO500`. `shell_native` E accepts `UO_ES_DOC=<path>` (pages500 without the target-dir trick),
   runs the editor's path (`uo_shell_relayout2` with the previous handle) and times the caret of the new layout.
1. **The quadratic block walk** (uo-docx-layout `refs.rs`, additive): `Locator::block/paragraph/story_paragraph` resolve
   step 0 of a body path through the Locator's indexed blocks (the exact output of `Story::blocks`) and the cell steps as
   `DocView::block` does — the same answer by construction. Used per line by `text_layer_paragraph` (SectionModel,
   annotations), `run_boxes_with`, and `EditLines::new` (three lines of C's `hit.rs`, declared below). `Story::block_at`
   in uo-docx unchanged (D-L2).
2. **The memo on Enter/Backspace** (gate R-3, ED-315 superseded): reproduced natively first (`shell_native` F): LA7
   itself sees `modifyStyle`, `updateStyleFromSelection`, their undo/redo and Enter (new `styles`/`formatting`/`sections`
   objects, `formatting_version`): a full relayout, reason `document`, pages byte-equal to `uo_session_layout`; the memo
   then answers 117 hits of 120 paragraphs on the next Enter. The `if (cs.formatting) this.shell?.clear()` of
   `rust-engine.ts` `apply` is REMOVED: the editor never drops the engine's state on a ChangeSet. (The first round kept a
   narrower `dropBase()` because f7b__7__8, f7b__7__10 and f8__7__5 of `editor_switch_objects` "ended with a stale page 0
   without it"; that reading was wrong — with the drop those three still differ in the end pixels of page 0, and front C
   showed the cause (gate-C1: the TS-saved document reopened in both engines differs the same way, a content layout
   difference). Without any drop the three harnesses give the same sets: objects 4,729 equal / 272 different, all
   declared, gate-C1 used for exactly those three; review and stories as before; `editor_switch --mode parity` stride 6
   and stride 1 green. The drop API of the first round — `LayoutEngine::drop_incremental_base`,
   `uo_shell_engine_drop_base`, `ShellEngine.dropBase()` — is removed with it.) Enter/Backspace are now `full/document`
   through the memo every time (339 of 345 paragraphs from the memo on pages50).
3. **Lazy derived data** (uo-wasm `write.rs`, additive `layout_handle_shell`, declared): a shell layout keeps a snapshot of
   the model it was laid out over (`Document` + context story, shallow: `Arc`s and persistent vectors, as LA7's
   `DocParts`) and computes the navigation, the run boxes (links, inlines) and the annotations on first request
   (`OnceCell`), and the Write section PER ENCODED PAGE (`SectionModel::page`, new, additive: the gathering of
   `SectionModel::new` restricted to one page; `wire_page` re-indexes paragraphs per page, so the bytes are the same —
   asserted for every page of the g8 corpus in `tests/g8.rs`). The viewer (`uo_write_layout`) and `uo_session_layout`
   stay eager (`UoLayout::eager`).
4. **No clone of the `Layout`** (D-L1 (a), ED-2 resolved): `LayoutEngine::lend_last / lent / give_back / take_spare`
   (additive) and `FallbackReason::Lent` (appended, wire code 17, `'lent'` in `REASONS`), uo-render
   `RenderView::into_parts / from_parts` + opaque `ViewRest` (additive, declared). New export `uo_shell_relayout2(e, prev,
   …)`: `prev` (the previous handle) gives the engine's result back (its pages out of the render view, the resources, the
   geometry), the engine relays out, the new result is lent to the new handle (`take_pages`, no copy). `prev` is then
   STALE: no pages, no lines, a fresh layout id, every export answers an empty layout's status (asserted: page count 0,
   `encode_page` status 6, caret not ok, navigation empty). A handle whose lend is not the engine's (older, another
   engine's, after `clear`) gives nothing back and keeps its pages; the relayout is then full, reason `lent`. A relayout
   error after the give-back lends the unconsumed layout back to `prev` (`take_spare` + `layout_relend`: the same render
   view mark and id, so the JS decode tables stay valid). **Declared:** when the failed relayout had already consumed the
   pages (the page assembly takes them; `take_spare` answers `None`) `prev` is made stale while it is still the installed
   layout; `rust-engine.ts` `paint` then returns early for a stale layout (`dl.inner.stale`), so the canvases KEEP the
   previous pixels (no blank pages) until the font loop's relayout installs a layout; a page first allocated in that
   window stays blank, the caret and the hit tests answer nothing. No clone is kept for that case (it would be the copy
   this step removes). `uo_shell_relayout` (the copy) stays for the tests and the
   other callers. JS: `ShellEngine.relayout(session, env, { …, prev })` (js/session.mjs) calls relayout2 and flags
   `prev.stale`; `DecodedLayout.freed` is true for a stale layout (every engine read answers `NO_PAGE`/null);
   `rust-engine.ts` `relayoutNow` and the font loop's `adapter.layout` pass the installed layout.
5. **Repaint only pages that changed**: `RelayoutReport::changed_pages` (additive): the relaid pages whose `LayoutPage`
   differs from the previous result's page of the same index — `bits::same_page`: the bulk columns (characters, glyph
   columns, text) by bits, every other field by `Debug` (the LA7 equality; a first `Debug`-only version cost 4.2 ms a key,
   85% of it formatting the character and glyph floats). Appended to the report wire (u32 n + n u32; `report().changed`).
   `install` deletes the page cache only for those. **D-L3 (declared):** with the same canvas sizes the buffers are kept
   (the TS releases every visible buffer per key and allocates them again: ~3 ms of `setTransform`/allocation a key) for
   the pages `ensureVisiblePainted` would allocate again (the others are released, so the same canvases hold pixels as in
   the TS), and those are painted again (overlays included) from the page cache; the TS sequence stays for a size or
   page-count change.
6. **Resources across layouts**: `uo_layout_shared_resources` (additive): per face and image, whether its bytes are the
   previous handle's of the same index (the same `Arc`, compared while `prev`'s `Arc`s are held, so no address is reused);
   `js/session.mjs` attaches it as `layout.shared`; `ViewerModule.adopt(inner, prev)` copies the font keys of the shared
   faces (no copy out of wasm, no FNV again); the viewer's `RustSession.inherit(dl, prev)` (declared, viewer
   `rust-engine.ts`) keeps the decoded images of the shared ids, used only when the new entry asks for the same decode
   (format, SVG, size). faceKey: 3.3 ms → 0.0 ms a key.
7. **Incremental EditLines** (C's `hit.rs`, additive delimited block, declared): `LineIndex::patch` and
   `EditLines::patch_body` — after an INCREMENTAL relayout over the layout the lines were built for (same blocks, same page
   count), only the paragraphs with a line on a changed page (before or after) or whose length changed get their lines
   again, from the same pages in the same order through the same `push_line`; body scope only; a plugin inline in a
   paragraph to redo, another scope or other counts rebuild. `handle.rs` `lines()` (declared; `LinesCache` keeps its
   `LineIndex`) patches when the layout's hint (`EditHint`: the previous handle's id and `changed_pages`, set by
   relayout2) names the cached generation. Checked: `shell_native` H — after each of 40 steps (33 incremental), every
   page's `uo_shell_page_lines` and the caret equal a rebuild.
8. **Engine micro-costs**: (a) `uo_jsnum::format::to_js_string` / `Js` integer fast path (`|x| < 2^53`, integral, `-0` →
   `0`: the integer's digits are exactly `Number::toString`), property test against `ryu_js` over 200,000 integers of
   every magnitude and the edges, plus the V8 capture parity of `tests/format.rs` (D-L6). (b) `paint_line`'s run key
   strings (uo-docx-layout `paint.rs`, declared) only when they can decide: equal components print the same string, so
   the strings are built only when the components differ and BOTH have a `|` in a string component (F5-5) — the same
   answer as building them per glyph.
10. **Band repaint of a changed page (plan step 10, D-L4, declared; fix round 1).** `install` no longer drops the page cache of a
   changed page: it marks it dirty with the previous layout's decoded page (`DecodedLayout.bandSnap`, no decode; null
   when that layout never decoded it). `baseOf` asks the new layout `bandSince(i, snap, hf, slack)` (js/viewer.ts; the
   comparison is the new js/band.ts): the items of the two pages are compared from both ends (structural equality, a
   table index equal only when it is the same index of an entry that paints the same — faces and images also need
   `inner.shared`, i.e. the same bytes); the vertical extent of the items in between is the band (glyph run: baseline
   −2 em … +1 em; rect, line: their geometry ± the line width). It answers `'same'` (nothing to paint), the band, or null
   (the whole page, as before) when: the snapshot is not of the layout this one was adopted after, the page size,
   background or header/footer item count differ, the changed items start inside the header/footer layer or under a
   pushed transform, a changed item is a clip, transform, picture, shape or path or a glyph run with text effects, or
   any picture/shape/path of the page is under a transform, has a picture look, shadow or effects, or reaches into the
   band widened by the margin. `baseOf` also requires that no base hook is on (S's gridlines and header/footer layers:
   their `baseKey` part is empty). The band (+ `BAND_MARGIN_PX` = 8 device px on each side) is painted — background,
   `under` hooks, every item, as in a whole paint — in a scratch canvas ANOTHER 8 px taller on each side, shifted by
   whole rows; only the band rows are copied (`clearRect` + 1:1 `drawImage`) over the cached canvas. Measured on the
   way (probe below): a clip on the page canvas changed the antialiased row where it cut a path on 6 of 76 documents (one
   row, 3–583 bytes); the edge of a band-sized scratch canvas changed the same rows the same way; a 2 px margin was short
   on one more document. The extra rows that are painted but not copied remove both. Exactness check (scratch probe
   `target/m2-latency/probe-band.mjs`, not in the repo): every fixture of the editor switch (75, stride 1) + pages50, at
   three places per document 4 × 2 typed characters, a long insertion, Enter, a character, five Backspaces; after EVERY
   step every cached page base (band-repainted or not) compared byte by byte with the same page painted whole into a
   fresh canvas: 2,700 steps, 6,944 page checks, 1,433 band repaints, 176 'same', 132 whole — **0 differences** (the
   same probe with the band forced off: 0; the clip version: 59, then 23 differing page checks, the cases above). The
   visible canvases are still repainted whole from the cache (clear + `drawImage` + overlays) for the 3–4 allocated
   pages, as in the first round.

#### Numbers

Same machine, real clock, TS and Rust in the same invocation (`editor_switch --mode latency`, count build; the machine was
shared with two other agents, so compare within a column). Median / p95 ms.

| | baseline (d4279c16) TS | baseline Rust | after steps 1–6 Rust | round 1 TS | round 1 Rust | fix round TS (3 rounds) | fix round Rust (3 rounds) |
|---|---|---|---|---|---|---|---|
| pages50 type | 7.4 / 11.6 | 54.3 / 65.7 | 27.3 / 35.4 | 6.6 / 9.2 | 10.1 / 18.2 | 7.2 · 6.4 · 6.2 / 9.9–11.3 | 6.9 · 6.7 · 6.5 / 8.5–8.9 |
| pages50 Enter | 196.5 / 217.8 | 452.6 / 476.2 | 217.3 / 232.6 | 129.0 / 183.6 | 66.1 / 91.2 | 192.6–196.9 / 216–225 | 98.7–101.6 / 117–125 |
| pages50 Backspace | 192.4 / 224.7 | 446.9 / 465.7 | 213.8 / 224.4 | 120.8 / 173.1 | 63.5 / 92.7 | 191.7–196.6 / 227–234 | 98.3–100.7 / 115–136 |
| pages500 type | 28.9 / 31.3 | 3,097 / 3,137 | 51.7 / 60.7 | 23.6 / 36.5 | 16.2 / 22.9 | 26.9–28.1 / 29–46 | 17.7–18.8 / 21–24 |
| pages500 Enter | 1,857 / 2,059 | 7,253 / 7,274 | 2,645 / 2,655 | 1,254 / 1,421 | 1,179 / 1,291 | 1,827–1,856 / 2,007–2,133 | 1,521–1,557 / 1,576–1,594 |
| pages500 Backspace | 1,822 / 2,157 | 7,253 / 7,354 | 2,619 / 2,646 | 1,258 / 1,321 | 1,067 / 1,140 | 1,859–1,901 / 1,902–2,154 | 1,540–1,572 / 1,544–1,645 |

The fix round ran `UO_ES_ROUNDS=3` (round medians separated by dots, p95 as a range). The machine was again shared, so the
TS column moved between runs (round 1: 6.6; the reviewer's run: 7.0; the fix round: 6.2–7.2, and 8.3 in one single-round
run under load where Rust gave 6.7). **Acceptance:** pages500 typing and Enter/Backspace on both documents are below the
TS in every round. **pages50 typing is at the TS fast path, not clearly under it:** Rust 6.5–6.9 vs TS 6.2–7.2 median
(below in round 0, +0.3 ms in rounds 1 and 2), p95 below in every round (8.5–8.9 vs 9.9–11.3). Not claimed as met; what
is left is under "Not done"; accepting the median tie is a maintainer decision, not taken here. Enter/Backspace: path
full (reason `document`: the keys do change the formatting objects), the memo answering 339 of 345 paragraphs (pages50)
and 2,194 of 3,338 (pages500: the memo's 64 MiB wasm cap, LA7 `MemoCaps`), against 0 in the baseline. (Round 1's lower
Enter numbers were a run with a TS of 121–129 ms, i.e. a lighter machine; in the fix round Rust is about half the TS on
pages50 and about 0.83 of it on pages500.)

Per-stage medians of a typed key (ms; marks of `rust-engine.ts` + the harness's instance wrappers, inclusive):

| pages50 | baseline | steps 1–6 | round 1 | fix round (D-L4) |
|---|---|---|---|---|
| exec | 0.2 | 0.2 | 0.1 | 0.1 |
| relayout (engine + give-back/lend + handle) | 27.2 | 14.6 | 4.2 | 3.5–3.7 |
| install + paint | 26.5 | 12.3 | 5.6 | 2.8–3.0 |
| caret (EditLines) | 7.3 | 1.5 | 0.1 | 0.1 |
| decode | 0.9 | 0.5 | 0.4 | 0.3 |
| faceKey | 3.3 | 0.0 | 0.0 | 0.0 |
| paintItems | 6.9 | 1.7 | 1.0 | 0.3 |
| baseOf | 10.4 | 3.4 | 2.6 | 1.1–1.2 |
| canvases | 1.1 | 1.1 | 0.0 | 0.0 |

pages500: relayout 2,150 → 13.5 → 7.1 → 8.6–9.0, install + paint 945 → 36.0 → 8.2 → 8.4–9.0 (`baseOf` 2.4–2.5: 30
keys only, and the first key on a page paints it whole), caret 923 → 20.0 → 0.4 → 0.6.

CDP profile (pages50, 40 keys, a `strip=false` build; ms per key): baseline 45 (planner) with `uo_shell_relayout` 21.3 and
`Blocks::next` + `DocView::block` 13–14; after steps 1–6: 21.9 with `uo_shell_relayout2` 11.2 (of which 4.2 the
`Debug`-only page comparison of step 5, then replaced by `same_page`); final 10.95: `uo_shell_relayout2` 4.32 (the
engine's `incremental` 4.11: `run_from` 3.30 — page 24 is laid out again from its checkpoint although only page 25
changed, the resume rule, untouched —, `Converge::before_block` 0.51, the page comparison 0.31), `paint` 5.15 (`baseOf`
2.63 for one page, R's `paintEngineMarks` + `pageLines` 1.32, `drawImage` 0.80, `setTransform` 0.69). No `Blocks::next`,
run boxes, annotations, `SectionModel::new`, navigation or `EditLines::new` per key.

Native `shell_native` E (release, re-run on the fix round's tree, two runs each; the editor's path, `uo_shell_relayout2`
with the previous handle): pages50 per key 2.74–3.05 ms (baseline 11.3), allocated 10,452,991 B (15.9 MB), peak
952,898 B (7.6 MB); pages500 4.49–4.70 ms (1,158), 13,591,416 B (103.8 MB), peak 3,691,325 B (75.9 MB). The caret of a
new layout as E measures it is the PATCHED one (E asks the previous layout's caret first, so the new layout's hint names
the cached generation and `lines()` patches): 0.05 ms (pages50), 0.25 ms (pages500). Round 1's "rebuilt 1.21 / 24.6 ms"
and "pages500 6.15 ms" came from a run before the final edits and are withdrawn; the rebuilt cost was not re-measured in
release (H checks that the patch equals a rebuild; debug pages50: 0.37 ms patched vs 2.93 ms rebuilt). `bench_relayout` (release): pages50 type 3.17 ms (planner: 7.1 under load), Enter 51 ms with the memo
(329 hits); pages500 type 7.95 ms, Enter 864 ms.

**Memory.** Live bytes / `memory.buffer` after opening: pages50 52.5 / 69.0 MB → 45.1 / 64.7 MB; pages500 447.2 / 666.3 MB
→ 373.4 / 624.2 MB: the handle's copy of the `Layout` is gone (−7.4 MB and −73.8 MB live). The engine's own incremental
state is unchanged (`bench_relayout`: kept / `Layout` 2.43 at 50 pages and 2.46 at 500 without the memo; 3.5–4.1× with it
in the LA7 record): sharing does not change that ratio, it removes the duplicate. Per key: allocated 10.5 / 13.6 MB (was
15.9 / 103.8), peak 0.95 / 3.7 MB (was 7.6 / 75.9). `memory.buffer` still grows while typing (pages50 64.7 → 76.7 MB over
200 keys while live bytes grow 0.9 MB; pages500 624 → 656 MB over the Enter phase): dlmalloc high-water marks (a first
version that let the patched EditLines columns double grew it by 20 MB; their compaction now keeps them at ≤ 1.25× live).
`--mode memory` (fix round too): sessions/engines/handles 0 after each remove, live bytes 16,986,450 at every cycle end
after warm-up (as wave 4), buffer 29,163,520 (one value). D-L4 adds one scratch canvas per editor (page width × band
height + 16 rows, reallocated per band) and keeps the previous layout's decoded page of a dirty cached page until it is
painted (JS heap, no wasm memory); the wasm figures are unchanged. shell_api M4 2,000 (FinalizationRegistry, live back to 12,782,895); shell_native
B (M1) 0 B left, C (M4) 10,000 engines 0 B left. pages500 still ends near 0.6 GB of wasm memory (D-L7: the LA7 retention
decision stays open).

#### Exactness checks

- `shell_native` A (30 seeded steps, every page message byte-equal to `uo_session_layout`), G (the same script through
  `uo_shell_relayout2`: pages equal, the previous handle stale — 0 pages, `encode_page` status 6, no caret —,
  `changed ⊆ relaid`, 20/20 incremental mid-document keys with every face shared, a handle without the lend keeps its
  pages and the relayout is full with reason 17), H (40 steps, 33 incremental: the patched edit lines and the caret equal
  a rebuild), F (R-3: modifyStyle, updateStyleFromSelection, undo/redo: full, reason `document`, pages equal; memo 117/120
  on Enter). `zero_generation` of the test also shrinks the Write section's length prefix with its layout id (two ids of
  different varint lengths — a parallel run passing 16,384 layouts — made A and G fail spuriously).
- `la7` 13/13 with two new checks in every differential step: the lend round trip (lent: no layout, no second lend; given
  back: the same canonical digest) and every relaid page outside `changed_pages` `Debug`-equal to the previous relayout's
  (line ids included); `layout_parity` 5, `g8` 4 (every page's per-page section bytes == the eager ones).
- uo-jsnum `tests/format.rs` 12 (V8 captures) + the fast path against `ryu_js` (200,000 integers and the edges).

#### Declarations (wave 6, L)

- **ED-2 resolved** by `uo_shell_relayout2` (the copy stays in `uo_shell_relayout` for the tests and other callers).
- **ED-315 superseded** (R's two lines in `apply` removed, fix round 1): no ChangeSet drops the engine's state; LA7 sees
  R-3's style edits (test F). Round 1 kept a `dropBase()` and claimed that f7b__7__8, f7b__7__10 and f8__7__5 of
  `editor_switch_objects` ended with a stale page 0 without it and had "0 undeclared differences" with it. That claim was
  wrong: the three differ in the end pixels of page 0 WITH the drop too (272 different facets, all declared since front
  C's gate-C1, whose probe shows a content layout difference of the TS-saved document in both engines, not a stale
  incremental page); round 1's own 01:44 run (before gate-C1) showed 2 undeclared end-pixel differences on each of the
  three with the drop. Without the drop: the same 4,729 / 272 facets and gate-C1 used for exactly those three; stories,
  review, `editor_switch --mode parity` (stride 6 and stride 1) unchanged. The drop API is removed.
- **D-L4 band repaint** (step 10): a changed page's cached base is painted again only in the band where its items differ
  from the previous layout's (the whole page in every case the band cannot bound, listed in step 10). Exact on every
  fixture of the editor switch by the byte-for-byte probe; `editor_switch --mode parity` at stride 1: every facet equal or
  declared by the pre-existing entries (screen pixels 168 equal / 4 declared, thumbnails 1,138 / 12 declared).
- **A stale installed layout is not painted** (step 4): the canvases keep their pixels after a failed relayout that
  consumed the pages, until the font loop installs a layout.
- **FallbackReason::Lent** (wire 17, `'lent'`), **stale handles** (`uo_layout_*`, caret and hit exports answer an empty
  layout's statuses; `DecodedLayout.freed` true; `WriteLayout.stale`), `report().changed` (appended to the 26-byte wire).
- **D-L3**: kept canvas buffers; only the allocated pages outside the `ensureVisiblePainted` window are released (the same
  canvases hold pixels as in the TS — a first version that kept every buffer failed R's revisions screen facet), the kept
  ones painted again with their overlays.
- **Edits outside L's files** (each additive): C's `hit.rs` (three `Locator` lookups in `EditLines::new`, the delimited
  patch block with `compact` and `same_as`), `handle.rs` (`LinesCache.index` / `edited`, the patch in `lines()`,
  `lines_edited()` instead of dropping the cache on a command, undo, redo or job), uo-docx-layout `paint.rs` (the run key
  strings), uo-render `view.rs` (`into_parts`/`from_parts`/`ViewRest`), uo-jsnum `format/mod.rs` (the fast path),
  `write.rs` (`OnceCell` derived data, `ShellPart`, `ModelSnapshot`, `EditHint`, `layout_handle_shell`, the lend helpers,
  `uo_layout_shared_resources`; its import order now passes rustfmt), the viewer `rust-engine.ts` (`inherit`),
  `write-engine.d.ts` / `write.d.ts` / `session.d.ts` / `write-session.d.ts` (optional fields and methods; fix round:
  `BandSnap`, `bandSnap`, `bandSince` in `write-engine.d.ts`, `dropBase` removed from the two session types), the new
  `js/band.ts` (imported by `js/viewer.ts`, so the viewer bundle carries it too; the viewer never calls it).

#### Not done / for the gate

- **pages50 typing is at the TS fast path, not clearly under it** (median 6.5–6.9 vs 6.2–7.2, p95 below). What is left:
  the engine's resumed run of the page before the edit (about 3.3 ms of the 3.5–3.7 ms relayout: the checkpoint rule, not
  relaxed); the repaint of the 3–4 allocated visible canvases from the cache with their overlays (about 1.4 ms: R's
  overlay hook `pageLines` per allocated page, which needs front R's agreement and a declaration (an index of the edit
  lines by page in `shell_review.rs`), and the clear + `drawImage` per page; repainting only the canvases a key changed
  needs the overlay hooks of the three fronts to say which pages they draw on); the band itself (`baseOf` 1.1 ms).
  Whether the median tie is accepted is the maintainer's decision.
- Verification of the fix round (wasm rebuilt after the drop API removal; logs `target/m2-latency/fix1-*.log`):
  `editor_switch_objects --mode all` exit 0 in both runs (4,729 / 272, all declared; round 1's intermittent
  `f7a__7__1|views` did not recur: views 358 / 4 declared). `editor_switch_review` and `editor_switch_stories` exit 0.
  `editor_switch` parity stride 6 (13 documents, no declared entry used) and stride 1 (74 documents: only the
  pre-existing entries gate-O3, gate-10, ED-600, ED-108 used). Latency and memory ok. `viewer_switch --mode parity`
  stride 6 with the viewer build and the session build. shell/write/session API node tests. `shell_native` A–H (8), the
  stories/objects/review natives, `la7` 13, `layout_parity` 5, `g8` 4, uo-jsnum format 12. `check-changed`, rustfmt
  of the touched files, editor and viewer `tsc`, strict `tsc` (band.ts included), editor vitest 329, viewer 25 + 1
  skipped.
- `rust-engine.ts` after round 1's restore from a backup: L checked that front C's edits are in the working tree (the
  `wave 6 (C)` blocks of `onKey` (word moves, Ctrl+Up/Down, Ctrl+Home/End, PageUp/PageDown, Ctrl+C/X), of
  `onMouseDown`, and the collapsed-selection clipboard) and that C's three harnesses pass on this tree. Front C's own
  confirmation is still to be given; L cannot give it.
- `same_page` compares the rest of a page by `Debug` (≈ 0.3 ms per relaid page); the wasm `memory.buffer` high-water
  growth while typing; D-L2 (`Story::block_at` O(i) in uo-docx) still open for the session ops.

### Wave 6 — C: clicks, keys and caret (the remaining parity causes of the editor switch)

Frontier C of wave 6 (plan of the wave's planner, measured at `d4279c16`; builds of `$T/m2-wsession/wave6-c`, which also
hold frontier L's in-progress code). Every cause below was measured first; each closed one is a port of the TS rule, and
its DECLARED entries are removed (not widened). ED numbers of this frontier: ED-600.. (decision D-C1).

**Causes closed**

1. **Gate item 1 (caret/hit x on mixed-size lines), gate item 2 (End on a wrapped line) and most of gate item 10 were one
   cause: the editor never hyphenated** (D-C0). `uo_write_env_register_hyphenation` (uo-wasm `write.rs`) refused the
   patterns with 'patterns in use' (`Arc::get_mut`) once any editor session had laid out on the page: the session's
   `HostFonts` and the shell engine (`ShellEngine::hyph`, shell.rs) keep a clone of the environment's registry for their
   whole life, and the viewer's `prepare` swallowed the error (`catch {}`). Measured: f7a__7__3 paragraph 0, TS line
   `[0, 84]` ending in "câ-mera", the session 82 (End 81). Fix: copy-on-write registration (`Arc::make_mut`; uo-fonts
   `HyphenationRegistry` derives `Clone`, additive: it holds `Arc<Hyphenator>`s): the holders keep the old snapshot until
   their next layout; the shell engine already rebuilds itself when the registry pointer changes, so the relayout after
   a registration is a FULL one (for frontier L's latency numbers: once per language and page). `hit.rs` and `caret.rs`
   were right (their `lineEndOffset` port is faithful: a hyphenated line has no trailing space). Native test A
   (`tests/shell_caret_native.rs`): registration OK, the relayout full, End at 84 (81 before the patterns); M1 of a
   registration + the relayout after it: 21,743,660 B allocated, 0 B left. D-C5: the viewer's `prepare` says a refused
   registration once (`console.warn`) instead of nothing.
2. **The click on an object (the 'ED-216; wave-6' entries, f7b__7__10)**: the first divergence was the parity driver:
   `clickTarget` sets the target's selection in the TS run only (to measure its caret), and a click that lands on a
   shape selects it and keeps the caret in both engines, so the Rust run kept another caret (D-C3). `runStep` now replays
   that `setSelection` before each click in both runs (a no-op in the TS run). What was left behind it was real (point 3).
3. **The keys with an object selected follow the TS keydown order** (universal-write.ts 10516–10746: the Ctrl block —
   `moveWord`, `deleteWord`, `copy`/`cut`, the map —, PageUp/Down, Ctrl+Home/End and Tab come before the `floatSel`
   branch): front O's `onKey` leaves Ctrl+Delete/Backspace and Ctrl+Left/Right to the shell (it deleted or nudged the
   shape); the shell's `deleteWord` extends the selection by the word move instead of the routed `setSelection` (which
   cleared the float selection; the TS assigns `this.selection`); Ctrl+Home/End assign the selection through new
   `Dir::DocStart`/`DocEnd` moves (uo-wasm `caret.rs`, codes 8/9; `js/session.mjs` dir names; no float selection or
   pending props cleared, TS 10606); PageUp/PageDown run with Ctrl too (the TS checks no modifier).
4. **Keyboard copy/cut (D-C2)**: Ctrl+C/Ctrl+X (with or without Shift) prevent the default and run the element's routed
   `copy()`/`cut()` (frontier R's port over `navigator.clipboard`, TS 10577): the session decides what is copied (a
   selected drawing object too) and the system clipboard gets the TS text — CRLF between paragraphs when read back in
   Chromium on Windows: the ED-16 CRLF entries (f7a__7__14, f7c__7__5 step 6, f8__7__5 step 3) are equal. The shell's
   `clip()` (copy/cut events that do not come from the keyboard) no longer returns early on a collapsed selection: the
   session's copy ports the TS early returns and the `floatSel` branch.
5. **gate-O4: Ctrl+ArrowUp/Down** (with Shift) move the caret as the TS `nav` (the Ctrl block handles Left/Right only);
   with an object selected they nudge it by 15 twips (front O's `onKey`, the TS `floatSel` branch). The five gate-O4
   entries of `editor_switch_objects.mjs` are removed.
6. **The text-box click (S ED-102, S-G2, f7d__7__3's ED-216 entry; D-C4)**: uo-docx `EditContext::Textbox` addresses the
   box by an opaque path into the outer story (`TextboxAt(Arc<[loc::Step]>)`: cells, group items, boxes in boxes;
   `top_level()`, `Display` for scope keys) instead of `(para, run)`; `story_of`/`story_of_mut` walk it with
   `loc::run_at(_mut)`; the new `ContextCmd::EnterTextboxAt { focus, page }` is the TS `enterTextbox(focusRun)` by the
   clicked box's order in `contextTextboxRuns` (the chain's head is resolved as `reenter_textbox` does, whose `top_level`
   restriction is gone, so undo re-enters nested boxes as the TS `te.h/te.f`); the old `EnterTextbox { para, run, … }`
   stays (wire-compatible). Public read accessor `context::textbox_of(doc, ctx)`. Schema: `ContextCmdEnterTextboxAt` and
   the enum variant appended (frontier R owned the schema in wave 5; codecs regenerated). `uo_shell_textbox_at` takes the
   TS rule: a placed object with content, `hf` of the context, whose box is among the outer story's boxes (the TS
   `mine`, by drawing), instead of top-level runs of the outer story only; it answers `focus`/`host` and `para`/`run`
   only for a top-level head (null otherwise; the context JSON carries `at`). Front S's `enterTextbox` sends
   `enterTextboxAt`; front O's `onPointer` drops its first-cut fallback (a content-bearing float the export did not
   answer); the shell's `onMouseDown` runs the fronts' pointer hooks before it requires a text hit (the TS runs the
   objects and the text-box entry before its `hitTest`, 10148–10209; the hooks get `hit: Pos | null`, R's comment hook
   guards it). The session's font-usage replay (uo-docx `session_font_usage`) now opens the 11 boxes anchored in table
   cells it could not (the pinned count 11 → 0, the replay still 1,401 equal, 0 mismatches).
7. **S-G3, linked boxes**: the shell's `textbox_chain` (the edit lines of a box in edit) is the TS `textboxChain` over
   the outer story in `forEachTextbox` order — group items included (f7d__7__10: 5 members, the first cut found 4).
   The f7d__7__10 chain of story 2 still has no caret at its start: measured in the frozen TS (scratch probe entering
   every box by order), the TS has none either (the head's first paragraph is laid out in no placed box; the fourth
   member is a group item without content), so it is parity, not a gap.
8. **S-G1**: the cause was not the layout: uo-docx `insert_table` entered the new table's first cell before
   `touch_structure`, which asks for `syncSections` only in the body context, so the sections kept their old block
   ranges and the paragraph after the table was outside every section of the live layouts (also in the body and after
   a full layout; another body edit synced it — measured natively). The TS builds the sections from the body at every
   layout, also during a cell edit: `insert_table` (a body table) syncs them right after the automatic caption (D-C6:
   a uo-docx fix, no layout crate change). Native test D; `shell_stories_native` C asserts the line now; the stories
   harness' T1 entry is removed.
9. **BUG-1**: `uo_session_selection_rects` draws `normalize_range(range)` (the TS `paintEngineSelection`); native test B
   (f7d__7__9 paragraph 6: 106 → 10 answers the rectangles of 10 → 106).

**Declarations (outside the TS semantics or TS defects)**
- **ED-600** (a frozen-TS defect, reference only, not reproduced; D-C7): typing in the anchor paragraph of a selected
  shape leaves the TS `floatSel` on a run the model no longer holds (traced: `selectedObjectCount()` 1 and
  `selectedFloat()` null from the typing on), so Delete misses the `floatSel` branch and deletes a character (one
  `uw-change`); the session keeps the shape selected and deletes it (two). f7d__7__11 from step 11.
- **ED-601** the shell keeps its `copy`/`cut` clipboard-event listeners (the TS has none: a copy event that is not the
  keyboard's does the browser's default there); the keyboard path, the only one the harnesses drive, is the TS's.
- **ED-602** the chain of a box in a header/footer is gathered over the part's story with its tables; the TS
  `storyBlocksOf` gives the part's paragraphs only (a linked box inside a header table: not in the corpus).

**What is still declared (each with its cause)**
- `editor_switch.mjs` (5 of the 18 of HEAD): f2-3-3 (**gate-O3**, measured by the end screens: the Strong runs are orange
  in the TS and red in the session — `w:color C00000` with `themeColor accent2` and no theme part; not hyphenation);
  f7c__7__1 and f7c__7__5 (**gate-10**, end thumbnails only, the saved bytes equal: the TS-saved document reopened in both
  engines (`editor_switch_objects --mode probe`) differs as well, and a full relayout before the end changes nothing — a
  layout difference of the content, not the editing path); f7d__7__11 (**ED-600**); f8__7__5 (**ED-108**, the TS save
  inside a text box).
- `editor_switch_stories.mjs`: X1 ×3, X2, X3, X4 (**ED-106**, the TS paints the handles of the box in edit: front O's
  paint, not the click) and X2's **ED-108**.
- `editor_switch_objects.mjs`: O's ED-202, ED-209, ED-213, ED-219, gate-O1, gate-O2, gate-O3; new **gate-C1** for
  f7b__7__8, f7b__7__10 and f8__7__5 (the end pixels of page 0 only, every step facet and `word/document.xml` equal; the
  TS-saved document reopened in both engines differs the same way: a layout difference of the content after the drags
  and nudges, exposed once gate-O4 was fixed — the gate-O4 entries had covered the end pixels); **gate-O5** stays: seen
  in every run now (2 of 2, f7b__7__3 objects and f7a__7__1 views), and a full relayout (incremental state dropped) right
  before the end changes nothing, so it is not the late-face race of an incremental relayout the record assumed: the
  bold table runs of f7b__7__3 page 0 are narrower in the session after the edits (cause not isolated; the step-8 rule of
  the plan — a full relayout on `onFontsResolved` — would not change it, so it was not made).

**Files touched outside the frontier's own files (each declared)**: uo-fonts `hyphen/registry.rs` (`Clone`); uo-wasm
`write.rs` (the registration, three lines; the file's pre-existing rustfmt diff untouched); uo-docx `session/ops/context.rs`,
`drawing/textbox.rs`, `tables.rs` and `tests/session_font_usage.rs`; `schema/session.uos` and the generated codecs; uo-wasm
`session/layout.rs` (frontier L's file: the scope key's one line, `t{at}`); `js/session.mjs`/`session.d.ts` (the two dir
names), `js/session-stories.d.ts` and `write/packages/editor/src/write-session*.d.ts` (nullable `para`/`run`, `at`);
`rust-engine-review.ts` (one guard line); `write/packages/viewer/src/rust-engine.ts` (D-C5); the parity driver of
`editor_switch.mjs` (the click preset, D-C3) and the DECLARED tables of the three harnesses.

**Numbers** (2026-11-12; builds of `$T/m2-wsession/wave6-c`)
- Native `tests/shell_caret_native.rs` (new, 4 tests, ~90 s): A (End 81 → 84 after the registration; M1 of a
  registration + relayout: 21,743,660 B allocated, 0 B left), B, C (37 boxes reached by the shell's `textbox_at` over the
  f7b/f7d/f8 fixtures entered by order, typed into and re-entered by undo: chains whose head is a group item — f7d__7__3
  `p0.2/i0`, f7d__7__4 `p2.2/i0` — and linked boxes; the two without a caret are the TS's too), D (S-G1). `shell_native`
  8, `shell_stories_native` 8, `shell_objects_native` 4: pass.
- uo-docx (`--test all`, the session/parity/no-panic/memory modules of d2a, f3, f4, f7, f8, f10 and `session_font_usage`):
  128 passed (font usage: 160 documents, 1,401 equal, 0 mismatches, 0 boxes not opened — was 11).
- wasm: `session_api` 5 pass; `session_replay` d2a/f3/f7/f10 0 mismatches (49,800 / 40,390 / 27,207 / 76,346 facets
  equal); `shell_api` 2 pass; `write_api` 8 pass.
- Chromium: `editor_switch --mode parity` (stride 1, 12 steps): 74 fixtures, selection 888/888, caret 888/888, clipboard
  888/888, every difference declared by 5 entries (18 at HEAD), 0 page errors; `--mode all` ok; `editor_switch_stories
  --mode all` ok (T1's S-G1 entry gone); `editor_switch_objects --mode all` ok after the declarations above (three full
  runs); `editor_switch_review --mode all` ok; `viewer_switch --mode all` (stride 1, the write.rs change) 74 documents,
  579 pages, 0 differences.
- Editor vitest 33 files / 329 tests, viewer 6 files / 25 + 1 skipped; editor and viewer tsc exit 0; strict core tsc 0;
  `uo-schema check` 20 files up to date; `check-changed` all steps passed; rustfmt clean on the touched files of this
  frontier (`write.rs` and the lines of frontier L in `handle.rs` not formatted).

**Process metrics**: one implementer (Opus) on the planner's measured plan; three measurement detours, each a scratch
probe outside git (the stale-`floatSel` trace of ED-600, the TS caret of every box of f7d__7__10 for S-G3, the reopen
probes of f7c__7__1/5 and gate-C1); the plan's step 8 (a full relayout on `onFontsResolved`) not made, measured as not
the cause. Two harness runs were lost to frontier L's in-progress `js/session.mjs` asking for a newer wasm export
(rebuilt from the shared tree).

### Wave 7 — A2: lists, paste, tables, objects and the page

Frontier A2 of wave 7 (the maintainer: "do everything that is missing"): the A2 families of the README's "What stays
disabled" list (lists, paste, tables, objects and the page), routed to the wasm session, 1:1 with the frozen TS
(`clickcompliance-docx` d0f45bc5). Plan: `core/target/m2-latency/a2-plan` (outside git). Front name `completion`. Two
implementer sessions: the first routed the families and wrote a first record (blockers: the previews, the text box gallery
save, the OLE edit, the cross-session Paste Link, ED-202/204/208/213/215, the picture bullet); the second ("tente
novamente") closed every one of them. This section replaces the first record.

**Routing result** (`node core/target/m2-latency/a2-plan/routes-probe.mjs`, all routing tables of the tree): public methods
511 — routed 494, shell 26, **disabled mutators 0, disabled queries 0** (8bf0d483: 65 + 41). Of the plan's 51 A2 names: 49
routed by A2, `ensureBuiltinStyle` and `paintTocTemplate` by A1. A2 also routed the two queries no frontier had
(`headerFooterTemplateXml`, `sectPrAt`) and front S's `paintHeaderFooterTemplate` (decision 5, ED-106 closed).

**Routes** (`engine-routes-completion.ts`, handlers `rust-engine-completion.ts` + `rust-completion-{core,lists,paste,tables,
objects,previews}.ts`; one additive line each in `engine-routes.ts` FRONTS and `rust-engine.ts` installRoutes):
- public (49): `applyListLibrary`, `applyMultilevelLibrary`, `applyListLevels`, `applyListStyle`, `createListStyle`,
  `modifyListStyle`, `defineListLevel` (a picture bullet too), `defineMultilevelList`, `setListLevel`, `restartNumbering`,
  `continueNumbering`, `setNumberingValue`; `listInfo`, `documentLists`, `listStylesInDocument`, `numberingValueInfo`,
  `listLibrary`, `markerPreview`; `pasteCells`, `hasCellsClip`, `pasteData`, `changeImageFromClipboard`; `insertQuickTable`,
  `quickTables`, `insertSpreadsheet`, `canEmbedSpreadsheet`, `editEmbeddedObject`, `setTableTool`, `tableStyleList`,
  `tableStyleXml`, `tableStyleBases`, `newTableStyleId`, `saveTableStyle`, `renderTableStyleXmlThumb`;
  `startEditWrapPoints`, `endEditWrapPoints`, `editingWrapPoints`, `startTransparentColorPick`, `editSelectedTextbox`,
  `textboxTemplates`, `saveSelectionToTextboxGallery`, `removeTextboxGalleryItem`, `setSelectObjectsMode`,
  `isSelectObjectsMode`, `pageFillOptions`, `shapeGradientVariations`, `resolveDmlColor`; `paintCoverTemplate`,
  `paintWatermarkTemplate`, `headerFooterTemplateXml`, `sectPrAt`.
- handlers of earlier fronts changed in A2-owned parts: S's `getTableTool`, `tableStyleListNow`, `renderTableStyleThumb`
  (the session's mini-document) and `paintHeaderFooterTemplate`; O's `insertImage` (`ole`, size), `insertTextbox('user:…')`
  (the stored drawing as `GalleryBox`), `selectedShape` (the shape parts, ED-213), `setWatermark` (`measuredBox`, ED-208),
  the pointer module (drag commits, inline drag, crop drag, the hover order); R's `clipboardItems` (ring kinds) and
  `pasteSpecial` (cross-editor Paste Link): one additive call each (decision 3).
- bar ids (all 38 of the plan): `list:defineBullet`, `list:defineNumber`, `list:setValue`, `list:defineMultilevel`,
  `list:defineStyle`, `list:indents`, `list:restart`, `list:continue`, `paste:keep`, `paste:merge`, `paste:picture`,
  `paste:text`, `paste:cells:overwrite|rows|nest|text`, `table:excel`, `table:style:new`, `table:style:modify`,
  `table:painter`, `table:draw`, `table:eraser`, `dlg:pageBorders`, `pic:wrap:editPoints`, `pic:transparentColor`,
  `pic:change:clip`, `shp:editText`, `select:objects`, `toc:level0`…`toc:level9`.

**Through the TS shell code** (`through()`: the TS body runs on the element and only reads the shell's state or calls routed
methods): `markerPreview`, `changeImageFromClipboard`, `quickTables`, `canEmbedSpreadsheet`, `setTableTool`/`getTableTool`,
`setSelectObjectsMode`/`isSelectObjectsMode`, `textboxTemplates`/`removeTextboxGalleryItem` (and `userTextboxes`/
`storeUserTextboxes` for the gallery save), `pageFillOptions`, `shapeGradientVariations`, `resolveDmlColor`.

**uo-docx** (additive; one `mod` line each in `ops/mod.rs` and `tables.rs`):
- `session/ops/completion.rs`: the read-only answers (lists, table style gallery/XML/bases/id, `textbox_gallery_item`,
  `ole_at`, `sect_pr_json` = `JSON.stringify` of the body section's `SectPr` through `JsonWriter`, `selected_shape_parts_json`
  = the `fill`/`style`/`textbox`/`body`/`story`/`normalEastAsianFlow` of `readShapeFormat`).
- `session/ops/previews.rs`: `PreviewKind` {`Cover`, `Watermark`, `TableStyleXml`, `TableStyleName`, `RenderClip`,
  `HeaderFooter`} and `Session::preview_document` (the TS `paintPreviewPage` 12142, `paintStyleThumb` 20178,
  `renderClipPng` 5641, `paintHeaderFooterTemplate` 18521 mini-documents; the session is not changed).
- `session/ops/shell_edits.rs`: `ShellEdit` + `Session::exec_shell_edit` (decision 1: outside `Command`; Busy while
  pending, one undo step each): `FilledTable` (insertQuickTable 20394), `Table(TableGesture)` (Edge/Move/Size/Paint/Erase/
  DrawSplit/DrawNew, 20646–21000), `SetWrapPolygon` (14280), `MoveSelectedBy` (9256), `DeleteSelectedObjects` (9266),
  `ReplaceOle` (editEmbeddedObject 20345), `MoveInlineRun` (moveInlineRun 13972, ED-204: the drop caret is the step's
  snapshot selection, as the TS caret follows the pointer), `CommitDrag(DragCommit)` (commitPictureDrag 13853,
  commitDrawingDrag 13881, the Crop drag's mouseup 14152; ED-202/215: rotation, size with a relative size made absolute,
  source rectangle, `posH`/`posV` by `pos_for` in the same references, written through `apply_drawing_xml(…, true)` = the
  effect extent recomputed), `DefineListLevelPicture` (defineListLevel 4010 with `o.picture`).
- `session/ops/tables/completion.rs`: the table gestures; `lists.rs`: `define_list_level_with` (the picture variant; the
  old entry calls it with `None`), `add_pic_bullet` over `lists/store.rs` `add_pic_bullet` (numbering-store.ts 90),
  `F2State.fresh_images` and `write_pic_bullet_images` at save (numbering-store.ts `writeImages` 115: `word/media/
  uwbulletN.ext`, the part's relationship, the content type). Visibility only: `previous_list`/`parse_level`,
  `OFFICE_THEME`/`ensure_table_style`, `move_float_by` → `pub(crate)`.

**uo-wasm** `src/session/shell_completion.rs` (+ one line in `session.rs`) and the wave-7 block of `shell_stories.rs`:
`uo_shell_list_info`, `_document_lists`, `_list_styles`, `_numbering_value`, `_list_library`, `_table_styles`,
`_table_style_xml`, `_table_style_bases`, `_new_table_style_id`, `_table_grid`, `_has_cells_clip`, `_clipboard_kinds`,
`_edit` (a `ShellEdit` as JSON: `filledTable`, `edge`, `move`, `size`, `paint`, `erase`, `drawSplit`, `drawNew`,
`wrapPolygon`, `moveSelected`, `deleteSelected`, `moveInline`, `commitDrag`), `_preview` (kind 0–5, an owned layout
handle laid out with the editor's options without revisions; the pipeline is a declared duplicate of A1's
`uo_shell_fmt_toc_template`), `_render_clip_margins`, `_textbox_gallery_item`, `_ole_at`, `_ole_data`, `_replace_ole`,
`_hf_template_xml`, `_sect_pr_at`, `_link_origin`, `_shape_parts`, `_watermark_text_box` (watermarkTextBox 11901 over the
session's fonts: the sfnt advance at the 600-dpi em, `(usWinAscent + usWinDescent) / unitsPerEm`, `explicit_text_box`),
`_define_list_level_picture`; `uo_shell_page_cells`, `uo_shell_table_rows`. JS `js/session-completion.mjs` + `.d.ts`
(`bindCompletion`), re-exported by `session.mjs`/`session.d.ts`, drift line in `editor.check.ts`; editor ambient
`write-session-completion.d.ts`.

**Shell**: `rust-completion-previews.ts` lays the preview out (`uo_shell_preview`) and paints page 0 with the TS crop and
scale (covers, watermarks, table style thumbnails and the dialog's preview, header/footer thumbnails) and runs the
session's `renderClip` job (Paste as Picture: the PNG at 2 px per 96-dpi pixel). The pointer: `a2Hooks` (rust-objects-
pointer.ts) — the pick, table tools/edges/handles and Select Objects before the objects (TS mousedown 10171–10180), the wrap
points after Draw/Link; the hover now in the TS mousemove order (10437: wrap points, crop, handles, floats, THEN the tables —
the table hover only changes with no object under the pointer; closes `f7b__7__0|menu` ED-219b). The dblclick on an
embedded spreadsheet runs before the other double clicks and selects the picture first (the TS picture stays selected
through the double click). Paste Link from another editor: a page registry of the Rust editors (`WeakRef`), the source by
its clip identity (`uo_shell_link_origin`), `AddLinkSource` on the source's session, `linkSourceName`, `PasteLinkWith` here.

**Declared** (A2):
1. `pasteData(d)` ignores a TS `d.rich` (a TS-model RichClip only exists in a TS-engine element); text, HTML and image go
   to `PasteData`.
2. A page mixing TS and Rust editors does not share the Office Clipboard (one ring per wasm instance: Rust editors share
   it, and Paste Link between two Rust editors works).
3. (open parity item, see "Wave 7 — merge state") The wrap-point drag shows the polygon live; the TS also relays the
   text out live during the drag (one step in both).
4. (open parity item, see "Wave 7 — merge state") The inline picture drag does not paint the drop caret during the
   drag (the TS moves its caret with the pointer; the session's selection is not changed before the drop); the end
   state, the undo step and its selection are the TS's.
5. `editEmbeddedObject(run)` takes the body position `{ para, offset }` of the run (the dblclick's `oleRunAt` hit) or the
   caret: a TS `Run` object does not exist in a Rust editor.
6. The harness's `<universal-sheet>` is a deterministic stub registered on both engines' pages (decision 6); the real
   Sheet editor in the dialog is checked by hand only.
7. (open, not measured, see "Wave 7 — merge state") Session rule seen by the native test, not A2's (plain
   `InsertTable` has it too): a `save_docx` between a table insert and its undo commits the pending table style (`Table
   Grid`) to the styles part outside the history, so the undo leaves it there. The tests compare without a save in
   between; whether the TS `saveDocx` does the same was not measured (for the milestone gate).

**Numbers** (D: free 47 GB at the start of the wave, 28–36 GB during this session with the other frontiers):
- uo-docx `completion_a2::` **10/10** (lists and table styles as the TS; every shell edit one undo step giving the package
  back; Draw Table two steps; the inline move (no step on itself, one step, the picture selected); the drag commits
  (`cx`/`cy` 571500×444500, `rot` 1800000, `a:srcRect l=10000 … b=20000`); the picture bullet (`w:numPicBullet` 0 with
  `o:title`, `w:lvlPicBulletId`, the Symbol character in reserve, `rIdUwPb1` → `media/uwbullet1.png`, the image in the
  layout's numbering); the queries (`sectPrAt`, the hf template XML, the previews of header/footer/current position, the
  shape parts, the gallery item); M1 0 B left).
- uo-wasm native `shell_completion_native` **5/5**: A lists, B table styles, C every edit through `uo_shell_edit` (one
  undo restores the package, redo changes it), D M1 13,007,399 B allocated, peak 917,960 B, 0 B left; E the second leva —
  every preview kind lays a page out (6/6), the queries' statuses, `watermarkTextBox` `{widthPt 412, heightPt 80}` for
  RASCUNHO 72 pt Liberation Sans, the picture bullet's media in the saved package; M1 6,220,203 B allocated, peak 644,011 B,
  0 B left.
- Native regressions: `shell_objects_native` 4/4, `shell_stories_native` 8/8, `shell_native` 8/8, `shell_caret_native`
  4/4, `shell_review_native` 3/3, `session_native` (selftest) 11/11.
- Wasm API: `session_api.test.mjs` 5/5 (M4 10,000 sessions, live bytes back exactly; M2 flat), `session_replay.test.mjs`
  (d2a,f1–f3,f5–f10) 10/10, `shell_api.test.mjs` (M4 2000) 2/2.
- Editor vitest 34 files / **334 tests green** (engine-completion 5/5: the 51-name inventory with no blocker left, the 38
  bar ids routed and reaching only routed or shell methods); viewer 28 passed, 1 skipped. Editor/viewer tsc: exit 0, no
  output. `check-changed.mjs`: it stopped on the Rust 1.100 clippy lints of files no frontier touched (the toolchain updated
  itself during the wave, see fix round 1 below); fixed in code at the merge (F3, "Wave 7 — merge state").
- `editor_switch_completion.mjs`, final (fix round 1's build; the per-mode numbers are under "Numbers of the fix round"
  below, the first session's per-mode runs are superseded): every cover (8), watermark (12), TOC (3), header/footer (all
  galleries) and table-style thumbnail (24 gallery + 3 dialog previews) **pixel-identical** in both engines; page
  (`sectPrAt`, `headerFooterTemplateXml`, the explicit-size watermarks 337×88 and 156×60 pt) equal; Paste as Picture and the
  cross-editor Paste Link `OLE_LINK1` equal. What the first session had left open, all closed in fix round 1:
  - `markerPreview` css (ED-C1: `"Carlito", "Liberation Sans"` in the TS, `"Liberation Sans"` in the Rust editor): the
    shell's TS font registry now gets the Rust engine's families before the TS body runs (fix round 1, below).
  - blank|objects end thumbnail — **corrected and fixed in the fix round** (the first session read it as a line break): the
    saved parts, the screen and the picture item (`x 2676, y 2107, w 4575, h 3075`, crop `l 0.07576 t 0.06818`, `pic.clrChange`,
    the same 300×200 ImageBitmap; `UO_EC_ITEMS=1`) are equal; only the resampled edges of the recoloured picture differ (2,301
    px in y 94–294, x 113–413). Cause: the picture painter's layer cache. The TS keys it on the image OBJECT, which survives a
    relayout, so the thumbnail reuses the layer built at the screen's resolution (ppt 1.3333; the thumbnail's 1.3338 rounds to
    the same TS key); the Rust cache hung from the layout, so the relayout of `end()`'s `refresh()` dropped it and the thumbnail
    built a fresh layer at 1.3338. Painting at 780 px (another key in both) gave the same hash in both engines.
  - f2-3-0|tables step 24 (`tbl:move` handledrag 2×1 px) — **fixed in the fix round** (`edit`/`applyNoScroll` of
    rust-completion-core.ts): the table gestures, drag commits, wrap points, multi-selection and OLE edit no longer scroll the
    caret into view (the TS paths only `refresh()`; only Quick Tables and Draw Table's new table go through `insertTable`,
    which scrolls). The shell's scroll after the step-23 size drag had moved the page under the move handle's pointer.
  - What differs at the end is the ED-304 field shading of the footer only (f2-3-0, f2-3-1 and f7c__7__2 lists; f2-3-0 paste,
    inline, previews and tables), declared in DECLARED_C as `FIELD_SHADING` — not an A2 path.
- Front regressions with the fix-round build: `editor_switch_review.mjs`, `editor_switch_stories.mjs` and
  `editor_switch_objects.mjs --mode all` ok; ED-219b (`f7b__7__8|objects`, `f7b__7__0|menu`, the hovered table's handles)
  closed by the hover order and the draw-mode hover of fix round 1, and B retired both entries. `editor_switch.mjs --mode
  all`: its chrome probe used the Format Painter as the example of a non-routed command, and A1 routes it now; at the merge
  F3 replaced the example with a synthetic command registered only for the probe (disabled and inert under
  `engine="rust"`, enabled and running with the TS engine), so the fail-closed check stays real.
- `editor_switch.mjs --mode parity` (session.wasm, stride 1, 12 steps): ok, 0 undeclared.
- uo-docx `--release --test all`: **795 passed, 0 failed**, 1 ignored (fix round 1). The first session's 791 / 2 (the two
  500-page budgets `memory_d2a::m3_per_keystroke…` and `memory_d2b_f1::m3_per_command…`, 1.6 MB per command) came from
  B's per-settle `sync_bookmarks`, which B moved to the layout pass before the fix round.

**DECLARED entries** (the tables are B's, A2 did not edit them): the entries the first session found stale were retired by B
(`editor_switch_objects.mjs` `f7b__7__8|objects` and `f7b__7__0|menu`, ED-219b; DECLARED_C's `MARKER_CSS`, ED-C1). The
one entry A2 asked for, `'f2-3-0|tables': FIELD_SHADING` (the ED-304 footer band: 473 px at x 338–403, y 1046–1064 on both
pages, thumbnails equal; dumps `es-c/dump/f2-3-0-tables-p{0,1}-{ts,rust}.png`), was added at the merge (F3).

**What A2 leaves disabled**: nothing — no public method and no bar command of the A2 families is disabled or a no-op under
`engine="rust"`. Open parity items: none of A2's (ED-C1, ED-219b, f2-3-0 tables step 24 and the blank|objects thumbnail
closed in the fix round).

**Fix round 1** (reviewer findings: the harness not passing, no policy guard on the shell edits, ED-219b, ED-C1, the
blank|objects attribution, the fail-closed probe, the harness coverage). Two sessions (the first was cut off by a usage
limit after the guard, the step-24 scroll and part of the investigation; the second checked and finished them).
- **Formatting policy on the shell edits** (major): `ShellEdit::guard()` answers the guarded TS method the edit reaches —
  `insertTable` for `FilledTable` (insertQuickTable calls the guarded `this.insertTable`) and `Table(DrawNew)` (finishDraw's
  `insertTable(1, 1, …)`), `defineListLevel` for `DefineListLevelPicture`; `exec_shell_edit` refuses as `exec` does (no
  effect, `Event::Blocked`, the change set). The other shell edits reach no `POLICY_GUARDS` method in the TS (table gestures
  through `editTableAt`; drag commits through `applyPictureXml`/`applyDrawingXml`; wrap points through `applyPictureXml`;
  multi-selection through `moveFloatBy`/`deleteSelectedFloat`; the OLE edit and inline move write the model; `pasteCells`
  through `editTable`/`pasteText`; `changeImageFromClipboard` → the routed `changeImage`). Test
  `completion_a2::shell_edits_follow_the_formatting_policy` (each variant a no-op raising `Blocked` under a policy without
  tables/lists; a move-handle gesture runs under the same policy; all run without it).
- **f2-3-0|tables step 24**: see the corrected bullet above (no caret scroll after the table gestures).
- **blank|objects thumbnail**: see the corrected bullet above. Fix: `PaintCaches.inherit(from, to, shared)` (uo-wasm
  js/paint.ts) carries the picture layers and adjusted bitmaps of the images a relayout shares (`sharedImage`) to the new
  layout, called by `DecodedLayout.inheritPaint(prev)` (js/viewer.ts, declared in write-engine.d.ts) from the viewer's
  `RustSession.inherit` (wave 6's decoded-image inheritance) — the TS lifetime of the cache (the image object).
- **ED-219b on f7b__7__8** (the hovered table's adorners after Draw Text Box): the TS `mousemove` gate is `floatDrag` only
  (10400), so the hover (and `tableMouseMove`) runs in the Draw Text Box / Create Link modes and through the draw drag (its
  ghost is a DOM div in the TS); the shell's `onHover` returned on `o.mode` and on the draw ghost `o.drag`. Now it returns
  only for a drag that is not the draw ghost.
- **ED-C1 `markerPreview` css**: before the TS body runs, the shell's TS font registry gets the families the Rust engine
  asked for (`applyResolvedFont` over `RustSession.pushedFamilies()` — new, the page environment's pushed names — plus the
  layout's faces), as the TS layout registers every family it asks for (f2-3-0: TS Aptos, Calibri, Arial, Times New Roman,
  Liberation Serif, Wingdings, Cambria; the layout's faces alone were only Liberation Serif). Declared: the pushed names are
  the page environment's (shared by the Rust editors of a page), so a family another Rust editor of the page asked for is
  in the registry too; the TS registry is per element.
- **Selection bug found by the new coverage** (front O's model, fixed here): `settle_float_sel` kept an inline picture of the
  body selected after `insertTable` + `selectTable` (the path `[Para 0, run 0]` checked against the new cell's empty
  paragraph answered "over": 0 = 0 + 0). It now requires the run to exist and be an inline picture of the edit context (the
  TS compares the run object, `picSel`).
- **Debug lines removed**: the two `A2TRACE` pointer-trace lines of the first session (rust-completion-tables.ts,
  rust-objects-pointer.ts); the harness's `window.__a2trace` read is optional.
- **engine-switch.test.ts**: the fail-closed path is probed with two SYNTHETIC public mutators on the prototype (one sync,
  one async) that no routing table classifies: a second element started with them counts 2 disabled mutators, answers
  `undefined` / a promise of `undefined`, records `disabled:<name>`, never runs the TS body, and `model()` stays null; the
  real list is asserted to have no unclassified mutator.
- **Harness coverage** (`editor_switch_completion.mjs`): paste — the Office Clipboard's IMAGE item (a picture copied and
  pasted from the ring with `pasteClipboardItem`), Change Picture › From Clipboard (`pic:change:clip` over a PNG written to
  the system clipboard), all in the body before the cell steps; the Paste Options re-paste (`repaste` text/merge/keep after
  `pasteData`); objects — an existing text box: border click (shape), `shp:editText`, Esc back to the shape, Esc; the memory
  cycle now has two editors per cycle: a blank one with real mouse drags (the inline picture's MoveInlineRun, undo, Wrap
  Square, the float's CommitDrag) removed and counted, then the document's with lists, Quick Table, cell paste, table-style
  queries and the preview paints (two covers, a watermark, a table-style thumbnail). `UO_EC_ITEMS=1` prints the image items
  of page 0 of each engine's layout (the thumbnail measurement).
- **Seen, not A2's** (for the merge): `el.newDocument()` right after `load` leaks one layout handle intermittently under
  `engine="rust"` (memory cycle variant "make → load f7b__7__1 → newDocument → insertImage → save → remove": `handles` 1
  after remove in some cycles; without the newDocument 0 in every cycle) — the adopt/font-loop path of rust-engine.ts. The
  memory cycle avoids it (a separate blank editor). **Closed by F1** ("Wave 7 — F1" below): `adopt` did not release the
  installed layout of a `refresh()`; the memory cycle now does load → newDocument again. And the toolchain moved to rustc/clippy 1.100.0 (2026-11-09): clippy
  `-D warnings` now fails on files nobody touched (`chunks_exact_to_as_chunks` in uo-codec reader.rs 172, uo-testkit
  sha256.rs 56/77/93, uo-opc cfb.rs 276/282 and flate.rs 887; `needless_range_loop` uo-docx doc/props.rs 1433;
  `manual_swap` uo-docx js/object.rs 201), so `check-changed.mjs` stops there; with those three lints allowed, clippy native
  and wasm32 over the four changed crates is clean. (Fixed in code at the merge, F3: no lint is allowed.)
- **Numbers of the fix round** (session_count.wasm rebuilt with the guard and the selection fix; CARGO_TARGET_DIR
  m2-latency): `editor_switch_completion.mjs --mode all` 445 s, **6 failures, all B's table**: the four stale MARKER_CSS
  entries (5 lines: `blank|lists` reported twice) and the undeclared ED-304 band of `f2-3-0|tables`; with DECLARED_C as
  proposed above (a scratch copy, the repository file untouched) **`ok`** — lists 1,460 equal / 3 ED-304, paste 1,068 / 1,
  tables 1,003 / 1, objects 1,971 / 0, inline 254 / 1, page 134 / 0, previews 199 / 1, memory 30 cycles: 0 sessions,
  engines and handles after each remove (both editors of the cycle), live bytes flat at 13,139,221 (15,383,165 in the
  second run), `memory.buffer` constant. `editor_switch_objects.mjs --mode all`: only the stale `f7b__7__8|objects`
  (2 lines). `editor_switch_stories.mjs` ok, `editor_switch_review.mjs` ok. `editor_switch.mjs --mode all`: the 2 known
  `chrome compact|ribbon … {"id":"painter"}` probe failures (B's harness). (Both harness items were resolved at the merge by F3: the
  DECLARED_C entry above and the synthetic probe command.) uo-docx `--release --test all` **795 passed, 0
  failed**, 1 ignored (`completion_a2::` 11/11 with the policy test). uo-wasm `shell_completion_native` 5/5,
  `shell_objects_native` 4/4. Editor vitest 34 files / 334 tests, viewer 28 passed + 1 skipped; editor/viewer tsc exit 0;
  the strict tsc over the uo-wasm js (paint.ts, viewer.ts, the d.ts) exit 0; rustfmt clean on the touched .rs.

**For the milestone gate**: fold `ShellEdit` and the JSON exports into `Command` + `session.uos` (replay/capture families,
C ABI, C# facade see them); decide declaration 7 (the pending table style committed by a save) against the TS `saveDocx`.

**Process metrics**: two implementer sessions (Opus) on the planner's plan; the second resumed from the first's tree
(`tente novamente`). Rework: the Select Objects overlap with A1 (first session), the OLE dblclick selection (the second
mousedown of a fast double click), the inline drag's undo selection (the TS snapshot holds the drop caret), the harness
targets (`inline:last`, an engine-neutral check instead of the TS-only `enumInlinePics`), `__seed` in a step (desynced the
random ids), two lint findings in the store port (`f64::max`, a float parse). Other frontiers edited A2 files during the
wave (B: `tableMouseMove`'s TS gate in rust-completion-tables.ts); A2 kept their lines.

### Wave 7 — A1: File, View, Font and Paragraph

Frontier A1 of wave 7 (the maintainer: "do everything that is missing"): the File, View, Font and Paragraph families of the
README's "What stays disabled" list, routed to the wasm session, 1:1 with the frozen TS (`clickcompliance-docx` d0f45bc5).
Plan and probes: `core/target/m2-wsession/plan-a1` (outside git). Front name `format`. The first implementer was cut off by a
usage limit; the second one kept its tree, fixed what the verification found (below) and verified everything.

**Routes** (`write/packages/editor/src/engine-routes-format.ts`; handlers `rust-engine-format.ts` joining
`rust-format-{core,file,text,view}.ts`; one additive line each in `engine-routes.ts` FRONTS and `rust-engine.ts` installRoutes,
where A1's handlers are merged LAST, over the other fronts, so `setView`, `save`, `load`, `loadOdt`, `exportOdt/Html/Markdown/
Xps` and `stats` of the main table are overridden from A1's own file):
- public (57 + the overrides): `print`, `printPages`; `getRulerInfo`, `showGridlines`, `gridlinesShown`, `showBookmarks`,
  `bookmarksShown`; `setFontFormat`, `setOpenType`, `setUnderlineColor`, `setTextEffects`, `applyTextEffectPreset`,
  `setTextGradient`, `textEffects`, `documentThemeColors`, `copyFormat`, `cancelFormatPainter`, `painterActive`, `painterSticky`;
  `setParagraphIndents`, `setSpecialIndent`, `setIndentRight`, `setTabStops`, `setDefaultTabStop`, `setParagraphFlow`,
  `setParagraphLayout`, `setParagraphBorders`, `paragraphBorderPreset`, `toggleBorder`, `insertHorizontalLine`,
  `getParagraphIndents`, `getTabStops`, `getDefaultTabStop`, `getParagraphFlow`, `getParagraphLayout`, `getParagraphBorders`,
  `paragraphBorderSides`; `selectSimilarFormatting`; `nextManualHyphenation`, `applyManualHyphenation`, `skipManualHyphenation`,
  `endManualHyphenation`; `findNext`, `replaceAll`, `countMatches`, `getLinks`, `getCustomInlines`, `insertCustomInline`,
  `hasExternalFields`, `styleKeyOf`, `ensureBuiltinStyle`, `paintTocTemplate`, `tocEntrySuggestion`, `hasAuthor`,
  `validBookmarkName`. Overrides: `load` (sniffs `.odt` as TS `detectFormat` 2184), `loadOdt`, `save` (odt/html/markdown/xps;
  `'auto'` = the loaded format), `exportOdt`, `exportHtml`, `exportMarkdown`, `exportXps`, `setView` (Web/Draft/Outline; print
  and read delegate to front O's), `refresh` (Web relays out to the viewport width), `stats` (words/characters over the session's
  plain text: a plugin inline counts as its fallback text).
- private: `ensureImagesDecoded`, `paintBookmarkMarks`, `paintTocControl`, `paintEngineInlines`, `paintDocGrid`,
  `tocTabMouseDown`.
- bar ids (51): `fontsInFile`, `export:odt|html|md|xps`, `print`, `compare`, `enableEditing`, `wordCount`, `mailmerge`,
  `mergefield`, `fontAccess`; `view:web|draft|outline|ruler|gridlines|bookmarks`; `color`, `color:gradient`, `dlg:font`,
  `underline:more`, `painter`, `fx:pane`; `borders`, `dlg:paragraph`, `dlg:tabs`, `dlg:borders`, `home:border:*` (12),
  `home:borders`, `home:hrule`, `select:similar`, `hyph:manual`. `select:objects` and `toc:level0…9` are in A2's table (agreed:
  A2 keeps Select Objects with its lasso and multi-selection; A1 dropped its copy).

**Rust** (each a port with the TS line in its doc comment; the native-only commands are `native_only` in `session.bind`):
- uo-docx: paragraph getters `paragraph_flow`/`paragraph_layout`/`paragraph_borders` and `default_tab_stop_now`
  (`ops/paragraph.rs` 654…), `FindCmd::ReplaceAllPlain` (`ops/find.rs` 178, TS 4764: JS `toLowerCase`, tracked =
  trackedDelete + insertTyped, one undo step; `tracked_delete` made `pub(crate)` in `ops/text.rs`),
  `TextCmd::InsertCustomInline` (`ops/text.rs` 35, TS 2108), `StyleCmd::EnsureBuiltinStyle` (`ops/styles.rs` 113) with
  `style_key_for`/`builtin_style_id_of` (`ops/styles/listing.rs` 71/88), `toc_sdt_range` public (`toc_cmds.rs` 219),
  `Session::open_odt` (`io.rs` 132, TS `loadOdt` 2275: the ODF reader over the empty package of `read/blank.rs` 46),
  `Session::has_external_fields` (`ops/settings.rs` 29) — also used for the `uw-protected` event's `hasFields`, which the
  session always sent `false` (`settings.rs` 56; found by the new `bar` family: the TS asks to update fields after Enable
  Editing, Rust did not). Tests: `tests/session_wave7_format.rs` (8, `mod` line in `tests/all.rs` 223).
- schema: `QueryOp::CountMatches(QCountMatches)` at the end of the enum (`session.uos` 311), regenerated codec/js.
- uo-wasm: `src/session/shell_format.rs` (one `mod` line in `src/session.rs`): `uo_shell_fmt_json` (the reads: flow, layout,
  borders, default tab, theme colours, external fields, links, custom inlines, style key, built-in id, drawing grid, a section,
  bookmark marks, TOC range, custom inlines of a paragraph, the plain text, section ends), `uo_shell_fmt_exec` (the three
  native-only commands), `uo_shell_fmt_hyphenation`, `uo_shell_fmt_export` (ODT, HTML, Markdown, the image list),
  `uo_shell_open_odt`, `uo_shell_fmt_ruler`, `uo_shell_engine_set_view` (stored in `ShellEngine.view`, applied in
  `uo_shell_relayout`: `shell.rs` 48/212), `uo_shell_fmt_toc_template`, `uo_shell_fmt_toc_crop`. `shell_state.rs` 80: the
  styles part of now. JS: `js/session-format.mjs` + `.d.ts` (re-exported by `session.mjs` 407 / `session.d.ts` 246),
  `write/packages/editor/src/write-session-format.d.ts`. Native tests `tests/shell_format_native.rs` (3: the reads, the
  commands/exports/.odt reopen, M1 live bytes; fix round 1: `d_compare`, the Rust diff over two fixtures with each option).
- Cargo: `session` gains `uo-docx/odf`, `uo-docx/export` (D-A1-4) and (fix round 1) `uo-docx/compare` (D-A1-3):
  `uo_shell_fmt_compare(a, b, flags)` in `shell_format.rs` answers `JSON.stringify` of the TS `DocDiff`
  (`DocDiff::to_json`), `compare(a, b, opts)` in `js/session-format.mjs`/`.d.ts` and `write-session-format.d.ts`.

**Shared TS edits** (additive, marked `wave 7 (A1)`): `rust-engine.ts` 12 (import), 99 (FrontApi: `paintEnv`, `adoptSession`,
`setUlButton`, `ss`, `pageEnv`, `shellEngine`), 263/865/869 (`hasFields` of `uw-protected` from `hasExternalFields()`, was
`false`), 566 + 1381 (`hooks.content`: painted after the base and before the print guard, so plugin inlines print), 1441
(installRoutes); `rust-review-overlays.ts` 9/14/18 (`paintBookmarkMarks` in OVERLAY_METHODS, ED-310 closed; the formatting
marks of `paintEngineMarks`, ED-309 closed); `engine-routes.ts` 14. Fix round 1: `rust-engine.ts` `hooks.pageHeight` (the
shown sheet height, read by `rebuildCanvases`) and FrontApi `imageState`; `write/packages/viewer/src/rust-engine.ts`
`RustSession.imageState(dl, id)` (`ready|loading|failed|idle`, additive); `chrome.ts` `compare()` passes
`{ diff: ed.rust.compareDiff }` to `loadPair` under `engine="rust"`; `write/packages/editor/src/xps.ts` the optional
`XpsGlyphRun.indices` (written as the XPS `Indices` attribute; the TS legacy runs have none, so its output is unchanged);
the compare product `write/packages/compare/src/universal-write-diff.ts` `loadPair(a, b, { diff })` (optional; without it
`diffDocuments` as before) with its README, `site/docs/universal-write-diff/index.html` and a CHANGELOG line.

**Decisions taken** (the plan's D-A1-n; the maintainer can still reverse them):
- D-A1-1 Web/Draft/Outline = the engine transforms (LQ2); Outline is front B's `ViewTransform::Outline`; one canvas per sheet,
  its SHOWN height the content's end plus the 288-tw margin (fix round 1: `hooks.pageHeight` = the viewer's `webPageHeight`,
  views.ts 62; before, the canvas was the whole 633,600-tw sheet, 42,240 CSS px with its buffer scaled to 0.39). The canvas
  CSS box and its buffer follow that height; hit-testing reads the canvas box, so it is unchanged. Fix round 2 (A1-G4 done):
  a sheet taller than 4000 CSS px is painted in STRIPS of at most 4000 CSS px (the TS `CONT_TILE_PX`, 12516), new
  `rust-format-tiles.ts`: the page canvas keeps its box (hit tests, `engPoint`, caret, scrolling, every front's
  `canvases[page]`) with a 1×1 buffer, and the strips (`canvas.page-strip`, absolutely placed over it, no pointer events,
  `bufferScaleFor` of 4000 px: the full DPR) exist only in the `ensureVisiblePainted` window and follow the scroll. Each
  strip's 2D context offsets every transform by its top (`setTransform`/`resetTransform`/`getTransform` patched), so the
  base, every front's hooks, the selection and the caret paint on it as on the page; the shell's `paint` asks the new
  `hooks.tiles` first (`rust-engine.ts`: the page body split into `paintOn`, the base painted directly on a strip, the
  cached base for a page). Memory: two or three 4000-px strips instead of one 16384-px canvas. The TS tiles are pages of its
  legacy layout; the Rust sheets stay one per section (the engine's transform), so the page canvases differ (ED-707/708 as
  before). In the continuous views `layoutPageCount()` answers 0 and `stats().pages`/`page` and `currentPage()` 1 (the TS:
  no engine layout, one logical page; 18539, 12804, 8831).
- D-A1-2 XPS (fix round 2: the plan's recommended route, done in Rust; the TS writer route of fix round 1 was an unapproved
  choice and is gone, `xps.ts` back to HEAD): uo-pdf `xps` (feature `xps`, `src/xps.rs`, enabled by uo-wasm `session`) writes
  the package from the same pages and resources as `render_pdf`: `Glyphs` with each face embedded once (`/Resources/Fonts/F<n>.ttf`),
  every glyph's index and placed advance (`Indices`), the run's alpha (`Opacity`), synthetic bold/italic (`StyleSimulations`),
  the character scale (`RenderTransform`); rects (a 6-digit hex fill only, as paint.ts; alpha), lines with their dashes, paths,
  shapes (the preset geometry of `uo_render::geom`, solid or gradient brush, outline, the outer shadow, a picture fill),
  pictures (`ImageBrush`: crop, rotation, flips, the pixel adjustments of the image and of Picture Format; JPEG/PNG as they are,
  the other rasters as PNG), clips and transforms as nested `Canvas`, the page colour and the page fill (a tiled `ImageBrush` of
  the PDF's tile). Shell export `uo_shell_fmt_xps(layout)`; `exportXps` in `rust-format-file.ts` (Web/Draft/Outline: the shell
  engine lays out the pages for the export, then the view again, as the TS export is always of the pages).
  Not written (declared, ED-704): text effects (the run plain), WordArt's outlined text (as glyphs), shape and picture effects
  (glow, soft edges, bevel, reflection), a picture's preset outline/frame, pattern fills of shapes, images without a raster
  decoder (EMF/WMF/SVG). The bytes differ from the TS legacy-layout XPS (ED-704); the harness checks the structure, per page
  the session's painted text in the `UnicodeString` runs in order, and per page the elements against the display list (`Glyphs`
  = the runs written, `ImageBrush` ≥ the pictures, `Opacity` ≥ the glyph runs and hex rects with an alpha under 1).
- D-A1-3 Compare (fix round 1: the recommended route): the Rust `diffDocuments` (uo-docx `compare`, D5a) over the session's
  `saveDocx()` and the picked file; `<universal-write-diff>` still reads both documents for its view (its own TS reader) and
  takes the `DocDiff` from `opts.diff`. The bar family compares the whole `DocDiff` TS vs Rust and checks that the Rust diff
  ran (and not in the TS element).
- D-A1-4 exporters in the shipped module: yes (ODF + HTML/Markdown).
- D-A1-5 no Ctrl+Shift+C/V painter keys (the TS has none): 1:1. Esc cancels the painter (key hook first). Fix round 2: the
  painter applies at the end of the shell's DEFAULT text click only (new `hooks.clickUp` called by `RustEditor.clickAt`'s
  mouseup, after its drag logic, and on the triple click; the TS `up` of 10311): a click another front handles (an object,
  a picture, Ctrl+click on a link, a cell selection, a text box) leaves it armed, as the TS.
- D-A1-6 exports are of the BODY (as Word); the plain find/replace work on the edit story as the TS. Exercised since fix
  round 2 (ED-711: the harness's `extra` family exports from the header).
- D-A1-7 the image compression stays the browser canvas (copied `compressImagesForExport`): HTML/MD byte-equal.
- D-A1-10 the harness owns its DECLARED table, each entry tied to an ED below.

**Declarations**:
- ED-701 copies of TS module code in the shell (declared duplicates): `compressImagesForExport` (8731), `detectFormat` (2184),
  `sectionColumns` (492), `BREAK_MARK_LABELS` (324), `DEFAULT_FONT_CSS`.
- ED-704 XPS: the session's pages through uo-pdf `xps` (fix round 2; was `xps.ts`), not the TS legacy layout; checked for
  structure (`[Content_Types].xml`, pages, embedded fonts), the page text and the elements per page (see D-A1-2), not
  byte-compared; what the writer leaves out is listed in D-A1-2.
- ED-711 (fix round 2, exercised): an export (HTML/Markdown/ODT/XPS) while a header/footer, note, text box or cell is edited
  writes the BODY, as Word; the TS swaps `this.model` to the edited story (universal-write.ts 2668, 2750, 18753, 18811) and
  exports that story. The `extra` family checks on `headers.docx` that the Rust export from the header equals its body export
  and that the body exports equal the TS ones (the TS story export logged as the declared difference).
- ED-712 (merge fixer F2: Word manda sobre o TS, measured; was "observed" in fix round 2) `insertTocTemplate('auto1')` on `f7a__7__0` writes the first entry's page number
  `vi` in the TS and `5` in the session; after `updateToc('all')` both write `5` (the TS insertion and update disagree; the
  references front's). The TOC-tab step compares after the update.
- ED-713 (merge fixer F2 round 2: Word manda sobre o TS, measured) a PAGEREF of a section with chapter page numbering
  (`w:chapStyle`): Word writes the chapter numbers and `w:chapSep` before the page (`1-i`), the frozen TS the page alone
  (`i`), on screen and in what `updateToc` writes. See "merge fixer F2, round 2".
- ED-707/708 Web/Draft/Outline: the ruler and the canvas sizes of the Web/Draft/Outline steps (and the Web screen) differ
  from the TS legacy continuous layout (600 tw margins, its strip); text/selection/events/saved document equal. Declared per
  step tag (`web`, `draft`, `outline`, `web-shot`), not over the print-view steps; undeclarable checks: every canvas keeps the
  full buffer scale, and a continuous view's sheets are not taller than the TS strip + 15 % + 40 px.
- ED-709 (fix round 1) custom-inline data that is not plain JSON (a `Date`, an `undefined` member): the session stores the
  data as JSON text (as the saved .docx does), the TS element keeps the object until it saves; declared on the one step that
  inserts such data (`inline-json`).
- Referenced, not A1's: ED-314 / front B's gate-O3 (`f2-3-0` has no theme part; the TS paints the written `w:color` C00000,
  the session now Word's 2023 scheme accent2 E97132 — B's `uo-docx-layout/src/theme_word.rs`, "Word decides" — on screen and
  in the pages the bar's Print sends to `host.print`; the same change makes `editor_switch.mjs --mode all` report f2-3-0's
  thumbnails/screen, B's DECLARED table), ED-304 (front R, the footer's field shading on `f2-3-2`).

**Harness** `core/crates/uo-wasm/tests/browser/editor_switch_format.mjs` (new; the page helpers copy-adapted from
`editor_switch_review.mjs`, declared): families `font` (29 steps), `para` (28), `text` (17), `view` (20), `bar` (23, compact
bar + host hooks), `file`, `memory`. The `bar` family was added by the second implementer: the compact bar's menus
(`dlg:tabs`, `dlg:borders`), commands in no layout through the chrome's registry (`borders`), the File commands ending in
host hooks (`exportFile` for `export:html|md|odt|xps`, `print` → `host.print` page hashes, `compare` → `openFileForCompare` +
the diff element's `stats` and text), `fontsInFile`, `wordCount`, `fontAccess`, `mailmerge`/`mergefield` (hidden in both: the
TS has no such methods), Enable Editing after a protected load.

Fix round 1 (the review's findings): the print step waits for `host.print` up to 120 s and fails explicitly (the printed
pages are a facet taken once both engines printed: no timing answer); the DECLARED entries narrowed to the facets and steps
they cover (no `f2-3-0|*`: the font and para scripts run on `f7a__7__0`, every end screen compared; the view declarations
only on the Web/Draft/Outline steps by tag, ruler and canvas sizes, plus the Web screen; the glow capture ED-710); captures
while the text effects are applied (glow, outline, preset, gradient); the dialogs edited before OK (More Underlines: wavy
red; Font: 13 pt and a colour; Paragraph: left 1,5 cm, hanging 0,5 cm, 6 pt before; Tabs: a right hyphen stop at 7 cm by
Set and a centred dot stop at 3 cm by OK; Borders: a red 1½ pt Box); a `ruler` family of real mouse drags on the ruler
(first-line, hanging, left and right indents, a tab added, moved and dragged off, the right and top margins, a section
column, a table cell boundary and an indent inside the cell; f7a__7__0, tables, sections); a canvas facet (CSS and buffer
sizes) in the view family with two checks no declaration covers (the buffer scale, the continuous sheet's height); the
compare `DocDiff` compared and the Rust diff required; the XPS page text equal to the text of every glyph run the session's
layout painted on that page.

Final run `--mode all` (fix round 1, session_count build of the tree at 19:24, `verify-a1/fix2_*.log`): font 859 facets
equal / 2 declared (ED-710), para 1078 / 0, ruler 654 / 0, text 708 / 7 declared (ED-709, ED-304), view 986 / 52 declared
(ED-707/708), bar 585 / 0 (the Rust `diffDocuments` ran, its `DocDiff` equal to the TS one); file: HTML, HTML without
images, Markdown, save(markdown), ODT and save(odt) part by part, printed pages equal on 3 documents, XPS well-formed with
every page's text equal to the session's painted text (1 + 1 + 3 pages), the TS `.odt` reopened in both engines equal
(pages, text, `currentStyles()` — the only style facet then —, `load('auto')`, `save('auto')` 0 parts different); 0 undeclared differences, 0 page errors; memory 30
cycles, handles/sessions/engines back to 0, live bytes flat after warm-up (8,247,280).

**Numbers**:
- probes (`plan-a1/routes-probe.mjs`, `cmds-probe.mjs`): before 511 public = 381 routed + 26 shell + 65 disabled mutators +
  41 disabled queries, bar 462 declared / 381 routed / 81 unrouted; after (with A2's tree of the same moment): 511 = 487
  routed + 26 shell + 0 + 0, bar 462 / 462 / 0. A1's share: 0 / 0 / 0.
- size: session module 9,731,936 raw / 3,218,551 gzip at HEAD → 10,306,895 / 3,428,195 with the whole wave-7 tree (A1 + A2 +
  B). A1's own share (names build, `wasm-share.mjs`): `uo_docx::odf` 135,103 raw code / 49.4 KB gzip, `uo_docx::export`
  63,135 / 20.9 KB, `shell_format` exports 40,325 / 13.9 KB — about 84 KB gzip. Fix round 1 (names build of the tree at
  19:50, `verify-a1/fix2_session_names.wasm`): `uo_docx::compare` 48 functions, 52,669 raw code / 19.9 KB gzip / 13.3 KB
  brotli, `uo_shell_fmt_compare` 1,892 / 1.0 KB; the shipped session module (stripped) 10,308,673 raw / 3,450,101 gzip -9
  (the whole wave-7 tree, the other frontiers' changes included). Viewer module (`write`): 4,126,839 →
  4,142,574 (not A1's: A1 changed no code in the viewer build's path; B's layout work is).
- layout parity: uo-docx-layout not touched by A1.

**Verification** (`core/target/m2-wsession/verify-a1/run.sh`, the m2-verify-e.sh commands with -j 3, own target dir; the whole
wave-7 tree of the moment, A2 and B still editing): schema check and uo-codec/uo-schema ok; uo-docx `--test all` 787 passed,
2 failed (`memory_d2a::m3_per_keystroke…` 1,613,233 B and `memory_d2b_f1::m3_per_command…` 1,608,720 B on the 500-page
document: per-command allocation, not A1's code path — most likely B's per-settle `sync_bookmarks`, unverified, for B/merge);
`session_wave7_format` 8/8; natives shell_format 3, shell 8, stories 8, objects 4, review 3, caret 4, session_native 11 ok;
session_api, hostile (10 s), replay (10 families, 0 mismatches), shell_api, write_api ok; editor/viewer tsc clean, vitest
editor 34 files / 334 tests, viewer 7 / 28 (+1 skipped); `editor_switch_review --mode all` ok, `editor_switch_stories --mode
all` ok, `viewer_switch --mode all` ok, `build-pkg font-files document-viewer write` (UO_REQUIRE_ENGINE_WASM=1) + `pkg_smoke`
ok. Failing, not A1's: `editor_switch --mode all/parity` (f2-3-0/1/2 thumbnails and screen: B's gate-O3 theme change, "f2-3-3
declared but equal now"; f7d__7__8 screen with every overlay off), `editor_switch_objects --mode all` (the drags of the objects
family: 147 differences on f7b__7__0 — identical with A1's pointer hook removed AND with A1's whole handler set removed from
installRoutes, so A2's in-flight objects code). `check-changed`: clippy native and wasm32 clean with the one blocker
`-A clippy::disallowed-methods` (A2's `f64::max` and float parse in `ops/lists/store.rs` 79–80); A1's own float parse and a
missing SAFETY comment fixed; rustfmt clean on A1's files.

**Verification, fix round 1** (`verify-a1/fix2_*.log`; the whole wave-7 tree of the moment, A2 and B still editing): session
count build ok; `editor_switch_format --mode all` 0 undeclared differences, 0 page errors (the numbers under Harness; the
font and file families re-run after the last harness edits: ok); routing probes (`plan-a1/probe.sh`): 511 public = 487 routed
+ 26 shell + 0 disabled mutators + 0 disabled queries, bar 462 declared / 462 routed / 0 unrouted; `shell_format_native` 4/4
(incl. `d_compare`); editor/viewer tsc clean; vitest editor 34 files / 334 tests, viewer 7 / 28 (+1 skipped); rustfmt clean
on A1's files. `check-changed`: the two clippy steps FAIL on lints the stable toolchain gained when it updated itself today at
16:25 (rustc/clippy 1.100.0): `chunks_exact_to_as_chunks` (uo-opc `cfb.rs` 276/282, `flate.rs` 887; uo-fonts
`registry/digest.rs`, `subset.rs` 303; uo-testkit `sha256.rs`), `needless_range_loop` (uo-docx `doc/props.rs` 1433) and
`manual_swap` (uo-docx `js/object.rs` 201) — files unchanged since HEAD, none A1's; with those three lints allowed, clippy
native and wasm32 over uo-docx/uo-docx-layout/uo-ffi/uo-wasm are clean (the `disallowed-methods` blocker is gone), and
float-casts and grep-bans pass. For the merge: fix those sites or pin the toolchain.

**Fix round 2** (the second review: 3 major, 4 minor; every one fixed):
- `.odt` styles (major): the session keeps the loaded format (`Session::loaded_odt`, set by `open_odt`; `loaded_format()`,
  `styles_editable()` in `io.rs`) and ports `stylesEditable()` (universal-write.ts 4582) at every place the TS checks it: the
  style commands refuse (`create_style`, `modify_style`, `set_default_font`, `update_style_from_selection`, `rename_style`,
  `delete_style`, `set_style_in_gallery`, `create_list_style`, `save_table_style`), and `builtin_style_id`,
  `ensure_style_used`, `apply_style_core`, the heading insertion (`navigation.rs`), `hf_style_id`, the TOC and
  table-of-figures formats, `use_builtin_style` (lists), the table-style definitions and `policy_style_id` define nothing
  (ops/styles.rs, lists.rs, tables.rs, header_footer.rs, navigation.rs, references/toc_cmds.rs, clipboard/sanitize.rs: one
  guard each, marked `wave 7, A1`). `style_list_entries` answers the `.odt` branch of `listStyles` (4156: the ODF paragraph
  styles of `doc.styles`, read-only entries; `odt_style_list()`, `odt_style_entries()` in `listing.rs`) and the shell
  (`rust-review-styles.ts`) skips the filter and the policy and sorts them by `localeCompare` there; the `styleLook` query
  answers no value when `style_formatting_available()` is false (`styleFormatting` null, 4176). The shell-only `st.loaded`
  is gone: `uo_shell_fmt_json` 18 answers `{ format, odtStyles }` (`loadedState` in `rust-format-core.ts`, used by
  `save('auto')` and `listStyles`). Test `odt_styles_are_read_only` (`session_wave7_format`, 9 tests); harness: after the
  `.odt` reopen, `listStyles` all/recommended/gallery/inUse/document, `paragraphStyles`, `styleFormatting` (Normal and the
  first ODF style), `createStyle`, `renameStyle`, `modifyStyle`, `deleteStyle`, `setDefaultFont`, `updateStyleFromSelection`,
  `ensureBuiltinStyle`, `applyStyle('Ttulo1')` + `currentStyles()`, `listStyles` after, and the `.odt` saved after them part
  by part — equal on the five documents of the file family (15–72 ODF styles).
- Tall continuous sheets (major): done, see D-A1-1 (`rust-format-tiles.ts`, `hooks.tiles`, `paintOn`).
- XPS (major): written in Rust, see D-A1-2 (uo-pdf `xps`, 3 unit tests; `uo_shell_fmt_xps`; `xps.ts` back to HEAD).
- Harness (minor): new `extra` family (`--mode extra`, in `all`): ED-711 exports from the header; the TOC tab clicked with
  the mouse in three loads ("Update Table…" with the update prevented, the menu button, "Update Table…" let through with
  the bar's dialog confirmed and the document text compared); `printPages()` of a page with a plugin inline (equal, the
  plugin's red box on the printed page); the Format Painter armed then a Ctrl+click on a link (stays armed in both), a click
  on a floating text box and a plain text click (equal); the XPS of a page with a semi-transparent text watermark (`Opacity`
  written for the alpha run); a section of 105,574 CSS px (160 pasted paragraphs) in the Web view at zoom 250 % and DPR 2
  (CDP `setDeviceMetricsOverride`): strips of 1052×4000 CSS px with 2104×8000 buffers at the top and in the middle (after the
  scroll), painted, and a click there lands in paragraph 27. The view family gains `layoutPageCount()`/`stats()` in every
  step (equal, Web/Draft/Outline included) and the strips in its `canvases` facet (the declared one of those steps) with the
  full-scale check. The file family gains `f7b__7__0` (shapes: 177 XPS paths) and `pictures.docx` (3 `ImageBrush`).
- Format Painter (minor): see D-A1-5 (`hooks.clickUp`).
- `editor.check.ts` (minor): the `format` line (`bindFormat` satisfies the editor's `FormatBinding`), compiled with both
  `session-format.d.ts` files (the strict tsc line of m2-verify-e.sh must list them: `crates/uo-wasm/js/session-format.d.ts
  ../write/packages/editor/src/write-session-format.d.ts`, with the completion pair of A2).
- Print: `ensureImagesDecoded` decodes every page before it asks for the images (the images table fills as pages decode).
- Observed, not A1's (for the merge; FIXED by the merge fixer F2, below): `pictures.docx`'s 70-byte PNG decodes in the TS element and FAILS in the Rust viewer's
  `decodeImage` (`imageState` 'failed': a placeholder on screen, nothing printed; the TS prints the picture) — the pictures
  path of the viewer/objects front; the file family logs its printed pages instead of comparing them. And the TOC gallery's
  first page number (`vi` vs `5`, ED list above).

**Verification, fix round 2** (`verify-a1/fix3_*.log`; session_count build of the tree at 20:45, the whole wave-7 tree of the
moment, A2 and B still editing): `editor_switch_format --mode all` 0 undeclared differences, 0 page errors (font, para,
ruler, text, view, bar, file, extra, memory), then view, bar, file, extra and memory again after the last TS edits (strip
base cache, the decode of every page before printing): ok; memory 30 cycles, handles/sessions/engines 0, live bytes flat at
8,247,280; `editor_switch_review`, `editor_switch_stories` and `editor_switch_objects --mode all` ok (the shell's `paintOn`
split and `clickUp` hook under every front's screens); routing probes (`plan-a1/probe.sh`): 511 public = 487 routed + 26 shell
+ 0 disabled mutators + 0 disabled queries, bar 462 declared / 462 routed / 0 unrouted; uo-docx `--test all` (`--skip memory_`,
the 500-page soaks are the orchestrator's) 649 passed, 0 failed; `session_wave7_format` 9/9; `shell_format_native` 4/4
(XPS and the loaded format added to B and to the M1 round); uo-pdf `xps` 3/3; the strict tsc line with the format and
completion d.ts pairs and the `format` check clean; editor/viewer tsc clean; vitest editor 34 files / 334 tests, viewer
7 / 28 (+1 skipped); rustfmt clean on A1's files (`xps.rs` formatted; `io.rs` 383, `text.rs`, `settle.rs`, uo-pdf `render.rs`
were unformatted before A1's lines). `check-changed --jobs 3` FAILS as in fix round 1 on the toolchain's new lints, now
reported in uo-testkit `sha256.rs` 56/77/93 and uo-codec `reader.rs` 172 (`chunks_exact_to_as_chunks`; files unchanged,
none A1's); with `-A clippy::chunks_exact_to_as_chunks -A clippy::needless_range_loop -A clippy::manual_swap` its two clippy
steps (native and wasm32 over uo-docx, uo-docx-layout, uo-ffi, uo-pdf, uo-wasm) are clean, float-casts and grep-bans pass.
For the merge: pin the toolchain or fix those sites. Size: the shipped session module (stripped, the whole wave-7 tree)
10,374,269 raw / 3,469,318 gzip -9 (fix round 1: 10,308,673 / 3,450,101; the XPS writer and the other frontiers' changes of
the interval together). `editor_switch --mode all/parity` and `viewer_switch` were not re-run in this round.

**Remaining** (explicit, none is "disabled"; updated by fix round 2):
- ~~A1-G1 XPS~~: done in Rust (D-A1-2, fix round 2: uo-pdf `xps`); what it leaves out is declared (ED-704).
- ~~A1-G2 compare in Rust~~: done (D-A1-3).
- ~~A1-G3~~ → declaration ED-711 (D-A1-6), exercised by the `extra` family since fix round 2.
- ~~A1-G4~~ tall continuous sheets: done (fix round 2, D-A1-1: the strips of `rust-format-tiles.ts`; the `extra` family's
  section of 105,574 CSS px at zoom 250 % and DPR 2 keeps the full buffer scale).
- ~~`engine-switch.test.ts` 141–144 name A2's `setTableTool`/`startEditWrapPoints` as disabled examples~~: replaced by A2's
  fix round 1 (two synthetic unclassified mutators probe the fail-closed path).
- ~~`check-changed` stops on the Rust 1.100 lints~~ (fix rounds 1–2 above): fixed in code at the merge (F3), no allowance.
- rustfmt: `ops/text.rs` and `io.rs` were already unformatted at HEAD 8bf0d483 (`insert_typed`, the page-colour condition);
  A1 did not reformat lines it did not write.

**Merge fixer F2 (print/picture/TOC divergences found by A1, and the A1 harness gaps)** — scratch in
`core/target/m2-wsession/f2` and `f2-toc` (outside git: the probes `probe-xps.mjs`, `probe-png.mjs`, the Word script
`toc-auto1-probe.ps1` and its `result*.txt`, the logs):
- **pictures.docx's picture (fixed)**. Cause: the image is a truncated PNG (70 bytes, IHDR 1200×600 RGBA, an IDAT of 13
  bytes); Chromium's `createImageBitmap` rejects it (`InvalidStateError: The source image could not be decoded`, measured),
  and the TS EDITOR's `decodeImage` (universal-write.ts 8384) catches that and decodes with the engine's `decodeRaster`
  (1200×600, first pixel 255,0,0,127, the rest zero), so its picture paints and prints. The Rust editor painted through the
  viewer's `decodeImage` (`image-decode.ts`), which has no fallback (`imageState` 'failed'). Now: new export
  `uo_layout_image_rgba(layout, image)` (src/write.rs: uo-image `decode_raster` at `ImageLimits::DEFAULT`; `width u32, height
  u32` + RGBA8, status 8 when refused, 6 unknown index; uo-image is a dependency of the `write` feature), `imageRgba(i)` in
  write.mjs/write.d.ts/viewer.ts/write-engine.d.ts, and `RustHost.rasterFallback` (viewer `rust-engine.ts` `imageOf`: a raster
  the browser refuses goes through `imageRgba` → `ImageData` → `createImageBitmap`, as the TS catch; the SVG path has none, as
  the TS). The editor sets it (`rust-engine.ts` `newRustSession`); the VIEWER does not, because the TS viewer
  (`universal-document-viewer.ts` `bitmapFor`) has no fallback either: in both viewers the picture stays a placeholder (the
  same `decodeImage` code; since round 2 pictures.docx is in `viewer_switch.mjs`'s corpus and both viewers are equal on it
  in every facet). Native test
  `write_native::the_editor_fallback_decodes_the_truncated_png_as_the_ts` (the RGBA's sha256 equals the frozen TS
  `decodeRaster` of the same bytes run under Node, `ed2ce009…836c`) and the export in the M1 loop. The format harness now
  COMPARES pictures.docx's printed pages (equal) and checks every Rust image `ready` after the print; no "not compared" path is
  left in it.
- **ED-712 — the TOC gallery's first page number (Word manda sobre o TS)**. `insertTocTemplate('auto1')` at the start of
  `f7a__7__0`: the TS shows `vi`, the session `5`. Measured in Word (16.0, pt-BR, invisible COM under `word-lock.mjs`,
  `toc-auto1-probe.ps1`): "Sumário Automático 1" inserted at `Range(0,0)` and `TablesOfContents(1).Update()` → `PAGEREF
  _Toc… \h` = **5** (the heading is in section 1, decimal numbering, page 5; section 2 is the `lowerRoman` one). Through COM
  the raw `BuildingBlock.Insert` shows `1` (Word has not repaginated yet: `result.txt`, "after insert"); `Update()` and
  `UpdatePageNumbers()` give `5`. The gallery insertion in Word's UI shows the paginated number, which is why `5` is the
  reference. Word agrees with Rust: kept. (A heading in section 2: see "merge fixer F2, round 2" below, ED-713.) Declared in
  the harness's TOC step: the text right after the insertion is compared, the only allowed difference is the TS `\tvi`
  where Rust and Word have `\t5` (`\tv` when a paragraph of section 2 was made a heading first).
- **Format Painter, sticky, applied twice** (harness): the font family's sticky painter now clicks a second place (still
  armed, `[true,true]`), then Esc (`[false,false]`): equal in both engines on `f7a__7__0` and `styles`.
- **exportXps from every view (fixed side effects)**. Measured on both engines (fixed page clock, a `TIME \@ "HH:mm:ss"` field,
  the clock moved 1 h 2 min 3 s before the export): the TS (`withLegacyLayout` 15996) in the Print view leaves a cell being
  edited (`exitCell`), exports and `refresh()`es (the date field moves on screen); in Web/Draft/Outline it has no engine
  (`!this.eng`) so it only exports (no clock move, no cell exit); the XPS holds the inserted time in every view. The shell
  did the opposite (Print: no refresh; Web/Draft/Outline: two `refresh()`, the field moved and the XPS held the new time).
  `rust-format-file.ts` now: Print view = `exitCell` if a cell is edited, export, `refresh()`; Web/Draft/Outline = the print
  layout through `relayoutNow` (the clock epoch kept, D-LA7-24), export, the view back with `relayoutNow`. The `extra` family
  checks Print/Web/Draft/Outline and Print with a cell being edited: view, selection, dirty flag, `uw-cellmode`, whether the
  export moved the field on screen, the XPS page count and the time written in it (Print: the screen after the export too)
  equal to the TS; the Rust XPS bytes are the same from the four views.

**Verification, F2** (`core/target/m2-wsession/f2/*.log`; `session_count.wasm` and `viewer.wasm` built from the tree at
F2's end): `editor_switch_format --mode all` ok, 0 undeclared differences, 0 "not compared" lines (font 947 equal / 2
declared, para 1078/0, ruler 654/0, text 708/7 declared, view 1070/52 declared, bar 585/0; file: pictures.docx printed pages
equal and every image `ready`; extra: ED-712 as declared, the five exportXps side-effect variants equal, the Rust XPS bytes
equal from the four views; memory 30 cycles flat at 9,076,334); `editor_switch_objects --mode all` ok; `viewer_switch --mode
all` ok (74 documents, 579 pages, 0 pixel differences, memory flat); `write_native` 8/8 (the new test included);
`shell_format_native` 4/4; uo-image tests ok; `write_api.test.mjs` 8/8; clippy `-D warnings` (the three toolchain lints
allowed) native and wasm32 on uo-wasm clean; editor/viewer tsc and the strict tsc line clean; vitest editor 34 files / 334
tests, viewer 7 / 28 (+1 skipped). No layout or render file changed (no layout parity run).

**Merge fixer F2, round 2** (the review of F2: one major, three minors) — scratch in `core/target/m2-wsession/f2-toc` (the Word
scripts `toc-chapter-probe.ps1`, `toc-roman-probe.ps1`, `list-paras.ps1` and their `result-chapter.txt`,
`result-roman.txt`, `result-roman-28.txt`, `paras.txt`) and `f2r2` (the wasm builds and the logs). Every Word measurement:
Word 16.0 pt-BR, invisible COM under `tools/parallel/word-lock.mjs`.
- **ED-713 — the page a PAGEREF holds (Word manda sobre o TS)**. Measured: Word writes a PAGEREF as the PAGE field of the
  bookmark's page — the section's `w:pgNumType w:fmt` and, with `w:chapStyle`, the chapter numbers and `w:chapSep` before
  it. A new document with Heading 1 numbered `%1`, one chapter per page and a TOC (`result-chapter.txt`): chapter numbering
  → `1-1 2-2 3-3`; chapter + `lowerRoman` → `1-i 2-ii 3-iii`; `lowerRoman` alone → `i ii iii`. On `f7a__7__0` (section 2
  is `lowerRoman`, continuing; Word pages 6–12): Word's paragraph 28 made Heading 1 and "Sumário Automático 1" inserted and
  updated → `5` and `viii` (`result-roman-28.txt`); its last paragraph → `xiii` (`result-roman.txt`, see the open item
  below). The frozen TS: the layout's PAGEREF (`bookmarkPages`, engine.ts:638) is formatted by the page's section but
  leaves the chapter out, and its TOC entry label (`pageLabelOf`, universal-write.ts 6623) is `String(page.number)`.
  Measured in both editors (the format harness, below): the TOC text after `updateToc('all')` comes from the field refresh
  (`refreshFieldCache` → the layout's bookmark pages), so on `f7a__7__0` both write `5`/`viii` like Word (the reviewer's
  "`13` saved" does not happen with a real layout); the difference is the chapter. Now:
  - uo-docx-layout `bookmark_pages` (engine/field_refs.rs) puts the chapter numbers and the separator before the page
    (`chapter_of` over the pass's headings, as PAGE), unless `LayoutOptions::ts_pageref_without_chapter` (new; the strict
    parity replays against the TS captures set it: `layout_parity.rs`, `f8_parity.rs`, uo-ffi `server_parity.rs`, wasm flag
    bit 11 `LAYOUT_TS_PAGEREF_WITHOUT_CHAPTER` / `tsPagerefWithoutChapter` in write.mjs, write.d.ts and
    `paint-common.mjs`'s `wasmOptions`).
  - ~~the session's TOC entry label (toc_cmds.rs `prepare_pages` → `page_numbering_of`, `PageNumbering::label`)~~:
    **reverted in wave 7c (S); ED-713 is the layout's only** (`bookmark_pages`). The label is the TS `String(page.number)`
    again and the refresh that ends every TOC command rebuilds the bookmark index and the sections first, as the TS layout
    pass ("Wave 7c — S", item 1); `OutlineParagraph::chapter` is gone.
  - Declared: `editor_switch_format` `extra` (Word's `toc-chap-chapter-roman.docx`: the TS writes `\ti` where Rust and Word
    write `\t1-i` after `updateToc('all')`, and the TS `updateToc('pages')` rewrites the 2nd and 3rd entries without the
    chapter; the printed TOC page differs, the other pages equal); `viewer_switch` (`WORD_AUTHORITY`: the two chapter
    documents, facets textLayer + pixels + pdf — scoped to page 0 for textLayer and pixels in wave 7c); ~~the d2b-F10
    replays' acceptance of the step result on `hfopts/chap-*.docx`~~ (dropped in wave 7c: with the TS label the counts
    are the TS's, 0 mismatches, 53 accepted instead of 55).
  - Native tests: uo-docx-layout `tests/toc_pages_w7.rs` (the three `fidelity/toc-chap-*.docx` are the documents the Word
    probe saved, with Word's PAGEREF results; `updateToc` all/pages write them; the layout shows them, and the TS flag the
    bare page; `f7a__7__0` paragraph 31 (Word's 28) → `5`, `viii`; its last paragraph → a roman label).
- ED-712's record now says that the raw COM insertion shows `1` before Word repaginates (above).
- pictures.docx: the format harness fails when the Rust image states are missing (fewer than its 3 pictures) before it
  requires every one `ready`; `viewer_switch.mjs` now runs pictures.docx and the three chapter documents besides the
  captures (78 documents): both viewers equal on pictures.docx in every facet (the same placeholder).
- **Open (layout, not F2's files)** — **closed in wave 7c (W, item 6: applied, `xiii` asserted in `toc_pages_w7.rs`; "Wave 7c — W")**: Word moves a "keep lines together" paragraph that is taller than a page to the next
  page when it does not fit: `f7a__7__0`'s last paragraph (1,543 characters, started on page 12) made Heading 1 (Título 1:
  16 pt, keepNext, keepLines) starts on page 13 in Word and spans 13–15 (`result-roman.txt`); the layout (= the frozen TS
  rule, flow.rs `need <= full` in the keepNext/keepLines check) keeps it on page 12, so its TOC entry is `xii` where Word
  writes `xiii`. The label is right for the layout's page; the page rule needs its own oracle series (keepLines and
  keepNext-only paragraphs taller than a page, at the top and mid-page, in a cell) and the parity-flag treatment before it
  changes — for the coordinator to assign to the layout front.

**Verification, F2 round 2** (`core/target/m2-wsession/f2r2/*.log`; `session_count.wasm`, `viewer.wasm` and `selftest.wasm`
built from the tree): `editor_switch_format --mode all` ok, 0 undeclared differences, no "not compared" line (font 947 equal
/ 2 declared, para 1078/0, ruler 654/0, text 708/7 declared, view 1070/52 declared, bar 585/0; extra: ED-712, ED-713 on
`f7a__7__0` — after the update both `5`/`viii` = Word, printed pages equal (11) — and on the chapter document as declared;
memory 30 cycles flat); `viewer_switch --mode all` ok (78 documents, 592 pages, 0 undeclared differences; declared gate-O3
×4 and ED-713 ×2; memory flat); uo-docx `--test all` 801 passed, 1 ignored, the d2b-F10 replay failed (2 mismatches on
`chap-endash`) before its ED-713 acceptance and passed when re-run after it (81,016 equal, 0 mismatches, 55 accepted, 2 of
them ED-713); wasm `session_replay` all families 0 mismatches (f10: 76,344 equal, 55 accepted); uo-docx-layout
suite 151 passed (`toc_pages_w7` 4/4 included); layout parity strict, captured from the frozen TS: group ci 200 documents
600 equal / 0 mismatches, group oracle 2,742 documents 8,226 equal / 0 mismatches; `paint_parity.test.mjs` with the frozen
TS 2 passed (1 skipped: `UO_V_ORACLE`); `write_api.test.mjs` 8/8; the strict tsc line (with `write.d.ts`) clean;
`check-changed` (clippy native + wasm32 `-D warnings`, float casts, bans) all steps passed.

### Wave 7 — B: fidelity with Word, the declared differences and the internal decisions

Frontier B of wave 7 (the maintainer: "do everything that is missing"; Word is the authority for the layout facts). Plan and
scratch: `core/target/m2-w7b` (outside git: `tools/` with the Word export script `word-pdf-save.ps1`, `rpdf.mjs` = the Rust
viewer module over any bytes, `lines.py`/`colors.py`/`order.py` over PDFs, `routes-probe.mjs`; `harn.sh` = the module builds
plus every front harness; `lp.sh` = the seven layout parity groups; `verify.sh` = the merge script with this wave's lines).
Two implementers (the first was cut off by a usage limit; the second resumed from its tree and verified it). Frozen TS:
`clickcompliance-docx` d0f45bc5.

#### Measurements (Word)

Hidden Word via COM under `tools/parallel/word-lock.mjs` (`Documents.Open` read-only, `ExportAsFixedFormat`, `SaveAs2`; one
Word session per file where it mattered), PDFs read with pdfplumber (`non_stroking_color`, words) and pypdfium2 (paint
order, rasters). Never a visible window, no desktop capture.

| Case | Word | TS | Rust now |
|---|---|---|---|
| gate-O3: `w:color C00000` + `w:themeColor`, package WITHOUT `theme1.xml` (generated `theme-none`: 13 slots + 13 tints/shades) | the Office 2023 scheme, identical to the same document WITH the 2023 theme part: accent1..6 `156082 E97132 196B24 0F9ED5 A02B93 4EA72E`, text1 `000000`, text2/dark2 `0E2841`, background2/light2 `E8E8E8`, hyperlink `467886`, followed `96607D` | viewer: the `w:color` (C00000…); editor: C00000 after the load and these parity steps (192,0,0 on its screens), E97132 once its style sheet is rewritten (the objects harness's f2-3-0 end screens: (233,113,50) on both sides) | the 2023 scheme, every base colour exact (233,113,50 on the editor screen; native test `theme_fallback_word`) |
| tints/shades of the same (accent2 t BF/99/66/33, s BF/80; text1 t A5/80; accent1 t 99, s BF; text2 t BF; background1 s BF/A6) | `EE9465 F1A983 F6C5AC FAE2D5 BF4E14 80340D 5A5A5A 7F7F7F 45B0E1 0F4761 215E99 BFBFBF A6A6A6` | its tint rule | 8 of 13 equal; `F1A984 F6C6AD FAE2D6 BF4F14 80350D 595959 215E9A`: one unit in one channel, the TS rule. **Wave 7c (W): still OPEN** — Word's rule not found (3,356 measured samples: the TS rule 2,323, the best candidate 2,662; "Wave 7c — W", item 1) |
| gate-C1: the TS-inserted text box over the ellipses (f7b__7__8, TS-saved bytes) | the box filled `FFFFFF` (scheme `lt1`), painted last | `FFFFFF` | `FFFFFF` (before: `FFC000` — `lt1` over the empty theme; the paint order was always Word's: not a z-order) |
| text boxes in cells (generated `cell-lic1/0`, `cell-nested`, `hf-cell`, `linked`) | opens all; the cell text at x 85.10 pt (= the margin) | = Rust (layout parity) | the whole table 5.67 pt (113 tw) right of Word's: a package WITHOUT `settings.xml` — Word lays the table out with its pre-2013 rule (the cell margin hangs left of the margin), both engines use compatibility mode 15 (`emptyFormatting`). **Wave 7c (W): applied** — Word opens such a package in mode 12 (COM `CompatibilityMode` 12), the Rust layout follows (`ts_missing_settings_mode15` keeps the TS); "Wave 7c — W", item 2 |
| two LINKED boxes in one cell (`linked`) | box 1 shows "primeira caixa" (y 92 pt), the chain continues in box 2 (y 129) | = Rust | the chain's text starts in box 2 (y 129 pt): both engines. **Wave 7c (W): applied** — the first box keeps the chain's first lines (re-measured baselines 100.82/137.78 pt; `ts_linked_box_cell_chain`); "Wave 7c — W", item 3 |
| a text box inside another box (`box-in-box`) | `Documents.Open` fails with RPC 0x800706BE and the Word process ends (twice, a fresh session each) | opens | opens (parity only) |
| ED-108: saving with the caret in a text box (stories X2, f8__7__5) | opens both: TS-saved 1 page / 11 paragraphs / 2 shapes; Rust-saved 16 / 194 / 13 = the original fixture (16 / 194 / 13) | the truncated `document.xml` (8,181 B against 82,596) | — |
| ED-600: Delete after typing with a shape selected (parity f7d__7__11) | opens both: TS-saved 22 paragraphs / 6 shapes (a character deleted), Rust-saved 23 / 5 (the shape deleted, as Word's Delete on a selected shape) | — | — |
| gate-O5 bold in cells (f7b__7__3, the first implementer) | 'quebra de linhas celulas' does not fit 109.2 pt in Carlito Bold (109.78 pt): Word breaks it | breaks | breaks (end screens equal the TS in three objects runs) |
| f7c__7__5: `REF bm2 \h`, `bm2` without `bookmarkEnd` | "Erro! Fonte de referência não encontrada." | 'brown dog quebra cada…' | = TS (gate-10). **Wave 7c (W): applied** — Word's error text, bold (`ts_ref_bookmarks`); "Wave 7c — W", item 4 |
| hyphenation f2-3-1/2/3 | 'responsabi-lidade', 're-', 'autom-' | 'responsabili-dade' | = TS; Word's hyphenator is not available (open hyph-utf8 patterns). **Wave 7c (W): OPEN** (licence + the maintainer's directive against exceptions from Word's output; "Wave 7c — W", item 5) |

#### Causes closed

- **gate-O5** (bold in table cells laid out narrower, f7b__7__1/3 objects, f7a__7__1 views, intermittent): the font supply.
  uo-fonts reports each missing (family, variant) ONCE per environment (`take_missing`); the viewer's `RustSession.pushNow`
  pushed a family the resolver held with its regular face only and never queued the bold, and the editor's synchronous
  `relayoutNow` took the missing list and dropped it. `write/packages/viewer/src/rust-engine.ts`: a pushed family still
  lacking a requested face is queued for `fetchLater`, and `carryMissing` keeps a list taken outside the loop for the next
  pass (the editor's one line below). Test `write/packages/viewer/test/engine-fonts.test.ts`.
- **gate-10 f7c__7__1 (page 2), f7c__7__5 (page 6)**: the session kept the READER's bookmark offsets, which stopped matching
  its runs once `markNoteRuns` gave each note reference its U+FFFC or the text was edited, so REF read a shifted range. The
  session builds the bookmark index of the TS editor's layout pass (`buildEngineDoc` + `bookmarkMarks`, engine-doc.ts
  111–130) from its current runs: uo-docx `session/settle.rs` `bookmarks_of_marks` + `Session::sync_bookmarks` (pub), run at
  the open (`io.rs`) and by the hosts right before a layout (the relayout path of `shell.rs`, `uo_session_layout` in
  `handle.rs`) — as the TS, in the layout pass. (The first cut ran it after EVERY command: a whole-body walk allocating 1.6 MB
  per keystroke on the 500-page document, which failed uo-docx `memory_d2a`/`memory_d2b_f1` M3, budget 64 KB; moved to the
  layout pass.) The `Arc` is replaced only when the index changes. **Correction (review fix round 1):** that did NOT keep
  LA7's fast path — typing in a paragraph that holds a bookmark moves its offsets, so the index changed at every key and
  `DocParts::same_parts` (`Arc::ptr_eq` on `bookmarks`) refused the incremental path (`full (document)`, 43–79 ms on
  f8__7__8 paras 0/4/8; measured by the reviewer). Fixed in uo-docx-layout `incr/keys.rs` `DocParts::bookmarks_follow`
  (called by `incr/mod.rs` once the changed blocks are known): the same names in the same order, each start/end either
  equal or in the same block inside the changed range, keep the fast path — the layout reads a bookmark's offsets only
  with its block (REF/NOTEREF text through `bookmark_para` → `flow_block`, a traced read of that block; `p` compares
  the offset only for a field in the same block) and its page through `bookmark_pages` in `next_input`, which the
  resumed run runs too. Anything else (a name added or removed, a block moved) stays `document`. Test `shell_native`
  `i_typing_in_a_bookmarked_paragraph_stays_incremental` (f8__7__8 paras 0/4/8/1/5 and the REF fixture, offsets 0 and 3:
  every key incremental and its pages byte-equal to a full layout); the reviewer's probe over the rebuilt module: paras
  0/1/4/5/8/9 all `incremental` (11–35 ms). Not synced: the layouts
  other frontiers added this wave outside those two paths (A1's print pages in `shell_format.rs`, A2's previews): a REF to a
  bookmark in an edited document can read the open's offsets there — for the merge. Repro `core/crates/uo-docx-layout/tests/fidelity/ref-after-note.docx`, `tests/fidelity_w7.rs`.
- **gate-O3 / ED-314 / R-1** (theme colours without a theme part; Word is the authority): uo-docx-layout `theme_word.rs`.
  When the model's theme is the empty one (`readTheme(undefined)`), every theme colour of the layout (`w:themeColor`,
  `w:themeFill`, DrawingML scheme colours: text, borders, shading, cells, shapes, boxes, pictures, the page background)
  resolves over the Office 2023 colour scheme — the ONE copy of the palette the writer already has
  (`uo_docx::write::ctx::DEFAULT_SHAPE_THEME`); fonts and format scheme unchanged (not measured). `LDoc::theme` carries it;
  nothing is written. `LayoutOptions::ts_empty_theme` (wasm layout flag bit 10, `tsEmptyTheme` in `write.mjs`) keeps the
  frozen TS behaviour for the strict replays only (`layout_parity.rs`, `f8_parity.rs`, `server_parity.rs`,
  `paint-common.mjs wasmOptions`); no product path sets it.
- **gate-C1** (f7b__7__8/10, f8__7__5): the inserted box's `lt1` fill over the empty theme (table above); closed by gate-O3.
- **gate-O1** (the cover page's coloured box, 7 documents): equal in every wave-7 run with the gate-O3 theme (the covers'
  colours are theme colours, these fixtures have no theme part); covered by `theme_fallback_word` and the harness.
- **gate-O2** (f7a__7__1: 11 pages against 10 after three columns): **equal in the wave-7 runs, cause NOT established**
  (correction of the review fix round: colours cannot change a page count, so it is not gate-O3's; the entry stays retired
  because it was never met again, not because a cause was found).
- **ED-106** (stories X1 ×3, X3, X4, X2's screen): a HARNESS artefact. The TS frame and handles of a box (after Esc, in edit)
  are front O's paint, which `editor_switch_stories.mjs` left out of the Rust screen (it kept front S's hooks only). Front O's
  overlay hook is tagged `front: 'O'` (`rust-engine-objects.ts`, one line) and both harnesses keep S and O: equal.
- **R-2** (Update Style to Match Selection) and **R-6** (the hyphens of f2-3-2): equal in the wave-7 runs (the review screens
  with the overlays off are equal), closed by the other frontiers' and wave 6's work; their entries retired.
- **f7b__7__0|menu, f7b__7__8 end screens**: the TS `tableMouseMove` gate is `print && !hfEdit && eng` (no `tbEdit`: only the
  mousedown has it); the shell's skipped a text box in edit — `rust-completion-tables.ts tableMouseMove` follows the TS gate
  (A2's file, additive). The two screens still differ (ED-219b below): that was not their cause.
- **SetClock validation** (decision; the open message's clock too since the review fix round: `open_options` in `handle.rs`
  calls `validated()`, test `session_native` `refusals_and_errors` opens with month 15, day 40, 29 Feb 2026 and month 0 —
  refused — and 28 Feb — opened): uo-docx `DateParts::is_valid` / `LocalNow::validated`; `uo_session_exec` `SetClock`
  refuses a hostile clock (`handle.rs`, one line) instead of reaching the DATE arithmetic (WS-D14). Test `clock_tests`.
- **The stories harness's chrome remainder**: empty — every id of it is routed now (A2: `table:excel/painter/draw/eraser/
  style:new/style:modify`, `paste:cells:*`, `dlg:pageBorders`; A1: `dlg:borders`): 128 routed ids × 5 contexts equal.

#### Declarations kept (each with its evidence and owner)

`editor_switch.mjs`: f2-3-0..2 **Word authority over TS (gate-O3)** (thumbnails and the screen: TS C00000, Word and Rust
E97132; f2-3-3 retired in the re-verification: equal in all five wave-7 parity runs); f7d__7__11 **ED-600** (TS defect, Word evidence above); f8__7__5 **ED-108** (TS defect); f7d__7__8 **SCR-1** (CLOSED in the review fix round, below — the
entry removed):
new in the final runs (3 of 3): after PageDown into a table, typing, Enter and a Shift+click in a cell, page 0's END SCREEN
differs (a text box and the lines around it) while thumbnails, every step facet and the saved package are equal, and the
saved bytes laid out fresh are the TS layout (`fidelity_w7` over them: session = reader): a screen paint state of the shell
with the wave-7 tree, cause not isolated — cross-front blocker.
`editor_switch_objects.mjs`: f2-3-0|pictures, f2-3-0|views, f7c__7__2|page, f7c__7__8|page, blank|keys **ED-304 (front R,
frontier A1)** — the only differing pixels are the gray shading of the field runs (and the note's own mark) in the header,
the footer and the notes: the TS `paintEngineMarks` shades field runs over every story's lines (`loc.kind !== 'other'`), the
shell's over the body; (f7b__7__8|objects, f7b__7__0|menu **ED-219b**, the hovered table's handles: retired in the
re-verification, equal in its two runs with A2's later tree); f7a__7__5|page **LOAD-1** (CLOSED in the review fix
round, below — the entry removed): one run in five, 14 pages against 12 right after the load.
Retired: every COVER/gate-O1/O2/O5/C1 entry, ED-202, ED-213 (+ `GLOBAL_O.shapeFillBody`), ED-219 and ED-209 (equal now).
`editor_switch_stories.mjs`: X2 f8__7__5 **ED-108** only (TS defect; Word evidence above).
`editor_switch_review.mjs` (rewritten from `UO_ER_COVERED=1`, a new switch listing the declared differences met):
`f2-3-0|*` ED-304 (the footer's field shading, overlays on only); styles|styles, headers|styles, f2-3-0|styles **Word authority
over TS (Subtle Reference look)**: the gallery look of the latent Subtle Reference (`5A5A5A`, text1, tint A5) is TS 595959 /
session 5A5A5A, and Word paints 5A5A5A (text1 tint A5 measured above); f2-3-2|find ED-304 (a search hit in a table cell, its
highlight and the footer shading); f8__7__0|spell ED-304. R-1/R-2/R-6 retired.
`editor_switch_completion.mjs` (A2's harness, its first declarations): ED-304 (the footer's field shading and the
shading of the pasted fields) on f2-3-0/f2-3-1/f7c__7__2 lists, f2-3-0 paste, inline and previews. (`markerPreview` css
**ED-C1** retired in the re-verification: equal on its 4 documents since A2's `shellFonts`.) f2-3-0|tables step 24 (the
move-handle drag, left undeclared here as A2's open item) was fixed in A2's fix round 1 (no caret scroll after the table
gestures); the end screens' ED-304 footer band it leaves is declared as `FIELD_SHADING` since the merge (F3). blank|objects
(the end thumbnail) is equal in both re-verification runs.
`editor_switch_fidelity.mjs` (new): box-in-box outer/inner **ED-106b (front O / A2)**: the TS paints the dashed frame and
handles of a box INSIDE a box in edit; the shell lists the context's own drawings only (`contextDrawingRuns`, uo-docx
`loc::drawing_paths` does not descend into a box), so it has no rectangle for it (Word refuses such a document); hf-cell,
hf-cell edge **HF-T1** (CLOSED in the review fix round, below — both entries removed): a click in a header TABLE cell — the TS header model flattens the cell paragraphs
(the caret lands in the cell), the session keeps the table and its header hit test answers the paragraph after the table,
where the typing goes (saved `header1.xml` differs). box-table, note-cell undo **TS defect, Word agrees with Rust** (review
fix round 2): Ctrl+Z inside a box or a note that holds a table — the TS undo parses its state from JSON, the edited list
and the blocks come back as separate copies, and the next `reconcileBlocks` moves every paragraph before the table and
leaves each cell an empty paragraph; Word (the same edits through COM) keeps the paragraphs in their cells, as the Rust
session. Evidence in the fix-round-2 entry below.
`viewer_switch.mjs` (a declared table with a stale check, new): f2-3-0..3 **Word authority over TS (gate-O3)**: pixels and
PDF (the TS viewer C00000; text, layout, find and pages equal).

#### Decisions taken (wave 7)

| Id | Choice | Reason | Evidence |
|---|---|---|---|
| G1, G2, G3, G6, G9, G10, G13, G15–G18, G20 | accepted as built | the milestone-1 review's recommendation; no result change | "For the milestone-1 gate" |
| G23 | keep the conversion thread (16 MiB; 64 MiB debug) | 64 and 200 nesting levels convert from a 1 MiB caller | G23 row |
| G25 | the trust limits as implemented | the frozen C# limits for untrusted input | G25 tests |
| G26 | accept | wasm2js is not the default engine | G26 row |
| G27 | keep the fallback | rebuilding a full environment needs session refcounts for a rare case | `write_hostile` M5 |
| G28 | accept and document | one module per page (SW-10) | viewer README: "returned to the browser only when the page reloads" |
| G29 | approve the page-lifetime baseline | flat, 0 handles, one environment (SPEED-1) | G29 row |
| G24 | Word's resolution rule in the PDF, the screen painter uncapped (TS parity), declared | a budget changes results against the TS | G24 row |
| G5 / G31 | keep the untrusted caps | the whole real corpus: **860 documents (E:\UniversalWriteTest), trusted and untrusted both converted, 860 byte-equal, 0 false refusals** (157 s) | `uo-ffi/tests/caps_sweep.rs` (opt-in `UO_CAPS_SWEEP_DIR`, bounded by `UO_CAPS_SWEEP_MAX`) |
| G19 | LSan over the C# facade: job `leak-sanitizer-facade` in `rust-core.yml` (uo-ffi with `-Zsanitizer=leak`, the runtime preloaded into `dotnet`, the CLR's modules suppressed) | the existing job covers the Rust tests only | the YAML parses (18 jobs); never run (G30-CI) |
| G21 | **open item for the maintainer** (not taken): the wasm-session fuzz target is NOT added to `core/fuzz` | `core/fuzz` links uo-ffi, and uo-wasm `#[path]`-includes uo-ffi's `pipeline.rs`: both export the same C symbols, the fuzz binaries fail to link (LNK1169, measured with a `session_exec` target over exec/query/layout/save). It needs its own fuzz crate (own lock file); the session's hostile input stays covered by `session_hostile.test.mjs` (fuzz loop) and the no-panic tests | the attempt in `core/target/m2-w7b/fuzz-session-attempt/` (outside git) |
| G8-W, G8-2, G11 | keep | as built | — |
| SW-1..SW-16, SPEED-1..5 | keep; SW-9 fatal in the publish workflow (`UO_REQUIRE_ENGINE_WASM=1`) | — | `publish-packages.yml` |
| D5 | two modules (viewer `write`, editor `session`) | as shipped since wave 6 | build-pkg |
| D2a (1)(2)(3) | keep the implemented variants | each is the implemented and tested one | D2a record |
| D2a-C1 (headers/footers, notes, text boxes) | the TS shape: a header/footer part, a note or a text box (`Synced` view: read from a .docx) that holds a table is edited through its flat paragraph list (`hf_flat.rs`); a box of the ODF reader (`TopLevel`, the TS `paras` = its top-level paragraphs) keeps its top-level list, as the TS (fix round 2: notes and boxes were the reviewer's major finding) | HF-T1 and the note/box case: 1:1 with the TS (the caret and the typing in the clicked cell); after an undo in a note or a box with a table, Word over the TS (declared, Word measured) | `session_wave7_hf_flat.rs` (4 tests, the TS `header1.xml` and `footnotes.xml` byte for byte), fidelity hf-cell, note-cell, note-plain, note-cell undo, box-table, box-table typing |
| R-8 / ED-301 | `ops/styles/listing.rs` stays in uo-docx | model logic of the C ABI and C# too | — |
| D-L7 | `IncrConfig::phone()` (a third of the wasm32 checkpoint cap, a quarter of the memo cap) for a phone font profile; the desktop unchanged | bit-exact (more full relayouts only: la7 `m5_memo_caps`); today's editor creates desktop environments only, so nothing measured changes until a host passes the phone profile (pages500 not re-measured) | `uo-docx-layout/src/incr/mod.rs`, `uo-wasm/src/session/shell.rs` |
| SetClock | validate (`SetClock` and the open message's clock) | a hostile clock reached the DATE arithmetic | `clock_tests`, `session_native` `refusals_and_errors` |
| `CommandError::Locked`, f4 `ExecOp::UndoReentering` | handed to A1 (the schema owner): remove the dead variant, add the op | schema changes are A1's | — |
| gate-O3 | follow Word in both Rust paths with the 2023 colour scheme; strict TS parity through the test-only switch | the maintainer's ruling | Measurements |
| f7c__7__5 REF to an unterminated bookmark | the session equals the reader and the TS | following Word changes the TS-capture parity of the generated corpora | Measurements. **Superseded in wave 7c (W)**: Word's error text in the Rust layout, the TS behind `ts_ref_bookmarks` for the strict replays ("Wave 7c — W", item 4) |
| the ED lists of waves 4–6 | ED-108, ED-600: TS defects (Word evidence above); gate-O3 and the Subtle Reference look: Word authority over TS; ED-6, ED-202b, ED-203, ED-205/206, ED-217, ED-601/602: engine-boundary differences with no TS behaviour missing (records of waves 4–6; not a maintainer acceptance); open with an owner: ED-106b, ED-304, and, at the merge, ED-707/708 and A2's declarations 3, 4 and 7 ("Wave 7 — merge state") (ED-219b and ED-C1 closed by A2 during the re-verification; HF-T1, SCR-1 and LOAD-1 closed in the review fix round, below); ED-106, ED-202, ED-213, ED-219, ED-209 and the rest: closed in wave 7 | — | the harness tables |

#### For the maintainer

- (review fix round; **applied at the merge**, F3: the lint sites fixed in code, no allowance, see "Wave 7 — merge
  state") **The Rust toolchain drift**: rustup `stable` = Rust 1.100.0 since 2026-11-12 16:25; `check-changed`
  (clippy `chunks_exact_to_as_chunks`, wider `manual_swap`/`needless_range_loop`), `rustfmt --check` (`io.rs:366`,
  `settle.rs:573`) and uo-jsnum `text_v8` (Unicode 17 case tables) fail on HEAD lines; `rust-core.yml` and the publish
  steps run `rustup default stable`, so the first CI run fails lint as the tree stands. Choice: pin
  `core/rust-toolchain.toml` to 1.99 and make the CI honour it, or fix the ~45 lint sites and give uo-jsnum its own
  Unicode-16 lowercase table. Not B's to take (it touches every crate and the CI policy).
- (review fix round) **G21**: a cargo-fuzz target over the session codec decode + exec needs its own fuzz crate with its
  own lock file depending on uo-wasm's session only (not uo-ffi: the two export the same C symbols, LNK1169 measured);
  adding a crate to the workspace layout and a nightly fuzz job is left to the maintainer. Until then the hostile input is
  covered by `session_hostile.test.mjs` (10 s fuzz loop), the no-panic tests and the hostile-clock tests.
- Pushing `rust-core` and the first GitHub CI run (`rust-core.yml` with the new `leak-sanitizer-facade` job, the editor job,
  `publish-packages.yml`'s engine steps); publishing; the 0.10 version and the CHANGELOG version; G14 (NuGet RID matrix) and
  G30 (the Chromium speed gap); the frozen-main security items (D3 S1/S6, the Slides/Sheet hyperlink schemes).
- (**wave 7c, W**: items 2, 3, 4 and the keepLines item APPLIED behind `ts_*` switches; the tint rounding and the hyphenation OPEN — see "Wave 7c — W") Result-changing Word facts measured and NOT applied (each changes the TS-capture parity of many documents): the theme
  tint/shade rounding (one unit, 5 of 13 cases); a table in a package without `settings.xml` (Word's pre-2013 position,
  5.67 pt left of both engines); linked boxes anchored in a cell; REF to an unterminated bookmark (Word's error text); Word's
  hyphenation (proprietary; the open patterns differ).
- Web, Draft and Outline under `engine="rust"`: the engine's view transforms (Outline added: `uo-docx-layout
  view_transform.rs` = the TS editor's outline rule — unstyled top-level body paragraphs 720 tw further in — over the Draft
  sheet), not the TS editor's legacy continuous canvas (A1's ED-707/708). B recommended accepting it for 0.10; the
  maintainer has not ruled, so at the merge it is an **open parity item** (the TS does it, the Rust editor does not), listed
  with its evidence in "Wave 7 — merge state".
- Typing latency at pages50 ties the TS fast path (wave 6, L): recommended to accept for 0.10.

#### What stays disabled (for the README at the merge)

Routing probe (`core/target/m2-w7b/tools/routes-probe.mjs` over the current sources) and `engine-switch.test.ts`: **public
methods 511 — routed 494, shell 26, disabled mutators 0, disabled queries 0; private routed 27; bar commands routed 516**;
`engine-completion.test.ts`: A2 49 of 51 routed + 2 by A1, no blocker. Nothing is disabled under `engine="rust"`. What
differs from the TS editor is the declared list above (owners named), not a disabled command. (The test's 494/26 count
the entries of its tables, with duplicates; the distinct counts at the merge — 485 routed, 24 shell, 2 both; 462 bar
commands declared and routed of 516 routed ids — and the open parity items are in "Wave 7 — merge state".)

#### Files touched outside B's files (each additive, commented `wave 7 (B…)`)

- `write/packages/editor/src/rust-engine.ts` (the font section, one line: `carryMissing` before `runFontLoop`);
  `rust-engine-objects.ts` (front O's overlay hook tagged `front: 'O'`); `rust-completion-tables.ts` (A2's: the hover gate).
- uo-docx: `session/settle.rs` (`bookmarks_of_marks`, `sync_bookmarks`), `session/io.rs` (one call), `session/mod.rs` (a
  comment where the first cut called it per command), `session/env.rs` (`is_valid`, `validated`, `clock_tests`).
- uo-wasm: `src/session/handle.rs` (SetClock, one line; `sync_bookmarks` before `uo_session_layout`), `src/session/shell.rs`
  (D-L7, two lines; `sync_bookmarks` before the relayout), `src/write.rs` +
  `js/write.mjs` + `js/write.d.ts` (layout flag bit 10), `tests/support/paint-common.mjs` (`wasmOptions`); uo-ffi
  `tests/server_parity.rs` (`ts_empty_theme`), new `tests/caps_sweep.rs`.
- Harnesses beyond their DECLARED tables: `editor_switch_objects.mjs` (`paintTableAdorners` compared, as A2 asked),
  `editor_switch_stories.mjs` and the new `editor_switch_fidelity.mjs` (front O's hooks kept; the routed tables of A1/A2 in
  `--mode chrome`), `editor_switch_review.mjs` (`UO_ER_COVERED`), `editor_switch.mjs`/`editor_switch_stories.mjs` (with
  `UO_ES_DUMP` the saved packages too), `viewer_switch.mjs` (its declared table).
- `.github/workflows/rust-core.yml` (G19); `docs/publishing.md` (step 2: the workflow builds the modules);
  `write/packages/viewer/README.md` (G28).
- Review fix round 1: `write/packages/editor/src/rust-engine-completion.ts` (A2's: the table adorners through the element's
  `paintTableAdorners`, SCR-1); uo-docx `model/section.rs` (`HfPart::flat`, `HfFlat`, `edit_story(_mut)`),
  `session/ops/context.rs` (two arms and the context step), `session/ops/drawing.rs` (`base_story(_mut)`), `session/mod.rs`
  (`sync_hf_flat` in `change_set`), `session/ops/mod.rs` (the module line), new `session/ops/hf_flat.rs`, `tests/all.rs` (one
  module line) and new `tests/session_wave7_hf_flat.rs`; uo-docx-layout `incr/keys.rs` + `incr/mod.rs` (`bookmarks_follow`);
  uo-wasm `tests/shell_native.rs` (I), `tests/session_native.rs` (the hostile open clock); the harness switches
  `UO_ES_NOBAND`, `UO_ES_LAT_BM` (`editor_switch.mjs`) and `UO_EO_LOADTRACE` + the TS `settleDocumentFonts` at load
  (`editor_switch_objects.mjs`).
- For A2 (through the orchestrator): B touched `rust-completion-tables.ts` (the hover gate) and `rust-engine-objects.ts` (the
  tag); ED-106b, ED-219b, ED-C1 and the completion harness's tables/objects/inline/previews differences are A2's.

#### Numbers

All on the final tree of the wave (A1/A2 still editing until the end; modules rebuilt for every final run). D: 29 GB free.
- Native: `fidelity_w7` 4/4 (`ref_after_a_note…`, `bookmark_index_follows_edits`, `session_equals_reader_on_the_fixtures`: 76
  documents — the committed captures and the dumps — session layout = reader layout but the 3 declared, `theme_fallback_word`);
  `clock_tests` 1/1; `theme_word` + `view_transform` units 3/3; the uo-docx-layout release suite green (`f8` 6/6 after its
  Outline assertion was updated: Outline lays out now); `caps_sweep` (opt-in) 860/860.
- Layout parity regenerated from the frozen TS and replayed with `ts_empty_theme` (lp.sh = verify.sh's `layout_group`): ci
  200 documents / 600 facets, f7a 300 / 900 strict, f7b 200 / 600 strict, f7c 300 / 900 strict, f7d 200 / 600 strict, f8 200 /
  3,000, Word oracle 2,742 / 8,226 strict — **0 mismatches** in every group (each capture deleted after its run).
- Chromium, final runs: `editor_switch --mode parity` (stride 1, 12 steps) ok — 74 documents, every step facet equal but the
  declared ED-600 (1 event, 1 para state); thumbnails 1,140 + 10 declared, screens 166 + 6 declared, saved parts 660 + 2
  declared; run twice. `editor_switch_fidelity --mode all` ok — 9 scenarios, 0 undeclared, memory 20 cycles flat (live
  4,374,500, buffer 8,519,680), run twice. `editor_switch_objects --mode all`: page 3,802 + 2 declared, objects 5,000 + 1, pictures
  854 + 1, menu 219 + 0, keys 170 + 1, views 361 + 1, memory 30 cycles flat (13,400,333); three runs (2, 4, 5): gate-O5 entries
  never met. `editor_switch_stories --mode all` ok — every facet equal but ED-108 (1 saved part), chrome 640 equal / 0
  different with an empty remainder, memory flat (6,682,346). `editor_switch_review --mode all` ok (0 undeclared in every
  family). `editor_switch_completion --mode all`: lists 1,456 + 7 declared, paste 808 + 1, inline/previews the footer shading
  (declared after the run), tables 999 + 5 and objects 1,706 + 1 open (A2, above), page 134 + 0, memory flat (15,234,456).
  `viewer_switch --mode all` ok — 74 documents, 579 pages, 0 undeclared (f2-3-0..3 pixels+PDF declared), memory 200 cycles
  flat (18,445,397 live, 31,719,424 buffer).
- Routing (`engine-switch.test.ts`, the probe): public 511, routed 494, shell 26, disabled 0 + 0; bar commands routed 516.
- Editor vitest 334/334, viewer 28 + 1 skipped (`engine-fonts.test.ts` included); editor and viewer tsc exit 0; the strict tsc
  line exit 0 once it lists the wave-7 declaration files (`write-session*.d.ts`, `session*.d.ts`, `write.d.ts` — verify.sh
  updated: the old line misses A2's `CompletionBinding`).
- Publish: `publish-packages.yml` parses, steps toolchain → engine modules → build (with `UO_REQUIRE_ENGINE_WASM=1`); the same
  two builds locally (`uo-write.wasm` 4,142,574 B, `uo-write-editor.wasm` 10,306,989 B), `build-pkg font-files
  document-viewer write` with `UO_REQUIRE_ENGINE_WASM=1` ok, `pkg_smoke` 17 ok.
- Lint (**corrected in the review fix round**; the re-verification below is the valid statement): rustfmt clean on every
  line B wrote; `io.rs:366` and `settle.rs:573` are HEAD lines that the current rustfmt (Rust 1.100) reformats, not other
  frontiers' lines. `node tools/check-changed.mjs` passed ("all steps passed") on the Rust 1.99 toolchain at 16:23; under
  Rust 1.100 (rustup `stable` since 16:25) it FAILS on HEAD lines (three new/wider clippy lints) and passes only with those
  three lints allowed (`lint-allow.sh`).

#### Process metrics

Two implementers on one plan. The first wrote gate-O5, gate-10, gate-O3, Outline and the publish steps, then was cut off; the
second verified them and found that gate-C1/O1/O2 were the theme too, that ED-106 was the stories harness's own hook filter,
and that the plan's "TS editor ED7D31" was a misreading (no ED7D31 exists in the TS sources; the TS editor paints C00000 after
the load and E97132 after a style-sheet rewrite). Rework: the G21 fuzz target (the link conflict, reverted), a pointer-module
change for ED-106b (reverted: the shell has no geometry for a nested box), a `JobError` formatting fix of the sweep. The tree
changed under the harness runs (A1/A2 editing until the end), so the modules were rebuilt and every harness ran again on the
final tree.

#### Re-verification (third implementer, 2026-11-12 evening)

The second implementer's last native runs were cut off (the uo-docx suite, `session_native`) after the `sync_bookmarks` move to
the layout pass, so the whole merge script ran again on the tree as it stood at 19:20–22:00 (A1 and A2 were still editing;
the four modules were rebuilt at 20:43–20:47 and every Chromium run used them). Scripts: `core/target/m2-w7b/verify-final.sh`
(= `verify.sh` without the layout groups: no layout source changed since their 15:16 runs, only `lib.rs`'s `pub mod
theme_word` line, which was in them), `verify-resume.sh`/`verify-tail*.sh` (the same steps, resumed after the disk stops),
`lint-allow.sh`. Log: `verify-final.log`.

- **Toolchain drift, not wave 7** (for the orchestrator): rustup's `stable` became **Rust 1.100.0** (2026-11-09) at 16:25
  today, after the last green lint (16:23). Three results changed on lines nobody touched this wave:
  - clippy: the new lint `chunks_exact_to_as_chunks` (~40 constant-size `chunks_exact[_mut]` lines in uo-testkit, uo-codec,
    uo-opc, uo-docx, uo-drawingml, uo-fonts, uo-image, uo-pdf, uo-render and tests) and wider `manual_swap`
    (`uo-docx/src/js/object.rs:201`) / `needless_range_loop` (`uo-docx/src/doc/props.rs:1433`) fail `check-changed`. With
    only those three allowed (`lint-allow.sh`, the crate list of `check-changed --dry-run`, native + wasm32 clippy,
    float-casts, grep-bans): **all steps passed** — nothing else fails on the wave-7 tree.
  - rustfmt: `uo-docx/src/session/io.rs:366` and `settle.rs:573` are reformatted — both HEAD lines, untouched (every line B
    added is clean).
  - `uo-jsnum` `text_v8`: `lower_cp`/`to_lowercase` follow std's Unicode 17 case tables in 1.100 (U+A7DD, U+A7E2, U+AB6C/D,
    U+1DF40… gain lowercase mappings) while the V8 table of the test is Unicode 16: 2 tests fail (20 code points). A product
    difference against the frozen TS for those code points only.
  The choice — pin `rust-toolchain.toml` to 1.99 (the CI's `rustup default stable` would pick 1.100 too) or fix the ~45
  lines and give uo-jsnum its own Unicode-16 table — is the orchestrator's/maintainer's; B changed none of it.
- **Disk**: D: fell to 14 GB, then 8 GB and 12 GB (the other agents' builds); the scripts stop under 15 GB before a build.
  B freed its own target twice (the 1.99 artifacts made stale by the toolchain, 6.3 GB; then the incremental caches and the
  debug tree once every native step had run, 7.5 GB); the remaining steps (Chromium harnesses, build-pkg, pkg_smoke — no
  cargo build) ran with a 5 GB guard. `core/target/m2-w7b` is 2 GB at the end.
- **Results** (re-verification): editor tsc + viewer tsc (`editor_switch_tsc`), the strict tsc line, schema tests and
  `uo-schema check`: exit 0. Editor vitest **334/334**, viewer **28 + 1 skipped**. Demo builds ok (36 outputs). uo-docx
  `--all-features --test all` **794 passed, 1 ignored** (`memory_d2a` M1/M3/M5 and every `memory_d2b_*` included: the per-layout
  bookmark sync keeps the keystroke budget). `session_native` 11/11, `shell_native` 8/8, stories 8/8, objects 4/4, caret 4/4,
  review 3/3, uo-render green, `fidelity_w7` **4/4**, `clock_tests` 1/1, uo-docx-layout release suite **147/147**.
  `session_api`, `session_hostile` (10 s fuzz), `shell_api` (M4 2,000), `write_api`: ok; `session_replay` d2a…f10: 0
  mismatches, every TS re-encode byte-identical. `editor_switch --mode parity` (stride 1, 12 steps), twice: **ok** — 74
  documents, step facets equal but ED-600 (1 + 1), thumbnails 1,140 + 10 declared, screens 166 + 6 declared, saved parts 660
  + 2 declared, page errors 0. `editor_switch --mode all`: the chrome check failed on a wave-4 assertion ("a non-routed
  command is shown disabled", with the Format Painter as its example) — the painter is routed now, so the check is turned
  around (nothing may be disabled; `--mode chrome` ok after it). At the merge F3 put the fail-closed half back with a
  synthetic command registered only for the probe ("Wave 7 — merge state"). Stories `--mode all` ok (chrome 640 equal, remainder
  disabled 0; memory flat 6,682,346). Objects `--mode all` twice: the second ok (page 3,802 + 2, objects 5,001 + 0, pictures 854
  + 1, menu 219 + 0, keys 170 + 1, views 361 + 1; memory flat 13,400,333). Fidelity `--mode all` ok (9 documents; memory flat
  4,374,500). Format (A1's) `--mode all` ok. Review `--mode all` ok. Completion (A2's) twice: lists 1,460 + 3, paste 1,068 + 1,
  objects 1,971 + 0, inline 254 + 1, page 134 + 0, previews 199 + 1; **f2-3-0|tables step 24 the only undeclared difference
  (A2's open hit test, left undeclared on purpose: exit 1)**; memory flat 16,071,393. `viewer_switch --mode all` ok (74
  documents, 579 pages, 0 differing; f2-3-0..3 declared gate-O3; 200 cycles flat at 18,445,397). `build-pkg font-files
  document-viewer write` with `UO_REQUIRE_ENGINE_WASM=1` ok, `pkg_smoke` ok. `publish-packages.yml` and `rust-core.yml` parse
  (18 jobs); step order toolchain → engine modules → build.
- **Declarations retired** (stale in these runs): `editor_switch.mjs` f2-3-3 (equal in all five wave-7 parity runs);
  `editor_switch_objects.mjs` f7b__7__8|objects and f7b__7__0|menu (ED-219b: A2's hovered-table adorners, equal in both runs,
  menu mode once more after the edit); `editor_switch_completion.mjs` the four `markerPreview` css entries (ED-C1: A2's
  `shellFonts`). The lists above ("Declarations kept", the ED row of the decisions) are updated.
- **Declared entries left** (owner): editor_switch f2-3-0..2 Word authority over TS (gate-O3, B), f7d__7__11 ED-600 (TS
  defect), f8__7__5 ED-108 (TS defect), f7d__7__8 SCR-1 (open, cross-front); objects f7c__7__2|page, f7c__7__8|page,
  f2-3-0|pictures, f2-3-0|views, blank|keys ED-304 (A1), f7a__7__5|page LOAD-1 (intermittent); stories X2 f8__7__5 ED-108;
  review f2-3-0|* and f2-3-2|find, f8__7__0|spell ED-304 (A1), styles|styles, headers|styles, f2-3-0|styles Word authority over
  TS (Subtle Reference look); completion f2-3-0/f2-3-1/f7c__7__2 lists, f2-3-0 paste/inline/previews ED-304 (A1); fidelity
  box-in-box ED-106b (A2/front O), hf-cell HF-T1 (front S model); viewer_switch f2-3-0..3 gate-O3. Undeclared and open at
  this point: completion f2-3-0|tables step 24 (A2) — closed by A2's fix round 1 and the merge's `FIELD_SHADING` entry.
- Not re-run: the seven layout parity groups (no layout source changed since their green 15:16 runs) and the Word
  measurements (the facts did not change).

#### Review fix round 1 (fourth implementer, 2026-11-12 night)

The reviewer's eight findings on B's uncommitted work. Nothing of the earlier fixer of this round was in the tree (no file
changed after the re-verification log), so every fix below is new. Modules rebuilt at 22:58 (`session_count`) and 23:13
(`session`) in `core/target/m2-w7b/verify-e`; scratch logs `core/target/m2-w7b/fx-*.log`, `load1.log`.

- **The bookmark sync and LA7 (major)**: fixed — `DocParts::bookmarks_follow` (Causes closed, gate-10, the correction).
  Typing latency measured again (`editor_switch.mjs --mode latency`, new switch `UO_ES_LAT_BM=1`: a bookmark after the
  caret in the typed paragraph, so its offsets move at every key; `session.wasm`, real clock, the machine shared with two
  agents): pages50 Rust type 11.5/29.5 ms with the bookmark (TS 10.0/12.8) against 7.1/9.9 without it (TS 7.5/11.1) in
  the same invocation, and 8.2–9.0 against 8.5 in three Rust-only repeats; pages500 21.1 against 21.3 (TS 33.2); every
  typed key `incremental` (200/200, 30/30) in both variants; Enter/Backspace full (`document`) in both, memo hits
  identical. The bookmark does not change the per-key path; the spread on pages50 is the machine's.
- **LOAD-1 (major)**: closed. `UO_EO_LOADTRACE=1` (new, objects harness): the page count at the load's settle point and
  1.5 s later after a refresh, with the FontFaceSet's loaded faces. Ten traced runs of `UO_EO_ONLY=f7a__7__5 --mode page`
  with a release cargo build on the machine: TS 14 → 14 and Rust 14 → 14 in 10 of 10. The one occurrence
  (`h-objects-4.log`: `ts 12, rust 14`) was the TS run: its settle is `document.fonts.ready` + 300 ms, while the TS
  fetches the engine's faces later (`engFontLoad`); the Rust run waits for its font loop (`rust.idle()`). The harness's
  `load` now also awaits the TS's own `settleDocumentFonts()` (what the TS runs before an export) — the counterpart of
  `idle()`; nothing in `viewer/src/rust-engine.ts` was at fault. Entry removed.
- **SCR-1 (major)**: closed, a harness/routing asymmetry, not a layout or band-repaint regression. Bisection switches
  `UO_ES_NOBAND=1` (no band repaint) and `=2` (every relayout handled as full, every page cache dropped) left the
  difference unchanged, so neither wave 6's band repaint nor the `changed` pages caused it; the dumped screens showed the
  hovered table's move/resize handles in the Rust screen only: after the Shift+click in a cell the pointer rests over the
  table, and the "TS overlays off" capture turns `paintTableAdorners` off on the TS element while A2's wave-7 adorners
  were painted by a hook. `rust-engine-completion.ts` (A2's file, one routing line, commented) installs the shell's painter
  as the element's `paintTableAdorners` (instance shadowing, as front R's overlays) and the hook calls it:
  `editor_switch --mode parity` f7d__7__8 then equal (screens 2 of 2). Entry removed. B's bookmark sync was not involved
  (the fixture has no bookmark).
- **HF-T1 (major)**: closed in uo-docx — the TS shape of D2a-C1 for header/footer parts. A part that holds a table is
  edited through its flat paragraph list (`hfParts.paras` = `flattenBlocks`), as the TS and as the edit lines' hit test
  already were: `HfPart::flat` (`model/section.rs`, never read by the layout or the writer), the context story of
  `story_of`/`story_of_mut` and `drawing.rs`'s `base_story` through `HfPart::edit_story(_mut)`; new
  `session/ops/hf_flat.rs`: `Session::sync_hf_flat` at the head of every `change_set` reconciles the view into the
  blocks by paragraph identity (`ParaId`, G22) with the TS `reconcileBlocks(blocks, paras, true)` rule (a new paragraph
  right after the previous one of the list that was there; a removed one leaves; an emptied cell gets an empty
  paragraph) and rebuilds the view from the blocks; the context step of the history keeps the BLOCKS (an undo restores
  them and the view follows). Two edge differences from the TS are written in the module comment (the TS reconciles from
  the blocks it read, every time; the empty paragraph of an emptied cell is in the list here). Notes and text boxes
  holding tables keep the top-level model (D2a-C1 for notes unchanged; no harness case) — **superseded in review fix
  round 2** (below): notes and boxes now follow the same rule. Tests: `hf_flat` unit
  (`reconcile_follows_the_ts_rule`), `tests/session_wave7_hf_flat.rs` (the TS `header1.xml` of `hf-cell edge` byte for
  byte; Enter/Backspace in a cell, undo, redo), fixtures `uo-docx-layout/tests/fidelity/hf-cell.docx` (generated by the
  fidelity harness, no fonts) and `hf-cell-edge-ts-header1.xml` (the frozen TS's saved part). Fidelity harness: both
  scenarios equal in every facet (20 + 2 steps, saved parts 12 of 12); entries removed.
- **The record (minor)**: Numbers' lint line, gate-O2 ("equal, cause not established") and the fast-path sentence
  corrected above.
- **The open clock (minor)**: validated (Causes closed, SetClock).
- **The toolchain drift (minor)** and **G21 (minor)**: not B's to take; both listed in "For the maintainer" with the
  choice each needs.

Results of this round (the tree as of 23:13; A1/A2 files unchanged since the re-verification):
- uo-docx `--all-features --test all` (release) **797 passed, 1 ignored** (the two HF tests added; `memory_d2a`/`d2b`
  budgets included); `hf_flat` unit 1/1; `clock_tests` 1/1; uo-docx-layout release suite **147/147** (la7's incremental
  = full tests included); `session_native` 11/11 (the hostile open clock), `shell_native` **9/9** (I new), stories 8/8,
  objects 4/4, caret 4/4, review 3/3, completion 5/5, format 4/4. The reviewer's probe (`bm-incr.mjs`) over the rebuilt
  module: f8__7__8 paras 0/1/4/5/8/9, offsets 0 and 3, all `incremental`.
- Chromium (`UO_WASM_PATH` = the rebuilt modules): `editor_switch --mode parity` (stride 1, 12 steps) **ok** —
  thumbnails 1,140 + 10 declared, screens **167 + 5** declared (SCR-1 gone), saved parts 660 + 2, step facets 887 + 1
  (ED-600); `--mode all` ok; fidelity `--mode all` ok (only ED-106b declared; memory flat 3,904,612); stories ok (ED-108
  only; memory flat 6,682,346); objects `--mode all` ok (memory flat 13,400,333); format ok; review: the first `--mode all`
  (run while a release cargo test held 3 cores) failed twice with the shell's overlays absent from the end capture
  (revisions|revisions "overlays painted ts 1 / rust 0", f2-3-2|find overlay mask), the same modes re-run alone and a
  second `--mode all` ok (0 undeclared) — a capture-timing flake under load, recorded for the orchestrator, no
  declaration added; completion: only A2's open f2-3-0|tables step 24 (as before; closed later, see above).
- Editor and viewer tsc exit 0; editor vitest 334/334, viewer 28 + 1 skipped; routing probe: public 511, disabled
  mutators 0, queries 0 (routed 487 + shell 26 on the current sources). rustfmt clean on every line B wrote this round;
  `grep-bans` ok; `float-casts` over uo-docx, uo-docx-layout and uo-wasm: no hit (run by mistake without
  `CARGO_TARGET_DIR`, i.e. in the shared `core/target`: its three new incremental directories, ~120 MB, deleted after).
- NOT run this round: `check-changed`/clippy (native and wasm32), the viewer modules and build-pkg/pkg_smoke, and the
  seven layout parity groups — **D: fell to 2.3 GB free** during the round (the other agents' builds; B deleted its own
  debug and release trees, 6 GB, leaving 8.4 GB), below the 15 GB stop line. The layout change of this round is in the
  incremental path only (`incr/keys.rs`, `incr/mod.rs`): the parity groups replay full layouts, which it does not touch;
  la7's incremental-equals-full tests and shell_native A/G/H/I cover it.

#### Review fix round 2 (fifth and sixth implementers, 2026-11-13 night)

The reviewer's findings on fix round 1: one major and three minor. The fifth implementer wrote the code, the tests and the
harness scenarios, then was cut off by a usage limit before running the Chromium harnesses. The sixth implementer checked
that work, kept it, rebuilt the modules, ran everything, and closed what the runs showed: the two undo declarations and
the old note test.

- **Notes and text boxes holding a table (major)**: closed in uo-docx. `hf_flat.rs` now covers every story kind the TS
  edits through `flattenBlocks`:
  - A NOTE: `enterNote` sets `this.model = { paragraphs: flattenBlocks(blocks) }`, and `syncNoteEdit` runs
    `reconcileBlocks(blocks, paragraphs, true)`.
  - A TEXT BOX read from a .docx: `tb.paras` = `flattenBlocks(tb.blocks)` (the `Synced` view), and
    `reconcileBlocks(tb.blocks, tb.paras, true)` runs in `liveEngineDoc` and `exitTextbox`.
  - The layout's hit test already flattened both scopes (`scope_paras`).
  - Where the views live: `Notes::flat` (`model/document.rs`, `NoteFlat { footnote, id, view }`, one note in edit at a
    time) and a hidden `flat` field of `DrawingTextbox` (`model/run.rs`). These two and `HfPart::flat` are now
    `model::section::EditSlot`s: session state kept next to the model it views. They are outside the model's equality
    (`PartialEq` is always true), and the JSON round trip of an undo step drops them (`JsNormalize`). So the history, the
    snapshots and the writer see only the blocks.
  - `story_of`, `story_of_mut` and `textbox_story` return the view while there is one. The note's context step in the
    history keeps the BLOCKS (`note_story`), as the header's step does.
  - `Session::sync_flat_views` (was `sync_hf_flat`, at the head of `change_set`) settles three kinds of view, in this
    order: the box in edit first (its edits change a paragraph of the outer story), then the note, then the header/footer
    parts. All three share one `settle_flat`, with round 1's rule unchanged. A box that is LEFT is reconciled and its view
    dropped; `Session::flat_box` remembers which box carries a view.
  - `sync_flat_views` also runs wherever a command indexes a story it has just entered or restored, so the caret lands on
    the flat list: `enter_note`, `reenter_textbox`, `restore_context` (undo/redo) and the header re-entry of an undo
    (`header_footer.rs`). In `enter_note`, a click on a note line inside a cell gives the shell's caret as a flat index.
    Before this fix that index was clamped to the top-level list.
  - A box from the ODF reader (`TopLevel`) keeps editing its top-level paragraphs, as the TS `paras` of that reader do.
  - The `D2a-C1` decision row now covers all three stories, and the line "notes keep the top-level model" is gone.
  - Native tests in `tests/session_wave7_hf_flat.rs`: 2 added, 4 in all.
    - `typing_in_a_note_cell_lands_in_the_cell` is the review's probe. Footnote 1 = a 2-cell table then
      `nota depois`. The test enters at flat (0,2) and types `xy`, Enter, `z`. The saved `word/footnotes.xml` equals the
      frozen TS's BYTE FOR BYTE, both when saved inside the note and after leaving it. Between commands, cell (0,0) holds
      the two paragraphs and the paragraph after the table is untouched. After an undo, the note's list is the flat list
      of the restored blocks.
    - `typing_in_a_box_cell_lands_in_the_cell`: a body box = a 2-cell table then `caixa depois`. The new paragraph lands
      in the box's first cell, before and after leaving the box, and in that order in the saved `document.xml`.
  - `session_d2b_f5::a_note_with_a_table_keeps_it_while_the_note_is_edited_as_its_top_level_paragraphs` asserted the old
    model: the note's story was 2 paragraphs and 1 table. It is now
    `…_edited_through_its_flat_list`: the context story is 3 paragraphs and 0 tables, the note's blocks stay 2 paragraphs
    and 1 table, and the saved `footnotes.xml` keeps `in the cell` inside the table and `last!` after it. The full suite
    found this; it was the only failure.
  - Fixtures in `uo-docx-layout/tests/fidelity/`: `note-cell.docx` and `box-table.docx`, made by the fidelity harness's
    generators (open fonts only), and `note-cell-ts-footnotes.xml`, the frozen TS's saved part from the reviewer's probe.
  - Harness `editor_switch_fidelity.mjs`, all with real clicks and keys:
    - New documents: `note-cell`, `note-plain` (the control) and `box-table`.
    - New scenarios: `note-cell` and `note-plain` run the review's NOTE script (click the first note line, `xy`, Enter,
      `z`). `note-cell undo` runs the same script, then Ctrl+Z, `w` and Escape. `box-table` runs the box script
      `BOX(tb(0))`, and `box-table typing` runs that script without its Ctrl+Z.
    - Results: `note-cell`, `note-plain`, `box-table typing`, `hf-cell` and `hf-cell edge` are equal on every facet,
      saved parts included.
- **Two new declarations: TS defect, Word agrees with Rust (undo inside a note or box that holds a table)**. The first
  `box-table` run differed from the Ctrl+Z on (step 9): the caret, the following click's offset, the screen, the
  thumbnail and the saved `document.xml`. `note-cell undo` differed the same way from its Ctrl+Z (step 5).
  - Cause, in the frozen TS: `popHistory` → `parseState` = `JSON.parse`. After an undo, the box's `tb.paras` and
    `tb.blocks` (the note's model and its blocks) are separate copies. The next `reconcileBlocks` finds none of the list's
    paragraphs in the blocks (it matches by object identity). It puts them ALL before the table and gives each cell an
    empty paragraph.
  - What each saved: the TS saved the box as `caefb` / `caixa celula 00` / `caixa celula 01` / `caixa depois`, then a
    table of two empty cells. It saved the note as `noxy` / `wta celula 00` / `nota celula 01` / `nota depois`, then two
    empty cells. Rust keeps the same texts in their cells.
  - Word, measured through COM (invisible, under `word-lock.mjs`) on the same bytes, scripts
    `target/m2-w7b/word/box-undo.ps1` and `note-undo.ps1`, outputs in `word/out8/`:
    - The box: the same keys up to Bold, then Undo. Cell (1,1) keeps `ab` and `ccaixa celula 00`, cell (1,2) keeps
      `caixa celula 01`, and `caixa depois` stays after the table (`Information(wdWithInTable)` = False).
    - The note: caret at offset 2 of cell (1,1), `xy`, Enter, `z`, Undo, `w`. The text stays in cell (1,1), cell (1,2) is
      unchanged, and `nota depois` stays after the table.
    - Word's COM `Undo(1)` also took back the paragraph break typed with `TypeText`/`TypeParagraph` (one undo unit). That
      is an undo-grouping question that the COM typing cannot settle (keystroke grouping was not measured), and both
      engines agree on it; it is not a parity item.
  - Declared in the harness, with that evidence: `box-table` from step 9 and `note-cell undo` from step 5. Each covers
    the caret, the selection, the step screen, and at the end the thumbnail, the screen and the saved bytes. Every facet
    before the Ctrl+Z is equal.
- **Verification provenance (minor)**: the modules were rebuilt from the final tree. The newest `.rs` was then
  `uo-pdf/src/lib.rs` at 00:43:07; the only later Rust edit is the `session_d2b_f5.rs` test. `verify-e/session_count.wasm`
  was built at **01:35:26** and `verify-e/session.wasm` at **01:36:02** (`target/m2-w7b/tmp/fx3-build.sh`). Every
  Chromium result below ran on those two modules.
- **Lint (minor)**:
  - The full run was over the tree with this round's Rust code, with `CARGO_INCREMENTAL=0`, using
    `target/m2-w7b/lint-allow.sh`: check-changed's steps plus the three Rust 1.100 allowances
    (`chunks_exact_to_as_chunks`, `manual_swap`, `needless_range_loop`). Clippy ran with `-D warnings`, native AND wasm32,
    over uo-docx, uo-docx-layout, uo-ffi, uo-pdf and uo-wasm (`--all-targets --all-features`), then `float-casts` and
    `grep-bans`. **All steps passed** (`fx2-lint.log`, 00:43). The only other output is four manifest-level "unused
    dependency" notes from uo-ffi; they are not lint failures.
  - After the test edit, clippy for uo-docx (`--all-targets --all-features`, the same allowances) and `grep-bans` were
    re-run: both exit 0.
  - rustfmt `--check` is clean on every file B wrote this round: `hf_flat.rs`, `context.rs`, `model/section.rs`,
    `document.rs`, `run.rs`, `normalize.rs`, `header_footer.rs`, `drawing.rs`, the incremental keys and the two tests.
  - The plain `check-changed` (without the allowances) still stopped on the Rust 1.100 lints in unowned crates (the
    toolchain drift of "For the maintainer"); fixed in code at the merge (F3), it passes without allowances.
  - **D-L7 is NOT EXERCISED**: `IncrConfig::phone()` only takes effect when a host passes the phone font profile, and no
    host does today. pages500 memory and latency were not measured again with a phone-profile environment.
- **Editor vitest flake (minor, not B's)**: no change. No TS or `js/` file changed after the reviewer's vitest run, and
  `formatting-policy-bar.test.ts` passes alone. A similar load flake hit `editor_switch_review` once this round (below).

Results of this round (modules as above; harness logs `target/m2-w7b/fx3-h-*.log`):
- uo-docx `--all-features --test all` (test profile, 3 threads): **798 passed, 1 failed, 1 ignored**. The failure was the
  old note test above. After its update, `session_d2b_f5` and `session_wave7_hf_flat` pass 20/20, so the suite stands at
  799 passed and 1 ignored. (No log of that 20/20 run was kept. F1 reran the whole binary into
  `target/m2-w7b/f1/docx-all.log`: 802 passed, 1 ignored — the 799 plus F1's three new tests; `session_d2b_f5` 16/16 and
  `session_wave7_hf_flat` 6/6 among them.)
- uo-wasm natives (`session,count`, run after the hf_flat edits): `shell_native` 9/9, stories 8/8, objects 4/4, caret
  4/4, review 3/3, completion 5/5, format 4/4.
- Chromium:
  - `editor_switch_fidelity --mode all` **ok**: 14 documents, step facets 205. The only declared differences are
    ED-106b ×2, `box-table` and `note-cell undo`. Memory is flat at 4,374,500 live bytes from cycle 5 within this run.
    (Corrected by F1: the memory mode runs only the `cell-lic1` box script, so the number of scenarios cannot change it.
    The baseline after warm-up varies between runs — 3,904,612 in round 1 and again in the reviewer's rerun on the same
    modules, 4,374,500 here — and is flat within each run.)
  - `editor_switch_stories` **ok**: only ED-108 is declared, and memory is flat at 6,682,346. The chrome routing is 640
    equal, with 0 commands left disabled.
  - `editor_switch_objects` **ok**: only ED-304 is declared, and memory is flat at 13,400,333.
  - `editor_switch --mode parity` (stride 1, 12 steps) **ok**, with the same declarations as round 1: gate-O3 ×3, ED-600
    and ED-108. Thumbnails: 1,140 equal + 10 declared. Screens: 167 + 5. Saved parts: 660 + 2.
  - `editor_switch --mode all` **ok**: 100 memory cycles.
  - `editor_switch_review`: the first run, while the uo-docx debug suite held 3 cores, failed with
    `f2-3-0|clipboard`. The bar's paste landed one step late in Rust (step 17 had no events; step 18 had 4 against 2). Run
    again alone, it was **ok** (0 undeclared). That is a load flake like round 1's, and no declaration was added.
- Not run this round: the layout parity groups, the viewer modules, build-pkg and pkg_smoke. No uo-docx-layout,
  uo-fonts, uo-render or viewer file changed this round. D: was at 13–16 GB, near the 15 GB stop line. To stay above it,
  B deleted its own `debug/incremental` and the partial `release` tree under `target/m2-w7b` (2.9 GB).

### Wave 7 — F1: session and shell correctness (fix round after the final review)

The open items of the wave-7 final review that touch the session and the shell (B's major, the A1/A2 minors, A2's handle
leak, B's memory sentence). Logs under `target/m2-w7b/f1/`; modules built from this tree
(`target/m2-w7b/f1/wasm/session_count.wasm`, `session,count`).

Fixes:
- **B major — Edit Text on a box that holds a table** (`shp:editText`, `editSelectedTextbox`): the session computed the
  caret from the box's top-level paragraphs before the box's flat view (`tb.paras` = `flattenBlocks`, `hf_flat.rs`)
  existed, so the caret landed at {para 0, offset 12} and typing went into the first cell (`caixa celulaQ 00`).
  `enter_textbox_at`, `enter_textbox_order` and `ContextCmd::EnterTextbox` now install the view at once
  (`sync_flat_views()`, as `reenter_textbox` and `enter_note` already did). The same cause in `insertTextbox` of a user
  gallery item (the first non-empty FLAT paragraph selected, TS 14788) is fixed by it. The note entry
  (`enter_note`) already synced; the header/footer entries set the caret to (0, 0), which is the same position in both
  views, so they did not share the bug. Native tests `session_wave7_hf_flat::edit_selected_box_with_a_table_puts_the_caret_after_the_table`
  and `::inserting_a_user_box_with_a_table_selects_the_first_flat_paragraph`; Chromium scenarios `box-table edit text`
  (the API) and `box-table edit text (bar)` (the bar's `shp:editText` through the command registry) in
  `editor_switch_fidelity.mjs`: caret {para 2, offset 12} after the table in both engines, `Q` typed there, every facet
  equal.
- **A2 minor — the inline picture selection by identity**: `F7State.inline_owner` keeps the `ParaId` of the paragraph
  the picture was selected in; `settle_float_sel` drops the selection when the paragraph at the stored index is another
  one (the TS `picSel` is the run object). Native test `completion_a2::inline_selection_is_the_run_not_its_index` (a
  body picture at [0, 0] selected, then a cell whose paragraph 0 holds a picture at run 0 under the selection: nothing
  selected, as the TS).
- **A2 minor — `pasteCells` in the Read view**: the shell's `locked()` is now the TS `locked()` (2124:
  `protectedMode || view === 'read'`, and no session), so every shell path guarded by it refuses in the Read view as the
  TS does; `pasteCells` also checks `view === 'read'` as TS 20485. Chromium scenario `read view paste`: `pasteCells`
  answers `false` twice in the Read view in both engines, nothing dirty, then pastes again in Print.
- **Found by that scenario — `setView('read')` inside a table cell**: the TS (2886) leaves the cell for every view but
  Print; the shell left it only for Web/Draft/Outline. `rust-format-view.ts` now leaves it for Read too (`uw-cellmode`,
  the body selection back). Equal in `read view paste` (step 4); the format harness `--mode view` stays ok (1070 equal,
  52 declared as before). The line is in A1's file, outside F1's ownership: the coordinating session confirms it at the
  merge.
- **A1 minor — an `.odt` across a DOM move** (fix round 2; round 1's route withdrawn): an `.odt` session is KEPT
  whole while the element is detached: `park()` releases the shell engine, the layouts and the viewer side as before but
  not the session; `unpark()` adopts the same session again (`adopt(…, reattach)`: selection, dirty, undo history and
  protection stay, no `uw-protected`, as the TS whose model survives the move). A `.docx` is parked as its package as
  before (ED-25). Round 1 parked the `.odt` export and read it back through `openOdt`; the final review measured that
  this loses data on a plain DOM move: the ODF writer has no `text:bookmark`/`text:note` and the reader inlines note
  bodies (a bookmark gone, a footnote back as body text, 76 → 77 characters), and lengths go through cm on both sides
  (writer `t/1440*2.54`, reader 567 per cm), so the page grew ~1.5 twips per move (11908 → 11909 → 11911 → 11912 wide).
  The harness hid it (one move, save('auto') only, a 0.01 cm matcher). The `.docx` route measured in round 1 (no styles
  part over the empty package, ODF text frames dropped) was not lossless either. **ED-F1-1 is retired** (no declaration
  remains; the cm matcher is deleted). While detached `engineStats()` counts the kept session (`sessions` 1, shell
  engines 0, layout handles 0). An element dropped while detached (never re-attached) frees it through the session
  binding's FinalizationRegistry, and an editor-side registry takes it out of `live.sessions`.
  - **Found by that check — the session binding died with its editor**: `startRustEditor` called `bindSession` per
    element, so each binding's FinalizationRegistry (sessions and shell engines) was collected with its editor and never
    ran its callbacks. Measured (`target/m2-w7b/f1/odt-drop-probe.mjs`, CDP `HeapProfiler.collectGarbage`): 20 elements,
    each loaded with the `.odt`, edited and dropped while detached; the JS objects (session, editor, element) were all
    collected, but live bytes grew +33,730 per cycle (3,361,350 → 4,002,220). With one binding per module (a `WeakMap`
    keyed by the module, as the format binding already was) they stay flat at 3,327,620 from cycle 0, sessions 0 after GC
    (`odt-drop-probe.log`); the same probe that frees the kept session explicitly gives 3,327,620 too. The `.docx`
    variant (session freed at park) was already flat (4,374,500).
  - The park step (`editor_switch_fidelity.mjs --mode park`, in `all`): the TS `.odt` of `note-plain`, loaded in both
    engines and edited after the load (`insertBookmark('marca1')`, `insertFootnote()` then `goToNoteRef`,
    `insertTextbox('simple')` then `exitTextbox`). It is moved in the DOM 3 times, then `undo()` runs twice. Before the
    moves, after each move and after each undo, these must equal the TS: `listStyles('all')`/`listStyles()`,
    `styleFormatting`, `getBookmarks`, `noteCount`, `getSection()` (page size), `dirty`, `selection`, the text, the kind
    and every part of save('auto'), and `createStyle`. The Rust answers after each move must also equal its answers before
    the moves. A guard fails the step if the edits are gone after the last move (`marca1`, 1 footnote) or if the undos do
    not change the document (the box, then the footnote: 0 footnotes).
- **A2 not_done — the layout handle after `newDocument()`**: `adopt()` set `this.dl = null` without releasing it. A
  layout of the synchronous relayout (`relayoutNow`: `refresh()` and every edit) is not in the `RustSession`'s set, so
  `adoptDocument` did not free it either: load → `refresh()` → `newDocument()` left one handle per document (the
  harness's `load()` calls `refresh()`, hence "some cycles"). `adopt()` now releases the installed layout first.
  Measured (`target/m2-w7b/f1/leak-probe.mjs`): before, 8 of 8 cycles kept a handle (1, 2, … 8 after each remove); after,
  30/30 cycles at 0 with the refresh and 30/30 without it, live bytes flat (9,826,503 and 9,035,525 from cycle 5). The
  completion harness's memory cycle now does load → newDocument in its first editor: 30 cycles, sessions/engines/handles
  0 after each remove, live bytes flat at 13,829,745, memory.buffer 25,034,752.
- **B minor — the fidelity memory sentence**: corrected in B's results (the memory mode runs only `cell-lic1`; the
  baseline varies between runs — 3,904,612 / 4,374,500 — and is flat within each), and the missing 20/20 log replaced by
  this round's full run.

Results (this tree):
- uo-docx `--release --all-features --test all` (3 threads): **802 passed, 0 failed, 1 ignored** (`f1/docx-all.log`).
- uo-wasm natives (`session,count`): shell_completion 5/5, shell_format 4/4, shell_objects 4/4, shell_stories 8/8.
- clippy uo-docx `--all-targets --all-features -D warnings`: exit 0;
  rustfmt `--check` clean on the touched `.rs`.
- editor/viewer tsc: exit 0, no output. Editor vitest (engine switch config): 34 files / 334 tests passed.
- Chromium (count module above):
  - round 2 (the reviewer's re-run, `target/m2-w7b/f1-rev2`): `editor_switch_fidelity --mode all` **ok**, 17 scenarios, 0
    undeclared, declared ED-106b ×2 + `note-cell undo` + `box-table`; the park step equal after 3 moves and 2 undos; memory
    flat at 4,374,500; the odt-drop probe flat at 3,327,620, sessions 0 after GC; completion memory flat at 14,679,681;
    completion paste 1,068 equal + 1 declared ED-304; `editor_switch --mode all` ok (16,986,957); stories ok, review ok,
    format `file` ok, format memory flat at 8,247,280, objects memory flat at 13,400,333; vitest 34 files / 334 tests; tsc
    exit 0. (Round 1's lines below are superseded by these.)
  - **Wave 7c (S) supersedes the park design** above: nothing is released at a detach any more (no kept-session special
    case for the `.odt`, no `.docx` reopened from bytes); see "Wave 7c — S", item 3.
  - `editor_switch_completion`, each family alone: paste 1068 + 1 declared, lists 1460 + 3, tables 1003 + 1, objects
    1971 + 0, inline 254 + 1, page 134 + 0, previews 199 + 1 (all declared ED-304) — all ok; memory as above.
  - `editor_switch_format --mode view` ok (1070 equal, 52 declared).
  - `editor_switch_format --mode all` ok (memory flat at 8,561,649), `editor_switch_objects --mode all` ok (memory flat
    at 13,400,333), `editor_switch_stories` ok (memory flat at 6,682,346), `editor_switch_review --mode all` ok,
    `editor_switch --mode all` ok (only gate-O3 declared; 100 memory cycles).
- Not run: the layout parity groups, the viewer, build-pkg and pkg_smoke (no layout, viewer or packaging file changed);
  `check-changed.mjs` without the allowances (the Rust 1.100 lints in unowned crates, as the other records say).
- Files: uo-docx `session/ops/drawing.rs`, `session/ops/drawing/textbox.rs`, `session/ops/context.rs`; tests
  `session_wave7_hf_flat.rs`, `completion_a2.rs`; editor `rust-engine.ts`, `rust-completion-paste.ts`,
  `rust-format-view.ts` (one line, the Read view); harnesses `editor_switch_fidelity.mjs` (3 scenarios, the park step, the
  command registry for `bar` scenarios), `editor_switch_completion.mjs` (the memory cycle's load → newDocument).

### Wave 7 — merge state

The final state of wave 7 after the three frontiers (A1, A2, B), their review rounds and the merge fixers F1 (session and
shell correctness), F2 (print, picture and TOC divergences, the A1 harness gaps) and F3 (toolchain, harness hygiene,
records and the final count). Where a subsection above reports an intermediate result, this one is the valid statement.
F3's logs and scripts: `core/target/m2-latency/f3*` (outside git).

**Nothing stays disabled under `engine="rust"`** (the maintainer's rule). Final count over the current sources
(`core/target/m2-wsession/plan-a1/probe.sh`, its two bundles rebuilt from the tree, and `engine-switch.test.ts`):
- public methods **511**: 485 routed, 24 handled by the shell, 2 both (`knownFontFamilies`, `onClipboardChange`) —
  **0 disabled mutators, 0 disabled queries**. Private routed: 27. **Wave 7c (S): the 5 duplicate names are gone** from
  `ROUTED_PUBLIC` (`currentStyles`, `copy`, `cut`, `paste`, `pastePlain` were in the base list and in front R's
  `engine-routes-review.ts`; the base list's copies dropped — the front handlers already won in `installRoutes`, no
  behaviour changed): 489 entries, 489 distinct. `engine-switch.test.ts` now asserts no duplicate in `ROUTED_PUBLIC` or
  `ROUTED_PRIVATE` and prints distinct names over `PUBLIC_METHODS`: "public methods 511: routed 487 (+ 2 indented), shell
  26, of which both 2, disabled mutators 0, disabled queries 0; private routed 27" (487 = 485 routed-only + the 2 both;
  the 2 indented are `setImageLockAnchor` and `setImageEffect`, outside `engine-public.ts`), equal to the probe's.
- bar commands: **462 declared, 462 routed, 0 unrouted** (`cmds-probe.mjs`: every command `registerWriteCommands`
  adds, against `ROUTED_COMMAND_IDS`; that set has 516 ids, the other 54 are pane and menu entries).
- the fail-closed path stays tested: `engine-switch.test.ts` (two synthetic public mutators, A2) and
  `editor_switch.mjs --mode chrome` (F3: a synthetic bar command registered with the component's own commands, before
  chrome.ts's gate, with a button on both bars: disabled and inert under `engine="rust"` — `ran 0` after a click — and
  enabled and run once with the TS engine, the control).

**What still differs from the TS editor** (none is a disabled command; each is declared in the harness tables with its
evidence). Nothing below is a maintainer acceptance: the maintainer has accepted none of these differences, and the
only rulings are the Word-authority ones (gate-O3 among them):
- Word authority over the TS (measured in Word, "Word manda sobre o TS"): gate-O3 (a theme colour without a theme part:
  Word and Rust E97132, the TS C00000), the Subtle Reference look, ED-710 (the text glow scaled by the font size, uo-render
  `fx_for_word`), ED-712 (the TOC gallery's first page number: Word and Rust `5`, the TS `vi`), ED-713 (a chapter-numbered
  PAGEREF: Word and Rust `1-i`, the TS `i`); TS defects with Word evidence: ED-108, ED-600.
- differences at the engine boundary, with no TS behaviour missing (recorded, not accepted by anyone): ED-704 (XPS: the
  engine writes the session's pages; the frozen TS `xps.ts` writes glyphs, lines and solid rects only — `XpsPage` has
  `fills`, `lines`, `glyphs` —, so the Rust package holds more, and the bytes differ), ED-709 (custom-inline data that is
  not plain JSON is kept as JSON text, as the saved `.docx` does), A2's declarations 1 (`pasteData` ignores a TS-model
  `d.rich`), 2 (one Office Clipboard ring per engine on a mixed page), 5 (`editEmbeddedObject` takes a position: a TS
  `Run` object does not exist) and 6 (the harness's `<universal-sheet>` stub), and those of waves 4–6 (ED-6, ED-202b,
  ED-203, ED-205/206, ED-217, ED-601/602 — the event timing, the job runner, the random ids, the clipboard listeners;
  each record says what a host can see). ED-F1-1 is retired (F1, above), not a difference. D-L3 (wave 6, the page buffers
  kept across relayouts; wave 7c V, fix round 1's evidence): a stroke on a rounding tie can come out 1/255 different on a
  canvas whose buffer outlived several paints (the table guide line of `paintTableAdorners`, 2 pixel columns, frames 0–1
  of f7b__7__0 nw and f8__7__11 rot); the drag-frame comparisons allocate every page buffer again in both engines
  (`dragFrame`), so they do not exercise the kept buffers (record "Wave 7c — V, fix round 2").
- **open parity items** (the TS does it, the Rust editor does not yet; not disabled, owner named). Each is a gap under
  the maintainer's rule, none was small enough for the merge fixers (F3's brief: route to an existing session op), and
  none is in F3's files:
  - **ED-707/708 — the presentation equal to the TS, the geometry Word's where measured (V, fix round 2); two parts
    open.** `editor_switch_format --mode view`: 2,888 facets equal, 302 declared (LQ2's line geometry and W's item 4), **0
    undeclared**, with the V5 interaction steps on. Fixed: the Web view lays out again at a selection change after a resize
    (the TS `refreshSelection` → `refresh()`), Word's at once on a resize (measured), the ruler frame read from the element's
    width (the TS 17141); the harness's two stale reads (a detached canvas after the TS rebuilt its canvases; a click point
    measured before the selection scrolled back). Word measured and applied (tests/views_w7c.rs, 171 line starts of 6
    documents equal to Word's in Web, Draft and Outline): the Outline wrap cap (8640 twips), Outline's left alignment, a
    page/column break's own line and the section break's line in Draft/Outline, Word's break rule and label, the Outline
    symbols. Declared as LQ2 (the legacy canvas layout of these views not ported): the line geometry's facets (`rulerLine`,
    `canvases`, the Web screen, the caret after PageDown). **Open**: (a) Word's PageDown (`MoveDown(wdScreen)`, measured)
    is neither engine's — both move 20 lines (the TS rule, every view); (b) f7a__7__0's lines differ from Word's in Print
    too (13 of 326 equal): not a view rule. Record "Wave 7c — V, fix round 2".
  - ~~**A2's declaration 3**~~ — **closed (V; frames on by default since fix round 1)**: the wrap-point drag relays the
    text out live, one undo step, the TS snapshot/discard events; `editor_switch_completion` `blank|objects` compares its
    frames by default, equal with no declaration. Record "Wave 7c — V" (2).
  - **the float move/resize/rotate and Crop drags** — live (no ghost), frames on by default since fix round 1; every frame
    equal in both harnesses but ONE facet (**open, F-1**): `editor_switch_objects` f7b__7__0, frame 0 of the float move, the
    proxy textarea's top (TS 284 px, Rust 258 px). Fix round 2 traced it: the TS's layout of the click's `refresh()` asks
    the resolver for "Times New Roman" for the first time (`loadEngineFonts`, 50 ms), the answer comes back as
    `onFontsResolved` (57 ms) → `queueRefresh` → `refresh` (137 ms) → `positionProxy` (194 ms), after the Shape Format tab
    grew the bar (178 → 204 px at ~94 ms); the Rust shell resolved every family with its first layout and asks nothing
    after the click (no reposition). A reposition on the bar's growth moves f7b__7__1/3, where the TS stays (fix round 1). Not resolved:
    the TS's late face request is not reproduced. Record "Wave 7c — V, fix round 2", F-1. (**Closed** in the WordCraft
    study, page: the proxy follows the caret on every chrome/scroll-container resize; the TS's stale proxy on f7b__7__1/3
    and f8__7__11 is declared — record "Wave 7c — V, fix round 2", F-1.)
  - ~~**A2's declaration 4**~~ — **closed (V, fix round 1)**: the drop caret follows the pointer, and the picture now paints
    on every frame (a page base painted while a picture decoded was reused by the band repaint); `editor_switch_completion
    --mode inline` frames equal by default. Record "Wave 7c — V" (3) and fix round 1.
  - ~~**A2's declaration 7**~~ — **closed in wave 7c (S)**: compared with the TS and measured in Word. The TS keeps the
    table style after insert → save → undo → save (its `pendingTableStyles` is outside the history; its reused package
    keeps the styles part a save wrote), and so did Rust; Word removes it ("Tabela com grade": styles 14 → 13 in the
    `.odt`, 5 → 4 in the `.docx`; deleting the table instead keeps it), and likewise an undone `createStyle`, list,
    odd/even pages, hyphenation and footnote format. Rust now follows Word (the pending map travels with the undo step,
    a save takes back what an earlier save wrote outside the writer: `session/save_parts.rs`), the TS behind
    `OpenOptions::ts_stale_parts` / `ts_table_style_survives_undo` for the strict replays; and the missing
    `pendingTableStyles` branch of a non-editable (`.odt`) or absent style sheet is ported (`addStylesXml`, 8641) — the
    F3 major. Record "Wave 7c — S", item 2.
  - ~~**ED-304**~~ — **closed (V, fix round 2)**: Word's look is the default in EVERY story — "When selected" in the
    header/footer, the footnotes/endnotes and the text boxes (the story line the caret is on); Word measured in the footer
    and a footnote (`v3/word-shading-stories.ps1`: no C0C0C0 with the caret outside, a 17-px band with it inside); the
    band's height from the engine's natural descent (`LineGeom::desc`, exposed in `uo_shell_story_lines`), not estimated;
    `editor_switch_review --mode wordmarks` checks the footer and the note too. The TS look under `tsShading`/`tsStrict`.
    Not measured: Word's black Office theme (an account setting; the dark values are a dark page colour's). Record
    "Wave 7c — V, fix round 2", ED-304.
  - ~~**ED-106b**~~ — **closed in wave 7c (V)**: the box inside a box in edit has its frame and handles (`boxFloat`, the
    layout's page object of that box). Record "Wave 7c — V" (5).
  - ~~the preview bookmark sync~~ — **not a difference (wave 7c, V, evidence)**: every preview mini-document (TOC gallery,
    covers, watermarks, header/footer galleries, table styles, Paste as Picture) lays out with an EMPTY bookmark index, as
    the TS's `bookmarks: new Map()` — 159 previews checked after an edit (`uo-docx/tests/previews_bookmarks_w7c.rs`); no
    stale offset can be read. Record "Wave 7c — V" (6).
  - **Word's layout rules not applied at the merge** (B's "For the maintainer" list and F2's keepLines item) — **wave 7c
    (W)**: four **applied** as "Word manda sobre o TS", each measured in a Word oracle series and asserted by
    `uo-docx-layout/tests/word_w7c.rs`, the frozen TS behind a `ts_*` switch for the strict replays: keepLines taller
    than what is left of the page (item 6, F2's `xiii`), a package without `settings.xml` = mode 12 with 708-tw tab stops
    (item 2), the first box of a linked chain keeps its first lines (item 3), REF/PAGEREF/NOTEREF to an undefined
    bookmark = Word's bold error text (item 4; its saved bold in the editor's lossless read is open, for the writer owner).
    Fix round 1 ("Wave 7c — W, fix round 1"): item 2's 708 tw is a fallback an explicit `setDefaultTabStop` overrides,
    `getDefaultTabStop` answers it, the session's compat reads use mode 12; the format harness's `tables|ruler` and
    print-cell differences attributed to item 2 and declared; `bookmark_where` reads a per-call index.
    Two stay **OPEN** with numbers (not relabelled, not accepted): the theme
    tint/shade rounding (item 1: Word's rule not found; 3,356 samples, the TS rule hits 2,323) and the hyphenation
    patterns (item 5: line splits equal to Word in pt 877/900, en 809/900, es 898/900 paragraphs; closing it needs
    licence-checked pattern sources or the maintainer lifting his directive against exceptions taken from Word's output).
    Record "Wave 7c — W".

**Toolchain (Rust 1.100.0 / clippy 0.1.100, rustup `stable` since 2026-11-12 16:25)**: the internal choice of B's
"For the maintainer" list ("pin 1.99 or fix the sites"), applied: fixed in code, behaviour-preserving, **no allowance
anywhere** (none of the wave's temporary `-A` flags is in a committed file):
- `chunks_exact_to_as_chunks` (64 reported sites, native and wasm32): `as_chunks::<N>()` / `as_chunks_mut::<N>()` with the
  remainder where the code used it (the SHA-256 of uo-testkit, uo-fonts and uo-schema now hash `&[u8; 64]` blocks; CRC-32
  of uo-opc; the cfb mini-FAT and directory; uo-codec `utf16`; the pixels of uo-drawingml, uo-image, uo-render, uo-pdf;
  uo-wasm `render.rs`, `handle.rs`, `shell_format.rs` and four native tests; uo-docx `docx_writer/drawing.rs`, the paste
  tree reader and two contour tests; the uo-docx-layout and uo-jsnum test decoders); `needless_range_loop` (uo-docx
  `doc/props.rs` `for_cells`, uo-docx-layout test `f5.rs`, uo-drawingml `tables/tests.rs`); `manual_swap` (uo-docx
  `js/object.rs` `sort_keys`, `<[u8]>::swap` is const); an irrefutable `let … else` (uo-render `text_fx.rs` `over`).
- uo-jsnum `text_v8` (2 tests failed under 1.100): the std lowercase tables are Unicode 17, the V8 of the frozen TS
  (Node 24.15) has 20 code points without a lowercase mapping (`UNICODE17_LOWER` in `text.rs`: U+A7DD, U+A7E2, U+AB6C/D
  and 16 of U+1DF40–U+1DF7E). `to_lowercase` and `lower_cp` keep them as V8 does (lowercasing the runs between them,
  which gives V8's Final_Sigma contexts: they were unassigned, neither cased nor case-ignorable). `text_v8` 12/12.
- rustfmt 1.100 reformats HEAD lines (no code change): uo-docx `session/io.rs`, `ops/text.rs`, `settle.rs`,
  `tests/all.rs` (the new module sort order), uo-pdf `render.rs`, uo-render `text_fx.rs`, uo-wasm tests `write_native.rs`
  and `render_native.rs` (formatted one file at a time through stdin, so no `mod` child was touched).
- cargo 1.100's `unused_dependencies` (a warning, not a failure: `check-changed` passed with it) reported four uo-ffi
  dependencies on wasm32 (`uo-alloc` normal and dev, `uo-docx-layout`, `uo-pdf`), where the crate is empty
  (`#![cfg(not(target_arch = "wasm32"))]`). F3 round 2: uo-ffi's normal dependencies and its `uo-alloc` dev-dependency
  are now `[target.'cfg(not(target_arch = "wasm32"))'…]` tables; `Cargo.lock` unchanged; uo-ffi clippy `--all-targets
  --all-features -D warnings` native and wasm32 with no warning.

**Harness hygiene (F3)**: `editor_switch.mjs` — the chrome probe above (the bundle exports the toolbar's
`CommandRegistry` through a dynamic import, so phase 0's static closure is unchanged); the Format Painter and the font
colour are checked enabled. `editor_switch_completion.mjs` DECLARED_C — `'f2-3-0|tables': FIELD_SHADING` (ED-304, the
473 px footer band; the thumbnails and every step facet equal). The retirements of B's re-verification hold in three runs
each: `MARKER_CSS`/ED-C1 (completion lists ×3 ok), ED-219b `f7b__7__8|objects` and `f7b__7__0|menu` (objects objects and
menu modes ×3 ok), f2-3-0|tables (×3 ok).

**Final numbers** (F3; the session_count module built from the tree at 03:57, `target/m2-latency/f3/session_count.wasm`,
10,396,603 B):
- `node tools/check-changed.mjs --jobs 3`: **all steps passed** (16 crates: clippy `-D warnings` native and wasm32,
  `--all-targets --all-features`, float-casts, grep-bans); whole-workspace clippy native and wasm32: 0 warnings.
  Re-run at 05:15 over the tree of that moment (157 changed files; F1 and F2 still editing): one failure, the new F2 test
  `uo-docx-layout/tests/toc_pages_w7.rs:61` (`str::trim_start`, banned by clippy.toml) — F3 changed that line to
  `uo_jsnum::text::trim_start` (the test 4/4), then **all steps passed**. The routing probe, tsc and the editor vitest were
  re-run at 05:11 on the same tree: the same numbers as above (511 / 0 / 0; 462 / 462 / 0; 34 files / 334 tests).
- tests: uo-testkit, uo-codec, uo-opc, uo-fonts, uo-drawingml, uo-image, uo-render, uo-pdf, uo-jsnum, uo-schema
  (`--all-features`): **1,141 passed, 0 failed**; uo-docx `--release --all-features --lib` 153 passed and `--test all
  --skip memory_` 657 passed, 0 failed, 1 ignored (the 500-page memory soaks are the orchestrator's).
- editor/viewer tsc exit 0, no output; editor vitest (engine-switch config) **34 files / 334 tests** passed;
  `engine-switch.test.ts` alone 4/4 in four of five runs — one run under the other agents' load failed "roteados chegam ao
  motor, desligados não tocam o modelo TS" and passed on the three reruns (a load flake, no change made).
- Chromium: `editor_switch.mjs --mode all` **ok** (parity 13 documents × 12 steps: every step facet equal, thumbnails 214
  + 2 declared, screens 28 + 1 declared, saved parts 133 equal; 100 memory cycles; keys; chrome as above);
  `editor_switch_completion.mjs --mode all` **ok** in 455 s (lists 1,460 + 3, paste 1,068 + 1, tables 1,003 + 1, objects
  1,971 + 0, inline 254 + 1, page 134 + 0, previews 199 + 1, every declared one ED-304; memory 30 cycles flat at
  15,073,265 live bytes, `memory.buffer` 26,607,616, handles and sessions 0 after each remove);
  `editor_switch_objects.mjs --mode all` **ok** in 499 s (page 3,802 + 2, objects 5,001 + 0, pictures 854 + 1, menu 219 +
  0, keys 170 + 1, views 361 + 1, all ED-304; memory flat at 13,400,333).

### Wave 7c — V: the continuous views, the live object drags, the overlays of every story, the nested box, the previews

Frontier V of wave 7c (one implementer; the planner's report and the brief: the merge-state items ED-707/708, A2's
declarations 3 and 4, ED-304, ED-106b and "preview bookmark sync", plus the same-class float/crop live drags the planner
found). Scratch, logs and the Word scripts: `core/target/m2-latency/v-*`, `v1/`, `v-plan/` (outside git); the Word
screenshots in `E:\UniversalWriteTest\_lab\views`. Nothing committed.

**V1 — Word measured** (Word 16, invisible COM under `tools/parallel/word-lock.mjs`, the planner's guarded
`word-views.ps1` copied to `v1/word-views-v1.ps1`: the window 1100 × 1300 at −20000, `foreground unchanged=True`; 96 dpi,
zoom 100; `GetPoint` of the first character of each paragraph). Documents made by `v1/mkdoc.py` (open fonts, no embedded
font), committed as `uo-docx-layout/tests/fidelity/v1-outline.docx` and `v1-breaks.docx`:

| `v1-outline.docx` paragraph (x from the window, pane at 8) | Draft (1) | Web (6) | Outline (2) |
|---|---|---|---|
| body before any heading, `w:ind/@left` 720 | 80 | 86 | 80 |
| Heading 1 (`outlineLvl` 0) | 32 | 38 | 56 |
| body | 32 | 38 | 80 |
| Heading 2 | 32 | 38 | 103 |
| body, `w:ind/@left` 1440, `before` 480 | 128 | 134 | 127 |
| Heading 3 | 32 | 38 | 150 |
| body | 32 | 38 | 174 |
| body with a direct `w:outlineLvl` 1 | 32 | 38 | 103 |
| body (× 2, the first wraps) | 32 | 38 | 127 |

Read: Draft puts the text 24 px into the pane and keeps the indents and the paragraph spacing (29-px pitch of the 11-pt
body with `after` 160 / `line` 259, as Print); Web 30 px in, the same content rules at the window's width; Outline
ignores the paragraph's indents and spacing: a heading of level *n* at Draft-left + 24 + 47·(*n*−1) px, body text 24 px
under the last heading before it (a body paragraph before any heading as under level 1), 17-px lines (single).
`v1-breaks.docx` (the second document ran with a tiny window: partly n/a): in Draft and Outline the text after a page or
column break continues on the next line of the same pane (no new sheet). The planner's V1 (f2-3-0, f7a__7__0, f8__7__0)
holds the rest used here: Web wraps at the window and keeps the list indents; Draft/Outline draw a page/column break as
a dotted line with its label right-aligned; Outline's ⊕ (heading) / ○ (body) 18 px left of the text; field shading
"Always" shades the footer fields too (C0C0C0, a text-height band), Word's default is "When selected".

**(1) ED-707/708 — closed.** The editor's Web, Draft and Outline are ONE sheet now, a new engine view
`ViewTransform::Continuous(ContinuousView { kind, width_tw, margin_tw, ts_strict })` (uo-docx-layout `options.rs`,
`view_transform.rs` `continuous_document`, `engine/flow.rs` behind `ViewTransform::is_continuous`): every section on the
sheet (continuous starts, no columns, no title page, no headers/footers), height `CONT_SHEET_H` = 100,000,000 twips (the
TS legacy layout had none: one logical page), margins = the TS `contMarginTwips()` (600, 240 narrow), the sheet width =
the TS canvas (Web: `contentWidthTwips() + 2 × margin`, universal-write.ts 10777/21559; Draft/Outline: the last section's
page width, widened to the widest section's text + 2 × margin), a page break, a column break and `pageBreakBefore` stay
on the sheet (flow.rs: `page_break_before` and `break_after` ignored in the continuous view). Content = Word: each
section at its own text width (Draft/Outline); Outline by outline level with Word's numbers (`OUTLINE_HEADING_INSET` 360,
`OUTLINE_LEVEL_STEP` 705, `OUTLINE_BODY_INSET` 360 twips; the paragraph's indents replaced, spacing 0, single lines —
`word_outline_document`, the keys written through `obj::set` and the editor props matched to the new pPr, as
`direct_ppr` reads both). `ts_strict` keeps the frozen TS's rules where Word differs (every section at the last one's
text width; Outline = the TS `+720` rule, spacing kept) — set by the harness only (`ed.tsStrict = true`). The viewer's
`ViewTransform::Web`/`Draft` and the f8 sweeps are unchanged (the viewer's views.ts and `uo-wasm write.rs` untouched).
Shell (`rust-format-view.ts`): `applyView` calls the new `uo_shell_engine_set_continuous` with the element's own
`contMarginTwips()`/`contGutterPx()` and the TS Web width; the sheet is shown to the content + the TS margin (the TS
`continuousHeight = y + 600`); the strips (`rust-format-tiles.ts`) are cut as the TS cuts its tiles (`contTileBreak`: a
strip ends at the top of the first line whose bottom passes 4000 CSS px; contiguous; past the last line plain 4000-px
cuts) and exposed as `ed.continuousCuts(i)`; `goToPage(n)` in those views scrolls to strip n (the TS tiles, 18542; no
caret move, as the TS without an engine line); `currentPage()` 1, `layoutPageCount()` 0 (A1's, unchanged); the ruler is
null in Outline (A1's). Word manda sobre o TS (V1): in Draft and Outline the page/column/section breaks are always drawn
(the dotted line and label of `paintBreakMark`; the TS only with Show Marks — `tsStrict` keeps it), and Outline paints
Word's symbols (⊕ heading with text under it, ⊖ without, ○ body; 270 twips left of the first line; the TS has none —
`tsStrict`). Collapse/expand and the Outlining tab: the frozen TS has neither (no outline code besides
universal-write.ts 12582/17122; `promoteHeading`/`demoteHeading` stay A1/R's routed commands) — not added.
Harness (`editor_switch_format.mjs`, family `view`): `VIEW_DECL` deleted; the view facets split into the presentation —
`present` (the first sheet's CSS width and `offsetLeft`, the text's left inset from the sheet at (0, 0), `currentPage`),
`ruler` (the frame, page geometry, indents, tabs), `pages` — compared strictly (TS vs Rust with `tsStrict`), and the
content geometry — `rulerLine` (the focus line's top) and `canvases` (the sheet/tile heights) and the Web screen —
declared once per document as `WORD_VIEWS` (id "Word manda sobre o TS (V1 measurement)", the numbers above); a strip
check fails any gap or strip over 4000 px (no declaration covers it); new steps type, Enter, Backspace and two zoom
changes in the Web view. Native: `uo-docx-layout/tests/views_w7c.rs` (the engine's Outline and Draft first-line x of all
ten paragraphs equal Word's to ≤ 1 px — every one exact to the pixel —, the Outline pitch 16.9 px vs Word 17, Draft
28.9 vs 29; `v1-breaks.docx` on one sheet in all three kinds) and unit tests in `view_transform.rs`.

**(2) A2's declaration 3, and the float/crop drags of the same class — closed.** One mechanism for every live gesture,
uo-docx `ShellEdit::LiveBegin` (the TS `snapshot()` at the mousedown: one undo step, dirty, `uw-change`),
`LiveSet(LiveForm { wrap, size, offset, page_at, rot, crop })` (the model fields the TS writes on every move, in place:
the anchor's `wrapPoly`/`wrapBox` and the format's `polygon`; `widthTwips`/`heightTwips`, the offsets, `moveAnchorTo`'s
page-relative `posH`/`posV` or `simplePos`, the rotation of the picture style / box / shape style / group, `pic.crop`; no
step, no event, the paragraph touched for the relayout), `LiveEnd(commit)` (the existing `SetWrapPolygon`/`CommitDrag`
writer inside the gesture's step — `collapse_to` keeps the mousedown's state —, `uw-change` once) and `LiveCancel`
(`discardSnapshot()`: the step goes, a document it made dirty is clean again). Wire: `uo_shell_edit` JSON `liveBegin`,
`liveSet`, `liveEnd { commit }`, `liveCancel`. Shell: `wrapPointsMouseDown` (rust-completion-objects.ts),
`startFloatDrag` and `startCropDrag` (rust-objects-pointer.ts) begin at the mousedown, send the live form once per
animation frame (the TS `refresh()` per frame) and end/cancel at the mouseup; the dashed ghost is gone for the
picture/shape move, resize, rotate and crop (`ghost: false`: the object itself moves live, as the TS paints it); the box in
edit keeps its handles at the dragged rectangle (`editingTb.rectTwips`); the crop overlay reads the live crop (the TS
`cropGeom` reads `pic.crop`); a group's commit carries its start size (`groupStart`, the TS reset 13893). The click
without a drag is the real snapshot/discard now (ED-202b's replayed `snapshotDiscardEvents` deleted).
Tests: `uo-docx/tests/live_drags_w7c.rs` (in `tests/all.rs`): 25 live forms + the commit = 1 undo step, undo gives the
package of the mousedown back byte for byte, redo the end; `LiveEnd` equals the one-shot `CommitDrag` and the one-shot
`SetWrapPolygon` byte for byte in `save_docx`; cancel: history depth and dirty as before (clean again, `DirtyChange
false`); events LiveBegin = [dirty, change], LiveSet = [], LiveEnd = [change]; 1,000 live forms: 400,000 B allocated,
0 B left.

**(3) A2's declaration 4 — closed.** `startInlineDrag`: on every move with a hit the caret goes to the point (the session's
`setSelection`, applied without the caret scroll: paint, proxy, `onSelectionChange`, `chromeSync` — the TS
`refreshSelection()`), which drops the picture's selection as the TS `selectedFloat()` does once the text selection left
the run (13550); a move without a hit clears the target (`target = this.hitTest(ev)`); without a drop the old selection
comes back (13928). The drop stays `MoveInlineRun` (its undo selection unchanged).

**L5 — frame by frame.** `editor_switch_completion.mjs` and `editor_switch_objects.mjs`: a drag step with `frames: true`
does the same six pointer moves as `{ steps: 6 }` and captures after the down and after each move (two animation frames
and the Rust idle): the selection, the proxy textarea's left/top/height, the `onSelectionChange` calls, the uw-* events
since the drag began and the hash of every page canvas — compared as facets `frame k sel|proxy|selChanges|events|pages`
with no declaration. Completion: `wrap:1`, `float:0`, `handle:0:se`, `handle:0:rot`, the crop `handle:0:w`/`n`, the two
`inline:0` drags; objects: `float:0`, `handle:0:se`/`nw`/`rot`. The memory modes keep their inline and float drags (now
the live path).

**(4) ED-304 — closed.** New export `uo_shell_story_lines(h, layout, page, fallback)` (shell_review.rs, JSON): the lines of
EVERY story on the page — the TS `eng.lines` (installEngineResult 16515: body, body table cells at any depth, the
header/footer parts the page shows, footnotes/endnotes; `other` skipped; `collapseCustomChars` applied) — then the text
boxes' content lines in their frame (`xf`; a follower of a linked chain in its head's content, the head = the lowest
`seq`), each with its paragraph once (key, text, runs `{ n, f, c, sp, lang }`, bookmark marks). Keys: `b<para>`,
`c<anchor>/<row>.<col>.<block>…|<k>` (the TS search location: the grid column, the cell's paragraph index), `h…`, `n…`,
`t…`. The shell (`storyLines` in rust-review-core.ts) feeds `paintEngineMarks` (field shading, comment highlight; the
boxes' content excluded as the TS), `paintBookmarkMarks` (every story), `paintEngineSpelling` (every story and the boxes'
content) and `paintSearchHits` (hits keyed by story paragraph: body and cells, the TS `searchHits` map), and `search()`
answers the page of a hit in a cell (the TS `pageOf` over `eng.lines`: f2-3-2's cell hit 0, was −1). F2 (Word's colour
C0C0C0, the text-height band, the "When selected" default): NOT applied — see "Not done".
Declarations removed: completion `FIELD_SHADING` (7 entries), objects `HF_SHADING` (4) and `blank|keys`, review
`f2-3-0|*`, the `f2-3-0|styles` ED-304 part, `f2-3-2|find`, `f8__7__0|spell`, format `f2-3-2|text`'s ED-304 end facet;
the review harness's overlay mask covers the whole page now (headers, footers and notes included). Native:
`shell_review_native` `d_story_lines_cover_every_story` (f2-3-0: body + footer lines and its field runs; f8__7__0: body,
header/footer and note lines; M1 0 B left).

**(5) ED-106b — closed.** uo-docx `Session::edited_box()` (the box in edit's drawing id, `rot`, and whether it is one of
the base story's own drawing runs); `uo_shell_objects` answers `boxFloat` for a box inside a box — the layout's page object
of that drawing with laid-out content (the TS `tbEdit.rectTwips`, 16470–16497), `i` −1 — and `activeHandle` takes it when
the box is not in `floats`. The two `box-in-box outer/inner` declarations deleted; the fidelity header's "Word opens it"
corrected (Word's COM `Documents.Open` fails, RPC 0x800706BE). The fixture is committed as
`uo-docx-layout/tests/fidelity/box-in-box.docx`; native `shell_objects_native` `e_box_in_box_has_a_frame` (the outer box
in `floats`, the inner as `boxFloat` at x 3823.5, y 1577.7, 1889.8 × 629.9 twips).

**(6) Preview bookmark sync — not a difference (evidence).** Every preview mini-document lays out with an EMPTY bookmark
index, as the TS's `bookmarks: new Map()` (7118 TOC, 12113 covers/watermarks, 18505 header/footer, 7874 Paste as
Picture, 20182 table styles): `uo-docx/tests/previews_bookmarks_w7c.rs` builds, after an edit that moved the bookmarks
of `ref-after-note.docx`, every cover, watermark and header/footer gallery item in pt/en/es, both table-style kinds, the
three TOC templates and a Paste as Picture of the paragraph holding `REF bm1`: 159 previews, every index empty, the
session's own index untouched. The layout reads bookmarks only through `DocView::bookmarks()` (no mark scan).
Not captured: the frozen TS's text of a REF inside a Paste-as-Picture preview (its empty map gives the cached result; the
Rust default follows W's `ts_ref_bookmarks` rule there).

**Files** (V's own: `rust-format-view.ts`, `rust-format-tiles.ts`, `rust-completion-objects.ts`, `rust-objects-pointer.ts`,
`rust-objects-core.ts`, the view/objects/completion scenarios and frames of the harnesses). Outside them (each additive,
re-read before every edit): uo-docx-layout `options.rs` (`ViewTransform::Continuous`, `ContinuousView`, `ContinuousKind`,
`is_continuous`), `view_transform.rs` (`continuous_document`, `word_outline_document`, the Outline constants, tests),
`engine/flow.rs` (two gates), `tests/views_w7c.rs`, `tests/fidelity/{box-in-box,v1-outline,v1-breaks}.docx`; uo-docx
`session/ops/shell_edits.rs` (the live variants, `LiveForm`, `shell_wrap_polygon` split out), `session/ops/drawing.rs`
(`F7State::live`, `Session::edited_box`), `tests/live_drags_w7c.rs`, `tests/previews_bookmarks_w7c.rs`, `tests/all.rs`
(two `mod` lines); uo-wasm `session/shell_review.rs` (`uo_shell_story_lines`), `session/review.rs` (`skip_run`/`no_proof`
`pub(crate)`), `session/shell_stories.rs` (`cell_ref` `pub(crate)`), `session/shell_objects.rs` (`boxFloat`),
`session/shell_completion.rs` (the live JSON), `session/shell_format.rs` (`uo_shell_engine_set_continuous`), `js/session.mjs`
+ `.d.ts` (`storyLines`, `StoryLine`/`StoryPara`/`StoryLines`, `boxFloat`), `js/session-completion.d.ts`,
`js/session-format.mjs` + `.d.ts` (`setContinuous`), `tests/shell_review_native.rs` (test d), `tests/shell_objects_native.rs`
(test e); editor `rust-review-core.ts` (`storyLines`, `searchLocKey`), `rust-review-overlays.ts`, `rust-review-find.ts`
(search hits/page, `goToPage`), `rust-review-spell.ts`, `rust-engine-format.ts` (`continuousCuts`), `write-session.d.ts`,
`write-session-objects.d.ts`, `write-session-completion.d.ts`, `write-session-format.d.ts`; harnesses
`editor_switch_{format,completion,objects,review,fidelity}.mjs`.

**Verification** (`target/m2-latency/v-verify.sh`, `v-parity.sh`; logs `v-verify-*.log`, `v-parity-*.log`; `-j 3`, ≤ 3 test
threads, the one target `core/target/m2-latency`; D: 52 → 44 GB free). The tree changed under the run: frontiers W and S
edited the layout engine and `rust-engine.ts` at the same time (W's Word-authority layout switches, S's detach lifecycle),
so the full harness runs at the end mix their open work with V's; V's items were each verified in isolated runs as they
landed, and the end runs are classified below.
- Rust: uo-docx `--test all -- previews_bookmarks_w7c:: live_drags_w7c:: completion_a2::` 17/17; uo-docx-layout `--test f8
  --test views_w7c` 6 + 2, `--lib` 13 (`view_transform` 4 of them); uo-wasm natives `shell_format/completion/objects/
  review/native/stories` 4 + 5 + 5 + 4 + 9 + 8 = 35 passed (corrected in fix round 1: the suites run format 4, completion 5,
  objects 5, review 4, shell 9, stories 8; the first count had the labels out of order) (after one fix of the new review test's `u32` page);
  `shell_review_native` d: f2-3-0 kinds body/cell/hf with 8 field runs, f8__7__0 body/hf/note with 15, M1 0 B left.
  `rustfmt --check` on every V-touched `.rs`: clean (formatted through stdin). `check-changed --jobs 3`: fails in
  uo-docx-layout on W's open lines only (`engine/chain.rs:60` `f64::min`, `engine/flow.rs:824` unfulfilled
  `too_many_lines` expectation); V's one finding there (`view_transform.rs` `&*ppr`) fixed.
- TS: the strict `tsc` line exit 0; `editor_switch_tsc.mjs` editor and viewer exit 0; vitest editor 34 files / 334 tests,
  viewer 7 files / 28 (+1 skipped).
- Wasm: session_count, session, selftest, viewer built (`target/m2-latency/v/`); `build-pkg font-files document-viewer
  write` with `UO_REQUIRE_ENGINE_WASM=1` (the clean `--features write` viewer module and the session module of this
  target) exit 0 (`uo-write.wasm` 4,043 KB, `uo-write-editor.wasm` 10,242 KB) and `pkg_smoke` ok.
- Layout parity (frozen TS captures, strict): ci 200, f7a 300, f7b 200, f7c 300, f7d 200 documents equal, 0 mismatches
  ; f8 200 documents (3,000 facets equal); the Word oracle 2,742/2,742 (8,226 facets), 0 mismatches.
- `viewer_switch --mode all` (viewer.wasm, stride 1): **ok** — 78 documents, 592 pages, 0 differences in pages, text layer,
  find, pixels (tolerance 0) and PDF; the Web view of the viewer unchanged.
- Isolated runs of V's items, each with the declarations removed: `editor_switch_fidelity --mode boxes` box-in-box outer
  and inner **ok** (36 selections, 36 caret rects, the step and end screens, the thumbnails and the saved parts equal;
  "declared differences used: none"); `editor_switch_review --mode all` **ok** in 121 s (f2-3-0|*, f2-3-0|styles,
  f2-3-2|find, f8__7__0|spell: squiggles, page base and the whole-page overlay mask equal); `editor_switch_completion
  --mode all` **ok** in 385 s (FIELD_SHADING gone; memory 30 cycles flat at 13,829,745, handles and sessions 0);
  `editor_switch_format --mode view` **ok** on all four documents (the `present`/`ruler`/`pages` facets strict).
- The end runs (the mixed tree): format view and the other families pass except `tables|ruler` (a cell's ruler frame
  x1 3485 vs 10205 in Print), `fields|text` and `fields|view` (the TOC/REF text: 196 vs 130 characters, the field
  shading over W's Word REF error text), the `extra TOC`/`updateFields` checks (W's ED-712/713 and REF items) and the
  `exportXps` print-cell case; objects pass every family except the page counts after `setColumns` of
  f7a__7__0/f7c__7__2/8 (TS 11 vs Rust 12–13: W's keepLines item 6) — `blank|keys` and the f2-3-0/f7c pictures/views/page
  screens equal with HF_SHADING removed; review passes except `fields|fields` (updateFields 9 vs 11, W); fidelity boxes,
  stories and `editor_switch --mode all`/`parity` differ in caret x after Tab in boxes, pagination and REF text (W's
  layout switches) — none on a V facet. (Superseded — fix round 1 reran the memory modes on the current tree: objects 30
  cycles flat at 13,400,333 live bytes, completion flat at 13,829,745, handles and sessions 0 after each remove; the
  one-handle-per-cycle reading of this run was S's detach work in progress, gone with S's GC step in the harnesses.)
- After V's last shell changes (the mousedown/mouseup callbacks of the drags, the bare snapshot), frames off:
  `editor_switch_completion --mode all` lists 1,462 + 1, paste 1,067 + 2, tables 1,004 + 0, objects 1,969 + 2, inline
  255 + 0, page 134 + 0, previews 200 + 0 — the differences: f7c__7__2|lists (a page-10 thumbnail: W's pagination),
  blank|paste (`naturalPt` of a changed picture read before its decode), f7d__7__10|objects (the linked boxes' chain: W's
  item 3, dumped thumbnails: the same model, the text in other boxes); `editor_switch_objects --mode all` pictures 855 +
  0, keys 171 + 0, views 362 + 0, objects f7b/f8 documents equal, the f7d__7__0/3/10 objects and f7d__7__3 menu
  differences (W's linked boxes: a click lands on another object) and the page counts after `setColumns` (W's keepLines).

**Not done / open (under the maintainer's rule these stay open, not decisions):**
1. **L5 frames** — the `frames: true` drags compare frame by frame only with `UO_EC_FRAMES=1` / `UO_EO_FRAMES=1` (off by
   default so the families keep their end-state checks while the frames are open). Measured with the frames on, after
   the mousedown/mouseup sequences were matched to the TS (the mousedown's snapshot and the no-move discard apply no paint,
   proxy or selection callback; the float mousedown raises the TS's `onSelectionChange`/`chromeSync` once, an already
   selected inline picture is refreshed as `selectInlineRun` does; the mouseup always runs the callbacks and `refresh()`):
   completion `blank|objects` (the wrap-point drag, the float move, the se resize, the rotation and both crop drags) 868
   facets equal, 0 different — every frame's selection, proxy, callbacks, events and page pixels; objects f7b__7__8 and
   f8__7__5 every frame equal. Still different: completion `blank|inline` — the page pixels of all 7 frames of the first
   inline drag (selection, proxy, callbacks and events equal); objects f7b__7__0 — frame 0 of the float move (the proxy top
   284 vs 258 px and one more allocated page canvas in Rust) and frames 0–1 of the nw resize (page pixels). Not resolved
   here.
2. **F2 (ED-304 against Word)** — Word's field shading colour (C0C0C0 behind the glyphs, a text-height band) and its
   default "When selected" are measured (planner) but not applied; the shell keeps the TS's always-on rgba(0,0,0,.10)
   over the line height. Applying it means a Word default plus `tsStrict` in every harness screen that shows a field.
3. **V1 not complete**: Word's continuous views were measured at zoom 100 and one window width; zoom 200, an 800-px
   window, sections of different widths/orientations, floats/notes in the views and the exact break-line drawing (dash,
   label font) were not. The Draft/Outline break lines use the TS `paintBreakMark` look; the Outline symbols' geometry is
   approximate (planner's positions); the TS chrome (sheet, 48-px gutter, 600-tw margin) is kept where Word's pane is
   white edge to edge (24/30 px insets) — the planner's scope question for the coordinator stands.
4. **Collapse/expand and the Outlining tab** — not in the frozen TS (no outline code besides universal-write.ts 12582/17122),
   so not added; Word has them.
5. **The continuous views' interaction coverage** — typing, Enter, Backspace and zoom are in the harness; clicks at caret
   points, Shift+click across a strip, PageDown, scrolling to a strip and an element resize are not.
6. **Typing latency and a 500-page document in the continuous views** — not measured (a continuous view relays the whole
   document out per edit: `FallbackReason::View`, as before).
7. **P1** — the frozen TS's text of a REF inside a Paste-as-Picture preview was not captured (evidence is the empty index).
8. **A linked text box's spelling** in a follower box resolves through the chain head chosen as the lowest `seq`
   (heuristic, not the TS `textboxChain`).
9. The memory soak has no wrap-point drag (the float and inline drags of the memory modes run the live path).

### Wave 7c — S: session, save and shell divergences

Frontier S of wave 7c (a planner, then an implementer cut off by a usage limit, then a second implementer that kept its
edits, finished the plan and verified everything). Items: F2's open major (the `updateFields` count), A2's declaration 7
with F3's major (the table style of a non-editable or absent style sheet), F1's minor and ED-25 (the element across a DOM
move), the duplicate `ROUTED_PUBLIC` names, the compact bar's clipboard check, F1's stale Results block. Scratch, probes,
Word scripts and logs: `core/target/m2-w7b/s-plan/` (planner: `probe-s.mjs`, `routes-dups.mjs`, the Word `.ps1` and
`w-out/`) and `core/target/m2-w7b/s7c/` (implementers: `verify-s.sh`, `verify.log`, `v-*.log`, `h-*.log`, `h2-*.log`,
`word-settings-undo.ps1`, `w-out/`), outside git. Nothing committed.

**Item 1 — `updateFields()` returns the TS count (F2 major).** F2 had made the session's TOC entry label Word's formatted
PAGEREF (`prepare_pages` → `page_numbering_of`, `PageNumbering::label`), so the refresh that ends `updateFields` found
the label already right and counted nothing (TS 4, Rust 1 on `toc-chap-chapter-roman`). Reverted: the label is the TS
`String(page.number)` again (`pageLabelOf`, universal-write.ts 6620–6624; `toc_cmds.rs` `prepare_pages`, the struct and
`page_numbering_of` deleted), and F2's `OutlineParagraph::chapter` (uo-docx `word/outline.rs`, outside `session/**`) is
back to HEAD — the revert of F2's addition. ED-713 lives only in the layout's PAGEREF (`bookmark_pages`,
`LayoutOptions::ts_pageref_without_chapter`). The refresh that ends a TOC command rebuilds the bookmark index and the
sections first (`fields.rs`, the TS layout pass `buildEngineDoc`/`syncSections`). Measured (probe A and the format
harness's new `extra updateFields` block; n1/n2 = first/second call on a fresh load):

| document | `('all','all')` TS / Rust | `('all','pages')` TS / Rust |
|---|---|---|
| `f7a__7__0` (para 31 → Titulo1, `insertTocTemplate('auto1')`) | 2,2 / 2,2 | **1,0 / 0,0** — ED-712 |
| `toc-chap-chapter-roman.docx` | 4,4 / 4,4 | 3,0 / 3,0 |
| `toc-chap-roman.docx` | 4,4 / 4,4 | 3,0 / 3,0 |
| `toc-chap-chapter.docx` | **1,1 / 4,4** — ED-713 | 3,0 / 3,0 |

The two remaining differences are consequences of Word-authority items, declared with these exact numbers in
`editor_switch_format.mjs` (anything else fails; a declaration no run meets fails as stale): ED-712 — the TS insertion
leaves a stale first label (`v`) that its refresh rewrites to Word's `5`, Rust already wrote `5`; ED-713 — the TS layout
has no chapter, so its rebuilt entries' `1` stays `1` (only the TOC field counts), the Rust layout writes Word's `1-1` in
the three entries. Native: uo-docx-layout `toc_pages_w7.rs` `update_fields_counts_follow_the_ts_algorithm` (default:
chapter 4, chapter-roman 4, roman 4; `ts_pageref_without_chapter`: chapter **1** = the TS's measured 1, the others 4;
`TocMode::Pages` 3 everywhere); its module header and the `viii` comment corrected (F2 minors). d2b-F10 replays: the two
ED-713 acceptances on `hfopts/chap-*.docx` dropped (native `parity_d2b_f10_session.rs`, wasm `session_replay.test.mjs`):
f10 **0 mismatches, 53 accepted** (was 55). `viewer_switch.mjs`: `WORD_AUTHORITY` entries take an optional `pages` map;
ED-713 on `toc-chap-chapter` and `toc-chap-chapter-roman` is scoped to page 0 for textLayer and pixels (a mismatch on any
other page fails), pdf stays per document (one hash of the file).

**Item 2 — the table style without an editable style sheet; parts after save → undo → save (A2 declaration 7, F3
major). Word manda sobre o TS.**
- Ported (TS 1:1): `tableStyleId`'s `else` branch (universal-write.ts 19283–19293: a non-editable `.odt` sheet or no styles
  part puts the built-in style in `pendingTableStyles`) — `Session::pending_tables` (an insertion-ordered map, the TS
  `Map` semantics) in `ops/tables.rs` `table_style_id`; `addStylesXml` (docx-core table-styles.ts 149–171, called last by
  the save, 8641) — `session/save_parts.rs` `add_styles_xml` (creates `word/styles.xml` with the TS namespace, the content
  type override, the relationship with the first free `rId<n>`; appends only the ids not already defined; replacements
  by closure). Before: a Rust `.odt` saved as `.docx` after `insertTable` had `<w:tblStyle w:val="Tabelacomgrade">` and no
  definition anywhere (3 parts, the TS 5), and so did `hyperlinks.docx` (no styles part).
- The `.odt` saves into a fresh empty package each time (`this.pkg ?? DocxPackage.empty()`, 8552; `pkg_scratch` from
  `open_odt`; the watermark and the cover page make the package the document's, `own_package`, TS 11877/12041; the note
  commands' `if (this.pkg)` guards, 19048/19228). The 26 `self.pkg` uses in `session/ops` audited: the others read an
  empty scratch package as the TS reads `null` (nothing found).
- Word measured (16.0 pt-BR, invisible COM under `tools/parallel/word-lock.mjs`, `Quit()` with no argument, no
  `/Automation` WINWORD left after the runs; logs in `s-plan/w-out` and `s7c/w-out`):

| edit → SaveAs2 → Undo → SaveAs2 | Word, first save | Word, after the undo |
|---|---|---|
| `Tables.Add` (Tabela com grade), `.odt`-derived | Tabelanormal + Tabelacomgrade (14 styles) | Tabelacomgrade gone (13) |
| the same, `.docx` | 5 styles | 4 |
| `Tables(1).Delete` instead of undo | — | Tabelacomgrade kept |
| `Styles.Add('Meu')` | 6 styles | 4 (Meu gone) |
| `ApplyNumberDefault` | 1 `w:num`, 1 `w:abstractNum` | 0 / 0, `w:numPr` gone |
| `OddAndEvenPagesHeaderFooter` | `<w:evenAndOddHeaders/>` | gone |
| `AutoHyphenation` | `<w:autoHyphenation/>` | gone |
| `Footnotes.NumberStyle = lowerRoman` | `w:footnotePr/w:numFmt lowerRoman` (settings and sectPr) | gone |
| `Footnotes.Add` | 2 footnotes | 1 (the note styles stay) |
| `DefaultTabStop = 72` | 1440 | `Undo()` → False: no undo step through COM (not measurable this way) |

  The frozen TS keeps all of them (its pending map is outside the history; its package is reused between saves and a part
  is written only when dirty against the load), and so did Rust. Rust now follows Word: the pending map belongs to the
  undo step (`EditState::pending_tables`, counted in the step cost; `swap_pending_tables` takes an id that leaves the map
  out of the sheet the tables resolve against and registers one that comes back, so a later `insertTable` writes its
  definition again), and `save_parts.rs` takes back first what an earlier save wrote outside the writer: the
  `addStylesXml` additions, a styles part that differs from the current sheet (keeping the note styles a save added, as
  Word keeps them), the numbering part (the loaded text back, or the part, its override and its relationship removed),
  and `settings.xml` (the loaded text, re-derived at each save from the current values; the note-part separators and
  `w:displayBackgroundShape`, which Word writes from a view option, made again). The TS behaviour stays behind
  `OpenOptions::ts_table_style_survives_undo` and `OpenOptions::ts_stale_parts` (`Session::set_ts_save_compat`; wasm
  self-test `uo_session_test_ts_save_compat`): the strict replays set them where a TS capture saves after an undo or a
  change back (`parity_d2b_f5_session.rs` and the f5/f10 wasm replays; fix round 1: `ts_stale_parts` only, and the
  steps it covers listed in "Wave 7c — S, fix round 1").
- Chromium (`editor_switch_format.mjs` `extra`, new "parts" block; TS vs Rust, every part byte-compared):

| case | before the edit | after the edit | after the undo | after redo + 2nd table |
|---|---|---|---|---|
| B `hyperlinks` as `.odt`, `insertTable` | equal (3 parts) | equal (5) | declared Word: TS keeps `Tabelanormal,Tabelacomgrade`, Rust = before (3 parts), body equal | equal (5) |
| D `hyperlinks.docx` (no styles part), `insertTable` | equal (4) | equal (5) | declared Word, as B (Rust 4 parts) | — |
| E `styles.docx`, `createStyle('Meu')` | equal (9) | equal (9) | declared Word: TS keeps `Meu`, Rust = before | — |
| E `toc-chap-roman.docx` (no numbering part), `toggleList` | equal (19) | equal (20) | declared Word: TS keeps `numbering.xml`, Rust = before | — |
| E' `styles.docx` (a numbering part toggleList reuses) | equal (9) | equal (9) | equal | — |
| F `hyperlinks` as `.odt`, `toggleList` | equal (3) | equal (5) | equal (3; both a fresh package) | — |

- Native: `tests/session_w7c_save_parts.rs` (6 tests: the `.odt` table style written and undone with the table, and kept
  with `ts_table_style_survives_undo`; redo + a 2nd table both defined; a `.docx` without a styles part; the `.odt` saved
  from an empty package; save → undo → save of the styles part; `createStyle`, list, odd/even pages and the footnote
  format undone after a save, Word and TS; M1 no leak).

**Item 3 — the element across a DOM move; ED-25 retired.** The TS `disconnectedCallback` frees nothing of the document
(universal-write.ts 1730–1757). `rust-engine.ts` now matches it: `park()` only leaves the connected count, runs the
fronts' `park` hooks (the spell checker's reference, as the TS) and stops the font loop; the session, shell engine, layout
and every front's state stay, and every API works while detached (queries, edits, undo, save). `unpark()` re-enters the
count and resumes a font loop the detach cut; it does NOT run `adopt()`, so the fronts' `adopt` hooks (new-document
resets) no longer clear the Format Painter, the search results and highlight, the crop mode, the table/drag/wrap state
across a move (F1's minor). `adopt` lost its `reattach` parameter; F1's `.odt` kept-session special case and the
`.docx` reopened from bytes are gone. An element removed for good frees its document when it is collected:
`parkedGone` (a FinalizationRegistry holding the editor's own live counters, so an explicit free while detached is never
counted twice), the session binding's registries, and `viewer.ts` `layoutGone` for a layout collected without `free()`.
Harness memory contract: `editor_switch_lib.mjs` `env.gc()` (a CDP session per page, 3 × `HeapProfiler.collectGarbage`
with a 30-ms task after each, up to 10 rounds until no session and no shell engine); every memory loop reads the counters
after `remove()` + `gc()` (`editor_switch.mjs`, `_fidelity`, `_format`, and — cross-ownership, needed by the contract —
`_completion` (both checks of the cycle), `_objects`, `_review`, `_stories`; `_format`'s `remove()` also drops the
harness's own reference to the element's command registry, which otherwise kept the element alive). Measured:
- probe C (`note-plain`: bookmark, search `a` (8 hits), `copyFormat(true)`, remove, queries and save while detached,
  re-append, `goToSearchResult(0)`, undo): `.docx` and `.odt` — every field equal to the TS (detached: bookmarks 1,
  dirty, selection `{0,1}–{0,3}`, painter true, `save('auto')` works; after the move: the same, goto true; undo → 0
  bookmarks). Only `engineStats()` differs (the TS has no engine: null).
- `editor_switch_fidelity --mode park` (now `.docx` and `.odt`, plus a crop variant on `pictures.docx`): equal to the TS
  before the moves, after each of 3 moves and after 2 undos; while detached bookmarks, dirty, selection, painter,
  `save('auto')` parts (8 / 4), an edit (`insertBookmark('m2')`) and its undo equal; `goToSearchResult(0)` equal;
  `croppingImage()` true before, while detached and after in both; while detached sessions 1, shellEngines 1, handles 1
  (= attached).
- `editor_switch_fidelity --mode drop` (F1's probe promoted, part of `all`): 20 cycles each of a `.docx` and an `.odt`
  element created, loaded, edited, removed and never re-attached, held by WeakRefs: after `gc()` sessions, engines and
  handles 0 and session, editor and element collected in every cycle; live bytes flat from cycle 5 (4,374,500 `.docx`,
  4,375,876 `.odt`).
- ED-25 (uo-bindings "Declarations") marked retired; the README/CHANGELOG sentence is the coordinator's (below).

**Item 4 — `ROUTED_PUBLIC` duplicates.** `engine-routes.ts`: `currentStyles` and `copy`, `cut`, `paste`, `pastePlain`
dropped from the base list (front R routes them, `engine-routes-review.ts` 29/35; front handlers win in `installRoutes`,
so no behaviour changed). `routes-dups.mjs`: 489 entries, 489 distinct, no duplicate in `ROUTED_PUBLIC`,
`ROUTED_PRIVATE` (27) or `SHELL` (27); over `PUBLIC_METHODS` (511): routed-only 485, shell-only 24, both 2, neither 0.
`engine-switch.test.ts` (declared: outside the brief's list, the routing test) asserts no duplicates and prints the
distinct counts (merge-state line updated).

**Item 5 — the compact bar's clipboard check.** `editor_switch.mjs --mode chrome` read `[data-a=cut|copy|paste]`, which
the compact bar does not have (Edit-menu items only), and a missing button passed; it also read after Ctrl+K had opened a
dialog. Now, before any dialog: the ribbon's buttons, and on the compact bar the Edit menu opened (`[data-menu="edit"]`)
and its `.uo-menu-item[data-a=…]` items; a missing control fails; with a selection cut/copy/paste enabled, with a
collapsed selection cut and copy disabled (the negative control, `isEnabled = selectedText().length > 0`), paste enabled;
the TS engine runs the same check (positive control) and the two must be equal. Result: compact and ribbon, rust = ts =
`selected` all enabled, `collapsed` cut/copy disabled, paste enabled.

**Item 6 — records.** F1's Results block: the round-2 numbers, the Rust 1.100 allowances sentence removed, the pointer
to this subsection; F2 round 2: the session label marked reverted and the d2b-F10 acceptances dropped; merge state:
ROUTED_PUBLIC, A2's declaration 7 (closed), ED-25 (retired).

**Results** (this tree; `s7c/verify.log` and the logs named there; wasm built from the tree with `-j 3` into
`target/m2-w7b`; the four modules `session_count.wasm`, `selftest.wasm`, `session.wasm`, `viewer.wasm` in `s7c/`):
- uo-docx `--release --all-features --test all` (3 threads): **813 passed, 0 failed, 1 ignored** (F1: 802; + the 6
  save-parts tests and the other frontiers' new tests).
- uo-docx-layout `toc_pages_w7`: **5 passed**.
- uo-wasm natives: session_native (session,count,session-selftest, 1 thread) 11/11; shell_native 9/9,
  shell_stories_native 8/8, shell_objects_native 5/5, shell_completion_native 5/5, shell_format_native 4/4.
- wasm replays (`selftest.wasm`, d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10): **0 mismatches in every family**, f10 53 accepted
  (was 55), every TS re-encode byte-identical; `session_api.test.mjs` 5/5.
- editor/viewer tsc: exit 0, no output. Editor vitest (engine-switch config): **34 files / 334 tests**; the routing test
  prints "public methods 511: routed 487 (+ 2 indented), shell 26, of which both 2, disabled mutators 0, disabled
  queries 0; private routed 27; bar commands routed 516".
- clippy `-D warnings`, uo-docx and uo-wasm each `--all-targets --all-features` native and `--all-features` wasm32: 0
  warnings. `node
  tools/check-changed.mjs --jobs 3` over the whole tree: **2 steps failed, none in S's files** — the clippy steps stop
  compiling uo-docx-layout on the other frontiers' in-progress code (`engine/chain.rs:60` disallowed `f64::min`, an
  unfulfilled lint expectation in `lib.rs:36`, `indexing_slicing` in `engine/flow.rs:824` and `view_transform.rs`
  303/306/309), so uo-wasm was linted alone (the line above). float-casts and grep-bans passed. rustfmt `--edition 2024
  --check` on every touched `.rs` (and the session modules they declare): clean.
- Chromium (`session_count.wasm` of this tree; `s7c/h-*.log`, `h2-*.log`):
  - `editor_switch.mjs --mode all` **ok**: parity 13 documents × 12 steps, every step facet equal or declared (0
    undeclared), saved parts 132 equal + 1 declared; 100 memory cycles, live bytes flat at 16,986,957, sessions/shell
    engines/handles 0 after each remove + `gc()`; chrome: the clipboard check above, compact and ribbon, rust = ts.
    `--mode parity` with `session.wasm`, stride 1, 12 steps: **ok** (thumbnails 1,088 + 64 declared, screens 148 + 25,
    saved parts 658 + 4; every declared one gate-O3 or W's Word-authority items).
  - `editor_switch_fidelity.mjs --mode all` **ok**: 18 box scenarios, 0 undeclared; park `.docx` and `.odt` and crop
    equal to the TS (above); drop flat at 4,374,500 / 4,375,876; memory 20 cycles flat at 4,374,500.
  - `editor_switch_format.mjs`: `--mode memory` **ok** (30 cycles flat at 8,563,009, sessions 0 after remove + GC — the
    first run failed with sessions 1: the harness's `api.reg` / `window.__lastReg` held the element's command registry;
    `remove()` drops them now); `--mode extra`: the `updateFields` and "parts" blocks as above, all equal or declared;
    **1 failure not S's**: `exportXps side effects, print-cell` — the screen after inserting a table in `hyperlinks.docx`
    (a package without `settings.xml`): the Rust table sits ~6 px left (Word's mode-12 rule, W's item 2,
    `ts_missing_settings_mode15`; the TS hash `fcd571a5`, Rust `13814fe5`, also in V's 08:00 run); `--mode all`: font
    947 + 2 declared, para 1,078, bar 585, file ok; **not S's**: `tables|ruler` 4 (the same no-settings table),
    `fields|text` 1 and `fields|view` 24 (fields.docx: after `insertTableOfContents` Rust shows W's REF error text "Erro!
    Fonte de referência não encontrada." / "Erro! Indicador não definido." where the TS shows the cached text —
    `s7c/probe-fields.mjs`; W's item 4).
  - `editor_switch_completion.mjs`: `--mode memory` **ok** (30 cycles flat at 13,829,745, `memory.buffer` 25,231,360;
    sessions 0 after both removes + GC — the first run kept one element: the `<universal-sheet>` stub class was defined
    inside `make()` and its context held the first `el` for good (found with a CDP heap snapshot, `s7c/heap-retainers.mjs`);
    it is defined outside `make()` now, and `remove()` drops the registry references); families paste 1,069, tables
    1,004, inline 255, page 134, previews 200 equal; **not S's**: `f7c__7__2|lists` 1 thumbnail (REF error, W item 4),
    `f7d__7__10|objects` 2 (linked boxes, W item 3).
  - `editor_switch_review.mjs`: `--mode memory` — **corrected in "Wave 7c — S, fix round 1"**: the 8,621,016 of this
    run was the second plateau of a timing-dependent growth (the Table Styles thumbnails' first use of a pushed face, in
    the page environment), not reproducibly flat; `--mode all` revisions 837,
    comments 187, styles 405 + 36 declared, find 502, clipboard 155, keys 243, spell equal; **not S's**: `fields|fields`
    5 (the same fields.docx REF error text, W item 4).
  - `editor_switch_objects.mjs --mode all`: memory **ok** (30 cycles flat at 13,400,333); pictures 855, keys 171, views
    362 equal; **not S's**: `f7a__7__0|page` 31 and `f7c__7__8|page` 4 (page count 14 vs 16 / a landscape page one
    later: W's keepLines item 6), `f7c__7__2|page` 30 (REF error), `f7d__7__0|objects` 2, `f7d__7__3|objects` 30,
    `f7d__7__10|objects` 26, `f7d__7__3|menu` 2 (linked boxes, W item 3).
  - `editor_switch_stories.mjs --mode all`: memory **ok** (30 cycles flat at 6,682,346); **not S's**: 30 undeclared on
    `f7d__7__8` (T2), `f7d__7__0` (X1) — linked boxes, W item 3 — and `f7c__7__13` (N2) — REF error, W item 4.
  - `viewer_switch.mjs --mode all` (`viewer.wasm`, stride 1): **ok**, 78 documents, 592 pages, 0 undeclared; ED-713
    used on `toc-chap-chapter` and `toc-chap-chapter-roman` (textLayer + pixels on page 0, pdf).
  - probe `s-plan/probe-s.mjs A B C D E F`: A as the table of item 1; B/D/E — Rust equal to the TS before the undo, the
    style/table style/numbering gone after it (Word); F equal; C equal in every field but `engineStats` (TS null).
- Word: no `/Automation` WINWORD process left (`Get-CimInstance Win32_Process`: only the user's two instances).

**Files** (S's; the other frontiers' edits in the same files untouched): uo-docx `session/io.rs`, `session/mod.rs`,
`session/state.rs`, `session/versions.rs`, `session/save_parts.rs` (new), `session/ops/tables.rs`, `ops/notes.rs`,
`ops/page/cover.rs`, `ops/page/watermark.rs`, `ops/references/toc_cmds.rs`, `ops/references/fields.rs` (the refresh's
index/section rebuild), `ops/styles/xform.rs` (`styles_part_text`), `word/outline.rs` (the revert of F2's addition);
tests `session_w7c_save_parts.rs` (new), `all.rs` (one `mod` line), `parity_d2b_f10_session.rs`,
`parity_d2b_f5_session.rs` (`ts_stale_parts` for its TS captures); uo-docx-layout `tests/toc_pages_w7.rs`; uo-wasm
`src/session/selftest.rs` (`uo_session_test_ts_save_compat`), `js/viewer.ts` (`layoutGone`), `tests/session_replay.test.mjs`;
editor `rust-engine.ts`, `engine-routes.ts`, `engine-switch.test.ts` (declared above); harnesses `editor_switch.mjs`,
`editor_switch_lib.mjs` (`gc()`), `editor_switch_fidelity.mjs` (park `.docx`/`.odt`/crop, `drop` mode, memory),
`editor_switch_format.mjs` (`updateFields`, parts, extra docs, memory, `remove()`), `viewer_switch.mjs` (`pages` scoping),
and — cross-ownership, the memory contract — `editor_switch_completion.mjs` (both memory checks after `gc()`, `remove()`,
the sheet stub outside `make()`), `editor_switch_objects.mjs`, `editor_switch_review.mjs`, `editor_switch_stories.mjs`
(their memory loops after `gc()`). Records: this subsection; F1 Results; F2 round 2; ED-25; the merge-state lines.

**For the coordinator** (README/CHANGELOG are rewritten at the merge): drop "reconnecting the element reopens the document
without the undo history;" from `write/packages/editor/README.md` (line 195–196) and "reconnecting reopens the document
without undo history;" from `CHANGELOG.md` (line 145) — a moved element now keeps its document, history, selection and
state, as `ts`. (CHANGELOG line 99, "reconnecting reopens the document (the scroll position resets)", is the viewer's and
is not changed by S.) The harness failures listed as "not S's" are W's Word-authority items (no settings → mode 12, REF
error text, linked boxes, keepLines), which W must declare in those harnesses.

**Not done / open (none is a decision):**
1. ~~The `DefaultTabStop` edit records no undo step through COM~~ — closed in fix round 1: measured through the Tabs
   dialog's command (`WordBasic.FormatTabs DefTabs`, which records an undo step): the undone default tab is gone at the
   next save, the rule Rust applies.
2. `check-changed.mjs` as a whole fails on the other frontiers' uo-docx-layout code (above); S's crates were linted one
   by one instead: uo-docx and uo-wasm `--all-features -D warnings`, native `--all-targets` and wasm32, 0 warnings.

### Wave 7c — V, fix round 1 (the reviewer's findings on V)

One implementer; scratch, logs and Word scripts in `core/target/m2-latency/v2/` (outside git), Word screenshots in
`E:\UniversalWriteTest\_lab\views\shading` and `views\v2`. The session_count module of this round: `v2/session_count.wasm`
(10,493,139 B, the tree's Rust; V changed no Rust source in this round). Nothing committed. Where this subsection and
"Wave 7c — V" differ, this one is valid.

**L5 — the live drags frame by frame, on by default.** `UO_EC_FRAMES`/`UO_EO_FRAMES` now default ON (`=0` runs plain
six-step drags). The four frame findings, each traced to its cause:
- *completion `blank|inline`, all 7 frames*: the inserted picture did not paint at all during the drag (diag: the frame's
  layout held the decoded bitmap, the canvas was white there). Cause: the page base painted while the picture was still
  decoding was reused — the decode's repaint skipped a layout already replaced, and the successor's band check (D-L4) or
  key hit kept the picture-less canvas. Fix (`viewer/src/rust-engine.ts` `pendingImages`, `editor/src/rust-engine.ts`
  `incomplete` on the page and strip bases): a base painted while a picture decoded is painted again whole; a decode that
  ends after its layout was replaced repaints. Now 7/7 frames equal.
- *objects f7b__7__0 nw resize frames 0–1 and f8__7__11 rot frames 0–1*: both mousedowns land on a table edge in BOTH
  engines (the guide line of `paintTableAdorners`); the pixels differed by 1/255 on the guide's two columns only. Measured:
  the same `tableDrag` (x 3694/3690), the same context state (transform 0.06666667 float32, width 15, dash [45,45], alpha 1,
  source-over, no clip, the same context attributes), and the same stroke on a fresh canvas gives the TS's pixels; the
  coverage is a rounding tie (59.5/255 at x 246.27 px, 127.5/255 at 246.0) that the visible canvas rounds differently
  when its buffer outlived several paints (the shell keeps the buffers across relayouts, D-L3; the TS allocates them again
  at every refresh). In-hook bisection: painting the base directly instead of `drawImage(base)` gave the TS's value;
  `--disable-gpu`, `--disable-accelerated-2d-canvas` and `willReadFrequently` changed nothing. The harnesses' `dragFrame`
  now allocates every page buffer again before the frame's paint, in both engines (pixels independent of the buffer's
  history). Equal.
- *objects f7b__7__0 float move frame 0 (one more page canvas) and f7b__7__1 (canvas 8)*: the click before the drag
  selected the shape through `apply`, whose caret scroll jumped the view from 5612 px to 244 px (pages 0–1 allocated
  there and kept). The TS selects with `floatSel = run; refreshSelection()` (no scroll). Fix (`rust-objects-pointer.ts`
  `selectCmd`): the click's object selection (and deselection) applies without the caret scroll. Equal.
- *objects f7b__7__0 float move frame 0, proxy top 284 vs 258 px* — **open (F-1)**. After the click the Shape Format tab
  grows the top bar by 26 px (178 → 204); the TS proxy follows because a resolver change (Liberation Serif / Times New
  Roman) reaches `onFontsResolved` → `queueRefresh` (80 ms) after the bar grew; in the shell that change arrives 18 ms
  after the click, before the tab grows (the shell's bar grows ~130 ms after the click, the TS's ~60 ms), and the routed
  `onFontsResolved` runs the font pass at once. Tried and reverted (each made other documents differ): a ResizeObserver
  on the bar (f7b__7__1: the TS stays at 7852, Rust 7878); a TS-timed refresh tail after every font pass
  (`onSelectionChange` mid-drag on f7b__7__1); the TS `queueRefresh`/`askedFonts` semantics on the routed methods (the
  shell gets the emit on f7b__7__1/3 too: its pushed names differ from the TS `askedFonts`, and its Times New Roman
  resolution comes after the click, the TS's at load). Not resolved.
  **Closed in the WordCraft study (page, 2026-11-14), by the behaviour, not by copying the TS's timing:** the Rust editor
  keeps the IME proxy at the caret whenever its chrome or scroll container changes size (`rust-engine.ts` `listen`: a
  `ResizeObserver` on the scroll container, the shadow root's other children and the element; `positionProxy` runs again
  when the caret's page canvas moved since the proxy was placed, `proxyMoved`). f7b__7__0 frame 0 is now equal (284 px in
  both). On f7b__7__1, f7b__7__3 and f8__7__11 the frozen TS has no late font event after the bar grew and leaves its
  proxy 26 px above the caret (7852/351/2900 px against the caret's 7878/377/2926; both engines equal again at @4, after
  the TS's next refresh): declared in `editor_switch_objects.mjs` as "WordCraft page F-1 (TS proxy stale after the bar
  grows)" — a TS defect, not a rule to copy. The memory soak stays flat at 13,400,333 live bytes (30 cycles).

Results with the frames on (no declaration for V): `editor_switch_completion --mode all` **ok** (497 s; lists 1,462 + 1,
paste 1,069, tables 1,004, objects 2,557 + 44 declared W item 3, inline 395, page 134, previews 200; memory 30 cycles
flat at 13,829,745 live bytes, handles 0); `editor_switch_objects --mode all` **1 failure = F-1** (561 s; page 3,739 + 65
declared W, objects 5,558 + 72 declared W items 3 + F-1, pictures 855, menu 217 + 2 W, keys 171, views 362; memory 30
cycles flat at 13,400,333, handles and sessions 0).

**ED-304 — Word measured and applied** (`v2/word-shading.ps1`, Word 16, invisible COM through `word-lock.mjs`, the window
off screen at −20000 without activation, `PrintWindow`, 96 dpi, zoom 100; foreground unchanged). A document of 11-pt
Liberation Sans paragraphs with QUOTE fields (1.5 lines, single, a 20-pt run on the field's line) and a comment, saved as
`uo-docx-layout/tests/fidelity/field-shading.docx` (the author scrubbed to "Ana"). Measured:

| case | Word |
|---|---|
| shading colour | C0C0C0, opaque, under the glyphs (black text on it; white text on a #202020 page colour) |
| default "When selected" (2) | no shading with the caret outside; only the field the caret is in |
| band, 11-pt line, 1.5 lines / single | rows 142–158 / 178–194: 17 px from the line top (not the 36/28-px line box) |
| band, 11-pt field on a line with a 20-pt run | 31 px from the line top (the line's natural height) |
| band x | the field result's first to last character (333–388; GetPoint 333…389) |
| comment | band E8E8E8, 17 px, 1-px brackets 424242 at the first and last character's edges; on #202020: 787878, white brackets |

Applied (Word manda sobre o TS): `rust-review-overlays.ts` `paintWordMarks` in the page base's `under` hook (under the
glyphs), its base key `wordMarksKey` = the field run the selection is in (the base repaints only when that changes); the
TS look (`rgba(0,0,0,.10)` over the line, every field; `rgba(255,187,0,.28)` comments, over the text) under
`ed.tsShading` / `ed.tsStrict`, set by every harness's strict replay (`editor_switch{,_completion,_fidelity,_format,
_objects,_review,_stories}.mjs`, after `uw-ready`). Check: `editor_switch_review --mode wordmarks` (also in `--mode all`):
Rust's bands on field-shading.docx 17/17/31 px and the comment band 17 px, none with the caret outside — as Word. Not
done: "When selected" in headers/footers, notes and text boxes (keyed for body and cells only; those get no shading); the
descent of a multiple-spaced line is estimated as 0.234 × the ascent (the lines expose no descent; exact for Liberation
Sans, ~0.6 px short for Carlito at 11 pt); Word's black Office theme (an account setting) not measured — the dark values
are a dark page colour's.

**V1b — Word measured (continuous views)** (`v2/word-views-v2.ps1`, the same recipe; `v2/mkdoc2.py` →
`v2/v1-sections.docx`: section 1 A4 portrait, text 8504 tw; section 2 landscape, text 14570 tw; a long paragraph in each):
Web wraps at the window at zoom 100 and 200 (3 lines at 100, 6 at 200 for the same text: the twips width halves); Draft
wraps each section at its own text width at both zooms (5 and 3 lines; section 2's lines run past the 814-px pane to
x 713 / 1370) — both as the engine's rules. Outline: section 1 6 lines and section 2 5 lines at zoom 100, 5 and 5 at zoom
200, text at +48 px (+95 at 200) — a rule not derived, not applied. The 800-px window collapsed to a 39-px-high Word
window (usable height 1): those numbers are not valid. Not measured: the break line's drawing, the Outline symbols'
positions, floats and notes in the views.

**V5 — interaction steps** (`editor_switch_format.mjs`, view family, `v5: true`, on by default, `UO_EF_V5=0` leaves them
out): in Web, Draft and Outline a real click at a caret point, a Shift+click at the last paragraph, PageDown, Ctrl+End and
an element resize to 800 px and back. Results (`--mode view`, 93 s): 34 undeclared differences — the resize (the shell's
Web sheet keeps its width: ruler frame 14580 vs the TS 10080 twips at 800 px; back at 1100 px the TS sheet 752 px, the
shell's 1052), the Shift+click (TS focus offset 143, Rust 0, at the paragraph's own caret point), PageDown (another
offset); the plain click, Ctrl+End and the typing steps equal. Open. With `UO_EF_V5=0`: 1 difference, the `fields|view`
saved REF text (W's item 4), as before.

**Other verification**: `editor_switch_review --mode all` **ok** (131 s; wordmarks included); editor/viewer tsc exit 0;
vitest editor 34 files / 334 tests, viewer 7 / 28 (+1 skipped); natives `shell_format/completion/objects/review/native/
stories` 4 + 5 + 5 + 4 + 9 + 8 = 35 passed; `tools/fonts/check-fonts.mjs` on field-shading.docx: no proprietary font. Not
rerun this round (no Rust or layout file changed by V): layout parity, the Word oracle, build-pkg/pkg_smoke,
viewer_switch, `editor_switch --mode all`/`parity`, fidelity, stories.

**Files (fix round 1)**: `write/packages/viewer/src/rust-engine.ts` (`pendingImages`, the late-decode repaint),
`write/packages/editor/src/rust-engine.ts` (the bases' `incomplete`), `rust-objects-pointer.ts` (`selectCmd`),
`rust-review-overlays.ts` (`tsShading`, `paintWordMarks`, `wordMarksKey`), `rust-engine-review.ts` (the `under`/`baseKey`
hooks); harnesses `editor_switch_{completion,objects}.mjs` (frames default on, buffer re-allocation, frame dumps,
`UO_EO_FDIAG`, the probe's `UO_EO_PROBE_STEPS`), `editor_switch_review.mjs` (`wordmarks`), `editor_switch_format.mjs` (V5
steps, Shift+click), `editor_switch_lib.mjs` (`UO_ES_CHROMIUM_ARGS`, empty by default), `editor_switch{,_fidelity,
_stories}.mjs` (the `tsShading` line); fixture `uo-docx-layout/tests/fidelity/field-shading.docx`. Noticed, not V's: three
F2/W fixtures (`toc-chap-*.docx`) carry a real author name in `docProps/core.xml`.

**Not done / open (none is a decision):** F-1 above; ED-707/708 (a) and (b) in the merge state; ED-304's other stories and
estimated descent; the memory soak has no wrap-point drag; typing latency and a 500-page document in the continuous views
not measured; a follower box's spelling still resolves the chain head by the lowest `seq` (not the TS `textboxChain`); the
frozen TS's REF text inside a Paste-as-Picture preview not captured.

### Wave 7c — S, fix round 1 (the reviewer's findings on S)

One major (the review memory soak not reproducibly flat) and three minors. Scratch, probes, Word script and logs:
`core/target/m2-w7b/s7d/` (`verify-s2.sh`, `verify.log`, `v-*.log`, `trace*.log`, `warm*.log`, `trace-font-*.log`,
`bisect-thumb.mjs`, `word-undo-nosave.ps1`, `w-out/`, `h-*.log`, `h2-review-mem80-*.log`), outside git. Nothing committed.

**Major — the review soak's growth, attributed and made deterministic.** Not an element's memory: sessions, shell
engines and handles were 0 after every GC, and every step lives in the page environment (`pageEnvs`, the shared font
registry), independent of the GC path. Attribution:
- `editor_switch_review.mjs` `UO_ER_TRACE=1` (new, diagnostic): every wasm export wrapped (outermost calls), each
  cycle's net per export, and the producer/free pairs (a returned handle or buffer and its free) whose nets do not
  cancel, with the preview's argument and the font traffic (`push_family` names, `take_missing` notes). In every
  growth cycle the only extra pair was a Table Styles gallery thumbnail (`uo_shell_preview` → `uo_layout_free`): Plain
  Table 1 left 1,792 (+ its 24-byte argument = the reviewer's +1,816), Table Classic 1 1,358 (+26 = +1,384), Table
  Classic 3 1,806, Grid Table 1 Light Accent 4 2,714 (cycle 0), never twice for a style in a page.
- `bisect-thumb.mjs` on a fresh page: Table Classic 1's first thumbnail notes the italic face missing (86 B); after the
  host pushes it (a relayout's `take_missing`), the next one keeps 1,358 and a third one 0; Table Classic 3 the same with
  bold italic (1,806). So: a pushed face is parsed on its first use (uo-fonts `Pushed::face`, a `OnceLock` into the
  shared `FaceCache`), bounded by the pushed faces (4 variants a family).
- The gallery lays its thumbnails out 8 per frame until the element is removed, so which faces a cycle first uses is
  timing. One late case: cycle 0 pushes "Calibri" twice (the second push, with the faces the thumbnails noted,
  replaces the family), and `trace-font-*.log` shows no push after cycle 0 but Plain Table 1's +1,336 at cycle 2 in one
  run and at cycle 18 in another: the replaced Bold parsed again by a later element's first bold thumbnail (that the
  removed element's layouts kept the first answer is the likely reason; not measured further).
- Fix (harness): the warm-up cycles 0 and 1 lay every thumbnail of `tableStyleListNow()` out, `refresh()` (the push),
  and lay them out again (`UO_ER_WARM_THUMBS=0` runs the old soak). Results with this tree's `session_count.wasm`,
  `UO_ER_SOAK=80`: 4 runs, **all flat at 8,621,016** (`h2-review-mem80-1..4.log`). With the warm-up in cycle 0 only,
  1 of 3 runs was not flat (+1,360 at cycle ~77); the reviewer saw 3 of 4. The S record's "flat at 8,621,016" is
  corrected there.

**Minor — the note styles after an undo without a save between (Word measured).** `word-undo-nosave.ps1` (invisible
COM under word-lock, `Quit()`, no `/Automation` WINWORD left: the two Word processes are the user's, started 02:47):
note-plain, `Footnotes.Add` → `Undo` (True, 1 footnote) → `SaveAs2` with no save before: Textodenotaderodap,
TextodenotaderodapChar, Refdenotaderodap in the file (as with the save between). Rust now keeps them from the command,
not from the package: `SaveParts::note_styles` (the ids `ensure_note_styles` added, `save_parts::added_note_styles`,
outside the history), put into the saved sheet when it lacks them (`put_missing_styles`; in the dirty branch, the
not-dirty branch, and a package without a sheet through `add_styles_xml`); `ts_stale_parts` keeps the TS (gone without
a save between). Native: `session_w7c_save_parts.rs` `note_styles_after_undo_and_note_options_set_back` (Word and TS,
with and without the save between).

**Minor — the flags of the strict replays.** f10 (native and wasm) now sets `ts_stale_parts` only (wasm flag 1, not 3);
`session_replay.test.mjs` takes `UO_WS_TS_SAVE_FLAGS=0` (no flags) and `UO_WS_FULL=1` (every mismatch, with the items
only one side has). With the flag: f5 0 mismatches, f10 **0 mismatches, 53 accepted**. Without the flag (this tree,
after the note-styles fix), the steps it covers:
- f5, 41 items of the `parts` facet. 10 `word/settings.xml` — a note option **set back** (a change, not an undo): Word
  saves the current options (measured: `noteopts-setback`, `w:numFmt lowerRoman` in the first copy, gone in the
  second; native assertion in the test above), the TS keeps the first save's: cn-span-mid #40/#48/#56/#60, cb-long-1
  #48, cb-emptysect-1col #40/#48/#56, cw-vc-none-c14 #56/#60. 31 `word/styles.xml` — a note insertion undone (Word keeps
  the note styles; the TS drops them with no save between), new with the fix above: bf-front #24/#32/#40/#48/#56/#60,
  b2-20 #48/#56, b3-10 #8/#16/#24, b3-20 #32/#40, b3-60 #8, cb-3col-long #48, cb-empty-2 #8, cb-long-2 #16,
  cb-emptysect-aftertbl-c11 #8/#16/#24/#32/#40/#48/#56, cb-emptysect-big #16, cb-end-of-num #24, bt-sz4 #32,
  cft-text-page-x5000-300-empty #24, cs2f-0-3col #16/#24, txt-times-new-roman-c15 #48 (attributed by the facet — only
  `styles.xml` differs — and by the f5 script's `nf` insert and `un` undo; not inspected one by one).
- f10, 3 items: `stylexml` #16 on sep-times14, ln-objects, styleref-seq — an undone style edit saved (Word: the sheet
  of now; the TS: the earlier save's).

**Minor — `DefaultTabStop` undone after a save (the S record's open item 1, closed).** Through the Tabs dialog's
command (`WordBasic.FormatTabs DefTabs:="72 pt"`, which records an undo step; `Document.DefaultTabStop` does not):
`deftab 72`, `Undo()` True → 35.4; the first copy `w:defaultTabStop 1440`, the second 708 (the loaded value), and 708
also with no save between. Rust's rule (`settings_back`, the loaded text back) is Word's. `save_parts.rs` module comment
updated.

**Results** (this tree; `s7d/verify.log`; wasm built with `-j 3` into `target/m2-w7b`, copies in `s7d/`):
- uo-docx `--release --all-features --test all`: **814 passed, 0 failed, 1 ignored** (the 7 save-parts tests among
  them; the f5/f10 native replays with `ts_stale_parts` only); uo-docx-layout `toc_pages_w7` 5 passed.
- uo-wasm natives: session_native 11/11, shell_native 9/9, shell_stories_native 8/8, shell_objects_native 5/5,
  shell_completion_native 5/5, shell_format_native 4/4. wasm replays (d2a,f1–f3,f5–f10): 0 mismatches in every family,
  f10 53 accepted, every TS re-encode byte-identical; `session_api.test.mjs` 5/5.
- editor/viewer tsc exit 0; editor vitest 34 files / 334 tests. clippy uo-docx `--all-targets --all-features -D
  warnings`: 0; rustfmt `--edition 2024 --check` on the touched files: clean.
- Chromium (`s7d/session_count.wasm`): review `--mode memory` 4 × 80 flat (above); `editor_switch.mjs --mode all` ok;
  `editor_switch_stories --mode all` ok; `editor_switch_format --mode extra`: the `updateFields` block (ED-712/713 as
  declared, the rest equal) and the "parts" block (B/D/E as declared, E'/F equal) as in the S record, 1 failure not
  S's (`exportXps side effects, print-cell`, W's no-settings table, as before); `editor_switch_review --mode all`: 3
  failures not S's (`wordmarks`: comment/field band heights against Word, V's fix-round item).

**Files** (this round): `editor_switch_review.mjs` (`UO_ER_TRACE`, the thumbnail warm-up), uo-docx
`session/save_parts.rs` (`put_missing_styles`, `added_note_styles`, `SaveParts::note_styles`, module comment),
`session/io.rs` (the styles branch of the save), `session/ops/notes.rs` (`ensure_note_styles` records), tests
`session_w7c_save_parts.rs` (one test), `parity_d2b_f10_session.rs` (flag), `parity_d2b_f5_session.rs` (comment),
uo-wasm `tests/session_replay.test.mjs` (flag 1, `UO_WS_TS_SAVE_FLAGS`, `UO_WS_FULL`); records: the S subsection (review
soak number, item 2's flag sentence, open item 1) and this one.

**Not done / open (none is a decision):** the in-memory style list after an undone note insertion still drops the note
styles (only the saved file keeps them, as measured in Word's file; whether Word's Styles pane lists them after the undo
is not measured).

### Wave 7c — W: Word-measured layout rules (B's "NOT applied" list and F2's keepLines item)

Frontier W of wave 7c (2026-11-13; one implementer cut off by a usage limit, resumed by a second one that kept its
work, finished items 1 and 5's measurements, the remaining harness declarations and the verification). Word 16 pt-BR is
the authority where it differs from the frozen TS ("Word manda sobre o TS"); every rule applied below was measured with
an oracle series in Word (invisible COM under `tools/parallel/word-lock.mjs`, `write/tools/layout-oracle/run.mjs <group>
--fixdir core/crates/uo-docx-layout/tests/oracle_w7c/<group>`, Liberation Sans, PDFs in `write/tools/layout-oracle/.out`,
not versioned) and is asserted by the new native test `uo-docx-layout/tests/word_w7c.rs` (a port of the TS
`oracle-compare.ts`: pages, line texts, the page of each line, baseline ±0.3 pt, x ±0.6 pt where the group measures it).
The frozen TS behaviour of each rule stays behind a `LayoutOptions::ts_*` switch; `LayoutOptions::ts_strict()` (new)
turns all eight on and is what every replay of TS captures uses. Scratch, scripts and logs: `core/target/m2-wsession/w7c-w`
(outside git). The groups live in `tests/oracle_w7c`, NOT in `write/packages/docx-layout/test/oracle`: that directory is
the frozen TS's 2,742-document corpus, which the uo-docx parity tests (`parity_full`, `parity_export`, `parity_odf`, …)
replay against TS captures — the first placement failed 46 of them ("not captured"), so the groups were moved.

#### Items

| Item | Word (measured) | Frozen TS | Status | Switch / wasm bit |
|---|---|---|---|---|
| 6 keepLines taller than what is left (F2: `f7a__7__0` last paragraph as Heading 1) | moves to the next page even when taller than a page, then splits (64 cases `keeptall`, modes 14/15: 1/30/54 filler lines × 56/70/120-line paragraphs, keepNext+keepLines, the exact page, keepNext only, widow control off); mode 15, 2 columns: a keepLines paragraph taller than the column goes to the next PAGE and each of its columns ends with a page break (top of a page too; mode 14 → next column); mode 15, in a table cell: a keepLines paragraph crossed by the row's split goes down whole (not at the top of a page; mode 14 ignores keepLines in cells) | moves only if `need <= full`; no column or cell rule | **applied**: 64/64 cases equal to Word (6,884 lines, max Δy 0.15 pt, max Δx 0.08 pt); `ts_strict()` differs in 37 of them; `toc_pages_w7` asserts F2's `xiii` (Word) and `xii` (`ts_strict`) | `ts_keep_lines_within_page` / bit 17 |
| 2 package without `word/settings.xml` | COM `CompatibilityMode` = 12 (and 12 for a part without `compatSetting`); `DefaultTabStop` 35.4 pt = 708 tw whenever `w:defaultTabStop` is absent (part absent, empty part, mode-15 part without it) | mode 15; 720 tw | **applied**: 25/25 cases `nosettings` (tables, nested table, default tab stops, justification, negative indents × no part / empty part / mode 15 without the tab / controls 12 and 15) equal (190 lines, max Δy 0.11 pt, max Δx 0.10 pt; `ts_strict` differs in 7); B's `cell-lic1` (no settings part): cell text 220.01 pt, box text 100.58 pt as Word (`ts_strict`: 225.65 / 106.22) | `ts_missing_settings_mode15` / bit 13 |
| 3 linked text boxes (B: two linked boxes in one cell) | a box of a chain is never left empty: box 1 keeps "primeira caixa" (re-measured baselines 100.82 / 137.78 pt; the planner's 92.10 / 129.06 were not baselines), in the body too; a box too low for one line still takes the first line; mode 15 wraps a cell's text around a `layoutInCell="0"` box | box 1 empty in mode 15 (the widow/orphan cut sends the whole paragraph on); no wrap | **applied**: 21/22 cases `txbxcell` equal (480 lines, max Δy 0.13 pt; `ts_strict` differs in 9); the 22nd (`lc-cell-lic0-c12`) is a mode-12 table rule outside the chain, listed under "Not done" | `ts_linked_box_cell_chain` / bit 14 |
| 4 REF/PAGEREF/NOTEREF to an undefined bookmark | missing, without `w:bookmarkEnd` or ending before it starts: REF with every switch (`\h \n \r \w \p`) "Erro! Fonte de referência não encontrada.", PAGEREF (also `\p`) and NOTEREF "Erro! Indicador não definido.", in BOLD in the PDF (STYLEREF's error too) and saved bold (`Fields.Update` + `SaveAs2`: `<w:b/><w:bCs/>`, PAGEREF also `<w:noProof/>`); a range over several paragraphs = all of them (each a new line); a bookmark in a cell, a text box or a header = its range's text | the start paragraph's text, page 1, the cached result | **applied**: 10/10 cases `refbm` equal (51 lines, max Δy 0.17 pt; read as the editor does, `sources`; `ts_strict` differs in 9); `ref_errors_are_bold`; the SAVED bold of the editor's lossless read stays open ("Not done") | `ts_ref_bookmarks` / bit 15 |
| 1 theme tint/shade rounding | **3,356** distinct (base, tint/shade byte) → Word colour samples, no two in conflict: 3,264 from a new COM series (`core/target/m2-wsession/w7c-w/tint`: 48 bases — greys, primaries, the 2007/2013/2023 accents, near-white/black, every hue sector — in the 12 theme slots, × 34 tint and 34 shade bytes 01…FE incl. the UI's 33/66/99/BF/A5/80/7F/E6/D9/F2/A6/CC; `w:color` a decoy; Word's PDF read with pypdfium2) + 92 hex values Word wrote in its own corpus documents; B's 15 PDF samples all coincide. Facts: greys are exact (`floor(L′·255)`, L′ = L·s or L·t+1−t); chromatic colours deviate from exact HSL by −1.75…+0.2 per channel, with no fixed (H, S) per base explaining a base's series (grid search) | HSL lightness, channels truncated: **2,323 / 3,356** | **OPEN**: no closed form found — exact HSL with a floor epsilon 2,557 (but it loses 16 samples the TS rule hits: Word has its own float error), the KB29240 integer HLS family ≤ 1,607, linear RGB 318, 255/256 scale variants ≤ 1,796, best quantisation (L′ to 1/1020, S to 1/1000, floored) **2,662**; none applied (a partial model trades samples). The samples are versioned (`tests/oracle_w7c/tint/word-samples.tsv`) and `word_w7c` `tint_shade_samples_against_the_ts_rule` pins 2,323 | bit 12 (reserved) |
| 5 hyphenation | measured anew (`hyphword`, `write/tools/layout-oracle/cases/hyphword.mjs`: 150 long words per language drawn with a fixed seed from the repository's open Hunspell dictionaries, each alone at 30/40/50/60/70/80 pt, Liberation Sans 11, Word's line splits against the Rust layout's): **pt-BR 877/900 paragraphs equal (135/150 words at every width), en-US 809/900 (105/150), es 898/900 (148/150)**; split points Word/Rust/common pt 425/431/416, en 382/384/337, es 438/438/437 (residuals listed in `w7c-w/hyph-agree.txt`, e.g. en geo-graph-ical × geo-graph-i-cal, pt oxirri-n-quídeo × oxir-rin-quídeo) | the same patterns (hyph-utf8) | **OPEN** for the maintainer: Word's hyphenator is proprietary; closing it needs either pattern sets from licence-checked sources (patgen over the public-domain Moby Hyphenator II for en; pt/es candidates — each a DOWNLOAD this frontier may not make without the maintainer, and each to be checked against the CLAUDE.md licence rules) or exceptions derived from Word's output, which the maintainer's standing directive forbids ("não derivar exceções em massa da saída do Word"). Not taken here | `ts_hyphenation_patterns` / bit 16 (reserved) |

#### What changed

- uo-docx-layout: `options.rs` (six switches, `ts_strict()`), `engine/flow.rs` (keepLines; `place_paragraph` wraps
  `place_paragraph_inner` to reset `Engine::kl_col_to_page`), `engine/page.rs` (`break_column`), `engine/mod.rs` (the new
  carried field; `incr/relabel.rs`, `incr/bits.rs` list it), `table.rs` (`ParaSplitInfo::keep`, `keep_cut`,
  `split_content_keep`, `RowSplit::split_keep`), `table/layout.rs` (`split_row(…, keep)`, `ldoc.compat_mode()`),
  `engine/static_layout.rs`, `engine/tables.rs` (`split_keep(…, !at_top)`, compat reads), `ldoc.rs` (`compat_word`,
  `default_tab_word`, `bookmark_marks`, `compat_mode()`, `default_tab_stop()`), `derive.rs`, `lines.rs`, `hyphenate.rs`,
  `para.rs` (compat reads; `DRun::bold` for the error texts), `engine/floats.rs`, `engine/chain.rs` (`keep_first_lines`),
  `engine/field_refs.rs` (`bookmark_where`, `ref_error`, `bookmark_error`, `is_ref_error_text`, the multi-paragraph
  range), `engine/fields.rs` (PAGEREF). Lint fixed in the resumed session: `chain.rs` `reduce(jm::min)` (clippy.toml bans
  `f64::min`), the `too_many_lines` expectation moved to `place_paragraph_inner`.
- uo-docx (outside W's crates, additive): `model/document.rs` `ReadFacts` (`settings_absent`, `default_tab_absent`;
  outside the JS shape and equality, never written) + `Document::read_facts`; `read/parts.rs`
  `read_doc_formatting_facts`; `read/fulldoc.rs`; `session/ops/references/fields.rs` `set_field_result` writes Word's error
  result of a REF/PAGEREF/NOTEREF in bold (the TS never produces these texts, so its replays are unchanged) — on the model's `fieldResult` path only (see "Not done": the lossless read's path).
- Strict replays → `.ts_strict()`: `layout_parity.rs`, `f8_parity.rs`, `f4_parity.rs`, `f6_parity.rs`, `g8.rs`, uo-ffi
  `server_parity.rs`; wasm `write.rs` bits 12–17 (`LAYOUT_TS_THEME_TINT` … `LAYOUT_TS_KEEP_LINES`), `js/write.mjs`
  (`tsThemeTint`, `tsMissingSettings`, `tsLinkedCell`, `tsRefBookmarks`, `tsHyphenation`, `tsKeepLines`), `write.d.ts`,
  `tests/support/paint-common.mjs` `wasmOptions`. No product path sets any of them.
- Tests: `word_w7c.rs` (new: the four groups, `cell_lic1_without_settings_is_laid_out_in_mode_12`,
  `linked_boxes_in_a_cell_start_in_the_first_box`, `ref_errors_are_bold`, `tint_shade_samples_against_the_ts_rule`; opt-ins
  `UO_W7C_ORACLE` — the Word-agreement replay —, `UO_W7C_ATTR` (+ `UO_W7C_ATTR_SOURCES` for the editor's read) — which
  switch changes a document —, `UO_W7C_ADHOC` — documents against their Word PDF with the installed fonts —,
  `UO_W7C_LINES` — the line dumps of the hyphenation measurement), `toc_pages_w7.rs` (`xiii`/`xii`), `fidelity_w7.rs` (the
  unterminated REF: Word's error; the TS text with `ts_strict`), `f6.rs`/`f6_parity.rs` (`keep: false`); fixtures
  `tests/fidelity/cell-lic1-nosettings.docx`, `linked-cell-c15.docx` (open fonts only, `check-fonts` clean),
  `tests/oracle_w7c/{keeptall,nosettings,txbxcell,refbm}` (121 `.docx` + `.word.json`) and `tests/oracle_w7c/tint/word-samples.tsv`;
  uo-docx-layout dev-dependency `uo-fonts` gains the `system` feature (the opt-in replays). uo-wasm `write_api.test.mjs`:
  the pin of `f8__7__10` (its REF bm1 is Word's error now; native `write_native` prints the same).
- Oracle tooling (additive): `docx-builder.mjs` `noSettings`, `rawSettings`; `run.mjs --fixdir`; cases `keeptall`,
  `nosettings`, `txbxcell`, `refbm`, `hyphword` (the last one measures only; its Word output stays in the scratch folder).

#### Harness declarations (Word authority only; each document attributed natively to ONE switch)

`word_w7c.rs attribute_documents_to_the_word_rules` over the committed captures: `f7a__7__12`, `f7c__7__11` keepLines;
`f7c__7__2/5/10/13/14/15`, `f8__7__2/10` REF; `f7d__7__0/3/6/7/8/10` linked boxes — nothing else changes. Each was exported
by Word and compared with the installed fonts (`adhoc_documents_against_word`): Word-equal lines/same-page lines go up under
Word's rules on every one (e.g. `f7d__7__0` 51 → 58 of 58 lines, equal to Word; `f7c__7__11` pages 3 → 4 = Word;
`f7a__7__12` same-page lines 422 → 523; `f8__7__10` 95 → 99). The editor harnesses' own documents, attributed the same way:
`fields.docx` (uo-docx d3 fixtures) — only `ts_ref_bookmarks` with the editor's read (`UO_W7C_ATTR_SOURCES`: its `REF
marca`/`NOTEREF n1` name bookmarks it does not define); `tables.docx` — only `ts_missing_settings_mode15`; the other seven
d3 documents — none; `f7a__7__0`, `f7c__7__2`, `f7c__7__8` with the objects harness's columns applied to the package
(`w7c-w/cols3.py`) — `keep_lines` (and `ref_bookmarks` for `f7c__7__2`).
- `viewer_switch.mjs` `W7C` table (16 documents, facets as measured).
- `editor_switch.mjs` DECLARED: `f7c__7__10`, `f8__7__10` (every facet: their REF sits where the script clicks), `f7c__7__2`,
  `f8__7__2`, `f7c__7__5/13/14/15` (end thumbnails/screens; `f7c__7__14` page sizes), `f7c__7__11`, `f7a__7__12` (keepLines:
  every facet, pages after load), `f7d__7__0/3/6/7/8/10` (end thumbnails and screen).
- `editor_switch_fidelity.mjs`: the generator now writes a mode-15 settings part (`defaultTabStop` 720) so the scenarios
  compare the engines in one mode; the new scenario `cell-lic1 nosettings` keeps a package without settings (declared,
  item 2); `linked first`/`linked second` (item 3, end screens); `cell-lic0` (item 3: the mode-15 wrap moves every
  scripted click, declared `*`).
- (resumed session) `editor_switch_stories.mjs`: `X1 f7d__7__0` (item 3: the click lands in the first box's text; every
  facet), `T2 f7d__7__8` (item 3) and `N2 f7c__7__13` (item 4) end thumbnails/screens. `editor_switch_objects.mjs`
  `DECLARED_O`: `f7a__7__0|page` (from step 5), `f7c__7__2|page`, `f7c__7__8|page` (from step 6: the setColumns steps —
  item 6 in columns), `f7d__7__3|objects`, `f7d__7__10|objects` (from step 1, item 3), `f7d__7__0|objects`,
  `f7d__7__3|menu` (end, item 3). `editor_switch_review.mjs`: `fields|fields` (item 4: `updateFields('all')` answers 11
  against the TS's 9 — the two error results —, the screens, the overlay mask, the saved body). `editor_switch_completion.mjs`
  `DECLARED_C`: `f7c__7__2|lists` (end thumbnail page 10, item 4), `f7d__7__10|objects` (from step 5, item 3).
  `editor_switch_format.mjs`: `fields|text` and `fields|view` gain `W7C_FIELDS_REF` (item 4: the TOC statistics from step 5,
  the end pages, the overlay screen and the saved body) — the matcher accepts an array of declarations per key and `from` + `facets`
  (additive; the existing tag entries unchanged).

#### Not done / open (for the coordinator)

- Items 1 and 5 above (OPEN, with the numbers; not relabelled, not accepted).
- PAGEREF to a bookmark in a header/footer: Word `1` on a one-page document; the layout keeps the cached result (no page
  of a header bookmark is known). REF/PAGEREF to a bookmark outside the body index in a document read WITHOUT the
  bookmark marks (the viewer's read, `sources` off: the reader indexes body bookmarks only and keeps no mark runs) keeps
  the TS behaviour (cached result); the editor's read has the marks and follows Word. A reader-side index of non-body
  bookmarks would close it (uo-docx).
- A multi-paragraph REF range carries the text and line breaks; Word also carries each run's formatting (bold "parte" in
  `rb-multi`) and the paragraphs' own properties.
- STYLEREF to a style NAME (`STYLEREF "Título 1"`): the engine (as the TS) knows only the numeric level form; Word resolves
  the localized name. Not one of W's items (found by the `refbm` series).
- Mode 12, `layoutInCell="0"`: Word lays the whole TABLE out beside the out-of-cell boxes (`lc-cell-lic0-c12`, left border
  at 196.4 pt; Δx 116.96 pt); both engines keep it at the margin (listed `open` in `word_w7c`). With item 2 this now
  concerns every package without `settings.xml` that has a table next to a float.
- A line 0.01–0.5 pt short of the inner width of a 0.5-pt-bordered box (102.36 pt, "aprovação gestão" 87.45 pt of 87.96)
  breaks in Word, not in either engine (the `txbxcell` boxes were widened to 104 pt to keep item 3's series on its rule).
- The SAVED form of the error result in the editor's (lossless) read: Word writes `<w:b/><w:bCs/>` on the result run; the
  session keeps the field's source runs there and replaces only the text, so the error is saved unbolded (seen in the end
  runs: `editor_switch_format` fields|view saved `<w:r><w:t>Erro! Fonte de referência não encontrada.</w:t></w:r>`). The
  resumed session tried giving the field run `bold` on that path (`set_field_result`): the lossless field writer does not
  emit it (a native test over `open_body` with REF/PAGEREF/NOTEREF to `nada` saved no `w:b`), so the attempt was reverted;
  closing it needs the lossless writer to patch the result run's `w:rPr` — for the session/writer owner. The layout paints
  the error bold in every read (`ref_errors_are_bold`).
- Writer: Word SAVES a package without settings with `compatibilityMode` 12 (+ `useWord2013TrackBottomHyphenation`); the
  session writes 15 (model unchanged on purpose: no saved-byte change) — for the session owner.
- **Corrected in fix round 1** (reviewer W/2; this paragraph called them "not W's" without evidence — wrong):
  `editor_switch_format` `tables|ruler` and the `exportXps` print-cell screen ARE item 2's — attributed with ONLY
  `ts_missing_settings_mode15` switched on ("Wave 7c — W, fix round 1") and declared there. Not W's (owners in their
  records): `sections|view` (the Web view's left 48 vs 24: V's continuous view; the document is changed by no W switch);
  `editor_switch_objects` `f7b__7__0|objects` frame 0 of the float move (V's "Not done 1").
- .NET (`UniversalOffice.Docx/DocxLayout`): none of the four applied rules ported; port them and regenerate the parity
  dumps (CLAUDE.md "porta com paridade").

#### Verification (resumed session; logs `core/target/m2-wsession/w7c-w/*.log`, `-j 3`, ≤ 3 test threads, the one target
`core/target/m2-wsession`; D: 51 → 43 GB free)

- Word series, `word_w7c` (release, 3 threads): **12 passed** — keeptall 64 cases / 6,884 lines / max Δy 0.15 pt / max Δx
  0.08 pt (`ts_strict` differs in 37); nosettings 25 / 190 / 0.11 / 0.10 (7); txbxcell 22 / 480 / 0.13 / — (9; 21 equal,
  `lc-cell-lic0-c12` open, Δx 116.96 pt); refbm 10 / 51 / 0.17 / 0.05 (9); tint 2,323 / 3,356 pinned; the opt-ins run by hand
  (`UO_W7C_ATTR`, `UO_W7C_ORACLE`, `UO_W7C_LINES`).
- uo-docx-layout, all targets (release): **168 passed, 0 failed** (13 unit + 155 in 27 test binaries: fidelity_w7 4,
  toc_pages_w7 5 with `xiii`, la7 13 incremental = full, views_w7c 2, layout_parity 5, f8_parity 2, …). uo-fonts
  `--all-features` + uo-render: **434 passed, 0 failed**. uo-docx `--release --all-features --test all --skip memory_`:
  **667 passed, 0 failed, 1 ignored** (11:15 run; the 500-page memory soaks are the orchestrator's), `--lib` 153. uo-ffi
  `server_parity` 3/3.
- Full layout parity, strict, captured from the frozen TS (`w7c-w/lp-*.log`): ci 200 documents / 600 facets, f7a 300 / 900,
  f7b 200 / 600, f7c 300 / 900, f7d 200 / 600 — **0 mismatches each**; f8 200 documents, **3,000 equal**, 0 mismatches; the Word
  oracle **2,742 documents / 8,226 facets, 0 mismatches**. The new groups (121 `.docx` of `tests/oracle_w7c`, captured in the
  capture's files mode, replayed strict): **363 equal, 0 mismatches**.
- Word agreement (`UO_W7C_ORACLE` over the frozen TS's 2,742 oracle documents, installed fonts): 2,741 cases with a
  `.word.json`; 21 change under Word's rules (hftpl 3, inside 8, rotwrap 5, tbinset 5) and every one is Word-equal before and
  after (21 → 21): the count does not go down (the changes stay within the comparer's tolerances; W's rules were measured
  with W's own groups, above).
- Wasm (`--profile wasm`, `-j 3`): viewer 4,142,626 B, session_count 10,498,171 B, session 10,497,667 B, write 4,142,647 B,
  selftest 11,853,874 B (built after V's concurrent `LineGeom::desc` edit had landed). Node: `paint_parity` 2 + 1 skipped,
  `session_replay` (d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10) **10/10, 0 mismatches in every family** (f2 1, f7 8, f8 122, f9 10, f10 53
  accepted — F3's numbers, unchanged), `write_api` 8/8, `session_api` 5/5, `shell_api` 2/2, `write_section` 2/2.
- Natives: `session_native` 11/11, `shell_native` 9/9, `shell_stories_native` 8/8, `shell_objects_native` 5/5,
  `shell_caret_native` 4/4, `shell_review_native` 4/4.
- Chromium (headless; the final pass 12:54–13:48 on the tree of that moment, frontiers V and S still editing):
  `viewer_switch --mode all` (stride 1) **ok** — 78 documents, 592 pages, 0 undeclared, the 16 W7C entries used;
  `editor_switch --mode parity` (13 documents × 12 steps) **ok**; `--mode all` **ok**; `editor_switch_fidelity` **ok**;
  `editor_switch_stories` **ok**; `editor_switch_completion` **ok**; `editor_switch_format` 2 failures, `editor_switch_objects`
  1, `editor_switch_review` 2 — `tables|ruler` (the cell's ruler frame x1 3485 vs 10205 at load) and the `exportXps` print-cell
  screen are W's item 2 (this line said otherwise without evidence: corrected and declared in "Wave 7c — W, fix round 1"); not
  W's: `f7b__7__0|objects` frame 0 of the float
  move (V's open frames; no W switch changes f7b documents), `wordmarks` (a new C0C0C0 band check of V's field-shading work,
  26 px vs Word 17). Each W declaration was exercised in a run where its document's other facets passed.
- Packaging: `build-pkg font-files document-viewer write` with `UO_REQUIRE_ENGINE_WASM=1` and this target's `write.wasm` /
  `session.wasm` exit 0 (`uo-write.wasm` 4,046 KB, `uo-write-editor.wasm` 10,252 KB); `pkg_smoke` ok. No pattern data shipped,
  so no notice or size change.
- Lint: `check-changed --jobs 3` **all steps passed** (16 crates, after the fixes of `word_w7c.rs`: the first run failed on
  the test's `char::is_whitespace`, `f64::round`/`f64::max` — now `uo_jsnum` `is_js_space`/`round`/`max`, as the TS's `\s`,
  `Math.round`, `Math.max` — and on its system-font opt-ins under wasm32 — the file is `cfg(not(target_family = "wasm"))`);
  `rustfmt --edition 2024 --check` clean on every W-touched `.rs`; `check-fonts` on `tests/oracle_w7c` and
  `tests/fidelity`: 54 fonts, none proprietary.

#### For README/CHANGELOG (the coordinator's merge)

- Word's rules over the frozen TS in the Rust layout ("Word manda sobre o TS", measured in Word 16): a keep-lines-together
  paragraph taller than what is left of the page goes to the next page (in columns: the next page; in a cell: below the
  row's split); a package without `word/settings.xml` is laid out in compatibility mode 12 with 708-twip default tab stops;
  the first box of a chain of linked text boxes keeps the chain's first lines; REF/PAGEREF/NOTEREF to an undefined,
  unterminated or inverted bookmark shows Word's error text ("Erro! Fonte de referência não encontrada." / "Erro!
  Indicador não definido."), painted bold (the editor still SAVES it unbolded — Word writes `<w:b/><w:bCs/>`, PAGEREF also
  `<w:noProof/>`: open, for the lossless writer), a multi-paragraph bookmark gives all its paragraphs.
- Open, not accepted: theme tint/shade rounding (Word's rule not found; 3,356 samples, the current rule matches 2,323),
  hyphenation (open patterns: pt 97.4 %, en 89.9 %, es 99.8 % of the measured line splits equal to Word).
- .NET: the four rules are not ported yet.

### Wave 7c — V, fix round 2 (the reviewer's second findings on V)

One implementer; scratch, logs, probes and Word scripts in `core/target/m2-latency/v3/` (outside git), Word screenshots in
`E:\UniversalWriteTest\_lab\views\v3`. The session_count module of this round: `v3/session_count.wasm`, built from the
tree with this round's Rust. Nothing committed. Where this subsection and the earlier V subsections differ, this one is
valid.

**Word measured** (Word 16, invisible COM through `tools/parallel/word-lock.mjs`, the window off screen at −20000 shown
without activation, PrintWindow; 96 dpi):
- `v3/word-v3.ps1` (window 1100 × 1000 px, zoom 100 and 200 for the shots): every LINE start (paragraph, offset, x, y) of
  seven documents in Web, Draft and Outline, walked with `Selection.MoveDown(wdLine)` + `HomeKey(wdLine)` and `GetPoint`;
  the end mark of each paragraph; `Selection.MoveDown(wdScreen)` from the start (PageDown); the Web view after the window
  is narrowed 1100 → 900 px with no view, zoom or selection change; PNG shots with the formatting marks off. Documents
  (open fonts, no embedded font; `v3/mkdoc3.py`), committed in `uo-docx-layout/tests/fidelity/`: `v3-web-edges.docx`
  (left/right/centred short paragraphs, a long one), `v3-sections-rev.docx` (fix round 1's two sections with landscape
  first), `v3-outline-wide.docx` (headings and body on a landscape page), `v1-sections.docx` (fix round 1's, now committed),
  and the committed `v1-outline.docx`, `v1-breaks.docx`, `captures/f7a/docs/f7a__7__0.docx`; `v3/word-lines.py` turns the
  log into `tests/fidelity/views-word-v3.tsv` (1,231 line starts; f7a__7__0 also in Print view, `-Views 3`). The first run
  ended with "foreground unchanged=False" (6 minutes of COM; no window of ours was activated — SW_SHOWNOACTIVATE, a tool
  window off screen —, so most likely the user's own switch); the two later runs: True.
- `v3/word-shading-stories.ps1` on `field-shading-stories.docx` (a QUOTE field in the footer and one in a footnote, 11-pt
  Liberation Sans, single; `v3/mkdoc-stories.py`), View.FieldShading = 2 ("When selected"): C0C0C0 rows — body caret none;
  footer caret outside the field none, inside 17 rows; footnote caret outside none, inside 17 rows (StoryType 9 and 2).

Findings: Web wraps again AT ONCE when the window is resized (the right-aligned paragraph's end 1045 → 845 px, a long one 2
→ 3 lines, no view or selection change). Web's text runs 38 → 1045 px in a 1100-px window (1007 px = 15105 twips). Outline
wraps a section at most 8640 twips (6") from Draft's text left — A4 portrait text 567 px stays 567 (break line 519 px from
the +48-px body indent); A4 landscape text 971 px → 576 (528 px), with the landscape section first, last or alone; Draft
keeps each section's own width. Outline sets every paragraph from the left (right-aligned and centred text at the body
indent). Draft and Outline give a page or column break a LINE of its own (`v1-breaks`: line starts 0, 25 = the break, 26)
and the section break a line after its paragraph (`v1-sections` Draft 165 → 183 → 212 px; Outline 154 → 171 → 188); Web
does neither. The break is drawn across the whole pane: 1-px black dots every 2 px (page), every 4 px (column), two rows
2 px apart (section), 2.3 px above the break line's baseline at zoom 100 (scaled at 200), the label ("Quebra de página",
"Quebra de coluna", "Quebra de seção (Próxima página)") in 9-pt Segoe UI NOT scaled by the zoom, centred in the pane,
its baseline 2 px under the rule, the dots stopping 3 px either side. Outline symbols (scale with the zoom): a heading's
circle 14 px across, a body paragraph's 8 px, fill E1E1E1, a 1-px 777777 ring, centred 17.5 px left of the first line's
text and 1 px above the line's middle; the heading's plus (minus without text under it) 444444, 2 px thick, 8 px long.
PageDown (`MoveDown(wdScreen)`) from the start of f7a__7__0: Web paragraph 5 offset 102, Draft 7/333 (the last full line of
the 963-px pane), Outline 8/363 (0-based) — a screen, not the TS's 20 lines; no consistent rule derived (Web lands
473 px down, Draft 912). f7a__7__0 in Print: the engine's first 13 lines equal Word's, then differ (paragraph 4: 305 vs
311), so its continuous-view lines differing from Word's is the engine's layout of that generated document (hidden
paragraph marks, mixed fonts and scales), not a view rule.

**(1) ED-707/708 — the blocker.** `editor_switch_format --mode view` (V5 steps on): **2,888 facets equal, 302 declared, 0
undeclared, ok** (fix round 1: 34/35 undeclared). Each difference the reviewer listed, traced:
- *the resize* — the TS lays a continuous view out on every `refresh()` and every selection change (`refreshSelection`
  without an engine line → `refresh()`, 21379; its `contentWidthTwips()` reads the element's width then), not on the
  resize itself (`measureSides` returns when no side panel changed); its ruler frame is computed from the element's width
  at the call (17141). Shell (`rust-format-view.ts`): `webRelayout` — the Web view laid out again at the viewport's width
  when it changed — on every selection-only change (new `hooks.selection` in `rust-engine.ts`: `apply`'s no-story-change
  branch and `refreshSelection`; registered in `rust-engine-format.ts`), and Word's rule (measured above) by default: a
  `ResizeObserver` on the element lays the Web view out at once, off under `tsStrict`; `getRulerInfo`'s Web frame =
  the TS's formula. Checked by `editor_switch_format --mode extra` "extra web resize" (Rust: the default 1052 → 752 px at
  once, back to 1052; `tsStrict` keeps 1052 until the next selection change, then 752; the frame 10680 at 800 px in both).
- *the TS "inset 64 / left 0 / w 752px"* — harness reads of a DETACHED canvas: `firstInset` and the `present` facet took
  the canvas before `firstInset`'s selection change, which (above) makes the TS rebuild its canvases. Both now read the
  canvas after it. With that, every `present`/`ruler` facet of the resize steps is equal.
- *the Shift+click (TS 143, Rust 0) and the `selected` facets* — the harness's `clickPoint` measured the point with the
  caret at the target, then put the selection back, which scrolls both engines back to the old caret: the click landed on
  another paragraph in each (TS 12, Rust 11, by their own line geometry). The view is now put back at the scroll the point
  was measured at; the Shift+click lands on the target in both (equal). The `fields|view` Shift+click selects across the
  TOC, whose REF/NOTEREF results are W's item 4: that entry declares `selected` for `fields|view` too.
- *`view:web` canvas left (TS 24, Rust 48)* — the same stale width: equal after the selection-change relayout.
- *PageDown* — the same code in both (0.9 of the view, 20 line moves, universal-write.ts 10597 / `rust-engine.ts`); the
  landing differs where the lines fall differently (the engine's layout vs the TS legacy canvas layout, LQ2). Declared
  with tags of their own (`web-pgdn`, `draft-pgdn`, `outline-pgdn`), not hidden under the view tags; Word's PageDown
  (measured above) is neither engine's: **open**.
- *the declaration* — `WORD_VIEWS` (290 facets "Word manda sobre o TS (V1 measurement)") is gone. `LQ2_VIEWS` (id "LQ2 (the
  engine's line geometry in the continuous views)") declares only the facets that follow from where the lines fall:
  `rulerLine` and `canvases` of the view steps, the Web screen, and the PageDown steps' caret facets. It does NOT claim
  Word: the engine equals Word line by line where measured (below); the harness documents were not measured in Word
  (f7a__7__0's lines differ from Word in Print too). The stale "ED-707/708 closed" comment is replaced.

**V1/V4 — Word's views applied** (all behind `!ts_strict`, the frozen TS kept for the strict replays):
- engine (`uo-docx-layout`): `view_transform.rs` `OUTLINE_WRAP_MAX` = 8640 (Outline wraps at min(the section's text
  width, 8640)), `word_outline_document` sets `jc` left (pPr and the editor props); `options.rs`
  `ViewTransform::word_break_lines()` (Continuous Draft/Outline, not strict); `lines.rs` `BreakSettings::break_own_line`
  (a page/column break that follows text ends the line before it and takes a line alone); `engine/flow.rs` the section
  break's line after a section-ending paragraph with text (the mark's line box: `line_box` of the mark face) and the empty
  section-carrier paragraph keeps its line in those views.
- shell (`rust-format-view.ts`): `paintWordBreak`/`paintWordBreaks` draw Word's rule and label in device pixels on the
  break's own line (the section break's on the line after its paragraph), with or without Show Marks; the TS's
  `paintBreakMark` only under `tsStrict` (or with Show Marks there); `paintOutlineSymbols` with the measured geometry and
  colours (dark page: not measured, the same colours).
- native test `uo-docx-layout/tests/views_w7c.rs` (the planned one): `continuous_views_wrap_and_place_lines_as_word` — every
  line start of `v3-web-edges`, `v1-sections`, `v3-sections-rev`, `v3-outline-wide`, `v1-outline`, `v1-breaks` in Web
  (the sheet = Word's 15105 twips + the TS margins), Draft and Outline equal to Word's (paragraph and offset; Word's
  section-break pseudo-lines left out), each line's x within 1.5 px, the pitch inside a paragraph within 1 px: **171 lines,
  0 differences**; `section_break_takes_a_line_in_draft_and_outline` (Draft 47.1 px vs Word 47, Outline 33.7 vs 34);
  `f7a_against_word_probe` (`UO_VIEWS_F7A=1`, prints only: the Print/Web/Draft/Outline comparison above); the earlier two
  tests unchanged and passing.

**(2) F-1 — open.** Traced with a probe (`v3/s-f1b.json`, both engines, the click at the TS target of float 0): the TS
calls `positionProxy` at 5 and 45 ms (258 px); its layout of the click's `refresh()` (7 ms) asks the resolver for "Times
New Roman" for the first time (`layoutEngine` → `scheduleEngineFonts` → `loadEngineFonts` → `resolveWithFile`, 50 ms —
`s-f1c.json`, a stack per call), the answer comes back as `onFontsResolved` (57 ms) → `queueRefresh` → `refresh` (137 ms)
→ `positionProxy` (194 ms): 284 px, after the Shape Format tab grew the bar 178 → 204 px (~94 ms). The Rust shell resolved
every family of the document with its first layout and asks nothing after the click (no resolver call, no event): 258 px. A
reposition on the bar's growth would move f7b__7__1/3, where the TS stays (fix round 1). Reproducing it needs the TS's
lazy face request; not done.

**(3) ED-304 — closed.** `rust-review-overlays.ts`: `selectionKey` answers the header/footer, note and text-box
paragraph the caret is on (the story line of the caret's page and frame holding the caret point and the focus offset);
Word's marks are painted over every story's lines (text boxes included; the TS look keeps its exclusion); the base key
reads the field runs from that story line (`runsOf` caches by layout and paragraph index only — after the body's
paragraph 0, a footer's paragraph 0 read the body's runs: the bug that hid the footer band in the first harness run;
R's other `runsOf` callers in a non-body story may meet the same cache, not changed here); the base is painted with the
selection that keyed it. The band's height: the engine's natural text descent — new `desc` on `PassLine`/`LineGeom`
(`Line::desc`; `items.rs`, `output.rs`, `engine/flow.rs`, `engine/mod.rs`; the TS captures decode 0, not compared:
`layout_parity` compares a list of named fields), `uo_shell_story_lines` writes it (`shell_review.rs`), `StoryLine.desc`
(`session.d.ts`, `write-session.d.ts`), `StoryLineP.desc` (`rust-review-core.ts`); the 0.234 × ascent estimate is gone.
`editor_switch_review --mode wordmarks`: body 17/17/31 px and the comment 17 px as before; footer and note: one 17-px band
with the caret in the field, none outside (found as the rows that change: the hf/note edit tints its story).

**Minor**: D-L3 recorded in the merge state's engine-boundary list with fix round 1's evidence (the frame comparisons
re-allocate the buffers). The memory soak's wrap-point drag: **not added** (the soak runs Rust only; the wrap-point target
needs the selected picture's rectangle, which only the TS internals or the shell's private handle give).

**Verification** (`v3/verify-rust.sh`, `v3/verify-b.sh`, `v3/run-harness.sh`; logs `v3/vr-*.log`, `v3/vb-*.log`,
`v3/h-*.log`; `-j 3`, ≤ 3 test threads, the one target `core/target/m2-latency`; D: 45 → 43 GB free; the session_count
module `v3/session_count.wasm`, 10,498,815 B):
- Rust: `uo-docx-layout --lib` 13 passed; `--test views_w7c --test word_w7c --test f5 --test f6 --test f8` 14 + 4 + 6 + 5
  + 12 passed (views_w7c 5: 171 Word line starts, 0 differences); uo-wasm natives `shell_review/format/native/stories/
  objects/completion` 4 + 4 + 9 + 8 + 5 + 5 = 35 passed; clippy `-D warnings` native (`uo-docx-layout` lib and the tests
  views_w7c, f5, f6, f6_parity, f8, layout_parity) and wasm32 (`uo-wasm` lib, `uo-docx-layout` lib), `--all-features`: 0
  warnings; `rustfmt --check` on every touched `.rs`: clean. `node tools/check-changed.mjs --jobs 3`: float-casts and
  grep-bans pass; clippy fails ONLY in W's `tests/word_w7c.rs` (13 lints and `uo_fonts::system` not found — W's work in
  progress in the tree; my first run's two findings, `view_transform.rs` test indexing and a `str::parse` in views_w7c.rs,
  fixed).
- Layout parity (frozen TS captures, this tree): ci 200 documents equal (600 facets, 0 mismatches); the Word oracle
  2,742/2,742 (8,226 facets), 0 mismatches; f8 200 documents (3,000 facets), 0 mismatches.
- TS: `editor_switch_tsc.mjs` editor and viewer exit 0, no output; vitest editor 34 files / 334 tests, viewer 7 / 28 (+1
  skipped).
- Chromium, V5 steps and frames ON, no V declaration but LQ2's geometry facets: `editor_switch_format --mode view` **ok**
  (2,888 equal, 302 declared, 0 undeclared, 96 s); `--mode extra` 1 failure **not V's**: `exportXps side effects,
  print-cell` (the same TS/Rust screen hashes `fcd571a5`/`13814fe5` S recorded: W's item 2, a package without
  settings.xml), the new "extra web resize" check passes; `editor_switch_review --mode all` 1 failure in the first run
  (the note's wordmarks band picked up a 3-row change from the spelling's squiggles arriving between the captures — the
  check now takes out, in, out again), `--mode wordmarks` ok ×3 after the fix, every other family 0 undeclared (styles 36
  and fields 5 declared: B's and W's); `editor_switch_completion --mode all` **ok** in 502 s (lists 1,462 + 1 W, paste
  1,069, tables 1,004, objects 2,557 + 44 W item 3, inline 395, page 134, previews 200; memory 30 cycles flat at
  13,829,745 live bytes, handles/sessions 0); `editor_switch_objects --mode all` in 534 s: 2 failures — **F-1**
  (f7b__7__0 frame 0 proxy 284 vs 258) and f7b__7__1 frame 0 proxy (TS 7852, Rust 7883: under the parallel cargo load;
  `UO_EO_ONLY` rerun of f7b__7__0/1: f7b__7__1 equal, F-1 alone) — page 3,739 + 65 W, objects 5,557 + 72 W, pictures 855,
  menu 217 + 2 W, keys 171, views 362; memory 30 cycles flat at 13,400,333, handles 0; `editor_switch --mode all` **ok**;
  `--mode parity` (session.wasm, stride 1, 12 steps) **ok** (every facet equal or declared); `editor_switch_stories --mode
  all` **ok**; `editor_switch_fidelity --mode all` **ok**; `viewer_switch --mode all` (viewer.wasm) **ok** — 78
  documents, 592 pages, 0 differences in pages, text layer, find, pixels (tolerance 0) and PDF, 200 open/close cycles
  flat; `build-pkg font-files document-viewer write` (`UO_REQUIRE_ENGINE_WASM=1`) exit 0, `pkg_smoke` ok.
- Not measured this round: typing latency and a 500-page document in the continuous views (a continuous view still lays
  the whole document out per edit, `FallbackReason::View`).

**Files (fix round 2)**: `write/packages/editor/src/rust-format-view.ts` (`webRelayout`, the Web relayout on a selection
change and Word's on a resize, the Web ruler frame, `paintWordBreak(s)`, the Outline symbols, `selectionRelayout`),
`rust-engine.ts` (`hooks.selection` + its two calls), `rust-engine-format.ts` (registers it), `rust-review-overlays.ts`
(`selectionKey` for every story, Word's marks on every story, the keyed selection, the real descent),
`rust-review-core.ts` (`StoryLineP.desc`), `write-session.d.ts` (`StoryLine.desc`); `core/crates/uo-wasm/js/session.d.ts`
(`StoryLine.desc`), `src/session/shell_review.rs` (`desc` in the story lines); uo-docx-layout `src/options.rs`
(`word_break_lines`), `src/view_transform.rs` (`OUTLINE_WRAP_MAX`, Outline `jc`, the tests' clippy allowance),
`src/lines.rs` (`BreakSettings::break_own_line`), `src/engine/flow.rs` (the section-break line; `PassLine.desc`),
`src/items.rs` + `src/output.rs` + `src/engine/mod.rs` (`desc`), `tests/f6.rs`, `tests/f6_parity.rs`,
`tests/common/decode.rs` (the new field), `tests/views_w7c.rs` (three tests), fixtures `tests/fidelity/v1-sections.docx`,
`v3-sections-rev.docx`, `v3-web-edges.docx`, `v3-outline-wide.docx`, `field-shading-stories.docx`, data
`tests/fidelity/views-word-v3.tsv`; harnesses `editor_switch_format.mjs` (`firstInset`/`present` read after the
selection change, `clickPoint` keeps the scroll, `LQ2_VIEWS` replacing `WORD_VIEWS`, the PageDown tags, W's REF entry
with `selected` for `fields|view`, the "extra web resize" check, the stale comment), `editor_switch_review.mjs` (the
footer/note wordmarks); this subsection and the merge state's ED-707/708, F-1, ED-304 and D-L3 lines. No fixture holds a
font (`tools/fonts/check-fonts.mjs`: no proprietary font).

**Not done / open (none is a decision):** (1) F-1 (the TS's late "Times New Roman" request; a timing variant on
f7b__7__1 under load); (2) Word's PageDown (`MoveDown(wdScreen)`) differs from both engines' 20-line rule in every view,
its rule not derived (Web 473 px, Draft 912 px from the same start); (3) f7a__7__0's engine layout differs from Word's in
Print as well (13 of 326 lines) — a layout item of that generated document, not of the views; (4) the memory soak still
has no wrap-point drag; (5) typing latency and a 500-page document in the continuous views not measured; (6) Word's black
Office theme not measured (the dark break/symbol colours are invented for visibility: light ink on a dark page); (7)
R's `runsOf` cache (by layout and paragraph index) can hand a header/footer or note paragraph the body paragraph's runs
— avoided in V's code, the other callers unchanged; (8) from fix round 1, unchanged: a follower box's spelling resolves
the chain head by the lowest `seq`; the frozen TS's REF text inside a Paste-as-Picture preview not captured.

### Wave 7c — W, fix round 1 (the reviewer's findings on W)

Frontier W, fix round 1 (2026-11-13). Scratch, scripts and logs: `core/target/m2-wsession/w7c-w/fr1*` (outside git); the one
target `core/target/m2-wsession`, `-j 3`, ≤ 3 test threads.

**W/1 (major) — item 2's 708 tw was not a fallback.** The layout took Word's 708 tw from `Document::read_facts`, which no
session op updates, so `setDefaultTabStop` did nothing under `engine="rust"` on a package without `w:defaultTabStop`,
`getDefaultTabStop` said 720 while the text was laid out at 708, 720 could not be chosen, and two session reads used the
model's mode 15. Fixed:
- uo-docx `model/document.rs`: `Document::word_compat_mode()` (12 for a package without settings, else the model's) and
  `Document::word_default_tab_fallback()` (708 while `read_facts.default_tab_absent`), outside the `canon` gate.
- `session/ops/text_layout.rs` `apply_default_tab` clears `read_facts.default_tab_absent`: an explicit value (the dialog,
  the undo's restore) is what the layout follows from then on.
- `session/ops/lists.rs` `default_tab_stop()` (`getDefaultTabStop`, the ruler's and `getTabStops`' `defaultTabTwips`, the
  list indent shift, `set_default_tab_stop`'s early return) answers Word's 708 when the settings give none;
  `session/ops/sections.rs` `F4State::install` loads that value (the undo restores 708).
- `session/ops/references/manual_hyph.rs` (the manual hyphenation minimums) and `session/ops/completion.rs` (the table style
  gallery's modern/2010 choice) read the effective mode.
- `session/io.rs` `OpenOptions::ts_missing_settings` (default `false`): drops the reader's facts at the open, so the session
  and the layout answer the frozen TS's 720 and mode 15 (for strict replays; none needed it: uo-docx 668/668 and the wasm
  `session_replay` 10/10 families with 0 mismatches unchanged).
- Test `uo-docx-layout/tests/word_w7c.rs` `default_tab_stop_of_a_package_without_one_follows_the_session` over
  `ns-tabs-none` (no part), `ns-tabs-bare` (empty part), `ns-tabs-c15notab`: `getDefaultTabStop` 708 at load, the session's
  layout = the reader's under Word's rules, `setDefaultTabStop(720)` and `(1440)` each move the tab-led lines, two undos
  return to 708 and the loaded layout; `ts_missing_settings` answers 720.
- Found by the format harness after the fix: the Tabs dialog re-read its own display — 708 tw shown "1,25 cm" parses to
  709 — so OK without touching the field called `setDefaultTabStop(709)` (a change event and a dirty document; Word keeps
  the value). `write/packages/editor/src/chrome-dialogs.ts`: the default tab stop is applied only when the field's text
  changed (both engines; the frozen TS's 720 = "1,27 cm" parsed back to 720 was unaffected).

**W/2 (major) — the two `editor_switch_format` failures are item 2's; the record's "not W's" was wrong.** Attributed:
- `uo-wasm/tests/w7c_item2_native.rs` (new): the shell's ruler line (`uo_shell_fmt_ruler`, what `getRulerInfo` reads) of
  `tables.docx` (no settings part) on the session's layout with the default options = `{"x":1701,"frame":[0,1784]}`
  (1701..3485, the first cell), with ONLY the layout bit 13 (`LAYOUT_TS_MISSING_SETTINGS`) = `{"x":1701,"frame":[0,8504]}`
  (1701..10205, the TS editor's answer in the harness).
- `word_w7c.rs` `item2_attribution_of_the_editor_harness_documents`: `tables.docx`, and `hyperlinks.docx` with a 2×2 table
  inserted at its start (the `print-cell` scenario): Word's rules with only `ts_missing_settings_mode15` on lay out exactly as
  `ts_strict()` (every line's x and frame, every painted page) and differently from Word's rules; the first cell's text at
  x 1701 (mode 12) against 1809 / 1814 (mode 15).
- Declared in `editor_switch_format.mjs`: `W7C_TABLES_RULER` (`tables|ruler`, the `ruler` facet of steps 0–2 — the matcher's
  new `to` bound — and the end screen; Word's own ruler over that cell is not measured, the frame follows from item 2's
  geometry), the `print-cell` check accepts a difference in the Print screen alone (logged as declared; TS `fcd571a5`, Rust
  `13814fe5`); W's record paragraphs "Not done" and "Verification" corrected in place.
- With W/1 the session answers 708 on every d3 fixture (none has `w:defaultTabStop`; `styles.docx` has a settings part
  without it): the format harness now keeps the default tab stop in ONE facet, `defTab` (`getDefaultTabStop` and the copies
  in `getTabStops`/`getRulerInfo`, checked to agree within each engine, stripped from `tabs` and `ruler`, which stay strict;
  the Outline step's answer drops it too), declared `W7C_DEFTAB` ("Word manda sobre o TS (item 2, default tab stop 708)":
  COM `DefaultTabStop` 35.4 pt, oracle `nosettings`) on `headers|para`, `tables|ruler`, `sections|ruler`, `hyperlinks|bar`,
  `fields|view`, `many-fields|view`, `sections|view`.

**W/3 (minor)** — the README/CHANGELOG bullet now says the error text is painted bold and that the editor's SAVED bold is
open (`<w:b/><w:bCs/>`, PAGEREF also `<w:noProof/>`: the lossless writer, for the session/writer owner). Not closed here.

**W/4 (minor) — `bookmark_where` scanned the whole document per reference and pass.** `ldoc.rs` `LDoc::bookmark_mark_index`
(a `OnceCell<BookmarkMarkIndex>`, name → the first paragraph's range text or `None` when unterminated there), built on the
first lookup of a layout call by `engine/field_refs.rs` `bookmark_mark_index`, its runs charged to the budget (`bookmark_where`
now takes `&mut Cx` and returns `LResult`; `note_ref_value` split into the bookmark check and `note_ref_body`). Same answers
(the `refbm` series 10/10 Word-equal, `ref_errors_are_bold`, fidelity_w7 4/4); `note_read_all()` stays (the result depends
on every story, LA7).

**W/5 (minor) — viewer_switch page scoping.** The 16 W7C entries scope `textLayer` and `pixels` to the pages the attributed
switch changes (`W7C_PAGES`, 0-based, printed by `attribute_documents_to_the_word_rules` — now with `[pages a → b; changed …]`
per switch): f7a__7__12 1 2 3 8 9; f7c__7__11 0–3 (3 → 4 pages); f7c__7__10 1 2; f7c__7__13 1 2 4 5 8 11–15; f7c__7__14
3 4 5; f7c__7__15 0 3; f7c__7__2 9; f7c__7__5 2 6 7; f7d__7__0/3/6/7/10 0; f7d__7__8 0 1; f8__7__10 1 2; f8__7__2 10.
`find`, `headings`, `linkBoxes`, `pages` and `pdf` stay per document (no page in their detail; the pdf is one hash).

**W/6 (minor)** — `options.rs` `ts_linked_box_cell_chain` documents its second rule: mode 15 wraps a cell's text around ANY
`layoutInCell="0"` box (linked or not); the uo-docx-layout.md row already listed it.

**W/7 (escalation, unchanged — still OPEN, not relabelled)**: item 1, tint/shade rounding (TS rule 2,323 / 3,356 samples,
best model 2,662; no closed form) — for the coordinator; item 5, hyphenation (pt 877/900, en 809/900, es 898/900; closing it
needs a download of licence-checked pattern sources, the maintainer's call) — for the coordinator/maintainer; smaller item-4
residuals — REF/PAGEREF to a missing or non-body bookmark in the viewer's non-lossless read (a reader-side index of non-body
bookmarks: uo-docx reader owner), PAGEREF to a header bookmark, run formatting of multi-paragraph REF ranges (uo-docx-layout,
W's next round), the saved bold (lossless writer: session/writer owner); the mode-12 `layoutInCell="0"` table placement
(`lc-cell-lic0-c12`, Δx 116.96 pt: uo-docx-layout, W's next round); the writer saving mode 12 for a package without settings
(session owner); the .NET port of the four rules and of this round's fallback (`UniversalOffice.Docx`, not touched).

#### Files (this round)

uo-docx: `src/model/document.rs`, `src/session/io.rs`, `src/session/ops/{lists,text_layout,sections,completion}.rs`,
`src/session/ops/references/manual_hyph.rs`. uo-docx-layout: `src/ldoc.rs`, `src/options.rs`, `src/engine/field_refs.rs`,
`src/engine/fields.rs`, `tests/word_w7c.rs`. uo-wasm: `tests/w7c_item2_native.rs` (new), `tests/browser/editor_switch_format.mjs`,
`tests/browser/viewer_switch.mjs`. Editor: `write/packages/editor/src/chrome-dialogs.ts`. Docs: this file.

#### Verification (logs `core/target/m2-wsession/w7c-w/fr1/`)

- Natives: uo-docx-layout all targets (release) **173 passed, 0 failed** (word_w7c **14**: the 12 + `default_tab_stop_of_a_package_without_one_follows_the_session`, `item2_attribution_of_the_editor_harness_documents`); uo-fonts `--all-features` + uo-render **434 passed, 0 failed**; uo-docx `--release --all-features --test all --skip memory_` **668 passed, 0 failed, 1 ignored**; uo-wasm `w7c_item2_native` 1/1, `session_native` 11/11, `shell_native` 9/9, `shell_stories_native` 8/8, `shell_objects_native` 5/5, `shell_caret_native` 4/4, `shell_review_native` 4/4, `shell_format_native` 4/4; uo-ffi `server_parity` 3/3.
- Full layout parity, strict, captured from the frozen TS: ci 200 documents / 600 facets, f7a 300 / 900, f7b 200 / 600, f7c 300 / 900, f7d 200 / 600 — **0 mismatches each**; f8 200 documents, **3,000 equal**, 0 mismatches; the Word oracle **2,742 documents / 8,226 facets, 0 mismatches** (= 2,742/2,742, unchanged). Word agreement (`UO_W7C_ORACLE`, installed fonts): 2,741 cases, 21 change under Word's rules, Word-equal 21 → 21 (unchanged).
- Wasm (`--profile wasm`, `-j 3`): selftest 11,872,102 B, session_count 10,502,583 B, session 10,502,079 B, viewer 4,146,725 B. Node: `session_api` 5/5, `session_replay` (d2a,f1,f2,f3,f5,f6,f7,f8,f9,f10) **10/10, 0 mismatches in every family** (accepted f2 1, f7 8, f8 122, f9 10, f10 53 — unchanged), `shell_api` 2/2, `write_api` 8/8.
- Chromium (headless): `viewer_switch --mode all` (stride 1) **ok** — 78 documents, 592 pages, 0 undeclared, the 16 W7C entries page-scoped; `editor_switch_format --mode all` **ok** (after the Tabs dialog fix; font 947 equal + 2 declared, para 1,073 + 5 (`headers|para` defTab), ruler 669 + 42 (`tables|ruler` defTab + item-2 frame/screen, `sections|ruler` defTab), text 708 + 7, view 2,931 + 431 (LQ2 + W7C_DEFTAB + item 4), bar 608 + 23 (`hyperlinks|bar` defTab); print-cell: the Print screen alone, declared); `editor_switch --mode all` **ok**; `--mode parity` **ok**; `editor_switch_fidelity` **ok**; `editor_switch_stories` **ok**; `editor_switch_review` **ok**; `editor_switch_completion` **ok**; `editor_switch_objects` 1 failure, **not W's**: `f7b__7__0|objects` frame 0 proxy 284 vs 258 (V's F-1, the same numbers as before this round; f7b documents carry settings with `w:defaultTabStop` and no W switch changes them).
- TS: `editor_switch_tsc.mjs` editor and viewer exit 0, no output; vitest editor 34 files / 334 tests.
- Packaging: `build-pkg font-files document-viewer write` (`UO_REQUIRE_ENGINE_WASM=1`) exit 0 (`uo-write.wasm` 4,049 KB, `uo-write-editor.wasm` 10,256 KB); `pkg_smoke` ok.
- Lint: `check-changed --jobs 3` **all steps passed** (after factoring `word_w7c.rs`'s closure type: `clippy::type_complexity`, and a SAFETY comment per unsafe block in `w7c_item2_native.rs`); `rustfmt --edition 2024 --check` clean on every file this round touched. D: 42 GB free at the end.

## Slides viewer (B7, first part: static rendering, 2026-11-17, Sonnet)

`<universal-slides-viewer engine="rust">` over `uo-slides.wasm` (feature `slides` of `uo-wasm`). Scope of this part: the still slide
(navigation, thumbnails, `renderSlideToCanvas`) is laid out and listed by Rust and painted by `paint-slides.ts`; the animation, show and
playback state machines are exported as handles and tested but not yet wired to a painter (B4 emits no slots, so there is nothing for
`evaluate_slots` to drive). Plan §9 roles: `uo-slides` viewer ≤ 1.6 MB gzip; `.ppt` is its own lazily loaded module.

### Files

- Rust (`core/crates/uo-wasm`): `src/slides.rs` (the module, feature `slides`), `src/slides_ppt.rs` (feature `slides-ppt`), `src/buf.rs` (the
  `UoBuf` result type and the four `uo_buf_*` exports moved out of `render.rs`, feature `buf`, so the Slides module links none of the Write
  paint exports; `render.rs` re-exports them, the Write build and its tests are unchanged), `Cargo.toml` (features `buf`, `slides`,
  `slides-ppt`; deps `uo-pptx`, `uo-dml-render`), `src/lib.rs`.
- JS: `js/slides.mjs` + `js/slides.d.mts` (binding: handles with `free()` and a `FinalizationRegistry`, result decoders), `js/slides-engine.ts`
  (module loader, trap rule, `SlidesDocument.paint` = emit + measure loop + decode + `SlidesPainter`, show/timeline handles),
  `js/slides-engine.check.ts` (drift check against the viewer's ambient declaration).
- Viewer (`slides/packages/viewer/src`): `rust-engine.ts` (loaded only by `import()`), `slides-engine.d.ts` (ambient module
  `@universal-office/slides-engine`), `universal-slides-viewer.ts` (attributes `engine` and `link-schemes`; `paintStill`, `configureEngine`,
  `getEngine()`, `rustReady()`, events `us-engine`, `us-engine-error`), `package.json` (+ `@universal-office/drawingml`), `README.md`.
- Tests: `tests/slides_api.test.mjs` (Node and WebKit), `tests/slides_drawops.test.mjs` (Node), `tests/support/slides-stub.mjs`,
  `tests/browser/slides_switch.mjs` (Chromium). Tooling: `tools/build-pkg.mjs` (`shipSlidesWasm`, alias `@universal-office/slides-engine`,
  `ENGINE_SOURCES.slides`), `core/tools/licenses/gen-notices.mjs` (artifacts `wasm-slides`, `wasm-slides-ppt`; `core/licenses/` regenerated: the
  dingbat-to-unicode notice of `uo-dml-render` is now in `texts/uo-dml-render/`), `.github/workflows/rust-core.yml`, `tests/browser/run.mjs`
  (forwards `UO_SLIDES_SOAK`, `UO_PPT_WASM_PATH`).

### API (the module documentation of `src/slides.rs` has the byte layouts)

| Handle | Created by | Exports |
|---|---|---|
| `UoSlidesDeck` | `uo_slides_open(bytes)` (null + `uo_slides_error()`; status 14 = a `.ppt`) | `count`, `size` (EMU), `notes(i)`, `part(name)` (media bytes), `set_caps`, `set_generation` (a font loaded: the measure table empties), `set_image_size`, `clear_caches`, `emit`, `answer` |
| `UoSlidesView` | `uo_slides_view_new(generation)` | one generation of the list stream (`SlidesResources`, `EmitCaches`, `SlidesEncoder`); `bitmap(handle)` |
| `UoSlidesShow` | `uo_slides_show_new(deck, from, now, flags, range)` | `command` (next, prev, go_to, first, last, update), `frame(now)`, `due_at`, `wake_at` |
| `UoSlidesTimeline` | `uo_slides_timeline_new(deck, options)` | `info`, `state(t)` |

`emit(view, i, scale, ox, oy)` returns status 0 (flags, chart frames, the `SlidesList` message), 12 (the measurement requests) or an error status
(3 deck or slide unreadable, 4 limit, 5 bad argument, 6 no such slide, 10 did not encode). The shell answers a status 12 with one `f64` group per request
(`CharAt1000`, `KernAt1000`, `Width`, `Metrics`, `FontAvailable`, `ImageSize` carrying the part name, and B5b's `MetaText` (a raw CSS font) and `MetaDecode` (compressed EMF+ bitmap bytes)) through `answer` and emits again: the two-phase
protocol of B3, at most `MAX_ROUNDS` (8) passes; a fresh table takes 2 or more, one once the table has the slide's advances. `slides-engine.ts` answers with the
browser canvas (`measureText`) using exactly the TS font strings (`layoutFontStr`: `[italic ]{700|400} {px}px {fontStack}`, `fontKerning` and
`letterSpacing` set as the TS sites set them, the `Current` state being the default `auto` of a scratch context); an `ImageSize` for a picture still
decoding is answered 0 x 0 and corrected by `imageReady(part, img)` (`set_image_size`) when it loads, then the viewer repaints. Canvas capabilities
(`'fontKerning' in ctx`, `letterSpacing`, `Intl.Segmenter`, `fontBoundingBox*`) are passed once and are part of the table. A font that loads (`document.fonts`
`loadingdone`, the resolver's `onChange`, an embedded face) bumps the generation (`fontsChanged`), which empties the table. One `SlidesView` serves
the main canvas, the thumbnails and the exports of an element; `EmitCaches::overflowed` (flag bit 0) makes the engine start a new generation after that paint.
`.ppt`: `uo_slides_ppt_convert` of `uo-slides-ppt.wasm` returns the `.pptx` bytes (`Deck::from_parts(...).save()`); the main module then opens
them. The viewer fetches that second module only for a file with the Compound File signature. Faults: a `WebAssembly.RuntimeError` kills the instance (every
handle refuses, the next load instantiates a fresh module, the viewer paints with the TS engine and tells `us-engine-error`).

### What the viewer routes and what stays TS (the hybrid, decided because B4 has no slots)

`engine="rust"` (an attribute of the viewer only; the editor ignores it: it edits the package, which the module would hold stale) paints the still slide through the
list. The TS model (`PptxPackage`, `readSlide`) stays loaded because these still need it: animations, `present()` and `play()` (`shapeAnimHooks` + TS
`paintSlideTo` for the staged slides; the `SlideShow` and timeline handles exist but only the tests drive them), hyperlinks (`hitTestHyperlink`), the
accessible text (`SlideA11y`), the media player, image decoding (`warmImage`, shared: the Rust path asks it for the decoded `part`; WMF, EMF and TIFF are rasterized by the TS
`rasterizeSpecialImage` until B5b). Charts are painted by the list since C-6 (`uo-charts`; `Emission::chart_frames` is empty, `paint` never returns `CHARTS_LEFT` (15) any more and the viewer
calls it without `skipCharts`). Slide notes come from Rust (`getSlideNotes`, compared with the TS: 0 differences).
The bevel light tables and the pattern rows of the painter env come from `@universal-office/drawingml` (TS), not from wasm exports. The hyperlink scheme rule
is Write's (`/^(https?:|mailto:)/i`, `noopener,noreferrer`) with `engine="rust"`; the frozen TS rule (any address, `noopener`) stays for `ts` and for
`link-schemes="any"`.

### Sizes (the shipped build, `--profile wasm`, no `count`)

| Module | Raw | gzip -9 | Budget |
|---|---|---|---|
| `uo-slides.wasm` (`--features slides`) | 2,914,742 B (2,846 KB) | **1,045,866 B** | 1.6 MB gzip (65%) |
| `uo-slides-ppt.wasm` (`--features slides-ppt`) | 736,896 B | 279,872 B | lazy, only for a `.ppt` |

`wasm-tools validate --features=-all,wasm1,mutable-global,saturating-float-to-int,sign-extension,reference-types,multi-value,bulk-memory` passes for both;
neither imports anything. The first build of this part was 1,726,759 B raw / 622,357 B gzip; the metafile players that B5b added to `uo-dml-render` meanwhile are 316 KB (106 KB gzip), and C-6 (the chart engine `uo-charts` with `uo-numfmt`, the host and the canvas) adds 872 KB raw / 317 KB gzip (the 728,867 B row above was measured before it). The Write viewer module is 4.1 MB for comparison (the Slides module has no layout of the Write engine and none of its paint exports).
The packages ship both through `shipSlidesWasm` (content check: no test exports, no counting allocator, the right export set; the notices of each artifact go to
`licenses/uo-slides-wasm/`; negative cases checked: a `count` build and the swapped module are refused). wasm2js: not built for Slides (listed).

### Harness numbers

- **Op stream through the module** (`slides_drawops.test.mjs`, Node; the capture's deterministic stub answers `need_measures`; a fresh view per slide, as the TS
  capture's fresh renderer): `drawops-b1a` 100 decks, 246 views, **246 equal**; `drawops-samples` 53 decks (the legacy ones through the ppt module), 1,166 views,
  **1,166 equal**, 36 views with chart frames skipped (the declared B4-4 gap). The same numbers as the native comparison of B4, now through wasm.
- **Pixels in Chromium** (`slides_switch.mjs --mode all`, headless 1248, open fonts: the Liberation substitutes, tolerance 0): 153 decks (the 53 committed samples and 100
  `b1a`), **847 slides, 847 pixel-equal** (853,795,840 pixels) between `engine="ts"` and `engine="rust"`; 829 of them were painted by the Rust list, 18 slides with
  charts by the TS fallback. Main canvas after `selectSlide` (505 compared) 0 different; thumbnails 0; notes 0; a11y text 0; all 153 decks opened in the Rust engine,
  0 errors. Phase 0: a viewer without `engine="rust"` requests neither wasm nor the engine chunk (the static closure holds none of the engine; the dynamic part is 81 KB of JS).
  The comparison takes 88 s for the 153 decks (a fresh page every 12 decks). Before the chart fallback the same run had 18 slides differing, all with a chart frame.
- **C-6, charts through the module** (`slides_drawops.test.mjs`): `drawops-samples` **1,202 equal of 1,202** (the 36 chart views included, none skipped), `drawops-b1a` 246/246, and the new `drawops-c1d` (60 generated decks
  with charts, pictures, tiles and `themeOverride`) 182/182. **Chromium** (`slides_switch.mjs --mode all`, now with 60 `c1d` decks, UO_SS_C1D): 213 decks, **938 slides, 938 pixel-equal** (968,355,840 pixels), **109 slides with
  charts painted by the Rust list (1,925 paints, 0 left to the TS painter)**, main canvas 596 compared 0 different, thumbnails 0, notes 0, a11y 0, errors 0; memory phase: module live bytes 1,148,702 at every sample (spread 0),
  0 handles after the element left. The harness counts the paint statuses and fails on any difference. Record: `uo-charts.md` "C-6 — Slides host".
- **Memory**: M4 in `slides_api.test.mjs`: 10,000 decks, views, shows and timelines through `free()` return the live bytes exactly (6.1 s), and 1,000 dropped without `free()`
  return them through the finalizer path (Node, `--expose-gc`); in WebKit (26.5, the Playwright build) the same file passes with the finalizer part left to Node. M2: 300 paints of one deck, flat
  (spread under 256 KiB allowed, observed 0). In Chromium: 200 slide changes plus 200 exports of the largest committed sample (15 slides): module live bytes **1,148,626 B at every
  sample (spread 0)**, wasm buffer 2,883,584 B flat, 0 handles and 0 live bytes after the element left (about 1 ms per change plus export).
- **Faults**: three corrupt decks (truncated, empty, byte flips): both engines throw alike and keep the open deck; a forced trap: the viewer drops the instance, paints with
  the TS engine, tells `us-engine-error`, and the next load instantiates a fresh module (`engine` back to `rust`); the hyperlink filter opens 3 of 6 test addresses with
  `rust` (http, https in any case, mailto; refuses `javascript:`, `data:`, `file:`) and all 6 with `ts` (frozen behaviour).
- API tests (`slides_api.test.mjs`, 7): errors (garbage, empty, a `.ppt` signature = status 14, NaN, negative and infinite scale, no such slide), the measure loop (2 or more passes on a fresh
  table, 1 pass later, a generation change empties it), the ppt module (a legacy sample converts; garbage is status 3), show and timeline (a `b6a` deck to the end screen,
  `go_to`, a range hides slides, the timeline state at the half and past the end, a NaN time).

### Gaps (and what B4b must deliver)

1. **Animations, slide show, automatic playback**: DONE by B4b (below, "B4b — the stage"): the frames of `present()` and `play()` are painted from the Rust list with slots. The text that follows is the
   brief B4b worked from. Before B4b: still the TS path. The handles `UoSlidesShow` (frame: the slide on stage, `anim_ms`, the transition and its origin, the number of effects of the
   slide) and `UoSlidesTimeline` are exported and tested, but painting an animated slide needs slots. **B4b must deliver**: `SlotBegin/End` around each shape (replacing the plain `Save/Restore`),
   `Background`, `Paragraph(i)` (`pbox`, `boxBase`), `Unit(k)` (`planUnits`, the pieces measured at device px: a second `Width` kind) and `MediaFrame` over the poster; the `SlotTemplate` filled
   at emission (`uo-pptx::slots`) and kept next to the list in the deck handle; the re-emission of shapes whose `PartState::needs_reemit()` (`fillColor`, `lineColor`, `textColor`, `fontScale`: `emit`
   with overrides); then here an `evaluate_slots(template, PlaybackSlide, anim_ms, media clock, AnimCaches)` export returning `SlotFrame` bytes (`AnimCaches` per deck view, cleared with `clear_caches`) and
   an `emit` for a staged slide at a transition's origin (frozen at `anim_ms`). After that the viewer's `paintPlaybackSlide` and `paintStage` take the Rust path and the TS model stays only for media and links.
2. **Charts**: DONE by C-6 (`chart::paint_chart_slot` emits through `uo-charts`; the viewer no longer passes `skipCharts`); the animated chart parts are DONE by B4b (`ChartPart j` slots, the `chartPartBounds` probes as `BoundsPass` gates). Left: chart pictures that are metafile, PICT or TIFF rasters (answered "unavailable").
3. **`.ppt`**: lazily loaded as its own module (done): 720 KB raw (the converter and the package writer), so a legacy deck costs two module loads.
   **`MetaDecode`: DONE in wave 4** (below, "Wave 4"): the engine decodes the bitmap with `createImageBitmap`, the paint waits
   (`PENDING`, 13) and `SlidesPaintHost.prepared` repaints; the metafile/PICT/TIFF pictures are painted by the Rust recipes (the engine had
   never wired `recipe`, so before wave 4 they were not drawn at all in the browser — see "Wave 4"). The text that follows is the gap as it was.
   Before: **`MetaDecode`** (B5b: decode a compressed EMF+ bitmap and report its size) was answered "cannot be decoded" by the engine: a browser decodes asynchronously, so it needs a prepare phase like the pictures (`createImageBitmap`, then `set_meta_decoded`, then a repaint). With the committed decks nothing asks for it yet (the op streams and the pixels are unchanged); EMF/WMF/TIFF pictures are still rasterized by the TS `rasterizeSpecialImage` and drawn as media.
4. **DONE in wave 4** (below): the deck opens without the TS model; the read-model queries answer the a11y text, the links, the pictures,
   the media table, the fonts and the show settings, and the media player runs on the Rust media states. What was the gap:
   **The TS model is still loaded in `rust` mode**: the open cost of a deck is TS plus Rust. The accessible text, the links and the media table need Rust read-model queries (slide text with run boxes,
   link boxes) before the TS model can be dropped; `Deck` has the data, nothing is exported for it yet. Find: the Slides viewer has no find API.
5. WebKit: `slides_api.test.mjs` passes in the Playwright WebKit build; real Safari 15 / iOS 15 (BrowserStack) was not run for the Slides module; the Safari 15 gate is `wasm-tools validate`.
6. wasm2js fallback for hosts that forbid WebAssembly: not built. A real `build:pkg slides-viewer` / `slides` run could not be made here (the tools root lacks `pdfjs-dist`, the same limit as the Write package build),
   so `shipSlidesWasm` is verified on its own; the viewer's vitest and its own `tsc -p` need `node_modules` (checked instead with a `paths` tsconfig under `core/target`: no errors in `viewer/src`).
7. Real fonts: the pixel comparison ran with the open substitutes; installed-by-name fonts (the TS rule T2 is kept) are drawn by the browser in both engines, but a comparison with installed Aptos or Calibri was not made.

### Process metrics

One session. Builds (`-j 4`, `--profile wasm`): `slides`, `count,slides` and `slides-ppt` 17-19 s incremental. Tests: `slides_api` 7.1 s (default soak), `slides_drawops` 3.3 s, WebKit 3.6 s,
`slides_switch --mode all` (100 `b1a` decks) 90 s. Rework: 5 (the `UoBuf` re-export for the existing native tests; the viewer's `PaintImage` ambient type; the harness converter alias and `pdfjs-dist` external
for the esbuild bundle; the memory phase first picked a one-slide deck; the finalizer part of M4 skipped where there is no `gc()`). 0 parity rounds: the op streams were equal on the first run through the module.

C-6 additions to this part (2026-11-17, Sonnet): `slides-engine.ts` measures a chart's empty-family face as `10px sans-serif`; `rust-engine.ts` drops `skipCharts`; `src/slides.rs` has one lint fix (`as_chunks`, Rust 1.100); the stub (`tests/support/slides-stub.mjs`) answers `metrics` like the recorder and `slides_drawops.test.mjs` gained the `c1d` gate. Wall time: the Chromium harness 6 min (213 decks, up from 88 s for 153), `slides_drawops` 5 s, wasm build 22–42 s.

### B4b — the stage: animations, `present()`, `play()` (2026-11-17, Opus)

Record of the Rust side: `uo-pptx.md` "B4b — playback emission"; emitter: `uo-dml-render.md` "B4b"; schema: `uo-render.md` "B4b schema
changes". Here the binding and the viewer.

**Exports** (`src/slides.rs`; layouts in the module documentation). A `UoSlidesView` now also holds two STAGES (0 = the slide on stage, 1 =
the transition origin), the `AnimCaches` of the slot evaluation and the frame stream (`FrameEncoder`, the `SlotFrame` messages):
- `uo_slides_stage_show(deck, view, show, now, which, scale, ox, oy, media, latency, clock_scale)` and `uo_slides_stage_timeline(deck, view,
  tl, t, which, ...)`: the frame of the show at `now` (or of the timeline at `t`) on stage `which` — the stage list (emitted again only
  when the slide, the viewport, the media flag or the hooks' structure changed; status 0 then carries it, flag bit 2), then the
  `SlotFrame` (values positional over the stage list's slots, plus the replacement lists of the shapes re-emitted this frame, each a
  `SlidesList` message of the same stream with its own values). Status 12 = the measurement requests (the same answer protocol). The timeline
  origin is painted in its final state (`anim_ms` infinity) and the show origin at its frozen `anim_ms`, as the viewer's `paintStage`. A
  new stage is kept, and its list encoded, only when its first frame is complete (a pass that needs measurements delivers nothing).
- `uo_slides_stage_clear(view)`: drops both stages and empties the `AnimCaches` (M1; the engine calls it when a font loads or a picture
  size arrives, since the stage lists were laid out with the old answers).
- JS: `deck.stage(view, src, at, which, ...)` (`js/slides.mjs`), `view.clearStage()`; `SlidesDocument.paintStage(ctx, src, at, which, scale,
  ox, oy, host, { media, clock, mediaFrame })` (`js/slides-engine.ts`: the measure loop, decode, the painter with the frame and its
  replacements; the `mediaFrame(shape)` hook reaches the painter's env only during that paint).

**Viewer** (`slides/packages/viewer/src`). With `engine="rust"`, `present()` creates a Rust show (`RustSlides.newShow`, the same options
and the same `now`) next to the TS `SlideShow`, and every presenter command and clock update is applied to both with the same `now`; `play()`
creates a Rust timeline with the same options. `paintPlaybackSlide` paints through `paintStageRust` (the stage of the mirrored handle at the
same instant) and falls back to the TS painter on any non-zero status or a trap (`us-engine-error`, as the still path). The TS state machines
stay the source of the events (`us-slide`, `us-step`, `dueAt`/`wakeAt` timers) and of the media player, which needs the TS `PlaybackSlide`:
each frame asks `media.frame(...)` once per media shape (its side effects drive the `<video>` elements, as the TS `mediaFrame` callback
did) and hands the results to the painter's `MediaFrame` items. The two state machines are the B6 parity pair (`sh.*` facets equal on
every script); the Chromium harness compares the frames they produce. `disposeRust` frees the mirrors.

**Numbers.** Node (`tests/slides_anim.test.mjs`, the frozen TS frames of `uo-pptx/js/capture-anim.mjs`): `anim-b6a` 472/472, `anim-samples`
551/551, `anim-b1a` 150/150 (and locally the full sets 1,015/1,015 and 1,462/1,462). Chromium (`slides_switch.mjs --mode anim`): 121 animated
decks, **5,089 frames pixel-equal of 5,089** (`play` sampled, `present` stepped), 5,254 stage paints by the Rust list; the CI form (`--mode all`,
100 `b1a` decks) 181 decks, 7,221/7,221, with the still parity 959/959 slides and phase 0, memory and faults green; memory: 300 playback
frames, module live bytes 251,720 at every sample (spread 0), 0 handles after the element left. The static modes are unchanged (the still path
emits no slots). Size of the shipped build (`--features slides`, `--profile wasm`): **3,453,481 B raw, 1,235,854 B gzip -9** (77 % of the
1.6 MB budget; the row of "Sizes" above, 1,045,866 B gzip, was measured at C-6; the growth to this tree was not broken down by crate here);
`wasm-tools validate` with the Safari 15 feature set passes.

**What stays TS** (gaps 3-7 above still held at B4b): links, the accessible text and the media player; the TS model stays loaded.
All three moved to Rust in wave 4 (next section).

### Wave 4 — links, the read model (the viewer without the TS model), `MetaDecode` (2026-11-17, Opus)

Rust side of the links: `uo-dml-render.md` "Links (wave 4)" and `uo-pdf.md` "Slides PDF" (`Emission::links`, `uo_pptx::links::slide_links`,
the PDF annotations). Here the binding and the viewer.

**Exports** (`src/slides.rs`, layouts in its module documentation, "Read model"): `uo_slides_outline` (the accessible outline of a
slide: per shape the flags, placeholder type, alternative text, paragraphs and table cells), `uo_slides_links` (the link boxes:
`RunUrl` = the TS viewer's hit-test geometry, plus the shape's own `hlinkClick` and run-level slide/show jumps), `uo_slides_assets`
(the pictures to decode, each part once in the TS `warmSlideImages` order, the metafile/PICT/TIFF ones flagged; the media and GIF shapes
with the slide timing model's media entry), `uo_slides_media_show` / `uo_slides_media_timeline` (the `mediaStateAt` of every media shape
of a stage at the instant of the show or the timeline), `uo_slides_fonts`, `uo_slides_typefaces`, `uo_slides_show_settings`;
`uo_slides_view_is_recipe` (see `MetaDecode` below). For the editor routing (another frontier, requested through the orchestrator):
`UoSlidesDeck::{deck_mut, refresh}`, `uo_slides_emit_suppress` (`uo_pptx::emit::emit_slide_suppressed`: the inline-edit text left
out), and in the JS `SlidesModule.exports`, `SlidesDocument.{deckPtr, refresh()}`, `SlidePaintOptions.suppressText`,
`RustSlides.adopt`, the viewer's `adoptRust`/`rustSession`.

**JS** (`js/slides.mjs`, `js/slides-engine.ts`): `SlidesDocument.{outline, links, assets, mediaStates, fonts, typefaces, showSettings}`
(outline/assets cached per slide, links for the last 64 views; `refresh` drops them), `currentSlidesModule` + `SlidesModule.openSync`
(a `.pptx` opened synchronously once the module is instantiated), `PENDING`, `SlidesPaintHost.prepared`, `preparing`/`prepared`
counters. `rust-engine.ts`: `preloadSlidesEngine`, `slidesEngineReady`, `RustSlides.openSync`.

**Viewer** (`universal-slides-viewer.ts`). With `engine="rust"` the element instantiates the module when it connects (and `src` waits for
it), so `loadPptx` opens the deck synchronously in Rust only: `this.pkg`, `this.pres` and `this.slides` stay empty and the viewer reads
`slideCount()`, `slideSize()`, the read model and the Rust state machines instead. The accessible text (`a11y.ts` now takes an
`A11yShape` outline, built from the TS model by `a11yShapesOf` in TS mode), the hit-tests (`hitTestLink`: run address first, as the TS,
then the shape's click and the jumps, which navigate: `selectSlide` / the show's `goTo`), the pictures (`warmPart`; metafiles, PICT and
TIFF are not decoded by the shell: the Rust recipes paint them), the GIFs and media elements (`MediaPlayer.mediaFrame`/`gifFrame`/
`warmGif` over the Rust states), the embedded fonts and families, the show settings, `present()` (`RustShowMachine`: the Rust show is THE
state machine, its actions and frames drive the events and the stage) and `play()` (the Rust timeline; the returned `PlaybackTimeline`
holds the slides' times — index, start, end, animation start, transition — not the TS placed effects: an API difference of rust mode).
The media player is asked when the painter reaches a shape's media frame (as the TS painter calls its hook): the B4b hybrid asked it
for every media shape of every frame, which created elements and positions the TS never asked for (fixed in both paths).
A `loadPptx` before the module is ready (or a `.ppt`, whose converter is asynchronous) opens in TS and in Rust behind it, and the TS model
is dropped when Rust opens (`dropTsModel`, unless a show or playback is running). The TS model is built again only when the module
fails (a trap: `healRust`/`ensureTsModel`, counted in `tsModelLoads`) or the attribute goes back to `ts`; a Rust-only show or playback
cannot fall back mid-frame (it keeps the frame and tells `us-engine-error`).

**`MetaDecode`.** A compressed EMF+ bitmap the layout needs is decoded by the engine (`createImageBitmap` of the bytes, keyed by
`bitmapKey`, the same key in Rust and TS); while it decodes the paint returns `PENDING` (13) and paints nothing, and the decode's end calls
`SlidesPaintHost.prepared` (the viewer repaints and redraws the thumbnails); the answer is then the bitmap's size (NaN for one the browser
cannot decode, as the TS skips it) and the recipe's `decode` hook gets the bitmap. Declared: during the decode the Rust paint shows the
slide without anything (the TS paints the rest of the slide and the picture appears later); the answer goes through `uo_slides_answer`,
not a separate `set_meta_decoded` export. **Recipes**: `uo_slides_view_is_recipe` exposes `SlidesResources::is_recipe`, and the engine's
painter env now has `recipe` — before this wave the engine had no `recipe` hook, so every metafile, PICT and TIFF picture was dropped in
the browser (the op-stream gates replay recipes with their own env and the Chromium decks had no visible metafile).

**Numbers.**
- Read model against the frozen TS model (`tests/slides_readmodel.test.mjs`, Node, ts-ref-092 dist): 273 decks (samples incl. `.ppt` through
  the ppt module, b1a/b5b/b6a/c1d 60 each), 1,043 slides, 6,716 shapes outlined, 187 link boxes, 1,529 pictures (243 rasterized by Rust),
  72 media/GIF shapes, 1,171 typefaces, the show settings: **0 differences**; media states: 6,396 stage frames (real time and the 30 fps
  export clock), **2,616 media-shape states equal**; no embedded fonts in that corpus (the font query is covered by the same code as the
  TS `readEmbeddedFonts` port, B1).
- Chromium (`slides_switch.mjs`, tolerance 0; 1248 headless): `--mode parity` over the samples + 10 b1a + 5 c1d: 68 decks **all opened
  without the TS model (68/68, 0 TS model loads)**, 633/633 slides pixel-equal, main canvas 291/291, thumbnails and notes 0 different,
  **a11y text 633/633 slides equal**, **link hit-tests 607,680 points (1,757 on a TS link) 0 different**, 0 Rust-only jump/click hits (no
  shape-level links in the corpus). The CI form (`UO_SS_DECKS=100 --mode all`): **213 decks, 213/213 TS-free, 959/959 slides
  pixel-equal, a11y 959/959, link hit-tests 920,640 points (19,698 on a TS link) 0 different, animated 7,221/7,221 frames over 181
  decks**, memory, faults and phase 0 green, 198 s + 282 s. `--mode anim` (first run, 91 animated decks, all TS-free): **4,059/4,059
  frames pixel-equal**, 4,291 stage paints by Rust, **media requests: 89 frames with requests, 0 different** (video position, play/pause, volume, mute, full screen, GIF frame);
  playback memory flat (251,752 B every sample, 0 handles left). `faults`: corrupt decks throw alike in both engines (the Rust element
  tries Rust, then TS), a trap under a TS-free deck: `us-engine-error`, the TS model is built and paints, the next load is TS-free again;
  link scheme filter unchanged. `memory`: 200 changes + exports flat (1,149,406 B every sample). `phase0` unchanged.
- **Open cost** (`--mode opencost`, the 12 largest decks of the set, a fresh page per deck and engine, heap after `gc()`): JS heap after open
  TS **31.15 MB** vs Rust **12.78 MB + 14.35 MB module live bytes = 27.1 MB**; before wave 4 the Rust element held the TS model and the module:
  31.15 + 14.35 = **45.5 MB** (−40%). `loadPptx` TS 860 ms vs Rust 568 ms (−34%), first frame 880 vs 590 ms.
- b5b metafile decks in Chromium (`UO_SS_B5B=40`): at the end of wave 4a 23 of their 81 slides differed inside metafile rasters
  (659/682 over that run); before the `recipe` hook those pictures were missing (80 slides differed). **Closed in wave 4b, 81/81**: see
  "Wave 4b — the b5b metafile pixels" below.
- Size: `uo-slides.wasm` (`--features slides`, `--profile wasm`) 3,537,657 B raw, **1,265,754 B gzip -9** (79% of 1.6 MB; +29,900 B over
  B4b: links, the read model, the recipe flag, the suppressed emission).

**What still needs TS** (counted): nothing in the open, still, a11y, link, media, show or playback paths of a `.pptx` once the module is
instantiated. The TS model is still built (1) for a `loadPptx` that comes before the module is ready, or for a `.ppt` (the converter is
asynchronous): until the Rust open finishes, then dropped; (2) after a module trap; (3) when the attribute goes back to `ts`. The TS code
(not the model) that the rust path still uses: `paintTransition`, `mediaFadeAlpha`/`gifFrameAt` (pure functions), the light tables and
pattern rows of `@universal-office/drawingml`.

### Wave 4b — the b5b metafile pixels (2026-11-18, Opus)

The 23 b5b slides that differed in Chromium (`slides_switch.mjs`, tolerance 0; the native op streams were already 1,267/1,267 and
162/162 equal, so both causes were in the browser shell). Classified from the debug PNGs (`UO_SS_DEBUG=1` now writes every differing
slide, `parity-diff-<deck>-s<slide>-{ts,rust}.png`) and the paint statuses (the harness prints the non-zero ones):

| Cause | Slides | Fix |
|---|---|---|
| **Canvas kind.** The recipe was replayed on an `OffscreenCanvas` (`scratchCanvas`), the TS rasterized on a `<canvas>` element (`warmImage`'s `document.createElement('canvas')`): Chromium anti-aliases the two differently, so path and text edges inside the picture differed (1 to 1,738 px a slide, deltas up to 179) | 20 | `paint-slides.ts`: the replay's default canvas is an element canvas (`elementCanvas`; no document → the scratch canvas). `metaCanvas`/`createCanvas` of the env still win (the op-stream gates give their recorder) |
| **Prepare phase too late.** An EMF+ picture with a compressed PNG/JPEG asked for its decode only when its slide was first painted (`MetaDecode` → `PENDING`, slide not painted); `renderSlideToCanvas` is synchronous, so a slide other than the first was blank (921,600 px; 2,184,960 on a portrait deck). The TS decodes every such picture when the deck opens (`warmImage` → `prepareSpecialImage`) | 3 | new export `uo_slides_meta_bitmaps(deck, part)` (`uo_dml_render::metafile::compressed_bitmaps`: the TS `scanEmfPlus(b).compressed`, nested metafiles included) → `SlidesDeck.metaBitmaps`, `SlidesDocument.prepareSpecial(part, host)`; `RustSlides` calls it for every special picture of every slide on `openSync`/`open`/`adopt`, so the decodes start at open and `settle`'s `preparing` wait covers them. Test: `the_prepare_list_is_what_the_rasterization_asks_for` (uo-dml-render `tests/metafile.rs`: the list equals the `Need::Decode` set of a rasterization for all 873 inputs) |
| Colour effects over a metafile picture (B5b-G1) | 0 | not a cause: 0 of the 313 metafile/PICT/TIFF pictures in the 698 committed and generated decks carry a colour effect (`a:grayscl`, `a:duotone`, `a:clrChange`, `a:lum`, `a:biLevel`…). Still open as a declared gap, see `uo-dml-render.md` B5b-1 |

Result: b5b **81/81 slides pixel-equal**, main canvas 81/81, thumbnails 0 different (focused run `UO_SS_B5B=40 UO_SS_ONLY=decks.b5b.
--mode parity`, 16 s; the 4 compressed bitmaps of the set are undecodable in both engines, the GDI fallback is drawn). The b5b decks are now
ON by default (`UO_SS_B5B` default 40), so the CI form (`UO_SS_DECKS=100 --mode all`, unchanged command) covers them: **253 decks, 1,040/1,040
slides pixel-equal**, 253/253 TS-free, main canvas 698/698, a11y 1,040/1,040, link hit-tests 998,400 points (20,067 on a TS link) 0 different,
animated 7,221/7,221, memory/faults/phase 0 green; parity 250 s, anim 375 s, 11 min wall in all (measured with two cargo builds running
alongside; the b5b decks add ≈ 16–20 s to the parity phase). Native gates unchanged: `compare-meta` 1,267/1,267, `drawops-b5b` 162/162,
static draw-ops samples 1,202 / b1a 246 / c1d 182, animated anim-b6a 472 / anim-samples 551 / anim-b1a 150, `slides_api` 8/8.
`UO_SS_ONLY=<regex>` (a focused run over the deck paths; under Git Bash do not start it with `/`, MSYS rewrites it as a path) is new.

### Serve mode (real Safari) (2026-11-18)

`slides_switch.mjs --mode serve` is the M4 gate's harness for real Safari (macOS and iOS on BrowserStack). It is modelled on the Write
viewer's serve mode ("Real Safari 15 / iOS (BrowserStack, 2026-11-10)"). No browser is launched. The server listens on `127.0.0.1:UO_SS_PORT`
(8767), serves the same bundle (the Rust engine with `tsStrict: true`), and sets `application/wasm`, `no-store` and no COOP/COEP (cross-origin
isolation is not needed). Only git-tracked decks and open fonts are served. BrowserStack cannot reach localhost, so expose the port with a
temporary tunnel and delete it after. `/` runs the following by itself, with no Playwright on the page side:

- the parity `compare` on every `UO_SS_STRIDE`-th (5) git-tracked deck of the parity set: 39 decks and 144 slides by default (11 samples, 8 b1a,
  4 c1d, 8 b5b, 8 b6a). Per slide it compares `renderSlideToCanvas` at 1280 px, and for the first 6 slides the main canvas and the
  thumbnail. It also compares the notes, the a11y text and the link hit-tests on a 40 × 24 grid. Tolerance is 0, at the device's own dpr,
  which is recorded in every row.
- `animate` on `UO_SS_ANIM` (6) animated decks of the stride (playback samples, b6a and b1a in turn), 8 steps each. This needs no Playwright
  clock, because the anim mode's virtual clock is the element's own `el.now`. Each deck gives `play({ media: 'seek' })` sampled over the
  timeline, then `present()` + `next()` at +0/180/420/900 ms, plus the media requests of every frame.
- a soak of `UO_SS_SOAK` (200) slide changes and exports of the largest .pptx of the stride. It uses the count build and samples the
  module's live bytes, the wasm buffer and the handles every 20 changes. Pass = spread ≤ 4 MiB and 0 handles after the element left.
- the CSP probe: `/csp?v=strict|wasm|eval`, then `/done`.

Every result is POSTed to `/result`, appended to `<CARGO_TARGET_DIR>/slides-viewer/serve-results.jsonl`, and the server prints one line per
deck. Record kinds:

- `start`: the features (wasm SIMD, `OffscreenCanvas`, `createImageBitmap`, the canvas `filter`/`roundRect`/`letterSpacing`/`fontKerning`/
  `reset`, `FinalizationRegistry`, dpr, viewport, time zone, UA).
- `row` (one per deck): the full `compare` result plus `ok`, `known`, `ms` and `dpr`. The first a11y difference is in `a11yFirst`.
- `anim`, `soak`, `step` (the driver's `window.__step`) and `crash`.
- `end`: `ok`, `verdict`, `summary` and the totals.
- `csp`.

Every page has a `#status` element that WebDriver can read. It shows the progress, then `finished <verdict>: …` on `/`, and
`done <verdict>: <totals>; csp strict=… wasm=… eval=…` on `/done`. The `/done` totals are the server's last `end` record for that user agent.

Like the Write page, this page does three things to stay stable on Safari:

- It fixes `Date` to `NOW`.
- It releases each comparison canvas after reading it.
- It resumes after a tab crash (OS memory on iOS). The deck and the step that were running are recorded, and the totals are kept in
  `sessionStorage`.

`/?solo=ts|rust&decks=k3.pptx,…` opens one viewer of one engine per deck, shows and renders every slide, and closes. It does not compare;
use it to triage a crash. The query switches for triage are:

- `?decks=` compares only the listed decks.
- `?debug=1` POSTs the PNGs of the differing slides; the server writes them next to the results.
- `?nooffscreen=1` hides `OffscreenCanvas` before the bundle loads.
- `?nocsp=1` stays on `/`.
- `?fresh=1` starts a new run: it drops the saved state of a previous run, then leaves the address, so a reload after a tab crash
  still resumes this run. The saved state also resets when the run's URL changes. Use it for every new navigation of the same tab:
  without it, a second navigation mid-run is taken for a crash.
- `?fontdelay=<ms>`: every font file answers that much later. The viewers use the font base `/fonts/d<ms>/`, which the server reads.
- `?fontlag=<ms>`: a face added to `document.fonts` reaches canvas text only that much later (`FontFaceSet.prototype.add` is
  deferred). This emulates what real Safari does.
- `?tsts=1` is a control: both elements use the TS engine.
- Every `start` record carries `readback`, a probe of whether the browser alters canvas readbacks. It reads the same drawing twice,
  reads a twin canvas drawn by the same code, and reads a `drawImage` copy. Safari's fingerprinting protection adds such noise.

Two things found while building it locally:

- **TS metafile player without `OffscreenCanvas` (an expected difference, not an engine finding).** `metafile.ts`, `emfplus.ts` and `pict.ts`
  draw embedded bitmaps and pattern brushes only through `OffscreenCanvas`, and skip them when it is missing. The Rust path falls back to a
  DOM canvas and draws them. Playwright WebKit (Windows) has no `OffscreenCanvas`, and neither does Safari before 16.4. There, 8 slides of 7
  of the 8 b5b decks differ, in pixels only. The same 8 slides differ in Chromium with `?nooffscreen=1`, so the cause is confirmed. The page
  flags such rows `known: 'ts-metafile-needs-offscreencanvas'`. A row qualifies only if it is a b5b deck, `OffscreenCanvas` is missing, and
  nothing but pixels differs. The verdict is then `PASS-EXCEPT-KNOWN` rather than `PASS`.
- **The live region races on slow devices.** `LiveRegion.announce` sets its text 60 ms after each `selectSlide`. Under WebKit at dpr 3 the
  a11y text was read when one element's timer had fired and the other's had not (5 of 144 slides). The serve page waits 80 ms before
  reading; timers fire in deadline order, so both announcements are in place. The default modes are unchanged.

Local results: Playwright 1.61, the count build `count,slides` (3,651,423 B) + `slides-ppt` (889,120 B), stride 5. Each run took 50–105 s.

| Browser (local stand-in) | dpr | Decks / slides | Differences | Anim (6 decks) | Soak | CSP `'self'` / `+wasm-unsafe-eval` / `+unsafe-eval` |
|---|---|---|---|---|---|---|
| Chromium headless shell 159 | 1 | 39 / 144 | **0** (main 0/107, thumbs 0, a11y 0/144, links 0/138,240) | 178 frames, 0 | spread 10,746 B, 0 handles | refused / compiles / compiles |
| Chromium, `?nooffscreen=1` | 1 | 39 / 144 | 8 slides, all `known` (b5b) | 178 frames, 0 | ok | — |
| WebKit 26.5 (Windows) | 1 | 39 / 144 | 8 slides, all `known` (b5b); a11y 0, links 0 | 178 frames, 0 | spread 4,928 B, 0 handles | refused / compiles / compiles |
| WebKit 26.5 (Windows) | 3 | 39 / 144 | 8 slides, all `known` (b5b); a11y 0, links 0 | 178 frames, 0 | spread 4,928 B, 0 handles | refused / compiles / compiles |

A forced reload in the middle of the run (standing in for iOS killing the tab) was recorded as a `crash` with
`at: "c:11"`, `step: "k11.pptx load rust"`. The page then resumed at deck 12 with the totals carried over and gave the verdict FAIL with
`crashes 1`.

#### First real-Safari run (BrowserStack, 2026-11-18) and its fixes

Two engine bugs, both fixed:

1. **Safari 15.6 (macOS) and iOS 15.5 never started.** The bundle held a regex lookbehind, which Safari before 16.4 rejects when it
   parses the script, so the whole viewer entry failed for TS and Rust alike. The published 0.9.2 has it too. The source was the
   chart word wrap, `shared/charts/src/render.ts` (`para.split(/(\s+|(?<=-))/)`). It now reads
   `para.split(/(\s+)/).flatMap((p, i) => (i % 2 ? [p] : p.match(/[^-]*-|[^-]+$/g) ?? []))`.
   - A property test compares old and new over 300,019 strings: lengths 0–13, hyphens, every `\s` kind (NBSP, em/ideographic
     space, U+2028, BOM), NUL and lone surrogates. 0 differ.
   - The charts vitest passes, 1,786/1,786.
   - The spell checker's address filter (`shared/spell/src/tokenize.ts`, `(?:^|(?<=\s))…` for `/usr/bin`, `./a/b`) had a lookbehind
     too, inside a `RegExp` built when the module loads. It now consumes the space, which `internetRanges` skips. A property test
     over 300,012 strings (117,767 with an address) finds 0 different ranges, and the spell vitest passes, 47/47.
   - The served bundle (entry and chunks) was scanned with the TypeScript parser for syntax Safari 15.0–16.3 cannot parse:
     lookbehind, the `v` flag, modifiers and duplicate group names in regex literals and `RegExp` strings, class static blocks,
     hashbang, `using`, import attributes. After the fix: 0. The 7 `RegExp` calls with built sources were checked by hand.
   - Elsewhere in the repository, not in the Slides bundle:
     - Lookbehinds in `RegExp` strings built at run time: `write/packages/editor/src/find-engine.ts` (find with whole words or
       the `<`/`>` wildcards) and `sheet/packages/calc/src/functions/regex.ts` (`\A`, `\b`, `\B` of the REGEX functions). Those
       features would throw on Safari < 16.4, but the products still load.
     - Class static blocks in `@universal-material/web` (ripple), seen in the Write editor's demo bundle, and in `pdfjs-dist`
       (lazy PDF chunk). esbuild lowers these with a `safari15` target; it cannot lower a lookbehind.
2. **Safari 26.4 (macOS) and iOS 26.5: a few slides differed, different ones on each run.**
   - What went wrong: text was laid out with the widths of the fallback face and drawn with the real one. On b1a/0002 slide 1
     this was Courier New → Liberation Mono.
   - Why: the Rust engine keeps its measurements, while the TS measures at every paint. A face the resolver adds after a slow
     fetch, or a bold/italic face fetched on first use, gets `addFace` → `emit` → `fontsChanged`, and the engine re-measures in
     that same task. Safari applies a face added to `document.fonts` to canvas text only at a later rendering update, and fires
     no event then (`loadingdone` does not fire for a face loaded before it was added). So the engine kept the fallback
     widths. `fontsReady()` does not wait for the resolver's fetch either.
   - Reproduced locally by emulating both conditions: `?fontdelay=300&fontlag=20` on the b1a decks k11–k18. Before the fix:
     Chromium 3 slides and WebKit 4 slides differ in `renderSlideToCanvas`, with thumbnails following; other delays give 0–2.
     The result changes with the delay, as it does on real Safari.
   - The fix:
     - `RustSlides` (`slides/packages/viewer/src/rust-engine.ts`) measures again at every still paint, and at every stage paint
       with `which` 0, for 1 s after a font change (`fontSettleUntil`). It drops its measuring context on a font change, and
       measures on a DOM canvas as the TS does (before: an `OffscreenCanvas` when one exists).
     - The viewer (`universal-slides-viewer.ts` `remeasureLater`) asks again and repaints two frames later and at 1.1 s, for
       both engines: the TS main canvas painted before the face took effect was stale too.
   - After the fix, 0 differences in 10 of 10 runs: 5 delay/lag combinations (300/20, 800/20 twice, 500/50, 1500/20) in
     Chromium and WebKit.
   - The serve compare now reads the main canvases only after both elements' pending font repaints have run (bounded at 1.5 s;
     nothing is waited when none is pending).

The tiny differences of that run (1–6 pixels, 31 on one slide, on different slides each run) may be Safari's canvas readback noise
(fingerprinting protection) rather than an engine difference. The page now records the `readback` probe; locally it gives
repeat/twin/copy 0/0/0 in Chromium and WebKit. `?tsts=1` (TS against TS) tells the two apart on the device.

Local results after the fixes (full set, stride 5):

| Browser | Query | Result |
|---|---|---|
| Chromium | (none) | **PASS**: 39 decks, 144 slides, 0 differ; anim 178 frames 0; soak 0 handles |
| Chromium | `fontdelay=300&fontlag=20` | **PASS**: 0 differ |
| WebKit (Windows) | (none) | PASS-EXCEPT-KNOWN: only the 8 b5b slides (TS metafile player without `OffscreenCanvas`) |
| WebKit (Windows) | `fontdelay=300&fontlag=20` | PASS-EXCEPT-KNOWN: same |
| Both | `tsts=1` | 0 pixel differences; the verdict is FAIL by construction, since nothing runs in Rust |

The same-tab test: a second navigation with `?fresh=1` started over with no crash, and a reload resumed with one `crash` record.

## Slides .NET facade (2026-11-17, Sonnet)

`UniversalOffice.Pptx` as a C# facade over the C ABI of `uo-ffi`, the way `UniversalOffice.Docx` is. Rust: `core/crates/uo-ffi/src/slides.rs`
(over `uo_pptx`: `Deck`, `ppt::convert`, `pdf`); C#: `core/dotnet/UniversalOffice.Pptx` (+ `.Tests`, `.Smoke`); tests
`core/crates/uo-ffi/tests/slides_abi.rs`, digests `core/crates/uo-ffi/tests/parity/slides-native.tsv`.

### Exports (all follow section 3: status code, per-thread last error, `UoBuf` out, sized option structs, `UoCancel`)

| Export | What |
|---|---|
| `uo_deck_open(data, len, const UoDeckOptions*, UoDeck**)` | `.pptx`, or a PowerPoint 97-2003 `.ppt` (Compound File signature) converted to the parts of a `.pptx`; `UoDeckOptions { size, flags }`, `DECK_TRUSTED = 1`, `DECK_TS_STRICT = 2` (wave 5) |
| `uo_deck_free(UoDeck*)`, `uo_deck_is_legacy(const UoDeck*)` | release (null is a no-op); 1/0, -1 for a null handle |
| `uo_deck_info(deck, u32* count, f64* width_emu, f64* height_emu)` | slide count and `<p:sldSz>` in EMU |
| `uo_deck_slide_notes(deck, index, UoBuf**)` | the notes text, UTF-8, paragraphs joined by `'\n'`; index past the end is code 9 |
| `uo_deck_save(deck, UoBuf**)` | `.pptx` bytes (a legacy `.ppt` comes out converted) |
| `uo_deck_to_pdf(deck, const UoConverter*, const UoSlidesJob*, const UoCancel*, UoBuf**)` | the PDF of an open deck; `UoSlidesJob { size, flags, reserved }`, `SLIDES_JOB_COMPRESS = 1`, `SLIDES_JOB_TS_STRICT = 2` (wave 5) |
| `uo_pptx_to_pdf(conv, data, len, job, cancel, UoBuf**)` | one call: open + PDF (the server path) |
| `uo_ppt_to_pptx(data, len, const UoDeckOptions*, UoBuf**)` | `.ppt` -> `.pptx` bytes; anything without the CFB signature is `INVALID_DATA` |

- The `UoConverter` is the Docx one (`uo_converter_new`: machine fonts + `font_dirs`, `CONVERTER_TRUSTED`): one font index per host for Word and
  Slides. The PDF font policy is `server-substitute` over that index: the family the deck asks for, else its metric-compatible substitute
  (Calibri to Carlito...), else Liberation Sans (`ServerFaces` in `slides.rs`, a `uo_pptx::pdf::FontSource` over `SubstituteFontProvider`; each
  variant is parsed once per PDF).
- **Trust** has two sides: the *deck's* flag picks the container limits of the read (untrusted: 256 MiB per inflated part and 512 MiB in all, the
  Docx server path's caps); the *converter's* trust picks the PDF's picture cap (untrusted: 40 Mpx decoded, as the Docx server path).
- **Threads**: every export that touches a deck runs on its own thread of `CONVERSION_STACK_BYTES` (group nesting and the text layout recurse; an
  overflow aborts the process). `UoDeck` holds the `Deck` behind a `Mutex`: any thread may call, one at a time; a panic inside a call poisons the
  handle (`INVALID_OPERATION` after it).
- **Error codes**: not a package / no presentation / missing part / `.ppt` refused (encrypted, PowerPoint 95, damaged) = 3; over-limit deck read
  (`PptxError::Limit`, `ReadError::Limit`) = 5; malformed XML in a part = 4; slide index out of range = 9; a PDF limit = 5; the sink = 13. A
  container limit of `uo_opc` (an inflation bomb) is reported by `uo_pptx` as `NotAPackage` today, so it arrives as 3, not 5 (the test accepts both):
  `package::load` maps every `uo_opc` error to `NotAPackage`; a typed variant there would give the exact class. The `.ppt` messages are the
  converter's (Portuguese, as the TS: they were written for the end user).
- **Cancellation: partial.** `uo_pptx::pdf::PdfOptions` carries no cancel hook (it is the PDF agent's file), so the flag is read **before the work and
  again before the bytes are handed over** (a flag raised in between gives `CANCELLED` after the PDF is built, nothing is interrupted). What it
  needs to be prompt: `PdfOptions.cancel: Option<Arc<AtomicBool>>`, checked per slide in `uo_pptx::pdf` and handed to `uo-pdf` as the Write PDF does
  (`PdfOptions.cancel` of `uo-pdf`), plus the `EmitFail`/measure rounds of one slide; then `Converter::deck_to_pdf` passes its `Arc`.
  The C# `ToPdf(..., CancellationToken)` is already wired to the native flag.

### C# API (`namespace UniversalOffice.Pptx`, `UniversalOffice.Export`)

```csharp
public sealed class SlidesPdfConverter : IDisposable {            // one per host, thread-safe
    public static SlidesPdfConverter Shared { get; }               // machine fonts + substitutes, untrusted
    public SlidesPdfConverter(SlidesPdfConverterOptions o);        // SystemFonts, FontDirectories, TrustedInput
    public IReadOnlyList<string> Families { get; }
    public byte[] ToPdf(byte[] deck, SlidesPdfOptions? opts = null, CancellationToken ct = default);   // .pptx or .ppt
    public byte[] ToPdf(PptxDeck deck, SlidesPdfOptions? opts = null, CancellationToken ct = default);
}
public sealed class SlidesPdfOptions { bool? Compress; }
public sealed class PptxDeck : IDisposable {                       // an open deck held by the Rust core
    public static PptxDeck Open(byte[] bytes, bool trusted = false);
    public int SlideCount; public double SlideWidthEmu, SlideHeightEmu; public bool IsLegacyPpt;
    public string GetSlideNotes(int index);  public byte[] Save();
    public byte[] ToPdf(SlidesPdfConverter c, SlidesPdfOptions? opts = null, CancellationToken ct = default);
}
public static class PptConverter { public static byte[] ToPptx(byte[] ppt, bool trusted = false); }
namespace UniversalOffice.Export; public static class PptxPdf { ToPdf(byte[], SlidesPdfOptions?); ToPdf(byte[], SlidesPdfConverter, SlidesPdfOptions?, CancellationToken) }
```

Exceptions are those of the Docx facade (`NativeMethods.ErrorOf`): 1 -> `ArgumentException`, 3/4 -> `InvalidDataException`, 5 ->
`OfficeLimitExceededException`, 10 -> `OperationCanceledException`, the rest `UniversalOfficeException` (`Code`, `Stage`); a slide index outside the
deck is `ArgumentOutOfRangeException` before the native call.

**Packaging decision.** `UniversalOffice.Pptx` has a `ProjectReference` to `UniversalOffice.Docx` (so a NuGet dependency) and an
`InternalsVisibleTo` from it: the `SafeHandle`s, `ErrorOf`, `NativeCore` and the exception types are the Docx assembly's, the one native library
per RID travels in the Docx package, and a host using both gets ONE `UniversalOfficeException`, not two. The cost is that a Slides-only host pulls
the Docx assembly (about 100 KB of managed code). The alternative, a `UniversalOffice.Native` package that both facades depend on (the empty
`core/dotnet/UniversalOffice.Native` folders are the stale start of it), is the cleaner shape when a third facade (Xlsx) arrives; it moves files, not
behaviour. `UniversalOffice.Xlsx` will follow this pattern.

### Relation to the old C#-only port (`dotnet/src/UniversalOffice.Pptx`, namespace `UniversalOffice.Pptx` / `UniversalOffice.Export`)

The plan says the facade replaces it; **nothing is deleted in this wave** (the old project stays the 0.9 reference and what `dotnet/UniversalOffice.sln`
builds). The switch is a later step: the facade takes the NuGet id `UniversalOffice.Pptx` at 0.10, the old project leaves the solution. Mapping of the
old public surface:

| Old (0.9, pure .NET) | 0.10 facade | |
|---|---|---|
| `PptxPdf.ToPdf(byte[] bytes, SlidesPdfOptions? opts)` (`UniversalOffice.Export`) | `PptxPdf.ToPdf(byte[], SlidesPdfOptions?)` and `ToPdf(byte[], SlidesPdfConverter, SlidesPdfOptions?, CancellationToken)` | **kept** (source compatible when no fonts are set) |
| `SlidesPdfOptions.Fonts` (`List<PdfFontSource>`, mandatory: the generator had no base-14) | `SlidesPdfOptions.Compress`; fonts come from the converter | **dropped**: fonts follow the `server-substitute` policy (`SlidesPdfConverterOptions.FontDirectories` for custom folders); `PdfFontSource` is gone with `UniversalOffice.Pdf` |
| `SlidesModelOps.LoadDeck` -> `Deck { Pres, Slides, Pkg }`; `Presentation.SlideW/SlideH/Slides.Count` | `PptxDeck.Open`, `SlideWidthEmu`, `SlideHeightEmu`, `SlideCount` | **mapped** (size and count only) |
| `SlidesModelOps.ReadNotes(pkg, slidePath)` | `PptxDeck.GetSlideNotes(index)` | **mapped** (by index, one string with `\n`, not `List<string>`) |
| `PptxPackage.Load` / `PresentationXml()` / `OpcPackage` parts | - | **dropped** (no package access: the deck lives in the core); `PptxDeck.Save()` gives the `.pptx` bytes |
| `PptxReader.ReadPresentation/ReadSlide`, `Model.cs` (`Slide`, `Shape`, `TextBody`, `Run`, `Paragraph`, `Fill`, `Xfrm`, `Shadow`, `ImageRef`...), `Theme.cs`, `TextStyle.cs`, `Geometry.cs` (`PresetPath`), `TextLayout.cs` (`LayoutTextBody`), `Emu` | - | **dropped**: the read model of the shapes does not cross the ABI (the Rust model is `uo_pptx::Deck`/`SlideModel`; a JSON/wire export of it is the way if a host needs it, as the Docx Editing wire) |
| `PptxManipulate.SetRunText(pkg, slidePath, runIndex, text)` | - | **dropped**: the editing session (`uo_pptx::edit`, B8) has no C ABI yet; the facade for it is the next step of the Slides editing frontier |
| `PptxMarkdown.ToMarkdown`, `PptxHtml.ToHtml` (`Options.Images`, `FullDocument`) | - | **dropped for now**: the `export` feature of `uo-pptx` (Markdown/HTML) is not ported (module record "Not done"); the exports follow when it is |
| (none) | `PptxDeck.IsLegacyPpt`, `PptConverter.ToPptx`, `SlidesPdfConverter.Shared/Families`, `CancellationToken` | **new**: `.ppt` -> `.pptx` and `.ppt` as input of every entry point (the old C# had no `.ppt`) |

Output: the PDFs are the Rust `uo_pptx::pdf` (display list interpreted through `uo-pdf`: real gradients, soft masks, blend modes, patterns, links, the
charts' frames counted as missing), not the old C# strip approximation of gradients, and charts are not grey placeholders where the Rust PDF draws them.
No parity with the 0.9 C# PDF is claimed or measured.

### Parity and tests

| Comparison | Corpus | Result |
|---|---|---|
| C# `ToPdf` (compressed) = Rust native bytes (`tests/parity/slides-native.tsv`, written by `UO_BLESS=1 cargo test -p uo-ffi --test slides_abi`) | 93 decks: the 8 samples + `showcase_animado.ppt`, 4 `legacy/*.ppt`, a third of `playback/` (`.pptx` and `.ppt`) and every 10th deck of `b1a`, `b3a`, `b5a`, `b5b`, `b6a`, `c1d` | 93 of 93 equal (blessed and checked on the same tree) |
| Rust native debug build = release build (digests blessed in debug, checked in release) | the same 93 | equal |
| deck route (`uo_deck_to_pdf`) = one-call route (`uo_pptx_to_pdf`) | the same 93 (Rust) and 2 decks (C#) | equal |

**The digests follow the Slides PDF**: any change of `uo-pptx/src/pdf`, `uo-pdf` or `uo-dml-render` that changes a PDF byte makes
`the_native_digests_of_the_fixture_decks_are_the_committed_ones` fail with the count of differing decks; re-bless with `UO_BLESS=1` once the change is
meant. The C# test reads the same file, so one re-bless fixes both. Tests: `slides_abi.rs` has contract (nulls, short structs, garbage codes and
stage), cancel, deck info/notes/save/re-open, `.ppt` -> `.pptx`, the 93-deck PDF check, hostile input (39 truncations x 2 formats, 30 bit-flip
rounds x 2, CFB header with zeros/ones, a 200,000-deep XML part, and in release a 300 MiB inflation bomb), the thread soak (4 threads, shared deck
handle) and M1, M3, M4.

### Numbers

Release build, one 2026 Windows machine, tree of 2026-11-17 (the Slides PDF is changing under the wave: the digests were re-blessed three times).

- **Rust ABI tests** (`cargo test -p uo-ffi --release --features count --test slides_abi`, 14 tests, 12 s): all pass. Hostile input: 42 truncations/bit
  flips converted and 98 refused with a documented code, 0 crashes; the 200,000-deep XML part is refused (not a stack overflow); the 300 MiB
  inflation bomb is refused (code 3 today, see above).
- **M4 (handles)**: 10,000 x (deck + cancel flag), every 50th deck also read and saved: 1,384,901,000 B allocated, **0 B left** (global counters: a
  deck is built and dropped on worker threads). The C# side: 10,000 decks and 10,000 converters through `Dispose()` and 2,000 abandoned decks through
  the finalizer: native live bytes back exactly.
- **M1 (a PDF leaves nothing)**, global counters, bytes allocated per call, all **0 B left**: ok 3.0 MB (the PDF is 55,446 B), refused 373 B,
  cancelled 256 B, a legacy `.ppt` 6.1 MB, a deck handle opened, converted twice, saved and freed 5.0 MB. C#: 100 conversions + 10 legacy + 10,000
  refused with cancel handles + 100 cancelled calls on a deck: 0 B left.
- **M3 (budgets, Rust API inline)**: `reference.pptx` (39 KB) 2.9 MB allocated, 0.96 MB peak; `stress-media.pptx` (137 KB) 11.4 MB, 2.9 MB peak;
  `stress-table-chart.pptx` (141 KB) 7.3 MB, 1.5 MB peak.
- **Threads**: 4 Rust threads x 4 x 2 conversions (one deck handle shared) give the single-thread bytes; C#: 8 threads x 3 rounds x 3 x 2, native live
  bytes flat after each round.
- **C# tests** (`dotnet test core/dotnet/UniversalOffice.Pptx.Tests`, 16 tests, 20 s): 16 pass, parity 93/93. Smoke: `reference.pptx` (3 slides) to a
  67,743 B PDF in 20 ms after a 379 ms font scan (359 families); `.ppt` to `.pptx` in 7 ms.
- **Existing Docx facade** (`dotnet test core/dotnet/UniversalOffice.Docx.Tests`): 41 of 43 pass; the two that fail are the PDF parity tests
  (`The_uncompressed_pdf_equals_the_ts_render_pdf_byte_for_byte`: 3 of 68 equal; `The_compressed_pdf_equals_the_rust_native_output_byte_for_byte`).
  Reproduced with a native library built WITHOUT this wave's change (Slides module and dependencies removed from `uo-ffi`), while the Rust-side
  `server_parity` passes on the same sources: not caused by the Slides facade; the C# Docx call path differs from the Rust one somewhere that this
  session did not chase (to be looked at by whoever owns the Docx facade; the C# output is a few hundred bytes longer per document).

### Not covered (counted)

1. ~~Cancellation inside the Slides PDF~~ — DONE (orchestrator, 2026-11-17): the flag is passed as `PdfOptions::cancel`,
   read before every slide and every raster effect.
2. No typed limit class for a container (inflation) limit of the deck read: `uo_pptx::package::load` maps every `uo_opc` error to `NotAPackage` (code 3).
3. The editing session (`uo_pptx::edit`, B8: set text, undo/redo, font embedding on save) has no C ABI; old `PptxManipulate.SetRunText` is dropped.
4. The read model (slides, shapes, text) does not cross the ABI; old `PptxReader`/`Model` are dropped.
5. Markdown/HTML export (`PptxMarkdown`, `PptxHtml`): no Rust counterpart yet (`uo-pptx` feature `export` is empty).
6. Slide titles, slide text, hidden-slide flag, and per-slide PDF/page ranges are not exported (only count, size, notes).
7. `UniversalOffice.Xlsx` facade: not started (the same pattern; its Rust side does not exist).
8. CI: the new steps in `.github/workflows/rust-core.yml` (`dotnet` job: the tests and the smoke; `leak-sanitizer-facade`: the Slides tests under LeakSanitizer) have never run;
   `server-pdf` already runs `slides_abi` through `cargo test -p uo-ffi --release --all-features`.
9. No `UniversalOffice.Pptx` NuGet pack was exercised (no native-pack gather yet either, as G14 for Docx).

### PowerPoint rules and the TS switch (wave 5, 2026-11-18)

The Slides PDF now follows PowerPoint where the two were measured to differ (record `uo-pdf.md` "PowerPoint rules of the PDF (wave
5)"): hidden slides are left out, link rectangles follow PowerPoint's rule, characters the face lacks take PowerPoint's per-script
fallback faces, and `.ppt` charts convert natively (agent B, `ConvertOptions`). The facade gets them by default. For the parity tests
against the frozen TS, as `JOB_TS_STRICT` of the Write PDF:

| Native | C# (internal, visible to `UniversalOffice.Pptx.Tests`) | What |
|---|---|---|
| `SLIDES_JOB_TS_STRICT = 2` (`UoSlidesJob.flags`) | `SlidesPdfOptions.TsStrict` | for that PDF only: `deck.set_rules(SlidesRules::ts_strict())` (restored afterwards: the handle keeps PowerPoint's rules), `PdfOptions::ts_hidden_slides`/`ts_link_rects`; on the one-call route a `.ppt` is converted with `ConvertOptions::ts_strict()` too |
| `DECK_TS_STRICT = 2` (`UoDeckOptions.flags`, `uo_deck_open`, `uo_ppt_to_pptx`) | `PptxDeck.Open(bytes, trusted, tsStrict)` | a `.ppt` converted as the TS did (charts as their fallback pictures); `uo_ppt_to_pptx` stays PowerPoint by default |

A deck whose slides are all hidden has no PDF (PowerPoint refuses to save it): `PdfFail::NoVisibleSlides` → `INVALID_OPERATION` (2,
message "every slide is hidden"), a `UniversalOfficeException` in C#; the TS job keeps the slides. `slides-native.tsv` writes
`refused` for such a fixture (`core/crates/uo-pptx/tests/captures/decks/b6a/0000.pptx`; the C# parity test expects the refusal).
Tests: `slides_abi.rs` `the_ts_strict_job_keeps_hidden_slides_for_that_job_only` (the synthetic `pp-rules.pptx`: 3 pages by default,
5 with the TS job, the deck handle back on PowerPoint's rules after it); the fixture test accepts the refusals and checks their TS
PDF; C# `TsStrictTests`.

Digests: the Slides PDF bytes change for every fixture deck with links (rectangles), hidden slides (pages left out), characters the
face lacks under fallback (other faces asked first; with the open fonts of the digest run the faces are the same, so these move only
where the order matters), the `.ppt` decks with charts (agent B), and the all-hidden deck (`refused`): re-bless with `UO_BLESS=1`
after all of wave 5 has landed.

## Slides editor (B8 routing, 2026-11-17, Opus)

`<universal-slides engine="rust">`: the package, every editing method, undo/redo and the save (with fonts) run in the Rust editing
session (`uo_pptx::edit`, B8) inside the SAME wasm instance as the viewer's deck, so the stills, the thumbnails and the stage of
`present()`/`play()` are emitted from the edited package by the viewer's existing Rust path (no second painter, no copy of the deck).

### Design

- **One deck, lent per call.** The deck is the viewer handle `UoSlidesDeck` (opened by `uo_slides_open`). `uo_slides_editor_new(deck)`
  makes a `UoSlidesEditor` holding a *detached* `uo_pptx::edit::Editor` (`Editor::detached`: the font option is read from the deck, the
  session keeps an empty placeholder); every editing export takes the editor AND its deck and runs inside `Editor::with_deck` (two
  `mem::swap`s). A call that changed the package re-reads the deck's slide list (`UoSlidesDeck::refresh`, hook in `slides.rs`); another
  deck answers status 11 (the editor keeps only the deck's address to check it, never dereferences it).
- **The shell's copy of the package.** Every editing answer (`EditResult`) carries the outcome, the UI state the shell mirrors (slide,
  selected shape id, undo/redo flags and labels, dirty, slide count, font option) and the parts that changed since the previous answer
  (`edit::changes_since` against a snapshot the editor handle keeps; `bytes` absent = removed). The element applies them to its TS
  `PptxPackage` and re-reads its TS model from it (only the changed slides when nothing deck-wide changed): the hit tests, the selection
  and handles, the bars' state (`getTextFormat`, `getShapeInfo`), find and the a11y text still read that model (counted below).
- **The Op codec** is schema-generated: `core/schema/slides_edit.uos` (messages `Apply`, `EditResult`, `Blank`, `Clip`, `Query`/`QueryResult`,
  `FontRequests`, `FontFaces`, `SaveResult`; the 29 `Op` variants and their argument types) with the bind map `slides_edit.bind`: `Op`,
  `Outcome`, `Target`, `Frame`, `RunFormat`, `BulletKind`, `ArrangeMode`, `DocProperties`, `ShapeClip`, `BlankDeckOptions` and the font types
  are bound to the Rust types (a new variant or field fails to compile until the schema has it); `TransitionSpec`, `ShowSettingsSpec` and
  `ClipRel` are `manual` binds (`slides_edit/wire.rs`: their Rust fields `type_`/`loop_` are not schema field names). Generated:
  `crates/uo-wasm/src/slides_edit/codec/{generated,generated_support,bind}.rs`, `crates/uo-wasm/js/gen/slides-edit{,.support}.ts`.
  Wire rules: `i64` as `i32` (checked), `u64` (the clip's session id) as `f64` (exact integers below 2^53, checked, no float cast),
  `utf16` strings with lone surrogates as U+FFFD. Decode limits: nesting 32; lists, elements and values up to 1 GiB on both sides
  (`codec::limits()`, `LIMITS` of `slides-editor.mjs`): a message carries whole files (an inserted picture, changed media parts, the saved
  package, font files) and a list cannot claim more elements than its buffer has bytes for.
- **Clipboard**: `copyShape` answers a `Clip` message (XML, relations with the bytes of portable parts, text, source session id, slide
  path); the element exposes it as the public `ShapeClip` with the bytes attached (a symbol), so `pasteShape` sends it back unchanged —
  into this editor (same session: shared parts, the 12 pt offset on the same slide) or any other Rust editor of the page; a clip from a
  `ts` editor is converted (source id 0 = another presentation).
- **Fonts on save**: two phases as in Write: `fontRequests()` (the plan's faces; none when the option is off), the element answers each
  request with the font file by the rule of the TS `planFontEmbedding` (resolver `resolve`, only host / Local Font Access / bundled faces
  have a file, the missing variant through `loadFace`), then `saveWithFonts(faces)` answers the bytes and the `us-font-embedding` report.
- **Inline text overlay**: unchanged in the shell; `commitEditShape` sends `Op::Typing`. The slide under the open overlay is painted by
  Rust with the shape's text suppressed (`uo_slides_emit_suppress`, hook in `slides.rs`/`slides-engine.ts`).
- **Opening**: `loadPptx` keeps the TS read (the viewer's), then opens the Rust session from the same bytes (`openSession`, lazily imported
  `rust-editor.ts`), adopts it into the viewer (`adoptRust`) and repaints. Edits made on the TS engine while the module downloads keep that
  presentation on `ts` (no two histories). Switching the attribute with a document open moves the CURRENT package to the other engine (the
  undo history starts again). A trap kills the instance: the element drops the session (`us-engine-error`) and keeps editing on `ts` from its
  copy of the package, which is the Rust package byte for byte. A DOM move keeps the session and its history (TS behaviour; viewer hook
  `rustAdopted`).
- **One module per page**: the editor build `uo-slides-editor.wasm` contains everything of the viewer's `uo-slides.wasm`;
  `UniversalSlidesElement.configureEngine()` sets the source of both elements. If a viewer of the page loaded `uo-slides.wasm` first, the
  editor finds no editor exports and stays on `ts` (`us-engine-error`, message names the fix).

### Files

- Rust: `crates/uo-wasm/src/slides_edit.rs` (exports, feature `slides-edit` = `slides` + `uo-pptx/edit`), `src/slides_edit/wire.rs`,
  `src/slides_edit/codec/` (mod + generated), `Cargo.toml` (feature; dev-deps `uo-pptx` canon+edit, `uo-dml-render`, `uo-fonts` for the
  replay), `src/lib.rs` (module line); `crates/uo-pptx/src/edit.rs` (`changes_since`), `src/edit/session.rs` (`detached`, `with_deck`).
  Hook in `src/slides.rs` (other frontier, by request): `UoSlidesDeck::{deck_mut, refresh}`, `uo_slides_emit_suppress`; the
  `allow(dead_code)` of the impl became `cfg_attr(not(feature = "slides-edit"), …)`.
- Schema: `core/schema/slides_edit.uos`, `slides_edit.bind`, one line in `manifest.txt`.
- JS: `crates/uo-wasm/js/slides-editor.mjs` + `.d.mts` (binding), `js/slides-editor-engine.ts` (loader over the viewer's `loadSlidesModule`,
  `SlidesEditSession` = the viewer's `SlidesDocument` + the editor, trap rule), `js/slides-editor-engine.check.ts` (drift check, CI tsc line).
- Element (`slides/packages/slides/src`): `rust-editor.ts` (lazily imported; open, blank, changed parts, clip conversion, font answers),
  `slides-editor-engine.d.ts` (ambient module `@universal-office/slides-editor-engine`), `universal-slides.ts` (the `rx*` members and a Rust
  branch in every mutator, undo/redo, dirty, font option, save, typing; `configureEngine` override; `rustReady()`).
- Tests: `crates/uo-pptx/tests/support/edit_capture.rs` (the B8 replay as a `Driver`; `edit_replay.rs` now drives `Editor` through it),
  `crates/uo-wasm/tests/slides_edit_native.rs`, `tests/slides_edit_api.test.mjs`, `tests/browser/slides_edit_switch.mjs`; unit test in
  `src/slides_edit.rs`; fuzz `core/fuzz/src/slides_edit_wire.rs` (+ `fuzz_targets/`, seeds, the codec compiled in through
  `fuzz/src/slides_edit.rs`: uo-wasm cannot be linked next to uo-ffi, both export `uo_buf_*`).
- Tooling: `tools/build-pkg.mjs` (alias `@universal-office/slides-editor-engine`, `ENGINE_SOURCES.slidesEditor`,
  `shipSlidesWasm(id, dist, 'editor')` → `chunks/uo-slides-editor.wasm` with the `wasm-slides` notices: `cargo tree` gives the same
  third-party crates as the viewer build), `.github/workflows/rust-core.yml` (tsc line, native replay in `soak`, Node API test, size/validate/ship
  step, the Chromium harness in `viewer`).

### Numbers

- **ABI replay** (`slides_edit_native.rs`, `UO_EDIT_ALL=1`, release, 13 s): every step of the four B8 captures encoded with the schema codec,
  applied through the exports on an editor over a viewer deck, the package compared AS THE SHELL MIRRORS IT (only from the changed parts):
  edit-samples 28 scripts / 1,680 steps (10,248 facets), edit-b1a+b6a 70 / 4,200 (25,620), edit-blank 12 / 1,440 (8,832), edit-fonts 25 /
  1,376 (8,406): **8,696/8,696 steps, 53,106 facets equal, 0 mismatches** (parts in order and bytes, UI state, the presentation and
  current-slide models read fresh from the mirror, the result of each step; the deck's slide count equals the state after every step).
- **Node, wasm module with `count`** (`slides_edit_api.test.mjs`, 6/6): an edit changes the list the viewer emits from the same deck; the
  mirror after insert/add/undo/undo equals the saved package part for part; clip between two editors; queries; save with Carlito embedded;
  refusals (another deck 11, null 11, junk 5, unknown history command 5, freed editor). **M4**: 10,000 editors through `free()` and 1,000
  through the `FinalizationRegistry`: live bytes back exactly. **M2**: 600 cycles of insert/format/frame/undo/redo/emit after 120 warm-up
  cycles: live bytes 211,680–211,684 (spread 1,600 B over 12 samples; the first sample 210,084 is still warm-up), wasm memory 8,126,464 B
  constant. **M5**: after 150 edits exactly 100 undo steps (the TS limit; the byte cap is B8's, `Limits::max_history_bytes`).
- **Fuzz** `slides_edit_wire` (stable fallback driver, 63 seeds: every `Op`, the queries, font answers, blank options): **1,458,566 executions
  in 90 s, 0 crashes**; invariants: the canonical form is stable (decode → encode → decode → encode equal), a failed command changes
  nothing, every `EditResult` encodes and decodes, undo+redo restores order and bytes, any accepted blank options save and reopen.
- **Sizes** (`--profile wasm`, no `count`): `uo-slides-editor.wasm` **3,975,236 B raw, 1,429,008 B gzip -9** (41% of the 3.5 MB budget);
  the viewer's `uo-slides.wasm` 3,454,622 B / 1,246,499 B gzip at the same tree (the editor adds 520 KB raw / 182 KB gzip).
- **Chromium harness** (`slides_edit_switch.mjs --mode all`, headless 1248, open fonts, tolerance 0; the TS and the Rust element in one
  page run the same seeded script through the public API — 38 kinds of step: every slide and shape method, the inline overlay commit,
  copy/paste, duplicate, notes, properties, replace, undo/redo, markClean, goto, the font option; both random sources seeded 1 as in the B8
  captures): **60 decks** (the 28 committed samples, 24 `b1a`, 8 `b6a`), all on the Rust engine, **2,400 steps**; after every step and after
  the load (2,460 checks) **package 2,460/2,460 equal** (part names, order, bytes), **UI state 2,460/2,460** (slide, selection, undo/redo,
  dirty, shape info, text format, notes, transition, hidden, background, properties, show settings, font option, editing), **slide pixels
  2,460/2,460** (`renderSlideToCanvas`, 960 px), **main canvas 2,460/2,460**; **`savePptx()` 360/360 equal** (read back part by part); 0 steps
  where only one engine threw (1 step where both did: a harness artefact, the overlay asked on a shape that opens none; guarded since).
  1,579 s. Phase 0: an editor without the attribute requests no wasm and no engine chunk, and the entry's static closure holds none of the
  engines' code. Memory: 300 insert/format/frame/undo/redo cycles: module live bytes 429,088–430,688 (spread 1,600 B), wasm buffer
  2,686,976 B from the 2nd sample on; a detached element keeps its 1 session, after switching to `ts` 0 sessions and 0 handles. Faults: DOM
  move then undo (session and history survive), the engine attribute ts→rust→ts with the document open (package kept), a forced trap (the
  element goes on in `ts` from the same package, `us-engine-error` fired): all pass. Harness bugs found while bringing it up (not engine
  bugs): two random draws inside the per-element callback (the elements got different arguments); the TS overlay commits twice when
  `commitEditShape()` runs with the overlay focused (its blur handler commits first; a TS quirk, both engines alike); the creation ids of
  copies (`freshCreationId`: the TS `Math.random` against the session's xorshift seeded from `crypto`; the harness seeds both with 1, the B8
  rule).

### Left on the TS path (counted; "complete = nothing disabled": nothing is disabled, these are TS reads or TS-only steps)

1. The TS model re-read from the mirrored package serves the hit tests, selection and handles, `getShapeInfo`/`getTextFormat`/`getLink`,
   `getLayouts`/`getSlideLayout`, `getSlideTransition`, `getSlideAnimations`, `getShowSettings`, `getProperties`, `findText` (the replace runs
   in Rust), the thumbnails' count, notes guard and the a11y text. Next wave: the read-model queries (other frontier) replace it.
2. `newPresentation()` builds the blank deck with the TS `blankDeck` (sync API; the Rust `uo_slides_editor_blank` is byte-equal part by part,
   B8) and opens it in both.
3. `insertImage` measures the picture's natural size with the browser (`imageSize`) and sends the frame (B8 decision 9).
4. The `.ppt` open: the TS converter feeds the TS copy, the Rust module converts the same bytes for the session (B2: part-equal).
5. Image decoding, font resolution and text measurement stay in the browser by design (T1/T2).

### Process metrics

| Stage | Notes |
|---|---|
| Design + port (one Opus session) | 3 hook requests to the viewer frontier (deck sharing, suppressed emission, JS adoption, lazy engine import, sessions surviving a DOM move), landed by the other agent and the orchestrator |
| Verification | native ABI replay 13 s release; Node API 4 s; fuzz 90 s; Chromium parity 1,579 s (60 decks x 40 steps), memory and faults about 1 min each |
| Rework | the codec's first decode limit (1 Mi list) refused a saved package with fonts (raised to the 1 GiB of the input); `type_`/`loop_` are not schema field names (manual binds); the fuzz crate cannot link uo-wasm next to uo-ffi (both export `uo_buf_*`: the codec is compiled in by path) |

## Slides wave 5 — PowerPoint rules at the wasm boundary (2026-11-18, Opus — agent A)

- **Exports** (`uo-wasm/src/slides.rs`): `uo_slides_set_rules(deck, flags) -> i32` (bit 0 = `SlidesRules::ts_strict()`; bits 1–5 =
  `ts_sys_clr`, `ts_first_baseline`, `ts_fallback_fonts`, `ts_date_locale`, `ts_warp` alone; a change empties the deck's caches, its
  measure table and the pending requests) and `uo_slides_set_field_clock(deck, year, month, day, hour, minute, second, lang_ptr,
  lang_len) -> i32` (the host's LOCAL time and the language of fields without one; `year` 0 = no clock). After `uo_slides_open` a deck
  follows PowerPoint's rules with no clock.
- **JS** (`js/slides.mjs`, `.d.mts`): `open(bytes, { tsStrict?, clock?, fieldLang? })` — `tsStrict` sets bit 0; otherwise the field
  clock is `clock ?? new Date()` in `fieldLang ?? navigator.language` (`clock: null` = none); `SlidesDeck.setRules(flags)`,
  `setFieldClock(date, lang)`. `slides-engine.ts` `LoadSlidesOptions` gains `tsStrict`, `fieldLang`, `now` (the viewer's and the editor's
  `configureEngine` pass them through); the default (no option) = PowerPoint's rules with the browser's clock.
- **Face names with a fallback context** (`!ts_fallback_fonts`): `fontStackWithFallback` (exported from `slides-engine.ts`) splits the
  U+001F name the Rust layout writes (`uo_dml_render::text::encode_face`) and inserts the run's ea, script and cs faces before the
  generic family of the host's stack; the measure answers and the painter's `fontStack` both use it.
- **Derived recipes** (B5b-G1): `meta-player.ts` `decodeFxRecipe` + `paint-slides.ts` `bitmapCanvas`: a version-2 recipe is `fxImage`
  over the base bitmap's canvas (the TS's `fxImage` over its rasterized canvas).
- **Parity under `ts_strict`**: the Node replays `tests/slides_{anim,api,drawops,edit_api,readmodel}.test.mjs` open with `{ tsStrict:
  true }`; the Chromium harnesses `tests/browser/slides_switch.mjs` and `slides_edit_switch.mjs` call `configureEngine({ tsStrict: true })`
  on the element class of the page bundle.
