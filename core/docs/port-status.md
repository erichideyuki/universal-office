# Port status — 2026-11-03 (updated after the shared/fonts and shared/drawingml completion)

Measured, not estimated, for the line counts; the "ported" fraction of a few split files (marked
partial) is a judgement from the frontier records.

**Method.** Behaviour source = the TS/C# that moves to Rust for Write, counted as non-blank,
non-comment lines (`grep -cvE '^\s*(//|\*|/\*|$)'`): `write/packages/docx-core/src`,
`write/packages/docx-layout/src` (without the shell painter `canvas.ts`, `inline-renderer.ts` and
the legacy canvas layout dropped by G19; 30% of `canvas-picture.ts`), `shared/fonts`,
`shared/drawingml`, the editor model (45% of `universal-write.ts` per the uo-docx planner's method
classification, plus the model files), `write/packages/compare` without its UI element, and the
C#-only Editing API + Export. A file counts as ported when its frontier passed its parity facets.

| Area | Lines | Ported |
|---|---|---|
| docx-core (model, reader, writer, ops, .doc, ODF, export) | 20,030 | 29.6% |
| docx-layout (layout, PDF, display list) | 12,827 | 47.8% |
| shared/fonts (Write scope; the 55 pptx lines of `ooxml.ts` belong to Slides) | 782 | 100% |
| shared/drawingml | 1,658 | 100% |
| editor model (to uo-docx session) | 12,889 | 0% |
| compare | 789 | 0% |
| C#-only (Editing API, export) | 4,188 | 0% |
| **Write total** | **53,163** | **27.3%** |
| **Milestone 1 (read path: reader → layout → render → PDF)** | **22,930** | **≈ 62%** |

Largest remaining pieces: the editor model D2 (~12.9k), `.doc` D4 (~4.9k), layout engine + tables
F6/F7 (`engine.ts` 3,128, `table.ts` 1,047, rest of `para.ts` F4), the C#-only Editing API D3
(4.2k), the writer D1b (~2.4k + write halves of props/picture-xml/lossless), ODF/export/compare D5
(~2.7k), R5 `fulldoc.ts` (723), the PDF painter (794), render helpers.

Rust written so far (non-comment lines, src / tests): uo-docx 18.5k / 10.7k, uo-docx-layout
9.0k / 5.7k, uo-fonts 6.5k / 12.0k, uo-opc 5.1k / 8.2k, uo-jsnum 4.5k / 3.9k, uo-drawingml
3.1k / 5.4k, uo-schema 2.7k / 2.7k, uo-pdf 1.6k / 2.3k, uo-wasm 1.4k, uo-image 1.3k / 0.9k,
uo-testkit 1.1k, uo-render 1.1k, uo-text 0.8k / 0.9k, uo-codec 0.5k / 0.2k — ≈ 57k src and
≈ 61k test lines; plus infrastructure with no TS source (uo-jsnum, uo-text, codec, harnesses).

Caveat: lines are not effort. What is left is weighted toward the hardest parts (the layout engine
and the editor model/undo), so the remaining effort share is larger than the remaining line share.

Update: `shared/drawingml` `contour.ts` and `shape-styles.ts` ported into `uo_docx::drawing` (Sonnet,
0 mismatches), and `shared/fonts` `ooxml.ts` (docx side) into `uo_docx::write::fonts` (Opus, the
first piece of D1b; corpus 27,530 facets equal, 0 parity rounds). Both shared packages are now
complete for Write; the pptx half of `ooxml.ts` waits for uo-pptx.

## Update (2026-11-05, state of commit c3f95ae5)

Same method as above (non-blank, non-comment TS lines per area, `grep -cvE '^\s*(//|\*|/\*|$)'`; a file counts as
ported when its frontier passed its parity facets; partial files marked). The commit holds the D1b writer (core
+ W1–W6), D4 `.doc` F0–F3 and F5, D5 export / ODF / compare, the R5 follow-ups and `odraw` in uo-drawingml. The
D2a session (`src/session`) is not in it and is not counted. The TS denominators were recounted: docx-core is
20,033 lines (72 files; the table above says 20,030), the compare package 789 (as before).

| Area | Lines | Ported 2026-11-03 | Ported now |
|---|---|---|---|
| docx-core (model, reader, writer, ops, .doc, ODF, export) | 20,033 | 29.6% (5,929) | **83.2%** (16,672; 73.7% before the D4 F4/F6 correction below) |
| docx-layout | 12,827 | 47.8% | 47.8% (no change in this commit) |
| shared/fonts | 782 | 100% | 100% |
| shared/drawingml | 1,658 | 100% | 100% |
| editor model | 12,889 | 0% | 0% (D2a not in the commit) |
| compare | 789 | 0% | **100%** (19,033 parity results equal) |
| C#-only (Editing API, export) | 4,188 | 0% | 0% |
| **Write total** | **53,166** | **27.3%** | **49.0%** (26,032 lines; 45.4% before the correction below) |

What the +8,836 docx-core lines are (TS lines of the files that passed their facets since the first table):

| Piece | TS files | Lines | Facets |
|---|---|---|---|
| D1b writer core and emitters W1–W6 | `serialize.ts` 1,474, `lossless.ts` 483, `rev-xml.ts` 96, `table-xml.ts` 203, `settings-xml.ts` 137, `notes.ts` 127, `hf-ops.ts` 62, `shape-xml.ts` 475, `picture-xml.ts` 893 | 3,950 | round trip, lossless, picture-xml 50,785, settings/notes/hf 45,617 (W4), shape-xml, table-xml, runs |
| D4 `.doc` F0–F3, F5 | `doc/index.ts`, `xmlw.ts`, `fib.ts`, `text.ts`, `fkp.ts`, `sprm.ts`, `fonts.ts`, `props.ts` 717, `stsh.ts`, `lists.ts`, `ooxml-props.ts`, `styles-xml.ts`, `numbering.ts`, `settings.ts` | 2,356 | `doc.capture` (69,350 comparisons equal) |
| `odraw` in uo-drawingml (OfficeArt) | `doc/bytes.ts` 85 (ByteReader for D4 F0, the record half for odraw), `doc/officeart.ts` 250, `doc/preset-map.ts` 232, `doc/reader.ts` partial (40 of 395: `loadDrawings`, `shape`, `backgroundShape`, `bse`) | 607 | odraw parity |
| D5a export | `export/model.ts` 110, `html.ts` 240, `markdown.ts` 124, `index.ts` 10, `from-docx.ts` partial (16 of 36) | 500 | export parity 15,154 equal + 1 accepted |
| D5b ODF | `odf/index.ts` 671, `write-odt.ts` 481, `styles.ts` 164, `units.ts` 27, `props.ts` 3, `zip.ts` 33 (replaced by uo-opc) | 1,379 | ODF parity 40,248 equal + 3 accepted |
| final view of `revisions.ts` | `isDeletedRun`, `finalRuns`, `finalParagraphs`, `finalBlocks` (partial, 44 of 437) | 44 | read by the export and compare facets |

The compare row (789) is `tokens.ts` 211, `myers.ts` 89, `histogram.ts` 133, `diff.ts` 330, `index.ts` 26 (the
`<universal-write-diff>` element, 598 lines, stays out of the count as before).

Judgement calls (not measured): `page-background.ts`, `watermark.ts`, `text-effects.ts` and `source.ts` had their
read halves in the first table and their write halves (W5, W6, the store) landed now; they are **not added again**,
so if the first table counted only the read halves the new figure is a little low. `bytes.ts` is counted whole.
**Correction (orchestrator, same day):** D4 F4 (`doc/reader.rs`, `pictures.rs`, `formfields.rs`, 355 + 108 + 84 TS
lines) and F6 (`doc/docx_writer/*`, `docx-writer.ts` 1,360) are in the commit and **do** pass their facets: the converter
runs the whole pipeline and `tests/parity_doc.rs` replays convert, parts, model and the reader facets — 179,929
comparisons equal, 0 mismatches, 4 accepted (N1, nesting past `max_nesting`). Their 1,907 lines are counted in the
table (docx-core 14,765 → 16,672). The capture script `js/capture-doc.mjs` was committed empty (lost in the
interrupted session) and is being rewritten against the intact capture. R5 follow-ups (nested-story stack, single-visit
hyphenation walk) add no TS lines.

Rust written now (non-blank, non-`//` lines, recounted from the commit; `src` leaves out generated data tables,
`tests` counts every `.rs` file): uo-docx **47.0k / 24.7k** (was 18.5k / 10.7k: `write` 12.2k, `doc` 10.5k,
`read` 11.3k, `word` 4.2k, `odf` 2.4k, `compare` 1.6k, `export` 1.3k, `model` 2.8k), uo-drawingml **4.4k / 6.2k** (was
3.1k / 5.4k, `odraw`), uo-docx-layout 9.1k / 5.7k, uo-fonts 6.5k / 12.0k, uo-opc 5.1k / 8.2k, uo-jsnum 4.5k / 3.9k,
uo-schema 2.7k / 2.7k, uo-pdf 1.6k / 2.3k, uo-wasm 1.4k, uo-image 1.3k / 0.9k, uo-testkit 1.1k, uo-render 1.1k,
uo-text 0.8k / 0.9k, uo-codec 0.5k / 0.2k — **≈ 87k src and ≈ 76k test lines** (was ≈ 57k and ≈ 61k). The unchanged
crates keep the figures of the first table (their line counts are identical in the two commits); the same rule
applied to both commits gives 55.9k → 85.7k src and 53.1k → 67.9k test lines, i.e. +29.8k and +14.8k.

Largest remaining pieces: the editor model D2 (~12.9k, the session `src/session` is in progress), layout engine +
tables F6/F7 (`engine.ts` 3,128, `table.ts` 1,047, rest of `para.ts` F4), the C#-only Editing API D3 (4.2k), the editing
operations of docx-core still unported (`table-ops.ts` 950, `model-ops.ts` 352, `section-ops.ts` 132,
`style-sheet.ts` 341, `table-formula.ts` 287, `table-sort.ts` 110, `table-style-edit.ts` 91 and the rest of
`revisions.ts`, which belong to D2b), the PDF painter (794), render helpers. Same caveat as above: lines are not
effort, and what is left is the hardest part. Not covered by any memory, no-panic or fuzz test: D5 export, ODF and
compare (see `uo-docx-d5/D5a.md`, `D5b.md`); the D5 soak has not been run.
