# Rust port — handoff (state of 2026-11-24; newest update: "Sheet task 0 — preconditions")

Entry point for the next session that continues the Rust migration (0.10). Read this, then
`.claude/skills/rust-port/SKILL.md` (the workflow: hybrid mode, models per role, memory gate) and the module records
in `core/docs/modules/`.

## Where things are

- **Worktree** `D:\Projects\OfficeNext\.claude\worktrees\rust-core`, **branch `rust-core`**. Workspace `core/`.
- **Pushed**: `origin/rust-core` = `8d0d0004` (2026-11-23, the `uo-xlsx` commit; `<u-grid>`'s `grid-xlsx` and
  `grid-pdf` pin `uo-xlsx`/`uo-opc`/`uo-fonts` by git `rev`). Commits after it exist only on this disk until the
  maintainer pushes again. Still no CI run of `rust-core.yml`.
- **Frozen TS reference** (behaviour source, never edit): `UO_TS_ROOT=D:/Projects/OfficeNext/.claude/worktrees/clickcompliance-docx`
  (commit `d0f45bc5`, `dist` built). Do **not** use the main checkout as TS root (its `dist` is stale, F2-20).
  Layout captures need `--accept-stale typography/font/sfnt.ts` (F7a-14) and `node --max-old-space-size=8192` past
  300 documents.
- Untracked and not needed: `core/crates/uo-docx/js/_dump.mjs` (a debug scratch of an earlier session).
- Port status in lines: `core/docs/port-status.md` (measured 49% of Write at `c3f95ae5`; **not re-measured since** —
  the estimate after this session's work is ≈ 90% of Write's TS/C# lines, ≈ 40% of the whole core with Sheet and
  Slides at 0%). Re-measure with the method at the top of that file.

## Done (commits on `rust-core`, newest last; each record has the numbers)

| Area | Commit(s) | Record | Result |
|---|---|---|---|
| D1b writer (core + W1–W6), D4 `.doc` (F0–F6), D5 export/ODF/compare, R5 follow-ups, `odraw` | `c3f95ae5`, `82c756c6`, `3d7312ef`, `ea62b4bb` | `uo-docx.md`, `uo-docx-d1b/`, `uo-docx-d5/` | parity 100%; `.doc` 179,929 equal |
| Layout F4, F6, F7a–F7d, F8 | `a7fd7d5d`, `06c22a51`, `81a4c5ef`, `4199fd8e` | `uo-docx-layout.md` | **Word oracle 2,742/2,742 documents equal end to end**, 0 mismatches; F8 41,130 facets equal |
| uo-render G (geometry, paint helpers) and E (canvas encoder, wasm paint exports) | `8ff85d49`, `2ce56e90` | `uo-render.md` | 100% facets |
| uo-pdf P (the PDF painter) | `bc62a847` | `uo-pdf.md` | byte-equal to TS `renderPdf` (300 oracle docs locally) |
| D2a session core + D2b families F1, F2, F3, F5, F6, F9, F4 (partial) | `2447379a`, `a936cdbb`, `bfb56594`, `c61f647a` | `uo-docx.md` "D2a — session (architecture)", `uo-docx-d2b/F*.md` | both harnesses (Node + Chromium) 0 mismatches |
| D3 Editing API (C# → Rust) with a C# differential harness | `54129cd7` | `uo-docx-d3.md` | 100,872 facets equal to the frozen C# |
| Milestone-1 bindings: wasm `write` viewer API, `uo-ffi` C ABI, C# facade (`core/dotnet/UniversalOffice.Docx`), server path S | `4bcf5504` | `uo-bindings.md` | first end-to-end Rust PDF byte-equal to the TS (68/68 fixtures, 211/211 oracle docs locally) |
| Docs: untrusted documents, CSP | `ff9053cc`, `9fa066ad` | `docs/security.md` | — |
| Memory benchmark `main × rust-core` | `01c72ab5` | `uo-docx-layout.md` "Benchmarks … memory" | Rust layout = **0.23–0.41×** the TS memory |
| wasm2js fallback + wasm-opt, measured | `ba3e15b9` | `uo-bindings.md` "No-WebAssembly fallback" | works under strict CSP; 4–8× slower, 1.57 MB gzip |
| wasm size items 1–3a (planner → Sonnet → reviewer) | `da31758d` | `size-optimization.md` §8 | raw −33.5%, gzip −4.1%, same output, same speed |

## Update 2026-11-24 — Sheet task 0 (preconditions of `slides-sheet-port-plan.md` §0 for tracks A and C)

The maintainer said "continue the Sheet migration, one task at a time". Order: 0 preconditions → 1 §4.0 architecture
(one cell store for sheet-core + calc) → 2 §4.3 captures (workbook generator, `calc-traps`, `calc-tests`, the 35,299
bank) → 3 track A (calc, M3) → 4 C1/C2 (`uo-xlsx` reader + surgical save, `.xls`) → 5 C3/C4 (grid model, viewport,
`engine="rust"`, M5) → 6 C5 + .NET `UniversalOffice.Xlsx` (M6).

- **Backup**: none needed — `origin/rust-core` = HEAD `8d0d0004`.
- **Disk**: 25 GB free on D: (plan asks ≥ 40). Cleared the `uo-xlsx` build outputs (`core/target/xlsx/{debug,release,
  wasm32-unknown-unknown}`, `xlsx-wasm*`, 3.6 GB) → 29 GB. `core/target` keeps ≈ 4.5 GB of old-wave scratch and
  `w4-oracle` (PowerPoint oracle cache, keep).
- **§13 A decisions for tracks A/C, applied with the recommended option** (revisitable at M3/M5):
  - **T2** font policy = the TS Sheet rule (as Slides: a font installed by name is drawn by the browser; the display
    list carries the family; GDI width tables + the shell's advance table for missing glyphs, plan §7 C3).
  - **R1** calc regex = `regress` (ECMAScript semantics, the TS's PCRE2 → JS translation ported 1:1) with a step
    budget and a Unicode-version check against Node 24.15.
  - **C1** locale scope = **host hook**: the TS derives decimal/group separators, date/time order, AM/PM and the
    currency symbol from `Intl` for ANY locale (`i18n/value-locale.ts:25-44`, `translate.ts:126`) and orders lists with
    `Intl.Collator` (`translate.ts:686`). Dropping that would regress Spanish (UI language since 0.9.2), so the shell
    passes the `Intl`-derived `ValueLocale` (+ the collator order where used) into Rust — browser behaviour equal to
    the TS; server/.NET fall back to the measured pt-BR/en-US tables. Function names stay pt-BR/en-US only.

### Sheet task 1 — architecture pass (§4.0), 2026-11-24

Opus pass, ≈ 25 min; results in plan §16, records `uo-calc.md` (new), `uo-xlsx.md` "Track C architecture",
`uo-render.md` "Sheet extensions"; skeletons `core/crates/uo-calc` (new: the one cell store + calc) and `uo-xlsx`
`book`/`grid`/`session` behind features `read`/`save`/`xls`/`grid`/`edit` (default build = the writer `grid-xlsx`
pins, unchanged; its tests green). **Maintainer priority: performance and memory first** — store estimated at
4.6 B/cell on giant-1m (TS measured 239 B/cell: three copies), budgets + benchmark gates per frontier (§16 item 14).
Next frontiers: (S0 ∥ A1) → A2 → … (§16). Open: SH1–SH6.

## Update 2026-11-23 — `uo-xlsx`, the xlsx writer (track C, writing side)

New crate `core/crates/uo-xlsx` (record `core/docs/modules/uo-xlsx.md`), asked by `<u-grid>` (spec 29 of
`D:\Projects\data-grid`, approved 2026-11-23): the grid adds a thin `grid-xlsx` cdylib that depends on it by git `rev`.

- From scratch (the TS writer is surgical: nothing to port, no parity target): one state machine
  (`add_style` → `begin_sheet` → `append_row`… → `end_sheet` → `finish`), shared or inline strings, numbers, booleans,
  wall-clock dates (ISO text outside Excel's range), links with a scheme filter, a deduplicated xf pool resolved per
  sheet direction (Start/End), outline, frozen panes, merges, print titles, `ColumnTitle` names, core properties.
- **uo-opc**: streamed zip entries (`ZipWriter::stream_begin/stream_write/stream_end`, `Deflater::stream_*`,
  `buffer_capacity`); the existing API is unchanged and the suite is green.
- Verified: 27 unit/package tests + golden parts + memory (M2: 0 B growth over 20,000 rows) + no-panic (200,000
  sequences locally) + fuzz `xlsx_write` (515,698 execs, 0 crashes); **Excel 365 opens every sample without repair**
  (negative controls fail as they must), values/formats/styles/panes/names read back through COM; the TS reader reads
  them. **LibreOffice and Google Sheets not checked** (not installed / needs an upload).
- 10,000 × 40 heat map: 1.95 MB, 133 ms native, 475 ms wasm, peak 4.5 MB. Minimal consumer `.wasm`: 231 KB raw /
  94 KB gzip (opt 3), 75 KB gzip with `opt-level = "z"`.
- Fixed on the way: the fuzz crate did not compile (`fuzz/src/ppt.rs` missed `ConvertOptions::ts_ppt_charts`).

## Update 2026-11-07 (later the same day) — WIP finished, workspace green

The WIP below was finished in one workflow run (Opus planner → Sonnet implementer → Opus reviewer, up to 2 fix
rounds; G8 implemented on Sonnet at the maintainer's request although SKILL.md says Opus for the layout crate):

- **D2b F7** done: record `core/docs/modules/uo-docx-d2b/F7.md` (Node + Chromium harnesses, regex sites, M1–M3,
  no-panic). Declared open items there: `gradientVariations`/`lineWith`/`POSITION_PRESETS` not ported; F7-D10 (typing
  with an object selected) and some multi-selection behaviours left for the milestone-2 gate; `align_objects` with a
  `rects` length mismatch (reviewer minor).
- **G8** done: records `uo-docx-layout.md` (frontier E / G8) and `uo-bindings.md` (G8 row). Maintainer decisions
  G8-W (section wire size) and G8-2 (lazy section in the wasm handle); the new CI steps have never run; no versioned
  fixture with comments (comment-rect path proven only on 10 local real documents).
- **Workspace**: `clippy -D warnings` native + wasm32 for the whole workspace, `clippy-ban`, `float-casts`,
  `grep-bans` all green; suites uo-docx (all features), uo-docx-layout, uo-wasm, uo-fonts green. Pre-existing F2
  float casts at `session/ops/lists.rs:761/1092/1126` are only visible with the `session` feature (the script runs
  without features) — fix and extend `float-casts.sh` next.

Next: queue item 2 (uo-render frontiers D and V).

## Update 2026-11-10 — milestone-1 code done (queue items 2–4)

Same workflow (Opus plan → Sonnet implement → Opus review; all three accepted after fix rounds):

- **uo-render D + V**: the TS painter of decoded pages (`core/crates/uo-wasm/js/paint.ts`, `picture.ts`, mask exports
  in uo-wasm/uo-render; look→spec helpers moved from uo-pdf to `uo_render::paint::picture`) and painter parity
  (`tests/paint_parity.test.mjs` Parts A–C, `browser/paint_pixels.mjs`, `paint_memory.test.mjs`). Record: `uo-render.md`.
- **G4**: `LayoutOptions.cancel` / `LayoutError::Cancelled` in `Budget::charge`, wired to uo-ffi (code 10), the C#
  facade and wasm (`cancelCheck`, import `uo_host.layout_poll`, layout flag bit 9). Records: `uo-docx-layout.md`,
  `uo-bindings.md` G4 row.
- **Viewer switch**: `engine="rust"` on `<universal-document-viewer>` (`write/packages/viewer/src/rust-engine.ts`,
  default `ts` untouched, wasm fetched only on first use as `chunks/uo-write.wasm`), CSP note, CHANGELOG entry. Full
  Chromium parity: **74/74 fixtures, 579 pages, 0 differences** (pixels exact, text layer, find, links, PDF).
- Verified by the orchestrator: workspace clippy native + wasm32, bans, suites uo-docx/layout/render/pdf/wasm/ffi/fonts
  green.

Open before the milestone-1 gate (besides queue item 5):
- Rust path opens slower in the browser (loadDocx median 251 ms vs TS 122 ms on the fixtures) — measure and explain.
- `tools/build-pkg.mjs` ships `uo-write.wasm` without the licence notices of the Rust crates compiled in — must add a
  THIRD-PARTY notice (CLAUDE.md rule) before any publish from rust-core.
- CI lines for D/V/G4/switch not added to `rust-core.yml`; Part A of the painter parity is platform-pinned (win32) and
  would always skip on Linux CI — recapture or relax the pin.
- `write_wasm2js.test.mjs` fails (`f8__7__0 messages: length 51757 vs 53200`), also at the previous base — investigate.
- Painter not run in WebKit/real Safari (only the handle layer).
- ~~Speed of verification~~: done 2026-11-10 (one uo-docx test binary, `tools/check-changed.mjs`; see the operating
  rules below).

## Update 2026-11-10 (later) — milestone-1 gate work (queue item 5, minus what needs the maintainer)

Workflow `rust-port-milestone1-gate` (3 frontiers plan → implement → review, then security review, milestone skeptic
and benchmarks, then a fix chain; every reviewer accepted). Items above, now:

- **Open speed** — root cause: one `WriteEnv` per viewer element (assets re-pushed, every face re-hashed, a discovery
  pass on every document). Now one environment per page and fonts base, superseded passes not decoded, pages
  decoded on demand (`uo_layout_page_heads`), one face copy out of wasm. Chromium, new element per document: Rust
  median 44–47 ms vs TS 62–72 ms. Still slower in total (S1/S2 9.4 s vs TS 8.9/7.6 s): wasm layout in Chromium
  (9.0 s vs 7.1 s), tier-up unproven (G30). Record: `uo-bindings.md` "Open speed".
- **Licences** — `core/tools/licenses/gen-notices.mjs` → `core/licenses/` (checked with `--check`);
  `build-pkg.mjs` `shipEngineWasm` fails without current notices; the NuGet packs them (and its `runtimes/` path no
  longer repeats the RID). `deny.toml` `[licenses]`.
- **CI** — `rust-core.yml` jobs for painter (Linux recapture of Part A), viewer switch (phase 0, parity, memory,
  faults), wasm2js, cargo deny licenses. **Never run** (branch not pushed).
- **wasm2js** — cause: LLVM O3 turns `OptF64`'s sNaN marker into an `f64.const nan:payload`, which wasm2js prints as
  `NaN`. `OptF64` is now an integer (0 = absent, bits + 1; still 8 B) — this settles **G22**; uo-image TIFF
  `UNDEF_BITS` is a plain NaN; `build-wasm2js.mjs` fails on NaN-payload constants. 5/5 on O3 and z.
- **WebKit** — painter Part A in Playwright WebKit (Windows WebKit 26.5); real Safari 15 still open.
- **F2-20** — the stale-stamped captures were recaptured against the frozen root (`d0f45bc5`).
- **Security review** (`core/docs/security.md`, new) — blocker fixed: 56+ nested tables overflowed the native stack
  in layout (kills the .NET server); `uo_docx_to_pdf` now converts on its own 16 MiB thread
  (`hostile_inputs.rs`, `write_hostile.test.mjs`). Untrusted documents open with the C# package limits (256 MiB per
  part, 512 MiB total). A wasm trap kills the instance and the next use loads a fresh module (`viewer_trap.test.mjs`).
  Per-effect native cancellation of raster effects; fuzz target `ffi_docx_to_pdf` (G21 in part).
  `uo_alloc::assert_no_leak_global` for tests whose work runs on another thread.
- **Benchmarks** (`uo-docx-layout.md` "Benchmarks main × rust-core — speed", `tools/bench/speed/run.mjs`): 68
  captures + 5 generated classes up to 500 pages, PDFs byte-identical in TS, wasm and native. Warm totals vs TS:
  native 0.90, wasm 0.94 (open 0.56/0.61, PDF 0.67/0.77, layout 0.96/0.98). 500-page wasm layout 1.40× TS (G30).
- **Skeptic** — checklist in `uo-bindings.md` "Milestone-1 skeptic review (2026-11)".

- **Real Safari** (2026-11-10, `viewer_switch.mjs --mode serve` + BrowserStack, temporary dev tunnel): 0 differences
  TS × Rust on Safari 15.6.1 (Monterey) and iOS 26.5, 19 documents / 82 pages, flat memory; iOS 15.5 (iPhone 13) 16 of 19
  equal, the 3 largest crash the TWO-viewer harness at dpr 3 (each engine alone opens them). Safari 15 refuses wasm even
  with `'wasm-unsafe-eval'` (needs `'unsafe-eval'`). Record: `uo-bindings.md` "Real Safari 15 / iOS".
- **Effect resolution as the Word** (maintainer: Rust only; the frozen TS keeps 200 dpi): the PDF writes a glow of
  20 pt or more and a shadow blur of 10 pt or more at `200 / k` dpi, an image per effect, as measured in the Word
  (uo-pdf `fx_downscale`). `glow_dense_600` (now generated with a colour per run, as its record said): 44 s → 1.3 s
  wasm (Word 1.2 s); viewer parity on every fixture unchanged (0 differences). Record: `uo-bindings.md` "Effect
  resolution as the Word".
- **The screen too** (`9b7edbeb`, maintainer: "the PDF rule, smoothed"): uo-render `fx_plan`/`fx_layer_set`, export
  `uo_render_fx_layer_set`, `paint.ts` draws coarse layers with smoothing; coarse layers pool a finer silhouette (a
  150 pt glow had vanished at 25 dpi). That page: 6.2 s → 1.5 s at 96 dpi, 39.7 s → 2.6 s at 192 dpi (Word 0.33 s).
  The Word's screen measured by PrintWindow (`1b6dd8c7`): fast, smooth, same size at every zoom.
- **Text effects scale with the font size** (`34caf5c0`): the Word sizes a run's glow radius, shadow blur and shadow
  distance by `(size/72)^ln 2` (measured 6–200 pt, within 2 %); the TS (and we) applied the picture-effect model
  unscaled, right at 72 pt only (3.5× too large at 12 pt). uo-render `word_fx_scale`/`fx_for_word`, screen and PDF;
  `fx_layers` and `PdfOptions::frozen_text_fx` keep the parity tests on the TS path. Record: `uo-bindings.md` "Text
  effects scale with the font size". Lab: `core/target/pdf-time/` (outside git: generators, Word scripts, captures,
  comparison sheets).

**Fidelity findings to fix (Word measured, not done; Rust only unless the maintainer says otherwise):**
1. **The glow grows the line** in the Word (17 pt between lines → 19.5 / 26 / 56 pt with 5 / 18 / 79 pt glow at 12 pt
   text: the 79 pt document is 2 pages in the Word, 1 in ours). Layout change: measure the rule (does it scale with
   the font size too? shadow and reflection?) in the oracle first.
2. **The reflection's distance**: a 10 pt `w14:dist` drew no gap in the Word at 72 and 144 pt; ours moves it. Measure
   the gallery presets (0 / 4 / 8 pt offsets), the blur and whether the reflection scales with the font size.
3. **A document without a styles part**: the Word uses Aptos 12 pt (compatibility mode 12 or 15); ours a smaller serif.
4. **A blur-free shadow** is vector in the Word's PDF (a copy of the text), an image in ours.
5. **The Slides** use the same text-effect model (`shared/drawingml` → uo-drawingml): measure whether PowerPoint also
   scales text effects by the font size (its COM opens windows: needs a no-focus recipe or the maintainer's ok).
6. Residuals on the straight edge: our glow sigma 1–4 % wider, half alpha up to 10 % farther (the Word's dilation
   may be `0.46·rad`); the PDF's `k` off by one at 72 pt / 150 pt (26 vs 27).

**Status of everything (2026-11-18):**

| Product / milestone | State | What is left | Gate |
|---|---|---|---|
| Write M1 — viewer + PDF + server | code complete (layout = Word oracle 2,742/2,742; Rust PDF byte-equal to TS) | — | **open, maintainer**: push + first green `rust-core.yml`, decisions G23–G31 |
| Write M2 — editor | code complete (every D2b family routed behind `engine="rust"`, editor switch done) | — | **open, maintainer**: M2 gate, then 0.10 |
| Slides M4 — viewer + PDF | code complete: B1–B7, `uo-charts`, `uo-dml-render`, PDF with the PowerPoint rules (wave 5, agreement 0.9734) | 6 Safari pending items, 5 PowerPoint rules measured with no rule found | **Safari accepted with pending items**; questions SP1–SP4, DR1–DR3 open |
| Slides (M6 part) — editor + .NET | editor on Rust (2,460/2,460 steps, saves 360/360), .NET facade (C# = native 93/93) | 5 paths still on the TS model by design (hit tests/getters via the mirrored TS model, `newPresentation`, `insertImage` size, `.ppt` converted twice, image decode) | M6 (with N1, the NuGet API shape) |
| Sheet track C — xlsx writer (`uo-xlsx`) | done 2026-11-23: from-scratch writer for `<u-grid>` (spec 29): streamed rows, styles, outline, panes, names, properties; opens in Excel without repair | reader, surgical save, LibreOffice/Google Sheets checks | — (record `uo-xlsx.md`) |
| Sheet M3 — calc (track A) | not started (`uo-numfmt` and the BIFF8 chart converter `uo_charts::biff` already reusable) | everything | — |
| Sheet M5 / M6 — viewer, editor | not started | everything | — |

Cross-cutting: the `rust-core` branch was pushed once (`62b381d7`, for the grid); no CI run exists for any of the above.

**How much is left (2026-11-10; TS lines = non-blank non-comment source of the frozen checkout, an indication of
size, not of effort):**

| Product | Done in Rust | Left | TS still to port |
|---|---|---|---|
| Write, milestone 1 (viewer + PDF + server) | everything: reader, writer, `.doc`, export/ODF/compare, layout, render, PDF, C# Editing API, viewer/FFI/C# bindings | the gate: push + first green CI, decisions G23–G31, D2a | — |
| Write, milestone 2 (editor) | session core D2a, every D2b family F1–F10 (wave 1 `4c003113`), incremental relayout LA7, the session in wasm, the D3 C ABI + C# facade (wave 2, 2026-11-11) | the editor switch (every family routed behind `engine="rust"`, waves 4–6, layout parity and Word oracle kept); left: the milestone-2 gate and 0.10 | editor package 36.8k (most of it stays as the TS shell) |
| Slides | the shared parts only (fonts, drawingml) | everything else: `slides-core` (reader, model, layout, render), the editor, the viewer; charts | 25.9k + charts 5.2k |
| Sheet | the shared parts only | everything: calc engine, `sheet-core`, the editor; charts (shared with Slides) | 34.7k |

**Still open for the milestone-1 gate (needs the maintainer):** push `rust-core` and get one green `rust-core.yml`
run; decisions **G23–G31** in `uo-bindings.md` "For the
milestone-1 gate" (G23 layout stack per level, G24 work budget for text effects — 600 glow runs = 61 s, now about 1 s in the PDF with the Word's resolution rule but unchanged on screen, G25 limits
of both trust levels, G26 wasm2js trap detection, G27/G28 page environment rebuild/shrink, G29 M3 baseline, G30
Chromium/500-page wasm layout and server PDF vs C# untimed, G31 G5 caps on the corpus and the deferred bounds); then
the gate itself.

## M4 gate (Slides viewer) — checklist (state after wave 5, `b457f372`)

"M4" here is the **milestone** of `slides-sheet-port-plan.md` §11 (Slides viewer: B1–B7, `uo-charts`, `uo-dml-render`,
Slides PDF in the §9 M4 scope). Do not confuse it with the **memory-gate step M4** of SKILL.md (10,000 create/drop per
handle, 0 bytes left), which every wave already checks and which is one input of this gate. The editor (B8) and the .NET
facade are done too but belong to milestone M6 (with decision N1, the NuGet API shape).

| Criterion (§11) | State | Evidence |
|---|---|---|
| Op-stream parity with the frozen TS | met | draw-ops samples 262/262 + b1a/c1d sets, animated 472 + 551 + 150, B2 `.ppt` 1,082 parts, all under `ts_strict()` |
| Pixel parity in Chromium (tolerance 0) | met | `slides_switch.mjs --mode all`: 253 decks, 1,040/1,040 slides, 7,221/7,221 animated frames, 0 TS-model loads |
| PowerPoint agreement | met (numbers) | 736 pages: all 0.9734, samples 0.9996 (99.8% ≥ 0.99); 5 items measured with no rule (wave 5 "Open") |
| Sizes | met | after wave 5: viewer `uo-slides.wasm` 1,307,808 B gzip = 82% of 1.6 MB; editor 1,482,490 B gzip = 42% of 3.5 MB |
| Memory gate M1–M5 | met | M4 handles 0 B, M2 soaks flat, M3 retained model 0.447× TS, M5 caches capped |
| Safari | **accepted for this version with pending items** (maintainer, 2026-11-18) — see `uo-bindings.md` "M4 Safari criterion: accepted with pending items" | real Safari on BrowserStack (Device Control, driven by the agent) through `slides_switch.mjs --mode serve` + a temporary devtunnel, 39 decks / 144 slides; iOS 26.5 PASS-EXCEPT-KNOWN; Safari 26.4, 15.6, iOS 15.5 FAIL only by 1–2 px pictures (one 22 px) where TS = TS and Rust = Rust; see below |

Real Safari runs (2026-11-18; record `uo-bindings.md` "First real-Safari run (BrowserStack, 2026-11-18) and its fixes"):
- Run 1 found and fixed (committed): a regex lookbehind in `shared/charts/src/render.ts` (and `shared/spell/src/tokenize.ts`)
  that Safari < 16.4 cannot parse — the whole Slides viewer failed to load on Safari 15 / iOS 15 (also in the published
  0.9.2, TS engine too); and a font-load race in the Rust engine (measure cache filled with the fallback face before Safari
  applies a new face).
- Run 2 (after the fixes), TS vs Rust under `ts_strict`, links/a11y/soak equal everywhere:
  Safari 26.4 macOS 5 slides differ (1–238 px), anim 178/178; iOS 26.5 iPhone 17 5 slides (2–38,875 px), anim 178/178;
  Safari 15.6 macOS 10 slides + 23 anim frames differ (plus 10 known: the TS metafile player needs `OffscreenCanvas`);
  iOS 15.4 iPhone 13: the `.ppt` converter wasm traps from the 2nd `.ppt` (OOB / unreachable in
  `uo_slides_ppt_convert_with`), canvases return null at dpr 3 (canvas memory, as in the Write iOS 15 run), pixel diffs.
  CSP as measured for Write (Safari 15 needs `'unsafe-eval'`; 26 accepts `'wasm-unsafe-eval'`).
- Pass 2 (record "Second real-Safari pass"): Safari 15's optimizing JIT truncates doubles stored into a `Uint8ClampedArray`, so
  path gradients and picture colour effects came out a level darker once the page warmed up (both engines' JS rasters now store
  `u8(v)`; Safari 15.6 b1a 562–15,898 px → 0). A WordArt's ink box kept the fallback face's metrics when a face took effect late
  on iOS 26 (the Rust engine now re-measures a probe per font on the painted context before each paint). `engine="rust"` measures
  on the painted canvas when the browser refuses its measuring canvas (iOS 15). The harness waits for the font state before the
  compared renders, and classifies Safari's own variance between elements with a TS and a Rust twin. JavaScriptCore's libm (TS
  coordinates one or two ulps from V8's) is shown by `?v8math=1`. After it: iOS 26.5 PASS-EXCEPT-KNOWN; anim 0 unexplained frames
  everywhere; left over 1–2 px pictures (and one 22 px slide) where both TS elements agree and both Rust elements agree, classed
  variance in other runs of the same device. The iOS 15.4 `.ppt` trap: fixed by the other agent (heap reserve, WebKit 238302).
  Results: `core/target/ss-pixel/slides-viewer/serve-results.jsonl`, BrowserStack build `slides-m4-pixels`.
- Final consolidated run (orchestrator, commit `9ed4275f`, 4 devices at once, build `slides-m4-final`; harness verdict FAIL on
  all four only by small unclassified leftovers): Safari 26.4 — 1 slide 26 px (b1a/0032), anim 178/178; Safari 15.6 — b6a/0022
  56 + 6 px and one main canvas (b1a/0012), anim 178/178, 9 known b5b; iOS 26.2 (iPhone 17) — every slide diff same-engine
  variance, 5 animated frames unclassified; iOS 15.5 (iPhone 13, `elw=640`) — b6a/0037 12 px and one main canvas, 0 traps,
  4 frames variance, 9 known b5b. Links (138,240 points), a11y, notes, soak equal everywhere. The leftovers move between decks
  from run to run and the same slides are classed same-engine variance in other runs.
- **Maintainer decision (2026-11-18): accepted for this version; the leftovers stay PENDING, to continue later** — not closed
  and not declared differences. The pending list (6 items: the unclassified leftovers, the iOS 15.4 `memory.grow` bug in the
  other wasm modules, the TS metafile player without `OffscreenCanvas`, JSC libm vs V8, the iOS 15 harness canvas memory,
  Safari < 16.4 in Write/Sheet and third-party code) is in `uo-bindings.md` "M4 Safari criterion: accepted with pending items".

Questions for the maintainer at the gate (each with the record's recommendation):
- `uo-pptx.md` "Questions": **SP1** the edit-history byte cap; **SP2** canonical model JSON with sorted keys; **SP3** the
  TS `Math.min(...hugeArray)` `RangeError` on hostile input (Rust paints the shape); **SP4** `uo-ppt.wasm` as a separate
  lazily loaded instance vs `.ppt` inside the editor module.
- `uo-dml-render.md`: **DR1** warp — the viewer still bends text in the shell (`slides-warp.ts`), the PDF in
  `uo-pptx/src/pdf/warp.rs` (wave 5 found no PowerPoint rule for non-circular paths, `ts_warp` unread); **DR2** `FaceKey`
  as the measure-table key; **DR3** `size_of::<Shape>() ≤ 256` — met (224 B).
- `uo-charts.md`: CH-1…CH-6 were applied with the recommended option (CH-4: no prefix canonicalization for Slides).
- Plan §13 A decisions applied with the recommended option, revisitable here: T1 advance tables + `TextRun`, T2 font
  policy, P1 PDF scope = §9 M4, corpus = generated decks + committed samples (no downloads).
- Wave 5 decisions to confirm: product default = PowerPoint rules (TS only via `ts_strict()`); unknown latin family keeps
  the repo `shared/fonts` rule (PowerPoint uses Calibri); new dependency `zune-jpeg =0.5.15` (PDF only).

Gate prep (orchestrator, before handing over): one consolidated verification run (the wave-5 gate set + sizes), a gate
report with the numbers above, and a Safari script for the maintainer (commands, pages, what to look at).

## Update 2026-11-18 (wave 5) — PowerPoint rules measured and switched (`SlidesRules` / `ChartRules` / `PdfOptions`)

- **Switches** (skeleton `1526ea3f`): `uo_dml_render::rules::SlidesRules` (`ts_sys_clr`, `ts_first_baseline`,
  `ts_fallback_fonts`, `ts_date_locale`, `ts_warp` (unread — no rule found), `ts_links`, `charts`),
  `uo_charts::rules::ChartRules` (`ts_insets`, `ts_auto_title`, `ts_pie_size`, `ts_err_bars`), `PdfOptions::{
  ts_hidden_slides, ts_link_rects}`, `ConvertOptions::ts_ppt_charts`. Default = PowerPoint everywhere (viewer, editor,
  PDF, server); `ts_strict()` = the frozen TS, used by every TS-parity replay (native, Node, Chromium: viewer 653/653
  slides, 626,880 link points, editor 756/756 + saves 108/108, draw-ops 262/262, B2 `.ppt` replay, uo-charts parity all
  equal under ts_strict). Bindings: wasm `uo_slides_set_rules`/`set_field_clock`/`ppt_convert_with`, JS `open(bytes,
  { tsStrict, clock, fieldLang })`; uo-ffi `SLIDES_JOB_TS_STRICT = 2`, `DECK_TS_STRICT = 2`; C# internal `TsStrict`.
- **Rules (measured in PowerPoint 365 pt-BR via COM + PDF export; records in `uo-dml-render.md`, `uo-charts.md`,
  `uo-pptx.md`, `uo-pdf.md` "wave 5")**: sysClr = stock Windows `GetSysColor` table (charts too); first-line baseline
  `1.2·A/(A+D)` em from OS/2 win/typo metrics (`ppt_line_box`, `known_line_box` for 30 families), mixed faces, reduced/
  expanded/exact spacing, block foot = one scaled descent; date fields per language (15 locale tables, era/Dangi;
  injectable `FieldClock`, none = cached text); colour effects over metafiles (derived recipe `UOMF` v2; browser + PDF);
  JPEG under picture effects (new `zune-jpeg =0.5.15`, PDF only, licences regenerated); PDF hidden slides left out
  (all-hidden → refused, as PowerPoint); link rectangles per line × formatting run, ink bounds, kept off-page, shape-click
  reach; fallback fonts per character class / script / language (`text/fallback.rs`, ea/cs + theme script table in
  the model, context carried in the face name); charts: insets, auto title, pie size, legends (spacing, corner), date-
  axis label insets, title line height, error bars (new; the TS never drew them); `.ppt` charts native when the
  round-trip blob says `convChart="1"` (new `uo_charts::biff`, BIFF8 chart → chartSpace, reusable by the Sheet port).
- **PowerPoint agreement, 736 pages** (full comparison once, before A's JPEG decoder and the last follow-ups): samples
  0.9964 → **0.9996** (99.8% ≥ 0.99), generated 0.8070 → 0.8565, all 0.9616 → **0.9734**; differing px 21.82 M →
  14.86 M (sysClr 15.69 → 10.52 M, baseline 2.41 → 0.57 M, missing glyphs 2.08 → 1.56 M, charts 0.18 → 0.06 M, `.ppt`
  charts 73 k → 6 k); B's follow-up later: chart pages 2.30 M → 2.10 M px, `.ppt` chart pages 235 k → 62 k. Rust × TS
  0.9910 → 0.9798 (by design). Open-font policy on samples 67.9% → 52.7% of pages ≥ 0.99: all Aptos text drawn by
  Liberation Sans at PowerPoint's (correct) baseline — not a bug.
- **Open, measured with no rule (5)**: colour-modifier rounding of exact halves (PowerPoint rounds some down — no fixed-
  point model fits); `fontAlgn` t/ctr offsets; warp placement on non-circular paths (offset ~0 at 1:1 and 3:1, ~7 pt at
  1.5:1–2:1); first-key inset of horizontal legends (0.11–0.49 pt; constant 0.4 used); Thai glyph source in a slot face
  without Thai. **Documented differences (by decision)**: unknown latin family → PowerPoint uses Calibri, we keep the
  repo `shared/fonts` rule; dates beyond the 15 measured locales; sysClr = stock table, not the machine's accent; turned
  shape-click rects; mapped-colour edges of bi-level/colour-change recipes.
- Orchestrator: skeleton, relays between agents (fallback API, `.ppt` converter switch, warp ownership), disk cleanup
  (stale `m2-wsession` build dirs + default `target/debug`, ~40 GB), digests re-blessed (93/93 moved; `b6a/0000` now
  `refused`: every slide hidden).
- **Next**: M4 gate prep for Slides (real Safari = maintainer); then track C (Sheet) per `slides-sheet-port-plan.md`.

## Update 2026-11-18 (wave 4b) — model memory compaction, PDF gradient text + SVG, the 23 b5b metafile diffs closed

- **Model memory** (Opus): rare fields boxed behind accessors (split by presence over the corpus's 15,171 shapes),
  spare capacity released after each read (`SlideModel`/`TimingModel::shrink_to_fit`), equal themes shared through a
  `ThemeTable` on `Deck` (FIFO, 32, emptied by `clear_caches`). Only storage changed — no value, no `f64`→`f32`, no
  float reordering. Retained `SlideModel` b1a/0008 slide 0: 156,307 → **64,224 B = 0.447× TS** (target 0.5×); `Shape`
  408 → 224 B (budget 256); every group ≤ 0.5× TS (all groups 100.2 MB → 40.3 MB vs 112.2 MB TS). TS side scripted as
  `uo-pptx/js/measure-memory.mjs` (the B1b method also counted the ~2 KB `DeckSource` read set; now a fresh source per
  read). Model/B2/B3/B6/draw-ops/B5b/B4b/B8/links/PDF parity unchanged; Slides PDF digests unchanged by it. New M3
  asserts (`m3_retained_slide_model_by_group`, `m3_model_type_sizes_are_within_budget`). Record: `uo-pptx.md` "Model
  memory". Next candidates (not needed): `TableCell` 288 B, string interning.
- **b5b metafile diffs closed** (Opus): the 23 slides were browser-side, not recipe bugs — 20 from replaying into an
  `OffscreenCanvas` (Chromium antialiases differently from the TS's `<canvas>`; `elementCanvas` is now the default),
  3 from EMF+ compressed bitmaps decoded only on first paint (now `uo_slides_meta_bitmaps` + `prepareSpecial` at open,
  as the TS). b5b **81/81** slides pixel-equal (40 decks), now in the default CI set: 253 decks, 1,040/1,040 slides,
  998,400 link points, 7,221/7,221 animated frames, 0 TS-model loads. Open: B5b-G1 (colour effects over a metafile;
  0 of 313 corpus metafiles) needs an `Fx` derived-recipe op + `uo-pptx/src/pdf/recipe.rs`.
- **PDF fixes** (Opus), oracle items 1 and 10: (1) gradient text — pdfium applies a text clip at `ET` only if the
  render mode is still a clip mode; the painter now keeps `7 Tr` until `ET` (inside `fill_spec`'s `q…Q`), and a
  gradient run with no glyphs paints nothing (empty clip = page flood in pdfium); broken fills over 627 oracle PDFs
  2,940 → 0. (10) SVG pictures with no PNG beside them drawn as vector forms (`uo-pptx/src/pdf/svg.rs`, the static
  subset of Office icons; gaps counted `svg:*`, `PdfReport::svgs`). PowerPoint agreement, all 736 pages: mean 0.9445 →
  **0.9616**; samples 0.9947 → 0.9964 (TS×PowerPoint 0.9905); generated 0.7207 → 0.8070 (TS 0.8057); PDF-only differing
  px 9.78 M → 0.86 M. Write PDF bytes unchanged (`uo-pdf` untouched). Links (row 9) re-measured: PowerPoint writes 4,043
  rects on 82 pages, 2,741 wholly off the page; Rust 181 on-page — PowerPoint's per-line rule needs measuring before
  the join changes. Record: `uo-pdf.md` "PDF fixes (items 1 and 10, 2026-11-18)".
- Orchestrator: `slides-native.tsv` re-blessed (the PDF fixes change Slides PDF bytes).
- **Next**: wave 5 = PowerPoint rules measured in PowerPoint and switched like Write's `ts_*` (sysClr, first-line
  baseline, fallback fonts, date locale, hidden slides in the PDF, warp geometry, chart insets, `.ppt` charts, link
  rectangles) + B5b-G1; then the M4 gate (real Safari = maintainer).

## Update 2026-11-17 (wave 4a) — Slides editor on Rust, viewer without the TS model, PDF links, PowerPoint oracle

- **Editor routed** (Opus): `<universal-slides engine="rust">` runs every edit, undo/redo and `savePptx()` (fonts
  included) in the Rust `Editor` (`uo-wasm/src/slides_edit.rs`, schema codec `core/schema/slides_edit.uos`), in
  `uo-slides-editor.wasm` (1,429,008 B gzip, budget 3.5 MB) sharing ONE deck with the viewer's painter (stills,
  thumbnails, inline-edit slide, present/play). Parity: ABI replay 8,696/8,696 steps (53,106 facets); Chromium TS vs Rust
  element 60 decks × 2,400 steps: package, UI state, slide pixels, main canvas 2,460/2,460 each, `savePptx` 360/360.
  M4 10,000 editors 0 B; M5 100 undo steps. B8-1 kept (failed op restores the snapshot = what a TS user sees). Still on
  TS (5, nothing disabled): the TS model re-read from the mirrored package serves hit tests/selection/getters/findText/
  a11y; `newPresentation()` blank deck (sync API); `insertImage` size in the browser; `.ppt` converted twice (equal
  parts); image decode/font resolution/measure by design.
- **Viewer without the TS model** (Opus): Rust read-model exports (`uo_slides_{outline,links,assets,media_show,
  media_timeline,fonts,typefaces,show_settings}`); `engine="rust"` opens a .pptx with the TS model never built; the
  Rust show/timeline drive `present()`/`play()` and the media player. Chromium CI form: 213/213 decks, 0 TS model
  loads, 959/959 slides pixel-equal, a11y text equal, 920,640 link hit-test points equal, 7,221/7,221 animated frames,
  media requests equal. Open cost (12 largest decks): JS heap 31.2 MB (TS) → 27.1 MB incl. the 14.35 MB module;
  `loadPptx` 831 → 532 ms. TS model still built only for a `loadPptx` before the module is ready, any `.ppt`, after a
  trap, or back on `ts`. `play()` in rust mode returns per-slide times only (CHANGELOG). Found + fixed a B5b gap: no
  `recipe` hook in the browser engine (every EMF/WMF/PICT/TIFF dropped in rust mode); now b5b 659/682 slides
  pixel-equal, **23 differ inside metafile rasters (closed in wave 4b)**. MetaDecode prepare phase via `createImageBitmap` +
  `uo_slides_answer` (a successful decode is not covered by the corpus). Viewer wasm 1,266,010 B gzip (79%).
- **PDF links**: `uo_pptx::links` (run boxes bit-equal to the TS `hitTestHyperlink`, 945/945), `Emission.links`;
  538 decks: 994 links placed, `hyperlink:unplaced` 690 → 0 (2,089 off-slide fragments counted as off-page).
- **PowerPoint-PDF oracle** (M4 prep, `uo-pptx/tests/oracle_ppt`, `slides/tools/ppt-lab/pdf-oracle*`; cached PowerPoint
  PDFs in `core/target/w4-oracle/ppt-oracle`, outside git): 139 decks / 736 pages (PowerPoint opens only 15% of the
  generated decks). Samples (601 pages): pixel agreement mean 0.9947 (Rust, installed fonts) vs 0.9905 TS×PowerPoint.
  Ranked differences: (1) gradient-filled text as `7 Tr` text clip + `sh` — pdfium/Chrome ignores the clip and floods
  the page (40% of differing px; Rust-only bug); (2) `a:sysClr` uses lastClr, PowerPoint the system colour; (3)
  first-line baseline per font (Aptos 0.037 em lower); (4) fallback fonts for CJK/Thai/Arabic (PowerPoint: Yu Gothic,
  Malgun…); (5) WordArt warp geometry; (6) date fields in the system locale; (7) chart insets; (8) `.ppt` charts as
  fallback pictures; hidden slides included (PowerPoint omits them). 2–8 are TS legacy (Office decides → measure,
  then `ts_*` switches as in Write).
- Orchestrator: three viewer hooks for the editor (`loadRustEngine`, `rustAdopted`, type export); digests re-blessed.
- **Next**: wave 4b = model memory compaction (`SlideModel` 0.95× TS → 0.5×, `Shape` 408 B → ≤ 256) + PDF-only fixes
  (gradient text as a shading pattern, SVG) + the 23 b5b metafile diffs; wave 5 = PowerPoint rules (sysClr, baseline,
  fallback fonts, date locale, hidden slides in PDF, warp, chart insets) measured and switched like Write's `ts_*`;
  then the M4 gate (real Safari = maintainer).

## Update 2026-11-17 (wave 3) — Slides B4b (animations in the Rust viewer), PDF effects + font fallback, .NET facade

- **B4b** (Opus): playback lists with `SlotBegin/End` (shape, background, paragraph, unit, unit gate, chart part,
  bounds pass, media frame + poster), `emit_shape_anim` re-emission, `uo-pptx::stage` (template at emission, per-frame
  `stage_frame`), wasm exports `uo_slides_stage_{show,timeline,clear}`; `<universal-slides-viewer engine="rust">` paints
  `present()`/`play()` frames from the Rust stage (the TS show/timeline still drive events and the media player,
  hybrid). Parity: still captures unchanged (samples 1,202, b1a 246, c1d 182, b5b 162); animated draw-ops anim-b6a
  472/472, anim-samples 551/551, anim-b1a 150/150 (full local sets 1,015 and 1,462); Chromium animated frames
  **7,221/7,221** pixel-equal, still slides 959/959. M3 busiest frame 28,276 B (budget 64 KiB). Wasm 1,235,854 B gzip
  (77% of 1.6 MB). Disabled: 0; four declared deviations in `uo-pptx.md` "B4b — playback emission".
- **Slides PDF wave 2** (Opus): raster interpreter at 144 dpi (`uo_pptx::pdf::raster`, generic parts in
  `uo_pdf::raster`), shadows as soft-masked images under vector shapes, soft edges/reflections/blur/bevel/XOR layers
  rasterized, WordArt warps as vector outlines, per-character font fallback (measure = draw; B3 logs still 456,505/0).
  Counters over 538 decks: shadow 10,306→0, blur/dest-in/dest-out/dropped layers →0, warp 227→0, bevel 89→0,
  missing glyphs 327,000→299,280 with open fonts (48,637 with system fonts). Write PDF bytes unchanged.
  `PdfOptions::cancel` (per slide and per raster effect). Remaining: 690 unplaced links, 5 metafile XORs, 5 bevel
  underlays without JPEG, 1 SVG, 1 singular transform (`uo-pdf.md`).
- **.NET facade `UniversalOffice.Pptx`** (Sonnet): `uo-ffi/src/slides.rs` (`uo_deck_*`, `uo_pptx_to_pdf`,
  `uo_ppt_to_pptx`), C# `core/dotnet/UniversalOffice.Pptx` (+ Tests, Smoke) reusing the Docx handles; C# PDF = native
  93/93 decks; M4 10,000 handles 0 B left; 98 hostile inputs refused, 0 crashes. Old C#-only API mapping in
  `uo-bindings.md` "Slides .NET facade" (the old project is untouched; the switch is a later step). Not covered: editor
  session ABI, read-model ABI, Markdown/HTML export, typed container-limit code.
- Orchestrator: the facade's PDF cancel wired to `PdfOptions::cancel`; **`UniversalOffice.Docx.Tests` had been failing
  2/43 since 8f53340f** (the facade could not ask for `ts_strict`): `JOB_TS_STRICT = 8` + internal
  `LayoutOptions.TsStrict` → 43/43; `slides-native.tsv` re-blessed after the PDF changes.
- **Next**: editor routing (`engine="rust"` in `<universal-slides>`: `Editor` exports, `Op` codec, clips, fonts) +
  editor harness; PDF hyperlinks (`Emission.links`); drop the TS model in rust mode (read-model queries for a11y,
  links, media; then the media player off the TS `PlaybackSlide`); memory compaction (`SlideModel` 0.95× TS, target
  0.5×); `MetaDecode` prepare phase; M4 gate (PowerPoint-PDF oracle; real Safari = maintainer).

## Update 2026-11-17 (later) — Slides wave 2: B4, B5b, B6, B7 (static), B8 core, uo-charts C-0…C-6, R-S5

Records have every number; summary (all parity against the frozen TS 0.9.2 in `ts-ref-092`):
- **B4** render → Slides display list (`uo-dml-render/src/emit/`, `uo-pptx/src/emit.rs`): draw-op streams equal through
  `paint-slides.ts` — samples **1,202/1,202** views (after C-6), b1a 246/246, b5a 440/440, b5b 162/162, c1d 182/182.
- **B5b** metafiles EMF/WMF/EMF+, PICT, Slides TIFF, EOT/MTX: 1,267/1,267 rasterizations. DESIGN CHANGE vs the brief:
  a metafile/PICT/TIFF is a *recipe* bitmap (recorded canvas ops replayed by `uo-wasm/js/meta-player.ts`), not a
  display sub-list. Open: colour effects over a metafile picture are not applied (`Derived` needs a media base);
  EMF+ compressed bitmaps (`MetaDecode`) answered "cannot decode" in the viewer (needs an async prepare phase).
- **B6** timing/animation/playback/show/media: every facet equal (model incl. timing now replayed by B1's replay,
  timelines, states, ~94k anim-hook calls); 9 of 5,220 anim formulas differ only by declared hostile-input bounds.
- **uo-charts** (new crate, Opus architecture + C-0…C-6): 16,213/16,236 facets equal on 600 generated charts (23
  accepted by name = the approved `Math.pow` G1 platform rule), samples 1,140/1,140, differential 2,757/2,760 (3 = G1);
  charts paint inside slides through the Slides list (schema: `ChartFont`, `FillText`, `RoundRect`, `TileDef.nat`).
  Decisions CH-1…CH-6 applied with the recommended option (CH-4: no prefix canonicalization for Slides = TS).
- **B7** `<universal-slides-viewer engine="rust">` + `uo-slides.wasm` (1.05 MB gzip of the 1.6 MB budget; `.ppt` in a
  lazily loaded `uo-slides-ppt.wasm`, 280 KB gzip): Chromium **938/938 slides pixel-equal** (213 decks, charts by Rust),
  M4 handles exact, memory flat over 200 slide changes. Open: animations/`present()`/`play()` still on the TS path until
  **B4b** (`SlotBegin/End` + paragraph/unit/media slots + re-emission; the B6 handles are exported); the TS model stays
  loaded in rust mode (a11y, links, media); no wasm2js for Slides; real Safari not run.
- **B8** editor session core (`uo-pptx/src/edit*`): 8,696/8,696 steps equal (ops, undo/redo, saves with fonts, blank
  decks byte-equal); `SetRunText` (C#) ported. Not yet routed: the editor element → wasm (needs `Editor` exports in the
  viewer's instance, an `Op` codec, clip serialization, the font protocol; `commitEditShape` = `Op::Typing`). Decision
  to review: a failed op restores the snapshot (TS keeps partial changes).
- **R-S5** Slides PDF (`uo_pptx::pdf`, feature `pdf`): 538 decks / 1,357 pages valid; the `uo-fonts` measure filler is
  bit-equal to B3's logs (456,505 measures); Write PDF unchanged. Missing (counted in `PdfReport`): shadows, blur, warp
  (WordArt text absent), bevel, destination-in/out (reflections), XOR; 686 hyperlinks unplaced (needs `Emission.links`);
  no font fallback for glyphs missing in the face (≈ 327k chars over the generated decks: CJK/Arabic/emoji on Latin
  fonts). PowerPoint-PDF oracle not built yet.
- Verification fixes by the orchestrator: float casts in `uo-fonts/src/mtx.rs` and `uo-image/src/tiff_slides.rs`
  through `uo_jsnum::conv`; the CI tsc line was failing on `editor.check.ts` since 8f53340f (missing
  `write-session-{completion,format}.d.ts`) — fixed, plus `meta-player.ts`.
- **Next**: B4b (slots) → viewer animations; editor routing (`engine="rust"` in `<universal-slides>`) + editor harness;
  PDF effects + font fallback + links; memory compaction (`SlideModel` 0.95× TS, target 0.5×; `Shape` 408 B); .NET
  facade `UniversalOffice.Pptx`; M4 gate (PowerPoint agreement, real Safari).

## Update 2026-11-17 — Slides port (track B of `slides-sheet-port-plan.md`) started: phase 0, B1, B2, B3, B5a

The maintainer said "Vamos continuar a migração para rust, faça o slides". Track B of `core/docs/slides-sheet-port-plan.md`
runs on autopilot (memory `rust-core-autopilot`), at most 3 agents at once.

- **Preconditions (§0)**: frozen TS reference for Slides/Sheet = worktree `.claude/worktrees/ts-ref-092` (detached at
  `b92b60fa` = 0.9.2, `dist` built for write/slides/sheet; its `slides/node_modules/@universal-office/slides-viewer`
  junction was added by hand — the main checkout still has the stale `presentation-viewer` link). Bundle backup of
  `rust-core` (before this wave) in `E:\Backups\rust-core-2026-11-17.bundle`. Old target caches cleared (63 GB free).
- **§13 A decisions applied with the plan's recommended option** (the maintainer may revisit at the M4 gate): T1 text
  parity = advance tables + `TextRun`; T2 font policy = keep the TS Slides rule (installed-by-name drawn by the
  browser); P1 PDF = §9 M4 scope; corpus = generated decks + committed samples (no downloads). R1/C1 are calc (track A).
- **Done** (records have the numbers): architecture pass (Opus) → `uo-pptx.md`, `uo-dml-render.md`, uo-render
  "Slides extensions — design", plan §14 (11 corrections: B5 split into B5a/B5b, text layout per scale, warp stays in
  the shell for the viewer…); capture harness + seeded deck generator (`core/crates/uo-pptx/js/`, groups `b1a`, `b3a`,
  `b5a`; `uo-pptx-captures.md`); `uo-numfmt` (0 mismatches over 162,606 facets, Excel 1,709/1,709 — the Opus calc-consumer
  verifier is still pending, track A); R-S1/R-S2 Slides display list + `slots` codec (Write bytes unchanged, pinned hash);
  R-S3/R-S4 TS interpreter `paint-slides.ts` + moved effect code + draw-op recorder (captures of TS op streams over the
  samples and 100 `b1a` decks; the Rust-vs-TS comparison waits for B4); B1 read path (model JSON 100%: b1a 1,500, samples
  472 facets); B2 `.ppt` converter (1,082/1,082 parts byte-equal; `a:fld` counter per conversion; capture date frozen,
  `@today`); B3 text layout (Opus; 12,396 layout calls equal; `uo_text::grapheme` from Node 24.15 ICU 78.2 / Unicode 17);
  B5a SmartArt (150 generated decks, 970 facets equal). Percent of Slides by TS lines moved: ≈ 47%.
- **Next, in order** (record `uo-pptx.md` "Frontiers"): B4 render → display list (Sonnet on the done architecture; the
  text side needs box geometry first, batched `need_measures`, `PlacedRun` → `TextRun`; schema gaps `BevelEffect.dir`,
  `WarpPaint`/`WarpTile` params; interpreter rules chosen by R-S3: `fallbackEnd` = end of the else block, `SlotBegin/End`
  = save/restore), then the draw-op comparison and Chromium pixels; B5b metafiles/PICT/TIFF/EOT; `uo-charts`; B6
  timing/playback; B7 viewer `engine="rust"`; R-S5 PDF; B8 editor; .NET facade. Memory follow-ups: retained `SlideModel`
  0.95× the TS (target 0.5×), `size_of::<Shape>()` 408 B (budget 256) — compaction plan in the records.
- **Toolchain moved to Rust 1.100** (2026-11-09 build): its std case tables are newer than V8's Unicode 17 (21 code
  points); `text_v8.rs` now reports those instead of failing, and rustfmt 1.100 reformatted a few untouched test files.

## Update 2026-11-14 — WordCraft study adopted (leva `wordcraft-adopt`, after wave 7 + 7c)

The maintainer asked to study `D:\Projects\wordcraft` (another Word-like engine, MIT/Apache): find divergences and adopt
WordCraft's side where the MEASURED Word agrees with it, compare hyphenation and fonts, and list Word features we lack.
Study data (outside git): `core/target/wordcraft-study/` (`QUEUE.md` the divergences, `VERDICTS.json` ~200 cases measured
in Word 365 pt-BR, `PLAN-L/page/tables/fields.md` the 40 items to adopt, `FEATURES.md` the missing features — NOT to
implement, only reported). Most divergences: Word sides with us.

- Implemented (four areas, Opus implementers → Opus reviewers, up to 2 fix rounds; then a fields/lint cleanup and a
  harness-declarations pass): records "WordCraft study — lines / page / tables / fields / harness declarations" at the
  end of `docs/modules/uo-docx-layout.md`; CHANGELOG 0.10 "Write layout (Rust engine): rules measured in Word". Every
  rule sits behind a `ts_*` switch (`ts_strict()`; wasm bits 18 LINES, 19 PAGE, 20 TABLES, 21 FIELDS; JS `tsWc*`);
  session switches `OpenOptions::ts_formula_errors`/`ts_numbering`. Native Word tests `tests/word_wc_{lines,page,tables,
  fields}.rs` over `tests/oracle_wc/<group>` (open fonts only; installed-font groups opt-in `UO_WC_INSTALLED`), uo-wasm
  `wc_{lines,tables,fields}_native.rs`. English hyphenation: new pattern set `en-us-word` (CC0 patterns + Moby
  exceptions, `core/tools/hyphen/gen_en_word.mjs`; the bindings' `hyphenationPatterns` takes the TS bit). F-1 (IME proxy
  after the bar grows) closed in `rust-engine.ts` (ResizeObserver).
- Verified on the final tree (`core/target/m2-verify-wc.sh` → `m2-verify-wc.log`, the wave-7 script plus uo-fonts,
  uo-ffi, `wc_*_native`, Word agreement, licences): lint, fmt, tsc, vitest, uo-docx 821, every native suite; strict
  replays ci 600, f7a 900, f7b 600, f7c 900, f7d 600 equal, f8 3,000 equal, oracle 2,742/2,742 documents (8,226 facets),
  0 mismatches; Word agreement on the 2,742-document oracle: 79 cases change, Word-equal 74 → 79, none worse; wasm API
  tests, `write_api` pins (f8__7__0/3/10 moved, commented); every browser harness 0 undeclared (declarations proved by
  all-TS runs, record "harness declarations"); build-pkg + pkg_smoke.
- Open (record sections list them): pt-BR DollarText (Word crashes), en/es UI texts unverified (only pt-BR Office),
  "CalibriLight"/"Segoe UI CE/Baltic" (no rule found), the invisible tight/through box wrap (tables, measured, not
  applied), L9's per-glyph shortfall, `= 5 \* CardText` in table formulas, `f7a__7__9` 11 pages in the editor vs 12 in
  the viewer under C1b (two Rust models lay it out differently — investigate), an intermittent completion paste image
  state (also at the TS rules), an intermittent `editor_switch_format` Web-view `present` left (TS 24 / Rust 48 px on
  f7a__7__0 or sections @50, seen in 2 of 5 full runs with the same wasm, never in `--mode view` alone: 3/3 equal; not
  declared; `front_review` once timed out opening a page, passed on rerun), `.NET` port of every rule (records list the needs). Machine note: the study's `snap`
  sandbox in `target/wc-tables` had junctions to the repo (removed with `fs.unlinkSync` before deleting).

## Update 2026-11-12 (later) — milestone 2, wave 6 committed: the Write editor switch is done

Records: `uo-bindings.md` "Wave 6 — C/L/P". **Queue item 9 is done; what is left of milestone 2 is the gate (item 10).**
- **C — clicks, keys, caret.** The big cause was hyphenation: `write.rs` refused pattern registration ("patterns in
  use") once any session held the registry, so the Rust editor never hyphenated (and a viewer on a page with an editor
  lost its patterns: `prepare()` swallowed the error, now logged). Copy-on-write registration fixed the wave-4 "caret x
  on mixed-size lines", End on a wrapped line and most of gate item 10. Also: the TS keydown order with an object
  selected, Ctrl+C/X through navigator.clipboard (CRLF as the TS, ED-16 closed), Ctrl+Up/Down, text boxes in groups and
  linked chains (enter by order index, additive uo-docx context change), S-G1 (sections sync after insert table), BUG-1,
  the harness's TS-only `setSelection` artefact. `editor_switch --mode parity` stride 1: 1 declared difference left
  (was 18). Open: gate-O5 (bold in cells laid out narrower, now reproducible), gate-C1 entries (f7b__7__8/10, f8__7__5:
  the TS-saved document differs in both engines the same way), gate-O3 (theme colour without a theme part), boxes in
  cells/boxes not covered by the harness.
- **L — typing latency** (real clock, same invocation): pages50 typing Rust 6.5–6.9 ms vs TS fast path 6.2–7.2 (tie on
  the median, p95 lower; target "under" not met — maintainer call), Enter/Backspace 66/64 vs 129/121; pages500 typing
  16 vs 24 ms, Enter 1,179 vs 1,254, Backspace 1,067 vs 1,258 (baseline 3.1 s per key). No `Layout` clone (shared),
  EditLines per page, band repaint (`js/band.ts`), memo for Enter, the `uo-jsnum` integer fast path (merge fix: exact-size
  allocation and no format flags). Bit-equal to a full layout. Open: `memory.buffer` grows while typing (pages500 ends
  near 0.6 GB, D-L7).
- **P — packaging, CI, docs.** `build-pkg` ships `chunks/uo-write-editor.wasm` (the session build) with notices and
  refuses test builds (S11); `pkg_smoke.mjs` checks the published package in Chromium (default path loads no wasm; module
  order rule); CI job `editor` (never run on GitHub); README "The Rust engine (preview)" and a CHANGELOG "Unreleased"
  entry (corrected at merge for C's results); `docs/publishing.md` gained the two module builds (merge).
  `publish-packages.yml` does not build the modules yet (maintainer).
- Verified at merge: everything of wave 5's list plus — **layout parity regenerated from the frozen TS and replayed
  strict: ci 200, f7a 300, f7b 200, f7c 300, f7d 200 (0 mismatches), f8 200 (3,000 equal), Word oracle 2,742/2,742
  (8,226 facets)**; uo-docx-layout release suite; uo-jsnum, uo-render; shell_caret_native; `build-pkg font-files
  document-viewer write` with `UO_REQUIRE_ENGINE_WASM=1` and `pkg_smoke` ok; the three front harnesses `--mode all` ok
  (objects needed a rerun after a Chromium timeout); viewer_switch ok. D: 44 GB free.
- **Next: the milestone-2 gate (maintainer):** CHANGELOG version and 0.10, the decisions in the wave records (ED lists,
  gate lists of S/O/R/C/L/P, the latency tie, `ops/styles/listing.rs` in uo-docx, Web/Draft/Outline views still
  disabled), first CI run on GitHub, `npm run docs:site` from the updated README.

## Update 2026-11-12 — milestone 2, wave 5 (the remaining editor families) committed

Three frontiers in parallel over per-front files (`engine-routes-<front>.ts`, `rust-engine-<front>.ts`, `rust-<front>-*.ts`,
`shell_<front>.rs`, `editor_switch_<front>.mjs`; records in `uo-bindings.md` "Milestone 2: the editor switch, wave 5"):
- **S — stories:** tables (cells, tableNavigate/moveCell, Tab adds a row, cell selection, the Table Design/Layout tabs),
  headers/footers (zones, bands, mode, options, page numbers), footnotes/endnotes, the caret inside a text box. The
  wave-4 ED-7 table/note/text-box entries of the main parity are equal now. Gate S-G1..: the empty paragraph after a new
  table has no line (core), text boxes in cells/boxes and in groups, linked boxes.
- **O — objects and the page:** pictures (insert via the two-phase jobs, handles, crop, format, compress, image paste),
  shapes/text boxes as objects, page setup/sections/page color/watermark/borders, Read view, the context menu. Web/Draft/
  Outline stay disabled; drags commit through SetLayout/SetRotation (ED-202); gate-O1..O5.
- **R — review and document:** comments, tracked changes, fields/TOC/captions/cross-references/bookmarks/hyperlinks,
  find/replace + navigation pane, spelling (shared Hunspell), the styles list/gallery/pane and style CRUD (new
  `uo-docx` `ops/styles/listing.rs`, ED-301, a maintainer point: model logic in uo-docx), clipboard buttons + Office
  clipboard pane, the Alt keys. R owns the schema additions.
- Verified at merge: lint; uo-schema check; strict tsc + editor/viewer tsc; editor vitest 329, viewer 25; three editor
  bundles; uo-docx 775; uo-wasm native session/shell/stories/objects/review; wasm API/hostile/replay (10 families,
  0 mismatches); shell/write API; `editor_switch --mode all` ok; the three front harnesses `--mode all` ok (memory flat);
  `viewer_switch --mode all` ok; `editor_switch --mode parity` at stride 1 ok after the merge widened 4 f7d entries
  (`ED-216; wave-6`: a click on a text box enters it in one engine and selects/keeps the caret in the other — the fronts
  had run stride 6 only). `rustfmt --check` still flags the pre-existing `uo-wasm/src/write.rs`.
- **Next: wave 6** — the text-box click rule (TS `hitTextbox` order; the 4 `wave-6` entries, f7b__7__10, S ED-102 groups,
  S-G3 linked boxes), gate-O4 Ctrl+Up/Down in the shell `onKey`, gate-O5 relayout when a face arrives, typing latency
  (no `Layout` clone, incremental EditLines, repaint only the changed region), the wave-4 parity causes (hit.rs mixed
  sizes, End on a wrapped line, BUG-1), D5 shipping `uo-write-editor.wasm`, CI jobs for the editor harnesses and vitest.

## Update 2026-11-11 (evening) — milestone 2, wave 4 (queue item 9, first cut) committed

- **E — the editor switch**, record `uo-bindings.md` "Milestone 2: the editor switch" (ED-1..ED-28, gate list 1–18):
  `<universal-write engine="rust">` is a TS shell over the wasm session (routing by instance shadowing, fail-closed
  table `engine-routes.ts`: 68 public methods routed, 26 shell, 293 disabled mutators = warned no-ops, 124 disabled
  queries; 57 bar commands routed, the rest visible and disabled). Taken by the session: open/new/save (.docx, with
  fonts, PDF), typing, Enter, Backspace/Delete, caret moves, mouse selection, undo/redo, character and paragraph
  formatting, lists, styles, keyboard clipboard (Ctrl+C/X/V, HTML paste, Paste Options), font embedding. NOT routed
  (disabled): tables, pictures, shapes/text boxes, headers/footers, notes, comments/tracked changes, fields, page setup,
  find, spelling, other views, clipboard without a clipboard event, the styles list, the context menu (ED-28). Without
  the attribute nothing of the engine is fetched and the TS editor is unchanged (editor suite 314 tests).
  Harness `tests/browser/editor_switch.mjs` (phase0, parity, memory, latency, keys, chrome): 74 fixtures × 12 steps,
  48 equal in every facet, 26 with one DECLARED entry each (0 undeclared, 0 page errors). Typing on 50 pages: Rust
  26 ms median vs the TS fast path 7 ms (clone of `Layout` + EditLines rebuild are gate items). Session wasm 3.13 MB
  gzip.
- **W — wave-2 follow-ups + CI**: exhaustive bind defaults, hostile query fuzz, stale-layout hit tests, `Locked`,
  font-save phase 1 kept, replay through the production open for d2a/f1/f2/f3/f5/f6/f7/f8/f9/f10 (f4 not in the
  wasm replay), fontUsage recapture with notes and text boxes, CI lines in `rust-core.yml` (session wasm tests and the
  replay families). `uo-docx-layout` `Layout`/`LayoutPage` gained `Clone`; `uo-docx` `days_from_civil` is `pub`.
- **Fixed at merge** (the last reviews' findings): `build:offline-demo`/`build:site-live` and `build-pkg` did not
  resolve the engine imports (aliases added; the three editor bundles resolve); `paragraphBorderPreset` ran its TS
  body as a "query" (now a mutator, and a test fails on any disabled query whose body takes an undo step); a `load*()`
  before `uw-ready` left a TS model behind (reset, ED-27); context menu declared (ED-28); the f7 wasm replay accepted
  `enc` unread (now compared: 27,207 equal, 8 accepted, as native); a codec refusal fails the protected-session test.
- Verified at merge: check-changed green; uo-codec/uo-schema; strict tsc line + editor/viewer tsc; editor vitest 30
  files / 314, viewer 25 (+1 skipped); uo-docx 775; uo-docx-layout release suite (la7 13); uo-wasm session_native 11,
  shell_native 5; wasm session API, hostile, replay of the 10 families 0 mismatches; shell_api, write_api;
  `editor_switch --mode all` ok and `--mode parity` stride 1 ok (0 undeclared); `viewer_switch --mode all` ok.
  D: 50 GB free.
- Not done / known: `rustfmt --check` reports a pre-existing diff in `uo-wasm/src/write.rs` (untouched by this wave;
  the CI `cargo fmt --all --check` will flag it and other HEAD files); no CI job runs `editor_switch` or the editor
  vitest yet; `chunks/uo-write-editor.wasm` is not shipped by `build-pkg` (D5, gate item 18); CHANGELOG not edited.
- **Next:** the rest of the editor switch (route the disabled families, latency work, the parity causes in
  `hit.rs`/`caret.rs`) and then the milestone-2 gate (maintainer).

## Update 2026-11-11 (later) — milestone 2, wave 2 (queue items 7–8) committed

- **R — incremental relayout (LA7)**, record `uo-docx-layout.md` "Stage M2 — incremental relayout": level 2 (page
  checkpoints, one- and multi-pass) and level 1 (the paragraph memo, plain body paragraphs; no line memo, D-LA7-26;
  no header/footer, cell, note or textbox memo, D-LA7-18). `relayout()` keeps its signature and gives the same
  `Layout` bits as a full layout: 2,901 differential relayouts, 0 mismatches. An Enter still costs ≈ 50 % of a full
  layout. Gate list: "For the milestone-2 gate" items 1–8 (memory of the incremental state 3.5–4.1× the `Layout`).
- **WS — the session in wasm**, record `uo-bindings.md` (UoWriteSession): `uo-wasm` feature `session`, hot paths +
  hit-test/caret, exec/query through a schema-generated codec (`schema/session.uos`, 300 commands, 82 queries; the
  new `uo-schema` emitter `rust_bind.rs`), two-phase font save, `fontUsage()` port (100 documents, 631 equal).
  Replay through the wasm: d2a/f1/f2/f6 0 mismatches; f7/f8/f10, jobs and the shared ring are not replayed yet. Size:
  9.23 MB raw / 3.03 MB gzip with the session (viewer alone 1.41 MB). Accepted minors left: bind `default` structs
  are not exhaustive (57 lines), no hostile *query* fuzz, stale-layout hit tests, `Locked` not asserted, phase 1 of
  the font save lost on a malformed phase 2, the replay opens through a test export.
- **D3-ABI**, record `uo-docx-d3.md` §4/§12: 92 C exports (`uo-ffi` feature `editing-api`) and the C# facade
  (`UniversalOffice.Docx` Editing/Native/Core, `OpcPartName` included — clickCompliance calls it), 11 declared
  public-API differences from 0.9 (§4.5). Fixed at merge: CRLF-safe FrozenSuite patterns, explicit generated files,
  validate-before-mutate in two exports, counts.
- Verified at merge: check-changed green; uo-docx 775 tests; uo-docx-layout 25 binaries (la7 13); generated parity
  f7a–f7d 300 equal each, f8 1,500; Word oracle 2,742/2,742; `viewer_switch --mode all` 0 differences (pixels
  tolerance 0, PDF); uo-ffi editing tests; .NET 43/43, FrozenSuite 134/134 Debug and Release; uo-wasm session native
  8/8, API, hostile (1.17 M calls), replay. D: ended with 46 GB free; `core/target/m2-*` hold ≈ 36 GB of caches.
- CI not wired yet (`rust-core.yml`): a release `la7` line, the session wasm tests, the uo-ffi `editing-api` tests.

## Update 2026-11-11 — milestone 2, wave 1 (queue item 6) committed `4c003113`

- F4 remainder (watermark, cover pages, header/footer galleries), F8 clipboard (paste-tree boundary v1 with limits,
  Paste Special/Link, the ring priced by what a clip really keeps) and F10 (fields, TOC, table of figures, captions,
  cross-references, manual hyphenation, galleries, date/time, document info, alignment tab). Records
  `uo-docx-d2b/F4.md`, `F8.md`, `F10.md`; each has a "For the milestone-2 gate" list (new: the third oracle method
  `shown_page_numbers`, F10-A2 cross-reference with position, Spanish dates F10-D10, F8-D1/D2 boundary, F8-O3/O4
  behaviours of other families found by the clipboard scripts).
- Orchestrator: `uo_docx_layout::oracle::shown_page_numbers` (the TS `pageLabelOf`) — without it every TOC command
  failed over the real engine; tested end to end (`tests/f8.rs`).
- Verified: check-changed green, uo-docx 770 tests, uo-docx-layout suite. The disk D: filled up (0 bytes free)
  during the verification: the milestone-1 `core/target/agent-*` caches were removed (79 GB). Watch `df -h /d`
  before launching a wave; each front's target is 4–10 GB.

## Work in progress — commit `6471786d` (historical: finished, see the update above)

It compiles (`cargo check --workspace --all-features --tests`) but was **not verified**:

- **D2b F7 (drawing objects, pictures)**: `session/ops/drawing{,/}.rs`, `pictures.rs`, a Node capture
  (`js/capture-d2b-f7-ops.mjs`), `tests/parity_d2b_f7_ops.rs`, `tests/session_d2b_f7.rs`. No Chromium capture, no
  memory/no-panic tests, no `uo-docx-d2b/F7.md`. Known failures: `parity_d2b_f7_ops` ("12080 equal, 0 mismatches,
  12000 not replayed"); clippy `-D warnings` errors in `session/ops/pictures.rs` and `session/ops/drawing.rs`;
  `core/tests/grep-bans.sh` flags `tests/parity_d2b_f7_ops.rs:16` (an `f64` parse).
- **G8 — `Layout::encode_page` (the Write page section)**: `schema/write.uos` extended and regenerated,
  `uo-docx-layout` declares its codec, `section.rs`/`annotations.rs`, `tests/g8.rs`, `tools/lib/facets-g8.mjs`, the wasm
  `UoLayout` section export and `uo-wasm/tests/write_section.test.mjs`. Parity, sizes and the record sections not
  done. Known: clippy error at `uo-docx-layout/src/annotations.rs:127`; the layout `adapter` test reports "840 not
  replayed" (`p3.*` facets) — confirm whether that predates `6471786d`.
- Also seen at HEAD: clippy on wasm32 fails in `uo-fonts/tests/registry_nopanic.rs` (unresolved `uo_fonts::system`
  import, a native-only module used by a test); `cargo fmt --all --check` hits a Windows "file name too long" error
  (run `rustfmt` per file instead).

**First thing to do next:** finish or revert these two frontiers, then make the whole workspace green
(`clippy -D warnings` native + wasm32, `grep-bans.sh`, `float-casts.sh`, all suites).

## Queue (in order; model per role as in SKILL.md)

Milestone 1 (viewer and PDF in Rust):
1. Finish G8 (Opus — touches the layout crate).
2. uo-render **frontier D** (TS shell: decode pages, canvas painter on decoded pages) and **V** (painter parity in
   Chromium) — Sonnet per the uo-render plan.
3. Cancellation inside the layout (G4: `LayoutOptions.cancel`, `LayoutError::Cancelled` in `Budget::charge`).
4. Viewer TS/Rust switch behind `engine="rust"` (`uo-bindings.md` §2 "The TS/Rust switch of the viewer").
5. Gate work: security review + milestone skeptic (Opus), benchmarks (speed), real Safari 15 / iOS on BrowserStack
   (the MCP needs re-authorisation via `/mcp`), recapture the stale-stamped fixtures (F2-20), **first run of the CI
   on GitHub (never run)**, then the maintainer's milestone-1 gate.

Milestone 2 (editor in Rust):
6. ~~D2b~~ done (`4c003113`). Was: finish F7; F4 remainder (watermark, cover pages, header/footer galleries, `insertDateTime`,
   `insertDocumentInfo`, `insertAlignmentTab`); **F8 clipboard** (Opus for the paste-tree boundary); **F10** the layout
   family (fields, TOC, captions, manual hyphenation, galleries — needs F8 of the layout, done).
7. ~~Incremental relayout~~ done (wave 2).
8. ~~Editor bindings~~ done (wave 2): the session in wasm (`UoWriteSession`, session codec G15) and the D3 C ABI + C# facade.
9. ~~The editor switch~~ done (waves 4–6, `d4279c16` and the wave-6 commit).
10. Milestone-2 gate, `CHANGELOG.md` (API breaks: `uo-bindings.md` §4), docs, publish 0.10.

Later: Sheet (calc engine on Opus, `sheet-core`), Slides (`slides-core`), charts, the converter — 0% today.

## Decisions waiting for the maintainer

- **D2a** (record "D2a — session (architecture)", "For the milestone-2 gate"): (1) order-sensitive signatures in
  the session (D2a-U6) vs a per-run order tag in `write::sig`; (2) D2a-C2 — a third `LayoutOracle` method answering
  which header/footer part a page shows, or the shell passes it (F4 implemented the shell-argument variant,
  isolated); (3) notes with tables: TS flattened list vs story editing (F5-D1 kept story editing; no corpus note has
  a table).
- **Bindings G1–G22** (`uo-bindings.md` "For the milestone-1 gate"): notably G4 cancellation, G5 limits for untrusted
  documents, G7 size budget, G8 page section, G14 NuGet RID packing, **G22 `OptF64`'s signalling-NaN "absent" marker**
  (breaks any JS/wasm2js build) — **done 2026-11-10** as an integer encoding (approve), and the new **G23–G31** from
  the gate work.
- **Size** (`size-optimization.md`): D-1 six NaN-capable layout sorts on the JS core; D-2 `opt-level = "s"` for
  uo-docx in the wasm profile (−68 KB gzip, open +14%) or `"z"` (−156 KB, open +60%). Viewer today: 3.96 MB raw /
  1.405 MB gzip (TS start: 0.30 MB gzip).
- **Layout**: the peak of one layout call is ≈ 2× the retained layout (174 MiB at 500 pages), over the plan's target
  as written; M3 retained 45–99 B/char (target 48 on the 500-page document: met).
- **Published product (outside the port, `main` is frozen)**: D3 review S1 (a `javascript:` hyperlink survives the
  C# sanitizer) and S6 (the C# importer's recursion overflows the stack on hostile documents — kills a server
  process); the Slides viewer (`slides/packages/viewer/src/universal-slides-viewer.ts:1161`) and Sheet
  (`sheet/packages/sheet/src/universal-sheet.ts:1559`) open hyperlink targets without a scheme filter (Write filters).
  All in `docs/security.md`.
- CSP: confirmed on a real Safari 15.6.1 — `'wasm-unsafe-eval'` does not let it compile wasm, only `'unsafe-eval'`
  does; strict-CSP hosts on Safari 15 need the TS engine or the wasm2js build (decide what the viewer does there).

## Operating rules learned in this session (follow them)

- **Faster verification (2026-11-10)**: uo-docx's 160 integration-test files are ONE test binary (`tests/all.rs`,
  `autotests = false`; each file is a module, the support modules are declared once there and imported with
  `use crate::<name>;`). A brief's `--test <file>` for uo-docx is now `--test all <file>::` (older records still show
  the old form). New test file = add `mod <file>;` to `tests/all.rs`. Local lint gate: `node tools/check-changed.mjs
  [base]` (clippy native + wasm32 and the bans on the changed crates and their dependents); the whole-workspace clippy
  is for CI and milestone checks.
- **Machine load**: at most 2–3 agents at once; every brief demands `cargo -j 4`, tests with `--test-threads` ≤ 4,
  bounded no-panic/soak tests (seconds; big runs behind an env var), own `CARGO_TARGET_DIR` under `core/target/<name>`,
  and "only your own test targets". **Only the orchestrator runs the full suite**, once per commit
  (`cargo test -j 4 --release -p uo-docx --no-fail-fast --features canon,editing-api,doc,odf,export,compare,session -- --test-threads=3`).
  With 5–7 agents the machine froze (a `nopanic_d2b_f3` test).
- **Junctions**: agents have built sandboxes inside `core/target/` with junctions to `write/`, `shared/`…; briefs now
  forbid sandboxes, repo copies, junctions and symlinks. Before deleting anything under `target/`, check for links
  with Node (`fs.lstatSync(p).isSymbolicLink()`, two levels deep) — not `cmd dir /AL` (it prints "File Not Found",
  which counts as a line). Remove a junction with `fs.unlinkSync`. Never `rm -rf` through `node_modules`.
- **Commits**: one per verified wave on `rust-core`, no push; never `git stash` (the stack is shared with other
  worktrees). Shared files edited by two running agents (`uo-docx.md`, `session/mod.rs`, `engine/*.rs`, `lib.rs`) are
  committed together when both are done. The pre-commit hook checks fonts by content — never bypass it (a fixture
  with a 64-byte zero "font" was allow-listed in `tools/fonts/allowed-fonts.txt` with its explanation).
- **Verify agent reports** before committing: rerun their suites, clippy native + wasm32, `grep-bans.sh`; agents have
  left lint errors, `f64::clamp` (banned by `clippy.toml`), stray files (a 0-byte file named with a private-use
  character) and an empty capture script.
- **Tools**: edit `.rs`/`.ts` with the Edit tool (CRLF files; `sed -i` converts them); the Bash tool halves double
  backslashes (write scripts with regexes through the Write tool); Playwright 1.61 needs `UO_CHROMIUM` pointing at the
  installed `chromium-1248` (its own revision is not downloaded); binaryen 132 (`wasm-opt`, `wasm2js`) is installed
  in `core/target/tools/binaryen` (npm, outside every `package.json`).
- Size tools: `core/tools/size/` (`wasm-size.mjs`, `wasm-share.mjs` — gzip cost of a function set, `wasm-ab.mjs` —
  interleaved speed A/B with an output digest; separate-process timings are useless on this machine).
