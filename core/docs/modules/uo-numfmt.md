# uo-numfmt — number/date format engine

Record of the rust-port workflow for one module (plan §4.1 of `core/docs/slides-sheet-port-plan.md`).
Hybrid-mode run: one Sonnet-class port frontier with its own parity/memory/no-panic verification
in the same session; no separate planner or skeptic (closed scope: 1,128 lines of one TS file).

## Scope

- TS covered: `shared/charts/src/numfmt.ts` (frozen copy in the `ts-ref-092` worktree, 1,128 lines):
  `FormatLocale`/`PT_BR`, `LANG_NAMES`, `BUILTIN_FORMATS`/`builtinFormatCode`, `decimalDigits`,
  `formatGeneral`, the tokenizer (`splitSections`, `parse`, `parseSection`, `resolveMinutes`,
  `analyzeNumber`), `formatValueParts`, `formatValue`, section choice, numeric/scientific/fraction/
  date/elapsed emission, `serialToDate`. Its tests (`numfmt.test.ts`) and the Excel-measured oracle
  (`numfmt-oracle.json`, 1,709 cases) are the parity inputs.
- C# covered: none (no C# counterpart; the .NET Sheet port is out of the plan).
- Left for other modules: the callers (calc `TEXT`, Sheet cell display with column width, chart
  labels in `uo-charts`), the `formula-locale`/`Intl` derivation of locales (calc A5): a host builds a
  `FormatLocale`.

## Plan (closed scope, approved with the §4.1 brief 2026-11-17)

- Crate and reuse: `uo-numfmt`, a leaf above `uo-jsnum` and `uo-text`; no Word assumption; shared by
  calc `TEXT`, Sheet and charts. Nothing else added (`uo-alloc`/`uo-testkit` are dev-dependencies).
- Data model: tokens are an enum (`Tok`), sections a struct, a parsed code a `Parsed` (sections +
  indices of the numeric and text sections: the TS compares sections by identity, here by index).
  Strings are `Vec<u16>`/`&[u16]` end to end (the TS indexes by code unit; see decisions).
- Public API (mirrors the TS exports): `format_value`, `format_value_parts`, `format_general`,
  `decimal_digits`, `builtin_format_code`, `builtin_format` (the table), `pt_br()`, `FormatLocale`,
  `FormatOptions`, `FormatParts`, `FormatResult`, `FmtPart`, `Value`, `serial_to_date_ms`,
  `utc_fields`, `clear_caches`, `cache_len`.
- Crate vs own port: `uo-jsnum` for `toPrecision(15)`, `String(x)`, `Number(s)`, `parseInt`,
  `parseFloat`, `Math.floor/round/log10/pow`, ToInt32 (V8-exact); `uo-text` for `toLowerCase`
  (bracket codes) and the UTF-16 helpers. No new dependency.
- Parity: capture of the frozen TS dist (`js/capture-numfmt.mjs`), inputs inside the records so the
  replay needs no generator; see Verification.
- Memory: M1 per public operation, M2 soak, M3 numbers, M5 for the one cache (below). Security: no
  panics on any code/value/locale; fuzz target `numfmt_format`.

## Decisions outside the source semantics

| # | Decision | Why |
|---|---|---|
| D1 | Strings are UTF-16: codes `&[u16]`, `FmtPart::Text(Vec<u16>)`, `Skip(u16)`/`Fill(u16)`, `FormatResult.text: Vec<u16>` (`text_lossy()` for a Rust `String`). | The TS reads `raw[i + 1]` by code unit: `_`/`*` before an astral character take only the high surrogate and the low half becomes the next literal; `format_value` re-joins the pair when it inserts the fill. A `String` cannot carry this. Pinned in `tests/decisions.rs`. |
| D2 | `FormatLocale` holds fixed arrays (`[String; 12]` months, `[String; 7]` days, `[String; 2]` AM/PM), `builtins: Vec<(u32, String)>` (last pair of an id wins, as a JS object literal). | A host cannot build a locale whose arrays are short (TS: `undefined` text or a `TypeError`). |
| D3 | `pt_br()` is one `&'static` instance; the TS test `names === loc` in `localeFor` is `ptr::eq(pt_br(), loc)`. A structural copy of PT_BR is NOT the object: for `[$-416]` the TS then spreads `PT_BR` over it (decimal separator included), and so does this port (`ptclone` locale in the capture). | Faithful identity semantics. |
| D4 | `[constructor]` / `[__proto__]` (any case): the TS looks the lower-cased text up in a plain object, finds `Object.prototype` members and returns a function/object as the result's `color`. Rust ignores the bracket (text identical; no colour). | A prototype-pollution accident, not behaviour; a non-string cannot cross the API. The TS text result is the same. |
| D5 | An empty AM/PM name with `a/p`: the TS throws a `TypeError` (`s[0].toLowerCase()`); Rust writes nothing for it. | No panic/exception path. |
| D6 | `*x` repetitions are capped at `MAX_FILL_REPS = 65,536` (the TS `repeat` throws a `RangeError` for an infinite width or past the V8 string limit). Fractional widths truncate as `repeat` does; 0, NaN and `None` are "unknown width" (once). | A column is at most 255 characters in Excel; avoids a hostile allocation. |
| D7 | `format_general(x, loc, budget)` clamps `budget` to `MAX_GENERAL_BUDGET = 4096`; `budget` is `i32`. | A finite double has at most 309 integer and 15 significant digits: for any budget from about 400 the text is the same (tested at 400/500/4096/`i32::MAX`); the TS spends `budget` characters on a zero run and throws past 2^29. |
| D8 | The parse cache is a per-thread map (cleared by `clear_caches()`), cap of 2,000 codes as in the TS (cleared when full), and codes longer than 4,096 units are never cached. | Memory bound only; the cache cannot change a result (checked by the fuzz target and the parity corpus, which formats each code cold and warm). |
| D9 | `Date` is only ever UTC in the TS (`Date.UTC`, `getUTC*`): no time-zone input exists. The calendar is computed from days (proleptic Gregorian, Hinnant's algorithm) over the same day counts; `serial_to_date_ms` reproduces `serialToDate(x).getTime()` including `TimeClip` (NaN beyond ±8.64e15) and `utc_fields` the `getUTC*` fields. | Same results without a `Date`. Parity-checked over 329 serials (negative, huge, NaN, ±Infinity, the 1900 gap). |
| D10 | `builtin_format_code(id: u32, locale)`: the TS default locale is `PT_BR` (pass `pt_br()`); `Option<&str>` borrows. `format_value*` take `Value::{Num, Str}`; `colWidthChars` is `Option<f64>`, `showZeros` a `bool` (default true), `date1904` a `bool`. | Rust types for TS unions/optionals. |
| D11 | Regexes are hand-matched on code units with the semantics of the non-unicode flag: `/i` over ASCII patterns is ASCII-only (a non-ASCII unit never canonicalizes to ASCII), `.` excludes `\n \r U+2028 U+2029`, `\s` is `uo_jsnum::text::is_js_space_unit`, `toLowerCase` is `uo_text` (so a Kelvin sign inside `[blacK]` is a colour, as in the TS). | Parity on exotic input; covered by the generator's astral/Kelvin cases. |
| D12 | Quadratic behaviour of pathological codes (`indexOf`/`includes` in the TS) is kept; no limit on code length in the crate (Excel caps a format at 255 characters; hosts bound it). `decimal_digits(v, n)` allocates `n` digits. | Faithful port; the no-panic test runs 1,500–3,000-unit hostile codes in milliseconds. |

## Verification

### Parity

`js/capture-numfmt.mjs` runs the frozen `shared/charts/dist/numfmt.js` (`UO_TS_ROOT`, default the
`ts-ref-092` worktree; node v24.15.0, V8 13.6.233.17 x64) and writes `tests/captures/numfmt.capture`
(4.48 MB, 1.24 MB gzipped, committed). `tests/parity.rs` replays it with `uo-testkit`'s exact
comparator; the platform line is checked (`log10`/`pow` are fdlibm bits).

| Corpus | Records | Facets compared | Mismatches |
|---|---|---|---|
| 1,709 Excel oracle cases (`o*`) | 1,709 | 1,709 text equalities TS=Rust | 0 |
| `numfmt.test.ts` cases + generated codes (`k*`): `formatValue` text + colour + `formatValueParts` (parts, colour, overflow) | 9,086 codes, **51,130** cases (70 from the test file) | 153,390 | 0 |
| `formatGeneral` (`g*`, budgets −1..14, 5 locales), `decimalDigits` (`d*`), `builtinFormatCode` (`b*`, ids 0..90 × 6 locales), `serialToDate` (`s*`) | 4,008 + 4,004 + 546 + 329 | 9,216 | 0 |

The generator (seeded xorshift32) draws format codes from number/fraction/scientific/date/elapsed
grammars, multi-section codes with colours, conditions (`[<100]`, `[>=5]`, `[=0]`, `[<>3]`…), LCIDs
(`[$-409]`, `[$-416]`, `[$-F800]`, `[$-F400]`, `[$€-2]`, hex junk…), text sections, literals/escapes,
`_x`/`*x`, token soup, astral characters and lone surrogates, against values (specials, integers,
decimals, dates and times, huge/tiny, ±0, NaN, ±Infinity, texts), the 5 locales (PT_BR object, a
structural clone, an English one, an odd one with `::` decimal and custom builtins, one with empty
separators), 1904 dates, `showZeros`, column widths (12, 0, 3.7, 20, 8). A local run with another seed
(`--n 150000 --seed 7`: 150,086 codes, **850,378** cases, 2,551,134 facets) is **0 mismatches**
(`UO_NUMFMT_BIG=1 UO_NUMFMT_CAPTURE=<file> cargo test -p uo-numfmt --release --test parity big`, not
committed: 65 MB).

`tests/ts_tests.rs` re-states every assertion of `numfmt.test.ts` with the TS test's own expected
values (independent of the capture): 9 tests pass.

### Agreement with Excel

The 1,709 measured cases: **TS = Excel in 1,709 / 1,709; Rust = Excel in 1,709 / 1,709; both agree
on the same 1,709 cases** (asserted case by case in `oracle_agreement`: the sets are identical).

### Memory (measured, `tests/memory.rs`, debug build, counting allocator)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | pass, live bytes return EXACTLY (the parse cache is cleared inside the measured closure; the two `OnceLock` tables are built before) | `format_value`, `format_value_parts`, `format_general` (also scientific), `decimal_digits`, `builtin_format_code`, `builtin_format`, `serial_to_date_ms`, `utc_fields`, `clear_caches`, a 24 codes × 14 values grid under two locales |
| M2 soak (native) | pass, tolerance 0 | 400 iterations of the 24-code grid (336 formats + 24 texts each, two locales alternating), a sample every 50: after the 1st sample live bytes are flat (24,544 B = the 24 cached codes); after `clear_caches()` back to the initial value exactly |
| M2 soak (wasm) | not run: the crate has no wasm entry point yet (clippy for `wasm32-unknown-unknown` is clean); belongs to the Sheet/chart bindings | — |
| M3 budgets (allocated / peak, first call with the code cold → cached) | recorded | number `#,##0.00`: 1,782 / 1,148 → 698 / 448 B; date `dd/mm/yyyy hh:mm:ss`: 3,607 / 1,706 → 1,525 / 706; fraction `# ??/??`: 1,933 / 1,281 → 817 / 485; scientific `##0.0E+0`: 1,705 / 1,116 → 685 / 416; General: 222 / 90. `format_general` 122 B, `decimal_digits` 51 B, `builtin_*`, `serial_to_date_ms`, `utc_fields`, `clear_caches`: 0 B. (A cached format allocates its output, parts and strings; asserted < 4 KB.) |
| M4 boundaries | n/a (no handles) | — |
| M5 cache caps | pass | 5,000 distinct codes: cache length peaks at 2,001 (cap 2,000 + the entry being inserted); a 5,000-unit code is never cached; `clear_caches()` returns every byte |
| M6 Miri/LSan | n/a: `#![forbid(unsafe_code)]` | — |

### Security

No panics: `tests/no_panic.rs` runs random codes (a 4 s time box, **516,168** format cases in a debug
build on 24 values incl. NaN/±Infinity/5e-324/1e300, with and without locale/width/1904/showZeros,
codes also used as the text value, lone surrogates) and hostile lengths (3,000-unit digit runs,
3,000 sections, 1,500 quotes/brackets/backslashes/underscores, huge/NaN/negative widths, every
`format_general` budget from `i32::MIN` to `i32::MAX`). Two guards came out of it (D6, D7: an
`i32::MAX` budget asked for 2^31 zeros). Fuzz target `numfmt_format` (`core/fuzz/src/numfmt.rs`, seeds in
`seed.rs`): format code as UTF-16LE, a value, option flags; asserts parts/text agreement, overflow =
`"#"`, at most one fill, merged texts, cold = cached, digits only. Run with the stable driver
(`fallback`, 23 seed inputs from `seed.rs`): **11,682,471 executions in 60 s, 0 crashes** (slowest 6 ms),
and 11,153,412 executions in 40 s from an empty corpus, 0 crashes. Not run under libFuzzer (Linux CI
runs every file of `fuzz_targets`, which now includes `numfmt_format`).

## Process metrics

| Stage | Wall time | Rounds | Notes |
|---|---|---|---|
| Port (7 source files, 2,700 lines incl. locale tables) | ≈ 20 min | 1 | compiled after one borrow fix; clippy fixes (negated float comparison, collapsible `if`, `approx_constant` in tests) |
| Parity harness (capture script + replay, 4 test files) | ≈ 15 min | 1 | 100% on the first replay: 0 port bugs found by the verifiers |
| Memory/no-panic/fuzz target | ≈ 10 min | 1 | found the `format_general` budget hazard (D7) and the harness-capture buffer in M2 baselines (test bug) |
| Total session | ≈ 55 min including ≈ 20 min waiting for a sibling frontier's half-created crate directory (`crates/uo-pptx` without a `Cargo.toml` blocks every cargo command in the workspace) | | |

Rework: none in the port; two test-side fixes (live-bytes baselines must exclude the soak series and
the `println!` capture buffer; hostile-length test sizes).

## Evolution

None proposed. Candidates for later: the quadratic `contains`/`position` scans in `emit_fraction`
over long placeholder runs could use a set (results unchanged); not worth it at Excel's 255-character
format limit.

## Final review and benchmarks (gate 3 — pending)

Not run. Opus verifier for the calc consumer is part of the plan (§4.1); the cheap checks it would
repeat are the three commands above.
