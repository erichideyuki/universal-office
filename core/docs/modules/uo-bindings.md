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

Open, not done here:
- **The screen painter** does the same work at screen resolution: the 600-colour page takes 6.2 s to paint at 96 dpi and
  39.7 s at 192 dpi (TS 1.1 s and 189 s); the rule is PDF only. Lowering the screen layers too would change screen pixels
  against the TS, and the Word's screen was not measured.
- **The glow is larger than the Word's.** At 79 pt the Word's glow reaches about 28 pt past the text and ours about 79 pt
  (the layer reserves `1.2 × rad`); at 18 pt the Word's line image is 36 pt tall, ours 54 pt. Rendered side by side
  (`core/target/pdf-time/png/`), the Word's 79 pt glow looks like a much smaller one. Pre-existing, TS too: needs the
  glow profile measured in the Word.
- **The glow grows the line in the Word**: 17 pt between lines without it, 19.5 / 26 / 56 pt with 5 / 18 / 79 pt (so the
  79 pt document is 2 pages in the Word and 1 in ours). The layout does not do it (TS too).
- The reflection's rule, the picture and shape effects (`paint_shape_effects`, `make_image`) and the TS: unchanged.

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
