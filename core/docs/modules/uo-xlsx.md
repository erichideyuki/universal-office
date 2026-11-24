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
  chosen for a custom code (their display changes with the reader's locale). `Style::builtin_num_fmt: Option<u32>`
  (2026-11-24, asked by the data-grid's `grid-xlsx`) writes a built-in id explicitly, as is (14 short date, 22 date
  and time in the reader's regional settings): no `<numFmts>` entry, it wins over `num_fmt`, a date cell keeps it
  (no implied 14/22), and an id `uo_numfmt::builtin_format` does not know is `Error::NumFmt`.
- **AutoFilter** (2026-11-24, asked by `grid-xlsx`): `XlsxWriter::set_auto_filter(AutoFilter { range, sort:
  Vec<SortKey { col, descending }> })` while the sheet is open (the last row is known only at the end; a second call
  replaces the first). `end_sheet` writes `<autoFilter ref>` right after `</sheetData>` (before `<mergeCells>`,
  CT_Worksheet order) with a `<sortState ref="range minus the header row">` of one `<sortCondition>` per key, in order
  (omitted without keys or without a data row); `finish` adds the hidden `_xlnm._FilterDatabase` name
  (`localSheetId`, absolute range), sorted with the other names. `SortKey::col` is 1-based and absolute like
  `CellRange`. A range outside the sheet, a key outside the range or more than `MAX_SORT_KEYS` (64) keys is
  `Error::InvalidRange` (no new error variant, so exhaustive matches downstream keep compiling).
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
| `table-styles.ts` 242 + `table-styles-data.ts` 146, `pivot-styles.ts` 102 | `table_style` (+ `data.rs` generated, `pivot.rs`) | specs per style name once per sheet; the format of a cell on demand (no per-cell maps) |
| `borders.ts` 127, `fills.ts` 96, `spill-border.ts` 11, `grid-layout.ts` 173 | `paint_rules` (+ `paint` for the layers and the border pass) | border conflict per edge, pattern masks, rich lines (the merge index and `bisectLE` were C3a's) |
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
| C1a (**done** 2026-11-24, section "C1a — as built") | `read`: model types, package, workbook/styles/theme/SST index, streaming worksheet → store, frame, CF/pivot/table/comments/hyperlinks/drawing-anchor/chart-data readers; `uo-opc` streaming inflate + pull reader | Sonnet | **yes** (this section + `uo-calc.md` S) | S0 |
| C1b (**done** 2026-11-24, section "C1b — as built") | `save`: frame surgery, streamed `<sheetData>`, save-calc, `prepareForSave` glue, XfPool + patch | Sonnet | **yes** | C1a, A5 (save rules) |
| C2 (**done** 2026-11-24, section "C2 — as built") | `xls`: BIFF → OOXML parts | Sonnet | **yes** | C1a |
| C3a (**done** 2026-11-24, section "C3a — as built") | `grid`: metrics, cell text, entry/entry-format/edit-text, geometry, resolved styles, row auto-fit | Sonnet | **yes** | C1a |
| C3b (**done** 2026-11-24, section "C3b — as built") | CF: `cond-format.ts` + `cf-formula.ts` (1:1) + `cf-icons.ts` | **Opus** (formula evaluation) | yes | C1a, A2 |
| C3c (**done** 2026-11-24, section "C3c — as built") | table/pivot styles, borders, fills, spill border, grid-layout helpers | Sonnet | **yes** | C3a |
| C4 (**done** 2026-11-24, section "C4 — as built"; drawings/charts and the wasm engine glue are open items) | viewport emission + `SheetFont`/`GlyphRun` (uo-render) + drawings/charts hookup + `<universal-sheet engine="rust">` read path + `uo-sheet.wasm` + viewer harness | Sonnet (Opus review of the emission order) | yes | C3*, A5, B4 (done) |
| C5a (**done** 2026-11-24, section "C5a — as built") | extraction of the edit operations into capture scripts | Sonnet | yes | C4 |
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

### T10a. Captures (harnesses built 2026-11-24)

Scripts in `core/crates/uo-xlsx/js/` (Node only; no Office, no browser window). The frozen TS runs headless: the
real `<universal-sheet>` element on the fake DOM of the TS's own editing tests (`write/test-support/fake-dom.ts`),
bundled from the TS sources by `harness/build-harness.mjs` (esbuild of the frozen checkout, the aliases of its
`build:demo`; `vitest` → `harness/vitest-shim.mjs`) into `core/target/sheet-cap/harness/`. Host injection (C1):
`TZ=UTC`, `Date` frozen at 2025-01-15 12:00 UTC, `Math.random` = a seeded xorshift32 recorded in call order.
Captures are deterministic (two runs give identical files, checked). The calc groups (bank, calc-traps, calc-tests,
calc-order) are in `uo-calc.md` "Captures".

| Group | Command | Committed (CI) | Local run | Covers |
|---|---|---|---|---|
| workbook generator | `node core/crates/uo-xlsx/js/gen-workbooks.mjs --gen x1` (`--check`: committed == regenerated; `--stats`; `--heavy <rows>`) | `tests/captures/workbooks/x1/`: 30 `.xlsx`, 291 KiB; every feature in ≥ 5 workbooks | `--gen x1-big` (400 workbooks, ≤ 120 rows, ≤ 4 sheets) into `core/target/sheet-cap` | seeded (xorshift32), byte-stable; features: styles, numFmt (custom + built-in), CF (cellIs, 2/3-colour scales, data bar, icon sets, expression, top10, duplicates, containsText), merges, frozen panes, defined names (global, local, `_xlnm.Print_Area`, undefined name), tables + structured refs, cross-sheet formulas (quoted non-ASCII names), shared formulas, CSE arrays, spills + `metadata.xml` (and a `#SPILL!` blocker), volatiles, cycles (+ iterative calcPr), drawings (shape) and a bar chart, rich SST runs, inline strings, non-canonical `<v>` (`1.50`, `1E3`, ` 7`, `-0`, `Infinity`, `0x10`…), rows/cells out of order and duplicated, `<c r>` on another `<row r>` (SH1), cells/rows without `r`, errors, booleans, dates (+ date1904), hidden rows/cols, widths/heights, formula cached values (absent, stale, typed, `ca`), calcPr variants (old calcId, fullCalcOnLoad, manual, forceFullCalc, none), astral/combining/lone-surrogate entities, columns past Z. `CALC_CLASSES`/`buildCalcHeavy`: the benchmark workbooks |
| `sheet-x1` | `node core/crates/uo-xlsx/js/capture-sheet.mjs --gen x1 --workbooks core/crates/uo-xlsx/tests/captures/workbooks/x1` | `sheet-x1.capture` 99 KiB + `.side.gz` 550 KiB (30 workbooks, 8 edit steps each) | `--gen x1-big --out core/target/sheet-cap/sheet-x1-big.capture`: 400 workbooks in 91 s, 1.4 MiB + 25 MiB sidecar (2026-11-24; no part XML unless `--parts`) | model JSON with sorted keys (`readWorkbook`, `readStyles`, theme, `readSharedStrings`, each `readWorksheet` minus rows), the store facet per sheet (component index rule: `<row r>` row, last duplicate wins) + file order + row metadata, the open-recalc plan (17 full / 4 none / 9 partial in x1), evaluation order + RAND values + `circular` + `internalErrors` of the open, engine values, the model after the open's write-back, display text per sheet (`debugCellDisplays`, the Range.Text equivalent), surgical save after a no-op (decompressed part digests + the changed parts' XML in the sidecar), and 8 seeded edit steps through the element's public API (text and formula entry in pt-BR, font, number format, borders, fill, merge, fill down/right, clear, freeze, width, hide, replaceAll, autoSum, copy/paste, undo, redo, recalculate) with the order/RAND/save/display after each |
| `sheet-samples` | `node --max-old-space-size=8192 core/crates/uo-xlsx/js/capture-sheet.mjs --samples` | `sheet-samples.capture` 18 KiB + `.side.gz` 232 KiB (7 samples; giant-1m 7.8 s, giant-200k 1.9 s) | — | the same facets; for giant-1m/giant-200k the digests cover every cell but the sidecars keep the first 200 rows (store, display of A1:AN200) and no edit steps |
| `sheet-cf` (C3b) | `node core/crates/uo-xlsx/js/capture-cf.mjs` (`--write-workbooks` rewrites `workbooks/cf1`) | `sheet-cf.capture` 6.5 KiB + `.side.gz` 42 KiB; `workbooks/cf1/` 40 `.xlsx`, 200 KiB | `--big`: 300 `cf-big` + 400 `x1-big` → `core/target/sheet-cap/cf-big.capture` (5 s) | per CF sheet the `CfResult` of every cell of its ranges, `evalFormula` traps + 3,000 seeded formulas, icon/bar canvas calls ("C3b — as built") |
| `sheet-paint` (C3c) | `node core/crates/uo-xlsx/js/capture-paint.mjs` (`--write-workbooks` rewrites `workbooks/paint1`; `--lines` keeps the border/display lines) | `sheet-paint.capture` 35 KiB + `.side.gz` 188 KiB; `workbooks/paint1/` 14 `.xlsx`, 218 KiB | `--big`: 160 `paint-big` + 400 `x1-big` → `core/target/sheet-cap/paint-big.capture` (15 s) | per sheet the table layer, filters, resolved specs, final `styleAt`, `paintCellFill`/`paintDiagonals` calls, the `fillRect`s of the real `paintGridBorders`, displays through `styleAt`; the pure helpers ("C3c — as built") |
| `--dir <folder>` | `capture-sheet.mjs --dir <folder>` | — | any folder (the `.xls` hook) | the same facets over arbitrary files |

Not captured here, and why: `.xls` (captured by frontier C2 with `js/capture-xls.mjs`, section "C2 — as built"; the generator writes `.xlsx` only and Excel refuses its hostile workbooks, hence the `xls1` group); the
viewport op stream (frontier C4: a recording context over `paint()` → `paintRegion`, `universal-sheet.ts:3257-3260`,
and `renderRegionToCanvas`, 3340); fonts outside the measured GDI tables (the fake canvas measures 0, so families the
component would measure in the browser — `mdwFromCanvas` — get the table fallback here; open fonts only in the
committed groups); the async font loading of the element (never awaited by the capture); copy/paste through the OS
clipboard (the script uses the element's internal clipboard); Excel's own recalculation of the generated workbooks
(`xl-calc.ps1`, needs COM).

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
| `cond-format.ts:109` `memo` (per cell, unbounded per sheet) | CF memo | 65,536 cells per sheet (open-addressing table, slot replaced when full; C3b-5) |
| `cf-formula.ts:162` parse cache | `cf` | none: each rule's formulas are compiled once at build (C3b-1); 16,384 rules per sheet |
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

**TS baseline, sheet suite (measured 2026-11-24, the numbers to beat).** `node core/tools/bench/speed/sheet.mjs`
(built beside `run.mjs`, which is docx-specific; TS only today, the rust-core engines join per frontier), frozen TS
`b92b60fa`, Node 24.15, each scenario in its own `--expose-gc` process, 1 warm-up + 3 rounds, medians; retained = heap
after GC with the result alive minus before. Results: `core/tools/bench/speed/results/sheet-ts-2026-11-24.{json,md}`;
the calc classes are in `uo-calc.md` "TS baseline".

| Scenario | Workbook | ms (median / min) | Retained MB | RSS MB after |
|---|---|---|---|---|
| open, sheet-core read (every part, every sheet) | giant-1m (1M cells) | **994** / 911 | 125 | 1,075 |
| | giant-200k | **212** / 209 | 27.2 | 397 |
| | calc-heavy-20k (129k cells, 49k formulas) | **176** / 166 | 19.3 | 334 |
| open, the `<universal-sheet>` element (`loadXlsx`: model, index, geometry, open-recalc) | giant-1m | **1,524** / 1,475 | 686 | 1,949 |
| | giant-200k | **394** / 343 | 150 | 610 |
| | calc-heavy-20k | **1,330** / 1,321 | 352 | 828 |
| save after one edit (`saveXlsx`, surgical) | giant-1m | **997** / 993 | — | 1,120 |
| | giant-200k | **214** | — | 469 |
| edit latency (`setActiveCellText`, engine built), calc-heavy-20k | head of a 20k chain | p50 **604**, p95 611 | — | 513 |
| | isolated cell | p50 **1.7**, p95 4.1 | — | — |

The element builds the calc engine at open on both giants (their open-recalc plan is `full`: the files do not pass
the "saved by a recent Excel" tests of `open-recalc.ts`), which is why its retained heap (686 B/cell on giant-1m) is
far above the 239 B/cell of S1 in `uo-calc.md` (model + index + engine only).

### T14. Security and fuzz

Limits: `uo_opc::Limits` (zip bombs, part sizes), `uo_calc::store::Limits` (cells, sheets, formulas, strings),
`book::ReadLimits` (sheets, nesting depth with an explicit stack instead of the TS recursion), CF rules, merges,
drawing objects, history bytes. Fuzz: `xlsx_read` (built in C1a, `core/fuzz/src/xlsx_read.rs`: package bytes, or one mutated part in a minimal package → open + load all + every shared string + a viewport; the plan's `xlsx_open`), `xlsx_sheetdata`
(structure-aware mutation of `<sheetData>`: order, duplicates, refs, types, values, entities), `xlsx_roundtrip` (open
→ random edits → save → reopen: store facet equal), `xls_read` (built in C2), `cf_formula`, plus `uo-calc`'s targets.

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


---

## C1a — as built (2026-11-24, Sonnet 5.5)

Frontier C1a of T9: the reader (feature `read`) and its two shared pieces in `uo-opc`. The default build (no feature) is the
writer, byte for byte as before (`grid-xlsx` pins it: the four writer files are untouched; `lib.rs` gains one `cfg`ed
module line, `Cargo.toml` the features and an example with `required-features`). Everything below was verified against the
frozen TS (`ts-ref-092` at `b92b60fa`) through the captures of T10a.

### Code

| Piece | Where | TS source |
|---|---|---|
| Streaming inflate | `uo-opc` `flate::Inflater` (resumable, 43 KiB of state), `Package::stream_part` → `PartStream::next_chunk` (64 KiB pieces, part cap and total counted, declared size truncates) | `fflate` `unzipSync` |
| XML pull tokenizer | `uo-opc` `xml::pull` (`Pull::feed`, `Token`, `StartTag::attrs`, `decode_text`/`decode_attr`/`decode_cdata` to UTF-16, `is_plain`) | `sheet-core/src/xml.ts` |
| Model types | `read/model.rs` (`Workbook`, `Styles`, `Theme`, `Worksheet`, `CondRule`, `DrawingObject`, `TableInfo`, `PivotInfo`, `XEl`…) | `model.ts`, `styles.ts`, `theme.ts` |
| Workbook, rels, external links, dynamic-array `cm` | `read/workbook.rs` | `read.ts:59-217`, `package.ts` helpers |
| Theme, indexed palette, `resolveColorEl`, `applyTint` (Windows integer HLS) | `read/theme.rs` | `theme.ts` |
| Styles, dxf, fills, borders | `read/styles.rs` | `styles.ts` |
| Shared strings: element tree from tokens (`MEl`/`MiniBuilder`), `readSharedStringEntry`, run properties table, the INDEX | `read/sst.rs` | `read.ts:21-34, 219-282` |
| Conditional formats (classic + x14) | `read/cf.rs` | `read-cf.ts` |
| Drawing: anchors, charts (raw XML kept), objects as `XEl`, OLE previews, form controls | `read/drawing.rs` | `drawing.ts` |
| Worksheet minus rows: merges, cols, pane, flags, hyperlinks, comments, tables, pivots, validations, autofilter, chart-sheet view | `read/sheet.rs` | `read.ts:285-571`, `read-pivot.ts` |
| Streaming `<sheetData>` → store | `read/stream.rs` (`read_sheet`) | `read.ts:573-635` |
| `Book` (open, active sheet first, `load_sheet`/`load_step`/`load_all`, `resolve_sst`) | `book.rs` | `universal-sheet.ts` open path |
| Canonical JSON of the model (feature `facet`) | `read/facet.rs` | `canon-json.mjs` |

### How a worksheet is read

`Package::stream_part` inflates 64 KiB at a time, `Pull` tokenizes, a `Sink` state machine (depth-tracked, no stack of
elements) takes `<row>`/`<c>`/`<v>`/`<f>`/`<is>` and feeds `SheetLoader` directly. The bytes before `<sheetData` and after
`</sheetData>` are collected raw (the frame); the part is never held whole and no tree of it exists. After the stream the
frame (prefix + the original `<sheetData…/>` tag + suffix) is parsed with the DOM reader for the sheet model. A canonical
cell (plain number, `1`/`0`, error text, shared-string index; no formula, no `<is>`, no `cm`/`vm`, `s` an integer, `r`
absent or equal to the position) takes `SheetLoader::cell_raw` (lanes only: no `String`, no allocation); everything else
builds a `FileCell`. `<is><t>text</t></is>` has a fast shape without an element tree; anything else under `<is>` builds the
`MEl` tree (replaying what the fast shape had seen). `Book::open` reads package, workbook, styles, theme, the SST index
and the active sheet; other sheets load on demand.

### Decisions (declared; the observable model equals the TS on every capture)

| Id | Decision |
|---|---|
| C1a-1 | **Lazy package**: entries inflate when read. The TS `unzipSync` inflates every entry at load, so a damaged unrelated entry fails the TS open; here it fails when that part is read. |
| C1a-2 | **Streaming inflate = `miniz_oxide`'s `InflateState`**, not the fflate port (that port is whole-buffer). Same bytes (differential test, any piece sizes); on a damaged stream the error comes where the damage is, not before the first byte; a declared size truncates, as `inflate_raw`. |
| C1a-3 | Texts of cells, inline strings and shared strings are UTF-16 (`JsString`): `&#xD800;` stays a lone surrogate (tested against the capture). Strings of the model read through the `uo-opc` DOM (sheet names, attributes, formulas of CF…) are UTF-8: a lone-surrogate reference there reads as U+FFFD (the existing lenient DOM's declared deviation; no capture exercises it). |
| C1a-4 | Numbers the TS gets from `parseInt`/`parseFloat` are `f64` in the model (NaN kept, printed `{"$num":"NaN"}`); `?:` booleans the TS sets only when true are `bool`; lists it sets only when non-empty are `Vec`s. |
| C1a-5 | Drawing objects, theme fill styles and OLE/control pictures are owned `XEl` trees (T2 said "a subtree of the frame's DOM": the frame's DOM is dropped after the model is read, which keeps the 8 MiB retention). |
| C1a-6 | **Cells outside A1:XFD1048576 are dropped and counted** (`Book::dropped_cells`; S0-8: the store refuses them; the TS model keeps them). A `<row r>` past 1,048,576 drops its cells. |
| C1a-7 | `load_step(budget)` loads whole sheets: the step ends after the sheet that crossed the budget. `SheetLoad::Streaming` is reserved (resuming inside a sheet needs the loader's state to outlive the borrow of the store). |
| C1a-8 | `RowMeta.spans` goes in as the empty-string placeholder while streaming (the loader borrows the store, so the pool is out of reach) and is patched after the sheet (`patch_spans`, distinct strings interned once, both holders of a `FileOrder` sheet patched). **Proposal for uo-calc (not applied: another agent works there)**: `SheetLoader::strings(&mut self) -> &mut StrPool` or `row_with_spans(row, meta, Option<JsStr>)`; then the patch goes away. |
| C1a-9 | The shared-string INDEX records `[start, end)` of each `<si>` in the decompressed part; the part's bytes stay in the package (`Package::find` keeps what it inflates) and entries decode on demand by pulling the slice. Retained cost: the decompressed part, not a decoded copy. |
| C1a-10 | `SheetFrame` gains `data_name`, `data_attrs`, `has_sheet_data`, `data_self_closing`: `prefix` is the text before the `<sheetData` TAG, whose qualified name (`x:sheetData`) the save re-emits (the TS regex `<sheetData[\s/>]` of `updateWorksheetXml` does not match a prefixed tag). Trailing content after `</sheetData>` is validated by the frame's DOM parse only. |
| C1a-11 | Limits added to `ReadLimits`: `max_shared_strings` (8 Mi native / 2 Mi wasm32; the index costs 8 B and the store's table 16 B per entry) and `max_cell_text` (4 MiB of bytes of one `<v>`, `<f>` or `<is>`; Excel holds 32,767 characters). The pull tokenizer's pending token is capped by the latter. |
| C1a-12 | **Not ported: `chart-data.ts` (333 lines).** It is not on `readWorksheet`'s path: the TS keeps the chart XML raw (`DrawingChart.chartXml`, kept here the same way) and the chart engine parses it on first paint (T3.5). Frontier C4 calls `uo-charts`' own parser. |
| C1a-13 | Open-recalc (`shouldRecalcOnOpen`, the `plan` facet) is not here: it needs every formula and the calc (C1b/A5). |

### Parity (`cargo test -p uo-xlsx --features facet --test read_parity`)

Per workbook: `read`, `wb`, `styles`, `theme`, `sst` (every entry decoded, rich runs and `rPr` included), `sheets`, and per
worksheet `ws<i>` (the whole model minus rows, canonical JSON, sorted keys), `st<i>` (the store facet of every cell),
`fo<i>` (file order), `rw<i>` (row attributes).

| Run | Input | Result |
|---|---|---|
| CI (`x1_model_and_store`) | the 30 committed generated workbooks: 65 sheets, 6,510 cells | 440 facets equal, **0 mismatches** |
| CI (`samples_model_and_store`) | 5 samples (giants skipped) | 106 equal, **0 mismatches** |
| `UO_XLSX_BIG=1` (release, 1 s) | + `giant-1m` and `giant-200k`: 7 workbooks, 21 sheets, **1,200,304 cells** | 126 equal, **0 mismatches** |

All of it passed at the first complete run (no round with the parity verifier: the harness was built beside the port; the
corrections found afterwards were performance and limits, below). Hand-made corners that no capture reaches are
`tests/read_hostile.rs`: `x:`-prefixed SpreadsheetML + BOM, `<c>` without `r`, CDATA in `<v>`, `_x0042_`, trimming and
`xml:space`, `&#xD800;`, `&#13;`, the `<is>` shapes (two `<t>`, nested elements, runs), a cell past XFD, a row past 1,048,576,
`$C$1` (no reference: the position after the previous cell), duplicates (last wins) and the file order. Not captured: `.xls`
(C2), the open-recalc plan, the viewport.

### Memory (`tests/read_memory.rs`, `uo-alloc` counting allocator)

| Gate | Result |
|---|---|
| M1 | open + load every sheet + decode every shared string + drop: live bytes back to exact zero for the 30 x1 workbooks and 5 samples; 72 damaged packages (truncated, bit-flipped) leave nothing behind (some are refused, some open) |
| M2 | open/drop of one workbook, 300 times: flat at tolerance 0 B |
| M3 (release, `UO_XLSX_BIG=1`) | **giant-1m: retained 4,494,708 B = 4.49 B/cell = the store's lanes (4,485,184) + 9,524 B** (frames, model, indexes); peak above start 5.11 MB; 12.3 MB allocated in total. giant-200k: retained 2,327,815 B (11.6 B/cell, strings pool included), peak 2.66 MB. 20-sheet workbook (840k cells): retained 13.2 MB (lanes 12.7 MB), peak 13.7 MB |

### Performance (native, i5-14600KF, release, medians of 5; `examples/read_bench.rs`; TS = the baseline of T13)

| Benchmark (T13 row) | Budget | TS | Rust | Verdict |
|---|---|---|---|---|
| giant-1m: open (zip, workbook, styles, theme, SST index, active sheet streamed) → first viewport gather (45×15: 5 µs) | ≤ 250 ms native, ≤ 500 ms wasm; peak ≤ input + 24 MiB; retained ≤ 8 MiB | 994 ms | **162 ms** (158–165); peak above start 5.1 MB; retained 4.49 MB | within (6.1× faster than TS); wasm not measured |
| giant-200k (28,000 inline strings, styles) | ≤ 60 ms native, ≤ 120 wasm; retained ≤ 4 MiB | 212 ms | **49.2 ms**; retained 2.33 MB | within (4.3×); wasm not measured |
| 20 generated sheets × 3,000 rows × 14 cols (840k cells, numbers/shared/inline/styled, formulas), load all (`js/bench-20sheets.mjs`) | ≤ 0.3× TS | 976 ms | **242 ms = 0.25×** | within (thin margin) |

Phases of `giant-1m`'s worksheet (31.7 MB inflated, 5.02 M tokens): inflate alone 15 ms, inflate + tokenize 68 ms, the rest
(attribute parse, lanes, store fill) ≈ 90 ms. History of the numbers (the 10% rule starts here): first version 282 ms /
101 ms; allocation-free cell references and `s`/`t` fast paths: 159 / 60 ms; `<is><t>` without an element tree: 162 / 49 ms
(giant-200k allocated 34 MB before, now a fraction).

### Security and fuzz

`tests/read_hostile.rs`: 20,000 nested elements in a cell → `Limit XmlDepth` (explicit stack, no recursion anywhere); 5,000 in
`styles.xml` → the DOM's depth limit; 5,000 `<si/>` over `max_shared_strings` → `Limit`; 300 KB and 10,000 CDATA pieces over
`max_cell_text` → `Limit`; a 64 MiB part of spaces under a 4 MiB cap → `Limit` with a peak of 8.5 MB (the pending text token
with its doubling; the part is never allocated); damaged XML, a bad character reference, no workbook part, junk zip → typed
errors. `uo-opc/tests/stream_part.rs`: the `Inflater` equals `inflate_raw` for any input/output piece size (stored, fixed and
dynamic blocks, 0 to 300 KB), truncated and garbage streams are `Corrupt`, `stream_part` equals `part`, the part cap and the
total are enforced, a bomb stops at the cap. Fuzz target **`xlsx_read`** (`core/fuzz/src/xlsx_read.rs`): the stable fallback driver (`cargo run --release --no-default-features --features fallback --bin fallback -- --secs 170 xlsx_read`, 127 seeds: the 30 generated workbooks as packages and as single parts, the minimal package and hostile corners): **5,431,605 executions in 170 s, 0 crashes, slowest input 6.6 ms** (memory bounded by the target's own limits: 8 MiB parts, 64 Ki shared strings, 64 KiB of cell text, depth 200). No `unsafe` in the reader; Miri not applicable.

### Lint and files

`clippy -D warnings` (native and wasm32, `--all-targets --all-features`) clean on `uo-opc` and `uo-xlsx` (also without
features), and on the dependents of `uo-opc`: `uo-docx`, `uo-pptx`, `uo-pdf`, `uo-fonts`, `uo-image`, `uo-drawingml`,
`uo-charts`, `uo-dml-render`, `uo-docx-layout`, `uo-wasm`, `uo-ffi`, `uo-calc` (native); `tests/grep-bans.sh` and
`float-casts.sh` clean; `rustfmt` on the new files. `uo-opc`'s own suite (18 unit tests and every integration binary, plus the 5 new) passes; the tests of the dependents `uo-calc`, `uo-fonts`, `uo-image`, `uo-pptx` and `uo-docx` ran in the snapshot (988 passed, 54 failed): every failure reads a fixture that the snapshot/worktree lacks (the open fonts of `shared/fonts/files` are not synced here, `site/assets/samples`, `write/…` documents), and none touches the changed code; the one numeric difference, uo-pptx `links` (943 of 945 run-URL boxes), is text measured without the open fonts. Not compared with a pristine baseline run.

**Process metrics.** C1a (Sonnet 5.5, closed scope): one session, about 55 minutes of wall time, about 150 tool calls, no
round with a verifier. Rework inside the session: (1) performance, 282 → 162 ms (cell references and attributes allocated per
cell; found with the phase timings of `read_bench`); (2) `read_sheet` leaked a `Box::leak`ed prefix list per inline cell in
the first draft (caught by reading it back, before M1 ran: the builder takes the prefixes per call now); (3) the first
`rustfmt` of `lib.rs` also reformatted the writer's four files (`mod` declarations are followed): reverted with
`git checkout`, the writer is untouched. Environment note: `uo-calc` was mid-edit by frontier A3 (a module missing), so the
build ran in a snapshot of `HEAD` (`git archive`) with the files of `uo-opc`/`uo-xlsx`/`fuzz` copied in; nothing in `uo-calc` was touched.

## C1b — as built (2026-11-24, Sonnet 5.5)

Frontier C1b of T9: the surgical save of an opened workbook (feature `save`, module `src/save/`). Behaviour source: the frozen TS `saveXlsx`
(`universal-sheet.ts:909-928`), `serialize.ts`, `save-calc.ts`, `calc-io.ts` and the XfPool of `sheet-calc.ts`; the calc rules are `uo-calc`'s
`save::prepare_for_save` (A5). Verified against the `sheet-x1` and `sheet-samples` captures (save facets) — below.

### Code

| Piece | Where | TS source |
|---|---|---|
| API, orchestration (`save`, `SaveRequest`, `Saved`, `SaveError`, `sheet_xml`) | `save/mod.rs` | `saveXlsx` |
| `<sheetData>` streamed from the store (row/cell/formula XML, `FileOrder` replay, created cells placed like `createCell`) | `save/sheet_data.rs` | `serialize.ts:30-109`, `sheet-calc.ts:202-222` |
| Frame surgery: `<cols>`, `<mergeCells>`, first `<sheetView>` (pane, gridlines) | `save/frame.rs` | `serialize.ts:138-254` |
| Calc parts: calc chain, `<calcPr>`, `metadata.xml` (XLDAPR), defined names | `save/calc.rs` | `save-calc.ts` |
| XfPool (`apply`, `derive`, `format_id`, `quote_prefix`, `patch`) and its change types | `save/xfpool.rs` | `sheet-calc.ts:511-853` |
| The engine of an opened book (`build_engine`, `engine_input`, `open_recalc_plan`, `open_recalc`) | `save/engine.rs` | `fromSheetCore`, `SheetCalc.ensure/reset` |
| JS text helpers (UTF-16 to UTF-8, `escXml`/`escAttr`/`encodeXstring`, JS number print) and the regex wrapper | `save/text.rs`, `save/rx.rs` | — |
| `Package::save_with` + `HostEntries` (the host writes some entries through the `ZipWriter`; the rest as `save`: verified compressed entries raw) | `uo-opc/src/package.rs` | `XlsxPackage.save` |

### API for C5 (and C4)

- `uo_xlsx::save::save(&mut Book, &SaveRequest { dirty_sheets, merge_dirty, view_dirty, pool, level }) -> Result<Saved, SaveError>`. Sheet numbers are
  workbook sheet indexes. `dirty_sheets` = the TS `dirtySheets` **plus the sheets the calc wrote** (the TS `touched` hook marks them: use
  `Changes.sheets` of every `calculate`/`recalc`/open-recalc); `merge_dirty`/`view_dirty` as the TS sets. With `book.engine` present `prepare_for_save` runs
  for the dirty sheets (`ca`/`aca`, `cm`, shared orphans, `fullCalcOnLoad` when `!sure`), its `touched` sheets are rewritten too; the calc chain goes
  whenever a sheet is rewritten (also without an engine, as the TS). `Saved { bytes, rewritten, sure }`.
- `XfPool::new(&book.styles)` once per open; `apply(&mut styles, from_xf, &StyleChange)`, `derive`, `format_id`, `quote_prefix`; the pool appends to
  `book.styles` as the TS does and `SaveRequest.pool` patches `styles.xml` at save (idempotent: a second save does not duplicate).
- `save::engine::build_engine(&mut book, &EngineOptions)` (loads every sheet, fills merges and filtered rows, hands names/tables/externals/calc options to
  the engine) and `open_recalc(&mut book, &opts, clock, &mut random) -> (OpenRecalcPlan, Changes)`.
- `calc::{ensure_dynamic_array_cm, remove_calc_chain, update_calc_pr, update_defined_names}` are public for the session (defined names).
- A `FileOrder` sheet (rows/cells out of order or duplicated in the file) is written in the TS order; a live cell the file order does not list (created
  after the open) goes into the first element of its row at the place `createCell`'s binary search finds, or into a row element of its own. C5 must keep
  `RowMeta` edits of such a sheet in `FileRow::meta` too (the save writes an element's own meta there, `sh.rows` for canonical sheets).

### Decisions (declared; the observable output equals the TS on every capture)

| Id | Decision |
|---|---|
| C1b-1 | **Prefixed `<x:sheetData>` is a typed error on save** (`SaveError::Frame`), as the TS (`<sheetData[\s/>]` does not match, it throws); an untouched sheet is copied. C1a-10's re-emit of the qualified name is not used: the rows are unprefixed in both. |
| C1b-2 | **The text surgery runs on the same ECMAScript regex engine as uo-calc** (the vendored `regress`, new optional dependency of feature `save`), with the TS patterns: `save-calc.ts`, the XfPool patch and `updateWorksheetXml` match exactly what the TS matches (first occurrence, `\b`, lazy `[^>]*?`, JS `\s`) by construction. A part with an unclosed block the TS would garble (`indexOf` = -1) is `SaveError::Frame`. |
| C1b-3 | The frame surgery runs on `prefix + <sheetData/> + suffix` (the frame, a few KB), then the streamed `<sheetData>` replaces the sentinel: no search ever sees the data. After a save the frame holds the patched text (a second save starts from it, as the TS package does). |
| C1b-4 | **Canonical cells never allocate**: a cell without extra or formula (the 99% case) is written straight from the lanes (`cell_fast`); anything else goes through `Store::file_cell` and the general `cellXml` port (`cell_file`). Rows come from `Store::each` merged with the row metas; a row is kept when it has cells, `ht`, `hidden` or `s` (`serializeSheetData`). |
| C1b-5 | `t="${c.type}"` is written unescaped, as the TS (a hostile `t` with a quote gives a part that does not parse); the fuzz target accepts it (read back is best effort). |
| C1b-6 | The reader writes `RowMeta.spans` through `SheetLoader::strings()` (A4a's one-line change): `patch_spans`, the placeholder and `uniq` are gone, a one-entry cache (own reference) avoids re-interning per row. The save recomputes `spans` (as the TS), so the stored text only feeds the `rw<i>` facet. `<col style>` now goes into the store (`Store::set_col_styles`, C3c open item 1): `default_style` answers the file's value. |
| C1b-7 | `Package::save_with` is the only change in `uo-opc`: the planner skips the host's names (their parts are never read or compressed), reserves the host's estimate (the replaced entry's compressed size + 25%) and calls the host in archive order. |
| C1b-8 | `build_engine` loads every sheet (the TS loads all at open) and sets merges (`a1ToArea`) and filtered rows (hidden rows inside an active filter, or every hidden row of a `filterMode` sheet) in the store, which the reader does not fill. |

### Parity (`cargo test -p uo-xlsx --features save,facet --test save_parity`; compares every part decompressed — name, length, SHA-256 prefix, archive order — and the set of parts that differ from the input)

| Run | Result |
|---|---|
| `sheet-x1`, save after the open (open-recalc applied, its sheets dirty: the TS `touched` hook), 30 workbooks | **27 equal, 0 mismatches**; 3 deferred to SH1 (`gen/x1/0002`, `0007`, `0008`: a `<c r>` on another `<row r>`; the TS engine places the cell by its ref, the store by its element, so the open-recalc itself differs — the store after the open (`pv<i>`) differs in exactly those 4 sheets, 61 of 65 equal; the SH1 decision of `uo-calc.md` T16) |
| `sheet-samples`, 7 workbooks including `giant-1m` and `giant-200k` (`UO_XLSX_BIG=1`, release) | **7 equal, 0 mismatches** (5 in CI) |
| store after the open (`pv<i>`), through the real reader + `build_engine` + `open_recalc` | 65 + 21 sheets compared, 4 differ (SH1, above); the plan facet equal on all 35 |
| edit steps (`e<k>.save`, `e<k>.chg`): the steps this crate replays with the APIs that exist (`font`, `fill`, `numfmt` over a range through the XfPool + undo/redo that did nothing; the XfPool patch of `styles.xml` is verified here) | **5 compared, 5 equal, 0 mismatches** |
| edit steps deferred to C5 | **211** of the 240 (27 workbooks that match × 8 steps = 216, 5 replayed): from the first step that needs the session — merge, fill down/right, paste, find/replace, clear, text and formula entry (grid entry + engine write + dirty tracking), borders, width, hide, freeze, autosum, undo/redo of a real edit, recalculate — on, because the scripts are cumulative. Not captured by the committed scripts at all: merge/pane XML (`frame.rs` `<mergeCells>`/`<sheetView>`, covered by unit tests of shapes only) and defined names (`update_defined_names`); both run the TS regular expressions, see C1b-2 |

### Hostile and memory (`tests/save_hostile.rs`, `tests/save_memory.rs`)

Odd frames (comment holding `<sheetData>`, self-closing `<cols/>`, `<sheetView/>`, panes with `1e400`/negative splits, `s="4294967295"`/`-1`/`1e999`,
`cm` past 2^64, `t` with a quote, a cell at XFD1048576 with control characters and `_x0001_`), the prefixed element (error, untouched copy), the TS rules on a
hand-made sheet (order kept, `spans`, `s` + `customFormat`, `ht`, a customHeight-only row dropped, `xml:space`, 365 error as `#VALUE!`), the XfPool on an
xf that does not exist with a styles part lacking fonts/fills/borders (created in schema order, escaped `formatCode`, the result opens), 300 seeded
mutations of `<sheetData>` (save, reopen, save: the same part, the same cell count), and the corpus (30 workbooks saved with every sheet edited reads back
with the same cell count; a worksheet part damaged at 3 random bytes, 4 times each: no panic, a typed error or a file that opens). **M1**: open, build the
engine, edit, save twice, drop: live bytes back to zero for the 30 workbooks and the samples after one warm-up run of each (the lazily built static tables
of the catalogue/languages show once: 4-8 KB on the first run of a workbook), and the failing save leaves nothing. **Peak** (release, `UO_XLSX_BIG=1`):
giant-200k output 760,847 B, peak above the start 1.31 MiB; giant-1m output 2,867,274 B, peak 3.78 MiB (output + 1.05 MiB; budget output + 16 MiB).
The fuzz target `xlsx_roundtrip` (`core/fuzz/src/xlsx_roundtrip.rs`, same inputs and seeds as `xlsx_read`; the crate did not build here because `uo-pptx`
was mid-edit by another agent, so the target itself was not run; the corpus + mutation tests above do what it does) is registered.

### Performance (native, i5-14600KF, release; `examples/save_bench.rs`, TS `tools/bench/speed/sheet.mjs --only save`, Node 24, same machine and day; medians of 5)

| Benchmark (T13 row) | Budget | TS | Rust | Verdict |
|---|---|---|---|---|
| giant-1m (1M cells): one edit, then the surgical save of the edited sheet | ≤ 400 ms native; peak ≤ output + 16 MiB | **1,081 ms** (968 min; the 2026-11-24 baseline: 997) | **163.7 ms** (158.9-170.0); peak 3.78 MiB above the start | within; 0.15x of the TS (6.6x faster); wasm not measured |
| giant-200k | — | **216 ms** (213 min) | **43.5 ms** (42.4-44.3); peak 1.31 MiB | 0.20x of the TS |

What the time is: the sheet part is 31.7 MB of XML written and deflated (about 95% of it); untouched parts are copied raw (verified by a CRC pass). The
engine build (26 ms on giant-1m) and the edit are outside the timing, as in the TS scenario. First measured numbers (the 10% rule starts here).

### Lint and files

`clippy -D warnings` clean native (`--all-targets --features save,facet`) and wasm32 (`--lib --features save`); with `--all-features` it stops in
`src/view/` (frontier C4, mid-edit). `grep-bans.sh` flags only `src/view/chrome.rs:372` (C4); `float-casts.sh` clean; `rustfmt` on the new files only.
The default build and the C1a/C2/C3a-c suites are unchanged and green (`cargo test -p uo-xlsx --features save,grid,facet --lib --tests`).
Changes outside `src/save/`: `uo-opc/src/package.rs` (`save_with`, `HostEntries`), `uo-xlsx/Cargo.toml` (feature `save` = `read` + `uo-text` + `regress`, the
`save_bench` example), `uo-xlsx/src/lib.rs` (the one `cfg`ed module line), `uo-xlsx/src/read/stream.rs` (spans through the loader's pool; `<col style>` into
the store), `core/fuzz` (`xlsx_roundtrip` target: `Cargo.toml`, `src/lib.rs`, `src/bin/seed.rs`, `src/xlsx_roundtrip.rs`, `fuzz_targets/xlsx_roundtrip.rs`;
`src/xlsx_read.rs` made `package`/`limits` crate-visible), `Cargo.lock`.

### Open items

1. SH1 (T16): the engine places `<c r>` by ref in the TS and by element here; until decided, the three x1 workbooks above cannot match after the open.
2. The edit steps listed as deferred to C5, and a replay of the merge/pane/defined-name XML against the TS (no committed capture reaches them): C5a should add the
   scripts, since every step after the first unsupported one is lost to the parity today.
3. The engine is built lazily by the TS (displays, formula bar) and only by the open-recalc here (`build_engine`): the save writes the same bytes with or
   without it until a sheet is dirty; C5 builds it at the first edit.
4. `FileOrder` sheets edited in the session (C5 must update `FileRow::meta`; duplicates and out-of-order rows are otherwise preserved).
5. `xlsx_roundtrip` not run under libFuzzer/the fallback driver (the fuzz crate did not build: `uo-pptx` mid-edit).
6. wasm timing of the save not measured (T13: 800 ms budget).

**Process metrics.** C1b (Sonnet 5.5, closed scope): about 70 minutes of wall time (11:49 to about 12:57, with a pause for a disk-full stop and a reboot),
about 150 tool calls, no round with a verifier. Rework inside the session: (1) the `FileOrder` replay needed the `createCell` placement of cells created after
the open (3 of 30 workbooks differed until then); (2) the first design of the frame surgery was hand-written scanners, replaced by the TS regular expressions on
`regress` (C1b-2) because the committed captures never reach the merge/pane paths; (3) the leak found by M1 was the lazily built static tables (warm-up per
file); (4) the python edits turned LF into CRLF twice and a cp1252 write corrupted one file (fixed; keep edits to the file tools or `newline=''`/`utf-8`).
Environment: the C: volume was full once (the tool temp directory), a machine reboot interrupted the run, and `uo-pptx`/`src/view` were mid-edit by other agents.

## C3a — as built (2026-11-24, Sonnet 5.5)

Frontier C3a of T9: the grid model (feature `grid`, `src/grid/`): the Excel metric tables, the displayed text of a cell for
a width, typed text → value/format/formula, geometry, resolved styles, the row auto-fit at open. The default build is the
writer as before (the module is behind the feature; `Cargo.toml` gains the optional `uo-text` and an example with
`required-features`). Verified against the frozen TS (`ts-ref-092` at `b92b60fa`) through the captures below.

### Code

| Module | Where | TS source |
|---|---|---|
| Tables measured in Excel (generated from the TS by `js/gen-grid-data.mjs`; `--check` compares) | `data.rs` (74 KB, `&'static`, sorted, binary search) | `excel-metrics-data.ts`, `glyph-widths-data.ts`, `row-descent-data.ts` |
| MDW, row px, column px, default widths, GDI glyph fixes, descents | `metrics.rs` | `grid-metrics.ts`, `glyph-widths.ts`, `rowDescentPx` of `text-metrics.ts` |
| Displayed text for a width, `####`, General fit, `*x`/`_x`, wrapped lines, "number as text", ISO dates, errors | `text.rs` | `cell-text.ts`, `wrapLines` of `cell-paint.ts` |
| A store cell as the layout reads it (the FILE model's type + `<v>`, shared strings, a shadowed duplicate) | `cellval.rs` | `cellTextInput`, `displayText`, `formatCell` of `universal-sheet.ts` |
| xf → font → indent → layout of a cell | `layout.rs` | `cellLayout`, `scriptFont`, `indentUnitPx` |
| Entry, entry formats, edit text, formula locale | `entry.rs` | `entry.ts`, `entry-format.ts`, `edit-text.ts`, `formula-locale.ts` |
| Toolbar number formats, ± decimals | `numbar.rs` | `number-formats.ts` |
| Cell fonts, the two-phase measurement table | `measure.rs` | `CellFonts` of `text-metrics.ts` |
| One resolved xf per `cellXfs` index + the row facts of its font | `style.rs` | `resolveStyle`, the font part of `fitRowHeights` |
| Geometry (checkpointed edges, runs of sizes, merge index, sheet extents/defaults) | `geometry.rs` | `recalcGrid`, `colWidth`/`rowHeight`, `mergeAt`, the `SheetView` fields built at load, `bisectLE` |
| Row auto-fit at open | `fit.rs` | `fitRowHeights` |

### API for C3b, C3c, C4, C5

- **Styles**: `StyleTable::build(&book.styles)` (`sync` when the XfPool adds xfs; `xf(index)`, `fit_of(index)`, `code_of`,
  `mdw`, `normal_row_px`, `set_canvas_mdw`). `ResolvedXf { font/fill/border (indices), num_fmt_id, code/code16, align, valign,
  wrap, indent, rotation, fit }`. C3b/C3c layer CF/table styles by replacing the xf/format BEFORE `layout_cell`.
- **Sheet**: `SheetGrid::open(&book, sheet, &styles)` (extents, widths, hidden, frozen panes, merges, defaults) then
  `fit_row_heights(&mut book, sheet, &styles, &grid, &mut FitEnv{ fonts, env, ctx })` → `RowFit` → `grid.set_row_fit(fit)`;
  `grid.col_geometry(zoom)` / `row_geometry(zoom)` → `Geometry { edge(i), edge0, fill_edges(from, &mut [f64]), size_of,
  bisect_le(x), total, len }` (rebuilt per zoom change, O(n) additions, active sheet only). `grid.merges.merge_at(col,row)`,
  `grid.col_width(col)`/`row_height(row)` (zoom 1, hidden = 0), `grid.row_desc(row, &styles)`.
- **Text**: `layout_cell(&CellView{store, fonts, styles, ctx}, &mut table, env, sheet, row, col, raw, col_px, zoom, &mut
  scratch, &mut CellText)` → `Laid::{Empty, NeedSst(i), Done(font)}`; `CellText { text, color, pieces (start,len,x into buf),
  width, hashes, filled, overflows }` is reused, `piece_text(i)`. Lower level: `text_input`/`file_text_input`,
  `layout_cell_text`, `display_text`, `wrap_lines` (ranges into the text), `cell_margin_px`, `autofit_wrap_width_px`.
- **Measurements** (two phases): `MeasureTable::{font(req, env), metrics(id), begin_frame, has_needs, take_needs → NeedMeasures,
  provide_advance/provide_text/provide_box, satisfy(&dyn Measurer)}`. The frame loop is: lay out; `has_needs()` → ask the
  shell → `provide_*` → lay out again. `begin_frame` clears the table past its caps. `CellFont::xl_ascent_descent()`,
  `hg_box()` for the painter.
- **Entry**: `parse_entry(&[u16], &EntryContext{locale, current, row, col, date1904, year, names, in_table}, &dyn FormulaSyntax)`
  → `Entry::{Clear, Value{value, fmt, quote_prefix}, Formula{text, ast, root}, Invalid{message}, Unsupported}`;
  `format_category`, `should_apply_format`, `infer_formula_format`, `currency_hint`, `edit_text`, `number_edit_text`,
  `auto_close`, `FormulaLocale::{pt_br, en_us, for_tag}`, `number_format_for`, `adjust_decimals`.

### Decisions (declared; the observable output equals the TS on every capture)

| Id | Decision |
|---|---|
| C3a-1 | **Tables are data, not code**: the three TS data files are generated into `&'static` slices of strings (one base-62 character per ppem, as in the TS); decoding walks the string, nothing is cached or allocated. A family name that is a property of a JS object (`constructor`, `__proto__`) is not a family here (the TS reads garbage from the prototype); the name is `normalize('NFKC').trim().toLowerCase()` through `uo-text`. |
| C3a-2 | **Text is UTF-16** (`&[u16]`, code points via `uo_text::utf16`), as `FmtPart::Text` of `uo-numfmt`: a lone surrogate keeps its width. `CellText` is a reusable buffer (text + piece arena): a warm frame allocates nothing for text and General integers; a formatted/fractional number costs the parts vector of `uo_numfmt::format_value_parts` (not this frontier). |
| C3a-3 | **General integers skip the format engine**: with no code (or `General`) and a whole number of at most 11 digits that fits, the text is its digits (what `formatGeneral` gives; verified over the layout capture). Everything else goes through `uo-numfmt` as the TS. |
| C3a-4 | **Measurement is a table + a protocol** (`measure.rs`): fonts keyed by (name, alt name, size, bold, italic, zoom), widths = the GDI table first, then the shell's rounded canvas advance, whole strings for complex scripts (`is_complex`); a missing width answers 0 and is recorded; caps 64 fonts × 4,096 non-ASCII characters × 256 strings (T12 M5), cleared at `begin_frame` when passed, hard limit 4,096 fonts (the extra font measures as font 0). The `'Hg'` box is asked only when the GDI descent is unknown (as the TS uses it only then). The canvas MDW (a Normal font outside the Excel table) is `StyleTable::mdw_from_canvas` + `set_canvas_mdw`; the host asks the shell (`NeedMeasures::canvas_mdw` is the slot for it). |
| C3a-5 | **Resolved styles per xf index**, not per cell: an index outside `cellXfs` is the empty style for painting and `fonts[0]` for the fit (the TS does both). The row facts of a font (`pxOf`, `descOf`, `row0Desc` with the super/subscript extra) are computed once per xf. |
| C3a-6 | **Geometry** = checkpoint every 256 entries + the same sequential `acc += size × zoom` inside a block: `edge(i)` bit-equal to the TS `Float64Array` at every index and zoom (5 zooms captured). Sizes are runs (hidden = 0; only the sizes that differ from the default); `<col min max>` is clamped to 16,384 (the TS loops to `max`), `maxRow`/`maxCol` to Excel's limits (a hostile anchor must not ask for billions of edges). `bisect_le` equals `bisectLE` for non-decreasing edges; a NaN size makes the TS array unordered and the result unspecified in both. Per-axis memory: 8/256 B per row (320 B for 10,000 rows). |
| C3a-7 | **Row fit = a column-wise fold** over the store's lanes into the accumulators of a 16,384-row block (one constant per uniform-style chunk); only cells with rich text or wrapped text are read one by one. Results are stored as `Runs` (equal adjacent rows merge). Shared strings the sheet references are decoded first (rich runs and wrapped text need them: open item 4). |
| C3a-8 | **Formulas in the entry go through `FormulaSyntax`** (the TS `fromLocal`, `functionKind`, `acceptsArgCount` of `calc/src/i18n`, which belong to the calc's i18n frontier A5). `CanonicalSyntax` covers `.` decimal / `,` list (en-US): function names upper-cased and given their `_xlfn.` prefix, booleans upper-cased; arity from the registered functions' min/max. Not covered: pt-BR names, LET/LAMBDA parameters (`_xlpm.`), structured references, `_xleta.`, R1C1, the catalogue's step counts and small-count lists. A pt-BR formula (and any formula `CanonicalSyntax` declines) answers `Entry::Unsupported`; it is never silently text. |
| C3a-9 | **A sheet whose `<sheetData>` is out of order or duplicated** (a `FileOrder`) is fitted element by element in file order, as the TS walks its model: shadowed duplicate cells count (`file_text_input` reads them as the model holds them) and a later `<row>` of the same number replaces the height of the earlier one. Displays use the last duplicate, as the component's index. |
| C3a-10 | **The capture opens the TS without the open-recalc** (`shouldRecalcOnOpen = () => false`, the host hook): the model then holds the file's values — what the Rust store holds — and the grid is the one of the open, not of a recalculation that writes spill/array cells and grows `maxRow`. The ordinary open-recalc facets stay in `sheet-x1` (C1b/A5). |
| C3a-11 | The typed-entry year is a context field (`EntryContext::year`, the host clock: `new Date().getFullYear()`); the capture freezes it at 2025. The language of booleans/errors in cells is `CellCtx::lang`. |

### Parity (`cargo test -p uo-xlsx --features grid,facet --test grid_parity`; capture `tests/captures/sheet-grid.{capture,side.gz}`, 28 KiB + 299 KiB, made by `js/capture-grid.mjs`)

The capture runs the real element headless (the harness of T10a, fake canvas: 6 px per UTF-16 unit, `'Hg'` box 9/3) and its
pure modules, deterministically (seeded, `Date` frozen).

| Facet | Compared | Equal |
|---|---|---|
| `metrics` — `excelRowPx`/`excelMdw`/`rowDescentPx`/`measuredFamily` over 73 names (aliases, NFKC, unknown, empty) × 67 sizes × bold; `gdiGlyphFixes` of every family/bold/ppem 6–34 (digest + size per line); `colWidthToPx`/`defaultColPx`/`rowPtToPx` | 13,657 lines | 13,657 |
| `layout` — `layoutCellText`: 4,200 numbers (34 values × 35 codes incl. `*x`, `_x`, conditions, dates, fractions, indent, edge, `showZeros`, 1904) + 1,600 texts (20 texts incl. RTL, Thai, CJK, emoji, lone surrogate × 15 codes) + 240 booleans/errors, 15 fonts (incl. zoom 1.25/0.75, an unmeasured family, bold/italic), 14 widths; 900 `wrapLines`; 15 font facts (ppem, ascent, descent, xlAscent, xlDescent); 139 helpers (`cellMarginPx`, `autofitWrapWidthPx`, `looksLikeNumberPtBR`, `isoToSerial`, `localizeError`) | 7,094 lines | 7,094 |
| `entry` — `parseEntry` pt-BR 2,068 + en-US 1,984 (hand-picked: numbers, %, currency, groups, exponents, fractions, dates/times, booleans, errors, apostrophe, formulas incl. invalid and auto-close; × 6 current formats; + 700 seeded decorated strings per language); `editText` 2,396; `formatCategory`/`shouldApplyFormat`/`currencyHint`/`numberFormatFor`/`adjustDecimals`/`inferFormulaFormat` 603; `autoClose` 19 | 7,070 lines | 7,070, of which **232 inputs are not compared** (226 pt-BR + 6 en-US formulas answering `Unsupported`, C3a-8; their line is taken from the TS); 6,838 truly compared |
| Workbooks, CI: the 30 `x1` + 5 non-giant samples: 84 sheets × (`g` grid summary: mdw, defaults, extents, frozen panes, widths, row heights after the fit, descents, hidden, merges; edges `xs`+`ys` at zoom 1, 0.4, 1.1, 1.75, 3) | 504 facets | 504 |
| displays of every cell (`debugCellDisplays`) | 4,612 cells | 4,612 (91 cells whose position is named by another `ref` are skipped; 22 sheets with conditional formats are skipped: C3b) |
| `UO_XLSX_BIG=1` (release): + giant-1m, giant-200k (the first 200 rows × 40 columns of the displays) | 516 facets, 20,612 cells | all |
| local `UO_GRID_BIG=1` (`x1-big`: 400 generated workbooks, 1,204 sheets; commands in the header of `capture-grid.mjs`; 20,843 rows with a height off the default) | 7,224 facets, 341,690 cells | all (8,354 cells at a position named by another ref and 555 sheets with CF skipped) |

Mismatches: **0** in every run. Round history: the first complete run had 4 classes (an unsorted alias table, `Infinity`
text cells, the pt-BR error/complex-script single-character measure request, the wrapped rows of sheets with duplicate
cells) — all fixed in the code, none in the capture. Not compared and why: pt-BR formula entry (needs the calc's i18n,
C3a-8); cells under conditional formats/table styles (C3b/C3c); glyph advances of the browser canvas (the fake measures 6 px;
Chromium comes with the C4 harness); the `NeedMeasures` round trip with a real shell; zoom changes of the live element.

### Memory (`tests/grid_memory.rs`, `uo-alloc`)

| Gate | Result |
|---|---|
| M1 | styles + grid + fit + geometry at 1.1 + a layout of every present cell of every sheet, then drop and clear the thread caches of `uo-numfmt`/`uo-text`: live bytes back to exact zero for the 30 x1 workbooks and the 5 samples (the process statics — the pt-BR locale, the empty style — are built once before the gate) |
| M2 | 600 scrolling frames (gather + `layout_cell` per cell, `begin_frame`) are flat at tolerance 0 B after warm-up |
| M5 caps | more than 64 fonts → cleared at `begin_frame`; the non-ASCII advances stop growing at 4,096 |
| M3 (giant-1m, release) | grid model of the active sheet: allocated 1,211 B, kept 863 B (style table 473 B, grid 36 B, geometry rows 320 B = 0.03 B/row = the 8/256 budget, columns 8 B); the fit's transient accumulators are 5 × 16,384 × 4 B ≈ 330 KB |

### Security (`tests/grid_hostile.rs`)

40,000 random typed strings (signs, separators, currency, exponents, dates, formulas, lone surrogates, NUL, U+2028; three
locales; seeded) through `parse_entry`/`edit_text`/`auto_close`; layout over NaN/±∞/negative/1e300 widths, indents and values
(a `####` column cannot allocate more than 65,536 characters); geometry with NaN/∞ sizes and absurd runs; 120 bit-flipped
workbooks through open → styles → grid → fit → geometry. No panics, no unbounded work.

### Performance (native, i5-14600KF, release, medians; `examples/grid_bench.rs`; TS: `js/bench-grid.mjs`, Node 24, same machine and day)

| Benchmark (T13 row) | Budget | TS | Rust | Verdict |
|---|---|---|---|---|
| giant-1m: open → active-sheet grid ready (reader + styles + grid + fit + geometry; the first viewport gather is 5 µs) | ≤ 250 ms | `loadXlsx` 1,495 ms | **167–202 ms** (reader 165–192 ms, noisy: other agents share the CPU; this frontier's part **1.7 ms**) | within |
| giant-1m `fitRowHeights` | — | 138.1 ms | **1.7 ms** (81×) | within |
| giant-1m `recalcGrid` (both axes, zoom 1) | — | 0.16 ms | **0.01 ms** | within |
| giant-1m 1080p viewport (1,537 cells: gather + resolve + text input + layout) | well under 1 ms | 4.39 ms | **0.094 ms** (61 ns/cell, 0 B allocated warm) | within (47×) |
| giant-1m layout of 100,000 cells | ≤ 0.5× TS | 345.6 ms | **7.1 ms** (49×) | within |
| giant-200k: grid ready | ≤ 60 ms | `loadXlsx` 371 ms | **49 ms** (fit 0.6 ms; TS 31.5 ms) | within |
| giant-200k viewport / 100k cells | — | 4.08 ms / 446 ms | **0.168 ms** (21 B/cell: fractional numbers in `uo-numfmt`) / **12.6 ms** | within |

History (the 10% rule starts here): the first fit took 9.4 ms on giant-1m (every lane went through the per-cell path because
the cells have no `s`); a chunk of uniform style with no row styles is now one constant: 1.7 ms. The first viewport frame
took 1.35 ms (879 ns/cell, 144 B/cell: the format engine's parts vector per number); General integers now skip it (C3a-3):
0.094 ms. wasm is not measured here (the build is clean for `wasm32-unknown-unknown`).

### Lint and files

`clippy -D warnings` clean native and `wasm32-unknown-unknown` on `uo-xlsx` (`--lib --tests --examples` with `grid,facet`; the
all-features run fails only inside `src/xls/` of frontier C2, mid-edit); `tests/grep-bans.sh` clean, `float-casts.sh` clean for
`uo-xlsx`; `rustfmt` on the new files (`data.rs` is `#[rustfmt::skip]`, generated). Files: `src/grid/*.rs` (11 + `data.rs`),
`tests/grid_parity.rs`, `grid_memory.rs`, `grid_hostile.rs`, `examples/grid_bench.rs`, `js/capture-grid.mjs`,
`js/gen-grid-data.mjs`, `js/bench-grid.mjs`, the capture pair. Outside the module: `Cargo.toml` (optional `uo-text` with its
`collation` feature, the `grid` feature, the `grid_bench` example) and `Cargo.lock` (one line); nothing in `uo-calc`, `src/xls*`,
`js/capture-sheet.mjs` or `js/harness`.

### Open items

1. **pt-BR formula entry** (and the rest of C3a-8) wait for the calc's i18n (A5): `from_local`, `functionKind` (reserved),
   the catalogue's `argStep`/`smallCounts`, `scopeParams` (`_xlpm.`/`_xlop.`), structured references, R1C1. Implement
   `FormulaSyntax` over it; the 232 skipped capture lines then compare.
2. (**closed by C3b and C3c**) **Conditional formats, table/pivot styles, borders, fills** (C3b/C3c) layer over `layout_cell`: the displays of 22 of 84 CI
   sheets and 555 of 1,204 `x1-big` sheets are not compared until the CF number-format override and the table-layer fonts
   exist.
3. **Allocations of formatted numbers**: `uo_numfmt::format_value_parts` returns a vector of parts per call (and `format_general`
   strings): ~20 B per fractional/formatted cell in a warm frame. A `format_value_parts_into(&mut Vec<FmtPart>)` or a
   specialised General path in `uo-numfmt` would remove it.
4. **The fit decodes every shared string the sheet references** (the TS decodes all): needed for rich runs and wrapped text.
   A raw `<si>` scan (`<r>` present?) would avoid the decode for sheets of plain strings; only wrapped cells need the text.
5. **The canvas MDW** (a Normal font outside the Excel tables) and the `NeedMeasures` round trip are tested with a native
   `Measurer`, not with a browser; `NeedMeasures::canvas_mdw` is not yet raised by any code (the host reads
   `StyleTable::mdw_from_canvas`).
6. `Geometry` is rebuilt per zoom (O(n) additions) and per sheet; an edit that changes one size should patch the runs and the
   checkpoints after it (C5).
7. wasm timings (the budgets of T13 are native-only here).

**Process metrics.** C3a (Sonnet 5.5, closed scope): one session, about 3.5 hours of wall time (an estimate: no timer was
kept), about 230 tool calls, no round with a verifier. Rework inside the session: (1) the capture first compared against the
`sheet-x1` displays, which hold the values AFTER the open-recalc (spill/array cells, grown `maxRow`): the new capture opens the
TS without the recalc (C3a-10) and carries its own displays; (2) the 400-workbook run found the shadowed-duplicate rows (C3a-9:
51 sheets, first excluded, then implemented), `Infinity` text cells and a complex-script single-character measure that never
settled; (3) performance, 9.4 → 1.7 ms (fit) and 1.35 → 0.094 ms (frame). Environment notes: the first `cargo` run used a relative
`CARGO_TARGET_DIR`, which created a stray `core/core/target` (build output only; left because a recursive delete was refused);
the C2 agent's `xls_dump` example has no `required-features`, so `cargo test -p uo-xlsx` needs `--lib --tests` until it does.


---

## C3b — as built (2026-11-24, Opus 5.5)

Frontier C3b of T9: the conditional formats (feature `grid`, `src/grid/cf/`): `cond-format.ts` (the engine),
`cf-formula.ts` (its OWN formula evaluator, ported 1:1 — merging it into the calc is evolution after M5) and `cf-icons.ts`.
Declared with one line in `src/grid/mod.rs`; the default build is the writer as before. Verified against the frozen TS
(`ts-ref-092` at `b92b60fa`) through the captures below.

### Code

| Module | Where | TS source |
|---|---|---|
| Values, `toNum`/`toStr`/`toBool`, `compare`, V8 `Date` (`serialOf`/`dateOf`, `Date.UTC` with 0–99 years, `TimeClip`), the wildcard `RegExp`s (`is` flags, `~*`/`~?`, leftmost match) | `cf/value.rs` | `cf-formula.ts:12-15, 175-220, 284-319, 392` |
| Tokenizer (`REF_RE` as a hand matcher with the regex's backtracking outcomes), `parseRef`, recursive-descent parser into an arena, evaluation (59 functions, `COUNTIF` criteria, implicit intersection, wrap-around of shifted references) | `cf/formula.rs` | `cf-formula.ts` |
| `parseSqref`, `cellCfValue` over the lanes, rules by priority (stable), `compute`, `mergeDxf`, `test` (every rule type), `inPeriod`, statistics, `cfvoValue`/`cfvoFormula`, `scaleColor`/`lerpTrunc`, `bar`, `icon`, the memo | `cf/engine.rs` | `cond-format.ts` |
| The text layout of a cell under its layer (`applyCfLayer` + `cellLayout`: the dxf number format for NUMBERS, bold/italic of the measured font, `showValue="0"`) | `cf/layer.rs` | `universal-sheet.ts:4403-4425, 4364-4376` |
| The canvas operations of an icon (`CanvasOp`, every set) and the rectangles of a data bar (`BarRect`) | `cf/icons.rs` | `cf-icons.ts`, `paintDataBar` (`cond-format.ts:489-522`) |

### API for C4 and C5

- **Build**: `CfEngine::for_book(&book, sheet, CfClock, CfLimits) -> Option<CfEngine>` (`None` without CF), or
  `CfEngine::new(&[CondFormat], &CfContext{sheet, views: &[(name, SheetIx)], styles, date1904, clock, limits})`. Then
  `prepare_book(&mut book, &eng)`: loads the sheets the engine may read and decodes the shared strings their cells use
  (an undecoded string reads `""` and is counted in `missing_strings()`, asserted 0 by the tests).
- **Per cell**: `eng.at(&store, row, col) -> Option<CfCell>` (0-based; `None` = the TS `undefined`, `Some(default)` = its
  `{}`). `CfCell` is 48 bytes, `Copy`: `fill: CfFill::{None, Dxf(i), Stop{rule, stop}, Rgb([u16;3])}`, `font_color()`,
  `bold()`/`italic()`/`underline()`/`strike()`, `border() -> [Option<dxf>; 4]` (left, right, top, bottom), `num_fmt()`,
  `hide_value()`, `bar_rule()`, `icon() -> Option<CfIcon{rule, custom, index}>`. Dxf indices are into `eng.dxf(i) ->
  CfDxf{dxf, code}` (`code` = the UTF-16 number format the layer applies).
- **Paint (C4)**: `eng.fill_color(cell.fill)` (the TS `fgColor` string of a colour-scale fill; a `Dxf` fill is the dxf's
  `Fill`), `eng.bar_paint(&cell) -> BarPaint{from, to, color, gradient, border, axis, axis_color, legacy, negative}` →
  `bar_rects(&paint, x, y, w, h) -> Vec<BarRect>`, `eng.icon_of(&icon) -> (set, index)` → `icon_ops(set, index, x, y, s)
  -> Vec<CanvasOp>` (the TS draws at `(round(left) + 2·zoom, round(top + h) − 17·zoom)`, side `16·zoom`).
- **Text**: `layout_cell_cf(&CellView, table, env, ix, row, col, cell, col_px, zoom, scratch, out, &eng, cf.as_ref())`
  (`None` = `layout_cell`).
- **Edits (C5)**: `eng.invalidate()` (statistics, preparations, memo); `clear_caches()`; rebuild the engine when
  formats, dxfs, number formats or sheet names change. The TS drops ONLY the edited sheet's engine (`touched(view)`,
  `universal-sheet.ts:449`): a CF formula reading another sheet stays stale there until that sheet is edited — C5 decides
  whether to keep that (parity) or invalidate every engine.

### Decisions (declared; the observable output equals the TS on every capture)

| Id | Decision |
|---|---|
| C3b-1 | **`cf-formula.ts` ported 1:1 as its own evaluator** (not the calc): its coercions (`toNum` turns `,` into `.`), comparison (lower-cased UTF-16 order), `criterion`, date arithmetic through V8's `Date` (no 1900 leap-day correction in `serialOf`), `formatGeneral`/`formatValue` of `uo-numfmt` for `&` and `TEXT`. Strings are UTF-16; `FErr` keeps one bit (`code === '#N/A'`), the only thing any call site reads. Its parse cache (`cf-formula.ts:162`) is replaced by compiling each rule's formulas once at build. |
| C3b-2 | **Depth limits** (`formula::MAX_DEPTH` = 384 for the AST depth and for parenthesis/function/unary nesting; texts over 65,536 units): deeper formulas are "unsupported". The TS parses them until V8's stack overflows (then `null`), and an overflow while evaluating becomes `#VALUE!`; every call site treats unsupported and error alike (`cellIs`, `expression`, text rules, `cfvoFormula`). The limits fit a 1 MiB stack in debug builds (tested on such a thread). |
| C3b-3 | `MAX`/`MIN` over more values than V8 can spread (`Math.max(...v)`, ≈ 10^5) return the extreme here; the TS throws a `RangeError` → `#VALUE!`. Not captured (machine-dependent). |
| C3b-4 | **Statistics once per sheet version**: every per-rule constant the TS recomputes per cell (`cfvoValue`, top-N threshold, average limit, colour-scale points) is computed once (`Prep`) from statistics computed once per DISTINCT range list (`Group`, shared by the rules of formats with the same sqref); the sorted numbers live only while the group's rules are prepared (O(n log n) once; the ascending sort is kept because the mean/standard deviation are summed in that order). Duplicate tables are keyed structurally (number bits with `-0` as `0`, lower-cased text, booleans: `keyOf`'s strings) and built from the lanes without a string per cell. |
| C3b-5 | **The memo** is an open-addressing table (keys and 48-byte cells in separate arrays, ≤ 16 probes, load ≤ ½ while growing) whose home slot keeps a row's cells consecutive (viewport rows are runs of columns); at its largest — the power of two ≤ `CfLimits::memo_cap` (65,536, T12) — an insert with no free slot replaces the cell at its home slot. The TS clears its `Map` past 200,000 entries. |
| C3b-6 | `CfCell` carries the data bar's rule and the cell's number, not the bar: `bar_paint` recomputes the geometry from the rule's preparation (pure arithmetic); after `invalidate` an old `CfCell`'s bar is `None`. |
| C3b-7 | **Hostile limits** (`CfLimits`): 16,384 rules per sheet (the first by priority), 65,536 range rectangles (formats past it are ignored), duplicate tables ≤ 64 MiB at once (the other groups' tables are dropped and rebuilt on demand); the dotted axis of a data bar is bounded to 65,536 px of height. The TS has none of these. |
| C3b-8 | **The clock is a host input** (`CfClock{year, month, day, seconds}`: `todaySerial`'s local date and `NOW`'s whole seconds, decision C1; `CfClock::from_now_serial` from the calc's `Clock`). The captures freeze 2025-01-15 12:00 (TZ = UTC). |
| C3b-9 | Sheet names of references are resolved at build (the first view whose lower-cased name matches, as `names.findIndex`); an unknown name is `#REF!` for a cell and nothing for a range. `each` is the store's row-major iteration: inside a row the TS iterates the row's cells in insertion order when the range is wider than the row (`universal-sheet.ts:4205`) — only a `FileOrder` sheet with cells out of order inside a row could order a multi-column `SUM` or the first `MATCH` hit differently (T11, not reached by any capture). |
| C3b-10 | `layout_cell_cf` repeats `layout_cell`'s composition in `cf/layer.rs` (C3a's `layout.rs` untouched) with the two overrides; a text cell keeps its own format (`numCode` applies to numbers only). |
| C3b-11 | Faithful oddities kept: `lerpTrunc` with a `NaN` factor gives `#NaNNaNNaN`; an empty stop colour is falsy (no fill); `box.roundRect(…) ?? box.rect(…)` adds BOTH paths (`roundRect` returns undefined); a token whose TEXT is `,`/`)`/`(` steers the parser whatever its kind (`IF(",",1)`); `COUNTIF` with more than two arguments behaves as `COUNTIFS`; `ROW(A5)` is `#VALUE!` (a single reference evaluates to its value). |

### Parity (`cargo test -p uo-xlsx --features grid,facet --test cf_parity`; capture `tests/captures/sheet-cf.{capture,side.gz}`, 6.5 KiB + 42 KiB, made by `js/capture-cf.mjs`; 40 generated workbooks `tests/captures/workbooks/cf1/`, 200 KiB, by `js/lib/gen-cf.mjs`)

The capture runs the real element headless (the harness of T10a, `shouldRecalcOnOpen = () => false` as C3a, `Date` frozen,
TZ = UTC) and calls its own `cfEngine().at(col, row)` for every cell of the sheet's CF ranges inside the used area + 2;
per cell with a result, the canonical JSON of the `CfResult` (the dxf `Fill` as the model's, bars and icons with their
colours). The `cf1` group: 16 hand-made workbooks (every `cellIs` operator with blanks/text/errors; 2/3-colour scales with
every cfvo type, a middle point clamped above and below, min = max, text in the range; 2007 and x14 data bars with
negatives, the three axis modes, lengths, gradient/border/negative colours, `showValue`; all 20 icon sets + `NoIcons` +
an unknown set, reverse, `gte="0"`, custom x14 icons; top/bottom N and % with ties, rank 0/huge/NaN; above/below average
with `equalAverage` and standard deviations; duplicates/unique over numbers, case-folded text, booleans, blanks, errors,
overlapping ranges; the four text rules with and without formulas; blanks/errors; the ten time periods around the
month/week boundaries of 2025-01-15 in the 1900 and 1904 systems; priorities, `stopIfTrue`, shared priorities, overlapping
dxfs, multi-range formats; cross-sheet formulas with quoted/accented/missing names; dxf number formats over numbers/text;
99 formulas of the evaluator's functions) + 24 seeded random workbooks (random data kinds incl. non-canonical `<v>`,
inline/shared strings, `t="d"`, formulas; random rules of every family).

| Run | Input | Result |
|---|---|---|
| CI (`cf_ci`) | 30 `x1` + 40 `cf1` + 5 samples: 67 CF sheets, **3,417 cells evaluated, 1,682 results** | 0 mismatches |
| CI (`cf_ci`) | `evalFormula` over a fixed table in 3 contexts (relative shift, A1, XFD1048576 + 1904): 361 hand-picked traps (number printing, `-0`, `1e21`, wrap-around, sheet names, every function's edges, `SEARCH`/`FIND` starts 0/−1/1.5/NaN, `MID`/`LEFT`/`RIGHT` with NaN/∞, wildcards, criteria, dates out of range, surrogates, `ß`/`İ`/Σ case mapping, odd syntax) + 3,000 seeded random formulas (token soup) | **3,361 × 3 equal** |
| CI (`cf_ci`) | `paintCfIcon` (22 sets × 8 indices incl. −1/7/NaN × 2 boxes) and `paintDataBar` (6 bars × 5 geometries) over a recording canvas | 382 equal |
| local (`UO_CF_BIG=1`, release, 0.9 s; capture 5 s by `capture-cf.mjs --big`) | 300 `cf-big` generated workbooks (20–200 rows) + the 400 `x1-big`: 911 CF sheets, **129,276 cells, 75,471 results** | **0 mismatches** |

Every count passed at the first complete run except one: `3TrafficLights2` (the `roundRect ?? rect` fall-through,
C3b-11), fixed in the code. **Displays under CF** (`grid_parity`, the C3a capture's `d<i>` facets, which already hold the
TS `debugCellDisplays` with the CF layer): the 22 CI sheets C3a skipped are compared now through `layout_cell_cf` except
the 3 that also have tables/pivots (C3c): CI 6,404 cells equal, of which **1,792 cells of 19 CF sheets**; local x1-big
589,637 equal, of which **247,947 cells of 471 CF sheets** (84 sheets with CF and tables/pivots left to C3c). 0 mismatches.

### Memory (`tests/cf_memory.rs`, `uo-alloc`)

| Gate | Result |
|---|---|
| M1 | an engine per CF sheet of the 40 `cf1` + 30 `x1` workbooks, every cell of its ranges (≤ 60 × 20 per range), the book dropped and the `uo-numfmt`/`uo-text` caches cleared: live bytes back to exact zero; `invalidate` after a warm pass returns the engine to its built size exactly |
| M2 | 800 scrolling frames (45 × 10 cells, a new window each frame) over 20,000 rows with ten rules, memo cap 4,096: flat at tolerance 0 B after warm-up (full tables replace slots in place) |
| M5 | memo cap 1,000 → 512 slots, `memo_len() ≤ 1000` over 30,000 cells; a 1-byte duplicate-table budget keeps only the table in use; `invalidate` empties both |
| M3 (`cf_bench`, release) | engine after the bench: ten rules 100k rows **280,692 B** (memo 4,096 slots × 56 B = 229 KB + rules + 977 distinct duplicate keys), giant-1m 230,400 B; a warm viewport allocates **0 B** |

### Security (`tests/cf_hostile.rs`, `tests/cf_fuzz.rs`, fuzz target `cf_eval`)

On a 1 MiB stack (debug): 100,000 nested parentheses, 200,000 unary minuses, 200,000-term `+` and `^` chains, 50,000
nested `ABS(`, 100,000 `%`, 70,000 quotes, 30,000 `A1:` → refused in < 2 s each; the deepest accepted shapes (383-term
chain, 120 parentheses, 60 nested functions) parse and evaluate. 20,000 seeded token-soup formulas through parse and two
contexts (incl. a NaN clock). A sqref covering the whole sheet three times with a colour scale, duplicates, a 10^20 rank
and `COUNTIF($A:$XFD,A1)`; 30,000 rules; 100,000 rectangles; absurd attributes (dxf ids out of range, a 10^29 priority,
`99999Arrows`, ±1e999 cfvos, `0/0` formulas, broken colours, `minLength=-5`), NaN/huge clocks, `bar_rects` at 1e12/NaN
sizes: no panic, bounded time. Fuzz target **`cf_eval`** (`core/fuzz/src/cf_eval.rs`, appended; mode 0 = a formula over a
table in three contexts, mode 1 = any `<conditionalFormatting>`/`<extLst>` tail in a minimal package, 30 × 12 cells twice +
after `invalidate`, bars and icons painted; 15 seeds in `seed.rs`): the stable fallback driver could not be relinked
(its `fallback.exe` was held open by another session), so the target ran through `tests/cf_fuzz.rs`, which includes the
same file under a seeded mutation loop (bit flips, inserts, deletes, token splices, crossover): **8,872,235 executions in
170 s, 0 crashes, slowest input 6.1 ms**. No `unsafe`.

### Performance (native, i5-14600KF, release, medians; `examples/cf_bench.rs`; TS: `js/bench-cf.mjs`, Node 24, same machine and day; other agents shared the CPU: the quiet runs)

Viewport = 45 rows × 34 columns (1,530 cells, C3a's 1080p). `cold` = a new engine + statistics + the viewport; `warm` = the
same viewport again; `scroll` = a new viewport per frame with the statistics kept; `stats` = the statistics alone.

| Scenario | Rust | TS | Ratio |
|---|---|---|---|
| ten rules over 100,000 rows × 10 columns (2 colour scales, data bar, icon set, top 10 %, above average, `between`, two expressions, duplicates): cold | **12.5 ms** (12.5–21 under load) | 141–168 ms | 0.09× |
| — warm (the user's budget: well under 1 ms) | **0.0057 ms**, 0 B allocated | 0.0126–0.0138 ms | 0.45× |
| — scroll | **0.066 ms** | 0.32–0.49 ms | 0.2× |
| — statistics | **12.4 ms** | 145–164 ms | 0.09× |
| T13 row: giant-1m (10,000 × 100) with a 3-colour scale + a formula rule over all of it: cold | **8.4 ms** | 78–115 ms | 0.11× |
| — warm | **0.0054–0.0062 ms**, 0 B | 0.0059–0.0096 ms | 0.6–1.0× |
| — scroll | **0.21 ms** | 1.5–2.6 ms | 0.1× |
| — statistics (1,000,000 numbers sorted once) | **8.0–8.4 ms** | 74–96 ms | 0.1× |

History (the 10% rule starts here): the first engine kept a two-generation `HashMap` memo of 120-byte results: warm 9.8 µs
on giant-1m (TS 6.4): slower than the TS. A 48-byte `CfCell` (bars recomputed from the rule, C3b-6) and the open-addressing
memo brought it to 6.4 µs, the row-local home slot (C3b-5) to 5.7 µs. The duplicate table built from the lanes (no string
per cell) took the ten-rule statistics from ≈ 20 ms to 12.4 ms. Warm on giant-1m is at parity with the TS rather than
below it (3.7 ns per `at`, mostly the 48-byte copy and the probe): see open item 1. wasm is not measured here (the build is
clippy-clean for `wasm32-unknown-unknown`).

### Lint and files

`clippy -D warnings` clean native and `wasm32-unknown-unknown` on `uo-xlsx` (`--lib --tests --examples`, `grid,facet`) and
without features; `tests/grep-bans.sh` clean; `float-casts.sh` clean for `uo-xlsx` and no float→int cast in `cf/` with
`grid` either; `rustfmt` on the new files. `check-changed.mjs` not run (it selects the whole workspace; the orchestrator
asked not to, disk). Files: `src/grid/cf/{mod,value,formula,engine,layer,icons}.rs`, `tests/cf_{parity,memory,hostile,fuzz}.rs`,
`tests/support/cf_pkg.rs`, `examples/cf_bench.rs`, `js/capture-cf.mjs`, `js/lib/gen-cf.mjs`, `js/bench-cf.mjs`, the capture
pair and `tests/captures/workbooks/cf1/` (40 files). Outside the module: one line in `src/grid/mod.rs`; `Cargo.toml` (the
`cf_bench` example with `required-features`); `tests/grid_parity.rs` (CF sheets compared through the layer, the totals);
`core/fuzz` (appended: `src/cf_eval.rs`, `fuzz_targets/cf_eval.rs`, its `[[bin]]`, the `lib.rs` registration, the seeds in
`seed.rs`, and `"grid"` added to the `uo-xlsx` features of its dependency line). Nothing in `uo-calc`, `src/xls*`,
`capture-sheet.mjs`, `capture-grid.mjs`.

**Additive helpers wished from uo-calc** (not applied; the module is being restructured by another agent): `dates::{date_utc,
utc fields of new Date(t) incl. getUTCDay}` public (the same V8 port is repeated in `cf/value.rs`); a `Store::each` that
reuses its cursor buffer (one `Vec` per call today; the CF statistics and the formula `each` call it per range).

### Open items

1. **Warm path vs the TS**: at parity on giant-1m (5.7 µs vs 5.9–9.6), 0.45× on the ten-rule sheet. A row-run API for C4
   (`at_run(store, row, c1..=c2, &mut [Option<CfCell>])`, one probe per run) or keeping the previous frame's layer block would
   take it well below; decide with C4's emission loop.
2. Statistics sort each range list (O(n log n)); only the average/standard-deviation sums need the ascending order —
   colour scales, top-N and percentiles could use selection (O(n)).
3. C4: paint the fills/fonts/borders of the dxfs, the bars and icons (`CanvasOp`/`BarRect` → display-list items), the
   hidden values. C5: `invalidate` on edits (the TS drops only the edited sheet's engine), rebuild on format/name changes.
4. (**closed by C3c**) CF sheets that also have tables/pivots: their displays are compared with C3c's table layer.
5. Run the real fallback driver on `cf_eval` when `core/fuzz/target/release/fallback.exe` is free (`cargo run --release
   --no-default-features --features fallback --bin fallback -- --secs 170 cf_eval`).
6. Merging `cf-formula` into the calc (one formula language) is evolution after M5 (as decided).
7. wasm timings.

**Process metrics.** C3b (Opus 5.5): one session, **56 minutes** of wall time measured (11:36:37 → 12:33:10 UTC), about 130
tool calls, no round with a verifier. Rework inside the session: (1) the only parity difference of the first complete run
(the `roundRect ?? rect` fall-through of `3TrafficLights2`), fixed in the code; (2) performance: the warm viewport was slower
than the TS with the first memo (9.8 vs 6.4 µs) — compact `CfCell` + open-addressing memo + row-local slots (5.7 µs); the
duplicate tables built without a string per cell (20 → 12.4 ms); (3) clippy (disallowed `f64::min`/`max` in tests, redundant
guards). Environment: the fuzz crate's `fallback.exe` was locked by another session (C3b-security above); disk D: at
≈ 10 GB free, so everything was built in `core/target/sheet-c1` only.

## C3c — as built (2026-11-24, Sonnet 5.5)

Frontier C3c of T9: table and pivot styles, the cell's paint layers, borders, fills, the spill border and the rest of
`grid-layout.ts` (feature `grid`, `src/grid/`). The default build is the writer as before. Verified against the frozen TS
(`ts-ref-092` at `b92b60fa`) through the captures below. Closes C3a open item 2 (the table layer of the displays) and C3b
open item 4 (CF sheets with tables/pivots).

### Code

| Module | Where | TS source |
|---|---|---|
| Colours (`Color`: packed `#rrggbb`, else a palette text), `Palette` | `paint_rules/mod.rs` | the CSS strings of every `Fill`/`BorderEdge`/`Font`/`Dxf` |
| Border styles and ranks, `winning_edge`, `edge_extent`, corners, the 1-px runs of a segment (`emit_edge`), diagonals | `paint_rules/border.rs` | `borders.ts` |
| Fills as paint data (`FillRef`, `PaintFill`, `fill_paint`, `path_gradient_rgba`), the 17 pattern masks | `paint_rules/fill.rs` | `fills.ts`, `PATTERN_MASKS` |
| Spill rectangle, table-handle rectangles, `frozen_extent`/`px_to_index`/`index_screen_pos`/`boundary_at`/`px_to_col_width_chars`/`px_to_row_height_pt` | `paint_rules/misc.rs` | `spill-border.ts`, `paintTableHandle`, `grid-layout.ts` |
| `layout_rich_lines` | `paint_rules/rich.rs` | `layoutRichLines` |
| Elements, specs (built-in + custom), `apply_element`, `table_cell_format`, `apply_outline`, `parse_range` | `table_style/mod.rs` | `table-styles.ts` |
| The 144 built-in definitions (generated by `js/gen-paint-data.mjs`, `--check`; `&'static`, binary search) | `table_style/data.rs` | `table-styles-data.ts` |
| Pivot regions: the page fields, the main loop, the filter buttons | `table_style/pivot.rs` | `pivot-styles.ts` |
| `PaintBook` (fills/fonts/borders of `styles.*`), `CfPaint`, `SheetPaint`, `PaintFrame` (+ the border pass), `PxEdges`, `layout_cell_layers` | `paint.rs` | `styleAt`, `applyTableLayer`, `applyCfLayer`, `buildTableLayer`, `paintGridBorders` |

`merge index` and `bisectLE` of `grid-layout.ts` were C3a's (`MergeIndex`, `Geometry::bisect_le`): not repeated.

### API for C4

- **Per workbook**: `PaintBook::build(&book.styles)` (`sync` when the XfPool adds fills/fonts/borders); `palette().text(color)`;
  `arena()`; `fill_paint(fill_ref, x, y, w, h, ox, oy) -> FillPaint` (= the canvas calls of `paintCellFill`: `Solid(c)`,
  `Pattern{rows, fg, bg, phase}`, `Linear{p0, p1, stops}`, `Path{inner, stops, w, h}` + `path_gradient_rgba` for the raster,
  `Nothing`); `border(i).diagonal_strokes(x, y, w, h)` (`paintDiagonals`: colour, width, dash, clip, 1–4 lines).
- **Per CF engine**: `pbook.attach_cf(&eng) -> CfPaint` (dxfs and stop colours as paint data; rebuild with the engine).
- **Per sheet**: `SheetPaint::build(&mut pbook, &worksheet, &styles, &cx)`: O(tables) (the regions, their specs resolved once
  per distinct style name; **no per-cell map**). `filter_at(col, row)`, `table_end_at`, `format_at` (1-based; scan everything:
  tests) and `xf_index(store, ix, row0, col0, cell)` (the TS `xfIndexAt`, with the column styles).
- **Per frame**: `PaintFrame::resolve(&PaintInputs{book, sheet, styles, store, ix}, cf: Option<(&mut CfEngine, &CfPaint)>,
  first_col, last_col, first_row, last_row)` (1-based, inclusive; resolves the region + 1 cell around it, the border pass
  needs the neighbours). `frame.cell(col, row) -> &CellPaint { fill: FillRef, filled, font: FontPaint{color, bold, italic,
  underline, strike}, border: [Edge; 4], table_border: [Edge; 4], border_ix, has_cf, … }` (`Copy`, 104 bytes; the final result
  of xf → table/pivot → CF), `frame.filter_at(&inp, col, row)`.
- **Border pass**: `frame.border_segments(&merges, show_grid, first_col, last_col, first_row, last_row, &px, &py, ax, ay,
  &mut |seg: BorderSegment|)` in the TS drawing order (all vertical segments row by row, then all horizontal ones);
  `seg.rects(clip, &mut |r: BorderRect{rect: PaintRect{x, y, w, h, erase}, color}|)` = the `fillRect`s of `paintHEdge`/
  `paintVEdge` (an erasing rectangle is `#ffffff`). `PxEdges::fill(&geometry, first, last, scroll)` / `.at(c)` give `px`/`py`
  (`Math.round(xs[c-1] - scroll)`) from one sequential pass (`Geometry::edge` costs up to 256 additions: do not call it per
  boundary). **Pass `clip = Some((0, viewport_px))`**: the TS has none and a hostile column width of 10^9 px would loop that long.
- **Text under the layers**: `layout_cell_layers(&CellView, table, env, ix, row, col, cell, col_px, zoom, scratch, out,
  tf: Option<&TableCellFormat>, cf: Option<(&CfEngine, &CfCell)>)` (the table layer's bold/italic and the CF's; `showValue="0"`
  hides). `SheetPaint::format_at` gives the `tf` of a cell (a frame-less call; C4 can keep the frame's `font` and ask only for
  the flags).
- Data bars, icons: C3b's `bar_rects`/`icon_ops` (unchanged).

### Decisions (declared; the observable output equals the TS on every capture)

| Id | Decision |
|---|---|
| C3c-1 | **Colours are `Copy`**: `Color(u32)` = `0x00RRGGBB` for the exact `#rrggbb` text (any case: the case is not observable on a canvas and the TS `sameColor` lower-cases), else a slot of the book's `Palette` holding the lower-cased text (`#abc`, `#NaNNaNNaN` of a NaN colour-scale factor, `#red`…). The palette only grows at build (book, dxfs, specs, CF stops); a frame never adds to it. |
| C3c-2 | **JS object-property names are not styles**: a table style named `constructor`/`__proto__`/`toString` makes `parseBuiltin` throw in the TS (the definition is a function), a border style or pattern name of that kind reads `BORDER_RANK`/`PATTERN_MASKS` garbage. Here they are unknown names (no table style; border rank 6 like any unknown style, solid dashes; the pattern paints `bg`). Not captured (the TS throws). |
| C3c-3 | **The dash quirk is kept**: `run()` marks "no dash open" with `start = -1` and tests `start < 0`, so at a NEGATIVE coordinate every lit pixel restarts the dash and none is emitted (a dash crossing 0 begins at 0). Found by the parity run (scrolled sheets: the first column). `emit_edge`'s `clip` only intersects the run with the viewport: with `lo >= 0` the rectangles are the TS's. |
| C3c-4 | **Layers on demand, in the TS order**: a table replaces what an earlier table gave a cell when its own format is non-empty, its `tableBorderDxfId` outline lays over it side by side; then the pivots (page-field cells merge onto what is there; the main loop replaces). Filter buttons: last writer wins (table filters, sheet autofilter, pivot buttons). A table whose `ref` is not a range gives nothing, not even its filter buttons. A frame lists the regions that touch it once (`collect`), so a cell pays only for those. |
| C3c-5 | **Fills**: `FillRef::{None, Solid, Id}`; a fill that paints nothing is `None` (`hasFill` false); a table element keeps `Option<FillRef>` because the TS object can exist and paint nothing (a gradient without stops overrides the earlier element's fill with nothing). A one-stop gradient is its colour; a name without a mask paints `bg`; the path gradient's stops are sorted stably (a NaN position keeps its place: V8's sort of an inconsistent comparator is unspecified); past 4,000,000 pixels the TS paints the first stop (the same here). |
| C3c-6 | **Final font facets** follow `applyTableLayer` then `applyCfLayer`: the table's colour only when the cell's own is missing or equals the Normal font's (`sameColor`, a missing one is `#000000`), `bold`/`italic`/`underline`/`strike` only turn ON; the CF sets them either way. Colours, sizes and names of the font stay in `fonts[xf.font]`. |
| C3c-7 | **Absent cells take the row/column style here, not from the store**: the reader (C1a) never calls `Store::set_col_styles` and `Store::default_style` reads a style as `u32` (NaN/negative → 0); `SheetPaint::xf_index` keeps its own `colXf` (`defaultXfMaps`: later `<col>` wins, style 0 skipped, columns up to 16,384, built newest-first with a next-free pointer: O(columns + definitions)) and answers the TS double (a NaN or out-of-range index is the empty style). **Open item for C1a**: load `<col style>` into the store and make `default_style` return the double. |
| C3c-8 | **Hostile caps**: at most `MAX_REGIONS` = 16,384 tables, pivots and filters per sheet (the first ones); pages of a pivot at most 10^6. The TS has none (a table over `A1:XFD1048576` builds 17 billion map entries and never returns; here it is a rectangle). |
| C3c-9 | **Frame cap**: past `MAX_FRAME_CELLS` = 262,144 cells (a 1080p window holds ≈ 1,500) the frame holds nothing and the pass draws no border; the TS stops at 4,000,000 (400 MB of cell data here). |
| C3c-10 | The edge style `none` and `""` are never edges (the reader drops them); `BStyle::Other` (an unknown non-empty text) ranks 6 like `thin`, has no extent and paints solid dashes (`dashOn` default). |
| C3c-11 | Pixel positions are `i32` (wrapping arithmetic, `Math.round` through `uo_jsnum`); region coordinates of tables and pivots are `f64` like the TS (`parseInt` of the reference digits: any double), so stripes of a weird size (`size="2.5"`) and hostile references behave as the TS. |
| C3c-12 | **The TS `debugCellDisplays` applies the table layer only when the sheet has conditional formats** (it reads `styleAt` then, `resolveStyle` otherwise), but the painter always uses `styleAt`. `grid_parity` compares the displays accordingly (CF + tables through `layout_cell_layers`, tables alone without the layer); the paint capture's own display facet always goes through `styleAt` (what is painted). |
| C3c-13 | `CellPaint` carries no `CfCell`: C4 asks the engine again for bars/icons (a memo hit) when `has_cf`. A pivot's `bbox` includes its page-field rows and filter buttons above the range. |

### Parity (`cargo test -p uo-xlsx --features grid,facet --test paint_parity`; capture `tests/captures/sheet-paint.{capture,side.gz}`, 35 KiB + 188 KiB, made by `js/capture-paint.mjs`; 14 generated workbooks `tests/captures/workbooks/paint1/`, 218 KiB, by `js/lib/gen-paint.mjs`; total committed 441 KiB)

The capture runs the real element headless (the harness of T10a, `shouldRecalcOnOpen = () => false`, `Date` frozen, fake DOM with
recording canvases for the pattern tiles and the path-gradient raster) over, per workbook and sheet, the region A1 to (used area
+ 2, at most 60 x 120): the table layer of every cell (`tableFmt`), the filter buttons and handles, the resolved spec of each
style name, the final `styleAt` (fill as paint, font facets, own and table borders, diagonal, layer flags), the canvas calls of
`paintCellFill` (zoom 1.1, scroll 29.25 x 13.5) and `paintDiagonals`, the `fillRect`s of the REAL `paintGridBorders` at two
zoom/scroll settings (digests; `--lines` keeps the lines), and the displayed text through `styleAt`. `misc/*`: the pattern
masks, `winningEdge`/`edgeExtent` over every style pair, `paintHEdge`/`paintVEdge` over 14 styles x 4 corner pairs x 4 anchors x
erase x direction, the 144 built-in styles under 3 themes (+ unknown and JS-property names), the grid-layout helpers over
zoomed edges with frozen panes, spill/handle rectangles, `layoutRichLines`. The `paint1` group: every built-in family (21
light, 28 medium, 11 dark), 84 pivot styles over compact/tabular layouts with subtotals, sub-headings, blank rows and page
fields, the 64 banding/header/total/first/last combinations, custom styles (stripe sizes 1.5/2/3, dxf borders and outlines,
gradient and pattern dxf fills, unknown elements and dxf ids, a style with no elements), overlapping tables, direct formatting
and conditional formats over tables and pivots, filters (hidden/active buttons, header-less tables, the sheet autofilter),
every border-style pair (13 x 13) and dense random borders with merges, hidden columns, row/column default styles, all 17
patterns and 14 gradients, diagonals, and hostile references. Pivots are minimal definitions (no cache: the TS reader reads
`location`, the items and the flags only).

| Run | Input | Result |
|---|---|---|
| CI (`ci_paint`) | 14 `paint1` + 30 `x1` + 5 samples: 101 sheets (46 with a table/pivot/CF layer), 68,047 cells resolved, 9,346 displays | **151,600 lines + 832 facets equal, 0 mismatches**; + 20,123 lines of 27 sheets with a cell at a position named by another `r` (accepted if different; all equal) |
| CI (`grid_parity`) | the displays of the 3 sheets with CF AND tables (skipped in C3b) through `layout_cell_layers` | 6,723 cells equal in total (+319 cells), 0 mismatches |
| local (`UO_PAINT_BIG=1`, release, 4 s; capture 15 s by `capture-paint.mjs --big`) | 160 `paint-big` (random mixes) + 400 `x1-big`: 1,525 sheets (967 with a layer), 2,587,909 cells resolved, 93,789 displays | **2,659,579 strict lines + 12,519 facets equal, 0 mismatches**; 580 odd-ref sheets: 1,876,328 of 1,893,349 lines equal, the rest are 9 facets of 2 workbooks (`x1-big/0007`, `/0257`: cells whose `r` names another position, C3a-9), accepted |
| local (`UO_GRID_BIG=1`) | `grid_parity` over the 400 `x1-big` | 627,929 display cells equal, of which 286,239 under CF and **84 CF+table sheets (38,292 cells now compared)**, 0 mismatches |

Round history: the first complete run found two classes — the dash quirk of negative coordinates (C3c-3) and the column default
styles the reader does not load (C3c-7) — then NaN geometry beyond `maxCol`/`maxRow` in my own capture lines (the TS reads
`undefined` past a `Float64Array`: the test now reads `edge()` as `NaN` there) and the 9 odd-ref facets. Not compared and why:
Chromium pixels (C4's harness); the pixels of the pattern tiles (compared as mask, colours and phase) and gradients (as
endpoints/stops; the path gradient raster by SHA-256 of its RGBA); the `save`/`clip`/`restore` calls of `paintDiagonals`
(strokes are compared); displays of x1/sample sheets with tables and no CF (the TS helper skips the layer, C3c-12: the `paint1`
displays cover it); a table style named like a JS property (the TS throws, C3c-2); real Excel pivot tables (none in the
corpus: the generated ones carry the same attributes the reader reads).

### Memory (`tests/paint_memory.rs`, `uo-alloc`)

| Gate | Result |
|---|---|
| M1 | the whole pipeline (paint book, sheet layer, CF engine, a frame 30 x 60, the border pass) over the 14 `paint1` + 40 `cf1` + 30 `x1` workbooks, dropped and the `uo-numfmt`/`uo-text` caches cleared: live bytes back to exact zero |
| M2 | 800 scrolling frames (resolve 45 x 34 + the border pass + every rectangle) over a 20,000-row sheet with two tables: flat at tolerance 0 B after warm-up; **a warm frame allocates 0 B** |
| M3 | table A1:H20000 → the sheet layer is **544 B** (its definition, not its cells); 600 tables 131,104 B (218 B per table); the paint book 548–1,076 B; a frame 172,741 B for 46 x 36 cells (104 B per cell, reused) |
| M5 | past `MAX_FRAME_CELLS` the frame is empty and the pass emits nothing; at the cap it resolves; 16,484 tables keep `MAX_REGIONS` |

### Security (`tests/paint_hostile.rs`)

Absurd style indices (cells `s=9999`/`4294967295`/`-5`, rows and columns with NaN/negative/1e30 styles, xfs with `fillId`/`borderId`
9999/-1/NaN/4294967295, custom elements with dxf ids -1/99999999999 and sizes 0/NaN); 12 tables that overlap, cover
`A1:XFD1048576`, have impossible references (`A0`, `:`, `nope`, a third part, `$B$2`, rows past the sheet) + a pivot with
page counts of 2,000,000 and negative header offsets, frames at the far corners of the sheet (rows 1,048,570, `u32::MAX - 3`);
2,000 tables over one viewport (< 30 s in debug; 1.7 s in total here); 400 seeded attribute mutations of a workbook with
tables, a pivot, custom styles, dxfs with gradients and borders, a merge and filters (tokens such as `NaN`, `1e308`,
`4294967295`, `A1:ZZZ9`, `""`: > 150 of them open and run end to end); `emit_edge` at the edges of `i32` with and without a clip,
fills over NaN/∞/negative geometry and a path gradient past the pixel cap, `px_to_index`/`boundary_at` with NaN/∞,
`layout_rich_lines` over a 2,000-character word, lone surrogates and NaN widths. No panics, bounded time. No `unsafe`. No fuzz
target was added (open item 4).

### Performance (native, i5-14600KF, release, medians; `examples/paint_bench.rs`; TS: `js/bench-paint.mjs`, Node 24, same machine and day; other agents shared the CPU)

The work of one 1080p frame (45 rows x 34 columns + 1 around): resolve every cell (xf, table/pivot layer, fill, font facets,
borders), the border pass with every rectangle, the fill paint data. TS = `styleAt` per cell + the real `paintGridBorders` +
`paintCellFill` on counting canvases. `layer build` = the sheet layer (TS: `buildTableLayer`, which maps EVERY cell of every table).

| Scenario | Rust warm | TS warm | Ratio | Rust scroll | TS scroll | layer build (Rust / TS) | cold frame (Rust / TS) |
|---|---|---|---|---|---|---|---|
| `table`: 50,000 x 12, one table A1:H50000 + fills/borders | **0.086 ms**, 0 B | 0.721 ms | 0.12× | 0.087 ms | 0.861 ms | **0.003 ms** / 189 ms | 0.20 / 3.8 ms |
| `many`: 600 tables stacked | **0.075 ms**, 0 B | 0.529 ms | 0.14× | 0.078 ms | 0.545 ms | **0.079 ms** / 7.6 ms | 0.12 / 1.5 ms |
| `borders`: 300 x 40, 52 xfs (every style x 4 fills), 19,917 rectangles | **0.370 ms**, 0 B | 2.161 ms | 0.17× | 0.364 ms | 2.203 ms | 0.001 ms / — | 0.61 / 6.7 ms |
| `giant`: giant-1m.xlsx, no layers | **0.030 ms**, 0 B | 0.439 ms | 0.07× | 0.030 ms | 0.263 ms | — | 0.06 / 0.37 ms |

History (the 10% rule starts here): no earlier record. `PxEdges` exists because `Geometry::edge` replays up to 256 additions per
call and the pass asks for every boundary of every row. The first allocation measure of a warm frame found 640 B: the test's own
per-frame `col_geometry`/`row_geometry` (geometry is cached per zoom, as C3a says). wasm is not measured here (the build is
clippy-clean for `wasm32-unknown-unknown`).

### Lint and files

`clippy -D warnings` clean native (`--all-targets --all-features --no-deps`) and `wasm32-unknown-unknown` (`--lib --features
grid,facet`) on `uo-xlsx` (the dependency `uo-calc` is mid-edit by another agent and does not pass clippy now); `tests/grep-bans.sh`
clean; `float-casts.sh` clean for `uo-xlsx` and no float→int cast in `grid/` with `grid` either; `rustfmt` on the new files
only (`table_style/data.rs` is generated and `#[rustfmt::skip]`; the pre-existing drift of other files is left). `cargo test -p
uo-xlsx --features grid,facet --lib --tests` green. Files: `src/grid/{paint.rs, paint_rules/{mod,border,fill,misc,rich}.rs,
table_style/{mod,pivot,data}.rs}`, `tests/paint_{parity,memory,hostile}.rs`, `tests/support/paint_{pkg,pkg_dense,run}.rs`,
`examples/paint_bench.rs`, `js/{capture-paint,gen-paint-data,bench-paint}.mjs`, `js/lib/{gen-paint,masks}.mjs`, the capture pair
and `tests/captures/workbooks/paint1/` (14 files). **Outside the module (additive)**: `src/grid/mod.rs` (the three module lines,
the doc rows, the re-exports), `src/grid/cf/engine.rs` (two methods: `CfEngine::stop_colors(rule)` — the stop colours without the
allocation `fill_color` makes — and `rule_count()`), `tests/grid_parity.rs` (the CF + table sheets compared through
`layout_cell_layers`, totals text), `Cargo.toml` (the `paint_bench` example). Nothing in `uo-calc`, `src/xls*`,
`capture-grid.mjs`/`capture-cf.mjs`/`capture-sheet.mjs`, `js/harness`.

### Open items

1. **C1a**: load `<col style>` into the store (`Store::set_col_styles`) and make `Store::default_style` return the double (NaN/
   negative = the empty style); then `SheetPaint::xf_index` shrinks to a store call (C3c-7).
2. C4: the emission order of the three passes (fills with the grid lines, text, borders, then markers) and the clip of every run
   to the canvas; `Pattern` tiles and `Path` rasters become display-list resources (`fill_paint` already says which); the
   overlay of the active selection and the spill border (`spill_border_rect`) are C4's.
3. `layout_cell_layers` repeats `layout_cell_cf`'s composition (C3a/C3b files untouched): once C4 settles the flow, fold the CF
   and table layers into one function and drop the other.
4. No fuzz target (the seeded mutation test of `paint_hostile.rs` stands in); add `paint_layers` to `core/fuzz` with the stable
   fallback driver when the fuzz crate is free.
5. `PaintFrame::resolve` asks the store per cell (`gather` + `row_meta` per absent cell); a run API over the lanes would halve the
   warm frame on sheets that are mostly absent cells (0.03 ms on giant-1m: not needed now).
6. Real Excel pivot workbooks (none in the corpus): the generated ones exercise the reader's attributes, not Excel's layouts.
7. wasm timings.

**Process metrics.** C3c (Sonnet 5.5, closed scope): one session, **53 minutes** of wall time measured (09:38:40 →
10:32:08 local, `date`), about 170 tool calls, no round with a verifier. Rework inside the session: (1) the parity run's first
real mismatches: the dash quirk of negative coordinates (C3c-3) and the column styles absent from the store (C3c-7); (2) my own
capture lines past `maxCol` (NaN edges) and the odd-ref sheets (accepted, as C3a-9); (3) the capture's tile cache is module-global
in the TS, so the pattern-tile stub had to be live for the whole element, not only around the recorded calls; (4) `uo-calc` did
not compile for ~4 minutes while another agent restructured it (waited); (5) clippy (`then`/`map_or`, an 8-argument test helper).
Environment: everything built in `core/target/sheet-c1` only; disk D: at ≈ 11 GB free.


---

## C5a — as built (2026-11-24, Sonnet 5.5)

Scope: JS and docs only. The edit operations of the frozen TS `<universal-sheet>` (`ts-ref-092`, `sheet/packages/sheet/src/universal-sheet.ts`, 4,593 lines;
line numbers below are that file unless a file is named) were inventoried and captured by `js/capture-edit.mjs`, so C5b can port the editor session against
them. No Rust, no TS source touched.

### Code

| File | Role |
|---|---|
| `crates/uo-xlsx/js/capture-edit.mjs` | the capture script (same harness, host injection and `CaptureWriter` as `capture-sheet.mjs`; imports its helpers, does not edit them) |
| `crates/uo-xlsx/tests/captures/sheet-edit.capture` (415 KiB) + `sheet-edit.side.gz` (363 KiB) | 80 scripts, 800 steps, 778 KiB in total, byte-identical across runs |

Run: `node core/crates/uo-xlsx/js/capture-edit.mjs` (3.2 s; `--only <substring of the script key>`, `--out`, `--no-xml`, `--ts-root`).

### What the element does NOT have (so C5b does not port it)

The TS element has no: insert/delete rows, columns or cells; sort; filter UI (the autofilter/table layer is read and painted only); series fill and the fill handle
(Fill Down/Right only); delete, rename, move or copy of a sheet (only `addSheet` at the end); defined-name editing (`updateDefinedNames` is a `sheet-core` library
function the host calls, `save-calc.ts:190`; the element never does); conditional-format, table, validation, comment, hyperlink or chart editing; page setup; outline
groups; paste-special other than values. They are absent from the inventory and from the capture; a backlog item, not part of the port.

### Operation inventory (47 operation ids; the id is the tag in the capture)

Common machinery (every row uses it): `record(label, cells, op, struct)` 2051-2062 snapshots `CellSnap`s before/after (`snapCells` 1953: `{col,row,edit,style,exists}`,
`edit` = the bar text), plus a `StructSnap` (`structSnap` 1968) when `struct`; it pushes an `UndoEntry {label, before[], after[], sel}` (`pushUndo` 1912; `UNDO_LIMIT`
100, 4431; clears redo) unless the call is non-struct and every cell's text and style are unchanged. `afterCells` 2198 re-derives the row/col xf maps, drops the CF
engine, regrids, repaints and fires `changed` (2078). `calc.edit` (`sheet-calc.ts:144`) marks `dirtySheets` through `host.touched` (`universal-sheet.ts:448`) for every
view whose values moved. XfPool = `sheet-calc.ts:556-853` (`apply` 600, `patch` 689 at save). "Dirty" columns: **D** = `dirtySheets`, **M** = `mergeDirty`, **V** = `viewDirty`
(the three sets `saveXlsx`, 909-930, reads: D picks the sheets whose `<sheetData>`/`<cols>` are regenerated, M adds `<mergeCells>`, V adds `<sheetView>`).
Row/col style, width, hidden and height changes are in the sheet part only (`<cols>`, `<row>` attributes); styles.xml grows only through the XfPool (`patchStyles` at save).

| Op id | Public API / UI command (`chrome-commands.ts` ids) | Private methods (lines) | Mutates | Undo record | Dirty | Proposed `Session` method |
|---|---|---|---|---|---|---|
| `entry.text` / `entry.clear` / `entry.formula` / `entry.refused` | `setActiveCellText` 3145; formula bar, `commitEdit` 3082 | `setCellValue` 3123 → `SheetCalc.edit` (`sheet-calc.ts:144-195`: `parseEntry`, `XfPool.derive`, `Engine.setCell`, `calculate`, `inferFormulaFormat`, `writeBack`); `recordCells` 2064 | cells (value/formula/type), cell xf (format hint, quote prefix), dependents on all sheets, maxCol/maxRow, spill ranges | label `typing`, 1 cell, edit text + style; not pushed when text and style are unchanged; refused entry (`ok:false`, localised message) pushes nothing | D (all touched views) | `enter(text) -> EditResult` on the active cell; core `edit(sheet, col, row, text)` |
| `entry.set` | `setCellValue` 3123 (host API, also used by every writer below) | as above | as above | none by itself | D | `set_cell_text(sheet, col, row, text) -> EditResult` (no undo) |
| `entry.editor` | `beginEdit` 2987, `commitEdit`, `cancelEdit` 3104, keys F2 / Enter / printable char (`onKeyDown` 1588-1700); `insertFunction` 2154 | editor `<input>` overlay (DOM), `barText` 1056 | nothing until commit; commit = `entry.text` (`typing`) | as `entry.text` | D | shell owns the input; Session offers `edit_text(col, row) -> String` (the `barText().edit`) and `enter` |
| `key.delete` | Delete/Backspace (`onKeyDown` 1633) | single cell: `recordCells('clearContents')` + `setCellValue('')`; range: `clear('contents')` | cell content | `clearContents` | D | `clear(Contents)` / `enter("")` |
| `fmt.font` / `fmt.fill` / `fmt.border` / `fmt.numfmt` / `fmt.decimals` / `fmt.align` / `fmt.format` | `setFont` 2454, `setFormat` 2414, `setBorders` 2484, `setNumberFormat` 2460, `adjustDecimals` 2465, `setAlignment` 2457; `bold`..`strike`, `fontColor`, `fillColor`, `grow`/`shrink`, `align:*`, `indent`/`outdent`, `wrap`, `borders:*`, `numfmt:*`, `decimal:inc/dec`, `dlg:formatCells` | `applyStyle` 2509 → `SheetCalc.styleFor` → `XfPool.apply`; `resolveNumFmt` 2425; `fitRowHeights` 747 when size/name/wrap/bold changes | cell xf of every cell in the range (empty cells are created, up to 100,000 cells, `MAX_FORMAT_CELLS`; beyond that only existing ones); cellXfs/fonts/fills/borders/numFmts growth; row heights (auto-fit, view only) | label `format` or `borders`, one `CellSnap` per cell of the range (style) | D | `format(range, StyleChange)`, `set_borders(range, kind, color)`, `set_number_format(range, NumFmtSpec)`, `adjust_decimals(range, i8)` |
| `fmt.lines` | same calls with a whole-column or whole-row or select-all range | `applyStyle` with `whole` → `setLineStyles` 2544 | `<col style>` runs (split/merged), `<row s customFormat>`, plus the xf of existing cells | one `StructSnap` + the cells that exist | D | same `format`/`set_borders` (the range type decides) |
| `merge` / `unmerge` | `merge` 2626 (`center` / `across` / `cells`), `unmerge` 2658, `mergeLosesData` 2616; `merge:*`, `merge:off` | `setMerges` 2602 (`ws.merges`, `mergedAreas`, view index), clears the covered cells (`setCellValue('')`), centre style on the origin | merge list (overlaps replaced), covered cells, origin xf | label `merge`/`unmerge`, cells of the range + `StructSnap` | D, **M** | `merge(range, MergeKind)`, `unmerge(range)`, `merge_loses_data(range) -> bool` (the confirm dialog is the shell's) |
| `fill.down` / `fill.right` | `fill` 2239; `fill:down` Ctrl+D, `fill:right` Ctrl+R | `writeCopy` 2264 (`SheetCalc.copyText` shifts relative references; xf copied) | target cells | `fillDown`/`fillRight`, the target cells | D | `fill(range, Dir)` |
| `clip.copy` / `clip.cut` | `copy` 2284, `cut` 2312; the DOM `copy`/`cut` events 2379; `copy`, `cut` | builds `InternalClip {sheet, rect, cells, text, cut}` + the TSV text (formatted values) | nothing (the clip) | none | none | `copy(range, cut) -> ClipText` (Session keeps the internal clip, `has_clipboard`) |
| `clip.pasteValues` / `clip.pasteText` / paste of the internal clip | `paste` 2314 (internal clip when text is absent or equals the last copy; else TSV text; `valuesOnly`); `paste`, `paste:values` | `copyText` (shifted unless cut), `valueText`; cut clears the origin; `writeCopy`; selection moves to the pasted block | destination cells and xf; origin cells on cut | label `paste` / `cutPaste`, destination + (same-sheet cut) origin cells; foreign TSV: one entry | D | `paste(at, Option<&str>, PasteMode)` |
| `clear.contents` / `clear.formats` / `clear.all` | `clear` 2211; `clear:*`, Delete | `setCellValue('')`, `delete cell.style` + `dropIfEmpty`; whole row/col: `setLineStyles(() => 0)` | content, xf, empty cells dropped, line styles when whole | `clearContents`/`clearFormats`/`clearAll`; `StructSnap` too when formats are cleared on whole lines | D | `clear(range, ClearKind)` |
| `find` | `find` 2877 (`allSheets`, `backwards`, `lookIn`, `wholeCell`, `matchCase`); `find`, `goto` | `matcher`, `searchText` 2857 | selection (and the active sheet) only | none | none | `find(opts) -> Option<(sheet, ref)>` (pure query; Session owns the selection) |
| `replace` / `replaceAll` | `replace` 2942, `replaceAll` 2954; `replace` | `replaceIn` 2923 → `setCellValue` per cell (edit text, case-insensitive RegExp-escaped) | cells (formula text too) | `replace` (1 cell); `replaceAll`: ONE entry with the before/after of every hit on every touched sheet | D (each sheet) | `replace(opts, text)`, `replace_all(opts, text) -> usize` |
| `view.freeze` | `freezePanes` 2683; `freeze:panes` / `topRow` / `firstCol` / `off` | `ws.pane = {xSplit, ySplit, topLeftCell, state:'frozen'}` or deleted; `v.frozenCols/Rows` | pane, scroll reset | `freeze`, `StructSnap`; no entry when unchanged | D, **V** | `freeze_panes(sheet, cols, rows)` |
| `view.gridlines` | setter `showGridlines` 2698; `view:gridlines` | `ws.showGridLines` | grid flag | `gridlines`, `StructSnap` | D, **V** | `set_gridlines(sheet, bool)` |
| `size.colwidth` / `size.rowheight` | `setColumnWidth` 2754, `setRowHeight` 2767; `col:width`, `row:height` | `applyResizePreview` 1255 + `commitResize` 1273 (the `<col>` run split, `customWidth`; `<row ht customHeight>`; 0 = hidden) | `ws.cols`, `Row.height`, view metrics | `columnWidth`/`rowHeight`, `StructSnap` (always pushed) | D | `set_col_width(cols, chars)`, `set_row_height(rows, pt)` |
| `size.drag` | mouse drag on a header edge (`onMouseDown` 1440-1470 → `onResizeDragMove/End` 1334-1358) | `resizeBefore = structSnap()`, preview, `commitResize`, `pushUndo` | same | `columnWidth`/`rowHeight`, `StructSnap` | D | `resize_commit(kind, index, px)` (the drag preview is shell) |
| `size.autofit` | `autofitColumns` 2779 (`autofitCol` 1308: widest formatted text, 2,000 rows scanned, canvas font measure), `autofitRows` 2786 (the row returns to automatic height); `col:autofit`, `row:autofit` | as above, `fitRowHeights` | widths / heights | `columnWidth`/`rowHeight`, `StructSnap` (pushed even when nothing changes) | D | `autofit_cols(cols)` (needs the `NeedMeasures` round trip, T7), `autofit_rows(rows)` |
| `size.hide` / `size.unhide` | `setHidden` 2804; `col:hide` Ctrl+0, `row:hide` Ctrl+9, unhide (also the hidden neighbours of the selection) | `ws.cols` run split with `hidden`, `<row hidden>`, view sets | hidden flags | `hide`/`unhide`, `StructSnap`; unhide with nothing hidden returns early | D | `set_hidden(kind, lines, bool)` |
| `fn.autosum` / `fn.insert` | `autoSum` 2164, `insertFunction` 2154, `getFunctionNames` 2149; `autosum:*` Alt+=, `function` | range: `record('autoSum')` + `setCellValue` per column below the range; single cell: only `beginEdit` with the guessed formula (nothing written until commit) | cells below the range | `autoSum`, one cell per column; single cell: the later `typing` | D | `auto_sum(range, Fn) -> AutoSum::{Written, EditText(String)}`, `function_names(lang)` |
| `sheet.add` | `addSheet` 2119; `sheet:insert` Shift+F11 | `saveXlsx` → `XlsxPackage.load` → `addSheetPart` → `loadXlsx` (the whole workbook is rebuilt; `dirty` carried; zoom kept; new sheet selected) | a blank worksheet part, workbook.xml, workbook rels, content types | the undo and redo stacks are EMPTY afterwards (observed) | all dirty sets cleared, the sheet is in the package | `add_sheet(name) -> String` (re-save + reopen, T8) |
| `sheet.select` | `selectSheet` 1002; `sheet:prev/next` Ctrl+PgUp/PgDn | selection reset to A1, scroll reset | selection only | none | none | `select_sheet(i)` |
| `book.new` | `newWorkbook` 2093; `new` | `loadXlsx(blankXlsx)` | everything | stacks reset | cleared | `Session::new_blank(lang)` |
| `undo` / `redo` | `undo` 1919, `redo` 1931 (Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z in `onKeyDown`); an open editor: Ctrl+Z cancels it and returns true | `restoreSnaps` 1985: struct via `restoreStruct` 2014, then per cell `calc.edit(edit text)` only when the bar text differs, style restored, `dropIfEmpty` when the cell did not exist; rows/cols xf maps rebuilt; `fitRowHeights`; selection restored (`restoreSelection` 1942) | the same facets as the op | moves the entry between stacks | D, plus **M** and **V** for every struct restore | `undo() -> bool`, `redo() -> bool` |
| `calc.recalc` | `recalculate` 2140; `calc:now` F9 | `SheetCalc.recalcAll` (`engine.recalc`, `writeBack`) | calculated values (volatile functions) | none | D only for sheets whose values moved (`writeBack` → `touched`); the capture's `entry-formula` scripts show no save change when nothing volatile exists | `recalculate()` |
| `host.defineName` (not an element op) | host calls `updateDefinedNames(pkg, upserts, removals)` (`sheet-core/src/save-calc.ts:190`) before `saveXlsx` | `<definedNames>` block rewritten in workbook.xml, unknown attributes kept | workbook.xml only | none | none (the host's package edit) | not a Session method: `save_calc::update_defined_names` of C1b (already ported) |

Also element state the shell keeps (no capture): `setZoom` 1017 (zoom is not saved), `goToRef`, `selectRange` 1791, `selectAll`, selection by mouse/keys, `ensureVisible`,
scroll, `getSelection`, `selectionStats` 1879, `isDirty`/`setDirty` 902-2090 (`changed` sets `dirty`; the host clears it after saving).

### Proposed `Session` API (C5b)

```
Session::open(bytes, SessionOpts { lang }) / Session::new_blank(lang)
// selection travels with undo (UndoEntry.sel): Session keeps { sheet, active, range } as data; the shell paints it
select(sheet, range) ; select_sheet(i) ; selection() -> Selection
// pure store/engine operations (everything below pushes one undo entry unless said otherwise)
enter(text) -> EditResult            set_cell_text(sheet, col, row, text) -> EditResult (no undo)
format(range, StyleChange) ; set_borders(range, kind, color) ; set_number_format(range, spec) ; adjust_decimals(range, delta)
merge(range, kind) ; unmerge(range) ; merge_loses_data(range) -> bool
fill(range, dir) ; clear(range, kind) ; copy(range, cut) -> ClipText ; paste(at, Option<&str>, mode) ; has_clipboard()
find(opts) -> Option<(sheet, ref)> (no undo) ; replace(opts, text) ; replace_all(opts, text) -> usize
freeze_panes(sheet, cols, rows) ; set_gridlines(sheet, bool)
set_col_width(cols, chars) ; set_row_height(rows, pt) ; autofit_cols(cols, &Measures) ; autofit_rows(rows) ; set_hidden(kind, lines, bool) ; resize_commit(kind, index, px)
auto_sum(range, fn) -> AutoSum ; function_names() -> Vec<String> ; edit_text(col, row) -> String (the bar text, pt-BR/en-US via the Translator)
add_sheet(name) -> String ; recalculate() ; undo() -> bool ; redo() -> bool ; undo_label()/redo_label() ; can_undo()/can_redo()
save() -> Vec<u8> (C1b `save::save` with dirty_sheets / merge_dirty / view_dirty accumulated by the ops above) ; is_dirty() ; set_dirty(bool)
```

Pure store/engine: everything above. Needs the shell (listed, not captured): the cell editor `<input>` and its keys (Enter moves down, Tab right, Escape cancels, blur commits;
autocomplete of function names), OS clipboard events (`onCopyEvent`/`onPasteEvent` 2379; the Session takes and returns text), every dialog and prompt (`chrome-dialogs.ts`:
column width / row height / go to / find / custom number format / format cells / merge data-loss confirm / sheet name for Insert Sheet), entry-error display (`us-entry-error`, the
`us-edit` and `us-change` events), mouse selection and the header-edge drag preview, zoom/scroll/`ensureVisible`, repaint, hyperlink following and comment tips, font loading
(`ensureSheetFonts`), the canvas measurement of autofit (a `NeedMeasures` round trip) and `recalcGrid` metrics.

### Capture format (`sheet-edit.capture` + `.side.gz`; the header comment of the script has the full list)

A record per script, key `edit/<scenario>/<workbook>/<lang>` (workbook = `x1/<NNNN>` from `tests/captures/workbooks/x1` or `samples/<file>`; lang = `formula-locale` pt-BR or
en-US, which also picks the entry texts and the localised refusal messages). A script opens the workbook FRESH in a new element, then runs 5-12 steps (one operation each; every
scenario ends with undo/redo pairs); scripts do not depend on each other. Literal arguments, no random choice; the coverage is the scenario table in the script (16 scenarios, each on
4 x1 workbooks chosen by feature hint (merge, frozen, hidden, names) with alternating languages, plus 1 sample). After the open (`o.*`) and after every step (`e<k>.*`):

| Facet | Content |
|---|---|
| `e<k>` / `e<k>.r` | `<op id> \| <description>`; the call's result (JSON; `EditResult`, find hit, widths, `throw:` text) |
| `.sel` `.sheets` `.dirty` `.xf` `.st<i>` `.ws<i>` `.rw<i>` `.d<i>` | carried forward: written only when the text differs from the previous step's (`o` always). Selection (sheet, active, range); sheet names + zoom + panes + gridlines + clipboard + canUndo/canRedo; `dirty` and the three private sets; cellXfs:numFmts counts; **store facet** per sheet (`count:digest` of the canonical cell lines of `sheet-x1`); worksheet model without rows (`cols`, `merges`, `pane`, `showGridLines`…); row metadata; display text of the ACTIVE sheet |
| `.undo` `.redo` `.utop` | stack shape `n:label[sheet:cells:S>sheet:cells:S] …` oldest first (S = has a `StructSnap`); digest of the top entry's before/after (cells `{col,row,edit,style,exists}`, struct canonical JSON) |
| `.save` `.chg` `.chgp` `.xmlfeat` | digest of the package after `saveXlsx` (name/len/sha lines); the parts that differ from the INPUT and from the PREVIOUS step (`-name` = removed); counts of the structural XML in the saved package (merge, pane, `<col>`, hidden cols/rows, row `ht`, `customFormat`, gridlines off, defined names, calcPr, sheets) |
| sidecar | the changed store lines (`+line`, `-r,c`), changed display lines, the model JSON when it changed, the top undo entry (≤ 4,000 chars), and for every part in `.chgp` the XML as a diff against the part's previous version (the input's on first change): `<prefix length>,<suffix length>,<JSON of the middle>` — the full text is `prev[..prefix] + middle + prev[len-suffix..]`. Drawings, charts, media and parts over 60 KB are digests only. The store/ws/rw/display lines of the open (`o`) are not repeated: `sheet-x1` has them |

The sidecar of step `o` holds the part digests of the opened package; the capture metadata has `@ops <op>=<steps>/<workbooks>/<languages>`.

Counts (steps / distinct workbooks; every operation has both languages and at least one sample): `entry.text` 70/13, `entry.formula` 35/11, `entry.editor` 25/5, `entry.clear` 5/5, `entry.refused` 5/5,
`entry.set` 5/5, `key.delete` 10/5, `fmt.font` 20/9, `fmt.fill` 10/5, `fmt.border` 20/5, `fmt.numfmt` 20/5, `fmt.decimals` 10/5, `fmt.align` 15/5, `fmt.format` 5/5, `fmt.lines` 20/5, `merge` 20/5, `unmerge` 10/5,
`fill.down` 10/5, `fill.right` 5/5, `clip.copy` 5/5, `clip.cut` 5/5, `clip.pasteValues` 5/5, `clip.pasteText` 5/5, `clear.contents` 10/5, `clear.formats` 10/10, `clear.all` 10/5, `find` 15/5, `replace` 5/5,
`replaceAll` 15/5, `view.freeze` 25/5, `view.gridlines` 10/5, `size.colwidth` 10/5, `size.rowheight` 5/5, `size.drag` 20/5, `size.autofit` 10/5, `size.hide` 10/5, `size.unhide` 15/5, `fn.autosum` 20/5,
`fn.insert` 10/5, `sheet.add` 10/5, `sheet.select` 5/5, `book.new` 5/5, `calc.recalc` 10/6, `host.defineName` 15/5, `undo` 145/21, `redo` 70/21. Total 800 steps in 80 scripts.

Reachability of the structural XML (scripts in which an edit CHANGED the saved package's count; the C1b open item "no committed capture reaches merge, pane or defined-name XML"): `<mergeCell>` 6 scripts,
`<pane>` 5, `<col>` definitions 18 (hidden cols 13), row `ht` 12, row `hidden` 13, row `customFormat` 6, `showGridLines="0"` 5, new sheets 5, `<calcPr>` 3, `<definedName>` 7 (the 5 `host.defineName` scripts, plus 2 `sheets` scripts where `newWorkbook` replaced a workbook that had names;
none through an element edit). Zero steps threw; `entry.refused` returns `ok:false` with the localised message (en-US and pt-BR texts differ).

### Observed behaviours C5b must reproduce (all in the capture)

- Undo never snapshots the store: the entry holds edit texts + xf indexes; undo RE-ENTERS the text (`calc.edit`) only when the bar text differs, so undoing an overwrite of a spill anchor restores the spill through the engine, and the xf indexes come back as numbers (the XfPool does not shrink; `.xf` shows it).
- Non-struct ops that change nothing push no entry; struct ops (`size.*`, `merge`, `freeze`, `gridlines`, autofit) push even when nothing visibly changed (the `.undo` shapes show it).
- `addSheet` empties both stacks (the package is reloaded) and keeps `dirty`; `newWorkbook` resets `dirty`.
- `replaceAll` is one entry across all touched sheets (`before`/`after` lists with a sheet index each); `paste` of a same-sheet cut records the origin cells too.
- Redo after a new edit is gone (`pushUndo` clears the redo stack); an undo restores the selection (sheet, active cell, range).
- The save marks: cell ops set D only; `merge`/`unmerge` D+M; `freeze`/`gridlines` D+V; undo of a struct entry sets D+M+V (observed in `.dirty` after `undo`). `host.defineName` is a package edit outside the three sets.

### Not captured, and why

The OS clipboard (the script calls `copy()`/`paste(text)` and the internal clip), dialogs, prompts, focus, the editor overlay beyond its value (the captured paths drive `beginEdit`/`commitEdit`/`cancelEdit` and the key handler with fake events),
mouse selection (the resize drag drives `onResizeDragMove/End` with synthetic events), zoom/scroll/paint (the display text is captured, the pixels are C4's), the font measurement of `autofitColumns` (the fake canvas measures 0, so the widths come
from the GDI tables fallback like the other captures; the result is in `e<k>.r`), the async font loading, and Excel's own opening of the saved files (the sheet-x1 caveat; `word-open`-style COM checks are C5b's gate).

### Lint and files

`node --check` clean; only `js/capture-edit.mjs` and the two capture files are new (`capture-sheet.mjs`, the other scripts, every Rust crate and the Sheet TS were not touched). No proprietary font anywhere (the workbooks are the committed x1 and samples).
Two runs (separate processes) give byte-identical `.capture` and `.side.gz` (checked with `cmp`).

### Open items

1. The capture saves after EVERY step (as `sheet-x1` does); the TS save has side effects on the package (`patchStyles`, `saveCalc` attribute rewrites), so a Rust session that saves only at the end can differ in the intermediate package digests — C5b compares the Session's step-wise save, or the final step only.
2. Defined-name XML is reachable only through the host function; a C5b `Session` need not expose it.
3. `size.autofit` widths depend on the measurement tables (no canvas text here): the Rust autofit uses the same `NeedMeasures` path; parity of the widths is only as good as the measured tables (C3a).
4. Sheets with more than 4 sheets, giant workbooks, tables/pivots edit and CF edit are not covered (the element has no such operations, or the sample is out of the budget); a 400-workbook `x1-big` run is possible by pointing `combos()` at `workbooks/x1-big` if C5b needs more breadth.

**Process metrics.** C5a (Sonnet 5.5, closed scope): one session, about 10 minutes (12:50-13:00) of wall time, about 50 tool calls, no verifier round. Rework: (1) the first run was 1.9 MiB (a sheet part per step plus whole display tables), cut to 778 KiB by carry-forward facets, a prefix/suffix XML diff and 4+1 combos per scenario; (2) a bash heredoc ate the backslashes of the first reachability regexes (they became backspace characters and counted nothing), so the audit patterns use character classes instead of `\b`; (3) the TS element has no insert/delete rows, sort, filter, series or sheet delete/rename/move, which the scope list assumed, so the inventory records the absence instead of capturing them.

## C2 — as built (2026-11-24, Sonnet 5.5)

Frontier C2 of T9: the `.xls` converter (feature `xls` = `read` + `uo-drawingml` with its `legacy` feature). A `.xls` (BIFF8, and BIFF5/7
inside a Compound File) becomes, in memory, **the same OOXML parts, byte for byte, as the frozen TS converter**
(`sheet-core/src/xls/*`, `ts-ref-092` at `b92b60fa`: 17 files, 5,542 lines), which then go through the normal reader (`Book::from_package`).
The default build is unchanged: the new dependency is optional, `lib.rs` gains one `cfg`ed line, `Cargo.toml` the dependency, the feature
edge and two `required-features` examples.

### Entry API (`uo_xlsx::xls`)

| Item | What |
|---|---|
| `convert(&[u8]) -> Result<XlsParts, XlsError>`, `convert_with(bytes, &XlsLimits)` | the package parts, in the order of the TS `Map` (`[Content_Types].xml` and `_rels/.rels` first, then workbook, its rels, each sheet with its comments/drawing/charts/media, styles, shared strings, theme) |
| `XlsParts` (`pub items: Vec<(String, Vec<u8>)>`, `get`, `contains`, `len`) | an insertion-ordered part map (a repeated name keeps its first place and takes the last bytes, as the JS `Map`); feed `uo_opc::Package::from_parts(parts.items, …)` |
| `Book::open_xls(bytes, ReadLimits) -> Result<Book, XlsOpenError>` | convert + `Package::from_parts` + `Book::from_package` (an inherent impl in `xls/mod.rs`; `book.rs` is untouched) |
| `is_xls_bytes(&[u8])` | the Compound File signature (`isXlsBytes`) |
| `XlsError::{Format(String), Encrypted, Limit(&str)}`, `XlsOpenError::{Xls, Read}` | typed errors; the TS throws `XlsFormatError` / `XlsEncryptedError` (Portuguese messages; here English, the shell localizes) |
| `XlsLimits { max_sheet_bytes, max_total_bytes }` | bounds of the output (defaults: the XML part cap and the total cap of `uo_opc::Limits`) |

### Code map (`crates/uo-xlsx/src/xls/`)

| Rust | TS (`sheet-core/src/xls/`) | Notes |
|---|---|---|
| `mod.rs` (`convert`, names, workbook.xml, `<definedNames>`, `<calcPr>`, content types, order) | `index.ts` | |
| `biff.rs` (records with CONTINUE joined, `Rd`, strings) | `biff.ts` | `Records` is an iterator over the stream, zero-copy when a record has no CONTINUE |
| `globals.rs` | `globals.ts` | fonts, formats, XFs, XFEXT, palette, SST, names, THEME, SUPBOOK/EXTERNSHEET |
| `formula.rs` (`decompile_into`, `formula_areas`, `num_text`) | `formula.ts` | single-buffer stack (C2-9); the function table is `data.rs` |
| `sheet.rs` (cells into an arena, rows, columns, merges, window, HLINK, NOTE/OBJ/TXO, CF/DV hooks) | `sheet.ts` | |
| `sheet_xml.rs`, `styles_xml.rs`, `theme_xml.rs`, `cf.rs`, `dv.rs`, `xmlw.rs` | `sheet-xml.ts`, `styles-xml.ts`, `theme-xml.ts`, `cf.ts`, `dv.ts`, `xmlw.ts` | |
| `officeart.rs` (the shape tree, `BlipStore`) | `officeart.ts` | records, FOPT (`Flavor::Sheet`), BStore, BLIP decoding and the msospt preset table are `uo_drawingml::odraw` |
| `geom.rs` (custGeom, arc, WordArt warps) | `odraw-geom.ts` | |
| `shapes.rs` (fills, lines, text bodies, metroBlob, pictures, groups) | `shapes.ts` | |
| `drawing.rs`, `metrics.rs` | `drawing.ts`, `grid-metrics.ts` + `excel-metrics-data.ts` | |
| `chart.rs` | `chart.ts` | C2-4 |
| `data.rs` | `FTAB_SRC` of `formula.ts`, `excel-metrics-data.ts` | generated by `js/gen-xls-data.mjs` (`--check` compares with the frozen TS) |

### Decisions (declared; the observable output equals the TS on every capture)

| Id | Decision |
|---|---|
| C2-1 | **Reuse**: `uo_opc::cfb` (the Compound File reader), `uo_opc::zip`/`flate` (the theme zip, the metroBlob zip, BLIP inflate), `uo_drawingml::odraw` with `Flavor::Sheet` (records, FOPT incl. the Sheet rule of keeping complex properties apart, BStore, BLIP decode, msospt → preset with `PresetVariant::Sheet`; its table already carried the Sheet differences), `uo_jsnum` (every number printed, `Math.*` through fdlibm), `uo_numfmt` (built-in format codes of the chart axes), the C1a `read::theme::INDEXED_COLORS_DEFAULT` and the `uo_opc` DOM (metroBlob XML with the `SHEET_XML` options). Nothing from `uo-pptx`: its `.ppt` OfficeArt reader is tied to `OaShape`. |
| C2-2 | **Strings are Rust `String`s.** Where the TS holds a lone surrogate (a split pair at a CONTINUE boundary, `fromCharCode` of a lone unit) the decoder writes U+FFFD, which is what `xmlText` writes for it, so every output is identical; the one place it can differ is the shared-string de-duplication of the strings added by LABEL/RSTRING (two different lone surrogates merge). UTF-16 slices (rich-text runs, the 31-character sheet name, text-box runs) are cut over UTF-16 units. `TextDecoder('utf-16le')` removes a leading BOM: so does `biff::utf16`. |
| C2-3 | **Cells go into one arena `String`**, one 16-byte entry each (`row u16, col u16, xf u16, off, len`), not a `Vec<String>` per row. The entries keep the file order; the sheet writer orders them (stable, by row and column, the last write of a column wins) only when the file was not in order, and the arena is freed as soon as the sheet XML is written. The `<f>` of the first cell of a shared/array/table formula (it arrives AFTER the cell) is a patch applied at write time (`patchF`: the first patch wins, found through `last_idx` of the cells that wait). |
| C2-4 | **`chart.ts` is ported again, not taken from `uo_charts::biff`.** That module started from the same file for the `.ppt` charts but reads the Office 2007 extension records (`ShapePropsStream`, `TextPropsStream`, `CrtLayout12`), the trendlines/error bars and a chart-only workbook, so it does not give this converter's bytes (Excel 2007+ writes those records into `.xls` charts). A flag that disables the extensions would be the way to unify them (open item). |
| C2-5 | **BIFF5/7 text in a non-Western codepage** decodes as Windows-1252 (437, 850, 367, 32769 and 1252 are what the TS decodes that way too); the TS uses the `TextDecoder` of 1250/1251/1253–1258, 874, 932, 936, 949, 950, … (no encoding tables in the workspace). BIFF8 is UTF-16/Latin-1 and not affected. |
| C2-6 | `metrics.rs` is the converter's own copy of `grid-metrics.ts` + the measured tables (it needs them to turn the cell anchors of shapes into EMU). The grid port (C3a, `grid::metrics`) has another copy: unify when the crates settle. The TS normalizes the font name with NFKC first; here only ASCII and the CJK names of the alias table are exact (full-width/compatibility forms are not normalized). |
| C2-7 | Where the TS **throws** inside a shape (a `DataView` past its data in `custGeomXml`: an `IMsoArray` with 1–3-byte elements) the caller leaves that shape out (`geom::Thrown`), as its `try/catch` does; the id and the media already taken stay taken. The other `try/catch` sites of the TS (formulas, CF, DV, charts) guard exceptions that cannot happen in Rust. |
| C2-8 | **Bounds the TS does not have** (they change the output only on hostile files): `XlsLimits` (a sheet's XML ≤ 256 MiB, all parts ≤ 4 GiB native; wasm32 per `uo_opc::Limits`), a file hyperlink's `../` steps ≤ 1,024 (`cAnti` is a u16: 65,535 × 3 bytes per 30-byte record), chart areas kept per sheet 512 (the TS 4,096, tested against every cell), values read per series ≤ 1,000,000 and per chart ≤ 2,000,000, shapes per drawing ≤ 65,536 (the odraw bound), chart records ≤ 500,000, metroBlob bytes inflated per workbook ≤ 64 MiB (each part ≤ 4 MiB as in the TS). |
| C2-9 | **The decompiler keeps its operands in one buffer** (the stack holds start offsets; operators, commas and parentheses are inserted), where the TS keeps a stack of strings: the benchmark file's 150,000 formulas took 107 ms of the first 219 ms; the whole conversion is 103 ms now, same text. |
| C2-10 | The default sheet name stays `Planilha<n>` and the nested-chart/`Gráfico n`/`Caixa de Texto n` names stay Portuguese: they are part of the bytes the TS writes. |
| C2-11 | A nested chart substream whose EOF never comes keeps its records up to the end of the stream (the TS fills the array as the records arrive): found by the record-level mutants. |
| C2-12 | The `uo-xls.wasm` module (T15) is the bindings' job: the feature compiles for `wasm32-unknown-unknown` (clippy clean) and has no I/O or clock. |

### Parity (`cargo test -p uo-xlsx --features xls,facet --test xls_parity`; capture `tests/captures/xls.capture` + `.side.gz`, 19 KiB + 116 KiB, made by `js/capture-xls.mjs`)

Per file: `read` (ok, or the class of the error), `names` (the part names in order), `parts` and `p<i>` (the digest of EVERY part's bytes), then the
C1a facets of the converted package as sheet-core reads it (`wb`, `styles`, `theme`, `sst`, `ws<i>`, `st<i>`, `fo<i>`, `rw<i>`, `sheets`).
Sources are `.xls` files saved by **Excel COM** `SaveAs` (FileFormat 56; 39 for BIFF5): the generated `xls1` group (the generator group
with the features Excel refuses removed, `js/lib/gen-workbook.mjs`), six feature workbooks built by `tools/xls-features.ps1` (every kind of formula, shared/array/data-table
formulas, names, styles, notes, hyperlinks, validations, every conditional format, autofilters, about 30 chart types and two chart sheets, over 150 autoshapes,
WordArt, freeforms, nested groups, connectors, text boxes, five picture formats, picture fills, a form button, a chart in a group), the five sample workbooks, and the 60,000-row benchmark
workbook. No `.xls` exists in the repository or on this machine.

| Run | Input | Files | Parts | Result |
|---|---|---|---|---|
| CI | `tests/captures/workbooks/xls/` (947 KiB, 14 files: 6 feature workbooks, 3 `xls1`, BIFF5 ×4, a sample) | 14 | 182 | 408 facets equal, **0 mismatches** |
| local | `xls1` (120 Excel-saved generated workbooks, 315 sheets, 117,410 cells) | 120 | 1,948 | 4,288 equal, 0 |
| local | the feature workbooks | 6 | 110 | 228 equal, 0 |
| local | BIFF5 (`SaveAs` 39 of 40 generated + the samples) | 45 | 450 | 1,387 equal, 0 |
| local | samples (5) and the benchmark workbook (389,615 cells) | 6 | 71 | 205 equal, 0 |
| local, hostile | byte-level mutants (truncations, flips, zeroed/0xFF runs, swapped blocks; `js/mutate-xls.mjs`) | 1,548 (746 refused by both, same class) | 11,045 | 28,583 equal, 0 |
| local, hostile | **record-level mutants** (Compound File repacked, BIFF records flipped/deleted/duplicated/swapped/truncated/retyped/inserted; `js/mutate-xls-records.mjs`), two seeds | 1,780 + 5,200 | 27,082 + 72,955 | 61,936 + 175,426 equal, **0** |

In all: 8,705 files, **113,661 parts byte-equal**, 0 mismatches (the first full run of the first corpus passed at once; the record-level mutants found C2-11, and
the byte-level ones nothing). The committed files are repacked by `js/pack-xls-subset.mjs` as a Compound File holding only the `Workbook` stream, with the WRITEACCESS record
(the user name) blanked and the author of the notes swapped for a same-length text; everything else in the stream is Excel's.

### Performance and memory (native, i5-14600KF, release; `examples/xls_bench.rs`, TS `js/bench-xls.mjs`, Node 24 `--expose-gc`, same machine and day; medians)

`calc-heavy-60000.xls` (`gen-workbooks.mjs --heavy 60000` saved by Excel: 9.44 MiB, 60,003 rows, 389,615 cells, 150,000 formulas; the output is 20.4 MiB):

| Benchmark (T13 row) | Budget | TS (two runs) | Rust | Verdict |
|---|---|---|---|---|
| `.xls` conversion (convert alone) | ≤ 0.5× TS | 424 / 445 ms (min 400 / 392) | **102.3 ms** (min 101.7) = **0.23–0.24×** | within |
| convert + read of every sheet (TS: `readWorkbook`/`readStyles`/`readSharedStrings`/`readWorksheet` of each; Rust: `Book::open_xls` + `load_all`) | — | 1,078 / 1,178 ms | **267.6 ms** = 0.23–0.25× | |
| retained after the conversion | — | 20.4 MiB (the parts) | **20.4 MiB** (the parts) | equal |
| peak while converting | — | process peak RSS 1.47 / 1.49 GiB (7 rounds, garbage included) | **73.5 MiB above the start** (input 9.4 MiB + the stream copy + the cell arena + the XML) | |
| retained by `open_xls` + `load_all` | — | — | 34.3 MiB (the package 20.4 + the store and model) | |

Small files (15–40 rounds): `f-shapes.xls` (0.46 MiB, over 150 shapes, 5 pictures) TS 22.8 ms, Rust 7.7 ms (0.34×); `e-charts.xls` (36 charts) 5.1 → 3.2 ms (0.63×);
`a-cells.xls` 1.6 → 0.8 ms (0.5×). History of the heavy number (the 10% rule starts here): first version 219 ms (formulas 107 ms of it: one `String` per
token), single-buffer decompiler and cells written straight into the arena: 103 ms.

Memory gates (`tests/xls_hostile.rs`, `uo-alloc`): **M1** convert, `open_xls`, `load_all` and drop leave exactly 0 B for every committed file and for 3 bit-flipped copies of each, for
one in 15 of 3,000 mutants, and for the error paths; **M2** 300 conversions of `e-charts.xls` are flat at tolerance 0.

### Security and fuzz

`tests/xls_hostile.rs`: junk, a header only, a Compound File without `Workbook`, a Word/PowerPoint/encrypted container, BIFF2–4, a stream without a BOF, FILEPASS, no sheets;
a file hyperlink with 65,535 steps (capped); a shared formula of ~4,000 bytes decompiled by 30,000 cells (passes under the default bound, `Limit("sheet size")`
under 1 MiB); 200 nested OfficeArt groups and 100,000 empty shapes (bounded in time); a chart that references A1:IV65536 3,000 times (bounded); a seeded loop of **3,000 record
mutants** of a workbook with cells, a formula, a hyperlink and a drawing (2,616 convert, none panics, each in < 3 s, one in 15 under the leak check) and 1,500
bit-flipped/truncated containers. Fuzz target **`xls_read`** (`core/fuzz/src/xls_read.rs`, appended to the fuzz crate: `Cargo.toml` feature on the `uo-xlsx` line + a `[[bin]]`,
`lib.rs`, `seed.rs`): byte 0 picks the container bytes, a `Workbook` stream in a minimal Compound File, or the records of one sheet between a fixed valid globals; what converts is
opened with the normal reader and loaded whole. The stable fallback driver, 28 seeds (the committed files and their streams): **1,352,907 executions in 170 s, 0 crashes, slowest
192 ms**. No `unsafe`.

### Tools (all under `crates/uo-xlsx/`)

`tools/xls-saveas.ps1` (Excel COM `SaveAs` of a folder: invisible, alerts off, read-only open, `-Format 56|39`; relative paths are resolved before Excel sees them),
`tools/xls-features.ps1` (the feature workbooks; UTF-8 BOM, `[string]` casts for COM), `js/gen-workbooks.mjs --gen xls1`, `js/capture-xls.mjs`, `js/dump-xls-parts.mjs` ↔
`examples/xls_dump.rs` (diff the parts of one file), `js/bench-xls.mjs` ↔ `examples/xls_bench.rs`, `js/mutate-xls.mjs`, `js/mutate-xls-records.mjs`, `js/pack-xls-subset.mjs`,
`js/gen-xls-data.mjs`. Excel cannot open the hostile `x1`/`x1-big` workbooks (a repair makes `Workbooks.Open` fail with alerts off), nor `LET` without `_xlpm.` names, nor
`cm` cells without `metadata.xml`: hence the `xls1` group.

### Lint and files

`clippy -D warnings` (native `--all-targets` with `xls,facet`, and wasm32 `--lib`) clean; `grep-bans.sh` clean; no float → integer `as` casts (checked with
`--features xls`: `float-casts.sh` runs the default features); `rustfmt` on the new files only (`src/xls/**`, the two tests, the two examples, `fuzz/src/xls_read.rs`). The default
`cargo build -p uo-xlsx` and its dependency tree are unchanged. `node tools/check-changed.mjs` (`--all-features`) stops in `src/grid/cf/` (frontier C3b, mid-edit: 7 clippy errors
in `engine.rs`/`formula.rs`/`value.rs`), before reaching this module; `uo-calc` was also mid-edit (a missing `stats` module for a while), so the benchmarks and the
parity runs of this record were made in a `git archive` snapshot of `HEAD` with the files of this module copied in (the tests were re-run in the working tree when it compiled again).
Changes outside the module: `core/fuzz` (the `xls_read` target: `Cargo.toml`, `src/lib.rs`, `src/bin/seed.rs`, new `src/xls_read.rs`, `fuzz_targets/xls_read.rs`) and `js/lib/gen-workbook.mjs`
(the added `xls1` group; the `x1` workbooks are byte-identical, `--check`). `Cargo.lock` and `fuzz/Cargo.lock` follow (the new edge `uo-xlsx` → `uo-drawingml`). Nothing in `uo-calc`, `grid/`, `capture-sheet.mjs`, `book.rs`.

### Open items

1. Unify the two copies of the grid metrics (C2-6) and, if wanted, the two chart ports (C2-4: an "Excel 97 only" flag in `uo_charts::biff`).
2. BIFF5/7 text in codepages other than the Windows-1252 family (C2-5): needs encoding tables.
3. The `uo-xls.wasm` binding and a browser run (C2-12); not measured in wasm.
4. Not captured: `.xls` with embedded OLE objects, external-workbook references (`SUPBOOK` of another file), pivot tables (the TS converter drops them too), VBA modules in BIFF5,
   and real-world files — no corpus exists here and none was deleted from this machine (the C1a corpus lab was erased on 2026-11-14).
5. Peak memory could drop ~25 MiB on the benchmark file by streaming the sheet XML straight from the records (the arena is needed for the patches and the ordering; sorted files without
   pending shared formulas could skip it).

**Process metrics.** C2 (Sonnet 5.5, closed scope): one session, about 2 hours of wall time, about 190 tool calls, no round with a verifier. Rework inside the session: (1) the generated
workbooks of C1a are hostile on purpose and Excel refuses them, so the `xls1` group had to be found by bisecting what Excel rejects (LET parameter names, file-only errors, `cm` without metadata);
(2) a relative destination path made every `SaveAs` fail (Excel resolves it against its own directory); (3) the record-level mutants found the nested-chart rule (C2-11) after the
first 100% on every Excel-made file; (4) performance, 219 → 103 ms (formula text built token by token as `String`s; `head()` allocated per cell). Environment notes: `uo-calc` and `grid/cf` were
mid-edit by other agents (see Lint and files); a bash heredoc eats double backslashes (the PowerShell and JS files with regexes were written with the file tool).


---

## C4 — as built (2026-11-24, Sonnet 5.5)

Frontier C4 of T9: the viewport display list of the Sheet viewer (feature `view`, `src/view/`), the two Sheet items of the
Slides list (`SheetFont`, `GlyphRun`), `uo-sheet.wasm` (`uo-wasm` feature `sheet`) and the read path of
`<universal-sheet engine="rust">`. Verified against the frozen TS (`ts-ref-092` at `b92b60fa`) with a recording 2D context over
the real element's `paint()` on the fake DOM (T10a).

### Code map

| Piece | Where | TS source |
|---|---|---|
| The list builder (arenas, symbol/face caches, canvas-state elision with the save/restore stack) | `src/view/out.rs` | the canvas calls |
| `paint()`, `paintRegion` (clip, white fill, grid lines, three phases, merges crossing the split, border pass), `paintCell`, overflow, `paintSelection`, `paintHeaders`, pane separators | `src/view/region.rs` | `universal-sheet.ts:3222-3500, 3861-3935, 4003, 4050` |
| `paintCellText` (plain, wrapped, justified, rotated, stacked, fill, centre-across, underline/strike, shrink to fit), the error mark | `src/view/text.rs`, shrink in `region.rs` | `cell-paint.ts` |
| Rich text (`paintRichText`, `richRunStyle`) | `src/view/rich.rs` | `cell-paint.ts`, `universal-sheet.ts:3935-3980` |
| Fills (solid, pattern tiles, linear, path rasters), data bars, icons, diagonals, comment indicator, filter arrow, table handle, spill border, `mix`/`rgba` | `src/view/chrome.rs` | `fills.ts`, `borders.ts`, `cf-icons.ts`, `universal-sheet.ts` |
| `Viewer` (activate: load, fit rows, CF engine, table layer; `emit`) | `src/view/mod.rs` | `loadXlsx`, `selectSheet`, `paint` |
| The two items | `schema/slides.uos` (`SheetFontItem`, `GlyphRunItem`, `advances: list<i32>`), generated codec Rust + TS | `text-metrics.ts:58, 104-130` |
| The TS painter replay (`paintGlyphRun`, `sheetFont`) | `uo-wasm/js/paint-slides.ts` | `drawCellText` |
| Module | `uo-wasm/src/sheet.rs`, `js/sheet.mjs`, `js/sheet-engine.ts` | |
| Component | `sheet/packages/sheet/src/{rust-engine.ts, sheet-engine.d.ts}`, hooks in `universal-sheet.ts` (`engine` attribute, `paintRust`) | |
| Harness | `js/capture-viewport.mjs`, `tests/viewport_parity.rs`, `tests/support/vp_canon.rs`, `tests/captures/sheet-viewport.capture` | |

### Wasm API (`uo-sheet.wasm`, handle `UoSheet`; layouts in `sheet.rs`)

`uo_sheet_open(bytes, clock)` → handle (`uo_sheet_error`), `uo_sheet_free`, `uo_sheet_sheets`, `uo_sheet_activate(i)` (0 or 12 =
measurements), `uo_sheet_viewport(h, sheet, w, h, dpr, sx, sy, zoom, sel*, flags, edit_col, edit_row)` → one message of the list stream
(status 0) or 12, `uo_sheet_needs` / `uo_sheet_font(id)` / `uo_sheet_provide` (the two-phase protocol of the Slides module: the table
asks for advances, whole strings of complex scripts and `'Hg'` boxes; the shell answers with `measureText`), `uo_sheet_bitmap(handle)`,
`uo_sheet_extent`, `uo_sheet_set_accent`, `uo_sheet_set_spill`, `uo_sheet_link_at` (Write's scheme filter: http, https, mailto, tel;
`#ref` for a location), `uo_sheet_recalc_plan` (open-recalc decision). JS: `sheet.mjs` `bindSheet` (explicit `free()` + a
`FinalizationRegistry`), `sheet-engine.ts` (loader, `SheetBook.paint`).

### Emission order (the TS line numbers are `universal-sheet.ts` of `b92b60fa`)

| Step | TS | Rust |
|---|---|---|
| `setTransform(dpr)`, `clearRect` | 3226-3229 | `region::paint` |
| `save`, `translate(46, 22)` | 3250-3252 | same |
| the four regions (main, top, left, corner) | 3254-3260 | `paint_region` x 4, same order |
| per region: `save`, clip, white fill, grid lines (one stroke), 3 phases | 3395-3422 | same |
| phase 0: merge white, `paintCellFill`, data bar, icon | 3470-3480 | `phase_fill` |
| phase 1: text (or the white patch of the cell in edition), error mark | 3482-3492, 3861-3935 | `phase_text`, `text.rs`, `rich.rs` |
| phase 2: border pass first, then diagonals, comment triangle, filter arrow, table handle | 3436, 3494-3516 | `border_pass`, `phase_marks` |
| quadrant clips of the drawings | 3263-3272 | clip + restore (drawings: C4-8) |
| selection, spill border | 4003 | `paint_selection` |
| headers, pane separators | 4050, 3300-3310 | `paint_headers`, `paint` |
| scrollbars, empty-state text | 3311, 3244 | shell |

### Decisions (declared)

| Id | Decision |
|---|---|
| C4-1 | **The list is the Slides list** with two items: `SheetFont { font: FontRef?, italic, css: sym? }` (the CellFont string `${italic}${400|700} ${ppem}px ${stack}`; a literal `css` for the headers; the face family is `name` or `name\|altName`, the shell's `fontStack` splits it) and `GlyphRun { text, x, y, advances, min_x, max_x, width, complex, mirrored }` with a new `advances: list<i32>` arena (one advance per code point). The schema gains the `advances` field: every producer of `SlidesList` literal gained `advances: Vec::new()` (list below); the Slides painter, PDF interpreters and tests are unchanged otherwise. |
| C4-2 | **Canvas-state elision** (`OutState`, with the save/restore stack): a repeated `fillStyle`/`strokeStyle`/`font`/`lineWidth`/`textBaseline`/`textAlign` is not emitted. The parity harness canonicalises both sides the same way (the state at the start of a frame is unknown), so the compared stream is "every canvas call that changes state". |
| C4-3 | **Canonical op stream**: floats as IEEE bits with the sign of zero normalised (`Math.round(-0.3)` is -0 in the TS and an `i32` here: not observable on a canvas), colours lower case, cell fonts reduced to `weight ppem family`, paths flushed at `fill`/`stroke`/`clip` (a `Path2D` call is the same path), gradients and pattern tiles by value (the RGBA of a tile by SHA-256), `GlyphRun` expanded as `drawCellText`. |
| C4-4 | **Icons**: C3b's op list folds `strokeStyle = c` into `Stroke(c)`; the TS sets it before the `lineWidth`/`lineCap` of the stroke (and, in the `!` symbol, sets it and never strokes). `emit_icon` re-places the assignment so the stream is the TS's. |
| C4-5 | **Borders**: the colour of an edge is set once per segment (`paintHEdge`/`paintVEdge`), the erasing gaps in white and back, as `run()`; the clip of the pass is `(-65536, 65536 + size)` (never the hostile width of a column) so scrolled sheets keep the rectangles the TS emits off the clip. |
| C4-6 | **Open-recalc**: `uo-sheet.wasm` reports the plan (`uo_sheet_recalc_plan`) and shows the file's values; applying it needs the engine that C1b's `save/engine.rs` is building (`Book::engine` glue). The module therefore does not contain the calc yet (wasm size below). |
| C4-7 | **Measurement**: the font table answers `NeedMeasures` after one frame (nothing is appended); `Viewer::activate` also loops (the row fit measures wrapped text). A face is cached by font id and checked against the font's identity (the table drops its fonts past its caps and reuses ids: found by the big run). |
| C4-8 | **Drawings and charts are not emitted** (open item 1): `paintDrawings` needs the `xdr` shape reader (uo-pptx) and `uo_charts` with the sheet-data host. The quadrant clips around them are in the stream; the capture stubs `paintDrawings`. Chart sheets paint the white page only. |
| C4-9 | Not in v1 (open items): list-validation dropdown of the selected cell, RTL camera render (`renderRegionToCanvas`: `GlyphRun.mirrored` and the painter replay exist, the region entry point does not), `SheetFont` face alt-name in the parity canon. |

### Parity (`cargo test -p uo-xlsx --features view,facet --test viewport_parity`; capture `tests/captures/sheet-viewport.capture` made by `js/capture-viewport.mjs`)

Per workbook, sheet and state (4 states: zoom 1/1.75/0.75/1.25, scrolls (0,0), (41.5,17.25), (123,57), (700.5,333), DPR 1/2/1/1.5, selections
A1, a range with a non-origin active cell, a cell, a range; the frozen panes of the model; the capture sets the 640 x 360 canvas) the canonical op stream after
`GlyphRun` expansion.

| Run | Input | Result |
|---|---|---|
| CI (`ci_viewport`) | 30 `x1` + 14 `paint1` + 40 `cf1` + 5 samples = 89 workbooks | **600 streams, 269,243 ops compared, 269,243 equal, 0 mismatches** |
| local (`UO_VP_CAPTURE=core/target/sheet-cap/viewport-big.capture`, release, `capture-viewport.mjs --big` 36 s) | 400 `x1-big` + `paint-big` + `cf-big`: 860 workbooks | **7,524 streams, 4,941,345 ops compared, 4,941,345 equal; 4 streams accepted**: sheets with a cell at a position named by another `r` (C3a-9: `x1-big/0257`, `/0267`) |

Round history: the first run was 241/303 equal. Classes found and fixed: rich text (not yet ported), the erase/colour order of the border
segments (C4-5), the line spans of wrapped text (`wrap_lines` returns `(start, end)`), the icon assignment order (C4-4, three variants), shrink to fit,
the face cache reused across font-table clears (C4-7), the sign of zero (C4-3).

### Memory, performance, size (native i5-14600KF, release, 1920 x 1080, zoom 1, `examples/view_bench.rs`; TS: `js/bench-viewport.mjs`)

| Scenario (1080p, zoom 1, scroll steps of the bench) | Rust emit (median) | TS `paint()` (median) | Ratio | Rust allocated / warm frame | Message crossing |
|---|---|---|---|---|---|
| showcase.xlsx | 0.006 ms | 0.013 ms | 0.46x | 5.0 KiB | 2.4 KiB (max 10.2 KiB) |
| giant-200k | 1.45 ms | 9.41 ms (24,938 canvas calls) | 0.15x | 236 KiB | 199 KiB |
| giant-1m | 1.12 ms | 14.71 ms (24,065 canvas calls) | 0.08x | 146 KiB | 194 KiB |
| giant-1m, zoom 0.5 (3,000+ cells, not a budget row) | 11.2 ms | - | - | 1.76 MiB | 717 KiB |

| Open | Rust (native) | TS (`loadXlsx`, Node) | Budget |
|---|---|---|---|
| giant-1m open -> first list | **171 ms** (open 166 ms) | 2,023 ms | <= 250 ms native: **met** (wasm 500 ms: not measured, see open items) |
| giant-200k open -> first list | **54 ms** | 337 ms | <= 60 ms native: **met** |

**Budgets missed**: the 1080p emission on the two dense giants (1.1-1.5 ms against 1 ms; 146-236 KiB allocated against 64 KiB; a 194-199 KiB message
against 48 KiB). The sample 1,500-cell viewport carries 25,000 glyphs (four-byte advances + two-byte units per glyph: 150 KB of the message). The rule
"slower than the TS or > 10% over the previous record fails" holds (8x to 13x faster than the TS); the absolute budgets are open item 2. Memory
gates (`tests/view_memory.rs`): M1 open + emit + drop leaves no live byte over the 84 committed workbooks, M2 600 scrolling frames flat at tolerance 0.
Slides regression: `uo-render`, `uo-dml-render` and `uo-pptx` suites green (the two PDF interpreters got arms for the Sheet items).

### Open items

1. **Drawings and charts** (C4-8): `emit_shapes` hookup (xdr reader, theme colours, media), `uo_charts` host over the sheet's cells, form controls, chart sheets.
2. **Budgets missed on dense sheets** (table above): the message is dominated by the glyph advances (4 B per code point) and the `texts` arena; options: advances as `u8` varints (a column of digits has 3-4 distinct values), a per-font advance table in the list (the painter already knows the faces), tile caching across scroll steps (evolution after M5), and a pooled `PathOp` buffer.
3. **Engine in the module** (C4-6) and the wasm size with the calc (SH3): load the calc lazily when the plan is not `none`.
4. RTL camera render, list-validation dropdown, alt-name in the canon (C4-9).
5. Browser: the component hooks are typechecked, not run in Chromium here (no node_modules in the worktree; the Write `node_modules` are needed for the existing bar tests); Chromium pixels second check not made.
6. The viewer's first frame measures after one round trip per new font/zoom: the shell answers with `measureText` (`sheet-engine.ts`).

### Wasm and size

`uo-sheet.wasm` = `cargo build -p uo-wasm --features sheet --target wasm32-unknown-unknown --profile wasm`: **1,505,770 B raw, 579,305 B gzip** (budget 1.6 MB gzip:
**met with a wide margin**), WITHOUT the calc engine (C4-6): `uo-calc` is linked only for the open-recalc decision. SH3 (calc inside or lazy) stays open
until C1b's engine glue exists; the margin (1.0 MB gzip) is the room for it. Tests: `tests/sheet_api.test.mjs` (open, sheets, measure loop, the viewport message
decoded with the generated TS codec and replayed by `paint-slides.ts` on a recording context, errors, M4 10,000 create/drop) passes in Node against the built module;
the WebKit run (`browser/run.mjs`) and Safari items were not run. Wasm timings of the emission were not measured (open item 7).

### Lint and files

`clippy -D warnings` clean native (`uo-xlsx --features view,facet --all-targets`, `uo-render --all-targets`) and `wasm32-unknown-unknown` (`uo-xlsx --features view --lib`,
`uo-wasm --features sheet`); `tests/grep-bans.sh` and `float-casts.sh` (`uo-xlsx uo-render uo-wasm`) clean; rustfmt on the new files. Slides regression: `uo-render`,
`uo-dml-render`, `uo-pptx` suites green; the Slides wasm tests are listed in the process metrics. TS: the component hooks typecheck (no new `tsc` error in
`rust-engine.ts`, `sheet-engine.d.ts` or the edited regions of `universal-sheet.ts`; the worktree has no `node_modules`, so the package's baseline `tsc` errors
and its vitest suites could not be compared or run).

Files (new): `src/view/{mod,out,text,rich,region,chrome}.rs`, `tests/{viewport_parity,view_memory}.rs`, `tests/support/vp_canon.rs`, `tests/captures/sheet-viewport.capture`
(89 workbooks, digests only: the lines are local, `--lines <dir>`), `examples/view_bench.rs`, `js/{capture-viewport,bench-viewport}.mjs`, `uo-wasm/src/sheet.rs`,
`uo-wasm/js/{sheet.mjs,sheet-engine.ts}`, `uo-wasm/tests/sheet_api.test.mjs`, `sheet/packages/sheet/src/{rust-engine.ts,sheet-engine.d.ts}`.
**Outside the module**: `schema/slides.uos` + the generated codec and `js/gen/slides*.ts`, `uo-render/src/slides.rs` (re-exports, validation), `uo-wasm/js/paint-slides.ts` (the two cases +
`paintGlyphRun`), `advances: Vec::new()` in the `SlidesList` literals of `uo-dml-render/src/emit/cx.rs`, `uo-pptx/tests/{pdf_slides,pdf_effects}.rs`,
`uo-render/tests/s_support/mod.rs`, `fuzz/src/slides.rs`; arms for the two Sheet items in `uo-pptx/src/pdf/{interp,raster}.rs`; `uo-wasm/{Cargo.toml,src/lib.rs}`;
`uo-xlsx/{Cargo.toml (feature view), src/lib.rs (one module line), src/grid/mod.rs (re-export), src/grid/paint.rs (layout_cell_layers split: `layout_cell_final` with
the final font flags, the kind of the value and a size override; same behaviour)}`, `tests/support/mod.rs`; `tools/build-pkg.mjs` (alias `@universal-office/sheet-engine`);
`sheet/packages/sheet/src/universal-sheet.ts` (the `engine` hooks: `startRust`, `openRust`, `paintRust`, `configureEngine`, `getEngine`; the TS path is unchanged
and default).

7. wasm timings of the emission; the WebKit/Safari run; `uo-sheet-editor.wasm` (C5).

**Process metrics.** C4 (Sonnet 5.5, closed scope): one session interrupted by a machine reboot; start 11:49:40, last gate about 13:10 local (`date`; about 1 h 20 min of
wall time including the reboot), about 330 tool calls, no round with a verifier. Rework inside the session: (1) the first parity run was
241/303 streams (rich text, border erase order, the `wrap_lines` span bug, three icon assignment orders, shrink to fit, the face cache across font-table clears, the
sign of zero); (2) the 4-state big run found the cf-big icon order and the stale face cache; (3) lint (disallowed `trim_start`/`is_whitespace`, float parse,
argument counts); (4) bash heredocs eat quotes and backslashes: scripts with quotes went through the Write tool; (5) the C: disk full: `TMP` on D:. Environment:
`core/target/sheet-c4` only.
