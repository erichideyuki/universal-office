# uo-xlsx — the xlsx writer (Sheet track C, writing side)

Record of the first piece of track C (plan `core/docs/slides-sheet-port-plan.md` §7, C1 "uo-xlsx read/write"):
the **writer** only, from scratch, for the export of `<u-grid>` (`@universal-material/data-grid`, spec 29,
approved by the maintainer 2026-11-23) and the Sheet track that follows. Hybrid-mode run, one Opus session
(2026-11-23): design, implementation, tests, Excel check.

## Scope

- **TS covered: none.** The frozen TS writer (`sheet/packages/sheet-core/src/serialize.ts`, `ts-ref-092`) is
  *surgical*: it replaces `<sheetData>`, `<cols>`, `<mergeCells>` and the first `<sheetView>` inside the worksheet
  XML of an OPENED package (`blank.ts` provides the empty one). There is no from-scratch TS writer to port, so there
  is no parity target: the oracle is **Excel** (open without repair, values and formats read back through COM) plus
  the repo's own TS reader. What the crate takes from the TS: `encodeXstring` (ST_Xstring escapes), the pane
  attributes of `paneXml`, the schema order of `AFTER_MERGES`, the blank workbook's `styles.xml` skeleton and
  `calcId="191029"`.
- C# covered: none (`UniversalOffice.Xlsx` writes through its own code; the facades come with C1 proper).
- Left for later (track C): the reader, the surgical save of an opened workbook (the TS `updateWorksheetXml`),
  formulas/calc, charts, drawings, conditional formatting, pivots, `.xls`, themes, rich text runs.

## Requirements (the consumer: `D:\Projects\data-grid\.specs\29-export-xlsx.md`)

Workbook with one or more sheets, sheet name, `rightToLeft`; shared strings, numbers, booleans, dates from a
wall-clock date (no time zone), text cells never formulas, hyperlinks; a deduplicated XF pool (numFmt with custom
codes, fills, fonts bold/italic/colour, thin/medium borders per side, alignment start/end/center/centerContinuous,
wrap); column widths, row outline with the summary below, frozen top rows, merges (optional), print titles, a
`ColumnTitle…` defined name; `docProps/core.xml` with title, creator and `dc:language`; rows appended in order with
bounded memory (10k × 40 and beyond); no threads, no file system, usable from a `cdylib` (`grid-xlsx`, pinned by git
`rev` like `grid-pdf` pins `uo-fonts`).

## Design

- **One state machine, no borrowed sub-writers**: `XlsxWriter::new(WorkbookOptions)` → `add_style` (any time) →
  per sheet `begin_sheet(SheetOptions)`, `append_row(&RowOptions, &[Cell])`…, `merge(CellRange)`, `end_sheet` →
  `finish() -> Vec<u8>`. A C-ABI or wasm wrapper keeps one writer in a thread-local (the grid-pdf pattern).
- **Streaming**: the worksheet XML is buffered up to 64 KiB and deflated into the archive as it grows, through a
  streamed zip entry added to `uo-opc` (`ZipWriter::stream_begin` / `stream_write` / `stream_end`,
  `Deflater::stream_begin` / `stream_feed` / `stream_finish`; the local header is patched with the CRC and sizes at
  the end; the streamed deflate output is byte-equal to the one-shot one, `streamed_deflate_equals_one_shot`). The
  shared-string table is deflated as it grows by its own `Deflater` and added raw at `finish`. Everything that the
  worksheet XML puts BEFORE `<sheetData>` (sheet properties, view and pane, `outlineLevelRow`, `<cols>`) is in
  `SheetOptions`; what comes after (merges, hyperlinks) is kept per sheet until `end_sheet`.
- **Styles**: `add_style(&Style) -> StyleId` interns the description once (a `u32` per cell afterwards: the heat map
  costs 4 bytes of style per cell). The `xf` is resolved when a cell is written, keyed by (style, sheet direction,
  implied date format): `Start`/`End` become `left`/`right` by the sheet's direction, and a date cell without a
  number format gets built-in 14 (or 22 with a time). Equal resolved formats share one `xf`; fonts, fills,
  borders and custom codes are pooled. A custom code equal to an INVARIANT built-in (ids 0-4, 9-13, 45-49, read from
  `uo_numfmt::builtin_format`) is written as that id; the locale-dependent built-ins (5-8, 14-22, 37-44) are never
  chosen for a custom code (their display changes with the reader's locale).
- **Part order** in the archive: worksheets (streamed), their hyperlink relationships, shared strings, styles,
  workbook, workbook relationships, core and app properties, root relationships, `[Content_Types].xml` last (the
  sheets are not known before `finish`; Excel, LibreOffice and the TS reader do not depend on the order).
- Deterministic: no clock, no hash-order output (maps are lookups only); `dcterms:created` is the caller's string.

## Decisions outside "what Excel writes"

| # | Decision | Why |
|---|---|---|
| X1 | `<sst>` without `count`/`uniqueCount`, worksheets without `<dimension>` and `spans`. | Optional in the schema; they precede data that is streamed. Excel opens the files without repair (below). |
| X2 | NaN/±Infinity are written as empty cells (kept when styled). | No `<v>` can hold them; an error value would be data the caller did not give. |
| X3 | A date outside 1900-01-01..9999-12-31 is written as ISO 8601 text (`1850-03-09 07:05:00`); invalid fields (month 13, 30 Feb) are `Error::InvalidDate` and the row is not written. | Excel has no serial for it; text keeps the information visible. |
| X4 | Serials follow Excel's 1900 system with the fictitious 1900-02-29 (1900-03-01 = 61); 1904 is not offered. | What Excel shows for these serials. |
| X5 | Text longer than 32,767 UTF-16 units is cut (never inside a surrogate pair) and counted (`cut_texts`). | Excel's cell limit; longer cells are repaired. |
| X6 | External links: only `http:`, `https:`, `mailto:`, `ftp:`, `ftps:`, `tel:`; targets up to 2,079 units; 65,530 links per sheet. Others are dropped and counted (`dropped_links`), the cell keeps its value. | The hyperlink scheme rule of the Write viewer (`docs/security.md`), Excel's limits. |
| X7 | Sheet names are validated (`validate_sheet_name`: 1-31 units, no `[]:*?/\`, no leading/trailing `'`, not `History`, no control characters; unique without case); `sanitize_sheet_name(name, fallback)` makes any text valid. | Excel's rules; the grid cleans names from the host. |
| X8 | Defined names are validated (letter/`_`/`\` first, then letters, digits, `_ . \ ?`; not an A1 or R1C1 reference; not `_xlnm.`; ≤ 255) and are sheet-scoped (`localSheetId`) or workbook-scoped by choice; they are written sorted by name as Excel does. | The `ColumnTitle` convention needs the same name on several sheets. |
| X9 | Limits are errors raised BEFORE writing anything of the call (the writer stays usable): rows past 1,048,576, columns past 16,384, outline levels above the declared one or 7, more than 64,000 cell formats or 1,024 fonts (the failing row is undone), row heights outside 0-409.5, widths outside 0-255. | A grid export must not produce a file Excel repairs. |
| X10 | `SheetOptions::outline_levels` must be declared up front (it is `outlineLevelRow`, before the rows). | Streaming. |
| X11 | The Normal font defaults to Aptos Narrow 11 (Excel 365, as `blank.ts`); fonts carry no `scheme` and no theme colours (there is no theme part). | Only the name is stored; no proprietary file involved. |
| X12 | Column width is Excel's file unit (it includes the 5 px padding: Excel's UI shows 24 as 23.29); `column_width_from_px(px, mdw)` gives the smallest width that Excel draws as exactly `px` pixels (`grid-metrics.ts` formula; unit-tested for every px 0-399 at MDW 5-11). | The grid knows pixels. |

## Verification (2026-11-23)

### Tests (`cargo test -p uo-xlsx`)

| File | What |
|---|---|
| `src/*` unit tests (11) | column names, numbers (`1e+21`, `-0` → `0`), ST_Xstring, cutting, sheet/defined-name validation, widths, ranges, style pools and resolution |
| `tests/package.rs` (16) | package structure (every part strict-parses, overrides ↔ parts, relationships resolve, `uo_opc::Package::open` + `verify`), determinism, shared strings and values, inline strings, long text, dates (serials, implied formats 14/22, ISO fallback, invalid fields), hyperlinks (rels, location, schemes, length), style deduplication (counts, XML), Start/End by direction, sheet layout (cols, panes, outline, merges, element order of `CT_Worksheet`/`CT_Workbook`, defined names), summary above / frozen columns, document properties, several sheets and names, limits and errors, 1,048,576 rows, 64,000 formats |
| `tests/golden.rs` | every part of the `features` sample byte for byte against `tests/golden/features/` (the sample checked in Excel; bless with `UO_XLSX_BLESS=1` after rerunning the Excel check) |
| `tests/memory.rs` | M1, M2, M3 (below) |
| `tests/no_panic.rs` | 3,000 seeded call sequences (0.4 s); finished workbooks must be valid packages. Local run with `UO_XLSX_NOPANIC_N=200000`: 200,000 sequences, 83,507 finished workbooks (491 MB), 0 panics |
| `uo-opc/tests/zip_write.rs` (+3) | streamed entries at levels 0/1/4/9 in pieces of 1 B to the whole part, stored names, misuse, an open entry dropped by `finish`, streamed deflate = one-shot deflate |

### Excel open check (Excel 365 pt-BR, COM, invisible, read-only)

`tools/excel-open-check.ps1` (through `tools/parallel/word-lock.mjs`): with `DisplayAlerts` off a workbook Excel would
repair FAILS `Workbooks.Open`. **Negative controls** (`examples/samples.rs` writes them to `controls/`): a
shared-string index past the table and overlapping merges both FAIL — the check detects a repair. Samples
(`cargo run --release -p uo-xlsx --example samples -- <dir>`):

| Sample | Result |
|---|---|
| `minimal.xlsx` (one cell) | opens |
| `features.xlsx`, `features-inline.xlsx` (2 sheets, the second RTL; every style kind; band row with `centerContinuous`; outline; frozen rows; frozen row+column on sheet 2; hidden row and column; links; merge; dates; `ColumnTitle` and print titles) | open; read back: values, `R$ #.##0,00;[Vermelho]-R$ #.##0,00`, `0,0%`, `dd/mm/aaaa`, built-in 14 → `dd/mm/aaaa`, 22 → `dd/mm/aaaa hh:mm`, bold/italic/colours/fills, `HorizontalAlignment` 7 (center across) / -4108 / -4131 / -4152 (Start → right on the RTL sheet), wrap, thin/medium borders, outline levels 1/2 with `SummaryRow` below, row height 30 and hidden row, widths (24 → 23.29), 3 links (the `javascript:` one dropped, a location link), `FreezePanes` with SplitRow 2 / SplitRow 1 + SplitColumn 1, `DisplayRightToLeft`, names `'Vendas & metas'!ColumnTitle` = `$A$2` and `Print_Titles` = `$1:$2`, title and author, `=SUM(A1:A2)` as text, the literal `_x0041_` kept |
| `heatmap-10k.xlsx` (10,000 users × 40 documents, 10-step styled scale, band and header rows, 200 groups) | opens; 10,202 rows × 41 columns, panes, names |
| `heatmap-inline-1k.xlsx` | opens |
| `wasm-heatmap-10k.xlsx` (written by the wasm consumer, 10,000 `mailto:` links) | opens; 10,000 hyperlinks |

The TS reader of the product (`tools/ts-read-check.mjs` over `UO_TS_ROOT`, `openSpreadsheet` + `readWorkbook` +
`readWorksheet`) reads every sample (rows, cells, columns, merges, panes as written).

**Not checked: LibreOffice and Google Sheets.** LibreOffice is not installed on this machine (installing it is a
download, the maintainer's call); Google Sheets needs an upload of the file. Both are open items for the grid's
"opens without repair in Excel, LibreOffice and Google Sheets" criterion.

### Memory (measured, `tests/memory.rs`, release, native)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak | pass | minimal, features (both string modes), heat map 500 × 20, a writer dropped half-way: live bytes back to the start exactly |
| M2 soak | pass | 20,000 rows × 40 cells after a 20,000-row warm-up: live bytes minus the archive buffer's capacity = 393,406 B at every sample (0 B spread): the writer state does not grow with rows |
| M3 10,000 × 40, shared strings | — | file 1.95 MB, peak 4.47 MB, working set (peak − file) 2.52 MB, 36.1 MB allocated in total |
| M3 10,000 × 40, inline | — | file 1.95 MB, peak 3.17 MB, working set 1.22 MB |
| M3 40,000 × 40, inline | — | file 7.87 MB, peak 11.46 MB, working set 3.59 MB (the archive `Vec`'s doubling slack; the M2 line shows nothing else grows) |

What grows with the data: the archive (the output), the compressed shared-string table and its index (up to
`SST_DEDUP_CAP` = 2^20 distinct strings; later new strings are appended without lookup), and the open sheet's merges
and links. `shared_strings: false` (inline strings) leaves only the archive. Speed: the 10,000 × 40 heat map takes
133-138 ms natively (release, this machine) and 475 ms in wasm (Node 24, including the JS-side encoding of the
consumer's buffers).

### wasm size (the minimal consumer, `wasm-consumer/`)

A `cdylib` with the shape of `grid-xlsx` (thread-local writer, `alloc`, `create` / `add_style` / `begin_sheet` /
`append_row` / `finish`, no wasm-bindgen), the grid-pdf release profile (`lto`, `codegen-units = 1`, `panic =
"abort"`, `strip`). `wasm32-unknown-unknown`, rustc 1.100 (`gzip -9` CLI):

| Profile | raw | gzip -9 | brotli 11 |
|---|---|---|---|
| `opt-level = 3` (grid-pdf's) | **231,210** | **93,825** | 78,176 |
| `opt-level = "s"` | 189,089 | 83,768 | — |
| `opt-level = "z"` | 155,531 | 74,997 | — |
| 3 + `wasm-opt -O3` | 203,591 | 91,569 | — |

Code by owner (opt 3, names kept): uo_xlsx 49 KB, core 25 KB, miniz_oxide 20 KB, hashbrown 17 KB, the consumer's
exports 37 KB (inlined writer code), uo_opc 11 KB, dlmalloc 8 KB; data 42 KB (CRC-32 slicing tables, deflate tables,
ryu-js). The spec's estimate was 150-300 KB. `node wasm-consumer/smoke.mjs <wasm> <out.xlsx>` runs it.

### Security

Inputs are the caller's calls (no file is parsed). Fuzz target `xlsx_write` (`fuzz/src/xlsx.rs`, the call
interpreter `tests/support/ops.rs` shared with the no-panic test): fallback driver 120 s, **515,698 executions, 0
crashes**, slowest 8.7 ms; every finished workbook is checked (entries inflate to their CRC, every part parses in
strict XML mode, every part has its content-type override). The fuzz crate did not compile at the base commit
(`fuzz/src/ppt.rs` missed `ConvertOptions::ts_ppt_charts`): fixed with `..ConvertOptions::default()`.

## Open items

- LibreOffice and Google Sheets open checks (see above).
- `[Content_Types].xml` is the last entry; tools that sniff the zip's first entry for OOXML (some `file`/libmagic
  versions) may not label the file as xlsx. A `sheet_count` hint in `WorkbookOptions` would allow writing it first.
- Start/End → left/right in RTL sheets is the writer's rule, not measured in Excel's UI (the COM read-back confirms
  what is stored, not how it looks); the alternative is `general` + `readingOrder`.
- For track C proper: the reader, the surgical save (TS `updateWorksheetXml`) and its parity, rich text, themes.

## Track C architecture (2026-11-24, Opus) — reader, store, save, `.xls`, grid, viewport, editor

Architecture pass for plan §4.0/§7 (record of the store and the calc: `uo-calc.md`; plan §16). Decisions applied:
T2, R1, C1 (HANDOFF 2026-11-24) and the maintainer's PERF rule (performance and memory first-class; internal
representation may differ from the TS when the observable output is equal). Behaviour source: the frozen TS
(`ts-ref-092` at `b92b60fa`), paths under `sheet/packages/`, raw line numbers, sizes in non-blank lines.
**The writer above is untouched**: everything below is behind features (`read`, `save`, `xls`, `grid`, `edit`;
`Cargo.toml`), the default build is the writer that `grid-xlsx` pins. Skeleton: `src/book.rs` (`read`),
`src/grid.rs` (`grid`), `src/session.rs` (`edit`).

### T1. Crate placement

| Piece | Where | Why |
|---|---|---|
| cell store, strings, formulas (file side), calc | `uo-calc` (new crate) | lowest Sheet layer; the calc must not depend on the file format; `uo-calc.md` "Why a crate" |
| workbook model outside the cells, reader, surgical save, save-calc, XfPool | `uo-xlsx` `read`/`save` | the TS `sheet-core` + the styles half of `sheet-calc.ts` |
| `.xls` → OOXML parts | `uo-xlsx` `xls` (lazily loaded `uo-xls.wasm`) | as `uo-pptx` `ppt`; uses `uo-opc` CFB, `uo-drawingml` odraw, `uo-charts` `biff` |
| grid model (cell text, entry, CF incl. `cf-formula.ts` 1:1, table/pivot styles, borders/fills, metrics, geometry) and the viewport display list | `uo-xlsx` `grid` | Sheet-only (reuse rule: a module of the owning crate) |
| drawings and charts in the grid | `uo-dml-render` (`read_drawing_elements`, `emit_shapes`, `special_image`; `uo-dml-render.md` "What Sheet calls") and `uo-charts` | already shared; `uo-xlsx` implements `uo_dml_render::PartSource` over its package |
| editor session, change-set undo | `uo-xlsx` `edit` | as `uo-pptx` `edit` |
| exporters csv/ods/xml2003/html/markdown/pdf (`export/`, 696) | `uo-xlsx` `export` feature, after C5 | not on the viewer path |
| streaming inflate, XML pull reader with the sheet-core grammar | `uo-opc` (`flate::Inflater::stream_*`, `xml::pull`) | shared pieces (none exists today: `uo-opc` streams only deflate, its XML is a tree); C1a adds them |

### T2. The workbook model (feature `read`, frontier C1a)

- `model.ts` (300) and `styles.ts` types are ported as plain Rust structs, mechanically: every TS `?:` is an
  `Option`, booleans the TS always sets are `bool`, string fields are `Box<str>` (attribute texts) or `JsString`
  (cell/user text), every `Map` whose iteration is behaviour is an ordered `Vec` (`Styles.numFmts`: the XfPool's
  first match, `sheet-calc.ts:589`), `Record` key order through `uo_jsnum::object`. The cell fields of
  `Worksheet.rows` are NOT in the model: they are the `uo-calc` store (row attributes: `store::RowMeta`).
- `Worksheet` keeps: merges (A1 texts + the parsed areas), cols, pane, flags, CF (`read-cf.ts`), comments, charts and
  drawing objects (anchors + the element as a subtree of the frame's DOM), chart-sheet view, hyperlinks,
  `sheetFormatPr`, tables, autoFilter, pivots (`read-pivot.ts`), list validations, `ignoredErrors`.
- `Book` (session root): package, `WorkbookModel`, `Styles`, theme/colour context, per sheet the `SheetFrame` and
  the model, the store, the lazily built engine.

### T3. The reader (C1a): streaming, lazy, no DOM of the sheet

The TS reads every worksheet at open (`universal-sheet.ts:584-585`) by decoding the whole part to a string and
parsing it into a recursive DOM (`read.ts:285-305`, `xml.ts:40-140`): 969 MB RSS after giant-1m in Node.

1. **Open** = zip directory, `[Content_Types]`, workbook, workbook rels, styles, theme/indexed colours, the
   shared-string **index** (one pass that records the byte offset of each `<si>`; entries are decoded into the pool
   on demand; the TS decodes all of them, `universal-sheet.ts:560`), and the **active sheet** (`activeTab`) — enough
   for the first viewport.
2. **Worksheet parse** = streaming inflate (new `uo_opc::flate::Inflater` streaming API) → pull tokenizer
   (`uo_opc::xml::pull`) with the sheet-core grammar: closing-tag names are not checked (`xml.ts:100-104`),
   comments/CDATA/PI skipped as `xml.ts:47-64, 106-116`, `\r\n`/`\r` → `\n` in text (`normalizeEol`), attribute
   white-space normalization (`normalizeAttr`), entities as `decodeEntities` (`&#x…;` through `fromCodePoint`, which
   THROWS past U+10FFFF: the open fails, as the TS), the SpreadsheetML main prefix stripped (`xml.ts:142-153`), UTF-8
   decoded per token (equal to decoding the whole text: markup bytes are ASCII and never inside a sequence). The
   bytes before `<sheetData>` and after `</sheetData>` are kept as the **frame** (`SheetFrame.prefix/suffix`,
   typically < 10 KB) and parsed into a `uo_opc::xml` DOM for the model; the `<sheetData>` content goes straight into
   the store row by row (`readRow`/`readCell` rules, `read.ts:573-621`: implicit `r`, `refToRowCol` failure →
   position-derived ref, `t` cast, `parseInt` of `s`, `cm`/`vm` only when all digits, `<f>` attributes, inline
   strings).
3. **Canonical vs extra**: a cell whose `<v>` text equals the canonical print of its parsed value (`numberLiteral`
   for numbers, the decimal of an SST index, `1`/`0`…) stores only lanes; anything else adds a `CellExtra`
   (`uo-calc.md` S2). Rows or cells out of order or duplicated switch the sheet to `FileOrder` (save order kept; the
   lookup keeps the LAST duplicate, as the component's index, `universal-sheet.ts:4226-4231`).
4. **Remaining sheets**: `Book::load_step(budget)` driven by the shell (idle callbacks) or synchronously by anything
   that needs a sheet (tab switch, cross-sheet formula, find in all sheets, save, open-recalc). The open-recalc
   decision (`shouldRecalcOnOpen`, `open-recalc.ts:93-131`) needs every formula: a workbook that is not decided by the
   workbook-level tests (manual / fullCalcOnLoad / forceFullCalc / not Excel / old calcId, lines 95-100) loads all
   sheets before the first recalc, as the TS did at open.
5. **Not parsed at open**: chart XML and drawing elements are parsed when first painted (the TS keeps chart XML raw
   and parses on demand too, `model.ts:340-353`).

### T4. Surgical save (feature `save`, frontier C1b)

- Untouched sheets are not re-serialized (the TS rewrites only `dirtySheets` + the calc's `touched`,
  `universal-sheet.ts:909-928`). A rewritten worksheet is `frame.prefix (patched) + <sheetData>… + frame.suffix
  (patched)`: `<cols>` lives in the prefix, `<mergeCells>` in the suffix, the first `<sheetView>` in the prefix — the
  same string surgery as `updateWorksheetXml`/`replaceMergeCells`/`updateSheetView` (`serialize.ts:138-254`), ported
  1:1 on the frame text.
- `<sheetData>` is generated **streaming from the store into the deflater** (64 KiB buffers, `ZipWriter::stream_*`),
  never as one string: rows ascending (the TS `sort` is stable; `FileOrder` sheets replay the TS order), the
  `rowXml`/`cellXml` rules (`serialize.ts:62-102`: `spans` recomputed, `s … customFormat="1"`, `ht` by `String(n)`,
  `t`, `cm`, `vm`, `<f>` attributes in the TS order, `<is>` with `xml:space`, `t="str"` empty `<v></v>`, 365-only
  errors as `#VALUE!`, `encodeXstring`), the value text from `CellExtra::raw_value` or the canonical print.
- `save-calc.ts` (178: calcChain removal, `calcPr`, `metadata.xml` XLDAPR, defined names) and `calc/save.ts`
  `prepareForSave` (formula attributes, shared children whose anchor vanished, `ca`/`aca`) run before; then the
  XfPool patch of `styles.xml` (`sheet-calc.ts:556-853`, regex surgery ported with the same matching).
- Other parts: copied. Parity is on decompressed bytes (as Write); the package writer may copy unchanged compressed
  entries raw (PERF; not observable).

### T5. `.xls` (feature `xls`, frontier C2)

`sheet-core/src/xls/*` (4,420) → the same in-memory OOXML part bytes as the TS converter, then the normal reader.
Sonnet, closed scope (plan §7 C2). `uo-xls.wasm` is a separate, lazily loaded instance (as `uo-ppt.wasm`).

### T6. The grid model (feature `grid`, frontier C3)

| TS | Rust (`uo_xlsx::grid::…`) | Notes |
|---|---|---|
| `grid-metrics.ts` 75, `glyph-widths.ts` 22 + `glyph-widths-data.ts` 841, `excel-metrics-data.ts` 112, `row-descent-data.ts` 60 | `metrics` | static tables (`&'static`), no cache |
| `cell-text.ts` 190, `number-formats.ts` 81 (+ `uo-numfmt`) | `text` | `layoutCellText`: `####`, General fit, fill/skip, widths from the measurement table |
| `entry.ts` 244, `entry-format.ts` 125, `edit-text.ts` 51, `formula-locale.ts` 35 | `entry` | typed text → value/format/formula (C1 hook for the locale) |
| `cond-format.ts` 438, `cf-formula.ts` 443 (**ported 1:1**, its own evaluator), `cf-icons.ts` 173 | `cf` | per viewport; results memoized per cell with a cap |
| `table-styles.ts` 242 + `table-styles-data.ts` 146, `pivot-styles.ts` 102 | `table_style` | |
| `borders.ts` 127, `fills.ts` 96, `spill-border.ts` 11, `grid-layout.ts` 173 | `paint_rules` | border conflict per edge, pattern masks, merge index, rich lines |
| `text-metrics.ts` 115 | `measure` | `CellFonts` → a measurement table (two-phase, below) |
| `universal-sheet.ts`: `resolveStyle`, `fitRowHeights` (747-…), `recalcGrid` (1172-1184), `colWidth`/`rowHeight` (1142-1149), geometry helpers, `mergeAt`; `cell-format.ts` `xfIndexAt` | `geometry`, `style` | ≈ 900 lines of private methods |

- **Resolved styles**: one `ResolvedXf` per cellXfs index (font face key, size, colours, fill, border ids,
  alignment, number-format code), built once per workbook and extended when the XfPool adds an xf; a cell resolves to
  `ResolvedXf` + table/pivot layer + CF overlay per viewport.
- **Geometry**: the TS prefix arrays are `Float64Array`s of sequential `acc += size × zoom`
  (`universal-sheet.ts:1172-1184`); an arithmetic shortcut over default rows is not bit-equal. `grid::Geometry`
  stores a checkpoint every 256 entries and replays the same sequential additions inside a block: bit-equal
  `edge(i)` for 8/256 bytes per row; `bisectLE`/`pxToIndex` (`grid-layout.ts:16-90`) binary-search the checkpoints
  then scan one block. Built per sheet and zoom, only for the active sheet (as the TS).
- **Row auto-fit at open** (`fitRowHeights`, `universal-sheet.ts:747-…`, walks every cell): folded column-wise over
  the store's style lanes into per-row maxima (a uniform-style chunk applies once to its rows).
- **Text widths** (T2): the GDI tables (`gdiGlyphFixes`) first, then the shell's measurements of the CSS font
  (`text-metrics.ts:54-99`: `'Hg'` font box, `measureText(ch)` rounded per character, whole-string widths for
  complex scripts); a missing entry makes the emission return `NeedMeasures` (two-phase, as Slides; the table
  persists across frames, so steady scrolling never round-trips).

### T7. The viewport display list (frontier C4)

- **List format**: the Slides canvas-IR list (`schema/slides.uos` `SlidesList`, same codec, same TS painter in
  `uo-wasm/js`) — the Sheet paint is a subset of it (`fillRect`, paths, clip, stroke, line dash, linear gradients for
  data bars, pattern fills, `drawImage` for pictures, `save`/`restore`/`translate`/`scale` for RTL). Two new items
  (record `uo-render.md` "Sheet extensions"):
  - `SheetFont(FontRef + italic)`: sets `ctx.font` to the CellFont string `${italic}${400|700} ${ppem}px ${stack}`
    (`text-metrics.ts:58`, `universal-sheet.ts:733`), the stack built by the shell's `FontResolver` (T2);
  - `GlyphRun { text, x, y, advances, min_x, max_x, complex, mirrored }`: the painter replays `drawCellText`
    (`text-metrics.ts:104-130`: one `fillText(ch, cx, y)` per non-space character at integer advances, one
    whole-string `fillText` for complex scripts, the RTL un-mirroring) — one item per text piece instead of one
    `FillText` per glyph (≈ 8,000 items per 1080p viewport otherwise). Op-stream parity is checked after expansion.
- **Emission** follows `paint`/`paintRegion` (`universal-sheet.ts:3222-3261, 3390-…`): up to four regions (frozen
  panes), grid lines first, three phases (fills, texts with overflow, borders + markers), merges crossing the split,
  then drawings/charts (`paintDrawings` 3569: `uo_dml_render::emit_shapes` per anchored object, `uo_charts` for
  charts, form controls as Sheet items), filter arrows, comment indicators, headers (`paintHeaders` 4050), selection
  (`paintSelection` 4003; the selection comes with the request). Scrollbars and the empty-state text stay shell.
- **Per-frame work is viewport-only**: gather the visible columns' chunks into a row-major block, resolve styles, CF
  and tables only for those cells, emit. Scratch buffers are reused across frames (≤ 64 KiB allocated per frame after
  warm-up). Tile caching of lists across scroll steps is evolution after M5 (measured).
- **What crosses the wasm boundary per viewport**: in, the `ViewportRequest` (≈ 64 B); out, the encoded list (budget
  ≤ 48 KiB at 1080p, ≤ 192 KiB at 4K) and, only when a font or zoom is new, one `NeedMeasures` round trip. No cell
  data, no change lists (`uo_calc::engine::Changes` is a per-sheet rectangle + count). Hyperlinks: Write's scheme
  filter (listed).

### T8. Editor session and undo (feature `edit`, frontier C5)

- No bridge: `Session { book: Book, history, … }`; `edit(sheet, col, row, text)` = `entry::parse`
  (`sheet-calc.ts:144-195`) → XfPool derive → `Engine::set_cell(&mut store)` → `calculate` → formula format hint
  (`inferFormulaFormat`) → `Changes`. Bar text, copy text, value text (`sheet-calc.ts:268-381`) are queries on the
  store + engine.
- **Undo = the TS change set**, not a store snapshot (`universal-sheet.ts:1900-2040`): per touched cell the edit
  text + style + existence, restored by RE-ENTERING the text through the same `edit` path (formats and xfs follow the
  same derivation), plus `StructSnap` for cols/rows/merges/panes/grid (`universal-sheet.ts:1968-1983, 4439-4452`).
  100 steps (`UNDO_LIMIT`, 4431) + a byte cap (new; 64 MiB wasm32 / 256 MiB native, oldest dropped first). Plan
  §7's "snapshot undo" is corrected: a snapshot of a 1M-cell store would cost what the change set avoids.
- `addSheet` re-saves and reopens the package (`universal-sheet.ts:2118-2122`): kept (the store is rebuilt by the
  reader).
- Extraction first (C5a): the edit operations are private methods of the 4,593-line element; they are captured by
  scripts driven in Chromium (or the fake DOM) before the port (plan §7 C5).

### T9. Frontiers (track C; models per `SKILL.md`)

| # | Scope | Model | Closed scope | Needs |
|---|---|---|---|---|
| C1a | `read`: model types, package, workbook/styles/theme/SST index, streaming worksheet → store, frame, CF/pivot/table/comments/hyperlinks/drawing-anchor/chart-data readers; `uo-opc` streaming inflate + pull reader | Sonnet | **yes** (this section + `uo-calc.md` S) | S0 |
| C1b | `save`: frame surgery, streamed `<sheetData>`, save-calc, `prepareForSave` glue, XfPool + patch | Sonnet | **yes** | C1a, A5 (save rules) |
| C2 | `xls`: BIFF → OOXML parts | Sonnet | **yes** | C1a |
| C3a | `grid`: metrics, cell text, entry/entry-format/edit-text, geometry, resolved styles, row auto-fit | Sonnet | **yes** | C1a |
| C3b | CF: `cond-format.ts` + `cf-formula.ts` (1:1) + `cf-icons.ts` | **Opus** (formula evaluation) | yes | C1a, A2 |
| C3c | table/pivot styles, borders, fills, spill border, grid-layout helpers | Sonnet | **yes** | C3a |
| C4 | viewport emission + `SheetFont`/`GlyphRun` (uo-render) + drawings/charts hookup + `<universal-sheet engine="rust">` read path + `uo-sheet.wasm` + viewer harness | Sonnet (Opus review of the emission order) | yes | C3*, A5, B4 (done) |
| C5a | extraction of the edit operations into capture scripts | Sonnet | yes | C4 |
| C5b | session, change-set undo, routing, editor harness, `uo-sheet-editor.wasm` | Sonnet; session core reviewed by Opus | yes | C5a, C1b |

Order: S0 → C1a → (C1b ∥ C2 ∥ C3a) → (C3b ∥ C3c) → C4 → C5a → C5b, inside the ≤ 3-agent cap shared with track A.

### T10. Parity (captures)

| Capture | TS entry | Compared |
|---|---|---|
| Model JSON | `readWorkbook`, `readStyles`, `readTheme`, `readWorksheet` minus `rows` | canonical JSON, keys sorted (§14 rule), `Record`-order sites as facets |
| Store facet | the sheet-core model's rows (`uo-calc.md` "Parity") | per cell |
| Save bytes | `saveXlsx` after scripted edits | every part, decompressed, exact |
| `.xls` | the converter's in-memory parts | every part's bytes |
| Cell text | `layoutCellText` with its measurement log | text pieces, x, colour by bits |
| CF | `CfEngine` results per cell over generated CF workbooks | per cell |
| Viewport op stream | a recording context over `paint()` (frozen panes, zoom, RTL, merges, CF, tables, drawings) | canonical op stream after `GlyphRun` expansion; Chromium pixels second |
| Editor | scripted edits + undo/redo (C5a) | save bytes + store facet after each step |

Corpus: the 7 committed workbooks (`sheet/samples`) + the seeded workbook generator of plan §4.3 + generated `.xls`
(Excel COM), open fonts only.

### T11. JS traps (Sheet side; the calc's are in `uo-calc.md`)

| Site | Trap | Helper |
|---|---|---|
| `refToRowCol` (`model.ts:531-538`): `/([A-Z]+)(\d+)/` unanchored, `parseInt` of the row | `$A$1`, `A1B2`, lowercase refs | hand matcher, `uo_jsnum::parse` |
| `parseInt(s)`/`parseFloat(ht)` (`read.ts:575-584, 604`) | NaN printed back as `NaN` by the save | `CellExtra::raw_style`, `uo_jsnum::format` |
| `decodeEntities` (`xml.ts:18-24`) | `fromCodePoint` throws past U+10FFFF; lone surrogates from `&#xD800;` | the pull reader's decision table |
| `String(row.height)` in the save (`serialize.ts:96`) | shortest digits | `uo_jsnum::format` |
| `Math.round` in geometry (`universal-sheet.ts:3417-3421`) | half toward +∞ | `uo_jsnum::math` |
| glyph loop `for (const ch of s)` (`text-metrics.ts:115, 125`) | code points, not units | `uo_text::utf16` |
| `toLocaleLowerCase()` of find (`universal-sheet.ts:2867-2869`) | host locale | root mapping (SH2) |
| CF `each` / replace-all iterating `Map` insertion order (`universal-sheet.ts:4205, 2964`) | order of created rows/cells | order-insensitive (values only); declared, checked by the CF facet |

### T12. Memory (M1–M5) and the M2 soak

M5 caps (Sheet side):

| TS cache | Rust | Cap |
|---|---|---|
| `rowMapCache` (`universal-sheet.ts:241`) | gone (the store is the index) | — |
| `CellFonts.cache` (`text-metrics.ts:34`) + per-font advances | measurement table | 64 fonts × 4,096 characters, cleared on font-set change |
| `cond-format.ts:109` `memo` (per cell, unbounded per sheet) | CF memo | 65,536 cells per sheet, LRU (viewport-driven) |
| `cf-formula.ts:162` parse cache | `cf` | 1,024 formulas |
| `table-styles.ts:49` (WeakMap per `Styles`) | per `Styles` | 256 specs |
| `fills.ts:26` `tileCache` | shell | 64 tiles |
| `chartCache` (`universal-sheet.ts:3528`) | per book | 32 models |
| `drawings.ts:198` `parsed` | per book | 256 objects, LRU |
| `ignoreRects` (`universal-sheet.ts:725`) | per sheet | the sheet's `ignoredErrors` |
| SST entries | data (decoded on demand) | `uo-calc` `Limits` |
| geometry | active sheet × zoom | 1 |
| viewport lists | per view | 2 retained, scratch reused |
| `History` | session | 100 steps + byte cap |

**M1**: open (ok and each error), `load_step`, save, `.xls` convert, every grid query, `emit_viewport` (both phases),
every editor operation + undo/redo — live bytes back after `clear_caches`. **M2 soak (giant-1m, plan §10):** open →
1,000 frames of scrolling (smooth steps, page jumps, zoom changes every 100, a frozen pane toggled every 250), live
bytes sampled every 50 frames: flat after warm-up (the measurement table stops growing), 0 B retained per frame; then
1,000 edits (number, text, formula `=A1*2`, a spilling `=SEQUENCE(5)`, clear, paste of 10×10, undo/redo every 10),
sampled every 50: flat except the history, which is capped; after `clear_caches` and drop, back to the initial
value. In wasm, `memory.buffer.byteLength` stops growing after warm-up. **iOS budget**: peak wasm memory ≤ 64 MiB for
the giant-1m viewer session, ≤ 96 MiB with the 1,000 edits. **M3**: the budgets of T13. **M4**: `Book`, `Session`,
encoded lists, measurement tables: 10,000 × create/drop on both bindings.

### T13. Performance budgets and benchmarks (PERF)

Baselines measured 2026-11-24 (frozen TS in Node 24, this machine, `--expose-gc`): giant-1m read 1.05–1.39 s + cell
index 0.11 s (+ engine 0.56 s when built), retained 228 MB, RSS 969 MB; giant-200k read 0.31 s, retained 52.6 MB,
RSS 377 MB. The browser baselines (open to first paint, scroll frame, edit latency in Chromium) are recorded by the
C4/C5 harness first.

| Benchmark (frontier) | Budget | Fail |
|---|---|---|
| giant-1m open → first viewport list (C1a, C4) | ≤ 250 ms native, ≤ 500 ms wasm; peak live ≤ input + 24 MiB; retained ≤ 8 MiB | > 1.10× previous record, or slower than TS |
| giant-200k open → first list | ≤ 60 ms native / 120 ms wasm; retained ≤ 4 MiB | same |
| load all sheets of a generated 20-sheet workbook (C1a) | ≤ 0.3× TS read | > 1.0× TS |
| save after one edit, giant-1m (C1b) | ≤ 400 ms native / 800 ms wasm; peak ≤ output + 16 MiB | > 1.0× TS |
| `.xls` conversion (C2) | ≤ 0.5× TS | > 1.0× TS |
| cell text layout, 100k cells (C3a); CF over 1M cells with a colour scale + a formula rule, per viewport (C3b) | ≤ 0.5× TS | > 1.0× TS |
| viewport emission 1080p / 4K (C4) | ≤ 1 ms / 4 ms native, ≤ 2.5 ms / 8 ms wasm; ≤ 48 / 192 KiB | > 1.10× previous |
| scroll frame total in Chromium (emit + decode + paint), 1080p (C4) | ≤ 8 ms p95 and ≤ the TS `paint()` | slower than TS |
| edit latency (entry → store → list ready), giant-1m (C5) | ≤ 16 ms p95 wasm | slower than TS |
| paste 10k cells / fill 100k / replace-all 1M / undo of each (C5) | ≤ 0.5× TS | > 1.0× TS |

Harness: `tools/bench/speed/run.mjs` `sheet` suite (TS frozen dist × rust-core native/wasm, Node and Chromium),
results in this record per frontier; a frontier does not close with a failing row unless the maintainer approves it.

### T14. Security and fuzz

Limits: `uo_opc::Limits` (zip bombs, part sizes), `uo_calc::store::Limits` (cells, sheets, formulas, strings),
`book::ReadLimits` (sheets, nesting depth with an explicit stack instead of the TS recursion), CF rules, merges,
drawing objects, history bytes. Fuzz: `xlsx_open` (bytes → open + load all + one viewport), `xlsx_sheetdata`
(structure-aware mutation of `<sheetData>`: order, duplicates, refs, types, values, entities), `xlsx_roundtrip` (open
→ random edits → save → reopen: store facet equal), `xls_convert`, `cf_formula`, plus `uo-calc`'s targets.

### T15. Wasm split (plan §9 budgets)

| Module | Contents | Budget (gzip) |
|---|---|---|
| `uo-sheet.wasm` | `uo-xlsx` read + grid, `uo-calc` (open-recalc needs it), `uo-numfmt`, `uo-dml-render`, `uo-charts`, the uo-render Slides+Sheet encoder | ≤ 1.6 MB |
| `uo-sheet-editor.wasm` | the above + save + edit + exporters | ≤ 3.5 MB |
| `uo-xls.wasm` | `.xls` → parts (lazily loaded, separate instance) | not in the viewer budget |

### T16. Questions for the M5 gate (batched; SH1, SH2, SH4 in `uo-calc.md`)

- SH3: `uo-sheet.wasm` holds the calc for the open-recalc rule; if it exceeds 1.6 MB gzip, load the calc lazily
  (fetched only when the open plan is not `none` or at the first edit) or raise the budget?
- SH5: the history byte cap (64 MiB wasm32 / 256 MiB native, oldest steps dropped) on top of the 100 steps — accept?
- SH6: `uo-xls.wasm` as a separate lazily loaded instance (the same question as Slides' SP4) — accept?
