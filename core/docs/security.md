# Security of the Rust core (rust-core)

The product-level notes on untrusted documents (antivirus, hyperlink schemes in the Slides and Sheet viewers, CSP,
the published C# sanitizer) are in the repository's `docs/security.md`. This file holds the security reviews of the
Rust port's milestones.

## Milestone-1 security review (2026-11)

Reviewer: security specialist (Opus), 2026-11-10. Scope: the milestone-1 read path for untrusted documents — uo-docx
model + reader, uo-docx-layout, uo-render (display list, TS painter `js/paint.ts`), uo-pdf, the wasm Write viewer
binding (`uo-wasm` feature `write`, `js/write.mjs`, `js/viewer.ts`), the viewer switch `engine="rust"`
(`write/packages/viewer/src/rust-engine.ts`), `uo-ffi` (C ABI) and the C# facade. Tree: HEAD `ec66f1a1` plus the
uncommitted stage-1 work of 2026-11-10 (speed, ship and w2js frontiers). Read-only review: no source was changed; the
probes below live in `core/target/agent-sec/` (ignored) and are not committed.

### Summary

| # | Severity | Finding |
|---|---|---|
| SEC-M1-1 | **blocker** | Native stack overflow in the layout on 56 nested tables (1.5 KB docx): aborts the server process |
| SEC-M1-2 | major | Text-effect rasters have no per-page/per-document budget: a 4.6 KB docx makes the PDF take 61 s (native) / 70 s (wasm, uncancellable) |
| SEC-M1-3 | major | Server package limits are 8x the C# it replaces (4 GiB inflated per conversion vs 512 MiB) |
| SEC-M1-4 | major | The page's single wasm module is kept after a trap; the viewer never reinstantiates it |
| SEC-M1-5 | minor | Browser viewer: synchronous layout of a 6.9 KB mutant takes 63 s in wasm (2.8 s native) and keeps 613 MiB; trusted PDF caps and the 100,000-page default |
| SEC-M1-6 | minor | `clippy::indexing_slicing` is not denied in uo-docx, uo-opc, uo-drawingml, uo-fonts, uo-text, uo-jsnum (335 sites) |
| SEC-M1-7 | minor | G21 still open: no committed fuzz target over the whole read path or the exports |
| SEC-M1-8 | minor | The JS API hands out `javascript:` link targets unfiltered (`navigation()`, `runBoxes()`, `udv-link-click`) |
| SEC-M1-9 | minor | The viewer README's CSP section omits `connect-src` for `chunks/uo-write.wasm` |

What held up: hyperlink schemes in the Rust viewer path (click: `SAFE_LINK` `/^(https?:|mailto:)/i` in
`universal-document-viewer.ts:28/1392`, shared by both engines; PDF: `uo-pdf/src/uri.rs` `safe_pdf_uri` — a
`javascript:alert(document.domain)` link gives 0 `/URI` actions and no `javascript` byte in the PDF); SVG images
(`image-decode.ts` draws them through an `<img>` from a `blob:` URL, where scripts and external loads do not run; no
`innerHTML`/HTML sink takes document text anywhere in the Rust path); `unsafe` is confined to `uo-wasm`, `uo-ffi` and
`uo-alloc` (every library crate of the read path is `#![forbid(unsafe_code)]`); the C ABI (null checks on every
handle and output, size-prefixed option structs refused when short, `catch_unwind` in every export → code 12,
per-thread last error, lock poisoning handled with `PoisonError::into_inner`); the wasm boundary (`take_bytes`
consumes exactly once, `mask_in` checks `len % 4` and `w*h`, handles checked for null, the JS wrapper zeroes its
pointer on `free()` and unregisters the finalizer, so no double free or use after free through the wrapper; the
borrowed view is re-resolved through `#live()` on every paint call); the cancel poll swallows host exceptions;
reader recursion (64 levels) and the parsers' depth guards (`formula_5000`, 200 nested fields → `Limit FieldDepth
(64)`) hold.

### Evidence gathered

- **Fuzzing** (fallback driver, `core/fuzz`, stable; the libFuzzer build failed to link on Windows MSVC — `LNK1561
  entry point must be defined` when cargo-fuzz's flags are not applied, not investigated further): 16 targets × 30 s
  (`read_docx`, `zip_open`, `inflate`, `xml_strict`, `xml_lenient`, `image_decode`, `fonts_parse`, `fonts_gpos`,
  `fonts_gsub`, `fonts_cmap`, `fonts_odttf`, `fonts_embed`, `codec_decode`, `dml_effects`, `dml_raster`, `dml_blip`):
  **38.5 M executions, 0 crashes**, slowest single input 222 ms (`image_decode`).
- **End-to-end mutation probe of the wasm viewer module** (`scripts/gen_mut.py` + `run_mut.mjs`): 3,000
  structure-aware mutants of the 74 committed layout fixtures (attribute values replaced by boundary numbers, `NaN`,
  `1e308`, empty…; elements duplicated ×1–30, deleted, moved into each other), each through open → layout → encode
  and decode of up to 30 pages (render message + Write section) → navigation, run boxes, annotations, page heads →
  PDF. About 980 mutants ran before the 500 s timeout: **0 traps, 0 JS exceptions other than `WriteError`**; 15 took
  more than 3 s (SEC-M1-5).
- **Hostile documents** (`scripts/gen_hostile.py`), wasm (`count`-less `write` build of the current tree,
  3,963,825 B) and native (`uo-ffi` example `bench_speed`, release, Windows x64 main thread = 1 MiB stack):

| Document (size) | wasm | native |
|---|---|---|
| 64 nested tables (1,508 B) | ok, layout 62 ms | **stack overflow, process abort** |
| 200 nested tables (read as 64 + text) | ok, 264 ms, 84 MiB | **stack overflow** |
| 52 / 56 / 60 nested tables | — | ok / **overflow** / **overflow** |
| 64 / 200 nested text boxes and tables (VML) | ok | ok |
| field code `= ((((…1…))))` 5,000 deep | ok | ok |
| 200 nested complex fields | `Limit FieldDepth (64)` at open | — |
| `javascript:` hyperlink | link box and navigation carry it raw; PDF has no URI action | — |
| PNG declaring 60,000 × 60,000 | ok, image dropped | ok |
| 20,000 page breaks | 20,001 pages, layout 730 ms, 56 MiB | layout 71 ms |
| 600 runs with a 78.7 pt glow on one page (4,632 B) | PDF **69.8 s** trusted, **71.7 s** untrusted | PDF **61.3 s** |
| 1 run, 200 pt, 78.7 pt glow | PDF 2.1 s (untrusted 1.4 s) | 0.87 s |

- **Miri** on the current tree: `cargo +nightly miri test -p uo-wasm --features write --test write_unsafe --target
  x86_64-unknown-linux-gnu` — 1 passed, no UB, no leak (the new `uo_layout_page_heads` is covered for the null handle
  only; it takes no buffer). `uo-ffi --lib` and `render_unsafe` were not re-run (no change in their unsafe code since
  their recorded clean runs).
- **RustSec**: `cargo audit --no-fetch` (advisory DB of 2026-11-02, 1,402 advisories) over `core/Cargo.lock`
  (23 crates): no advisory. `cargo deny` is not installed here (CI job `deny` is the first run).

### SEC-M1-1 (blocker) — native stack overflow on nested tables kills the host process

- **Evidence**: `nest_tables_64.docx` (1,508 B: 64 tables, each in the only cell of the previous) through
  `pipeline::open` + `lay_out` + `render_pdf_to` on the Windows main thread (1 MiB stack, release + LTO):
  `thread 'main' has overflowed its stack`, exit 127. Bisected: 52 levels pass, 56 overflow (≈ 19 KB of native stack
  per nesting level). The reader alone handles 64 levels on the same thread (`fallback --reproduce read_docx`, 0.9 ms),
  and the PDF painter is flat over items, so the overflow is in the layout. The reader admits 64 levels
  (`DocxLimits::max_nesting`) and the layout guard is `LayoutLimits::max_depth` 192, so neither stops it. In wasm the
  same documents pass (wasm frames are much smaller; the 1 MiB wasm stack holds 64 levels).
- **Impact**: a stack overflow is not a panic: `catch_unwind` in `uo-ffi` does not catch it, the process aborts.
  `uo_docx_to_pdf` runs on the caller's thread (`server.rs` `convert`), so a 1.5 KB upload kills the .NET server
  process (thread-pool threads have 1–1.5 MiB; IIS worker threads have less). This is the same class as the
  published C#'s D3 S6, which the port was meant to close. The existing no-panic tests run on the test harness's
  2 MiB threads, so they cannot see it.
- **Fix**: run the whole conversion of `uo_docx_to_pdf` (and any future native export that lays out) on a thread of
  a fixed, documented stack (`std::thread::Builder::stack_size`, e.g. 16 MiB, joined; or one worker per converter),
  so the host's thread size stops mattering; measure the layout's stack per nesting level and add a regression test
  that lays out `max_nesting` (64) nested tables (and mixed text boxes) natively on a 1 MiB thread and in wasm; then
  decide whether to cut the per-level frames or lower the layout's effective depth.

### SEC-M1-2 (major) — no budget on text-effect raster work

- **Evidence**: `glow_dense_600.docx` (4,632 B, one page, 600 runs `ab ` each with `w14:glow rad=1000000` and a
  distinct colour): PDF 61.3 s native, 69.8 s wasm with the viewer's trusted caps, 71.7 s with `PDF_UNTRUSTED` (each
  layer stays under the per-call caps: kernel radius 256, 16 Mpx). Cost is linear in the runs (1 run of 200 pt:
  0.87 s native), so 6,000 runs (≈ 46 KB) are about 10 minutes. The page in the browser paints the same layers
  through `paint.ts` `paintGlyphsFx` → `rasters.fxLayer` per run (at screen resolution; not measured), and the
  painter cache (`capFx`) is cleared when full, so a page with more runs than the cap recomputes them on every repaint.
- **Impact**: on wasm `uo_layout_pdf` has no poll (G4-4), and the viewer's print/download (`universal-document-
  viewer.ts:1489` `lay.pdf()`) is synchronous on the main thread: the tab freezes for minutes, uncancellable. On the
  server, uo-pdf checks the cancel flag every 256 items, so cancellation latency on this page is ≈ 26 s; with no
  cancellation token the conversion runs to the end. The frozen TS has the same work (parity) and no caps, so this is
  not a regression, but the plan's rule "where the TS hangs, Rust stops" is not met for effects.
- **Fix**: a paint work budget per page and per document (the sum of layer pixels × kernel work of every text effect
  and picture effect, like the layout's `Budget`), past which effects are dropped (with a warning) or the call fails
  with `Limit`; poll the cancel flag per effect item in uo-pdf; give `uo_layout_pdf` the same opt-in `layout_poll`
  as the layout (or the budget alone in the browser).

### SEC-M1-3 (major) — server package limits are 8x the C#

- **Evidence**: `uo-ffi/src/server.rs:126` opens with `DocxLimits::DEFAULT`, whose package limits on native are
  `uo_opc::Limits::NATIVE` (`uo-opc/src/limits.rs:44`): 2 GiB per binary part and **4 GiB in total**. The frozen C#
  (`dotnet/src/UniversalOffice.Core/OpcPackageLimits.cs:12/15`) stops at 256 MiB per part and **512 MiB in total**.
  `Trust::Untrusted` (the server default) lowers only the paint and image caps (`pipeline.rs` `pdf_options`). Deflate
  reaches 1,032:1, so a ≈ 4 MB upload can make one conversion hold up to 4 GiB (uo-opc record: "340 B → 4 GiB held"
  before F7; with real deflate the ratio bound applies); concurrent conversions multiply it.
- **Fix**: with `Trust::Untrusted`, open with the C# package limits (256 MiB per part, 512 MiB total) or lower; let
  the facade raise them explicitly (`TrustedInput` or a limits option); add a test that a 600 MiB inflated document is
  `LIMIT_EXCEEDED` on the server path.

### SEC-M1-4 (major) — the page's wasm module survives a trap

- **Evidence**: `js/viewer.ts:318` `loadViewerModule` memoizes ONE module per page; `rust-engine.ts` `relayout`
  rethrows any non-`WriteError` (`throw e`), and nothing catches `WebAssembly.RuntimeError` to drop the module. The
  module is built with `panic = "abort"`, and on wasm an allocation failure also aborts; a trap stops Rust in the
  middle of a call, so the allocator, `RefCell` borrow flags (the last-error slot) and the no-threads `Mutex`es of
  the font registry/face cache (which abort when locked again) can be left inconsistent. The page environment of
  SPEED-1 also lives in that module for the page's life. No trap was found in this review (fuzz and mutation probe:
  0), but out-of-memory is reachable: a 6.9 KB mutant grows the module to 613 MiB (SEC-M1-5), the wasm32 package
  limit is 1 GiB inflated, and a phone tab dies far below the desktop's 4 GiB.
- **Impact**: one hostile document can leave every Rust viewer of the page failing (or, in the worst case, computing
  on corrupted state) until the page reloads.
- **Fix**: treat any `WebAssembly.RuntimeError` from the module as fatal for that instance: mark it dead, reset
  `loading`, forget the page environments and every handle of it, report the error on the element, and instantiate
  a fresh module on the next use; a test that forces a trap (a test-only export in the `count` build) and then opens
  a document on the same page.

### SEC-M1-5 (minor) — browser viewer: synchronous layout time, retained memory, trusted caps

- **Evidence**: mutant `m00253` (6,915 B, from `f7b__7__0`; table widths of `2147483647` and similar values) lays out
  to 3,004 pages in **63 s in wasm** and **2.8 s natively** (the original fixture: 90 ms wasm vs 38 ms native, so
  this input is ≈ 22× slower in wasm, against ≈ 2.4× for the fixture — a wasm-only superlinear path, cause not
  investigated); `m00045` (6,849 B) 3,003 pages, 25.7 s, module memory 613 MiB (wasm memory never shrinks, and the
  module is page-wide). 15 of ≈ 980 mutants took more than 3 s. The layout call is synchronous on the main thread,
  so the G4 `cancelCheck` (`stale()`, element disconnected) cannot become true while it runs; the work budget allows
  it. The viewer also uses the trusted PDF caps (SW-11) and the default `max_pages` 100,000.
- **Fix**: product decisions for the gate: a page cap for the viewer (`maxPages`), untrusted PDF caps for documents
  of unknown origin, the phone profile on phones, and a Worker for the engine in a later milestone (then G4 works);
  hand `m00253` to the speed frontier as a reproducible wasm-vs-native outlier.

### SEC-M1-6 (minor) — panicking indexing not linted in six crates of the read path

- **Evidence**: `cargo clippy -p uo-docx -p uo-opc -p uo-fonts -p uo-text -p uo-drawingml --lib -- -W
  clippy::indexing_slicing`: uo-docx 80, uo-opc 77, uo-drawingml 98, uo-fonts 27, uo-text 2, uo-jsnum 51 sites
  (uo-docx-layout, uo-render, uo-pdf, uo-image and uo-codec already `deny` it). Many are fixed-size tables (e.g.
  `uo-fonts/src/registry/digest.rs`), and fuzzing found none reachable; each wrong invariant would be a wasm trap
  (SEC-M1-4).
- **Fix**: `#![deny(clippy::indexing_slicing)]` in those crates, with `#[expect(..., reason)]` on the audited
  fixed-size sites.

### SEC-M1-7 (minor) — no committed fuzz target over the read path (G21)

- **Evidence**: `core/fuzz` covers the container, XML, fonts, images, DrawingML, codec and the reader, not
  `layout_document`, the render encoder or `render_pdf_to`, nor the exports. The mutation probe of this review found
  SEC-M1-5 in minutes; SEC-M1-1 was found only because the native driver runs on a 1 MiB thread.
- **Fix**: add `pipeline_docx_to_pdf` (native, the target body on a 1 MiB thread, `max_pages` and the work budget
  lowered so executions stay short) seeded with the layout fixtures, and a CI job running the wasm mutation runner
  for a few minutes; make the fallback driver accept a `--slow` artifact as a failure for this target.

### SEC-M1-8 (minor) — raw link targets in the JS API

- **Evidence**: for `javascript:alert(document.domain)`, `WriteLayout.runBoxes('links')[].value` and
  `navigation().links[].uri` return the target as written; `udv-link-click` hands it to the host in
  `detail.address` before the element's own `SAFE_LINK` check. Same as the TS engine (`runBoxes`, `pdfNavigation`).
- **Fix**: say in `write.d.ts`, `write-engine.d.ts` and the viewer README that these strings are untrusted document
  data and that a host must apply the same scheme rule before navigating; optionally expose the rule as a helper.

### SEC-M1-9 (minor) — CSP section incomplete for the Rust engine

- **Evidence**: `write/packages/viewer/README.md` "Content Security Policy" lists only `script-src
  'wasm-unsafe-eval'`. With `engine="rust"` the viewer also fetches `chunks/uo-write.wasm` (`connect-src`), which the
  TS engine never does; SVG pictures need `img-src blob:` and print needs `frame-src blob:` (both engines).
- **Fix**: list `connect-src` for the wasm URL (and the asset/font base) and the `blob:` sources in that section.

### Not done

- libFuzzer (coverage-guided) runs: the Windows MSVC build did not link here; the CI should run them on Linux.
- WebKit/Safari: the probes ran in Node only; real Safari 15 and iOS memory behaviour (SEC-M1-4/-5) not measured.
- The screen painter's cost on SEC-M1-2's page; the TS engine on the slow mutants (whether it is as slow).
- The C# facade under a real server thread (IIS/ASP.NET) for SEC-M1-1: measured on a 1 MiB native thread only.
- Reproduce: `python -I core/target/agent-sec/scripts/gen_hostile.py <dir>`; `node scripts/probe_wasm.mjs <wasm> <dir>
  [file]`; `printf 'run\t<file>\t1\nquit\n' | target/<dir>/release/examples/bench_speed.exe` (`cargo build -j 4 --release
  -p uo-ffi --example bench_speed`); the scripts are under `core/target/agent-sec/scripts/` (not committed; move them
  into `core/fuzz` with SEC-M1-7 if kept).

### Process metrics

Security specialist (Opus), one session, about 1 h 30 min wall (estimate), about 75 tool calls. Rework: the first
batch wasm probe piped through `tail` and hid which document hung (rerun one by one); the libFuzzer build was dropped
for the fallback driver after a link error; the mutation run's summary line was lost to its outer timeout (counts
taken from the mutant indices in the log).
