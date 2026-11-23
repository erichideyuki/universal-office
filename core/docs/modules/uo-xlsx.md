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
