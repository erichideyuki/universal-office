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
`collapse_blank_lines`, `repeat_n`. Re-running clippy for this record: not done.

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

## Memory, work and security

- **Memory gate**: **no M1/M3/M5 test exists for export or compare**: no `memory_*` or `nopanic_*` test and no
  fuzz target (`core/fuzz/fuzz_targets`) calls `to_html`, `to_markdown` or `diff_*` (checked by listing those
  files; the only callers under `tests/` are `parity_export.rs` and `parity_compare.rs`). The plan's D5 soak
  ("1,000 × read → html → markdown → odt → read_odt → diff", `uo-docx.md`, memory table) has not been run.
  Not measured.
- **Work bounds**: by construction (items 5 and D5a-8) and by the unit tests `limits_answer_none`,
  `coarse_is_a_valid_script` (myers), `repetitive_input_finishes_within_the_budget`, `exhausted_budget_gives_the_coarse_script`,
  `deep_recursion_is_iterative` (histogram). **There is no test that drives `diff_blocks_with_limits` with small
  limits and checks `DocDiff::limited` end to end**: `tests/parity_compare.rs` says that "the limits have their own
  tests (`tests/compare_limits.rs`)", and **that file does not exist** (finding).
- **No panic**: the code uses `get`/`first`/`Result` paths and `saturating_*`; no panic test. Not measured.

## Checks

`tests/parity_export.rs` (5 tests), `tests/parity_compare.rs` (5 tests), the two regex tests, the unit tests of
the modules (16, all pass). Harness: `tests/d5a_support/mod.rs`. Generators: `js/quirk-d5a.mjs` (synthetic
documents and pairs). Recapture: `UO_TS_ROOT=<built checkout of main> node
core/crates/uo-docx/js/capture-export.mjs` and `.../capture-compare.mjs` (`--only <key prefix>`); the CSS is
regenerated by `tools/gen-export-style.mjs`.

## Not done

- Memory (M1/M3/M5), no-panic and fuzz coverage of export and compare; the D5 soak.
- `tests/compare_limits.rs` (referred to by `parity_compare.rs`) and an end-to-end test of `DocDiff::limited`.
- `docxToMarkdown` / `docxToHtml` (the converter's copy of the exporters), and the compare element
  (`universal-write-diff.ts`, the shell's; it needs a document query through the binding).
- Parity of pairs above 3,000 blocks (the TS cannot run them); the Rust is covered there only by the unit tests of
  the limits.
- Benchmarks against `main` (gate 3).
- Re-running clippy and rustfmt after the 2026-11-05 clippy-only rewrites for this record.

## Process metrics

| Item | Value |
|---|---|
| Model / mode | Sonnet, hybrid mode, faithful 1:1 port (per the plan); the Opus orchestrator made the clippy-only rewrites of 2026-11-05 and this record was written afterwards from the code and the test runs |
| Wall time | not recorded (the session was interrupted before the record) |
| Tool calls | not recorded |
| Parity rounds Rust vs TS | not recorded; the facets are at 0 mismatches now (the numbers above are from the 2026-11-05 run) |
| Rework | clippy-only rewrites of 2026-11-05 (above); nothing else recorded |
