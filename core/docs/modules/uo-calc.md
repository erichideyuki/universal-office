# uo-calc — the one cell store and the calc engine (architecture, plan §4.0 + track A)

Record of the rust-port workflow (hybrid mode, `SKILL.md` "Architecture pass (Opus), only for the big shapes").
Status: **architecture pass done 2026-11-24 (Opus)**: data model, frontiers, captures, budgets; type skeleton in
`core/crates/uo-calc` (clippy clean native and wasm32). **Frontier S0 (the store) done 2026-11-24 (Opus)**: section
"S0 — the store, as built". **Frontier A1 (values, coercion, collation entry points, criteria, dates, stats base,
special functions, libm, math support) done 2026-11-24 (Opus)**: section "A1 — as built"; A2–A5 are not ported yet.
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

## Frontiers (closed scope per frontier; all Opus)

| # | Scope | Delivers | Captures | Lines | Needs |
|---|---|---|---|---|---|
| **S0** (**done** 2026-11-24, section "S0 — the store, as built") | `key`, `strings`, `store`, `formula` (file side), `cellValue`/`fileCellValue`/`numberLiteral` | the store API: get/set/delete, kind/payload/style lanes with widening, area iteration row-major and column runs, viewport gather, string pool with refcounts and compaction, SST lazy decode hooks, extras, `FileOrder`, row meta, default style, the **store facet** (canonical JSON of every cell, below) | store facet of the TS model for every committed sample + generated workbooks (via a TS adapter that turns the sheet-core model into the facet) | ≈ 600 TS-equivalent + new | — |
| A1 (**done** 2026-11-24, section "A1 — as built") | values/coerce/collate (already in `uo-text`)/criteria/dates/stats/special/libm/math-util | as plan §5 A1; the context walkers and `FnDef` factories go to A2/A4 (A1-7) | the new `a1` group (every A1 entry point, traps + seeded) and the A1 calls of `calc-tests`; `calc-traps`/bank enter through the evaluator (A2+) | ≈ 1,900 | — |
| A2 | ast/parse/print/a1/evaluate/apply/ops/fn | `parse`/`print` with a depth limit and an explicit stack, `trait EvalHost` (the TS interface, `evaluate.ts:34-61`) | parse/print round trips over the bank's formulas, `calc-tests` | ≈ 1,460 | A1 |
| A3 | engine (graph, full/incremental recalc, `NeedCell` as `Result`, spills, CSE, data tables skip, cycles/iteration, volatiles) on S0 | `Engine` over `&mut Store` | per-cell values + evaluation-order facet over generated workbooks, incremental = full, `calc-tests` | ≈ 900 | S0, A2 |
| A4a | math, math-util, libm, complex, engineering | functions | bank + traps | 2,000 | A3 |
| A4b | statistical, distributions, regression, ets | functions | bank + traps | 1,628 | A3 |
| A4c | financial, daycount, datetime | functions | bank + traps | 1,390 | A3 |
| A4d | text, text-format (via `uo-numfmt`), regex (`regress`, R1), filterxml (XPath subset), regional, tdl-util | functions | bank + traps (non-ASCII, surrogates) | 1,701 | A3 |
| A4e | lookup, reference, dynamic, database, conditional | functions (stable sorts: `uo_jsnum::sort`) | bank + traps | 1,632 | A3 |
| A4f | logical, info, cube | functions | bank + traps | 467 | A3 |
| A5 | implicit (+ 508 measured classes), i18n (pt-BR/en-US tables, C1 hook), open-recalc, save/file-form, addin, `harness` (`writeValues`, `testing.ts`), the catalogue | as plan §5 A5 | i18n round trips, open-recalc plans, save attrs, `calc-tests` | ≈ 2,800 | A3 |

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
| **calc-order** | `node core/crates/uo-calc/js/capture-order.mjs`; local `--gen x1-big --heavy 20000` | `calc-order.capture` 74 KiB + `.side.gz` 423 KiB, 30 workbooks | the evaluation-order facet of §16 item 4: the generated workbooks (uo-xlsx `gen-workbook.mjs` group x1, read by sheet-core's dist, `fromSheetCore` → the `CalcWorkbook`, kept in the sidecar so A3 can replay before C1a exists) through `new Engine` + `recalc()`, a `calculate()`, 12 seeded `setCell` + `calculate()` edits, and a fresh full recalc of the final inputs: per run the `evaluateRec` sequence (13,970 evaluations in x1), the RAND values (1,570), `circular` sorted (13 workbooks with cycles), `internalErrors` (0), all values, and incremental vs full (equal in 6, differ in 24: volatiles and cycles — recorded, not judged). The x1 workbooks put ranges wider than the formula columns next to spills/CSE (the `fc.keys()` and `rangeMasters` sites of S7) |
| **a1** (A1) | `node core/crates/uo-calc/js/capture-a1.mjs`; local `--big --out core/target/sheet-cap/a1-big.capture.gz` (19 s) | `a1.capture.gz` 521 KiB, 52,402 calls (big: 486,022) | every A1 entry point of the calc dist called directly: traps + seeded inputs in four locales (en-US, pt-BR, a de-DE host locale, an empty-group locale), clock frozen at 2025-01-15 12:00 UTC; records `<fn>#<i>` with `in` (value texts) and `out`; replayed by `tests/a1.rs` (section "A1 — as built") |
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
| parse + print the bank's formulas (A2) | ≤ 0.5× TS Node | > 1.0× TS |
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

Fuzz (`core/fuzz`): `calc_formula` (formula text → parse/print/eval), `calc_workbook` (generated hostile workbooks:
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
