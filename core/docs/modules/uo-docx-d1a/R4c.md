# uo-docx D1a reader — frontier R4c: declarations

Part of the record `core/docs/modules/uo-docx.md` ("Milestone 1 — D1a foundation", brief §B). One
table row per site; every row names the TS file:line (`document.ts`). Item 9 rows need an Opus check
before they land.

Scope: `src/read/paragraph.rs` ← `document.ts` 1583–2213 (`textContentOf`, `runTarget`, `closeField`,
`readRunsInto`, `parseCustomInlineTag`, `readMathNodes`, `collectMathText`, `readSingleRun`,
`readParagraph`, `collectInstrText`, `buildReadContext`) and `src/read/json_parse.rs` (`JSON.parse`
and `decodeURIComponent` of the custom-inline tag). Public API of `paragraph.rs`: `ReadContext`
(`new` = `buildReadContext`, `read_paragraph` = `readParagraph`, `read_runs_into`, `resolve_image`,
`resolve_link`, `resolve_part`, `set_source_part`, the public fields `ctx`, `blocks`,
`collect_bookmarks`, `bookmark_events`), `BlocksMode`, `BookmarkEvent`, `ResolvedMedia`, `RunList`,
`parse_custom_inline_tag`, `is_toc_instr`, `is_custom_inline_type`. `readRPr`/`readPPr`/`readRevisionInfo`
(R1), `runPropsFromRPr`/`paraPropsFromPPr` (R4a), `readDrawingRun`/`readVmlRuns` (R4b, through
`inline_objects::with_context`) and `captureParagraph` with the segment types (R6,
`sources::capture_paragraph`, `PendingSeg`, `RunSide`, `inline_wrapper`) are called, not duplicated. No
model type was changed. `readDocument` (2213, the legacy flat reader) is not ported: see 9.12.

The shape of the port:

- the TS `ReadContext` object (`resolveImage`, `resolveLink`, `resolvePart`, `activeComments`,
  `bookmarkIds`, `fieldStack`, `onBookmark`/`onBookmarkEnd`, `readBlocks`/`readBlocksGrouped`,
  `sources`, `sourceXml`) is the struct `ReadContext` borrowing the read's `ReadCtx`: the resolvers are
  methods, the two bookmark callbacks are `BookmarkEvent`s queued when `collect_bookmarks` is set and
  drained by the consumer after each block (`fulldoc.ts:687` reads `blocks.length` when the callback
  runs: draining before the block is pushed gives the same value), the two block readers are the
  flag `BlocksMode` (`None` = no `readBlocks`; `Blocks`; `Grouped`) that R4b's host reads, `sourceXml`
  is `set_source_part` (the index `ReadCtx::source_part` gave the part's text);
- `readRunsInto` and `readSingleRun` (mutually recursive in the TS) are one loop over an explicit stack
  of frames (`Container` for a wrapper's children, `Run` for a `w:r`'s children): no depth of the XML
  uses the native stack. The TS symbol keys of a run (`RPR_EL`, `OBJ_EL`) are a `RunSide` next to each
  run in a `RunList` and go to R6's `capture_paragraph` with the segments.

## 1. Numeric parse and rounding sites (uo-jsnum function used)

All `parseInt` through `uo_jsnum::parse::parse_int`; results stay `f64` (`JsNumber::Num`) as they are.
No rounding site, no float → integer cast (`float-casts.sh` clean).

| TS site | What | Rust |
|---|---|---|
| document.ts:1646, 1649 | `parseInt(w:id ?? '-1', 10)` of `commentRangeStart`/`End`, the key of the `Set` | `int_attr(child, "w:id", "-1")`, `NumKey::new` (SameValueZero: NaN equals NaN, `-0` is `+0`) |
| document.ts:1654 | `parseInt(w:id ?? '', 10)` of a bookmark; `Number.isFinite(id)` | `parse_int`, `f64::is_finite` |
| document.ts:1658 | `runs.reduce((n, r) => n + r.text.length, 0)` (the bookmark offset) | `RunList::text_len`, a `usize` of UTF-16 units kept up to date at each push (the TS recomputes it per bookmark) |
| document.ts:1899 | `parseInt(size, 10)` (`FormField.sizeHalfPt`), only when the `w:size` value is a non-empty string | `parse_int`, `JsNumber::Num` |
| document.ts:1907–1909 | `parseInt(val ?? '', 10)` of `w:result`/`w:default`; `Number.isInteger(k) && k >= 0 && k < entries.length` | `parse_int`, `conv::is_integer`; `-0` passes (`-0 >= 0`) and is stored as `-0` in `selected` |
| document.ts:1968 | `parseInt(w:char ?? 'F0B7', 16)` (`0x` accepted by radix 16, NaN for no digits) | `parse_int(hex, 16)` |
| document.ts:1972 | `if (code <= 0xff) code \|= 0xf000` | `code <= 255.0` then `f64::from(conv::to_int32(code) \| 0xf000)` (`-5 \| 0xf000` is `-5`, `-0` gives `0xf000`; NaN is not `<= 0xff`) |
| document.ts:1975 | `String.fromCharCode(code)` (ToUint16 of any number, NaN and ±Infinity give 0) | `conv::to_uint16` → one UTF-16 unit, a lone surrogate allowed (`Utf16::from_units`) |
| document.ts:1983 | `parseInt(w:id ?? '0', 10)` of a note reference | `parse_int` → `NoteRef.id` (`JsNumber::Num`, NaN/Infinity included) |
| json_parse.rs | JSON numbers (`JSON.parse`): the strict grammar `-?(0\|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?` then `Number(text)` | `uo_jsnum::parse::to_number` (`-0` is `-0`, `1e999` is `Infinity`; differential: 3,020 inputs equal to V8) |

## 2. Regex sites (template or matcher; dynamic ones named; row in `js/regex-sites/r4c.mjs`)

All six rows × 99 inputs (594 results) equal to V8 (`tests/regex_r4c.rs`). No dynamic pattern.

| TS site | Regex | Rust | Template |
|---|---|---|---|
| document.ts:1863 | `/^\s*TOC\b/i.test(f.instr)` (a run inside a TOC field result) | `is_toc_instr`: `Cur::spaces`, `lit_ci("TOC")`, `boundary` | 5; evaluated **once, at `separate`** (see 9.8) |
| document.ts:2107 | `/\bTOC\b/.test(instr)` (the paragraph's `instrText`) | `matchers::find_word(instr, "TOC", false)` | 2 |
| document.ts:1937 | `.replace(/­/g, '‑')` | `matchers::replace_char` | 6 |
| document.ts:1937 | `.replace(/\r\n\|[\r\n]/g, ' ')` | `matchers::newlines_to_space` | 6 |
| document.ts:1766 | `/^[a-zA-Z0-9._-]+$/.test(type)` (custom-inline type) | `is_custom_inline_type` = `matchers::all_in` | 4 |

Not regexes but replaced by exact ports: `decodeURIComponent` → `json_parse::decode_uri_component` (every
`%XX` decoded, multi-byte sequences validated as UTF-8: no overlong form, no surrogate, nothing past
U+10FFFF; anything else is the `URIError` the TS catches), `JSON.parse` → `json_parse::parse`. Both are
in the differential `parity_custom_tag` (3,020 inputs: hand-written tags, generated JSON of every shape
percent-encoded ten ways, and their mutations; 850 parse; 0 mismatches).

## 3. Fields that can store NaN/±Infinity, keys stored as `undefined` (cleared bits)

Stored NaN/±Infinity (`JsNumber::Num`, F1):

| Field | TS site | Values |
|---|---|---|
| `NoteRef.id` | 1983 | NaN, ±Infinity (`parseInt` of a bad or 310-digit `w:id`) |
| `FormField.sizeHalfPt` | 1899 | NaN, ±Infinity |
| `FormField.selected` | 1911 | never NaN or infinite; `-0` (`Number.isInteger(-0)`) is stored |
| `RunProps.commentIds[]` | 1857 | NaN, ±Infinity (ids of open comment ranges; `-0` is `0` through the `Set`) |
| `BookmarkMark.id` | 1667 | finite only (`Number.isFinite(id)` guards it) |
| `CustomInline.data` numbers | 1768 | ±Infinity (`1e999`), `-0`; JSON has no NaN |

Keys stored as `undefined`:

| Key | TS site | When |
|---|---|---|
| `RunProps.fontAscii` | 1976 `run.props.fontAscii = font ?? rProps.fontAscii` | a `w:sym` without `w:font` in a run whose `rPr` gives no ascii font: the key exists with `undefined` (`mark_undefined`); when `rProps` already had the key (`w:rFonts` without `w:ascii`, R4a) the bit is kept, and a value clears it |

## 4. Map/object iteration orders relied on (and key orders recorded with `mark`)

Orders relied on:

| TS site | Order | Rust |
|---|---|---|
| 1646–1649, 1857 | `activeComments` is a `Set<number>`: `[...set]` is insertion order, `delete` then `add` moves an id to the end | `IndexSet<NumKey>` with `shift_remove` (keeps the order of the rest) |
| 1655–1660 | `bookmarkIds` is a `Map` used for `get`/`set`/`delete` only | `HashMap<NumKey, Str>` (no iteration) |
| 1768 | `JSON.parse` objects: duplicate key = first position, last value; integer-like keys first, ascending | `JsRecord` (`insert` keeps the place of an existing key; array-index keys first) |
| 1591, 1611, 2083 | `fieldStack` top/last, scanned from the top at the end of a paragraph | `Vec<FieldFrame>`, `iter().rev()` |

Key orders (the digest's order facet is compared, `order`/`order-src`: 0 mismatches). The TS assigns keys in
code order; the model's declaration order is used when it is the same, and every key the TS adds after
others in a different order is `mark`ed (a recorded list is the whole order, see `append_key`):

| Object | Order | How |
|---|---|---|
| `Run` (text, props, `field`, extras, `rpr`, `inToc`) | the model flattens `RunExt` after the run's own keys, so `rpr` precedes the rare fields in the digest; the TS has it after: **normalised on the TS side of the capture** (9.16) | rare fields: `ext.mark(..)` after each assignment, in the TS order (e.g. `formField` before `fieldResult` in `closeField`, `field`, `fieldOpen`, `fieldResult` at the paragraph end, `ptab`, `noteRef`, `symFont` after `inToc`) |
| `RunProps` of a hyperlink run | `link`, `linkTip`, `underline` (in place), `color` appended when absent (1850–1851) | `append_key(color)` after the link keys; `commentIds`, `ins`, `del` appended by `make` (1857–1859): `mark_if_recorded` (an object in declaration order already has them there) |
| `RunProps.fontAscii`/`vertAlign` assigned after `make` (1976, 1987) | appended at the end when absent | `append_key` (freezes the current order then marks) |
| `MathZone` | `nodes` then `jc` (1714), declaration is `jc`, `nodes` | `mark` both |
| `MathNode` | `{ t, text, sty, rpr }`, `{ t, num, den, type }`, `{ t, rpr }` | declaration order: no mark |
| `FormField` | `{ type, checked, sizeHalfPt, sizeAuto }`, `{ type, entries, selected }` | declaration order: no mark |
| `SegSource` (R6's type) | `raw, sids, wrapOpen, wrapClose`, then R6's `open`, `link`, `tip` | `mark` of `raw`, `sids`, `wrapOpen`, `wrapClose` when a wrapper is set (2064–2074); `forget` of both when a later child joins the segment (2061) |
| `ParaProps` | R4a's order, then `tocInstr` (2107) | declaration order |

## 5. Recursions (made iterative, or bounded by `max_nesting`)

| TS site | Recursion | Rust |
|---|---|---|
| 1637–1756 / 1838–2040 `readRunsInto` ↔ `readSingleRun` (wrappers, `w:fldSimple`, content controls, revisions, `w:ruby`) | to the XML depth (1,000) | one loop over a `Vec<Frame>` (heap); the lists a field or a custom inline reads into are a stack of `RunList`s. Tests: wrappers 900 deep, ruby 300 deep, in threads of 256 KiB (`r4c_unit`), and 480–980 deep of every kind on the 1 MiB stack of wasm32 (`nopanic_r4c`) |
| 1775–1826 `readMathNodes` (fractions, delimiters, any other element flattened) | to the XML depth | a stack of `MathFrame` (`Walk`, `Emit`, `Frac`) over a stack of node lists; same order of pushes as the recursion. 480 fractions/delimiters deep: no overflow on 1 MiB, digest and drop included |
| 1828–1836 `collectMathText`, 2156–2164 `collectInstrText`, 1583–1587 `textContentOf` | to the XML depth | explicit stacks / `Element::text_content` (iterative in uo-opc) |
| `JSON.parse` (V8 is iterative, no limit) | arrays/objects | explicit stack; nesting past `json_parse::MAX_DEPTH` (128) is a parse failure (9.3) |
| 2002/2010/2018 → R4b → `textboxBlocks` → `readParagraph` (a textbox's paragraphs are read from inside a run) | one story per level | `ReadCtx::enter_nested` (64) in R4b; `read_paragraph`, `read_children`, `run_child`, `run_object` are small `#[inline(never)]` frames (the large structs — `RunProps`, `PPr`, `ParaProps` — are built in functions that return before the recursion). **Measured** (a textbox in a textbox, `nopanic_r4c`): ≈ 50 KB of native stack per level in a debug build, of which R4c's frames ≈ 8 KB, and ≈ 14 KB per level in a release build (64 levels ≈ 0.9 MB): the rest is R4b's VML/drawing readers. The G20 wasm stack test with R5 needs those frames cut (finding for the milestone gate) |
| 1600 `closeField` | none | the field stack is a `Vec`, bounded by `max_field_depth` (9.1) |

## 6. Module-level / process-global state and its per-read replacement

| TS | State | Per-read replacement |
|---|---|---|
| 1640–1700 (`ctx.activeComments`, `ctx.bookmarkIds`, `ctx.fieldStack`) | fields of the context object, shared by every paragraph (and every textbox paragraph) read through it; `ctx.bookmarkIds ??= new Map()`, `ctx.fieldStack ??= []` | fields of `ReadContext`, one per part (`buildReadContext` is per part); shared with R4b through the same `&mut ReadContext`, so a textbox paragraph sees and changes the field stack and the comment ranges of the paragraph that holds it, as the TS |
| 876, 883 `RPR_EL`, `OBJ_EL` (symbols on the runs) | read-time side data of a run | `RunSide`, kept in the `RunList` next to the run (fields' result lists included), handed to `capture_paragraph` |
| 886 `tagObjectRuns(tgt, from, el)` | `tgt` is the array the object runs went to (the paragraph's runs, or the open field's result) | `Target::{List, Frame(id), Discard}` with a unique id per field frame: the same list is tagged unless the frame was popped by the object's own content (see 8) |
| 1905 ff.: `FieldFrame` | stack of open fields across runs and paragraphs | `Vec<FieldFrame>` in `ReadContext` |
| 687, 690 `ctx.onBookmark`, `ctx.onBookmarkEnd` | closures set by `readFullDocument` | `bookmark_events` queue + `collect_bookmarks` (no closures stored) |
| none (new) | `next_frame`, `fields_opened`, `comments_ver` | per context: frame identity, the `max_fields_per_story` count, the version of `activeComments` that invalidates the cached props of a run |

The TS process-globals of other frontiers (`wrapIds`, `blockSdtKeys`) are not used here.

## 7. `try {…} catch {}` sites and the partial result kept

| TS site | Catch | Result |
|---|---|---|
| 1767–1771 `parseCustomInlineTag` | `decodeURIComponent` (`URIError`) and `JSON.parse` (`SyntaxError`) | `undefined`: no custom inline, the content control is read as an ordinary one (`readRunsInto(runs, content)`): `parse_custom_inline_tag` returns `None` |

No other `try` in the range. Errors that are values here: limits (field depth, fields per story, runs and
paragraphs per part, open comment ranges), a malformed relationships part (`DocxError::Package`), and
what R4b returns.

## 8. Aliasing (two model positions sharing one TS object) and how A2 gives the same result

| TS site | Aliasing | A2 result |
|---|---|---|
| 1855–1865 `make`: `props = { ...rProps }` | one fresh copy per run | `Arc<RunProps>` shared by the runs of one `w:r` while nothing differs (comment ids, `ins`, `del` are the same for the whole `w:r`; `comments_ver` invalidates the cache when a textbox paragraph changes the comment ranges between two runs); copy-on-write, so `props.src` (R6) or `fontAscii`/`vertAlign` on one run never reach another |
| 1862 `run.rpr = rpr` | the same `RPr` object on every run of the `w:r`, on the field frame and on the field run | `Arc<RPr>` shared: the same identity as the TS |
| 1620, 1690, 1730 `{ ...first.props }` | the field run copies the first result run's props | `Arc::clone` (copy-on-write) |
| 1629, 1695 `run.fieldResult = f.runs` / `inner` | the result array is the frame's array | moved (the frame is gone) |
| 1787, 1792 `rpr` of `m:r` on each node | one `RPr` object on every `MathNode` of the `m:r` | `Box<RPr>` per node (A2 type): **cloned**, equal digest; a memory amplification of one `RPr` per node (9.11) |
| 1625–1627 `setRPrEl`, 886 `tagObjectRuns` | symbols on run objects | `RunSide` per run, in step with the run list |
| 2001, 2010, 2017 `tgt` | the array the run goes to, evaluated *before* `readDrawingRun`, whose textbox paragraphs may open or close fields | the frame id: tagging goes to the same field result as long as the frame is on the stack (an unmatched `end` inside the textbox that pops the outer frame leaves the TS tagging a detached array and the Rust tagging nothing: no observable difference except the `obj`/`objHead` of the sources of that pathological case) |
| A2 views (cell `paragraphs`/`blocks`, textbox `paras`/`blocks`) | none read here | R4b/R5 |

## 9. Saturating/wrapping/truncating choices and data-shape deviations from A2

No change to a model type; no change of arithmetic semantics (every number site is a `uo_jsnum`
function, item 1). Rows 1–3, 9, 11, 16 need the Opus check.

| # | Site | Choice | Why |
|---|---|---|---|
| 1 | document.ts:1883 `begin` (no limit in the TS) | `max_field_depth` (64) open fields at once and `max_fields_per_story` (100,000, counted per `ReadContext`, i.e. per part) are `DocxError::Limit` values | F21 (a limit is an error, not a different result); the stack, the `code`/`instr` strings and the result runs of a hostile part are otherwise unbounded. Real files nest 1–3 |
| 2 | document.ts:1646 (no limit in the TS) | **new** limit: more than 64 comment ranges open at once is `DocxError::Corrupt("more than 64 comment ranges open at once")`; a repeated id is one range | every run copies the open ids (`props.commentIds = [...ctx.activeComments]`): n unclosed ranges and n runs hold n²/2 ids (30,000 of each: 7 GB measured in the first version of the no-panic test; the TS has the same quadratic memory). Proposal: a `DocxLimits` field and a `DocxLimitKind` variant (shared files, not edited here) |
| 3 | document.ts:1768 `JSON.parse` | nesting past 128 levels is a failed parse (→ the content control is read as an ordinary one) | V8 has no limit; the digest, `Clone` and `Drop` of `JsonValue` are recursive. A real tag has depth ≤ 5 |
| 4 | json_parse.rs object keys | a key with a lone surrogate (`"\ud800"` as a key) is stored with U+FFFD (`JsRecord` keys are `Str`) | A2 `Str`; values keep their surrogates (`Utf16`). Not in the corpus |
| 5 | document.ts:1613 `parent.instr += text`, 1621 `field: …instr…` | the instruction/code of a field are `String`; the text of a nested field's result that holds a lone surrogate (a `w:sym` code unit D800–DFFF) becomes U+FFFD in the outer field's `Str` | A2 `Str`; the run text keeps the surrogate. **Accepted by name A2** in `parity_paragraph` (2 quirk cases) |
| 6 | character references to surrogates in XML text | U+FFFD (uo-opc, the decision of A5) | **Accepted by name A3** in `parity_paragraph` (1 quirk case) |
| 7 | document.ts:1634 `RunInherit.field` | not ported | the reader never sets it (`readRunsInto` is not exported; no caller passes `field`) |
| 8 | document.ts:1863 `stack.some(f => f.phase === 'result' && /^\s*TOC\b/i.test(f.instr))` per run | `FieldFrame.toc` is computed at `separate` and read per run | `instr` only grows in the `instr` phase (1926, 1613) and a frame never goes back to it, so the value is the same; saves a scan and a regex per run |
| 9 | document.ts:1605, 1855 | `Run.props` and `RunProps` values are `Arc`-shared where the TS copies (8) | copy-on-write gives the same observable result; the milestone-2 operations that mutate a props object in place must `make_mut` |
| 10 | document.ts:1937 text | the text of a run is accumulated as UTF-8 and converted at the flush; only `w:sym` builds a lone surrogate (`Utf16::from_units`) | XML text has none (A3) |
| 11 | document.ts:1787 `rpr` on math nodes (8) | one `RPr` clone per `MathNode` | a hostile `m:r` with a large `rPr` and many `w:br` children multiplies the `rPr`; proposal: `MathNode.rpr` as `Arc<RPr>` (model type, not changed here) |
| 12 | document.ts:2213 `readDocument` (the flat `DocumentModel` of the body paragraphs and `sectPr`) | **not ported** | the read path is `readFullDocument` (R5); `readDocument` is only exported by `core.ts` and imported (unused in the part reviewed) by the editor shell; its body is `buildReadContext` + `readParagraph` per body paragraph, available from this module. Ask at the gate whether any consumer remains |
| 13 | document.ts:2047–2079, 2114–2154 | the segment loop of `readParagraph` is here (`read_with_segments`), building R6's `PendingSeg`s; `captureParagraph` is R6's `capture_paragraph` | as the brief; the `wrapOpen`/`wrapClose` marks are applied here |
| 14 | document.ts:2186–2194 `resolveImage` | the media path is `Package::resolve_target` (uo-opc D6), the extension is the lower-cased last `.`-segment of that path, the bytes go to the media store (G24) | the accepted D6 rule of uo-opc R1 replaces `word/${target}` |
| 15 | document.ts:2166–2183 `buildReadContext` | a relationship needs a non-empty `Id` and `Target` (as `if (id && target)`); the last of two equal ids wins; `./` is stripped once | as the TS |
| 16 | A2 `Run`/`DrawingTextbox` flattening | the digest writes `rpr` right after `props` (the TS: after the rare fields; the runs of a group item have `props` after the rare fields) and a textbox's content last | **the capture normalises the TS side** (`js/capture-paragraph.mjs`, `normalize`): the order facet is compared exactly otherwise. Consumers that read key order (LQ20 `json_key` of a whole `Run`) see Rust's order, consistent inside Rust |
| 17 | `RunList`/`ReadContext` | `RunList` has private fields; `ReadContext` borrows `ReadCtx` mutably for the part | the TS has one mutable context object |

## Parity and memory results

**Parity** (`tests/parity_paragraph.rs`, capture `tests/parity/paragraph.capture` by
`js/capture-paragraph.mjs`, Node 24.15 x64, frozen TS `d0f45bc5`): every paragraph that is not inside
another paragraph of every story part (`word/document.xml`, `header*`, `footer*`, `footnotes`, `endnotes`,
`comments`) of the 2,742 oracle documents of docx-layout and the docx-core fixtures (2,866 parts), and of
the paragraph quirk corpus (`js/quirk-paragraph.mjs`, `tests/parity/paragraph.quirks.txt`, 817 cases:
117 hand-written — complex fields across runs, wrappers and paragraphs, nested and unmatched, form fields,
TOC marks, hyperlinks, bookmarks and comment ranges of every id kind, revisions, content controls with
custom-inline tags, math, `w:sym`, notes, ruby, text controls, nesting chains to 980 levels, 60 nested
fields — and 700 seeded), read in document order with one `ReadContext` per part as `readFullDocument`
does (the TS `buildReadContext` + `readParagraph`; the textbox content through R4b without a block
reader, `ParasOnly`). Two reads per part, without sources and with `{ sources: true }` (positions,
R6's capture). Facets per part: `n`, `paragraph`, `order`, `paragraph-src`, `order-src`, `sources` (the
`SourceStore` after the part). **22,088 facets compared (68,047 paragraphs): 22,082 equal, 0 mismatches,
6 accepted by name (A2 × 4, A3 × 2: three quirk cases, two facets each), 0 not replayed**, after
three corrections of the capture's normalisation (item 16) and the quirk corpus (no surrogate
references in the seeded texts: they are covered by the two named cases, so the 700 seeded cases are
compared strictly). Sensitivity checked by mutation of the port (quirk corpus only): TOC flag off 338
mismatches, `parse_int(.., 16)` → 10 in `w:sym` 386, hyperlink underline off 184, tooltip inheritance
off 10, `formField` mark removed 486, `ins` mark removed 12, `w:t` text taken from descendants 2.
Custom-inline tags: **3,020 inputs equal to V8, 0 mismatches** (`parity_custom_tag`, 850 parse). Regex rows:
**594 results equal to V8** (`regex_r4c`).

**Unit tests** (`tests/r4c_unit.rs`, 20, plus `json_parse` unit tests): text rules, complex/nested/
crossing fields, the limit errors, bookmark events, sources, hyperlinks, the resolvers, custom inline,
math, deep wrappers and ruby on a 256 KiB stack, `w:sym`, notes.

**Memory (M1)** (`tests/memory_r4c.rs`, 4 tests): `ReadContext::new` (relationships, a malformed
relationships part), the resolvers, `read_paragraph` over 15 bodies (complex, simple, nested, crossing
fields, form fields, hyperlinks, bookmarks, comment ranges, revisions, content controls, math, `w:sym`,
notes, ruby, objects) with and without sources, its error paths (field depth, fields per story, runs,
paragraphs), `parse_custom_inline_tag`, `json_parse::parse` (incl. depth 200 and syntax errors),
`decode_uri_component`: live bytes back to the start, **0 B** in every case.

**No-panic** (`tests/nopanic_r4c.rs`, 9 tests): the quirk corpus (with and without sources), 3,000 seeded
mutations of it (also with small limits), 10 nesting chains up to 980 levels on a 1 MiB stack, textboxes
nested 1–120 levels (see the stack measurement in item 5), hostile field codes (64 and 65 deep, 20,000 in
a row, 200,000-word instruction, stray `separate`/`end`, a field across 1,000 paragraphs, the per-story
limit), 30,000 bookmarks and comment ranges (linear), the comment-range cap, and 20,000 random strings
through the tag parsers (plus 500,000-element and 100,000-deep JSON, 100,000 percent sequences).

Checks: `cargo fmt` (my files), `cargo clippy -p uo-docx --lib` and my test targets `--all-features -D
warnings` (native and `wasm32-unknown-unknown`), `float-casts` for uo-docx and `grep-bans.sh`: clean;
`cargo test -p uo-docx --all-features` green. (`clippy --all-targets` on the whole crate stops at another
frontier's `word_unit.rs` collapsible-`if`.)

## Process metrics (wall time, rounds with the verifiers, rework)

One Sonnet session, ≈ 140 tool calls; wall time not measured by the agent. No verifier round yet.
Rework: (1) the first facet run had 222 mismatches, all of one of four kinds: surrogate references in the
seeded texts (named differences A2/A3: moved to two hand-written cases), `props`-after-rare-fields in group
items and the textbox content position (model flattening: normalised in the capture, item 16), and
`ins`/`del`/`commentIds` appended without a `mark` (a port bug: fixed, `mark_if_recorded`); (2) the
no-panic loop found the quadratic comment-id memory (item 9.2) and the stack cost of nested stories
(item 5); (3) the frames of `run_child`/`read_paragraph` were split into small functions for it.
Found outside this frontier: `SourceStore::default()` starts `next` at 0 where the TS starts at 1 (the
`sources` facet agrees, so R6 handled it); the nested-story stack cost is mostly R4b's.
