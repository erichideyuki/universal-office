# uo-docx D1a reader — frontier R4a: declarations

Part of the record `core/docs/modules/uo-docx.md` ("Milestone 1 — D1a foundation", brief §B). One
table row per site; every row names the TS file:line. Item 9 rows need an Opus check before they land.

Scope: `src/read/props_map.rs` ← `document.ts` `readBorderEdge` (584), `runPropsFromRPr` (706),
`themeRefOf` (743), `revInfo` (749), `editBaseProps` (766), `paraPropsFromPPr` (778), `readSectPr`
(2226) with `tw` (2208), plus `UNDERLINE_STYLES` (22) and `TIGHT_WRAP` (775). Public API:
`read_border_edge`, `run_props_from_rpr`, `rev_info`, `edit_base_props`, `para_props_from_ppr`,
`read_sect_pr`, `UNDERLINE_STYLES`. `readBorder` (R1, `props::read_border`) and `readNoteProps` (R2,
`parts::read_note_props`) are called, not duplicated. No model type was changed.

## 1. Numeric parse and rounding sites (uo-jsnum function used)

All through `uo_jsnum::parse::parse_int(v, 10)`; the result is stored as `JsNumber::Num` as it is
(NaN and ±Infinity included, see 3).

| TS site | What | Rust |
|---|---|---|
| document.ts:591 | `parseInt(w:sz ?? '4', 10)` (`BorderEdge.sizeEighthPt`) | `parse_int(attr.unwrap_or("4"), 10)` |
| document.ts:593 | `parseInt(w:space ?? '0', 10)` (`BorderEdge.spacePt`) | same |
| document.ts:2210 | `tw`: `parseInt(v, 10)` of the attribute, the fallback only when it is missing (`pgSz` w/h, `pgMar` left/right/top/bottom/header/footer: 2231–2238) | `tw()` |
| document.ts:2242 | `parseInt(w:num ?? '1', 10)` (`num`, `num > 1` guard) | `parse_int`; the guard is an f64 `>` |
| document.ts:2246 | `parseInt(w:space ?? '720', 10)` (`cols.spaceTwips`) | `parse_int` |
| document.ts:2254, 2255 | `parseInt(…, 10) \|\| 0` (`cols.widths`) | `or_zero` = `conv::truthy(x) ? x : 0.0` (NaN and ±0 become `+0`) |
| document.ts:2279–2280 | `parseInt(w:start)`, kept when `!Number.isNaN(n)` | `parse_int` + `is_nan()` |
| document.ts:2284–2285 | `parseInt(w:chapStyle ?? '')`, kept when `> 0` | `parse_int("")` is NaN, `> 0.0` |
| document.ts:2295 | `parseInt(w:val, 10) > 0` and `= parseInt(w:val, 10)` (`footnoteColumns`, parsed twice) | `parse_int` twice, same value |
| document.ts:2298 | `lnNumType` helper `parseInt(attr ?? '', 10)`, kept when not NaN | `parse_int` + `is_nan()` |

No rounding site. Comparisons that read a number: `rpr.w !== 100` (722), `rpr.spacing !== 0` (734),
`numId === 1002` (789): f64 comparisons (`-0 == 0`, NaN differs from everything, as JS).
`-ppr.ind.hanging` (795) is f64 negation (`0` gives `-0`, as JS; the digest keeps the sign).
`defs.length >= num` and `defs.slice(0, num)` (2252–2253): `conv::js_index(num, len + 1)`
(item 9). No float → integer cast anywhere (`float-casts.sh` clean).

## 2. Regex sites (template or matcher; dynamic ones named; row in `js/regex-sites/r4a.mjs`)

None. The seven functions use no regular expression (string equality and `includes` on a constant
list only), so there is no `js/regex-sites/r4a.mjs` and no `tests/regex_r4a.rs`.

## 3. Fields that can store NaN/±Infinity, keys stored as `undefined` (cleared bits)

Stored NaN/±Infinity (`JsNumber::Num`, F1; `parseInt` of a bad attribute, or of a digit string of
more than 308 digits, which is `Infinity`):

| Field | TS site | Values |
|---|---|---|
| `BorderEdge.sizeEighthPt`, `spacePt` (both from `readBorderEdge`) | 591, 593 | NaN, ±Infinity |
| `SectPr.pageWidthTwips`, `pageHeightTwips`, `marginLeft/Right/Top/BottomTwips`, `headerDistanceTwips`, `footerDistanceTwips` | 2231–2238 | NaN, ±Infinity |
| `SectPr.cols.spaceTwips` | 2246 | NaN, ±Infinity |
| `SectPr.cols.count` | 2245 | +Infinity only (`num > 1` excludes NaN and ≤ 1) |
| `SectPr.pgNumStart`, `footnoteColumns`, `lnNumType.countBy/start/distance` | 2280, 2295, 2300 | ±Infinity only (NaN is filtered) |
| `SectPr.pgNumChapStyle` | 2286 | +Infinity only (`> 0`) |
| `cols.widths[i].widthTwips/spaceTwips` | 2254, 2255 | never (`\|\| 0`) |

Fields copied from R1's typed props (`RunProps.scalePct`, `positionHalfPt`, `kernHalfPt`, `szHalfPt`,
`charSpacingTwips`, `ParaProps.indent*`, `line/space*`, `outlineLvl`, `numbering.ilvl/numId`,
`BorderEdge` of `paraPropsFromPPr` 820/822, tab `posTwips`) carry whatever the `RPr`/`PPr` hold:
`JsNumber` passes through unchanged, `null` included (see 9). R1 declares which of those can be NaN.

Keys stored as `undefined` (`mark_undefined`, cleared bit):

| Key | TS site | When |
|---|---|---|
| `RunProps.fontAscii` | 733 `p.fontAscii = rpr.rFonts.ascii` | `w:rFonts` without `w:ascii` |
| `BorderEdge.color` | 592 (`readBorderEdge`) and 821 (`paraPropsFromPPr`) | no colour, empty or `auto` |

`editBaseProps` (771) assigning `fontAscii` over such a key clears the bit (the key now holds a value, in
place).

## 4. Map/object iteration orders relied on (and key orders recorded with `mark`)

No `Map` and no object iteration. `Object.keys(borders).length > 0` (825) is a count (a flag).

Key orders (the digest's non-compared `order` facet is compared here anyway, see Parity):

| Object | Order | How |
|---|---|---|
| `RunProps` from `runPropsFromRPr` | `DEFAULT_RUN_PROPS` six keys, then the assignment order of 709–738 | the struct declares the fields in that order: no `mark` |
| `ParaProps` from `paraPropsFromPPr` | assignment order of 782–839 | the struct declares them in that order: no `mark` |
| `RunProps` from `editBaseProps` | the base's order, then `color` (769), `vertAlign` (770), `fontAscii` (771) appended when absent | `freeze_order` marks every present key in order, then `mark` of the added key (the recorded list must be the whole order) |
| `SectPr` | the 8 literal keys (2231–2238, `headerDistanceTwips` and `footerDistanceTwips` before `type`), then each key in assignment order; differs from the declaration order (`pgBorders` sits after `pgNumChapSep` there, `headerDistanceTwips` after `titlePg`) | `mark` of every key as it is assigned |
| `Cols`, `PageBorders`, `LineNumbering`, `BorderEdge`, `ThemeColorAttrs`, `FormatChange`, `ParaFormatChange`, `NumberingProps`, `ParaTabStop`, `RevisionInfo` | the object literal / assignment order of the TS | declaration order: no `mark` |

## 5. Recursions (made iterative, or bounded by `max_nesting`)

| TS site | Recursion | Rust |
|---|---|---|
| document.ts:738 `runPropsFromRPr(rpr.change.rPr)` | through `w:rPrChange` nesting | iterative: the `change.r_pr` chain is walked down into a `Vec<&RPr>` and the result built from the innermost out; no `max_nesting` involved (the depth is bounded by the XML depth limit 1000 at the reader, ≤ 500 levels) |
| document.ts:839 `paraPropsFromPPr(ppr.change.pPr)` | through `w:pPrChange` nesting | same |

Test: chains of 2,000 levels map without recursion (`run_props_chain_is_iterative`); dropping such
a chain recurses through `Box` drops (as any nested model), covered by the 16 MB-stack test thread and
bounded in practice by the reader's depth limit.

## 6. Module-level / process-global state and its per-read replacement

None: no module-level mutable state in the seven functions (`UNDERLINE_STYLES` and `TIGHT_WRAP` are
constants). Nothing replaces anything.

## 7. `try {…} catch {}` sites and the partial result kept

None in these functions.

## 8. Aliasing (two model positions sharing one TS object) and how A2 gives the same result

| TS site | Aliasing | A2 result |
|---|---|---|
| document.ts:736 `p.textFx = rpr.fx` and 737 `p.openType = rpr.ot` | the `RunProps` field is the same object as the `RPr` field (one object, two positions) | the model fields are `Box<TextEffects>` / `Box<OpenTypeProps>` (not `Arc`), so the mapping **clones** the value: the digest and the JSON are identical; the sharing is observable only if an edit mutates one of the two in place. For D2 (milestone 2): an operation that mutates `props.textFx`/`openType` in place in the TS must be checked against the `rpr` side (the layout's `directRPr` compares them); a clone per mapped run costs one allocation per effect (M3 budget, rare: effects are uncommon) |
| document.ts:738, 839 `old` | `old: runPropsFromRPr(rpr.change.rPr)` is a fresh object | no aliasing |
| document.ts:767 `base` | a fresh object each call, shares nothing with `run.props` | no aliasing |

## 9. Saturating/wrapping/truncating choices and data-shape deviations from A2

No change to a model type and no change of arithmetic semantics. The choices, for the Opus check:

| # | Site | Choice | Why it gives the TS result |
|---|---|---|---|
| 1 | document.ts:2252–2253 `defs.length >= num` and `defs.slice(0, num)` | `conv::js_index(num, defs.len() + 1)` gives `Some(n)` iff `num` is a non-negative integral value with `num <= len`; `take(n)` | `num > 1` holds and `num` is a `parseInt` result, so integral, never NaN; `Infinity` gives `None` as `defs.length >= Infinity` is false |
| 2 | `JsNumber::Null` (the post-undo state of a NaN/±Infinity, F1) | the reader never produces it, but the functions are the base the session reuses, so they follow JS: `x !== undefined` holds for `null`; `x ?? d` takes `d` (`or_default`); `x !== 100`/`!== 0` hold; truthiness is false (`JsNumber::truthy`); `-null` is `-0` (`neg`); `numId === 1002` is false | each rule is the JS rule for `null` |
| 3 | document.ts:2254–2255 `parseInt(..) \|\| 0` | `conv::truthy(x) ? x : 0.0` | NaN and ±0 give `+0` as `x \|\| 0` |
| 4 | F2 (string unions are `Str`) | `underlineStyle`, `alignment`, `vertAlign`, `textboxTightWrap`, `type`, `vAlign`, `pgNumChapSep`… keep the validated string, no narrowing | the TS stores the matched literal; the validation (`includes`, `TIGHT_WRAP`, literal chains) is ported |
| 5 | `RunProps.fmt` / `ParaProps.pfmt` | built from `RPrChange`/`PPrChange` through `rev_info_of` (author, `date`/`dateUtc` when present, `move: true` only when truthy) | `{ ...revInfo(change), old }` |
| 6 | `read_sect_pr` | returns `SectPr` by value (callers wrap it in `Arc`) | A2 stores `Arc<SectPr>`; the function has no sharing to preserve |
| 7 | `rev_info(&RevisionInfo)` | takes the model type; `RPrChange`/`PPrChange` (which spread a `RevisionInfo`) go through the private `rev_info_of(author, date, date_utc, move)` | one rule for all three |

## Parity and memory results

**Parity** (facet `props-map`, `tests/parity_props_map.rs`, capture `tests/parity/props-map.capture`
by `js/capture-props-map.mjs`, Node 24.15 x64, frozen TS `d0f45bc5`): every `w:rPr`
(`runPropsFromRPr` and `editBaseProps` over four runs: link, note reference, symbol font, all three),
`w:pPr` (`paraPropsFromPPr`), `w:sectPr` (`readSectPr`) and paragraph-border edge (`readBorderEdge`,
children of `w:pBdr`) of the `word/*.xml` parts of every git-tracked `.docx` (2,767 files: the 2,742
oracle documents and the other fixtures; 8,896 parts, 72,758 elements) and of the generated
attribute-quirk corpus (`tests/parity/props-map.quirks.txt`, seeded, 302 parts, 3,717 elements:
`"1e3"`, `" 12 "`, `"12pt"`, `"0x10"`, `"-0"`, `"NaN"`, `Infinity`, 310-digit strings, empty and
missing attributes, `rPrChange`/`pPrChange` nesting to 300 levels, shuffled children, `jc`
start/end/distribute, `numId` 1002, `hanging`, `equalWidth` with too few `w:col`, bad `chapStyle`, …)
plus `readSectPr(undefined)`. Replayed per part: `n` (elements), `props-map` (digest of the values)
and `order` (the key-order facet): **9,199 records, 27,597 facets, 27,597 equal, 0 mismatches, 0
accepted, 0 not replayed**, first run, with R1's `read_rpr`/`read_ppr`, `read_border` and R2's
`read_note_props`. Sensitivity checked by mutation (a wrong `numId` constant: 8 mismatches; the
`freeze_order` calls removed: 4,211; one `sect.mark` removed: 73).

The quirk corpus is R4a's own (inside `capture-props-map.mjs`), not R1's `quirk-corpus.mjs`.

**Unit tests** (`tests/props_map.rs`, 13): every mapping rule (alignment, numbering, indents, spacing,
borders, tabs, flags, theme colours, underline styles, `fontAscii` undefined, `change` chains, key
orders as JSON, section defaults, columns and widths, `Infinity` cases, page borders) and the no-panic
loop (3,000 random parts of random attribute values through `read_rpr`/`read_ppr` and the six
functions, 25,256 elements, 400-digit strings and non-BMP text included).

**Memory** (M1, `tests/memory_r4a.rs`, 3 tests): `run_props_from_rpr` (none, flat, 50-level chain),
`para_props_from_ppr` (same), `rev_info`, `edit_base_props` (three runs × three inputs),
`read_sect_pr` (full, empty, none), `read_border_edge` (edge, nil, none) and the mapping over a parsed
part: live bytes back to the start, 0 B each. The functions return values (no error path).

**Checks:** `rustfmt --check` on my files; `cargo clippy -p uo-docx --all-targets --all-features -- -D warnings`
and `--target wasm32-unknown-unknown` clean for my files (the crate as a whole fails only on other
frontiers' files in progress); `float-casts.sh` (uo-docx lib) and `grep-bans.sh` clean.

## Process metrics (wall time, rounds with the verifiers, rework)

Sonnet, one session. Port 1:1 first try; parity 0 mismatches on the first replay (the only first-run
difference was the test's own digest framing of `readSectPr(undefined)`); no round with a verifier
yet; no rework of the port. Tool calls: see the Process-metrics table of `uo-docx.md`.
