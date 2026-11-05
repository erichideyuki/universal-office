# uo-docx D5b (ODF): declarations

Brief: `../uo-docx.md`, "D1b — writer (architecture)", "Emitter frontiers" (the nine kinds of declaration, one
table per kind, each row with the TS file:line), applied to D5 ("derived documents"), part `odf`. Code:
`src/odf/{mod,props,units,styles,reader,write_odt}.rs` (feature `odf`), with `src/final_view.rs` and
`src/u16buf.rs` (shared with D5a). Frozen TS: `write/packages/docx-core/src/odf/` (`index.ts`, `units.ts`,
`props.ts`, `styles.ts`, `write-odt.ts`; `zip.ts` is replaced by uo-opc and not ported). Sonnet, hybrid mode,
2026-11-03/05; the record was written afterwards from the code, the TS and the test runs of 2026-11-05.

**Ported** (TS name → Rust name): `index.ts` `isOdt` → `is_odt` (takes `&DocxLimits`), `readOdt` → `read_odt` /
`read_package` (over an open `Package`), `ODT_MIMETYPE`, `DEFAULT_CONTENT_WIDTH_TWIPS`, `DEFAULT_SECTPR` →
`default_sect_pr()`, `HEADING_LOOKS` → `HEADING_SIZES` + `HEADING_COLOR`, `readBodyChild`, `readParagraph`,
`readHeading`, `clampLevel`, `paraPropsFromStyle`, `baseRunPropsFor`, `mergeRunProps` → `merge_run_props`
(over `styles::overlay`), `collectRuns` (a loop over `Frame`s, see "Decisions"), `flattenNoteBody` (inline in the
`text:note` arm), `readAnnotation`, `textOf` (`Element::text_content`), `readList`, `readListItem`,
`resolveListKind`, `lookupListKind`, `readTable`, `collectRows`, `readColumns`, `columnWidth`, `buildGridCols`,
`readCell`, `readImage`, `readTextFrame`, `decodeBase64` → `decode_base64`, `applyParaBorders`,
`bordersFromStyle` → `borders_from_style`, `parseBorder`, `cellPropsEl`, `cellShading`, `cellBorders`,
`readPageGeometry`, `findPageLayout`, `buildStylesMap`, `normalizeStyleId`; `units.ts` `lengthToTwips`,
`lengthToTwipsOr`, `lengthToHalfPt` (+ `split_length`, the hand matcher of the file's one regex); `props.ts`
(the two aliases are the model types); `styles.ts` `RawStyle`, `ListKind`, `OdfStyleIndex`, `readRawStyle`,
`readListStyle`, `collectStyles`, `buildStyleIndex`, `isTrue`, `readTextProps`, `readParaPropsStyle`, `normHex`,
`stripQuotes`, `resolveStyle` → `OdfStyleIndex::resolve_style`; `write-odt.ts` `writeOdt` → `write_odt`
(`WriteOdtOptions` → `OdtOptions`, `images: 'embed' | 'omit'` → `OdtImages`), `xmlEscape`, `twipsToPt`,
`twipsToCm`, `halfPtToPt`, `eighthPtToPt`, `runStyleKey`, `getRunStyle`, `runStyleXml`, `HIGHLIGHTS`,
`highlightToHex`, `paraStyleKey`, `borderKey`, `getParaStyle`, `paraStyleXml`, `cssStyle`, `annotationsForRun`,
`runXml`, `isAllDefault`, `escapeText`, `imageXml`, `textFrameXml`, `paragraphXml`, `headingLevelOf`,
`emitParagraph`, `emitList`, `tableXml`, `tableCellXml`, `emitBlocks`, `styleStandardXml`, `pageLayoutXml`,
`masterStyleXml`, `NS`, `contentXml`, `stylesXml`, `manifestXml`; `finalBlocks` of `revisions.ts` →
`final_view::final_paragraphs` + the table walk in `write_odt.rs` (`final_blocks`, `final_table`). The re-exports
of `OdfPackage`/`writeZip` are uo-opc's (`Package`, `zip::write`).

**Facet** (`js/capture-odf.mjs` → `tests/parity_odf.rs`, Node 24.15 x64, `cargo test -p uo-docx --features
canon,odf,export,compare --test parity_odf`, run on 2026-11-05): **40,248 results equal, 0 mismatches, 3
accepted, 0 not replayed** over **7,398 capture records** in 7 tests (the whole file runs in 9.06 s in a debug
build):

| Test | Records | Results |
|---|---|---|
| `odf_parity_oracle_0..3` | the 2,753 docx of the oracle corpus and the docx-core fixtures (`read_document` → `write_odt`: part names in order, each decompressed part, `is_odt`, `read_odt` of the output by the canonical digest of `{ blocks, sections, comments, styles }`) | 6,303 + 5,764 + 6,209 + 6,350 = 24,626 equal |
| `odf_parity_synthetic_docx` | 741 synthetic docx of R6 and R5 (`sources.inputs`, `full.inputs`: 341 of the `quirk/full` family), the same pipeline | 5,900 equal |
| (inside both) `#odt-omit` | 352 records: the same with `images: 'omit'`, for documents whose output has pictures | part names, `n`, `p<i>` |
| `odf_parity_generated_odt` | 931 generated `.odt` packages (`js/quirk-odf.mjs`: namespaces and prefixes, missing parts, style chains with cycles/defaults/duplicates, every unit form, headings, lists, tables with spans and repeats, frames with images and text boxes, annotations, notes, bookmarks, unknown elements, zip damage, **400 seeded random mixes**, seed `0x0df5b001`): `read_odt` (model digest or the class of the error) and `is_odt` | 1,859 equal, 3 accepted |
| `odf_parity_units` | 2,621 strings (`js/quirk-odf.mjs` `LENGTHS` plus seeded ones) through `length_to_twips`, `length_to_half_pt`, `length_to_twips_or(…, 7)` as f64 bits | 7,863 equal (3 per string) |

The generated `.odt` set includes the error classes the TS gives: the capture holds 13 `err:xml`, 7 `err:zip`,
6 `err:limit`, 4 `err:no-content`, 4 `err:no-text`, 15 `err:base64`, and 4 docx that `readFullDocument` itself
rejects (`rderr`, compared as `err`); the read errors compare as a class. The repository has no `.odt` of its own and neither Word nor LibreOffice runs
on the build machines (plan U15/U20), so the read side is covered by the writer's own output (2,753 + 741
documents round-tripped) and the generated packages, not by `.odt` files made by Word or LibreOffice: **not
measured against a foreign producer**. No dedicated V8 regex-site file exists for ODF (there is none for
`odf` in `js/regex-sites/`); its regex sites (table 2) are exercised by the parity inputs above (the length
strings, the style names and ids of the corpus, generated names). Parity rounds: see "Process metrics".

**Two fixes of 2026-11-05 (orchestrator, Opus)**, found when the first full replay of the facet showed about
4,400 mismatches:

- **(a) The digest, not the port.** The ODF parity digest in `tests/parity_odf.rs` and `examples/odf_dump.rs`
  now writes the TS canon's `@views: "synced"` on every section (`js/canon.mjs` `sections[]` rule). It had
  caused about 4,400 harness-only mismatches; nothing in `src/odf` changed for it.
- **(b) `TextboxView::TopLevel`** (`src/model/run.rs`) for the ODF text frame. The TS `paras` of a frame read by
  `readTextFrame` is only the **top-level paragraphs** of its `blocks` (`odf/index.ts:594`,
  `blocks.flatMap(b => b.type === 'paragraph' ? [b.paragraph] : [])`), which is neither the docx reader's
  "synced" view nor "paras only". The canon writes "split" (both views) when the blocks hold cell paragraphs
  (i.e. a table in the frame), else "synced". The JSON `paras`, `write/story.rs` `textbox_paras_xml` and the
  hyphenation walk (`word/hyphlangs.rs`, TS order: top-level paragraphs, then blocks; a single visit keeps
  F2-21) follow it. After both fixes the 7 ODF parity tests pass with 0 mismatches (the run above).

## 1. Numeric parse/format sites and the uo-jsnum function used

| Site | uo-jsnum / Rust |
|---|---|
| units.ts:32 `parseFloat(m[1])`, :35 `Number.isNaN(num)`, :36 `Math.round(num * factor)` | `parse::parse_float` on the matched number text, `math::round` (`split_length` already guarantees the text is `-?D+` or `-?D*.D+`, so NaN cannot occur; the guard stays) |
| units.ts:14–21 `TWIPS_PER_UNIT` (`mm: 56.7`) | `factor()`, the same f64 constants |
| units.ts:50 `Math.round(twips / 10)` | `math::round(twips / 10.0)` |
| styles.ts:62 `parseInt(lvl.attrs['text:level'] ?? '1', 10)` | `parse::parse_int(_, 10)`, key `NumKey::new(level)` (NaN is a key like any other, as the TS `Map`) |
| index.ts:196 `parseInt(hEl.attrs['text:outline-level'] ?? '1', 10)`, :215–218 `clampLevel` (NaN → 1, <1 → 1, >6 → 6) | `parse_int`, `clamp_level`; `Heading${level}` via `format::to_js_string(level)`; `HEADING_LOOKS[level]` via `conv::to_uint32` index (the level is an integer in 1..6 after the clamp) |
| index.ts:295 `parseInt(child.attrs['text:c'] ?? '1', 10)`, :296 `' '.repeat(NaN or < 1 ? 1 : count)` | `parse_int`; the count above `MAX_SPACE_REPEAT` (16,777,216) is `DocxError::Corrupt` (decision 5); `conv::to_uint32` |
| index.ts:489 `parseInt(c.attrs['table:number-columns-repeated'] ?? '1', 10)`, :491 loop | `parse_int`; widths pushed with `resize`; past `MAX_COLUMNS` (65,536) `DocxError::Corrupt` (decision 5) |
| index.ts:462–466 `reduce((s, c) => s + c.gridSpan, 0)` and `Math.max` | f64 sum in a loop, `math::max` (a `NaN` gridSpan cannot happen: :539 maps NaN to 1) |
| index.ts:512–518 `Math.max(totalCols, colWidths.length, 1)`, :516 `Math.round(9026 / n)` | `math::max`, `math::round`; `n` past `MAX_COLUMNS` is an error |
| index.ts:523, :539 `parseInt(cellEl.attrs['table:number-columns-spanned'] ?? '1', 10)` and the NaN/<1 → 1 rule | `parse_int`; `JsNumber::Num` (a span can be huge: it is the writer that bounds it, item 7) |
| index.ts:574–575, :592–598, :615–619, :751–756 `lengthToTwips(…) ?? default` | `length_to_twips(…).unwrap_or(default)` |
| index.ts:621 `parseInt(frame.attrs['draw:z-index'] ?? '0', 10) \|\| 0` | `parse_int`, NaN or 0 → `0.0` (so `-0` and NaN become `+0`) |
| index.ts:697–700 `widthTwips / 20`, `Math.max(2, Math.round(pt * 8))` | `math::round`, `math::max` |
| index.ts:188 `if (base.szHalfPt)` (truthy: 0 and NaN are false) | `JsNumber::truthy` |
| write-odt.ts:64 `(t / 20).toFixed(2)`, :65 `(t / 1440 * 2.54).toFixed(4)`, :66 `(hp / 2).toFixed(2)`, :177/:391 `eighthPtToPt(e.sizeEighthPt).toFixed(2)` | `format::to_fixed(x, digits)` (the TS operation order kept: `t / 1440 * 2.54`; a non-finite value prints as the JS text) |
| write-odt.ts:77–79, :139–140, :147 numbers inside the style keys (`p.szHalfPt ?? ''`, `charSpacingTwips ?? ''`, the paragraph indents, `${x.sizeEighthPt}`) | `num_or_empty` (`format::to_js_string`; absent and `null` print as the empty string) |
| write-odt.ts:113, :119, :167–171 truthiness tests of `szHalfPt`, `charSpacingTwips`, the indents/spaces (0 and NaN are false) | `truthy_num` (`JsNumber::truthy`) |
| write-odt.ts:247 `n - 1` (the `text:c` count of a run of spaces) | integer arithmetic on the run length |
| write-odt.ts:304 `Math.min(6, parseInt(m[1], 10))` | digit from the matched unit, `min(6)` |
| write-odt.ts:353, :385 `T${paraStyleDefs.length + runStyleDefs.length + 1}`, `A${paraStyleDefs.length}` | `usize` lengths (the same counters, item 8) |
| write-odt.ts:357 `t.gridColsTwips.reduce((a, b) => a + b, 0)` | f64 sum in order (a `null` width is NaN through `to_number`) |
| write-odt.ts:377 `for (let i = 1; i < cell.gridSpan; i++)` | f64 counter; span past `MAX_COLUMNS` is `DocxError::Write` (the TS runs out of memory) |
| write-odt.ts:404 `cell.gridSpan > 1` and `${cell.gridSpan}` | `to_number() > 1.0`, `js_str` (`null` prints `null`) |
| write-odt.ts:448, :456 `(lvl + 1) * 0.5` (printed `0.5cm`, `1cm`, `1.5cm`, `2cm`, `2.5cm`) | constants in `style_standard_xml` (the values are exact in f64) |
| write-odt.ts:269, :280, :282 `twipsToCm(a.posH.offset ?? 0)`, widths and heights | `twips_to_cm(x.map_or(0.0, to_number))` |

## 2. Regex sites and their matcher (dynamic ones through a core template, named)

No site is dynamic: every pattern is a literal in the TS. All are hand matchers; none is a `regex` crate use.

| Site | Matcher |
|---|---|
| units.ts:30 `/^\s*(-?[0-9]*\.?[0-9]+)\s*(in\|cm\|mm\|pt\|pc\|px)\s*$/` | `units::split_length` (JS `\s` through `uo_jsnum::text::js_space_len`; ASCII `[0-9]`; a `.` with no digit after it cannot match; the unit is case-sensitive and exactly two bytes) |
| index.ts:84 `mime?.trim()` | `uo_jsnum::text::trim` |
| index.ts:559 `href.replace(/^\.\//, '')` | `strip_prefix("./")` |
| index.ts:561 `path.split('.').pop()` and `e.toLowerCase()` | `rsplit('.')` (never empty), `uo_text::case::to_lowercase_str` |
| index.ts:568 `textOf(binary).replace(/\s+/g, '')` | filter on `is_js_space` |
| index.ts:687 `/^#?[0-9a-fA-F]{6}$/` and `part.replace(/^#/, '').toUpperCase()` | `is_hex6`; `styles::norm_hex` (`uo_text::case::to_uppercase_str`: full Unicode case mapping, as `toUpperCase`) |
| index.ts:680 `spec.trim().split(/\s+/)` | `split_js_space(trim(spec))` |
| index.ts:718 `bg.replace(/^#/, '').toUpperCase()` | `norm_hex` |
| index.ts:806 `name.replace(/\s+/g, '')` | filter on `is_js_space` (`normalize_style_id`) |
| styles.ts:140 `/^super\b/`, `/^[1-9]/`, :141 `/^sub\b/`, `/^-/` | `word_prefix` (`\b` = next byte not `[A-Za-z0-9_]`, `uo_jsnum::text::is_js_word`), first-byte range test, `starts_with('-')` |
| styles.ts:176 `v.replace(/^#/, '').toUpperCase()` | `norm_hex` |
| styles.ts:180 `v.replace(/^['"]\|['"]$/g, '').split(',')[0].trim()` | `strip_quotes` (a leading quote and a trailing one removed; a lone quote once, as the global replace) |
| write-odt.ts:57–61 `xmlEscape` (`& < > " '` in this order) | `xml_escape` (single pass; `&` first in the TS is equivalent) |
| write-odt.ts:301 `/^title$/i`, `/^t[íi]tulo$/i`; :302 `/^subtitle$/i`, `/^subt[íi]tulo$/i` | `ci_whole` over UTF-16 units with `uo_text::case::regexp_canonicalize` (the `i` flag without `u`: `Canonicalize` by single-unit upper-casing) |
| write-odt.ts:303 `/(?:heading\|t[íi]tulo\|titulo)\s*([1-9])/i` anywhere in the id; `/^h([1-6])$/i` | `heading_level_of`: for each start, the three alternatives in order, then JS white space, then the digit (backtracking over `\s*` is a no-op because a digit is not white space); the `h1`–`h6` form last |

## 3. Template-literal values printed without escaping

Reader: none (it builds objects, not text). Writer, values printed raw inside attributes or element content:

| Site | Values |
|---|---|
| write-odt.ts:95–119 `runStyleXml` | `p.underlineColor`, `p.color`, `highlightToHex(p.highlight)` (a name outside the table is printed as `#name`), the numbers of `halfPtToPt`/`twipsToPt`; the font name is escaped (`xmlEscape`) |
| :172, :177, :391 `paraStyleXml`/`tableCellXml` | `p.shadingHex`, `cell.shadingHex`, `e.color ?? '000000'`, `eighthPtToPt(...)`; `cssStyle` is a constant set |
| :184 | `parentStyleName` escaped (always a constant in practice: `Standard`, `Heading_20_N`) |
| :205–206 `annotationsForRun` | author escaped; the annotation text through `escapeText` |
| :223 `runXml` | link escaped; style names `R<n>` constants |
| :268–271 `imageXml` | `path` = `Pictures/img<idx>.<ext>` with **`ext` = the lowercased `img.ext`, not escaped** (finding F1); `idx` a number |
| :282 `textFrameXml` | `at` (`as-char`, `page`, `paragraph`), `ctx.frames`, the `twipsToCm` strings |
| :293, :295 | `styleName`, `asHeading` |
| :339–346 | list style names (constants) |
| :353–398 | `tableName`, `colName`, `cellName`, `twipsToPt` strings, `cell.gridSpan` printed raw (`${cell.gridSpan}`) |
| :436–459, :468–472, :479–503 | constants, page geometry through `twipsToCm`, `NS` |
| :515–526 `manifestXml` | `p.path`, and `image/${p.ext}` for an extension other than jpeg/jpg/png/gif — **unescaped** (F1) |

## 4. Object/Map orders relied on (JSON keys, `Object.entries`)

| Site | Order |
|---|---|
| styles.ts:38–39, :77–86 `index.styles` is a `Map`; `set` of an existing name keeps the first position and the new value; :107–110 filled in the order styles.xml `office:styles`, styles.xml `office:automatic-styles`, content.xml `office:automatic-styles` (later wins) | `OdfStyleIndex::styles` is an `IndexMap` (insert keeps the position), filled in the same three steps |
| index.ts:780–789 `buildStylesMap` iterates `index.styles` (insertion order); `out.set(id, entry)` then `out.set(name, entry)` when the id differs; :791–800 `Heading1..6` added if absent | `JsMap<Str, ModelStyle>` filled in the same order (id first, then the original name; a later collision overwrites in place); headings added after |
| styles.ts:41, :59–71 `listStyles` `Map<number, ListKind>`; `levels.get(level) ?? levels.get(1)` | `JsMap<NumKey, ListKind>` (keys as f64 bits, NaN allowed), `HashMap` for the outer map (never iterated) |
| styles.ts:213–214 `{ ...baseFromDefault.run, ...baseFromParent.run, ...readTextProps(raw.text) }` | `overlay` copies, per key, only the slots `over` holds with a value; the order of the keys of the result is not compared by the digest (A6: keys sorted; the insertion order is a separate non-compared facet) |
| index.ts:243 `Object.keys(over)` in `mergeRunProps` | `overlay` over `T::keys()` |
| index.ts:129, :355–369 `ctx.comments` array and `nextCommentId++` | `Vec<Comment>`, ids 0.. in document order |
| write-odt.ts:43–46, :495–498 `paraStyleDefs` (paragraph, table, column and cell styles share it) then `runStyleDefs` | two `Vec<String>`, concatenated in that order into `office:automatic-styles` |
| :41–42, :84–86, :152–154 `paraStyles`/`runStyles` `Map` (lookup only) | `HashMap` (never iterated) |
| :48, :544 `comments` `Map<number, CommentInfo>` built from `doc.comments` (a repeated id: the last wins, at the first position) | `JsMap<NumKey, &Comment>` (same rule) |
| :50 `emittedComments` `Set` (membership only) | `HashSet<NumKey>` |
| :553–559 `parts` object: `mimetype`, `content.xml`, `styles.xml`, `META-INF/manifest.xml`, then the pictures (`Pictures/img<n>.<ext>` are not array-index keys, so insertion order) | `Vec<(&str, &[u8])>` in that order; `mimetype` is `stored` and first |
| :326–333 `emitList` builds the tree from the `ilvl` of the items in order (a stack popped while `ilvl >=`) | arena of nodes + index stack, rendered by a loop in the TS depth-first order |
| :415–430 `emitBlocks` groups contiguous paragraphs that have `numbering` | same grouping, flushed at the first non-list block and at the end |

## 5. Recursions and their bound

| Site | Bound |
|---|---|
| index.ts:143–167 `readBodyChild` → `text:section` (:159), `text:list` (:152), `table:table` (:155) → `readCell` (:528) → `readBodyChild`; :588 `readTextFrame` → `readBodyChild`; :321 `readAnnotation` → `collectRuns` | a counter `Reader::depth`; past `DocxLimits::max_nesting` (64) a list, table or section is read as **one paragraph of its plain text**, a frame goes inline as text, an annotation is dropped (decision 3; accepted difference "ODF nesting"). The TS has no limit (it overflows its stack at some thousands of levels) |
| index.ts:255–345 `collectRuns` (span, link, unknown element, note body, frame text) | not recursive in Rust: a `Vec<Frame>` stack with one `Frame` per TS call (items, position, base props, pending link, text buffer); the flush points and the order in which an unknown element's text comes out after the nested runs are kept |
| index.ts:380–411 `readList` ↔ `readListItem` (nested `text:list`) | `depth` counter (+1 per nested list, `max_nesting`), the same fallback |
| index.ts:486–496 `readColumns`' `visit` (column groups) | explicit stack, document order kept (children pushed reversed) |
| index.ts:650–674 `bordersFromStyle` climbs `parent` without a seen set | a loop of at most `max_based_on_chain` (10,000) steps; a longer chain or a **cycle** is `DocxError::Limit(BasedOnChain)` where the TS overflows its stack (the capture holds those cases as class `limit`: `pp-border-cycle`, `pp-border-self`) |
| index.ts:707–713 `cellPropsEl` (has a `seen` set) | a loop with a `HashSet`; a cycle ends with `None` as the TS; longer than `max_based_on_chain`: `Limit` |
| styles.ts:194–217 `resolveStyle` (parent chain, `seen` set) | a loop up to `max_based_on_chain`; a cycle ends the walk (`cyclic`), the chain is resolved from the style the walk started with; longer chains are a `Limit` error; acyclic results are cached per style (`CACHE_CAP` 4,096, M5) and never when the chain ends in a cycle (the result depends on the start through `seen`) |
| write-odt.ts:275–284 `textFrameXml` → `emitBlocks` → `runXml` → `textFrameXml`; :351–413 `tableXml` → `tableCellXml` → `emitBlocks` → `tableXml` | `Writer::depth`, `MAX_WRITE_DEPTH` 256 on tables and text boxes; deeper is `DocxError::Write` (the reader stops at 64, so only a model built another way reaches it) |
| write-odt.ts:336–347 `render` (nested lists) | iterative (arena + frames), bounded by the number of items |

## 6. Module state and where it lives in `WriteCtx`

There is no module-level mutable state in the TS (`HEADING_LOOKS`, `DEFAULT_SECTPR`, `HIGHLIGHTS`, `NS`, the
TWIPS table are constants; `DEFAULT_SECTPR` is spread into a fresh object). The per-call state is the `ReadCtx`
(`pkg`, `styleIndex`, `comments`, `nextCommentId`) and the writer's `Ctx` (`paraStyles`, `runStyles`, the two
definition lists, `pictures`, `options`, `comments`, `emittedComments`, `frames`). ODF does not go through
`write::WriteCtx` (that is the docx writer's): the equivalents are `Reader<'p, 'd>` (`pkg`, `index`, `comments`,
`next_comment_id`, `ids: IdAlloc`, `media: MediaStore`, `depth`, `limits`) and `Writer<'a>` (the same fields as
the TS `Ctx` plus `media`, `depth`). Added state with no TS counterpart: `OdfStyleIndex::cache` (per call, cap
4,096), the id allocator and media store of the model the reader builds, and the depth counters. Nothing is
kept between calls; `clear_cache` exists for the memory harness.

## 7. `throw` sites → `DocxError::Write` (and the read errors)

| Site | Rust |
|---|---|
| index.ts:91–93 `throw new Error('Pacote ODF sem content.xml …')` | `DocxError::Corrupt("ODF package without content.xml")` |
| :102–104 `throw new Error('content.xml sem office:body/office:text')` | `DocxError::Corrupt("content.xml without office:body/office:text")` |
| :79–82 `isOdt` catch-all (zip load fails) → `false` | `Package::open` or `verify` fails → `false`; `verify` stands for the TS reading (inflating) every entry at load |
| :89, :94–96 `OdfPackage.load` / `parseXml` throwing on a damaged zip or malformed XML | the uo-opc errors (`DocxError` from `uo_opc::Error`); the parity compares the class (`zip`, `xml`) |
| :629–639 `decodeBase64`: `atob` throws on a bad character, a misplaced `=` or a remainder of 1 (`InvalidCharacterError`) | `DocxError::Corrupt("image data is not valid base64")` from the hand port of `atob` (the forgiving decode of the Infra standard: no `=` but at the end, at most two, only when the length is a multiple of 4) |
| :640 `throw new Error('sem decodificador base64 …')` | not reachable in Rust (no environment test) |
| (not a TS throw) a run of `text:s` over 16,777,216 spaces, a table with over 65,536 columns (`number-columns-repeated`, a span, the grid), a style chain over 10,000 links or a cycle in `bordersFromStyle` | `Corrupt` / `Limit(BasedOnChain)`; the TS dies with `RangeError` or an out-of-memory (decision 5) |
| write-odt.ts:560 `writeZip` failing | `Err` from `uo_opc::zip::write` |
| (not a TS throw) tables/text boxes nested past 256 in the model, a cell spanning over 65,536 columns | `DocxError::Write("tables nested too deep" / "text boxes nested too deep" / "a table cell spans too many columns")` |

## 8. Evaluation order around `revisionAttrs` / `mediaRelId` / `linkRelId`

None of the three exists in the ODF code. The orders that matter are the TS ones and are kept:

- **Style names** are numbered by emission order: `R<n>`/`P<n>` = `defs.length + 1` at the first use of a key
  (write-odt.ts:87, :155); in `paragraphXml` the paragraph style is taken **before** its runs (:290–291), in
  `runXml` the run style before the text; a list item's paragraph is rendered before its child list (:342–343).
- **Table names**: `tableName` is computed from the lengths of both lists **before** its own style is pushed
  (:353); each cell name uses `paraStyleDefs.length` at the time (:385), after the table and column styles and
  after every style created by earlier cells; the cell content is emitted after its style is pushed.
- **Pictures** are numbered at the time the run is emitted (`pictures.length + 1`, :264); with `images: 'omit'`
  the run emits nothing and no number is used (:213). A comment is emitted once, at the first run that carries
  its id, **before** that run's image or text (:214, :216, :220).
- **Frame numbers** (`Moldura<n>`) are taken after the inner blocks are emitted (:281), so inner frames get
  the lower numbers.
- **`content.xml` depends on the style lists**, which are filled while the body is emitted (:547–548): the body
  is built first, the automatic styles are concatenated after.
- **Reader**: the style index is built before the body (`styles.xml` named styles, its automatic styles, then
  `content.xml` automatic styles, the later overriding); annotations are numbered in document order, at the point
  `collectRuns` meets them (after flushing the pending text); `collectRuns` does not flush before an unknown
  element, so its text comes out after the nested runs (Frame stack keeps it).

## 9. Deviations (Opus check first): decisions outside the TS semantics

| # | Deviation | Why |
|---|---|---|
| D5b-1 | `zip.ts` is not ported: the container is `uo_opc::Package` (read) and `uo_opc::zip::write` (write, `mimetype` stored first). `read_odt` calls `Package::verify` to reproduce "every entry is inflated at load" | the container belongs to uo-opc (module table of `uo-docx.md`); `DocxLimits.package` bounds the input |
| D5b-2 | The error type is `DocxError` (`Corrupt`, `Limit`, XML and zip errors) instead of `Error` with a message; parity compares the class | typed errors |
| D5b-3 | **Nesting past `max_nesting` (64)** in the reader: a list, table or section is read as one paragraph of text, a frame goes inline, an annotation is dropped (plan G20 applied to ODF). The TS recurses until its stack ends | no stack overflow on untrusted input; **accepted difference "ODF nesting"**, below |
| D5b-4 | The model shape differs from the TS object graph: the body is a `Story`; a table cell is one story (paragraphs, then the tables anchored after the last), where the TS cell has `paragraphs` and `tables` and no `blocks`; a text frame is a `DrawingTextbox` with its content story and the view `TextboxView::TopLevel` (fix (b) above). The digest rebuilds the TS `blocks` of a cell from `paragraphs` and `tables` (`js/odf-canon.mjs`) | the model is one `Story` per container (A2); declared in the canon header of `capture-odf.mjs` |
| D5b-5 | `collectRuns` and `readColumns` are loops over explicit stacks; `resolveStyle` is a loop with a per-call cache (4,096 entries); `emitList` renders from an arena by a loop | no host-stack dependence; results identical on the parity set |
| D5b-6 | A style chain over 10,000 links, and **any cycle in `bordersFromStyle`**, is `Limit(BasedOnChain)`; the TS overflows its stack in the same cases (the capture records the class `limit`) | same observable class (an error), bounded work |
| D5b-7 | Limits the TS does not have: `text:s` count over 16,777,216 (`Corrupt`), a table with over 65,536 columns declared, repeated or spanned (`Corrupt`), a covered-cell span over 65,536 in the writer (`Write`), a model nested past 256 tables/text boxes in the writer (`Write`) | the TS allocates one array entry or one string per unit and runs out of memory; the limits are far above any real document |
| D5b-8 | The writer builds the body in UTF-16 units (`U16Buf`) and encodes once; a lone surrogate becomes U+FFFD, and a high surrogate at the end of one plain run joins a low one at the start of the next, as in the JS string | `TextEncoder` semantics (A1.4); declared in `write_odt.rs` |
| D5b-9 | Pictures are read from the `MediaStore` of the model (`MediaRef`) and the reader adds them with `media.add`; the TS holds the bytes in the run. A missing media id writes an empty picture, as `Uint8Array` of length 0 would | the model stores media once |
| D5b-10 | `decode_base64` is a hand port of `atob` (Node's checks) and not a library | no dependency; covered by the 15 `err:base64` and the valid cases |
| D5b-11 | `heading_level_of` and the other `/i` patterns canonicalize each unit with `regexp_canonicalize` (single-unit upper-casing, no `u` flag) | the TS flag semantics |
| D5b-12 | `final_view` (`finalBlocks` of `revisions.ts`) is ported only as far as the exporters and the diff read it (`finalRuns`, `finalParagraphs`); the rest of `revisions.ts` is D2a's. In a table, the cell paragraphs go through it and the nested tables of a cell stay as they are, as the TS replaces `cell.paragraphs` | session work is a separate frontier |
| D5b-13 | The reader sets `ids` (paragraph, table, drawing) and `media` on the model it builds; the digest does not emit them (A6: ids relabelled/ignored) | the Rust model carries ids the TS gets from process globals |

No deviation changes a result on the 7,398 captured records except the three named below.

### Named accepted differences (in `tests/parity_odf.rs`)

| Name | Where | Rule | Records |
|---|---|---|---|
| **ODF nesting past `max_nesting` (64)** | `odt_accepted_reason`: `quirk/odf/list-deeper-than-limit.odt`, `table-nested-deeper-than-limit.odt`, `frame-nested-deeper-than-limit.odt`, `text-section-deep.odt` | decision D5b-3: the block walk reads the element as text. Facet `rd`. | 3 accepted in the run (the fourth of the four named inputs equals the TS) |
| **G20** (docx reader): a story nested past `max_nesting` (64) is read as plain text | `accepted_reason`: synthetic docx `quirk/full/nest-*-N` with N > 64 (docx → odt pipeline) | the docx reader's rule, inherited by the pipeline | 0 accepted in the run (the pipeline output equals the TS on these inputs; the rule is kept as a guard) |
| **D6** (uo-opc): the relationship target is resolved with the OPC rule | `accepted_reason`: `quirk/full/rel-absolute`, `rel-dotdot`, `rel-uppercase`, `rel-fragment`, `rel-encoded` | the docx reader's D6, inherited | 0 accepted in the run (same) |

## Memory, work and security

- **Memory gate**: **no ODF-specific M1/M3/M5 test exists** (no `memory_*`, `nopanic_*` or fuzz target
  calls `read_odt`/`write_odt`; checked with a search over `tests/`, `examples/` and the workflows). The plan's D5 soak
  ("1,000 × read → html → markdown → odt → read_odt → diff", `uo-docx.md` memory table) has not been run. Not
  measured; listed in "Not done".
- **Work bounds by construction** (not measured by a dedicated test): the inline walk, column reader, style
  resolution and list writer are iterative; the style cache is capped at 4,096; the limits of items 5, 7 and 9
  bound recursion, repeats and spans; the package limits (`DocxLimits.package`) bound the zip. The parity set
  includes hostile shapes (cycles, deep nesting, huge repeats, base64 damage, zip damage) and finishes the 7 tests
  in 9.06 s.
- **No-panic**: the code uses `get`/`first`/`last` access and `Result` paths; there is no panic test for ODF.
  Not measured.

## Checks

`tests/parity_odf.rs` (7 tests, above). Unit tests in `units.rs` (`lengths`), `reader.rs` (`base64_follows_atob`,
`style_ids_lose_white_space`) and `write_odt.rs` (`escape_text_follows_the_ts`, `heading_levels`). Debug tools:
`js/odf-dump.mjs` (TS side) and `examples/odf_dump.rs` (Rust side): `docx <file> <part>`, `trace <key>`;
`UO_ODF_DUMP` in the test. Recapture: `UO_TS_ROOT=<built checkout of main> node
core/crates/uo-docx/js/capture-odf.mjs [--only <key prefix>]`. clippy/rustfmt status of the 2026-11-05 clippy-only
rewrites: not re-run for this record.

## Not done

- ODF memory (M1/M3/M5), the D5 soak, a no-panic/fuzz target for `read_odt`/`write_odt`.
- `.odt` made by Word (`SaveAs2` format 23) or LibreOffice for the read side (U15/U20): none exist in the
  repository and the producers are not on the build machines.
- Wiring to the wasm/FFI surface (`uo-wasm`, `uo-ffi`, the C# facade): no bindings call `odf` yet.
- A V8 regex-site file for ODF in the style of `js/regex-sites/*.mjs` (the sites are covered through the parity
  inputs).
- CHANGELOG lines (with the switch).

## Process metrics

| Item | Value |
|---|---|
| Model / mode | Sonnet, hybrid mode, faithful 1:1 port of D5 (export, ODF, compare), with Opus fixes on 2026-11-05 (the digest, `TextboxView::TopLevel`, clippy-only rewrites) |
| Parity rounds Rust vs TS | not recorded for the port itself; the first full replay of the facet, on 2026-11-05, showed about 4,400 mismatches, all in the harness digest (fix (a)); fix (b) corrected one model-shape difference; the final run has 0 mismatches |
| Rework | clippy-only rewrites of 2026-11-05 (D5a's, see `D5a.md`); fixes (a) and (b) above |
| Time | not recorded (the session that wrote the code was not timed; this record was written afterwards from the code and a fresh test run) |
| Tool calls | not recorded |
