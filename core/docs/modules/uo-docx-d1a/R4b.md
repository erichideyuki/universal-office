# uo-docx D1a reader — frontier R4b: declarations

Part of the record `core/docs/modules/uo-docx.md` ("Milestone 1 — D1a foundation", brief §B). One
table row per site; every row names the TS file:line. Item 9 rows need an Opus check before they land.

Scope: `src/read/inline_objects.rs` ← `document.ts` 185–1583 outside R4a's mapping functions
(`vmlFixed`, `vmlImageEffects`, `blipFxImageEffects`, `blipImageEffects`, `readPic`, `bodyPrOf`,
`textboxBlocks`, `readGraphicObject`, `dmlRunColor`, `dmlParagraphs`, `readDiagram`,
`readDrawingRun`, `syncImageFromXml`, `syncShapeFromXml`, `syncGroupFromXml`, `readVmlRuns`,
`textContentOf`), and `src/read/picture_format.rs` ← `picture-xml.ts` 100–556, read side
(`readDmlColor`, `dmlColorSpec`, `partsOf`, `readPos`, `readPicLine`, `readShadow`, `readEffects`,
`readRot`, `read3D`, `readBlip`, `readPictureFormat`, `int`, `bool`). Math and legacy form fields
are R4c's in the brief's table (`readSingleRun`, `readMathNodes`, 1583+), not here.

Public API (`read::inline_objects`): `ObjectHost` (the TS `ReadContext` members the readers use:
`ids`, `resolve_image`, `add_media`, `resolve_part`, `enter_nested`/`leave_nested`,
`read_textbox_content` → `TextboxRead::{Blocks { story, txbx_src }, Paragraphs}`), `ResolvedImage`,
`read_object_runs(el, host)` (the `w:drawing` / `w:pict`+`w:object` / `mc:AlternateContent` cases
of `readSingleRun`), `read_drawing_run(drawing, raw, host)`, `read_vml_runs(container, inline, host)`
(all `Result<_, DocxError>`, objects as `RunExt`: the TS `Omit<Run, 'props'>`, `text` is U+FFFC),
`sync_image_from_xml`, `sync_shape_from_xml`, `blip_fx_image_effects`, `with_context::{read_drawing_run,
read_vml_runs}` (the exact signatures R4c's stub `object` module has, over `ReadContext`; `impl
ObjectHost for ReadContext`), and the string helpers behind the regex rows. `read::picture_format`:
`read_picture_format(raw, any_shape)`, `parse_drawing`, `parts_of`, `read_dml_color(_element)`,
`dml_color_spec`, `read_pic_line`, `read_effects`, `read_3d`, `read_blip`, `pic_int`, `pic_bool`.
Reused, not duplicated: R3's `read_sp_pr`, `read_wp_anchor`, `anchor_child`, `srgb_of`,
`parse_vml_style`, `vml_anchor`, `vml_len_twips`, `vml_shape_style`, `vml_single_textbox`,
`vml_textbox_to_drawing`, `strip_quotes`, `val_int_group`; R4a's `run_props_from_rpr`,
`para_props_from_ppr`; `plain::plain_paragraphs`; `crate::word::num::{js_to_radix, pad_start_zero}`
(layout's V8 `toString(radix)`); `uo_text::case::to_uppercase_str`. No model type was changed.

## 1. Numeric parse and rounding sites (uo-jsnum function used)

`pint` = `parse::parse_int(v ?? d, 10)`; `or0` = `x || 0` (`conv::truthy`); `fin` = `is_finite`;
`EMU` = 635. Stored numbers are `JsNumber::Num`; there is no float → integer cast in the two files.

| TS file:line | Site | Rust |
|---|---|---|
| document.ts:187-188 | `vmlFixed`: `parseInt(v,10)/65536` or `parseFloat(v)`, `Number.isFinite` | `parse_int`/`parse_float`, `is_finite` |
| document.ts:195-200 | VML `gain` (`!== 1`), `blacklevel` (`2*bl`, truthy) | same expressions |
| document.ts:209-216 | `blipFxImageEffects`: `Math.max(-1, Math.min(0.999, (contrast ?? 0)/100000))`, `1/(1-c)` / `1+c`, `br = (bright ?? 0)/100000`, `biLevel/100000` | `math::max/min`, same operations; `JsNumber::Null` reads as 0 (`null ?? 0`) |
| document.ts:222 | `blipImageEffects` `int`: `parseInt(v ?? d, 10) \|\| 0` | `pint` + `or0` |
| document.ts:944 | `readPic` crop `(parseInt(src.attrs[k] ?? '0',10) \|\| 0)/100000` | `or0(pint)/1e5` |
| document.ts:961 | `bodyPrOf` `Math.round(parseInt(v,10)/EMU)` (insets) | `math::round(parse_int / 635.0)` |
| document.ts:1014-1015 | `parseInt(txbx.attrs.id ?? '',10)`, `Number.isFinite` → `story` | `parse_int`, `is_finite` |
| document.ts:1019 | `linkedTxbx` `id \|\| 0`, `seq \|\| 1` | `or0`; `truthy ? x : 1.0` |
| document.ts:1031, 1036 | tile `tx ty sx sy` `parseInt(.. ?? String(d)) \|\| 0`; `alphaModFix amt` `(.. \|\| 0)/100000` | `or0(pint(v, "<d>"))` |
| document.ts:1058-1073 | group `n()` = `parseInt \|\| 0`; `cw = n(chExt.cx) \|\| n(ext.cx)`, `sx = cw ? (wTw*EMU)/cw : 1`; item `x y w h` = `((off - chOff)*s)/EMU`, `(ext*s)/EMU` | same expressions in the same order |
| document.ts:1082 | group `rot = parseInt(..)`, truthy → `rot/60000` | `pint`, `truthy` |
| document.ts:1102 | scheme tint/shade: `Math.round(((parseInt(val ?? '100000') \|\| 0)/100000)*255).toString(16).padStart(2,'0').toUpperCase()` | `or0(pint)`, `math::round`, `word::num::js_to_radix(x,16)` (exact V8 for negatives, ±Infinity, ≥ 2^53), `pad_start_zero`, `to_uppercase_str` |
| document.ts:1118 | `Math.round((parseInt(sz ?? '1800') \|\| 1800)/50)` | `pint`, `truthy ? x : 1800`, `math::round` |
| document.ts:1135, 1153, 1158, 1163, 1169 | `num`, `Math.max(mark.sz ?? 36, ...runs.map(r => r.rpr?.sz ?? 0))/2`, `gap` (`Math.round(num/5)`, `Math.round((num/100000)*1.2*szPt*20)`), `emu`, `line` (`Math.round(num/5)`, `Math.round((240*num)/100000)`) | `or0(pint)`, `math::max_n`, `math::round`, same operation order |
| document.ts:1203-1204 | `presStyleIdx` `\|\| 0`; `cs[i % cs.length]` / `cs[Math.min(i, cs.length-1)]` | `or0(pint)`; `math::rem`/`math::min` and `conv::js_index` (a negative, ±Infinity or NaN index is the TS `undefined`: item 7) |
| document.ts:1207, 1232 | SmartArt `n()` `(parseInt \|\| 0)/EMU`; `rot` `parseInt` truthy → `/60000` | same |
| document.ts:1247-1248 | `wTw = parseInt(cx ?? '0')/EMU` (no `\|\| 0`: NaN and ±Infinity stay) | `pint / 635.0` |
| document.ts:1264, 1271-1276 | `effectExtent` `(parseInt \|\| 0)/EMU`; group `it.x += e('l')`, `width += e('l') + e('r')` | same grouping of the sums |
| document.ts:1398-1399 | `(parseFloat(st.width ?? '0') \|\| 0) * sx` | `or0(parse_float) * sx` |
| document.ts:1417 | `a !== undefined && a < 1`, `Math.max(0, a)` (WordArt alpha) | `JsNumber::to_number` (`null < 1` is true as in JS), `math::max` |
| document.ts:1425, 1449 | VML `rotation` `parseFloat`, crop `parseInt/65536` or `parseFloat` | `parse_float`, `parse_int` |
| document.ts:1484 | `1 + (parseInt(spid.replace(/\D/g,''),10) \|\| 0)` | `keep_ascii_digits`, `pint`, `or0` |
| document.ts:1503, 1513-1514 | `hrpct` `parseInt > 0`; `Math.abs(x2 - x1)` | `parse_int`, `math::abs` |
| document.ts:1541-1551 | `coordsize` `parseFloat \|\| 1000`, `coordorigin` `parseFloat \|\| 0`, `w / (cs[0] \|\| 1)`, `((parseFloat(left) \|\| 0) - co[0]) * sx` | same expressions |
| picture-xml.ts:121 | colour modifiers `parseInt(val ?? '0',10) \|\| 0` | `parse_int`, `truthy` |
| picture-xml.ts:277-282 | `int(v, d)` (`parseInt`, finite or `d`), `bool` | `pic_int`, `pic_bool` |
| picture-xml.ts:321-331, 333-352, 356-368, 370-387, 389-429, 431-467, 469-551 | every `int(...)` of `readPos`, `readPicLine`, `readShadow`, `readEffects`, `readRot`, `read3D`, `readBlip` (`amt` 100000, `thresh` 50000, `sat` 100000, `colorTemp` 6500) and `readPictureFormat` | `pic_int` with the same defaults |
| picture-xml.ts:503-504 | `/^val\s+(-?\d+)/` then `parseInt(m[1],10)` | `val_int_group`, `parse_int` |
| picture-xml.ts:136-160 | `dmlColorSpec`: `val.toUpperCase()`, `` `${n}=${v}` ``, `alpha = v/100000` | `to_uppercase_str`, `to_js_string`, same division |

## 2. Regex sites (template or matcher; dynamic ones named; row in `js/regex-sites/r4b.mjs`)

Differential: 10 rows × 91 inputs = 910 results, all equal to V8 (`tests/regex_r4b.rs`). No site builds
a pattern from data.

| TS file:line | TS regex | Rust | Row |
|---|---|---|---|
| document.ts:1137, 1283 | `/^(1\|true)$/.test(v ?? '')` | template 1 `is_one_of` | `r2.one_of` |
| document.ts:1253 | `/\/drawingml\/2006\/diagram$/.test(uri ?? '')` | `is_diagram_uri` (`ends_with`; `$` without `m` is the end of the text) | `r1.diagram_uri` |
| document.ts:1356 | `/<a:blipFill\b/.test(raw)` | template 2 `find_lit_then_boundary` (also in `templates.mjs`) | `r5.blipfill` |
| document.ts:1407 | `.replace(/^["']\|["']$/g, '')` then `.trim()` | R3's `watermark::strip_quotes`, `text::trim` | `r4.quotes` |
| document.ts:1418 | `/bold\|[6-9]00/.test(font-weight)` | `has_bold_weight` (substring and a byte window) | `r3.bold` |
| document.ts:1419 | `/italic/.test(font-style)` | `str::contains` | `r3.italic` |
| document.ts:1478 | `next.replace(/^#/, '')` | `strip_leading_hash` | `r4.hash` |
| document.ts:1484 | `.replace(/\D/g, '')` | template 4 `keep_ascii_digits` | `r4.digits` |
| document.ts:1489 | `el.name.replace(/^v:/, '')` | `strip_vml_prefix` | `r4.vtag` |
| picture-xml.ts:503 | `/^val\s+(-?\d+)/.exec(fmla)` | R3's `val_int_group` | `r6.val_int` |

`.split(',')` (VML insets, `from`/`to`, `coordsize`, `coordorigin`) is not a regex (`str::split`,
the same pieces, including the single empty piece of `''`). The writer-side regexes of `XmlPatch`
(picture-xml.ts:36-100, 744-745) are D1b/D2b's.

## 3. Fields that can store NaN/±Infinity, keys stored as `undefined` (cleared bits)

Every number is a `JsNumber` (F1). The corpus carries `NaN`, `1e999`, 20-digit and 400-digit strings.

| Field | How a non-finite value gets there |
|---|---|
| `InlineImage.widthTwips/heightTwips`, `DrawingShape.*`, `DrawingTextbox.*`, `DrawingGroup.*` | document.ts:1247-1248 has no `\|\| 0`: a bad `cx`/`cy` is NaN, 310 digits is ±Infinity; VML: `vmlLenTwips('.')` is NaN (R3), group `sx` with a NaN or ±Infinity extent |
| `GroupItem.x/y/w/h`, `DrawingGroup.rot`, `inlineEffect` | `cw`/`ch` ±Infinity; `parseInt` of 310 digits (`\|\| 0` removes NaN, not ±Infinity): 1058-1073, 1082, 1264 |
| `DrawingTextbox.insets.*`, `rot` | `Math.round(NaN)` = NaN (961); `% 360` of ±Infinity is NaN (1044); VML `rotation` `parseFloat` (truthy, so ±Infinity only) |
| `DrawingTextbox.link.id/seq`, `story` | ±Infinity (1019); `story` only finite (`isFinite`, 1015) |
| `InlineImage.crop.*` | `parseInt \|\| 0`: ±Infinity (944); VML `parseFloat('x')` is NaN and is stored when another side is truthy (1449) |
| `ImageEffects.gain/bright/biLevel` | ±Infinity from the 1/100000 scaling of a 310-digit `bright`/`thresh` (209-216); the VML values are finite (`isFinite`) |
| `RPr.sz/szCs`, `SpacingSpec.*`, `IndSpec.*` of SmartArt paragraphs | ±Infinity (`\|\| 1800` / `\|\| 0` remove NaN); `0 * Infinity` can make NaN in `gap` (1158) |
| `TextFill.alpha` (WordArt) | not stored when NaN (`a < 1` is false); finite otherwise |
| `ColorSpec.themeTint/themeShade` | the text of `toString(16)` of a rounded value: `"-5"`, `"Infinity"`→`"INFINITY"`, a long hex of 2^53 and more |
| `PictureFormat.*` numbers, `PicColor.mods` values, `adj` values | `int` returns a finite number or the default; `adj` and `mods` (`parseInt \|\| 0`) can be ±Infinity |

Keys stored as `undefined`: **none written by these two files.** Every optional key is set inside
an `if`; `...(cond ? { k } : {})` spreads are `put!` under a condition. The one `undefined` key of
a SmartArt paragraph is `RunProps.fontAscii` (`rFonts` with a theme latin font and no `ascii`),
which R4a's `run_props_from_rpr` writes with `mark_undefined`. No `mark_undefined` here.

## 4. Map/object iteration orders relied on (and key orders recorded with `mark`)

Maps: `lbls` and `txFill` of `readDiagram` (document.ts:1193, 1197-1205) are looked up by key only,
a later `set` of the same key wins (`HashMap::insert`). `walk` of the SmartArt tree visits
children in document order, a group's before the next sibling (an explicit stack of child
iterators).

`Object.keys(x).length` (document.ts:195-203, 214-218, picture-xml.ts:504-505) is "any key set"
(`has_keys`, `adj.is_empty()`). The `Set` of `syncShapeFromXml`/`syncGroupFromXml` (document.ts:1349-1356,
1384-1388) is `[...Object.keys(dst), ...Object.keys(src)]`: keys of `dst` in their order, then the
keys only `src` has; `delete` forgets a key, a new key goes last (`sync_keys` rebuilds the whole
recorded order).

Key orders (`mark` after **every** assignment, the Rust structs declare other orders):

| Object | Order of the TS assignments |
|---|---|
| `InlineImage` (DrawingML) | `data, ext, [svg], widthTwips, heightTwips, [effects], [crop], [style]` (927-952), then `[inlineEffect]`, `[floating, anchor, behindDoc]`, `[relH, relV, offsetXTwips, offsetYTwips]`, `[rawDrawingXml]`, `[format]` (1262-1306) |
| `InlineImage` (VML) | `data, ext, widthTwips, heightTwips, vml, [crop], [effects]`, then floating/anchor/behindDoc/rel*/offsets (1440-1458, 1525-1540); `ole` last (1573); `floating`, `anchor` deleted for `w:object` (`forget`: removed from the recorded order) |
| `DrawingTextbox` | DrawingML: `widthTwips, heightTwips`, `[story]` or `[link]`, the `bodyPr` keys in the TS order (`autoFit, noWrap, insets, vAnchor, vert, upright, wordArt`), `[fontColor], [shape], lineInset, [geom], [rot]` (1000-1050), then anchor/rel*/hidden/raw as above; VML text box: `widthTwips, heightTwips, vml, insets, [autoFit], [noWrap], [vert], [vAnchor], [vmlId], [vmlNext], [shape], [rawDrawingXml]`; WordArt: `widthTwips, heightTwips, vml, insets, vAnchor, wordArt, [rot]`; SmartArt: `widthTwips, heightTwips`, bodyPr keys, `[rot]` |
| `DrawingShape`, `DrawingGroup` | `style, widthTwips, heightTwips`, `[hr, hrPct, hrNoShade, hrAlign]`, `[line]`, floating/anchor/hidden/raw; group: `widthTwips, heightTwips, items, [rot], [flipH], [flipV]`, `[floating, anchor]` |
| `ImageEffects` | VML `gain, bright, gray, biLevel`; blip `gain, bright, floor, gray, biLevel` |
| `PicLine`, `PicShadow`, `PicEffects`, `PicBlipFx`, `A14Props`, `PictureFormat` | `readPicLine` (333-352) in child order; `readShadow` `kind, color, …`; `readEffects` and `readBlip` in child order; `PictureFormat`: the literal's 20 keys (`inline … blip`), then `posH, posV, relativeHeight, sizeRelH, sizeRelV, polygon, adj, custGeom, line, threeD, embed` |
| `RPr`, `PPr`, `RFonts` (SmartArt, WordArt) | `rFonts, sz, szCs, b, bCs, i, iCs, color, [u]`; `jc, spacing, ind, contextualSpacing, rPr`; `ascii, hAnsi \| asciiTheme, hAnsiTheme`, `eastAsia \| eastAsiaTheme`, `cs \| cstheme` |

Compared, not just declared: every record of the parity facet has a `<facet>.o` digest of the key-order
facet of the canonical stream, equal on all 3,712 records. Two key positions the model fixes by
design are normalised on the TS side of the capture (and of the unit cases): `props` of a group
item's run goes after `text` and before the object key (`Run` declares `text, props, rpr`, then
the rare fields), and `paras`/`blocks` of a text box go after the object's own keys (the model keeps the
content outside the keys; the TS walker already writes `@views` last).

## 5. Recursions (made iterative, or bounded by `max_nesting`)

| TS site | Recursion | Rust |
|---|---|---|
| document.ts:1056-1090 `readGraphicObject` → `readGraphicObject` | nested `wpg:grpSp` inside a group, bounded only by the XML depth (1,000) | **bounded**: each nested group child enters `enter_nested` (64); past it the child is skipped (item 9 row 4) |
| document.ts:974-993 `textboxBlocks` → the block reader → paragraphs → drawings → `textboxBlocks` | text box in a text box (through cells, fields…) | `enter_nested` around `read_textbox_content`; past 64 the content is `plain::plain_paragraphs` (G20) |
| document.ts:1213-1240 `walk(parent)` (SmartArt `dsp:grpSp`) | recursive | iterative (explicit stack of child iterators, same pre-order) |
| document.ts:843-849 `findDeep`, 1583-1587 `textContentOf`, picture-xml.ts:284-290 `findDeep` | recursive search / concatenation | `Element::descend`, `Element::text_content` (iterative, uo-opc) |
| document.ts:1362-1388 `syncGroupFromXml` → `readDrawingRun` | not recursive (one level: items are scaled, not re-read) | — |

Native stack: the deepest chain is 64 frames of `read_graphic_object` (tested at 300 nested groups
and 250 nested VML groups, `nopanic_r4b.rs`); everything else is a loop.

## 6. Module-level / process-global state and its per-read replacement

| TS site | State | Replacement |
|---|---|---|
| picture-xml.ts:353-354 `rawSrc` (set by `readPictureFormat`, read by `rawOf` in `readPicLine`, `readEffects`, `readBlip`) | module variable: the text the raw slices come from | the source of the re-parsed drawing: `Element::source_slice` (positions parse of the raw text in `parse_drawing`); outside `readPictureFormat` the TS has `rawSrc === ''` and `rawOf` gives `''`: a caller handing these functions elements of a parse **without** positions gets `''` too, as the TS |
| document.ts:876-892 `RPR_EL`, `OBJ_EL` symbol keys | per-run source tags | R4c's `RunSide` (not here) |

No counter or cache in the two files.

## 7. `try {…} catch {}` sites and the partial result kept

| TS site | Site | Rust |
|---|---|---|
| document.ts:1314 `syncImageFromXml` | `try { root = parseXml(raw) } catch { return }` | the parse error returns; the image is untouched |
| document.ts:1338 `syncShapeFromXml`, 1366 `syncGroupFromXml` | same | same |
| picture-xml.ts:471 `readPictureFormat` | `try { p = partsOf(raw) } catch { return undefined }` (parse error, or no `wp:inline`/`wp:anchor`: `throw new Error`) | `None` (`parse_drawing(raw)?`, `parts_of(&doc)?`) |

**Not caught in the TS, therefore `Err(DocxError)` here and not caught here either** (the exception
leaves `readParagraph`/`readFullDocument`; the parity facet records it as `!throw` on both sides: 49 cases of the
quirk corpus):

| TS site | Cause | Rust |
|---|---|---|
| document.ts:1204 `readDiagram` | `cs[i % cs.length]!` / `cs[Math.min(i, cs.length-1)]!` is `undefined` for a negative, ±Infinity (`Infinity % n` is NaN) `presStyleIdx`; `dmlRunColor({ children: [undefined] })` throws `TypeError` in `firstChild` | `DocxError::Corrupt("SmartArt colour list index out of range …")` |
| document.ts:1484 `readVmlRuns` | `vmlTextboxToDrawing` throws when a raw VML attribute value breaks the generated text (R3 row 7) | the `Err` of `vml_textbox_to_drawing` is propagated with `?` |
| document.ts:991 `textboxBlocks` (no block reader) / `resolvePart` | `readParagraph` and `parseXml` of a SmartArt part can throw | the host's `Err` is propagated (`read_textbox_content`, `resolve_part`) |

## 8. Aliasing (two model positions sharing one TS object) and how A2 gives the same result

| TS site | Sharing | A2 result |
|---|---|---|
| document.ts:1039-1041 | `tb.shape = style` and `tb.lineInset = style.lineInset ?? {}` (the same object); `tb.geom.adj = style.adj` (shared with `style.adj`) | `style.clone()` into `shape`, `lineInset` and `adj` cloned (values: the digest is the same; an in-place edit of one of them in D2b would not reach the other, as the TS would: check there) |
| document.ts:978-989, 1229-1234, 1421 | text box `paras` and `blocks` hold the same paragraph objects | one `Story` + `TextboxView` (F10); `Synced`, `Phantom` (no paragraph: the TS builds a fresh empty paragraph outside `blocks`), `ParasOnly` (no block reader) |
| document.ts:1078, 1223, 1234, 1553 | group item `run: { ...run, props: {...DEFAULT_RUN_PROPS} }`: a new object that shares the object (`image`/`textbox`/…) of `run`, which is dropped | `Run` built around the moved `RunExt`: no sharing left |
| document.ts:1115, 1131 | `refColor` (one `ColorSpec`) is the `color` of every `rpr` of the SmartArt shape | `Box<ColorSpec>` cloned per `rpr`: same digest; shared edits are D2b's (SmartArt text is not edited by the session) |
| document.ts:1423-1432 | WordArt: `wppr` is both `para.ppr` and the input of `paraPropsFromPPr`; `rpr` is `run.rpr` and the input of `runPropsFromRPr` | `Arc<PPr>`/`Arc<RPr>` + the derived props (R4a returns a new value, as the TS) |
| document.ts:1297-1305 | `t.rawDrawingXml = drawing.raw` (strings) | `Str` |
| document.ts:933-934 | `media.data` of `resolveImage` is one `Uint8Array` shared by image, svg, fill and OLE | `MediaRef` by content (G24, `MediaStore::add` dedupes) |
| document.ts:1311-1330 | `dst[k] = src[k]` shares `anchor`, `style`, `format`… between the old image and the fresh one (the fresh one is dropped) | `assign_slot` clones |

## 9. Saturating/wrapping/truncating choices and data-shape deviations from A2

Needs an Opus check before landing (rows 1, 3, 4, 5 especially). No change of arithmetic semantics.

| # | Choice | Why / effect |
|---|---|---|
| 1 | Object runs are returned as `RunExt` (`Result<Option<RunExt>>`, `Vec<RunExt>`), not `Run`; `text` is implied (U+FFFC) | the TS return type is `Omit<Run, 'props'>`; `with_context` wraps each as a `Run { text, ext }` for R4c's `push_object` (which takes `text` and `ext`) |
| 2 | The TS `ReadContext` members the readers use are the trait `ObjectHost` (`resolve_image`, `resolve_part`, `add_media`, `ids`, `enter_nested`/`leave_nested`, `read_textbox_content`); the block reader of `readBlocks`/`readBlocksGrouped` + `sources.addTextboxGroups` is one host call returning `TextboxRead::Blocks { story, txbx_src }`; no stored closure. `impl ObjectHost for ReadContext` reads the `w:p` children when `BlocksMode::None` (the TS `readParagraph` path) and returns `DocxError::Corrupt` for `Blocks`/`Grouped` **until R5 wires `fulldoc::read_blocks_of`** (one match arm in `inline_objects.rs`, `read_textbox_content`) | the brief: callbacks become methods; R5's reader does not exist yet |
| 3 | The two uncaught TS exceptions are `Err` values (item 7); SmartArt's is `DocxError::Corrupt` with a static text | LQ2: errors are values; the variant is what parity compares |
| 4 | **Nested groups are bounded by `max_nesting`** (64): a `wpg:grpSp` child at the 65th nested level is skipped, and a group left without items reads as `undefined` (the TS rule for an empty group), so a chain with nothing beside its last link vanishes completely. The TS recurses to the XML depth (1,000) | item 5 / G20: the native stack must not depend on the file; a real group nests 2-3 levels. The text boxes inside groups follow G20 (plain paragraphs past the limit, row 5) |
| 5 | Text box content past `max_nesting` is `plain::plain_paragraphs` (view `Synced`/`Phantom` by the paragraphs it gives) | G20, F17; the TS has no limit |
| 6 | **`sync_shape_from_xml` of a different kind leaves the object alone**: a text box whose raw XML now reads as a shape (or the reverse, or a group): the TS copies the keys of one kind into an object of the other (a hybrid); the model cannot hold it. Also, the sync's internal context has no block reader and does not read the text box paragraphs the TS reads and discards (`keep` has `paras`/`blocks`): the only observable difference would be a `readParagraph` that throws (a nested VML box with a broken attribute) | named; no D2b operation produces a kind change from the editor's own raw edits |
| 7 | `dml_color_spec` (TS `dmlColorSpec`): a preset colour named like an `Object.prototype` member (`constructor`, `toString`, `__proto__`…) **without a modifier** is, in the TS, that member itself (a function, or the prototype object) because the string concatenation happens only when a modifier exists; Rust returns `String(member)`. With a modifier, and for scheme colours (`` `@${…}` ``), the strings are equal. The twelve names are the same table as R3's `proto_string` (copied: R3's is private) | named, pinned: the parity facet `spec` compares the TS value as an empty object and the Rust text, so any other difference fails |
| 8 | `ColorSpec`/`PicColor` text: `toUpperCase` is `uo_text::case::to_uppercase_str` (JS-faithful, F26 refined for these two files); `m.name.slice(2)` of an element name that starts with a non-BMP character would leave a lone surrogate, `Str` holds U+FFFD (the name of a colour modifier is `a:xxx` in every file) | named, unmeasured |
| 9 | `ResolvedImage.path` (TS `{ data, ext, path }`) is not carried: no reader here uses it | none |
| 10 | The mixed-view `TextboxView::ParasOnly` is produced for `BlocksMode::None` (comments, sync); `Synced` otherwise | F10 |
| 11 | No saturating, wrapping or truncating conversion in the two files: the only integer conversion is the length of a SmartArt colour list (`u32::try_from(len)`, bounded by the XML element limit) into the `f64` of `i % cs.length`; array positions are decided by `conv::js_index` | — |
| 12 | `read_picture_format`/`parts_of` take the raw text (and re-parse it with positions) as the TS does; `Parts` borrows the parsed `XmlDocument` (`parse_drawing` returns it) | the TS `Parts` holds the element tree of its own parse |
| 13 | A group item's run has `props = default_run_props()` (the TS `{ ...DEFAULT_RUN_PROPS }`); the drawing ids (`DrawingId`) are given when the object is created, group items before their group | ids are not observable (relabelled) |

## Parity and memory results

**Parity** (facet `drawing-runs`, `tests/parity_drawing_runs.rs`, capture `tests/parity/drawing-runs.capture`
by `js/capture-drawing-runs.mjs`, Node 24.15 x64, frozen TS `d0f45bc5`): every `w:drawing`, `w:pict`,
`w:object` and `mc:AlternateContent` of the `word/*.xml` parts of the 2,767 git-tracked docx (1,268
targets; 608 pictures, 553 text boxes, 81 shapes, 24 groups read) and of the seeded quirk corpus
`js/quirk-objects.mjs` (`tests/parity/drawing-runs.quirks.txt`: 2,214 cases, 2,444 targets: DrawingML
inline/anchor pictures with every blip effect, crop, SVG, layer, tile, `srcRect`, effect, 3D and
`a:ln` form; text boxes, linked boxes, `bodyPr`, `wps:style` font colours, picture fills; shapes,
groups three levels deep, canvases; SmartArt with its three parts, every colour kind, cycle/repeat
and negative/huge indices; VML shapes, groups, images with crop/gain/blacklevel, WordArt, text boxes
(single ones through `vmlTextboxToDrawing`, with injected attribute text), lines, rules, OLE
objects; `mc:AlternateContent` with and without a usable drawing; prototype-named colours), 3,712
records: facets `runs` (`[{ text, image?, textbox?, shape?, group? }]`), `fmt`, `fmtany`
(`readPictureFormat`), `spec` (`dmlColorSpec` of every colour in the drawing) and `sync` (nine edits of
the raw text through `syncImageFromXml`/`syncShapeFromXml`/`syncGroupFromXml`), each with its key-order
digest, plus the target counts. **29,877 comparisons: 29,877 equal, 0 mismatches, 0 accepted, 0 not
replayed** (the main digests of the first full replay were equal except for one bug found by the
corpus: `linkedTxbx seq` is `parseInt(..) || 1`, not `|| 0`; the key-order digests differed only by the two
model-fixed positions of item 4, normalised on the TS side). The 49 raising cases (SmartArt indices,
VML text boxes whose attribute text breaks the generated XML) are `!throw` on both sides. Sensitivity
by mutation (a wrong `effectExtent` key, a wrong default scale, a missing `abs`, `\|\|` for `&&`
in `blipFxImageEffects`): 64, 114, 5 and 72 mismatches.

**Unit tests** (`tests/inline_objects_unit.rs`, 10; `tests/inline_objects_unit.cases`, 58 cases by
`js/unit-cases-r4b.mjs`, expected values are TS `JSON.stringify` output with the key order): every kind
of object (pictures, text boxes, shapes, groups, SmartArt, VML shapes/groups/images/WordArt/lines/rules/OLE,
alternate content), `readPictureFormat` (`anyShape`, bad input), `blipFxImageEffects`, the eight sync
cases, the SmartArt and VML exceptions; and without a TS counterpart: the G20 fallback, the empty text
box `Phantom` view, the bound of nested groups (65 levels of 200, 3 of 5 with `max_nesting` 2, the guard
balanced), raw text given by the caller, sync of another kind or without raw, the dispatch of other
elements, the picture-xml pieces, `dml_color_spec`, and the host of the document reads
(`ReadContext`, `BlocksMode::None` paragraphs, `Blocks` an error, nesting balanced).

**Regex** (`tests/regex_r4b.rs`): 10 rows × 91 inputs = 910 results equal to V8.

**Memory M1** (`tests/memory_r4b.rs`): `read_object_runs`, `read_drawing_run`, `read_vml_runs`,
`read_picture_format`, `parse_drawing`, `parts_of`, the sync functions, the JSON of the results and the
picture-xml readers over every target and every element of the quirk corpus (error paths included):
live bytes back to the start, 0 B, host and media included. M2 soak and M3 budgets are the memory verifier's.

**No panic** (`tests/nopanic_r4b.rs`): the corpus, 6,000 seeded mutations of it, 3,000 documents with
hostile numeric attributes (`1e999`, 29 digits, `NaN`, `-0`, `0x10`, empty, `12pt`) in pictures, text boxes,
groups and VML, 300 nested DrawingML groups and 250 nested VML groups, 30,000 random strings through the
helpers; every element of every parse is offered to every reader; the invariant checked: a sync of an
edited raw text keeps the object's kind.

Checks: `rustfmt --check` (my files), `cargo clippy -p uo-docx --all-targets --all-features -- -D warnings`
native and `--target wasm32-unknown-unknown` (clean for my files; the crate as a whole failed at times
on other frontiers' files in progress), `core/tests/float-casts.sh`, `core/tests/grep-bans.sh`,
`cargo test -p uo-docx --all-features` green.

## Process metrics (wall time, rounds with the verifiers, rework)

Sonnet 5.5, one session. Port 1:1 first compile; parity rounds with the verifier: 0 (one bug found by
the corpus on the first replay, one named difference found when the `spec` facet was added). Rework:
corpus tuning (more declared shapes and SmartArt, hand-written OLE and SmartArt cases), the `drawing-runs`
capture normalisation of the two model-fixed key positions. Tool calls: see the uo-docx record,
Process metrics.
