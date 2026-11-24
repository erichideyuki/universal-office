# Slides and Sheet → Rust: migration plan (draft 2, 2026-11-14)

Status: DRAFT for the maintainer. Follows `.claude/skills/rust-port/SKILL.md` (hybrid mode, models per role, memory gate,
parity against the frozen TS) and continues `core/docs/HANDOFF.md` (Write: milestone 1 and 2 code done, gates pending).
Inputs: read-only inventories of `slides/`, `sheet/`, `shared/charts` and the Rust workspace (2026-11-14). Line counts are
non-blank, non-comment TS lines of the frozen `main` (0.9.2) unless marked "raw".

**Draft 2** folds in an independent review (Fable, 2026-11-14): §0 preconditions added; text parity redesigned (advance
tables + `TextRun`, §4.3) instead of swapping the measurer; an Opus architecture pass for the data models (§4.0); the
Slides/Sheet shape renderer moved to a shared crate (§4.5); PDF scoped per milestone with its own oracle gate (§9);
animation slots widened (§4.2); `cf-formula` ported 1:1 instead of merged; editors re-estimated; schedule rebuilt for
≤ 3 agents in total and a disk budget (§12); decisions split into "before phase 0" and "at the gates" (§13).

## 0. Preconditions (before any implementer starts)

1. **Back up and push decision**: `rust-core` has never left this disk (HANDOFF). A `git bundle` backup at least; the
   push (and one green `rust-core.yml` run, 751 lines, never executed) is the maintainer's call — strongly recommended
   before starting, because this plan adds CI jobs per milestone and none has ever been proven on GitHub.
2. **The Slides and Sheet real-document corpora are gone** (maintainer, 2026-11-14: deleted). Lost: `E:\UniversalSlidesTest`
   (1,445 real decks, the chart lab `_lab/syn` with `gen-charts.cjs`, the triage/PowerPoint scripts) and
   `E:\UniversalSheetTest` (2,175 real workbooks incl. the 237-workbook calc corpus, the `_lab/calc/core` harness
   `xl-corpus.mjs`/`corpus-calc.mjs`/`incr-check.mjs`, the locale capture data). Kept in git: their results
   (`slides/tools/ppt-lab/CORPUS-REPORT.md`, `sheet/tools/excel-lab/CORPUS-REPORT.md`), the calc oracle bank (71 files,
   35,299 Excel-measured cases), `numfmt-oracle.json` (1,709), `slides/samples` (53 files incl. `legacy/` and `playback/`),
   `sheet/samples` (7 workbooks incl. `giant-1m.xlsx`/`giant-200k.xlsx`), the generators and COM scripts of
   `slides/tools/ppt-lab` (`gen-stress*.ps1`, `export-deck.ps1`, `gen-reference.ps1`, `model-regress.mjs`, playback lab)
   and `sheet/tools/excel-lab` (`gen-oracle-xlsx.mjs`, `gen-smoke-xlsx.mjs`, `gen-giant.mjs`, `oracle-format.ps1`,
   `excel-screenshot.ps1`), and `sheet/packages/calc/tools` (`oracle.mjs`, `xl-calc.ps1`, `locale/*`).
   **Replacement in this plan** (the Write port's model, which also gated on generated documents):
   - parity = **differential generated documents** (decks and workbooks produced by seeded generators, like Write's
     `capture-layout.mjs --gen f7a…f8`), captured from the frozen TS and replayed strict, plus every committed sample;
   - Office agreement = **synthetic oracle groups** rebuilt by the committed COM scripts (PowerPoint PNG/PDF/CreateVideo,
     Excel `Range.Text`/`Chart.Export`/`xl-calc`), versioned as Write's `oracle_wc` groups (open fonts only);
   - calc: the 35,299-case bank (bit-exact vs TS, 15 digits vs Excel) + generated workbooks recalculated by Excel through
     `xl-calc.ps1` (replaces the 237-workbook gate; the incremental-vs-full check runs on the generated workbooks);
   - a new real-document corpus is OPTIONAL and the maintainer's call (it needs downloads); without it the "unknown
     real-world feature" risk is covered only by the generators and fuzzing — stated in each gate.
3. **Pin the TS reference** for Slides and Sheet: a frozen checkout with `dist` built (Write uses
   `clickcompliance-docx` at `d0f45bc5`, not an ancestor of `main`; name the commit used for Slides/Sheet — the 0.9.2
   `main` head is the natural choice) and its capability set (Chromium version, `ctx.filter`, conic gradients,
   `letterSpacing`, `fontKerning`).
4. **Decisions needed before day 1** (§13 list A): text parity design, regex engine, locale scope, PDF scope.
5. Disk: ≥ 40 GB free (57 GB on 2026-11-14); old `core/target/*` caches cleared per the HANDOFF rules.

## 1. Scope and size

| Product part | TS lines | Moves to Rust | Stays TS (shell) |
|---|---|---|---|
| `shared/charts` | 5,153 (+514 tests) | all: parse, scale, trend, chartex, render → display list; `numfmt.ts` (1,128) as its own crate | — |
| `slides-core` | 19,250 | all: package/XML, reader + model, theme/text styles, `.ppt` (5.3k raw), render (→ display list), text layout, text warp, SmartArt, metafiles (EMF/WMF/EMF+), PICT, TIFF, EOT reader, timing/animation/playback/slideshow state, export model | — |
| `slides` (editor) | 5,173 | `deck-edit.ts` (46 exports), `xml-edit.ts`, `history.ts`, `blank-deck.ts` (~1.7k) | element, chrome, inline text overlay, dialogs |
| `slides viewer` | 1,463 | — | all (shell over the wasm) |
| `calc` | 15,948 | all (535 functions, engine, i18n pt-BR/en-US, open/save rules) | — |
| `sheet-core` | 9,048 | all: reader/writer (surgical), styles/theme, metrics, CF/pivot reading, drawings/charts data, `.xls` BIFF (4.4k), exporters csv/ods/xml2003 | — |
| `sheet` (element) | 10,013 | ~5k: cell text/entry/edit text, conditional formatting incl. `cf-formula.ts` (ported 1:1), table/pivot styles, grid geometry, border/fill decisions, drawing anchors, the calc bridge, edit operations + undo (today private methods of the 4,592-line raw `universal-sheet.ts`) | events, scroll, autocomplete UI (fed by the Rust function catalogue), chrome, canvas calls |
| .NET `UniversalOffice.Pptx` / `.Xlsx` | 2,610 / 3,456 C# | become C# facades over the C ABI (as `UniversalOffice.Docx`) | — |

Total ≈ 56k TS lines (Write: ≈ 53k TS + 4k C#-only in 13 calendar days from foundation to milestone-2 code, under the
same agent cap) — but here the renderer must be restructured (canvas → display list), not translated.

## 2. What is reused (already in Rust)

- `uo-opc` (zip, XML, OPC, limits, **CFB**: Slides' `ppt/cfb.ts` and Sheet's `xls/cfb.ts` differ from Write's in 4 and 7
  lines), `uo-alloc`, `uo-testkit`, `uo-codec`/`uo-schema`, `uo-image` (PNG/GIF/BMP/TIFF), `uo-text` (JS-exact NFC/NFD/
  NFKD, `\p{}`, full case mapping, UTF-16, **the calc `collation` already ported 1:1**), `uo-jsnum` (V8-exact Math,
  fdlibm, number ↔ string, stable sort), `uo-fonts` (sfnt, cmap, GPOS/GSUB, subset, odttf, registry, policies),
  `uo-drawingml` (187 presets, guides, arcToSeg, light3d, patterns, `odraw` with the Slides/Sheet `Flavor`), `uo-pdf`
  (writer), `uo-render` (display list + TS painter), bindings patterns, harnesses, ~35 app-neutral fuzz targets, tools.
- Separate algorithms in the TS, ported as their own variants and reconciled later by the oracle: Slides' EMF/WMF/EMF+
  (plays EMF+, ignores the GDI fallback — Write the opposite), Slides' TIFF, Slides' HSL colour modifiers (`theme.ts`),
  Slides' canvas-native effects. Missing in Rust: EOT reader + MicroType Express, PICT, SVG on the server/PDF path.
  No JPEG decoder needed (images decoded by the shell; PDF embeds JPEG as DCT).

## 3. Strategy

1. **Three tracks, one implementer each** (A calc, B Slides, C Sheet) — at most 3 agents running in total, verifiers
   included; sub-frontiers run serially inside a track; one `CARGO_TARGET_DIR` per track. Track C's grid does NOT wait
   for Slides: its shapes come from the shared renderer of §4.5, built in track B first.
2. **Parity against the real TS, including its text.** The TS measures and paints text with the browser (`measureText`,
   `fillText(string)` with a letter-spacing fit, CSS font stacks, fonts installed by name). Rust layout takes the
   measurements as INPUT (advance tables, §4.3) and emits `TextRun` items the canvas painter draws exactly as the TS;
   the PDF/server path draws them as glyphs with `uo-fonts`. No product behaviour changes for parity's sake.
3. **One crossing per frame / per viewport.** Slides: static display list per slide and scale + per-frame slot table;
   Sheet: one display list per viewport.
4. **Pixel-reading effects stay in the TS painter** for the viewer at first (`Effect` items with their parameters, the
   same JS code) — the PDF/.NET path lists them as missing until they are ported and measured (§9).
5. **Port the TS's own quirks; reconcile later with the oracle.** Behaviour changes are listed items behind `ts_*`
   switches, as in Write (Office decides).

## 4. Phase 0 (shared)

### 4.0 Architecture pass (Opus, half a day, before any port)
- One cell store for Sheet (today the sheet-core model and the calc `engine.ts` `cells` Map are bridged by
  `sheet-calc.ts`, 853 lines: a 1:1 port doubles memory on `giant-1m.xlsx`), the style/XF pool, interning, ordered maps
  wherever the TS relies on `Map` insertion order, the `u64` cell key.
- The Slides model: master/layout/placeholder graph, the text cascade, the package-part session (byte parts + snapshot
  undo), per-slide re-read after edits.
- The display-list extensions (§4.2) and the shared shape renderer boundary (§4.5).
- Output: the data-model sections of the module records; this is what makes the later Sonnet frontiers "closed scope".

### 4.1 `uo-numfmt` (new crate, from `shared/charts/src/numfmt.ts`)
Feeds calc `TEXT`, Sheet cell display and chart labels. Exact capture: the 1,709 Excel cases + a differential generator.
Sonnet port, **Opus verifier** (it feeds the calc). 0.5 day.

### 4.2 `uo-render` extensions (Opus architecture, Sonnet port)
- path clips (nonzero/evenodd), full affine transforms, layer groups (alpha, composite and blend modes), mask layers;
  linear/radial/conic gradients, transformed patterns, generated-bitmap resources;
- `TextRun` (string, CSS font, origin, laid width, kerning flag, letter spacing; the PDF painter draws it as glyphs);
- `Effect` items executed by the shell (shadow/blur, bevel, recolour, raster op, warp strips);
- animation slots at SHAPE granularity: per shape/paragraph/word/letter/background/chart part the slot table carries
  opacity, transform, reveal clip and colour (fill, line, text); a shape whose text style or size is animated
  (`PartState`: `w/h`, `fontWeight/Style`, `underline`, `fontScale`, `fontFamily`, `filters`) gets its sub-list
  re-emitted by Rust for that frame (only that shape crosses);
- display lists are emitted per scale (the TS sizes layers, patterns and path-gradient bitmaps by scale), with the
  scale-dependent decisions listed;
- media slots (video/GIF frame, cover), selection outlines; `GeomEval::Drawingml` implemented.
Write unchanged (twips stay its unit). 2 days.

### 4.3 Capture harnesses
- **Slides draw-op stream (primary gate)**: a recording context over `paintSlideTo` turned into a canonical op stream
  (device-space geometry, resolved paint, alpha, clip, composite), compared in Node with the stream the TS painter gives
  for the Rust display list. **Chromium pixels (tolerance 0 where deterministic) are the second gate.** The capture pins
  the TS capability set (§0.3).
- **Advance tables**: the capture records, from the real TS run, every measurement the layout asked for (per font and
  character the advance at 1000 px rounded to 1/8 pt, kerning deltas, whole-string widths for complex scripts and
  `actualBoundingBox`/`fontBoundingBox` used by text warp). Native replay feeds the same tables → parity against the
  true TS without proprietary font files in CI. In the product, the shell fills the table in two phases (as Write's
  jobs); the server/PDF fills it from `uo-fonts`.
- **Slides model/layout**: `Slide` model JSON, `layoutTextBody` results, `.ppt` → `.pptx` XML bytes (`model-regress.mjs`
  hashes), SmartArt `dsp:spTree` text, timing models and playback timelines.
- **Calc**: bit-exact TS outputs of the 35,299 cases (777 known divergences, 47 Excel errors) — committed; per-cell
  values and the incremental check over GENERATED workbooks (seeded: formulas from the bank's functions, cross-sheet
  references, names, tables, spills, volatiles, cycles), each also recalculated by Excel through `xl-calc.ps1` (§0.2).
  Two extra capture sources (taken from `rust-anything`'s `record`, 2026-11-19; the tool itself is not adopted — our
  harness already does its record/diff/gate/shadow and its equivalence target is the TS only, not Office):
  - **Per-parameter traps** (`uo-testkit` generator `calc-traps`, seeded): for every function of the catalogue and
    every parameter, the values where JS and Rust semantics part — `NaN`, `-0`, `±Infinity`, `1e21`/`1e-7` (the
    `Number→String` switch points), 2^53±1, 15/16/17-digit numbers, denormals, empty string, lone surrogates, astral
    and combining characters, non-ASCII `\w`/`\b`/case-folding text, each error value as an argument, empty/1×1/
    ragged/whole-column ranges, booleans and text-numbers in pt-BR and en-US. Captured from the frozen TS, replayed
    strict; a CI subset and a large local run. The interesting cases (TS ≠ Excel) go to `xl-calc.ps1` in one batch.
  - **Calls of the TS test suite**: run `sheet/packages/calc/test/*.test.ts` (19 files) against the frozen TS with
    the engine's public entry points (`evaluate`, recalc, parse/print, function `apply`) wrapped by a recorder, and
    keep every call + result as a capture group (`calc-tests`). Cheap coverage of the cases someone already thought
    about; re-run when the frozen reference changes (the usual stale stamp).
- **Generators (new, day 1–3)**: a seeded deck generator (shapes × presets × fills × effects × text bodies × tables ×
  charts × SmartArt × animations; `.ppt` through PowerPoint COM `SaveAs` from generated `.pptx`, reusing `gen-stress*.ps1`)
  and a seeded workbook generator (styles, number formats, CF, merges, frozen panes, drawings, charts, `.xls` through
  Excel COM), each with a CI subset and a large local run, as Write's `--gen` groups.
- **Sheet**: model JSON (xlsx/xls), save bytes, cell display text, the viewport op stream over `paintRegion`;
  editor scripts driven in Chromium (the edit logic has no capture points today: an extraction step, §7 C5).
  Added by the Sheet architecture pass (§16): model JSON with sorted keys (the §14 rule), a per-cell **store facet**
  (file kind, the value text the save would write, style, formula and attributes) compared instead of the model's
  `rows`, and an **evaluation-order facet** for the calc (cells in evaluation order per run, `circular` sorted) —
  the TS `Map`/`Set` order sites that are behaviour are listed in `uo-calc.md` S7; `RandomSource::Sequence` and a
  fixed `Clock` in every capture.
- Versioned fixtures use open fonts only; installed-font groups are opt-in (as Write's `UO_WC_INSTALLED`).
1.5 days.

### 4.4 Smaller shared pieces
- `uo-fonts`: EOT reader + MicroType Express (`eot.ts`), `sfntStyle`/`looksLikeSfnt`/symbol fonts.
- `uo-image`: Slides' TIFF variant; PICT (Slides module); SVG for the server/PDF path (scope decision).
- Calc regex: `regress` (ECMAScript semantics, so the TS's PCRE2 → JS translation ports 1:1) WITH a step budget (a
  hostile pattern must not hang the tab or the server) and a Unicode-version check against Node 24.15 (decision R1).
- `uo-pdf`: `PdfProfile` (finding S9: `render_pdf` hard-wires Word rules).

### 4.5 Shared shape renderer (new crate, e.g. `uo-dml-render`)
Sheet's drawings use the Slides shape reader, text cascade, text layout, painter, SmartArt and metafiles
(`sheet/src/drawings.ts:14-19`). Under the reuse rule these go to a shared crate (shape model, text body layout,
shape/picture/effect → display list, metafile, SmartArt); `uo-pptx` keeps the presentation (slides, masters,
timing, `.ppt`); `.ppt` and `.xls` are features (bytes → bytes converters, candidates for a lazily loaded module).

## 5. Track A — `uo-numfmt` → `uo-calc` (Opus for every role)

Serial frontiers: A1 values/coerce/collate/criteria/dates (host timezone)/stats/special functions/`libm.ts` double-double;
A2 parse/print/a1 with a parse-depth limit and an explicit stack, evaluate/apply/ops/fn registry, evaluation depth
measured in wasm (stack per level, as Write's G23); A3 engine (graph, full + incremental recalc, `NeedCell` as `Result`,
volatiles with injected `now`/`random`, cycles/iteration, `u64` key) on the §4.0 cell store; A4 functions by family
(math/libm, statistical/distributions/regression/ets, financial/daycount, text/text-format/regex/filterxml with an XPath
subset, lookup/reference/dynamic with `uo-jsnum` stable sorts, datetime, the rest); A5 implicit intersection (508
measured functions), i18n (pt-BR/en-US tables; `Intl.NumberFormat/DateTimeFormat/Locale` derivation and the
`Intl.Collator` list order at `i18n/translate.ts:686` per decision C1), open-recalc/save/file-form/addin, adapters,
the function catalogue for the shell's autocomplete.
Done: 35,299 cases bit-equal to the TS; the `calc-traps` and `calc-tests` groups (§4.3) bit-equal to the TS; the
generated workbooks equal to the TS per cell (and their Excel agreement not
lower than the TS's); incremental = full; M1–M5; fuzz (formula text,
hostile workbooks); clippy/bans. 4–5 days (numeric tails RATE/IRR/XIRR/YIELD, FORECAST.ETS, distribution inverses stay
equal to the TS; Excel differences stay "known").

## 6. Track B — Slides (+ the shared renderer and charts)

B1 `uo-pptx` read path (Sonnet on the §4.0 model): package/XML (own parser semantics), reader, theme (HSL profile), text
and table styles; capture: model JSON — 1 day. B2 `.ppt` (Sonnet) on `cfb` + `odraw`, same in-memory XML bytes as the TS
(`uo-jsnum` number formatting) — 1 day. B3 text layout + warp in `uo-dml-render` (Opus), fed by advance tables, laid out
once per slide (today on every paint and frame) — 1 day. B4 render → display list (Opus architecture, Sonnet port):
shapes, pictures, tables, effects as `Effect` items, 3D matrices, per scale — 2 days. B5 SmartArt, metafiles EMF/WMF/EMF+
and PICT (Slides variants), EOT — 1.5 days. `uo-charts` (Opus architecture, Sonnet port; text by advance tables;
oracles: the 12 PowerPoint-measured rule blocks, a rebuilt synthetic chart group (the lost `gen-charts.cjs`
lab redone from the rules in `charts.test.ts` and `CORPUS-REPORT.md`), Excel `Chart.Export`) — 1.5 days.
B6 timing/animation/playback/slideshow/media state → slot table (PowerPoint `CreateVideo` lab as oracle) — 1 day.
B7 `<universal-slides-viewer engine="rust">` + `uo-slides.wasm` + viewer harness (op stream, pixels, text, links, find,
memory, faults, Safari) + hyperlink scheme filter (Write's rule, behind a switch, listed) — 1.5 days.
B8 editor session (byte-part package, XML surgery with `xml-edit.ts` string-scan semantics, the `deck-edit.ts` operations
(46 exports), snapshot undo, font embedding on save, per-slide re-read; `PptxManipulate.SetRunText`), routing behind
`engine="rust"`, editor harness — 2–3 days.

## 7. Track C — Sheet

C1 `uo-xlsx` read/write (Sonnet on the §4.0 cell store): model, styles, theme, surgical serialize, save-calc, CF/pivot
reading, anchors, chart-data, metrics tables — 1.5 days. C2 `.xls` (Sonnet) to the same in-memory OOXML bytes — 1 day.
C3 grid model (Opus architecture): cell text via `uo-numfmt`, entry/entry-format, CF evaluation with **`cf-formula.ts`
ported 1:1** (its merge into the calc is evolution after M5, Excel-measured), table/pivot styles, geometry (prefix arrays,
frozen panes, hit testing), border/fill decisions, text widths from the GDI tables + the shell's advance table for
missing glyphs — 1.5 days. C4 viewport display list + `<universal-sheet engine="rust">` read path (`uo-sheet.wasm`
contains the calc: the open-recalc rule needs it), drawings through `uo-dml-render`, charts through `uo-charts`,
hyperlink filter — 1.5 days. C5 editor: extract the edit operations from `universal-sheet.ts` private methods into
captured scripts (Chromium or the fake DOM), then the session (merge, fill, paste, find, setFormat, entry, snapshot undo,
the calc bridge, change sets), routing, editor harness — 2–3 days. Evolution (after M6, measured): structural sharing for
undo, interning for iOS memory.

## 8. Order and dependencies

numfmt → calc (A) — independent. §4.0 → §4.2 → §4.3 → B1–B5 → `uo-dml-render` ready → C4 drawings; `uo-charts` after
§4.2, needed by B7 and C4. C1–C3 need only §4.0 and numfmt. Editors (B8, C5) after their viewers.

## 9. PDF, server and .NET (scoped per milestone)

- M4 Slides PDF: vector shapes, text (as glyphs from advance tables filled by `uo-fonts`), pictures, gradients, charts;
  **`Effect` items listed as missing** (shadow, glow, soft edge, bevel, recolour, warp, XOR metafiles) until each is ported
  and measured against PowerPoint PDF (its own oracle gate). The vendor `slides-processing` fork is replaced (SKILL: the
  converter moves to Rust) — what changes vs it is listed.
- Sheet PDF is NOT in C4: print pagination (print areas, breaks, fit-to-page, headers/footers) exists nowhere in the TS;
  it is its own item after M5 with an Excel PDF oracle.
- .NET: C# facades over `uo-ffi` replace `UniversalOffice.Pptx`/`.Xlsx` (stale C#-only `XlsxPdf`, `XlsxHtml`,
  `XlsxMarkdown`, `PptxPdf` replaced); their PDF has the M4 scope; public API shape = decision N1.
- wasm: one module per product and role (`uo-slides`, `uo-slides-editor`, `uo-sheet`, `uo-sheet-editor`), size budgets
  fixed now: viewer ≤ 1.6 MB gzip, editor ≤ 3.5 MB gzip (Write: 1.41 / 3.03–3.47 MB); `.ppt`/`.xls` lazily loaded.
  M4 handle tests (10,000 create/drop) for every new handle.

## 10. Memory, security, performance

M5 caps for every Slides cache (`advCache`, `layerPool`, `pattCache`, `pathGradCache`, `fxCache`, `dpiCache`, emfplus
`decoded`, metafile `fontAvail`, anim `pathCache`, anim-formula `cache`, diagram metrics, `read.ts` WeakMaps,
roundtrip/textconv) and the calc's; M2 soaks: slideshow 1,000 frames, Sheet scroll + edit on `giant-1m.xlsx` (iOS budget).
Fuzz: `.ppt`/`.xls` binaries, formula text, EMF/EMF+/PICT, EOT/MTX bombs, SmartArt depth, regex budget. Real Safari at
M4 and M5. Benchmarks `main × rust-core` (open, frame, scroll, recalc). Module records with process metrics per frontier.

## 11. Milestones and gates

| Milestone | Content | Gate |
|---|---|---|
| M3 — foundations + calc | §0, §4, track A, C1/C2 captures | calc parity numbers, CI green |
| M4 — Slides viewer | B1–B7, `uo-charts`, `uo-dml-render`, Slides PDF (M4 scope) | op-stream + pixel parity, PowerPoint agreement, sizes, Safari |
| M5 — Sheet viewer | C3–C4 | viewport parity, Excel agreement, 1M-row soak, Safari |
| M6 — editors + .NET | B8, C5, §9 .NET | editor harness parity, latency, N1 |

M4 state (2026-11-18): op-stream/pixel parity, PowerPoint agreement, sizes and the memory gate met; the **Safari** criterion
was run on real Safari (BrowserStack: macOS 26.4 and 15.6, iOS 26 and 15) and **accepted by the maintainer for this version
with pending items** to continue later — list in `docs/modules/uo-bindings.md` "M4 Safari criterion: accepted with pending
items", checklist in `docs/HANDOFF.md` "M4 gate". The M4 questions (SP1–SP4, DR1–DR3) are still open.

## 12. Schedule (≤ 3 agents in total; probable case from the Write port's real pace)

| Days | Track A (calc) | Track B (Slides) | Track C (Sheet) |
|---|---|---|---|
| 1 | numfmt; calc captures | §4.0 architecture pass (shared, Opus) | — |
| 2–3 | A1, A2 | §4.2 render extensions, §4.3 recorder | C1 |
| 4–5 | A3 | B1, B2 | C2 |
| 6–7 | A4 | B3, B4 | C3 |
| 8 | A4, A5 | B5, `uo-charts` | C3 |
| 9 | M3 verification | `uo-charts`, B6 | (waits for `uo-dml-render`) |
| 10–11 | — (M3 gate) | B7, M4 verification | C4 |
| 12–14 | — | B8 | M5 verification, C5 |
| 15–17 | — | M6 .NET, CI | C5, M6 verification |

The two generators of §4.3 (≈ 1 day each) run inside days 1–3 of tracks B and C (track C starts on day 1 with the
workbook generator instead of waiting).
≈ 15–17 calendar days to M6 code (draft 1 said 10–12: it exceeded the agent cap and under-counted the editors and PDF),
plus the four gates. Each day ends with a target clean-up (disk ≥ 20 GB). Sheet PDF, effects in Rust and the
`cf-formula` merge are off the critical path.

## 13. Decisions for the maintainer

A. Before phase 0: **push/backup + CI** (§0.1); **corpus** (§0.2: the real-document corpora were deleted; the plan gates on generated documents and synthetic
Office oracles — rebuild a real corpus, which needs downloads, or not); **T1 text parity** = advance
tables + `TextRun` (recommended) — and, separately, **T2 font policy** for the Rust Slides/Sheet (keep the TS rule that
a font installed by name is drawn by the browser, vs Write's file-only rule); **R1** `regress` with a step budget;
**C1** locale scope (drop the `Intl` derivations for unmeasured locales, or a host hook); **P1 PDF scope** (§9).
B. At the gates: **N1** NuGet API shape; the Slides effects in Rust and their measurement order; the `cf-formula` merge;
the size budgets if exceeded.
Not maintainer decisions (done per the existing rules, listed in the records): the hyperlink scheme filter (Write's
rule), replacing the vendor converter (SKILL), the per-product wasm split.

## 14. Architecture pass results (Slides) — 2026-11-17

Opus pass for §4.0 (Slides), the §4.2 design and the §4.5 boundary, with T1, T2, P1 and the corpus decision applied.
Records: `core/docs/modules/uo-pptx.md` (presentation model P1–P7, frontiers B1–B8, parity, JS traps, M1–M5 with a cap
per cache, fuzz targets, wasm split), `core/docs/modules/uo-dml-render.md` (shared shape renderer: boundary, what Sheet
calls, dependency direction, charts/numfmt plug-in, **the measurement protocol**), `core/docs/modules/uo-render.md`
"Slides extensions (plan §4.2) — design" (the canvas-IR list, CTM operands, layers, `TextRun`, `Effect`s, slots, the
scale-dependent decisions, codec/painter/PDF). Type skeletons: `core/crates/uo-pptx`, `core/crates/uo-dml-render`.

Corrections to this plan (evidence in `uo-pptx.md` "Plan corrections"):

1. §4.3 advance tables: the TS keeps the **unrounded** em at 1000 px (rounding to 1/8 pt is per use), kerning is a
   **whole-string** delta (`w1 − w0`), non-`PER_CHAR` strings (astral characters included) are measured whole at the run
   size, WordArt layout uses a second plain measurer, unit animations and the run fit measure at device scale, and
   metafiles, PICT, charts and image natural sizes are measurements too (12 sites, `uo-dml-render.md`).
2. §6 B3 "laid out once per slide": not bit-exact — the layout box is `(cx × scale) / scale`; layout runs per
   (slide, scale), like the list.
3. §6 B3 text warp: in the viewer it stays shell code (`Effect::Warp`, TS `text-warp.ts`, which measures ink boxes and
   rasterizes lines); its Rust port joins the PDF effects wave (§9 already lists warp missing). B3 is ≈ 840 lines.
4. §6 B1 "own parser semantics": already in `uo-opc` (lenient slides preset).
5. §1 "slides-core: all": `reveal.ts`, `transition.ts`, the pixel-reading effects of `render.ts` and the warp raster
   (≈ 1,900 raw lines of canvas code) stay in the shell painter, driven by Rust values.
6. §6 B5 is ≈ 6.8k raw lines (SmartArt 2.9k; metafiles/PICT/TIFF/EOT 3.8k): split B5a ∥ B5b, ≈ 2 days, not 1.5.
7. §4.2 slots: a slot table is per (frame, scale, origin) — `AnimBase` normalizes px geometry; unit bases need
   device-scale measurements (two-phase).
8. §4.2 colours: colour (and font/size) animation re-emits the shape's sub-list (`ov.fill` changes the paint kind), so
   slots carry only what `applyPart` applies.
9. §4.2 `GeomEval::Drawingml` is not needed: the Slides emitter evaluates presets and emits paths.
10. §4.4: the EOT reader + MTX goes into `uo-fonts` (only the writer exists); `uo-text` needs grapheme segmentation
    (`Intl.Segmenter`) — new shared piece for B3.
11. §4.3 capture harness: model JSON must be compared with **sorted keys** (creation order is not behaviour); the
    record lists the `Record` iteration sites that get their own facets.

Order unchanged in substance: §4.2 implementation (R-S1–R-S5 in `uo-render.md`) → B1a → B1b → (B2 ∥ B3) → B4 →
(B5a ∥ B5b) → B6 → B7 → B8. Questions for the M4 gate: SP1–SP4 (`uo-pptx.md`), DR1–DR3 (`uo-dml-render.md`).

## 15. Charts architecture results (`uo-charts`) — 2026-11-17

Opus pass for §6 `uo-charts`. Record: `core/docs/modules/uo-charts.md` (scope, data model, host interface, the five
measurement sites, emission map, schema additions, per-scale decisions, frontiers C-0…C-7, parity capture and the
`c1a` generator, Office oracle plan, M1–M5, fuzz, JS traps). Type skeleton: `core/crates/uo-charts` (model, host,
measure, emit, layout, error; clippy clean native and wasm32).

Corrections to this plan:

1. **Size**: ≈ 5,100 raw TS lines in the crate (render.ts alone 2,862) + ≈ 160 of the Slides host, ≈ 6,900 Rust with
   the harness. Probable time ≈ **2 days** with two agents (C-0 ∥ C-1 → C-2 ∥ C-3 → C-4 ∥ C-5 → C-6 + verification),
   not 1.5.
2. **Order**: C-0…C-5 need nothing from B4 (the chart paints through a `Canvas` trait and compares natively through a
   Rust port of the recorder's `draw` form); only C-6 (the Slides host inside `uo-dml-render`'s `emit::chart`) waits
   for B4 and the schema additions. `uo-charts` can start now, in parallel with B4.
3. **Dependency direction**: `uo-charts` depends on `uo-opc` and `uo-numfmt` only — no `uo-render` and no
   `uo-dml-render` (`ChartMeasure` and `Canvas` traits defined in `uo-charts`, implemented by `uo-dml-render` for its
   `MeasureTable` and emission context). `CssColor`/`FaceKey` are duplicated (`ChartColor`/`ChartFace`) until they move
   down after B4.
4. **Missing pieces**: schema additions A1 `ChartFont` (the chart's `[italic ][bold ]<toFixed(2)>px` font string), A2
   plain `FillText`, A3 `PathOp::RoundRect`, A4 a denominator in `TileParams` (charts compute `w × 72 / dpi`); recorder
   additions (`roundRect`, canvas font validity, `fontBoundingBox*` from the capture measurer) and painter support.
   The Excel `Chart.Export` driver was lost with the corpus and is rebuilt for the agreement run; `tilePattern` moves
   to the shell painter (B4 already did it for shapes).
5. **Text**: the chart measures widths at the canvas's current kerning (`auto`) and the `'Hg'` font box at ≈ 100 px;
   label wraps can need one round per line — a prefetch rule (CH-1) or more rounds, decided on the captures.
6. **Animated chart parts**: `chartPartBounds` is a fold over the chart's own canvas calls (`BoundsCanvas`), and its
   unbounded per-model cache becomes an LRU in the host's chart cache.

Questions for the M4 gate: CH-1…CH-6 (`uo-charts.md`).

## 16. Architecture pass results (Sheet) — 2026-11-24

Opus pass for §4.0 (Sheet), with T2, R1, C1 applied and the maintainer's performance rule (performance and memory are
first-class; internal representation may differ from the TS when the observable output is equal). Records:
`core/docs/modules/uo-calc.md` (new crate: the one cell store S1–S7, the C1 host hook, frontiers S0 + A1–A5, parity,
JS traps, M1–M5 with a cap per cache, performance budgets and benchmark gates, fuzz), `core/docs/modules/uo-xlsx.md`
"Track C architecture" (T1–T16: crate placement, model, streaming reader, surgical save, `.xls`, grid model,
viewport list, change-set undo, frontiers C1a–C5b, captures, traps, M2 soak on giant-1m, budgets, wasm split),
`core/docs/modules/uo-render.md` "Sheet extensions" (`SheetFont`, `GlyphRun`). Type skeletons: `core/crates/uo-calc`
(key, strings, value, store, formula, engine, locale; a layout-size test) and the `read`/`grid`/`edit` modules of
`core/crates/uo-xlsx` behind new features (the default build, the writer pinned by `grid-xlsx`, is unchanged).
Clippy clean native and wasm32 (`uo-calc`, `uo-xlsx`, all features).

**The store in one paragraph.** One store per workbook in `uo-calc`: per sheet a column directory, per column chunks of
256 rows, per chunk four lanes in their narrowest form (presence dense/offsets, kind byte = file type + value family +
flags, payload `u32` (integers, string ids, SST indices) or `f64` bits, style `s+1` uniform/`u16`/`u32`); rare file
details (non-canonical `<v>` text, raw refs, `cm`/`vm`, rich runs, out-of-order/duplicate cells) in sorted side
tables; strings interned once (Latin-1 or UTF-16, JS code-unit semantics, reference counted); formulas' file side in a
per-column table, their parsed side in the lazily built engine under the same id. The engine reads and writes the
store (`&mut Store`), so there is no bridge and no copy. Estimated 4.6 B/cell on giant-1m and ≈ 14 B/cell on
giant-200k (strings included), against **239 and 263 B/cell measured in the TS** (three copies: model 132.5 +
component index 37 + engine 69.5 on giant-1m).

Corrections to this plan (evidence in the records):

1. §4.0 "a 1:1 port doubles memory": it triples. Measured on the frozen dists (Node, after GC): giant-1m 239 B/cell
   (228 MB; RSS 969 MB after open), giant-200k 263 B/cell; the engine is lazy (`sheet-calc.ts:118`), so a TS viewer
   holds 170 B/cell and an editor 239 (`uo-calc.md` S1).
2. §1/§4.0 "`sheet-calc.ts`, 853 lines, the bridge": ≈ 150 of its 665 non-blank lines are the bridge (`ensure`,
   `writeBack`, `createCell`, `removeCell`) and disappear; the `XfPool` + `styles.xml` patch (`sheet-calc.ts:556-853`)
   moves to `uo-xlsx` save/edit and `edit`/`barText`/`copyText`/`valueText` to C5.
3. There are two value write-back paths with different text: the product's `writeBack` + `writeScalar`
   (`sheet-calc.ts:230-262, 461-471`: `numberLiteral` with `E+`, constant text → `inlineStr`, `null` → no `<v>`) and
   `to-sheet-core.ts:112-145` `writeValues` (`numberText` with `E`, `null` → `'0'`), which only the calc tests use: the
   store implements the product path; `writeValues` is a harness feature for the `calc-tests` capture.
4. §4.0 "ordered maps wherever the TS relies on `Map` insertion order" — the sites that are behaviour: `rangeMasters`
   (`engine.ts:205`, iterated at 669 and 818) and the formula columns' `fc.keys()` (`engine.ts:817`) decide the push
   order of precedents, hence the evaluation order (the `RAND` sequence, cycle membership and the cached values of
   cycles); `XfPool.formatId` takes the first `numFmts` entry with the code (`sheet-calc.ts:589`). The others
   (`formulas`, `rev.byCol`, `changes`, CF `each`, replace-all) are order-free in their results (table in
   `uo-calc.md` S7). §4.3 gains an **evaluation-order facet** (the sequence of evaluated cells per run).
5. The surgical save writes the model's raw texts back (`serialize.ts:62-102`: `c.value` verbatim, raw `r`, cells
   in model order, rows stably sorted, `serialize.ts:104-109`), so the store keeps non-canonical file texts and
   out-of-order/duplicate cells in side tables (`CellExtra`, `FileOrder`); the lookup keeps the last duplicate, as
   the component's index (`universal-sheet.ts:4226-4231`).
6. The TS paints a `<c r>` at its `<row r>` (`read.ts:589-593`, `universal-sheet.ts:4229-4230`) but calculates it at
   the ref's row (`from-sheet-core.ts:130`); one store has one position — the row element's (SH1).
7. The TS decodes each worksheet whole and parses it into a recursive DOM (`read.ts:289`, `xml.ts:67`) and reads all
   sheets and the whole SST at open (`universal-sheet.ts:560, 584-585`). Rust: streaming inflate + a pull reader with
   the sheet-core grammar (new in `uo-opc`; `uo-opc` streams only deflate today and its XML is a tree), `<sheetData>`
   straight into the store, the rest of the part kept as a small frame for the save; the active sheet first, the
   rest by `load_step` or on demand; the SST indexed and decoded on demand. Open-recalc still loads every sheet when
   the workbook-level tests do not decide it.
8. §7 C3 geometry "prefix arrays": the TS sums `size × zoom` sequentially into `Float64Array`s
   (`universal-sheet.ts:1172-1184`), so a closed-form sum over default rows is not bit-equal; checkpointed prefix
   sums (every 256 rows, same sequential additions inside a block) are bit-equal at 8/256 B per row.
9. §4.2/§7 C4 display list: the Sheet draws text glyph by glyph (`text-metrics.ts:104-130`, `fillText(ch, cx, y)`
   at integer GDI advances) and builds its own font string (`text-metrics.ts:58`); the Slides list needs two items,
   `SheetFont` and `GlyphRun` (≈ 8,000 `FillText` items per 1080p viewport otherwise). Everything else of the Sheet
   paint is already a Slides item.
10. §7 C5 "snapshot undo": the TS undo is a change set re-entered through the edit path
    (`universal-sheet.ts:1953-2001`, `CellSnap` 4436) plus structure snapshots; C5 ports that (no store snapshot) with
    a byte cap added to the 100 steps (SH5).
11. C1 scope: besides `value-locale.ts` and `translate.ts:126`, the host supplies the clock (`engine.ts:186-189`,
    `coerce.ts:197`, `cf-formula.ts:405`, `cond-format.ts:526`) and randomness (`engine.ts:503`); the
    `functionNames` collator order (`translate.ts:686`) is the autocomplete UI's and stays in the shell (Rust returns
    the catalogue unsorted). Host-default `toLocaleLowerCase()` in find/replace (`universal-sheet.ts:2867-2869`) is
    mapped to the root case mapping (SH2).
12. Unbounded caches not in §10: the engine's per-recalc `cache` (`engine.ts:220`, lookup indexes), the CF per-cell
    memo (`cond-format.ts:109`), the CellFonts cache (`text-metrics.ts:34`) — caps in the records.
13. Frontiers re-cut: a store frontier **S0** (Opus) precedes A3 and C1; A4 splits into six families (A4a math/
    engineering 2,000 lines, A4b statistics 1,628, A4c finance/dates 1,390, A4d text/regex/filterxml 1,701, A4e
    lookup/dynamic/database 1,632, A4f logical/info/cube 467); C1 → C1a (read) + C1b (save); C3 → C3a (text, entry,
    geometry; Sonnet) + C3b (CF with `cf-formula.ts` 1:1; **Opus**, it evaluates formulas) + C3c (table/pivot styles,
    borders, fills; Sonnet); C5 → C5a (extraction) + C5b (session).
14. Performance is gated, not observed: budgets per frontier (open → first viewport list on giant-1m ≤ 250 ms
    native / 500 ms wasm, retained ≤ 8 MiB; viewport emission ≤ 1 ms native / 2.5 ms wasm at 1080p, ≤ 48 KiB
    crossing; full recalc ≤ 0.5× TS native / 0.8× wasm; edit latency ≤ 16 ms p95 wasm) and the rule "slower than the
    TS or > 10% over the previous record fails the frontier unless approved" (`uo-calc.md`, `uo-xlsx.md` T13), with a
    `sheet` suite in `tools/bench/speed/run.mjs`.

Resulting order (≤ 3 agents): (S0 ∥ A1) → A2 → (A3 ∥ C1a) → (A4 families, two at a time ∥ C1b, C2, C3a) → (A5 ∥ C3b
∥ C3c) → C4 (needs A5 for open-recalc and B4, done) → C5a → C5b. Track A is Opus throughout; C1a, C1b, C2, C3a, C3c,
C4, C5a, C5b are Sonnet with closed scope from the records; C3b is Opus.

Questions for the M3/M5 gates: SH1 (one cell position for inconsistent `<c r>`/`<row r>`), SH2 (root case mapping
for find/replace), SH4 (the performance gates as M3/M5 criteria) in `uo-calc.md`; SH3 (calc inside `uo-sheet.wasm`
vs a lazily loaded part if over 1.6 MB gzip), SH5 (history byte cap), SH6 (`uo-xls.wasm` separate, as SP4) in
`uo-xlsx.md` T16.
