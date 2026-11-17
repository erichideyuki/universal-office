# uo-text — JavaScript text semantics over UTF-16

Record of the rust-port workflow (hybrid mode, frontier U of milestone 1) for the crate `core/crates/uo-text`.
Requirements: `uo-docx-layout.md` LQ11 / K10 (NFC, `\p{M}`, JS `\s` over `u16`, SpecialCasing case mapping),
`uo-docx.md` G10 / G11 (Windows collation, Unicode tables pinned to Node 24.15), `uo-render.md` and `uo-pdf.md`
(JS `\s` trim/replace over UTF-16 for `psName`, URIs and outline titles).

## Scope

Everything works on `&[u16]`, because a JS string can hold lone surrogates and a Rust `&str` cannot (`_str`
variants exist for valid text). Unicode tables are **generated from the ICU inside Node 24.15 (V8 13.6): Unicode
17.0, ICU 78.2**, by probing V8, never by reading a Unicode data file, and committed as Rust source (no parsing at
run time).

| Module | JS it implements | Source of truth |
|---|---|---|
| `nfc` | `normalize('NFC')`, `'NFD'`, and `'NFKD'` (feature `collation`), lone surrogates pass through | `String.prototype.normalize` |
| `props` | `\p{L}` `\p{M}` `\p{N}` `\p{P}` with the `u` flag (the only classes `write/packages` uses: hyphenate.ts, text-layout.ts, find-engine.ts, navigation.ts, references.ts, universal-write.ts) | `RegExp` |
| `case` | `toUpperCase`, `toLowerCase` (full mapping, SpecialCasing, Final_Sigma, root locale), the `/i` Canonicalize of a regexp without `u` | `String.prototype.toUpperCase/toLowerCase`, `RegExp` |
| `space` | JS `\s` over `u16`: `trim*`, `split(/\s+/)`, `replace(/\s/g)`, `replace(/\s+/g)` | `String`, `RegExp` |
| `collation` (feature `collation`) | `compareText`, `textCollateEqual`, `collationKey` of `sheet/packages/calc/src/collation.ts` (the Windows word sort that Write's table sort imports as `@universal-office/collation`) | the TS file itself, run in Node |
| `utf16` | code points over `&[u16]` with the JS view of lone surrogates | — |

Boundary with `uo_jsnum::text`: the JS space set stays **one** definition there (`is_js_space_unit`, re-exported as
`space::is_space`); uo-text adds the `u16` helpers, the case mapping and the rest. `uo_jsnum::text::to_lowercase`
(`&str`, Rust std Unicode) is **not** replaced: measured over every code point it equals V8's (test
`jsnum_to_lowercase_agrees_with_v8_tables`), and so does `char::to_uppercase` (`std_to_uppercase_agrees_with_v8_tables`),
both at Unicode 17.0. They differ from uo-text in shape only (UTF-8, no lone surrogates, no Final_Sigma context across
`u16` strings with lone surrogates), and the std version would silently diverge at the next Rust or Unicode update;
callers that must equal V8 (caps, small caps, `doNotHyphenateCaps`, the length change of `ß` -> `SS`) use `uo_text::case`.

## API

```
utf16:     decode_at, decode_before, code_points, push, from_str, to_string_lossy
nfc:       nfc, nfd, nfkd (collation), is_nfc, nfc_str, combining_class_rank
props:     LETTER MARK NUMBER PUNCTUATION, mask, is_letter, is_mark, is_number, is_punctuation,
           is_letter_or_mark, is_single_code_point_in(units, classes)   // /^[...]$/u.test(s)
case:      CaseMapping{Same,One,Many}, upper_mapping, lower_mapping, upper_len, to_uppercase, to_lowercase,
           to_uppercase_str, to_lowercase_str, regexp_canonicalize
space:     is_space, trim, trim_start, trim_end, split_spaces, replace_each, replace_runs
collation: CollationKey{new, compare, to_key_string}, compare_text, text_collate_equal, collation_key
lib:       UNICODE_VERSION, ICU_VERSION, table_sizes, clear_caches
```

Functions that may return their input return `Cow` (borrowed when nothing changes, so the common path allocates
nothing). The crate has no cache, no global state and no `unsafe`; no function panics on any input.

## Generator and tables

`js/gen-tables.mjs` writes `src/tables.rs` and `src/tables_compat.rs` (`--check` compares instead of writing).
Deterministic; refuses to run on any Node but 24.15. Each table is a compaction of one probe over every code point
`0..=10FFFF`:

| Table | Probe | Entries | Bytes |
|---|---|---|---|
| `CCC` | classes found by `NFD` reordering (U+0345 and U+0334 as probes), sorted by the same relation | 404 runs, 55 ranks | 3,232 |
| `NFD_*` | `normalize('NFD')` (Hangul syllables are algorithmic) | 2,081 | 16,830 |
| `COMP_*` | primary composites found as `NFC(a + b) === c` | 961 (928 BMP + 33 astral) | 5,964 |
| `PROPS` | `/^\p{L}$/u` ... over every code point | 1,356 runs | 10,848 |
| `SIGMA` | Final_Sigma classes (cased / skipped) found by probing `c + 'Σ'` and `'A' + c + 'Σ'` | 615 runs | 4,920 |
| `UPPER_*` | `toUpperCase` of every code point (1,478 single mappings in 198 runs, 102 expansions) | | 3,632 |
| `LOWER_*` | `toLowerCase` (1,487 in 185 runs, 1 expansion: U+0130) | | 2,232 |
| `COMPAT_*` (feature `collation`) | `normalize('NFKD')` where it differs from NFD | 3,849 | 29,826 |

Default build 47,658 bytes of tables, 77,484 with `collation`.

Regeneration diff: `tests/regeneration.rs` runs both generators with `--check` (it fails, rather than passes
silently, without Node 24.15; `UO_TEXT_SKIP_REGEN=1` skips it knowingly). CI should run it on the Node of the
`rust-core` workflow.

## Oracle results (V8 = Node 24.15.0, Unicode 17.0, ICU 78.2)

`js/capture-oracle.mjs` writes `tests/captures/text_v8.txt` (0.95 MB) and `collation_v8.txt` (0.58 MB), deterministic,
replayed with no Node. Collation is captured by **importing `collation.ts` itself**. Every result is compared; the
counts of what was compared are asserted (a capture that shrinks fails).

| Check | Compared | Mismatches |
|---|---|---|
| NFC and NFD of every code point (lone surrogates unchanged and borrowed) | 1,114,112 | 0 |
| Hangul: NFD and NFC of the 11,172 syllables, NFC of the 11,172 L V and L V T sequences (FNV digests of V8's results) | 3 x 11,172 | 0 |
| NFC of every composition first x second pair (the composites are firsts too) | > 50,000 pairs, 2,219 that change | 0 |
| NFC and NFD of random sequences (bases, marks of every common class, jamo, Indic/Tibetan composites, astral, lone surrogates) and of 160 runs of 25 to 85 marks | 2,160 | 0 |
| `\p{L}` `\p{M}` `\p{N}` `\p{P}` of every code point (and: the classes never overlap) | 4 x 1,114,112 | 0 |
| `toUpperCase`, `toLowerCase` of every code point (lone surrogates map to themselves) | 2 x 1,114,112 | 0 |
| Final_Sigma over every code point `c` in `cΣ`, `AcΣ`, `AΣc`, `AΣca`, `AΣcc`, `cΣc` | 6 x 1,114,112 | 0 |
| `/i` Canonicalize of every code unit (cross-checked against V8 `RegExp` in the generator: class matches and 200,000 random pairs) | 65,536 | 0 |
| 1,800 random UTF-16 strings x 12 operations (NFC, NFD, upper, lower, trim, trimStart, trimEnd, `split(/\s+/)`, 3 replaces) | 21,600 | 0 |
| Collation key of every code point (exceptions to the stated default + Hangul digest) | 1,114,112 | 0 |
| Collation 244 x 244 matrix, 6,000 random pairs (half one edit apart), 1,200 key strings, 24 sorts of 80 | 59,536 + 6,000 + 1,200 + 24 | 0 |
| wasm32 build (`js/wasm-check.mjs`, the `size_probe_all` example vs V8 and the TS collation) | 300,000 triples | 0 |

Mutation check: changing `>=` to `>` in the composition blocking rule fails 2 oracle tests (the oracles are not vacuous).

No-panic and algebra (`tests/no_panic.rs`): 30,000 random UTF-16 strings (NFC/NFD idempotent, `NFC(NFD(x)) = NFC(x)`,
all functions called), every unit pair and triple of a 40-unit pool, a run of 400,000 alternating marks (O(n)), 2 M
random units, 100,000 `Σ` + mark pairs (Final_Sigma scans are linear), and on `collation` antisymmetry, transitivity
and 200,000-unit inputs. Every test of the crate runs with and without `--all-features`.

## Memory (measured, `uo-alloc`)

| Check | Result |
|---|---|
| M1 no leak per operation | every public function, 10 inputs (empty, ASCII, ligatures, combining, Σ, Hangul, astral, whitespace, lone surrogates): live bytes return exactly; `m1_covers_every_public_function` fails if a `pub fn` has no call in `tests/memory.rs`; borrowed paths allocate 0 bytes |
| M2 soak (native) | 4,000 rounds of every operation, sampled every 50: flat to the byte (tolerance 0) |
| M2 soak (wasm) | 300,000 calls of the `size_probe_all` export: `memory.buffer` stays at 1,245,184 bytes |
| M3 budgets, 1,000 units of mixed text (allocated / peak above start) | nfc 11,000 / 11,000; nfd 11,000 / 9,000; to_uppercase 6,012 / 4,008 (the buffer grows for the `ß` expansions); to_lowercase 2,004 / 2,004; replace_runs 2,000 / 2,000. Budgets in the test are these + 10% |
| M4 boundaries | none: no handle crosses to JS or .NET |
| M5 caches | none (a test scans the sources for `static mut`, `Mutex`, `OnceLock`, `LazyLock`, `RefCell`, `Cell<`, `Atomic`, `HashMap`, `thread_local!`) |
| M6 | no `unsafe` (`#![forbid(unsafe_code)]`), nothing for Miri |

## wasm size (`wasm32-unknown-unknown`, profile `wasm`, `examples/size_probe_*`, one cdylib per family)

| Build | .wasm | code | data (tables) |
|---|---|---|---|
| base (allocation + code points, no table) | 14,430 | 13,615 | 638 |
| props only | 11,268 | 341 | 10,858 |
| NFC only (what the layout's hyphenation needs) | 47,565 | 20,704 | 26,667 |
| toUpperCase + toLowerCase | 29,308 | 17,711 | 11,414 |
| everything with `collation` | 111,841 | 32,709 | 78,915 |

The linker drops the tables a build does not reach, so the layout pays only for what it calls; subtracting the 14 KB
of allocator and panic code every probe shares, NFC costs about 33 KB, case mapping 15 KB and `\p{...}` 11 KB, and the
collation adds about 40 KB (the NFKD table is 29.8 KB of it). The wasm validates against the Safari 15 feature set
(`wasm-tools validate --features -all,wasm1,mutable-global,saturating-float-to-int,sign-extension,reference-types,multi-value,bulk-memory`).

## Decisions outside the requirements

1. **Collation is in this crate now**, behind the feature `collation`, although LQ11 left it for milestone 2: the
   task for this frontier asked for it, it is the same crate as G10 names, and the feature keeps the 30 KB NFKD table
   out of the layout's wasm. Consumers enable it (`uo-docx` for the table sort, Sheet later).
2. **Combining classes are stored as ranks** (55 of them), not Unicode's values: V8 does not expose `ccc`, and only the
   order is used (probed by NFD reordering). `combining_class_rank` documents it.
3. **Stable sort of a run of marks**: insertion sort up to 24 marks and a counting sort over the byte-sized class
   beyond that, instead of the library's merge sort (same result, O(n) on a hostile run, and 10 KB less wasm code).
4. **Collation keeps the TS arithmetic**: weights are `f64`, computed with the same expressions (`40 + (cp - 0xa1) /
   100`, `90 + (cp - 0x2010) / 10000`, `s * 32 + mark`), so the keys are bit-identical (checked through
   `collationKey`'s numbers). The two caches of the TS (a key per string capped at 20,000, an element per character)
   are **removed**: results are unchanged, and the crate stays free of state (M5). Sorting callers build a
   `CollationKey` once per text. `collation_key` formats numbers with `uo_jsnum::format::to_js_string`. No change of
   arithmetic semantics otherwise: no saturating, wrapping or truncating casts are involved.
5. **`regexp_canonicalize`** (the `/i` without `u` of the layout's CJK font-name regex and field switches) is derived
   from the upper-case table by the ES algorithm; V8's regexp engine was checked against it in the generator.
6. **Final_Sigma uses a table derived from V8's behaviour** (cased / skipped classes found by probing `Σ`), not from the
   `Cased` and `Case_Ignorable` properties, because the two can overlap (modifier letters) and ICU's precedence is what
   V8 does. The scan is linear (a test with 100,000 `Σ` + mark pairs).
7. **`replace_each` and `replace_runs` take a literal replacement**: JS reads `$&` and friends in a replacement string;
   the call sites (`replace(/\s+/g, '')`, `psName`) never use them.
8. **Hangul and the bulk collation keys are replayed by digest** (FNV-1a 64 over V8's results) instead of 11,172 lines each;
   everything else is a listed oracle. A digest mismatch names the block, not the code point: rerun the generator with a
   dump to localize.
9. **`utf16::push` drops values above U+10FFFF** (the tables never produce them) instead of panicking.
10. `table_sizes()` and the `size_probe_*` examples are part of the crate so the sizes in this record can be regenerated.
11. **Licence**: the tables are facts probed from ICU's Unicode 17.0 data; the crate keeps the workspace `MIT`. If the
    maintainer wants the Unicode License v3 notice shipped next to generated tables, it is a `NOTICE` entry, not a code
    change.

## Not done

- `/iu` simple case folding and the other properties named in `uo-docx.md` G10 for the find engine (`\p{Alphabetic}`,
  folding for `Intl`-free word boundaries): no consumer in milestone 1 (the find matcher is D2b); the generator pattern
  (probe V8 over every code point, compact, oracle) extends to them.
- `toLocaleUpperCase/LowerCase` (locale-dependent), `normalize('NFKC')`, `Intl.Collator`, `Intl.Segmenter`: not used by the
  Write sources searched (`write/packages/docx-layout/src`, `docx-core/src`, editor regexes).
- The wasm tests run through a probe export and Node (`js/wasm-check.mjs`), not yet through `uo-wasm` bindings or WebKit
  (no binding exists for uo-text; it is consumed in-process by the other crates).
- CI wiring (`rust-core.yml`): the regeneration test needs Node 24.15 in the test job and `wasm-check.mjs` could join the
  wasm job after building the `size_probe_all` example.

## Grapheme clusters and `\p{Ll}` (frontier B3 of the Slides port, 2026-11-17)

The Slides text layout breaks a word longer than the line by `Intl.Segmenter(undefined, { granularity: 'grapheme' })`
(`text-layout.ts:217`) and splits small caps by `\p{Ll}` (`text-layout.ts:171`); both are here (`uo-dml-render.md`
"B3 — text layout").

- `grapheme`: `gcb(cp)`, `is_extended_pictographic(cp)`, `incb(cp)`, the forward state machine `Breaker::step` (UAX #29
  GB3–GB13 with GB9c, the Indic conjunct rule of Unicode 15.1+), `boundaries(&[u16])` (interior offsets, lone surrogates as
  their own code points) and `graphemes(&str)` (clusters as sub-slices, no allocation).
- `props::is_lowercase_letter(cp)`: `\p{Ll}` (a new `LOWER_LETTER` run table in `tables.rs`, 664 runs, from `gen-tables.mjs`;
  the rest of `tables.rs` is byte-identical).
- **Tables probed, not read** (`js/gen-grapheme.mjs`, writes `src/grapheme_tables.rs` and the oracle; `--check` in
  `tests/regeneration.rs`): each code point's class is found by where the segmenter breaks around it — after/before `a`,
  before U+0301 (Control), against Hangul jamo L/V/T/LVT (the five Hangul classes), `c c c` (regional indicators), `U+1F600
  c ZWJ U+1F600` (Extend vs SpacingMark), `c ZWJ U+1F600` (Extended_Pictographic, cross-checked against
  `\p{Extended_Pictographic}`), `c VIRAMA KA`, `KA c KA`, `KA c VIRAMA KA` (InCB Consonant/Linker/Extend). GCB 1,386 runs,
  EP 156, InCB 473; 21 KB of tables in all (`table_sizes()`: `GRAPHEME`, `LOWER_LETTER`). Lone surrogates get no property
  (Any). 20 s on Node 24.15 (ICU 78.2, Unicode 17.0).
- **Self-check before writing**: the generator runs its own copy of the rules (the algorithm `grapheme.rs` ports) over every
  pair of class representatives, the triples with the joiners and 6,000 seeded random strings (lone surrogates and random
  code points included) against the segmenter, and stops on any difference — none on the first run.
- **Oracle** `tests/captures/grapheme_v8.txt` (630 KB): those 22,000 strings and their boundaries, and the `\p{Ll}` runs;
  `tests/grapheme_v8.rs` replays them (boundaries over `&[u16]`, the `&str` iterator on the valid ones, `\p{Ll}` of every code
  point): 0 mismatches. M1 (`tests/memory.rs`) and the no-panic laws over random UTF-16 (`tests/no_panic.rs`: boundaries
  strictly increasing and interior) cover the new functions.

## Commands

```
cargo test -p uo-text --all-features
cargo clippy -p uo-text --all-targets --all-features -- -D warnings     # and with --target wasm32-unknown-unknown
node core/crates/uo-text/js/gen-tables.mjs [--check]
node core/crates/uo-text/js/capture-oracle.mjs [--check]
node core/crates/uo-text/js/gen-grapheme.mjs [--check]      # grapheme tables + oracle (about 20 s)
cargo build -p uo-text --features collation --example size_probe_all --target wasm32-unknown-unknown --profile wasm
node core/crates/uo-text/js/wasm-check.mjs core/target/<dir>/wasm32-unknown-unknown/wasm/examples/size_probe_all.wasm
```

## Process metrics

| Stage | Agent / model | Wall time | Rounds | Rework |
|---|---|---|---|---|
| 3 port (hybrid, frontier U) | Sonnet 5.5, one agent | not measured; about 110 tool calls | 0 oracle rounds: after the first run every V8 comparison passed except one wrongly stated test expectation (`cΣc` has no final sigma by construction, threshold of the test fixed) | clippy `manual_is_multiple_of` x2 and the banned `str::trim` in two tests (replaced by `uo_jsnum::text::trim*`); a composition-pair check in the generator that was stricter than the algorithm (`Â + U+0323` composes through a reordering) moved to the Rust tests; the NFC mark sort replaced after the wasm size showed the library merge sort cost 10 KB; no V8 mismatch in any table |
