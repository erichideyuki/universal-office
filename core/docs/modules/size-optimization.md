# Size of the viewer module — removing code bloat (plan)

Plan of the rust-port workflow, stage 5 ("Evolution", `simplification-algorithms`): shrink the viewer module
(`cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm`) by removing duplicated
monomorphised code, **without changing any result** and **without slowing the hot paths** (layout, pages, PDF).
Parity is the safety net; the two changes that could alter a result or cost speed are separate decisions (D-1, D-2)
for the maintainer. Written 2026-11-06 against `ba3e15b9`; every "measured" number below comes from a throwaway build
of that commit plus the stated patch, in `core/target/plan-size` (reverted; the tree was left as found).

Implementer: Sonnet for items 0–3a and 4; **items 3b and D-1 touch `uo-docx-layout` and run on Opus**
(`.claude/skills/rust-port/SKILL.md`, "Models": every role of `uo-docx-layout`). Reviewer: Opus.

## Summary

| Step | Raw | gzip -9 | brotli 11 | Speed | Basis |
|---|---|---|---|---|---|
| baseline `ba3e15b9` | 5,960,000 | 1,465,195 | 1,055,236 | — | measured |
| item 1 — one signature-member sort | 4,281,147 (−28.2%) | 1,445,881 (−1.3%) | 1,047,542 | unchanged | measured |
| item 2 — one JSON walker | 4,017,179 (−32.6%) | 1,425,281 (−2.7%) | 1,036,706 | unchanged (layout +0.7%, noise) | measured (prototype) |
| item 3 — one sort core for cold sorts | ≈ 3,960,000 (−34%) | ≈ 1,408,000 (−3.9%) | ≈ 1,027,000 | cold paths only | estimated (marginal shares) |
| D-1 (decision) — NaN-capable layout sorts on the core | ≈ −49,000 more | ≈ −10,000 more | ≈ −7,000 more | — | estimated; changes order only for NaN keys, towards V8 |
| D-2 (decision) — uo-docx at `opt-level = "s"` | ≈ −125,000 more | **≈ −68,000 more** | ≈ −31,000 more | layout +3.3%, open +14%, total +2.4% | measured on items 1+2 |

So the code-bloat work proper (items 1–3) takes a third off the raw module (less to download uncompressed, validate,
compile and keep in memory) but **only ≈ 4% off the gzip transfer**, because gzip and brotli already fold byte-identical
copies; the transfer-size levers are D-2 (speed trade) and the "not now" splits (section 7).

## 1. What the module is made of (measured, corrected)

Baseline `ba3e15b9`: **5,960,000 B raw / 1,465,195 B gzip -9 (CLI) / 1,055,236 B brotli 11** (as in `uo-bindings.md`,
G7). Code 5.52 MB, data 0.42 MB (161 KB of the gzip).

**Finding: the earlier attribution was shifted by one function.** `core/target/size/wasm-size.mjs` and
`wasm-families.mjs` (uncommitted) skip the import section with `p += leb()`, which reads `p` before `leb()` advances
it, so they count 0 imported functions instead of 1 and give every body the name of the *next* function. Family
totals were roughly right (neighbours share a family), single functions were not: "`FlowState::clone` 31 KB" is
really `Engine::layout_static_in`; `FlowState::clone` is 1.8 KB. "`uo_pdf::num::Out::write_fmt` 24.7 KB" is
`LineBreaker::next_in`; `core::fmt` as a whole is 14.6 KB, not 84 KB. The committed tools (item 0) fix this; the
numbers below use them.

By owner (names kept, `node core/tools/size/wasm-size.mjs <names-kept.wasm>`): `core` 2.04 MB (of which
`core::slice::sort` **1.83 MB in 497 functions**), `uo_docx` 1.37 MB, `uo_docx_layout` 0.93, `uo_render` 0.18,
`hashbrown` 0.17, `alloc` 0.16, `uo_fonts` 0.11, `uo_pdf` 0.10, `uo_opc` 0.09, `uo_drawingml` 0.07, `indexmap` 0.06.

**Raw bytes are not transfer bytes.** gzip/brotli already fold code that is duplicated byte for byte, so deleting
1.68 MB of identical sort copies saves 28% raw but only 1.3% gzip. The tool `wasm-share.mjs` gives the *marginal*
compressed cost of a set of functions (gzip of all bodies minus gzip without the set); it predicted −20.2 KB gzip for
item 1 and the build measured −19.5 KB. Marginal shares on the baseline (upper bounds of what removing a set saves):

| Set (regex over demangled names) | fns | raw | gzip share | brotli share |
|---|---|---|---|---|
| `core::slice::sort::` (all) | 497 | 1,830,773 | 66,979 | 36,553 |
| — of which inside `object_json_with<T>` (127 types × 3 fns) | 382 | 1,688,830 | 20,850 | 6,820 |
| `<uo_docx::model::* as JsValue>::json` | 132 | 270,559 | 27,238 | 15,359 |
| `<uo_docx::model::* as JsValue>::exact` | 93 | 40,176 | 10,808 | 6,927 |
| `… as JsObject>::*` (mark, key_index, slots_in_order) | 32 | 14,925 | 5,149 | 3,200 |
| `js_object!` `ASSIGN`/`REMOVE` closures | 288 | 35,378 | 7,225 | 4,952 |
| every `Clone::clone` | 163 | 167,733 | 72,649 | 40,827 |
| `drop_glue` | 481 | 139,372 | 30,340 | 17,747 |
| `hashbrown::` + `indexmap::` | 475 | ≈ 231,000 | ≈ 68,700 | ≈ 39,700 |
| `uo_pdf` (+ `uo_layout_pdf`) | 91 | 115,604 | 46,049 | 34,185 |
| `uo_image` | 27 | 35,814 | 15,325 | 12,536 |
| `miniz_oxide::deflate` | 11 | 20,577 | 8,273 | 6,715 |

The sort instantiations that are not `object_json_with` (24 call sites in the module; element type, caller, raw):

| Site | Element | Comparator | Raw | Kind |
|---|---|---|---|---|
| `uo-fonts/src/cmap.rs:122` | `u32` | `sort_by_key(range_start)` (u32) | 16,196 | consistent |
| `uo-docx-layout/src/annotations.rs:155` | `CommentEntry` | `d < 0 / d > 0` on page/y/x | 12,636 | **NaN-capable** |
| `uo-jsnum/src/object.rs:26` (`js_object_order<ZipEntry>`) | `(String, ZipEntry)` | `sort_by_key(array_index)` | 9,960 | consistent |
| `uo-docx-layout/src/engine/page.rs:708` | `&ZObj` | z then seq, `partial_cmp(0)` | 7,787 | **NaN-capable** |
| `uo-drawingml/src/raster.rs:378` (`fill_polygons`) | `(f64, i32)` | `total_cmp` | 7,432 | consistent, **hot** (per scan line) |
| `uo-fonts/src/registry/cache.rs:242` | `(u64, ContentKey, u64)` | `sort_unstable_by_key(used)` | 7,330 | unstable |
| `uo-docx-layout/src/engine/mod.rs:578` (`z_sorted`) | `ZGroup` | z then seq | 7,057 | **NaN-capable** |
| `uo-fonts/src/hyphen/liang.rs:100` | `(u32, u32)` | `sort_unstable_by_key(cp)` | 6,850 | unstable (unique keys) |
| `uo-opc/src/package.rs:740` | `(u64, u64, usize)` | `sort_unstable()` | 6,199 | unique tuples |
| `uo-docx/src/read/drawing.rs:526` | `GradStop` | `pos.partial_cmp` | 6,121 | consistent (pos never NaN) |
| `uo-fonts/src/subset.rs:244` | `(TableRecord, u64)` | `sort_by_key(tag)` | 5,867 | consistent |
| `uo-docx-layout/src/engine/flow.rs:1662` | `&PassLine` | `d = a.y - b.y` | 5,841 | **NaN-capable** |
| `uo-docx-layout/src/drawing.rs:1118` (`fill_of`) | `(f64, Vec<u16>, f64)` | `d = a.0 - b.0` | 5,330 | **NaN-capable** |
| `uo-docx-layout/src/engine/tables.rs:1585` | `(FloatOut, usize, PassItems)` | `Reverse(n.1)` (usize) | 5,325 | consistent |
| `uo-opc/src/xml/read.rs:565` (`js_key_order`) | `(u32, PAttr)` | `sort_by_key(x.0)` | 5,391 | consistent |
| `uo-docx-layout/src/engine/tables.rs:818` | `Ins` | `Reverse(n.at)` (i64) | 5,229 | consistent |
| `uo-docx-layout/src/engine/floats.rs:396` | `&PassLine` | `d = a.y - b.y` | 4,626 | **NaN-capable** |
| `uo-opc/src/xml/read.rs:355` (`Parser::finish`) | `(u32, u32, u32)` | `sort_unstable_by_key(r.0)` | 4,597 | unstable |
| primitive `sort_unstable()` (`u16`, `u32`, `u64`) | | `PartialOrd::lt` | 12,107 | unstable, already 1 per type |

`word::obj::js_sort` (the V8 `Array.prototype.sort` port) has 7 instantiations, 5,847 B.

## 2. Items, in order (biggest and safest first)

Every item is one commit with its own size and speed measurement (section 4). Do them in this order; each one's
"expected" is on top of the previous items.

### Item 0 — the size tools (done with this plan)

Committed in `core/tools/size/` (Node 24, `wasm-tools` on PATH for `demangle`/`strip`):

- `wasm-read.mjs` — module reader shared by the tools (sections, bodies, demangled names; the import bug fixed).
- `wasm-size.mjs <file.wasm> [--group owner|generic|sort|name] [--filter re] [--top N]` — raw/gzip/brotli,
  sections, code grouped.
- `wasm-share.mjs <names-kept.wasm> <regex>…` — marginal gzip/brotli share of each function set.
- `wasm-ab.mjs [--runs 21] <base.wasm> <new.wasm>…` — interleaved speed of several modules in one process (open,
  layout, pages, PDF, total: sum over 4 fixtures of min and median) and the digest of everything a viewer reads from
  the 12 committed fixtures (`tests/wasm2js_common.mjs` `collect`). **Separate processes are useless on this machine**
  (the same module varied 143–219 ms on one fixture between runs); only the interleaved minimum is comparable.

Delete the superseded `core/target/size/*.mjs` (they are outside git) or leave them; never quote them again.

### Item 1 — one sort for the signature members of declared objects (measured)

**Change.** `crates/uo-docx/src/js/object.rs`, `object_json_with`: the line
`all.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));` is inside a function generic over `T: JsObject`,
so the closure has a different type per model type and instantiates the whole std stable sort 127 times (3 functions,
13.4 KB each). Move it to one non-generic function:

```rust
/// The members of a signature object in UTF-16 code-unit order of their names (G23): one sort for every type.
#[inline(never)]
fn sort_members(all: &mut [(&str, &dyn JsValue)]) {
    all.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
}
```

and call `sort_members(&mut all)` there. Optional, native/test only (no viewer bytes, smaller test binaries): the
same pattern in `object_canon_with` (`all.sort_by(… utf16_cmp …)`) and `JsRecord::canon` (`js/record.rs:115`), both
under `feature = "canon"`.

**Why results cannot change.** Same algorithm (`slice::sort_by`), same element type `(&str, &dyn JsValue)`, same
comparator, same input: the sort performs the same comparisons in the same order; only the code is shared.

**Expected (measured):** raw **−1,678,845** (5,959,992 → 4,281,147, −28.2%), gzip **−19,540** (−1.3%), brotli −7,694.
Digest of the 12 fixtures identical; layout/pages/PDF unchanged (the sorted path is the D1b signature writer's).

**Suites:** `cargo test --release -p uo-docx --all-features` (the D1b signature tests — `memory_d1b_writer`,
`nopanic_d1b_writer`, `parity_write_roundtrip`, `parity_lossless`, `memory_w1`–`w3` — exercise `SigMode::Canonical`).

### Item 2 — one JSON walker for every declared object (measured)

**Change.** `crates/uo-docx/src/js/object.rs` (+ the re-export in `js/mod.rs`). Today `object_json<T>` and
`object_json_with<T>` are inlined into every `<Model as JsValue>::json` (132 types, 2–3.5 KB each). Add an
object-safe view with a blanket impl, and make the two JSON functions take it:

```rust
/// The object-safe view of a declared object: the JSON walker exists once, each type only has this vtable.
pub trait JsObjectDyn {
    fn js_keys(&self) -> &'static [&'static str];
    fn js_slot(&self, i: usize) -> Slot<'_>;
    fn js_key_order(&self) -> &KeyOrder;
}
impl<T: JsObject> JsObjectDyn for T { /* T::keys(), self.slot(i), self.key_order() */ }

/// `JsObject::for_each_in_order` over the view (same body).
#[inline(never)]
fn for_each_in_order_dyn<'a>(o: &'a dyn JsObjectDyn, f: &mut dyn FnMut(&'static str, Slot<'a>)) { … }

#[doc(hidden)]
#[inline(never)]
pub fn object_json(o: &dyn JsObjectDyn, w: &mut JsonWriter<'_>) { … for_each_in_order_dyn(o, …) … }
#[inline(never)]
pub fn object_json_with(o: &dyn JsObjectDyn, before: &[…], after: &[…], w: &mut JsonWriter<'_>) {
    // sorted path: collect the `Slot::Val` members with for_each_in_order_dyn (replaces o.slots_in_order()),
    // then item 1's sort_members
}
```

Callers keep compiling unchanged (`object_json_with(self, …)` in `model/run.rs`, `table.rs`, `section.rs` coerces
`&Model` to `&dyn JsObjectDyn`; `section.rs:226` passes `self.section`). **Do not touch**, measured as regressions
(prototype J and J2):

- `JsObject::for_each_in_order` / `slots_in_order` stay the static default methods: `word::obj::key_indices` (2–2.5%
  of the whole profile) walks them on the layout path; routing them through the view cost **layout +3.5%**.
- `JsValue::exact` and `__exact_fields` stay as generated: a dyn version (−41.7 KB raw, −11.1 KB gzip more) cost
  **open +38%** (the reader interns every props object).
- `JsObject::key_index` stays inlined (an out-of-line version was not smaller).

**Why results cannot change.** The walker is the same code over the same `Slot`s in the same order; only the call of
`slot(i)` becomes a vtable call. Keys, `undefined` omission and numbers are written by the same `JsonWriter` calls.

**Expected (measured, prototype J4 on top of item 1):** raw **−263,968** (4,281,147 → 4,017,179), gzip **−20,600**
(→ 1,425,281), brotli −10,836. Digest identical; interleaved min: layout +0.7% (noise level), open, pages, PDF equal.

**Suites:** `uo-docx --all-features` (JSON tests in `r4c_unit`, `memory_r1`, `props_unit`, `word_unit`, the
`parity_*` suites), `uo-docx-layout` `f0` (`json_keys_follow_the_js_rules`) and the layout parity (JSON keys: bdrKey,
the shaping key of `r.ot`, `json_eq`), the wasm tests.

**Speed guard:** `wasm-ab.mjs` layout and total min within +2% of the item-1 module (repeat once on a miss).

### Item 3 — one sort core over indices for the cold sorts (estimated)

**Change.** New module `crates/uo-jsnum/src/sort.rs` (`pub mod sort;` in `lib.rs`; uo-jsnum is the one crate every
caller already depends on, and `Array.prototype.sort` is JS semantics like the rest of the crate):

```rust
/// The permutation `arr.sort(cmp)` gives in V8 (third_party/v8/builtins/array-sort.tq), over indices: `cmp(i, j)`
/// compares the elements at ORIGINAL indices i and j and answers like a JS comparator (< 0: i first; NaN is
/// "not less"). Below 64 elements: CountAndMakeRun + BinaryInsertionSort exactly as `word::obj::js_sort` does today;
/// from 64: the same top-down stable merge (split at len / 2, take right only when cmp(right, left) < 0), with one
/// scratch buffer instead of a Vec per level. One instantiation for every element type.
pub fn js_sort_order(n: usize, cmp: &mut dyn FnMut(usize, usize) -> f64) -> Vec<usize>;

/// Applies `order` (position k receives the element at original index order[k]) in place, by cycles of swaps.
pub fn permute<T>(v: &mut [T], order: Vec<usize>);

/// `arr.sort(cmp)` with a JS comparator (what `word::obj::js_sort` is today).
pub fn js_sort_by<T>(v: &mut Vec<T>, mut cmp: impl FnMut(&T, &T) -> f64);

/// A stable sort by a Rust ordering through the same core: identical to `slice::sort_by` for every consistent
/// comparator (a stable sort's output is unique). Less = -1, Equal = 0, Greater = 1.
pub fn sort_stable_by<T>(v: &mut [T], cmp: impl FnMut(&T, &T) -> core::cmp::Ordering);
pub fn sort_stable_by_key<T, K: Ord>(v: &mut [T], key: impl FnMut(&T) -> K);
```

Then:

- **3a (Sonnet):** `uo_docx::word::obj::js_sort` becomes `uo_jsnum::sort::js_sort_by(v, cmp)` (its callers in
  uo-docx-layout — `derive.rs:246`, `floats.rs:586/595`, `table.rs`, `table/widths.rs`, `table/layout.rs` — are not
  edited); convert the consistent cold sites: `uo-fonts/src/cmap.rs:122`, `uo-fonts/src/subset.rs:244`,
  `uo-jsnum/src/object.rs:26`, `uo-opc/src/xml/read.rs:565`, `uo-opc/src/package.rs:740` (`sort_unstable` on unique
  `(start, end, index)` tuples: no ties, any sort gives the same order), `uo-docx/src/read/drawing.rs:526`
  (`pos = n(or0(pint(…)) / 100000)`: `or0` maps NaN to 0, ±Infinity compare consistently with `partial_cmp`).
- **3b (Opus, uo-docx-layout):** `engine/tables.rs:818` (`Reverse(n.at)`, i64) and `engine/tables.rs:1585`
  (`Reverse(n.1)`, usize) → `sort_stable_by_key`.

**Not converted:** `uo-drawingml/src/raster.rs:378` (per scan line of every raster effect: hot), every `sort_unstable*`
other than `package.rs:740` (their order with ties is the unstable algorithm's — `cache.rs:242` eviction ticks,
`xml/read.rs:355` — and they would need a uniqueness proof for little gain), the primitive `sort_unstable()` calls
(one instantiation per primitive type already), and the NaN-capable layout comparators (decision D-1).

**Why results cannot change.**
- `js_sort` callers: the index core makes the same comparator calls, in the same order, on the same logical
  elements, so even inconsistent (NaN) comparators give the same permutation. Required test (in `uo-jsnum`): keep
  today's `js_sort` body as the reference in the test file and assert identical output for random vectors, lengths
  0..300, values with NaN, ±0, ±Infinity and duplicates, comparator `a - b`, plus a seeded comparator whose answer
  depends only on the call count (proves the same call sequence), plus the existing V8 vectors of
  `word::obj::tests::js_sort_is_stable_and_takes_nan_as_not_less`.
- std sites: each converted comparator is a consistent total preorder (table above); a stable sort of a consistent
  comparator has exactly one output, so driftsort and the index core agree. Required test: for each key type used
  (u32, i64 `Reverse`, usize `Reverse`, `Tag`, `Option<u32>`, f64 with ±Infinity and duplicates), random vectors with
  many ties: `sort_stable_by(_key)` == `slice::sort_by(_key)`.
- Memory: the order vector (n `usize`) and, from 64 elements, one scratch of n `usize`; covered by the crates' M1–M3
  tests (`uo-jsnum` `memory`, `uo-fonts` `foundation_memory`, `uo-opc` `package_memory`).

**Expected (estimated from the marginal shares, on top of item 2):** the converted std sites are 60.3 KB raw / 20.8 KB
gzip / 13.0 KB brotli of today's module, `js_sort` 5.8 KB raw; the new code is one core (≈ 2 KB) plus a small
`permute` + closure per element type (≈ 0.3–0.5 KB each, ≈ 15 types): **raw ≈ −55 to −60 KB, gzip ≈ −16 to −19 KB,
brotli ≈ −10 KB.**

**Speed risk:** small, all sites are cold (once per open, face parse, subset, table layout pass). The largest `n` is
`cmap.rs:122` (cmap ranges of a face; hundreds in the open fonts, up to the CmapRanges limit) — a merge sort with an
indirect comparator is ≈ 2× slower than driftsort on large inputs. Guard: `wasm-ab.mjs` open/layout/total min within
+2%, and `cargo test --release -p uo-fonts --test face_parse --test real_fonts` not measurably slower (print the
test time before/after).

**Suites:** all of section 4, with the layout parity (3b and the `js_sort` callers).

### Item 4 — records

Update `uo-bindings.md`, "Size analysis of the viewer module (G7)": the corrected attribution (section 1, the
off-by-one note), the measured rows of items 1–3 (raw/gzip/brotli after each), and point here. Record the speed
rows of `wasm-ab.mjs` for each item.

## 3. Decisions for the maintainer (results or speed)

### D-1 — the NaN-capable layout sorts on the JS core (changes results only where they are wrong today)

Sites: `annotations.rs:155`, `engine/page.rs:708`, `engine/mod.rs:578` (`z_sorted`), `engine/flow.rs:1662`,
`engine/floats.rs:396`, `drawing.rs:1118`. Each reproduces a TS `sort((a, b) => a.k - b.k …)` with `d < 0 / d > 0`
(or `partial_cmp(0).unwrap_or(Equal)`), so a NaN key answers "equal" to everything: the comparator is then
inconsistent and the order is whatever the algorithm does — **driftsort's today, not V8's** (the TS reference runs
V8 TimSort). `js_sort_by(v, |a, b| a.k - b.k)` reproduces V8 exactly below 64 elements (`js_sort`'s existing
evidence) and stays identical for every NaN-free input. Saving ≈ 49 KB raw / ≤ 12.7 KB gzip / ≤ 8.3 KB brotli.
Evidence needed before switching (Opus): a NaN input per site run through the TS engine (frozen main) and both Rust
versions; switch only where V8's order is reproduced. If NaN cannot reach a site (prove from the producer), it is a
pure item-3 conversion.

### D-2 — `opt-level = "s"` for uo-docx in the wasm profile (a speed trade)

`[profile.wasm.package.uo-docx] opt-level = "s"` in `core/Cargo.toml` (affects only `--profile wasm`). Measured with
`--config 'profile.wasm.package.uo-docx.opt-level="s"'` (fat LTO keeps the per-function size attributes):

| On top of | Raw | gzip | Layout (min) | Open | Total |
|---|---|---|---|---|---|
| item 1, `"s"` | −181,088 | **−84,914** | +2.7% | +9% | +2.4% |
| items 1+2, `"s"` | −124,768 | **−67,959** | +3.3% | +14% | +2.4% |
| item 1, `"z"` | −438,991 | −155,994 | +6.6% to +8.8% | +60% | +5.2% |

This is the largest transfer saving available without splitting the module, but it slows the layout: the profile
shows uo-docx code inside the layout (`word::obj::key_indices`, `model::story::Blocks::next`, `word::resolve::{
cascade_rpr, merge_ppr, merge_rpr, font_of_slot}`, `word::direct::direct_rpr_with`, `KeyOrder::touch`). Proposal: take
`"s"` only if the maintainer accepts ≤ +3% layout/total and ≤ +15% open (1–3 ms per document); a follow-up may try
`#[inline]` on those hot functions (an `#[inline]` body is codegened in the calling crate at its opt-level) and
re-measure. The wasm2js pipeline builds the whole module at `"z"` (`CARGO_PROFILE_WASM_OPT_LEVEL=z`): a package
override still wins there, so that build must pass `--config 'profile.wasm.package.uo-docx.opt-level="z"'` too.

## 4. Acceptance (every item)

Build (one at a time, `-j 4`; own target dir if another session builds):

```bash
cd core
cargo build -j 4 -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm      # shipped module
gzip -9 -c target/wasm32-unknown-unknown/wasm/uo_wasm.wasm | wc -c
CARGO_PROFILE_WASM_STRIP=false CARGO_TARGET_DIR=target/size \
  cargo build -j 4 -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm    # names kept
node tools/size/wasm-size.mjs target/size/wasm32-unknown-unknown/wasm/uo_wasm.wasm --group sort --top 40
node tools/size/wasm-ab.mjs --runs 21 <module of the previous item>.wasm target/wasm32-unknown-unknown/wasm/uo_wasm.wasm
```

Keep a copy of each item's shipped module (outside git) as the next item's base. Pass criteria:

1. **Size:** raw, gzip (CLI) and brotli recorded; each item at least 50% of its expected gzip saving (item 1:
   the raw saving), none larger than before.
2. **Same output:** `wasm-ab.mjs` digest identical to the base module's (12 fixtures, every byte a viewer reads).
3. **Speed:** `wasm-ab.mjs` minimum of layout, pages, PDF and total within **+2%** of the base, open within **+5%**
   (repeat the run once on a miss; two misses fail). D-2 uses the thresholds the maintainer sets.
4. **Native parity and memory:**
   `cargo test --release -p uo-docx -p uo-docx-layout -p uo-render -p uo-pdf -p uo-fonts -p uo-opc -p uo-drawingml -p uo-jsnum --all-features`
   and `cargo test --workspace --all-features` (CI's test job; includes the wasm native tests
   `cargo test -p uo-wasm --features write --test write_native` with the pinned PDF checksums).
5. **Layout parity** (items 2, 3, D-1, D-2): the `layout-parity` job of `.github/workflows/rust-core.yml` (frozen TS
   engine from `main`, `capture-layout.mjs` + `--test adapter`, `--test layout_parity` with `UO_LAYOUT_PARITY=1`,
   the f7a/f7b/f7c/f7d strict runs and `--test f8_parity`), locally with a built `main` checkout as `--ts-root`.
6. **wasm tests:**
   `cargo build -p uo-wasm --features count,write --target wasm32-unknown-unknown --profile wasm` then
   `UO_WASM_PATH=<that .wasm> node --expose-gc --test crates/uo-wasm/tests/write_api.test.mjs` and
   `UO_WASM_PATH=<that .wasm> node --test crates/uo-wasm/tests/write_section.test.mjs`; plus the CI wasm job's other builds
   (`count,opc-memtest` → `boundary`, `opc_memory`; `fonts-memtest`, `fonts-selftest`, `dml-memtest`,
   `dml-selftest` → their `.test.mjs`) for items touching uo-opc, uo-fonts, uo-drawingml or uo-jsnum (item 3).
7. **Lint:** `cargo fmt --all --check`;
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
   `cargo clippy --workspace --all-targets --all-features --target wasm32-unknown-unknown -- -D warnings`;
   `bash tests/clippy-ban/check.sh`; `bash tests/float-casts.sh`; `bash tests/grep-bans.sh`.

## 5. Traps

- **Inconsistent comparators.** std stable sort and any other stable sort agree only for consistent comparators.
  The six NaN-capable layout sites are D-1, not item 3; `read/drawing.rs:526` is consistent (proof in item 3). The
  comparators must keep their current answers exactly (`d < 0` / `d > 0`, `partial_cmp(...).unwrap_or(Equal)`): no
  `total_cmp` rewrites (it orders −0 before +0 and NaN by sign — a different order).
- **`sort_unstable` keeps its exact current order** only where there are no ties; `package.rs:740` is the only one
  converted (its tuples end with the unique entry index). `cache.rs:242`, `liang.rs:100`, `xml/read.rs:355` and the
  primitive ones stay.
- **The V8 core must replay `js_sort` call for call** (`order >= 0` breaks a descending run, `< 0` an ascending one:
  NaN breaks neither), not a "less" predicate — `!(x < 0)` is not `x >= 0` for NaN.
- **`clippy.toml` bans `f64::max/min/clamp/round…`**: the core only compares (`< 0.0`, `>= 0.0`); no float → int `as`
  (`float-casts.sh`).
- **Measurement.** Names kept (`CARGO_PROFILE_WASM_STRIP=false`) + `wasm-tools strip --all` equals the shipped
  module within 8 B; record shipped numbers from the plain build. Node's `zlib` gzip -9 is ~0.6% above the gzip CLI
  (records use the CLI). Speed only through `wasm-ab.mjs` (interleaved, minimum).
- **`uo-docx-layout` is Opus-only** (3b, D-1). 3a changes uo-docx's `js_sort`, which the layout calls: the layout
  parity is part of 3a's acceptance even though no layout file changes.
- **Do not route the hot `JsObject` walkers or `exact` through `dyn`** (item 2's measured regressions).

## 6. Measured and rejected

| Idea | Raw | gzip | Cost | Verdict |
|---|---|---|---|---|
| `-C llvm-args=-inline-threshold=50` (whole module), on item 1 | −344,914 | −73,608 | layout +8.4%, total +8.1% | rejected: D-2 `"s"` saves more for a third of the cost |
| `opt-level = "z"` for uo-pdf, uo-opc, uo-image, miniz_oxide, on item 1 | −38,285 | −13,996 | PDF +13.8%, open +30% (miniz is 13% of the profile) | rejected |
| `exact` through the dyn view (on item 2) | −41,662 | −11,054 | open +38% | rejected |
| `for_each_in_order` through the dyn view | (part of the above) | | layout +3.5% | rejected |
| `wasm-opt -O3`/`-Oz` (uo-bindings.md) | −10% / −17% | −0.2% / +2.3% | — | no transfer gain (recorded) |

## 7. Not now (with their size)

- **PDF in a second module** loaded on print/export: `uo_pdf` 46 KB gzip share + `miniz_oxide::deflate` 8 KB + the
  image encoders of `uo_image` (≤ 15 KB): **≈ 60–70 KB gzip off the start module** (raw ≈ 170 KB). Needs a second
  `cdylib` or a split loader and the layout handed across; its own plan.
- **Data section:** 424 KB raw / **161 KB gzip** (11% of the transfer: Unicode tables, font catalogue, codec tables).
  Attribute first (`-C link-arg=--Map=…` from wasm-ld, or `wasm-tools objdump` of the data segments), then decide
  per table (on-demand chunk like the hyphenation patterns, or compression).
- **`Clone` (168 KB raw / 73 KB gzip) and drop glue (139 KB / 30 KB):** distinct per model type
  (`InlineImage::clone` 13 KB, `RPr` 9 KB, `ShapeStyle` 8 KB…); only a data-model change (shared `Rc`/`Arc` props,
  fewer `Option<Box<…>>` fields) removes it. Not a code-size task.
- **Map instantiations:** `hashbrown`/`indexmap` ≈ 69 KB gzip in 475 functions (`RawTable::reserve_rehash` alone
  133 KB raw in 132 key/value types). One map *type* does not help (it is per K, V); fewer distinct K/V types would.
  Dropping `RandomState` (SipHash, 17 KB raw, ≈ 5 KB gzip) for `FxHashMap` is possible only where keys are not
  document-controlled strings (hash flooding on native hosts) and order is not observed; small, not now.
- **`core::fmt`:** 14.6 KB in total (Debug paths of error values among it): too small.
- **`opt-level = "z"` for uo-docx:** see D-2 (−156 KB gzip, but layout +7–9% and open +60%).

## 8. Results (implemented 2026-11-07, items 0-3a and 4)

Measured on the shipped module (`--features write --profile wasm`), sizes in bytes: raw / gzip -9 (CLI) / brotli 11
(Node `zlib`, window 24; the brotli of the baseline is 1,050,575 with these parameters, 1,055,236 in the planning
tables). Speed: `wasm-ab.mjs --runs 21`, each item against the previous one in one process, minimum in ms; the
digest of the 12 fixtures was `cca8d8794ec4bfe0` for all four modules.

| Step | Raw | gzip | brotli | Speed vs previous (min) |
|---|---|---|---|---|
| baseline `ba3e15b9` | 5,960,000 | 1,465,192 | 1,050,575 | - |
| item 1 (`sort_members`) | 4,281,155 (-1,678,845) | 1,445,744 (-19,448) | 1,046,845 (-3,730) | open -0.7%, layout +0.1%, pages -1.2%, PDF +0.5%, total +0.1% |
| item 2 (`JsObjectDyn`, J4 variant) | 4,014,011 (-267,144) | 1,422,858 (-22,886) | 1,034,548 (-12,297) | open -2.1%, layout -0.2%, pages -0.5%, PDF +0.3%, total 0.0% |
| item 3a (shared sort core, 6 sites + `js_sort`) | 3,960,432 (-53,579) | 1,405,088 (-17,770) | 1,023,890 (-10,658) | open +0.2%, layout 0.0%, pages -2.0%, PDF +0.1%, total -0.5% |
| **total** | **3,960,432 (-33.5%)** | **1,405,088 (-4.1%)** | **1,023,890 (-2.5%)** | base vs final: open +0.4%, layout +1.3%, pages -1.8%, PDF +0.1%, total +0.9% |

All three items are within the plan's estimates (item 3a: gzip -17.8 KB against the expected -16 to -19 KB). The
item 3a row is the module rebuilt from the final tree by the review (the kept `i3.wasm` predated the `&mut [T]`
signature change: 374 B more code, same digest); the planning tables' 1,465,195 B gzip baseline is the same module
compressed in another run (3 B apart), `keep/base.wasm` gives 1,465,192 with `gzip -9`.

Review (Opus, 2026-11-07): no result change, no new panic, no hot-path slowdown; `js_sort_order` traced call for
call against the old `js_sort` and 13 mutants run (every behaviour-changing one caught). Left for later: (a)
`uo-fonts/src/cmap.rs:122` now sorts through an index order (`Vec<usize>`, ~12n bytes extra natively, merge-sort
comparisons where the old sort was O(n) on sorted input) — negligible for real fonts, a cost only for a hostile
font near `max_cmap_ranges`: use a `u32` order or skip the sort when already ascending; (b) the call-count test in
`tests/sort.rs` proves the same calls and decisions, not the same `(i, j)` argument pairs — log and compare the pairs.

Notes:

- Item 3a files: `uo-jsnum/src/sort.rs` (new; `js_sort_order`, `permute`, `js_sort_by`, `sort_stable_by`,
  `sort_stable_by_key`), `uo-jsnum/tests/sort.rs` (new), `uo-docx/src/word/obj.rs` (`js_sort` delegates and its
  `merge_sort` is gone), and the sites `uo-fonts/src/cmap.rs:122`, `uo-fonts/src/subset.rs:244`,
  `uo-jsnum/src/object.rs:26`, `uo-opc/src/xml/read.rs:565`, `uo-opc/src/package.rs:740`,
  `uo-docx/src/read/drawing.rs:526` (the line numbers did not move).
- Deviation: `js_sort_by` and `word::obj::js_sort` take `&mut [T]` instead of `&mut Vec<T>` (clippy `ptr_arg`;
  every caller passes `&mut Vec<T>`, which coerces, so no `uo-docx-layout` file was touched). `package.rs:740` uses
  `sort_stable_by(|a, b| a.cmp(b))` on the unique tuples.
- The `uo-jsnum` property tests (`tests/sort.rs`) keep today's `js_sort` as the reference: identical permutation for
  lengths 0..300 (NaN, +-0, +-Infinity, duplicates), for a comparator answering by call count (same call sequence,
  n up to 257), the V8 vectors, ties against `slice::sort_by(_key)` for u32, `Reverse<i64>`, `Reverse<usize>`, tags,
  `Option<u32>`, f64 with +-Infinity and unique tuples against `sort_unstable`, and `permute` on random permutations
  (and non-permutations: no panic, no loop).
- Not run: the `face_parse`/`real_fonts` before/after timing of the plan (only the interleaved `open` of `wasm-ab.mjs`,
  which parses the fonts of the fixtures: +0.2%).
