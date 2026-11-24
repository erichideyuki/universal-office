# uo-calc — the one cell store and the calc engine (architecture, plan §4.0 + track A)

Record of the rust-port workflow (hybrid mode, `SKILL.md` "Architecture pass (Opus), only for the big shapes").
Status: **architecture pass done 2026-11-24 (Opus)**: data model, frontiers, captures, budgets; type skeleton in
`core/crates/uo-calc` (clippy clean native and wasm32). **Frontier S0 (the store) done 2026-11-24 (Opus)**: section
"S0 — the store, as built". **Frontier A1 (values, coercion, collation entry points, criteria, dates, stats base,
special functions, libm, math support) done 2026-11-24 (Opus)**: section "A1 — as built". **Frontier A2 (addresses,
the tree arena, parser, printer, registry contract, operators, apply, evaluator) done 2026-11-24 (Opus)**: section "A2 —
as built". **Frontier A3 (the engine: graph, full/incremental recalc, spills, CSE, cycles, iteration, volatiles) done
2026-11-24 (Opus)**: section "A3 — as built". **Frontier A4a (the shared function layer + the math family: math,
complex, engineering) done 2026-11-24 (Opus)**: section "A4a — as built"; A4b–A4f (the other five families) and A5 are
not ported yet.
The M3 gate (plan §11)
approves it together with the track C section of `uo-xlsx.md` ("Track C architecture").

Behaviour source (frozen, never edited): `D:\Projects\OfficeNext\.claude\worktrees\ts-ref-092` at `b92b60fa` (0.9.2,
`dist` built), called **TS** below. Paths are under `sheet/packages/` unless given; line numbers are raw lines of that
checkout; sizes are non-blank non-comment lines. **Every role of this crate runs on Opus** (`SKILL.md` "Models":
the Sheet calc engine), the store frontier S0 included (the engine's hot path lives in it).

## Decisions already applied (recorded; revisitable at the M3 gate)

| Id | Decision | Effect here |
|---|---|---|
| T2 | Font policy = the TS Sheet rule | not in this crate (`uo-xlsx.md` C3/C4) |
| R1 | Calc regex = `regress` with a step budget + Unicode-version check against Node 24.15 | A4d (`functions/regex.ts`, 293) |
| C1 | Locale = host hook | `locale::ValueLocale` + `HostHooks` + `Clock` + `RandomSource` (section "C1 host hook") |
| PERF | Maintainer, 2026-11-24: performance and memory are first-class; where performance and 1:1 TS parity conflict in INTERNAL representation, choose performance as long as observable output stays equal | the store design below; budgets and benchmark gates per frontier |

## Scope

What `uo-calc` owns: **the cell store of a workbook** (every cell of every sheet, once: file representation and
calc value in the same lanes), the interned strings, the file side of formulas, and **the calc engine** (values,
coercion, collation, criteria, dates, parse/print/A1, evaluator, 535 functions, implicit intersection, i18n of
formulas and values, open-recalc and save rules, add-ins, the function catalogue).

What it does not own: reading/writing the package (`uo-xlsx` feature `read`/`save`), the sheet model outside the
cells (styles, theme, CF, tables, drawings: `uo-xlsx`), number formats (`uo-numfmt`, called by `TEXT` and by the
grid), the grid and the viewport (`uo-xlsx` feature `grid`).

### Why a crate (reuse rule)

`SKILL.md`: a crate exists if it is used by more than one app or is a binding boundary. `uo-calc` is Sheet-only, but:
(1) the repo rule (`CLAUDE.md` "Pacotes") names `sheet-calc` an engine package "sem DOM fora dos apps" and the .NET
`UniversalOffice.Xlsx` recalculates on the server without the grid; (2) dependency direction: the store is the lowest
Sheet layer (`uo-xlsx` reader → store; calc ↔ store), and putting it in `uo-xlsx` would make the calc depend on the
file format; (3) `uo-xlsx`'s writer is pinned by an external consumer (`grid-xlsx`), which must keep building only the
writer — the calc and the store are an optional dependency of `uo-xlsx` (features `read`…`edit`). No other new crate:
the grid, viewport and session are features of `uo-xlsx` (as `uo-pptx` holds the presentation, its timing and editor).

### TS covered

| TS (non-blank lines) | Rust module (`uo_calc::…`) | Frontier |
|---|---|---|
| `engine.ts` `SheetStore` (89-160), `CellRec` (75-80), `key` (32); sheet-core `Row`/`Cell` value fields; the component's `cellIndex` (`universal-sheet.ts:4223-4236`); `from-sheet-core.ts` `cellValue`/`errorOf` (73-101); `to-sheet-core.ts` `fileCellValue`/`numberText` (38-55) | `key`, `strings`, `store`, `formula` | **S0** |
| `values.ts` 70, `coerce.ts` 281, `collate.ts` 12 + `collation.ts` 129 (already in `uo-text`), `criteria.ts` 109, `dates.ts` 64, `stats.ts` 219, `special-functions.ts` 409, `functions/libm.ts` 374 (double-double), `functions/math-util.ts` 251 | `value`, `coerce`, `criteria`, `dates`, `stats`, `special`, `libm` | A1 |
| `ast.ts` 32, `parse.ts` 520, `print.ts` 115, `a1.ts` 82, `evaluate.ts` 416, `apply.ts` 95, `ops.ts` 100, `fn.ts` 102 | `ast`, `parse`, `print`, `a1`, `eval`, `apply`, `ops`, `fn` | A2 |
| `engine.ts` 887 (minus SheetStore), `workbook.ts` 55 | `engine` | A3 |
| `functions/*` 8,818 (6 families below) | `functions::*` | A4a–A4f |
| `implicit.ts` 179 + `implicit-classes.ts` 511, `i18n/*` 1,825, `open-recalc.ts` 86, `save.ts` 127, `file-form.ts` 71, `addin.ts` 25, `to-sheet-core.ts` `writeValues` (harness only), `testing.ts` 297 (harness) | `implicit`, `i18n`, `open_recalc`, `save`, `file_form`, `addin`, `harness` | A5 |

`from-sheet-core.ts` `fromSheetCore` (105-181) and the bridge `sheet/src/sheet-calc.ts` (`ensure`, `writeBack`,
`createCell`, `removeCell`: ≈ 150 of its 665 lines) **disappear**: there is no second model to convert from or copy
back into. The rest of `sheet-calc.ts` moves to `uo-xlsx`: `XfPool` + its XML patch (482-853, ≈ 300 lines: styles,
C1b/C5) and `edit`/`barText`/`copyText`/`valueText` (C5).

## S. The cell store (data model; skeleton `src/store.rs`, `src/strings.rs`, `src/key.rs`, `src/formula.rs`)

### S1. What the TS keeps today, measured

`node --expose-gc` over the frozen dists (`openSpreadsheet` + `readWorkbook` + `readSharedStrings` + `readWorksheet`,
then the component's `Map<row, Map<col, Cell>>`, then `new Engine(fromSheetCore(…))`), heap after GC, 2026-11-24:

| Workbook | cells | sheet-core model | component index | engine | **total** | read | engine build | RSS after |
|---|---|---|---|---|---|---|---|---|
| `giant-1m.xlsx` (10,000 × 100 integers) | 1,000,000 | 132.5 B/cell | 37.0 | 69.5 | **239 B/cell (228 MB)** | 1.05–1.39 s | 0.56 s | 969 MB |
| `giant-200k.xlsx` (2,000 × 100, inline text every 7th column, styles) | 200,000 | 144.1 | 36.6 | 82.3 | **263 B/cell (52.6 MB)** | 0.31 s | 0.08 s | 377 MB |

Three copies, not two (plan §4.0 said "doubles"): the model (`Cell` objects with string `ref`/`value`), the
component's index and the engine's `CellRec` map. The engine is built lazily (`sheet-calc.ts:118`), so a TS viewer
pays 170 B/cell and an editor 239. The RSS shows the transient DOM of the whole sheet (`read.ts:289`).

### S2. Layout: column-major chunks of struct-of-arrays lanes

```
Store { sheets: Vec<SheetCells>, strings: StrPool, sst: Vec<Option<SstEntry>>, date1904 }
SheetCells { columns: Vec<Column>  (index = column),  rows: Vec<(row, RowMeta)>, col_styles: runs,
             hidden/filtered: RowSet, merges, extras: BTreeMap<CellKey, CellExtra>,
             formulas: FormulaTable, file_order: Option<Box<FileOrder>> }
Column  { chunks: Vec<Chunk> }       sorted by base, no empty chunk
Chunk   { base (multiple of 256), present: Dense | Rows(Vec<u8>), kinds: Uniform(CellKind) | PerCell(Vec<u8>),
          payload: Narrow(Vec<u32>) | Wide(Vec<u64>), styles: Uniform(u32) | U16(Vec<u16>) | U32(Vec<u32>) }
```

- **Why column-major.** The engine's hot loops are column scans (`SUM(A:A)`, `MATCH`/`VLOOKUP` over a column, the
  formula column index of `pendingPrecedents`, `engine.ts:799-830`, and `each`'s wide path, `engine.ts:137-160`).
  A column chunk is a contiguous run of lanes. The viewport (≈ 45 × 15 cells at 1080p) gathers each visible column's
  one or two chunks once per frame into a small row-major block (C4), so row-major painting costs nothing extra.
  Text overflow (a long text crossing empty neighbours) scans neighbours per column inside the gathered block.
- **Chunk of 256 rows.** A viewport touches ≤ 2 chunks per column; the chunk header (≈ 136 B) is ≤ 0.55 B/cell dense;
  row offsets fit a `u8`; inserting/deleting rows shifts whole chunks.
- **Kind byte** ([`CellKind`]): the low nibble is the value variant AND the file type (`ValueTag`: `Num`, explicit
  `t="n"`, `Bool`, `Err`, `Sst` (index kept for the save), `Inline`, `Str`, `Date`, other `t`, typed-without-value,
  `Empty`), so `cellValue` (`from-sheet-core.ts:73-101`) is a pure function of (kind, payload) — no per-cell string.
  Flags: `HAS_FORMULA`, `OWNED` (spill/CSE member: the TS `CellRec.owner`), `HAS_EXTRA`.
- **Payload lane**: `Narrow` when every payload fits 32 bits — numbers that are `i32` integers (not `-0`), string
  ids, shared-string indices, error codes, booleans; `Wide` (`f64` bits) otherwise. A non-integer number widens the
  chunk once (copy of ≤ 256 values); narrowing happens only on a chunk rebuild (load, compaction). NaN payloads and
  `-0` are kept bit-exact.
- **Style lane**: `s + 1` (0 = no `s` attribute: the TS distinguishes `style: undefined` from `0`, `serialize.ts:64`,
  and `setStyle(cell, 0)` deletes it, `sheet-calc.ts:197-199`).
- **Side tables, only for what is rare**: `CellExtra` (raw `<v>` text when it is not the canonical print, raw `r`,
  raw `t`, unparsable `s`, `cm`, `vm`, rich inline runs), `FileOrder` (sheets whose `<sheetData>` was out of order or
  had duplicates), `RowMeta` (rows with attributes), column default-style runs.

**Bytes per cell (estimates from the layout; S0 measures them with `uo-alloc` and they become M3 budgets):**

| Data | Bytes/cell |
|---|---|
| dense integer column, no style (giant-1m) | 4 (payload) + 136/256 (header) ≈ **4.6** |
| dense decimal column, uniform style | 8 + 0.55 ≈ 8.6 |
| mixed kinds + per-cell styles (`u16`) | 8 + 1 + 2 + 0.55 ≈ 11.6 |
| sparse cells (offset lane) | + 1 |
| each DISTINCT string (pooled once) | units × 1 (Latin-1) or × 2 + 12 (entry) + ≈ 8 (index) |
| formula (file side + engine side, excluding a distinct AST) | ≈ 24 (`FileFormula`) + text + 8 (column index) + ≈ 56 (`FormulaRec`) + deps ≈ **≤ 200 B/formula** |

giant-1m ≈ 4.6 MB retained (TS 228 MB, ≈ 50× less); giant-200k ≈ 9.6 B/cell + 28,000 distinct strings ≈ 2.8 MB (TS
52.6 MB). **Budgets (M3, fail over them):** ≤ 6 B/cell for giant-1m, ≤ 16 B/cell for giant-200k (strings included),
≤ 200 B per formula on the generated calc workbooks, and never a per-cell heap object (`Box`, `String`, `Vec`).

### S3. Values, strings and keys

- `CellKey(u64)` = `sheet << 34 | row << 14 | col` (0-based); its order is (sheet, row, col) = the row-major order of
  `SheetStore.each` (`engine.ts:122-160`); `ts_key()` = the TS `row * 16384 + col`. External-link sheets keep the TS
  numbering (`EXTERNAL_BASE + book × EXT_STRIDE + i`, `engine.ts:29-30`) and live in the engine (read-only caches).
- `Scalar` is 16 bytes (`Empty | Num(f64) | Str(StrRef) | Bool | Err(ErrorCode)`); `ErrorCode` is a `u8` whose value
  is `ERROR.TYPE` (`values.ts:44-48`). `StrRef` = `Pooled(StrId)` (a cell's string; copy, no refcount traffic) or
  `Temp(u32)` (a string built by the running evaluation, in the engine's per-pass arena, cleared at the end of `run`
  like the TS `cache`, `engine.ts:713-727`); a temp is interned only when written to a cell.
- **Strings are JS strings**: Latin-1 (1 byte/unit, covers pt/es text) or UTF-16 (`JsStr`/`JsString`), code-unit
  semantics, lone surrogates kept. `uo-numfmt` takes `&[u16]` (its D1): a Latin-1 string is widened on the call (a
  short stack buffer).
- **`StrPool`**: arena + 12-byte entries + content hash index; entries are **reference counted by cells** (and SST
  entries), freed slots are reused LIFO (deterministic ids), the arena is compacted when garbage > ½ (M2 flat under
  edits that churn strings). The shared-string table is **indexed at open, decoded on demand** (`sst: Vec<Option<…>>`
  filled as cells or calc ask; offsets of the `<si>` elements are kept by the reader) — rich runs in a separate table.

### S4. Formulas: file side in the store, parsed side in the engine

- `FormulaTable` per sheet: `FileFormula` (text index, `t`, flags `aca/ca/bx/dt2D/dtr/del1/del2`, `si`, boxed
  `ref/r1/r2` texts) by `FormulaId`, and per column `rows` ascending + `ids` + the dirty count (`dirtyCount`,
  `engine.ts:207`). **`col_order` keeps the columns in first-insertion order and never removes one**: `fc.keys()` in
  `pendingPrecedents` (`engine.ts:817`) iterates them in that order, which decides the push order and so the
  evaluation order (see "Order sites").
- The engine's `FormulaRec` (`engine.ts:36-72`) is created lazily under the same `(SheetIx, FormulaId)`: AST ids
  (`node`, `exec`), area, flags, `scan_pass`, deps slice. **Shared formulas share their anchor's AST** (`engine.ts:261-
  272`, 290-293; relative `RefPart`s are offsets, `ast.ts`), and the legacy `@` rewrite is cached per AST
  (`execCache`, `engine.ts:327`). Static precedents (`depsOf`, `engine.ts:826-858`) are computed per cell when first
  needed, as the TS; a per-AST relative "dependency template" (instantiated by offset) is an S0/A3 internal choice
  allowed by PERF if it gives the same areas (checked by the deps facet).
- Formula TEXT stays as read (the engine does not parse at open): a viewer that never edits never parses
  (`sheet-calc.ts:96-130`). The open-recalc rule reads texts only (`open-recalc.ts:93-131`).

### S5. One write path: recalc writes the store

The TS engine writes `rec.v` (`engine.ts:929-938`), records `changes` (`touch`, `engine.ts:939-943`), and the bridge
copies every changed cell back into the model with `writeScalar` (`sheet-calc.ts:230-262, 461-471`). In Rust the
engine's `put`/`write` IS the store write: `Engine::{recalc, calculate, set_cell}` take `&mut Store`, write kind +
payload directly with the **product** semantics of `writeScalar` (number → no `t`, `numberLiteral` text at save;
constant text → `inlineStr`; formula/spill text → `t="str"`; bool → `t="b"`; error → `t="e"`; `null` → no value,
and a cell left without value, formula, owner and style disappears), and creates a missing cell with the row/column
default style (`createCell(…, xfIndexAt(v, undefined, col, row))`, `sheet-calc.ts:253`; `cell-format.ts:9-31`:
`<row s customFormat>` wins over `<col style>`, last definition wins). Dynamic anchors get `t="array"`/`ref`/`cm`/`ca`
as `setDynamicAttrs` (`sheet-calc.ts:479-492`). The host receives `Changes` (per sheet: bounding area + count +
`grew`), not a cell list; the next viewport emission reads the store.

`to-sheet-core.ts:112-145` `writeValues` (`numberText` without `+` in the exponent, `null` → `'0'`) is NOT the product
path (only calc tests and the old I/O lab call it): ported under feature `harness` to replay the `calc-tests` group.

### S6. Laziness (open cost)

- Store fill is a streamed pass per sheet (`uo-xlsx` C1a); the engine is not built at open unless the open-recalc
  rule says `full`/`partial` (`sheet-calc.ts:98-112`), in which case every sheet must be loaded first.
- Engine build = parse formulas + analyze (no cell copy): giant-1m (no formula) costs 0 (TS: 0.56 s and 66 MB).
- The reverse dependency index (`rev`, `engine.ts:866-893`) and deps are built on first need and dropped by
  `clear_caches` (the TS rebuilds them lazily after any formula change, `ordered`/`rev = undefined`).

### S7. Order sites (TS `Map`/`Set` iteration that is behaviour)

| Site | TS | Observable? | Rust |
|---|---|---|---|
| `rangeMasters` (`engine.ts:205`), iterated in `setCell` (669) and `pendingPrecedents` (818) | insertion order | **yes**: push order → evaluation order → `RAND` sequence, `circular` membership, cached values of cycles | `Vec` in insertion order, removal keeps the rest's order |
| `formulaCols[sheet].keys()` (`engine.ts:817`) when the range is wider than the column count | first-insertion order of columns | **yes** (same chain) | `FormulaTable::col_order` |
| `formulas` `Set` (`engine.ts:203`): `recalc` (683), `calculate` (690), `run` (724), `rev` build (869) | insertion order | dirty marking is order-free; `rev` lists → `dependentsOfArea` result order → `markDependents` queue order (result is a set) and `dependentsOf` (only membership is used, `file-form.ts:106`) | insertion-ordered `Vec`; not compared beyond the evaluation-order facet |
| `rev.byCol` (`engine.ts:890`) | insertion order of columns | as above | ordered `Vec` index |
| `changes` → `takeChanges` (`engine.ts:946-951`) | sheet then key insertion order | no (the bridge writes each cell independently) | replaced by `Changes`; declared, not compared |
| `circular` (`engine.ts:212`) → `iterateCycles` sorts (1031) | — | no | `Vec` + sort |
| engine `cache` (`engine.ts:220`), `names`, `volatileNames`, `tableByName` | lookups only | no | hash maps |
| `XfPool.formatId` over `styles.numFmts` (`sheet-calc.ts:589`) | first id with the code, file order | **yes** (which id a typed format gets) | `uo-xlsx` styles: ordered `Vec<(id, code)>` |
| open-recalc `ws.rows`/`row.cells` (`open-recalc.ts:113-128`) | file order, duplicates included | the `partial` list order: no (`calculateCells` only marks) | store iteration; duplicates from `FileOrder` |
| function-level `Map`s (`UNIQUE`, `MODE`, `database.ts:94`, `statistical.ts:65`…) | per function | per function | A4 lists them in its record; `uo_jsnum::object` for `Record` key order |

## C1 host hook (decision C1, concrete)

- **`ValueLocale`** (`src/locale.rs`): the TS `ValueLocale` (`fn.ts:74-104`) field for field — `id`, `decimal`,
  `group`, `true_text`, `false_text`, `date_order` (MDY/DMY/YMD), `date_sep`, `time_sep`, `am`, `pm`,
  `month_names[12]`, `month_abbr[12]`, `day_names[7]`, `day_abbr[7]`, `currency`, `general_name?`,
  `format_letters?` (y/m/d/h/m/s), `r1c1?` (row/col letters). Fixed-size arrays (as `uo-numfmt` D2).
- **Browser**: the shell keeps `valueLocaleFor`/`deriveValueLocale` (`i18n/value-locale.ts:24-66`, TS code, `Intl`)
  and passes the record once per locale change (codec record, ≈ 1 KB). `pt-BR`/`en-US` select the measured tables
  in Rust (the TS returns them by identity, `value-locale.ts:61-62`; Rust compares the `id` the same way).
- **`HostHooks::regional_decimal`**: the `Intl.NumberFormat(id).formatToParts(1.5)` decimal of a language outside the
  Excel tables, used by the formula SYNTAX (`translate.ts:126`, `resolveSyntax` 117-123; also `formula-locale.ts:32`).
- **`functionNames` collation** (`translate.ts:684-692`, `Intl.Collator`): the list is the autocomplete UI's; Rust
  returns the catalogue (local name, canonical name, min/max args) unsorted and the shell sorts it with the same
  `Intl.Collator` call. No collator crosses the boundary.
- **`Clock`** (per evaluating call): local "now" as a serial (`nowSerial`, `engine.ts:186-189`, computed by the shell
  with the same expression; read once per run, `engine.ts:502`); it also replaces `new Date().getFullYear()`
  (`coerce.ts:197`, date entry without a year) and the CF `NOW`/today (`cf-formula.ts:405`, `cond-format.ts:526`).
- **`RandomSource`**: `Sequence` (captures replay the recorded `Math.random` values in call order) or `Seeded`
  (product; seeded by the shell from `crypto.getRandomValues`). V8's values are not reproducible anyway.
- **Case mapping with a locale**: `toLocaleUpperCase(lang)` for pt/en/es (`translate.ts:61`, `entry.ts:96-98`) equals
  the root mapping (`uo_text::case`); the host-default `toLocaleLowerCase()` of find/replace
  (`universal-sheet.ts:2867-2869, 2928`) uses the root mapping too (differs only on tr/az/lt hosts: SH2).
- **Server/.NET**: no `Intl`. `ValueLocale::pt_br()`/`en_us()` (the measured tables, `fn.ts:107-…`); any other id
  falls back by language (`pt*` → pt-BR, else en-US — declared); the C ABI/NuGet take an explicit `ValueLocale` so a
  host can pass its own. `Clock` from the host's local time (no time-zone database in Rust).

## S0 — the store, as built (2026-11-24, Opus)

Code: `src/strings.rs` (JS strings, `StrPool`), `src/store.rs` (lanes, chunks, `Store` API, iteration, gather),
`src/cell.rs` (the sheet-core cell ↔ lanes, `cellValue`, `errorOf`, `numberLiteral`, `numberText`, `fileCellValue`,
`writeScalar`/`writeBack` as `Store::write_value`), `src/load.rs` (`SheetLoader`: file-order streaming, `FileOrder`,
duplicates), `src/formula.rs` (`FormulaTable`), `src/facet.rs` (feature `facet`: the store facet). No `unsafe`
(`#![forbid(unsafe_code)]`); dependencies `uo-jsnum`, `uo-text` (+ `uo-testkit` under `facet`).

**API** (all 0-based; `SheetIx`): `get`/`get_key` → `RawCell {kind, payload, style}`; `set_file_cell`/`file_cell`
(the model cell, lossless); `remove`; `write_value(CellWrite, WriteKind)` (the product write path, S5); `set_style`,
`set_owned`, `default_style`/`style_at` (`xfIndexAt`), `set_col_styles`, `set_row_meta`; `each(area)` (row-major,
the TS `each` order), `column_runs(col, r1, r2)` → `ColumnRun` (`fold_numbers` fast path), `gather(r1..r2 ×
c1..c2, &mut Viewport)` (row-major block, buffers reused); SST: `set_sst_len`, `set_sst`, `resolve_sst(i, &mut dyn
SstSource)`, `cell_value(..) -> Result<Scalar, NeedSst>`, `cell_value_decoding`; `load_sheet(sheet) -> SheetLoader`
(`row`, `cell(FileCell)`, `cell_raw(RawCell)` — the reader's fast path for canonical cells, with
`cell::canonical_number(text)`), `shrink_to_fit` (re-narrows lanes), `check_invariants`, `lane_bytes`.

**Decisions taken in S0 (declared; internal representation per PERF, observable output equal):**

| Id | Decision |
|---|---|
| S0-1 | The kind nibble is the `t` attribute only (`None`, `N`, `Bool`, `Err`, `Sst`, `Inline`, `Str`, `Date`, `Other`) and bit 7 is `HAS_VALUE` (`<v>` present): the skeleton's `Empty`/`TypedEmpty` tags are gone, so a typed cell without `<v>` needs no extra. |
| S0-2 | Payloads: numbers (`None`/`N`/`Other`) `i32` narrow or `f64` bits; `Bool` 0/1; `Err` the code; `Sst` the index; `Inline` the pooled `textOf(is)`; `Str`/`Date` the pooled `<v>` (the `t="d"` serial is computed on read). An empty `<v>` text on a number cell stores NaN (raw text in the extra), so `cellValue`'s `'' → null` is a function of the lanes. |
| S0-3 | Canonical = what needs no extra: number text `numberLiteral(Number(v))`, boolean `1`/`0`, error text exact, SST index `String(i)` (`i < 2^32`), `<is>{text}</is>` on inline cells and no `<is>` elsewhere, `s` an integer in `0..2^32-1`, `r` the A1 of the position. Everything else (raw texts as `JsString`, `s`/`cm`/`vm`/`si` as the JS number `parseInt` gave: NaN, `-0`, > 2^32 survive) lives in `CellExtra`. |
| S0-4 | `-0` written by the engine keeps its bits in the payload (the engine value); the file text prints `"0"` (`numberLiteral`), as the TS (its engine holds `-0`, its model `"0"`; only a reload reads `+0`). |
| S0-5 | Rich runs: `RichRun {text, rpr, props}` where `props` is a host-owned id (uo-xlsx C1a's run-property table); the store never reads it. |
| S0-6 | `StrPool`: two arenas (Latin-1 bytes, UTF-16 units: no `unsafe` reinterpretation), 12-byte entries, its own open-addressing index (4 B per bucket, SipHash keyed by `RandomState`: ids stay deterministic — they come from the LIFO free list —, crafted collisions cannot degrade it), the empty string pinned as id 1, compaction when garbage > ½ and ≥ 4 KiB. |
| S0-7 | Column directory = `Vec<Column>` indexed by column (24 B per column up to the last used one; a lone cell in XFD costs 393 KB) — chosen for `get` speed over a sorted directory. |
| S0-8 | A cell outside A1:XFD1048576 is refused (`CalcError::Limit`); the TS model keeps such rows (`read.ts:574`). C1a decides drop + warning. Limits default: 64M cells, 4096 sheets, 2^24 formulas per sheet, pool 2^24 entries / 512 MiB (256 MiB wasm32). |
| S0-9 | A decimal in an integer chunk widens the whole chunk (≤ 256 values); narrowing happens on `shrink_to_fit` (load end). The M2 soak shows the cost: a 100×100 window touched at random widens ≈ 4,000 chunks (+8.3 MB, then flat). |
| S0-10 | Duplicate `<c>`: the LAST wins in the lookup (the facet/`storeOf` rule and `fromSheetCore` + `SheetStore.set`); earlier ones go to `FileOrder::shadowed` (owned `FileCell`). Rows/cells out of order are detected while streaming; the order so far (ascending by construction) is rebuilt from the store at the first anomaly, so canonical sheets pay nothing. |
| S0-11 | `ymdToSerial` for `t="d"` ported inside `cell.rs` (A1 owns `dates.ts`; same arithmetic, trap-tested); `errorOf` uses `uo_text::case` (full Unicode upper case: `#dıv/0!` → `#DIV/0!` as V8). |
| S0-12 | `OWNED` is not in the store facet (the TS model has no owner); A3's facets cover it. |

**Parity** (`tests/parity.rs`, capture group **store**: `js/capture-store.mjs`, the TS adapter = `capture-sheet.mjs`'s
`storeOf` rule + the file-order input of the test-only loader + `cellValue` per cell + per-parameter traps; committed
`tests/captures/store.capture` 19 KiB + `store.side.gz` 146 KiB, 0.2 s):

| Run | Input | Result |
|---|---|---|
| CI (`store_facet_ci`) | 5 samples (giants excluded) + 30 generated x1 workbooks: 84 sheets, 6,814 cells, streamed in file order (out-of-order rows/cells, duplicates, `SH1` refs, non-canonical `<v>`, lone surrogates) | `st`/`fo`/`rw`/`cv` 336 facets + traps (1,750 calls: `numberLiteral`, `numberText`, `fileCellValue`, `errorOf`, `cellValue` over 10 types × 47 texts × formula × `<is>` shapes × date1904): **0 mismatches**; cross-check against uo-xlsx `sheet-samples`/`sheet-x1` (independent adapter): 252 `st`/`fo`/`rw` + 84 `pv` (model after the open's write-back, from its sidecar lines) **0 mismatches** |
| local (`UO_STORE_BIG=1`, `store-big.capture`, 47 s capture, 26 MB sidecar in `core/target/sheet-cap`) | + giant-1m, giant-200k, 400 x1-big workbooks: 437 workbooks, 1,290 sheets, 1,844,178 cells | 5,161 facets **0 mismatches**; cross 342 equal, 0 mismatches; 5.9 s release |

**Memory** (`tests/store.rs`, `uo-alloc` counting allocator):

| Item | Result |
|---|---|
| M1 (each op + its inverse, then `compact` + `shrink_to_fit`: live bytes exact) | 6 ops green: file cell with every extra + formula + remove; `write_value` text + clear; overwrite a string cell and restore; rich inline + remove; load a sheet + drop; gather + each + column scan. Found and fixed: an emptied `BTreeMap` keeps its root leaf (1,776 B) — dropped when the extras table empties |
| M2(a) store soak | giant-1m + 1,000,000 random ops (400,000 in the debug CI run) on a 100×100 window spread over all chunks (40% constant text from 50,000 keys, 30% numbers half decimal, 20% clear, 10% file text cells with raw refs), sampled every 50,000: warm-up 4.49 → 12.8 MB (chunk widening, S0-9), then **flat: spread 41 KB over 1M ops (release), 14.6 KB over 400k (debug)** (tolerance 256 KiB) |
| M3 bytes/cell (live bytes after `shrink_to_fit`) | **giant-1m 4.49 B/cell** (budget 6; estimate 4.6), **giant-200k 10.99 B/cell** with its 28,000 distinct strings (budget 16; estimate ≈ 14), chain formulas **52.4 B/formula** file side + cell (budget 200); peaks: giant-1m 4.79 MB, giant-200k 2.22 MB |
| M5 | the pool refuses past `max_entries`/`max_bytes` (an existing string is still found), the store past `max_cells`/`max_sheets` and outside the grid, leaving no string behind; compaction keeps ids |

**Robustness**: `tests/support/ops.rs` interprets bytes as store operations and checks them against a `BTreeMap`
model of sheet-core cells (file cell, `cellValue` written straight over the model, row-major iteration, gather =
get, column runs, `fold_numbers`) plus `check_invariants` (sorted non-empty chunks, lane lengths, `HAS_EXTRA`/
`HAS_FORMULA` against the side tables, pool reference counts = holders) every 64 ops (≤ 1,024 ops per input). Seeded test `ops_seeded`
(300 inputs + 4 hostile, ≈ 2 s debug) and the fuzz target **`store_ops`** (`core/fuzz/src/calc.rs`). Found while
building it: the payload lane was widened after the kinds lane changed (a string id widened as a number: a pool
reference leak) — fixed (widen first); `each` on a sheet with no column indexed `columns[0]` — fixed.
Bounded local run of `store_ops` with the stable fallback driver (`cargo run --release --no-default-features --features
fallback --bin fallback -- --secs N store_ops`, 14 seeds from `calc_seeds`): 117,099 executions in 270 s, **0 crashes**,
slowest input 120 ms (a first version checked the whole model every 16 ops and took up to 3 s per input: the harness,
not the store; now every 64 ops). No `unsafe` in the crate, so no Miri job.

**Benchmarks** (`benches/store.rs`, native, i5-14600KF, medians):

| Benchmark (S0 rows of "Performance budgets") | Median | Budget | Verdict |
|---|---|---|---|
| store fill giant-1m from decoded lanes (`SheetLoader::cell_raw`, row-major, 10,000 × 100) | **20.0 ms** | ≤ 40 ms | within |
| column scan sum 1M (`column_runs` + `fold_numbers`, one column of 1,000,000 integers) | **0.38 ms** | ≤ 1 ms | within |
| random get 1M (giant-1m, seeded positions) | **3.7 ms** | ≤ 15 ms | within |
| viewport gather 45 × 15 | **1.6 µs** | ≤ 20 µs | within |
| viewport gather 120 × 40 | **8.9 µs** | ≤ 120 µs | within |
| store bytes/cell (M3 above) | 4.49 (giant-1m) / 10.99 (giant-200k) | ≤ 6 / ≤ 16 | within |
| info: fill giant-1m through `FileCell` (the model path, one `JsString` per `<v>`) | 140 ms | — | the reader must use `cell_raw` for canonical cells |
| info: fill giant-200k (28,000 inline strings through `FileCell`) | 11.2 ms | — | — |
| info: column sum 1M by `get()` | 1.44 ms | — | — |

The first run had random get 1M at 16.4 ms (over budget): `Column::find` binary-searched the chunk list; a direct probe
(chunk `i` at base `i·256` when the column is filled from its top) brought it to 3.7 ms. These are the first S0 records
(the 10% regression rule starts from them). The TS has no equivalent operation to compare (its store is the `Map` of
`CellRec`s, S1); wasm numbers come with the `sheet` suite of `tools/bench/speed` when C1a/C4 wire the store into
`uo-sheet.wasm`.

## A1 — as built (2026-11-24, Opus)

Code (`core/crates/uo-calc/src`): `value.rs` (+ `ScalarView`, `ArrayValue::{get, fill, of}`, `broadcast_at`,
`lift_shape`), `coerce.rs` (`coerce.ts`), `collate.rs` (`collate.ts` + the JS-string entry points of `collation.ts`,
whose rules stay in `uo_text::collation`), `criteria.rs`, `dates.rs`, `stats.rs`, `special.rs`
(`special-functions.ts`), `libm.rs` (`functions/libm.ts`), `math_util.rs` (`functions/math-util.ts`), `locale.rs`
(+ `ValueLocale::{en_us, pt_br}` = `fn.ts` `EN_US`/`PT_BR`, `Clock::current_year`), private `jstext.rs` (UTF-16 view of a
`JsStr` without allocating, digit runs) and `cast.rs` (the named float → int conversions of the float-casts gate). No
`unsafe`. `uo-text` is now used with its feature `collation` and gained one additive getter,
`CollationKey::weights()` (covered by its M1 test).

**API** (all pure functions; text is a borrowed `JsStr`, values a `ScalarView`, an Excel error is `Err(ErrorCode)`):
`to_number`/`to_text`/`to_bool`/`number_to_text`/`parse_number_text`/`parse_date_time_text`/`parse_date_time_parts`/
`compare_scalars`/`same_text_ci`; `compare_text_js`/`compare_text_ord`/`text_equals`/`collation_key`/`round15`/
`compare_numbers`/`PreparedText` (a text compared many times); `parse_criterion(crit, locale, year, prefix) ->
Criterion` + `Criterion::matches`, `Wildcard`, `has_wildcard`, `compare15`, `locale_error_code`; `dates::*`
(`ymd_to_serial`, `serial_to_ymd`, `weekday`, `serial_to_hms`, `days_in_month`, `is_leap`, `split_serial`,
`excel_date`, `max_serial`); `stats::*` (`xl_add`, `xl_sum`, `stat_num`, `mean`, `devsq`, `variance`, `skew`,
`skew_p`, `kurt`, `sorted`, `Collect`, `zip_sparse`, `arg_number`/`arg_int`/`arg_bool`, `BIG_AREA`); `special::*`
(every export of `special-functions.ts`); `libm::*` (every export of `libm.ts`); `math_util::*` (`fin`, `fin_flush`,
`approx15`, `exponent_of`, `snap_add`, `decimal_round`, `excel_power`, `atp_num`, `nums`, `EachNumber`, `TRIG_LIMIT`,
`sin_ext`/`cos_ext`/`tan_ext`).

**Decisions taken in A1 (declared; observable output equal unless said):**

| Id | Decision |
|---|---|
| A1-1 | The `RegExp`s the TS builds from the locale's separators (`numberCore`, the time pattern) are small backtracking matchers that try the alternatives in V8's order (greedy, the ES empty-iteration rule of `?`), so any separator string (empty, multi-unit, digits) gives the TS captures. They carry a step budget (2^20) and a group-nesting limit (512 groups): the only inputs that reach them are an EMPTY group separator over a long digit run (catastrophic backtracking in V8: the TS hangs) and ≥ 1,536-digit grouped numbers (a value ≥ 1e308, rejected by the TS too); Rust answers "not a number" there. The other `RegExp`s (`/ *: */g`, `split(/ +/)`, the mixed fraction, AM/PM, the criterion operator) are deterministic and hand-coded. |
| A1-2 | NaN payloads are not compared: V8 hands out `0x7ff8000000000001` or `0x7ff8000000000000` depending on the tier that ran the code (the `uo_jsnum::conv::canonicalize_nan` convention); the replay accepts a NaN-for-NaN difference with that reason (191 CI / 1,568 big calls: results of NaN-producing arithmetic — `∞ − ∞`, `0·∞`, `log` of a negative — in the statistics, special functions and libm). |
| A1-3 | The four series/continued fractions without an explicit TS bound (`logcf`, the small-x series of `pgamma`, `pdUpperSeries`, `pdLowerSeries`) stop at `special::LOOP_CAP` (2^26) iterations. The branch conditions of `pgamma` keep their ratios ≤ 0.8 (x < 1 for the small-x series), so for every finite input they converge in a few hundred steps and the cap never binds; it only bounds hostile inputs. |
| A1-4 | Host values are explicit (C1): `year` (dates typed without a year; the TS `new Date().getFullYear()`) comes from `Clock::current_year()`, `date1904` is a parameter where the TS has a default. |
| A1-5 | A criterion is data (`Criterion`, an enum of the TS closures' branches) with `matches`; its text side is prepared once (`PreparedText`: Latin-1 bytes + collation key; `Wildcard`: canonicalized tokens), and the wildcard `RegExp` (`^…$`, `i`, no `u`) is a glob over code units with the `RegExp` `Canonicalize` of each unit, which accepts exactly the texts the regular expression accepts. |
| A1-6 | `localeErrorCode` = `canonicalError(text, locale.id)` (`i18n/translate.ts:663`) is ported here for what the TS registers: only `pt-BR` (any case) resolves to the pt-BR error names (embedded), every other id to English (`pt` alone resolves to `pt-PT`, which has no table). A5 owns the i18n tables and re-points this to them. |
| A1-7 | The walkers that need the evaluation context (`collectNumbers`/`collectInto`, `scalarsOf`, `gridOf`, `zipCells`, `numericPairs`, `eachNumber`, `asMatrix`, `atpRaw`, `atpList`, `liftScalars`) and the `FnDef` factories (`numFn`, `atpFn`, `def`) are wired by A2/A4 (they need `FnContext` and `Value` resolution, A2's types); A1 ships their per-value rules (`Collect`, `EachNumber`, `zip_sparse`, `broadcast_at`, `lift_shape`, `atp_num`, `arg_*`), replayed through a fake context in the capture. |
| A1-8 | `stirlerr` of a negative half-integer reads past its table in the TS and returns `undefined`; Rust returns NaN (any use is arithmetic: NaN in both). The capture records that case as NaN. |
| A1-9 | Hot paths allocate nothing (measured, `a1_robust::memory_m1_and_zero_alloc_hot_paths`): `parse_number_text`/`to_number` of a number text (a short Latin-1 text is widened on the stack; the digits for `Number` are built in a stack buffer; a text without any digit is rejected first — every accepted form has one), `Criterion::matches` (numbers, wildcard, ASCII and Latin-1 text), `compare_text` of ASCII (a weight table) and Latin-1 text (a 256-entry table of collation elements built once from `uo_text`'s keys, streamed level by level). Non-Latin-1 text still builds `uo_text` keys per comparison (the TS caches 20,000 keys; a per-pass key cache is an A3/A4 choice). The date/time path allocates its word list. |
| A1-10 | `sorted` = `Float64Array.prototype.sort`: numeric, `-0` before `+0`, NaN last (payload kept). `ArrayValue::of` has no `RangeError` past ~10^5 rows (already listed in "JS semantics that are traps"). S0's integer-only `ymd_to_serial` in `cell.rs` stays (same arithmetic on its domain); `dates::ymd_to_serial` is the general one (V8's `MakeDay` with its year/month range checks, `Date.UTC`'s 0–99 years, `setUTCFullYear`). |

**Parity** (`tests/a1.rs`):

| Group | Input | Result |
|---|---|---|
| **a1** (new, `js/capture-a1.mjs`, committed `tests/captures/a1.capture.gz` 521 KiB) | every A1 entry point of the frozen TS called directly (83 entry points), traps + seeded inputs, four locales (en-US, pt-BR, a de-DE host locale with NBSP group, `.` time separator, YMD order, € and German months, and `xx` with an EMPTY group, `٫` decimal, `h` time separator), clock frozen at 2025-01-15: texts from a token grammar (digits of both scripts, every separator, currencies, signs, parentheses, %, exponents, months in any case, AM/PM, lone surrogates), locale-formatted numbers and dates, 678 criteria × 59 cell values per locale (+ `prefix` in en-US/pt-BR), dates with fractional/negative/huge/NaN parts, statistics lists, `collectNumbers` through a fake context, special functions and libm over wide ranges | CI: **52,402 calls, 52,211 equal, 191 NaN-payload accepted (A1-2), 0 mismatches**; local `--big` (`core/target/sheet-cap/a1-big.capture.gz`, 10× the seeded inputs, 19 s capture): **486,022 calls, 484,454 equal, 1,568 NaN accepted, 0 mismatches** |
| **calc-tests** (A1 calls) | the A1 entry points the TS test suite calls (`compareText`, `compareScalars`, `parseDateTimeParts`, `parseNumberText`, `numberToText`, `toBool`, `compareNumbers`, `compare15`, `ymdToSerial`, `serialToYmd`, `weekday`, `serialToHms`, `snapAdd`, `excelPower`), results re-serialized as the recorder's canonical JSON | CI **548/548 equal**; big **1,262/1,262 equal**; 5 calls not replayable (`parseCriterion` returns a closure, `wildcardRegExp` a `RegExp`: their behaviour is in the a1 group) |
| calc-traps / bank | callFn per Excel function / `evaluate` | not replayable before A2–A4 (they enter through the evaluator); none of the 23 TS calls that never return (`COMBINA`, `BESSEL*`, `COUP*`, `ODDFPRICE`, `FORECAST.ETS.CONFINT`) hangs in A1 code — their loops are in `math.ts`, `engineering.ts`, `daycount.ts`, `financial.ts`, `ets.ts` (A4) |

**Robustness** (`tests/a1_robust.rs`, seeded, ≈ 3 s debug): 20,000 random UTF-16 texts (lone surrogates, both digit
scripts, every separator) through coercion, dates, criteria, comparisons and wildcards under random host locales
(empty, multi-unit and digit separators; worst call group 0.34 ms), 20,000 random bit patterns through every numeric
function (worst 7 ms), and the hostile patterns (4,000 digits with an empty group: < 0.5 s; 20,000 groups; 200
stars over 5,000 units; a 40,000-unit date): no panic, bounded. No fuzz target is listed for A1 (the record's targets
are formula/workbook/regex/store); the seeded tests cover the same ground in seconds.

**Memory**: M1 for every allocating entry (outputs dropped, live bytes back exactly) and zero allocations on the hot
paths (A1-9). The only caches are two lazily built static tables (Latin-1 collation elements, 14 KiB; Latin-1 `RegExp`
canonicalization, 512 B), fixed size (M5 trivially). The TS's `numReCache`/`charCache`/`keyCache` do not exist here.

**Benchmarks** (`benches/a1.rs` vs `js/bench-a1.mjs`, the frozen TS in Node 24.15 on the same inputs, i5-14600KF,
medians of 7, ns per call):

| Operation | Rust native | TS Node | Rust / TS |
|---|---|---|---|
| `parseNumberText` pt-BR, 100k mixed texts | 104 | 743 | 0.14 |
| criteria `>=50` match (number cells / text cells) | 2.9 | 10.9 | 0.27 |
| criteria `=1` match (text cells parsed as numbers) | 100 | 614 | 0.16 |
| criteria `a*` (wildcard) | 3.1 | 12.3 | 0.25 |
| criteria `abc` (collation equality) | 4.9 | 16.7 | 0.29 |
| `compareText` ASCII pairs | 9.0 | 102 | 0.09 |
| `compareText` Latin-1 (accented) pairs | 10.5 | 577 | 0.02 |
| sort 20k ASCII words by `compareText` (per item) | 181 | 619 | 0.29 |
| `numberToText` | 243 | 432 | 0.56 |
| `crLog` (double-double) | 243 | 578 | 0.42 |
| `xlExp` (x87 EXP) | 259 | 703 | 0.37 |

Rework found while building: the first `compareText` of Latin-1 text went through `uo_text` keys (1,500 ns, 2.6× the
TS, which caches keys) → the Latin-1 element table (10.5 ns); a criterion's ASCII text was prepared as UTF-16 and
missed the fast path (259 ns) → narrow storage (4.9 ns); `numberCore` formatted its exponent with `to_string` (20 B per
call) → stack digits. These are the first A1 records (the 10% regression rule starts from them).

## A2 — as built (2026-11-24, Opus)

Code (`core/crates/uo-calc/src`): `a1.rs` (`a1.ts`), `ast.rs` (`ast.ts`: the tree arena + feature `facet`'s canonical
JSON), `parse.rs` (`parse.ts`), `print.rs` (`print.ts`), `fnreg.rs` (`fn.ts`: the registry contract, `FnContext`,
`Throw`; `fn` is a keyword), `fn_table.rs` (generated by `js/gen-fn-table.mjs` from the frozen TS registry + the
catalogue, `--check` fails when stale), `ops.rs` (`ops.ts` + the pass arena of texts), `apply.rs` (`apply.ts`), `eval.rs`
(`evaluate.ts`: `trait EvalHost`, `Evaluator`, `EvalState`), `functions/mod.rs` (the A4 hook: `body(ix)` = `None` for
every function today). `value.rs` changed shape (A2-1). No `unsafe` (the stack probes in `examples/` paint memory, as
uo-docx's). Function bodies (A4) and the engine (A3) are not here: evaluating a registered function stops with
`Throw::NotPorted`, and the parity harness checks that the TS called a body at exactly those points.

**API**: `parse::FormulaParser::parse(&mut Ast, JsStr, &ParseOptions) -> Result<NodeId, SyntaxError>` (reusable
buffers; a failed parse rolls the arena back), `parse::number_literal_value`; `print::{print_formula, print_into}`
(`PrintOptions {row, col, r1c1, ui}`); `a1::*` (every export of `a1.ts`); `fnreg::{FN_TABLE, find, lookup_fn,
catalog_entry, FnMeta::arg_mode, FnContext, Arg, Thunk, CallArgs, FnBody, Throw, EvalResult}`; `apply::{apply_fn,
implicit_intersection, top_left, FnDef, ApplyOptions}`; `ops::{arith, compare, compare_values, concat, negate, percent,
num_result, add_sub, add_sub_top, power, TempStrs}`; `eval::{EvalHost, Evaluator::{eval_formula, eval_node,
call_lambda, parse_ref, text}, EvalState::{clear_pass, release}, Scope, TableDef, MAX_EVAL_DEPTH, MAX_NAME_DEPTH}`;
`ast::{Ast, Node, NodeId, MAX_AST_DEPTH, Ast::walk}`.

**Decisions taken in A2 (declared; observable output equal unless said):**

| Id | Decision |
|---|---|
| A2-1 | **Shapes.** One `Ast` arena per engine: 24-byte `Node`s + a 4-byte position, argument lists as spans of one child vector, texts interned once (dedup by content, SipHash with `RandomState`), a reference part packed in 4 bytes (`n` as `i29` + abs bit; `-0`, non-integers and offsets past ±2^28 — only R1C1 `R[…]` makes them — in a side table), array constants as `Arc<[Scalar]>` (evaluating `{…}` copies nothing). Function names resolve at parse time to a row of `FN_TABLE` (or an interned name). `value.rs`: `ArrayValue.data` is `Arc<[Scalar]>` (a LET binding used twice shares it, as the TS shares the object), `RefValue` is `One(Area)` inline or `Many(Arc<RefList>)` (union areas + a one-sheet flag), `StrRef::Lit` points at a literal of the arena (evaluating `"abc"` copies nothing). Value 32 B, Scalar 16 B. |
| A2-2 | **Depth.** The parser runs the grammar on an explicit stack of continuations (no recursion; memory bounded by the text). Operators and parentheses never recurse anywhere: the evaluator runs them on a work stack (`Work`: left, its `operand`, right, its `operand`, apply — the TS order) and the printer on a task stack, so `1+1+…+1`, `1^(1^(…))`, `((…))`, `----1` have **no depth limit** (the TS overflows V8 at ≈ 2,100 chained operators when evaluating, 3,100 nested parentheses when parsing, 7,000 when printing — a `RangeError` the engine turns into `#NUM!` or, at parse, an exception it does not catch). Function/call nesting is limited to `MAX_AST_DEPTH` = 512 (deeper text is `SyntaxKind::TooDeep`; the TS parses ≈ 2,300 nested functions; Excel accepts 64). The evaluation recursion (functions, calls, names, LAMBDA bodies) is guarded by `MAX_EVAL_DEPTH` = 1024 levels: past it `Throw::Range`, which is `#NUM!` at the innermost LAMBDA call or for the formula (the TS `RangeError` path); the TS `MAX_DEPTH` 300 for names/LAMBDAs is kept and binds first in every measured case. Stack, measured (probes below): native release (Windows x64) 433 B per nested function (511 levels: 221 KB), worst 515 KB (a LAMBDA recursing 300 times, the TS cap); wasm32 shadow stack 133 B per nested function (511 levels: 68 KB), worst 219 KB; operators and parentheses ≈ constant (4 KB native, 3.5 KB shadow) at 100,000 levels. A walker added later (A3 dependency scan, A5 rewrites) must not recurse through operators: `Ast::walk` is iterative. |
| A2-3 | `FormulaSyntaxError` messages (Portuguese, with the formula text) are not reproduced: no consumer reads them (`engine.ts:240, 277, 280`, `entry.ts:253`, INDIRECT test `instanceof` only). The class and the position are compared; the kind is an enum (`SyntaxKind`). |
| A2-4 | A `(` never closed at the end of the text: the TS reads past its token list (`this.peek()` undefined) and throws a `TypeError` instead of a syntax error — its engine and INDIRECT rethrow it. Rust returns `SyntaxKind::UnclosedParenAtEnd`; `parse_ref` (INDIRECT) maps it to `Throw::Internal` (the TS `internalErrors` + `#VALUE!` path); the parity harness maps it to the TS `TypeError` (14 CI records, 25 calc-tests calls). |
| A2-5 | An R1C1 relative offset of ±Infinity (a `R[…]` with 309+ digits) evaluates to `#REF!`; the TS builds an area with NaN corners (empty cells). Finite huge offsets wrap exactly as the TS (`fmod` on doubles). |
| A2-6 | Host R1C1 letters (`ValueLocale.r1c1`) are sets of code units of their upper and lower case; a letter that is a `RegExp` metacharacter (`]`, `-`, `\`, `^`) is taken literally (the TS builds a character class from it). Measured letters are R/C and L/C only. |
| A2-7 | INDIRECT's letters: `locale.r1c1`, else L/C when the id lower-cased is `pt-br` (the only language table with other letters; `en` falls back to R/C, the default), else R/C. A5 owns `getLanguage` and re-points this. |
| A2-8 | The registry is static data generated from the TS (535 names = the catalogue = the registry; prefix, min/max, flags, argument modes sampled at positions 0..599 and stored as fixed prefix + repeating cycle — exact for every TS `args(i)`, e.g. `COUNTIFS` value/ref alternation, `CUBEVALUE` 255 positions). The registry's own prefix is kept separately from the catalogue's (the printer uses the catalogue's first, as `print.ts`). |
| A2-9 | `EvalHost`/`FnContext` are object-safe traits; cell reads return `Result<_, Throw>` (`NeedCell` for A3, as the TS exception); strings are resolved through `text(StrRef)`; `defined_name` takes `Option<SheetIx>` (`None` = the TS `-1`) and returns a node of the same arena; tables are borrowed `TableDef`s. The TS `cache` (`Map<string, unknown>`) is not in the trait yet: A4 adds the typed caches its lookups need (record "Memory plan", 64/256 MiB cap). |
| A2-10 | Texts built while evaluating go to the pass arena `TempStrs` (Latin-1 or UTF-16, capacity kept across passes); past 256 MiB native / 64 MiB wasm32 a new text is `Throw::Internal` (TS: keeps allocating; only a `&` chain building > 256 MiB of intermediate text in one pass reaches it — each step is still capped at 32,767 units as the TS). LET frames, variables and closures live in per-pass vectors, truncated when a LAMBDA call or a formula returns anything but a LAMBDA (a `MAP` over a million elements does not grow them). |
| A2-11 | The union `,` appends in place (unshared list) and keeps a one-sheet flag, so `(A1,A1,…)` costs O(1) per area (TS: copies the list at each `,`, O(n²); 50,000 areas: 44 s debug before → 187 ms). |
| A2-12 | Function calls recycle their argument vectors (`EvalState` pool); `apply_fn` converts the arguments in place. The lifting memo keys are built exactly as the TS (`s<text>`, `n<String(x)>`, `e<code>`, `b<true\|false>`, `z`, space-separated — collisions included). |

**Parity** (`tests/a2.rs`; group **a2**: `js/capture-a2.mjs`, committed `tests/captures/a2.capture.gz` 1.1 MiB, 2 s
capture):

| Run | Input | Result |
|---|---|---|
| CI (`a2_ci`) | 10,496 formulas (the bank's 2,840, the generated workbooks' 1,530, a seeded grammar over every syntax of `parse.ts` — references A1/R1C1/3D/external/structured, names, `_xlpm/_xlop/_xleta`, prefixes, arrays, TRIMRANGE, every white space — and seeded mutations, hostile shapes up to 500 levels): 21,167 parse records (6,352 errors) × printed in 6 forms + round trip; 15,775 evaluations over a mock workbook (4 sheets, names incl. LAMBDA/recursive/sheet-scoped/broken, two tables, a spill anchor, an external book; both array contexts, en-US and pt-BR): 6,252 computed, 9,523 stopped at a function body in both; 14,350 operator calls over 42 trap values; 128 `a1.ts` calls; 28 `numberLiteralValue`; 1,500 `applyFn` calls over synthetic definitions (every mode, lift, acceptsErrors, volatile, omitted, thunks, legacy/array context; the body's call log compared) | **158,153 facets equal, 0 mismatches** |
| local (`UO_A2_BIG=1`, `--big`, 14 s capture) | 62,307 formulas: 118,323 parse records, 103,572 evaluations (55,226 computed), 15,000 `applyFn` | **612,438 facets equal, 0 mismatches** |
| calc-tests (`a2_calc_tests`) | the A2 calls of the TS test suite: `parseFormula` (502), `printFormula` (300; trees with A5's extra keys), `a1ToCell` (349), `numberLiteralValue` (91), `lookupFn` (200: name, min/max, prefix, flags, modes) | CI **1,442 equal**; big **30,186 equal**, 0 mismatches |

**Robustness** (`tests/a2_robust.rs`, debug ≈ 14 s): hostile texts — 1M nested parentheses, a 300,000-term chain,
1M unary minus, 200,000 right-nested powers, 100,000 nested functions (refused at 512), 1M-digit number (refused),
1M-unit text and name, unterminated quotes/brackets, a 200,000-element array, a 50,000-area union, 100,000 arguments —
parsed, printed in four forms, round-tripped and evaluated in A1 and R1C1, each ≤ 1.1 s debug; 20,000 random UTF-16
texts (lone surrogates, formula tokens) the same way (worst 0.3 ms); the depth limit exactly at `MAX_AST_DEPTH` and the
arena rollback of a failed parse. Fuzz target **`formula_parse`** (`core/fuzz/src/calc.rs`: option byte + Latin-1 or
UTF-16 text → parse, four prints, re-parse, evaluation in both contexts over an empty workbook; 16 seeds), bounded
local runs with the stable fallback driver: 397,360 executions in 300 s and 765,531 in 600 s (`--seed 12345`),
**0 crashes**, slowest input 170 ms. Open item: the very first 300 s run (same deterministic sequence, while cargo
builds ran in parallel) did not stop and was killed after 10 min with no artifact; two reruns with an input dump did
not reproduce it — the orchestrator may want a longer libFuzzer run with a per-input timeout.

**Stack probes** (decision A2-2): `examples/eval_stack_native.rs` (paints 24 MiB below the caller, `cargo run --release
--example eval_stack_native`) and `examples/eval_stack_wasm.rs` + `js/wasm-stack.mjs` (paints the shadow stack below an
export; budget 512 KiB of the 1 MiB shadow stack, the script fails over it), seven shapes × depths up to 100,000.
Rework found by them: the first evaluator recursed per node (464 B native per parenthesis, 1.26 KB per chained
operator: a 1,000-term chain needed 1.26 MB, more than a Windows main thread); splitting frames helped little (Win64
frames are ≥ 100 B), so the operator machine and the iterative printer replaced the recursion.

**Memory**: M1 for parse + print + evaluate of seven formula shapes (arena, state, texts, closures dropped: live bytes
back exactly); **zero allocations** evaluating eight scalar formulas (arithmetic with references, `%`, comparison of
texts, `&` of texts, `LET`, parentheses, an 8-term chain) once the state is warm (`memory_m1_and_zero_alloc_eval`).
Caches: none new (the pass arenas are data, capped by A2-10; `FormulaParser` buffers are reused, not cached).

**Benchmarks** (`benches/a2.rs` vs `js/bench-a2.mjs`, the frozen TS in Node 24.15 on the same inputs, i5-14600KF,
medians of 7, ns per formula):

| Operation | Rust native | TS Node | Rust / TS |
|---|---|---|---|
| parse, 9,028 bank + generated formulas at their cells | 479 | 1,151 | 0.42 |
| print (file form) | 168 | 215 | 0.78 |
| **parse + print** (PERF row: budget ≤ 0.5× TS) | **687** | **1,457** | **0.47** |
| evaluate 10,000 operator formulas (refs, arithmetic, comparisons, `&`, `%`, `^`, LET, intersections, array constants) | 344 | 601 | 0.57 |

These are the first A2 records (the 10% regression rule starts from them). Wasm numbers come with the `sheet` suite of
`tools/bench/speed` when the engine is wired into `uo-sheet.wasm`.

## A3 — as built (2026-11-24, Opus)

Code (`core/crates/uo-calc/src/engine/`): `mod.rs` (types, `Engine::new` = the TS constructor + `load` in FILE order,
`analyze`, `nameVolatile`, the public API), `graph.rs` (the formula column index `formulaCols`, the record slab,
`allFormulas`, the reverse index), `host.rs` (the engine's view of a TS `CellRec` derived from the store's lanes, the
`EvalHost` implementation), `recalc.rs` (`setCell`, `markDependents`, `pendingPrecedents`, `computeRoot`,
`evaluateRec`, `spill`, the CSE fill, `run`, `iterateCycles`, the one write path). Store additions (S0 files):
`Store::{claim_owned, clear_content, set_formula, formula_attrs_mut}`, `CellWrite::Pooled` (a pooled text written
without a copy) and a number fast path in `write_value`; `FormulaTable::get_mut`; `RandomSource::{seeded, sequence,
next_f64, consumed}`; `Ast::tree_key` (hash-consing). No `unsafe`.

**API**: `Engine::new(&mut Store, &EngineInput, &EngineOptions, Option<&mut dyn SstSource>)`; `recalc`, `calculate`,
`calculate_cells` (the partial open recalc) and `set_cell(.., Option<CellInput>)` take `&mut Store`, the `Clock` and the
`RandomSource` of the call and return `Changes`; reads `value`, `for_each_value`, `has_formula`, `owner`, `spill_of`,
`circular`, `internal_errors`, `not_ported`, `precedents_of`, `dependents_of`, `orphans`; `take_trace` (evaluation
order, `EngineOptions::trace`); `check_invariants`, `heap_bytes`, `clear_caches`. `EngineInput` carries what is not in
the store: sheet names, defined names, tables, external caches, `calcPr`, the dynamic-array `cm` values (the reader,
C1a, fills it). Edits: `CellInput::{Value, Text, Formula(FormulaInput { text, array, dynamic, ref, ca })}`.

**Shapes (PERF).** A formula is a 32-byte `Rec` in a slab (sheet, row, col, flags, tree, scan pass, walk epoch, load
order, template): no per-cell heap object anywhere in the graph. Formula trees are **hash-consed per sheet** at parse
(`Ast::tree_key`: every field, text positions included — relative references are offsets, so `A1+1` in B1 and `A2+1` in
B2 are one tree; a shared formula's children use the anchor's tree as the TS). **Static precedents are a template per
(tree, sheet)**: the reference nodes of the tree with names expanded in the TS `depsOf` order; a reference without a
sheet prefix is instantiated by arithmetic at the formula's cell (the evaluator's `refOf`: absent part → sheet edge,
absolute → itself, relative → wrapped offset), any other (prefix, 3D, external, table) through the evaluator; nothing is
stored per formula. The **reverse index** (built at the first `markDependents`, as the TS) keeps per column the row
intervals bucketed by `⌈log2 length⌉` and sorted by first row: a stabbing query is `O(log n + hits)` per class where the
TS scans the column (the reason a 20k-chain edit costs the TS 0.6–1.2 s); once built it is **maintained incrementally**
on formula edits (the TS drops and rebuilds it). The **dirty set** is a list: a pass sorts the dirty formulas by
(sheet, row, col, load order) and touches nothing else; past 1/8 of the formulas it walks the sorted `allFormulas`
(rebuilt lazily). `formulaCols` keeps the TS first-insertion order of columns (the `fc.keys()` order site of S7), each
column's current formula per row (probe at `row − first row`, binary search otherwise) and its dirty count (orphans
included, as `dirtyCount`); `rangeMasters` is an insertion-ordered `Vec`. Owners of spilled/CSE cells are one map by
cell key (+ a bit for `ignoreCache` claims), mirrored by the store's `OWNED` bit.

**Decisions taken in A3 (declared; observable output equal unless said):**

| Id | Decision |
|---|---|
| A3-1 | The TS `CellRec` is derived from the store: a cell exists for the engine when it has a current formula, an owner, or a non-null `cellValue`; a formula cell without a value reads 0 (`inp.v ?? 0`; a data table `null`); a styled cell without a value is invisible (the TS never loads it). `ignoreCache`: a formula reads 0 until written (flag on the record), an owned cell `null`; the file value a cycle restores (`cached`) is kept per formula (texts as arena literals). `-0` is kept (S0-4); a cell is written only when the JS `!==` says it changed (`0`/`-0` equal, texts by content). |
| A3-2 | One write path (S5): the engine writes the store with the bridge's kinds (`formula` / `spill` / `constant` by the cell's state), `claim_owned` creates a spilled cell with the row/column default style, a released cell is stripped and deleted unless styled; `setDynamicAttrs` updates the anchor's `<f>` attributes (`t="array"`, `ref`, `ca`, `cm` = the first dynamic `cm` when it spills) whenever its value or area changes. `Changes` = per sheet the bounding area and the number of writes (not distinct cells) + `grew`; the TS restore of a cycle's cached value (`rec.v = m.cached`, untouched) is a store write here and counts as a change. |
| A3-3 | **Not here — A4**: function bodies. An evaluation that reaches one gets `#VALUE!` and is listed in `not_ported` (sheet, row, col, function). **A5**: the legacy `@` rewrite (`exec` = the tree: legacy formulas with ranges/tables/multi-cell names evaluate as dynamic ones until A5), `_xll.` add-in resolution, `sourceText` with implicit positions (`FORMULATEXT` returns the original text or the printed tree), `setComputedCriterion`/`criterionAt` (wired when the database functions land), the `cellInfo` host callback (C5; `fileName` is done). |
| A3-4 | Duplicate `<c>` elements: the store keeps the last; the shadowed earlier formulas are loaded from `FileOrder` as **orphaned records** (the TS keeps them in `formulas`: they are evaluated in sheet order and write the cell); their own cached value under `ignoreCache` is not kept (the live duplicate's value is in the store). |
| A3-5 | A formula the TS parser throws a `TypeError` on (an unclosed `(` at the end, A2-4) makes the TS constructor throw; Rust treats it as a syntax error (`#NAME?`). A syntax error in a defined name: `#NAME?` as the TS. |
| A3-6 | A non-finite number written by the engine reads back as empty (the S0-2 NaN convention); no operator produces one (`num_result`), only a future hostile body could. |
| A3-7 | `values()` of an area still above 2^24 cells after the used-row cap: `Throw::Range` (`#NUM!`) instead of allocating; `iterateCount` capped at 32,767 (NaN or ≤ 0 → no sweep, as the TS loop). |
| A3-8 | A table name used alone resolves to a node carrying the table's own name (the TS keeps the case as written: same evaluation, a printed `FORMULATEXT` could differ in case). |
| A3-9 | Shared strings: `Engine::new(.., Some(src))` decodes every entry the sheets use; a read of an undecoded entry gives `""` (the reader must pass its source). |
| A3-10 | Evaluator fast paths (A2 code, A2 parity re-run clean): an own-sheet cell operand that the parent operator converts at once is read directly (`Work::Operand`; unary `+` keeps references, `+A1#`), `Num × Num` arithmetic skips the coercion context, the clock's year is computed once per call. |
| A3-11 | `RandomSource::Seeded` is xorshift128+ (53-bit doubles), `Sequence` replays captures; the product seeds it from the host. |

**Parity** (`tests/a3.rs`, group **calc-order**, re-captured with the `ops` group, the `legacy`/`opts` facets and the
`full.vals` sidecar lines; replayed from the captured `CalcWorkbook` streamed into a store through the S0 loader —
out-of-order rows and duplicates included; `UO_A3_KEY=… debug_key` prints a run's order against the TS):

| Run | Input | Result |
|---|---|---|
| CI `calc_order_ops` | 40 operator-only workbooks × 14 runs (`recalc`, `calculate`, 12 edits: values, texts, operator formulas, dynamic and CSE arrays, `ca`, deletes) + the full recalc of the final inputs | **560/560 runs: evaluation order equal, random values equal, `circular` 40/40 equal, 129,106 cell values equal, incremental-vs-full verdict 40/40 equal, 0 mismatches**; 2,915 values deferred (the legacy `@` of a sheet-scoped name used on another sheet: A5) |
| CI `calc_order_x1` | the 30 generated x1 workbooks (functions everywhere) | order equal in 199/420 runs, 221 deferred (a body ran); `circular` 30/30 equal; **62,928 values equal, 0 mismatches**, 36,688 deferred; incremental-vs-full 8 equal, 22 deferred |
| local `calc_order_big` (`UO_A3_BIG=1`, release, 14 s) | x1-big (400) + ops (400): 11,200 runs, 1,012,819 evaluations | order equal 6,990, deferred 4,210, **0 mismatches**; `circular` 756 equal, 44 deferred; **7,601,138 values equal, 0 mismatches**, 3,434,705 deferred; incremental-vs-full 495 equal, 305 deferred |

**Deferred = what waits for A4/A5, precisely**: a cell is deferred when its evaluation reached a function body
(`not_ported`), when it is a legacy formula the TS rewrote with `@` (capture facet `legacy`; edit formulas by a
conservative text test), when a step's order differs because a body ran and two formulas (orphaned duplicates) write the
same cell, or when it depends — through the static graph, a spill footprint that differs or an owner — on such a cell.
A run's order is deferred when a body ran in it or when the order without the deferred cells is equal (a body-less
anchor reads `#VALUE!` where the TS has `#SPILL!`, so an edit dirties it there and not here). The `RAND` sequence is
deferred with its body (`RAND`/`RANDBETWEEN`/`RANDARRAY` are A4): in the CI x1 group 208 runs consumed random values
in the TS; the order sites that decide it (`rangeMasters`, `fc.keys()`, the static precedent order) are compared by the
order facet. When A4 lands, the same replay turns the deferred cells into compared ones with no harness change.

**Robustness** (`tests/a3_robust.rs`, debug ≈ 7 s): a 200,000-formula chain (1,000,000 in release) whose first formula
needs the whole chain, on a **256 KiB thread** (no recursion in the scheduler) + an edit at its bottom; a 50,000-formula
ring with and without iteration (all circular; previous values kept / converges); **incremental = full** on 60 seeded
acyclic operator workbooks × 30 edits (400 in release), every value compared with a fresh engine's full recalc
(138,477 values in debug); 150 seeded **hostile workbooks** (1,500 in release: cycles, CSE areas, spills into each
other, merges, duplicates, iteration, `ignoreCache`, every kind of edit) with the engine's and the store's invariants
checked after every call (worst workbook 18 ms debug). Fuzz target **`calc_workbook`** (`core/fuzz/src/calc.rs`: bytes
→ workbook + edit script, invariants after every call; 10 seeds): bounded local run with the stable fallback driver (`--secs 180 calc_workbook`, 11 seeds): 1,216,203 executions, **0 crashes**, slowest input 15.7 ms.

**Memory**: M1 — `recalc` + `calculate` + `dependents_of` + `clear_caches`, an edit and its inverse, and building and
dropping an engine leave the live bytes exactly where they were. M2 — 1,000 × (edit → calculate) on a 6,000-cell
workbook (numbers, texts, a formula replaced back and forth, a spill anchor re-typed), sampled every 50: after warm-up
the series stays within 0.4 KB (tolerance 64 KiB). M3 — engine heap on shared-1m: **50.3 B/formula** (50.3 MB for 1,000,000 shared formulas; 62.9 B with the reverse index built) — budget ≤ 200 B/formula; the build's peak equals what it keeps (42 MB: the load streams the file order in bounded batches — a first version materialized one record per cell and peaked at 213 MB); a recalc peaks at 9.4 MB above its start (the sorted formula list). TS: 636 MB retained (the same cells); M5 — the only per-pass
buffers are the evaluator's arenas, cleared after every formula (A2-10 caps them), and the scheduler's scratch vectors.

**Benchmarks** (`benches/a3.rs` vs `js/bench-a3.mjs`: the frozen TS engine on the SAME cells, built as its
`CalcWorkbook`; one process each, i5-14600KF, medians of 5 after a warm-up; Rust native release; operator-only
variants where the baseline class needs a function body):

| Class | formulas | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | TS retained MB | Rust engine heap MB |
|---|---|---|---|---|---|---|---|---|
| chain-100k (`A{i}+1`) | 99,999 | 147 | 62 | 30.4 | **17.5** | 0.57 | 88.7 | 6.0 |
| sum3-100k (`A{i}+A{i+1}+A{i+2}*2`; the window `SUM` is A4) | 100,000 | 293 | 85 | 47.2 | **39.7** | 0.84 | 54.2 | 6.0 |
| text-50k (`A{i}&"x"&i`; the text functions are A4) | 50,000 | 106 | 48 | 58.0 | **37.5** | 0.65 | 58.9 | 21.7 |
| dynamic-1k (20 spills of `$A$1:$A$1000` with operators; SORT/UNIQUE/FILTER are A4) | 20 | 1.0 | 0.1 | 5.2 | **4.2** | 0.81 | — | 0.4 |
| volatile-10k (`ca` formulas + dependents; RAND is A4): recalc / **calculate** | 20,000 | 38.5 | 18.6 | 5.2 / 318 | **3.5 / 4.1** | 0.68 / **0.013** | 14.3 | 9.3 |
| cycles-iterate (1,000 two-cell cycles, iterate 100/0.001) | 2,000 | 7.8 | 1.4 | 22.1 | **1.3** | **0.06** | — | 0.1 |
| shared-1m (one shared formula over 1M cells) | 1,000,000 | 1,097 | 198 (**0.18×**) | 706 | **167** | **0.24** | 636 | 48 |
| edit: head of a 20k chain (setCell + calculate), p50 / p95 | — | — | — | 1,262 / 1,340 | **4.8 / 5.7** | **0.004** | — | — |
| edit: an isolated cell, p50 / p95 | — | — | — | 0.14 / 0.29 | **0.00 / 0.01** | — | — | — |

TS retained MB is the heap after GC per class run in its own process (`--only`); negative or missing values of the
all-classes run are GC noise. The record's TS baseline (`sheet.mjs`, through the .xlsx reader) has chain-100k at 72.7 ms
and shared-1m at 1,390 ms; against those, chain-100k is 0.24× and shared-1m 0.12×.

**Verdict (PERF rows of A3)**: shared-1m (recalc 0.24×, build 0.18×, memory 13× smaller), the cycles, `calculate` with
volatiles and the edit latency (5.7 ms p95 native for 20,000 dirtied formulas; TS 1.3 s — the reverse-index scan) are far
inside the budgets. **The small-formula classes miss the ≤ 0.5× native budget** against the same-cells TS: chain 0.57×,
text 0.65×, volatile recalc 0.68×, dynamic 0.81×, sum3 0.84× (all < 1.0×, so no hard failure). Measured by phase
(Instant probes, since removed): the scheduler costs ≈ 25 ns/formula (precedent scan) and the write-back ≈ 40 ns; the
rest is the A2 evaluator's operator machine (≈ 100 ns for `A1+1`, ≈ 300 ns for three references and three operators) —
it is the limit, not the graph. Already done here (A3-10): direct cell operands, `Num × Num` without the coercion
context, the cached year, a canonical-lane `cellValue` fast path, the dense probe in the column index. Proposed for the
stage-5 pass (maintainer decision, not done): compile each hash-consed tree once into a flat instruction list
(templates make that one compile per filled-down column) instead of re-walking the node arena per evaluation. Wasm
numbers (edit ≤ 16 ms p95) come with the `sheet` suite when the engine is wired into `uo-sheet.wasm` (C4); the wasm32
stack probe after the evaluator change: worst 147 KB of the 1 MiB shadow stack (A2: 219 KB), native worst 511 KB (A2:
515 KB).

**Open items**: (1) the tree arena only grows: a long editing session typing distinct formulas keeps their trees (≈ 100
B each; filled-down and retyped formulas are deduplicated) — an arena compaction belongs with A5/C5's edit session; (2)
`calc-tests` Engine calls (24 engines, 159 calls) are not replayed yet: almost all of them go through function bodies —
A4 adds the replay together with the bodies; (3) the wasm edit-latency number (C4).

## A4a — as built (2026-11-24, Opus)

Two things: **the shared function layer** every family uses (so A4b–A4f run in parallel, each owning one module) and
**the math family** (`math.ts`, `complex.ts`, `engineering.ts`; `math-util.ts` and `libm.ts` were A1's pure rules,
their context walkers and factories are here). Plus RAND/RANDBETWEEN (`math.ts`; RANDARRAY is `dynamic.ts`, A4e),
the A4 parity harness (bank + calc-traps through the TS `testing.ts` entry points), and `SheetLoader::strings` for the
C1a reader.

**Code** (`core/crates/uo-calc/src/functions/`):

| File | Content |
|---|---|
| `mod.rs` | the dispatch (`body(ix)`: one slot per `FN_TABLE` row, built once from the families' lists), `ported()`, the family contract (below), the consistency test |
| `shared/mod.rs` | `R<T> = Result<T, Stop>` (`Stop::Err` = an Excel error value, `Stop::Throw` = a host throw), `done`, outputs (`num`, `err`, `boolean`, `num_or_err`, `fin_value`, `text`, `ascii`), argument access (`arg`, `scalar`, `len`, `EMPTY`), `view`, `to_num` (`math-util`'s `num`), `to_num_d` (`tdl-util`'s), `top`, the `bodies!` macro |
| `shared/mathu.rs` | `math-util.ts` walkers and factories: `NumSpec` + `num_fn`/`num_args` (`numFn`), `atp_fn` (`atpFn`), `Nums` (the converted numbers on the stack, ≤ 16), `Raw`/`atp_raw`/`atp_num`, `lift_scalars`, `each_number`/`each_number_ref`, `as_matrix`, `atp_list` |
| `shared/statw.rs` | `stats.ts` walkers for A4b/A4e: `collect_numbers`, `collect_into`, `collect_into_skipping` (`skipCell`), `scalars_of`, `grid_of`, `zip_cells` (the sparse path for two big areas), `numeric_pairs` |
| `shared/tdl.rs` | `tdl-util.ts` (used by A4c, A4d, A4e, A4f): `lift_atp` (`AtpArg::{Omitted, S, Raw}`), `atp_num`, `num`, `txt`, `int`, `scalars_of`, `existing_scalars`, `as_array`, `SPARSE_CELLS` |
| `shared/agg.rs` | the basic `Agg`s of `statistical.ts` that `database.ts` also uses: `average`, `max`, `min`, `sum`, `product`, `count`, `var_s`, `var_p`, `stdev_s`, `stdev_p` |
| `math/mod.rs` | `math.ts`: 79 bodies (SUM … MUNIT, ECMA.CEILING/ISO.CEILING sharing bodies, ROUNDBAHT*) |
| `math/complex.rs` | `complex.ts`: COMPLEX + 25 IM* (format, parse, arithmetic, the measured trigonometry) |
| `math/engineering.rs` | `engineering.ts`: 12 base conversions, 5 bit functions, CONVERT (the unit tables, prefixes, temperatures), DELTA, GESTEP, 4 ERF*, 4 BESSEL* |
| `stats.rs`, `finance.rs`, `text.rs`, `lookup.rs`, `logical.rs` | the five other families, empty (`BODIES = &[]`) |

Elsewhere in the crate: `math_util::{digits15, num_e, round15_exact}` and the integer path of `decimal_round` (A4a-7;
`collate::round15` takes it too); `value::{MAX_ARRAY_CELLS, array_too_big}` (A4a-4, also applied to the A2
operator broadcast and `apply_fn`'s lift); `FnContext::each_number` and `EvalHost::each_number` (default methods: no
implementor has to change) with the engine's lane fast path (`engine/host.rs`, A4a-8) and `store::ColumnRun::
uniform_kind`; `EvalState::set_bodies` (A4a-10); `load::SheetLoader::strings`; `Engine::new` builds its arena with
`Ast::new()` (A4a-11).

### The family contract

A family is a module `functions::<family>` (file or directory) exporting one item, a list per TS file:
`pub(super) const BODIES: &[&[(&str, FnBody)]] = &[LIST, …]` with `const LIST: &[(&str, FnBody)] = shared::bodies!
["NAME" => function, …]`, where `NAME` is the canonical registry name and `function` is `fn(&CallArgs<'_>, &mut dyn
FnContext) -> shared::R<Value>`. The metadata (modes, lift, volatile, min/max, prefix) is the generated `FN_TABLE` row:
a body never re-declares it. `functions/mod.rs` already lists the six families; a family never edits it nor `shared/`
(a helper a second family needs moves to `shared/` — owned by whoever needs it second, announced in its record). The
test `functions::tests::family_lists_are_consistent` fails on an unknown or duplicated name. Dispatch cost: one
`OnceLock` load + an index per call (the table is 535 × 8 B, built at the first evaluation; M1 tests warm it like the
static collation tables).

Cross-family dependencies the TS has, for the orchestrator: `lookup.ts` (A4e) → `regex.ts` `compilePcre` (A4d);
`engine.ts` → `database.ts` `setComputedCriterion` (A3-3, wired with A4e); `datetime.ts` → `daycount.ts` (both A4c).
Everything else the families share is in `shared/` now.

### Decisions taken in A4a (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4a-1 | **Dispatch** = static per-family lists folded into one table indexed by `FnIx` (built once, `OnceLock`); no per-call search or allocation. |
| A4a-2 | **Factories** are a spec + a closure (`NumSpec` + `num_fn`/`atp_fn`): the closure is monomorphized inside a plain `fn` body, so every body stays an `FnBody` pointer. The converted numbers live on the stack (`Nums`, 16 slots; the largest TS `maxArgs` of a factory is 8). `atpFn`'s positional quirk is kept: an empty optional argument with a default is appended after the present ones. |
| A4a-3 | **Bounded loops**: the Bessel recurrences stop after 2^22 steps with `#NUM!` (answered at once when the iteration count is known to pass it) — the TS never returns for an order of ∞, 1e21 or 2^53+2 (95 calls of the big trap run) and takes seconds per cell for huge finite orders (results for orders above ≈ 2·10^6 differ by design); BESSELY/BESSELK stop as soon as the value is not finite (∞/NaN are absorbing in the upward recurrence: same result); `binomial` stops when the product is not finite (every factor is ≥ 1 after the swap: same result) — `COMBINA(n; ∞)`, a TS hang, answers `#NUM!`. |
| A4a-4 | **Array caps**: a lift or a broadcast past 2^24 cells (the engine's `values` cap: `lift_scalars`, `lift_atp`, `apply_fn`'s lift, the evaluator's operators), MUNIT past it, and MMULT/MDETERM/MINVERSE past 2^30 multiply-adds throw a `RangeError` (`#NUM!` in the engine). The TS throws the same past 2^32 − 1 elements; between the caps it allocates gigabytes or computes for minutes. Found while preparing the fuzz run: `A1:A1048576+A1:XFD1` (1.7·10^10 cells) aborted the process in the A2 evaluator — now `#NUM!` in 8 ms. |
| A4a-5 | **JS-object edges**: a 0-row array reaching an ATP/complex body as a scalar, and a LAMBDA as SERIESSUM's coefficients, read `#VALUE!` (the TS converts an object). Reproduced as the TS: `Object.prototype` names as CONVERT units (`constructor`, `__proto__`, `kconstructor`… are truthy entries of category `undefined`: equal names → x, else `#NUM!`); full Unicode upper case in DECIMAL/HEX2DEC/ARABIC (`HEX2DEC("ﬀ")` = 255, `ARABIC("ıv")` = 4); `BASE(NaN;2)` = "undefined"; `COMPLEX(NaN;1)` = "0,N+i" (the string surgery on "NaN"); BITRSHIFT/BITLSHIFT with a NaN shift and MUNIT(NaN) throw the TS `RangeError`; IMPRODUCT of no value throws its `TypeError`. |
| A4a-6 | `parseComplex`'s three `RegExp`s are one deterministic scanner (every optional part is taken exactly when the regex takes it: giving one back always leaves a character the rest rejects — argued in `complex.rs`); a host decimal separator that is a `RegExp` metacharacter other than `.` is taken literally (the TS would build another pattern; only exotic C1 locales). |
| A4a-7 | **No allocation on the scalar paths**: ROUND/ROUNDUP/ROUNDDOWN/TRUNC and the 15-digit view (`approx15`: INT, FLOOR*, CEILING*, MROUND's neighbours, `compareNumbers`) use exact `u128` arithmetic for 1e-8 ≤ \|x\| < 2^128 and an integral \|d\| ≤ 22 (V8's precision digits: the exact expansion rounded half up; back through Clinger's fast path), the text path of the TS otherwise; `POWER(10;n)` formats on the stack. Checked equal bit for bit to the text path on 600,000 values (`math_util::tests::exact_paths_equal_text_paths`). |
| A4a-8 | **`each_number`** (`SUM`/`PRODUCT`/`SUMSQ` over a reference): a new default method of `FnContext`/`EvalHost`; the engine reads a one-column area from the store's lanes (uniform plain-number runs folded directly; formulas, owned and side-table cells through the engine's view, the `NeedCell` check kept after an error as the TS walk) instead of `for_each_cell` (whose `Store::each` allocates a cursor list and looks each cell up twice). Several columns keep the row-major walk. |
| A4a-9 | `tdl-util.ts` (four families) and the ten basic `Agg`s (`statistical.ts`, used by `database.ts`) live in `shared/`, not in A4d/A4b. |
| A4a-10 | **Harness**: `EvalState::set_bodies(false)` for the A2 replay (its capture stopped at the first body); the A3 replay defers a run's random cells (formulas with RAND* directly, through a shared anchor or a defined name) when a body that did not run in Rust may have shifted the random stream; the bank defers legacy formula cases with a range, table, name or spill reference (the TS rewrites them with `@`: A5). |
| A4a-11 | **A3 bug found by the bank**: the engine's arena did not reserve `StrIx(0)` for the empty string (`Engine::default()`), so a table-less `[b]` — a LAMBDA's optional parameter — read the first interned string as its table and gave `#VALUE!` (`core-fmt-714..716`). Fixed. |

### Parity

The replay (`tests/a4.rs`, `tests/support/fnmock.rs`) is the TS `testing.ts` in Rust: `mockContext` (cells, the LCG
random of the mock, `formulaAt`/`isSubtotalCell`, hidden/filtered rows, `parseRef`, JS LAMBDAs), `callFn` (`applyFn`
with the modes, thunks, omitted positions), `runCase` (plain path, post-processing of references and legacy arrays) and
`runFormulaCase` (the CalcWorkbook → store → `Engine.recalc`, the spill area when it spills). A case or call whose
evaluation reaches a body not ported yet is deferred: **every family that lands is compared by the same test, no
harness change**. NaN payloads are accepted (A1-2; none occurred). A TS call that never returned (`timeout:5000ms`) must
return in Rust within 2 s.

| Group | Compared | Equal | Of which the functions with a body | Deferred | Notes |
|---|---|---|---|---|---|
| bank CI (pt-BR, 35,299 cases) | 10,014 | **10,014** | 9,132 (131 functions) | 25,224 (other families) + 57 legacy `@` (A5) | the rest compared are operator/formula cases and argument errors `applyFn` answers before a body |
| bank big (pt-BR + en-US, local) | 20,028 | **20,028** | 18,264 | 50,448 + 114 A5 | |
| calc-traps CI (54,120 calls) | 8,969 | **8,969** | 8,969 (130 functions) | 45,141 | 17 calls with random values (RANDBETWEEN) compared value and consumption; 10 TS timeouts (BESSELJ/I/K, COMBINA) answered in ≤ 11 ms; a debug build skips the 219 whole-column calls (1M-cell fills), the release run compares them |
| calc-traps big (308,120 calls, pairs) | 46,489 | **46,489** | 46,489 | 261,538 | 95 TS timeouts answered (≤ 17.5 ms), 112 random records |
| calc-order CI x1 (A3 replay, unchanged harness but A4a-10) | values 69,396 (A3: 62,928) | **69,396** | — | 30,220 (A3: 36,688) | order equal 244 runs (A3: 199), random sequences equal in 274 runs (A3 deferred every run whose formulas consumed one); ops group unchanged (560 runs, 129,106 values) |
| calc-order big | values 8,290,438 (A3: 7,601,138) | **8,290,438** | — | 2,745,405 (A3: 3,434,705) | order equal 8,205 (6,990), random equal 8,484, `circular` 775 (756) |
| a2 / a1 / store / calc-tests groups | unchanged | 0 mismatches | | | a2 with bodies off (A4a-10) |

Deferred cells of calc-order that remain need another family (most x1 formulas mix IF, lookups, text, statistics) or A5.

### Robustness, memory, fuzz

`tests/a4_robust.rs`: every function with a body called with seeded random argument lists (the JS number traps, texts
shaped like complex numbers, Roman numerals, bases, units — `Object.prototype` names included —, lone surrogates,
booleans, errors, omitted arguments, arrays, references up to a whole column), 600 calls per function in release (60
in debug): no panic, slowest call 309 ms (FLOOR.MATH lifted over a whole column, as the TS); hostile shapes (A4a-4)
answer `#NUM!` in ≤ 175 ms debug. **M1**: each function called over a fresh mock leaves the live bytes exactly where
they were. **Zero allocation per evaluation**: a second full recalc of 2,000 math formulas (ROUND, MOD, POWER, SQRT,
INT, a 10-row SUM window, LN, SIN, FLOOR, CEILING.MATH, TRUNC, ATAN2) allocates exactly what one of 2,000 operator
formulas does (112 B: the scheduler's lists). Caches: none (the dispatch table is static data, 4,280 B).

Fuzz: `formula_parse` gained nine seeds over the family (lifts, ATP rules, complex texts, `Object.prototype` units,
hostile loops and shapes). Runs of 2026-11-24 (open A2 item: the stall of the first `formula_parse` run): **libFuzzer with a per-input timeout**
(`-timeout=5 -rss_limit_mb=4096`, ASan, 1,200 s each; built from a scratch crate over the same `core/fuzz/src/calc.rs`,
open item 4): `formula_parse` 1,168,185 executions, `calc_workbook` 55,125 (ASan-slow engine), **0 crashes, 0 timeouts,
0 OOM**; the stable fallback driver 600 s each: `formula_parse` 16,918,568 executions, `calc_workbook` 3,547,483, 0
crashes — but 11 `formula_parse` inputs took 2.7–5.0 s: hundreds of `BESSEL*(x; huge order)` calls, each running to the
step budget. Fixed (A4a-3): a loop whose iteration count is known to pass the budget answers at once; rerun 300 s:
15,649,534 executions, 0 crashes, slowest 418 ms (a lift over a whole column). The A2 stall did not reproduce in any of
these runs (5 s per input would have caught it); the candidate it most resembles — an unbounded loop in a body — cannot
happen before A4 (no body ran) and is bounded now.

### Performance (PERF)

`benches/a3.rs` vs `js/bench-a3.mjs` (the frozen TS engine on the same cells, Node 24.15; one process each, medians of 5
after a warm-up; i5-14600KF). New classes: **sum-ranges-100k** (the TS baseline's window `SUM(A{i-99}:A{i})`) and
**math-100k** (ROUND/MOD/POWER/SQRT+INT/ABS(SIN), 20,000 each).

| Class | formulas | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | Rust engine heap MB |
|---|---|---|---|---|---|---|---|
| **sum-ranges-100k** (`SUM` of a 100-row window) | 100,000 | 280 | 70.1 | 145.1 | **34.5** | **0.24** | 6.0 |
| **math-100k** (ROUND, MOD, POWER, SQRT+INT, ABS(SIN)) | 100,000 | 322 | 103.4 | 121.2 | **42.9** | **0.35** | 6.0 |
| chain-100k / sum3-100k / text-50k / dynamic-1k / volatile-10k / cycles / shared-1m (A3 classes) | — | — | — | — | 16.8 / 39.6 / 38.3 / 4.2 / 3.5 (calc. 4.2) / 1.3 / 163.8 | unchanged against the A3 record (all within ±3%) | — |
| edit: head of a 20k chain p50 / p95 | — | — | — | — | 4.9 / 5.7 ms | A3: 4.8 / 5.7 | — |

Against the record's TS baseline through the .xlsx reader (`sheet.mjs`, sum-ranges-100k recalc 199 ms) the window class is
0.17×. **Verdict**: both new classes inside the ≤ 0.5× native budget. The window `SUM` first measured 145 ms (1.0×
the TS: `for_each_cell` → `Store::each` allocating its cursor list and a second lookup per cell through the engine's
view); A4a-8 brought it to 34.5 ms. A2 benchmarks after A4a: parse + print 661 ns/formula (record 687; the input set
grew from 9,028 to 12,692 formulas with the generated workbooks), evaluate 361 ns (record 344, +5%: within the 10%
rule; the body lookup is one branch on `EvalState` per call). These are the first A4 records.

### Open items

(1) `RANDARRAY` is `dynamic.ts` (A4e). (2) The cross-family TS dependencies above. (3) A debug `cargo test` skips the
219 whole-column trap calls (compared by `--release` and the big run). (4) The libFuzzer build of `core/fuzz` does not
link on Windows (the `uo-ffi` cdylib with coverage; `-s none` also misses `__start___sancov_cntrs`): the calc targets
ran from a scratch crate with ASan (`-s address`) over the same `core/fuzz/src/calc.rs`. (5) `calc-tests` Engine calls
(A3 open item 2) still wait for the families they use.

## A4b — as built (2026-11-24, Opus)

The statistics family: every body of `statistical.ts` (56 names: AVERAGE … AGGREGATE), `distributions.ts` (70 names,
the legacy ones included), `regression.ts` (15: the pair functions, FORECAST/FORECAST.LINEAR, LINEST/LOGEST/TREND/
GROWTH with their array results) and `ets.ts` (FORECAST.ETS, .CONFINT, .SEASONALITY, .STAT) — 145 bodies, all in the
family module; nothing in `shared/`, `functions/mod.rs` or another family changed. The `*IF`/`*IFS` functions are
`conditional.ts` (A4e).

**Code** (`core/crates/uo-calc/src/functions/stats/`):

| File | Content |
|---|---|
| `mod.rs` | `statistical.ts` bodies; the family's support: `sn` (`num` of `stats.ts` as a value), `an`/`ai` (`argNumber`/`argInt` of `v ?? null`), `absent`, `PLAIN`/`A_OPTS`, `walk` (`collectInto` as a stream, with the lane fast path for plain references), `collect`, `pairs` (`numericPairs` with the lane fast path, A4b-3), `xadd` (`xlAdd` with the exponent read from the bits, A4b-7), the typed order (`typed_cmp`, `select`, `typed_min/max`), `median_of`, `kth`, the percentiles, `modes`, SUBTOTAL/AGGREGATE dispatch |
| `dist.rs` | `distributions.ts`: `nums` (stack array), `logical`, `opt_num`, `solve` (Brent, ≤ 500 steps), `quantile_pos` (≤ 3,000 doublings), the normal/lognormal, exponential/Weibull, gamma/χ², beta, t, F, binomial/negative binomial/hypergeometric/Poisson, CONFIDENCE, Z/T/F/CHISQ tests |
| `regress.rs` | `regression.ts`: `moments`, the pair functions, `numeric_grid`, `least_squares` (Householder with pivoting, as the TS), `design`, LINEST/LOGEST (1 × (k+1) or 5 × (k+1)), TREND/GROWTH |
| `ets.rs` | `ets.ts`: `build_series`, `detect_season`, `run`, `fit` (grid search), `prepare`, `forecast_at`/`value_at`, the four bodies |

Tests: `src/functions/stats/*` unit tests (selection = full typed sort, also with `-0`/`+0`/NaN; `floor_log2` =
`⌊Math.log2⌋` over every binade edge and 200,000 seeded doubles, `xadd` = A1's `xl_add`; the ETS and regression
budgets); `tests/a4b.rs` (zero allocation per evaluation on the scalar paths, the hostile ETS shapes); the family is
also in `tests/a4.rs` (parity) and `tests/a4_robust.rs` (random calls, M1) through `functions::ported()`. Benchmark
class `stats-10k` in `benches/a3.rs` and `js/bench-a3.mjs` (`UO_STATS_ONLY=<j>` times one function of the class).

### Decisions taken in A4b (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4b-1 | **Streamed aggregations**: AVERAGE/AVERAGEA (the `xlSum` = plain sum of all but the last, then `snapAdd`), MAX/MAXA/MIN/MINA, COUNT and RANK.EQ/RANK.AVG/RANK count while the arguments are walked, without the number list the TS builds; the order of reads and the first error are the TS's (RANK reads its order argument before the walk — a pure conversion — and answers its error after the walk's, as the TS). |
| A4b-2 | **Order statistics by selection** (`select_nth_unstable` with the `Float64Array.sort` order: numeric, `-0` before `+0`, NaN last): MEDIAN, LARGE/SMALL, PERCENTILE*/QUARTILE* (the element at `i − 1` and the minimum of the rest), AGGREGATE 12/14–19 and the SUBTOTAL median; PERCENTRANK finds `lo`, `s[lo]` and `s[lo − 1]` in one pass. The same element of the same total order as `sorted(xs)[i]` (unit test over `-0`/`+0`/NaN mixes), O(n) instead of O(n log n). **The per-recalc cache** (plan §16 item 12) was measured not to be needed: with selection MEDIAN/PERCENTILE cost 0.03× the TS on stats-10k, and the `FnContext` has no pass cache to put a sorted range in (adding one is an engine (A3) change, not a family one). TRIMMEAN and FREQUENCY keep a full sort (they need the order of every element). |
| A4b-3 | **`numericPairs` fast path** (`pairs`, every pair function, FORECAST, PROB, T.TEST type 1): two one-area references of the same size (≤ `BIG_AREA`) are read through `each_number` (the store's lanes); when both deliver one number per cell and no error, the pairs are the two sequences; otherwise `zipCells` as the TS (same pairs, same first error; the reads are x then y in both paths, so a `NeedCell` comes from the same walk). CORREL on stats-10k: 2.2 s → 0.57 s. |
| A4b-4 | **ETS grid search**: the 5,325 (seasonal) or 485 runs of the search keep only (α, β, γ, sse); the winner is run once more with its trace (errors, fitted values, final states) — `run` is deterministic, so the model is the TS's. The TS allocates the arrays of every run. |
| A4b-5 | **ETS bounds** (TS hangs or exhausts memory): a series whose length passes the JS array limit (`2^32 − 1`, the TS `new Array(n)` `RangeError`) throws `Throw::Range` before any allocation; a series with more than 30 % gaps answers `#NUM!` from the count of filled indices before the series is built (the TS allocates `n` slots first: 10^9 for a timeline `{1;2;3;1E9}`); gap filling reads the next known index from one backward pass (the TS rescans each gap run: quadratic); `detect_season` (`Σ (n − m)` multiply-adds), the grid search (`runs × n`) and CONFINT's horizon loop (`⌈h⌉ − 1` steps; a target date of 1e21 never returns in the TS — 2 calls of the CI traps, 26 of the big run) throw `Throw::Range` (`#NUM!` in the engine) past 2^31 steps, checked before each loop. Results below the budget are the TS's. |
| A4b-6 | **Least-squares bound**: a fit with `k² > 2^24` or `k·p·n + p³ > 2^32` multiply-adds (k columns, n rows, p = min(k, n)) throws `Throw::Range` before its `k × k` matrix exists (the TS allocates it and runs for minutes); the pattern of A4a-4. |
| A4b-7 | **`xadd`** = `xlAdd` of `stats.ts` with `⌊log2|x|⌋` from the exponent bits when the mantissa is below `2 − 2^−20` (fdlibm's log2 is then at least 6.8·10^−7 below the next integer, and exact at a power of two), `Math.log2` otherwise — equal to A1's `xl_add` for every input (argued in `floor_log2`, tested over every binade edge and 200,000 seeded doubles). Used by the family's own sums (moments, AVEDEV, HARMEAN, PROB); A1's `variance`/`devsq`/moments keep `xl_add`. |
| A4b-8 | **JS-object edges**: a LAMBDA as FREQUENCY's data or bins counts nothing (the TS puts the object in a 1 × 1 array and the collection ignores it); a LAMBDA as AGGREGATE's k reads `#VALUE!`; ETS `Map` keys read `-0` as `+0` (the TS `Map`), timeline keys sorted with the V8 sort replay (`js_sort_by`), duplicates aggregated in walk order; `Math.max(...xs)`/`Math.min(...xs)` of an ETS duplicate group never throw (the TS spread throws past ~120k duplicates of one date — accepted divergence, as `uo_jsnum::max_n`). |

### Parity

Replay of `tests/a4.rs`, unchanged harness (the family's cases turned from deferred into compared):

| Group | Compared (all families) | Equal | Of which A4b (145 functions, `UO_A4_FN`) | Deferred | Notes |
|---|---|---|---|---|---|
| bank CI (35,299 cases) | 15,870 (A4a: 10,014) | **15,870** | 5,965 compared, 5,965 equal, 1 deferred (a formula case that reaches another family) | 19,368 (A4a: 25,224) + 57 legacy `@` (A5) | |
| bank big (pt-BR + en-US) | 31,740 (20,028) | **31,740** | — | 38,736 (50,448) + 114 A5 | |
| calc-traps CI (54,120 calls) | 26,925 (8,969) | **26,925** | 17,956 compared, 17,956 equal, 0 deferred | 27,183 (45,141) | 2 TS timeouts of the family (FORECAST.ETS.CONFINT, target 1e21 / 2^53+2) answered in 0.3 ms (A4b-5) |
| calc-traps big (308,120 calls) | 152,659 (46,489) | 152,657 | — | 155,342 (261,538) | 121 TS timeouts answered (slowest 1.9 ms); **2 platform differences**, below |
| calc-order CI x1 (A3 replay) | values 72,038 (A4a: 69,396) | **72,038** | — | 27,578 (30,220) | order equal 384 runs (244), random equal 274; ops group unchanged (560 runs, 129,106 values) |
| calc-order big | values 8,595,287 (8,290,438) | **8,595,287** | — | 2,440,556 (2,745,405) | order equal 10,719 (8,205), random equal 8,484, `circular` 800/800, incremental = full 577 |

**The 2 differences of the big trap run are the platform of the capture, not the port**: `WEIBULL.DIST(0.5; 2.5;
2^53 − 1; TRUE)` (pt-BR and en-US) = `(0.5/(2^53 − 1))^2.5`, and `Math.pow` there is 1 ulp apart between the Windows
UCRT `pow` of the capturing Node (`…02`) and the reference the port follows (decision G1 (a) of `uo-jsnum.md`: glibc's
non-FMA `pow`, `…03`; checked in WSL with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`: `…03`). The exact value is
`2^−135·(1 + 2^−52)^2.5 = 2^−135·(1 + 2.5ε + 1.875ε² + …)`, so `…03` is the correctly rounded one. `a4_big` asserts
no mismatch and therefore fails on these 2 records until the harness (A4a's) learns platform records (open item 1).

### Robustness, memory, fuzz

`a4_robust.rs` runs the 145 new bodies through its random calls (600 per function release, 60 debug) and M1 (every
function over a fresh mock leaves the live bytes exactly where they were): no panic, slowest call 396 ms (LOGNORM.DIST lifted over a whole column, the lift's 1M-cell fill, as A4a's FLOOR.MATH), M1 green. **Zero allocation per evaluation**
(`tests/a4b.rs`): a second full recalc of 2,000 statistics formulas (NORM.DIST, T.DIST, GAMMA.DIST, BINOM.DIST,
CHISQ.DIST.RT, F.DIST, POISSON.DIST, the Brent inverses NORM.S.INV/T.INV/GAMMA.INV/F.INV.RT, BETA.DIST, a 10-row
AVERAGE/MAX/RANK.EQ window, STANDARDIZE+FISHER, PERMUT+HYPGEOM.DIST, CONFIDENCE.T+WEIBULL.DIST) allocates exactly what
2,000 operator formulas do. The bodies that build a list (STDEV/VAR/MEDIAN/PERCENTILE…, the pairs) allocate it per
call, as the TS; no cache was added (nothing to cap). Hostile ETS shapes (`tests/a4b.rs`, A4b-5) answer `#NUM!` in
under a millisecond.

Fuzz: six `formula_parse` seeds over the family (order statistics and percentile ranks, AGGREGATE/SUBTOTAL/MODE.MULT/
FREQUENCY, the iterative inverses at their extremes, the discrete loops at their caps, the ETS hostile shapes, LINEST/
TREND/LOGEST/GROWTH and the tests over whole columns). Seeds in `core/fuzz/src/calc.rs` `formula_seeds`. Runs of 2026-11-24:
libFuzzer with ASan from the scratch crate (A4a open item 4; `-timeout=5 -rss_limit_mb=4096`): 261,002 executions in
≈ 6 min, **0 crashes, 0 timeouts, 0 OOM** (stopped early: disk D: near full, the scratch target dir was removed);
the stable fallback driver (`core/fuzz`, `--slow 1`) 2 × 300 s: 11,494,550 + 10,939,469 executions, **0 crashes**,
slowest 1.50 / 1.57 s. Every input over 1 s holds `BETA.INV(0.5;1E-300;1E300)` (1.05 s alone): the TS's own bound —
Brent's 500 steps × the incomplete beta's 100,000 continued-fraction steps (A1 `betacf`) — not a hang, and the answer
is the TS's (R1: bounded loops return what the TS returns); kept, open item 5.

### Performance (PERF)

`benches/a3.rs` vs `js/bench-a3.mjs` (frozen TS engine, Node 24.15, same cells; medians of 5 after a warm-up;
i5-14600KF). New class **stats-10k**: 10,000 data rows (A, B) and 10,000 formulas each of AVERAGE, STDEV.S, MEDIAN,
PERCENTILE(·;0.9), RANK.EQ, CORREL and SLOPE over windows `$A$1:$A$w`, w = 1,000–10,000 (70,000 formulas, ≈ 5.5·10^8
cells read).

| Class / function (10,000 formulas each) | TS recalc ms | **Rust recalc ms** | Rust / TS |
|---|---|---|---|
| **stats-10k** (all 70,000; build TS 295 / Rust 83 ms; engine heap TS 86.6 MB retained / Rust 31.4 MB) | 38,155 | **1,960** | **0.051** |
| AVERAGE (streamed, lanes) | 1,902 | 87.9 | 0.046 |
| STDEV.S | 2,031 | 208.4 | 0.103 |
| MEDIAN (selection) | 6,017 | 203.8 | 0.034 |
| PERCENTILE (selection) | 5,830 | 189.7 | 0.033 |
| RANK.EQ (streamed) | 1,867 | 124.5 | 0.067 |
| CORREL (`pairs` + `xadd`) | 5,105 | 568.7 | 0.111 |
| SLOPE | 4,964 | 566.7 | 0.114 |

Per function: `UO_STATS_ONLY=<j>` in both runners (one Node process per function). Before A4b-3/A4b-7 the class took
4,949 ms (CORREL 2,204, SLOPE 2,367: `zipCells` through `values()` and two `Math.log2` per mixed-sign term) — already
0.13× the TS. The other classes are unchanged against the A4a record (sum-ranges-100k 34.2, math-100k 45.3,
chain-100k 16.9, sum3-100k 39.8, text-50k 36.9, dynamic-1k 4.0, volatile-10k 3.5, cycles 1.3, shared-1m 163–173 ms;
edit head of a 20k chain p50 4.7 ms) — no regression. **Verdict**: inside the ≤ 0.5× budget by an order of magnitude;
no per-recalc cache (A4b-2).

### Open items

(1) `a4_big` fails on the 2 platform records above (Windows UCRT `pow` in the capture vs the glibc reference): the
harness (A4a) needs a rule for them — either the captures are retaken under the reference (Node in WSL with the
tunable, as `uo-jsnum.md` prescribes for G1 (a)) or `tests/a4.rs` accepts a listed platform difference; decision for
the orchestrator. (2) Shared-helper candidates for `shared/` (none needed by another family yet): `walk` (the
`collectInto` stream with the `each_number` fast path for plain references — `database.ts` (A4e) would gain the same
speed), `pairs` (A4b-3), `xadd`/`floor_log2` (A4b-7: A1's `xl_add` could take it, every SUM-like sum with mixed signs
calls `Math.log2` twice per term), `select`/`typed_min`/`typed_max` (any `sorted(xs)[i]`). (3) A per-recalc cache of
sorted ranges would need a pass cache in `FnContext` (engine, A3 owner) — not needed at the measured speeds (A4b-2).
(4) COUNT and the `*A` versions walk references through `for_each_cell` (the engine's `Store::each`): the lane path
only knows "numbers until the first error"; a lane walk with the other value kinds would be an engine change. (5) BETA.INV/BETAINV
(and the other Brent inverses over the incomplete beta: T.INV, F.INV) cost up to 500 × 100,000 continued-fraction steps
on extreme shapes (≈ 1 s per call, `BETA.INV(0.5;1E-300;1E300)`), as the TS; a faster inverse would change results in
the last bits — a simplification-stage question with oracle evidence, not a port one.

## A4c — as built (2026-11-24, Opus)

The finance/date family: every body of `financial.ts` (55 names: PV … ODDFYIELD) and `datetime.ts` (25 names: DATE …
YEARFRAC, NOW/TODAY through the host clock), and `daycount.ts` (the ATP day counts and coupon dates, no bodies of its
own) — 80 bodies, all in the family module; nothing in `functions/mod.rs`, `shared/` or another family changed.

**Code** (`core/crates/uo-calc/src/functions/finance/`):

| File | Content |
|---|---|
| `mod.rs` | `BODIES` (`financial`, `datetime`); the family's support: `Steps` (a step counter with a cap), `LOOP_BUDGET` (2^22) and `PRICE_BUDGET` (2^23, A4c-2), `F64Buf` (cash flows on the stack up to 64), `type_error()` (the TS `TypeError` of a JS object reaching `toNumber`), `lift_atp` (`liftAtp` of `tdl-util.ts` with the arguments on the stack, A4c-1) |
| `daycount.rs` (`pub`) | `daycount.ts`: `CDate` (LibreOffice's `ScaDate`: `of`, `set_day`, `add_months`, `serial`, `lt`/`gt`, `diff`), `couppcd_date`/`coupncd_date` (bounded, A4c-2), `Bond`, `coup_*`, `edate`, `days360eu`, `diff_date`, `year_frac_parts`, `days_in_year`, `year_frac` — every field an `f64` as in the TS (a NaN basis takes the `default` branches; a date outside the JS `Date` range has NaN fields) |
| `financial.rs` | `financial.ts`: time value of money (`fv_of`, `pv_of`, `pmt_of`, `ipmt_of`, `rate_of`), cash flows (NPV streamed, `array_flows`, `irr_of`, `atp_array`, `x_args`, `xirr_of`, FVSCHEDULE), depreciation (DB, `ddb_of`, `inter_vdb`, AMORDEGRC/AMORLINC), bonds (`bond_of`, `Coupons`, `price_of`, `yield_of`, `duration_of`, the discount family, `accr_args`/`accr_nums`, `quasi_at`, `day_count`, `oddl_sums`, `OddF`/`oddf_setup`/`oddf_price`) |
| `datetime.rs` | `datetime.ts`: `split`, `date_arg`, `atp_date`, weekend masks (`[bool; 7]`), `Holidays` (the TS `Set` as a sorted, deduplicated vector + a NaN flag), `network_days`, `workday` (bounded), `iso_week`, `datedif` (+ `Unit`), `days360`, `add_months`, the 25 bodies |

Tests: `src/functions/finance/datetime.rs` unit test (DATEDIF units); `tests/a4c.rs` — zero allocation per evaluation
on 50 single-call finance/date formulas (every value checked to be a number but non-converging IRR windows),
the hostile loops (A4c-2/-3) answered at once, NOW/TODAY over the frozen clock; the family is also in `tests/a4.rs`
(parity) and `tests/a4_robust.rs` (random calls, M1) through `functions::ported()`. Benchmarks `benches/a4c.rs` +
`js/bench-a4c.mjs` (own files: `[[bench]] a4c` is the only `Cargo.toml` change).

### Decisions taken in A4c (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4c-1 | **No per-call allocation on the scalar paths**: the date ATP bodies (EDATE, EOMONTH, NETWORKDAYS(.INTL), WORKDAY(.INTL), WEEKNUM, YEARFRAC) go through the family's `lift_atp`, the shared `tdl::lift_atp` with its `Vec` of arguments replaced by a 4-slot stack array (same rules: a several-cell reference → `#VALUE!`, LAMBDA → `#CALC!`, the passed position unlifted, broadcast with `#N/A` leftovers); NPV sums the flows as they are visited (the TS collects them first: same terms, same order); IRR/MIRR collect into `F64Buf` (stack up to 64 flows); FVSCHEDULE validates the rates in order and then multiplies (no list); holidays allocate only when given. Values unchanged: YIELD computes E/N/A (`coupDays`/`coupNum`/`coupDayBs`) once per call and ODDFYIELD the yield-independent part of `oddfPrice` (the quasi-coupon sums, the exponent Nq + DSC/NL, the coupon count) once — the TS recomputes the same values at each price. |
| A4c-2 | **Bounded loops** (decision R1; the TS hangs or takes seconds): (a) the coupon walks (`couppcdDate`/`coupncdDate`) stop after `COUP_CAP` = 1,000 steps with `#NUM!` — a walk over finite fields takes ≤ 9 steps; the TS `coupncdDate` never ends once a field is NaN (a NaN settlement, a maturity past the JS `Date` range): COUPNCD/COUPDAYSNC, 10 TS timeouts in the CI traps; (b) ODDFPRICE/ODDFYIELD answer `#NUM!` at once for an issue before every finite serial (< −101,000,000: the JS `Date` range starts at serial ≈ −99,974,431) and when a quasi-coupon of the irregular first period leaves the `Date` range (NaN) before reaching the issue — in both cases the TS walks back forever (1 CI timeout: issue −∞); the quasi-coupon walks of ACCRINT, ODDF* and ODDL* carry a cap of 2^23 steps that the `Date` range keeps them under (≤ ≈ 2.4M periods), and each quasi-coupon date is computed once (the TS computes it up to three times: a pure function of the period); (c) `LOOP_BUDGET` = 2^22 periods per body for CUMIPMT/CUMPRINC and VDB (both branches; a period count known to pass it fails before the loop), DB and AMORDEGRC — past it `#NUM!` (the TS takes seconds past a few million periods and never returns for a life/period of 1e21); (d) `PRICE_BUDGET` = 2^23 powers computed by YIELD's secant and ODDFYIELD's Newton — never reached by dates up to 31/12/9999 (≤ 32,400 coupons × 202 prices); a maturity near the `Date` limit answers `#NUM!` after ≈ 330 ms release (the TS returns `#NUM!` too on the captured cases, after its 100 iterations); (e) WORKDAY's day-by-day walk stops after 2^23 steps with `#NUM!`: a finite start leaves the date range within ≈ 3M steps (the TS answers `#NUM!` there too); only a NaN start (a trap value) runs past it, where the TS counts `days` down (1e300: forever). |
| A4c-3 | **Fixed points instead of iterations** (the TS result exactly, where the TS loop would run 1e15+ steps): DB stops when the book value repeats bit for bit (0 rate, a subnormal, ±∞, NaN — reached within ≈ 1.5M steps from any start); AMORDEGRC stops past the life (every later period is 0 and changes nothing; `rest` was already checked) and, when a period changes neither the book value nor the rest, jumps to the second-to-last period of the life (or ends: a NaN/infinite life, or past the last period). |
| A4c-4 | **JS-object edges**: an array or a LAMBDA reaching `toNumber` throws the TS `TypeError` (`Throw::Internal`: `#VALUE!` + `internalErrors` at the engine) — XNPV/XIRR/FVSCHEDULE given an array as rate, guess or principal (measured in the traps: `XIRR#p2/a22` = `throw:TypeError`), a LAMBDA as XNPV/XIRR values/dates or as holidays. The holidays' `xs.push(...data)` throws `RangeError` (`Throw::Range`) for an area of more than 120,000 values (the bound of `uo_dml_render::metafile::SPREAD_LIMIT`; V8's limit is stack-dependent — declared divergence between 120,000 and V8's exact value), measured: whole-column holidays = `throw:RangeError` in the big trap run (17 records). |
| A4c-5 | **NOW/TODAY** = `ctx.now()` / its floor (the host clock, decision C1; volatility is the registry's, A3). No bank case or trap call has them (zero arguments): `tests/a4c.rs` `clock` checks them over the frozen capture clock (45658.5). |
| A4c-6 | DATEDIF's `unit.toUpperCase()` is compared ASCII-case-insensitively: no non-ASCII code unit upper-cases to `D`, `M` or `Y`, and the full-case expansions that start with one (`ẙ` → `Y` + U+030A) are two units long, so no unit text other than the ASCII ones matches. |
| A4c-7 | **Harness**: `tests/a4_robust.rs` `random_calls` bounds a call at 4 s in a debug build (1.5 s release, unchanged): with the 80 new bodies the seeded sequence draws PMT/RATE lifted over a whole column × a 1 × 3 array (3M elements: 2.0 s debug, the lift's own cost — ABS over one column costs 0.26 s debug; release 0.39 s for the slowest call of the run, LOGNORM.DIST). |

### Parity

Replay of `tests/a4.rs`, unchanged harness (the family's cases turned from deferred into compared). All families
(debug CI) and the family alone (`UO_A4_FN=<the 80 names>`, release, so the whole-column traps are compared):

| Group | Compared (all families) | Equal | Of which A4c (80 functions) | Deferred | Notes |
|---|---|---|---|---|---|
| bank CI (35,299 cases) | 24,246 (A4b: 15,870) | **24,246** | 8,403 compared, 8,403 equal, 0 deferred | 10,992 (19,368) + 57 legacy `@` (A5) | NOW/TODAY have no case (A4c-5) |
| bank big (pt-BR + en-US) | — | — | 16,806 compared, **16,806 equal** | 0 | |
| calc-traps CI (54,120 calls) | 36,137 (26,925) | **36,137** | 10,116 compared (release), 10,116 equal, 0 deferred | 17,056 (27,183) | 11 TS timeouts of the family (10 COUPNCD/COUPDAYSNC, 1 ODDFPRICE) answered in < 0.1 ms (A4c-2) |
| calc-traps big (308,120 calls) | — | — | 61,849 compared, **61,848 equal + 1 NaN payload** (A1-2) | 0 | 67 TS timeouts answered (< 0.1 ms each); the slowest compared calls: ODDFYIELD with a maturity past the `Date` range (1.1M coupons: 360–400 ms, as many prices as the TS computes before its `#NUM!`) |
| calc-order CI x1 (A3 replay) | values 73,775 (A4b: 72,038) | **73,775** | — | 25,841 (27,578) | order equal 384 runs (384), random equal 274 (274), incremental = full 19 (11 deferred); ops group unchanged (560 runs, 129,106 values) |
| calc-order big | values 8,741,784 (8,595,287) | **8,741,784** | — | 2,294,059 (2,440,556) | order equal 10,719, random equal 8,484, `circular` 800/800, incremental = full 577 — all unchanged |

0 parity rounds: the first replay had 4 mismatches, all one harness-visible defect of the port (an Excel error from
the holidays propagated out of the whole lift instead of becoming the element's value, `NETWORKDAYS` over an array),
and the first release run 17 more (the holidays' spread `RangeError`, A4c-4) — both fixed by the same agent before
any verifier; the CI bank was equal from the first run. The full `a4_big` (every family) still fails on A4b's 2
platform records (A4b open item 1), not on this family.

### Robustness, memory, fuzz

`a4_robust.rs` runs the 80 bodies through its random calls (600 per function release, 60 debug) and M1: no panic, M1
green; release slowest call of the run 390 ms (LOGNORM.DIST, A4b's), debug A4c-7. **Zero allocation per evaluation**
(`tests/a4c.rs`): a second full recalc of 2,000 formulas cycling 50 single calls (PMT, FV, PV, RATE, NPV/IRR/MIRR over
20-value windows, IPMT, PPMT, NPER, DB, DDB, SLN, VDB, DATE, EDATE, EOMONTH, NETWORKDAYS(.INTL), YEARFRAC 1/0, WEEKDAY,
WEEKNUM, ISOWEEKNUM, YEAR, DAY, MINUTE, DATEDIF, DAYS360, DAYS, WORKDAY(.INTL), COUPDAYS, COUPNCD, COUPNUM, COUPDAYSNC,
PRICE, YIELD, DURATION, DISC, ACCRINT, TBILLEQ, TIME, TODAY, NOW, EFFECT, CUMIPMT, AMORDEGRC, ODDLPRICE, ODDFYIELD)
allocates exactly what 2,000 operator formulas do (112 B). (A long operator chain alone makes the evaluator's stacks
grow once per recalc — 160 B for `A1+A1*2+…+A1*6`, any family — so the test keeps one call per formula.) Hostile loops
(debug): COUPNCD(·;1E21) and COUPDAYSNC(·;1E300) `#NUM!` in ≤ 1.3 ms, ODDFPRICE with issue −1E300 0.2 ms (270 ms before
the serial bound of A4c-2 (b)), CUMIPMT(·;1E21 periods) 0.1 ms, VDB life 1E21 0.3 ms (388 ms before the count check), DB life 1E300 and AMORDEGRC 1E15
periods 0 (the TS result) in 0.2 ms, WORKDAY 1E300 days over holidays 106 ms. Caches: none (the family keeps nothing
between calls).

Fuzz: five `formula_parse` seeds over the family (solvers at their extremes, the unbounded TS loops at their budgets,
coupon walks over NaN fields, the ATP lifts with holidays and whole columns, the date edges). Seeds in `core/fuzz/src/calc.rs` `formula_seeds`. Stable fallback driver
(`core/fuzz`, `--slow 1`, `--seed 4242`), 2026-11-24: run 1 — 441,158 executions in 300 s, **0 crashes**, 116
inputs over 1 s (slowest 3.17 s): ODDF* walking back to the `Date` limit for an issue of −1E300 (270 ms per call) and
VDB running its whole budget (390 ms) → the serial bound and the count check of A4c-2 (b)/(c), and the quasi-coupon
dates computed once; run 2 — 643,011 executions in 300 s, **0 crashes**, 130 inputs over 1 s (slowest 3.38 s; the
driver exits 1 on slow inputs): every one a mutation of the A4c seed that holds several quasi-coupon walks to the
`Date` limit (ODDLPRICE/ACCRINT with a date of 1E8: 125–150 ms per call and context, ≈ 1.1M periods; **the TS takes
1.0–1.7 s for each and the values are equal**, pinned in `tests/a4c.rs`) or of A4b's BETA.INV seed (A4b open item
5). The executions per second are low (2,137/s) because the family's seeds are heavy, not because of a stall. The
fuzz crate was built in its own existing target directory (`core/fuzz/target`, incremental) rather than in
`target/sheet-arch`, which would have rebuilt every crate of the fuzz workspace in release (disk).

### Performance (PERF)

`benches/a4c.rs` vs `js/bench-a4c.mjs` (frozen TS engine, Node 24.15, same cells; medians of 5 after a warm-up;
i5-14600KF; both runners print the count of numbers/errors and the bits of their sum in row order — equal:
`c16d134e5f16dff8` / `41cb51b34158ce3e`). **finance-50k**: 50,000 formulas over a column of flows (one in 7 negative)
and of rates — PMT, FV, PV, RATE, NPV and IRR over 20-value windows, MIRR, IPMT+PPMT, DB+DDB, a 2-coupons-a-year PRICE;
**dates-50k**: 50,000 formulas over dates 2020–2026 — DATE, EDATE, EOMONTH, NETWORKDAYS (≤ 400 days), YEARFRAC basis 1
and 0, WEEKDAY+WEEKNUM, DATEDIF "YD", WORKDAY, DAYS360+YEAR+MONTH+DAY.

| Class | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | engine heap (TS retained / Rust) |
|---|---|---|---|---|---|---|
| **finance-50k** | 281–295 | 94 | 216 | **82** | **0.38** | 94.8 MB / 12.8 MB |
| **dates-50k** | 275–286 | 105 | 74 | **29.6** | **0.39** | (GC noise) / 33.9 MB |

Per kind (`UO_A4C_ONLY=<j>`: 50,000 formulas of one kind; ms TS → Rust): PMT 31.4 → 23.8 (0.76), FV 32.6 → 17.8,
PV 33.0 → 15.0, RATE 78.8 → 39.9 (0.51), NPV 53.9 → 20.6 (0.38), IRR 113.1 → 58.8 (0.52), MIRR 49.6 → 23.7,
IPMT+PPMT 57.5 → 36.8 (0.64), DB+DDB 136.3 → 68.7, PRICE 1,268 → 496 (0.39: ≈ 30 `excelPower` per call, A1's
double-double `log`/x87 `exp`); DATE 57.0 → 33.1 (0.58), EDATE 48.0 → 22.9, EOMONTH 47.3 → 22.8, NETWORKDAYS 44.5 →
24.9, YEARFRAC/1 52.5 → 26.4, YEARFRAC/0 53.4 → 24.7, WEEKDAY+WEEKNUM 45.3 → 26.0, DATEDIF 79.0 → 30.2 (0.38), WORKDAY
40.9 → 21.8, DAYS360+YEAR/MONTH/DAY 110.2 → 56.6. The cheapest kinds are dominated by the engine's per-formula cost
(≈ 300 ns Rust for a one-call formula: FV, PV); PMT's three fdlibm calls (`log1p`, `exp`, `expm1`) are the dearest of
the scalar bodies. **Verdict**: both classes inside the ≤ 0.5× budget; these are the first A4c records (the 10% rule
starts from them). The A3/A4a/A4b classes are unchanged (no shared code changed).

### Open items

(1) Shared-helper candidates (none needed by another family yet): `lift_atp` (stack arguments — `tdl::lift_atp`
allocates a `Vec` per call; the text, lookup and information ATP bodies of A4d–A4f would get the same zero-allocation
path), `Steps` (a step budget), `F64Buf` (a small vector of numbers), `type_error()`. (2) `a4_robust`'s debug bound
(A4c-7) is a harness choice for the orchestrator: the alternative is skipping whole-column lifts in debug, as `a4.rs`
does. (3) YIELD/ODDFYIELD at their budgets cost up to ≈ 0.3 s per cell (release) for dates near the `Date` limit
(beyond Excel's 9999); a cheaper rule (a budget in coupons, checked first) would answer `#NUM!` earlier for inputs the
TS answers after its 100 iterations — a simplification-stage question. (4) The `daycount` module is `pub`: the grid
or the server may want `year_frac`/`CDate` (none does today).

## A4d — as built (2026-11-24, Opus)

The text family: every body of `text.ts` (48 names: CONCATENATE … DETECTLANGUAGE), `regex.ts` (REGEXTEST,
REGEXEXTRACT, REGEXREPLACE), `filterxml.ts` (FILTERXML) and `regional.ts` (10 names: THAI*, NUMBERSTRING, DATESTRING),
with `text-format.ts` (TEXT's code translation, FIXED/DOLLAR digits; no bodies of its own) mapped onto `uo-numfmt` —
62 bodies in the family module. Outside it: `criteria::pt_error_name` became `pub(crate)` (VALUETOTEXT/ARRAYTOTEXT's
`errorText`), `tests/a4_robust.rs`'s M1 calls `functions::text::clear_caches()` after each call (the family keeps
caches), `uo-calc`'s `Cargo.toml` gained `uo-numfmt`, the vendored `regress` (A4d-1) and `[[bench]] a4d`; the fuzz crate
gained six seeds. Nothing in `functions/mod.rs`, `shared/` or another family changed.

**Code** (`core/crates/uo-calc/src/functions/text/`):

| File | Content |
|---|---|
| `mod.rs` | `text.ts`: `BODIES`, `clear_caches()`, the units (code points, UTF-16 units of the …B functions in pt-BR/en-US, DBCS bytes in ja/zh/ko: `Unit`, `len_in`, `slice_in`, `byte_range`), case (`upper_into` without expansion, `lower_of` with `İ` → `I`, `proper_into`), the SEARCH wildcards (`wild_segment`/`wild_at`/`wild_search`, A4d-9), the language tables (`errorText`, list and array separators of `getLanguage`), `value_text`, TEXTBEFORE/AFTER (`scan_forward`, `nth_index`, `not_found`), TEXTSPLIT, NUMBERVALUE, FIXED/DOLLAR, BAHTTEXT, the rest; `SPREAD_MAX` (A4d-6) and `scalars_js` |
| `js.rs` | the `String.prototype` primitives over UTF-16 with the JS argument rules (`rel`/`clamp`/`slice`/`index_of`/`last_index_of`, `chars` = `Array.from`, `cp_count`) and `Buf`, an output text on the stack up to 160 units |
| `format.rs` | `text-format.ts`: `format_locale_of` (cache per locale id, cap 8), `split_local`, `translate_format_code`/`translate_section`/`translate_bracket` (the TS regexes as unit matchers with non-`u` semantics), `format_for_text` (`uo_numfmt::format_value_parts` + `format_general`), `fixed_digits` (`uo_numfmt::decimal_digits`), `MAX_TEXT` |
| `regex.rs` | `regex.ts`: `translate` (PCRE2 → the JS source, unit for unit the TS output), `compile_pcre` (`pub(crate)`: XLOOKUP/XMATCH mode 3 of A4e call it, with `start_budget`/`check_budget`), the cache (A4d-2), `Pcre::{exec, is_match, all}`, `parse_replacement`, the three bodies; `unicode_version` test (R1) |
| `xml.rs` | `filterxml.ts`: `parse_xml` into an arena in document order (each element knows its descendant range), `decode`, `tokenize`, the XPath `Parser` into an expression tree (`Ev`), `Doc::{eval, eval_path, compare}` with the work budget (A4d-5) |
| `regional.rs` | `regional.ts`: `lang_is` (`/^xx\b/i`), `million_groups` (bounded, A4d-3), `thai_below`, `thai_words`, `cjk_number`, the ten bodies |

Vendored: `core/vendor/regress` — `regress` 0.12.0 (MIT OR Apache-2.0, licence texts beside it, `src/` only) with
the step budget of A4d-1 (`src/budget.rs` + two marked hunks in `src/classicalbacktrack.rs`, "uo-calc budget"), built
with `prohibit-unsafe`. Tests: unit tests in every file (translation of the measured format codes and PCRE patterns,
the wildcard search, JS slices, Thai/CJK numbers, XPath paths and predicates, the catastrophic-regex budget, the
Unicode-version check); `tests/a4d.rs` — zero allocation per evaluation on the number/substring paths, the hostile
inputs, the regex cache cap (M5); the family is also in `tests/a4.rs` and `tests/a4_robust.rs` through
`functions::ported()`. Benchmarks `benches/a4d.rs` + `js/bench-a4d.mjs`.

### Decisions taken in A4d (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4d-1 | **Regex step budget through a vendored `regress`** (decision R1). Upstream `regress` 0.12 has no way to bound backtracking (measured: `(a+)+b` over 24 a's 0.5 s, `(a\|a)*b` 1.2 s, each further a doubles it; `.*.*.*=` over 2,000 units did not end in minutes) and keeps its executor private, so the budget cannot be added from outside. `core/vendor/regress` adds a per-thread budget: every instruction dispatched (backtracks re-enter the dispatch) costs one step; a search takes the remaining budget and gives back what it did not use, so a budget spans every search of one call; past it the search reports no more matches and `budget_exhausted()` turns true. The bodies give each call `STEP_BUDGET` = 2^24 steps; exhausted → the `RangeError` path (`Throw::Range`, `#NUM!` in the engine) — **divergence by design**: the TS never returns there (no capture has such a call). Release: `(a+)+b` over 40 a's 46 ms, `.*.*.*=` over 30,000 a's 47 ms. The path dependency is a **workspace member by Cargo's rule** (path dependencies inside the workspace directory); its `Cargo.toml` turns clippy's lints off (third-party style). **For the maintainer** (with the M3 gate): accept a vendored third-party crate in `core/vendor/` (alternatives: upstream a budget API to `regress` and return to crates.io; or move the copy outside the workspace directory), and `tools/licenses/gen-notices.mjs` must list it (a `publish = false` path crate may be skipped as private — not run here). |
| A4d-2 | **Compiled-pattern cache**: per thread, 256 patterns (the TS keeps 500 and clears all past it; here the same clear-all at 256), patterns longer than 1,024 units compiled per call and never cached; the lookup copies the key on the stack (no allocation on a hit). `clear_caches()` empties it with the TEXT locale cache (8 locales) and `uo_numfmt`'s parse cache. REGEXEXTRACT mode 1 and REGEXREPLACE scan with one executor (after an empty match it moves one code point, after a non-empty one it starts at its end: the TS `lastIndex` loop; equal on every capture). |
| A4d-3 | **Bounded TS hangs**: (a) the TS translator loops for ever on `\p{` without `}` (`i = end + 1` with `end = -1`) until the growing source throws and its `catch` gives `#VALUE!`: answered `#VALUE!` at once; (b) BAHTTEXT of a value whose integer part rounds to ∞ in `toPrecision(15)` (≈ 1.79769313486232e308, the 2 BAHTTEXT timeouts of the big trap run) and THAINUMSOUND/THAINUMSTRING of ∞ (`while (x > 0)` with `∞ % 1e6` = NaN): `Throw::Range`; (c) TEXT with an empty `generalName` (a C1 host locale: `i += -1` for ever): the name never matches. |
| A4d-4 | **Exotic C1 host locales** (not pt-BR/en-US; declared): format letters of more than one unit are matched as literals in `[hh]`-style brackets (the TS interpolates them into a `RegExp` unescaped); a group separator holding `$` is inserted literally by FIXED/DOLLAR (the TS `String.replace` would read `$&`, `$1`…); a locale string with a lone surrogate reaches `uo_numfmt`'s `FormatLocale` (`String`) as U+FFFD. |
| A4d-5 | **FILTERXML**: the elements in one arena in document order (descendants = a contiguous range; no recursion); node sets deduplicated as they are built (the TS pushes every candidate — up to n² for `//*//*` — then makes a `Set`: same items, same order); attribute items carry a serial (the TS makes a new object per read, so a union never merges two attribute items, measured equal); the descendant walks and predicate evaluations of a call share a budget of 2^25 steps → `Throw::Range` (5,000 nested elements with 40 `//*` steps: 88 ms release; the TS is quadratic per step and takes minutes); a `&#…;` past U+10FFFF throws the TS `RangeError` (`String.fromCodePoint`, outside its `try`). |
| A4d-6 | **JS call spreads**: `scalarsOf`'s `out.push(...ctx.values(v, k).data)` (TEXTJOIN/TEXTBEFORE/TEXTAFTER/TEXTSPLIT delimiters, TEXTJOIN items), `Math.min(...)` of the delimiter lengths and `Math.max(...)` of TEXTSPLIT's rows throw V8's `RangeError` past `SPREAD_MAX` = 120,000 elements (A4c-4's bound; V8's limit is stack-dependent: declared divergence between 120,000 and V8's exact value); measured equal on the whole-column traps. |
| A4d-7 | **JS-object edges**: a LAMBDA as TEXTBEFORE/AFTER's `if_not_found` or TEXTSPLIT's pad reads `#VALUE!` (the TS converts the object); reproduced as the TS: `[$-constructor]`/`[$-__proto__]` in a TEXT code (the `LCID` object literal: `[$-function Object() { [native code] }]` reaches the number format), `[constructor]`/`[__proto__]` as colours, `[[:constructor:]]` and every other `Object.prototype` POSIX name an invalid pattern (the TS class text breaks the `u` syntax); NaN positions as JS reads them (`FIND("";s;NaN)` = NaN, `SUBSTITUTE(s;o;r;NaN)` = `s.slice(0,-1) + r + s.slice(o.length - 1)`, `CHAR(NaN)` = "\0", `UNICHAR(NaN)` and `REPT("";∞)` the `RangeError`). |
| A4d-8 | **Bounded memory where the TS builds the text first**: CONCAT, TEXTJOIN, ARRAYTOTEXT, SUBSTITUTE, REGEXREPLACE and ENCODEURL stop keeping the text once it passes 32,767 units (the errors of the arguments are still read in order; the length check answers `#VALUE!` as the TS); TEXTSPLIT past 2^24 cells throws `Throw::Range` (A4a-4). |
| A4d-9 | **Performance without changing results**: outputs on the stack (`Buf`) and copied once into the pass arena; LEN/LEFT/RIGHT/MID/FIND/SEARCH/EXACT/CODE/UNICODE read their texts in place (no allocation per evaluation for short Latin-1 or UTF-16 texts); SEARCH's wildcard `RegExp` (`[\s\S]*`, `/i` without `u`) is a segment matcher — the first segment at its leftmost place, each later one leftmost after it (feasibility only shrinks as the start moves right, so the first start that fits is the regex's `index`), O(n·m) instead of backtracking; TEXTBEFORE/AFTER/TEXTSPLIT remember each delimiter's next occurrence while it stays ahead (the TS re-runs `indexOf` per step) and `countHits` is one forward scan (the TS re-runs `nthIndex` per count: quadratic). |

### Parity

Replay of `tests/a4.rs`, unchanged harness. All families (release, so the whole-column traps are compared) and the
family alone (`UO_A4_FN=<the 62 names>`). The all-family counts were taken while A4f's bodies were landing in the same
tree (438–456 functions with a body in the big runs), so they mix both frontiers; the A4d column is the family alone:

| Group | Compared (all families) | Equal | Of which A4d (62 functions) | Deferred | Notes |
|---|---|---|---|---|---|
| bank CI (35,299 cases) | 28,352 (A4c: 24,246) | **28,352** | 4,224 compared, **4,224 equal**, 0 deferred | 6,886 + 57 legacy `@` (A5) | |
| bank big (pt-BR + en-US) | 57,904 | 57,895 (the 9 clock records below) | 8,448 compared, **8,439 equal + 9 clock records** (below) | 0 | |
| calc-traps CI (54,120 calls) | 42,248 (36,137) | **42,248** | 5,207 compared (release; 5,080 debug), **5,207 equal**, 0 deferred | 11,849 | the 27 TS throws of the family (stack overflows of whole-column spreads, `UNICHAR(NaN)`) equal |
| calc-traps big (308,120 calls) | 260,998 | 260,995 + 1 NaN payload (the other 2: A4b's `pow` platform records) | 28,874 compared, **28,874 equal** | 0 | the 2 TS timeouts of the family (BAHTTEXT of ≈ 1.8e308, A4d-3 (b)) answered `throw:RangeError` in < 0.1 ms |
| calc-order CI x1 (A3 replay) | values 82,915 (A4c: 73,775) | **82,915** | — | 16,701 (25,841) | order equal 384, random equal 274, incremental = full 19; ops group unchanged (560 runs, 129,106 values). Counted while A4f's bodies were landing in the same tree (IF and the information functions also turned some cells from deferred into compared) |
| calc-order big | values 9,751,663 (A4c: 8,741,784) | **9,751,663** | — | 1,284,180 (2,294,059) | order equal 10,719, random equal 8,484, `circular` 800/800, incremental = full 591 (577) |

**The 9 records of the big bank are the capture's clock, not the port**: VALUE of a year-less date text in en-US
(`VALUE("jan-1")`, `text-value-1441@en-US` and 8 more): the TS reads the year from `new Date().getFullYear()`
(`coerce.ts:197`), which `capture-bank.mjs` does not freeze, so the big capture (taken 2026-11-24) holds 1/1/2026
(46023) where the Rust mock's frozen clock (45658.5, decision C1) gives 1/1/2025 (45658) — every difference is exactly
365 days, the pt-BR records of the same cases (`jan-1` read as January 2001) are equal, and the CI capture has no
en-US record. The fix belongs to the capture (freeze `Date` in `capture-bank.mjs`, or record the year), open item (7);
`a4_big` asserts no mismatch and fails on them, as on A4b's 2 platform records.

**0 parity rounds**: the first CI replay of the family (bank and traps) was equal; the only defect found by the
first run was a panic of the output buffer (`Buf` past its stack capacity with an empty stack part), fixed before
any comparison counted. Unicode-version check (R1): `regress`'s `\p{L}`, `\p{N}` and `\p{Ll}` tables equal V8 13.6's
(`uo_text`, Unicode 17) on all 1,112,064 scalar values — 0 differences.

### Robustness, memory, fuzz

`a4_robust.rs` runs the 62 bodies through its random calls and M1 (with the family's `clear_caches()` after each
call): no panic, M1 green. **Zero allocation per evaluation** (`tests/a4d.rs`): a recalc of 2,000 formulas of each of
LEN, LEFT, RIGHT, MID, FIND, SEARCH (wildcards), EXACT, CODE+UNICODE, VALUE, LENB+FINDB over text cells allocates no
more than 2,000 `A1&""` formulas (112 B, the scheduler's lists). Hostile inputs (release): the catastrophic regexes
above at the budget (≤ 47 ms), FILTERXML 88 ms, REPT at 16,383/16,384 × "ab" (the limit), SUBSTITUTE growing past the
limit stopped early, TEXTSPLIT of 16,000 cells, SEARCH with 500 wildcard segments over 32,000 units — all bounded.
**M5**: the pattern cache stays ≤ 256 past 300 patterns and `clear_caches()` empties it. Caches: the pattern cache
(A4d-2), the `FormatLocale` per locale id (cap 8; the TS's is never cleared) and `uo_numfmt`'s code cache (its own
cap, D8 of `uo-numfmt.md`).

Fuzz: six `formula_parse` seeds over the family (UTF-16 positions and NaN starts, wildcards, the format-code
translation with `[$-constructor]`, splits over long texts and whole columns, the catastrophic regexes and the
untranslatable `\p{L`, hostile XML/XPath and the `&#1114112;` reference, BAHTTEXT of the largest double, the regional
functions). Seeds in `core/fuzz/src/calc.rs` `formula_seeds`. **The fuzz run was not executed in this session**: the
fuzz workspace builds every engine crate in release, and with 7.7 GB free on D: (shared with two other frontiers
building) the run into its own `core/fuzz/target` was refused by the session's permission check and a build into
`target/sheet-s0` was not attempted (disk). Instead `tests/a4d.rs` `fuzz_seeds_recalc_bounded` recalculates the six
seed formulas through the engine (no panic; 83 ms release, 2.3 s debug). Open item (6).

### Performance (PERF)

`benches/a4d.rs` vs `js/bench-a4d.mjs` (frozen TS engine, Node 24.15, same cells; medians of 5 after a warm-up;
i5-14600KF). **text-ts-50k**: the TS baseline's text class (`gen-workbook.mjs`: UPPER, LEN&"x", SUBSTITUTE, TEXT,
LEFT&RIGHT over 50,000 rows); **text-fn-50k**: LEFT, MID, FIND, SUBSTITUTE, UPPER, TEXT(·;"#.##0,00"), CONCAT, TEXTJOIN,
LOWER, TRIM over 50,000 rows; **regex-5k**: REGEXTEST/REGEXEXTRACT (modes 0 and 2)/REGEXREPLACE (with a group) over
5,000 rows, six patterns (the cache).

| Class | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | engine heap (TS retained / Rust) |
|---|---|---|---|---|---|---|
| **text-ts-50k** | 182 | 53 | 72.8 | **39.3** | **0.54** | 64.1 MB / 6.6 MB |
| **text-fn-50k** | 161 | 45 | 66.7 | **37.4** | **0.56** | (GC noise) / 3.0 MB |
| **regex-5k** | 23 | 5.7 | 6.93 | **6.91** | **1.00** | (GC noise) / 0.4 MB |
| text-50k of `benches/a3.rs` (`A{i}&"x"&i`, operators only) | 129 | 51 | 61.6 | **38.8–40.1** | **0.63–0.65** | 58.9 MB / 21.7 MB |

**Verdict: over the ≤ 0.5× recalc budget (0.54/0.56) and at 1.00× on regex-5k (rule: ≤ 1.0× per function micro) —
for the maintainer.** Where the time goes, measured: the bodies are not it — 50,000 `LEN(A{i})` (a number result over
the same text cells) recalc in 7.9 ms, while every class whose formulas produce a TEXT costs ≈ 0.75 µs per formula
whatever the function (`A&"x"&i`, operators only, already 38.8 ms = 0.63×, the A3 record's 0.65×). The cost is the
engine's string-result path (A3: the pass arena → `store_write` → `write_value` interning into the pool and releasing
the old text, per formula and per recalc); a skip of re-interning an identical text in `write_value` was tried and
changed nothing (reverted), so the cost is elsewhere on that path — an A3 profiling item, not a body one. regex-5k:
`regress` is an interpreter against V8's compiled irregexp; 1.10× before A4d-2's single-executor scan, 1.00× after; the
dearest pattern (`[\w.]+@[\w.]+`, quadratic over the start positions in both engines) costs 1.4 µs per call: an ASCII
bitmap for bracket classes and the start-position prefilter that the `utf16` feature disables in `regress` are the
next levers (both vendored-code changes). These are the first A4d records (the 10% rule starts from them).

### Open items

(1) **Maintainer**: the vendored `regress` (A4d-1: placement, licence notices) and the two budget misses above
(text classes 0.54–0.56×, regex-5k 1.00×). (2) The engine's string-result path (A3 owner): ≈ 0.6 µs per text formula
beyond a number formula; it decides the text classes and `a3`'s text-50k. (3) Shared-helper candidates (none needed
by a second family yet): `js.rs` (JS string primitives over UTF-16 + the stack `Buf`), `scalars_js` (the
`scalarsOf` spread bound; `tdl::scalars_of` itself does not throw — A4e's `scalarsOf` callers will need the same
rule). (4) `tests/a3_robust.rs` `incremental_equals_full` failed in this tree on a `-0`/`0` cell (seed 2) after A4f's
`IF` got a body mid-run — not this family (its generator has no text function); left to A4f. (5) The TS sorts its
`FILTERXML` items with `Array.prototype.sort` on `order` numbers; Rust sorts by an integer key (stable) — equal
because node keys are unique and attribute items of one owner keep insertion order in both. (6) The fuzz run of the
six new seeds (stable driver, `--slow 1`) is still to do, when D: has room for the fuzz workspace's release build.
(7) The bank capture's year (`new Date()` in `coerce.ts:197`, not frozen by `capture-bank.mjs`): the 9 en-US clock
records of the big bank — the harness (A4a's) needs the year frozen or recorded.

## A4e — as built (2026-11-24, Opus)

The lookup family, the last function family: every body of `lookup.ts` (15 names: VLOOKUP … FIELDVALUE, with
TRANSPOSE, HYPERLINK, IMAGE and the connection stubs), `reference.ts` (16: INDEX, OFFSET, INDIRECT, ROW, COLUMN, ROWS,
COLUMNS, AREAS, SHEET, SHEETS, FORMULATEXT, ISREF, ISFORMULA, CELL, INFO, SINGLE), `dynamic.ts` (21: FILTER, SORT,
SORTBY, UNIQUE, SEQUENCE, RANDARRAY, TAKE, DROP, EXPAND, VSTACK, HSTACK, TOCOL, TOROW, WRAPROWS, WRAPCOLS,
CHOOSEROWS, CHOOSECOLS, TRIMRANGE, PERCENTOF, GROUPBY, PIVOTBY), `database.ts` (12 D-functions) and `conditional.ts`
(8: COUNTIF … MINIFS) — 72 bodies. Two engine hooks (A4e-1, A4e-5), additive.

**Code** (`core/crates/uo-calc/src/functions/lookup/`, ≈ 4,000 lines with tests):

| File | Content |
|---|---|
| `mod.rs` | `BODIES`; the family's comparisons (`kind`, `cmp_same`, `xrank`, `cmp_total` = `cmpTotal`/`sortCmp`), `area_ref`, `fail` |
| `line.rs` | the TS `Vec` (`LineVec`: a dense array line, or the existing cells `idx`/`val` of a reference line; `get`, `scan`, `blank`, `exact_first` with the TS hash index, `collate_find`, A4e-2), `of_area_cached`, and **`PassCache`** (the engine's per-pass memory, `pub`) |
| `search.rs` | `lookup.ts`: `shape_of`, `line_of`, `cell_of`, `Pred` (`exactPred`), `lookup_wildcard` (`lookupRegExp` on the criteria `Wildcard`), `find_exact`/`find_approx` (Excel's binary search over unsorted mixed data, step for step)/`find_desc`, `xfind`/`xbin`, the bodies, `sheet_prefix`, `letter_name` |
| `reference.rs` | `reference.ts`: `scalar_arg`, `array_of_refs`, `index_area`/`index_array`, OFFSET/INDIRECT, `per_cell`, FORMULATEXT through A5's `i18n::Translator` (`toLocal`), CELL/INFO through `FnContext::cell_info` |
| `dynamic.rs` | `dynamic.ts`: `mat` (`A`), `int_or`/`bool_or`/`scalar_of`, `fill_js` (A4e-7), the sorts on `uo_jsnum::sort::js_sort_order` (V8's replay), `push_cell_key` (`cellKey`), GROUPBY/PIVOTBY (`aggregate` with `lambda_arity`) |
| `database.rs` | `database.ts`: `field_index`, `parse_criteria` (field conditions and computed criteria), `matched`, `db_numeric` over `shared::agg` |
| `conditional.rs` | `conditional.ts`: `Gathered`/`Areas` (one walk per distinct area), `Src` (gathered cells, `ctx.cell` past a stopped walk), `all_match`, `count_matches`, `matched_reduce` (streamed `rSum`/`rAvg`/`rMax`/`rMin`), the bodies (A4e-3) |

Outside the family (hooks, all additive): `fnreg.rs` — `FnContext::pass_cache()` and `FnContext::computed_criterion()`
(default methods: `None`); `eval.rs` — `EvalHost::pass_cache()` and `EvalHost::formula_node()` (defaults `None`),
`CellCtx` forwards the cache and implements `criterionAt` (`engine.ts:583-589`: the criterion cell's tree evaluated
at `row + offset`, dynamic, then `topScalar`); `engine/mod.rs` — `Graph::pass_cache` (also reset by `clear_caches`);
`engine/host.rs` — `Host::pass_cache`, `Host::formula_node` (the cell's current formula tree); `engine/recalc.rs` —
the four clears of `cache` (`run` start and end, `computeDirty`, each sweep of `iterateCycles`). Harness (A4e-10):
`tests/support/fnmock.rs` (`lambda_arity`, no file name), `tests/a3.rs` (the full recalc's random stream). New:
`tests/a4e.rs`, `benches/a4e.rs` + `[[bench]] a4e`, `js/bench-a4e.mjs`.

### Decisions taken in A4e (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4e-1 | **The pass cache is the TS `ctx.cache`** (`engine.ts:220`; only `lookup.ts` uses it, key `tdl:vec:<area>`): the lookup vector of a reference line is built once per pass (`Vec.ofAreaCached`) and shared (`Arc`), with the same clear points as the TS; a vector whose walk throws `NeedCell` is not kept. Cap (M5, record "Memory plan"): 64 MiB wasm32 / 256 MiB native, a vector and its estimated indexes accounted (`k · 116 B`); past it the vector is not kept (rebuilt by the next search). Without an engine (the mocks) every call builds its own. Declared: a cached vector holds pool ids; a cycle member's text rewritten later in the same pass could release its pool entry where the TS keeps its JS string (only the stale reads of a non-iterative cycle, whose values the TS itself leaves stale). |
| A4e-2 | **Exact indexes.** MATCH 0/VLOOKUP/HLOOKUP exact use the TS's own hash index (`exactFirst`: lower-case texts, `-0` = `0`, booleans), built at the first exact search of a cached vector; a dense array line of one call is scanned (same first hit, no map). XLOOKUP/XMATCH mode 0 (and 2 without wildcards), search ±1, are **linear in the TS** (`compareText` per cell); here a cached vector gets a collation index (collation key → first and last index; numbers `===`, NaN never equal): the same hit because equal keys ⇔ `compareText` = 0 (`collationKey`'s invariant, also the TS's UNIQUE). 1,000 text XLOOKUPs over 100,000 rows: TS 28.9 s, Rust 156 ms. Binary modes (±2), approximate (±1) and regex (3) keep the TS's scans. |
| A4e-3 | **Conditional reads.** The TS walks one area (`forEachCell`) and reads the other conditions at each position (`ctx.cell`), so a `NeedCell` names the first dirty cell in that interleaved order. Here every distinct area is gathered with one walk; when every walk completed nothing can throw (the reads are lookups with a forward cursor, O(cells)); when a walk stopped at a dirty cell, the TS algorithm runs over the gathered prefix, positions past it are read for real at the TS's moment, and the walk of an area whose gather stopped throws that gather's `NeedCell` where the TS walk would reach it — the first throw is the TS's. Verified by the A3 order facet (11,200 big runs equal) and `tests/a4e.rs` (INDIRECT ranges over dirty formulas, incremental = full). |
| A4e-4 | **Locale borrow.** A `Criterion` borrows the locale; matching between `&mut` cell reads needs a copy: the D-functions copy the `ValueLocale` once per call (their reads interleave by nature); the conditional functions copy it only when a walk stopped (A4e-3), otherwise they match through `&ctx`. |
| A4e-5 | **Computed criterion** (`computedCriterion` = the engine's `criterionAt`): `FnContext::computed_criterion(sheet, row, col, offset)`; the engine evaluates the criterion cell's current tree (`formula_node`; A5's legacy `exec` tree is to be returned there when it differs) at `row + offset`; no formula or no engine → the constant rule (only TRUE matches). Evaluated in the TS order (`alts.some(conds.every)`, short-circuit), so its `NeedCell`s and random draws are the TS's. |
| A4e-6 | **OFFSET with a NaN offset or size** (trap values only: no formula yields NaN) returns `Throw::Range`: the TS returns an area with NaN coordinates whose read is `new Array(NaN)` (the capture records `throw:RangeError`, `OFFSET#x22@en-US`). |
| A4e-7 | **Bounds** (R1 / A4a-4): `fill_js` gives `ArrayValue.fill` the JS `new Array(n)` rule (NaN, negative or fractional → `Throw::Range`) and A4a-4's 2^24-cell cap (SEQUENCE, RANDARRAY — checked before any random number is drawn —, EXPAND, TAKE/DROP, WRAPROWS/WRAPCOLS, CHOOSEROWS/CHOOSECOLS, TRIMRANGE, VSTACK/HSTACK, OFFSET/INDIRECT arrays of `#VALUE!`, ROW/ISFORMULA/FORMULATEXT per cell); XLOOKUP/XMATCH regex mode gets A4d's step budget **per test** (a whole column of legitimate tests does not add up to the budget) → `Throw::Range` past it (the TS never returns on a catastrophic pattern). Measured (`tests/a4e.rs`, release): `XMATCH("(a+)+b";…;3)` over 300 texts, SEQUENCE(1048576;16384), RANDARRAY(1048576;1000), EXPAND to the sheet, MATCH/XLOOKUP/COUNTIF/SUMIFS/UNIQUE over whole columns: 79 ms together. |
| A4e-8 | **Sorts and grouping**: SORT/SORTBY/GROUPBY/PIVOTBY replay V8's `Array.prototype.sort` (`js_sort_order`: the comparators are not total with NaN); UNIQUE and GROUPBY group by the TS strings (`cellKey` joined by U+0001, texts by their collation key); PIVOTBY's `rr.filter((r) => cc.includes(r))` is a row mark (O(n), same order; the TS is O(n·m) per cell). |
| A4e-9 | **Streamed reductions**: the *IF/*IFS sums, averages, maxima and minima fold the matching numbers in walk order (the same additions as over the TS list, no list); AVERAGEIF(S) of a non-finite sum is NaN, as the TS (`rSum(xs) as number / n` divides the `#NUM!` object). |
| A4e-10 | **Harness** (three fixes, each to the TS's own harness): `fnmock`'s LAMBDA of a function name has the function's `minArgs` parameters (`testing.ts`: GROUPBY passes the total to PERCENTOF — 4 bank cases); `fnmock`'s formula cases build the engine without a file name (`runFormulaCase` passes none: `CELL("address")` of another sheet has no workbook — 2 bank cases; the A4a mock had `Pasta1.xlsx`); `tests/a3.rs` reproduces the full recalc's random stream when the capture has no `full.rnd` (`capture-order.mjs`: `RecordedRandom(hash32(key + "|full") || 1)`, xorshift32 / 2^32 — before A4e every run with a RAND* cell was deferred by RANDARRAY's missing body). |

### Parity

`tests/a4.rs` (unchanged replay but A4e-10) and `tests/a3.rs`. The family alone (`UO_A4_FN=<the 72 names>`) and all
families after A4e:

| Group | Compared (all families) | Equal | Of which A4e (72 functions) | Deferred | Notes |
|---|---|---|---|---|---|
| bank CI (35,299 cases) | 35,295 | **35,295** | 6,078 compared (55 functions have cases), **6,078 equal** | **0** | the 57 legacy `@` cases compared (A5) |
| bank big (pt-BR + en-US) | 70,590 | 70,581 + the 9 clock records (A4d open item 7) | 12,156, **12,156 equal** | **0** | |
| calc-traps CI (54,120 calls, release) | 54,097 | **54,097** | 8,405 compared, **8,405 equal** (debug 8,200 + 205 whole-column) | **0** | 8 random records (RANDARRAY) equal value and consumption |
| calc-traps big (308,120) | 307,930 | 307,927 + 1 NaN payload + A4b's 2 `pow` platform records | 46,932, **46,932 equal** | **0** | 54 random records; the 1 first mismatch was OFFSET NaN (A4e-6) |
| calc-order CI x1 (A3 replay) | values 99,616 (A4d: 82,915) | **99,616** | — | **0** (A4d: 16,701) | order equal 420/420 runs, random equal 420, `circular` 30/30, incremental = full 30/30 |
| calc-order CI ops | values 132,021 | **132,021** | — | 0 | 560/560 runs |
| calc-order big | values 11,035,843 (A4d: 9,751,663) | **11,035,843** | — | **0** (A4d: 1,284,180) | order equal 11,200/11,200, random 11,200, `circular` 800/800, incremental = full 800/800 |

**What stays deferred after A4e: nothing** in the bank, trap and calc-order replays. Outside them: the `cellInfo` host
callback (C5) — no capture has a host `cellInfo` (CELL "format"/"color"/… answer the TS defaults when the host has
none, which is what every capture records); the `calc-tests` Engine calls (A3 open item 2, now replayable: every body
exists); the known capture issues (A4b's 2 `pow` records, A4d's 9 clock records). 0 parity rounds: the first CI bank
replay had 6 mismatches, all harness (A4e-10); the first big trap run 1 (A4e-6); the first A3 replay 128 values +
2 verdicts, all the full recalc's missing stream (A4e-10).

### Robustness, memory, fuzz

`a4_robust.rs` runs the 72 bodies through its random calls (600 per function release; slowest call of the run 892 ms,
A4c's DB over a whole column) and M1 (each call over a fresh mock leaves no live bytes: the family keeps no cache
outside the engine's pass memory). `tests/a4e.rs`: **allocation per pass, not per formula** — a second recalc of
2,000 and of 4,000 exact VLOOKUP/MATCH/XLOOKUP/XMATCH over one 1,000-row range allocates the same 111,036 B beyond the
operator formulas (one vector + one index per pass), INDEX over a range 0 B, INDEX(MATCH approximate) 41,312 B; text
keys allocate their lower-cased or collation key per search. The computed criterion through the engine (DSUM/DCOUNT/
DGET with `=B2>10` under a non-field label, and after an edit); conditional functions over INDIRECT ranges of dirty
formulas equal a fresh full recalc after 30 edits; the hostile shapes of A4e-7. **M5**: `PassCache` past its cap keeps
nothing more (`line.rs` unit test); the engine clears it at the end of every `run` (a recalc leaves no vectors).

Fuzz: **not run** (disk: 8 GB free on D:, the fuzz workspace builds every engine crate in release). Seeds for
`core/fuzz/src/calc.rs` `formula_seeds` (to add with the run): `=XMATCH("(a+)+b";A1:A300;3)`,
`=SUMIFS(INDIRECT("B1:B200");INDIRECT("C1:C200");"y")` over dirty formulas, `=GROUPBY(A1:A20;B1:B20;PERCENTOF;3;-2;{-2;1})`,
`=PIVOTBY(A1:A9;B1:B9;C1:C9;SUM;3;-1;;-1)`, `=OFFSET(A1;1E21;0;-1E300;0)`, `=SEQUENCE(1048576;16384)`,
`=TAKE(A:A;-1E21)`, `=WRAPROWS(A1:A5;0/0)`, `=DSUM(A1:B5;"Qty";F1:F2)` with `F2 = B2>10`, `=MATCH("zz";A:A;1)`,
`=XLOOKUP(1;A:A;B:B;;1;-2)`, `=UNIQUE(A:A;;TRUE)`.

### Performance (PERF)

`benches/a4e.rs` vs `js/bench-a4e.mjs` (frozen TS engine, Node 24.15, same cells; medians of 5 after a warm-up;
i5-14600KF). Both runners print the count of numbers/errors and the bits of their sum in row order: **equal in every
class**.

| Class | formulas | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | engine heap (TS retained / Rust) |
|---|---|---|---|---|---|---|---|
| **lookup-10kx10k** (the TS baseline class: VLOOKUP/MATCH exact, XLOOKUP over 10k rows; record 93.7 ms) | 10,000 | 62.8 | 19.8 | 88.5 | **3.75** | **0.042** | 15.2 MB / 5.4 MB |
| **dynamic-1k** (the TS baseline class: 20 spills of SORT/UNIQUE/FILTER/SEQUENCE over 1k rows; record 9.8 ms; A3's operator variant 0.81×) | 20 | 1.2 | 0.1 | 6.3 | **2.88** | **0.46** | — / 0.36 MB |
| vlookup-exact-100k (10k exact VLOOKUP over 100k number keys) | 10,000 | 88.3 | 24.6 | 37.8 | **7.77** | **0.21** | 24.3 MB / 5.6 MB |
| xlookup-text-100k (1k exact XLOOKUP over 100k text keys) | 1,000 | 51.9 | 6.2 | 28,867 | **156** | **0.005** | — / 0.05 MB |
| match-approx-100k (10k MATCH 1 / VLOOKUP TRUE, binary search, 100k sorted rows) | 10,000 | 171 | 23.9 | 45.7 | **9.64** | **0.21** | 8.0 MB / 5.6 MB |
| ifs-100k (200 COUNTIFS/SUMIFS/COUNTIF/AVERAGEIF over 100k rows) | 200 | 73.8 | 5.8 | 3,516 | **1,657** | **0.47** | — / 0.1 MB |

**Verdict: every class inside the ≤ 0.5× native budget**; these are the first A4e records (the 10% rule starts from
them). The tight ones are dynamic-1k (0.46×: SORT/UNIQUE/FILTER of 1,000 rows are a few µs each, the rest is the
engine's spill write-back of 17,480 cells) and ifs-100k (0.47×: ≈ 8.3 ms per formula, ≈ 30 ns per cell for the walk
of 1.75 areas of 100,000 cells through `for_each_cell` — the engine's `Store::each` + view per cell; a lane walk for
all kinds would be an engine change, open item 2). The xlookup-text class is 156 ms of collation keys per pass (the
index is rebuilt each recalc, A4e-1).

### Open items

(1) **Fixed at the orchestrator's request** (last frontier in the crate): `_xlfn.`-prefixed calls — and with A5 the legacy `@` → `_xlfn.SINGLE(…)` — copied their prefix in `eval.rs` `func_call` (`to_utf16()` for the `_xludf.`/`_xll.` test, ≈ 17 B per call); the test now reads the prefix in place and `tests/a4f.rs` `zero_alloc_scalar_paths` is green again (112 B = the operator baseline). (2) The conditional functions' cost is the
engine's cell walk (≈ 30 ns per cell): a column-run walk that hands every value kind (A4b open item 4's extension) would
roughly halve ifs-100k. (3) The lookup indexes are per pass (the TS semantics of `cache`); keeping them across passes
needs invalidation on writes (an engine design question). (4) `formula_node` returns the parsed tree; A5's legacy
`exec` tree belongs there when it differs (`f.exec ?? f.node`). (5) The fuzz run of the seeds above. (6) Clippy `--all-targets --all-features -D warnings` (native and wasm32) is clean on every target of the crate but `tests/a5_robust.rs:128` (`f64::max` → `uo_jsnum::math::max`; A5's file, the edit was refused by this session's permission check); `tests/a4d.rs:179`'s `str::trim` was moved to `uo_jsnum::text::trim`.

## A4f — as built (2026-11-24, Opus)

The logical/information/cube family: every body of `logical.ts` (21 names: IF, IFS, IFERROR, IFNA, SWITCH, CHOOSE,
AND, OR, XOR, NOT, TRUE, FALSE, the catalogue bodies of LET/LAMBDA/ISOMITTED, MAP, REDUCE, SCAN, BYROW, BYCOL,
MAKEARRAY), `info.ts` (15: ISBLANK … ISLOGICAL, ISEVEN, ISODD, N, T, NA, TYPE, ERROR.TYPE) and `cube.ts` (8: the seven
CUBE* and STOCKHISTORY) — 44 bodies. ISREF, ISFORMULA, CELL, INFO, SHEET, SHEETS and FORMULATEXT are `reference.ts`
in the frozen TS (family `lookup`, A4e), so CELL's `cellInfo` host callback (C5) is A4e's, not here. LET, LAMBDA and
ISOMITTED stay special forms of the A2 evaluator (`Intrinsic`); their registered bodies (`#CALC!`, `#CALC!`, FALSE)
answer only when the function is used as a value (`_xleta.LET`). The evaluator has no special case for IF/IFS/SWITCH/
IFERROR: they are `lazy` functions (A2 `Arg::Thunk`), and the bodies force the thunks.

**Code** (`core/crates/uo-calc/src/functions/logical/`):

| File | Content |
|---|---|
| `mod.rs` | `BODIES` (`logical`, `info`, `cube`); `logical.ts`: `Sa` + `sa` (`scalarOrArray`), `dims`, `fill` (capped), `Memo` (the TS `x ??= f()`), `force`/`of_thunk` (`ofThunk`), `text_to_bool`/`bool_of`, `pick_array`, IFS's array path, `replace_errors`, SWITCH (`switch_one`, `same_value`), CHOOSE + `number_text` (`toNumberText`), `Fold` (`logicalFold`), `Input` (`lambdaInput`), `lambda_of` (the parameter check), `fill_lambda`, `lambda_scalar`, MAP/REDUCE/SCAN/BYROW/BYCOL/MAKEARRAY, `to_int` |
| `info.rs` | `info.ts`: the `is` bodies, `parity`, `lift1` (`liftAtp` of one argument, A4f-4), `first_or_each`/`first_cell`, N, T, NA, TYPE, ERROR.TYPE |
| `cube.rs` | `cube.ts`: `first_error`, `cube` (`cubeFn`'s body; the `members` positions are the registry's argument modes), STOCKHISTORY |

Outside the family: `FnContext::lambda_arity` (`fnreg.rs`, a new default method) and its evaluator implementation
(`eval.rs`, `CellCtx`), A4f-2. Tests: the unit test of `Memo`; `tests/a4f.rs` — zero allocation per evaluation over 36
single-call formulas, the LAMBDA parameter checks through formula LAMBDAs, the hostile shapes (A4f-3); the family is
in `tests/a4.rs` (parity) and `tests/a4_robust.rs` (random calls, M1) through `functions::ported()`. Harness changes
(A4f-8): `tests/a3_robust.rs` `incremental_equals_full`, `tests/a4_robust.rs` `random_calls`. Benchmarks
`benches/a4f.rs` + `js/bench-a4f.mjs` (`[[bench]] a4f` appended to `Cargo.toml`).

### Decisions taken in A4f (declared; observable output equal unless said)

| Id | Decision |
|---|---|
| A4f-1 | **Lazy evaluation as the TS forces it**: IF/IFS with an array condition evaluate each branch once, in the order the condition first uses them (`[...used]` of a `Set`); SWITCH caches its keys but evaluates the chosen result per element; the caches the TS keeps with `??=` (IF's branches, IFS's values, SWITCH's keys) **re-evaluate a value that was `null`** at its next use (`Memo`: an empty-cell branch is forced again per element — the number of evaluations, random draws and `NeedCell`s is the TS's). The scalar paths keep no cache (each thunk is forced at most once there). |
| A4f-2 | **`FnContext::lambda_arity(f) -> (params, body is a function)`** (`fn.params.length`, `typeof fn.body === 'function'`): MAP/REDUCE/SCAN/BYROW/BYCOL check a formula LAMBDA's parameter count before any call, as the TS (`#VALUE!` once, not an array of `#VALUE!`); a function used as a value (`_xleta.SUM`: one parameter) is not checked. New default method of the A2 trait (no implementor has to change): the default is the test mocks' JS LAMBDA (1 parameter, a function body); the evaluator answers from its closure records. The parity mocks only have JS LAMBDAs, so the check is tested through the engine (`tests/a4f.rs` `lambda_arity`). For A4e: `dynamic.ts` reads `fn.params.length >= 2` (GROUPBY/PIVOTBY) — the same method; `testing.ts` gives a JS LAMBDA `minArgs` parameters, which `fnmock`'s default (1) does not — A4e overrides it in the mock if its cases need it. |
| A4f-3 | **Bounds** (decision R1 / A4a-4): the broadcasts of IF/IFS/IFERROR/SWITCH and the LAMBDA fills (MAP, SCAN, BYROW, BYCOL, MAKEARRAY) past 2^24 cells throw `Throw::Range` (`#NUM!` in the engine; the TS allocates up to 2^32 − 1 cells); REDUCE over more than 2^24 cells throws the same before its first call (the TS calls the LAMBDA 1.7·10^10 times for `A:XFD` — hours); MAKEARRAY keeps the TS `#NUM!` past the sheet size (`n·m > 1048576·16384`) and throws `Range` between 2^24 and it (the TS `new Array(n·m)` throws past 2^32 − 1 and computes for minutes below). `fill_lambda` reserves at most 65,536 cells up front (the TS `new Array(n)` is holey: a first nested array answers `#CALC!` without the memory). Measured (`tests/a4f.rs`, debug): `REDUCE(0;A:XFD;…)`, `SCAN`/`MAP` over `A:XFD`, `MAKEARRAY(1048576;16384;…)`, `MAKEARRAY(1E10;1;…)` `#NUM!` in ≤ 1.4 ms; `IF(A:A>0;1:1;0)` and `IFERROR(A:A/0;1:1)` (1M × 16K broadcasts) `#NUM!` in 60–70 ms. |
| A4f-4 | **ISEVEN/ISODD** go through `lift1`, `liftAtp` of one argument without its argument list (no allocation on the scalar path; a one-cell reference read, a larger one or a union `#VALUE!`, a LAMBDA `#CALC!`, an array element by element at least 1 × 1 with `#N/A` leftovers). An omitted argument (`OMITTED`, a JS `Symbol`) reaching `toNumber` throws the TS `TypeError` (`Throw::Internal`: `#VALUE!` + `internalErrors` at the engine; an argument that evaluates to `null`, e.g. an omitted optional LAMBDA parameter); an Excel error of an element is the element's value (A4c's lesson: not a `?` out of the lift). |
| A4f-5 | **AND/OR/XOR over a one-cell area** read the cell (`ctx.cell`) instead of `for_each_cell`: the same value (an absent cell reads empty, which the fold ignores) and the same `NeedCell`, without the engine's area cursor; larger areas keep `for_each_cell` (open item 1). Text given directly counts only when it is the locale's boolean name (`textToBool`: full Unicode upper case, no trim — `uo_text::case`). |
| A4f-6 | **T** makes the empty text (`''`, the pass arena) only when a result needs it; SWITCH's equality (`sameValue`) compares the JS types first (empty = 0) and then `compareScalars` (texts by the collation); CHOOSE's and MAKEARRAY's text numbers (`toNumberText`) remove the locale's group separators, turn the first decimal into a point and read `Number` (`cell::js_number`: JS white space, hex/octal/binary, `Infinity` not finite → `#VALUE!`). |
| A4f-7 | **CUBE\*/STOCKHISTORY**: the first scalar error of any argument (arrays and references at the `any` positions do not count), then CUBERANKEDMEMBER's rank / CUBEKPIMEMBER's property must be numbers (text that is not → `#VALUE!`), then `#N/A`; STOCKHISTORY validates the code (a text not all JS white space) and the two dates, then `#BUSY!` (no quote service). |
| A4f-8 | **Harness** (two tests that changed once IF had a body): `tests/a3_robust.rs` `incremental_equals_full` compares `-0` and `0` as one value — a fresh engine's formula cell starts at 0 and a computed `-0` is not written (`rec.v !== v`, A3-1), while the incremental engine keeps a `-0` it wrote earlier: both are the TS's behaviour (seed 2, `IF(F$3>1;F$3;B4)` with B4 = −0); `tests/a4_robust.rs` `random_calls` skips the calls with a whole-column argument in a debug build (as `a4.rs` skips the whole-column traps; a lift over 1M cells costs seconds in debug — RATE 6.2 s under load) and the bound is 1,500 ms in both builds again (A4c-7's 4 s debug bound removed); the random sequence is the same in both builds (the legacy flag is drawn before the skip); the release run keeps every call. |

### Parity

Replay of `tests/a4.rs`, unchanged harness (the family's cases turned from deferred into compared). The family alone
(`UO_A4_FN=<the 44 names>`):

| Group | Compared | Equal | Deferred | Notes |
|---|---|---|---|---|
| bank CI (pt-BR) | 517 (24 functions have cases) | **517** | 7 | the 7: `core-lambda-374/387/388/389/410/411/414`, formula cases whose LAMBDA calls ROWS/COLUMNS/ISREF (`reference.ts`, A4e) |
| bank big (pt-BR + en-US) | 1,034 | **1,034** | 14 | the same cases in both locales |
| calc-traps CI | debug 3,360 (84 whole-column calls skipped); release **3,444** | **3,444** | 0 | 41 functions (TRUE, FALSE, NA take no argument: no trap); lazy arguments as the TS `callFn` thunks; MAP/REDUCE/SCAN/BYROW/BYCOL/MAKEARRAY with the JS test LAMBDAs |
| calc-traps big | 17,616 | **17,616** | 0 | no TS timeout among the family |

Every family together (debug CI, this tree with A4d's text family in): bank 28,952 compared, all equal, 6,286
deferred + 57 legacy `@` (A5); traps 44,577 compared, all equal, 8,405 deferred (1,115 whole-column calls skipped).

**A3 replay (calc-order)**, release, the same tree with and without the family's bodies (`BODIES = &[]`), so the
difference is A4f's alone:

| Run | values compared (without → with A4f) | deferred | other facets |
|---|---|---|---|
| CI x1 | 80,484 → **82,915** (+2,431), 0 mismatches | 19,132 → 16,701 | order equal 384, random equal 274, incremental = full 19 (11 deferred), `circular` 30/30 — unchanged; ops group unchanged (560 runs, 129,106 values) |
| big (`UO_A3_BIG=1`) | 9,492,809 → **9,751,663** (+258,854), 0 mismatches | 1,543,034 → 1,284,180 | incremental = full 590 → 591 (210 → 209 deferred); order equal 10,719, random equal 8,484, `circular` 800/800 — unchanged |

The deferred cells that remain need A4e (lookups, ROWS/COLUMNS, INDEX…) or A5 (the legacy `@`). 0 parity rounds.

### Robustness, memory, fuzz

`a4_robust.rs` runs the 44 bodies through its random calls (600 per function in release, 60 in debug) and M1 (each
function over a fresh mock leaves the live bytes exactly where they were): no panic, M1 green; the family's slowest
call in release is 301 ms (CUBERANKEDMEMBER lifted over a whole column: `apply_fn`'s 1M-cell lift, as A4a's
FLOOR.MATH); every family together, release: slowest 749–785 ms (LOGNORM.DIST over a whole column, A4b's — 390 ms in
its record; this machine ran other agents' builds). **Zero allocation per evaluation** (`tests/a4f.rs`): a second full
recalc of 2,000 formulas cycling 36 single calls (IF scalar and with a text literal, IFS, IFERROR, IFNA, two SWITCH,
CHOOSE, AND/OR/XOR/NOT of comparisons and of a one-cell reference, TRUE, FALSE, the eight IS*, ISEVEN, ISODD, N, T,
TYPE, ERROR.TYPE, NA, CUBEMEMBER, CUBERANKEDMEMBER, CUBEVALUE, STOCKHISTORY, nested IF/IFERROR) allocates exactly what
2,000 operator formulas do (112 B). Not in it: AND/OR/XOR over a multi-cell reference (≈ 96 B per call inside the
engine's `for_each_cell`, open item 1); the array paths allocate their result as the TS. Caches: none (the family
keeps nothing between calls).

Fuzz: seven `formula_parse` seeds over the family (listed in the frontier report for the orchestrator to add to
`core/fuzz/src/calc.rs` `formula_seeds`; A4d owned that file during this frontier — run from local corpus files):
array conditions of IF/IFS/SWITCH, IFERROR/IFNA/CHOOSE/AND/XOR/NOT with locale texts, MAP/REDUCE/SCAN over ranges and
whole sheets, BYROW/BYCOL/MAKEARRAY at their bounds and with nested arrays, the IS*/N/T/TYPE/ERROR.TYPE edges, the
CUBE*/STOCKHISTORY checks, whole-column broadcasts and the eta forms of LET/ISOMITTED. Stable fallback driver
(`core/fuzz`, its own target directory, `--slow 1 --seed 4242`), 2026-11-24, 300 s: 345,478 executions, **0
crashes**; 14 inputs over 1 s, none in this family: 13 are A4b's BETA.INV seed (≈ 1–2.2 s, A4b open item 5) and one
(86.8 s) a 12.6 KB mutation repeating ODDFYIELD(…;−1E300;…)/ODDLPRICE/ACCRINT 148 times each — A4c's coupon walks to
the `Date` limit (A4c open item 3) multiplied, reported to the orchestrator. The family's slowest input of the run
(0.8 s, under the 1 s mark at replay) is `BYROW(A:XFD;LAMBDA(r;…))`: 1,048,576 LAMBDA calls, the TS's own work.

### Performance (PERF)

`benches/a4f.rs` vs `js/bench-a4f.mjs` (frozen TS engine, Node 24.15, same cells; medians of 5 after a warm-up;
i5-14600KF, other agents' builds running on the machine; both runners print the count of numbers + booleans, the rest,
and the bits of their sum in row order — equal: `84344` / `15656` / `41643f2d06bc8a4f`). **logical-100k**: 100,000
formulas over a column of numbers A (one in 9 empty, one in 11 `#DIV/0!`) and booleans B — IF, AND, IFERROR,
ISNUMBER, SWITCH (text results), IFS, OR/NOT/XOR, CHOOSE, ISBLANK/ISEVEN, IFNA/TYPE.

| Class | TS build ms | Rust build ms | TS recalc ms | **Rust recalc ms** | Rust / TS | engine heap (TS retained / Rust) |
|---|---|---|---|---|---|---|
| **logical-100k** | 543–592 | 187–199 | 164–184 | **44.1–45.3** | **0.25–0.27** | 205 MB / 6.0 MB |

Per kind (`UO_A4F_ONLY=<j>`: 100,000 formulas of one kind; ms TS → Rust): IF 118 → 42.4 (0.36), AND(A>100;A<900;B)
81–100 → 49.4–50.2 (0.50–0.61), IFERROR 101 → 32.6 (0.32), ISNUMBER 36–40 → 17.3–17.5 (0.45–0.48), SWITCH 223 → 43.6
(0.20), IFS 217 → 54.4 (0.25), OR(…;NOT(B);XOR(…)) 121–179 → 67.8–69.6 (0.39–0.56), CHOOSE 261 → 52.0 (0.20),
IF(ISBLANK;0;ISEVEN) 141 → 35.6 (0.25), IFNA(IF(…;NA();…);TYPE) 177 → 45.7 (0.26). The bodies themselves cost a few
ns; the AND/OR kinds are the evaluator's per-node cost (three or four nodes and one or three calls: ≈ 100 ns per
comparison, the A3 record's limit), where V8 is at its best. **Verdict**: the class inside the ≤ 0.5× budget (0.25×);
these are the first A4f records (the 10% rule starts from them). The A3/A4a–A4c classes are unchanged (no shared code
on their paths changed; `lambda_arity` is a new trait method called only by the LAMBDA helpers).

### Open items

(1) AND/OR/XOR over a multi-cell reference walk it with `for_each_cell`, which allocates the engine's area cursor
(`Store::each`, ≈ 96 B per call) — the same as A4b open item 4 (COUNT, the `*A` functions); a lane walk with every
value kind is an engine (A3) change. (2) A4e: `fnmock`'s JS LAMBDAs answer `lambda_arity`'s default (1 parameter);
the TS mock gives them `minArgs` parameters — override in the mock if `dynamic.ts`'s `fn.params.length >= 2` cases
need it (A4f-2). (3) The 7 (CI) / 14 (big) deferred bank cases wait for ROWS/COLUMNS/ISREF (A4e). (4) Per kind, AND
and OR of comparisons run at 0.5–0.6× the TS: the evaluator's per-node cost, not the bodies — the A3 stage-5 proposal
(compiled trees) is the lever. (5) No helper of this family is needed by another one (`scalarOrArray`, `textToBool`,
`boolOf` are not imported elsewhere in the TS).

## A5 — as built (2026-11-24, Opus)

The formula-language layer and the workbook-level rules of the calc: the legacy `@` rewrite with the 508 measured
classes (`implicit.ts` + `implicit-classes.ts`) wired into the engine (`execCache`, `sourceText`, FORMULATEXT, the
formula bar), the Excel catalogue (`i18n/catalog.ts`), the language tables and the translator canonical ↔ local
(`i18n/*`), the `_xll.` add-in resolution (`addin.ts`), the open-recalc rule (`open-recalc.ts`), the save rules
(`save.ts` + `file-form.ts`) over the store, and the parity harness of these TS surfaces.

**Code** (`core/crates/uo-calc/src`):

| File | Content |
|---|---|
| `catalogue.rs` | `catalog()`, `catalog_entry`, `entry_exact`, `function_kind` (`Sheet`/`Reserved`/`Unknown`), `is_unavailable_function`, `accepts_arg_count`, `with_file_prefix`, `strip_file_prefix_into`/`strip_upper_prefix`, `upper_utf8_into` (root `toUpperCase` into a reused buffer), `operator_of`, `function_names(lang)` (catalogue order, unsorted: the shell sorts with `Intl.Collator`, plan §16 item 11) |
| `i18n/mod.rs` | the types (`CatalogEntry`, `FormulaLanguage`, `Kw`, `ExcelLanguageInfo`…), `EN_US`/`PT_BR`, `languages`, `excel_languages`, `get_language` (exact → main variant of the language → none; no allocation for ASCII ids), `resolve_language`, `value_locale_for` (the measured ids only; the rest is the shell's `deriveValueLocale`, C1), `local_function_name`, `canonical_function_name`, `localize_error`, `canonical_error`, `localize_bool` |
| `i18n/tables.rs` | GENERATED by `js/gen-i18n-tables.mjs` (`--check`): the 535 catalogue rows (id, prefix, min/max, argStep, smallCounts, operator), reserved and unavailable names, the operator functions, the 47 Office languages, en-US and pt-BR with the lookup maps `langData` builds at run time (folded local name → canonical incl. the diacritic-free keys, errors longest first, keywords), the 508 implicit classes |
| `i18n/translate.rs` + `translate/tokenize.rs` | `Translator` (`to_local[_into]`, `from_local[_into]`), `SyntaxOptions` (+ `LangSpec`, `NameSet`), `resolve_syntax`: the TS tokenizer, `rewriteOperators`, `scopeParams`, `parseStructured`, `render`, `renderStructured` |
| `implicit.rs` | `ParamClass`, `FnClassInfo`, `fn_class`, `param_class`, `trait LegacyHost` (`name_is_multi`, `is_name`), `NoNames`, `legacy_to_dynamic[_in]` → `Legacy { root, inserted: (node, implicitAt) }`, `implicit_positions`, `source_text` |
| `addin.rs` | `ATP_FUNCTIONS`, `has_addin_call`, `resolve_addin_calls` |
| `open_recalc.rs` | `TRUSTED_CALC_ID`, `EXCEL_CALC_ID`, `OPEN_VOLATILE`, `OpenRecalcWorkbook`/`CalcPrFlags`, `OpenRecalcPlan`/`Mode`/`Reason`, `has_volatile_function`, `should_recalc_on_open(book, &Store)`, `apply_open_recalc` |
| `file_form.rs` | `FileForm`, `to_file_formula`, `strip_singles`, `has_call`, `array_in_value` |
| `save.rs` | `ENVIRONMENT_FUNCTIONS`, `SaveFlags`, `save_flags`, `unsure_reasons`, `SaveOptions`, `SavePrep`, `prepare_for_save(engine, &mut Store, opts)`; `EngineNames` (the engine's `nameIsMulti`/`isName`) |
| engine (`engine/mod.rs`, `host.rs`, `recalc.rs`, `graph.rs`) | `fl::EXEC`, `Graph::exec` (per (tree, sheet): the dynamic tree + implicit positions) and `alt_text` (resolved `_xll.` texts), `Engine::exec_of` at load and `set_cell` (kind normal only, as `analyze`), `Graph::exec_node` (`f.exec ?? f.node`) for the evaluation, `formula_node` (the database criteria) and the formula texts; `parse_at` resolves `_xll.` before hash-consing; `Engine::formula_text` (+ `FORMULATEXT`) and `Engine::formula_display` |
| re-pointed | `criteria::locale_error_code` → `i18n::canonical_error` (A1-6); INDIRECT's R1C1 letters → `i18n::get_language(id).r1c1` (A2-7) |

**Public API for the other frontiers:**

- **C3a-8 (grid entry, uo-xlsx `FormulaSyntax`)**: `Translator::from_local_into(text, &SyntaxOptions { lang: LangSpec::Id(ui id), r1c1, file_prefixes: true, in_table, names: Some(&NameSet::new(workbook names)), regional_decimal: host hook, .. }, &mut out)` gives the OOXML text the TS `fromLocal` gives (`_xlfn.`, `_xlfn._xlws.`, `_xleta.`, `_xlpm.`/`_xlop.` LET/LAMBDA parameters, `Tabela[[#This Row],…]`, R1C1 in the language's letters, `_xludf.` on an English name typed in pt-BR); `to_local_into` the formula bar text; `catalogue::catalog_entry`/`function_kind`/`accepts_arg_count` the reserved-function and argument-count checks (`argStep`, `smallCounts`); `resolve_syntax` the separators the UI shows. A translation allocates nothing but its output once the `Translator` is warm (measured below).
- **C1b (surgical save)**: `save::prepare_for_save(&mut engine, &mut store, &mut SaveOptions { dynamic_cms, dynamic_cm: Some(&mut || next_cm), sheets })` after the values are written (the engine's one write path already did): edits the store's formula table (`ca`/`aca`, `t`/`ref` of dynamic anchors, the file form of formulas typed in the session — legacy when it computes the same, else `t="array"` + `cm` —, orphaned shared children made own formulas) and cell extras (`cm`, error `vm` dropped); returns `touched` sheets (rewrite their `<sheetData>`) and `sure` (`false` → `fullCalcOnLoad="1"`, `unsure` lists why). `save_flags` and `unsure_reasons` are public for diagnostics.
- **C1a/C4 (open)**: `open_recalc::should_recalc_on_open(&OpenRecalcWorkbook { app_name_xl, calc_pr }, &store)` → the plan; `apply_open_recalc(&mut engine, &mut store, &plan, clock, &mut random)` (engine built WITHOUT `ignore_cache`). The decision reads the workbook flags first and then only the formula tables (never every cell; file order for `FileOrder` sheets, row-major otherwise — the plan's cells come in file order as the TS).
- **C5 (editing)**: `Engine::formula_text`/`formula_display` (the legacy `@` shown at its text position, resolved add-ins), `catalogue::function_names(lang)` for the autocomplete (to be sorted by the shell), `i18n::localize_error`/`localize_bool`, `file_form::to_file_formula`.

**Decisions taken in A5 (declared; observable output equal unless said):**

| Id | Decision |
|---|---|
| A5-1 | **Rewrite once per tree**: the legacy rewrite runs at load/`set_cell` once per (hash-consed tree, sheet) — the TS `execCache` keyed by node object; trees are per sheet in both — and a record carries `fl::EXEC` only when the rewrite inserted `@`; evaluation looks the exec tree up only for those (one hash lookup). The rewrite builds nodes only along changed paths (the TS copies every visited node and returns the original when nothing was inserted: the same trees) on an explicit task stack; `multi`/`hasRange`/`hasAnchor` are iterative through operators and parentheses and recurse only through function arguments (≤ `MAX_AST_DEPTH`). The implicit positions live beside the exec tree (`Graph::exec`), not in the node (`implicitAt`): `sourceText` and the calc-tests replay read them from there. |
| A5-2 | **Static data generated** (`gen-i18n-tables.mjs`): the run-time maps of `langData` (folded name → canonical with the diacritic-free keys inserted in the TS Map order, `errIn` sorted longest first, `kwIn` last-wins) are built in Node with the TS semantics and searched by binary search here; a typed word is folded (root upper case = `toLocaleUpperCase('pt-BR'/'en-US')`, C1) into a reused UTF-8 buffer; `stripMarks` (NFD, drop `\p{M}`, NFC) only on a miss of a non-ASCII word. `IMPLICIT_CLASSES` keeps its non-catalogue names (EUROCONVERT: reached when a defined name is called with that name). |
| A5-3 | **Translator shapes (PERF)**: tokens are spans into the input (a quoted name keeps a span + an unescape flag; an R1C1 typed in A1 mode keeps the column number and the row digits), structured-reference items live in one reused vector, `rewriteOperators` runs on an explicit stack with the matching parentheses precomputed (`@` pushed when its frame opens: no insertion), `scopeParams` keeps the parameter names of the open LET/LAMBDA frames in one buffer. `matchBracket` is memoized after the first failed scan of a text (one stack pass matches every `[` it visits), so a run of unbalanced `[` is linear (the TS is quadratic). Hostile inputs only: an EMPTY list/array separator never matches (the TS loops for ever on `startsWith('')`); the case-insensitive R1C1 letters compare by simple case folding through the root lower-case mapping (+ `ſ` → `s`) — the `iu` rule for every letter the tables hold (R, C, L). |
| A5-4 | **Host hook C1**: an id without a table resolves to English with the decimal the shell's `Intl` gives (`SyntaxOptions::regional_decimal` = `HostHooks::regional_decimal`); `value_locale_for` answers only the measured ids. The captures record the capture machine's `Intl` (Node 24.15, default locale pt-BR: an unknown id such as `xx-YY` gets `,`). |
| A5-5 | **Open-recalc over the store**: the TS walks `ws.rows`/`row.cells` of the sheet-core model; here the formula tables (file order through `FileOrder`, shadowed duplicates included; row-major for canonical sheets, whose file order it is). "Cached value" = the `<v>` bit or an `<is>` (the extra's inline shape). The volatile test is a hand scan of `\b(?:RAND|…|INFO)\s*\(` (ASCII `\b`/`i`, JS `\s`) and parses only texts that pass it, cached per text. A text whose parse ends in the TS `TypeError` (an unclosed `(` at the end, A2-4) — which the TS lets escape and abort the whole open — counts as not parsable (full recalc). |
| A5-6 | **Save over the store** (decision S5): the rules edit the store's `FileFormula` (text, attributes) and the cell extra (`cm`, `vm`) where the TS edits the model's `SyncCell`; `allFormulas`/`flags.get(f)` by record in sheet order; `dependentsOf` through `Engine::dependents_of` (positions); the shared anchors of a sheet are collected once per sheet (the TS rebuilds its cell index per call). `to_file_formula`'s probe trees are rolled back from the engine arena (no growth per save). |
| A5-7 | `_xll.` resolution happens in `parse_at` before hash-consing (resolved node: no prefix, no position, as the TS object literal); the printed text (`engine.ts:301`) is a per-record side text (`alt_text`) — the store keeps the file's text (the save writes what was read; the TS rec.text is engine-only too). |
| A5-8 | `writeValues` (`to-sheet-core.ts`) is not ported: S5 made the engine's write path the store write (the TS bridge copy does not exist); its only TS callers are the calc tests and the old I/O lab. `testing.ts` is the A4a harness (`tests/support/fnmock.rs`). `deriveValueLocale` and the `functionNames` collator stay in the shell (C1). |
| A5-9 | **Harness**: the A4a bank deferral of legacy `@` cases and the A3 replay's `legacy` facet/edit-text deferral are gone (`fnmock::legacy_deferred` → false, `a3.rs` `maybe_legacy` → false); a cell whose rewrite reaches `SINGLE` was deferred as a body not ported until A4e. `calc_order_ops` allows one `circular` deferral while `SINGLE` is missing (ops/0027: `=Local&$D1` in D1, a cycle the TS closes inside SINGLE's cell read). |

**Parity:**

| Group | Input | Result |
|---|---|---|
| **calc-tests** A5 calls (`a5_calc_tests`) | toLocal/fromLocal (lang as id or table, decimal/list, r1c1, filePrefixes, inTable, names; ids without a table), localizeBool/Error, canonicalError, local/canonicalFunctionName, resolveSyntax, getLanguage, languages, catalog, catalogEntry, functionKind, acceptsArgCount, withFilePrefix, functionNames (as a set: the order is the shell's), valueLocaleFor (measured ids), legacyToDynamic (tree + `implicitAt` positions), toFileFormula, hasVolatileFunction, shouldRecalcOnOpen (store built from the sheet-core model) | CI **3,928 equal, 0 mismatches**; big (`UO_A5_BIG=1`) **14,333 equal, 0 mismatches**. Not replayable: 5 `toFileFormula` calls with JS callbacks (the `a5` group covers them with a deterministic host), `deriveValueLocale` (shell), the engine-object calls (`prepareForSave`/`saveFlags`/`writeValues`/`applyOpenRecalc`/`unsureReasons` take the TS engine and the sheet-core model: ported as assertions below) |
| **a5** (new, `js/capture-a5.mjs`, committed `tests/captures/a5.capture.gz` 644 KiB, 1.6 s capture, deterministic) | every catalogue name both ways (+ file prefixes, lower case, without diacritics), every error/boolean, 14 option sets × the grammar (R1C1 both letter sets, structured references, LET/LAMBDA/`_xlpm`/`_xlop`, `_xleta`, SINGLE/ANCHORARRAY/`_TRO_*`, quoted/3D/external sheets, names, `_xludf`, line breaks), pt-BR typed texts, seeded mutations; the legacy rewrite + file form + volatile test of the bank's and the generated workbooks' formulas and of every catalogue function over 11 argument shapes (deterministic host rule in the meta line); `_xll.` resolution in 3 languages | CI **142,102 records equal, 0 mismatches** (tl 21,051, fl 99,812, cf 6,420, ln 3,210, ce 144, lg 3,807, ff 3,807, vo 3,824, ad 27); big (`--big`, 1.1 MiB, local) **184,640 equal, 0 mismatches** |
| **Excel oracle** (`implicit_classes_match_excel`) | `sheet/packages/calc/test/oracle/implicit.f2.json`: 4,6 thousand legacy → `Formula2` pairs measured in Excel 365 pt-BR for every catalogue function + the 13 hand pairs of `implicit.test.ts` | **all equal** (the 508 classes, OVERRIDES, ARRAY_RESULT, NO_ARRAY_PROPAGATION, needsAt) |
| **open-recalc plans of uo-xlsx** (`open_recalc_plans_of_xlsx_captures`) | the `plan=` facet of `sheet-x1` (30) + `sheet-samples` (5 with a `wb` sidecar), the decision rebuilt from the `wb` sidecar and the store group's file-order cells (`in<i>`; `st<i>` otherwise) | **35/35 equal** (20 full, 6 none, 9 partial with the exact cell count). C1b's end-to-end replay through the real reader checks the same facet |
| **bank legacy `@`** (A4a's 57 CI / 114 big deferred cases) | `tests/a4.rs` (formula cases through the engine) | final run (A4e's SINGLE now in the crate): **57/57 compared and equal**; the whole CI bank **35,295/35,295 equal, 0 deferred** |
| **calc-order ops** (A3's 2,915 deferred values: legacy `@` of sheet-scoped names) | `tests/a3.rs` | final run: **132,021/132,021 values equal (A3: 129,106 + 2,915 deferred), 0 deferred, `circular` 40/40, order/random 560/560** |
| calc-order x1 | `tests/a3.rs` | final run: **99,616 values equal, 0 deferred, order/random 420/420, `circular` 30/30, incremental-vs-full 30/30**. With A5 switched off (scratch copy) the same group had 1,663 mismatches (the legacy formulas evaluated without `@`). During the session 128 mismatches on random values of the full recalc came and went with A4e's in-progress code (identical with A5 off and with the pre-A5 harness) |
| engine-level TS tests (ported assertions, `engine_tests`) | `engine.test.ts` legacy, `addin.test.ts`, `open-recalc.test.ts` (partial/full apply), `save.test.ts` (ca/aca/cm; unsure; orphaned shared child; typed formula's file form) | equal; the values that need SINGLE (A4e) or ISEVEN (A4f) are asserted when their body is present |

**Robustness and memory** (`tests/a5_robust.rs`): hostile texts both ways and in R1C1/file-prefix mode — 100,000 nested parentheses, 100,000 unary minus, a 100,000-term chain, 20,000 nested `_xlfn.SINGLE(`, 2,000 nested `_TRO_ALL(`, a 20,000-parameter LET, a 100,000-argument SUM, 100,000 unbalanced `[`, unterminated quotes, a 100,000-element array, 25,000 errors — and 20,000 random UTF-16 texts of formula tokens and lone surrogates: no panic, worst random text < 50 ms debug, every hostile text within the bound (the `[` run took 79 s before A5-3's memo); the rewrite and the file form over 100,000-level parentheses, a 100,000-term range chain, 500 nested functions: bounded, no recursion. **M1** (after a warm-up of the lazily built static tables): translate both ways, rewrite + file form, the open-recalc decision, engine build + apply + formula texts, a second `prepare_for_save` (idempotent: touches nothing): live bytes back exactly. **Zero allocation**: 2,000 warm translations allocate 0 bytes beyond the reused output (`benches/a5.rs`). Caches: none new (the TS `LANG_CACHE` is static data; `Graph::exec`/`alt_text` are per-tree/per-record data dropped with the engine; the open-recalc text cache lives for one call).

**Performance** (`benches/a5.rs` vs `js/bench-a5.mjs`, the frozen TS in Node 24.15 on the same inputs, i5-14600KF, medians of 7; Rust native release):

| Operation | Rust | TS | Rust / TS |
|---|---|---|---|
| toLocal pt-BR, 100,000 formulas (15 templates), ns/formula | **700** | 3,083 | **0.23** |
| fromLocal pt-BR, 100,000 formulas, ns/formula | **577** | 6,090 | **0.095** |
| legacy rewrite + screen print, per tree (ns) | **643** | 1,162 | **0.55** |
| open-recalc decision, giant-1m (no formula), ms | **0.00** | 10.8 | — |
| open-recalc decision, 100,000 formulas → none, ms | **6.2** | 17.5 | **0.35** |
| open-recalc decision, 100,000 formulas + `NOW()` → full, ms | **6.4** | 16.6 | **0.38** |

Rework found by the benchmark: the first translator allocated per name token (upper case of LET/LAMBDA names) and per call (`getLanguage` lower-casing the id, the boolean folds) — 270 B per translation, toLocal 0.54× — now 0 B and 0.23×; the first rewrite allocated its stacks and an argument vector per function node (1.40× the TS, a failure) → reusable `LegacyBufs` and in-place argument reads (0.55×). These are the first A5 records (the 10% rule starts from them).

**Fuzz**: no fuzz file edited (`fuzz/src/calc.rs` is not this frontier's); seeds proposed for the orchestrator, for `formula_parse` (they also exercise the engine's rewrite through its evaluation in both contexts) and for a new target `formula_i18n` (bytes → option byte + UTF-16 text → `to_local`/`from_local` in pt-BR/en-US × A1/R1C1 × file prefixes, then `legacy_to_dynamic` + `to_file_formula` of the parsed text): `=SOMA(A1:A3;1,5)`, `={1\2;3\4}`, `=LAMBDA(taxa;taxa*2)(3)`, `=_xlfn.LET(_xlpm.x,A1:A3,_xlpm.x)`, `=Tabela1[[#Esta Linha];[Qtd]]`, `=_xlfn.SINGLE(_xlfn._TRO_ALL(A1:B5))`, `=L[1]C[-1]+R1C1`, `=_xll.ÉPAR(A1:A3)`, `=IF(A1#>1,A1:A3,INDEX(B1:B3,0))`, `=XLOOKUP(1,A1:A3,B1:C3)`, `=T[[[[`, `='a''b'!A1:B2 C1:C3`.

**Open items**: (1) SINGLE's body (A4e, `reference.ts`) landed in the crate during this session: the legacy cells are compared now (above); `calc_order_ops` still tolerates one `circular` deferral (A5-9) for the window where SINGLE was missing — it can be tightened back to 0. (2) `tests/a4f.rs` `zero_alloc_scalar_paths` (A4f's) now fails: its legacy formulas `SWITCH(…)`, `IFS(…)`, `STOCKHISTORY(…)` become `@SWITCH(…)` etc. exactly as the TS rewrites them (classes `top`), and evaluating the `SINGLE` call adds a constant ≈ 3.5 KB per recalc (not per formula; measured: 112 B → 3,512 B for 200 formulas, 3,903 B for 2,000) — the A4f/A4e owners should either type those formulas as dynamic in the test or find the per-pass allocation of the nested call. (3) `tests/a4e.rs` `lookups_allocate_per_pass_not_per_formula` failed during the session on A4e's in-progress code (XLOOKUP gets no `@`: not an A5 effect). (4) The crate's clippy currently fails only on A4e's in-progress `functions/lookup/{conditional,dynamic}.rs` and A4d's `tests/a4d.rs` (`str::trim`); every A5 file is clean native and wasm32 (checked in a scratch copy with the lookup family stubbed). (5) `writeValues` replay (A5-8) and the engine-object calc-tests calls (24 engines) remain the A3 open item 2 — the A5 ones are covered by the ported assertions. (6) The translator's `_TRO_` colon search is quadratic in the nesting depth of `_TRO_` calls (as the TS); bounded in the hostile test (2,000 levels).

## Frontiers (closed scope per frontier; all Opus)

| # | Scope | Delivers | Captures | Lines | Needs |
|---|---|---|---|---|---|
| **S0** (**done** 2026-11-24, section "S0 — the store, as built") | `key`, `strings`, `store`, `formula` (file side), `cellValue`/`fileCellValue`/`numberLiteral` | the store API: get/set/delete, kind/payload/style lanes with widening, area iteration row-major and column runs, viewport gather, string pool with refcounts and compaction, SST lazy decode hooks, extras, `FileOrder`, row meta, default style, the **store facet** (canonical JSON of every cell, below) | store facet of the TS model for every committed sample + generated workbooks (via a TS adapter that turns the sheet-core model into the facet) | ≈ 600 TS-equivalent + new | — |
| A1 (**done** 2026-11-24, section "A1 — as built") | values/coerce/collate (already in `uo-text`)/criteria/dates/stats/special/libm/math-util | as plan §5 A1; the context walkers and `FnDef` factories go to A2/A4 (A1-7) | the new `a1` group (every A1 entry point, traps + seeded) and the A1 calls of `calc-tests`; `calc-traps`/bank enter through the evaluator (A2+) | ≈ 1,900 | — |
| A2 (**done** 2026-11-24, section "A2 — as built") | ast/parse/print/a1/evaluate/apply/ops/fn | `parse`/`print` with a depth limit and an explicit stack, `trait EvalHost` (the TS interface, `evaluate.ts:34-61`); as built: no recursion through operators anywhere (A2-2), the registry table generated from the TS, bodies hooked for A4 | the new `a2` group (parse/print/round trip, evaluation over a mock workbook, ops, a1, applyFn) and the A2 calls of `calc-tests` | ≈ 1,460 | A1 |
| A3 (**done** 2026-11-24, section "A3 — as built") | engine (graph, full/incremental recalc, `NeedCell` as `Result`, spills, CSE, data tables skip, cycles/iteration, volatiles) on S0 | `Engine` over `&mut Store` | per-cell values + evaluation-order facet over generated workbooks, incremental = full, `calc-tests` | ≈ 900 | S0, A2 |
| A4a (**done** 2026-11-24, section "A4a — as built") | the shared function layer (factories, walkers of math-util/stats/tdl-util, basic aggs, dispatch per family) + math, math-util, libm, complex, engineering | functions | bank + traps (replay `tests/a4.rs`, every family) | 2,000 | A3 |
| A4b (**done** 2026-11-24, section "A4b — as built") | statistical, distributions, regression, ets | functions (145 bodies) | bank + traps | 1,628 | A3 |
| A4c (**done** 2026-11-24, section "A4c — as built") | financial, daycount, datetime | functions (80 bodies) | bank + traps | 1,390 | A3 |
| A4d (**done** 2026-11-24, section "A4d — as built") | text, text-format (via `uo-numfmt`), regex (`regress`, R1), filterxml (XPath subset), regional, tdl-util | functions (62 bodies) | bank + traps (non-ASCII, surrogates) | 1,701 | A3 |
| A4e (**done** 2026-11-24, section "A4e — as built") | lookup, reference, dynamic, database, conditional | functions (72 bodies; stable sorts: `uo_jsnum::sort`), the pass cache and `criterionAt` hooks | bank + traps + calc-order | 1,925 | A3 |
| A4f (**done** 2026-11-24, section "A4f — as built") | logical, info, cube (ISREF/ISFORMULA/CELL/INFO/SHEET(S)/FORMULATEXT are `reference.ts`: A4e) | functions (44 bodies) | bank + traps | 467 | A3 |
| A5 (**done** 2026-11-24, section "A5 — as built") | implicit (+ 508 measured classes), i18n (pt-BR/en-US tables, C1 hook), open-recalc, save/file-form, addin, the catalogue; `writeValues` not ported (A5-8), `testing.ts` = A4a's harness | the legacy `@` in the engine (`execCache`, `sourceText`, FORMULATEXT), `Translator`, `catalogue`, `should_recalc_on_open`, `prepare_for_save` over the store | calc-tests (3,928 / big 14,333), the new `a5` group (142,102 / big 184,640), the Excel `implicit.f2.json` pairs, the uo-xlsx open-recalc plans (35) | ≈ 2,800 | A3 |

Order: (S0 ∥ A1) → A2 → A3 → (A4a…A4f, two at a time) → A5. A4 families are independent after A3; the track A
agent runs them serially or in pairs inside the ≤ 3-agent cap. C1a (`uo-xlsx`) needs only S0.

## Parity (capture points; plan §4.3 adjusted)

| Capture | TS entry | Compared |
|---|---|---|
| **Bank** | the 35,299 oracle cases (`calc/test/oracle/*.cases.json`, 71 files) through `evaluate` | bit-exact vs TS (777 known TS≠Excel, 47 Excel errors stay "known") |
| **calc-traps** (seeded) | every function × parameter × trap value (plan §4.3) | bit-exact |
| **calc-tests** | the 19 test files with `evaluate`/recalc/parse/print/`apply`/`writeValues` wrapped | bit-exact |
| **Store facet** | sheet-core model (+ the index rule: last duplicate wins, row from `<row r>`) | canonical JSON per cell, keys sorted: `{c, cm, f, fa, is, r, ref (only when not the canonical A1), s, t, v, vm}` (as built in S0: the model has no `owned`, S0-12); numbers by `uo_jsnum` `String(n)`; + `fo` (file order), `rw` (row attributes), `cv` (`cellValue` per cell) |
| **Per-cell values** | `Engine` after `recalc`/`calculate`/`applyOpenRecalc` on generated workbooks | `Scalar` by bits per cell |
| **Evaluation order** (new facet) | `evaluateRec` wrapped: the sequence of (sheet,row,col) per run, `circular` as a sorted list, `internalErrors` | exact — this is what catches the order sites of S7 |
| **Incremental = full** | edits on generated workbooks, `calculate` vs a fresh `recalc` | per cell |
| **Save attributes** | `prepareForSave` + `setDynamicAttrs` + `writeScalar` | the cell facet after save |

Captures use `RandomSource::Sequence` and a fixed `Clock`; the generated workbooks are also recalculated by Excel
(`xl-calc.ps1`, plan §0.2) for the agreement number (not lower than the TS's).

## Captures (harnesses built 2026-11-24, plan §4.3)

Scripts in `core/crates/uo-calc/js/` (Node only, no Office, no browser), capture files in the `uo-testkit` format
(`src/capture.rs`) under `core/crates/uo-calc/tests/captures/` (committed CI subsets) or `core/target/sheet-cap/`
(large local runs, gitignored). Every file is stamped with the frozen TS commit (`@ts-root-commit`, `@ts-root-calc
clean|dirty`), `@tz UTC`, `@now-serial 45658.5` and the platform line; values are compared through one canonical
text (`lib/common.mjs` `makeValueText`: `n<f64 bits>`, `s<JSON>`, `b1/b0`, `e<code>`, `a<r>x<c>[…]`, `r<areas>`,
`_`). Random values come from a seeded xorshift32 and are recorded in call order (`RandomSource::Sequence`).

| Group | Command (CI subset; local run) | Committed | Covers |
|---|---|---|---|
| **bank** | `node core/crates/uo-calc/js/capture-bank.mjs`; local `--locales pt-BR,en-US --out core/target/sheet-cap/bank-big.capture` | `bank.capture` 2.9 MiB + `bank.values.gz` (values over 512 units), 35,299 records, 0.9 s | `runCase(case, PT_BR)` of `testing.ts` (the TS oracle test's entry: `callFn` over `mockContext`, or parse + `Engine.recalc` for whole-formula cases) over the 27 `*.cases.json` of `sheet/packages/calc/test/oracle` (read from this worktree; `@oracle-digest` stamps them, `in` per case). Status per case: 34,477 match, **775 known** (`*.known.json`, reason digest in `why`; + 2 in `locale-pt-BR.known.json` = the 777, i18n tables replayed by calc-tests), **47 xlerr** (Excel could not answer), 0 diverge, 0 throw; the Excel answer is kept for every non-match. The local run adds every case in en-US (TS results only) |
| **calc-tests** | `node core/crates/uo-calc/js/capture-tests.mjs`; local `--big` (no cap) | `calc-tests.capture` 1.3 MiB + `calc-tests.calls.gz` 268 KiB (the full calls), 10,866 calls | the 16 `calc/test/*.test.ts` except `oracle.test.ts` (the bank replays it) + `sheet/test/{sheet-calc,style-pool,entry}.test.ts` = 19 files, 152 tests, all passing under the recorder. Every `export function` of `calc/src` (248 sites: evaluate, parse/print, applyFn/callFn, writeValues, fromSheetCore, open-recalc, save, file-form, i18n…) and the public methods of `Engine`, `SheetCalc`, `XfPool` are wrapped in memory by a Vite plugin (`calc-tests/`; the checkout is never written); only outermost calls are kept, with canonical arguments, result or exception, receiver id and the `Math.random` values consumed. Clock frozen, `Math.random` seeded per file. Filtered: 3,186 registration calls at module load; CI keeps ≤ 200 pure calls per (test, function) by stride (41,665 dropped, all kept by `--big`) |
| **calc-traps** | `node --max-old-space-size=8192 core/crates/uo-calc/js/capture-traps.mjs` (≈ 8 min); local `--big` (41 min) | `calc-traps.capture` 1.1 MiB, 535 functions, 54,120 calls | per function, a seeded baseline from the bank (the plain-argument case with the most arguments) and each position 0..3 replaced by 31 CORE traps (NaN, -0, ±∞, 1e21, 1e-7, 2^53+2, 17 digits, denormal, empty, lone surrogate, astral, combining, non-ASCII, Turkish İı, pt-BR/en-US text-numbers, TRUE, empty, omitted, #N/A/#DIV/0!/#VALUE!, empty/1×1/union/whole-column ranges, 2×2 and ragged arrays) in pt-BR, plus the text traps in en-US. `--big`: 98 traps (every error code, more digit counts, subnormals, white space, wildcards, regex text, dates, booleans as text, rows/blocks), positions 0..5, both locales, 24 seeded pairs per function and locale. The trap table is in the file (`@trap.*`). Calls run in a child process with a 5 s watchdog per call; a call that does not return is `timeout:5000ms`, one that kills the process (V8 fatal) `crash:<exit>`, and the child restarts at the next call |
| **calc-order** | `node core/crates/uo-calc/js/capture-order.mjs`; local `--gen x1-big --ops 400 --out core/target/sheet-cap/calc-order-big.capture` (61 s) | `calc-order.capture` 172 KiB + `.side.gz` 975 KiB, 30 x1 + 40 ops workbooks (A3 added the `ops` group, the `legacy` and `opts` facets and the `full.vals` sidecar lines; section "A3 — as built") | the evaluation-order facet of §16 item 4: the generated workbooks (uo-xlsx `gen-workbook.mjs` group x1, read by sheet-core's dist, `fromSheetCore` → the `CalcWorkbook`, kept in the sidecar so A3 can replay before C1a exists) through `new Engine` + `recalc()`, a `calculate()`, 12 seeded `setCell` + `calculate()` edits, and a fresh full recalc of the final inputs: per run the `evaluateRec` sequence (13,970 evaluations in x1), the RAND values (1,570), `circular` sorted (13 workbooks with cycles), `internalErrors` (0), all values, and incremental vs full (equal in 6, differ in 24: volatiles and cycles — recorded, not judged). The x1 workbooks put ranges wider than the formula columns next to spills/CSE (the `fc.keys()` and `rangeMasters` sites of S7) |
| **a1** (A1) | `node core/crates/uo-calc/js/capture-a1.mjs`; local `--big --out core/target/sheet-cap/a1-big.capture.gz` (19 s) | `a1.capture.gz` 521 KiB, 52,402 calls (big: 486,022) | every A1 entry point of the calc dist called directly: traps + seeded inputs in four locales (en-US, pt-BR, a de-DE host locale, an empty-group locale), clock frozen at 2025-01-15 12:00 UTC; records `<fn>#<i>` with `in` (value texts) and `out`; replayed by `tests/a1.rs` (section "A1 — as built") |
| **a2** (A2) | `node core/crates/uo-calc/js/capture-a2.mjs`; local `--big --out core/target/sheet-cap/a2-big.capture.gz` (14 s) | `a2.capture.gz` 1.1 MiB, 52,948 records (big: 251,407) | parse (`canon` of the AST = the calc-tests recorder's JSON, or `throw:<Class>:<pos>`), six prints and the round trip of the bank's, the generated workbooks' and seeded/mutated formulas under eight option sets; `Evaluator.evalFormula` over a mock host described in `@mock` (function bodies replaced by a marker: `A4` where the TS called one); `ops.ts` over 42 trap values in two locales; `a1.ts`; `numberLiteralValue`; `applyFn` over synthetic definitions with the body's call log. Replayed by `tests/a2.rs` |
| **store** (S0) | `node core/crates/uo-calc/js/capture-store.mjs`; local `--big --out core/target/sheet-cap/store-big.capture` (47 s) | `store.capture` 19 KiB + `store.side.gz` 146 KiB, 35 workbooks + 1,750 traps (0.2 s) | the TS adapter of the store facet (`storeOf` rule of uo-xlsx `capture-sheet.mjs`): per sheet the model cells in FILE order (`in<i>`, the test-only loader's input), `st<i>`, `fo<i>`, `rw<i>`, `cellValue` per cell (`cv<i>`), the SST texts; samples (giants only in `--big`) + x1 (`--big`: + x1-big); per-parameter traps of `numberLiteral`/`numberText`/`fileCellValue`/`errorOf`/`cellValue`. Replayed by `tests/parity.rs` (section "S0 — the store, as built") |

TS findings the traps recorded (kept as TS behaviour; the Rust port must reproduce or the record must declare):
23 calls did not return within 5 s (`timeout:5000ms` in the capture, `@timeouts`): `COMBINA(n, ∞)` (the
`binomial` loop runs to `k = ∞`), `BESSELJ/I/K(x, ∞ | 1e21 | 2^53+2)`, `COUPDAYSNC`/`COUPNCD` with `NaN` settlement or
maturity and with maturity `∞ | 1e21 | 2^53+2`, `ODDFPRICE` with `-∞` issue, `FORECAST.ETS.CONFINT` with target date
`1e21 | 2^53+2`. 27 calls threw out of `callFn` (inside the engine a `RangeError` becomes `#NUM!` and any other
exception `#VALUE!` + `internalErrors`, `engine.ts:903-910`): `RangeError` "Invalid array length" (`SEQUENCE`,
`EXPAND`, `TAKE`, `MUNIT`, `WRAPROWS/COLS` with `NaN`), "cannot be converted to a BigInt" (`BITRSHIFT(·, NaN)`),
"Invalid code point" (`UNICHAR(NaN)`), "Maximum call stack size exceeded" (a whole column spread into a call:
`TEXTJOIN`, `TEXTSPLIT`, `TEXTBEFORE/AFTER`, `WORKDAY(.INTL)`, `NETWORKDAYS(.INTL)` holidays — V8 stack dependent),
`TypeError` (`ISEVEN/ISODD` of an empty/omitted argument: a `Symbol` reaches a string conversion; `FVSCHEDULE`/`XIRR`
with an array literal holding text).

Local runs of 2026-11-24 (`core/target/sheet-cap`, not committed): bank in pt-BR + en-US 5.6 MiB (1.2 s);
calc-tests `--big` 52,531 calls, 6.2 MiB + 1.0 MiB calls (17 s); calc-order over x1-big (400 workbooks) +
calc-heavy-20k: 1,049,214 evaluations, 51,024 random values, 209 workbooks with cycles, 1.0 MiB + 51 MiB sidecar
(55 s); calc-traps `--big` 308,120 calls in 41 min, 6.6 MiB: 102 threw, 190 timed out (COUPNCD 31, BESSELK 31, COUPDAYSNC 30, BESSELJ 27, FORECAST.ETS.CONFINT 26, BESSELI 26, ODDFPRICE 6, MUNIT 6, COMBINA 3, BESSELY 2, BAHTTEXT 2), 0 crashes (with worker threads instead of child processes an earlier attempt died of a V8 fatal "invalid table size" in the worker, which aborts the whole process).

What these captures cannot catch (passing = equal on what was captured): the TS's own bugs (the bank's 775 + 2 known
divergences stay "known", and calc-traps/calc-tests record TS behaviour, not Excel's — the interesting TS≠Excel
cases go to `xl-calc.ps1` in one batch when COM is allowed); results that depend on `Intl` outside pt-BR/en-US (C1
hook: only the two measured tables are exercised); the `functionNames` collator order (shell); timing and memory
(the benchmarks and M1–M5 cover them); V8-vs-Rust differences on inputs no group generates (the traps are per
parameter, pairs only in the local run); thunk arguments in calc-tests are evaluated once at record time (a lazy
argument with a side effect — `RAND` inside a thunk — is recorded with that call's value).

## JS semantics that are traps (helper to use)

| TS construct (site) | Trap | Helper |
|---|---|---|
| `Number(c.value)` (`from-sheet-core.ts:86-90`), `parseInt`/`parseFloat` of attributes (`read.ts:575-584, 603, 616-617`) | white space, `0x`, `Infinity`, trailing garbage, `NaN` | `uo_jsnum::parse`, `conv` |
| `String(v)`, `numberLiteral` (`print.ts:24-27`: `E+`), `numberText` (`to-sheet-core.ts:47-52`: `E`, `-0` → `0`) | shortest digits, `1e21`, `-0` | `uo_jsnum::format`; the save prints from `f64` unless `CellExtra::raw_value` holds the file text |
| `-0` in cells (`Object.is(v, -0)`) | `-0 === 0`; `rec.v !== v` (`engine.ts:927`) treats `0`/`-0` as equal, NaN as always changed | the store compares with the JS `!==` semantics, not bits, when deciding "changed" |
| NaN as a value | `NaN !== NaN` → a NaN result is always "changed" | same rule (`Changes` counts it) |
| `Map`/`Set` insertion order | S7 | ordered `Vec`s |
| `Array.prototype.sort` (`allFormulas` 608, `iterateCycles` 1031, `each` 158, `rowsOf` 116) | stable; numeric comparators | `uo_jsnum::sort` (stable) |
| `r * W + c`, `Math.floor(k / W)` (`engine.ts:32, 948`) | exact below 2^53 | `CellKey` |
| `new Date()`, `getTimezoneOffset`, `Math.random` (`engine.ts:186-189, 503`, `coerce.ts:197`) | host clock/zone/randomness | `Clock`, `RandomSource` |
| `toLocaleUpperCase(lang)` (`translate.ts:61`) | locale tailoring | `uo_text::case` (root = pt/en/es) |
| `/_xll\./i`, `/^_xlnm\./i` and the parser's regexes | ASCII-only `i` | hand matchers on code units (`uo-numfmt` D11 pattern) |
| `RegExp` built from a user pattern (`functions/regex.ts`) | PCRE2 → JS translation, catastrophic backtracking | `regress` + step budget (R1) |
| `[...text]`, `.length`, `charCodeAt` in text functions | code points vs UTF-16 units | `JsStr` (code units) + `uo_text::utf16` |
| deep recursion (`parseFormula`, `walk`, `evalNode`) | JS stack overflow = `RangeError` → `#NUM!` (`engine.ts:908`) | explicit stacks + depth limits; a limit hit gives `#NUM!` like the TS `RangeError` path (declared where the depth differs) |
| `Math.max(...rows.map(…))` (`values.ts:77`) | `RangeError` past ~10^5 arguments | loop (declared difference on hostile arrays) |

## Memory plan (M1–M5)

**M5 — every cache with a cap:**

| Cache (TS site) | Rust | Cap |
|---|---|---|
| engine `cache` (`engine.ts:220`; MATCH/VLOOKUP indexes, criteria; cleared per pass) | `Engine::pass_cache` | 64 MiB wasm32 / 256 MiB native per pass; over the cap an entry is not stored (recomputed: same values) |
| `execCache` (`engine.ts:327`) | `exec` id in the AST arena | bounded by formulas |
| `volatileNames` (`engine.ts:356`) | engine map | bounded by names × sheets |
| `rev`, `deps`, `ordered` (`engine.ts:217, 826, 607`) | engine | data of the formulas; dropped by `clear_caches` |
| temp string arena (new) | per pass | 64 MiB wasm32 / 256 MiB native, `#VALUE!` past it (declared, hostile only) |
| `numReCache` (`coerce.ts:97`) | none: A1's matchers take the separators directly (A1-1) | — |
| `charCache`, `keyCache` (`collation.ts:62, 125`) | `uo_text::collation` keeps none; uo-calc has the static Latin-1 element table (14 KiB, fixed) | fixed |
| regex `cache` (`functions/regex.ts:191`) | `functions::regex` | 256 compiled patterns, LRU |
| `fmtLocaleCache` (`text-format.ts:26`) | per `ValueLocale` | 8 |
| `LANG_CACHE` (`translate.ts:47`) | static tables | fixed |
| `open-recalc` `volatileCache` (102) | per call | dropped at return |
| `StrPool` (new) | data, not a cache | `Limits`: 2^24 entries / 512 MiB arena (wasm32 256 MiB); open fails with a typed error past it |

**M1**: every public store operation, `Engine::new`, `recalc`, `calculate`, `set_cell`, `evaluate` (one-off), parse,
print — live bytes back after `clear_caches`. **M2**: (a) store: 1,000,000 random set/clear operations over giant-1m
with string churn, sampled every 50,000: flat (pool reuse + compaction); (b) engine: generated workbook, 1,000 × (edit
→ `calculate`), flat; (c) the Sheet soak of `uo-xlsx.md` (scroll + edit on giant-1m). **M3**: the bytes/cell budgets of
S2; peak per recalc ≤ 2× the retained engine + the pass cache; recorded per frontier. **M4**: in `uo-xlsx`'s handles.

## Performance budgets and benchmarks (PERF)

Harness: `tools/bench/speed/run.mjs` gains a `sheet` suite (TS frozen dist in Node and Chromium × rust-core native and
wasm), workbooks: `giant-1m`, `giant-200k`, and generated calc classes (seeded): `chain-100k` (A2=A1+1…),
`sum-ranges-100k`, `lookup-10k×10k` (VLOOKUP/MATCH/XLOOKUP), `text-50k`, `dynamic-1k` (SORT/UNIQUE/FILTER spills),
`volatile-10k`, `cycles-iterate`, `shared-1m` (one shared formula over 1M cells).

| Benchmark (frontier) | Budget | Fail |
|---|---|---|
| store fill giant-1m from decoded lanes (S0) | ≤ 40 ms native | > 1.10× previous record |
| store bytes/cell (S0) | S2 budgets | over budget |
| column scan sum 1M / random get 1M / viewport gather 45×15 and 120×40 (S0) | ≤ 1 ms / ≤ 15 ms / ≤ 20 µs / ≤ 120 µs native | > 1.10× previous |
| parse + print the bank's formulas (A2) | ≤ 0.5× TS Node (A2: 0.47×) | > 1.0× TS |
| full recalc, each generated class (A3–A5) | native ≤ 0.5× TS Node, wasm ≤ 0.8× TS Chromium | > 1.0× TS, or > 1.10× previous |
| incremental: one edit dirtying k formulas (A3) | ≤ 16 ms wasm for k ≤ 10,000 | > 1.0× TS |
| engine build, giant-1m / shared-1m (A3) | 0 ms (no formula) / ≤ 0.3× TS | > 1.0× TS |
| per-family function micro (A4: VLOOKUP/MATCH 100k, SUMIFS, SORT/UNIQUE 100k, TEXT 100k, REGEX 10k) | ≤ 1.0× TS | > 1.0× TS |

A benchmark slower than the TS fails the frontier unless the maintainer approves it (listed in the record); a
regression > 10% against the previous rust-core record fails (M3 rule).

### TS baseline (measured 2026-11-24, the numbers to beat)

`node core/tools/bench/speed/sheet.mjs` (beside `run.mjs`: the Write runner is docx-specific; the Rust engines join
this suite per frontier), frozen TS `b92b60fa`, Node 24.15, this machine (i5-14600KF), each scenario in its own
`--expose-gc` process, 1 warm-up + 3 rounds, medians; retained = heap after GC with the result alive minus before.
Results: `core/tools/bench/speed/results/sheet-ts-2026-11-24.{json,md}`. Calc classes from
`core/crates/uo-xlsx/js/lib/gen-workbook.mjs` `CALC_CLASSES` (one sheet, no cached values).

| Class (calc dist: `fromSheetCore` + `new Engine`, then `recalc()`) | formulas | build ms | **recalc ms** | engine retained MB |
|---|---|---|---|---|
| chain-100k | 99,999 | 167 | **72.7** | 98.2 |
| sum-ranges-100k (`SUM` of a 100-row window) | 100,000 | 308 | **199** | 124 |
| lookup-10k×10k (VLOOKUP/MATCH/XLOOKUP over 10k rows) | 10,000 | 56 | **93.7** | 15.4 |
| text-50k (UPPER/LEN/SUBSTITUTE/TEXT/LEFT&RIGHT) | 50,000 | 185 | **112** | 68.9 |
| dynamic-1k (20 SORT/UNIQUE/FILTER/SEQUENCE spills over 1k rows) | 20 | 0.9 | **9.8** | 2.5 |
| volatile-10k (RAND + dependents) | 20,000 | 52 | **18.4** | 17.7 |
| cycles-iterate (1,000 two-cell cycles, iterate 100/0.001) | 2,000 | 7.0 | **42.0** | 2.3 |
| shared-1m (one shared formula over 1M cells) | 1,000,000 | 2,085 | **1,390** | 738 (RSS 3.2 GB) |
| calc-heavy-20k (mixed: chain, IF, window SUMs, lookups, text, SUMIF, spills, volatiles) | 49,205 | 198 | **677** | 89.0 |

Edit latency (the element, calc-heavy-20k open, engine built): `setActiveCellText` on the head of the 20k chain
p50 **604 ms** / p95 611 ms; on an isolated cell p50 **1.7 ms** / p95 4.1 ms (20 edits each). The open and save
baselines of the Sheet are in `uo-xlsx.md` T13.

## Security and fuzz targets

Limits (`store::Limits`): cells, sheets, formulas, formula text length, parse depth, evaluation depth (measured in
wasm, as Write's G23), spill size (`MAX_ROW`/`MAX_COL`), iteration count (≤ 32,767), array size (2^24 cells, the TS
cap at `engine.ts:465`), regex steps. Errors are typed; a function that would panic returns `#VALUE!`/`#NUM!` as the
TS `internalErrors` path (`engine.ts:909`) and is recorded.

Fuzz (`core/fuzz`): `formula_parse` (formula text → parse/print/eval; built in A2, the plan's `calc_formula`), `calc_workbook` (generated hostile workbooks:
deep chains, wide cycles, huge ranges, spills into merges/tables), `calc_regex` (pattern × text under the budget),
`store_ops` (random store operations differentially against a `BTreeMap` model: same facet, invariants: sorted chunks,
no empty chunk, lane lengths, refcounts = cell references).

## Wasm split

The calc and the store are inside `uo-sheet.wasm` (the open-recalc rule needs the engine in the viewer: plan §7 C4);
budgets in `uo-xlsx.md`. If the viewer exceeds 1.6 MB gzip, the fallback is a lazily loaded `uo-calc` part (SH3).

## Questions for the M3 gate (batched; ids shared with `uo-xlsx.md`)

- SH1: a `<c r>` whose row differs from its `<row r>` paints at the row and calculates at the ref's row in the TS
  (`universal-sheet.ts:4229-4230` vs `from-sheet-core.ts:130`): the one store uses the `<row>` position for both — accept
  (calc results change on such hostile files)?
- SH2: find/replace `toLocaleLowerCase()` with the host's default locale → root case mapping (differs on tr/az/lt
  hosts) — accept?
- SH4: performance gates: recalc ≤ 0.5× TS native / ≤ 0.8× wasm, everything else ≤ 1.0× TS, 10% regression rule —
  accept as the M3/M5 criteria?
- SH7 (found in S0): a duplicate `<row r>` element. The component's paint index REPLACES the row's whole cell map with
  the later element's (`universal-sheet.ts:4228`, `rowMap.set(r.r, cm)`), so the earlier element's cells vanish from
  the TS paint, while the calc (`fromSheetCore` + `SheetStore.set`) and the store facet keep every position, the last
  duplicate winning per cell. The one store follows the calc/facet rule for both — accept (the paint changes on such
  hostile files; the generated x1 group has them)?
- SH8 (found in S0, for C1b): a `<row>` whose last cell is deleted stays in the TS model and is saved as an empty
  `<row r="…"/>`; the store keeps a row element only when it has attributes or had no cell at load. The surgical save
  would drop such an emptied row element (Excel accepts both) — accept, or should C1b track "emptied row elements"?

## Plan corrections found in this pass

Listed in the plan's §16 with the evidence.

## Process metrics

| Stage | Agent | Wall time | Notes |
|---|---|---|---|
| Architecture pass (S, C1 hook, frontiers, budgets) | Opus | ≈ 25 min wall (00:53-01:18, shared with `uo-xlsx.md`) | TS heap measured with the frozen dists |
| Capture harnesses (plan §4.3: bank, calc-tests, calc-traps, calc-order; Sheet captures, workbook generator, `sheet` bench suite) | Opus | ≈ 1 h 50 min wall (01:15-03:07, ≈ 1 h of it the large local runs; shared with `uo-xlsx.md` T10a/T13) | no Rust code touched; one rework: the calc-traps run hung on TS inputs that never return (COMBINA(n, ∞) and 22 more), so calls moved into a watchdogged child process (a worker thread was not enough: the big run hit a V8 fatal error, which aborts the whole process) |
| S0 store (port + TS adapter + parity + memory + fuzz target + benchmarks) | Opus | ≈ 42 min wall (03:08-03:50) | 0 parity rounds (the first replay was clean: CI 337 facets, local 5,161); rework found by the verifiers built in the same session, all fixed by the same agent: (1) payload lane widened after the kinds lane changed — a string id widened as a number, caught by the differential ops test as a pool reference count ≠ holders; (2) `each` on a sheet without columns indexed `columns[0]` (ops test); (3) an emptied `BTreeMap` of extras kept its root leaf, 1,776 B (M1); (4) random get 1M over budget (16.4 ms) until `Column::find` probed the direct index (3.7 ms); (5) the ops harness's full-model check made fuzz inputs take 3 s (now every 64 ops) |
| A1 (port of values/coerce/collate entry points/criteria/dates/stats/special/libm/math-util + `a1` capture + parity + robustness + memory + benchmarks vs the TS) | Opus | ≈ 35 min wall (03:52-04:27) | 0 parity rounds: the first replay of the 52,402-call capture had only NaN-payload differences (A1-2), the big run (486,022) and the calc-tests calls (548 + 1,262) were clean; rework found by the tests and benchmarks of the same session, all fixed by the same agent: (1) `BASE_1900` constant off by one day (dates unit test); (2) a `usize` loop underflow in `sin_dd` (debug overflow, before any run); (3) Latin-1 `compareText` 2.6× slower than the TS (keys per call) → element table; (4) ASCII criterion text missed the fast path (stored as UTF-16); (5) the exponent of `numberCore` allocated (zero-allocation test); (6) 13 float → int casts routed through `cast.rs`/`js_index` for the float-casts gate |
| A2 (port of a1/ast/parse/print/fn/ops/apply/evaluate + function table generator + `a2` capture + parity + robustness + stack probes native/wasm32 + fuzz target + memory + benchmarks vs the TS) | Opus | ≈ 1 h 26 min wall (04:28-05:54) | 0 parity rounds: the first replay of the CI capture (158,153 facets), the big run (612,438) and the calc-tests calls (1,442 + 30,186) were clean, and stayed clean after the evaluator rewrite; rework found by the tests and probes of the same session, all fixed by the same agent: (1) a recursive evaluator and printer used 464 B (paren) to 1.26 KB (chained operator) of native stack per level — a 1,000-term chain overflowed a 1 MiB thread → operator work stack + iterative printer (A2-2), parser depth counted on function nesting only; (2) the union `,` was quadratic (50k areas 44 s debug) → in-place append + one-sheet flag (A2-11); (3) clippy (collapsible ifs, `f64::min/max` → `uo_jsnum::math`, 6 float casts → `cast.rs`); (4) the bench's JSON reader rejected a cases file with a trailing byte; (5) the first fuzz run stalled once, not reproduced (see A2 robustness) |
| A3 (engine: graph, templates, reverse index, scheduler, spills/CSE, cycles/iteration, edits, the one write path + `ops` capture group + parity replay + robustness/fuzz target + memory + benchmarks vs the TS) | Opus | ≈ 1 h 5 min wall (05:56-07:01) | 0 parity rounds (no port bug sent back by a verifier); rework found by the replay and tests of the same session, all fixed by the same agent: (1) shadowed duplicates read the store's `(col, FileCell)` as `(row, …)` — orphaned formulas were lost (ops order facet); (2) `circular` compared as numbers, the capture sorts strings (harness); (3) the taint closure only grew from new seeds, so new formulas reading old deferred cells were compared (harness); (4) under `ignoreCache` a cycle restored the store value, which an orphan had overwritten — the cached file value is now kept per formula; (5) a run whose order differs only in deferred cells was counted as a mismatch (harness); (6) the `Work::Operand` fast path converted the operand of a unary `+`, breaking `+(A1)#` (a2 replay, 2 records); (7) the load materialized one record per cell (213 MB peak on shared-1m → 42 MB, streamed batches); (8) `-0` lost by the robustness test's loader (test); (9) clippy (`f64::min`, an `x + 1 <= y`, a complex type) |
| A4a (shared function layer + math family: math/complex/engineering bodies, dispatch per family, bank + calc-traps replay harness for every family, robustness/memory/zero-alloc tests, the `each_number` lane fast path, benchmarks vs the TS, `SheetLoader::strings`, fuzz) | Opus | ≈ 1 h 30 min wall (07:10-08:40) | 0 parity rounds on the bodies (the first bank and trap replays of the math family were equal except what the harness or another frontier explained); rework found by the replays, tests and benchmarks of the same session, all fixed by the same agent: (1) the A2 replay compared against bodies now present → `EvalState::set_bodies(false)`; (2) the A3 replay compared RAND cells whose stream a non-ported body had shifted (784 values) → random cells deferred with the stream (A4a-10); (3) the dispatch table counted as a leak in A2's M1 test (4,280 B) → warmed first; (4) the bank exposed an A3 bug: the engine's arena lacked the empty string at `StrIx(0)`, breaking LAMBDA optional parameters (A4a-11); (5) legacy `@` bank cases (A5) deferred; (6) `sum-ranges-100k` at 1.0× the TS (145 ms) until `each_number` read the store lanes (34 ms, A4a-8); (7) a 1.7·10^10-cell broadcast aborted the A2 evaluator → array caps (A4a-4); (8) the CI trap replay took 23 s release (whole-column fills and 18 MB value texts) → texts built in one buffer, non-ported functions not run, whole-column traps release-only; (9) Bessel budget lowered from 2^26 to 2^22 steps (170 ms → 10 ms per hostile call); (10) clippy (excessive precision of the TS literals, approximate constants, a negated float comparison, a complex type); (11) the fuzz run found formulas of hundreds of hostile Bessel calls taking 5 s → the budget is checked before the loop |
| A4b (statistics family: statistical/distributions/regression/ets, 145 bodies, the `pairs`/`walk`/`xadd` fast paths, selection for order statistics, ETS and least-squares bounds, `tests/a4b.rs`, the stats-10k benchmark in both runners, fuzz seeds) | Opus | ≈ 1 h wall (08:40-09:40) | **0 parity rounds**: the first bank and trap replays of the 145 bodies were equal (CI bank 5,965, traps 17,956); the big trap run showed 2 records that are the capture's platform (Windows UCRT `pow`, checked in WSL against the glibc reference: open item 1). Rework found by the tests, benchmarks and fuzz of the same session, all fixed by the same agent: (1) clippy (4 negated float comparisons, a range loop in a test, an ambiguous float literal); (2) stats-10k at 4,949 ms (0.13× the TS) with CORREL/SLOPE 2.2 s each → `pairs` on the store lanes (A4b-3) and `xadd` (A4b-7): 1,960 ms (0.051×); (3) the ETS gap filling rescanned each gap run (quadratic) → next-known index from one pass; (4) the fuzz run found `BETA.INV` at 1 s per call — the TS bound, kept (open item 5); (5) the first libFuzzer attempt built through `core/fuzz`'s workspace (the `uo-ffi` cdylib does not link) and then lacked the ASan DLL on `PATH` — both setup, no code |
| A4c (finance/date family: financial/daycount/datetime, 80 bodies, the bounded loops and fixed points, the stack `lift_atp`, `tests/a4c.rs`, the finance-50k/dates-50k benchmarks in both runners, fuzz seeds) | Opus | ≈ 55 min wall (09:25-10:20) | **0 parity rounds** (no port bug sent back by a verifier); rework found by the replays, tests, benchmarks and fuzz of the same session, all fixed by the same agent: (1) an Excel error of the holidays left the whole lift through `?` instead of becoming the element's value (4 CI trap records, NETWORKDAYS over arrays); (2) the holidays' spread `RangeError` of a whole column (17 big-run records, A4c-4); (3) clippy (identical `if` blocks, negated float comparisons); (4) the zero-allocation test first compared long formulas: a 6-term operator chain alone grows the evaluator's stacks (160 B per recalc) → one call per formula, values checked to be numbers; (5) `a4_robust`'s debug bound hit by a 3M-element lift of PMT/RATE (the lift's cost) → debug bound 4 s (A4c-7); (6) fuzz: ODDF* walking to the `Date` limit for an issue of −1E300 and VDB running its whole budget → serial bound, count check, quasi-coupon dates computed once |
| A4d (text family: text/text-format/regex/filterxml/regional, 62 bodies, the vendored `regress` with a step budget, the FILTERXML arena and work budget, the stack `Buf` and in-place reads, `tests/a4d.rs`, the text-ts-50k/text-fn-50k/regex-5k benchmarks in both runners, fuzz seeds) | Opus | ≈ 1 h 10 min wall (09:39-10:50) | **0 parity rounds**: the first CI replays of the family (bank 4,224, traps 5,080) were equal. Rework found by the tests, benchmarks and probes of the same session, all fixed by the same agent: (1) upstream `regress` has no step budget (probe: `(a+)+b` over 24 a's 0.5 s) → vendored copy with the budget (A4d-1); (2) `Buf` panicked when its first write passed the stack part (the first bank run, before any comparison); (3) two wrong expectations in new unit tests (`cjk_number`: the TS type 3 drops the sign; the "零" rule); (4) SEARCH's wildcard segments allocated per call (zero-allocation test) → matcher over the pattern units; (5) regex-5k 1.10× the TS → cache lookup without allocation and one executor per scan (1.00×); (6) a skip of re-interning identical texts in `Store::write_value` measured no gain → reverted (the text-result cost is elsewhere in A3's path, open item 2); (7) clippy (`f64::min/max` → `uo_jsnum::math`, an `x + 1 <=`, negated float comparisons, an enum variant name, an assign op); (8) the fuzz run was refused by the session's permission check (disk) → seeds recalculated by `tests/a4d.rs` instead, the run left open |
| A4e (lookup family: lookup/reference/dynamic/database/conditional, 72 bodies, the pass cache and computed-criterion hooks, the exact and collation indexes, the gather-then-match conditional reads, `tests/a4e.rs`, the six benchmark classes in both runners, three harness fixes; at the end the crate's final checks: the evaluator's prefix copy, clippy of `tests/a4d.rs`, fmt of `fnmock.rs`) | Opus | ≈ 1 h 5 min wall (10:48-11:53) | **0 parity rounds** (no port bug sent back by a verifier): the first CI replays had 6 bank mismatches, all harness (`fnmock` LAMBDA arity and file name, A4e-10), the first big trap run 1 (OFFSET with a NaN size: `Throw::Range`, A4e-6), the first A3 replay 128 values of the full recalc (its random stream was not captured: reproduced, A4e-10). Rework found by the tests and benchmarks of the same session, all fixed by the same agent: (1) SORT validated every index before the orders (the TS converts and checks element by element) — caught in review; (2) a read-only `FnContext` adapter for the conditional walk replaced by a two-state source (`Ctx::Ro`/`Rw`); (3) the TS bench's 10,000 text XLOOKUPs (10^9 collation compares, linear) would have run for an hour → 1,000; the Rust side got the collation index (A4e-2); (4) the zero-allocation test found `_xlfn.` names allocating in the evaluator (open item 1) → prefixed lookups measured against a prefixed baseline; (5) clippy (`is_multiple_of`, `f64::min/max` → `uo_jsnum::math`). The crate was mid-edit by A4f/A5 at times (A4f's record notes 10 min without compiling caused by this frontier's half-written modules). |
| A4f (logical/info/cube family: 44 bodies, `FnContext::lambda_arity`, the `Memo` of the TS `??=` caches, the LAMBDA fills and bounds, `tests/a4f.rs`, the logical-100k benchmark in both runners, two harness fixes, fuzz seeds) | Opus | ≈ 55 min wall (10:16-11:10) | **0 parity rounds**: the first bank and trap replays of the family were equal (CI bank 517, traps 3,360; release 3,444; big 1,034 + 17,616). Rework found by the tests, benchmarks and fuzz of the same session, all fixed by the same agent: (1) ISEVEN/ISODD's array lift turned an element's Excel error into the whole result through `?` — caught in review before any run (A4c's lesson); (2) T built its empty text on every call → only when a result needs it; (3) the zero-allocation test showed AND/OR over a multi-cell area allocating in the engine (≈ 96 B per call, open item 1) — the one-cell area reads the cell (A4f-5); (4) two tests of other frontiers changed once IF had a body (A4f-8): `a3_robust` incremental = full on a `-0` the TS write-back keeps (traced: a fresh formula cell starts at 0 and `-0 !== 0` is false), and `a4_robust`'s debug bound hit by A4c's RATE over a whole column (6.2 s under load) → whole-column calls skipped in debug, bound 1,500 ms again; (5) the bench's first expectations of array texts (format `a3x1[…;…]`) in `tests/a4f.rs`; (6) a Python patch of a test lost its backslashes (Bash halving) — redone from a file. At the end the crate did not compile for 10 min (A4e mid-edit: `PassCache`, the `conditional`/`database`/`dynamic` modules), so the two last cases of `tests/a4f.rs` `lambda_arity` (ISEVEN/ISODD of an omitted optional parameter, checked against the frozen TS: `#VALUE!` + an internal error) were not run in Rust; everything else above ran green |
| A5 (legacy `@` rewrite + engine wiring, catalogue, i18n tables generator, translator, add-ins, open-recalc, save/file-form over the store, `a5` capture, calc-tests/Excel-oracle/uo-xlsx-plan replays, robustness/M1/zero-alloc tests, benchmarks in both runners, harness un-deferral in `a3.rs`/`a4.rs`) | Opus | ≈ 1 h 15 min wall (10:33-11:48) | **0 parity rounds on the ported code** (the first replays of the translator and the rewrite were clean except harness slips); rework found by the replays, tests and benchmarks of the same session, all fixed by the same agent: (1) the capture script computed the expected value of half its mutated records with the wrong direction (460 records) and hard-coded the `Intl` decimal of an unknown id (`xx-YY` is `,` on the pt-BR capture machine) — both harness; (2) `implicit.f2.json` ends with a newline the test reader rejected (harness); (3) the add-in test expected `#NAME?` where SINGLE is not ported yet (A4e); (4) translator allocations per name token and per call (270 B/translation) and a rewrite 1.40× the TS (stacks and argument vectors per node) — fixed (0 B, 0.55×); (5) a 100,000-unit run of unbalanced `[` took 79 s (quadratic `matchBracket`, as the TS) → memo (A5-3); (6) clippy (a negated float comparison, a collapsible `if`, a counter loop, a redundant closure); (7) the shared crate did not build for ≈ 35 min while A4e wrote `functions/lookup` — the A5 code was compiled and tested in a scratch copy with the family stubbed (same target dir), then in the crate once it built |
