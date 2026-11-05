# uo-docx D5a (export and compare): declarations

Brief: `../uo-docx.md`, "D1b — writer (architecture)", "Emitter frontiers" (the nine kinds of declaration, one
table per kind, each row with the TS file:line), applied to D5 ("derived documents"), parts `export` and
`compare`. Code: `src/export/{mod,model,from_docx,html,markdown,matchers,style}.rs` (feature `export`),
`src/compare/{mod,tokens,myers,histogram,diff,json,budget,matchers}.rs` (feature `compare`), and the two helpers
both use, `src/final_view.rs` and `src/u16buf.rs` (the ODF record, `D5b.md`, shares them). Frozen TS:
`write/packages/docx-core/src/export/` (`model.ts`, `from-docx.ts`, `html.ts`, `markdown.ts`, `index.ts`) and
`write/packages/compare/src/` (`tokens.ts`, `myers.ts`, `histogram.ts`, `diff.ts`, `index.ts`; the
`<universal-write-diff>` element, `universal-write-diff.ts`, is the shell's and is not ported). Sonnet, hybrid
mode, 2026-11-03/05; the record was written afterwards (2026-11-05) from the code, the TS and the test runs of
that day. The myers/histogram sources are a port of the OpenCompare project: an algorithm correction goes to
both repositories (memory note "Port do motor de diff sincroniza com OpenCompare").

**Ported** (TS name → Rust name):

- `export/model.ts` → `export::model`: `ExportDoc` (borrowing `ExportDoc<'a>`, see decision D5a-1),
  `finalExportDoc` → `FinalDoc` (`new`, `ordered_blocks`, `note_paragraphs`), `OrderedBlock`, `orderedBlocks` →
  `ordered_blocks` (+ `interleave`), `headingLevel` → `heading_level` (over `matchers::heading_level_of`),
  `paraPlainText` → `para_plain_text`, `isListItem` → `is_list_item`, `escapeHtml`/`escapeAttr` →
  `escape_html_into`/`escape_attr_into` (+ `_str_` forms), `imgMime` → `img_mime`, `base64` → `base64`,
  `dataUri` → `data_uri`, `resolveImageUri` → `resolve_image_uri`, `twipsToPx`/`twipsToPt`/`halfPtToPt` →
  `twips_to_px`/`twips_to_pt`/`half_pt_to_pt`. Rust-only: `NumberKey`, `OrderedNumbers` (a TS `Set<number>`),
  `comments_by_id` (a TS `Map<number, CommentInfo>`).
- `export/from-docx.ts` → `export::from_docx`: `exportDocFromFullDocument` → `export_doc_from_full_document`
  (also `ExportDoc::from_document`). `docxToMarkdown`/`docxToHtml` of that file only delegate to the converter
  package's own copy of the exporters (class prefix `wn-`, no final view): **not ported here** (converter port).
- `export/html.ts` → `export::html`: `HtmlOptions` (`images: 'data' | 'omit'` → `ImagesMode`), `Ctx`, `HIGHLIGHTS`
  → `highlight_css`, `cssBorderStyle` → `css_border_style`, `edgeCss` → `edge_css`, `runHtml` → `run_html`,
  `paraStyles` → `para_styles`, `paraInner`, `paraHtml`, `tableHtml` → `table_html`, `listHtml` → `list_html`
  (stack instead of recursion), `bodyHtml`, `STYLE` → `style::STYLE` (copied byte for byte by
  `tools/gen-export-style.mjs`), `toHtml` → `to_html` / `to_html_units`.
- `export/markdown.ts` → `export::markdown`: `MarkdownOptions`, `escapeMd` → `matchers::escape_md_into`,
  `emphasize`, `runMd` → `run_md`, `inlineRuns` → `inline_runs`, `cellText`, `tableMd` → `table_md`, `toMarkdown` →
  `to_markdown` / `to_markdown_units`.
- `export/index.ts`: the re-exports are `export/mod.rs`'s `pub use` list.
- `compare/tokens.ts` → `compare::tokens`: `BlockToken` (keys interned to `KeyId`), `fnv1a`, `bytesEqual` →
  `bytes_equal`, `sanitize`, `imageContentMarker`, `frame`, `runPropsSig`, `paraPropsSig`, `paragraphContentKey`,
  `paragraphFormatKey`, `collectParagraphImages`, `cellContentKey`, `tableContentKey`, `cellFormatKey`,
  `tableFormatKey`, `collectTableImages`, `tokenizeBlock(s)`, `tokensEqual` (+ `tokens_equal_cost`), `rowContentKey`
  → `row_content_key`, `rowFormatKey` → `row_format_key`; Rust-only `Interner`.
- `compare/myers.ts` → `compare::myers`: `DiffOp`, `EqFn`, `myersDiff` → `myers_diff` (returns `None` past the
  limits), plus the coarse script (`coarse_script`).
- `compare/histogram.ts` → `compare::histogram`: `histogramDiff` → `histogram_diff` (the `recurse` driver is an
  explicit stack of `Task`s).
- `compare/diff.ts` → `compare::diff`: `DocEditKind`, `BlockRange`, `DocIntraEdit`, `TableRowEdit`, `DocEdit`,
  `DocDiffStats`, `DocDiff`, `DocDiffOptions`, `DEFAULT_DOC_DIFF_OPTIONS`, `opsToRuns`, `runsToEdits`, `wordTokens`
  → `matchers::word_tokens`, `intraWordMarks` → `intra_word_marks`, `tableRowDiff` → `table_row_diff`, `diffBlocks`
  → `diff_blocks` / `diff_blocks_with_limits`, `diffDocuments` → `diff_documents` / `diff_documents_with_limits`
  (in `compare/mod.rs`); the shape of the result as JSON is `compare::json` (`DocDiff::to_json`).
- `revisions.ts` (`finalRuns`, `finalParagraphs`, `finalBlocks`, lines 72–123, the part the exporters and the diff
  read) → `final_view::final_paragraphs` and `compare::final_blocks`; `paraText` (`model-ops.ts:35`) →
  `compare::diff::para_text`. The rest of `revisions.ts` is D2a's.
- Rust-only (no TS source): `u16buf::U16Buf` and `trim_units` (a JS string under construction, see D5a-2),
  `compare::budget` (the work limits, D5a-8), `export::ExportError`.

## Evidence

Node 24.15 x64 captures from the frozen TS (`UO_TS_ROOT`); Rust numbers from a debug-profile run on 2026-11-05,
`cargo test -p uo-docx --features canon,odf,export,compare` (CARGO_TARGET_DIR `core/target/agent-docs`).

| Facet | Capture → test | Result of the run |
|---|---|---|
| `export` | `js/capture-export.mjs` → `tests/parity_export.rs` (5 tests: 4 corpus shards + the synthetic quirks; 2.4 s) | **15,154 results equal, 0 mismatches, 1 accepted, 0 not replayed**: shards 3,530 + 3,255 + 3,455 + 3,525, quirks 1,389 + 1 accepted |
| `compare` | `js/capture-compare.mjs` → `tests/parity_compare.rs` (5 tests; 75.6 s) | **19,033 results equal, 0 mismatches, 0 accepted, 0 not replayed**: shards 2,766 + 4,469 + 2,469 + 8,453, quirks 876 |
| export regex sites | `js/regex-sites/d5a-export.mjs` → `tests/regex_d5a_export.rs` | **875 results equal to V8** (5 sites × 175 inputs) |
| compare regex sites | `js/regex-sites/d5a-compare.mjs` → `tests/regex_d5a_compare.rs` | **92 results equal to V8** (2 sites × 46 inputs) |
| unit tests of the modules | `cargo test --lib` over `compare::`, `export::`, `final_view`, `u16buf` | 16 tests pass (myers 4, histogram 4, compare matchers 1, export matchers 3, final_view 2, u16buf 2) |

**Export capture** (`tests/parity/export.capture`): 3,032 documents (the 2,753 oracle/fixture docx of the corpus
plus 279 synthetic packages of `js/quirk-d5a.mjs`: lists with odd levels, tracked changes, comments, notes,
highlights named like `Object.prototype` members, tables, images, whitespace around emphasis, seeded mixes,
seed `0xd5a00001`), 15,156 facets, none skipped and none unreadable. Facets per document: `html` (full document),
`html-frag` (fragment, images omitted), `html-title` (title `T <&> "x"`), `md`, `md-omit` (images omitted, with a
title); each is the digest of the output string (UTF-8, lone surrogates as U+FFFD). In 4 documents the TS
Markdown export throws (`error-md`: a list level `'  '.repeat(ilvl)` rejects) and the Rust side returns
`ExportError::InvalidListLevel` in the same 4.

**Compare capture** (`tests/parity/compare.capture`): 6,347 pairs of documents (292 synthetic): every corpus
document with itself, with the next one of the corpus order, and one in five with the seventh after it; the
synthetic pairs derive a second document from the first by one edit or several (insert, delete, change and move
paragraphs; change text inside runs, split runs; change formatting; table rows and cells; images resized or
replaced; unrelated documents, repeated paragraphs, reversed order, words, tracked changes; seeded, 260 each).
Facets: `diff` (histogram, word refinement; 6,347), `diff-myers` (6,339: pairs of at most 800 blocks together),
`diff-nointra` (6,347); each is the digest of `JSON.stringify(diffDocuments(a, b, opts))`. Pairs above 3,000
blocks together (4,000 for a document with itself) are not captured, because the TS needs O((N+M)·D) memory for
Myers and O(N·M·N) time for the histogram on repetitive text.

**Clippy-only rewrites of 2026-11-05** (behaviour unchanged, parity unchanged: every number above is from the run
after them): `myers.rs`, the snapshot loop `k <= di - 1` → `k < di` (integers, identical); `diff.rs`, the delete
+ insert fold test through `matches!`; `html.rs` `list_html`, the TS condition `ilvl >= level && !(ilvl > level)` →
`ilvl == level` (identical for NaN, which makes both false, and for ±0); `export/matchers.rs`
`collapse_blank_lines`, `repeat_n`. Clippy re-run for this record on 2026-11-05 (clean, with the tests).

## 1. Numeric parse/format sites and the uo-jsnum function used

| Site | uo-jsnum / Rust |
|---|---|
| model.ts:78 `Math.min(6, parseInt(m[1], 10))`; :80 `parseInt(m[1], 10)` (a single ASCII digit) | `char::to_digit(10)` (ASCII only) with `.min(6)`; no `parse_int` needed |
| model.ts:142 `twipsToPx` `Math.round((t / 1440) * 96)` | `math::round(JsNumber::to_number(t) / 1440.0 * 96.0)`, same operation order (`null` is 0) |
| model.ts:143 `twipsToPt` `+(t / 20).toFixed(2)` | `format::to_fixed(.., 2)` then `parse::to_number` (the unary `+`) |
| model.ts:144 `halfPtToPt` `h / 2` | `f64` division |
| html.ts:52 `Math.max(0.5, e.sizeEighthPt / 8)`, printed `${w}pt` | `math::max` (NaN stays NaN), `format::to_js_string` |
| html.ts:65–67 `width="${w}" height="${h}"` of the image | `U16Buf::push_num` (`to_js_string`) |
| html.ts:82 `p.szHalfPt` truthy then `font-size:${halfPtToPt(..)}pt`; :87 `p.charSpacingTwips`; :115–117 the three indents | `JsNumber::truthy`, `to_js_string(twips_to_pt(..))` / `half_pt_to_pt` |
| html.ts:71, :75, :203 `ctx.notes.length + 1`, `idx + 1` | `usize` printed with `to_string` |
| html.ts:104 `p.commentIds.join(',')` (`null` joins as the empty string); :214 `cmt-${id}` (`null` prints `null`) | `join_number`, `to_js_string_null` |
| html.ts:148 `cell.gridSpan > 1` and `colspan="${cell.gridSpan}"` | `n.to_number() > 1.0 && !n.is_null()`, `join_number` |
| html.ts:165–169 `items[i].ilvl >= level`, `> level` on numbers (NaN possible) | `f64` compares; `==` for the first (D5a clippy note above) |
| markdown.ts:322 `++ctx.noteSeq`; :341 `[^c${id}]`; :325 `[^n${seq}]` | `usize`; `number_text` (`null` prints `null`) |
| markdown.ts:388 `'  '.repeat(p.props.numbering.ilvl)` | `indent_count`: `ToIntegerOrInfinity` (NaN → 0, `math::trunc`); negative or infinite → `ExportError::InvalidListLevel` (the TS `RangeError`); past 65,535 → `ListLevelLimit` (D5a-4); `conv::to_uint32` |
| markdown.ts:394 `'#'.repeat(lvl)` (1..6) | `str::repeat` |
| tokens.ts:48–52 `fnv1a`: `Math.imul(h, 0x01000193)`, `h >>> 0`, `.toString(16)` | `u32::wrapping_mul`, `format!("{h:x}")` |
| tokens.ts:74 `data.byteLength`; :77 `s.length` of the framed string; :148 `t.length` | `len()` of bytes / of the UTF-16 buffer (`U16Buf::len`) |
| tokens.ts:88, :94, :108–113, :213 `p.szHalfPt ?? ''`, `charSpacingTwips ?? ''`, indents, spacing, `c.widthTwips ?? ''` (numbers inside an array `.join`) | `opt_num`: `Some(Num)` → `to_js_string`, otherwise the empty string |
| tokens.ts:107 `${numbering.kind}:${numbering.ilvl}`; :172 `${widthTwips}x${heightTwips}`; :197 `span${c.gridSpan}` | `num_template` (`null` prints `null`) |
| tokens.ts:172 `${offsetXTwips ?? 0},${offsetYTwips ?? 0}` | `opt_num_or_zero` |
| tokens.ts:179 `curLen += r.text.length` | `usize` over UTF-16 units |
| tokens.ts:224 `t.gridColsTwips.join(',')` | `null` entries as the empty string, numbers by `to_js_string` |
| myers.ts:49–88 `max = n + m`, `Int32Array` V, `x = down > right ? down : right`, `y = x - k`; :62 `k += 2` | `isize` arithmetic; V values stored as `i32` in the snapshots; `max >= 2^30` answers `None` (a TS `Int32Array` of that size fails) |
| histogram.ts:240–306 `Number.POSITIVE_INFINITY` start values, `|aL − bL|`, comparisons | `Option<Best>` (the first candidate always wins), `usize::abs_diff` |
| diff.ts:134–153 `aStart = aPos + 1`, `end = start + len − 1`, `end = start − 1` for an empty range | `usize`; `range_of` uses `saturating_sub` (`start ≥ 1`, so no underflow) |
| diff.ts:233 `m.index + m[0].length` (UTF-16 offsets) | code-unit offsets from `word_tokens` |
| diff.ts:365 `Math.min(..)`, :394 `Math.max(..)` | `Ord::min` / `max` |

## 2. Regex sites and their matcher (dynamic ones through a core template, named)

No dynamic pattern: every `RegExp` of these files is a literal. The matchers are hand-written, linear, over code
units (G9); the white space `\s` is the JS set (`uo_jsnum::text::is_js_space_unit`), and a lone surrogate is `\S`.

| Site | Matcher |
|---|---|
| model.ts:77 `/(?:heading|t[íi]tulo|titulo)\s*([1-9])/i` | `matchers::heading_level_of` (`ci_eq`/`match_ci`: with `/i` and no `u`, a unit canonicalizes by the simple upper-case map and never maps a non-ASCII character onto ASCII; `í` matches `Í`; U+0131 and U+212A do not match) |
| model.ts:79 `/^h([1-6])$/i`; :81 `/^(title|t[íi]tulo)$/i` | the same function |
| model.ts:97 `escapeHtml` (`& < >`, three `replace` calls); :101 `escapeAttr` (then `"`) | `escape_html_into`, `escape_attr_into` (one pass; the same result because `&` is replaced first) |
| model.ts:106–113 `ext.toLowerCase()` | `uo_jsnum::text::to_lowercase` |
| html.ts:83 `HIGHLIGHTS[p.highlight] ?? p.highlight` (an object lookup; not a regex, but prototype-dependent) | `highlight_css` (D5a-5) |
| markdown.ts:301 `/([\\`*_{}\[\]<>|])/g` → `\\$1` | `matchers::escape_md_into` |
| markdown.ts:305 `(t.match(/^\s*/) ?? [''])[0]` | `lead_space_len` |
| markdown.ts:306 `(t.match(/\s*$/) ?? [''])[0]` (leftmost match: for an all-white string it starts at 0) | `trail_space_start` |
| markdown.ts:348 `.replace(/\|/g, '\\|').replace(/\n/g, ' ')` | `cell_text_escape` |
| markdown.ts:413 `.replace(/\n{3,}/g, '\n\n').trim()` | `collapse_blank_lines`, `u16buf::trim_units` |
| html.ts:73 / markdown.ts:324 `.trim()` | `trim_units` |
| tokens.ts:70 `s.includes(SEP)`, `s.split(SEP).join('')` | `push_sanitized` / `sanitized_str` (drop U+001F) |
| tokens.ts:136 `r.field.trim()` | `uo_jsnum::text::trim` |
| diff.ts:230 `/\S+|\s+/g` | `compare::matchers::word_tokens` |
| diff.ts:259 `/\S/.test(text)` | `compare::matchers::has_non_space` |

Checked against V8 in `tests/regex_d5a_export.rs` (875 results: `heading_level`, `escape_md`, `lead_trail`,
`cell_text`, `collapse`, 175 inputs each) and `tests/regex_d5a_compare.rs` (92: `word_tokens`, `has_word`, 46
inputs each); the corpus parity (items above) exercises the rest.

## 3. Template-literal values printed without escaping

| Site | Values |
|---|---|
| html.ts:61–63 | constant markup |
| html.ts:67 `<img src="${resolveImageUri(..)}" width= height=` | the data URI (the cache value is the host's, **not escaped**; the built one has no `"`), the two numbers |
| html.ts:75, :203 | `seq`, `idx + 1` |
| html.ts:81–87 `color:#${p.color}`, `font-family:'${p.fontAscii}',serif`, `background-color:${HIGHLIGHTS[..] ?? p.highlight}` | `color`, `fontAscii`, `highlight` **unescaped inside a `style="..."` attribute**: a `"` in the model breaks the attribute (ported as is) |
| html.ts:53 `edgeCss` `#${e.color}`; :118 `shadingHex`; :149 `cell.shadingHex` | unescaped, inside `style="..."` |
| html.ts:104 `data-comment="${ids.join(',')}"` | numbers |
| html.ts:137–140 `<h${lvl}${attr}>` | `lvl` 1..6, `attr` the style string |
| html.ts:73–74, :203 | the note text is escaped by `escapeHtml` twice (`noteHtml`, then `html: escapeHtml(noteHtml)`): ported as is |
| markdown.ts:320 `![](${uri})`; :329 `[${t}](${link})`; :377 `# ${title}`; :408 `**${author}**: ${c.text}`; :325 `[^n${seq}]: ${txt}` | URI, link, title, comment author and text, note text: **not Markdown-escaped** (`escapeMd` is applied to run text only) |
| tokens.ts:73–77, :137, :145, :148 | `data.byteLength`, `fnv1a`, `instr`, `kind`, `custom.type`, `fallbackText` (sanitized of U+001F) inside the keys |

The Rust side formats the same values (`format!`, `U16Buf::push_str`) with no extra escape.

## 4. Object/Map orders relied on (JSON keys, `Object.entries`)

| Site | Order |
|---|---|
| html.ts:37–43 `HIGHLIGHTS[p.highlight]` on a model string | a plain object lookup: a name `Object.prototype` has gives the function's source text: `constructor` → `function Object() { [native code] }`, `__proto__` → `[object Object]`, and `toString`, `valueOf`, `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`, `toLocaleString`, `__defineGetter__`, `__defineSetter__`, `__lookupGetter__`, `__lookupSetter__` → `function NAME() { [native code] }` (`highlight_css`, checked by the `export-highlights` quirk) |
| html.ts:246, markdown.ts:366 `new Map(comments.map(c => [c.id, c]))` | a later duplicate id wins (`comments_by_id`); keys are numbers, `null` and NaN included (`NumberKey`) |
| html.ts:247, :209 `usedComments: Set<number>`, `[...usedComments].filter(..)`; markdown.ts:335–343 `ids = new Set`, :294, :403 | insertion order, no duplicates (`OrderedNumbers`) |
| model.ts:37–38 `new Map([...m].map(..))` (the final view of the notes) and `get(id)` | lookup by id only (`note_paragraphs`) |
| model.ts:60–71 `byPara: Map<number, TableBlock[]>` | read by index, never iterated; tables of one anchor keep the order of `tableAnchors` (`interleave`) |
| html.ts:178–206, markdown.ts:375–410 the order of `parts` / `out` | the block order of `orderedBlocks`, then notes, then comments |
| tokens.ts:116, :119, :122, :215, :225 `JSON.stringify(tabStops / borders / sectPr / cell.borders / table.borders)` | the key order of the objects the reader builds (A6/LQ20), written by `JsonWriter` over the model's `JsValue` view; an error in the writer gives the empty string (not reachable on a reader-built model) |
| histogram.ts:228–237 `histA = new Map<string, number[]>()` | looked up only; each bucket lists the indices in ascending order (`HashMap<u32, Vec<usize>>`), and the scan of B is in order, so the candidate order is the TS's |
| diff.ts JSON (`JSON.stringify(diff)`): `DocEdit` keys `kind, a, b, aBlocks, bBlocks`, then `formatOnly` and `importance` (a format-only change), then `intra`, then `tableRows` (assigned after the creation); `TableRowEdit` `kind, aRows, bRows, formatOnly`; `DocIntraEdit` `kind, side, pairOffset, startCh, endCh`; `stats` `inserted, deleted, changed, formatOnly, equal` | `compare::json` writes this order (`DocDiff::to_json`); `limited` is not part of it |

## 5. Recursions and their bound

| Site | Bound |
|---|---|
| html.ts:143–157 `tableHtml` → `tableHtml` over `cell.tables` | recursion kept, cut at `MAX_TABLE_DEPTH` = 256 (an inner `<table></table>` and nothing below); the reader stops at `max_nesting` = 64 stories |
| html.ts:162–173 `build` (nested lists) | `list_html` is iterative: an explicit stack of frames; depth up to the number of levels, memory linear in the items |
| markdown.ts `tableMd` | no nesting: only `cell.paragraphs`; nested tables of a cell are not exported |
| tokens.ts:192–228 `cellContentKey`/`tableContentKey`/`cellFormatKey`/`tableFormatKey`; :230–237 `collectTableImages` | recursion kept, cut at `MAX_DEPTH` = 512 (a nested table below that adds nothing to the keys or the images) |
| histogram.ts:196–334 `recurse` (depth up to the number of blocks) | explicit stack of `Task::Range`/`Task::Equal`, the order of output unchanged (before, the anchor run, after); unit test `deep_recursion_is_iterative` (4,000 blocks) |
| myers.ts, diff.ts, `final_view` | iterative |

## 6. Module state and where it lives in `WriteCtx`

None of this is the writer, so `WriteCtx` is not involved. The TS modules hold only constants (`HIGHLIGHTS`,
`STYLE`, `SEP`) and per-call context objects (`Ctx` in `html.ts` and `markdown.ts`: comment map, used set, notes,
sequence counter, image mode; the `FinalDoc` of the final view). In Rust the constants are `const`/`static`, each
`Ctx` is a local struct of the call (`to_html_units`, `to_markdown_units`), and the compare keeps per call an
`Interner` (strings → dense `u32` ids; the TS compares strings) and two `Budget`s (the work counters, Rust only).
Nothing survives a call; the functions are reentrant.

## 7. `throw` sites → `DocxError::Write`

These modules return their own errors (`ExportError`) or none; they never write a package.

| Site | Rust |
|---|---|
| markdown.ts:388 `'  '.repeat(ilvl)` with a negative or infinite `ilvl` (`RangeError`) | `ExportError::InvalidListLevel` (4 of the 3,032 corpus documents) |
| the same, with a level whose string passes V8's string length (2^29 − 24 units) | not reproduced: `ExportError::ListLevelLimit` past 65,535 levels (D5a-4) |
| any output string past 2^29 − 24 units (V8 `RangeError`) | `ExportError::OutputLimit` past 2^28 units, Markdown only (D5a-4) |
| markdown.ts:354 `Math.max(...rows.map(r => r.length))` (a spread: a `RangeError` for a table of more than about 120,000 rows) | not reproduced: `Iterator::max`; the table is rendered (D5a-9) |
| model.ts:126 `throw new Error('sem base64 (btoa/Buffer)…')` | none: `base64` is always available |
| myers.ts:56 `new Int32Array(size)` for a huge `n + m` (a `RangeError`) | `myers_diff` answers `None` for `n + m >= 2^30`, and the caller falls back to the coarse script |

The compare has no error type: a limit is `DocDiff::limited = true`.

## 8. Evaluation order around `revisionAttrs` / `mediaRelId` / `linkRelId`

None of the three is used (no writer). The orders that matter and are kept:

- The sequence numbers of notes and the set of used comments follow the visit order of the runs: `runHtml`
  (html.ts:70–76, :98–105) numbers a note and adds the comment ids when the run is visited, and the list items
  are rendered (`paraInner`, :189) when they are buffered, not when the list is flushed; so the numbers follow
  the document order of the blocks. `body_html` does the same. The markdown `inlineRuns` (:333–343) visits all
  the runs of a paragraph (notes defined in run order), then appends the references to the paragraph's comments;
  `tableMd` computes every cell's text in row order before it writes the header (:352).
- `tableHtml` writes a cell's paragraphs and then its nested tables (:150–151); `cellContentKey` the same
  (:194–196).
- `finalExportDoc` (model.ts:35–51) is applied once at the start of `toHtml`/`toMarkdown`: the body, the table
  anchors (re-targeted to the final paragraph list) and the notes; the cells of a body table are finalized when
  rendered (`final_paragraphs` of the cell), the tables nested in a cell are not.
- The compare interns the keys of side A, then side B, then (on demand) the rows of changed table pairs; ids carry
  no order, only equality, and the tie-breaks of the histogram use indices.

## 9. Deviations (Opus check first): decisions outside the TS semantics

| # | Deviation | Why |
|---|---|---|
| D5a-1 | `ExportDoc<'a>` borrows the model (`body: &Story`, `comments`, `footnotes`/`endnotes: Option<&JsMap<NumKey, Story>>`, `media`, `image_data_uris: Option<&HashMap<DrawingId, Box<str>>>`) instead of the TS arrays (`paragraphs` + `tableAnchors`); the `sectPr` field is not carried (no exporter reads it); `title` is `None` from a document | the model is one `Story` per container (A2); the `WeakMap` cache is keyed by the image object in the TS, by the drawing id here |
| D5a-2 | Output is built in UTF-16 code units (`U16Buf`) and converted once (`from_utf16_lossy`): a lone surrogate becomes U+FFFD (`TextEncoder`), and a surrogate pair split across two runs joins | A1.4: the model text is UTF-16 with lone surrogates; the JS string concatenation joins pairs |
| D5a-3 | `docxToMarkdown` / `docxToHtml` of `from-docx.ts` are not ported (they call the converter package) | they belong to the converter port |
| D5a-4 | Limits the TS does not have: `MAX_LIST_LEVEL` = 65,535 (the TS indents with a string of up to 2^29 − 24 units), `MAX_OUTPUT_UNITS` = 2^28 for Markdown (the TS fails at 2^29 − 24), `MAX_TABLE_DEPTH` = 256 (HTML), `MAX_DEPTH` = 512 (keys and images) | bounded memory and stack on untrusted input; the limits are far above any real document. **Named accepted difference "D5a list-level limit"**: `quirk/d5a/export-list-huge.docx#export` (a level of 70,000: the TS indents it, the Rust refuses past 65,535); in the run, 1 accepted (`error-md`; its `md` and `md-omit` facets are then not replayed) |
| D5a-5 | `HIGHLIGHTS[name]` of a name `Object.prototype` has gives V8's function source text; `highlight_css` has the 12 names and their texts written out | faithful to the quirk (the TS object has a prototype); checked by `export-highlights` |
| D5a-6 | The HTML list builder keeps the TS behaviour for a run of list items whose first is not the shallowest: it stops at the first item shallower than the first one and **the rest of the run is dropped**; a note's text is HTML-escaped twice; the comment tooltip joins only the comments the map has | ported as is (the output is what the TS gives) |
| D5a-7 | Tokens: the keys are UTF-16 strings **interned to `u32` ids** (equal strings, equal ids), so `eq`, the histogram buckets and the format-key test compare integers; `fnv1a` stays (it is part of the image marker text); the images are the bytes of the `MediaStore` of each side | speed and memory; results identical |
| D5a-8 | **Work limits (`compare::budget`)**: block level `DIFF_WORK_LIMIT` = 2^26 steps, `MYERS_TRACE_CELLS` = 4 Mi `i32` cells (D up to about 2,900), word refinement `INTRA_WORK_LIMIT` = 2^25 steps and `INTRA_TRACE_CELLS` = 1 Mi cells (D up to about 1,400); image byte comparison charged at one step per 256 bytes. Past a limit the range is answered with the **coarse script** (common prefix and suffix, the middle deleted then inserted: valid, not minimal) or, for a word refinement, no marks; `DocDiff::limited` says so (not in `to_json`). Below the limits the result is the TS's, exactly; the parity replay runs with unlimited limits and asserts `!limited` | the TS is unbounded (Myers O((N+M)·D) memory, histogram O(N·M·N) time on repetitive text); a hostile pair must finish in bounded time and memory. Accepted by design; **Opus to confirm the numbers** |
| D5a-9 | `tableMd`'s `Math.max(...)` over rows has no spread limit in Rust | a TS `RangeError` on a table of more than about 120,000 rows is a V8 artefact |
| D5a-10 | Myers keeps, per step `d`, only the `d` diagonal values the backtrack reads (O(D²) cells) instead of a snapshot of the whole `V` (O(D·(N+M))): the same script | memory |
| D5a-11 | `histogram_diff` and `list_html` replace the recursion with explicit stacks | no host stack dependence (item 5) |
| D5a-12 | `diffDocuments(a, b)` takes two `&Document`s; the final view is `compare::final_blocks` (the cells' paragraphs final, the tables nested in a cell as they are, as `finalBlocks` rewrites `cell.paragraphs` and leaves `cell.tables`); the TS `cell.blocks` has no Rust counterpart (a cell is one story) | model shape (see also D5b-4) |
| D5a-13 | `final_view::final_paragraphs` returns `Cow` (borrowed when nothing in the paragraph changed) and an `index` vector; the TS returns `{ paras, index }` with `p` itself or `{ ...p, runs }` | no copy of untouched paragraphs |
| D5a-14 | `to_json` is the shape of `JSON.stringify(DocDiff)` for the shell and for parity | the binding (M4) |

No deviation changes a result on the captured records except the named accepted difference (D5a-4); the work
limits (D5a-8) are not reached by any captured pair.

## Memory (measured)

2026-11-05, debug build, `uo-alloc` counting allocator, one thread per test. Tests: `tests/memory_export.rs`,
`tests/memory_compare.rs` (and `tests/memory_odf.rs` for the D5 soak, see `D5b.md`); the documents come from
`tests/d5_support/mod.rs` (corpus samples across the size range, the synthetic packages of `export.inputs` and
`compare.inputs`, generated bodies). The numbers below are what the tests print (`--nocapture`).

**M1 — no leak per operation (0 B left after the outputs are dropped, `clear_caches()` first)**

| Area | Operations | Inputs | Result |
|---|---|---|---|
| Export | `ExportDoc::from_document`, `to_html` (full, fragment with images omitted, titled), `to_html_units`, `to_markdown` (plain, titled with images omitted), `to_markdown_units`, `ordered_blocks`, `heading_level`, `is_list_item`, `para_plain_text`, the escapes, `base64`/`data_uri` | 129 documents (36 corpus, 93 synthetic) in every shape; error paths: list level 70,000 (`ListLevelLimit`), -1, 1e21, empty documents | 0 B left each; costliest single run 5.3 MB allocated (`oracle/justify5/sh-23.docx`) |
| Compare | `final_blocks`, `blocks_of`, `tokenize_blocks`, `myers_diff`, `histogram_diff`, `intra_word_marks`, `diff_blocks`/`diff_blocks_with_limits`, `diff_documents`/`diff_documents_with_limits` (both algorithms, with and without the word refinement, default and tight limits), `DocDiff::to_json` | 204 pairs (146 of `compare.inputs`, 58 corpus: next and itself); the building blocks over random keys at limits (huge, 200/64, 0/0); empty sides | 0 B left each; costliest 16.2 MB allocated (`seed-68-table-delete-row+image-bytes`) |

**M2 — soak (1,000 iterations, sample every 50, flat after one sample of warm-up, back to the start after `clear_caches()`)**

| Soak | Documents | Live bytes (21 samples) | After `clear_caches()` |
|---|---|---|---|
| 1,000 × (html ×4 shapes, markdown ×3) | 33 | 1,724,420 B in every sample | 1,724,420 B |
| 1,000 × 7 diffs (both algorithms, default and tight limits, `diff_blocks`) | 41 pairs | 5,941,786 B in every sample | 5,941,786 B |
| **D5 soak**: 1,000 × (read → html → markdown → odt → read_odt → diff original/read back), `memory_odf.rs` | 35 packages | 497,456 B in every sample | 497,456 B |

(The baseline is the documents the test holds; the tolerance asserted is 0 B.) The D5 soak runs in 7.7 s.

**M3 — budgets** (allocated bytes / peak live bytes above the start; the budget in the test is the measured
value + 25%, and a regression over 10% of the recorded measurement fails the gate)

| Export | Output | Allocated | Peak |
|---|---|---|---|
| html, small corpus document | 1.5-3.1 KB | 12.7-64.7 KB | 6.3-19.5 KB |
| markdown, small corpus document | 36 B-1.5 KB | 1.4-41.8 KB | 0.75-20 KB |
| html, generated 1,000 blocks | 88 KB | 2.76 MB | 740 KB |
| markdown, generated 1,000 blocks | 73 KB | 2.19 MB | 1.03 MB |
| html, generated 10,000 blocks | 866 KB | 26.7 MB | 6.9 MB |
| markdown, generated 10,000 blocks | 728 KB | 21.1 MB | 9.8 MB |

`ExportDoc::from_document` allocates 0 B (it borrows). Asserted: beyond a fixed part (80,000 B allocated, 20,000 B
peak: the `Ctx`, the final view, the style sheet) at most 40 allocated bytes per output byte (measured worst 30.7
html, 29.0 markdown) and 20 peak bytes per output byte (measured 13.8). The cost is high because the output is
built in UTF-16 (`U16Buf`, 2 B per unit), then copied to a `String`, and the final view clones the paragraphs it
changes; it is linear (the 1,000 and 10,000 block rows have the same ratio).

| Diff (`diff_documents`, histogram, default limits) | Blocks (A+B) | Allocated | Peak | Budget (alloc / peak) |
|---|---|---|---|---|
| corpus pair `tabbeyond/tb-end-default-c15` / next | 60 | 90,717 B | 25,316 B | 114,000 / 32,000 |
| corpus pair `metrics/rule-aptos` / next | 84 | 766,776 B | 47,188 B | 959,000 / 59,000 |
| corpus pair `imgul/iu-ar-l240-22-double` / next | 8 | 13,948 B | 4,222 B | 18,000 / 6,000 |
| generated 500 blocks, 5% edited | 1,000 | 2,419,176 B | 289,960 B | 3,025,000 / 363,000 |
| generated 500 blocks, unrelated | 1,000 | 13,103,334 B | 2,499,310 B | 16,380,000 / 3,125,000 |
| generated 2,000 blocks, 5% edited | 4,000 | 20,296,024 B | 1,157,200 B | 25,370,000 / 1,447,000 |
| generated 2,000 blocks, unrelated | 4,000 | 68,961,168 B | 18,201,704 B | 86,200,000 / 22,750,000 |
| repetitive 2,000 against 1,990 identical | 3,990 | 3,376,102 B | 649,826 B | 4,221,000 / 813,000 |

The cost does not follow the block count alone: it follows the changed paragraphs (word refinement: one Myers
trace per changed pair, up to `INTRA_TRACE_CELLS` = 1 Mi cells = 4 MiB) and the block-level trace (up to
`MYERS_TRACE_CELLS` = 4 Mi cells = 16 MiB, hit by the "unrelated 2,000" row). Observation, not a defect: a pair
of 84 blocks with 408 B of result allocates 767 KB (long paragraphs refined word by word); the first thing to
look at if the diff becomes a hot path.

**M5 — caches**: the exporters and the diff keep **no cache** between calls: `Ctx`, the interner and the budget
live in the call. Tested: the second run of the same export allocates exactly the bytes of the first and keeps
0 B (`m5_no_cache_to_cap`; same for the diff in `m5_the_trace_is_capped_and_nothing_is_cached`). What does grow
with the data, Myers' trace, is capped by `myers_trace_cells`: 4,000 against 4,000 different blocks (D = 8,000)
finishes with a peak of 18,155,566 B (Myers) and 18,553,958 B (histogram) against the 16,777,216 B of the trace
limit (1.4-1.8 MB of linear part), and `limited` is set.

## Work limits (tested)

`tests/compare_limits.rs` (11 tests; it is the file `parity_compare.rs` cites), on generated documents and the
pairs of `compare.inputs`:

- `limited` is `false` for all 73 sampled capture pairs under the default limits, and a result that is not `limited`
  under shrunk limits equals the unlimited one exactly (436 runs); 544 runs under limits from `(0,0,0,0)` to
  `(5000, 4096, 5000, 4096)` are `limited`.
- Under work 0, 1, 10, 50, 200 the script is **valid** for every pair and both algorithms: each block of each side
  in exactly one edit, in order; `equal` edits pair blocks with the same text; the result is deterministic; the
  JSON does not carry `limited`.
- The coarse script keeps the common prefix and suffix (60 + 40 shared blocks around 200 against 180 different:
  the first and last edits are `equal` and cover them) with work 0 and with 8 trace cells.
- A trace limit of 100 cells on a pair with D = 300 sets `limited`; 1 Mi cells does not; an equal pair has D = 0 and
  is never limited, whatever the limit.
- **The work stays within the limit**: counted comparisons of `histogram_diff` and `myers_diff` over four shapes
  (repeating, one value, random over 4 values, disjoint; 3,000 against 3,000) at work 1,000 / 20,000 / 400,000 and
  1 Ki / 64 Ki cells stay under `limit + 4·(n+m)` (histogram) and `limit + 2·(n+m)` (Myers): the slack is the coarse
  script's prefix/suffix scan; the 48 runs take 0.24 s in debug. 4,000 repetitive paragraphs against 4,000 almost
  identical ones finish in 0.20 s (histogram, `limited`) and 0.12 s (Myers).
- **Peak memory follows the trace limit**: 800 blocks all different at 256 / 4,096 / 65,536 / 1 Mi cells peak at
  200 KB / 200 KB / 400 KB / 2.3 MB.
- The word refinement is left out (fewer marks, `limited`), never wrong, when `intra_work` is 0 or the intra trace is
  8 cells; the block-level stats do not change; `intra_word_marks` with a spent budget falls back.
- Limits at the top of their range (`usize::MAX` cells, `u64::MAX` work) are clamped, not overflowed (bug 1 below).

## Security (continuous)

2026-11-05, debug build (overflow checks on), `tests/nopanic_export.rs`, `tests/nopanic_compare.rs`; fuzz
targets `export_docx` and `compare_docx` (`core/fuzz/src/d5.rs`, shared with `odt_read`, see `D5b.md`).

| Test | Inputs | Result |
|---|---|---|
| `nopanic_export`: mutated synthetic packages | 1,500 runs over `export.inputs` + `full.inputs` (bytes flipped, cut, duplicated, `>` removed, tokens for odd levels, spans, revisions, `&#xD800;`), read with default and tight `DocxLimits`, exported in every shape | no panic (about 29% of the runs read) |
| mutated corpus | 600 sampled corpus documents with parts mutated and re-zipped, plus the raw zip bytes mutated | no panic |
| random bodies | 4,000 bodies from 16 fragments and 14 list levels (0, 65,534, 65,535, 65,536, 70,000, -1, 1e21, empty, text, NaN, 2^32) | no panic; 1,868 exported to Markdown, 6,132 refused for the list level |
| list level edge | 65,535 rendered, 65,536 and 70,000 `ListLevelLimit`, never a panic; html unaffected | exact |
| hostile shapes (time bound 20 s each) | 20,000 list items 0.2 s; 200 items at level 65,535 (26 M units of Markdown) 1.6 s; tables nested 10 / 64 / 70 / 300 deep under 3 ms; one paragraph of 1 M characters 0.14 s; 100,000 runs 0.8 s; 20,000 tables 0.8 s; 30,000 emphasis paragraphs 0.55 s | all finish |
| `nopanic_compare`: mutated pairs | 1,500 runs over `compare.inputs` (side A, side B or both mutated), 603 diffed, each in both algorithms, with and without refinement, default and random limits | no panic; the script is a partition of both sides every time |
| random pairs and limits | 2,500 pairs of 15 fragments (tables with merged cells, revisions, text, empty), each diffed 3 ways (a/b, b/a, a/a) in 4 configurations with limits drawn from 0 / 1 / 2 / 10 / 1,000 / 100,000 / `u64::MAX` work and 0 … `usize::MAX` cells | **one panic found and fixed** (below); then none |
| the algorithms directly | 6,000 random key sequences with an `eq` that disagrees with the keys, work 0 … `u64::MAX`, cells 0 … `usize::MAX`; 5,000 random UTF-16 texts (lone surrogates, pairs, NBSP, combining marks) through `intra_word_marks` (marks always inside the text) | no panic after the fix |
| hostile pairs (30 s bound) | 6,000 identical against 5,990 + 10 different: 2.6 s histogram / 0.07 s Myers; 5,000 against 5,000 disjoint 0.43 s / 0.40 s (`limited`); two paragraphs of 60,000 words 0.12 s (`limited`); one word changed in 60,000 0.04 s; 4,000 tables 2.6 s | all finish |

Fuzz (stable driver `core/fuzz/src/bin/fallback.rs`, release build with overflow checks, 70 s each, seeded from the
docx seeds of the reader, the pairs of `compare.inputs` and corpus pairs):

| Target | Executions | Exec/s | Seeds | Crashes | Slowest |
|---|---|---|---|---|---|
| `export_docx` (read → html/units/markdown/units; the String is the lossy form of the units, the full page is whole, a Markdown error is a named `ExportError`) | 1,982,050 | 28,314 | 574 | 0 | 10.6 ms |
| `compare_docx` (two packages → diff in both algorithms and 4 limit sets; partition, determinism, a document equals itself, a not-limited result equals the default-limits one) | 558,010 | 7,972 | 323 | 0 | 21.5 ms |

libFuzzer (`cargo +nightly fuzz run`) was not run: no nightly on this machine.

**Bugs found by these tests (fixed, parity unchanged — `parity_export`, `parity_compare`, `parity_odf` at 100%)**

1. `compare::myers::max_d_for(cells)`: with `DiffLimits::myers_trace_cells` / `intra_trace_cells` = `usize::MAX`
   (or any limit whose triangular number does not fit) `total += d` overflowed: a panic in debug, a loop that never
   ends in release. Defaults are 4 Mi and 1 Mi cells, so only a host that passes an extreme limit was affected.
   Fix: the search is capped by `n + m` (the longest script) and sums with `saturating_add`. Test:
   `limits_at_the_top_of_their_range_are_clamped_not_overflowed`, `nopanic_compare`.
2. `compare::histogram::range`: the work budget charged only the successful comparisons of the run extension
   (backwards and forwards from an anchor candidate), not the one that ends the run: a hostile input made up to
   three comparisons per charged step (measured: 871,748 comparisons at a limit of 400,000). Fix: charge before every
   comparison. Below the limits nothing changes (the parity replay uses unlimited limits); past them the fallback
   starts at the real limit. Test: `the_work_stays_within_the_limit`.

## Checks

`tests/parity_export.rs` (5 tests), `tests/parity_compare.rs` (5 tests), the two regex tests, the unit tests of
the modules (16, all pass); `tests/compare_limits.rs` (11), `tests/memory_export.rs` (5), `tests/memory_compare.rs`
(5), `tests/nopanic_export.rs` (5), `tests/nopanic_compare.rs` (5), all green with `--features
canon,odf,export,compare`; `cargo clippy -p uo-docx --features canon,odf,export,compare --tests --no-deps -- -D
warnings`, `core/tests/grep-bans.sh` and `rustfmt --check` on the files touched are clean. Harness: `tests/d5a_support/mod.rs`. Generators: `js/quirk-d5a.mjs` (synthetic
documents and pairs). Recapture: `UO_TS_ROOT=<built checkout of main> node
core/crates/uo-docx/js/capture-export.mjs` and `.../capture-compare.mjs` (`--only <key prefix>`); the CSS is
regenerated by `tools/gen-export-style.mjs`.

## Not done

- The `OutputLimit` of the Markdown exporter (2^28 units) is not driven by a test (half a gigabyte of output); it is
  a length comparison before each append. libFuzzer runs of `export_docx`/`compare_docx` (nightly, Linux CI); the
  stable driver ran 70 s each.
- `docxToMarkdown` / `docxToHtml` (the converter's copy of the exporters), and the compare element
  (`universal-write-diff.ts`, the shell's; it needs a document query through the binding).
- Parity of pairs above 3,000 blocks (the TS cannot run them); the Rust is covered there only by the unit tests of
  the limits.
- Benchmarks against `main` (gate 3).
- (Done 2026-11-05: clippy and rustfmt re-run clean on the tests and the files touched.)

## Process metrics

| Item | Value |
|---|---|
| Model / mode | Sonnet, hybrid mode, faithful 1:1 port (per the plan); the Opus orchestrator made the clippy-only rewrites of 2026-11-05 and this record was written afterwards from the code and the test runs |
| Wall time | not recorded (the session was interrupted before the record) |
| Tool calls | not recorded |
| Parity rounds Rust vs TS | not recorded; the facets are at 0 mismatches now (the numbers above are from the 2026-11-05 run) |
| Rework | clippy-only rewrites of 2026-11-05 (above); nothing else recorded. **Verification pass 2026-11-05 (memory, no-panic, limits, fuzz)**: two src fixes in `compare` (`max_d_for` overflow with an extreme trace limit; the histogram work budget did not charge the comparison that ends a run, see "Security (continuous)"); neither changes a parity result |
