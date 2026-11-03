# Port status — 2026-11-03 (HEAD 5cfa71d9)

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
| shared/fonts | 837 | 65.9% |
| shared/drawingml | 1,658 | 91.4% |
| editor model (to uo-docx session) | 12,889 | 0% |
| compare | 789 | 0% |
| C#-only (Editing API, export) | 4,188 | 0% |
| **Write total** | **53,218** | **26.5%** |
| **Milestone 1 (read path: reader → layout → render → PDF)** | **22,700** | **≈ 62%** |

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
