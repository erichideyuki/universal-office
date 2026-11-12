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
| `uo_shell_hyphenation_patterns(h, env, langs)` | `hyphenationPatterns(env, langs)` | the pattern ids the document needs (`uo_write_hyphenation_patterns` over the session) |
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
- **ED-25** (fix round 2) `engineStats().editors` counts the CONNECTED `engine="rust"` elements: a disconnect parks the
  editor (document saved, session freed) and leaves the count, a reconnect re-enters it. The first rounds had a
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
