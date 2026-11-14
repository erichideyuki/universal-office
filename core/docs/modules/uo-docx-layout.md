# uo-docx-layout — the Write layout engine (pages, lines, tables, floats, notes, fields, display list)

Record of the rust-port workflow. Status: **milestone-1 gate approved 2026-11-03** (hybrid
mode, `SKILL.md` "Hybrid mode"; every role of this module runs on Opus, implementation and verifiers
included — "Models"). Planner and reuse analysis together (one Opus agent, 2026-11-03), then the
analysis skeptic (K1–K20, section "Analysis skeptic", kept as written), then this revision: every
change it caused is marked *(revised after the skeptic, Kn)*, and the milestone-1 questions are
consolidated into one list (G15, G19, G21, LQ1–LQ22). Under the hybrid mode there is no per-frontier brief: the
architecture below fixes the shapes (inputs, identity keys, pass state, output, caches, the painter
and PDF boundaries, the editor API), the Opus frontiers port 1:1 inside it, and every decision
outside the source semantics is declared in this record (section "Stage 3", per frontier, with the
nine-item list of uo-docx A7).

Line counts are non-test source lines on `main` 0.9.x (frozen), counted in this worktree.
Milestone 1 is the read path: uo-docx D1a → **uo-docx-layout** → uo-render → uo-pdf (viewer and
server PDF). This module is its long pole (≈ 12.6k lines of behaviour, all Opus).

## Scope

### TS covered (behaviour source, `write/packages/docx-layout/src`)

| TS files | Lines | Rust module (crate `uo-docx-layout` unless noted) |
|---|---|---|
| `engine.ts` (4,183: `layoutDocument` pass loop + column balancing search, `class Engine` with ≈ 90 methods), `chain.ts` (91), `frames.ts` (130) | 4,404 | `engine/` (`mod.rs` pass loop and balancing; `page.rs` geometry, sections, columns, page borders, line numbers; `flow.rs` `placeParagraph`, `layoutStatic`, keep rules, contextual spacing, border groups; `tables.rs` `placeTable`, floating and late tables, row emission; `floats.rs` `placeFloats`, `wrapFrame`, `polyBandIntervals`, objects, textbox chains and frames; `notes.rs` footnotes/endnotes, separators, note columns; `hf.rs` headers/footers; `fields.rs` SEQ/REF/NOTEREF/STYLEREF/PAGEREF/SECTIONPAGES/chapter numbers); `derive.rs` (frames lifted, chain slices) |
| `para.ts` (1,178), `hyphenate.ts` (254), `math.ts` (327) | 1,759 | `para.rs` (`prepareParagraph`, `resolveRunStyle`, glyph fallback order, the 600-DPI width model `advanceTwips`/`wordWidth`/`scaledUnits`/`toWordUnit`), `hyphenate.rs` (Word's choice over `uo_fonts::Hyphenator`), `math.rs` (OMML layout + its own font reader `parseFont`, uo-fonts S14) |
| `lines.ts` (689), `underline.ts` (184), `paint.ts` (504) | 1,377 | `lines.rs` (`LineBreaker`; seed: `experiments/rust-spike/src/linebreak.rs`, 0 mismatches on 13,353 lines), `underline.rs`, `paint.rs` (lines, boxes and borders → display-list items) |
| `table.ts` | 1,406 | `table.rs` (grid widths, row layout, row split, vertical merges, nested rows) |
| `fonts.ts` (355), `context.ts` (374); *(revised after the skeptic, K7)* `typography/font/sfnt.ts` twips scaling `kernTwips`/`kernGlyphsTwips` (sfnt.ts:198–213, live in para.ts:539–543) | 729 + ≈ 20 | `fonts.rs` (`FaceResolver` Word substitution chain, the deviceEmPt kerning scale, `faceMetrics` DirectWrite vertical metrics + the East Asian 1.3× rule, `VERT_TABLE`, superscript/subscript, `parsePostScriptName`, `withEmbeddedFonts` precedence), `context.rs` (`DocContext`, flow state `saveFlow`/`restoreFlow`/`pure`, note marks) |
| `resolve.ts` (414), `paraformat.ts` (31), `numbering.ts` (236), `datefmt.ts` (103), `field-formula.ts` (129), `outline.ts` (185), `numpages.ts` (30), `revisions.ts` (178); `context.ts` `directRPr`/`directPPr` and `para.ts` `resolvePPr` | 1,306 | **Word semantics shared with the session** — placement is question **LQ3** (recommended: `uo_docx::word`, ported by these frontiers) |
| `geometry.ts` (layout half ≈ 380: `presetPathLocal`, `presetPath`, `mapPath`, `pathBounds`, `custPaths`, `withThemeStyles`, `shapeExtras`, `groupChild`), `picture.ts` `pictureLook` (≈ 75), `page-fill.ts` `resolvePageFill` + `patternRowsFromImage` (≈ 80), `textfx.ts` `resolveTextFx`/`fxColor`/`gradOf`/`fxMarginPt` (≈ 80) | ≈ 615 | `drawing.rs` (shapes, groups, pictures, page fill, text effects resolved into display-list values) |
| `emf.ts` (633), `wmf.ts` (148) | 781 | `metafile.rs` (EMF/WMF records → display-list items; needs only a face lookup — LQ7) |
| `refs.ts` (41), `run-boxes.ts` (85); editor `engine-doc.ts` `caretXIn`, `offsetAtX`, `locateRef`, `isSortedByX` (≈ 60) | ≈ 190 | `query.rs` (refs, run boxes, hit testing, caret geometry — the editor-facing API) |
| `tree.ts` (275) | 275 | the item vocabulary goes to **uo-render** (types + encoding); the Write-only geometry (`LaidOutLine`, `PlacedObject`, `PlacedCell`, `ObjectFrame`, `LineParaFormat`) stays here (`output.rs`) |
| `typography/lineheight.ts` `deviceEmPt` | 5 | `para.rs` |

Total ported here *(revised after the skeptic, K20)* ≈ **12,570 lines** without `tree.ts` (12,847
with it) (≈ 11,260 if LQ3 places the Word-semantics files in uo-docx; they are ported by the same
frontiers either way). Plus, outside this crate but on its critical path (K6, K10): the uo-fonts
`registry`/`system` modules and the provider policies, and the uo-text UTF-16 tables.

### C# (type guide only)

`dotnet/src/UniversalOffice.Docx/DocxLayout/**` (18,797 lines; ≈ 15.9k excluding `Pdf.cs`,
`PdfPicture.cs`, `PdfNavigation.cs`, `PdfWordSpaces.cs`, `Images.cs`): a port of the TS, useful
where the TS has unions, `any` and optional fields (e.g. `Tree.cs` item records, `Para.cs` `Glyph`).
*(revised after the skeptic, K6)* One policy exists only in C#: `SubstituteFontProvider.cs` (system
face, else a `FontCatalog.MetricSubstitute` stand-in, a fresh object per call), which the
clickCompliance server PDF uses; it moves to Rust as the `server-substitute` registry policy with
**C# as its reference** (LQ8). `SystemFontProvider.cs`/`FontCatalog.cs` are host font discovery
(LQ8). The .NET `DocxPdf` API also takes a host `IFontProvider` and `List<PdfFontSource>`
(Export/DocxPdf.cs:24–39) — dropped or wrapped by the facade (CHANGELOG). `LayoutDump.cs` is the 0.01-rounded dump of `tools/gen-parity-layout.mjs` (JS × .NET parity) — the
seed of the canonical walker below, not a reference. The C# is not a parity reference (TS is);
differences found are reported, not fixed (frozen).

### Left for other modules

| Part | Goes to | Why |
|---|---|---|
| `pdf.ts` (540), `pdf-picture.ts` (326), `pdf-word-spaces.ts` (41), `pdf-writer/*` (840); `pdf-navigation.ts` (46) | **uo-pdf** (navigation: computed here as a neutral `Navigation` value, written by uo-pdf) | PDF painter and writer |
| item types and their binary encoding (`tree.ts`), and the painter-side helpers both painters need: `geometry.ts` fills (`gradientSamples`, `gradientGeometry`, `patternRaster`, `blipTiles`, `isRectGradient`, `rectGradientRaster` ≈ 140), `page-fill.ts` tiles (≈ 140), `textfx.ts` `fxLayers`/`runBox`/`runOutlineSegs`/`dashPattern` (≈ 135) | **uo-render** (`types`, `codec`, `paint`) | the contract with the TS painter and with uo-pdf; app-neutral (uo-drawingml G6 already sends "fills" there) |
| image codecs (`images.ts` `decodePng/Gif/Bmp/Tiff`, `jpegInfo`, `imagePixelSize`) | **uo-image** (uo-drawingml G5) | codecs |
| blip effects (`images.ts` `applyImageEffects`, `applyPictureAdjust`, `effectDecode`; `picture.ts` `applyBlipFx`) | **uo-drawingml** `blip` (G4 there) | done there |
| font files, kerning, ligatures, Liang, subset, embedding plans | **uo-fonts** (gate 2 passed) | used as is |
| the host font registry and the per-document face budget (uo-fonts gate-2 D4) | **uo-fonts** `registry` (new, designed here — LQ8) | app-neutral; one face cache per wasm instance serves Write, Slides and Sheet |
| *(revised after the skeptic, K6)* the four `provider.face` policies: viewer `fonts.ts:90–137` (variant fallback order, non-embeddable licence → metric substitute, `standIn`, `source === 'default'` → `null`), editor `liveFontProvider` (16087, cache by `fonts.version`), C# `SubstituteFontProvider.cs` (clickCompliance), `node-fonts.mjs:162–181` (score pick, the oracle) | **uo-fonts `registry` named policies** `viewer`, `server-substitute`, `oracle` (test-only); `editor` in milestone 2. Ported as a Sonnet-eligible closed-scope frontier P (LQ8) | each with its own parity facet: TS viewer provider over a stub resolver; C# provider over a fixed index (C# is the reference, frozen); node-fonts over the oracle machine |
| *(revised after the skeptic, K7)* the `SfntFont` advance tables (`setAdvanceTable`, sfnt.ts:386–399; loaded by viewer fonts.ts:66–67 and editor 1822, 1866) | **uo-pdf** plan | they never reach the engine's width model (para.ts:378–405 uses `advanceWidth`); they drive the PDF word-space insertion (pdf-word-spaces.ts:24): the registry attaches them per face, or they are dropped with the PDF text-extraction change accepted by name — decided there |
| *(revised after the skeptic, K8, LQ15)* the editor's `fontUsage()` | **uo-docx session** (literal port) | model-only; leaves the `LayoutOracle` |
| *(revised after the skeptic, K5, LQ16)* `documentHyphenationLanguages` walk | **uo-docx** (`word`), explicit list of every `lang` site, own facet | its result is a binding input (`hyphenation_loaded`) |
| the model, `DocView`, `DocRef`/`StoryRef`/`BlockPath`/`RunRef`, embedded-font reading | **uo-docx** (A2, A4, D1a) | |

### Stays in the TS shell

`canvas.ts` (601) becomes a decoder/painter of the uo-render display list; `canvas-picture.ts`
(391) stays as the canvas side of the picture/shape effect pipelines (raster work through
`uo-render::paint` exports, LQ6); `inline-renderer.ts` (89, `paintCustomInlines`: plugin renderers
draw over the fallback-text box from `run_boxes`; *(revised after the skeptic, K17e)* it reads
`run0.face.font` and hands the model `RunProps` to plugins, inline-renderer.ts:63–80, so `run_boxes`
carries those props and a `FaceId`, and `measure_text` is exported); `FontFace`/Local Font Access/`fetch` of font
bytes (they feed the registry); the text layer, scrolling, zoom; the editor's input handling until
milestone 2.

### Not ported (G19, LQ2)

`typography/linebreak.ts` (105), `richbreak.ts` (282), `paginate.ts` (89), `lineheight.ts`
`singleLineHeightPt`/`lineHeightPt` (≈ 30), `units.ts` (3): the **legacy canvas layout** used only by
`universal-write.ts` for the web/draft/outline views, the XPS export (`withLegacyLayout`,
universal-write.ts:15996, 22642) and the attribute `layout="legacy"` (15977). Header/footer and
textbox editing already run on the engine (`engineCanRun`, 15988: only `print`/`read` views gate it;
`layoutHfEdit`/`layoutTextboxEdit` run only when the engine cannot). The lab-only options
`onWordFit`, non-default `advanceEm`/`advanceRound`/`justifyShrink` (LQ12).

*(revised after the skeptic, K8)* Dropping the legacy layout also removes: `buildSpans`
(universal-write.ts:10893–10918, the only caller of `InlineRenderer.measure`, used by the legacy
layout, `fontUsage()` and `effectiveTbHeight` 12841/13399); the **engine-failure fallbacks** — canvas
`engineFailed` (1328, 16161) and `exportPdf` → `exportPdfCanvas` (22365–22372). After the switch a
`LayoutError` means no page and no PDF. `fontUsage()` (2319–2335: the save-time embedding plan, 2302,
and `settleDocumentFonts` before a PDF, 3336–3341) needs no layout and moves to the uo-docx session
as a literal port (LQ15). The comment at 15971 ("HF/textbox editing stay legacy") is stale.

## Plan (milestone-1 gate — pending)

### 1. Boundaries and dependencies

```
uo-jsnum ─┬─ uo-opc ── uo-docx (model, DocView, word semantics [LQ3]) ──┐
          ├─ uo-fonts (+ registry [LQ8]) ───────────────────────────────┼── uo-docx-layout ──► Layout
          ├─ uo-drawingml (presets, colour mods, blip, raster masks) ───┤        │
          └─ uo-text (NFC, \p{M} — LQ11) ───────────────────────────────┘        ▼
                                          uo-render (item types, codec, paint helpers) ◄── uo-pdf
                                                     ▲                                  (no dependency
                                    TS canvas painter (decodes)                         on uo-docx)
```

- **uo-docx-layout depends on** uo-docx (read-only `DocView`), uo-fonts, uo-drawingml, uo-render
  (it produces uo-render values), uo-jsnum, uo-text. **Nothing depends on it** except the bindings
  (`uo-wasm`, `uo-ffi`), which implement uo-docx's `LayoutOracle` with it.
- **uo-render** owns the item vocabulary (glyph run, rect, line, image, shape/path, clip, xform,
  fills, effects), the per-page encoding and the painter-side raster helpers. It knows nothing of
  Word: no refs, no runs, no paragraphs. Slides and Sheet emit the same items.
- **uo-pdf** consumes uo-render pages + a resource table (faces with bytes, images with bytes) +
  a neutral `Navigation` (outline entries, link rectangles, named destinations). It does not depend
  on uo-docx or this crate.
- **Reuse by Sheet/Slides**: the display list, the paint helpers, the font registry/face cache
  (LQ8), the metafile renderer later (LQ7). Not shared (checked): Word line breaking, the
  justification and tab rules, `FaceResolver` (Word substitution; the FontSubstitutes `ALIASES`
  and `parsePostScriptName` are app-neutral and can move to `uo_fonts::catalog` with Slides, as
  uo-fonts recorded), table layout, OMML layout (Word only today).

### 2. Data model and ownership

#### LA1. Principles

1. **One input**: a `DocView<'a>` (uo-docx A4) borrowed for the duration of one layout call. The
   layout never mutates the model; field values and hyphenation points go back through
   `LayoutOracle` values (G21). *(revised after the skeptic, K8, K17c)* Font usage leaves the oracle
   (LQ15) and `pages_of` is dropped from it (G21).
2. **No object graph**: every TS identity (`===`, `WeakMap`, `WeakSet`, `Set<object>`,
   `Map<object, …>`) becomes an explicit key (LA3). No `Rc`, `RefCell`, `Weak`, closures stored in
   data, or global state.
3. **Faithful floats**: every number the TS computes is an `f64`, computed in the TS order of
   operations; `Math.*` only through uo-jsnum (`max`/`min` propagate NaN and order `-0`; `round`;
   the fdlibm `sin`/`cos`/`tan`, `hypot`, `sqrt` is IEEE); the clippy `disallowed-methods` list of
   the workspace applies. Integers only where the TS value is provably integral (indices, counts);
   anything derived from a model `JsNumber` stays `f64` (it can be NaN).
4. **Checkpointable pass state**: the `Engine` struct is plain data (`Clone`, and `PartialEq` over
   its flow state), so milestone 2 can resume a pass at a page boundary (LA7). This is the shape
   decision that cannot be retrofitted cheaply after a 1:1 port.
5. **Output owns no model data**: the display list and the geometry hold ids and copied values,
   never borrows of the `Document`, so a `Layout` outlives edits and crosses to JS by encoding.

#### LA2. The derived document of one layout call

`layoutDocument` derives a document before laying out: frames lifted into synthetic textboxes
anchored in a copy of the next paragraph (`liftFrames`, frames.ts:87), chains of linked textboxes
(`indexChains`), continuation slices of chained paragraphs (`sliceParagraphFrom`), deleted
paragraphs joined in the "final" revision view (`joinDeleted`), the revision author order. Rust:

*(revised after the skeptic, K1, K3)* The TS copies are **shallow**: the anchor copy
`{ ...base, runs: [...base.runs, run] }` (frames.ts:121), the revision view `{ ...para, ppr, props }`
(revisions.ts:124, 126), `joinDeleted` `{ ...next, runs: [...carried, ...next.runs] }` (revisions.ts:157,
runs taken from other paragraphs, with the symbol key `REV_JOINS` copied by later spreads) and the
frame inner copy `{ ...p, ppr }` (frames.ts:50) all keep the **model run objects**. So the derived
paragraph is a list of run keys with optional overrides, never a cloned `Paragraph`:

```rust
/// One per layout call; borrows the view, owns only what the TS synthesizes.
pub(crate) struct LDoc<'a> {
    view: DocView<'a>,
    paras: Vec<LPara<'a>>,          // LParaId(u32): every paragraph the passes touch, model or copy
    runs: Vec<RunSrc<'a>>,          // RunKey(u32): every run the passes touch
    stories: Vec<LStory>,           // body, each hf part (per slot use), each note, each textbox content: block lists of LBlock
    textboxes: Vec<LTextbox<'a>>,   // TbKey(u32): model textboxes (DrawingId) + synthetic frame boxes (FrameInfo inside)
    chains: Vec<Chain>,             // TbKey members in seq order; TbKey -> chain
    revision_order: Vec<&'a str>,
}
pub(crate) enum ParaSrc<'a> { Model { para: &'a Paragraph, id: ParaId }, Copy { base: LParaId } }
pub(crate) struct LPara<'a> {
    src: ParaSrc<'a>,
    ppr: Option<Arc<PPr>>, props: Option<Arc<ParaProps>>,   // overrides of a copy (revision view, frame inner copy)
    runs: Vec<RunKey>,                                      // the TS `runs` array of this object, in order
    rev_joins: SmallVec<[u32; 1]>,                          // REV_JOINS (symbol key, copied by spreads)
    flags: ParaFlags,                                       // CONTINUATION, FRAME_SKIP, INNER
    frame_carry: Option<RunKey>, story: StoryRef, path: BlockPath,
}
pub(crate) enum RunSrc<'a> { Model { run: &'a Run, at: RunRef }, Synth(Box<Run>) }  // Synth: slices `{ ...r, text }`, frame runs
```

- `ParaSrc::Model` only when the TS hands the **model object itself** to the engine; every TS copy is
  `Copy` and has **no `ParaId`**, so what the TS keys by paragraph object behaves the same: a manual
  hyphenation skip (`hyphenationSkip(para, offset)`) never matches a continuation slice, a frame
  anchor copy or a revision-view copy, in the TS or in Rust.
- **Runs keep their identity through copies**: a model run inside a copied paragraph is the same
  `RunKey` with its `RunRef`, so `field_values` reports it (the TS `onField(run, v)`, para.ts:549, is
  found by the editor, universal-write.ts:6462) and a float anchored in a copy has the model run as
  `PlacedObject.run` (`h.run === run`, 10701, 13541). Only sliced runs and frame runs are `Synth`,
  with no `RunRef` (absent from `field_values`). **No text is copied**: a slice stores the offset
  and reads the model run's UTF-16 text.
- *(K3)* The TS frame caches (`FRAME_SKIP`, `FRAME_CARRY`, `INNER`, `runCache`, `anchorCache`,
  frames.ts:21–30) and `CTX` (paraformat.ts:7) are module `WeakMap`s keyed by **model** objects and
  survive across layout calls: after an in-place edit (G22 keeps the id) the TS can show a stale
  frame or anchor text, or a duplicate frame (`anchorCache` appends `[...prevRuns, run]`). **Rust
  state is per call; the TS staleness is fixed and accepted by name (LQ18).** Within one call the
  frame run is one `RunKey` used by every pass, as the TS object is.
- *(K3)* `liftFrames` runs in every non-cell `layoutStatic` (engine.ts:1204: headers/footers and
  textbox contents, per page, per probe and pass), not only on the body (193): `LDoc` lifts frames
  in every story once when it is built, with the same `stops` rule per story.
- Cost: O(paragraphs + runs) per call; nothing of the model is cloned. The `Story::blocks()` order
  of uo-docx gives the TS block indices, so `BlockPath` equals the TS ref path.

#### LA3. TS identity → Rust key (the port's rule list; each frontier declares its sites)

| TS identity (file:line) | Use | Rust key |
|---|---|---|
| `Run` objects: `lateFloats`/`lateFound` `Map<number, Run[]>` + `mergeLate` `Set<Run>` (engine.ts:203, 304), `prePlaced: Set<Run>` (371), `PlacedObject.run`, `GLUED_FIELD: WeakSet<Run>` (para.ts:176), `FRAME_CARRY` (frames.ts:25), `Glyph.obj` | late floats per pass; editor finds an object by its run | `RunKey` (LDoc); `PlacedObject` carries `RunRef` (uo-docx) when the run is a model run and its `DrawingId` |
| `Paragraph` objects: `CONTINUATION`, `FRAME_SKIP`, `INNER` (`WeakSet`), `anchorCache`, `listItemValues` `Map<Paragraph, …>`, `hyphenationSkip(para, …)` | flags, caches, editor lookups | `LParaId` + `ParaSrc::Model { id }` (LA2 rule) |
| `DrawingTextbox`: `FRAME_INFO` (`WeakMap`), `indexChains` `Map<DrawingTextbox, TextboxChain>`, editor `members.has(t)` | frame data, chains | `TbKey`; model ones map to `DrawingId` |
| `TableLayout`: `floatFlow` `WeakMap` (engine.ts:363) | split floating tables | `TableLayoutId(u32)` in a per-pass arena |
| `PreparedPara`: `PFMT` `WeakMap` (engine.ts:3931) — one `LineParaFormat` object shared by every line of a paragraph | ruler | per-page `pfmts: Vec<LineParaFormat>` + index in `LineGeom`. *(revised after the skeptic, K20)* The sharing is never observed (the ruler reads values only, universal-write.ts:17135–17138): the canonical stream compares values, no "same object" groups |
| items **and lines** *(revised after the skeptic, K2e)*: `LATE_ITEMS` `WeakMap<object>` (table.ts:1383), `moved`/`own`/`keep` sets holding items and `LaidOutLine`s (table.ts:130–164, 1356), `lastOf.get(last.ref) === last` (table.ts:124), `idx: Map<object, number>`, `seen` | moving items and lines between arrays during row splits and floats | `ItemMeta(u32)` side-table index carried by each pass-scope item **and line**; a `shiftItem`/`shiftLine` copy keeps or drops it exactly as the TS keeps or drops the entry (declared per site) |
| *(K2e)* `CellContent.nest` (`NestRow.items`/`lines` alias objects of `c.items`/`c.lines`, remapped through `moved`, table.ts:141–145) | nested-table rows that split | `NestRow` stores **index ranges** into the content's items/lines, never copies |
| `row.split` closure (table.ts `RowLayout.split`), `CellLayouter` callback, `ParaEnv.field` callback | deferred work with captured state | `RowSplit` data (the captured contents) + `fn split(&self, avail, cx)`; `CellLayouter` implemented by `Engine` through a `Cx` parameter (LA4, K18); `FieldSource` enum + engine method |
| *(revised after the skeptic, K2f)* `boxPage: items` (engine.ts:1398; `page?: object`, 1687) — the **items array** the border box went into (a page body, a cell content or a static frame) | border groups | `ContainerId` (page body `PageIx` \| static frame / cell content id), not a `PageIx` |
| `lnPage` (line numbering), `PageState` objects | line numbering | `PageIx(u32)` |
| *(K2a)* **`FontFace` objects**: kerning, ligatures and run merging apply only across the *same face object* (para.ts:488, 524, 1016; math.ts:252); the objects come from the provider's caches (viewer keys `${family}\|bi` case-sensitively, viewer fonts.ts:94; node-fonts in lower case; C# `SubstituteFontProvider` returns a fresh stand-in per call) | glyph merging, kerning across runs | `FaceId` = **identity of the provider's answer**, not the bytes: each registry policy (K6) reproduces its TS cache keying, and the P2 transcript records "same object as answer j"; the `FaceCache` still shares parsed `Face`s by bytes underneath |
| *(K2b)* `cellNb: WeakMap<TableCell, CellNeighbors>` (engine.ts:1560) | contextual spacing across cells | `CellKey { table: TableId, row: u32, cell: u32 }` — **requires a table id per table and stable cell positions in uo-docx A2** (`TableId` exists; the cell is addressed by its position in its row) |
| *(K2c)* `pinnedFloats: Map<Run, { ps: PageState }>` (engine.ts:2891) | floats pinned on earlier pages | `IndexMap<RunKey, PageIx>` |
| *(K2d)* `chainSlices: Map<TextboxChain, Map<DrawingTextbox, CellContent>>` (engine.ts:3263) | chained textbox content per member | `IndexMap<ChainId, IndexMap<TbKey, CellContentId>>` |
| module counter `lineSeq` (paint.ts:16, never reset) | `lineBox.line` (also drives `cullOffPage`, engine.ts:4005–4038) | per call; the canonical stream relabels by first appearance (the TS value depends on every layout run before in the process). *(K20)* `zSeq` is an `Engine` field (372), ported as such |
| `metricsCache` (fonts.ts:55), `infoCache` (math.ts:43), `CTX` (paraformat.ts:7) | memo of pure functions | values on the face table / per call; no global (K3) |

#### LA4. Pass state

```rust
pub struct LayoutRun<'a> {          // one layoutDocument call
    ldoc: LDoc<'a>, opts: &'a LayoutOptions, fonts: &'a mut FaceTable, budget: Budget,
    resolver_cache: FaceResolverCache,  // (revised after the skeptic, K4) per call; see the note below
    // inter-pass inputs, exactly the TS loop variables (engine.ts:197–251):
    total: f64, marks: IndexMap<Box<str>, Utf16>, late: IndexMap<u32, Vec<RunKey>>,
    late_tables: IndexMap<u32, LateTable>, note_marks: Option<IndexMap<Box<str>, Utf16>>,
    note_refs: IndexMap<Box<str>, Utf16>, chaps: ChapterIndex, sect_pages: IndexMap<u32, f64>,
    balance: IndexMap<i64, BalanceSpec>,
}
#[derive(Clone)]
pub(crate) struct Engine {           // one pass (TS `class Engine`) — plain data, no borrows (revised after the skeptic, K18)
    ctx: DocContext,                 // flow state (numbering, SEQ, notes), revision display, warnings (IndexSet)
    pages: Vec<PageState>, cur: PageIx, col: u32, y: f64, col_top: f64, at_top: bool, prev: Option<PrevPara>,
    table_side: IndexMap<TableLayoutId, FloatFlow>, items_meta: Vec<ItemMetaData>, float_carry: Vec<FloatCarry>, …
    found: PassFindings,             // lateFound, lateTablesFound, balanceFound, used* flags, heading marks
}
pub(crate) struct Cx<'c, 'a> {       // passed to every Engine method (K18)
    ldoc: &'c LDoc<'a>, input: &'c PassInput, fonts: &'c mut FaceTable, resolver: &'c mut FaceResolverCache,
    hyph: &'c mut HyphenSessions, budget: &'c mut Budget, opts: &'c LayoutOptions,
}
```

- *(revised after the skeptic, K18)* **Borrow shape**: `TableLayout`s are owned by the stack frame
  that builds them, as the TS locals are; the `Engine` keeps only `TableLayoutId → side data` (the
  TS `floatFlow` `WeakMap`). `layout_table(…, &mut Engine, &mut Cx)` calls back into the engine for
  cells without aliasing an arena inside it. The ≈ 60 `Engine` fields (engine.ts:326–3261) are
  sorted by F0 into three classes, each named in the record: **output** (pages, items, lines),
  **carried flow state** (compared by the level-2 checkpoints: `y`, `col`, `prev`, numbering, notes,
  `lnCount`, `lastBox`, `pinnedFloats`, `floatCarry`, `lastRowEmit`…), **caches** (`listTexts`,
  heading marks). Fields that point at earlier pages (`pinnedFloats`, `lnPage`, `lastBox`) hold
  `PageIx`/`ContainerId`.
- *(K4)* `DocContext` — and with it `FaceResolver` (context.ts:112) — is new per pass and per
  balancing probe (`newCtx()`, engine.ts:214–217, 225, 256), so the TS provider is asked again in
  every pass. The resolver cache therefore lives **per layout call** (on `LayoutRun`; the face answers
  are identical across passes because the provider caches them); P2 compares the first request per
  lower-case key and the answer identity (LQ19). Per-document reuse is a milestone-2 optimisation
  behind the registry generation.
- **Pass loop and balancing** are ported literally: up to 5 passes until stable (engine.ts:224),
  then per balanced section a binary search where each probe is a full pass (263–290; guard 20).
  Every pass is a fresh `Engine`; only `PassFindings` survive it. *(revised after the skeptic, K19)*
  The final result is the **last executed** pass: when `!ok(hi)` the result is the rejected
  `trial(hi)` layout while `last` stays the previous accepted engine (engine.ts:255–283) — ported
  literally; the work budget and the milestone-2 checkpoints also follow "last executed".
- `DocContext::pure(f)` (save flow → run → restore) clones the flow state: `NumberingState`
  (`IndexMap<Box<str>, Vec<Option<LevelCount>>>` + `IndexSet`s), SEQ counters, heads, note flow.
  Small structs; the clone count is part of the work budget (LA8 of security).
- **Recursion** (cell → `layoutStatic` → table → cell; textboxes in cells; notes) is kept
  recursive and faithful, bounded by the model's `max_nesting` 64 (uo-docx G20) and by a layout
  depth guard; the wasm stack is sized by measurement (LQ13).

#### LA5. Output

```rust
pub struct Layout {
    pub pages: Vec<LayoutPage>,
    pub resources: uo_render::Resources,      // FaceId -> FaceDesc { key, family, bold, italic, synth_bold, synth_italic,
                                              //   stand_in, face: uo_fonts::Face }, ImageId -> ImageRef (K12, below)
    pub warnings: IndexSet<Box<str>>,         // TS Set<string> order
    pub missing_faces: Vec<FaceRequest>,      // provider misses (the shell loads them and relayouts, as the viewer does)
    pub fields: Option<Vec<(RunRef, Utf16)>>, // only with LayoutOptions::collect_fields (onField; last value per run, first-set order)
    stamp: LayoutStamp,                       // formatting_version + story roots it was computed from (editor reuse)
}
pub struct LayoutPage {
    pub page: uo_render::Page,                // w, h, items (paint order), background colour + fill, display flag
    pub section: u32, pub number: f64,        // TS `number` (can be any JS number)
    pub hf_items: u32,
    pub lines: Vec<LineGeom>, pub chars: CharsSoA /* o: u32, x: f64, w: f64 */, pub pfmts: Vec<LineParaFormat>,
    pub objects: Vec<PlacedObject>, pub cells: Vec<PlacedCell>,
}
pub struct LineGeom { pub at: LineRef, pub start: u32, pub end: u32 /* UTF-16 */, pub x: f64, pub y: f64,
    pub height: f64, pub baseline: f64, pub width: f64, pub chars: Option<Range<u32>>, pub hyph: Option<u32>,
    pub rev: Option<(f64, f64)>, pub frame: Option<(f64, f64)>, pub pfmt: Option<u32> }
pub struct LineRef { pub story: LineStory, pub path: BlockPath }  // "b3/r0/c1/b0", "hdr:default/b0", "txbx/b3" in the canonical stream
pub enum LineStory { Body, Note(NoteKind, i32), Textbox(TbKey),
    HeaderFooter { which: HfWhich /* header | footer */, slot: HfSlot /* default | first | even */, part: HfPartId } } // K13
pub enum ImageRef { Media(MediaId), Derived { bytes: Arc<[u8]>, ext: Sym } }   // K12, deduped by content per Layout
pub struct PlacedObject { pub run: Option<RunRef>, pub drawing: Option<DrawingId>, pub x: f64, pub y: f64, pub w: f64, pub h: f64,
    pub layer: Layer, pub hf: bool, pub frame: Option<ObjectFrame>, pub content: Option<(Range<u32> /* lines */, [f64; 6])> }
```

- **uo-render page** (owned by uo-render, sketched for the boundary): `items: Vec<Item>` with
  structure-of-arrays side buffers (glyph `cp: u32`, `x: f64`, optional `gid`; UTF-16 `text`; path
  commands; dash arrays), interned strings per layout (`Sym(u32)`: colours, preset names, dash
  names), so a glyph costs ≈ 12–16 bytes instead of a JS object. **Colours stay the TS strings**
  (interned), because the TS writes what the document had (`'ffffff'`, `'auto'`…) and the painters
  test `HEX6` (canvas.ts) — an `Rgb(u32)` would change which items paint. *(revised after the
  skeptic, K14)* The two painters read colour strings differently: the canvas skips non-`HEX6`
  strings (canvas.ts:32, 131, 159, 223, 320), the PDF parses any string through `hexColor` →
  `parseInt`, so `'auto'` gives NaN components (pdf-writer/document.ts:44–51). The uo-render plan
  states, **per item field, which painter's parse is the reference**, and both new painters follow
  it per field; the engine keeps emitting the raw document strings (e.g. page background,
  engine.ts:759).
- *(K14)* **Word-only decisions travel in the Write geometry section**, not in the shared uo-render
  item: `lineBox.line` (the cull group of `cullOffPage`, engine.ts:4005–4038), `lineNum` (kept off
  page in the PDF), `ClipItem.src` (the editor finds the EMF by its item), and the Word `bdr*` dash
  names (resolved to dash arrays for the shared item, name kept in the Write section). `asPath`/
  `colr` and `PictureLook` preset strings are generic enough for the shared item.
- *(K13)* Header/footer line refs name the **slot** (`hdr:${kind}`/`ftr:${kind}`, engine.ts:1157–1185)
  and one part can serve several slots: `LineStory::HeaderFooter` carries the slot and the part.
- *(K12)* **The layout creates images**: EMF bitmap records become BMP bytes (emf.ts:171–195,
  465–503), EMF+ compressed images are copied (474), `resolvePageFill` builds a new `InlineImage`
  (page-fill.ts:34) — new objects per pass and per call. `ImageRef::Derived` holds them, deduped by
  content per `Layout`, counted in M3, and travels to uo-pdf in the resource table.
- **Everything the painters derive from model objects is resolved here** (LQ6): `ShapeItem.style`
  (a docx-core `ShapeStyle`) becomes resolved geometry — path commands from `presetPath`/`custPaths`
  with flips and rotation, open/closed and even-odd flags, `fillAlpha`/`line.alpha` — and
  `ImageItem.image`/`ClipItem.src`/`blip.image` (model `InlineImage`) become `ImageId`s with the
  media id and the effect parameters. Font-dependent painter work (outlines of `gid` glyphs, `asPath`
  and COLR runs; the F0xx symbol remap; canvas batching hints) is resolved at **encode time** from
  the face table, so the painter never queries a font.
- Numbers: `f64` in memory and in the encoding (LQ5). Positions are twips from the page top-left,
  as today.

#### LA6. Fonts

- `FontRegistry` (uo-fonts `registry`, LQ8): the host pushes faces (`family`, `bold`, `italic`,
  `bytes: Arc<[u8]>`, `ttc index`, `stand_in`, `source`, licence flags) and asks for the misses after
  a layout. Synchronous, no callbacks into the shell (the TS `FontProvider.face` is synchronous over
  loaded fonts too). *(revised after the skeptic, K6)* The answer to `face(family, bold, italic)`
  comes from a **named policy** — `viewer` (viewer fonts.ts:90–137), `server-substitute` (C#
  `SubstituteFontProvider.cs`), `oracle` (node-fonts.mjs:162–181, test-only), `editor`
  (`liveFontProvider`, milestone 2) — each reproducing its TS (or C#) cache keying, because the
  answer's identity is observable (K2a). Each policy has its own parity facet (§5).
  Embedded document fonts (uo-docx `read_embedded_fonts`, deobfuscated) are registered per document
  with the `withEmbeddedFonts` precedence (fonts.ts:316–352, incl. "variant not embedded but the
  family exists on the system → prefer the system").
- `FaceCache` (uo-fonts `registry`): parsed `uo_fonts::Face` deduplicated by bytes (length +
  SHA-256, then the `Arc` is shared), **one per wasm instance / native process**, LRU over
  `bytes.len() + face.heap_bytes()` (needs a new `Face::heap_bytes()` in uo-fonts — LQ8), faces
  pinned while a layout or a live `Layout` references them.
- **Per-document face budget (uo-fonts D4)** *(revised after the skeptic, K15)*: host-configurable
  profiles — `desktop` (wasm32: 128 MiB pinned per document, 64 MiB unpinned cache; native 1 GiB /
  512 MiB) and **`phone`** (48 MiB pinned, 16 MiB cache), chosen by the binding. The phone M3 target
  (160 MiB) counts **JS and wasm** face bytes together, because the canvas painter also needs every
  face as a browser `FontFace` (canvas.ts:321–323 via `fontOf`): a face's bytes exist once in wasm
  (the registry copy) and once in the browser's font store; the shell drops its fetch buffer as soon
  as the face is registered and the `FontFace` is created. Past the document budget, a new face resolves as a miss of the
  provider (the `FaceResolver` chain continues to the fallbacks; warning `"font budget"`), which
  differs from the TS only on documents that embed tens of fonts — accepted by name, counted by the
  capture (0 expected on the corpus). First numbers; measured at gate 2 (M3).
- `FaceTable` (per `Layout`): `FaceId` in first-use order → resolved face (`FaceResolver` result:
  key, effective family, synthetic flags) + the `FaceMetrics` computed once. *(revised after the
  skeptic, K4)* `FaceResolver`'s cache (fonts.ts:229, keyed `family|bi`) lives **per layout call**
  (LA4), as the TS one lives per pass with the provider's own cache under it.
- Hyphenation: `uo_fonts::HyphenationRegistry` per engine, one `HyphenSession` per language
  (word cache capped, M5). *(revised after the skeptic, K5)* Which languages hyphenate is an
  **input**, not a property of what is compiled in: the TS hyphenates only the patterns loaded in
  the process (typography/hyphen/index.ts:47–80), and every host loads a list — the viewer
  `hyphenationLangs(full)` (viewer:605) = `documentHyphenationLanguages` (hyphenate.ts:66–80) ∩ the
  app list `['pt-BR','en','es']` (viewer:28, editor:308); the editor asynchronously (16098–16107);
  clickCompliance passes `HyphenationLanguages` to the server PDF; `oracle.test.ts` loads all 7.
  `LayoutOptions.hyphenation_loaded: Set<PatternId>` is computed by the binding exactly as the shell
  does and recorded in every capture (LQ16).

#### LA7. Caches and incremental relayout (API now, implementation milestone 2)

- **Criterion**: an incremental relayout must give **the same `Layout` bits as a full layout** of
  the same view. The TS fast path (`tryFastLayout`/`tryFastTable`, universal-write.ts:16235–16438:
  one paragraph laid out alone in a synthetic tall page, spliced in when the line count and heights
  match, then a full relayout 1.5 s later) is a transient approximation; the D2 parity harness
  checkpoints after the idle relayout (uo-docx G17), so it is not a parity target. Not reproducing
  its transient output is accepted by name (LQ4).
- **Milestone 1**: `relayout(view, changes)` exists and is a full layout (correct by definition);
  the viewer never edits.
- **Milestone 2, level 1** — memo of the pure per-paragraph stages: `prepare` + line breaking keyed
  by (`LParaId` source `ParaId`, `Arc::ptr_eq` of the paragraph node, a hashed `ParaEnvKey`,
  `formatting_version`, the frame widths sequence, and the **recorded reads**: field values,
  numbering marker, note marks, other paragraphs read by REF/STYLEREF/NOTEREF), replaying the
  recorded flow effects (numbering `next`, SEQ, note marks, warnings) on a hit. Capped (M5).
- **Milestone 2, level 2** — page checkpoints: the `Engine` flow state at each page start is kept
  (LA1.4); a relayout resumes at the page holding the first changed block and stops when the state
  at a page start equals the previous run's and no inter-pass input changed (`total`, marks, late
  floats, balance) — then the old pages are spliced. Multi-pass interplay (NUMPAGES, PAGEREF) falls
  back to the full loop when an inter-pass input changes.
- Both levels are verified by a differential test "incremental == full" over random edit scripts
  (Rust only) plus "Rust full == TS full" on the edited documents. *(revised after the skeptic, K3)*
  The TS reference for an edited state is always a **fresh read** of that state's saved docx in a
  fresh process, never the live editor objects (whose module caches can be stale, LQ18).

### 3. Public API

```rust
// uo_docx_layout
pub struct LayoutOptions {
    pub lang: Box<str>,                       // 'pt-BR'
    pub now: LocalDateTime, pub tz: TimeZoneRule,   // DATE/TIME, CREATEDATE…: local parts and offset at an instant (LQ10)
    pub hyphenation: Hyphenation,             // On | Off | Force
    pub hyphenation_loaded: IndexSet<PatternId>,      // (revised after the skeptic, K5, LQ16) what the TS process had loaded
    pub hyphenation_languages: Option<Vec<Box<str>>>,
    pub hyphenation_skips: HyphenSkips,       // FxHashSet<(ParaId, u32)> (TS callback keyed by paragraph object)
    pub revisions: Option<RevisionView>,
    pub view: ViewTransform,                  // Print | Web { width } | Draft { … } | Outline { … } (LQ2)
    pub collect_fields: Option<FieldScope>,   // onField; FieldScope = all | a set of RunRefs (K1: model runs inside copies included)
    pub limits: LayoutLimits,
}
pub struct LayoutEngine { /* per document: hyphen sessions, para memo (M2), last Layout; the FaceResolver cache is per call (K4) */ }
impl LayoutEngine {
    pub fn new(registry: Arc<FontRegistry>, cache: Arc<FaceCache>) -> LayoutEngine;
    pub fn layout(&mut self, view: DocView<'_>, o: &LayoutOptions) -> Result<&Layout, LayoutError>;
    pub fn relayout(&mut self, view: DocView<'_>, changes: &[StoryChange], o: &LayoutOptions) -> Result<&Layout, LayoutError>;
    pub fn last(&self) -> Option<&Layout>;
    pub fn clear_caches(&mut self);
}
pub enum LayoutError { Limit(LimitKind), Internal(&'static str) /* where the TS would throw */, NoFace }

impl Layout {                                  // viewer, PDF and editor queries (query.rs)
    pub fn page_count(&self) -> usize;
    pub fn encode_page(&self, i: usize, out: &mut Vec<u8>);            // uo-render codec + Write geometry section
    pub fn run_boxes(&self, view: DocView<'_>, pick: RunFilter) -> Vec<RunBox>;   // links, plugin inlines (run-boxes.ts);
                                               // (K17e) each box carries the RunProps the plugin contract exposes and its FaceId
    pub fn measure_text(&self, face: FaceId, size_pt: f64, text: &[u16]) -> f64;  // (K17e) for paintCustomInlines (read run0.face.font)
    pub fn navigation(&self, view: DocView<'_>) -> uo_render::Navigation;          // pdf-navigation.ts → uo-pdf
    pub fn lines_of(&self, story: StoryRef, path: &BlockPath) -> impl Iterator<Item = (u32, &LineGeom)>;
    pub fn hit_test(&self, page: u32, x: f64, y: f64) -> Option<Hit>;            // DocRef + object + cell
    pub fn caret(&self, at: &DocRef, affinity: Affinity) -> Option<CaretRect>;    // caretXIn
    pub fn offset_at_x(&self, line: LineHandle, x: f64) -> u32;                   // offsetAtX
    pub fn selection_rects(&self, from: &DocRef, to: &DocRef) -> Vec<(u32, Rect)>;
    pub fn page_of(&self, at: &DocRef) -> Option<u32>;   // (K17c) the editor's `pageOf` in the find list (universal-write.ts:17889) — a Layout query, not an oracle call
}
// the LayoutOracle of uo-docx (G21, revised after the skeptic: no pages_of, no font_usage)
impl LayoutOracle for BindingLayout { field_values, next_hyphenation }
```

- **No measuring callback** into the shell. *(revised after the skeptic, K8)* `InlineRenderer.measure`
  is called by the editor only through `buildSpans` (universal-write.ts:10893–10918: the legacy
  layout, `fontUsage()`, `effectiveTbHeight`), never by the engine; after G19 it is dead. Custom
  inlines are laid out by their fallback text and the renderer draws over the box from `run_boxes`.
  uo-docx A4's `InlineMeasurer` is dropped (LQ1; plugin API change in the CHANGELOG).
- *(K17c)* `pages_of` leaves the oracle: TOC and PAGEREF page numbers reach the editor through
  `onField` (`refreshFieldCache`, universal-write.ts:6457–6466), i.e. `field_values`; `bookmarkPages()`
  is internal to the pass loop (engine.ts:638). The only other editor use is `pageOf` for find
  results, a query on the binding's `Layout`.
- *(K8)* `font_usage` leaves the oracle (LQ15): the embedding plan and the fonts to load before a PDF
  come from a literal port of the editor's model-only `fontUsage()` in the uo-docx session.
- **What crosses to JS** (milestone 1): one handle per opened document for the viewer
  (`UoDocumentView`: `Document` + `LayoutEngine`) and the instance-wide `FontRegistry`/`FaceCache`;
  `layout()` returns page count, warnings, misses; pages are pulled encoded (`encode_page`, Rust-owned
  buffer, JS copies and frees) only for the pages being painted; text layer and link boxes come with
  the page encoding; PDF export is one call returning bytes. In milestone 2 the same `LayoutEngine`
  lives inside `UoWriteSession`, and the hot paths (hit test, caret, selection rectangles, line
  motion) are dedicated exports.
- **.NET** (server PDF): `DocxPdf.Convert(bytes, options) -> bytes` through uo-ffi, faces from the
  native system index (LQ8); no layout handle crosses. *(revised after the skeptic, K6)* The facade
  keeps `HyphenationLanguages`, `Now` and the substitute policy that clickCompliance uses
  (`UniversalOfficeWordToPdfConversionService.cs:34–39`), and drops or wraps the host
  `IFontProvider`/`List<PdfFontSource>` parameters (CHANGELOG).
- **Changed from 0.9** (CHANGELOG at the switch): `layoutDocument(full, provider, opts)` becomes an
  engine object with a registry; `LayoutResult` items reference faces/images by id; `ShapeItem`
  carries resolved geometry instead of a `ShapeStyle`; refs are structured; `onField`/
  `hyphenationSkip` callbacks become data; the lab options are gone (LQ12); `layout="legacy"` and
  the legacy canvas layout are removed (LQ2).

### 4. Crate vs own port

| Need | Candidates | Recommendation |
|---|---|---|
| Ordered maps/sets (TS `Map`/`Set` order) | `indexmap` (allowed) | use |
| Fast hash for keyed caches | `rustc-hash` (spike 1 lesson) | use (`FxHashMap`) where order is not observable |
| Arena | `typed-arena`, `slotmap` | **own** `Vec` + typed index (no removal needed within a pass) |
| Small vectors (`BlockPath`) | `smallvec` | as uo-docx decides |
| NFC composition, `\p{M}` | `unicode-normalization` (Unicode version not pinned to Node 24.15's ICU) | **generated tables** in `uo-text` (LQ11) |
| Line breaking / shaping | `unicode-linebreak`, `rustybuzz`, `swash` | **none** — the engine implements Word's rules, not UAX #14 or OpenType shaping; any of them breaks parity |
| Polygon/path geometry | `kurbo`, `lyon` | **own** (TS arithmetic order; uo-drawingml already decided the same) |
| Regex for field instructions (*revised after the skeptic, K20*: 97 sites, 0 dynamic, 3 with `u`) | `regex`, `regress` | **own matchers** from a spec table (uo-docx G9 policy) |
| *(K9)* `JSON.stringify` keys and equality | `serde_json` (no JS key-order/`undefined`/NaN rules) | **own** `json_key`/`json_eq` per type with the JS JSON rules (LQ20) |
| *(K10)* UTF-16 case mapping with SpecialCasing | `char::to_uppercase` (UTF-8 chars, Rust's Unicode version) | **generated tables** in `uo-text` over `&[u16]` (LQ11) |

`#![forbid(unsafe_code)]`, `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]`,
`#![deny(clippy::indexing_slicing)]` (LQ9).

### 5. Parity strategy

#### Captures (Node 24.15 x64, uo-jsnum G2; inputs and outputs, exact)

| Capture | Where in the TS | Compared |
|---|---|---|
| **P1 layout** (the gate) | `layoutDocument(full, withEmbeddedFonts(provider, …), opts)` → `LayoutResult` | every page: `widthTw`, `heightTw`, `section`, `number`, `hfItems`, background fields, every item (all fields, **f64 bits**, `-0` kept, NaN canonical), every line (all fields incl. `chars`), `objects`, `cells`; `warnings` in order |
| P1 painter inputs | the same items, through the TS functions the painters call on them: `presetPath`/`custPaths`/`shapePath` for shapes, `imagePixelSize` for blips, symbol remap and outlines for `gid`/`asPath`/COLR glyphs | the resolved geometry Rust stores (LA5), exact |
| **P2 provider transcript** *(revised after the skeptic, K2a, K4)* | every `provider.face(family, bold, italic)` call and its answer (`null`, or file path + SHA-256 + face index + `standIn`) **plus the answer's identity** ("same object as answer j") | the Rust replay answers from the transcript with the same identities; compared: the **first request per lower-case key** and the answer identity (the TS asks again in every pass, K4; Rust resolves once per call) |
| *(K6)* **P2b provider policies** | the TS viewer provider (viewer fonts.ts:90–137) over a stub resolver; the C# `SubstituteFontProvider` over a fixed system index (C# is the reference); node-fonts' score pick over the oracle machine | the registry policies `viewer`, `server-substitute`, `oracle`, exact answers and identities |
| P3 facets (localization, not the gate) | `prepareParagraph` (glyph arrays), `LineBreaker` calls (the spike's `lines/capture.mjs`), `layoutTable` per table, `NumberingState` markers, `StyleResolver` outputs, `markHyphenation`, `resolveTextFx`, `pictureLook`, `renderEmf`/`renderWmf`; *(K5)* `documentHyphenationLanguages` | per facet, exact |
| P4 editor queries *(revised after the skeptic, K8, K17c)* | `onField` values (`collect_fields`, incl. model runs in copied paragraphs), the hyphenation search with `'force'` + skips (universal-write.ts:17337–17353), `runBoxes`, `documentOutline`/`listItemValues`/`numberedItems`/`outlineParagraphs`, `pdfNavigation`, `caretXIn`/`offsetAtX` over every line, `pageOf` (17889); the uo-docx session's `fontUsage()` port. Editor-private functions are reached on an unminified bundle (uo-docx G17) | exact |

- **Canonical stream** (uo-testkit `Stream`, `.cap`): a TS walker extending
  `tools/gen-parity-layout.mjs` `normalizeLayout` (same field coverage) with exact bits instead of
  0.01 rounding; object references mapped to stable values — `FontFace` → (key, family, flags,
  file digest), `InlineImage` → (media part name, SHA-256), `PlacedObject.run` → the TS ref of the
  run (story path + run index) or `synthetic`, `lineBox.line` relabelled by first appearance, refs
  as the TS strings (HF refs by slot, K13). *(revised after the skeptic, K20)* `pfmt` compared by
  value (its sharing is never observed). The Rust `Canon` emits the same stream from `Layout` (refs
  printed as TS strings).
- **Determinism**: `opts.now` fixed, `TZ=UTC` (plus one facet under `TZ=America/Sao_Paulo` for the
  date fields), `mk-*` cases with `revisions: { mode: 'all' }` (as the oracle and the .NET parity),
  a fresh process per shard (`lineSeq` is process-global; relabelled anyway). *(revised after the
  skeptic, K5)* The loaded pattern set is an input recorded in each capture: the default run loads
  all 7 (as `oracle.test.ts`), and a second sweep uses the production setting of all three hosts
  (`documentHyphenationLanguages` ∩ `['pt-BR','en','es']`). *(K3)* The document is **read again for
  every option run** (never reuse `FullDocument` objects across runs: the TS frame caches are keyed by
  them).
- **Fonts and logistics** *(revised after the skeptic, K17)*: the Word oracle runs on the machine's
  installed fonts (never versioned, CLAUDE.md). **The oracle has no open-font-only document** (Arial
  is referenced in 2,179 of the 2,742, Calibri in 333, Times New Roman in 183), so **CI layout parity
  is the generated documents only**. For the gate, the P1 capture is **regenerated in the same gate
  run on the same machine** (TS, then Rust); a face whose digest differs from the transcript **fails
  the gate** instead of skipping (a stored capture plus a Windows font update would otherwise turn
  the gate into mass skips). Between gates, stored captures are a development aid only.

#### Corpus

| Set | Size | Where it runs |
|---|---|---|
| Word oracle (`test/oracle`, 259 groups) | 2,742 docx | local Windows with Office fonts (as `oracle.test.ts` and `LayoutParityTests`), TS capture regenerated in the gate run (K17) |
| .NET synthetic inputs (`dotnet/tests/fixtures/input`: pictures, page, page-pattern, page-frame, page-tile, textboxes, texteffects) | 7 docx | local |
| option sweeps: the four revision modes over `mk-*`, `hyphenation: 'force'` with seeded skips, `lang: en-US`, the production hyphenation list (K5), Web/Draft transforms (LQ2, Rust-only after the switch) | ≈ 3,300 runs | local |
| **differential generated documents** (uo-testkit generator: random sections, columns, tables incl. nested and merged, floats, textboxes and chains, notes, fields, lists, revisions) with **open fonts only** (Liberation, Carlito, Caladea, Cambria Math substitute) | 2,000 per CI run, seeded | CI |
| stress: a 500-page document, *(K16)* 500 pages × 20 balanced continuous sections, nesting 64, 10,000-row table, 1,000 continuous column sections, one 1 MB word, a field code of 10⁵ nested parentheses (K11) | 12 | CI (time and memory, and parity where the TS finishes) |

#### Against Word (where the TS already differs — accepted by name)

The target is the **TS output**, never Word. The Word comparison (`oracle-compare.ts`) runs over the
Rust output too, as a sanity check with the TS thresholds frozen as they are: `SWEEPS` (justify2/3/4/5/8,
jsweep: ≥ 98% of lines), `vertalign` (≥ 90%), the `DY_TOL`/`DX_TOL` tolerances, `ORDERLESS`
(en-cols, sect-cols2), `BY_BASELINE` (lnnum2), `CHAR_ONLY` (supraise), `STRICT`. Before the port
starts, the TS oracle suite is run once and **its failing cases are frozen by name** in the capture
metadata; a Rust result that equals the TS is accepted there whatever Word shows.

### 6. Memory plan (measured gate)

- **M1** (exact return to the start after `clear_caches`): `layout` (ok; each `LayoutError` incl. a
  limit hit mid-pass and mid-balancing), `relayout`, `encode_page`, `run_boxes`, `navigation`,
  `hit_test`, `caret`, `selection_rects`, `page_of`, `measure_text`, each oracle call, registry
  add/remove (each policy), face-budget eviction.
- **M2 soak**: (A) viewer — 1,000 × (layout → encode every page → drop the layout) over 20 corpus
  documents incl. the largest and the 500-page stress, model kept; (B) editor-like (milestone 2) —
  1,000 × (100 edits through a test mutation helper on `Story` → `relayout` each; every 10th compared
  bit-equal with a full layout → restore the previous root → relayout). Sample every 50; tolerance:
  flat within the cache caps after warm-up, **0 B** after `clear_caches`; wasm `memory.buffer`
  flat after 200 iterations.
- **M3 budgets** (first record; the targets below are proposals to be replaced by measurements):

| Operation | Reference | Target |
|---|---|---|
| retained `Layout` (display list + geometry) | TS heap delta of `LayoutResult` (node `--expose-gc`), per document class | ≤ 0.5 × TS (spike 2: 0.48 × for lines) and ≤ 48 B per laid-out character on the 500-page document |
| peak of one layout call | — | ≤ retained `Layout` + 2 × the largest page's working set + the face table |
| encoded page | — | ≤ 1.5 × the page's in-memory size; buffer freed by JS |
| viewer, 100-page typical document, *(revised after the skeptic, K15)* wasm high-water **plus the JS-side face bytes** (browser `FontFace`s, fetch buffers) | today's TS tab heap | ≤ 160 MiB with the `phone` face profile (48 MiB pinned, 16 MiB cache) (phone baseline, iOS 15) |
| 500-page stress, wasm high-water | — | ≤ 384 MiB (phone risk, reported, not a gate) |
| per-document faces pinned | uo-fonts D4 | ≤ the profile's `face_budget` (LA6) |
| *(K12)* derived images per `Layout` (EMF bitmaps, page-fill images) | TS: new objects per pass | deduped by content; counted in the retained `Layout` |

- **M4**: `UoDocumentView` (10,000 create/free and the finalizer path, with a laid-out document
  inside), encoded page buffers, registry handles; .NET: the PDF call returns owned bytes only.
- **M5** (each filled past its cap): `FaceCache` (bytes, both profiles), per-document face pins
  (budget), the per-call `FaceResolver` cache (dropped with the call; M1 covers it), `HyphenSession` word caches (100,000 words per language),
  milestone-2 para memo (entries + 64 MiB wasm32), page checkpoints (bounded by pages × state size,
  dropped with the layout), the binding's encoded-page cache (pages).
- **M6**: no `unsafe` planned (Miri only if it appears); LeakSanitizer through the uo-ffi PDF path.

### 7. Security plan (hostile documents)

Untrusted inputs reaching the module: the model of an untrusted docx (already bounded by D1a:
nesting 64, `DocxLimits`), font bytes (uo-fonts limits), EMF/WMF records, field instructions and
pictures (`\@`, `\#`, formulas), numbering formats and huge counters, every numeric attribute
(sizes, indents, spacings, grid, page size, columns, spans) including NaN and ±∞.

| Risk (TS behaviour) | Limit (`LayoutLimits`) | Outcome |
|---|---|---|
| work explosion: 5 passes × balancing probes (≤ min(N, 20) sections × (log₂ h + 2), each a full pass), `pure()` re-layouts of cells per nesting level (kᵈ), late-float/late-table ping-pong | *(revised after the skeptic, K16)* `max_work` **proportional to the input**: units per prepared glyph × the passes the TS loop can legitimately run (5 + the balancing bound for the document's balanced sections) × a nesting factor, plus a floor — not a constant above the (small) oracle documents | `Err(Limit(Work))` (TS: hangs); the stress row "500 pages × 20 balanced sections" must pass |
| *(K11)* V8 `RangeError` throws: argument spreads above ≈ 10⁵ elements (`items.push(...floatItems)`, engine.ts:1495–1496; `Math.min/max(...)`, 3645, 4031–4032, table.ts:129, 153; `lines.push(...)`, table.ts:1339; `path.push(...poly(pts))`, emf.ts:289, 314), `'*†‡§'[…].repeat(…)` on huge counters (numbering.ts:63) | none (Rust does not throw) | the TS throws and `layoutDocument` fails; Rust succeeds — **accepted by name** (LQ14), counted by the capture |
| *(K11c)* unbounded recursion in `parseArithmetic` (field-formula.ts:93–117) on `((((…` or `----…` in a field code (the TS rethrows the `RangeError`, 126, so the layout fails; in Rust a stack overflow aborts the wasm instance); field codes are run-level, so `max_nesting` does not bound them | depth guard on `parseArithmetic` and on **every recursive parser over untrusted text** (LQ13) | `Err(Limit(Depth))`, as the TS fails |
| page explosion: zero/negative page or column size, 1 glyph per line over 1M lines, `pgSz` 1e9 | `max_pages` (e.g. 100,000), `min_content_width` guard where the TS would loop without progress | `Err(Limit(Pages))` / the TS's own progress rule where it has one |
| huge tables (100k rows × 63 columns, `gridSpan` 1e9) | model limits + `max_table_cells` per table | `Err(Limit(Table))` |
| deep recursion (cells, textboxes, groups, OMML) | model `max_nesting` 64 + layout depth guard + measured wasm stack (LQ13) | text fallback from the reader; `Err(Limit(Depth))` past the guard |
| metafiles: record counts, nested save/restore, huge polypolygons, text records with hostile fonts | `max_metafile_records`, `max_metafile_items`, path points (uo-drawingml `max_path_points`) | truncated drawing + warning (TS draws until memory runs out) |
| number formatting: roman/alphabetic/ordinal text of 1e15, `\#` pictures, date pictures of 10k characters | output length caps per field | truncated + warning |
| pathological line breaking (one 1 MB word, all-NBSP, thousands of tabs) | `max_work` | `Err(Limit(Work))` |
| faces (many embedded fonts) | per-document face budget (LA6) | fallback face + warning |
| panics (wasm `panic = "abort"` kills every document in the instance) | `deny(indexing_slicing)`, checked integer arithmetic on counters, no `unwrap` (workspace) | a TS `x[i]!` that would throw → `Err(Internal)`; a TS `x[i]` read as `undefined` → `get()` with the TS fallback (declared per site) |

Each limit sits above everything the corpus and the stress set reach (the capture records the
per-counter maxima); results differ from the TS only where the TS hangs, exhausts memory or throws a
`RangeError` (K11). **Fuzz targets**:
`layout_docx` (bytes → read with small limits → layout with open fonts: no panic, terminates within
`max_work`, every index in the output valid, encode → decode round trip), `layout_model`
(structure-aware, uo-testkit generator), `emf_records`, `wmf_records`, `field_instr` (field formula,
date and number pictures, numbering formats), `hit_test` (random queries on laid-out pages),
`incremental` (milestone 2: random edits, incremental == full), `omml`.

### 8. JS semantics and risks (each becomes a declared item per frontier)

1. **Floats and `Math`** *(revised after the skeptic, K20: counts over the ported files only)*: 291
   `Math.max`, 127 `min`, 46 `round`, 31 `floor`, 18 `ceil`, 34 `sin`/`cos`, 3 `hypot`, 1 `tan`,
   1 `pow`, 1 `exp`, 4 `sqrt` — all through uo-jsnum; `Math.max(...[])` = −∞ (lines.ts `nextTab`);
   `x | 0`, `>>> 0` as `uo_jsnum::conv`.
2. **Iteration order**: `Map`/`Set` insertion order (`IndexMap`/`IndexSet`); plain-object records
   iterate integer-like keys first (e.g. `Object.values(styles).find(...)`, engine.ts:1672, over
   styles keyed by `styleId`) — the uo-docx `StyleSheet` order must be the TS object order.
3. **Identity** (LA3) and **module state** (LA3 last rows).
4. **Closures** stored in data (`RowLayout.split`, `CellLayouter`, `ParaEnv.field`) → data + traits.
5. **Text**: UTF-16 offsets (`text.length`, `codePointAt` with lone surrogates, `Array.from` by code
   point, `String.fromCodePoint`); NFC of hyphenation clusters and `\p{M}` via uo-text (LQ11).
   *(revised after the skeptic, K10)* `uo_jsnum::text` has **no case mapping** and works on `&str`
   (whitespace, `trim`, `split_js_space`, `utf16_len`, `utf16_slice`). Layout needs JS
   `toUpperCase`/`toLowerCase` with SpecialCasing over `&[u16]` with lone surrogates: caps
   (para.ts:749), small caps with `toUpperCase().length === 1` (789–820), `doNotHyphenateCaps`, and
   the length changes (`ß` → `SS`) that move offsets — plus JS `\s` split/trim over `u16`. All in
   uo-text, generated from Node 24.15 (LQ11).
6. **Regex** (*K20*: 97 sites, 0 dynamic, 3 with `u`; mostly field instructions): JS `\s` (Unicode white space + line
   terminators + U+FEFF), ASCII `\b`, `/i` without `u` (simple case folding), `\p{M}` with `u` —
   hand matchers from a spec table, differential against JS `RegExp`.
6b. **`JSON.stringify` as key and equality** *(revised after the skeptic, K9)*: `bdrKey`
   (engine.ts:4089) groups paragraph borders by `JSON.stringify([pBdr, ind])` with `pBdr` built by
   `{ ...base.pBdr, ...over.pBdr }` (resolve.ts:82), so the grouping depends on key insertion order;
   also `fxKey` (para.ts:249), the shaping key with `JSON.stringify(r.ot)` (476), `eq`
   (context.ts:217), the old-formatting comparisons (revisions.ts:111, 122). One `json_key`/`json_eq`
   per type with the JS rules (insertion order, `undefined` keys omitted, NaN/±∞ → `null`, −0 → `0`);
   `derive(PartialEq)` is banned for these uses (it is the opposite for NaN and ignores order). Key
   order comes from the uo-docx insertion-order facet (A6/G23) or is accepted by name with a capture
   counter for the cases where order changes the TS result (LQ20).
7. **Exceptions**: a TS throw inside `layoutDocument` aborts the layout → `Err` (*K11*: V8 `RangeError`
   throws on large spreads and `repeat` are not reproduced — accepted by name); the TS `try/finally`
   (e.g. `listNumberOf` restoring numbering, engine.ts:541) and `try/catch` with partial results
   (unreadable embedded font ignored, fonts.ts:327) kept.
8. **Sort**: 14 `.sort` sites; JS sort is stable → `sort_by` (stable); comparators with NaN
   reproduce the TS order only where the comparator is consistent — inconsistent ones are captured.
9. **Dates**: `new Date(iso)` (`Date.parse` grammar, engine.ts:1132), local getters in `datefmt.ts`
   → `LocalDateTime` + `TimeZoneRule` (LQ10).
10. **Number to string**: `String(n)`, `toFixed`, `toString(16)`, `padStart` → uo-jsnum B.
11. **Recursion depth** (LQ13). 12. **`undefined` vs absent** options (`??` defaults) — the
    `LayoutOptions` defaults equal the TS ones (`advanceEm: 'nominal'`, `advanceRound: 'word'`,
    `justifyShrink: { ratio: 0.25, expandRatio: 0.5 }`, `lang: 'pt-BR'`, hyphenation on).

### 9. Port order: Opus frontiers

All frontiers of this crate are Opus (Models rule; the maintainer's "uo-docx-layout inteiro em Opus"
stands). Each compiles with clippy clean against the F0 types, runs its facet of the capture
continuously, and declares its sites (uo-docx A7 items 1–9, the LA3 keys, the `json_key` sites, every
limit check and its TS behaviour).

*(revised after the skeptic: K1, K2, K6, K7, K10, K12, K13, K18 and the order verdict)*

| # | Frontier | TS → Rust | Lines | Depends on |
|---|---|---|---|---|
| **F0** | foundation and harness | crate skeleton; `LDoc` with the shallow `LPara`/`RunSrc` (K1), the LA3 keys incl. face answer identity, `CellKey`, `pinnedFloats`, `chainSlices`, line `ItemMeta`, `nest` by index, `ContainerId` (K2); `Engine` struct with **every field classified** as output / carried flow state / cache, `Cx`, `PassInput`/`PassFindings` (K18); `Glyph`/`PreparedPara`/`Line`/`PlacedGlyph`/`CellContent`/`RowLayout`/`RowSplit`/`TableLayout` types (spike `linedata.rs` as seed); `LayoutOptions` (incl. `hyphenation_loaded`), `LayoutError`, `LayoutLimits`/`Budget` (input-proportional, K16); `Layout`/`LayoutPage`/`LineGeom`/`LineStory` (HF slot, K13), `ImageRef::Derived` (K12), the Write geometry section (K14) + the uo-render item types (with the uo-render planner), `FaceTable`; `json_key`/`json_eq` scaffolding (K9); the TS canonical walker, the provider transcript with answer identity, the same-run capture script (K17), the Rust replay test and `Canon` | — (new) | **uo-docx D1a types** (A2/A4 + the requirements below), uo-render item types |
| **P** *(outside the crate; Sonnet-eligible, closed scope)* | font registry and policies | uo-fonts `registry` (`FontRegistry`, `FaceCache`, profiles, `Face::heap_bytes()`), policies `viewer` (viewer fonts.ts:90–137), `server-substitute` (C# `SubstituteFontProvider.cs`, C# reference), `oracle` (node-fonts.mjs:162–181); `uo_fonts::system` (C# `SystemFonts.cs`) | ≈ 600 (TS/C#) + design of LA6 | F0 (face identity contract) |
| **U** *(outside the crate; Sonnet-eligible)* | uo-text Unicode tables | NFC composition, `\p{M}`, JS `\s` split/trim over `u16`, `toUpperCase`/`toLowerCase` with SpecialCasing over `&[u16]` (K10), generated from Node 24.15 with the regeneration diff test | generator + tables | — |
| F1 | fonts and measuring | `fonts.ts`, `context.ts`, `deviceEmPt`, **the `SfntFont` kerning scale `kernTwips`/`kernGlyphsTwips`** (K7), the 600-DPI width model of `para.ts` (`advanceTwips`, `wordWidth`, `scaledUnits`, `toWordUnit`, `spacingTwips`), `math.ts` `parseFont` | ≈ 920 | F0, P (registry API) |
| F2 | Word semantics | `resolve.ts`, `paraformat.ts`, `directRPr`/`directPPr`, `resolvePPr`, `numbering.ts`, `datefmt.ts`, `field-formula.ts` (depth-guarded `parseArithmetic`, K11), `numpages.ts`, `outline.ts`, `revisions.ts`, the `documentHyphenationLanguages` walk as an explicit site list (K5) (placement LQ3) | ≈ 1,400 | F0, U (case mapping) |
| F3 | drawing values and metafiles | `geometry.ts` (layout half), `pictureLook`, `resolvePageFill` + `patternRowsFromImage`, `resolveTextFx`, `emf.ts`, `wmf.ts` (derived images, K12) | ≈ 1,400 | F0, F1 (faces), uo-drawingml |
| F4 | paragraph preparation | `para.ts` (minus F1's width model), `hyphenate.ts`, `math.ts` layout | ≈ 1,650 | F1, F2, F3 (`resolveTextFx`), U |
| F5 | lines and paint | `lines.ts` (spike seed), `underline.ts`, `paint.ts` | ≈ 1,380 | **F1** (`advanceTwips(ctx, …)`, lines.ts:243; hyphenation through `ctx`) — not only F0 |
| F6 | tables | `table.ts` | ≈ 1,410 | F4, F5 (through `CellLayouter` + `Cx`) |
| F7a | engine core | `engine.ts`: pass loop, balancing (last-executed result, K19), `run`, page/section/column geometry, `placeParagraph`, `layoutStatic`, keep rules, border groups, line numbers; `frames.ts`, `chain.ts` (`derive.rs`, frames lifted in every story, K3) | ≈ 1,900 | F4, F5 |
| F7b | engine: tables and floats | `placeTable`, floating/late tables, row emission, `placeFloats`, `wrapFrame`, objects, textboxes and chains | ≈ 1,500 | F6, F7a |
| F7c | engine: notes, headers/footers, fields | notes, separators, note columns, endnotes, `layoutHeaderFooter`, SEQ/REF/NOTEREF/STYLEREF/PAGEREF/chapters | ≈ 1,000 | F7a |
| F8 | editor-facing API and oracle | `refs.ts`, `run-boxes.ts` (+ props and `measure_text`, K17e), `caretXIn`/`offsetAtX`/`locateRef`, hit tests, selection rectangles, `page_of`, `navigation`, `collect_fields`, `next_hyphenation`; the `LayoutOracle` impl in the bindings; `ViewTransform` (LQ2) | ≈ 400 + new | F7a–c |
| (M2) | incremental relayout; uo-docx session `fontUsage()` port (LQ15, uo-docx D2) | LA7 levels 1 and 2, differential tests — **both done** (level 1 without the line memo, D-LA7-26; "Stage M2 — incremental relayout (LA7)") | new | F7, D2a |

**Order and parallelism** *(revised after the skeptic)*. The facet inputs are model objects and no
capture → model decoder is planned, so **every parity facet needs the uo-docx D1a reader at digest
parity first**; F0 needs only the D1a *types*.

```
D1a types + uo-render item types ──► F0 ──┬─► P, U (Sonnet, parallel, outside the crate)
                                          ├─► F1 ─┬─► F3 ─┐
                                          ├─► F2 ─┤       ├─► F4 ─► F6 ─┐
                                          │       └─► F5 ─┘             ├─► F7a ─► F7b ∥ F7c ─► F8
D1a reader at digest parity ─────────────────────── (wave-1 parity starts here) ───────────────►
```

Wave 0: F0 alone. Wave 1: F1 and F2 (plus P and U outside the crate); then F3 and F5 (both need F1).
Wave 2: F4, then F6. Wave 3: F7a, then F7b ∥ F7c (separate `impl Engine` modules on one struct),
then F8. Code can be written before the D1a reader is green; facet parity cannot. Memory and fuzzing
run continuously from wave 1; P1 from F7a. uo-render's codec and uo-pdf proceed in parallel with
waves 1–3 against the F0 item types. uo-drawingml phase 2 is untouched (its raster/effects are used,
not modified).

### 10. Estimate

*(revised after the skeptic: the first figure, 55–80 h, is a floor — it left out K1/K2/K12/K13 type
work, K6, K9, K10 and the P4 harness)*

| Part | Hours (agent time, through gate 2, before maintainer latency) | Measured or assumed |
|---|---|---|
| F0 foundation + harness (types incl. K1/K2/K12/K13/K18, walker with answer identity, transcript, same-run capture, replay, `Canon`, `json_key` scaffolding) | 14–18 | assumed; uo-docx A6/A7 harness work is comparable |
| F1–F8 port, ≈ 12.6k lines on Opus | 16–24 | **measured basis**: uo-opc's Opus port ran at ≈ 1,300 lines/h port-only (33 min / 730 lines); **assumed**: layout is denser, 520–800 lines/h |
| P (registry, policies, `system`) and U (uo-text tables, case mapping), with their facets | 8–12 | assumed; closed scope, Sonnet-eligible; the C#-reference facet for `server-substitute` is new harness |
| parity rounds (rule-dense module, 2,742 oracle + 2,000 generated + sweeps) | 18–28 | assumed; **measured** only for the line breaker (spike 2: 0 mismatches on 13,353 lines, first run) and for Sonnet ports elsewhere (0 rounds in uo-fonts/uo-opc, defects from plan gaps); the engine's pass state is the risk |
| editor API + oracle + view transforms (F8) + P4 harness for editor-private functions | 7–10 | assumed |
| memory and security verification, fuzzing | 10–13 | **measured basis**: uo-fonts stage 4 ≈ 160 min for 2.7k lines; scaled ×4–5 and Opus: assumed |
| **working band** | **≈ 75–105 (midpoint ≈ 90); floor ≈ 55** | the uo-docx record's hybrid figure gave layout ≈ 30–35 h of its Opus share; this plan adds the harness, the provider policies, the Unicode tables, the painter boundary and the editor API |

Critical path ≈ 45–60 h, **plus the wait on the D1a reader reaching digest parity** (wave-1 parity
cannot start before it), which this module does not control. Not included: the M2 incremental
relayout (≈ 10–15 h) and the session `fontUsage()` port (≈ 1 h), uo-render and uo-pdf, the TS
painter rewrite and the shell tiling of the tall web page for the editor's web/draft/outline views
(LQ2), real-Safari/BrowserStack runs, maintainer latency. The first Write-specific rate will come
from F0–F2 process metrics; this table is re-based on them.

## Milestone-1 gate — consolidated questions (G15, G19, G21, LQ1–LQ22)

*(revised after the skeptic: one list; the skeptic's revisions (section "Revisions to the milestone-1
questions") and LQ15–LQ22 merged; final recommendation per question.)* Batched into the milestone-1
gate with uo-docx G1–G27. **When**: **F0** = must be decided before F0 starts (it fixes types or the
harness); **W1** = before wave-1 parity; **M2** = with milestone 2; **U** = in the uo-render / uo-pdf
plans (this plan only needs the stated constraint).

| # | Question | Final recommendation | When |
|---|---|---|---|
| **G21** (uo-docx) | `LayoutOracle` shape | **Two methods, not four**: (1) `field_values(view, scope) -> Vec<(RunRef, Utf16)>` with the TS semantics (last value per run, first-set order), **including model runs inside copied paragraphs** (K1); sliced and frame runs absent; `FieldScope` = all \| a set of `RunRef`s; (2) `next_hyphenation` as A4. **`pages_of` dropped** (TOC/PAGEREF numbers come through `onField`; `bookmarkPages()` has no editor call site; find results use `Layout::page_of`). **`font_usage` leaves the oracle** (LQ15). **No `InlineMeasurer`** (LQ1). The binding reuses its last `Layout` when `formatting_version` and the story roots are unchanged. | F0 |
| **G19** (uo-docx) | legacy canvas views | **= LQ2** | F0 |
| **G15** (uo-docx) | codec shared with uo-render | **Own schema-driven generator** (one schema → Rust types and the TS encoder/decoder; little-endian, varints for counts and ids, f64 coordinates (LQ5), UTF-16 strings, interned symbols per layout), shared by the session `exec`/`query` and the display list; pages encoded on demand with a resource-table delta (faces, images incl. derived ones, symbols). This plan needs only plain-data items, per-page encoding and stable ids. | U (constraint fixed now) |
| LQ1 | Drop uo-docx A4's `InlineMeasurer` | **Yes.** Corrected evidence (K8): `InlineRenderer.measure` is called only through the editor's `buildSpans` (legacy layout, `fontUsage()`, `effectiveTbHeight`), never by the engine, and is dead after LQ2; plugin API change in the CHANGELOG. | F0 |
| LQ2 | G19: legacy canvas layout (web/draft/outline views, XPS export, `layout="legacy"`) | **Drop it; port nothing of `breakRich`/`paginate`/`lineHeightPt`.** Web = the engine over the viewer's `webDocument` transform (views.ts:28); Draft = the same at the section's text width; Outline = an engine transform designed with milestone 2; XPS from the display list (milestone 2). Header/footer and textbox editing already run on the engine. Also removed (K8): the engine-failure fallbacks (canvas `engineFailed`, `exportPdf` → `exportPdfCanvas`) — **a `LayoutError` now means no page and no PDF**; `fontUsage()` moves (LQ15); the editor's tall web page needs shell tiling (TS work, not estimated). Accepted by name, CHANGELOG. | F0 |
| LQ3 | Placement of the Word-semantics files (`resolve.ts`, `paraformat.ts`, `directRPr`/`directPPr`/`resolvePPr`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `outline.ts`, `numpages.ts`, revision colours, the hyphenation-language walk) | **`uo_docx::word`**, ported by F2 (Opus); the editor uses them without layout. (Skeptic note, not recommended: `numbering.ts`/`datefmt.ts` would then be uo-docx code, Sonnet-eligible if the maintainer reclassifies them.) | F0 |
| LQ4 | Incremental relayout: criterion = bit-equal to a full layout; the TS fast path's transient output not reproduced; milestone 1 ships `relayout` as a full layout; levels 1–2 in milestone 2; `Engine` classified and checkpointable now (K18) | **Yes** | F0 (shape) / M2 |
| LQ5 | `f64` in the display list and its encoding; `f32` only as a stage-5 candidate by pixel compare | **Yes** | F0 |
| LQ6 | Painter boundary: the TS canvas painter only decodes; everything derived from model objects or fonts resolved in Rust; shared raster helpers in `uo-render::paint` exported to wasm; **per item field, which painter's colour parse is the reference** (canvas `HEX6` skip vs PDF `parseInt` → NaN, K14); Word-only flags (`lineNum`, cull group, `ClipItem.src`, `bdr*` names) in the Write geometry section; painter parity = equality of a recorded canvas draw-call log, with `image()` stubbed and `'fontKerning' in ctx` fixed (it selects the batching path, canvas.ts:350) | **Yes** | F0 (item split) / U (details) |
| LQ7 | EMF/WMF renderer placement | **`uo-docx-layout::metafile` now** (`trait FaceLookup`; derived images per K12); moves to a shared crate with Slides' EMF path; uo-image keeps the codecs only (corrects the uo-docx estimate table) | F0 |
| LQ8 | Fonts: (a) uo-fonts `registry` (`FontRegistry`, `FaceCache`, `Face::heap_bytes()`) with **named provider policies** `viewer`, `server-substitute` (C# reference), `oracle` (test), `editor` (M2), each with a facet, `FaceId` = answer identity (K2a, K6); (b) **host-configurable face budgets**: `desktop` wasm32 128 MiB pinned / 64 MiB cache, native 1 GiB / 512 MiB; **`phone` 48 MiB / 16 MiB**; the phone M3 target counts JS and wasm face bytes (K15); (c) `uo_fonts::system` ported in milestone 1 for the server PDF; (d) the .NET facade keeps `HyphenationLanguages`, `Now` and the substitute policy, drops or wraps `IFontProvider`/`PdfFontSource` (CHANGELOG); (e) `SfntFont` advance tables decided in the uo-pdf plan (K7) | **Yes to all**; (c) alternative: the C# facade enumerates and feeds bytes for one more release | F0 (a, b) / W1 (c, d) / U (e) |
| LQ9 | No-panic policy: `deny(clippy::indexing_slicing)`; a TS `x[i]!` throw → `Err(Internal)`; a TS `undefined` read → `get()` with the TS fallback, declared per site | **Yes** | F0 |
| LQ10 | Dates: `LocalDateTime` + `TimeZoneRule` from the binding's `Env`; captures under `TZ=UTC` plus one São Paulo facet | **Yes** | F0 |
| LQ11 | uo-text in milestone 1: NFC for hyphenation clusters, `\p{M}`, JS `\s` split/trim over `u16`, **`toUpperCase`/`toLowerCase` with SpecialCasing over `&[u16]`** (K10: `uo_jsnum::text` has no case mapping and works on `&str`), generated from Node 24.15 with a regeneration diff test; frontier U (Sonnet-eligible) | **Yes** (collation stays milestone 2) | F0 (scope) / W1 (tables) |
| LQ12 | Drop the lab-only options (`onWordFit`, `advanceEm: 'device'`, `advanceRound` ≠ `'word'`, `justifyShrink` override) | **Yes**, CHANGELOG | F0 |
| LQ13 | Recursion: TS recursion kept, bounded by `max_nesting` 64 and a layout depth guard; wasm stack sized by measurement (≤ 8 MiB) with a depth-64 test; **a depth guard on `parseArithmetic` and on every recursive parser over untrusted text** (K11c); explicit stack only if 8 MiB is not enough (stage 5) | **Yes** | F0 |
| LQ14 | `LayoutLimits` outcomes: a limit → `Err(Limit(kind))`; metafiles and field texts truncate with a warning; faces past the budget fall back; **"TS throws `RangeError`" accepted by name** (large spreads, `repeat`, K11); the work budget **proportional to the input** (K16), with the stress row "500 pages × 20 balanced sections" required to pass | **Yes** | F0 |
| LQ15 | Font usage (save-time embedding plan, fonts to load before a PDF): (a) literal port of the editor's model-only `fontUsage()` in the uo-docx session; (b) engine-based with the embedding change accepted by name | **(a)** | F0 (oracle shape) / M2 (port) |
| LQ16 | Hyphenation availability as an input (`hyphenation_loaded`), computed by the binding through an explicit `documentHyphenationLanguages` walk with its own facet; the production app list `['pt-BR','en','es']` in the sweeps (K5) | **Yes** | F0 |
| LQ17 | F0 type additions: shallow `LPara` + `RunSrc` (K1); the K2 identity rows (face answer identity, `CellKey`, `pinnedFloats`, `chainSlices`, line identity in row splits, `nest` by index, `ContainerId`); HF slot in `LineStory` (K13); `ImageRef::Derived` (K12); `Cx` and the Engine field classification (K18) | **Yes, before F0 closes** (done in this revision at the plan level) | F0 |
| LQ18 | TS cross-call caches (frames, `CTX`): Rust state per call, the TS staleness fixed and accepted by name; the milestone-2 TS reference is a fresh read per edited state (K3) | **Yes** | F0 |
| LQ19 | `FaceResolver` cache per layout call; P2 compares the first request per lower-case key and the answer identity (K4) | **Yes** | F0 |
| LQ20 | `json_key`/`json_eq` with the JS JSON rules; key order from the uo-docx insertion-order facet, or a counted acceptance where order changes the TS result (K9) | **Yes** (needs the uo-docx A6 order facet, or the counter) | F0 |
| LQ21 | Parity gate logistics: the P1 TS capture regenerated in the gate run on the same machine; face drift **fails** the gate; CI layout parity = generated documents only (the oracle has no open-font-only document); the document read again per option run (K17, K3) | **Yes** | F0 (harness) |
| LQ22 | Order and estimate: F0 after the D1a types; **every parity facet after the D1a reader reaches digest parity**; F5 after F1; P and U outside the crate in parallel; working band **≈ 75–105 h** (floor 55), critical path ≈ 45–60 h plus the D1a wait | **Maintainer — confirm** | F0 |

**Decide before F0 starts:** G21, G19/LQ2, LQ1, LQ3, LQ4 (shape), LQ5, LQ6 (item split), LQ7, LQ8 (a, b),
LQ9, LQ10, LQ11 (scope), LQ12, LQ13, LQ14, LQ15 (oracle shape), LQ16, LQ17, LQ18, LQ19, LQ20, LQ21,
LQ22. **Before wave-1 parity:** LQ8 (c, d), LQ11 (tables). **Later:** G15 details and LQ6 details and
LQ8 (e) in the uo-render/uo-pdf plans; LQ4 levels and LQ15's port with milestone 2.

**Requirements on uo-docx** (for its D1a types; not questions): `DocView` always presents the
reconciled block order (never a stale `CellView`); `RunRef { story, path, run: u32 }` defined next to
`DocRef`; `read_embedded_fonts` returns `Arc<[u8]>` per face; `Paragraph`/`Run` constructible by
this crate (synthetic paragraphs, LA2); the `StyleSheet` iterates in TS object order (integer-like
`styleId`s first); `formatting_version` also bumps on font-table, theme and section changes.
*(revised after the skeptic)* Also: **every table has a `TableId` and every cell is addressable by
(`TableId`, row index, cell index) through `DocView`** — the layout keys the TS `cellNb`
`WeakMap<TableCell>` by it (K2b), so A2 must not leave cells anonymous inside nested `Story`s;
`RunRef` addresses every model run reachable from a copy (runs taken from other paragraphs by
`joinDeleted`, K1); the header/footer accessors expose the slot → part mapping per section (K13);
the A6 canonical stream keeps the props' key insertion order as a facet usable by `json_key` (K9,
LQ20); the uo-docx `word` module includes the `documentHyphenationLanguages` walk as an explicit
`lang`-site list (K5); the session gets the literal `fontUsage()` port in milestone 2 (LQ15).

## Analysis skeptic (verified in the code)

Stage 2, Opus, 2026-11-03. Every claim below was checked in the frozen TS (`write/packages/docx-layout/src`,
`editor/src/universal-write.ts`, `viewer/src`, `write/tools/corpus-lab`, `tools/gen-parity-layout.mjs`),
the C# (`dotnet/src/UniversalOffice.Docx/DocxLayout`, `Export/DocxPdf.cs`), the clickCompliance host
call, and `core/crates/uo-jsnum`. Regex sites were counted with the TypeScript parser. Line numbers
are of the `rust-core` worktree. Claims that held are listed in K20 and not repeated.

| # | Finding (evidence) | Blocking | Proposed decision |
|---|---|---|---|
| K1 | **TS paragraph copies are shallow; LA2's `Synth(Box<Paragraph>)` is deep.** The anchor copy `{ ...base, runs: [...base.runs, run] }` (frames.ts:121), the revision view `{ ...para, ppr, props }` (revisions.ts:124, 126), `joinDeleted` `{ ...next, runs: [...carried, ...next.runs] }` (revisions.ts:157, with runs taken **from other paragraphs**) and the frame inner copy `{ ...p, ppr }` (frames.ts:50) all keep the **model run objects**. So `onField(run, v)` (para.ts:549) reports model runs from copied paragraphs, and the editor finds them (`only.has(r)`, universal-write.ts:6462); a floating object anchored in a copy has the model run as `PlacedObject.run` (`h.run === run`, 10701, 13541). The plan's "absent from `field_values`" holds only for sliced runs (`{ ...r, text }`, chain.ts:57). `REV_JOINS` is a symbol key on the joined paragraph (revisions.ts:151), copied by later spreads. A deep `Paragraph` clone loses run identity and copies every run's UTF-16 text on each copy. | **yes** (F0 type) | `LPara { src: Model{para, id} \| Copy{base: LParaId}, ppr/props override: Option<Arc<…>>, runs: Vec<RunKey>, rev_joins: SmallVec<[u32; 1]>, flags }`, with `RunKey → RunSrc::Model(RunRef) \| Synth(Box<Run>)` (only slices and frame runs are synthetic). A copy has no `ParaId`, but its model runs keep their `RunRef`. No text is copied. |
| K2 | **Identity sites that LA3 misses.** (a) **`FontFace` objects**: kerning, ligatures and run merging apply only across the *same face object* (para.ts:488, 524, 1016; math.ts:252). The TS objects come from provider caches: the viewer keys them case-sensitively (`${family}\|bi`, viewer fonts.ts:94), node-fonts keys them in lower case, and C# `SubstituteFontProvider` returns a fresh stand-in object per call. LA6's FaceTable dedupes by content. (b) `cellNb: WeakMap<TableCell>` (engine.ts:1560). A2 cells have no id. (c) `pinnedFloats: Map<Run, {ps: PageState}>` (2891) points at earlier pages. (d) `chainSlices: Map<TextboxChain, Map<DrawingTextbox, CellContent>>` (3263). (e) **Lines have identity too**: `lastOf.get(last.ref) === last` (table.ts:124). `moved`/`own`/`keep` hold items **and** `LaidOutLine`s (table.ts:130–164, 1356). `CellContent.nest` aliases objects of `c.items`/`c.lines` and is remapped through `moved` (141–145). (f) `boxPage` is the **items array** the box went into (`boxPage: items`, engine.ts:1398; `page?: object`, 1687), a container that can be a cell or a static frame, not a `PageIx`. | **yes** (F0 types) | Add rows to LA3. (a) `FaceId` = identity of the provider's answer: the transcript records "same object as answer j", not only file + digest. (b) `CellKey(TableId, row, col)`. (c, d) keys + `PageIx`. (e) `ItemMeta` also covers lines; `nest` stores index ranges, not copies. (f) `ContainerId` (page body \| static frame). |
| K3 | **Cross-call module state.** `FRAME_SKIP`, `FRAME_CARRY`, `INNER`, `runCache` and `anchorCache` (frames.ts:21–30) are module `WeakMap`s keyed by **model** paragraphs. They live across layout calls: `runCache` returns the frame built at the first call, and `anchorCache` appends `[...prevRuns, run]` and rebuilds from its own cached copy `c.para` (frames.ts:116–123). The viewer's model never changes, so this is invisible in milestone 1. In the editor, an in-place edit (G22 keeps the id) can show a stale frame or anchor text, or a duplicate frame. The same holds for `CTX: WeakMap<DocFormatting, DocContext>` (paraformat.ts:7: style-resolver caches across calls, LQ3 code). `liftFrames` also runs in **every non-cell `layoutStatic`** (engine.ts:1204: headers/footers and textbox contents, per page, probe and pass), not only for the body (193). | no | Rust state is per call: TS staleness is fixed and the fix is accepted by name. `LDoc` lifts frames for every story when it is built. The milestone-2 differential ("Rust full == TS full on edited documents") takes the TS reference from a fresh read of each edited state, never from the live editor objects. |
| K4 | **P2 compares a call sequence that LA6 cannot produce.** `DocContext`, and with it `FaceResolver` (context.ts:112), is new **per pass and per balancing probe** (`newCtx()`, engine.ts:214–217, 225, 256). The provider is therefore asked again in every pass. LA6 keeps the resolver cache per document. | no | Keep the `FaceResolver` cache per layout call, on `LayoutRun`. P2 compares the first request per lower-case key and the answer identity (K2a). Reuse per document only as a milestone-2 optimisation, behind the registry generation. |
| K5 | **Hyphenation availability is process state and an input.** `loaded` (typography/hyphen/index.ts:47–80) decides whether a language hyphenates. The viewer loads `hyphenationLangs(full)` before laying out (viewer:605). That list is `documentHyphenationLanguages`, a reflective `Object.entries` walk over the whole model (hyphenate.ts:66–80), intersected with the app list `['pt-BR','en','es']` (viewer:28, editor:308). The editor loads asynchronously and lays out again (16098–16107). clickCompliance passes `HyphenationLanguages` to the server PDF (`UniversalOfficeWordToPdfConversionService.cs:34–39`). `oracle.test.ts` loads all 7 and passes no list. With patterns built in, Rust would hyphenate whatever the walk missed. | no | `LayoutOptions.hyphenation_loaded: Option<Set<PatternId>>`, computed by the binding as the shell does; the captures record it. Port the walk as an explicit list of every `lang` site (runs, style rPr, revision `change` records, textboxes, `fieldResult`…), with its own facet. Add the app list to the sweeps (it is the production setting on all three hosts). |
| K6 | **Font provider policies are out of scope, and one exists only in C#.** The registry push model (LA6) replaces four `provider.face` policies. Viewer `fonts.ts:90–137`: variant fallback order, a non-embeddable licence gives the metric substitute, `standIn`, `source === 'default'` gives `null`. Editor `liveFontProvider` (16087): cache across calls by `fonts.version`. C# `SubstituteFontProvider.cs`: system face, else a `FontCatalog.MetricSubstitute` stand-in, used by clickCompliance. node-fonts.mjs:162–181: score pick (the oracle). The public .NET API also takes a host `IFontProvider` and `List<PdfFontSource>` (Export/DocxPdf.cs:24–39). P2 replay checks the engine side only. "Nothing exists only in C#" does not hold. | no | LQ8 revised: the registry implements named policies (`viewer`, `server-substitute`; the editor's in milestone 2), each with its own parity facet: the TS viewer provider over a stub resolver, the C# provider over a fixed index (C# is the reference here, frozen). The .NET facade keeps `HyphenationLanguages`, `Now` and the substitute policy, and drops or wraps `IFontProvider` (CHANGELOG). |
| K7 | **The twips scaling of `SfntFont` and the advance tables were handed to this crate (uo-fonts.md:51) but are not in F1.** `face.font.kernTwips` (deviceEmPt scaling, sfnt.ts:198–213) is live in the engine (para.ts:539–543). `setAdvanceTable` (sfnt.ts:386–399; loaded by viewer fonts.ts:66–67 and editor 1822, 1866) does not reach the engine's width model (para.ts:378–405 uses `advanceWidth`), but `SfntFont.advanceTwips` with the table drives uo-pdf's word-space insertion (pdf-word-spaces.ts:24). | no | Kern scaling goes into F1. The advance table goes to the uo-pdf plan: either the registry attaches it per face, or it is dropped with the change in PDF text extraction accepted by name. |
| K8 | **LQ1 and G19 evidence is incomplete.** `InlineRenderer.measure` **is** called by the editor: `buildSpans` (universal-write.ts:10893–10918) serves the legacy layout, `fontUsage()` (2319–2335) and `effectiveTbHeight` (12841; also called during textbox hit tests under the engine, 13399, result overridden). `fontUsage()` (ascii family via `fontAscii ?? style ?? docDefaults`, no notes, no east-Asian/cs slots) drives the **save-time font embedding plan** (2302) and `settleDocumentFonts` before a PDF (3336–3341). Dropping the legacy layout also removes the **engine-failure fallbacks**: canvas `engineFailed` (1328, 16161) and `exportPdf` → `exportPdfCanvas` (22365–22372). The comment at 15971 ("HF/textbox editing stay legacy") is stale: `engineCanRun` (15988) does not gate them. That claim of the plan holds. | no | LQ1 stays (`measure` is dead after G19; plugin API change in the CHANGELOG). Font usage needs no layout, so take it **out of `LayoutOracle`**: a literal port of `fontUsage()` in the uo-docx session (recommended), or an engine-based usage with the embedding change accepted by name (LQ15). State in LQ2 that a `LayoutError` now means no page and no PDF. |
| K9 | **`JSON.stringify` keys and equality.** `bdrKey` (engine.ts:4089) groups paragraph borders by `JSON.stringify([pBdr, ind])`, and `pBdr` is built by `{ ...base.pBdr, ...over.pBdr }` (resolve.ts:82), so the grouping depends on key insertion order. Also `fxKey` (para.ts:249), the shaping key with `JSON.stringify(r.ot)` (476), `eq` (context.ts:217) and the old-formatting comparisons (revisions.ts:111, 122). JSON rules: insertion order, `undefined` keys omitted, NaN/±∞ → `null`, −0 → `0`. `derive(PartialEq)` gives the opposite result for NaN and ignores order. | no | One `json_key`/`json_eq` per type with the JS rules. Key order comes from uo-docx's insertion-order facet (A6/G23), or is accepted by name with a capture counter for the cases where order changes the TS result. |
| K10 | **`uo_jsnum::text` has no case mapping, and it works on UTF-8.** It exports JS whitespace, `trim`, `split_js_space`, `utf16_len` and `utf16_slice` over `&str` (text.rs:13–105). §8.5 and LQ11 assume `toUpperCase` there. Layout needs `toUpperCase`/`toLowerCase` with SpecialCasing over `&[u16]` with lone surrogates: caps (para.ts:749), small caps with `toUpperCase().length === 1` (789–820), and the `ß` → `SS` length change that moves offsets. | no | Add a UTF-16 case mapping and JS `\s` split/trim over `u16` to uo-text (LQ11), generated from Node 24.15 with the regeneration diff test. |
| K11 | **V8 throws where Rust would not.** (a) Argument spreads over large arrays throw `RangeError` above about 10^5 elements: `items.push(...floatItems)` (engine.ts:1495–1496), `Math.min/max(...)` (3645, 4031–4032; table.ts:129, 153), `lines.push(...)` (table.ts:1339), `path.push(...poly(pts))` (emf.ts:289, 314). (b) `'*†‡§'[…].repeat(…)` (numbering.ts:63) throws on huge counters. (c) **`parseArithmetic` recursion** (field-formula.ts:93–117) is unbounded on `((((…` or `----…` in a field code. The `RangeError` is rethrown (126), so `layoutDocument` throws; in Rust a stack overflow aborts the wasm instance. Field codes are run-level, so `max_nesting` 64 does not bound them. §7 says Rust differs only where the TS hangs or runs out of memory. | no | Add "TS throws `RangeError`" to the outcomes accepted by name (LQ14). Put a depth guard on `parseArithmetic` and on every recursive parser over untrusted text (LQ13). |
| K12 | **The layout creates images.** EMF bitmap records become BMP bytes (emf.ts:171–195, 465–503), EMF+ compressed images are copied (474), and `resolvePageFill` builds a new `InlineImage` (page-fill.ts:34). These are new objects per pass and per call. LA5's `ImageRef { media: MediaId }` cannot express them. | no | `ImageRef::Media(MediaId) \| Derived(Arc<[u8]>, ext)`, deduped by content per `Layout` and counted in M3. |
| K13 | **Header/footer line refs name the slot, not the part.** The refs are `hdr:${kind}`/`ftr:${kind}` (engine.ts:1157–1185), and one part can serve several slots. `StoryRef::HeaderFooter(HfPartId)` cannot print the TS ref the canonical stream needs. | no | The `LineRef` HF variant carries (header \| footer, slot) plus the part. |
| K14 | **The display list carries Word and DrawingML decisions, and colours mean different things per painter.** `lineBox.line` drives `cullOffPage` (engine.ts:4005–4038), `lineNum` keeps line numbers off-page in the PDF, `bdr*` dash names (tree.ts:63, `BORDER_DASHES`), `asPath`/`colr`, `ClipItem.src` (the editor finds the EMF by its item), `PictureLook` preset strings. Colours: the canvas skips non-`HEX6` strings (canvas.ts:32, 131, 159, 223, 320). The PDF parses any string through `hexColor` → `parseInt`, so `'auto'` gives NaN components (pdf-writer/document.ts:44–51). The engine emits raw document strings (page background, engine.ts:759). | no | Verdict: interned TS strings are the right contract for parity (the plan holds). The uo-render plan states per item field which painter's parse is the reference, and puts Word-only flags (`lineNum`, cull group, `src`) in the Write geometry section, not in the shared item. |
| K15 | **The memory numbers contradict each other.** The wasm32 face budget of 128 MiB plus the 64 MiB cache (LA6) exceeds the 160 MiB phone high-water target (M3), and wasm memory never shrinks. The canvas painter also needs the same faces as browser `FontFace`s (canvas.ts:321–323 via `fontOf`), so face bytes live both in JS/browser and in wasm. | no | Host-configurable budgets, with a phone profile (proposal: 48 MiB pinned, 16 MiB cache). The phone M3 target counts JS and wasm face bytes, and the shell drops its fetch buffer once the face is registered. |
| K16 | **A work limit "above the corpus maxima" will fail legitimate large documents.** The oracle documents are small. A 500-page document with balanced continuous sections runs up to 5 passes plus min(N, 20) × (log₂ h + 2) full passes (engine.ts:224, 263–290). | no | Budget proportional to the input (units per prepared glyph × the passes allowed). Stress row "500 pages × 20 balanced sections". |
| K17 | **Parity logistics.** (a) The oracle has **no open-font-only document**. A scan of the 2,742 docx: Arial referenced in 2,179, Calibri 333, Times New Roman 183. "CI runs the open-font subset" of the oracle is empty, so CI layout parity means the generated documents only. (b) Stored captures plus a Windows font update give mass `font drift` skips, which would make the gate vacuous. (c) P4's `bookmarkPages()` is internal to the engine (engine.ts:638; only the pass loop calls it). TOC and PAGEREF page numbers reach the editor through `onField` (universal-write.ts:6454–6500), so G21's `pages_of` has no TS call site to capture. (d) Read the document again for every option run (K3). (e) In the viewer, `paintCustomInlines` (stays TS) reads `run0.face.font` and hands the **model `RunProps`** to plugins (inline-renderer.ts:63–80); after the switch the shell has neither. | no | (a) Say so in §5. (b) Regenerate the P1 capture in the same gate run on the same machine (TS, then Rust); drift fails the gate instead of skipping. (c) Define `pages_of` from a real editor call site, or drop it from G21. (e) `run_boxes` returns the props the plugin contract exposes and a per-`FaceId` measure export. |
| K18 | **The Engine borrow shape.** `trait CellLayouter` implemented by `Engine` plus `tables: Arena<TableLayout>` inside the `Engine` gives `layout_table(&mut self.tables[i])` calling back into `&mut self`. In the TS, `TableLayout`s are locals; `floatFlow` is a side `WeakMap`. Engine state is about 60 fields declared across engine.ts:326–3261, some of them pointing at earlier pages (`pinnedFloats`, `lnPage`, `lastBox`), so "`Clone` + `PartialEq` over its flow state" (LA1.4) is not yet a field list. | no | `TableLayout` stays owned by the stack frame; the `Engine` keeps only `TableLayoutId → side data`. A `Cx` parameter (`&LDoc`, `&mut FaceTable`, `&mut Budget`, hyphenation sessions) goes to the `Engine` methods. F0 sorts every field into output, carried flow state or cache. |
| K19 | **"The final result is the last pass" means the last *executed* pass.** When `!ok(hi)`, the result is the rejected `trial(hi)` layout, while `last` stays the previous accepted engine (engine.ts:255–283). | no | Port it literally, and keep the work budget and the milestone-2 checkpoints on "last executed". |
| K20 | **Counts.** Verified: every line count in the scope table; 259 groups and 2,742 docx; `engineCanRun` (15988) gates only the view, `layout="legacy"` and failure; `layoutHfEdit`/`layoutTextboxEdit` run only when the engine cannot (12462–12475); the viewer's web view is `webDocument` (views.ts:28). Off: the §8 `Math` counts are for the whole `src` (canvas, pdf, images included). The ported files have 291 `max`, 127 `min`, 46 `round`, 31 `floor`, 18 `ceil`, 34 `sin`/`cos`, 3 `hypot`, 1 `tan`, 1 `pow`, 1 `exp`, 0 `clz32`, plus 4 `sqrt`. Regex sites: 97 (0 dynamic, 3 with `u`), not ≈ 80. Total ≈ 12.57k lines without tree.ts (12,847 with it), not 12,340. `zSeq` is an `Engine` field (372), not module state. `PFMT` sharing is never observed (the ruler reads values only, universal-write.ts:17135–17138), so the canonical stream can drop the "same object" groups. | no | Correct §8 and LA3. |

### Verdicts

- **Boundaries**: uo-render with string colours: **yes** (K14 adds the per-painter reference). uo-pdf
  independent of uo-docx: **yes**, provided `Navigation`, the derived images (K12), the faces (with or
  without advance tables, K7) and the colour rule travel in the resource table. LQ3 `uo_docx::word`:
  **yes**. LQ7 metafiles here: **yes** (`renderEmf` takes `DocContext` for faces and warnings only). LQ6
  painter-as-decoder: **yes**. The recording context must stub `image()` and fix `'fontKerning' in
  ctx` (canvas.ts:350), which selects the batching path.
- **Parity**: exact-bit P1 is feasible. It runs on one machine with Office fonts, with the TS capture
  regenerated in the same run (K17). Node is deterministic once `now`, TZ and the loaded patterns
  (K5) are fixed and the document is read again per run (K3). The editor's timers (1.5 s idle
  relayout, async patterns and fonts) never reach a Node P1/P3 capture.
- **Estimate**: 55–80 h is a floor. Not in it: the provider policies and their facets (K6, LQ8),
  the uo-text case mapping (K10), the F0 type additions (K1, K2, K12, K13), the JSON-equality helpers
  (K9), the P4 harness for editor-private functions, and the wait on the D1a reader. Working band
  **≈ 75–105 h** through gate 2.
- **Order**: F0 needs the D1a *types* and the uo-render item types. **Every parity facet needs the
  D1a reader at digest parity**, because facet inputs are model objects and no capture → model
  decoder is planned. So: D1a reader green → wave-1 parity. F5 depends on F1, not only on F0
  (`advanceTwips(ctx, …)` in lines.ts:243; hyphenation through `ctx`).
- **Models (note only; the maintainer's "uo-docx-layout inteiro em Opus" stands)**: some work outside
  this crate is already Sonnet-eligible under the hybrid rules and runs on the critical path: the
  uo-fonts `registry`/`system` and the provider-policy ports (K6), and the uo-text generator (K10).
  Inside the crate, `emf.ts`/`wmf.ts` record decoding and `numbering.ts`/`datefmt.ts` are the most
  mechanical parts. If LQ3 puts the latter in `uo_docx::word` they become uo-docx code, which a
  Sonnet assignment would only need the maintainer to classify. Not recommended here.

### Revisions to the milestone-1 questions

| # | Revision |
|---|---|
| G21 | `field_values` includes model runs inside copied paragraphs (K1); `FieldScope` is a set of `RunRef`s; **`font_usage` leaves the oracle** (K8, LQ15); `pages_of` is defined from a real editor call site or dropped (K17c). |
| LQ1 | Yes, with corrected evidence: `measure` is called through `buildSpans` (legacy layout, `fontUsage`, `effectiveTbHeight`) and becomes dead after G19 (K8). |
| LQ2 | Plus: the engine-failure fallbacks (canvas and `exportPdf`) disappear, so a `LayoutError` means no page and no PDF. `fontUsage()` moves (LQ15). The editor's web/draft/outline views need shell tiling of the tall web page (TS work, not estimated). |
| LQ6 | Plus the per-field painter reference for colour strings, Word-only flags in the Write section (K14), and the recording-context stubs. |
| LQ8 | Plus named provider policies with facets; the .NET facade keeps `HyphenationLanguages`, `Now` and the substitute policy (K6); host-configurable budgets with a phone profile (K15); advance tables decided with uo-pdf (K7). |
| LQ11 | Plus UTF-16 case mapping with SpecialCasing and JS `\s` over `u16` (K10). |
| LQ13 | Plus a depth guard for recursive parsers over field-code text (K11c). |
| LQ14 | Plus "TS throws `RangeError`" as an accepted outcome (K11); work budget proportional to the input (K16). |
| **LQ15** | Font usage (embedding plan, fonts to load before a PDF): (a) literal port of the editor's `fontUsage()` in the uo-docx session (model-only), or (b) engine-based, with the embedding change accepted by name. **(a)** |
| **LQ16** | Hyphenation availability as an explicit option (`hyphenation_loaded`), computed by the binding through an explicit `documentHyphenationLanguages` walk with a facet; the app list in the sweeps (K5). **Yes** |
| **LQ17** | F0 type additions: shallow `LPara` with `RunKey` lists (K1); the identity rows of K2 (face answer identity, `CellKey`, `pinnedFloats`, `chainSlices`, line identity in row splits, `nest` by index, `ContainerId`); HF slot in `LineRef` (K13); derived images in the resource table (K12); `Cx` and the Engine field classification (K18). **Yes, before F0 closes** |
| **LQ18** | TS cross-call caches (frames, `CTX`): Rust state per call, TS staleness fixed by name; the milestone-2 TS reference is a fresh read per edited state (K3). **Yes** |
| **LQ19** | `FaceResolver` cache per layout call; P2 compares first requests per key and answer identity (K4). **Yes** |
| **LQ20** | `json_key`/`json_eq` with JS JSON rules; key order from the uo-docx insertion-order facet or a counted acceptance (K9). **Yes** |
| **LQ21** | Parity gate: TS capture regenerated in the gate run on the same machine, drift fails the gate; CI layout parity on generated documents only (K17). **Yes** |
| **LQ22** | Order and estimate: D1a reader at digest parity before wave-1 parity; F5 after F1; working band ≈ 75–105 h (floor 55). **Maintainer** (confirm) |

## Stage 3 — port

Pending. Per frontier: wall time, parity rounds, and the declared sites (uo-docx A7 items 1–9, the
LA3 keys, every limit check and its TS behaviour).

## Verification (gate 2)

Pending.

### Parity

Corpus: … Mismatches: … Differential: …

### Memory (measured)

| Check | Result | Numbers |
|---|---|---|
| M1 no leak per operation | | |
| M2 soak (native) | | |
| M2 soak (wasm) | | |
| M3 budgets | | |
| M4 boundaries | | |
| M5 cache caps | | |
| M6 Miri / LeakSanitizer | | |

### Security

Milestone-1 gate fixes (2026-11-10, record `uo-bindings.md` "Milestone-1 gate fixes"). **Native stack**: the layout of nested tables uses about
19 KB of release stack per level (Windows x64, LTO): on a 1 MiB thread 52 nested tables pass and 56 overflow (`nest_tables_64.docx`, 1,508 B, aborts
the process; the reader's `max_nesting` is 64 and `LayoutLimits.max_depth` 192, so neither stops it). The C ABI `uo_docx_to_pdf` now converts on its own
16 MiB thread (64 MiB in debug), and 64 and 200 nested tables, 64 VML boxes and a 64-level mix convert from a 1 MiB caller thread (release and debug); wasm
lays the same four out (`write_hostile.test.mjs`). The per-level frame size is not reduced (maintainer decision G23). The rest of this section is still
**pending the security review** (collected as they are found):

- F2-21: the TS `documentHyphenationLanguages` is exponential in nested text boxes and tables
  (2^depth; 30 levels ≈ 25 min); the Rust walk visits each paragraph once (same output). The TS bug
  remains in the shell until the switch (not edited by the port).

## Evolution

Pending. Candidates: f32 coordinates (LQ5), level-2 incremental relayout tuning, the `pure()` flow
clones as persistent structures, explicit-stack nesting (LQ13), the format-2 kerning class tables
(uo-fonts D5).

## Final review and benchmarks (gate 3)

Memory and speed of the milestone-1 read path are measured below (2026-11-06 and 2026-11-10, Node and Chromium; WebKit only the wasm size and the
wasm2js speed rows). Open: the Rust server PDF against the frozen C#, WebKit timings, the editor keystroke budget in milestone 2.

### Benchmarks main × rust-core — memory (measured)

Measured 2026-11-06 on the milestone-1 tree (branch `rust-core`, Rust release / wasm `--profile wasm`, Node 24.15.0,
Chromium (build 1248) and WebKit (build 2311) from the local Playwright, Windows 11, a contended 20-thread machine; one measurement process at a
time). TS side: the frozen `d0f45bc5` (`UO_TS_ROOT`, `docx-core` + `docx-layout` dist) and the published TS viewer 0.9.0.
Harness: `core/tools/bench/memory/` (results of this run in `results/memory-2026-11-06.{json,md}` and
`results/browser-2026-11-06.json`), the Rust side in `crates/uo-docx-layout/examples/bench_memory.rs`.

**Documents** (all open fonts, `gen_docs.mjs` seeded, same bytes for the three engines; the page and character counts
below are equal in the TS and the Rust layout — parity of the generated documents): `small` = the committed fixture
`f8__7__0` (3 pages); `typical` (20 pages, mixed runs and fonts, header/footer with PAGE); `tables` (18 tables of 12 × 6
cells, 37 pages); `images` (40 noise PNGs of 320 × 240, 9.3 MB docx, 28 pages); `pages100` (100 pages, 399,589 laid-out
characters) and `pages500` (500 pages, 2,008,124 characters). "Characters" = the engine's per-character records
(`chars.o` rows; TS `lines[].chars` entries).

**How each number is taken**

| Number | Method |
|---|---|
| TS retained (`ts_mem.mjs`, one process per document) | `heapUsed + arrayBuffers` after four `gc()` with the outputs alive, minus a baseline taken after a warm-up pass of the same document (faces and GPOS caches filled, hyphenation loaded) followed by a pass of a tiny document (so no module state still holds the warm-up's pass). "read" = `readFullDocument`; "layout" = `layoutDocument` (the `LayoutResult`); the result is kept next to the model it points into |
| TS peak (`peak RSS − idle`) | `process.resourceUsage().maxRSS` minus the RSS of the bare process before the engine is imported. It includes the engine modules, the open font files and the hyphenation tables the TS keeps, and the warm-up pass: it is an upper bound of "the peak of one layout", not that peak |
| Rust retained / peak / allocated (`bench_memory`, `uo-alloc` counting allocator, one process per document) | after the same warm-up (face cache, hyphenation registry), `measure` of (a) open + read (document kept), (b) `derive::build` (the layout tree, kept), (c) `layout_document` (the `Layout` kept). Retained = `live_delta`; peak = highest live bytes above the start of the step; `call_peak` = derived tree + (c)'s peak. A reallocation counts as a new allocation before the old block is freed (uo-alloc), so peaks of growing vectors are an upper bound |
| wasm in Node (`wasm_mem.mjs`, one process per document, module built with `--features count,write`) | the viewer's sequence through `js/write.mjs`: env + 5 open families (20 faces, 7.0 MiB pushed), open, register the patterns the document asks for, layout, free, layout again (the second one is the retained `UoLayout`: live bytes after it minus live bytes after the first was freed, so the instance face cache is excluded), `encodePage` for every page (the first 5 as a viewer's visible pages), navigation, run boxes, the PDF in chunks, then `free()` of layout, document and environment. `memory.buffer.byteLength` is read after each step (high-water); `uo_memory_live_bytes` is the module's counted live bytes. The `count` build and the shipped build give the same `byteLength` (48,037,888 B for `pages100` in both) |
| Browser tab (`browser.mjs`, Chromium) | one browser per scenario; `Performance.getMetrics` `JSHeapUsedSize` after `HeapProfiler.collectGarbage` twice, `performance.measureUserAgentSpecificMemory()` (the server sends COOP/COEP, `crossOriginIsolated` is true) and the **private bytes of the renderer process** (CDP `SystemInfo.getProcessInfo` → `Get-Process`), the largest of the renderers, minus a blank tab of the same browser |

**Retained memory** (MiB; `layout` = the `LayoutResult` / `Layout`; the target column is the plan's §6)

| class | pages | chars | TS layout | Rust layout | Rust / TS | Rust B/char | wasm layout (`UoLayout`) | wasm B/char | TS read + layout | Rust read + layout | Rust / TS |
|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 3 | 5,430 | 0.95 | 0.31 | 0.32 | 59.2 | 0.32 | 61.7 | 0.80 | 0.40 | 0.50 |
| typical | 20 | 78,768 | 14.20 | 3.22 | 0.23 | 42.9 | 3.37 | 44.9 | 14.78 | 3.45 | 0.23 |
| tables | 37 | 72,188 | 14.65 | 6.00 | 0.41 | 87.1 | 6.11 | 88.8 | 16.80 | 6.80 | 0.40 |
| images | 28 | 57,481 | 10.44 | 2.78 | 0.27 | 50.7 | 2.88 | 52.6 | 19.80 | 11.86 | 0.60 |
| pages100 | 100 | 399,589 | 70.03 | 17.62 | 0.25 | 46.2 | 18.37 | 48.2 | 72.39 | 18.65 | 0.26 |
| pages500 | 500 | 2,008,124 | 353.19 | 86.23 | 0.24 | 45.0 | 89.94 | 47.0 | 361.84 | 91.31 | 0.25 |
| **target** | | | | | **≤ 0.5** | **≤ 48 on pages500** | | **≤ 48 on pages500** | | |

The Rust document model is small (`read` retained: 0.23 MiB typical, 1.1 MiB pages100, 5.3 MiB pages500; the TS model
takes 0.6 / 2.4 / 8.7 MiB) and the derived tree is smaller still (0.5 MiB on pages100). In the `images` class the
document's pictures dominate the "read" in both engines (9.1 MiB of media bytes kept in the Rust model; the TS keeps 8.9 MiB of
`ArrayBuffer`s; TS read + layout 19.8 MiB), which is why its total ratio is 0.60 while its layout ratio is 0.27.

**Peaks, the wasm instance and what dropping it gives back** (MiB)

| class | TS peak RSS − idle | Rust layout-call peak (`uo-alloc`) | peak / retained layout | wasm high-water (`byteLength`, PDF done) | wasm high-water + JS face bytes (7.0) | Node RSS peak − before (wasm session) | RSS after dropping the instance − before |
|---|---|---|---|---|---|---|---|
| small | 218.8 | 0.98 | 3.19 | 11.81 | 18.84 | 41.6 | 2.7 |
| typical | 213.4 | 6.72 | 2.09 | 16.75 | 23.78 | 40.5 | 2.1 |
| tables | 224.4 | 12.85 | 2.14 | 23.56 | 30.59 | 48.0 | 2.5 |
| images | 249.2 | 5.52 | 1.99 | 35.44 | 42.46 | 67.7 | 2.1 |
| pages100 | 339.5 | 35.19 | 2.00 | 46.88 | 53.90 | 65.7 | 2.0 |
| pages500 | 729.7 | 174.09 | 2.02 | 192.44 | 199.46 | 265.9 | 2.3 |

- **The wasm linear memory never shrinks**, as expected: after `free()` of layout, document and environment the module's
  live bytes are back at 2.1 MiB (the instance face cache, a cap of uo-fonts, not a leak) but `byteLength` stays at the
  high-water (46.9 MiB for `pages100`, 192.4 MiB for `pages500`). The high-water is set by the first layout (the second
  one reuses the freed blocks: `layout1` 45.8 MiB, `layout2` the same, for `pages100`) and is 1.6–1.9 × the live bytes of
  the session (the allocator's free blocks and the transient peak of the layout call).
- **Dropping the instance returns it all.** With every handle freed and the instance unreachable, an asynchronous major GC
  (`await gc({ type: 'major', execution: 'async' })`) collects the `WebAssembly.Memory` (checked with a `WeakRef`: `true`
  for every class) and the process RSS goes from +65.7 MiB (pages100) / +265.9 MiB (pages500) back to +2.0 / +2.3 MiB over
  the baseline; a **new instance starts again at 1.4 MiB** and reaches the same high-water for the same work (45.8 MiB
  for `pages100`, 192.4 MiB for `pages500`). Pitfall measured here: a synchronous `gc()` (or four of them) did not release
  the dropped instance in Node 24; only the asynchronous major collection did. So the "give it back" operation of a host
  is: free the handles, drop every reference to the instance and the wrappers (`bindWrite` closures and the module's
  `FinalizationRegistry` callbacks keep it alive while anything is reachable), let the browser collect it, instantiate a
  new one — which costs the font pushes again (7.0 MiB copied into wasm, plus the parse of the faces on the first layout).
- The **Rust layout-call peak is 2.0 × the retained layout** from 20 pages on (3.2 × on the 3-page document, where the
  fixed working set dominates), not "retained + 2 × the largest page + the face table" (plan §6): the working set grows with
  the document. The probable causes are the growth of the large vectors (`Vec` doubling counts the new block before the
  old one is freed in uo-alloc) and the passes that hold the previous pass's result while the next is built (the layout is
  repeated for balancing); not investigated here. In absolute terms: 35 MiB for 100 pages and 174 MiB for 500 pages, against
  the TS process peak (RSS − idle) of 339 and 730 MiB.
- **Timing** (same runs, informative only, the Rust numbers include the counting allocator): layout of `pages100` 435 ms TS,
  455 ms Rust; `pages500` 1.9 s TS, 2.3 s Rust. Not a gate of this section.

**Browser tab, the same 100-page document (Chromium, headless, 1280 × 900)** — numbers minus a blank tab

| scenario | JS heap used (after GC) | `measureUserAgentSpecificMemory` | renderer private bytes | wasm `byteLength` |
|---|---|---|---|---|
| blank tab (baseline) | 0.6 | 0.5 | 43.4 – 46.8 | — |
| TS viewer 0.9.0, first page shown | 49.2 | 71.9 | 170.5 (+127 over a blank tab) | — |
| TS viewer 0.9.0, after scrolling through all 100 pages and back | 49.3 | 72.1 | 193.8 (+150) | — |
| Rust wasm path, desktop profile (layout + 100 page messages encoded, 5 kept) | 0.9 | 1.3 | 113.8 (+67) | 45.8 |
| Rust wasm path, phone profile | 0.9 | 1.3 | 116.2 (+72) | 45.8 |
| Rust wasm path in WebKit (Playwright build 2311) | n/a | n/a | n/a | 45.8 (identical) |

Limits of this table, stated plainly: (1) **there is no Rust painter yet** (frontier D): the Rust page lays out, encodes
every page and keeps five messages, but decodes nothing and paints nothing, so it is the engine side only — a lower bound for
the Rust viewer — while the TS viewer's number includes its DOM, canvases and decoded pages. (2) `measureUserAgentSpecificMemory`
does **not** include the wasm memory in this Chromium (1.3 MiB for a page holding 45.8 MiB of wasm), and the CDP heap
metrics do not either; the wasm share is the `byteLength` and the renderer's private bytes, which is why the private bytes
are the comparable column. (3) Private bytes of a renderer are noisy (the OS trims working sets; the largest of three
renderer processes is taken; ±10 MiB between runs of the same scenario); the JS heap and `byteLength` columns are exact.
(4) WebKit exposes no memory API: only the wasm size is read there. (5) The phone profile changes the face budget (48 MiB
pinned, 16 MiB cache), which these 5 families (7.0 MiB) never reach, so desktop and phone are equal here.

**Against the plan's targets**

| Target (§6) | Result | Verdict |
|---|---|---|
| retained `Layout` ≤ 0.5 × the TS heap delta, per class | 0.23 (typical) – 0.41 (tables); 0.24–0.25 on the 100- and 500-page documents; the wasm `UoLayout` adds 2–5 % over the native `Layout` (navigation, run boxes, resources) | met in every class |
| ≤ 48 B per laid-out character on the 500-page document | 45.0 B native, 47.0 B as `UoLayout` in wasm | met (the wasm margin is 3 B) |
| per-character cost on other classes (not a target) | 42.9 (typical), 46.2 / 48.2 (pages100, native / wasm), 50.7 (images), **59.2 (small), 87.1 (tables)** | small documents carry fixed per-page items (header/footer lines), and a table-heavy page has many borders, cell boxes and few characters per cell: B/char is not the right unit there; per page: tables 0.16 MiB/page, typical 0.16, pages100 0.18, pages500 0.17 |
| peak of one layout call ≤ retained + 2 × the largest page's working set + the face table | 2.0 × the retained layout (35.2 MiB for 100 pages, 174.1 MiB for 500) | **not met as written**: the extra grows with the document; decision for the maintainer (cause above, not investigated) |
| encoded page ≤ 1.5 × the page's in-memory size, freed by JS | 11.1 MiB of messages for 100 pages (0.11 MiB each), 0 B left in the module after `encodePage` of every page (the live bytes do not change) | the second half met; the 1.5 × ratio is not measured here (the in-memory size of one page is not separated) |
| viewer, 100-page document: wasm high-water + JS-side face bytes ≤ 160 MiB (phone profile) | 46.9 MiB (with the PDF; 45.8 without) + 7.0 MiB of faces = **53.9 MiB** (TS tab: +127 – 150 MiB over a blank tab, engine and viewer together) | met by 106 MiB; against the TS viewer the Rust engine side is about 0.45 × its private bytes, with the missing painter caveat |
| 500-page stress wasm high-water ≤ 384 MiB (reported, not a gate) | 192.4 MiB (+7.0 MiB faces) | within the report threshold; a phone would still struggle (tab limits), as the plan says |
| per-document faces pinned ≤ the profile's `face_budget` | 7.0 MiB pushed; the instance face cache holds 2.1 MiB after everything is freed (the profile's budget applies to it) | met |

**Deviations and things to know**

- The TS "retained" is a heap delta of a long-lived Node process (V8 object overheads, hidden classes, boxed numbers in
  `chars`), the Rust one a count of allocator bytes; both are with the outputs alive after a warm-up, so the font caches and
  the hyphenation tables are in neither. The TS number would be higher on a cold process.
- `images`: the TS per-pass copy of the media is part of its read; neither engine decodes the pixels during layout.
- The oracle corpus is not part of the default run (its documents use proprietary fonts that the open provider
  substitutes, so the engines would be compared on different faces); `UO_MEMBENCH_CORPUS=<n>` runs every k-th oracle
  document for a smoke slice.
- Not measured: the Rust native process RSS (no portable counter in the test harness; the allocator counts are the native
  numbers), WebKit's tab memory (no API), Firefox, real Safari/iOS devices (BrowserStack, gate 3), and a Rust viewer with a
  painter (frontier D) — re-run `browser.mjs` when it exists.

**Re-run** (Git Bash, nothing else running; `-j 4` is inside the script)

```
export UO_TS_ROOT=D:/Projects/OfficeNext/.claude/worktrees/clickcompliance-docx   # frozen TS, dist built
export CARGO_TARGET_DIR=<own target dir>
node core/tools/bench/memory/run.mjs --tag <name>                  # builds bench_memory + the count,write wasm, runs the 6 classes
node core/tools/bench/memory/run.mjs --no-build --classes pages100,pages500 --skip phone
UO_MEMBENCH_CORPUS=30 node core/tools/bench/memory/run.mjs --no-build --classes small          # + 30 oracle documents
cargo build -p uo-wasm --features write --target wasm32-unknown-unknown --profile wasm -j 4    # then copy the .wasm to $CARGO_TARGET_DIR/uo_wasm_ship.wasm
UO_TS_VIEWER=<published viewer folder> node core/tools/bench/memory/browser.mjs --webkit       # Chromium TS viewer vs Rust page (+ WebKit wasm size)
```

`browser.mjs` points Playwright at the installed Chromium/WebKit executables (`UO_CHROMIUM`, `UO_WEBKIT` override; the
Playwright package comes from `UO_PLAYWRIGHT`, default the `sheet/node_modules` one of the main checkout).

### Benchmarks main × rust-core — speed (measured 2026-11-10)

Measured on HEAD `ec66f1a1` plus the uncommitted stage-1 work (tree of 2026-11-10), Node 24.15.0, Windows 11, a 20-thread
machine that was quiet during the runs (3 % CPU before each; other agents compile on it at other times, so absolute
milliseconds move by up to 2× between sessions and only the interleaved ratios are used). TS side: the frozen
`d0f45bc5` (`UO_TS_ROOT`; `main`'s own `dist` is stale, HANDOFF F2-20, as in the earlier runs). Harness: `core/tools/bench/speed/run.mjs`
(results `results/speed-2026-11-10-ec66f1a1-dirty.{md,json}`), the native side in `crates/uo-ffi/examples/bench_speed.rs`.

**Method.** ONE Node process drives three engines over the same documents, **interleaved per document and per round** (the engine
that starts rotates every round), after one untimed warm-up iteration per engine and document (faces parsed, JIT warm, caches filled):
`ts` = `DocxPackage.load` + `readFullDocument` + `readEmbeddedFonts` (open); `layoutDocument` + `pdfNavigation` (layout);
`renderPdf` compressed (pdf). `wasm` = the shipped module (`--features write --profile wasm`, 3.96 MB) through `js/write.mjs`: `open`,
`layout` (which includes the navigation, as `pipeline::lay_out`), `pdf({ compress: true })`, one page-level environment with the
5 open families pushed once (as the viewer) and the hyphenation patterns registered on demand. `native` = a persistent child process
(`bench_speed`, release, `oracle` policy over the open fonts, the same functions `uo_docx_to_pdf` runs, one stdin line per
iteration). Documents: the **68 oracle capture documents** of the five families the server parity uses (`f7a`–`f7d`, `f8`; 568 pages,
open fonts; 16 ms – 2.6 s in TS) and the five generated classes of the memory benchmark (`typical` 20 pages, `tables` 37, `images`
28, `pages100`, `pages500`; the same bytes as the memory run). Rounds: 9 for the captures, 7 for the classes, 5 for `pages100`, 3 for
`pages500`; the table uses the **median of each document's rounds, summed over the class**. Output equality is checked in an
untimed pass per document on the uncompressed PDF (the compressed bytes depend on the deflate implementation; the parity gates compare
raw): **TS, native and wasm give byte-identical PDFs on all 73 documents** (0 mismatches; the first run had 1 because the TS formats
`now` in the local zone, which the harness now pins to UTC like the capture tools).

| class | docs | pages | stage | TS (ms) | wasm in Node (ms) | native (ms) | wasm / TS | native / TS |
|---|---|---|---|---|---|---|---|---|
| captures | 68 | 568 | open | 171 | 105 | 95.6 | 0.61 | 0.56 |
| captures | 68 | 568 | layout | 9,685 | 9,531 | 9,262 | 0.98 | 0.96 |
| captures | 68 | 568 | pdf | 1,968 | 1,515 | 1,313 | 0.77 | 0.67 |
| captures | 68 | 568 | **total** | 11,846 | 11,161 | 10,677 | **0.94** | **0.90** |
| typical | 1 | 20 | total | 220 | 199 | 193 | 0.91 | 0.88 |
| tables | 1 | 37 | total | 371 | 345 | 299 | 0.93 | 0.81 |
| images | 1 | 28 | total (layout 159 / 111 / 110, pdf 515 / 362 / 315) | 688 | 482 | 438 | 0.70 | 0.64 |
| pages100 | 1 | 100 | total | 1,084 | 1,044 | 954 | 0.96 | 0.88 |
| pages500 | 1 | 500 | open | 121 | 56.3 | 54.1 | 0.47 | 0.45 |
| pages500 | 1 | 500 | layout | 4,199 | **5,865** | 4,069 | **1.40** | 0.97 |
| pages500 | 1 | 500 | pdf | 1,445 | 1,108 | 911 | 0.77 | 0.63 |
| pages500 | 1 | 500 | **total** | 5,772 | **7,029** | 5,036 | **1.22** | 0.87 |

Per-document distribution on the 68 captures (ratio to TS of the document's median): **total** wasm median 0.90 (p10 0.84, p90 0.99,
max 1.06, 5 documents above 1.0), native median 0.85 (p10 0.79, p90 0.94, max 1.05, 1 above 1.0); **open** wasm 0.66 / native 0.58 (always
faster); **pdf** wasm 0.78 / native 0.67 (always faster); **layout** wasm median 1.00 (max 1.17, 34 of 68 documents slower than TS) and
native median 0.95 (max 1.19, 20 of 68 slower). Per page on the generated documents: layout TS 8.3 / 8.4 ms (100 / 500 pages), native 7.5 / 8.1,
wasm 8.1 / **11.7**; PDF TS 2.4 / 2.9, native 1.8 / 1.8, wasm 2.3 / 2.2. The first run (before the UTC pin, same tree, 9 rounds) gave
captures totals 12,227 / 11,371 / 10,822 ms (0.93 / 0.89), `pages500` wasm 1.20 total: the ratios repeat to within 0.01–0.03. The untimed
first iteration of every document (faces already warm from earlier documents): TS 12.9 s, wasm 11.4 s, native 10.8 s over the 68 captures.

**Reading.** In Node, with warm caches, the Rust read path is **not slower than the TS one in total on any class up to 100 pages** (native
0.64–0.90, wasm 0.70–0.96) and wins in the open (0.45–0.92) and in the PDF writer (0.61–0.93). The layout stage — most of the time —
is **at parity, not faster**: native 0.96 and wasm 0.98 of TS on the corpus sum, with a third of the documents (native 20, wasm 34 of 68) a
few percent slower than TS on this stage. The earlier sentence "natively and in Node the same Rust layout is as fast or faster than TS" (uo-bindings.md,
"Open speed") is therefore backed by these numbers only as "as fast, on the corpus sum"; no number here supports "faster".

**Findings against the targets.** The plan has no numeric speed target for the read path (the §6 targets are memory); the working bar of the
records is "not slower than the TS" per stage.

1. **wasm layout of the 500-page document is 1.40 × the TS** (5.9 s against 4.2 s; native 0.97). The per-page layout cost of the wasm
   module rises 45 % from 100 to 500 pages (8.1 → 11.7 ms) while native rises 9 % and TS 2 %. Not warm-up (the run repeats 3 times; `node --no-liftoff`
   gives 6.98 s against 7.12 s, `--liftoff-only` 19.5 s), not interference (the wasm engine alone in the process: 7.12 s total; with TS next to it 7.14 s).
   The wasm linear memory of this run grows to 192 MiB (memory record); cause not investigated (candidates: allocator behaviour or page-fault cost of the growing
   memory, cache and TLB locality at that size). Decision for the maintainer: investigate before the gate or accept (a 500-page document is the stress case, not a
   viewer scenario: the viewer lays out incrementally in the browser's tab limits).
2. **Browser: the Rust wasm is slower than TS in Chromium on the whole** (record "Open speed", viewer switch, 74 fixtures; cited, not re-run here): S1 (a new
   element per document) Rust median 43–48 ms against TS 66–71 (ratio 0.70) but the wasm layout alone is 9.0 s against 7.1 s; S2 (one element) total 9.4–9.6 s against
   7.6–7.7 s (ratio 1.12–1.20); S0 cold first document 131–173 ms against 68–97 ms. Node and Chromium differ for the same module (Node wasm total 0.94 of TS on
   the captures), the Chromium tier-up cause is still a hypothesis.
3. **Layout is the only stage without a Rust advantage** (above): any further speed work belongs there (the memory work is done, the open and PDF stages already
   win).
4. **Gaps of this run.** Not measured: the Rust server PDF against the frozen C# `DocxPdf.ToPdf` (the C# needs a build of the frozen `dotnet/`; the earlier fonts/lines
   spikes had C# columns, this run has none), WebKit or Firefox timings (the wasm2js section has WebKit speed rows for 4 fixtures only), cold-process start-up, the
   phone profile, concurrency (several documents at once through one converter), and documents with proprietary fonts (the open provider substitutes them on both sides).

**Re-run** (Git Bash, nothing else running; builds with `-j 4` inside the script)

```
export UO_TS_ROOT=D:/Projects/OfficeNext/.claude/worktrees/clickcompliance-docx   # frozen TS, dist built
export CARGO_TARGET_DIR=<own target dir>
node core/tools/bench/speed/run.mjs --tag <name>                      # builds bench_speed + the shipped wasm, 68 captures + 5 classes
node core/tools/bench/speed/run.mjs --no-build --docs pages500 --rounds 3 --engines wasm,native
```

**Memory** was re-measured on the same tree with `core/tools/bench/memory/run.mjs --skip phone` (`results/memory-2026-11-10.{json,md}`): the
retained and peak tables above are **unchanged** (Rust layout 0.23–0.41 × TS, 45.0 B/char native and 47.0 B/char wasm on `pages500`, wasm high-water
46.88 / 192.44 MiB for `pages100` / `pages500`, layout-call peak 2.0 × retained); the wasm high-water of the small classes moved by ≤ 0.13 MiB
(`typical` 16.75 → 16.88) and TS heap numbers by ≤ 0.02 MiB. SPEED-1 (the page-level environment) does not enter this harness (one environment per
process, freed at the end), so its retained cost after a viewer closes is not seen here (skeptic S6 in uo-bindings.md).

**Process metrics (this run).** Sonnet 5.5, about 1 h wall time (estimated: native example build 2 min 47 s, wasm build 1 min 10 s, two full speed runs of
about 5 min each, one memory run of about 8 min, probes 5 min), about 50 tool calls. Rework: the first speed run compared PDFs from a TS side in the local time
zone (one document differed; pinned to UTC and re-run); a Python edit that went through the Bash tool lost a backslash in a template string (the native engine
got the wrong compress flag; caught by the equality check).

## Process metrics

| Stage | Agent (model) | Time | Tool calls |
|---|---|---|---|
| 1 | planner + reuse, architecture plan (Opus) | (orchestrator) | ≈ 45 |
| 2 | analysis skeptic, verified in the code (Opus) | (orchestrator) | ≈ 85 |
| 1 (revision) | planner: K1–K20 written in, consolidated question list G15/G19/G21/LQ1–LQ22, order and estimate re-based (Opus) | (orchestrator) | ≈ 30 |
| 3 — F0 | foundation + harness: crate skeleton, every shared type, uo-render types + IDL (frontier T), capture/replay harness proven on 2,755 TS layouts, codec wiring, reconciliation with uo-docx D1a and the uo-fonts registry, wave-1 briefs (Opus) | one session, built in parallel with uo-docx D1a, codec C and registry P | ≈ 165 |
| 3 — F1 | fonts and the 600-DPI width model: `fonts.ts`, `context.ts` flow and notes, the `SfntFont` kerning scale (K7), the `para.ts` width model, `math.ts` `parseFont`; facets p3.metrics/advance/kern/faces + p2first and the resolver sweep, equal on the oracle (2,741), 200 CI documents and 5,000 sweep scenarios at the first replay (0 rounds) (Opus) | one session, ≈ 1.5 h, in parallel with F3, F5, D1a, uo-image | ≈ 95 |
| 3 — F3 | drawing values and metafiles: `geometry.ts` 356–475 (`substPhClr`, `withThemeStyles`, `shapeExtras`, `groupChild`, the `ShapeGeom` spec), `pictureLook`, `resolvePageFill`/`imagePixelInfo`/`patternRowsFromImage` (+ docx-core `resolveColor2`, `vmlColor`, `dmlColorSpec`), `textfx.ts` `fxColor`/`fillOf`/`resolveTextFx`, `emf.ts` (GDI + EMF+) and `wmf.ts`; facets p3.emf/wmf/picture/pagefill/textfx/shape equal on the oracle (2,742 documents, 184,916 calls), the .NET inputs and 11,480 generated metafiles; **1 parity round** (POLYBEZIER with no point: the TS throws a `TypeError`), found by the generated differential (Opus) | one session, ≈ 2 h, in parallel with F1, F5, D1a, uo-image | ≈ 115 |
| 3 — F5 | lines and paint: `lines.ts` (re-ported from the frozen TS on the spike's structure), `hyphenate.ts` `chooseHyphenation`, `para.ts` `shadowExtent`, `underline.ts`, `paint.ts` (`paintInlineObject` ends left to F7b); facets p3.lines (replacing the F0 seed) and p3.paint equal on the oracle (2,742 documents: 142,171 breaker and 139,904 paint calls), 200 CI documents and the committed fixture at the first replay (0 rounds) (Opus) | one session, ≈ 50 min, in parallel with F1, F2, F3, D1a, uo-image | ≈ 130 |
| 3 — F2 | Word semantics in `uo_docx::word` (LQ3): `resolve.ts`, `directRPr`/`directPPr`/`resolvePPr`/`paraformat.ts`, `numbering.ts`, `datefmt.ts`, `field-formula.ts` (+ docx-core `formatPicture`/`formatFormulaResult`), `outline.ts`, `numpages.ts`, `revisions.ts`, `refs.ts` (model side), the `documentHyphenationLanguages` walk; the `JsAssign` spread machinery with key order; unit differential (25k generated cases, 222,757 facets), engine facets p3.datefmt/formula/numbering/style equal on the oracle and the F2 documents, São Paulo facet 1,485/1,485 — 0 parity rounds; 1 defect found by the no-panic loops (`sort_by` panics on NaN tab positions, F2-7) (Opus) | one session, ≈ 3 h, in parallel with F1, F3, F5 and R4b/R4c | ≈ 200 |
| 3 — F4 | paragraph preparation: `para.ts` (`prepareParagraph`, `resolveRunStyle`, `faceFor` and the glyph fallback order, `displayRuns`/`fieldRuns`, ligatures, run-boundary kerning, NBSP glue, kinsoku and the East Asian layout, the marker, `paraVisibility`, `isDark`, `isCjkFont`, `isMarkRun`), `hyphenate.ts` `markHyphenation` (+ the hyphenator sessions), `math.ts` layout (`mathAtoms`, `rowBox`, `fracBox`, `mathTextMetrics`); facet p3.prepare (the whole `PreparedPara` per call, replacing the F0 seed) replayed over the uo-docx reader: 0 mismatches on the oracle (84,426 calls equal, 1,026 pending: engine-derived copies, F7a), 200 CI documents (15,108) and the .NET inputs — **1 parity round** (the metrics face of a `.notdef` symbol took `w:sym`'s font: 4 oracle calls) (Opus) | one session, ≈ 1 h 20 min | ≈ 120 |
| 3 — F6 | tables: `table.ts` whole (`layoutTable` with the cascade, borders and bands, the autofit width model, vertical merges and text, RTL, separated borders, `paintRow`/`build`/`row.split` as `RowSplit` data, `splitContent`/`splitAtNest`/`nestedRowCut`/`widowCut`, `measureCell`/`anchoredWidth`/`paraContentWidths`, `mergeTextFirst`/`atPageTop`/`dropMoved`), the `ObjId` object identity (K2e); facets p3.table/p3.rowsplit/p3.splitcontent replayed through a transcript `CellLayouter`: 0 mismatches on the oracle (1,590 calls equal, 1 pending: a chain-continuation table, F7a), 200 CI documents (902), the .NET inputs (5) and the committed fixture — **0 port parity rounds**, 2 harness rounds (split ids written in values; the `'preview'` ref) (Opus) | one session, ≈ 2 h 15 min, in parallel with F7a | ≈ 110 |
| 3 — F7a | engine core: `layoutDocument` (passes, balancing, K19), `run` + the page assembly, geometry/pages/sections/columns, `placeParagraph` with keep rules, border groups, line numbers, the F7a field values, the derived document; runtime copies through a relocated `LDoc`; P1 through the real engine (D1a reader → derive → engine): **1,203 of 2,742 oracle documents equal, 0 mismatches**, 1,539 pending by reason (F7b/F7c), 1,300 generated F7a documents and the .NET page fills equal — **1 parity round** (the marker moved by `joinHidden` kept its paragraph's style indices); the `--gen f7a` generator, the stale-build refusal, the CI step (Opus) | one session, ≈ 3 h, in parallel with F6 | ≈ 150 |
| 3 — F7b | engine tables and floats: `placeTable` + floating/late tables and the float carry, row emission, the cell layouter, the static-flow tables and cell floats, `wrapFrame`/`pickSeg`/`moreSegs`, `placeFloat(s)`/early floats/`paintObject`/inline drawings, the float branches of `placeParagraph`, the image table and the F5-9 ends, `liftFrames` and `indexChains` in `derive`; P1: **2,728 of 2,742 oracle documents equal (with F7a/F7c), 0 mismatches**, 14 pending (linked textbox chains), 300 generated `--gen f7b` documents and the committed fixture equal — **2 parity rounds** (`pinFloatsOverTable` without the table height: 2 documents; chains not indexed: 11); M1/M2 0 B, no panic on 200 hostile documents (Opus) | one session, ≈ 2 h 45 min, in parallel with F7c | ≈ 190 |
| 3 — F7c | notes, headers/footers, fields and `layoutStatic`: the static flow (stable entry, F7b wired its float/table sites into it), `layoutHeaderFooter` with the scratch-page probes, the note area (separator, columns, split notes, the carry), the endnote flow, SEQ/REF/NOTEREF/STYLEREF/PAGEREF and `bookmarkPages`, V8's whole date parser; P1 through the real engine: **0 oracle documents pending for an F7c reason, 2,728 of 2,742 equal and 0 mismatches** (with F7b), 850 generated F7c documents, the 30 F0 documents and the committed fixture equal — **0 port parity rounds**; 1 defect from the no-panic loop (the endnote separator looked up before the early return); the `--gen f7c` generator, the CI step (Opus) | one session, ≈ 4 h, in parallel with F7b | ≈ 190 |
| 3 — F7d | linked textbox chains and endnote-flow tables: `chainOf`/`chainSlice` with `sliceParagraphFrom` (an `RtPara` with the cut first run and `CONTINUATION`, prepared through the scratch `LDoc`), the row-sliced table copy (an owned `Table` in the frame, a reborrowed `Cx`), `remapLines`, the three call sites, `nextBlockLead`'s endnote-flow table; P1: **2,742 of 2,742 oracle documents equal, 0 pending, 0 mismatches**, 900 generated `--gen f7d` documents and the committed fixture equal, F6's `lk-table2` equal (a facet name for row-sliced copies) — **0 port parity rounds**; M1/M2 0 B, no panic on 200 hostile chain/endnote documents; the module docs of the engine updated; the `--gen f7d` generator, the CI step (Opus) | one session, ≈ 2 h 15 min, in parallel with F8 and D2b | ≈ 120 |
| 3 — F8 | editor-facing API and oracle: `refs.rs` (line → paragraph: `locateRef`/`lineParagraph`/`hfBlocksFor`/`noteBlocks`), `hit.rs` (`caretXIn`/`offsetAtX`, the editor's `eng.lines` and `layoutCache` per scope, `hitTestRaw`, `caretRectAt`, `paintEngineSelection`, `engLineAt`, `cellBoxAt`, `pageOf`), `run_boxes.rs` (+ `paintCustomInlines`' face and `measureText`, K17e), `navigation.rs`, `onField` → `Layout::fields`, `missing_faces` + `"font budget"` (uo-fonts signal), `ViewTransform` Web/Draft, the uo-docx `LayoutOracle` (`EngineOracle`, tested over a `Session`); P4 harness with the editor's private methods extracted from the frozen source; **2,742 of 2,742 oracle documents equal in every P4 facet (41,130 facets) + P1**, 300 generated `--gen f8` and the committed fixture equal — **4 parity rounds** (1 harness: client-pixel scaling; 3 port: field results one level down, flat header/footer/textbox scopes, the TS's repeated in-place collapse of shared `chars`); M1 0 B, hit tests 0 B allocated, no panic on hostile probes (Opus) | one session, ≈ 5 h, in parallel with F7d and D2b | ≈ 230 |

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (G15, G19, G21, LQ1–LQ22). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.

## Milestone 1 — F0 (foundation and harness)

Opus, 2026-11-03, hybrid mode. Built in parallel with uo-docx D1a (Opus), the uo-render codec
generator (frontier C, Sonnet) and the uo-fonts registry (frontier P), against each as it landed.

### What exists

**Crate `core/crates/uo-docx-layout`** (`forbid(unsafe_code)`, `deny(cast_possible_truncation,
cast_sign_loss, indexing_slicing)`; fmt, clippy `-D warnings` native and wasm32, the float-cast and
grep ban scripts clean; builds for `wasm32-unknown-unknown`). Every shared type of the plan, the
module layout of §1 and the stage entry points of every later frontier with their signatures:

| Module | Content (F0) | Filled by |
|---|---|---|
| `model` | the only module naming uo-docx: re-exports of `DocView`, `RunRef` (+ `RunSub`), `CellKey`, `StoryRef`, `BlockPath`, `DocRef`, `NoteKind`, `HfWhich`, `FieldScope`, `HyphenSkips`, `HyphenCandidate`, ids, `Utf16`, `NumKey`, `Paragraph`/`Run`/`PPr`/`ParaProps`; `StoryChange` local until D2a | — |
| `keys` | the LA3 keys: `LParaId`, `RunKey`, `TbKey`, `ChainId`, `TableLayoutId`, `ItemMeta`, `PageIx`, `CellContentId`, `LineSeq`, `ContainerId` (table of TS identity → key in the module doc) | — |
| `ldoc` | `LDoc` with shallow `LPara` (`ParaSrc::Model{para,id}`/`Copy{base}`, overrides, `RunKey` list, `REV_JOINS`, `ParaFlags`, `FRAME_CARRY`), `RunSrc::Model{run, at: RunRef}` / `Slice{base, from}` (no text copied) / `Synth`, `LStory`, `LTextbox` + `FrameInfo`, `Chain`; `run_text` reads slices from the model text | F7a builds it (`derive::build`) |
| `options` | `LayoutOptions` (defaults = the TS ones; `hyphenation_loaded`, `Clock` + `TimeZoneRule`, `RevisionView`, `ViewTransform`, `collect_fields`), `LayoutLimits` (first numbers), `WorkInput` + `Budget` (input-proportional, K16) | — |
| `error` | `LayoutError::{Limit, Internal, NoFace, Unported}`, `LimitKind`, `Frontier`, `LResult` | `Unported` removed before gate 2 |
| `output` | `Layout`, `LayoutPage` (uo-render `Page` + the Write section: `lines`, `content_lines`, `chars` SoA, `pfmts`, `objects`, `cells`, `item_meta` = `lineBox`/`lineNum`, `clip_src`), `LineGeom`, `LineRef`/`RefPath`/`LineStory` (HF slot + part, K13), `ImageOrigin` (K12), `PlacedObject`, `ObjectFrame`, `PlacedCell`, `LineParaFormat`/`TabStop` | — |
| `items` | `PassItems` (uo-render `ItemList` + `ItemSide` per item: `ItemMeta`, glyph meta, clip src) and `PassLine` (line identity, K2e); `push_shifted` = `shiftItem` with the side data kept, `shifted`/`shifted_xy` = `shiftLine`/`shiftLineXY` | — |
| `fonts` | the registry's `FontProvider`/`FontAnswer`/`AnswerId` (re-exported), `FaceMetrics`, `FaceEntry`, `FaceTable`, `FaceResolverCache` (per call, K4), `device_em_pt` (ported) | F1 |
| `context` | `DocContext` (classified: carried `flow`, cache `resolver`, input `rev`, output `warnings`…), `FlowState`, `NoteFlow`, `NoteNumbering`; `save_flow`/`restore_flow`/`warn` | F1 |
| `word` | facade over `uo_docx::word` with placeholders for `NumberingState` (+ `LevelCount`), `NumberMarker`, `StyleResolver`, `RevisionDisplay` | F2 (in uo-docx) |
| `para` | `RunStyle`, `GlyphKind`, `Glyph` (face = `FaceId`, style = `StyleIx`, obj = `RunKey`, flags), `ParaMarker`, `PreparedPara`, `RevBars`, `ParaEnv` + `FieldSource` (the `field` closure as data), `prepare_paragraph`, `advance_twips` | F4, F1 |
| `hyphenate`, `math` | `HyphenUnit`, `HyphenChoice`, `HyphenSessions` (+ `WORD_CACHE_CAP`); `MathBox`, `MathItem`, `MathAtom` | F4 |
| `lines`, `underline`, `paint` | `PlacedGlyph` (+ `GlyphSrc`: paragraph, marker or automatic hyphen), `Line`, `LineFrame`, `BreakMark`, `LineBreaker::{new,next,reset}`, `line_box`; `UlMetrics`, `UlGeom`; `paint_line` | F5 |
| `table` | `CellContent` (`nest` by index ranges), `NestRow`, `ParaSplitInfo`, `RowLayout` (all TS fields), `RowSplit` (the closure as data), `TableLayout`, `CellEnv`, `trait CellLayouter`, `layout_table`, `split_content` | F6 |
| `engine` | `Engine` (every field classified, below), `Cx`, `PassInput`, `PassFindings`, `LayoutRun`, `BalanceSpec`/`BalanceInfo`/`LateTable`, `ChapterIndex`; `page` (`SectGeom`, `PageState`, `PageOut`, `ZObj`, `FloatBox`, `WrapFrame`), `flow` (`PrevPara`, `CellNeighbors`, `HiddenJoin`, `LastBox`), `tables` (`FloatFlow`, `FloatCarry`, `LastRowEmit`), `floats` (`Pinned`, `FloatReceipt`, `ZLayer`), `notes`, `hf`, `fields`; `layout_document`, `Engine::run` | F7a–c |
| `derive`, `drawing`, `metafile` (`trait FaceLookup`, `render_emf`/`render_wmf`), `query` (`RunBox`, `RunFilter`, `Hit`, `CaretRect`, `LineHandle`, the `Layout` queries; `lines_of` ported) | stubs and doc | F7a, F3, F8 |
| `json` | `JsonKey`, `JsonBuilder` (JS rules via `uo_jsnum::json`; model values through the uo-docx `JsonWriter` with `raw`) | F2/F4 use it |
| `canon` | the Rust P1 writer (`canon_layout`, `CanonSink`, `CANON_VERSION`) | — |
| `codec` | the generated Write-section codec (`core/schema/write.uos`, frontier C) | E wires it |
| crate root | `LayoutEngine::{new, layout, relayout, last, clear_caches}` over a `&dyn FontProvider` | F7a/F8 |

Stage-3 stubs (`Err(LayoutError::Unported(frontier, site))` for the fallible entry points; the
infallible queries answer "nothing" and are listed in `query::PENDING`, the geometry in
`uo_render::geom::PENDING`): `tests/f0.rs` asserts the lists only shrink. Never `todo!`/`panic!`.

**Harness** (`tools/`, Node 24.15, no change to the frozen TS):

- `capture-layout.mjs` — modes `ci` (seeded generated documents, open fonts only: node-fonts with
  `WINDIR`/`LOCALAPPDATA` pointed at nothing and `EXTRA_FONT_DIRS` = the repository's open fonts),
  `oracle` (installed fonts; refuses to run without Arial/Calibri/Times), `files`. Determinism: TZ=UTC
  set before any `Date`, `now` = 2026-01-15T12:00:00Z, the 7 pattern sets loaded, every document read
  again per run (K3), `mk-*` with revisions `all`. Facets per document: `p1`, `p2`, `p2first`,
  `p3.prepare`, `p3.lines`, `pages`, `error`. `--dump` writes the raw P1 stream, the transcript and the
  image payloads; `--docs-out` the generated `.docx`.
- `lib/canon.mjs` — **the P1 walker; its header is the specification** of the stream `src/canon.rs`
  writes (version 1): exact f64 bits, UTF-16 strings, faces/images/`lineBox.line` labelled by first
  appearance, optional booleans by truthiness, refs as the TS strings, `PlacedObject.run` as the run's
  model position (`b3/r0/c1/b0#2`, `hf<n>/…`, `fn<id>/…`, `…/tx/…`, `…/g<j>`).
- `lib/transcript.mjs` (P2: every `provider.face` call and the answer identity; facets `p2` and
  `p2first` = first request per lower-case key, LQ19; TSV transcript with digests and sources),
  `lib/facets.mjs` + `lib/loader.mjs` (P3: an in-memory module-loader rewrite wraps exported
  functions, prototype patching wraps classes; seeds `p3.prepare` and `p3.lines`), `lib/sink.mjs`,
  `lib/gen-docx.mjs` (the seeded generator: paragraphs, runs, alignment, indents, spacing, tabs, page
  breaks, tables, two-column continuous sections, header and footer with PAGE).
- Rust: `tests/common` (the `StreamSink`, the transcript reader, the blob index by SHA-256 over the
  repository fonts + transcript paths + dumped media — a face not found **fails**, LQ21 — and
  `TranscriptProvider`, the replay policy implementing the registry's `FontProvider` with the TS
  answer identities), `tests/common/decode.rs` (P1 stream → `Layout`), `tests/adapter.rs`,
  `tests/layout_parity.rs` (opt-in replay; reports `pending` per frontier, `UO_LAYOUT_STRICT=1` for the
  gate), `tests/f0.rs`.
- CI job `layout-parity` (`.github/workflows/rust-core.yml`): checks out and builds main's TS, captures
  200 generated documents (seed = run number), runs the adapter check and the replay.

### Harness proven end to end (the adapter path of uo-render R-P2)

The frozen TS layout of a document → raw P1 stream → decoded into the Rust types (`Layout`, uo-render
pages and resources, the Write section) → `canon_layout` → the same `count:digest` as the TS:

| Corpus | Equal | Not decodable | Mismatches |
|---|---|---|---|
| Word oracle, all 2,742 documents (installed fonts, local) | **2,741** | 1 (`emwidth/ew-emb`: embedded fonts come from the reader) | 0 |
| .NET synthetic inputs (pictures, page fills, pattern, frame, tile, textboxes, text effects, synthetic) | **8** | 0 | 0 |
| generated (CI mode), seed 1, 6 documents; committed fixture `tests/captures/f0` (2 of them) | **6** | 0 | 0 |

So every field the TS engine emitted on the whole corpus has a Rust representation, and the Rust
walker equals the TS walker item by item. TS capture of the full oracle: 33 s (2,742 documents,
4,629 pages); Rust decode + canon of all of them: 16 s (release). Repeated captures are identical
(determinism check). `layout_parity` runs and reports every document `pending` (`F7a`: reader +
engine) until F7a wires the uo-docx reader into `rust_layout`.

### Engine field classification (K18)

Classes: **output** (what the pass produces), **carried** (flow state a milestone-2 page checkpoint must
hold and compare), **cache** (memo of a pure function of the document within the pass), **finding**
(read by the pass loop after the pass), **input** (`PassInput`: the constructor parameters and the
guesses set by `layoutDocument`).

| TS field (engine.ts line) | Rust | Class |
|---|---|---|
| `ctx` (ctor) | `ctx: DocContext` — `flow` carried, `resolver` cache, `rev` input, `warnings` output | mixed (split in `DocContext`) |
| `full` (ctor; the endnotes swap its blocks, 1088) | `story: u32` | carried |
| `totalPagesGuess`, `marksGuess`, `lateFloats`, `lateTables`, `balance`, `noteRefGuess` (ctor), `chaptersGuess` 376, `sectionPagesGuess` 345, `noteMarks` (context) | `PassInput` | input |
| `pages` 327 | `pages: Vec<PageState>` (with `PageOut` + per-page working state) | output |
| `cur` 328 | `cur: Option<PageIx>` | carried |
| `col` 329, `y` 330, `colTop` 332, `atTop` 333, `topGapPrevAfter` 338, `prev` 339 | same names | carried |
| `lastBox` 341 (`page?: object`) | `last_box: Option<LastBox>` (`ContainerId`, K2f) | carried |
| `firstInDoc` 342 | `first_in_doc` | carried |
| `usedNumPages` 343, `usedSectionPages` 346, `usedPageRef` 374, `usedChapter` 377, `usedNoteRef` 395 | `found.used_*` | finding |
| `geoms` 347 | `geoms: Vec<SectGeom>` | cache |
| `lnCount` 349, `lnPage` 350 (`PageState` → `PageIx`), `lnSect` 351 | same | carried |
| `lastRowEmit` 353 | `last_row_emit` (owns a clone of the row) | carried |
| `tblX0` 356, `inHf` 358 | same | carried |
| `floatCarry` 360 | `float_carry: Vec<FloatCarry>` (owns the `TableLayout`) | carried |
| `floatFlow` 363 (`WeakMap<TableLayout>`) | `float_flow: IndexMap<TableLayoutId, FloatFlow>` + `next_table_layout` | carried |
| `onlyTextFloats` 369, `prePlaced` 371 (`Set<Run>`), `zSeq` 372 | `only_text_floats`, `pre_placed: IndexSet<RunKey>`, `z_seq` | carried |
| `headingMarks` 379 | `found.heading_marks` | finding |
| `balanceFound` 398, `lateFound` 416, `lateTablesFound` 417 | `found.*` | finding |
| `colUsed` 400, `balanceSaved` 402, `balanceLastFree` 413 | same | carried |
| `placedTables` 419, `lateSplit` 421 | `IndexSet<u32>` | carried |
| `listTexts` 524 | `list_texts` | cache |
| `colBottoms` 1052 | same | carried |
| `coverPage` 1091, `firstPageNumber` 1092 (read at the start of `run()`) | same | cache |
| `inCell` 1501, `hfFrameAfter` 1503, `hfFrameTbl` 1505, `staticDx` 1507, `cellMar` 1509, `colMaxY` 1511, `sectGapAfter` 1514 | same | carried |
| `cellNb` 1560 (`WeakMap<TableCell>`) | `cell_nb: FxHashMap<CellKey, CellNeighbors>` | cache |
| `pendingJoin` 1716, `pendingDel` 1718 (`Run[]`), `floatTableGap` 1721, `carryAnchorGap` 1723 | same (`RunKey`s) | carried |
| `noteCache` 2218, `sepCache` 2301 | same | cache |
| `noteCarry` 2389, `rowNoteCut` 2391, `flowFills` 2393, `enFlow` 2442, `curEndnote` 2444, `endnotesDone` 2446 | same | carried |
| `pinnedFloats` 2891 (`Map<Run, {ps, y}>`) | `pinned_floats: IndexMap<RunKey, Pinned>` | carried |
| `chains` 3261 (lazy `indexChains`) | `LDoc::chains` (once per call) | cache (moved) |
| `chainSlices` 3263 | `chain_slices: IndexMap<ChainId, IndexMap<TbKey, CellContent>>` | carried (memo held by a checkpoint) |

### Decisions beyond the plan (declared)

| # | Decision | Why |
|---|---|---|
| F0-1 | `model` is the only module naming uo-docx; F0 started on placeholders with the A2/A4 names and switched to re-exports when D1a compiled (same session). `StoryChange` stays local until D2a | parallel construction; one place to reconcile |
| F0-2 | uo-render `Item` uses **inline struct variants** (`Item::Rect { … }`), boxed `Image`/`Shape`, `XformPush { m: Option<Box<[f64; 6]>> }`: measured 48 B. A glyph run keeps `text`, `scaleX`, `alpha`, `fx` in a per-run side record (`RunSide`) | tuple variants over payload structs cannot place the tag in the padding: 56 B |
| F0-3 | `OptF64`: optional numbers in 8 bytes, "absent" = a signalling-NaN payload JS cannot produce (2026-11-10: now the integer 0, G22, see uo-bindings.md) (values are NaN-canonicalized on entry, as the codec does) | `Option<f64>` is 16 B; RA1.2 needs absent ≠ NaN |
| F0-4 | `ItemList` (items + glyph/run/text/path columns) is the storage of pages **and** of every intermediate item array; `push_shifted` implements `shiftItem` in uo-render (glyph x only when `dx` is truthy, a clip push without `y` untouched, `M'(p+d) = M(p)+d`) | items move between arrays by copying column ranges; the arithmetic is app-neutral |
| F0-5 | `LineRef.path` is the **TS ref path** with the table cell by GRID column (`c${slot.lcol}`, table.ts:1060), a type of its own (`RefPath`), not uo-docx `BlockPath` (cell index in the row) | the shell resolves line refs with `cellAtGrid`; both meanings exist in the TS |
| F0-6 | `LineStory::Note` carries the note id as `NumKey` (printed with `String(n)`); `HfWhich` is uo-docx's | the TS id is a number; no duplicate enum |
| F0-7 | The provider is the registry's `FontProvider` (`&self`, `Send + Sync`); `FaceEntry` holds the `Arc<FontAnswer>` (pins the face while the layout is being built) | frontier P landed its trait first; one trait for every policy |
| F0-8 | uo-render `FaceRes` = `face: Option<Face>` (TS `font`) + `bytes: Option<Arc<[u8]>>` (TS `bytes`, absent = not embeddable) + `advances` (PQ1); `ImageRes` gains `svg` and `width_twips`/`height_twips` | the TS objects have them and the painters or the page fill read them (`svg`: viewer:962) |
| F0-9 | Engine exceptions to "TableLayout owned by its stack frame" (K18): `floatCarry` owns the `TableLayout` (the TS keeps it alive across pages), `lastRowEmit` owns a clone of the row; `chains` moved to `LDoc` (pure, per call); `full` (swapped by the endnotes) is the `story` index; `PartialEq` over the flow state is deferred to the milestone-2 checkpoints | follow the TS lifetimes exactly |
| F0-10 | `SectGeom.headers/footers` hold the story index per slot `[default, first, even]` | K13: one part may serve several slots |
| F0-11 | `PassItems`/`ItemSide`: `ItemMeta`, `lineBox`/`lineNum` and `ClipItem.src` travel next to each pass-scope item and are copied by `shiftItem` | the TS `shiftItem` copies `LATE_ITEMS` and spreads the fields (RQ2, K2e) |
| F0-12 | The stage-3 placeholder is `LayoutError::Unported(Frontier, site)`; the infallible stubs are listed in `PENDING` consts | the brief: no `todo!`/`panic!`, exact signatures |
| F0-13 | `JsonBuilder` writes the layout's own keys over `uo_jsnum::json`; model values come from the uo-docx `JsonWriter` (`raw`) | no second JSON implementation (K9, LQ20) |
| F0-14 | Canonical stream decisions (canon.mjs header): faces and images labelled by first appearance with their description once; optional booleans by truthiness; strings as UTF-16 bytes; `PlacedObject.run` printed as the run's model position; the object's `drawing` is not in the stream (not a TS field) | exact, identity-aware, lone surrogates kept |
| F0-15 | P3 facets by an in-memory ESM loader rewrite of the frozen dist (`module.register`) plus prototype patching | module functions cannot be patched from outside; nothing on disk changes |
| F0-16 | CI mode font isolation through node-fonts' own roots (env) instead of a new provider | the oracle policy stays the TS one |
| F0-17 | The transcript is TSV (percent-encoded strings), not JSON | no JSON parser in the Rust tests |
| F0-18 | The Write section schema (`write.uos`) carries `run` as the run ref string and `pfmts` by value; `lineBox`/`lineNum` are not encoded (RQ2) | the shell's Write code reads strings; K20 |
| F0-19 | `LayoutLimits` first numbers: 100,000 pages, 4·10⁶ cells per table, depth 192, parser depth 256, 10⁶ metafile records/items, 65,536-unit field texts; work = 5·10⁷ + 4,000 × glyphs × (5 + min(balanced, 20) × (⌈log₂ h⌉ + 2)) × 4^nesting(≤ 8) | K16 shape; replaced by measurements at gate 2 |

### Wave-1 briefs (for the orchestrator)

Common to every frontier (all Opus): work only in the files named; compile against the F0 types
(change a shared type only with a line in this record); `fmt`, `clippy --no-deps -D warnings` (native
and `--target wasm32-unknown-unknown`), `core/tests/float-casts.sh` and `grep-bans.sh` clean; no
`todo!`/`panic!`/indexing; every `Math.*` through `uo_jsnum`; each removed stub leaves its
`Unported`/`PENDING` entry. Declare in this record, section "Stage 3 — F<n>", the uo-docx A7 items
1–9, the LA3 key of every identity site, every `json_key` site and every limit check with its TS
behaviour. Facet parity (P3) runs as soon as the D1a reader reaches digest parity (LQ22); before
that, unit facets over captured inputs. Capture tools: extend `tools/lib/facets.mjs` (the `WRAPS`
table and a recorder) with the frontier's facet, and write its Rust twin next to the module. Record
wall time, rounds and tool calls in "Process metrics".

**F1 — fonts and the width model** (≈ 920 lines). TS: `fonts.ts` (355: `faceMetrics` 57, `VERT_TABLE`/
`vertTable` 122, `superscriptRaiseFor`/`subscriptDropPt`/`superscriptRaisePt` 132–141,
`isSymbolFont` 150, `parsePostScriptName` 170, `genericFor` 193, `FaceResolver` 224 — the Word
substitution chain over the provider, its cache keyed `${family}|bi` per call (K4) — and
`withEmbeddedFonts` 315 as a registry-side wrapper), `context.ts` (`DocContext` constructor,
`saveFlow`/`restoreFlow`/`pure`, `beginNoteSection`/`beginNotePage`, `noteMark`/`noteMarkOf`/
`allNoteMarks`; **not** `directRPr`/`directPPr`, which are F2), `typography/lineheight.ts`
`deviceEmPt` (done), `typography/font/sfnt.ts` `kernTwips`/`kernGlyphsTwips` (198–213, K7), the
width model of `para.ts` (`wordWidth` 349, `toWordUnit` 355, `scaledUnits` 365, `spacingTwips` 373,
`advanceTwips` 378–405, `roundAdvance` 411, `kernGlyphs` 423, `kernTwips` 540) with `advanceEm`
fixed to `'nominal'` and `advanceRound` to `'word'` (LQ12), `math.ts` `parseFont`/`infoCache` (43; a
value on the face table, uo-fonts S14). Rust: `fonts.rs` (`face_metrics`, resolver, `FaceTable`
growth: a new `AnswerId` gets the next `FaceId` and a `FaceRes` in `Resources`), `context.rs`,
`para.rs` (`advance_twips` and the helpers), `math.rs` (font info). Inputs: the registry's
`FontAnswer` (`face()`, `bytes()`, `advances`), `uo_fonts::Face` metrics/kerning/VDMX. Facets:
`p2first` (the resolver's first request per key and answer identity, already captured) plus a new
`p3.metrics` (`faceMetrics` per face) and `p3.advance` (`advanceTwips` per call). Pitfalls: the
FaceResolver cache is **per call** and asks the provider once per key where the TS asks again in
every pass; identity is `AnswerId`, never bytes; `withEmbeddedFonts` precedence ("variant not embedded
but the family exists on the system → prefer the system"); the East Asian 1.3× rule; `metricsCache`
is a `WeakMap` → per face entry.

**F2 — Word semantics in `uo_docx::word`** (≈ 1,400 lines; LQ3). TS: `resolve.ts` (414:
`StyleResolver`, `cascadeRPr`, `mergePPr`/`mergeRPr`/`mergeTblPr`/`mergeTcPr`, `resolveColor`,
`resolveDrawingColor`, `shadingFill`, `highlightHex`, `fontOfSlot`, `COND_ORDER`), `paraformat.ts`
(31; `CTX` WeakMap → per call, K3/LQ18), `context.ts` `directRPr` 245/`directPPr` 317 (+ `setFx`,
`setOt`, `eq` → `json_eq`), `para.ts` `resolvePPr` 682, `numbering.ts` (236: `NumberingState`,
`formatNumber`; `repeat` on huge counters → capped by `max_field_text`, K11b), `datefmt.ts` (103;
local getters through `Clock`/`TimeZoneRule`, LQ10), `field-formula.ts` (129; `parseArithmetic`
with a depth guard = `max_parse_depth`, K11c), `outline.ts` (185), `numpages.ts` (30),
`revisions.ts` (178: `RevisionDisplay`, `revisionRanges`, `joinDeleted` data, `AUTHOR_PALETTE`),
`hyphenate.ts` `documentHyphenationLanguages` 65–80 as an explicit list of every `lang` site (K5).
Rust: `core/crates/uo-docx/src/word/` (module doc already there), then replace the placeholders of
uo-docx-layout `src/word.rs` with re-exports. Inputs: the uo-docx model (`DocFormatting`, props with
`KeyOrder` for `json_key`), `uo-text` (`case::to_uppercase`/`to_lowercase` over `&[u16]`, `space`),
uo-jsnum (`parse_int`, `to_js_string`, `to_fixed`), uo-drawingml colour modifiers. Facets:
`p3.style` (`StyleResolver` outputs per paragraph/run), `p3.numbering` (markers), `p3.datefmt`,
`p3.formula`, `p3.hyphlangs` (the walk), each wrapped in `facets.mjs`. Pitfalls: plain-object key
order (integer-like `styleId`s first — `Object.values(styles).find`), the `{ ...base.pBdr,
...over.pBdr }` insertion order that `bdrKey` observes (K9), regex sites as hand matchers from a
spec table (97 sites over the ported files), `Map` orders, numbering clones in `pure()` (the clone
count is charged).

**F3 — drawing values and metafiles** (≈ 1,100 lines after RQ1). TS: `geometry.ts` 356–475
(`substPhClr`, `withThemeStyles`, `shapeExtras`, `groupChild`: build `uo_render::types::ShapeGeom`/
`GradFill`/`ShapeShadow`/`Pattern`/`Blip`/`ShapeFx` — the path evaluation is `uo_render::geom`,
frontier G), `picture.ts` `pictureLook` (≈ 75 → `PictureLook`), `page-fill.ts` `resolvePageFill`
(34: a **derived image per pass**, RQ3), `imagePixelInfo`, `patternRowsFromImage`, `textfx.ts`
`fxColor`, `fillOf`, `resolveTextFx` (→ `ResolvedTextFx`, registered in `Resources.text_fx`),
`emf.ts` (633) and `wmf.ts` (148) → `metafile.rs` (`render_emf`/`render_wmf` over `FaceLookup`;
bitmap records → BMP bytes and EMF+ images → `ImageRes` with `ImageOrigin::Derived`, K12; records
and items bounded by `max_metafile_records`/`_items` with a warning). Rust: `drawing.rs`,
`metafile.rs`. Inputs: uo-docx `ShapeStyle`/`DrawingGroup`/`PageBackground`/`TextEffects`/`Theme`,
uo-drawingml (colour mods, `ImageEffects`), uo-image (`imagePixelSize`/decoders for page fills and
blip natural sizes), F1 (faces for EMF text). Facets: `p3.shape` (`withThemeStyles` +
`shapeExtras` outputs), `p3.picture`, `p3.pagefill`, `p3.textfx`, `p3.emf`/`p3.wmf` (items per
metafile in the P1 item encoding). Pitfalls: colours stay the strings the document wrote (RQ6), the
argument spreads of `emf.ts:289, 314` that throw `RangeError` in V8 succeed here (accepted, K11a),
`ClipItem.src` goes to `ItemSide::clip_src`, image identity per TS object (one `ImageId` per drawing
and per derived creation).

**F5 — lines and paint** (≈ 1,380 lines; after F1). TS: `lines.ts` (689: `nextTab`, `moreTabs`,
`lineBox` 107, `LineBreaker` 140 — seed `experiments/rust-spike/src/linebreak.rs`, 0 mismatches on
13,353 lines, written against its own `linedata.rs`: re-target to `para::Glyph`/`PreparedPara` and
`lines::Line`), `underline.ts` (184: `ulMetrics` 20, `ulGeometry` 36, `paintUnderlines` 74),
`paint.ts` (504: `paintLine` 19, leaders, math, check boxes, inline objects, `o:hr`, decorations,
`borderWidth` 290, `borderBands`, `borderPaintWidth`, `paintBorder` 368, `paintWave`,
`paintParaBox` 430, `pageBorderItems` 478; `lineSeq` → `keys::LineSeq` per call). Rust: `lines.rs`,
`underline.rs`, `paint.rs` (emit into `PassItems` with `GlyphMeta` for `lineBox`/`lineNum`; glyph
runs merge only across the same `FaceId`, K2a; `LineDash` for border dashes, RQ13). Inputs: F1
(`advance_twips` through `Cx`, hyphenation through `Cx.hyph`), F0 types. Facets: `p3.lines`
(captured now: frame, every line field, placed x/w/leader) extended with glyph identity
(`GlyphSrc`), plus `p3.paint` (items of each `paintLine` in the P1 item encoding). Pitfalls:
`Math.max(...[])` = −∞ in `nextTab`; tabs relative to the frame's left edge with the default grid
from 0 and the hanging indent as an implicit stop; the automatic hyphen is a synthetic glyph
(`GlyphSrc::AutoHyphen`), not a paragraph glyph.

### Not done (handed on)

- The real replay (`tests/layout_parity.rs` `rust_layout`) waits for the D1a reader and F7a; the P2
  and P3 facets are captured but not yet compared by Rust (their writers come with F1, F4, F5).
- Embedded-font documents are not decodable in the adapter (bytes come from the reader): 1 of 2,742.
- The production hyphenation sweep (`documentHyphenationLanguages` ∩ `['pt-BR','en','es']`, K5) needs
  one process per document (loaded patterns only grow): a `--hyph app` mode for wave 1.
- The São Paulo date facet (LQ10) and the per-counter maxima of the limits are not captured yet.
- The CI job `layout-parity` is new and has not run on GitHub (its TS build commands follow `ci.yml`).
- M1–M5 memory tests start with the first ported stage (no allocation path exists yet).
- uo-drawingml does not host `CustPath` yet: uo-render defines it (to become a re-export).

## Wave 1 — F1 (fonts and the 600-DPI width model)

Opus, 2026-11-03, hybrid mode, one session (≈ 1.5 h) in parallel with F3, F5, the uo-docx D1a
foundation and uo-image. 0 parity rounds: every facet was equal at its first replay.

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| `fonts.ts` `faceMetrics` 57 (DirectWrite metrics, East Asian 1.3× rule, `metricsCache`) | `fonts::face_metrics` (infallible); the metrics live on `FaceEntry`, computed when the answer enters the table |
| `fonts.ts` `VERT_TABLE`/`VERT_ALIAS`/`vertTable` 108–129, `superscriptRaiseFor` 132, `subscriptDropPt` 137, `superscriptRaisePt` 141, `isSymbolFont` 150 | `fonts::{superscript_raise_for, subscript_drop_pt, superscript_raise_pt, is_symbol_font}` |
| `fonts.ts` `ALIASES` 161, `parsePostScriptName` 170, `genericFor` 193, `CJK_DEFAULT` 215, `FaceResolver.get`/`lookup` 235–300 | `fonts::{parse_post_script_name, FaceResolverCache::get}` over a `FaceEnv` (provider, font table, face table, resources) + `Cx::face`; `Err(NoFace)` where the TS throws |
| `fonts.ts` `withEmbeddedFonts` 315 | `fonts::EmbeddedFontProvider::{wrap, new}`, a registry-side `FontProvider` over the host one |
| `sfnt.ts` `kernTwips`/`kernGlyphsTwips` 183–199 (K7) | `fonts::{sfnt_kern_twips, sfnt_kern_glyphs_twips}` over `uo_fonts::Face::kern_units` |
| face identity (K2a) | `FaceTable::intern`: a new `AnswerId` gets the next `FaceId` and a `FaceRes` (key, family, flags, face, bytes, advances) in `Resources`; `entry`/`face`/`metrics`/`math_info` |
| `para.ts` `WORD_UNIT` 347, `wordWidth` 349, `toWordUnit` 355, `scaledUnits` 365, `spacingTwips` 373, `SPACE_WIDTHS` 335, `advanceTwips` 378–405, `glyphAdvTwips` 432, `kernGlyphs` 423, `kernTwips` 540 (`advanceEm: 'nominal'`, `advanceRound: 'word'`, LQ12) | `para::{WORD_UNIT, word_width, to_word_unit, scaled_units, spacing_twips, advance_twips_font, glyph_adv_twips, kern_glyphs, kern_twips}` over `&uo_fonts::Face`, and `advance_twips`/`advance_twips_w` over `Cx` + `FaceId` |
| `context.ts` constructor, `warn`, `saveFlow`/`restoreFlow`/`pure`, `beginNoteSection`/`beginNotePage`, `noteMark`/`noteMarkOf`/`allNoteMarks`, `endnoteOrder` | `context::DocContext::{new, warn, save_flow, restore_flow, pure, begin_note_section, begin_note_page, note_mark, note_mark_of, all_note_marks, endnote_order}`, `NoteNumberings`, `NoteKindFlow`, `note_key` |
| `math.ts` `fontInfo`/`infoCache` 43–57, `parseFont` 59, `box` 86, `forCoverage` 95 | `math::{MathFontInfo, parse_math_font}` (on `uo_fonts::read::View`, its own directory rules, S14), `MathFontInfo::glyph_box`; cached per face entry (`FaceTable::math_info`) |

`Unported(F1, …)` is gone from the crate.

### Parity (P2/P3 facets; TS capture regenerated in the same run, LQ21)

Facets added in `tools/lib/facets-f1.mjs` (hooked by `facets.mjs`; `capture-layout.mjs` writes their
input dumps `<key>.f1.tsv`): `p3.metrics` (`faceMetrics`, first call per font object), `p3.advance`
(`advanceTwips`, every distinct input), `p3.kern` (`SfntFont.kernGlyphsTwips`, every distinct
input), `p3.faces` (`FaceResolver.get`, every distinct request over all passes); plus the existing
`p2first`. `tests/f1_parity.rs` recomputes every value from the dumped inputs, compares each bit for
bit with the TS value and rebuilds the five streams; `p2first` comes from the Rust resolver's own
provider calls over the transcript replay (the TS answers and identities).

| Corpus | Documents | Facets equal | Mismatches |
|---|---|---|---|
| committed fixture `tests/captures/f1` (CI mode, seed 1, 2 documents) | 2 | 10 | 0 |
| CI mode, 200 generated documents (seed 7), open fonts | 200 | 1,000 | 0 |
| Word oracle, all documents, installed fonts | 2,741 (+1 skipped: `emwidth/ew-emb`, embedded-font bytes are not in the index, as in F0) | 13,705 | 0 |
| resolver sweep `tools/capture-f1-resolver.mjs`, committed (40 scenarios) | 40 | 80 | 0 |
| resolver sweep, 5,000 scenarios (seed 11) | 5,000 | 10,000 | 0 |

Values compared in the oracle run: 3,114 font metrics, 95,813 distinct `advanceTwips` calls, 4,204
distinct kerning pairs, 3,173 distinct resolver requests, 88 font-table entries. The corpus barely
exercises the substitution chain (≈ 1 family per document, node-fonts never answers a stand-in), so
the **resolver sweep** runs the frozen `FaceResolver` over seeded random font tables (charsets,
generic families, `altName` lists, PANOSE, `w:sig`) and requests (aliases, PostScript names, East
Asian names, blanks, case and white space) over the open fonts behind a viewer-like stand-in wrapper:
31,691 requests, 9,635 table entries, 16,024 stand-in faces described. Sensitivity: disabling the
stand-in re-check gives 542 mismatches, swapping Arial/Sakkal Majalla 46. Timing: TS oracle capture
30 s; Rust replay 33 s (release). The CI job `layout-parity` gains the sweep (2,000 scenarios) and
the `f1_parity` replay of its 200-document capture.

### Memory and robustness (`tests/f1.rs`)

| Check | Result |
|---|---|
| M1: resolver + face table + embedded provider (with a document ledger) + widths + kerning + math info, private `FaceCache` | 33,718 B allocated, 10,502 B peak, **0 B left** |
| M1: `DocContext` (100 note marks, 50 under `pure`, `all_note_marks`), `parse_math_font` + `glyph_box` | **0 B left** |
| M5: the embedded provider's answer cache (cap 16,384) filled with 16,484 requests | held at the cap; past it a new answer per request |
| no panic: 1,500 mutated fonts (handcrafted with MATH/glyf/kern/post, and Carlito) through `face_metrics`, 40 advances and kernings each (NaN, 0, negative, 1e300 sizes and scales), `MathFontInfo` and `glyph_box` | 756 parsed, 1,044 math infos, no panic |
| no panic: 3,000 random font tables and families through the resolver | ok |
| hostile MATH coverage (2,000 ranges × 65,535) | stops at the step cap → fallback info |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`round`, `floor`, `max`, `min`); `parseInt` through
   `uo_jsnum::parse::parse_int` (`vertTable` base 36, `genericFor` hex, `w:sig` `usb0` hex with
   `to_int32` for `& (1 << 13)`); `String(id)` through `to_js_string`; `w !== 100` an exact `f64`
   compare (NaN scales). **No arithmetic-semantics change**: the TS order of operations everywhere.
2. **Regex**: `parsePostScriptName` (fonts.ts:171) as a hand matcher (the first `-`/`,` is the
   separator because the first group excludes both, the rest must equal one suffix, JS `$` is the
   end of input); `/MT$/`, `/PS$/` → one `strip_suffix` each; `/([a-z])([A-Z])/g` → a left-to-right
   non-overlapping scan; `altName.split(/[,;]/)` → `split([',', ';'])`. None dynamic.
3. **NaN/±∞ and absent keys**: `supOffset` absent = `None`; advances propagate NaN sizes as the TS;
   note ids are JS numbers → `NumKey` (NaN and −0 behave as `Map` keys).
4. **Orders**: `FaceId` = first use of an answer identity; `NoteFlow` marks in `IndexMap` (insertion
   order; `allNoteMarks` footnotes then endnotes); the font table through `JsRecord` (own keys); the
   resolver cache, the face table index and the embedded cache are `FxHashMap` (only `get` is
   observable).
5. **Recursion**: `advanceTwips` recurses once at most (`SPACE_WIDTHS` strings point at U+0030,
   U+002E, U+0020, which are not in the table). No parser recursion.
6. **Module state**: `metricsCache` (`WeakMap<LayoutFont>`) → `FaceEntry.metrics`, computed eagerly
   (pure); `infoCache` (`WeakMap` keyed by the font object) → `FaceEntry.math`, per answer (same
   values: two answers over one font object compute the same info); the `FaceResolver` cache per
   layout call (K4, LQ19).
7. **try/catch**: an embedded font that does not parse is ignored (fonts.ts:326); `fontInfo`'s catch →
   `MathFontInfo::fallback`; the `box` reads of `loca`/`glyf` are outside that `try` → `Err(Internal)`
   (the TS throws out of `layoutDocument`); the resolver's throw → `Err(NoFace)`.
8. **Aliasing / identity (LA3)**: `FontFace` objects (fonts.ts:225, 285, 328) → `AnswerId` → `FaceId`;
   two answers over the same bytes are two faces (K2a). No `json_key` site in F1 (`eq` of context.ts
   is F2's `directRPr`).
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F1-1 | The provider is asked once per lower-case key **per layout call**, not per pass (LQ19). Observable only through cross-pass identity of a policy that makes fresh objects (`server-substitute`: the TS has new stand-ins each pass, Rust one per call); nothing in a pass compares faces of another pass | K4, approved shape |
| F1-2 | `face_metrics` is infallible (the F0 stub returned `LResult`) | the TS cannot throw there |
| F1-3 | `DocContext` has no `faces`, `opts` or `docDefault` (`void this.docDefault` in the TS); `DocContext::new(resolver, numbering, rev)` takes the F2 parts; `note_numbering` is `NoteNumberings` with the TS defaults (F0 had `Option<(…)>`); note ids are `NumKey` (F0 had `i32`) — `Glyph.note_id` and `FieldSource::Footnote/Endnote` in `para.rs` are still `i32` (F4/F7c convert) | TS `Map<number>` semantics |
| F1-4 | `note_mark(kind, id, custom, lang, format_number)` takes `formatNumber` as a parameter until F2 lands it in `uo_docx::word` | F2 runs separately |
| F1-5 | `pure` moves the snapshot back instead of cloning it (the TS clones in `restoreFlow`; the snapshot is not reused) | same state, one clone less |
| F1-6 | `EmbeddedFontProvider` keeps answers per TS key up to `MAX_EMBEDDED_ANSWERS` = 16,384 (past it: a new answer per request, the registry's R7 rule); with a document `FaceLedger` a face the budget refuses is ignored (LA6: a miss) | M5, LA6 |
| F1-7 | `parseFont`'s italic coverage walk skips the steps the TS does nothing in (`idx >= cnt`, exact) and stops past `MAX_MATH_COVERAGE_STEPS` = 2²² steps: the font counts as malformed (fallback info). The TS would walk up to 4.3·10⁹ steps | LQ14 (where the TS effectively hangs) |
| F1-8 | Kerning through `uo_fonts::Face::kern_units` and advances through `advance_width` (uo-fonts gate-2 rules; a read past the end is 0 there where `SfntFont` throws, uo-fonts G4) | reuse, decided in uo-fonts |
| F1-9 | `advance_twips` keeps the F0 signature (w = 100); `advance_twips_w` takes `w`; the `*_font` functions take a `&uo_fonts::Face` for F4/F5 and the tests | the TS default parameter |
| F1-10 | Case mapping of family names and the font-table charset through `uo_text::case` (Node 24.15 tables); frontier P keys its policies with `uo_jsnum::text::to_lowercase` (std tables) — both are JS `toLowerCase` up to Unicode-version deltas, and only cache keys depend on them | exact V8 behaviour here |
| F1-11 | `p3.advance`, `p3.kern` and `p3.faces` record **distinct inputs** in first-call order (pure functions; per-call resolver cache), not every call; `p3.kern` is taken at the `SfntFont` level (prototype patch), the para-level rescaling is unit-tested; the facet object of `facets.mjs` carries the input dumps as a non-enumerable `dumps` property (two lines in `capture-layout.mjs`) | dump size (the per-call form was 0.7 MB per generated document) |
| F1-12 | A new capture tool, `tools/capture-f1-resolver.mjs`, with a stand-in wrapper (one object per request key, as the viewer's per-call cache) | the corpus does not exercise the chain |
| F1-13 | Additive shared edits: `model.rs` re-exports `FontInfo`, `JsRecord`, `EmbeddedFont`; `Cx::face` is an inherent `impl` in `fonts.rs`; the `layout-parity` CI job gains two steps. One `cargo fmt -p uo-docx-layout` early in the session also formatted F3/F5 files in progress (formatting only) | — |

Limit checks: the math coverage step cap (F1-7, new); `NoFace` (TS throw); the embedded answer cap
(F1-6); the document face budget through the ledger (LA6).

### Not done (handed on)

- The `"font budget"` warning (LA6): a `FontProvider` miss does not say it came from the budget; the
  registry/policies need a signal (or the engine checks the ledger) — F7a with the binding.
- `formatNumber` for `note_mark` comes with F2; the `i32` note ids of `para.rs` (F0) with F4/F7c.
- The per-function facets are replayed from dumped inputs; the same facets through the Rust engine
  wait for the D1a reader and F7a (LQ22). The embedded-font document needs the reader's bytes (or a
  dump of embedded font bytes in the capture).

## Wave 1 — F3 (drawing values and metafiles)

Opus, 2026-11-03, hybrid mode, one session (≈ 2 h) in parallel with F1, F5, the uo-docx D1a
foundation and uo-image. **1 parity round**: the generated metafile differential found one port bug
(below); every other facet was equal at its first replay.

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| `geometry.ts` `substPhClr` 356, `withThemeStyles` 366, `shapeExtras` 389, `groupChild` 438 | `drawing::{subst_ph_clr, with_theme_styles (Cow), shape_extras → ShapeExtras { grad, shadow, pattern, blip: BlipRef, fx }, group_child → GroupChild { run: Cow<Run>, x, y, w, h }}` |
| what the painters read of `ShapeItem.style` (RQ1) | `drawing::shape_geom → uo_render::types::ShapeGeom` (preset, `adj` in JS own-key order, custom paths in `Resources.cust`, flips, rotation, `fillAlpha`, `line.alpha`, `GeomEval::Compact`); `drawing::resolve_color` = `resolveDrawingColor` of a model colour (the engine's shape fill and stroke) |
| `picture.ts` `color` 13, `pictureLook` 22, `blipLook` 77; docx-core `picture-xml.ts` `dmlColorSpec` 136 | `drawing::{picture_look → Option<PictureLook>, dml_color_spec}`; `drawing::resolve_color_str` = `resolveDrawingColor` over a spec **string** the layout builds (`split('|')`, `split(',')`, `split('=')` as the TS) |
| `page-fill.ts` `resolvePageFill` 22, `imagePixelInfo` 176, `patternRowsFromImage` 207; docx-core `resolveColor2` (page-background.ts:359), `vmlColor` (drawing.ts:471) | `drawing::{resolve_page_fill → Option<PageFill>, image_pixel_info, pattern_rows_from_image, resolve_color2, vml_color}` (decoders and headers from uo-image) |
| `textfx.ts` `fxColor` 34, `fillOf` 41, `resolveTextFx` 54 | `drawing::{fx_color → (hex units, alpha), resolve_text_fx → Option<ResolvedTextFx>}` |
| `emf.ts` (633: `isEmf`, `renderEmf` with every record it plays, `dib`, `emfRastersDest`, `emfPlusOnly`, `emfPlusBitmap`, `EmfPlusPlayer`) | `metafile::{is_emf, render_emf}` (+ private `EmfPlayer`, `EmfPlusPlayer`, `emf_plus_bitmap`) |
| `wmf.ts` (148: `isWmf`, `renderWmf`) | `metafile::{is_wmf, render_wmf}` |
| the metafile branch of `paintObject` (engine.ts:3605–3608) and `paintInlineObject` (paint.ts:202–210) | `metafile::paint_metafile` (clip push with `src` in `ItemSide::clip_src`, the player's items, clip pop; `Ok(false)` for other bytes) |

Entry points for the engine: `FaceLookup` (`face(res, family, bold, italic) -> LResult<FaceId>`,
`warn`) is all the players need (LQ7; the face's metrics, cmap and advances are read from
`Resources.faces[i].face`, the TS `face.font`), and `ResWriter { res, origins }` is where F3 writes
symbols, derived images (with `ImageOrigin::Derived`) and custom paths. `Unported(F3, …)` is gone
from `drawing.rs`/`metafile.rs` (F5's `paint_inline_object` stub can now call `paint_metafile` and
`picture_look`).

### Parity (P3 facets; TS capture regenerated in the same run, LQ21)

Facets added in `tools/lib/facets-f3.mjs` (hooked by `facets.mjs`: `p3.emf`, `p3.wmf`, `p3.picture`,
`p3.pagefill`, `p3.textfx`, `p3.shape`; each call's value is the P1 canonical stream of a one-page
layout holding the call's result, so the Rust side writes it with `canon_layout`), with the dump
`<key>.f3.jsonl` of every call's inputs (model objects as JSON, bytes by digest with the media in
hex, non-finite numbers and −0 kept, the theme, the faces the pass's resolver had answered).
`tests/f3_facets.rs` decodes each call (`tests/f3_support`: a JSON reader and the model decoders)
and compares its value bit for bit; with `UO_F3_CAPTURE` it also checks the per-document `p3.*`
aggregates. The corpus has 26 EMF and 2 WMF calls and no EMF+, so a new tool,
**`tools/capture-f3.mjs`**, generates seeded EMF (GDI and EMF+ only) and WMF files covering every
record the players read — transforms, map modes, save/restore, pens/brushes/fonts, paths, every poly
record, text with and without Dx, glyph indices, alignment, the three bitmap records with 24/32 bpp
DIBs and premultiplied alpha, ROP3 without a bitmap, EMF+ raw/compressed/continued images, crops,
int16/float points, page units, Save/Restore — plus malformed ones (short records, huge counts,
truncation), played by the frozen `renderEmf`/`renderWmf` over the repository's open fonts.

| Corpus | Calls compared | Mismatches |
|---|---|---|
| committed `tests/captures/f3`: the 8 .NET synthetic inputs (pictures, page fills ×4, textboxes, text effects, synthetic) | picture 22, pagefill 4, shape 33, textfx 384 | 0 |
| committed `tests/captures/f3/metafile-gen.f3.jsonl` (`capture-f3.mjs --seed 1`) | 320 EMF (≈ 20 % EMF+), 160 WMF; 41 TS throws reproduced as `Err` | 0 |
| generated, seeds 2–6 (local) | 7,500 EMF, 3,500 WMF; 992 TS throws reproduced | 0 |
| Word oracle, all 2,742 documents, installed fonts (`p3.*` aggregates also equal) | emf 26, wmf 2, picture 752, shape 445, textfx 183,691 | 0 |

Item coverage of the committed generated set: 901 EMF paths, 67 EMF glyph runs (14 `asPath`), 150
images (EMF bitmaps and EMF+, 1 cropped), 17 ROP rectangles, 397 WMF paths, 251 WMF glyph runs.
Round 1: `POLYBEZIER` with no point — the TS reads `pts[0]![0]` and throws a `TypeError`; the port
had skipped the move. Fixed (`Err(Internal)`); found by seed 1. A harness issue on the way (not a
port bug): the generator first indexed the main checkout's ignored local fonts (Calibri); it now
indexes the fonts this repository tracks. Timing: TS oracle capture ≈ 35 s; Rust replay of the
oracle dumps 2.2 s (release).

### Memory and robustness (`tests/f3.rs`)

| Check | Result |
|---|---|
| M1: `render_emf` on a 1,088-byte generated EMF | 7,414 B allocated, 3,982 B peak, **0 B left** |
| M1: `render_wmf` on a 194-byte generated WMF | 2,910 B allocated, 2,140 B peak, **0 B left** |
| M1: `shape_geom` + `shape_extras` (theme of `pictures.docx`), `resolve_page_fill`, `subst_ph_clr`, `resolve_color2` | **0 B left** |
| M5 | no cache in F3 |
| limits (LQ14): records, items, payload each cut a 50-rectangle EMF at the limit, one `metafile truncated` warning, what was drawn kept | ok |
| TS throws kept (LQ9): short header, a count past the end, POLYBEZIER without points | `Err(Internal)` |
| no panic: the 480 generated metafiles mutated (byte noise, sign flips, random 32-bit counts/sizes, truncation, insertion), 1 in 4 with tiny limits: 9,600 runs (release) | 7,111 drawn, 2,489 errors where the TS throws, no panic |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`min`, `max`, `abs`, `round`, `floor`, `hypot`,
   `sign`, `sin`, `cos`, `%` = `rem`); `parseInt` through `parse_int`/`parse_int_utf16`;
   `toString(16)` through `to_radix_string` (NaN prints `NaN`); `String(n)` through `to_js_string`;
   `getFloat32` → `f64::from(f32)` (exact); integer reads → `f64::from`. Offsets and the EMF+ row
   arithmetic in `i64` (exact where the TS `f64` is: below 2⁵³, and `stride * h > length` gives the
   same answer because rounding is monotonic and the length is exact). **No arithmetic-semantics
   change.**
2. **Regex**: `resolveColor2`'s `/^\s*fill(?:\s+(darken|lighten)\((\d+)\))?\s*$/i` as a hand matcher
   (non-`u` `/i`: no non-ASCII character canonicalizes to these ASCII letters); `vmlColor`'s
   `/\s+/` through JS `trim` + `split_js_space`; `/^[0-9A-Fa-f]{6}$/` through `is_hex6`. None dynamic.
3. **NaN/±∞ and absent keys**: a model `null` (`JsNumber::Null`) is nullish for `??`, present for
   `!== undefined`, 0 in arithmetic and falsy, as in JS; an optional number written only when
   `!== undefined` becomes `OptF64::NONE` for `null` too, because the canonical stream writes
   `optNum(null)` as absent. Metafile coordinates propagate NaN as the TS (`emTw < 1` false for NaN).
4. **Orders**: `adj` in JS own-key order (`JsRecord`); `fillOf`'s stop sort is `sort_by` with the TS
   comparator (stable as V8's TimSort; equal for consistent comparators); EMF objects, EMF+ images
   and saves are `FxHashMap` (only `get`/`set`/`delete` observable); WMF objects keep the array with
   first-`null` reuse; `RESTOREDC` in WMF reproduces `splice(length + k)` with a negative start
   counted from the end again.
5. **Recursion**: none.
6. **Module state**: only constants (`STOCK`, `CP1252`, the colour tables); every player is per call.
7. **try/catch**: none in the F3 TS. TS throws kept as `Err(Internal)`: `DataView` reads past the end
   or at negative offsets, `Uint8Array.set` past the end, `pts[0]![0]` of an empty POLYBEZIER, a
   solid fill without colour or a gradient without stops in `fillOf`, a pattern image the decoder
   finds malformed (uo-image `Malformed`), a face without parsed font; the resolver's throw is
   `Err(NoFace)` (from `FaceLookup`).
8. **Aliasing / identity (LA3)**: derived images get a new `ImageId` per TS object — one per bitmap
   record, one per EMF+ image object (given at its first draw, shared by every draw of it), one per
   `resolvePageFill` call — with the bytes shared by content in the layout (K12, RQ3); the shape's
   blip is the model object (`BlipRef { image: &ImageData }`), mapped to its id by the engine;
   `ClipItem.src` → `ItemSide::clip_src`; faces through `FaceLookup` (`FaceId`); `group_child`'s
   `Cow::Owned` is the TS `{ ...r0 }` copy (a **new** identity: chains and object lookups must not
   take it for the model's run). **No `json_key` site in F3** (F4's `fxKey` stringifies the
   `ResolvedTextFx` it gets from here).
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F3-1 | Metafile limits (LQ14): `max_metafile_records`, `max_metafile_items` and `MAX_METAFILE_PAYLOAD` = 2²⁰ path commands + glyphs (a const in `metafile.rs`, not a `LayoutLimits` field, to leave the shared type alone; gate 2 may move it). A hit stops the drawing, keeps what was drawn and warns `metafile truncated` once; the TS 200,000-record guard is kept. An EMF+ bitmap above `uo_image::ImageLimits::DEFAULT.max_px` is skipped the same way. The TS has no bound: a record may declare more points than it holds and read the records after it, so its work grows with records × file size | security §7 |
| F3-2 | Signatures fixed: `render_emf`/`render_wmf(bytes, dest, MetafileLimits, &mut dyn FaceLookup, &mut ResWriter, &mut PassItems) -> LResult<()>` (F0 had no limits/resources and returned `LResult<bool>`); the `bool` moved to `paint_metafile`, which also pushes the clip pair. `FaceLookup::face` takes `&mut Resources` (the face table registers `FaceRes` there) | the borrow split of `Cx`: faces and resources are separate parameters |
| F3-3 | `ResWriter { res, origins }`: `Cx` has `res` but no image origins; F7 adds `origins` to `Cx` or builds the writer from the `Layout` | K12 needs the origin per image |
| F3-4 | `shape_extras` returns `BlipRef` (the model `ImageData`); the engine gives it the id of that model object. Read from `s0`: `withThemeStyles` never sets `blip`, so it is the same object | identity per model object needs the drawing key the engine has |
| F3-5 | `with_theme_styles` returns `Cow` and does not keep the key order of the style it derives (nothing stringifies it) | no JSON site |
| F3-6 | `group_child` clones the run when it transforms it (the textbox content `Story` included) | the TS copy is shallow; the clone is the cost of `Cow` — a transform-as-data variant is a stage-5 candidate if F7 measures it |
| F3-7 | `resolve_page_fill` registers the derived image only when the fill is kept (the TS builds the object before its two checks); a fill type outside the five the reader produces → `None` (only an edit could make one); a pattern image past the decoder limits → no pattern rows (so no fill) + warning `page fill image too large` | resources retained by the layout; uo-image limits |
| F3-8 | `dmlColorSpec` of a `prstClr` whose `val` is an `Object.prototype` key: the TS reads a function from `PRST_RGB` and the colour never resolves; here the head is a non-hex string (same result); `__proto__` without modifiers makes the TS throw a `TypeError` — here no colour | named JS quirk |
| F3-9 | `resolveTextFx`'s `CAP[o.cap ?? 'flat']`: a prototype key gives a function in the TS; here `butt` | `LineCap` is an enum |
| F3-10 | `resolve_text_fx` returns the value; the caller registers it in `Resources.text_fx` (the TS makes a new object per call; the canonical stream writes it by value). `fx_color` returns the hex as UTF-16 units: the fallback (the run's colour) is passed through unchanged | F4 owns the glyph style |
| F3-11 | `vml_color`/`resolve_color2` work on UTF-16 units (the TS splits by unit); upper case through `uo_text::case` (Node 24.15 tables), lower case through `uo_jsnum::text` | exact V8 strings |
| F3-12 | Model colours resolve through uo-drawingml's structured `resolve_word_spec` (the uo-docx G12 named difference applies); strings the layout builds (`dmlColorSpec`, `fxColor`) through `resolve_color_str` with the TS splits | both TS paths kept |
| F3-13 | Custom paths are converted to `uo_render::types::CustPath` (until uo-render re-exports uo-drawingml's) and shared by value in `Resources.cust` | the canonical stream writes them by value |
| F3-14 | The EMF device context is plain `Copy` data: colours as COLORREFs (`colorOf` at emission), font face names in a per-player list; `SETBKMODE`/`SETBKCOLOR` are read (they can throw) but not kept (the TS never reads them) | `SAVEDC` copies without allocation |
| F3-15 | Derived images carry `natural_px` from `uo_image::image_pixel_size`; a blip's `ImageRes` (built by the engine) must have NaN `width_twips`/`height_twips` — the TS object is `{ data, ext }` and the walker writes `num(undefined)` | note for F7 |
| F3-16 | The content dedupe of derived images is a linear scan of the layout's images (length first) | stage-5 candidate (an index on `Resources`) |
| F3-17 | K11a accepted: `path.push(...)` spreads that throw `RangeError` in V8 above ≈ 10⁵ points (emf.ts:289, 314) succeed here | approved (LQ14) |
| F3-18 | Shared files, additive only: `model.rs` re-exports (`Nullable`, `Str`, `MediaStore`, `PageBackground`, `DrawingColor`, `ShapeStyle`, `FxColor`, `FxFill`, `TextEffects`, `Theme`, `ImageData`, `PicBlipFx`, `PicColor`, `DrawingGroup`, `GroupItem`); `Cargo.toml` gains `uo-image`; `facets.mjs` gains the F3 wraps, recorder, reporter call and dump | — |

Limit checks: `max_metafile_records`, `max_metafile_items`, `MAX_METAFILE_PAYLOAD`, the EMF+ bitmap
pixel cap (F3-1), the page-fill decoder limits (F3-7); every one ends with a warning, never an `Err`.

### Not done (handed on)

- The fuzz targets `emf_records`/`wmf_records` of §7 are not in `core/fuzz` (the wave-1 brief did not
  ask for them; `core/fuzz` would need uo-docx-layout, and with it uo-docx in construction, as a
  dependency). `tests/f3.rs` runs the same no-panic loop on stable; the target bodies are that loop.
- The engine's shape `fill`/`stroke`/`line` assembly (engine.ts:3621–3634, paint.ts:221) is F7b/F5's
  (with `resolve_color`, `shape_geom`, `shape_extras`); `p3.shape` does not include the `fill` of
  `withThemeStyles(s).fill` (its theme-style path is covered through the gradient, pattern and shadow).
- F7: `FaceLookup` over `Cx` (F1's resolver), `origins` for `ResWriter`, the id of a blip's model
  image, and derived images per pass: the TS makes new ones every pass, here they stay in
  `Resources` with the layout — F7 decides whether passes share one table (M3).
- The same facets through the Rust engine wait for the D1a reader and F7 (LQ22).

## Wave 1 — F5 (lines and paint)

Opus, 2026-11-03, hybrid mode, one session (≈ 50 min) in parallel with F1, F2, F3, the uo-docx D1a
foundation and uo-image. **0 parity rounds**: every facet was equal at its first replay. Built
against F1's signatures while F1 was in flight (its `to_word_unit`, `advance_twips`,
`FaceTable::metrics/face` are the ones used) and wired to F2's `uo_docx::word::resolve` once it
compiled.

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| `lines.ts` (689: `FIXED_SPACE`, `FIT_EPS`, `nextTab` 76, `moreTabs` 98, `lineBox` 108, `LineBreaker` 140 — `done`, `peekWordWidth`, `indentFirst`, `mark`/`reset`, `fitRight`, `next` with `closePending`) | `lines::{next_tab, more_tabs, line_box, LineBreaker::{new, done, peek_word_width, mark, reset, next, next_in}}`, `Line`, `PlacedGlyph`, `GlyphSrc`, `glyph_of` (the automatic hyphen `{ ...kg, kind: 'shy', … }`) |
| `hyphenate.ts` `HYPHEN_GAP15`, `HYPHEN_EXPAND15`, `HYPHEN_ZONE_DEFAULT`, `hyphenCandidates` 182, `chooseHyphenation` 206 (called by `next`) | `hyphenate::{hyphen_candidates, choose_hyphenation}` (F5-3; `markHyphenation` stays F4's) |
| `para.ts` `shadowExtent` 321 (the empty-line height reads it) | `lines::shadow_extent` (F5-4) |
| `underline.ts` (184: `ulMetrics` 20, `ulGeometry` 36, `PATTERNS`, `HEAVY`, `paintUnderlines` 74, `band`, `wave`, `paintStyle`) | `underline::{ul_metrics, ul_geometry, paint_underlines}` (+ private `band`, `wave`, `paint_style`) |
| `paint.ts` (504: `lineSeq`, `paintLine` 19, `paintLeader` 102, `displayEnd` 140, `paintMath` 160, `paintCheckBox` 180, `paintInlineObject` 196, `paintHr` 234, `paintDecorations` 253, `borderWidth` 290, `borderBands` 319, `toWhite` 342, `BORDER_DASH`, `borderPaintWidth` 357, `paintBorder` 368, `paintWave` 403, `paintParaBox` 430, `pageBorderItems` 478) | `paint::{paint_line, paint_line_in, paint_inline_object, border_width, border_bands, to_white, border_paint_width, paint_border, paint_border_in, paint_para_box, page_border_items, page_border_items_in}` (+ private `paint_leader`, `display_end`, `paint_math`, `paint_check_box`, `paint_hr`, `paint_decorations`, `paint_wave`); `lineSeq` → `Cx::line_seq` (per call, LA3) |

What the TS reads through `this.ctx` and `face.font` (settings, `advanceTwips`, `faceMetrics`, the
run's `w:ptab` and `o:hr` shape, the font's `unitsPerEm`/`usWinDescent`/`descender`/VDMX,
`advanceWidth(glyphId)`, COLR, the face key and synthetic flags, symbols, F2's colours) goes
through two traits, `lines::LineEnv` and `paint::PaintEnv`, implemented by `Cx` (the engine) and by
the facet replay (F5-1). The F0 entry points keep their signatures (`LineBreaker::next(cx, p, frame)`,
`paint_line(cx, p, line, at, out)`).

### Parity (P3 facets; TS capture regenerated in the same run, LQ21)

`tools/lib/facets-f5.mjs` (hooked by `facets.mjs`; `p3.lines` **replaces the F0 seed**): every
`LineBreaker.next`/`reset`/`peekWordWidth` and every `paintLine` call is one `bytes` item of the
facet stream holding the call's raw encoding (`next`: the frame and every line field with the placed
glyph identity — paragraph glyph index, marker glyph index or the automatic hyphen's offset;
`paintLine`: the P1 canonical stream (canon.mjs v1) of a page holding the appended items, faces as
descriptors). The dump `<key>.f5.bin` holds the inputs (faces with `faceMetrics` and the font numbers
the paint reads, run styles with `rpr`, text effects and `!!fontOfSlot(…eastAsia…)`, prepared
paragraphs with every glyph field lines and paint read, the `advanceTwips` answers the breaker asks
for, VDMX/leader/COLR answers, the breakers' settings, each call with its TS raw output).
`tests/f5_parity.rs` decodes them, runs `LineBreaker::next_in`/`reset`/`peek_word_width` and
`paint_line_in` → `canon_layout`, and compares every call's bytes and the per-document facet digests.
The loader hook gained an optional pre-call hook (`__uoFacetPre`, for `out.length` before
`paintLine`); the recorder's own `advanceTwips`/`faceMetrics` calls run with the reporter off, so F1's
streams see only the engine's calls.

| Corpus | `next`/`reset`/`peek` calls | `paintLine` calls | Mismatches |
|---|---|---|---|
| committed `tests/captures/f5` (generated `gen/1/6`, open fonts) | 21 | 21 | 0 |
| generated, CI mode, seed 7, 200 documents (open fonts; the CI job runs its own seed every time) | 57,016 | 57,016 | 0 |
| Word oracle, all 2,742 documents, installed fonts | 142,171 | 139,904 (+ 246 pending: inline objects, F5-9) | 0 |

On the oracle 2,609 documents are equal on both facet digests; the 133 with a pending inline object
are compared call by call. Timing: TS oracle capture with dumps 52 s (913 MB of dumps, not kept);
Rust replay 4.3 s (release).

### Memory and robustness (`tests/f5.rs`)

| Check | Result |
|---|---|
| M1: break a 76-glyph justified paragraph with a right tab into every line and paint each (wavy coloured underline, leaders) | 159,384 B allocated, 44,192 B peak, **0 B left** |
| M1: `line_box`, `border_bands` | **0 B left** |
| M5 | no cache in F5 (the per-group underline metrics memo of the TS is a local `Vec`) |
| no panic: 400 hostile paragraphs (NaN, ±∞, −0, 1e300 and negative values in advances, metrics, indents, frames, tab stops, line rules, settings; random glyph kinds and flags), each broken until `done` and every line painted with a 200,000-unit work cap | progress holds (≤ n + 2 lines), every placed glyph resolves, the item side data stays parallel, no panic |
| unit tests | line box rules (exact, at least, multiple above/below single, grid, grid 0/NaN), breaking and hanging spaces, character breaking and guaranteed progress, center/right/justify, default and right tab stops, the automatic hyphen, the trailing line break, underline geometry, border widths/bands/`toWhite` (incl. `parseInt('auto', 16)` = 10), borders as rectangles/dashes/waves, glyph runs merged and split, underline + strike + leader items |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`, `min`, `max_n`, `min_n`, `round`, `floor`,
   `ceil`, `abs`, `pow`, `sin`, `%` = `rem`); `parseInt(hex, 16)` through `parse_int_utf16`, `>>` and
   `&` through `to_int32`; `toString(16)` through `to_radix_string`; `String(n)` through
   `to_js_string` (run keys); `x > 0 || x < 0` through `conv::truthy`. Glyph indices `u32`/`usize`
   from array positions. **No arithmetic-semantics change** (`lineIdx` saturates, unreachable within
   the budget).
2. **Regex**: `LETTER`/`MARK` (`/^[\p{L}\p{M}]$/u`, `/^\p{M}$/u`) through
   `uo_text::props::is_single_code_point_in`; none dynamic.
3. **NaN/±∞ and absent keys**: `??` on a model `null` is nullish (`rpr.sz`, `rpr.w`, `b.sz`,
   `b.space`, `consecutiveHyphenLimit`, `hyphenationZone`, `ie.l`); truthiness of numbers through
   `conv::truthy` (`oAsc ||`, `shT ?`, `hrPct ?`, `feShrink`); optional glyph fields are `OptF64`;
   NaN comparisons fall as in JS.
4. **Orders**: placed glyphs in the TS order; the TS `includes`/`indexOf` over `PlacedGlyph` objects
   are index masks and positions.
5. **Recursion**: none.
6. **Module state**: `lineSeq` → `Cx::line_seq`, per call (LA3; the canonical stream relabels it).
7. **Throws** (LQ9): a `placed[…]!`/`gs[…]!`/style/math read that cannot fail in the TS but would
   throw → `Err(Internal)`; `String.fromCodePoint` above U+10FFFF → `Err(Internal)`.
8. **Aliasing / identity (LA3)**: `PlacedGlyph.g` → `GlyphSrc` (`Para(i)` is the object `gs[i]`; the
   automatic hyphen is a new object, never `===` a paragraph glyph, as in
   `placed[…].g !== gs[lastBreak]`); `sh.includes(pg)`, `spaces.includes(pg)` → masks;
   `cur.g.style === g.style` → `StyleIx` equality (**F4 must keep one `styles` entry per TS
   `RunStyle` object, math styles included**); `Glyph.obj` → `RunKey` (a slice resolves to its base
   run's fields); the glyph-run key uses the face's **`key` string** (the TS template), not the
   answer identity: two answers with the same key merge into one run whose face is the first glyph's,
   as in the TS. **No `json_key` site** (`fxKey` is F4's string).
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F5-1 | `LineEnv`/`PaintEnv` traits over what the TS reads through `ctx`/`face.font`; `Cx` implements both; `next_in`/`paint_line_in` are generic | the facets replay the algorithms on the TS answers now (no D1a reader/F4 needed); the arithmetic and order are untouched |
| F5-2 | `line_box` is infallible and returns `(height, baseline)` (F0 had `LResult`) | it cannot fail |
| F5-3 | `hyphenCandidates`/`chooseHyphenation` and the three constants ported here, in `hyphenate.rs` | `next` calls them; F4 keeps `markHyphenation` |
| F5-4 | `shadowExtent` in `lines.rs` (pub) | the empty-line height reads it; F4 uses this one |
| F5-5 | the `paintLine` run key compared component by component with JS-string equality per number (`-0` = `0`, NaN = NaN), the full TS string built when a string component (face key, colour, `fxKey`) contains `\|`; the `paintMath` key always as the full string | identical result without a string per glyph |
| F5-6 | a TS leader name outside `TabLeader` maps to `Dot` (`leader_from_ts`) | paint draws `'.'` for any other name; only a facet's leader name would differ |
| F5-7 | `Object.prototype` names as `w:val` (`BORDER_DASH[val]`, `PATTERNS[val]`, e.g. `constructor`) are not looked up | the TS reads inherited members there (a function as dash pattern); accepted by name |
| F5-8 | shared additions: `RunStyle.rpr: Arc<RPr>`, `PreparedPara.ppr: Arc<PPr>` (para.rs; F4 fills them), `Cx::line_seq` (engine/mod.rs), model.rs re-exports (`JsNumber`, `Settings`, `BorderSpec`, `PBdr`, `RFonts`, `RPr`, `ShadingSpec`, `TextShadow`, `DrawingShape`, `InlineImage`, `Ptab`, `PageBorders`, and F2's `ColorRef`, `FontSlot`, `font_of_slot`, `resolve_color`, `shading_fill`), the `uo-text` dependency, `loader.mjs` `__uoFacetPre`, the `f5` deps in `capture-layout.mjs`, the CI step | the TS fields the port reads; one place naming uo-docx |
| F5-9 | `paintInlineObject`: the control flow is ported (effects margin, EMF/WMF before raster, the coordinates); its four ends are F7b stubs — the `ImageId` of a model drawing (RQ3), `metafile::paint_metafile` with the pass's `FaceLookup`/`ResWriter`, the image item with `drawing::picture_look`, the shape item the floating objects build | the engine holds the image table, `origins` and `FaceLookup` (F3 record, "Not done"); 246 oracle paint calls pending |
| F5-10 | work budget: `next` charges the glyphs it visits + 1; the leader-dot, dash-band and wave loops 1 per iteration | the TS has no limit and hangs on absurd coordinates (LQ14) |
| F5-11 | `Glyph.ptab` is an index into the paragraph's `LPara.runs` (the F0 doc) | F4 fills it so |
| F5-12 | `fitRight` always rounds to the Word unit | the lab options are gone (LQ12) |
| F5-13 | `displayEnd`'s East Asian check is `font_of_slot(…EastAsia…)` non-empty (`!!`); `paintBorder`/`pageBorderItems` colours through F2's `resolve_color`, `paintParaBox` through `shading_fill`; `paint_border_in`/`page_border_items_in` are the bodies after the colour, page-border colours resolved before drawing | F2's functions; resolution has no side effect |
| F5-14 | `o:hr` fill = `sh.style.fill` as its TS string (`DrawingColor::to_ts_string`), else `A0A0A0` | the TS `sh.style.fill ?? 'A0A0A0'` |
| F5-15 | VDMX at `em`: a size outside 0–65535 or not an integer finds no record | the TS `Map.get` misses there |

**Limits** (LQ14): only the work budget (F5-10); every other loop is bounded by the glyph count.

### Not done (handed on)

- F7b: the four ends of `paint_inline_object` (F5-9).
- The `Cx` adapters (`impl LineEnv`/`PaintEnv for Cx`) are covered by P1 from F7a on; the replay env
  answers with the TS values, and the F1/F2 functions they delegate to have their own facets.
- `paintBorder`, `paintParaBox`, `pageBorderItems` have unit tests but no facet yet: their callers
  are the engine (F6, F7a); a facet with them when they call (engine.ts call sites).
- `tests/layout_parity.rs` (F7a's) skips `p3.lines` in its `Ok` path; it should also skip or compare
  `p3.paint` (and F1/F3's facets) when F7a wires the reader.

## Wave 1 — F2 (Word semantics in `uo_docx::word`, LQ3)

Opus, 2026-11-03, hybrid mode, one session (≈ 3 h) in parallel with F1, F3, F5 and the uo-docx
reader frontiers. 0 parity rounds on the Word-semantics code: the first replay of every facet was
equal except 24 cases of the unit differential whose test harness decoded lone surrogates into
U+FFFD (a harness bug, fixed there); the no-panic loops found one real defect (below, F2-7).

### Ported (TS → Rust, TS operation order)

| TS | Rust (`core/crates/uo-docx/src/word/`) |
|---|---|
| `resolve.ts` (414): `mergeRFonts`, `mergeRPr`, `mergeTabs`, `mergePPr`, `mergeBorders`/`mergeMargins`, `mergeTblPr`/`mergeTrPr`/`mergeTcPr`, `StyleResolver` (`chain`, `paraChain`, `paraStyleId`, `paraStyle`, `charStyle`, `tableStyle`), `cascadeRPr` + `TOGGLES`, `COND_ORDER`, `fontOfSlot`, `own`, `highlightHex`, `applyTintShade`, `resolveColor`, `shadingFill`, `resolveDrawingColor` | `resolve.rs` |
| `context.ts` `directRPr` 245 (+ `setFx`, `setOt`, `uSpec`, `colorSpec`, `eq`), `directPPr` 317; `para.ts` `resolvePPr` 682; `paraformat.ts` `inheritedPPr`, `effectivePPr` | `direct.rs` (`direct_rpr`, `direct_rpr_with`, `direct_ppr`, `resolve_ppr` → `ResolvedPPr`, `inherited_ppr`, `effective_ppr`) |
| `numbering.ts` (236): `roman`, `letters`, `cardinalPt`, `formatNumber`, `NumberMarker`, `LevelCount`, `NumberingState` (`clone`, `listKey`, `abstractFor`, `level`, `next`) | `numbering.rs` (+ `TextCap`, `LevelSrc`) |
| `datefmt.ts` (103): `formatWordDate`, `dateFieldValue` | `datefmt.rs` (+ `LocalDateTime`, `time_clip`) |
| `field-formula.ts` (129): `fieldName`, `hasNestedField`, `evaluateFieldFormula`, `substituteNested`, `parseNumber`, `parseArithmetic`; docx-core `table-formula.ts` `formatFormulaResult`, `formatPicture` (+ `roundAway`, `group`, `splitSections`) | `field_formula.rs` |
| `outline.ts` (185): `headingText`, `outlineLevelOf`, `documentOutline`, `listItemValues`, `numberedItems`, `outlineParagraphs` | `outline.rs` (over `DocView`) |
| `numpages.ts` (30) | `numpages.rs` |
| `revisions.ts` (178): `AUTHOR_PALETTE`, colours and bar constants, `authorColor`, `barX`, `RevisionDisplay` (`run`, `directRPr`, `paragraph`, `markGone`, `barFor`, `barColor`), `joinDeleted`'s offset, `revisionRanges` | `revisions.rs` |
| `hyphenate.ts` `documentHyphenationLanguages` 65–80 (K5) | `hyphlangs.rs`: the explicit `lang`-site walk, in each object's key order |
| `refs.ts` `cellAtGrid`, `paragraphAtPath` (`cellBlocks` = `Story::blocks`) | `refs.rs` (model side; the layout queries stay F8's) |
| JS object operations (spread, `Object.assign`, `delete`, key order, `JSON.stringify` equality, `Array.prototype.sort`) and `Number.prototype.toString(radix)` | `obj.rs`, `num.rs` |

uo-docx-layout `src/word.rs` is now the facade: re-exports of `uo_docx::word` (the F0 placeholders
are gone) plus the glue to the layout's types — `TimeZoneRule::offset_min_at`, `Clock::local`/
`local_now` (LQ10), `revision_display(opts.revisions)`, `text_cap(limits)`, `numbering_state(lang,
limits)`. F5 already calls `resolve_color`/`ColorRef`/`shading_fill` through it; F1's `note_mark`
can now take `word::format_number`.

### Parity (TS capture regenerated in the same run, LQ21)

**Unit differential** (`uo-docx/js/capture-word.mjs` → `uo-docx/tests/parity/word.{capture,inputs}`,
replayed by `uo-docx/tests/word_unit.rs`): seeded generated inputs through the frozen functions —
random OOXML `rPr`/`pPr`/`tblPr`/`trPr`/`tcPr` (data-dependent child orders, explicit `w:val="0"`,
bad numbers, theme colours, w14 effects, `rPrChange`/`pPrChange`), random `styles.xml` (integer-like
ids, `basedOn` cycles, linked styles, table styles with `tblStylePr` incl. `__proto__`), `numbering.xml`
(numStyleLink, overrides, restarts, `isLgl`, `w:null` texts, every number format) and themes, read on
both sides by the D1a readers (R1, R2); edit scripts over the editing props (R4a's
`runPropsFromRPr`/`paraPropsFromPPr`); strings and numbers for dates, pictures, formulas and
formats. Compared: canonical digests (A6) of every result **and of its key-order facet** (K9/LQ20),
the JSON of strings. The facet `in` (digest of the inputs as read) tells a reader difference from a
Word-semantics one: 0 reader differences. Regenerated after the uo-docx D1a fixes I1 (`TblBorders`
diagonals) and I3 (`canon.mjs` own-key rules): byte-identical.

| Run | Cases | Facets equal | Mismatches | Accepted by name |
|---|---|---|---|---|
| committed (seed 7) | 5,127 | 23,098 | 0 | 23 TS throws (F2-5) |
| local (seed 11, `--count 4000 --fmt-count 400`) | 24,967 | 222,757 | 0 | 35 TS throws (F2-5) |

The local run covered 1,772 non-null numbering markers, 1,948 distinct resolved paragraph styles,
660 distinct table styles, 10,334 distinct merge results.

**Engine facets** (`tools/lib/facets-f2.mjs`, hooked by `facets.mjs`; dumps `<key>.f2.tsv`; replayed by
`tests/f2_parity.rs`): `p3.datefmt` (every `dateFieldValue`), `p3.formula` (every
`evaluateFieldFormula`, nested calls included, with the transcript of its nested-field callback),
`p3.numbering` (every `NumberingState.next` of the engine's states, with the `clone()` events of
`pure()`/`saveFlow`, replayed over the formatting R2 reads from the docx), `p3.style` (the first
`paraStyle`/`charStyle`/`tableStyle` query per id), `p3.hyphlangs` (once per document).

| Corpus | Documents | Facets equal | Calls compared |
|---|---|---|---|
| committed fixture `tests/captures/f2` (4 documents of `tools/gen-f2-docs.mjs`: lists, styles, DATE/TIME/CREATEDATE, formulas) | 4 | 16 | 33 DATE/TIME, 33 formulas, 125 markers, 22 style queries |
| Word oracle, all documents (installed fonts) | 2,742 | 10,968 | 1,167 markers (17,125 clone events), 6,479 style queries |

The oracle and the F0 generator have **no** DATE/TIME or formula field, hence `gen-f2-docs.mjs`.
`p3.hyphlangs` replays since R5 landed `read_document`: `f2_parity` checks it over the real reader on the four fixture documents, and uo-docx's `parity_full` (facet `hyphlangs`, `js/capture-full.mjs`: `documentHyphenationLanguages` with and without `force` over the 2,753 corpus documents and 771 synthetic packages) is equal on every one (R5 record). Finding: the walk visited a text box's `paras` and `blocks` views both, 2^depth visits for nested text boxes (the TS does too): **fixed by a single visit, same output (F2-21)**.

**São Paulo facet** (LQ10; `capture-word.mjs --tz America/Sao_Paulo` → `tests/captures/f2/
datefmt-America-Sao-Paulo.*`, replayed by `f2_parity.rs::sao_paulo_dates` through `Clock::local` with
a `TimeZoneRule::Table` built from the zone's transitions the capture recorded, 1914–2040): 1,485
cases (17 instants incl. the 2018 and 2019 switches and the 1985 DST era, 3 languages, 17 pictures
and 16 instructions), **1,485 equal**; the rule's offset equals V8's at all 1,386 valid instants.

### Memory and robustness

| Check | Result |
|---|---|
| M1 (`uo-docx/tests/memory_word.rs`): merges, cascade, colours, style resolver (+ `clear`), direct formatting, `resolve_ppr`/`inherited_ppr`/`effective_ppr`, numbering (states and clones), every number format, dates, formulas (incl. a depth-limit error), pictures, revisions, outline functions, the hyphenation walk — 32 operations | 118,761 B allocated in total, largest peak 36,772 B, **0 B left** each |
| no panic (`uo-docx/tests/nopanic_word.rs`): 20,000 random field codes (braces, backslashes, quotes, lone surrogates, nested PAGE/NUMPAGES with hostile answers); formulas of 10⁵ nested parentheses, 10⁶ minus signs, 1,000 nested fields, 2·10⁵-digit pictures | depth → `FormulaError::Depth`; 200-field guard → none; pictures cut at the cap |
| no panic: 20,000 random date pictures/instructions at invalid and extreme instants; every number format × 14 hostile counters (±1e300, NaN, ±∞, 2⁵³+1, fractions) | texts within their caps |
| no panic: hostile numbering (level texts of up to 5,400 placeholders, starts 1e15/NaN, numStyleLink cycles) and 300-style `basedOn` cycles; 3,000 random merges/cascades/direct formatting of hostile OOXML; V8 sort with NaN comparators (0–200 elements) | markers within the cap (bullets keep their written text), chains ≤ 32, permutations kept |
| fmt, clippy `--no-deps -D warnings` (`-p uo-docx -p uo-docx-layout`, all targets, native and wasm32), `float-casts.sh`, `grep-bans.sh` | clean for the F2 files (other frontiers' files in progress muted with `-A clippy::collapsible_if/collapsible_match` to reach the test targets) |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`/`min`/`max_n`/`min_n`/`round`/`floor`/`rem`/
   `pow`/`sign`/`abs`/`trunc`), `parseInt` → `parse_int`/`parse_int_utf16` (over UTF-16 slices: hex
   pairs of colours), `parseFloat` → `parse_float`, `String(n)` → `to_js_string`, `toFixed` →
   `to_fixed`, `toString(16)` → `js_to_radix` (F2-8), `String.fromCharCode` → `to_uint16`. Model
   numbers are `JsNumber`: `null` as a number is 0 in arithmetic, nullish for `??`, `!== undefined`,
   never `=== 0`; every `!==` between optional numbers is the JS one (`ne_num`). **No
   arithmetic-semantics change**: the TS order of operations everywhere (`applyTintShade`'s HSL,
   `roundAway`, the formula parser).
2. **Regex** (hand matchers, none dynamic): `/%([1-9])/g` (`fill`, `refs`), `/%[1-9]/` search,
   `/\\@\s*(?:"([^"]*)"|(\S+))/`, the same with `#` (`find_switch_picture`: the quoted alternative
   needs its closing quote, else `\S+` from the quote), `/\\\*\s*(\w+)/g` (`format_switches`),
   `/(^|\s)(\S)/g` (`Caps`, one unit), `/\{([^{}]*)\}/` (`inner_braces`),
   `/^\s*=([^\\]*)((?:\\[\s\S]*)?)$/`, the sticky tokenizer `/\s*(\d[\d.]*(?:,\d+)?|[-+*/()])/y`,
   `parseNumber`'s `/^([-+]?)(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d+))?$/` (greedy groups are exact: a
   shorter choice always leaves a `.`), `/,(\d+)$/`, `/^[0-9a-fA-F]{6}$/`, `/^pct\d+$/`,
   `/^heading ([1-9])$/i` (ASCII: no letter of "heading" has a non-ASCII `/i` fold), `/^'(.*)'$/` (no
   line terminators), `/[0#x]\.[0#x]/`, `/\B(?=(\d{3})+(?!\d))/g` (`group`: a digit run whose length
   is a multiple of 3, at a non-boundary), `/^[bcr](\d+)$/`, the soft hyphen and U+FFFC, `[\t\n\r\v\f]`, `\s+`.
   JS `\s` = uo-text's white space over units.
3. **NaN/±∞ and `undefined` keys**: counters and starts carry NaN (a NaN level writes its own slot,
   as `c[NaN]` on the array); a `null` start override is stored as 0 (its only use is `+ 1`);
   explicit `undefined` keys are produced where the TS writes them (`{ ...lvl.pPr, tabs: undefined }`,
   `asciiTheme: undefined`…, `base.b = p.bold` with the prop absent) and copied by every spread
   (`set_undefined`, `JsAssign`).
4. **Orders**: every merge keeps the TS key insertion order (`obj::spread`/`assign`/`set`/`delete`:
   base keys first, new keys appended, re-assignments in place, `delete` then set appends); style and
   numbering records through `JsRecord` (integer-like keys first, own keys only); counters, seen
   `w:num`s and deferred overrides in `IndexMap`/`IndexSet` (TS `Map`/`Set`); `mergeTabs` through an
   `IndexMap` keyed by SameValueZero position and V8's sort (F2-7); the hyphenation walk in each
   object's key order (F2-16). Validated by the order facet of the unit differential.
5. **Recursion**: `parseArithmetic` bounded by `max_parse_depth` (K11c, F2-6);
   `evaluateFieldFormula` ↔ `substituteNested` recurses one level (the inner code has no braces);
   `abstractFor` ≤ 4 (TS guard); `chain` ≤ 32 (TS guard); the outline/hyphenation walks recurse per
   table nesting (bounded by the model's 64, uo-docx F17); `roman`/`letters`/`chicago` loops bounded
   by `TextCap`.
6. **Module state**: the `CTX` `WeakMap<DocFormatting, DocContext>` of `paraformat.ts` → a resolver
   passed by the caller (K3, LQ18: per call); no global.
7. **try/catch**: `parseArithmetic`'s `SyntaxError` → `Ok(None)`; its rethrown `RangeError` → the
   depth limit; `formatNumber`'s and `toFixed`'s throws → F2-5.
8. **Aliasing / identity (LA3)**: `NumberMarker.level` (the `NumLevel` object) → `LevelSrc` locator +
   `level_in(fmt)`; `charStyle` of a linked paragraph style shares the paragraph style's `Arc<RPr>` (the
   TS returns the same object); `listItemValues`' `Map<Paragraph>` → `IndexMap<ParaId>`;
   `outlineParagraphs`' paragraph objects → references; `REV_JOINS` (symbol key) → the offsets the
   layout's `LPara.rev_joins` keeps (`join_offset`). `json_key`/`json_eq` sites: `eq` in `directRPr`
   (`textFx`, `openType`, `underlineTheme`, `colorTheme`) and `directPPr` (`alignment`, `numbering`,
   `borders`, `tabStops`), the old-formatting comparisons of `RevisionDisplay.directRPr`/`paragraph`
   — all through `obj::json_eq` (uo-docx `JsonWriter`, insertion order).
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F2-1 | **Shared D1a infrastructure, additive**: `js_object!` now also implements `JsAssign` (`assign_slot`, `remove_slot`: typed per-key copy/removal) for every model type; `word::obj` builds the JS spread/assign/delete with key order on it | no per-type hand-written spreads of 43-field objects; K9/LQ20 exact |
| F2-2 | `StyleResolver` and `NumberingState` are plain data (caches/counters only); their methods take the `DocFormatting` (the TS objects kept references) | K18: the `Engine` holds them and clones them in `pure()` |
| F2-3 | Counters: `[Option<LevelCount>; 9]` plus the NaN slot; fractional levels (only reachable by API misuse: `parseInt` gives integers) are not stored | the TS indexes an array by number |
| F2-4 | `TextCap` (= `max_field_text`, K11b, LQ14): roman/alphabetic/chicago texts, a marker's filled text and its placeholder count (a `none` format would never stop it), DATE/TIME results and formula results stop at the cap and set a flag (`take_truncated`) for the caller's warning; the per-placeholder level formats are looked up once per call | the TS builds them until memory runs out or `repeat` throws; results differ only past 65,536 units |
| F2-5 | **TS throws accepted by name** (extends K11/LQ14): `chicago` of a fraction or NaN (`TypeError`) → `""`; `decimalEnclosedCircle` of a fraction (`RangeError`) → `String(n)`; `repeat(Infinity)` / past the string limit → the capped text; `toFixed` past 100 digits in `formatPicture`/`formatFormulaResult` → `None` (the cached result stays); a solid w14 fill without a colour in `setFx` (`TypeError`) → a colour without fields. Captured: 23 (committed) and 35 (local) throws, all of these kinds (`Replay::accept`) | the layout fails in the TS; Rust continues |
| F2-6 | `parseArithmetic` depth past `max_parse_depth` (256) → `FormulaError::Depth`; the TS succeeds up to V8's stack (≈ 10⁴ frames) and then throws | K11c/LQ13: a stack overflow would abort the wasm instance |
| F2-7 | `js_sort` (`mergeTabs`): V8's run detection and binary insertion below 64 elements, exact even for NaN comparators; from 64 on a stable merge sort (equal to V8 for consistent comparators). Found by the no-panic loop: `slice::sort_by` **panics** on an inconsistent order (NaN tab positions), so it is not used | no panic on hostile documents |
| F2-8 | `js_to_radix`: V8's `DoubleToRadixCString` (fractions, ≥ 2⁵³, NaN/∞) ported here; uo-jsnum's `to_radix_string` covers safe integers only | `formatNumber('hex')`, shading mixes |
| F2-9 | Dates: the local getters become `LocalDateTime` parts from the time value and the offset at it (LQ10; ECMA-262 date algorithms); the glue `Clock::local`/`TimeZoneRule::offset_min_at` lives in the layout facade (inherent impls in `word.rs`); a table rule before its first transition uses the first offset. `TimeZoneRule` has minute resolution (F0), so local mean time (sub-minute offsets, São Paulo before 1914) is not representable: the facet starts in 1914 | F0 types |
| F2-10 | `resolveDrawingColor` = uo-drawingml's `resolve_word_spec` over the structured spec (its named difference: modifier values holding `\|`, `,`, `=`); `THEME_ALIAS` = `word_theme_slot`; `toHex` = `to_hex_upper` | reuse (uo-drawingml G12) |
| F2-11 | A `tblStylePr` type `__proto__`: the TS assignment sets `cond`'s prototype; Rust keeps an ordinary key. Consumers read `COND_ORDER` only (equal in the differential, which generates it) | unobservable |
| F2-12 | `RevisionDisplay.color(author)` for an author named like an `Object.prototype` key (`toString`…): the TS returns that function from the `colors` record (and `run()` then loses its `color` key in JSON); Rust returns the palette colour. Not generated; named | prototype lookup |
| F2-13 | `RevisionDisplay::new(mode, insDel, formatting, authors, colors, order, docOrder)` (uo-docx has no options type; the facade converts the layout's `RevisionView`); `paragraph()` returns the `(ppr, props)` override of the copy (LA2); `revision_ranges` takes the runs as (props, UTF-16 length) and the `REV_JOINS` offsets | the layout's derived paragraphs (LA2) |
| F2-14 | `RevisionDisplay.directRPr` uses `direct_rpr_with(run, props)` instead of cloning the run (`{ ...run, props }`): `editBaseProps` reads the same link/note/symbol | no text copy |
| F2-15 | The outline functions take a `DocView` (`full.formatting ?? emptyFormatting()` → the document's formatting, always present) | A4 |
| F2-16 | `documentHyphenationLanguages`: the explicit site list (K5) in each object's key order; a section visits its parts' flat paragraph views (`headers`/`footers` come before `headerBlocks` in the TS object), a cell its direct paragraphs then its tables, a textbox its `paras` view then its blocks (the textbox's second visit is gone since F2-21), custom inline data as any JSON | the output order of the TS `Set` |
| F2-17 | docx-core `table-formula.ts` `formatFormulaResult`/`formatPicture` and docx-layout `refs.ts` `cellAtGrid`/`paragraphAtPath` are ported here | the layout and the editor need them; one implementation |
| F2-18 | New dependency uo-docx → uo-text (UTF-16 case mapping with SpecialCasing, JS white space over units); uo-docx-layout dev-dependencies: uo-docx with `canon`, uo-opc (the F2 replay) | LQ11/K10 |
| F2-19 | Harness: `uo-docx/js/capture-word.mjs` (unit differential, São Paulo facet with `--tz`), `tools/lib/facets-f2.mjs`, `tools/gen-f2-docs.mjs`; shared edits additive: `facets.mjs` imports and chains the F2 recorder after the F1/F3/F5 hooks (the pre-hook carries the formula callback rewiring), `capture-layout.mjs` passes the three TS classes it patches | the corpus has no DATE/formula field |
| F2-20 | **The TS checkout of the captures**: `.claude/worktrees/clickcompliance-docx` at `d0f45bc5` (= this branch's frozen `write/`; its dist built after `field-formula.ts`). The main checkout (`b92b60fa`) has a **stale dist** (built 2026-10-23, before `field-formula.ts`/`numpages.ts` were added on 10-30): no `dist/field-formula.js`, `numpages.js`, `run-boxes.js`, `pdf-navigation.js`, `pdf-word-spaces.js`, `inline-renderer.js`, `docx-core/dist/inspect.js`, `style-names-es.js`, and different `engine.js`, `para.js`, `pdf.js`, `document.js`, `fulldoc.js`, `xml.js`. The captures naming `@ts-commit b92b60fa` (F0, F1, F5 `layout-ci`, F1 `resolver`) come from that older engine | **Done (2026-11-10, gate fixes).** The four were recaptured with the frozen `d0f45bc5` (`capture-layout.mjs --mode ci --count 7 --seed 1 --accept-stale typography/font/sfnt.ts`; `capture-f1-resolver.mjs --count 40 --seed 1`). The digests of every record differed from the old ones (content, not only the header: p1, p2, p2first and the p3 facets), so the record lines (same facets per key), the dumps (`p1.bin`, `f1.tsv`, `f5.bin`, `transcript.tsv`) and the headers were replaced; the f1 resolver capture was replaced whole. Replays: `adapter` 2 equal, `f1_parity` 10 + 80 equal (40 resolver documents), `f5_parity` 2 equal, `layout_parity` 5 tests, 0 mismatches; `grep -rl b92b60fa core/crates/*/tests` is empty |
| F2-21 | **Deliberate divergence from the TS (security; same output)**: `document_hyphenation_languages` visits each paragraph once. The TS `visit` (`hyphenate.ts:65`, faithfully ported in F2-16) walks a text box's `paras` view and then its `blocks` view, and a cell's `paragraphs`/`tables` and then its `blocks`: the same subtree twice per level, **2^depth visits** for nested text boxes or tables (the TS has the same bug; not edited here). A repeated visit adds nothing to the `Set` (the walk is pure and the subtree's languages are already in the set after its first visit), so the single visit gives the same list in the same order. The F2 port already visited a cell once; the text box's `blocks` view is now skipped too (`paras` holds every paragraph of the blocks, cells included). Proof: `parity_full` (facet `hyphlangs`, 2,753 corpus documents and 771 synthetic packages: 16,518 + 4,506 facets equal, 0 mismatches) and `f2_parity` (`p3.hyphlangs`) unchanged; `tests/r5_followups.rs` compares the single visit with the TS double visit (`hyphlangs::document_hyphenation_languages_ts_walk`, feature `canon`, test-only) on 7 nesting patterns (VML, DrawingML, `wpg` group, `mc:AlternateContent`, tables, the five in turn, text box/table alternating) × depth 0–14 × sources off/on × `force` (420 equal lists, every language of every level in first-appearance order), and runs 30, 64, 65 and 80 levels of each pattern in under 2 s (measured, release, nested `wps` boxes: the single visit 26–38 µs at depths 10–24; the TS double visit takes 2.2 ms at depth 10, 1.43 s at 20, 23.9 s at 24, ×2 per level: ≈ 25 min at 30) | a document with 30 nested text boxes stalls any open with automatic hyphenation (and Manual Hyphenation's `force`); listed for the security review below |

**Limits** (LQ14): `max_parse_depth` (formula nesting, F2-6) and `max_field_text` (F2-4); every
other loop is bounded by its input or by the TS guards (`chain` 32, `abstractFor` 4,
`substituteNested` 200).

### Not done (handed on)

- The outline functions over real documents (the `p3.hyphlangs` replay is done, see above; the walk
  is linear since F2-21; the TS stays exponential in nested text boxes).
- `resolvePPr` and `cascadeRPr` are compared on generated inputs (unit differential) only; their
  engine call sites (F4 `prepareParagraph`, F7) are covered by P1 once F7a runs.
- F1's `DocContext::note_mark` still takes `formatNumber` as a parameter (pass
  `word::format_number` with the call's `text_cap`); F7c calls `date_field_value` with
  `Clock::local(…)` and the same cap and warns on `take_truncated()`.
- The stale-dist recapture of F2-20 (other frontiers' fixtures).

## Stop point (2026-11-03, maintainer asked to stop after this round)

Done: F0 and wave 1 — F1 (0 parity rounds), F2 (0), F3 (1: an empty POLYBEZIER throws in the TS),
F5 (0). Next: wave 2 (F4 paragraph preparation, hyphenation, math; then F6 tables), then wave 3
(F7a engine core, F7b/F7c, F8).

Open:
- **Stale TS build in some committed captures (F2-20)**: the main checkout `D:/Projects/OfficeNext`
  (b92b60fa) has a `dist` built 2026-10-23, older than its sources (missing `field-formula.js`,
  `numpages.js`, `run-boxes.js`; `engine.js`, `para.js`, `pdf.js`, docx-core `document.js`,
  `fulldoc.js`, `xml.js` differ). Captures stamped `@ts-commit b92b60fa` came from it: the F0, F1 and
  F5 `layout-ci` fixtures and F1's `resolver`. Recapture them from the frozen build
  (`.claude/worktrees/clickcompliance-docx`, d0f45bc5) before the gate, and make the capture tools
  refuse a `dist` older than its sources. The oracle runs that used d0f45bc5 are unaffected.
- F1's `note_mark` should take `word::format_number`; F7c calls `date_field_value` with
  `Clock::local` and the text cap; F7b owns the four `paint_inline_object` ends (246 pending oracle
  paint calls); the "font budget" warning needs a registry signal.

## Stage 3 — F4 (paragraph preparation, hyphenation marks, equation layout)

Opus, 2026-11-05, hybrid mode, one session (≈ 1 h 20 min), in parallel with uo-docx D2a (session),
the `.doc` frontier and the docs records in the same tree. **1 parity round** (below); every other
call was equal at its first replay. The F0 types were changed where the TS needed it (F4-3 … F4-5).

### Brief (written from the frozen TS before porting)

**F4 — paragraph preparation** (≈ 1,650 lines). TS (d0f45bc5): `para.ts` minus F1's width model —
`resolveRunStyle` 206 (colour cascade `w14:textFill` → `w:color` → `env.defaultColor` → black/white,
underline colour, shading, `w14` solid fill and `resolveTextFx` + `fxKey = JSON.stringify(fx)`,
superscript/subscript raise by the run's own face), `faceFor` 266 (slot by Unicode range,
`w:sym`, the `hint="eastAsia"` quotes, the F0xx `.notdef`, `fallbackOrder`/`classFallback` by
PANOSE), `vMetrics` 301, `isHyperlinkStyle` 329, `otFeatures`/`ligFeatures`/`applyLigatures`
440–538, `fieldValueRun`/`fieldRuns`/`displayRuns` 548–605 (the `env.field` callback, formulas
with nested fields, field results, drop-downs; `GLUED_FIELD`), `isDark` 608, kinsoku
`NO_LINE_END`, `isCjkFont`/`docEastAsiaCjk`/`eastAsianBreakRules`/`hangPunct` 616–669,
`paraVisibility` 716, `prepareParagraph` 729–1175 (the run loop with objects, check boxes, custom
inlines, `w:ptab`, note references, equations, `pushChars` with caps/small caps/kerning/spacing;
the post passes: break before inline objects, dashes across runs, ligatures, run-boundary kerning,
NBSP glue, kinsoku, the East Asian layout of modes ≤ 14; `markHyphenation`; the marker with picture
bullets and the editor's synthetic lists 1001/1002; indents, `jc`, the equation-only `jc`, revision
bars, `empty`); `hyphenate.ts` `hyphenMins`, `langOf`, `langAllowed`, `markHyphenation` 86 (NFC
clusters, `doNotHyphenateCaps`, manual-hyphenation skips) and `hyphenatorFor` (typography/hyphen);
`math.ts` layout (`mathTextMetrics` 115, `clsOf`, `mathAlpha` 124, `mathAtoms` 202, `rowBox` 258,
`fracBox` 277, `kEm`/`q600`/`CM`). Rust: `para.rs`, `hyphenate.rs` (`mark_hyphenation`,
`HyphenSessions`), `math.rs` (layout half). Inputs: F1 (resolver `Cx::face`, widths, kerning,
`FaceTable::math_info`), F2 (`resolve_ppr`, `cascade_rpr`, `merge_rpr`, `char_style`,
`RevisionDisplay`, `NumberingState`, `evaluate_field_formula`, `format_number`, colours), F3
(`fx_color`, `resolve_text_fx`), F5 (`shadow_extent`, `leader_from_ts`), uo-text (case mapping with
SpecialCasing, NFC, `\p{L}`/`\p{M}`, JS `\s`), uo-fonts (`ligate`, `kern_in_latin`, hyphenation
sessions). Pick-ups from earlier frontiers: F1-3 (the `i32` note ids of `para.rs`), F1-4/F2 (the
note mark formatted with `word::format_number`), F5-8/F5-11 (`RunStyle.rpr`, `PreparedPara.ppr`,
`Glyph.ptab`), F5 "one `styles` entry per TS `RunStyle` object, math styles included", F3-10
(`fxKey` is F4's), F2 "`resolvePPr`/`cascadeRPr` at their engine call sites". Facet: `p3.prepare`
(the whole `PreparedPara` per call) replayed over the uo-docx reader, the table-style env rebuilt
from (style, conditions), the field/numbering/note-mark answers from the TS transcript, the faces
from the P2 transcript. Pitfalls: `RunStyle` identity per object; field-value copies are not model
runs; `-1` offsets of marker glyphs; the `{}` literal and the per-cell merges of table envs;
`fxKey`/`JSON.stringify(r.ot)` key order; a `.notdef` symbol measured by the run's font.

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| `para.ts` `resolveRunStyle` 206, `faceFor` 266, `notdefPua` 263, `vMetrics` 301, `classFallback`/`fallbackOrder` 190–204 | `para::Prep::{resolve_run_style, face_for, notdef_pua, v_metrics}`, `class_fallback`, `fallback_order` |
| `isHyperlinkStyle` 329, `otFeatures` 440, `ligFeatures` 452, `applyLigatures` 471 (`keyOf`) | `is_hyperlink_style`, `ot_features`, `lig_features`, `apply_ligatures` (`lig_key`), `fx_json` (`fxKey`) |
| `fieldValueRun` 548, `fieldRuns` 557, `isMarkRun` 583, `displayRuns` 586 | `Prep::{field_value_run, field_runs, display_runs}` (`DRun`: the run object the TS pushes), `is_mark_run` |
| `isDark` 608, `NO_LINE_END`, `CJK_FONT_NAME`/`isCjkFont` 620, `docEastAsiaCjk` 634, `isCjkLang`, `eastAsianBreakRules` 651, `HANG_*`/`hangPunct` 661, `SPACE_LIKE` | `is_dark`, `is_cjk_font` (+ `Prep::is_cjk_font`), `Prep::{doc_east_asia_cjk, east_asian_break_rules}`, `cjk_font_name`, `known_proportional_ea`, `hang_punct`, `space_like` |
| `paraVisibility` 716, `prepareParagraph` 729 (`pushChars` 747, `pushMath` 851, the run loop, the post passes, the marker, the result) | `para_visibility`, `prepare_paragraph` (`Prep::push_chars`, `Prep::push_math` + `PrepMath`) |
| `hyphenate.ts` `hyphenMins` 44, `langOf`, `langAllowed`, `markHyphenation` 86; typography/hyphen `hyphenatorFor` | `hyphenate::{hyphen_mins, mark_hyphenation}`, `HyphenSessions::{new, session, clear}` over `uo_fonts::hyphen` |
| `math.ts` `K`, `CM`, `kEm`, `q600`, `mathTextMetrics` 115, `REL`/`BIN`/`OPEN`/`CLOSE`/`clsOf`, `mathAlpha` 124, `mathAtoms` 202, `rowBox` 258, `fracBox` 277, `MathEnv` | `math::{math_text_metrics, math_atoms, MathEnv, Lvl}` (+ private `k`, `cm`, `k_em`, `q600`, `cls_of`, `math_alpha`, `math_atoms_raw`, `row_box`, `frac_box`) |

`Unported(F4, …)` is gone from the crate.

### Parity (P3 facet; TS capture regenerated in the same run, LQ21)

`tools/lib/facets-f4.mjs` (hooked by `facets.mjs`; **`p3.prepare` replaces the F0 seed**): every
`prepareParagraph` call (all passes, measuring calls in `pure()` included) is one `bytes` item
holding the raw encoding of the whole `PreparedPara` — styles (every field, `fxKey`, the uo-docx
canonical digest of `rpr`) and faces labelled per call by first use, the `ppr` digest, indents,
tabs, spacing, flags, revision bars, the marker (text, level locator, suffix, `jc`, chapter,
glyphs) and every glyph field (`obj` as the run's model position, `ptab`, check box, the
equation box with its items and rules). The dump `<key>.f4.bin` holds per call: the paragraph's
model position (`modelRefs`, the walk of canon.mjs at paragraph level), the env, and the
transcript of what the call asked outside the model — `env.field` answers, `ctx.numbering.next`
markers (level as `nums[k].overrides[i].lvl` / `abstractNums[k].levels[i]`), `ctx.noteMark`
marks. The **table-style env** is recorded as (style key, conditions): `mergePPr`/`mergeRPr`
(resolve.js) are wrapped to keep each merge's recipe and every resolver's `tableStyle` answer
registers its parts (a table can be laid out by another pass context than the paragraph's), so
the env `merge(…merge(style.pPr, cond₁.pPr)…)` (table.ts:741–757) is replayed by F2's
`table_style` + `merge_ppr`/`merge_rpr`; the literal `{ pPr: {}, rPr: {} }` of the nested-table
measure (table.ts:537) is its own record. `tests/f4_parity.rs` reads the docx with
`uo_docx::read::read_document`, builds an `LDoc` of the model paragraphs (runs and field results
keyed), answers the faces from the P2 transcript over the real font files (`TranscriptProvider`),
runs `prepare_paragraph` with a transcript `PrepFlow`, encodes the result as the TS and compares
byte for byte, then rebuilds the facet digest.

| Corpus | Documents | Calls | Equal | Pending | Mismatches |
|---|---|---|---|---|---|
| committed fixture `tests/captures/f4` (CI `gen/1/4`, open fonts; a table) | 1 | 32 | 32 | 0 | 0 |
| CI mode, seed 1 | 5 | 385 | 385 | 0 | 0 |
| CI mode, seed 7, 200 documents | 200 | 15,108 | 15,108 | 0 | 0 |
| .NET synthetic inputs (pictures, page fills ×4, synthetic, textboxes, text effects) | 8 | 153 | 149 | 4 | 0 |
| Word oracle, all documents, installed fonts (`emwidth/ew-emb` skipped: embedded bytes, as F0/F1) | 2,741 | 85,452 | 84,426 | 1,026 | 0 |

On the oracle 2,570 documents are equal on the facet digest; the 171 others hold a pending call and
are compared call by call. **Pending = the paragraph objects the engine derives (LA2, F7a)**, never
counted as equal: 199 synthetic `{ runs: [], props, ppr }` (the note separator's paragraph,
engine.ts:2311), 159 copies whose runs are model runs (frame inner copies, revision views,
`joinDeleted`), 668 whose runs are not all model runs (frame anchors with the frame run,
continuation slices). They are replayed by P1 once F7a's `derive` builds them. Coverage on the
oracle includes the `omath`, `hyph*`, `kinsoku`/`fe*`, `ligwidth`/`kern*`, `symnotdef`, `glyphfb`,
`supraise`, `revisions-render`, `fields`, `numrpr`/`picbullet` groups. Timing: TS oracle capture
with dumps 70 s – 3 min (the merge wraps; 1.4 GB of dumps, kept under `target/`, not committed);
Rust replay 87 s (release).

**Round 1** (the only one): `symnotdef/sn-arial`, 4 calls — the metrics of a private-use
character drawn as Calibri's `.notdef` come from `faceFor(ctx, rpr, 0x41, style)` **without** the
run's `w:sym` font; the port passed `symFont` to that second call. Found by the oracle replay,
fixed in `push_chars`.

### Memory and robustness (`tests/f4.rs`)

| Check | Result |
|---|---|
| M1: every body paragraph of the fixture document (14) prepared with a fresh face table, resources and hyphenation sessions, plus `para_visibility` | 2,598,539 B allocated, 119,748 B peak, **0 B left** |
| M1: a document of every path (caps, small caps, kerning, ligatures and stylistic sets, superscript, `w14` fill, PAGE/formula/TOC-result fields, drop-down, check box, custom inline, footnote reference, `w:ptab`, `w:sym`, an equation with a fraction and `m:br`, a synthetic list, Portuguese hyphenation, CJK kinsoku) — body env and a header-like env | 406,687 B allocated, 40,453 B peak, **0 B left** (each) |
| M5 | no new cache: the hyphenation word cache is the uo-fonts `HyphenSession`'s (cleared past 50,000 words, the TS rule, tested in uo-fonts); one session per pattern set used (asserted) |
| no panic: 400 hostile documents × 6 paragraphs (NaN/±∞/−0/1e300/`null` sizes, kerning, scales, spacings, positions, indents, tab positions, levels; lone surrogates, PUA symbols, form feeds, U+2028, soft hyphens, combining marks; 40-deep field-result chains, formulas with nested fields, unknown note kinds, fractions nested up to 300, `m:r` without text, `m:f` without parts, drop-downs with NaN selections, synthetic lists with NaN/negative levels, unknown `lineRule`/`jc`/leaders) | 1,950 prepared, 450 errors where the TS throws or a limit stops it (258 `Limit(Depth)`, 112 `Internal` noteRef kind, 80 `Internal` `m:r` without text); every glyph's style, face and equation index valid |
| work budget: one 160,000-character paragraph | prepared, the budget charged per code point |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`round`, `max`, `min`, `floor`, `ceil` in
   `q600`, `rem` for `ilvl % 3`); `parseInt(hex, 16)` through `parse_int` (`isDark`); `String(n)`
   through `to_js_string` (`ss${String(n).padStart(2, '0')}`, the ligature key, the picture-bullet
   key, `null` → `"null"`); `offset0 + Math.min(a.offset, last)` through `to_uint32` of the f64;
   model numbers are `JsNumber` (`??` treats `null` as nullish, `ToNumber(null)` = 0, so
   `-ind.hanging` of `null` is −0 as in JS). Offsets and run indices are `u32` UTF-16 positions,
   `MARKER_IX` = the TS `-1` of the marker's glyphs. **No arithmetic-semantics change**.
2. **Regex** (hand matchers, none dynamic): `NO_BREAK_AFTER_DASH` (`/^[\s…]$/u`: exactly one code
   point, JS `\s` = `uo_text::space::is_space`), `/standard/i` etc. (ASCII lowercase `contains`: no
   non-ASCII character folds to these letters without `u`), `CJK_FONT_NAME` (prefix alternatives
   ASCII-insensitive, `hg.*` stopping at the line terminators, the CJK class over UTF-16 units as
   the TS without `u`), `/^(ms (p|ui )(gothic|mincho)|meiryo|malgun gothic)\b/i`,
   `/^(ja|zh|ko)\b/i`, `/^ja/i`, `/^ko/i`, `/^zh-(tw|hk|mo)/i`, `/^[0-9A-Fa-f]{6}/`,
   `/^\s*FORMTEXT\b/i`, `/_MARK\s*$/`, `split(/\s+/)[0]` of a trimmed instruction, `LETTER`/`MARK`
   through `uo_text::props`.
3. **NaN/±∞ and absent keys**: optional glyph numbers are `OptF64`; truthiness of model numbers
   through `JsNumber::truthy`; `rpr.w && rpr.w !== 100` (NaN falsy); NaN size comparisons fall as
   in JS (`#[expect(float_cmp)]` where the TS compares exactly). `tf.alpha === null` (only an edit
   could make one; the reader never does) gives an absent alpha (F4-7).
4. **Orders**: glyphs, display runs, styles and equation boxes in TS creation order;
   `mathRuns` (`Set<number>`, only `has`) is a `Vec`; ligature groups in `ligate` order; the
   numbering and note marks in call order (the flow's).
5. **Recursion**: `fieldRuns` over nested field results and `fracBox` ↔ `mathAtoms` are bounded by
   `max_depth` → `Err(Limit(Depth))` (the TS recurses until V8's stack throws `RangeError`, which
   fails the layout: same outcome class, F4-9); a copy chain of derived paragraphs and of slices
   ≤ 64 (as F0's `model_run`).
6. **Module state**: `GLUED_FIELD` (`WeakSet<Run>` of field-value copies, never read elsewhere) →
   a flag on the display entry; `infoCache` → `FaceTable::math_info` (F1); the hyphenation
   module's `loaded` map → `HyphenSessions` per engine, filtered by `hyphenation_loaded` (K5); the
   `Hyphenator` word cache → the uo-fonts `HyphenSession` per pattern set.
7. **Throws** (no `try` in the F4 TS) → `Err(Internal)`: `fxColor(undefined)` of a solid `w14`
   fill without colour (`c.mods` of `undefined`), `Array.from(nd.text)` of a run node without
   text, `for … of f.num`/`f.den` of a fraction without them, `ctx.noteMark` of an unknown note
   kind (`this.notes.marks[kind]` is `undefined`), a ligature group past the glyphs
   (`glyphs[pos]!.cp`), `String.fromCodePoint` past U+10FFFF; the formula depth (F2-6) →
   `Err(Limit(Depth))`.
8. **Aliasing / identity (LA3)**: `RunStyle` objects → `StyleIx`, one per TS object (the
   picture bullet's `{ ...ms, shift: 0 }` and every `styleOf` call of an equation are new
   entries); `FontFace` → `FaceId` (F1's resolver); `Glyph.obj` → `RunKey` of the paragraph run,
   a field-result run through `LDoc::field_results` (one level, F4-12), `None` for the copies the
   TS makes (`fieldValueRun`, drop-down: new objects, never model runs, F4-6); `Glyph.ptab` → the
   `RunKey` of the run carrying it (F4-4); `ctx.rev.paragraph(para0)` copy → the (`ppr`, `props`)
   override with **no `ParaId`** (a manual-hyphenation skip never matches it, LA2); `MathBox` →
   index into `PreparedPara::math`; `NumLevel` object → `MarkerLevel::Doc(LevelSrc)` or
   `MarkerLevel::Synthetic`. **`json_key` sites**: `fxKey = JSON.stringify(fx)` (para.ts:249) →
   `para::fx_json` (the key order `resolveTextFx` writes, NaN → `null`); `JSON.stringify(r.ot ??
   null)` in the ligature key (para.ts:476) → uo-docx `JsonWriter::stringify` (model key order).
   Both are compared as strings only.
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F4-1 | `PrepFlow` trait for what the TS reaches through `ctx`/`env` outside the model: `ctx()`, `field(cx, src, name, instr, at)`, `number_next`, `note_mark` (default: the real `DocContext` with `word::format_number` under the text cap), `on_field`; the engine implements it (F7), the facet replays the TS transcript | as F5-1: the algorithm is replayed on the TS answers now (no engine yet); the flow state stays the engine's |
| F4-2 | `prepare_paragraph(cx, flow, para: LParaId, env)`; `para_visibility(cx, flow, para, env) -> (markHidden, visible)`; `is_cjk_font(cx, flow, name)` | F0 had no flow; the `DocContext` is the engine's |
| F4-3 | `ParaEnv.table_ppr`/`table_rpr: Option<Arc<PPr/RPr>>` (F0: `table_style: Option<u32>`); `FieldSource::Footnote/Endnote` carry a `NumKey` (F1-3); `FieldSource::None` is a callback that answers nothing | the TS passes the merged objects; JS number ids |
| F4-4 | `Glyph.ptab: Option<RunKey>` (F5-11 had an index into `LPara.runs`, wrong for a `w:ptab` in a field result); F5's `ptab_of_cx` and its facet decoder adapted | the TS object is the display run's |
| F4-5 | `Ch` holds 4 units (the upper case of one small-caps letter is up to three BMP code points, e.g. U+1F52); `Glyph.note_id: OptF64`; `Glyph.pic_bullet: Option<Box<PicBulletGlyph>>` (the synthetic `{ text: '￼', image }` run of a picture bullet: id + drawn size; `obj` is `None`); `ParaMarker.level: MarkerLevel`; `RevBars.ranges` as `f64` (F2's `revision_ranges`) | the TS values |
| F4-6 | Field-value and drop-down copies are display entries (`DRun`) over the base run's fields with their own text; `GLUED_FIELD` is the entry's flag | no `Run` clone; the copies are new objects in the TS too |
| F4-7 | `tf.alpha === null` → no `alpha` (the TS writes `alpha: null`) | `OptF64` has no `null`; the reader never produces it |
| F4-8 | A field or formula text cut by `TextCap` (F2-4) warns `field text truncated` | the TS has no cap (K11b) |
| F4-9 | Depth guards on `fieldRuns` and on fractions (`max_depth`, 192) | LQ13: a stack overflow would abort the wasm instance |
| F4-10 | Work budget: one unit per code point in `pushChars`, per glyph of a ligature run, per letter run of `markHyphenation`, per equation text unit | K16 |
| F4-11 | `HyphenSessions { registry, sessions }`: a pattern set is used only when the registry holds it **and** `hyphenation_loaded` names it; the F0 `WordCache`/`WORD_CACHE_CAP` (100,000) are gone — the uo-fonts session keeps its own cache (TS rule: cleared past 50,000) | one cache, the TS one |
| F4-12 | `LDoc::field_results: FxHashMap<(RunKey, u32), RunKey>` (filled by `derive`, F7a): the key of a field-result run; deeper results have no key (no `RunRef` either) — an inline picture there has `obj` `None` | additive to F0's `LDoc` |
| F4-13 | `math::MathEnv` trait for the TS closure object `menv` (style, face, advance, kerning, the face table, the depth guard, the budget); `math_text_metrics(fonts, face)` | no closures in data |
| F4-14 | Strings outside the TS unions map to the value every consumer gives them: `lineRule` ≠ exact/atLeast → `Auto` (lines.ts:110–129, engine.ts:2087), `suff` ≠ tab/space → `Nothing` (lines.ts:230, table.ts:588), a tab `val` outside the enum → `Left` (`nextTab` treats it as left), a leader → F5-6 | enums; only the facet would print the original string (none in any corpus) |
| F4-15 | The hyphenation word goes to the uo-fonts session as UTF-8 (letters, marks and their NFC are valid code points) | the uo-fonts API takes `&str` |
| F4-16 | Harness: `facets-f4.mjs` (+ `F4_WRAPS` = `resolve.js` `mergePPr`/`mergeRPr`, the `tableStyle` hook installed after F2's patch, the `UO_F4_STATS` counters), `facets.mjs` wires it (the F0 seed of `p3.prepare` removed), `facets-f5.mjs` exports `Raw` and `textfx`; `tests/f4_parity.rs`, `tests/f4.rs`, the committed fixture `tests/captures/f4` (one CI document, its dump, transcript and docx; open fonts only) | — |
| F4-17 | Shared edits inside the crate, additive: `model.rs` re-exports (`DocFormatting`, `MathNode`, `OpenTypeProps`, `TabSpec`, `RunProps`, `JsonWriter`), `word.rs` re-exports `uo_docx::word::obj`, `ldoc.rs` `field_results`, `lines.rs` `ptab_of_cx`, `Cargo.toml` dev-dependency `uo-fonts` with `hyphen-data` (the replay loads the seven pattern sets as the capture), `tests/f5.rs`/`f5_parity.rs` follow the type changes. **No file outside `core/crates/uo-docx-layout` was touched** | — |

**Limits** (LQ14): `max_depth` (field-result nesting, fraction nesting), `max_parse_depth`
(formulas, F2), `max_field_text` (F2's `TextCap`), the work budget (F4-10); every other loop is
bounded by the paragraph's runs and code points.

### Not done (handed on)

- **F7a**: the derived paragraphs (`derive.rs`) — the 1,026 pending oracle calls (above) are
  compared through P1 once they exist; `derive` fills `LDoc::field_results`. The engine's
  `PrepFlow` (field sources: page, header/footer, note marks; `on_field` → `collect_fields`).
- **F7b**: the picture-bullet glyph has `obj` `None` and `pic_bullet` set — F5's paint gate
  (`g.kind == Obj && g.obj.is_some() && g.adv > 0`, paint.rs:288) and `paint_inline_object` must
  take it (the image of `numbering.picBullets[id].image` at the drawn size).
- **F7c**: `DocContext::note_mark` keeps its `format_number` closure parameter; the engine's
  `PrepFlow::note_mark` default already passes `word::format_number` with the call's text cap.
- The `p3.prepare` values in the older committed captures (`tests/captures/f0`, `f1`, `f5`
  `layout-ci.capture`) are the F0 seed encoding; nothing compares them (`layout_parity` reports
  `pending` until F7a). Recapture them with the frozen build when F2-20 is handled.
- The CI job `layout-parity` (`.github/workflows/rust-core.yml`, outside the crate) does not run
  `f4_parity` against its fresh capture yet (it needs `--docs-out` and `UO_F4_DOCS`); the committed
  fixture runs in `cargo test`.

## Stage 3 — F6 (tables): brief (2026-11-05, written before the port)

F4 closed green in this session; F6 was **not started** (no file of F6 was touched: `table.rs`
keeps its F0 stubs `Unported(F6, …)`), because a faithful port of `table.ts` and its facet harness
(which must replay the `CellLayouter` callback with whole item lists) does not fit in what was left
of the session without leaving a half-ported module. The brief below was written from the frozen TS
(d0f45bc5) for the next session.

**F6 — tables** (`table.ts`, 1,406 lines; Opus). TS, in file order:
- `splitContent` 91 (cut a cell's content at a vertical position: `lastOf` by line object — K2e:
  line `ItemMeta`; `moved` `Map<object, object>` of shifted items/lines → index remaps; `nest` rows
  remapped by index, F0's `NestRow` by ranges), `itemTop` 152, `splitAtNest` 158, `nestedRowCut`
  198 (refs split by `/`), `widowCut` 227 (`ParaSplitInfo` per line ref: widows/orphans, `after`,
  `band`).
- The width model: `widthTw` 266, `GeoCell` 274, `boundaryWidths` 293, `prefOf` 321, `fitPctMins`
  327, `fixedMins` 352, `columnWidths`/`columnWidthsExact` 396–522 (Word's autofit: `S1`/`S3`/`S4`
  stages, `lerp`, `noWrap` floors, `tblW` dxa/pct, the stored grid when it already has the width) —
  pure over numbers, the first piece to port and unit-facet.
- `measureCell` 524 (`ctx.pure` around `prepareParagraph` per cell paragraph — F4 done; the nested
  table literal `{ pPr: {}, rPr: {} }` — the F4 facet already replays it), `anchoredWidth` 558
  (textbox `autoFit && noWrap`: `prepareParagraph(ctx, tp, {})` of the box's `paras`),
  `paraContentWidths` 576 (min/max line widths with the marker's `suff`, tabs at `defaultTabStop`).
- `CellSlot` 616, `isHiddenRow` 638 (`paraVisibility` — F4 done).
- `layoutTable` 647–1266: the hidden-row filter; style cascade (`tableStyle`, `mergeTblPr`,
  `tblLook` over `DEFAULT_LOOK`, `defMar` 108 only without any table style, `noVaSp`); the slot
  grid (gridBefore/After, gridSpan, vMerge spans, RTL mirroring `col` vs logical `lcol`); the
  conditional cascade per slot (`condFor` over `COND_ORDER`, banding, `resolved` map 741–757 → the
  `tablePPr`/`tableRPr` F4 consumes); borders (`edge`, `heavier`, `topWinner`, separated vs
  collapsed, `tblPrEx`), cell margins (`cellMarOf`, half borders); column widths through
  `measureCell` (`rawMeasure` memo); row by row: cell contents through the **`CellLayouter`**
  (`layoutCell(cell, x, width, { tablePPr, tableRPr, darkBackground, cellMar }, ref)`, also inside
  `ctx.pure` to measure, 852/947–950), vertical merges (`pendingMerges`, `layoutVertical`,
  `mergeAt`/`mergeText`/`mergeFills`), `trHeight` exact/atLeast, `natural`, vertical alignment
  (`contentDy`, `placeContent`), text direction (`verticalDir`, `paintVertical`), shading
  (`cellFill`/`shdFill`), border items through F5's `paint_border`/`border_width`, `breakBorderItems`,
  `topCut`/`topBorders`/`ownTopBorders`, cell boxes for the editor (`cells`), `effR`,
  `borderLeft`, `tblpPr`; and **`RowLayout.split`** 1193 (a closure: F0's `RowSplit` data +
  `split(&self, avail, cx)`), called by the engine (engine.ts:2756), not by `layoutTable`.
- Exports the engine uses (F7b): `mergeTextFirst` 1330, `atPageTop` 1351, `dropMoved` 1375,
  `LATE_ITEMS` 1383 (`WeakMap<object>` → `ItemSide::meta`), `shiftItem`/`shiftItem0` 1385 (F0's
  `push_shifted`/`shifted`), `isHiddenRow`, `splitContent`.

Rust: `table.rs` (F0 types: `CellContent` with `nest` by index ranges, `NestRow`,
`ParaSplitInfo`, `RowLayout` with every TS field, `RowSplit`, `TableLayout`, `CellEnv`, `trait
CellLayouter` implemented by the engine through `Cx`, `layout_table`, `split_content`). Inputs:
F2 (`table_style`, `merge_tbl_pr`/`merge_tc_pr`/`merge_ppr`/`merge_rpr`, `COND_ORDER`,
`resolve_color`, `shading_fill`), F4 (`prepare_paragraph` with a `PrepFlow`, `para_visibility`,
`is_dark`), F5 (`paint_border`, `border_width`), the F0 item side tables (K2e).

**Facets** (to add in `tools/lib/facets-f6.mjs`): `p3.colwidths` — the autofit stage, recorded by
wrapping `layoutTable` and recomputing nothing (the widths are visible as the cell `x`/`width` the
`CellLayouter` receives, in call order); `p3.table` — every `layoutTable` call: inputs (the table's
model position in `modelRefs` terms, `frameX`, `frameWidth`, `ref`) and the transcript of every
`layoutCell` call it makes (cell position, `x`, `width`, the env as the F4 recipe of
`tablePPr`/`tableRPr` + `darkBackground` + `cellMar`, `ref`, and the returned `CellContent`: items in
the P1 item encoding of canon.mjs, lines, height, flags, `paras`, `nest` as index ranges, whether it
ran inside `pure()`), value = the `TableLayout` in the P1 encoding (rows: every field, items, lines,
border items, merge records, cell boxes, `split` present); `p3.rowsplit` — every `row.split(avail)`
the engine makes (the recorder wraps the closures of the returned rows) with its two parts. The
Rust replay decodes the `CellContent`s with `tests/common/decode.rs` (the P1 decoder of F0) and
answers the `CellLayouter` from the transcript, as F4 answers its `PrepFlow`.

**Pitfalls**: item and line identity across `moved`/`keep`/`own` sets and `lastOf.get(ref) === l`
(K2e, LA3 `ItemMeta` rows) — the TS keeps the *object*, so a shifted copy is a different line;
`Math.max(...[])`/`Math.min(...[])` = ∓∞ in the width model; `reduce` sums in array order; the
`resolved` map is keyed by slot object (index the slots); `measureCell`/`layoutVertical` run the
`CellLayouter` inside `ctx.pure()` (flow state restored, the clone count charged to the budget);
RTL mirrors the grid but conditions use the logical column; `widthTw` of `pct` against the table
width vs the frame width; spreads `argument lists ≥ 10⁵` (`Math.max(...)`, table.ts:129, 153; K11a)
succeed in Rust (accepted, LQ14); `max_table_cells` checked when the slot grid is built.

## Stage 3 — F6 (tables)

Opus, 2026-11-05, hybrid mode, one session (≈ 2 h 15 min), in parallel with F7a (engine core, same
crate) and the uo-docx session work. **0 port parity rounds**: every `layoutTable` call, row split
and content split of every corpus was equal at its first replay; the two harness rounds are below.
Built from the brief above, against the frozen TS (d0f45bc5).

### Ported (TS → Rust, TS operation order)

| TS (`table.ts`) | Rust |
|---|---|
| `splitContent` 91, `itemTop`/`keyY` 129/152, `splitAtNest` 158, `nestedRowCut` 198, `widowCut` 227 | `table::{split_content}` (+ private `split_content_at`, `split_at_nest`, `nested_row_cut`, `widow_cut`, `item_top`) |
| `widthTw` 266, `GeoCell` 274, `boundaryWidths` 293, `prefOf` 321, `fitPctMins` 327, `fixedMins` 352, `columnWidths`/`columnWidthsExact` 396–517, `paraContentWidths` 576 | `table/widths.rs`: `width_tw`, `GeoCell`, `boundary_widths`, `pref_of`, `fit_pct_mins`, `fixed_mins`, `column_widths`/`column_widths_exact`, `para_content_widths` |
| `measureCell` 524, `anchoredWidth` 558, `CellSlot` 616, `isHiddenRow` 638 | `table/layout.rs` `measure_cell`, `anchored_width`, `Slot`; `table::is_hidden_row` |
| `layoutTable` 647–1261: hidden-row filter, style cascade and `tblLook`, slot grid (gridBefore/After, gridSpan, `hMerge`, vMerge spans, RTL mirroring), `condFor` + the `resolved` cascade, `edge`/`heavier`/`covering`/`topWinner`, bands, the geometry (`legacy`, `marOf`, `padOf`, `rawOf`, the geo cells, `jc`/indent/RTL/`rowShift`), separated borders, `layoutVertical`, the row loop (margins, row maxima, `trHeight`, vertical cells, `absorb`, merges ending in the row, `brkW`, `minRow`), `paintRow`, `build`, `r.split`, the pending vertical merges | `table::layout_table` (`table/layout.rs`: `layout_table`, `Tbl`/`RowCtx`/`CellInfo`, `paint_row`, `build`, `split_row` = `RowSplit::split`, `layout_vertical`) |
| `cellFill` 1267, `shdFill` 1271, `swapLR` 1274, `verticalDir` 1286, `paintVertical` 1298, `contentDy` 1310, `placeContent` 1318 | `fill_of`, `shd_fill`, `swap_mar`/`swap_tbl`/`swap_tc`, `table::vertical_dir` (+ `VDir`), `paint_vertical`, `content_dy`, `place_content` |
| `mergeTextFirst` 1330, `atPageTop` 1351, `dropMoved` 1375, `LATE_ITEMS`/`shiftItem` 1383–1406 | `table::{merge_text_first, at_page_top, drop_moved}`; `LATE_ITEMS` = `ItemSide::meta`, `shiftItem` = `PassItems::push_shifted` (F0) |

`Unported(F6, …)` is gone from the crate.

### Parity (P3 facets; TS capture regenerated in the same run, LQ21)

`tools/lib/facets-f6.mjs` (hooked by `facets.mjs`; `layoutTable`, `splitContent` and
`paraVisibility` wrapped by the loader): **`p3.table`** — every `layoutTable` call (completion
order) as one `bytes` item holding the whole `TableLayout`; **`p3.rowsplit`** — every outermost
`row.split(avail)` the engine makes (the split closures are wrapped when they leave `layoutTable`
or a split and named `<call>.<row>`/`s<n>b`; an engine copy `{ ...row }` keeps the wrapper); and
**`p3.splitcontent`** — every outermost engine `splitContent`. A value is `bytes(structure)
bytes(P1)`: the item and line lists are the pages of one canon.mjs stream (faces, images and
`lineBox` lines labelled by first appearance across the value; faces/images made canonical per
descriptor first), the structure holds every other field, the `LATE_ITEMS` marks and the object
identities as list indices (`mergeText`, `topBorders`, `mergeFills`, `NestRow.items/lines`). The
dump `<key>.f6.bin` holds per call the table's model position, `frameX`/`frameWidth`/`ref` and the
transcript of what the table asked outside the model: every `layoutCell` (cell, `x`, `width`, the
env as the uo-docx digests of `tablePPr`/`tableRPr` + `darkBackground` + `cellMar`, `ref`, inside
`pure()` or not, and the returned content in the same encoding with the split ids of its nested
rows), every measuring `prepareParagraph` (paragraph position, env, `pure`, what
`paraContentWidths` reads) and every `paraVisibility` of `isHiddenRow`. `tests/f6_parity.rs`
reads the docx with the uo-docx reader, finds the table by its position, answers the
`CellLayouter` from the transcript (the contents decoded with `tests/common/decode.rs`'s new
`decode_p1_shared`: one resource table per document), runs `layout_table`/`RowSplit::split`/
`split_content` with the real style cascade, borders and paint, encodes the result as the TS and
compares byte for byte, then rebuilds the three facet digests.

| Corpus | Documents | `layoutTable` | row splits | content splits | Equal | Pending | Mismatches |
|---|---|---|---|---|---|---|---|
| committed fixture `tests/captures/f6` (CI `gen/1/7`, open fonts) | 1 | 3 | 1 | 0 | 4 | 0 | 0 |
| CI mode, seed 1 | 10 | 21 | 17 | 0 | 38 | 0 | 0 |
| CI mode, seed 7, 200 documents | 200 | 502 | 400 | 0 | 902 | 0 | 0 |
| .NET synthetic inputs | 8 | 1 | 0 | 4 | 5 | 0 | 0 |
| Word oracle, all documents, installed fonts (`emwidth/ew-emb` skipped: embedded bytes, as F0/F1) | 2,741 | 1,353 | 180 | 58 | 1,590 | 1 | 0 |

On the oracle 2,740 documents are equal on the three facet digests. **Pending** (never counted
equal): `txbxlink/lk-table2`, a table of a chained textbox's continuation (a copy the engine derives,
F7a's `derive`; its model position is unknown). Coverage includes the `tables`, `tblw`, `tblpct`,
`tblsp`, `cellsp`, `vmorder`/`vmshd`/`vmexact`/`vmdouble`, `rowsplit`/`rowwidow`/`rowmin`/
`nestsplit`/`nestwidow`, `tblvert`, `rowjc`, `hidden`, `keeptbl`/`keepchain`/`partbl` (the
`preview` tables) and `txbxlink` groups. Timing: TS oracle capture with dumps 1 min 46 s (1.5 GB of
dumps, kept under `target/`, not committed); Rust replay 17–20 s (release).

Harness rounds (no port bug): (1) the JS encoder wrote the nested rows' split ids in the *values*
too (they belong only to the inputs): 2 oracle content splits differed; fixed in `facets-f6.mjs`.
(2) the engine's keep-with-next preview lays tables out with the ref `'preview'`
(engine.ts:2550), which `LineRef` could not hold: 16 oracle documents failed to decode; fixed with
`LineStory::Preview` (F6-6).

### Memory and robustness (`tests/f6.rs`)

| Check | Result |
|---|---|
| M1: four tables covering every path (borders and fills, a dark cell, vertical merges with fills, exact/atLeast heights, vertical text in single and merged cells, RTL + separated borders + fixed layout + gridBefore/After, a nested table split as a row, a hidden row, `noWrap`) laid out with a synthetic `CellLayouter`; every row through `merge_text_first` → `drop_moved` → `at_page_top`, every split row split at four heights (and its rest again), a nested-row content through `split_content` at four heights | 12 rows, 21 splits: 5,014,184 B allocated, 473,494 B peak, **0 B left** |
| M1/M3: one 500-row × 3-cell table | 35,828,092 B allocated, 7,010,730 B peak (≈ 14 KB per row, the synthetic contents included), **0 B left** |
| M5 | no new cache: `rawMeasure` and the measures live in the call; the table-style cache is F2's `StyleResolver` in the pass's `DocContext` |
| no panic: 600 hostile tables (two modes: wild — NaN/±∞/−0/1e300/negative spans, `gridBefore` and widths; moderate — sane spans, hostile widths), random `vMerge`/`hMerge`/`textDirection`/`noWrap`/borders/`trHeight`/spacing/`jc`/RTL/fixed, nesting 3 deep; every row split at two heights and its rest through `at_page_top` | 429 laid out (912 rows), 82 `Limit(Table)`, 89 `Internal` (`byEnd[e]` of a NaN/negative boundary: the TS throws a `TypeError`); item side data parallel everywhere |
| limits | `gridSpan` 1e9 → `Limit(Table)`; 10,001 cells over a 10,000 cap → `Limit(Table)`; 13 nested tables over `max_depth` 8 → `Limit(Depth)` |
| unit | fixed layout with `tcW`: the widths, the cell boxes, the mode-14 position (`x = frameX − 108`), the cell refs |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`, `min`, `max_n`, `min_n` for the spreads,
   `round`, `floor`, `ceil`, `trunc`, `abs`, `rem` for `Math.floor(c0 / band) % 2`). JS array reads
   through `table::at` (an index that is negative, fractional, NaN or past the end reads
   `undefined` = NaN in arithmetic; a write there is ignored, as it is invisible to the array),
   `Array.prototype.slice` through `table::slice` (`ToIntegerOrInfinity`, negative from the end),
   `Array.from({ length })` through `ToLength`. Spans, columns, `gridBefore`/`gridAfter` and every
   width stay `f64` (NaN and negatives reach them); integers only for positions. Model numbers
   with `null`: `?? ` nullish (`nn`), `ToNumber(null)` = 0 in arithmetic and comparisons. **No
   arithmetic-semantics change.**
2. **Regex**: `/\/r\d+\/c\d+/.test(ref)` (the `legacy` test, table.ts:804) by a hand matcher over
   the TS string of the ref (`has_cell_ref`); none other.
3. **NaN/±∞ and absent keys**: `??` and `||` per site as the TS (`widthTw(tblW) || frameWidth`,
   the `??` chains of `marOf`); a `dxa` of `null` is `undefined` for `widthTw` (every caller
   coalesces it); keys present with `undefined` (the cleared bit) spread as the TS through uo-docx
   `assign`/`spread` (`{ ...DEFAULT_LOOK, ...look }`, the row's `trp`, the cell margins, `{
   ...tblBorders, ...exb }`, `swapLR`); an empty colour string is falsy for every `if (fill)`; an
   empty `vAlign` is left out of `mergeText`. NaN comparisons fall as in JS
   (`#[expect(float_cmp)]` where the TS compares exactly).
4. **Orders**: slots in row order (reversed for RTL), cells, pending merges and transcript calls in
   TS order; the four sorts (`late` by span, cells by `bx0`, `mergeAt` by `at`/`col`, tab stops)
   through uo-docx `js_sort` (V8's decision on NaN); `nestedRowCut`'s rows and `widowCut`'s refs
   in `Map` insertion order (`IndexMap`), `paras` in `Map` order.
5. **Recursion**: nested-table measures, cell → engine → nested table, and split → `splitContent`
   → nested split are bounded by `max_depth` → `Err(Limit(Depth))` (the TS recurses until V8's
   stack throws `RangeError`); `cx.depth` counts the host calls.
6. **Module state**: `LATE_ITEMS` (`WeakMap<object>`, the only one) → `ItemSide::meta`, copied by
   every `shiftItem` copy as the TS copies the entry.
7. **Throws** → `Err(Internal)`: `byEnd[e]!.push` with a boundary that is no index (NaN or a
   negative grid column: `TypeError`), `rows[startRow]!`, and the slot/cell lookups the TS asserts
   (unreachable). V8 argument-spread limits (`Math.max(...rows)`, `...cellInfos`, K11a) succeed
   here (accepted, LQ14).
8. **Aliasing / identity (LA3)**: item and line **objects** → the new `ObjId` (keys.rs) on
   `ItemSide::obj`/`PassLine::obj`: `NestRow.items`/`lines`, `mergeText.items`/`lines`,
   `mergeFills.item`, `topBorders`, and the sets `own` (splitAtNest), `moved` (splitContent's remap,
   `mergeTextFirst`/`dropMoved`), `keep`/`top` (atPageTop); a `shiftItem`/`shiftLine` copy has none
   (`push_shifted`/`shifted`/`shifted_xy` drop it), a move keeps it (`push_same`, clone).
   `lastOf.get(ref) === l` → the line's position in `c.lines`; `Map<CellSlot, …>` (`resolved`,
   `rawMeasure`, `contents`) → fields of `Slot` / vectors by slot or cell position; `Map<GeoCell>` →
   `GeoCell::ix`; `o === s` → the index; `target !== first` → row indices; the split closure →
   `RowSplit { Arc<RowCtx>, contents, draw_top }` (LA3 "closures stored in data"). The rows
   themselves carry no identity (F7b keys `floatFlow.idx` by its own row positions). **`json_key`
   sites: none** (no `JSON.stringify` in table.ts).
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F6-1 | `trait CellLayouter { ctx(), layout_cell(cx, cell: &Cell, key: CellKey, x, width, env, at), prepare(cx, para, env), visibility(cx, para), pure_depth(Δ) (default no-op) }`: the TS `layoutCell` callback plus what the table code reaches of the engine's `ctx` (the style resolver, `pure()`'s flow state) and the two `para.ts` calls on model paragraphs (the engine maps a model paragraph to its `LParaId`); `Engine` implements it (F7a/F7b), the facet replay answers from the TS transcript | no closures in data (K18); the engine holds the flow state and the `LDoc` keys |
| F6-2 | `layout_table(cx, host, table: &Table, frame_x, frame_width, at)` (F0: `table_block: u32`); the hidden-row filter keeps the model row index for `CellKey` while refs use the filtered index, as the TS | the engine has the table; `CellKey` addresses the model |
| F6-3 | `split_content(cx, c, avail)` and `RowSplit::split(&self, cx, avail)` take the `Cx` (a nested row's split paints borders) | F0's signatures had none |
| F6-4 | `ObjId` (keys.rs), `ItemSide::obj`, `PassLine::obj`, `PassItems::push_same`; `push_shifted`/`PassLine::shifted`/`shifted_xy` drop the identity and keep the `LATE_ITEMS` mark | one field could not be both the `LATE_ITEMS` mark (copied) and the object identity (not copied), F0 conflated them |
| F6-5 | type changes against F0: `CellContent.paras: Option<Arc<IndexMap<LineRef, ParaSplitInfo>>>` (one TS `Map` shared by the parts); `NestRow.items: Vec<ObjId>`, `lines: Vec<NestLine { obj, at }>` (not index ranges: rows are not contiguous after splices and a line that left the content keeps its ref, which engine.ts:3313 reads); `MergeAt { col: f64, at: u32, end_row: u32 }`; `MergeText.items/lines` are copies carrying the row's ids, `content: Option<Arc<CellContent>>`; `MergeFill { item: ObjId, end: u32 }`; `RowLayout.top_borders: Option<Vec<ObjId>>`; `TableLayout.tblp_pr: Option<Box<TblpPr>>` (F0: `floating: bool`); `CellEnv.table_ppr/table_rpr: Option<Arc<…>>` (F0: `table_style`) | the TS values; K2e |
| F6-6 | a cell ref's grid column goes through `to_uint32` (a negative `gridBefore` prints `c4294967295` where the TS prints `c-1`; only hostile files); `LineStory::Preview` (output.rs) for the engine's `'preview'` table, whose path starts at the row | `RefPath` holds `u32`; the TS ref exists |
| F6-7 | `merge_text_first`/`at_page_top`/`drop_moved` return `Option<RowLayout>` (`None` = the row itself: the engine compares `row === row0`); `fits: &mut dyn FnMut(f64) -> bool`; `moved: IndexSet<ObjId>` | identity of the returned row |
| F6-8 | a moved copy in `splitContent`/`splitAtNest` takes the original's `ObjId` (the original is not in that part: `moved.get(it) ?? it` names the copy); an `atPageTop` cut copy keeps its id (only `mergeFills` name it: `items[row.items.indexOf(f.item)]`) | same names, no remap tables |
| F6-9 | identity scopes: the rows of one `layoutTable` take ids from one counter from 1; a split's own top borders from 2³⁰ (compared only inside that part's `borderItems`); a split content's new nest objects from 1 + the content's largest id. **F7a's `layoutStatic` tags each nested row's items and lines with ids unique in the content** (from 1) | ids are compared only inside these families |
| F6-10 | `pure()` (table.rs): `save_flow`/restore around the host call, restored also on `Err`, charged 1 unit and `pure_clones` | LA4 |
| F6-11 | work budget: `layout_table` 1 + rows, one per slot, per `vMerge` scan step and per measured block; `paint_row` 1 + cells; a split 1 + cells; `split_content` 1 + lines | K16 |
| F6-12 | `RowSplit` holds an `Arc<RowCtx>` (row cells, bands, margins) with an `Arc<Tbl>` (slots with their resolved `tcPr`, borders, geometry) shared by every closure of the table | the TS closure scope, without copies |
| F6-13 | harness: `facets-f6.mjs`; `facets.mjs` and `capture-layout.mjs` wire it (`LATE_ITEMS` passed in); `tests/common/decode.rs` gains `decode_p1_shared` + `Share` (faces/images deduplicated by descriptor, `lineBox` labels offset per stream); the JS makes faces/images canonical per descriptor per value (two TS objects with one descriptor would be labelled once on both sides) | one resource table per document for the replay |

**Limits** (LQ14): the grid's column count (`Array.from`/`push` up to `usedCols`) and the slot count
over `max_table_cells` → `Err(Limit(Table))` (the TS allocates until memory runs out); `fixedMins`'
`Array.from` length over `max_table_cells` → `Limit(Table)` (TS: `RangeError`/memory); recursion
over `max_depth` → `Limit(Depth)` (TS: `RangeError`); the work budget (F6-11) → `Limit(Work)`
(TS: hangs).

### Shared files edited (additive)

`keys.rs` (`ObjId`, the LA3 table row), `items.rs` (`ItemSide::obj`, `PassLine::obj`,
`push_same`, the identity dropped by the shifts), `model.rs` (table re-exports), `output.rs`
(`LineStory::Preview` + its `to_ts`), `tools/lib/facets.mjs`, `tools/capture-layout.mjs`,
`tests/common/decode.rs` (`decode_p1_shared`, `preview` refs). Owned: `src/table.rs`,
`src/table/{layout,widths}.rs`, `tests/f6.rs`, `tests/f6_parity.rs`, `tests/captures/f6/`,
`tools/lib/facets-f6.mjs`.

### Not done (handed on)

- **F7a/F7b — the engine's `CellLayouter`**: `ctx` = the pass's `DocContext`; `layout_cell` =
  `layoutCell(cell, x, w, env, ref, ps)` (cell by `&Cell` or `CellKey`); `prepare` =
  `prepare_paragraph` of the model paragraph's `LParaId` (cell paragraphs and textbox paragraphs
  of `anchoredWidth`); `visibility` = `para_visibility(…, ParaEnv::default())`. `layoutStatic`
  builds `NestRow`s with `ObjId`s on the row's items and lines (F6-9) and `paras` as an `Arc`; a
  nested row is placed with `push_shifted` (a new object) and then tagged.
- **F7b call sites**: `placeTable`, floating tables, row emission (`RowSplit::split`,
  `merge_text_first` with the engine's `fits`, `at_page_top`, `drop_moved`), the `mergeFills` skip
  of engine.ts:3126 by `ObjId`, `fl.idx` (row positions), `fl.seen` (row, entry), `LastRowEmit`;
  the `ItemMeta` → `{ z, seq, behind }` table of `LATE_ITEMS` lives on the engine; the preview of
  `nextBlockLead` uses `LineStory::Preview`.
- `mergeTextFirst`, `atPageTop` and `dropMoved` have no facet of their own (their inputs are engine
  rows with identities): P1 covers them through F7b; M1 and the no-panic loop run them.
- The chain-continuation table of `txbxlink/lk-table2` is pending until F7a's `derive` builds it.
- The CI job `layout-parity` does not run `f6_parity` against its fresh capture yet (needs
  `--docs-out` and `UO_F6_*`); the committed fixture runs in `cargo test`.

## Stop point (2026-11-05, after F4)

Done: F0; wave 1 — F1 (0 parity rounds), F2 (0), F3 (1), F5 (0); wave 2 — **F4 (1 round:
the `.notdef` metrics face)**. F6 not started (brief above). Next: F6 tables, then wave 3 (F7a
engine core, F7b ∥ F7c, F8). Open items carried: the F2-20 recapture of the older fixtures (now
also their `p3.prepare` encoding), the `layout-parity` CI steps for `f4_parity`, the F4 hand-ons
to F7a (derived paragraphs, the engine `PrepFlow`), F7b (picture-bullet painting) and F7c.

## Stage 3 — F7a (engine core)

Opus, 2026-11-05, hybrid mode, in parallel with F6 (tables, same crate) and uo-docx session work.

### Brief (written from the frozen TS d0f45bc5 before porting)

**F7a — engine core** (`engine.ts` ≈ 1,900 of 4,183 lines, `frames.ts`, `chain.ts`). TS, in file
order: `layoutDocument` 188 (the ≤ 5-pass loop with its stability test over NUMPAGES,
SECTIONPAGES, PAGEREF marks, NOTEREF marks, late floats/tables, the each-page note marks and the
chapter index; then the balancing search: per balanced section ≤ 20 searches, `byAvg` for ≥ 3
columns, `trial`/`ok`, a bisection where every probe is a full pass — the result is the **last
executed** pass, K19), `mergeLate`, `sameCounts`, `sameMap`; `class Engine`: `applyBalance` 405,
`run` 669 (geometry of every section, the section loop with next-column / continuous / new-page
starts, `placeEndnotes`, the float carry, then per page the layer assembly: `LATE_ITEMS` split,
`zItems` order, `cullOffPage`, page borders front/back, revision bars, `hfItems`, background
colour/fill/show), `pageBorders` 770, `geomOf` 784, `pageNumber`, `newPage` (even/odd blank
page), `newPageAt`, `makePage`, `continuousSection`, `recordBalance`, `nextColumnFits`,
`nextColumnSection`, `pageCols`, `colFrame`, `breakColumn`, `noteColContent`, `closeColumns`
(column separator rects), `sectionPageCounts`, `numPagesShown`, `fieldValue` (PAGE, NUMPAGES,
SECTIONPAGES, SECTION, DATE/TIME, SAVEDATE/CREATEDATE/PRINTDATE; SEQ/REF/NOTEREF/STYLEREF/PAGEREF/
ENDNOTEREF_MARK are F7c), `layoutStatic` 1202, `collapse`, `borderTopExtent`,
`borderBottomExtent`, `groupsWithNext`, `paintLineNumber` 1658, `paintBox` 1687, `listOf`,
`gridFor`, `joinHidden`, `noShownRuns`, **`placeParagraph` 1743** (frame skip, deleted marks,
hidden marks, the empty continuous-break paragraph, page break at the start, page break before,
keepNext/keepLines with `paragraphLines`/`closingFit`/`nextBlockLead`, the gap rules — collapse,
contextual, auto spacing in lists, section gap, top gap — the line loop with widow/orphan control
and the keepNext tail, column/page breaks, the border group, the prev record),
`paragraphLines`/`closingFit`/`paragraphHeight`/`nextBlockLead` 2514–2595, `wrapFrame` 3139
(no float on the page: the frame itself), `pickSeg`, `moreSegs`; module helpers `lineRecord`,
`pfmtOf`, `revisionBarItems`, `cullOffPage`, `bdrKey`, `shiftLine`, `markLineH`; `frames.ts`
`liftFrames`/`frameRun` and `chain.ts` `indexChains`/`sliceParagraphFrom` → `derive.rs`.
Out of F7a (pending with their frontier, never counted equal): tables and floating tables,
floats, inline groups/textboxes, frames placed, chains (F7b); notes, endnotes, separators,
headers/footers, SEQ/REF/NOTEREF/STYLEREF/PAGEREF/chapter fields (F7c).

Rust: `engine/mod.rs` (pass loop, balancing, `run`, page assembly), `engine/page.rs` (geometry,
pages, sections, columns), `engine/flow.rs` (`placeParagraph`, keep rules, border groups, line
numbers, `lineRecord`), `engine/fields.rs` (the F7a field values), `derive.rs` (the derived
document). The engine implements F4's `PrepFlow`; runtime paragraph copies (`joinDeleted`,
`joinHidden`, the line-number paragraph) are prepared through a relocated scratch `LDoc` so `Cx`
keeps its shared `&LDoc` (no change to the F4–F6 signatures). Harness: `tests/layout_parity.rs`
`rust_layout` reads the docx with the D1a reader and runs `LayoutEngine::layout`; documents whose
content needs F7b/F7c are counted `pending` by the `Unported` the engine returns.
Pitfalls: K19 (last executed pass), `-0` kept through item moves (no `+0` shift), `Math.max(...[])`
in `closeColumns`, the `JSON.stringify([pBdr, ind])` group key (K9), `lineSeq` per call, the
`cur.geom` reassigned by continuous sections, `boxPage` as a container (K2f).

### Ported (TS → Rust, TS operation order)

| TS (engine.ts unless noted) | Rust |
|---|---|
| `layoutDocument` 188 (revision author order, the ≤ 5-pass loop and its stability test, the balancing search with `byAvg`, `trial`/`ok`, bisection; the result of the **last executed** pass, K19), `mergeLate`, `sameCounts`, `sameMap`, `sameChapters` | `engine::layout_document` (`Call::pass`, `merge_late`, `same_counts`, `same_map`, `same_chapters`, `work_input`) |
| `run` 669 (cover page, first page number, geometry of every section, the default section, the section loop: next-column / new page / continuous, the block loop, `placeEndnotes`, the float carry, the last `closeColumns`/`paintNotes`) | `Engine::run` |
| the end of `run` 714–764 (`LATE_ITEMS` split, `lateZ`, `zItems`, `cullOffPage`, page borders behind/in front, revision bars, `hfItems`, background colour/fill/show, the warnings) | `Engine::finish`, `cull_off_page`, `z_sorted`, `late_split`, `late_z`, `emit_page` |
| `noteMarkFix` 588, `chapterOf` 612, `chapterIndex` 622, `sectionPageCounts` 1083, `numPagesShown` 1094 | `Engine::{note_mark_fix, chapter_of, chapter_index, section_page_counts}` |
| `applyBalance` 405, `pageBorders` 770, `geomOf` 784, `pageNumber` 849, `newPage` 857, `newPageAt` 872, `makePage` 912, `continuousSection` 946, `recordBalance` 978, `nextColumnFits` 997, `nextColumnSection` 1007, `pageCols` 1023, `colFrame` 1027, `breakColumn` 1034, `noteColContent` 1055, `closeColumns` 1067 | `engine/page.rs` (same names) |
| `fieldValue` 1099: PAGE (+ chapter), NUMPAGES, SECTIONPAGES, SECTION, DATE/TIME, SAVEDATE/CREATEDATE/PRINTDATE (`new Date(iso)`) | `engine/fields.rs` `Engine::field_value`, `parse_iso_date`; `impl PrepFlow for Engine` (F4-1) |
| `layoutHeaderFooter` 1139 up to the slot choice | `Engine::layout_header_footer` (a slot with content: `Unported(F7c)`) |
| `collapse` 1614, `borderTopExtent` 1624, `borderBottomExtent` 1635, `groupsWithNext` 1642, `paintLineNumber` 1658, `paintBox` 1687, `listOf` 1704, `gridFor` 1710, `joinHidden` 1730, `noShownRuns` 1739, **`placeParagraph` 1743** (with `flushFrag`), `headingStyleLevel` 551 | `engine/flow.rs` (same names; `flush_frag`, `heading_style_level`) |
| `paragraphLines` 2514, `closingFit` 2523, `paragraphHeight` 2528, `nextBlockLead` 2536 | `engine/flow.rs` (`next_block_lead` + `next_block_lead_step`, F7a-5) |
| `lineRecord` 3938, `pfmtOf`, `revisionBarItems` 3974, `bdrKey` 4089, `shiftLine` (F0) | `flow::{line_record, revision_bar_items, bdr_key}` |
| revisions.ts `joinDeleted` 155 | `Engine::join_deleted` (an `RtPara`, F7a-2) |
| frames.ts `frameOf` 33 | `derive::frame_of` (the lift itself is F7b's, below) |
| the derived document (LA2): every model paragraph with its runs and field results keyed once, the stories (body, header/footer parts, notes, textbox contents), `revisionAuthors` | `derive::build` |

`Unported(F7a, …)` is gone from the crate. The engine stops with `Unported(F7b, …)` at: a table
block (`placeTable`), a framed paragraph (`liftFrames`), a floating object or an inline
group/textbox in a paragraph, a picture bullet, the inline-object ends of F5's paint (F5-9), a
table after a keepNext paragraph (`nextBlockLead` preview), floats on the page (`wrapFrame`), late
floats/tables, the float carry; with `Unported(F7c, …)` at: a header/footer slot with content,
note references and endnotes, the SEQ/REF/NOTEREF/STYLEREF/PAGEREF fields, the endnote flow, a
`Date.parse` string outside the ISO format.

### Parity (P1 through the real engine; TS capture regenerated in the same run, LQ21)

`tests/layout_parity.rs` reads each `.docx` with the uo-docx reader, builds the derived document,
runs `engine::layout_document` with the transcript policy and the capture's options (`now`
2026-01-15T12:00Z, TZ = UTC, the seven pattern sets, `revisions: all` for `mk-*`), and compares
**P1** (the canonical stream), **`pages`** and **`p2first`** (the first request per lower-case key
and the answer identity, LQ19; `p2`, every call, is skipped: the TS asks again in every pass).
The `p3.*` facets are compared by their own replays (`f1_parity`, `f2_parity`, `f4_parity`,
`f5_parity`, `f6_parity`) over the same reader. A document counts as equal only when all three are.
The embedded fonts of a document are added to the font index from the reader (the F0/F1 gap of
`emwidth/ew-emb` is closed this way: it is among the equal documents). `UO_F7A_DIFF=1` prints where a mismatching layout first
differs (pages, lines, items) by decoding the TS dump with F0's decoder.

| Corpus | Documents | Equal | Pending (reason) | Mismatches |
|---|---|---|---|---|
| Word oracle, all documents, installed fonts | 2,742 | **1,203** | 1,539: tables 716, floating objects 331, header/footer content 250, inline images 102 + 1 shape (F5-9), frames 53, footnote/endnote references 44, table after keepNext 16, endnotes 13, SEQ/REF/… fields 10, inline groups/textboxes 3 | **0** |
| `--gen f7a` (tools/lib/gen-f7a.mjs), seed 1 | 300 | **300** | 0 | 0 |
| `--gen f7a`, seed 2 | 500 | **500** | 0 | 0 |
| `--gen f7a`, seed 3 | 500 | **500** | 0 | 0 |
| committed fixture `tests/captures/f7a` (`--gen f7a` seed 7, open fonts; runs in `cargo test`) | 16 | **16** | 0 | 0 |
| .NET synthetic inputs | 8 | **3** (page fills: page, page-pattern, page-tile) | 5: floats, inline image, picture bullet, table, header | 0 |
| F0 generator (`--mode ci`), seed 1 | 30 | 0 | 30: header/footer 27, tables 3 | 0 |

**Round 1** (the only one): 5 of the first 300 generated documents — a hidden paragraph mark
joined to the next paragraph (`joinHidden`) carries its **marker** to the joined paragraph; the TS
marker glyphs hold their `RunStyle` objects, the Rust glyphs a `StyleIx` into their own
paragraph's `styles`, so the moved marker pointed at the wrong styles (two layouts failed with
`Internal("paint: glyph style")`, three painted the wrong style). Fixed by carrying the marker's
styles (`detach_marker`/`attach_marker`, F7a-8); every document was equal at the next replay.
Timing: oracle TS capture with dumps 80 s; Rust replay of the oracle 125 s (release).

### Memory and robustness (`tests/f7a.rs`)

| Check | Result |
|---|---|
| M1: the every-path document (40 paragraphs, three sections, balanced 2- and 3-column sections, line numbers, borders and shading, lists, hidden and deleted marks, keepNext/keepLines, PAGE/NUMPAGES, even-page break): read → derive → layout → drop | 630,165,490 B allocated, 2,122,745 B peak, **0 B left** |
| M1: the same with `max_pages: 2` (the call ends in `Limit(Pages)` mid-pass) | **0 B left** |
| M1: `LayoutEngine::with_hyphenation(…).layout(…)` + `clear_caches()` (after one warm-up) | 15,689,470 B allocated, 841,271 B peak, **0 B left** |
| M2 soak: 1,000 × (open → read → derive → layout → drop) of a one-section fixture document, sampled every 50, and 50 × the every-path document | flat after the first sample, **0 B** variation |
| M3 (release, per call, the 16 fixture documents) | 3.6–367 MB allocated, 0.33–3.5 MB peak, 98 KB–1.6 MB retained; **57–76 B per laid-out character retained** (target of §6: ≤ 48 B/char on the 500-page document — over it here; the char SoA, the glyph runs and the per-line `pfmt` copies are the candidates, stage 5) |
| M5 | no new cache: the face table, the resolver cache, the resources and the hyphenation sessions are per call (F1, F4); `LayoutEngine::clear_caches` keeps the registry and drops the sessions |
| no panic: 300 hostile documents (release; 60 in debug) — page sizes and margins 0, −720, 1e9, NaN, `abc`; columns 0…10⁹; column spaces, line pitches, line-number `countBy`/`start`/`distance`, page-number starts and section types out of range; spacing, indents, line rules, borders (`sz` 1e9, `space` NaN), list levels 9; keepNext/keepLines/pageBreakBefore/hidden marks at random | 264 laid out with every output index valid, 36 `Limit(Capacity)` (columns past `MAX_COLUMNS`); no panic |
| limits | `max_pages` → `Limit(Pages)`; a work floor of 1,000 units → `Limit(Work)`; 10⁶ columns → `Limit(Capacity)` |

The allocation count is dominated by the passes: every balancing probe is a full pass (the
every-path document runs ≈ 30), and each pass prepares every paragraph again, as the TS does.

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`, `min`, `max_n`, `floor`, `ceil`, `rem`,
   `trunc`); `String(n)` through `to_js_string` (NUMPAGES, SECTIONPAGES, SECTION, line numbers);
   `===` between numbers through `js_eq` (NaN ≠ NaN, −0 = 0); block indices of a section are the
   TS numbers (`SectGeom.blocks: (f64, f64)`), read as array indices through `index_of`
   (fractional or NaN → the TS `undefined` read, `Err(Internal)`); `x % 2 === 0` of page numbers
   through `rem`. **No arithmetic-semantics change**; the nested sum of `nextBlockLead` is folded
   from the innermost term as the TS recursion adds (F7a-5).
2. **Regex** (hand matchers, none dynamic): `/^heading ([1-9])$/i` (`heading_level`: ASCII
   case-insensitive, non-`u` `/i` folds no non-ASCII letter to these), `/^(hdr|ftr):/` of
   `recordBalance` → the line's `LineStory`, the ISO date grammar of `new Date(iso)` (F7a-7).
3. **NaN/±∞ and absent keys**: model `??` defaults through `JsNumber::as_f64` (`null` is nullish)
   in `geomOf`; `!!x` of strings (`vAlign`, `pgFmt`, `list`, `styleId`) as non-empty; the empty
   `Math.max(...[])` of `closeColumns` is −∞ (the guard returns); NaN comparisons fall as in JS
   (`!(bottom > colTop)` returns on NaN; `hi - lo > 1` stops a NaN bisection).
4. **Orders**: pages, lines and items in TS order; `balanceFound`/`lateFound`/the inputs in
   `IndexMap` (insertion order); `zItems` a stable sort by `z || seq` (NaN = 0, as V8 reads it); the
   page assembly keeps the layer order of the TS spreads; `chapterIndex` per level in page order.
5. **Recursion**: `nextBlockLead` is a loop (F7a-5); no other recursion in F7a (`newPage` →
   `newPageAt` → `newPage` only for a page of notes, F7c).
6. **Module state**: `lineSeq` per call (F5); the frame caches of frames.ts are not reproduced
   (LQ18; the lift is F7b's); nothing else.
7. **Throws** → `Err(Internal)`: `this.cur` without a page, a block index the TS reads as
   `undefined` (`block.type` of `undefined`), `colFrame` of a page without columns, a paragraph
   whose derived copy has no model root.
8. **Aliasing / identity (LA3)**: `PageState` → `PageIx` (`lnPage`, the `fieldValue` closure's
   `ps`, `chapterOf`'s `pages.indexOf(ps)`); `SectGeom` → its index (`cur.geom === g`); `boxPage`
   → `ContainerId::PageBody(PageIx)` (K2f); the `fieldValue(ps, block)` closure → `FieldSource::Page`
   + `Engine::field_at` (F7a-3); the paragraph objects the engine makes → `PSrc::Rt(RtPara)` over
   the call's keys, prepared through a relocated `LDoc` whose glyph `obj`/`ptab` keys are mapped
   back (K1: a model run keeps its `RunKey`); copies have no `ParaId` (LA2); the marker moved by
   `joinHidden` keeps its own `RunStyle` entries (new indices of the joined paragraph, F7a-8);
   `LATE_ITEMS` marks by `ItemMeta` (`Engine::late_marks`, filled by F7b); `pfmtOf`'s object per
   paragraph → one `Arc<LineParaFormat>` per placed paragraph, emitted once per run of lines.
   **`json_key` sites**: `bdrKey` → `bdr_key` (`JsonBuilder` + uo-docx `JsonWriter::stringify` of
   `pBdr` and `ind`, model key order, `undefined` → `null`), compared with `JsonKey::json_eq`.
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F7a-1 | The page assembly (`Engine::finish`) runs once, on the last executed pass; the TS assembles every pass's `LayoutResult` | only the last one is returned (K19); the assembly of the others has no effect on it — except derived page-fill images, which the TS makes per pass and Rust once |
| F7a-2 | Runtime paragraph copies (`joinDeleted`, `joinHidden`, the line-number paragraph) are `RtPara`s prepared through a scratch `LDoc` that relocates exactly their runs (and field results); `Cx` keeps `&LDoc` | no change to the F4–F6 signatures; works inside cell layouts (F7b) where only a `Cx` is at hand; model runs keep their identity |
| F7a-3 | `ParaEnv.field` of the body = `FieldSource::Page` + the engine's `field_at: (PageIx, block)` set right before the paragraph is prepared | the TS closure captures `this.cur` and `blockIdx`; preparing is synchronous |
| F7a-4 | The line-number paragraph is a synthetic `Paragraph` with `ParaId(u32::MAX)` (`{ runs: [run], props: {}, ppr: {} }`), its run a `RunSrc::Synth` | a digit string never meets a manual-hyphenation skip |
| F7a-5 | `nextBlockLead` over a keepNext chain is a loop with one save/restore of the flow (the inner pairs are invisible under the outer one) and the terms folded from the innermost out | no recursion over a chain of blocks; the TS throws `RangeError` past ≈ 10⁴ chained keepNext paragraphs, Rust lays them out (K11, accepted) |
| F7a-6 | `w:cols/@w:num` above `MAX_COLUMNS` = 4,096 → `Err(Limit(Capacity))` | the TS makes one object per column (10⁹ hangs); Word allows 45 |
| F7a-7 | `new Date(iso)` of SAVEDATE/CREATEDATE/PRINTDATE: the ISO forms only (`YYYY[-MM[-DD]][THH:mm[:ss[.f…]]][Z\|±HH:mm]`, date-only = UTC, date-time = local); any other string → `Unported(F7c)` (V8's legacy grammar) | core properties are W3CDTF; the legacy parser is a port of its own |
| F7a-8 | A marker moved by `joinHidden` carries its glyphs' `RunStyle`s (`HiddenJoin.marker_styles`); attached as new entries of the joined paragraph's `styles` | `StyleIx` is per paragraph; the TS glyphs hold the style objects (round 1) |
| F7a-9 | Work budget: 1 unit per pass, per body block, per page and per `nextBlockLead` level (plus F4/F5's per glyph); `Budget::pure_clones` counts the flow snapshots | K16; a bisection that never ends (an infinite column height) stops at the budget |
| F7a-10 | The TS `Object.prototype` names as `w:chapSep` (e.g. `constructor`) give `-` | the TS would concatenate a function's source |
| F7a-11 | `{ ...st.footnotePr, ...sp.footnotePr }` merged per field (`sp.x ?? st.x` by presence) | the reader writes no key with an `undefined` value |
| F7a-12 | `Layout.missing_faces` stays empty and the `"font budget"` warning is not raised | the registry policies do not report misses yet (F1 hand-on; binding work, F8) |
| F7a-13 | Item moves of the assembly and `paintBox`'s `splice` copy items exactly (`push_copy`, no `+ 0`) | `-0` coordinates stay `-0` (the TS moves objects) |
| F7a-14 | Harness: `layout_parity` through the engine (P1 + `pages` + `p2first`; `p2` skipped, LQ19), the `--gen f7a` generator (`tools/lib/gen-f7a.mjs`), the stale-build refusal of `capture-layout.mjs` with `--accept-stale` (F2-20: the frozen d0f45bc5 `dist` is older than `typography/font/sfnt.ts`, whose last edit is a GPOS cache with the same results — accepted by name), the committed fixture, the CI step (strict) | the gate needs end-to-end parity; the old fixtures need no recapture for F7a (its replay uses fresh captures and its own fixture) |
| F7a-15 | `LayoutEngine::with_hyphenation(registry)` (the F0 `new()` has no patterns); `clear_caches` clears the sessions and keeps the registry | the public path must hyphenate as the replay does; M1 after `clear_caches` |

**Limits** (LQ14): `max_pages` (`makePage`), `MAX_COLUMNS` (F7a-6), the work budget (F7a-9),
`max_depth` in `derive` (table and textbox nesting of the model); every other loop is bounded by
the blocks, the lines of a paragraph or the pages.

### Shared files edited (additive)

`ldoc.rs` (`by_para`, `hf_stories`, `note_stories`, `tb_of_drawing` and their `empty()` values),
`lib.rs` (`LayoutEngine::with_hyphenation`, `clear_caches` keeps the registry),
`tools/capture-layout.mjs` (stale-build refusal, `--accept-stale`, `--gen f7a`),
`.github/workflows/rust-core.yml` (the F7a capture + strict replay step), this record. F0 files
owned by F7a and rewritten: `engine/mod.rs`, `engine/page.rs` (`SectGeom.blocks` is `(f64, f64)`),
`engine/flow.rs` (`PrevPara.p_bdr` is a flag, `HiddenJoin` holds a `PSrc` and the marker styles),
`engine/fields.rs`, `derive.rs`, `tests/layout_parity.rs`.

### Not done (handed on)

- **F7a scope left for the frontiers that make it reachable**: `layoutStatic` (engine.ts:1202:
  cells, headers/footers, textbox contents) and `layoutCell`/`layoutCellIn` — every caller is F7b
  (cells, textboxes) or F7c (headers/footers); with F6's hand-on (`NestRow`s with `ObjId`s,
  `paras` as an `Arc`) and the `CellLayouter` impl. The runtime copies of `layoutStatic`
  (`joinDeleted`/`joinHidden` in cells) use the same `RtPara` path (F7a-2), which works under a
  plain `Cx`.
- **F7b**: `liftFrames`/`frameRun` in every story (K3: the INNER copies, the anchor copy with the
  frame run, `FRAME_SKIP`/`FRAME_CARRY` as `LPara` flags and `frame_carry`, the synthetic
  `DrawingTextbox` → `LTextbox` with `FrameInfo`) and `indexChains`/`sliceParagraphFrom`
  (`RunSrc::Slice`, `CONTINUATION`) — `derive::build` builds neither yet (frames are pending in
  53 oracle documents); `wrapFrame`/`pickSeg`/`moreSegs` with floats and every float branch of
  `placeParagraph` (`early`, `anchorAt`, `frameHere`'s `skipTo`, the tall-line and narrow-gap
  rules, `brclear`), `placeFloats`/`placeFloatsEarly`, `paintInlineDrawings`, late floats and
  tables in `newPageAt`, `placeFloatCarry`, `placeTable` and the table preview of `nextBlockLead`
  (`LineStory::Preview`), the `LATE_ITEMS` marks (`Engine::late_marks`).
- **F7c**: `layoutHeaderFooter` (the slot is chosen; `layoutStatic` over the slot's story), note
  references (`noteNeed`, `reserveNotes`, `notePartial`), `paintNotes`, `placeEndnotes`, the
  continuation separator, `noteCarry`, the SEQ/REF/NOTEREF/STYLEREF/PAGEREF and ENDNOTEREF_MARK
  values (`seqValue`, `refValue`, `noteRefValue`, `styleRefValue`, `bookmarkPages`, `listNumberOf`),
  the header/footer and note `FieldSource`s, V8's legacy `Date.parse` grammar (F7a-7).
- **F8**: `collect_fields` (`PrepFlow::on_field`: the keys of a relocated copy map back through
  the same table as `Glyph.obj`), `missing_faces` and the `"font budget"` warning (F7a-12).
- The P3 facets are not re-derived from the engine run (they have their own replays over the same
  reader); the 1,026 engine-derived `prepareParagraph` calls pending in F4's facet are covered by
  P1 where F7a makes them (joined copies in the `mk-*`/hidden-mark documents) and stay pending for
  the frames, slices and note separator (F7b/F7c).
- M3 is over the §6 target (57–76 B per laid-out character retained, target ≤ 48): stage 5.
- The F0/F1/F5 `layout-ci` fixtures stamped `b92b60fa` (F2-20) are not recaptured: nothing of
  F7a reads them; the capture tool now refuses such a build.

## Stage 3 — F7c (notes, headers/footers, fields) and `layoutStatic`

Opus, 2026-11-05, hybrid mode, one session (≈ 4 h), in parallel with F7b (tables, floats, frames,
objects, textboxes, chains, the engine's `CellLayouter`) in the same crate and with the uo-docx
session work. **0 port parity rounds**: every generated document and every oracle document that
needs nothing of F7b was equal at its first replay. Built from the F7a hand-ons against the
frozen TS (d0f45bc5).

### Ported (TS → Rust, TS operation order)

| TS (engine.ts unless noted) | Rust |
|---|---|
| `layoutStatic` 1202 (the paragraph flow: deleted and hidden marks, contextual/automatic spacing, cell neighbours, the border box, `paras` for row splits, `keepNext`, `alignH`/`shape`; the header/footer and cell float sites, `liftFrames`, `wrapFrame`, `paintInlineDrawings` and the table branches first stopped with `Unported(F7b)` where the TS reaches F7b's code, then F7b wired its code in), `scratchPage` 4148, `ctx.pure` on the engine | `engine/static_layout.rs`: `Engine::layout_static` (stable entry for F7b: `&[SBlock]`, `x`, `width`, `&ParaEnv`, `&LineRef`, `Option<PageIx>`, `y_origin`, `Option<&CellNeighbors>`), `story_blocks`, `make_scratch`/`drop_scratch`, `pure_static`, `run_floats`, `child_ref` |
| `layoutHeaderFooter` 1139 (slot choice, own numbering, the header probe on a scratch page, the two/three footer probes with `onlyTextFloats`, the shifted items and lines, body top/bottom) | `engine/hf.rs`: `Engine::layout_header_footer`, `hf_static`, `hf_probe` |
| `noteCols` 2223, `noteContent` 2232 (cache `id:round(w)`), `rowNoteIds` 2255, `rowNoteExtra` 2272, `paraByRef` 2277, `sepGeom` 2308, `separatorHeight`, `sepLine`, `noteAreaOf` 2332, `packNotes` 2339, `notesExtra` 2353, `noteLineFit` 2362, `noteNeed` 2367, `reserveNotes` 2376, `notePartial` 2402, `paintNotes` 2418, `placeEndnotes` 2455, `continuationSeparator` 2507; `makePage`'s `noteCarry` 924 | `engine/notes.rs` (same names; `place_note_carry`; `row_note_ids`/`row_note_extra` take the row's lines, for F7b's row emission) |
| `placeParagraph`'s note sites (`noteNeed`, `noteCut`/`notePartial`, the fit test with the note room, `reserveNotes` after a page change); `newPageAt`/`breakColumn`'s continuation separator; `makePage`'s `paintNotes`; `run`'s `placeEndnotes`/`paintNotes` | additive edits in `engine/flow.rs`, `engine/page.rs`, `engine/mod.rs` |
| `seqValue` 426, `positionWord` 450, `noteRefValue` 461, `refValue` 485, `listNumberOf` 525, `styleRefValue` 565, `bookmarkPages` 638; `fieldValue`'s PAGEREF/SEQ/ENDNOTEREF_MARK/REF/NOTEREF/STYLEREF arms; the header/footer and footnote field sources | `engine/field_refs.rs`; `engine/fields.rs` (dispatch, `instr`/`at`/`block` optional as the TS `undefined`); `impl PrepFlow::field` (`FieldSource::HeaderFooter`, `Footnote(id)`) |
| V8 `new Date(s)` outside the ISO subset (F7a-7): `DateParser::Parse`, `ParseES5DateTime`, the tokenizer, `DayComposer`/`TimeComposer`/`TimeZoneComposer`, `ReadMilliseconds`, `KeywordTable`; `ParseDateTimeString` | `engine/date_legacy.rs` |

`Unported(F7c, …)` is gone from the crate.

### Parity (P1 through the real engine; TS capture regenerated in the same run, LQ21)

`tests/layout_parity.rs` as F7a (P1 + `pages` + `p2first`). The `--gen f7c` generator
(`tools/lib/gen-f7c.mjs`): headers and footers in the three slots (`titlePg`,
`evenAndOddHeaders`, inherited between sections; PAGE in four formats, NUMPAGES, SECTIONPAGES,
SECTION, DATE/TIME, lists, borders, automatic spacing), footnotes (one to three paragraphs, notes
long enough to split, custom marks, `numFmt` incl. chicago, `numRestart` eachPage/eachSect,
`beneathText`, `w15:footnoteColumns`), endnotes (`sectEnd`/document end, `noEndnote`, two-column
sections), SEQ (`\* \r \c \h \s`), REF (text, `\n \r \w \d`, `\p`), PAGEREF, NOTEREF, STYLEREF
over bookmarks and numbered headings, page-number formats/starts, compatibility modes 14/15.

| Corpus | Documents | Equal | Pending (reason) | Mismatches |
|---|---|---|---|---|
| `--gen f7c`, seed 1 | 100 | **100** | 0 | 0 |
| `--gen f7c`, seed 2 | 500 | **500** | 0 | 0 |
| `--gen f7c`, seed 3 | 250 | **250** | 0 | 0 |
| committed fixture `tests/captures/f7c` (`--gen f7c` seed 7, open fonts; runs in `cargo test`) | 16 | **16** | 0 | 0 |
| F0 generator (`--mode ci`), seed 1 (27 with headers/footers) | 30 | **30** | 0 | 0 |
| .NET synthetic inputs | 8 | **7** | 1: F7b linked textbox chains | 0 |
| Word oracle, all documents, installed fonts (first run, F7b mid-port: its static-flow sites not wired yet) | 2,742 | 2,437 | 292, all F7b: header/footer floats 96, floats of a static paragraph 91, frames 54, tables in a static flow 46, frames in a static flow 5 | 13 (F7b work in progress: `txbxlink/*` chains, an undumped `toptbl` image) |
| Word oracle, all documents, installed fonts (last run, with F7b's static-flow sites wired into `layout_static`) | 2,742 | **2,728** | 14: F7b linked textbox chains | **0** |

On the oracle **no document is pending for an F7c reason** (F7a: header/footer content 250,
note references 44, endnotes 13, SEQ/REF/… fields 10): every one is equal, or was pending only on
F7b's sites inside the static flow, which F7b then wired into `layout_static` (its
`static_table`, `static_cell_floats`, `place_float`/`place_floats`, `wrap_frame`) — the last
run has 2,728 equal and 0 mismatches. The 13 mismatches of the first run were F7b's work in
progress (linked textbox chains, an undumped image); none involved a header/footer, a note or an
F7c field. Timing: TS oracle capture with dumps 5 min 32 s
(on a machine shared with three other agents); Rust replay of the oracle ≈ 10 min (release, same
load); 500 generated documents 2 min 23 s.

### Memory and robustness (`tests/f7c.rs`)

| Check | Result |
|---|---|
| M1: the every-path document (44 paragraphs, two sections, header/footer in three slots with PAGE/NUMPAGES, a list and a border, `evenAndOddHeaders`, `titlePg`; footnotes — one of 400 words that splits —, `lowerRoman` then `eachPage` + `beneathText`, a two-column last section; endnotes at the section end; SEQ/REF/PAGEREF/NOTEREF/STYLEREF over bookmarks and numbered headings): read → derive → layout → drop | 3,053,410,441 B allocated, 9,289,936 B peak, **0 B left** (12 pages; the balancing probes of the two-column section are full passes, each laying the notes and headers again, as the TS) |
| M1: the same ending in `Limit(Pages)` at 1, 2 and 3 pages (inside a header, a note or the endnote flow) | **0 B left** each |
| M2 soak: 1,000 × (open → read → derive → layout → drop) of a fixture document and 50 × the every-path document, sampled every 50 | flat, **0 B** variation |
| M3 (release, per call, the 16 fixture documents + every-path) | 3.6 MB–2.3 GB allocated (the 2.3 GB: a two-column fixture with long notes, every balancing probe a pass), 0.40–9.3 MB peak, 99 KB–2.4 MB retained; **50.8–65.9 B per laid-out character retained** (as F7a: over the §6 target of 48, stage 5) |
| M5 | no cross-call cache: `noteCache` (note id × width), `sepCache`, `listTexts` and the scratch page are pass state, dropped with the pass (bounded by the notes, the blocks and one page) |
| no panic: 200 hostile documents (release; 30 in debug) — note ids `-1`, `0`, `NaN`, `1e9`, `abc`, `2.5`, `-0`, missing and duplicated notes, notes referencing themselves with fields, notes of 400 words, headers/footers in random slots (empty, eight times taller than a page, with note references, hidden marks, `sz` 1e9 borders), simple and complex fields nested 0–4 deep with every F7c code (`SEQ \r 1e400`, `\s 9`, `\* constructor`, REF/PAGEREF/NOTEREF to missing bookmarks, unterminated `\d "`, STYLEREF 9, FOOTNOTEREF_MARK outside a note), hostile `footnotePr`, `pgNumType chapStyle/chapSep`, page sizes 1000/1e9/NaN, `header`/`footer` distances out of range | 160 laid out with every output index valid and no `ensep` line, 38 `Limit(Pages)`, 2 `Internal` (endnote references without any endnote paragraph: the separator was looked up before the TS's early return — fixed, F7c-5; green since); no panic |
| limits | the work budget → `Limit(Work)`; a header taller than every page ends in `Limit(Pages)` or lays out; `w15:footnoteColumns` past 4,096 → `Limit(Capacity)` (F7c-13); static-flow recursion over `max_depth` → `Limit(Depth)` |
| unit | `date_legacy::v8_forms` (ES5 extensions, legacy forms, zone names, AM/PM, comments, the invalid ones) |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`, `min`, `round`, `ceil`, `trunc`, `rem`);
   `String(n)` through `to_js_string` (the note cache key, `${kind[0]}${id}` keys); `parseInt`
   through `parse_int_utf16` (SEQ `\r`/`\s`); `===` between numbers through `js_eq`; `slice` with
   `ToIntegerOrInfinity`; `Array.from({ length })` with `ToLength`. V8's date parser keeps its
   C++ `int` arithmetic (nine significant digits per numeral, `Smi` year bound) in `i64`, the
   date arithmetic in `f64` (`MakeDay`/`MakeTime`/`MakeDate`, `TimeClip`). **No
   arithmetic-semantics change.**
2. **Regex** (hand matchers, none dynamic; listed in `field_refs.rs`): `/\\s\s*(\d)/i`,
   `/\\r\s*(\d+)/i`, `/\\c\b/i`, `/\\h\b/i`, `/\\\*\s*(\w+)/`, `/\\p\b/i`, `/\\[nrw]\b/i`,
   `/\\d\s+"([^"]*)"/i`, `/\\w\b/i`, `/\\[sn]\b/i`, `/\\s\b/i`, `/\.+$/`, `/[\t\n\f\u000B]/g`,
   `/^pt/i`, `/^\s*STYLEREF\s+"?([1-9])"?/i`, `/^b(\d+)/` and `` l.ref === `b${m[1]}` `` (a body
   line ref: `LineStory::Body`, path length 1), `/^(hdr|ftr):\w+$/` (a header/footer flow:
   `LineStory::HeaderFooter`, empty path), `/\/r\d+\/c\d+$/` (the ref ends in a cell: the
   segment tags), `ref.split('/')` of `paraByRef` (the `RefPath` segments). Non-`u` semantics:
   `\s` = the JS space set, `\d` ASCII, `\w`/`\b` ASCII word characters, `/i` ASCII folding.
3. **NaN/±∞ and absent keys**: `instr ?? ''`, `instr ?? name` and the `block === undefined`
   tests as `Option`s (the footnote env calls `fieldValue(cur)(name)` with neither); `n?.numId ?`
   truthy; `n.ilvl ?? 0` nullish; `markStyle.sizePt || 10`; `?? 0` of `cols.dx[…]`; a band of 0
   left out of `ParaSplitInfo` (`...(bandB ? { band } : {})`); NaN comparisons fall as in JS.
4. **Orders**: notes per page in reservation order, `packNotes` in that order; the endnote flow
   in `endnoteOrder`, each note's blocks in story order; `marksGuess`/`noteRefGuess`/`paras` in
   `IndexMap` insertion order; `bookmarkPages` in `bookmarks` (insertion) order with
   `firstPage`/`paraLines` filled in page and line order.
5. **Recursion**: `layoutStatic` (cell → static flow → table → cell, textbox contents) bounded by
   `max_depth` (`cx.depth`, as F6) → `Limit(Depth)` (TS: `RangeError`); `listNumberOf`'s table
   visit by `max_depth`; `newPage` → `newPageAt` → `newPage` of a page of notes only
   (`notesFull`) ends because the carried note shrinks or the pages run out (`max_pages`).
6. **Module state**: none new (the TS keeps everything of F7c on the engine).
7. **Throws** → `Err(Internal)`: a header/footer story without its part, a story table that is
   not in its model story, the endnote flow without its separator paragraph, a scratch page that
   is not there, `notePartial` with no id.
8. **Aliasing / identity (LA3)**: `scratchPage(ps)` → `Engine::scratch` addressed by the
   `SCRATCH` `PageIx` (`pages.indexOf(scratch)` = −1 for PAGE's chapter); the static flow's items
   array (`boxPage: items`) → `ContainerId::Static(CellContentId)` from a per-pass counter;
   `lastPara` (a `ParaSplitInfo` object mutated after it went into `paras`) → its key in `paras`;
   `ps.notes.some(n => n.id === id)` → `js_eq`, `ids.includes(id)` → `NumKey` (SameValueZero);
   `this.full = { ...saved, blocks }` of the endnote flow → `Engine::en_blocks` (block, ref,
   owner) read by `flow_block`/`flow_ref`/`story_len`; `gEn = { ...g, … }` → a geometry appended
   to `geoms` (pages keep its index; `geoms[g.index + 1]` bounded by `sect_geoms`); the separator
   paragraph object → the derived `LDoc::en_sep` copy; `ps === this.cur` → `PageIx` equality.
   **`json_key` sites: none.**
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F7c-1 | `layout_static(cx, &[SBlock], x, width, &ParaEnv, &LineRef, Option<PageIx>, y_origin, Option<&CellNeighbors>)`; `SBlock::{Para(LParaId), Table(&Table)}`; `story_blocks(cx, story)` (needs `LStory::sref`); `pure_static(cx, f)` = `ctx.pure` on the engine (the flow restored also on `Err`, 1 work unit and `pure_clones`) | a stable entry for F7b's cells and textboxes; tables are read through the view (`LBlock::Table` holds an index) |
| F7c-2 | `scratchPage(ps)` is `Engine::scratch` behind `SCRATCH` = `PageIx(u32::MAX)`; `ps()`/`ps_mut()` resolve it; dropped after each probe | the copy must not enter `pages` (`pages.length` is read by the float code) |
| F7c-3 | `FieldSource::HeaderFooter` = `fieldValue(ps)` without a block, its page on `Engine::field_page` (set around the header/footer, restored after); `FieldSource::Footnote(id)`: `FOOTNOTEREF_MARK` = the note's mark read at the call (the TS captures it when `noteContent` starts: the same value, as laying out the note never changes its own mark), anything else `fieldValue(this.cur)(name)` with `instr`/`at` undefined; `FieldSource::Endnote` is not produced (the endnote flow uses `Page` + `cur_endnote`, as the TS) | the F4 `FieldSource` carries no page |
| F7c-4 | `SEQ \* name` with an `Object.prototype` name (`constructor`, …) formats `decimal` | the TS passes a function to `formatNumber` |
| F7c-5 | the endnote flow: `Engine::en_blocks` (`EnBlock { block, at, owner }`), the separator paragraph a derived copy (`ParaSrc::Copy` of the first model paragraph with both `ppr`/`props` overrides and no runs, `LDoc::en_sep`, built when the document has endnotes), its line `LineStory::EndnoteSep` (`ensep`, taken off the page right after); the separator is looked up only when some endnote has blocks (the TS returns before) | `Cx` keeps one `&LDoc`; nothing of the base is read |
| F7c-6 | `new Date(s)`: the F7a ISO subset first, then V8's whole parser (ES5 extensions and legacy forms); `kMaxTimeBeforeUTCInMs` = the time range + 10 days (as V8's `date.h`, from memory of the source: not measured). *Orchestrator check (2026-11-05):* the constant is not observable as long as it is ≥ 1 day — every zone offset is below a day, so a local time past the range + 1 day stays past it after the offset and the final `TimeClip` gives NaN either way; Node 24.15 in UTC gives `Sep 13 275760 00:00:00` = 8.64e15 (the top of the range); a Rust unit case at that edge is still to add | a date V8 reads must not stop the layout |
| F7c-7 | `placeParagraph`'s `markGone(para0)` and `headingStyleLevel(para0)` read the paragraph object's own fields (`fields_of`), not its model root's | the same for model paragraphs; right for copies (the separator, F7b's frame copies) |
| F7c-8 | `PageNote.id`, `NoteCut.id`, `NoteCarry.id`, `cur_endnote` are `NumKey` (F0: `i32`) | note ids are JS numbers |
| F7c-9 | bookmark names, SEQ identifiers and `marksGuess` keys from UTF-16 by lossy conversion (a lone surrogate becomes U+FFFD) | `JsMap<Box<str>, …>` keys; only hostile names collide |
| F7c-10 | an endnote flow block keeps its story (`EnBlock::story`): its tables are read from the note's story (`placeTable` through F7b's `place_table`, `listNumberOf`, `paraByRef`) | the flow's blocks come from several stories |
| F7c-11 | work budget: 1 unit per static flow, per static block and line, per pure probe, per endnote block, per `listNumberOf` block/cell block | K16 |
| F7c-12 | `PrepFlow::field` reports `instr`/`at` as given; `fieldValue`'s `block` is `Option<i64>` | the TS `undefined` |
| F7c-13 | `w15:footnoteColumns` above 4,096 → `Err(Limit(Capacity))` (as F7a-6 for page columns) | the TS makes one array entry per column |

**Limits** (LQ14): `max_depth` (static flows, the list visit), `max_pages` (headers taller than the
page, notes that fill pages), the work budget (F7c-11), `MAX_NOTE_COLUMNS` (F7c-13).

### Shared files edited (additive)

`ldoc.rs` (`LStory::sref`, `LDoc::en_sep`), `derive.rs` (`sref`, the separator), `output.rs`
(`LineStory::EndnoteSep`), `engine/mod.rs` (the modules; `Engine::{scratch, static_seq,
field_page, en_blocks, sect_geoms}`; `SCRATCH` in `ps()`/`ps_mut()`; `story_len` over the endnote
flow; `run` calls `place_endnotes`/`paint_notes`; `note_mark_fix` over `NumKey` ids; PAGEREF's
`bookmark_pages` in `layout_document`; `cur_endnote: Option<NumKey>`), `engine/page.rs`
(`PageNote.id: NumKey`, the continuation separator in `new_page_at`/`break_column`, `paint_notes`
and `place_note_carry` in `make_page`, `layout_header_footer` moved to `hf.rs`,
`record_balance` pub(crate)), `engine/flow.rs` (visibility of the helpers the static flow shares,
`with_ldoc` re-exported, `block_at` over the endnote flow, `fields_of` for the mark and heading,
`flow_ref`, the note sites of the line loop, the note stop of `check_scope` removed, `PrepFlow::
field` sources, `geoms[g.index + 1]` bounded), `engine/fields.rs` (signature, dispatch, V8 dates),
`tests/layout_parity.rs` (the F7c fixture test), `tools/capture-layout.mjs` (`--gen f7c`),
`.github/workflows/rust-core.yml` (the F7c capture + strict replay step), this record. Owned:
`engine/{static_layout,hf,notes,field_refs,date_legacy}.rs`, `tests/f7c.rs`,
`tests/captures/f7c/`, `tools/lib/gen-f7c.mjs`.

### Not done (handed on)

- **F7b** (done by F7b in this session, recorded in its section): the F7b sites of
  `layout_static` (floats, frames, wraps, tables), the extra wrap segments' `reserveNotes`, the
  row notes (`row_note_ids`/`row_note_extra`/`rowNoteCut`). `layout_static`'s signature did not
  change after F7b started calling it.
- **F8**: `collect_fields` over the header/footer and note flows (their `PrepFlow::on_field`),
  `page_of`/navigation over `LineStory::Note`/`HeaderFooter` lines.
- The CI job runs the F7c capture with 300 documents; the oracle gate run regenerates the TS
  capture (5–6 min with dumps); rerun it when F7b's chains land (the 14 pending).

## Stage 3 — F7b (engine: tables and floats)

Opus, 2026-11-05, hybrid mode, one session (≈ 2 h 45 min), in parallel with F7c (notes,
headers/footers, fields, `layoutStatic`) and the uo-docx session work. Built from the F6/F7a
hand-offs against the frozen TS (d0f45bc5). **2 port parity rounds** (below).

### Ported (TS → Rust, TS operation order)

| TS (engine.ts unless noted) | Rust |
|---|---|
| `placeTable` 2595 (hidden-row tables, the floating branch with text/page anchors, late and placed tables, the anchor that follows a split table; the flow branch: header rows kept with the first data row (mode 15), keepNext row chains, the 500-step row loop with `belowFloats`/`floatFloor`/`rowNoteExtra`/`breakW`/`atPageTop`/`notePartial`/`row.split`/the closing border/repeated headers, `mergeTextFirst` + `dropMoved`, vertical merges flowing across pages with the grown part, the merge-text and merge-fill insertions) | `engine/tables.rs` `Engine::place_table`, `place_floating_table_block`, `after_float_table` |
| `floatFloor` 2862, `belowFloats` 2874, `floatMinY` 2886, `pinFloatsOverTable` 2900 | `float_floor`, `below_floats` (+ `below_floats_in`), `float_min_y`, `pin_floats_over_table` |
| `placeFloatingTable` 2947, `emitFloatRows` 2970, `flowFloatMerges` 3062, `noBreakWrapped` 3092, `placeFloatCarry` 3097, `rowCanStart` 3112, `emitRow` 3119 | `place_floating_table`, `emit_float_rows`, `flow_float_merges`, `no_break_wrapped`, `place_float_carry`, `row_can_start`, `emit_row`; the late tables of `newPageAt` 896 → `place_late_tables` |
| `layoutCell` 1516, `layoutCellIn` 1528, `cellBlocks`, `firstParaNeighbor`, `lastParaNeighbor`, `rowEndNeighbor`, `rowEndPrev`, `dropsAfterBeforeTable`, `assignCellNeighbors` 1560–1612 | `layout_cell_ps`, `layout_cell_in`, `cell_blocks`, … `assign_cell_neighbors`; `impl CellLayouter for Engine` |
| the table branches of `layoutStatic` 1405–1488 (hidden, floating in a cell, floating in a header/footer, nested in the flow → `nest` rows with `ObjId`s, F6-9; the `hfFrameTbl` splice) | `static_table` (`StaticFloats`, `StaticAcc`), wired into F7c's `layout_static` |
| the table preview of `nextBlockLead` 2547 | `table_lead` (`LineStory::Preview`) |
| `wrapFrame` 3139, `pickSeg` 3222, `moreSegs` 3234, `polyBandIntervals` 89 | `engine/floats.rs` `wrap_frame`/`wrap_frame_in`, `pick_seg`, `more_segs`, `poly_band_intervals` |
| `placeFloats` 3348, `placeFloat` 3359 (hidden, textbox content with frame auto width / noWrap / autoFit / `tbFinal`, simplePos, pinned, the vertical clamps, the cell branch, the frame flow in a header/footer, the wrap box with polygon and the rotated swap, `lateFound`), `unplaceFloat` 3548, `placeFloatsEarly` 3563, `paintInlineDrawings` 3583, `paintObject` 3602 (metafile, picture, shape, textbox with WordArt/rotation/vertical text/clipping, groups) | `place_floats`, `place_float`, `unplace_float`, `place_floats_early`, `paint_inline_drawings`, `paint_object`, `rotated_ink_y`; the cell floats of `layoutStatic` 1340 → `static_cell_floats` |
| module helpers 3748–4183 (`sideOf`, `floatRot`, `swapsBox`, `horizRef`, `vertRef`, `tbInsets`, `vertKind`, `tbFrame`, `verticalMatrix`, `verticalBands`, `verticalHeight`, `verticalContent`, `textRectInsets`, `alignPos`, `tbContentPlacement`, `rotation`, `WRAPPING`, `ABSOLUTE_V`, `MIN_GAP`, `floatAnchor`, `followsText`, `narrower`, `markLineH`) | same names in `floats.rs` |
| the float branches of `placeParagraph` 1903–2213 (hff14wrap, `paraRef`/`selfGap`/`ftGap`, `anchorAt`, early floats and the lift, `frameHere`, the tall-line, lower-text, gapind and tabgap re-breaks, ftafter, the floats that follow the first line, `moreSegs` with notes, `brclear`, the anchored floats after the lines), `FRAME_SKIP` in `placeParagraph`/`layoutStatic`, the floats/wrap/frames of `layoutStatic` | `flow.rs` (`frame_here`, `para_wrap`, `below_wrapping`, `anchor_here`), F7c's `static_layout.rs` |
| paint.ts `paintInlineObject` ends (F5-9) and the picture-bullet glyph (F4) | `engine/objects.rs` (`ImageTable`, `picture_id`, `blip_id`, `pic_bullet_id`, `push_metafile`, `push_image_item`, `push_shape_item`); `paint.rs` calls them |
| frames.ts `frameOf`, `liftFrames`, `frameRun`; chain.ts `indexChains` | `derive.rs` `frame_pr_of`, `lift_frames`, `frame_run`, `index_chains` |

Not ported: chain.ts `sliceParagraphFrom`/`remapLines`/`topBlockOf`/`topRowOf` and engine.ts
`chainSlice` 3278 — a textbox of a linked chain stops with `Unported(F7b, "linked textbox chains")`
(F7b-6); the tables of the endnote flow (`placeEndnotes`, F7c's call site) stop with `Unported`.

### Parity (P1 through the real engine; TS capture regenerated in the same run, LQ21)

| Corpus | Documents | Equal | Pending (reason) | Mismatches |
|---|---|---|---|---|
| Word oracle, all documents, installed fonts (F7a + F7b + F7c together) | 2,742 | **2,728** | 14: linked textbox chains (`txbxlink`) | **0** |
| `--gen f7b` (tools/lib/gen-f7b.mjs), seed 1 | 100 | **100** | 0 | 0 |
| `--gen f7b`, seed 2 | 200 | **200** | 0 | 0 |
| committed fixture `tests/captures/f7b` (`--gen f7b` seed 7, open fonts; runs in `cargo test`) | 12 | **12** | 0 | 0 |
| the F7a and F7c fixtures (regression) | 16 + 16 | 32 | 0 | 0 |

Of the F7b pending reasons at the start of the session (tables 716, floating objects 331, inline
images 102 + 1 shape, frames 53, table after keepNext 16, inline groups/textboxes 3) every document
is equal except the 14 chain documents. P1 now also compares `PlacedObject.run` (the replay's
`run_names` = canon.mjs `runRefs`; it printed nothing before, F7b-9). `UO_LAYOUT_ONLY=<prefix,…>`
runs a subset; `UO_LAYOUT_VERBOSE=1` prints each pending document; the `UO_F7A_DIFF` decoder now
loads the dumped image payloads.

**Round 1** (2,437 equal, 13 mismatches): `pinFloatsOverTable` left out the table height in the
anchor's first position (`top + H + collapse(0, before)`): 2 `toptbl` documents; the 11 `txbxlink`
mismatches were boxes of linked chains laid out as single boxes (`indexChains` was not built yet) —
`index_chains` now marks them and they are pending. **Round 2**: 2,728 equal, 0 mismatches. The
generated corpora were equal at their first replay. Generator rounds (no port bug): the first
`gen-f7b` documents ran to 3,500 pages — the TS's own 500-step guard of `placeTable` repeats a row
that cannot start under its repeated headers, or a row taller than a page that cannot split; the
generator keeps header rows, `cantSplit` rows and cell floats short (Rust matched the TS there too,
but the dumps were 2–7 GB). Timing: oracle TS capture with dumps ≈ 2 min; Rust replay of the oracle
≈ 3 min (release).

### Memory and robustness (`tests/f7b.rs`)

| Check | Result |
|---|---|
| M1: the every-path document (a 15-row table with a repeated header, vertical merges with fills across a page, a nested table, exact and cantSplit rows, a float in a cell, a keepNext heading before the table; an inline picture; square/largest/topAndBottom/behind floats, a picture, an auto-fit textbox, a late page-relative float, a frame, a floating table split over two pages): read → derive → layout → drop | 105,010,591 B allocated, 2,437,699 B peak, **0 B left** |
| M1: the same with `max_pages: 2` (the call ends in `Limit(Pages)` mid-pass) | **0 B left** |
| M2 soak: 100 × (open → read → derive → layout → drop) of the every-path document, sampled every 10 | flat, **0 B** variation (11,240,554 B live throughout) |
| M3 (release, per call, the 12 fixture documents) | 3.6–194 MB allocated, 0.23–9.0 MB peak, 88 KB–2.9 MB retained; **58–93 B per laid-out character retained** (over the §6 target as F7a's: stage 5) |
| M5 | `ImageTable` holds one id per model drawing object and slot (bounded by the document's drawings; per call, dropped with it); a table of 200 inline pictures: ≥ 200 and ≤ 1,000 images over the passes (one per drawing and pass at most) |
| no panic: 200 hostile documents (release; 40 in debug) — floats with offsets/sizes 0…1e7 and NaN text, every wrap incl. tight/through polygons, every relativeFrom incl. unknown ones, behind; tables with hostile `trHeight`/`gridSpan` (0, −1)/`vMerge`/`textDirection`/`vAlign`/`tblHeader`/`cantSplit`, floating tables (NaN `tblpX`, page/center/bottom); frames with hostile `w`/`h`/`x`/`y`/anchors; breaks with `w:clear`; linked textbox chains; mode 14 with `doNotBreakWrappedTables` | 123 laid out with every output index valid, 77 `Unported(F7b, "linked textbox chains")`; no panic |
| limits | `max_pages` → `Limit(Pages)`; a work floor of 1,000 units → `Limit(Work)`; 12 nested tables over `max_depth: 8` → `Limit(Depth)` (the default 192 lays them out) |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: every `Math.*` through uo-jsnum (`max`, `min`, `max_n`, `min_n` for the spreads,
   `round` in `markLineH`, `rem` for `% 2`/`% 90`/`% 180`/`% 360`, `abs`, `cos`, `sin`); `===`
   between numbers through `js_eq`; model numbers with `??` through `JsNumber::as_f64` (`null`
   nullish); `!!x`/truthiness of numbers through `conv::truthy`. **No arithmetic-semantics
   change.** The splice indices of the merge insertions are integers and keep the JS meaning of a
   negative index (from the end).
2. **Regex** (hand matchers): `/^(hdr|ftr|txbx)/.test(l.ref)` → the line's `LineStory`
   (`HeaderFooter`/`Textbox`; `itb`/`grp` refs do not match, as in the TS); `/^b\d+$/.test(l.ref)` →
   `Body` with a one-step path; `/\s/.test(g.ch)` → `uo_text::space::is_space` over the glyph's
   units; `/\/r\d+\/c\d+$/` and `/^(hdr|ftr):\w+$/` are F7c's helpers.
3. **NaN/±∞ and absent keys**: `??` per site (`anchor.posV.relativeFrom ?? 'paragraph'` is a
   required field of the model: the reader always writes it); `fp.tblpXSpec`/`tblpYSpec`/`xAlign`
   truthy = non-empty; a hidden float's object is `{ x: 0, … }` and goes nowhere; `Math.min(...[])`
   of the frame text is `∞` (`min_n`).
4. **Orders**: insertions sorted stably by index descending (`sort_by_key(Reverse)`, the integers
   of V8's comparator); `polyBandIntervals` sorts through uo-docx `js_sort`; `prePlaced`,
   `lateFound`, `floatFlow`, `seen`, `moved` are `IndexSet`/`IndexMap` (insertion order);
   `cell_collect` merges cell `paras` with `Map.set` semantics (an existing key keeps its place).
5. **Recursion**: cell → `layoutStatic` → table → cell and textbox → `layoutStatic` are bounded by
   `max_depth` (F7c's `layout_static` and F6's `layout_table` count `cx.depth`); `paintObject` over
   groups recurses on the model's own group nesting (bounded by the reader).
6. **Module state**: `LATE_ITEMS` → `Engine::late_marks` + `ItemSide::meta` (the cell floats set
   them); `FRAME_INFO`/`FRAME_SKIP`/`FRAME_CARRY`/`INNER` → `LTextbox.frame`, `ParaFlags`,
   `LPara::frame_carry`; `runCache`/`anchorCache` are per derive (LQ18).
7. **Throws** → `Err(Internal)`: a merge fill that is not a rect, a cell paragraph or textbox not in
   the derived document, a picture bullet without its numbering picture, a frame anchor not derived.
8. **Aliasing / identity (LA3)**: floats by their run's `RunKey` (`prePlaced`, `pinnedFloats`,
   `lateFound`); a placed `ZObj` by its `seq` (unique per pass); `unplaceFloat` removes the object
   and the wrap box by value (F7b-3); table rows of a floating table by `RowTag` (`t.rows[k]`, a
   split part with its `floatFlow.idx` row, or another copy); the items array of an emitted float
   row by `FloatOut` (page, layer, `seq` of the `ZObj` it becomes); `floatFlow` by `TableLayoutId`;
   merge texts `seen` by (row, entry); `lastRowEmit` keeps only the row's closing border; images
   by `ImageTable` (F7b-1); nested rows by `ObjId`s unique in the content (F6-9).
   **`json_key` sites: none.**
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F7b-1 | `Cx::images: &mut ImageTable` (call scope): one `ImageId` per model `InlineImage` (`DrawingId`) and per shape/textbox blip (`DrawingId`, slot), a new one for every object the layout builds (picture bullets per paint, group children copied under the group's rotation/flip, EMF bitmaps); the origins moved from `Call` into it; the metafile warnings of a pass go into its `DocContext` when it ends | RQ3 object identity without pointers; `paint_inline_object` only has a `Cx` |
| F7b-2 | `Engine::cell_ps: Option<Option<PageIx>>` and `cell_collect` carry the `ps` and the `paras` sink of the TS `layoutCell` closures into `impl CellLayouter` (saved/restored around each table) | F6-1's trait has no closure state |
| F7b-3 | `unplaceFloat` removes the placed object and the wrap box by value (bit equality); the `ZObj` by `seq` | removing any of several equal elements leaves the same array |
| F7b-4 | A cell's `penv.field = fieldValue(ps)` is `FieldSource::HeaderFooter` with `field_page = ps` (F7c's block-less page source), saved/restored | the TS closure has no block |
| F7b-5 | `layout_cell_in`/`tb_final_layout` reborrow `Cx` with a shorter `'a` (`with_ldoc!`) | the cell and the box content outlive the call but are not tied to the `LDoc` borrow |
| F7b-6 | Linked textbox chains (`chainSlice`) stop with `Unported(F7b)`: the continuation needs runtime paragraph slices (`RunSrc::Slice` + `CONTINUATION`) and row-sliced table copies that the shared `&LDoc` cannot take during a pass | 14 oracle documents; see hand-offs |
| F7b-7 | Frames in a header/footer go into the flow (`hf_frame_after`/`flow_items`) and are not removed by `unplaceFloat` (`inFlow`): only body floats are ever unplaced | the TS removes `inFlow` items only for early floats, which never are frames in a header/footer |
| F7b-8 | `liftFrames` runs once per call in `derive` over every story (body with section stops; headers/footers, notes and textbox contents without); the TS lifts inside `layoutStatic` only outside a cell, so a frame inside a textbox **inside a table cell** is lifted here and not in the TS | no oracle document has one; the TS lift is cached per paragraph object anyway |
| F7b-9 | Harness: `layout_parity` writes `PlacedObject.run` (canon.mjs `runRefs` over the derived document: `b…#i`, `hf<n>`, `fn<id>`, `<textbox run>/tx`, `/g<j>#0`), `UO_LAYOUT_ONLY`, `UO_LAYOUT_VERBOSE`, the dumped media in the diff decoder, the F7b fixture test; `--gen f7b` in `capture-layout.mjs` and the CI step (strict) | objects were compared without their run until now |
| F7b-10 | Synthetic frame textboxes take `DrawingId(u32::MAX − k)` (`tb_of_drawing`) and an empty `content`; their content is the `LStory` of the `INNER` copies | the model ids are dense from 0; no model object exists for a frame |
| F7b-11 | Work budget: 1 unit per table row emitted (flow and floating), per `placeFloat`, per `paintObject`, per static-flow line (F7c) | K16 |

**Limits** (LQ14): `max_depth` for cell/textbox recursion (`Limit(Depth)`), `max_pages`
(`Limit(Pages)`) — the TS 500-step row guard and the 100-page float-carry guard are kept as the TS
has them; `ImageTable` grows with the document's drawings; the metafile limits are F3's.

### Shared files edited (additive)

`engine/mod.rs` (`Cx::images`, `Call::images`, `finish` without the origins parameter, the table
block of `run`, `cell_ps`/`cell_collect`, `objects` module), `engine/flow.rs` (the float branches of
`placeParagraph`, `flush_frag` inline drawings, the table preview, the frame skip; `check_scope`
removed), `engine/page.rs` (`PageOut::content_lines`; float carry, late floats and tables in
`new_page_at`), `engine/static_layout.rs` (F7c's: cell floats, header/footer floats, wrap frames,
inline drawings, frames, tables), `derive.rs` (frames, chains), `paint.rs` (the F5-9 ends, the
picture-bullet guard), `model.rs`, `output.rs` (`LineStory::InlineTextbox`/`GroupTextbox`),
`drawing.rs` (`color_truthy` visibility), `tests/layout_parity.rs`, `tests/f4.rs`,
`tests/f4_parity.rs`, `tests/f6.rs`, `tests/f6_parity.rs` (one `images` line each),
`tools/capture-layout.mjs`, `.github/workflows/rust-core.yml`. Owned: `engine/tables.rs`,
`engine/floats.rs`, `engine/objects.rs`, `tests/f7b.rs`, `tests/captures/f7b/`,
`tools/lib/gen-f7b.mjs`.

### Not done (handed on)

- **Chains (F7b-6)**: `chainSlice` with `sliceParagraphFrom` (a runtime `RtPara` with `RunSrc::Slice`
  runs and `CONTINUATION`, prepared through the relocated scratch `LDoc` of F7a-2, or slices built
  per chain in `derive`), the row-sliced table copy (an owned `Table` with `rows[row..]`, its
  `cellNb` keys re-assigned) and `remapLines`; `Engine::chain_slices` already exists. Then the 14
  `txbxlink` documents and `txbxlink/lk-table2` of F6. *(Done in F7d.)*
- **F8**: the `PlacedObject.content` lines are `LineGeom`s in `content_lines` (editor caret in
  boxes); hit tests over `objects`/`cells`.
- The engine module docs of `mod.rs`/`flow.rs` still describe F7a's stop points. *(Updated in F7d.)*
- M3 stays over the §6 per-character target (stage 5, as F7a).

## Stage 3 — F7d (chains, endnote tables)

Opus, 2026-11-05, hybrid mode, one session (≈ 2 h 15 min), in parallel with F8 (editor-facing
API) in the same crate and with the uo-docx D2b session work. Built from the F7b/F7c hand-offs
against the frozen TS (d0f45bc5). **0 port parity rounds**: the 14 `txbxlink` oracle documents,
every generated document and the fixture were equal at their first replay.

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| engine.ts `chainOf` 3265, `chainSlice` 3278 (the head's content laid out at each box's width in `seq` order through `ctx.pure(layoutStatic(…, 'txbx'))`; the continuation's widow count; `splitContent` at the box's inner height; the first moved line; the nested-row re-cut; the paragraph cut `sliceParagraphFrom`, the row cut `{ ...pb, rows: pb.rows.slice(row) }`, or the whole blocks from `j`; `remapLines`; the per-pass memo) | `engine/chain.rs` `Engine::chain_of`, `chain_content`, `chain_slice`, `chain_layout` |
| chain.ts `sliceParagraphFrom` 57, `topBlockOf` 72, `topRowOf` 78, `remapLines` 86 | `slice_paragraph_from`, `top_block_of`, `top_row_of`, `remap_lines` |
| the three call sites: `placeFloat` 3383, `paintInlineDrawings` 3592, `paintObject`'s group child 3721 | `engine/floats.rs` (`check_chain` and its `Unported` removed) |
| `layoutStatic`'s `CONTINUATION.has(para)` 1272 and its block walk over a continuation copy | `engine/static_layout.rs` (`SBlock::Rt`) |
| `nextBlockLead`'s table branch 2550 in the endnote flow (`this.full.blocks` = the flow's) | `engine/flow.rs` (`flow_table` of the endnote block's story) |

The engine no longer returns `LayoutError::Unported` anywhere (`view_transform.rs`'s Outline
view, F8/milestone 2, is the only producer left in the crate); `error::unported`, the F0 stub
helper, is gone.

### Parity (P1 through the real engine; TS capture regenerated in the same run, LQ21)

`tests/layout_parity.rs` as F7a–c (P1 + `pages` + `p2first`). The `--gen f7d` generator
(`tools/lib/gen-f7d.mjs`): chains of 2–5 boxes — floating (page/margin/column/paragraph anchors,
none/square wrapping), inline (`wp:inline`) and inside drawing groups (`wpg:wgp`), sizes and
insets at random, in the body and in a header; the head's content: paragraphs of one to four runs
in five open families and four sizes (cut in the middle of a run and at run boundaries), a PAGE
field and tabs (never cut), first-line and hanging indents, a numbered list (the continuation
loses its marker), spacing, contextual spacing, widow control on/off, keepNext, justification,
and tables of 2–10 rows (cantSplit, row heights, header rows) cut between rows; `seq` reversed or
with gaps, links to a missing id, a linked box before its head, overflowing last boxes. Endnotes
of one to four blocks: keepNext paragraphs right before tables (`nextBlockLead` in the endnote
flow), tables of up to 44 rows crossing pages, at the section end or the document end; two-column
sections now and then.

| Corpus | Documents | Equal | Pending | Mismatches |
|---|---|---|---|---|
| Word oracle, all documents, installed fonts | 2,742 | **2,742** | **0** | **0** |
| `txbxlink/*` of the oracle (the 14 F7b-6 documents) | 14 | **14** | 0 | 0 |
| `--gen f7d`, seed 1 | 100 | **100** | 0 | 0 |
| `--gen f7d`, seed 2 | 500 | **500** | 0 | 0 |
| `--gen f7d`, seed 3 | 300 | **300** | 0 | 0 |
| committed fixture `tests/captures/f7d` (`--gen f7d` seed 7, open fonts; runs in `cargo test`) | 12 | **12** | 0 | 0 |
| the F7a, F7b and F7c fixtures (regression) | 16 + 12 + 16 | 44 | 0 | 0 |
| F6 facets (`p3.table`/`p3.rowsplit`/`p3.splitcontent`) of `txbxlink/lk-table` and `lk-table2` (the row-sliced copy) | 2 | **6 calls equal** (4 tables, 2 content splits) | **0** (was 1) | 0 |

Coverage of the generated corpus (seed 1, counted once by a temporary probe, then removed): every
document with a chain had non-head boxes with content; 120 distinct continuation states with a
paragraph cut (`cont > 0`) and 21 with a table cut between rows (`rows > 0`).
The F6 replay needed one facet change: a row-sliced copy is not a model table, so the facet now
names it `<table ref>+r<k>` when its rows are the model table's own row objects from `k` on and
every other key is the table's (facets-f6.mjs `positions` keeps a `Row → { table, ri }` map), and
`f6_parity` rebuilds the copy from the model table (F7d-11).

Timing: TS oracle capture with dumps 1 min 38 s; Rust replay of the oracle 1 min 41 s (release);
generated documents: capture 1 min 49 s (100), 17 min (500) and 9 min (300) on a machine shared with the F8 and D2b agents, replay 11 s, 78 s and 57 s.

### Memory and robustness (`tests/f7d.rs`)

| Check | Result |
|---|---|
| M1: the every-path document (a body chain of four floating boxes over a numbered paragraph, a paragraph with a PAGE field and a first-line indent, a 12-row table and a paragraph without widow control — cut mid-run, between rows and overflowing the last box; a chain of an inline head and a grouped linked box; a header chain; a box linked to a missing id; endnotes with keepNext paragraphs before a 60-row table crossing a page and a 3-row table): read → derive → layout → drop | 67,987,712 B allocated, 2,614,594 B peak, **0 B left** |
| M1: the same ending in `Limit(Pages)` (1 page, inside the endnote flow) and in `Limit(Work)` (a work floor of 3,000 units, inside a chain) | **0 B left** each |
| M2 soak: 100 × (open → read → derive → layout → drop) of the every-path document, sampled every 10 | flat, **0 B** variation (11,245,792 B live throughout) |
| M3 (release, per call, the 12 fixture documents) | 7.5–591 MB allocated (the two of ≈ 0.5 GB: two-column sections, every balancing probe a full pass that lays the chains out again, as the TS), 0.54–2.9 MB peak, 157–624 KB retained; **52.5–99.6 B per laid-out character retained** (over the §6 target as F7a–c: stage 5) |
| M5 | no new cross-call cache: `chain_slices` is pass state (one entry per chain and box, dropped with the pass); the slices and the row-sliced table copies live in `chain_layout`'s frame |
| no panic: 200 hostile documents (release; 30 in debug) — boxes with link ids `1`, `0`, `-1`, `NaN`, `abc`, empty, `1e9`, `2.5`, `99999999999` and seqs `0`, `-1`, `NaN`, empty, `1e9`, `2.5`, duplicated; several heads with the same id; boxes of 0 × 0 up to 40,000 twips; heads whose content holds an inline box of their own chain or of another (re-entrant `chainSlice`, mutual chains), tables in the content; floating, inline and grouped members; header chains; endnotes with tables of up to 40 rows with `cantSplit`, `tblHeader`, `trHeight` `1e9`/`NaN` exact and keepNext paragraphs before them | 200 laid out with every output index valid; no error, no panic |
| limits | a chain of 2,000 linked boxes (300 in debug) over 40 paragraphs lays out; the same with a work floor of 2,000 units → `Limit(Work)`; chains nested 12 deep in each other's content with `max_depth: 8` → `Limit(Depth)` (the default lays them out) |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: `Math.max` through uo-jsnum (`max(0, topBlockOf)`, `max(1, fr.iw)`); the block and
   row indices of the refs are the TS numbers (`topBlockOf`/`topRowOf` −1 when absent) read as
   indices through `index_of`; `rows + row`, `cont + off` and the remapped `start`/`end`/`o` in
   `u32` with saturating adds (JS numbers: the values are bounded by a text length or a row
   count, F7d-6); the slice's offsets in `u64` while walking the runs. **No arithmetic-semantics
   change.**
2. **Regex** (hand matchers over `LineRef` segments): `/^[^/]+\/b(\d+)/` → the first segment;
   `/^[^/]+\/b\d+\/r(\d+)\//` → the second when a third follows; `/^[^/]+\/b\d+$/` → a one-segment
   path; `remapLines`' `/^([^/]+)\/b(\d+)(.*)$/` and `/^\/r(\d+)/` → the first segment and the row
   segment after it (segments cycle b, r, c).
3. **NaN/±∞ and absent keys**: the `moved` filter `l.y >= cut - 0.01` drops NaN as the TS;
   `map[j] ?? j`; `r.field || r.fieldResult || r.image || …` = a non-empty `field` or any of the
   others present (an empty `fieldResult` array is truthy); `slices.get(tb) ?? empty`.
4. **Orders**: boxes in `members` order (the `seq` sort of F7b's `index_chains`); `chain_slices`
   an `IndexMap` (`Map` insertion order; `set` of an existing key keeps its place); the paras map
   of a continuation updated in place.
5. **Recursion**: `chainSlice` → `layoutStatic` → `paintInlineDrawings`/`placeFloat` →
   `chainSlice` of another chain is bounded by `max_depth` (`layout_static` counts it); the same
   chain re-entered returns the memo (F7d-4).
6. **Module state**: `CONTINUATION` (a `WeakSet`) → `RtPara::continuation` on the copy object;
   nothing else.
7. **Throws** → `Err(Internal)`: `moved.reduce` of an empty array after the nested-row re-cut
   (F7d-7), a chain without members, a cut after the first kept run (unreachable: every run before
   a cut is skipped), a continuation block where the cell flow expects model blocks.
8. **Aliasing / identity (LA3)**: `TextboxChain` → `ChainId`, `DrawingTextbox` (members, `out`
   keys) → `TbKey`; the slice paragraph object → an `RtPara` (no `ParaId`, as any TS copy); its cut
   run → a `RunSrc::Slice` of the relocated base in the scratch `LDoc`, its glyphs mapped back to
   the base's `RunKey` (F7d-2); the row-sliced copy → an owned `Table` with the model's `TableId`
   (F7d-3); remapped lines → copies without `ObjId` (F7d-6). **`json_key` sites: none.**
9. **Choices outside the source semantics**:

| # | Decision | Why |
|---|---|---|
| F7d-1 | `ldoc::Chain` carries the model box of each member (`Chain<'a>::boxes`, filled by `index_chains`) | `chainSlice` reads each box's size, insets and colours; `DocView::find_textbox` is O(document) per call |
| F7d-2 | A continuation is `RtPara { first_from, continuation }`: `runs[0]` is cut from `first_from` (0 = whole). `sliceParagraphFrom` cuts at most one run and always the first one kept, so one offset covers slices of slices (offsets add up) and the copies built from them (`join_deleted` takes the carried runs' `first_from` and counts the cut text in `REV_JOINS`; `join_hidden` keeps the joined paragraph's). `prepare_rt` makes the cut run a `RunSrc::Slice` of the relocated base; its glyphs map back to the base's key | `Cx` keeps one shared `&LDoc` (F7a-2); the TS copy is a new run object, but a cut run is a text run (fields and objects are never cut), so nothing keyed by its identity differs |
| F7d-3 | The row cut is an owned `Table` (`rows_copy`: the rows from `k` cloned, every other field copied, the model's `TableId`); slices of slices add up (`RBlock::Rows(t, from)`), one copy per box layout. The copies and the continuation paragraphs live in `chain_layout`'s frame and reach `layout_static` through a `Cx` reborrowed with that frame's lifetime (as F7b-5). `cellNb` keys (table, row, cell) name the copy's rows; they are assigned right before each table layout, as the TS assigns them on the (shared) cell objects | no model object exists for the copy; the cells' paragraphs keep their ids, so the derived paragraphs are found |
| F7d-4 | `chain_slices[chain]` is inserted empty before the chain is laid out and filled box by box: a `chainSlice` of the same chain met while it is being laid out (a box of the chain inside its own content) gets the boxes done so far or empty; the env of the first caller of the pass (`field` page, `hf`) holds for every box | the TS `Map.set(chain, out)` comes before the loop; the memo is per pass |
| F7d-5 | The flow ref `'txbx'` of every box is `LineStory::Textbox(head)` | the remapped refs name the head's content blocks; the TS string is the same for every box |
| F7d-6 | `remapLines`' changed lines are copies (`ObjId` dropped); `start`/`end`/`o` + `cont` are saturating `u32` adds | `{ ...l }`; the sums are text offsets |
| F7d-7 | `moved.reduce(…)` of an empty array (only reachable after the nested-row re-cut) → `Err(Internal("chain: no moved line"))` | the TS throws `TypeError` |
| F7d-8 | Work budget: 1 unit per box of a chain (plus `layout_static`'s and `split_content`'s own) | K16; a very long chain stops at the budget |
| F7d-9 | `nextBlockLead`'s table in the endnote flow is read from the note's story (`flow_table`) | the TS reads `this.full.blocks`, which the endnote flow replaced (F7c-10) |
| F7d-10 | `SBlock::Rt(&RtPara)` is only ever the first block of a box's flow: `FRAME_SKIP` is never set on it (a `WeakSet` per object) and it is never lifted again (a frame paragraph has no line in the flow, so no cut falls in one); the cell flow's `invisible` answers `Internal` for it (unreachable) | `liftFrames` runs once in `derive` (F7b-8) |
| F7d-11 | Harness: facets-f6.mjs names a row-sliced copy `<table ref>+r<k>` (the same row objects from `k`, every other key equal); `f6_parity` rebuilds it from the model table | the facet keyed tables by model position only (`lk-table2` was pending) |
| F7d-12 | Harness: `--gen f7d` in `capture-layout.mjs`, the committed fixture and its test, the CI step (strict) | the gate needs end-to-end parity on chains and endnote tables |

**Limits** (LQ14): `max_depth` (chains in each other's content, through `layout_static`), the work
budget (F7d-8), `max_pages` (endnote tables crossing pages); a chain's boxes and blocks bound
every other loop.

### Shared files edited (additive)

`ldoc.rs` (`Chain<'a>::boxes`), `derive.rs` (`index_chains` fills `boxes`; module doc),
`engine/mod.rs` (`pub mod chain`; module doc), `engine/flow.rs` (`RtPara::{first_from,
continuation}`, the cut run in `prepare_rt`, `join_deleted(…, first_from, …)`, `join_hidden`
keeps `first_from`, `no_shown_run_keys`, the endnote-flow table of `nextBlockLead`; module doc),
`engine/static_layout.rs` (`SBlock::Rt` and the continuation in the block walk),
`engine/floats.rs` (the three chain sites, `tb_env` `pub(crate)`, `check_chain` removed),
`engine/tables.rs` (the `SBlock::Rt` arm of `invisible`), `error.rs` (`unported` removed),
`lib.rs` (module map), `tests/layout_parity.rs` (the F7d fixture test), `tests/f6_parity.rs` (the
`+r<k>` copies), `tests/f7b.rs` (a stale comment), `tools/capture-layout.mjs` (`--gen f7d`),
`tools/lib/facets-f6.mjs` (F7d-11), `.github/workflows/rust-core.yml` (the F7d capture + strict
replay step), this record. Owned: `engine/chain.rs`, `tests/f7d.rs`, `tests/captures/f7d/`,
`tools/lib/gen-f7d.mjs`.

### Not done (handed on)

- **F8**: the editor's chain queries (`rebuildTextboxLines` over the boxes of a chain) read the
  `PlacedObject.content` lines the engine now fills for every box.
- `engine/tables.rs` keeps F7b's dead `unported` helper (`#[allow(dead_code)]`), F7b's file.
- M3 stays over the §6 per-character target (stage 5, as F7a–c).

## Stage 3 — F8 (the editor-facing API and the layout oracle)

Opus, 2026-11-05, hybrid mode, one session, in parallel with F7d (chains, endnote tables) in the
same crate and with the uo-docx D2b frontiers. Behaviour source: the frozen TS d0f45bc5 —
`refs.ts`, `run-boxes.ts`, `pdf-navigation.ts`, `inline-renderer.ts` (`paintCustomInlines`), the
editor's `engine-doc.ts` (`caretXIn`, `offsetAtX`, `isSortedByX`, `locateRef`, `resolveCell`,
`hfBlocksFor`), `notes.ts` (`noteBlocks`, `flattenBlocks`), the viewer's `views.ts`
(`webDocument`), and the `<universal-write>` methods that call them over the engine lines.

### API shape (refines plan §3: the plan's single `hit_test`/`caret`/`selection_rects` on `Layout` lacked the edit context the TS keeps)

| Rust | TS | Notes |
|---|---|---|
| `refs::Locator::new(view)` → `locate(section, &LineRef) -> Located { kind: LocKind, para: Option<ParaLoc> }`; `path_in`, `body_path`, `cell_hops`, `hf_story_for`, `paragraph`, `para_length` | `locateRef` + `lineParagraph` (+ `paragraphAtPath`, `resolveCell`, `hfBlocksFor`, `noteBlocks`) | `ParaLoc` = story + model `BlockPath` (a cell by its index); `refs.ts` itself stays ported once in `uo_docx::word::refs` (`cell_at_grid`) |
| `hit::caret_x_in`, `offset_at_x`, `is_sorted_by_x`, `collapse_custom_chars`, `xf_invert` over a `CharsView` | engine-doc.ts:371–400, universal-write.ts:953, 555 | pure, allocation-free |
| `Layout::line_index(view) -> LineIndex` (`installEngineResult`'s `eng.lines`): `located`, `line_at(layout, page, x, y)` (`engLineAt`), `page_of(layout, &ParaLoc, off)` (the find list's `pageOf`, K17c); `hit::cell_at(loc, layout, page, x, y)` (`cellBoxAt`) | universal-write.ts:16507, 18689, 17874, 20255 | |
| `Layout::edit_lines(view, &LineIndex, &EditScope) -> EditLines` (`layoutCache`): scopes `Body`, `Cell { table, row, cell }`, `HeaderFooter { which, part, page }`, `Note { kind, id }`, `Textbox { chain, page }`; `EditLines::{hit, caret, line_of, caret_x, hit_line_x, selection_rects, doc_ref, edit_pos}` with `EditPos { para, offset }`, `CaretRect` (+ `xf`), `SelRect` (+ `xf`) | `rebuildEditLines`/`rebuildTextboxLines`, `hitTestRaw` (engine branch) + `hitLineX`, `lineOf` + `caretX` (`caretRectAt`), `paintEngineSelection` | built once per layout; the queries allocate nothing |
| `run_boxes::run_boxes(view, layout, RunFilter::{Links, CustomInlines}) -> Vec<RunBox>` (`Layout::run_boxes`); `RunBox { page, x, y, w, h, baseline, run: RunRef, props: Arc<RunProps>, value }` | `runBoxes` | K17e: the props and the `RunRef` travel with the box |
| `Layout::custom_inline_face(&RunBox) -> InlineFace { face, size_pt, color }`, `Layout::measure_text(Option<FaceId>, size_pt, &[u16])` | `paintCustomInlines`' `run0` lookup and `measureText` | K17e |
| `navigation::navigation(view, layout) -> uo_render::Navigation` (`Layout::navigation`), `block_line` | `pdfNavigation`, `blockLine` | |
| `LayoutOptions::collect_fields` → `Layout::fields` | `onField` | over every pass of the call |
| `oracle::EngineOracle` (feature `oracle`, default on) `impl uo_docx::view::LayoutOracle` (`field_values`, `next_hyphenation`); `oracle::first_hyphenation`, `to_oracle_error` | `refreshFieldCache`, the layout half of `nextManualHyphenation` | here until the bindings carry a layout (F8-10) |
| `view_transform::{transformed, web_document}`; `LayoutEngine::layout` applies `o.view` | viewer `webDocument` | LQ2 |
| `Layout::missing_faces`, the warning `"font budget"` | — | closes F7a-12; registry signal F8-11 |

`query.rs` now only gathers these (`Layout::{run_boxes, navigation, lines_of, line_index,
edit_lines}`); its `PENDING` held `encode_page` alone — the Write-section `AppSection` encoder, which
landed with G8 (section "Stage 3 — G8"; `PENDING` is empty now).

### Ported (TS → Rust, TS operation order)

| TS | Rust |
|---|---|
| engine-doc.ts `locateRef` 305, `resolveCell` 336, `hfBlocksFor` 358; universal-write.ts `lineParagraph` 16531; notes.ts `noteBlocks` 89 | `refs.rs` |
| engine-doc.ts `caretXIn` 371, `offsetAtX` 386, `isSortedByX` 397; universal-write.ts `collapseCustomChars` 953 (= run-boxes.ts `collapseCustomInlineChars` 34), `xfInvert` 555, `shiftEngLine` 549 | `hit.rs` |
| universal-write.ts `installEngineResult` 16507, `rebuildEditLines` 16403, `rebuildTextboxLines` 16463, `allLines`/`lineOf`/`caretX`/`hitLineX` 13297–13334, `hitTestRaw` 15859 (engine branch), `caretRectAt` 5105, `paintEngineSelection` 18602, `engLineAt` 18689, `cellBoxAt` 20255, `search()`'s `pageOf` 17874; notes.ts `flattenBlocks` 96 | `hit.rs` (`LineIndex`, `EditLines`, `cell_at`) |
| run-boxes.ts `runBoxes` 57; inline-renderer.ts `paintCustomInlines` 50 (`run0`, `measureText`) | `run_boxes.rs` |
| pdf-navigation.ts `pdfNavigation` 32, `blockLine` 20 | `navigation.rs` |
| para.ts `fieldValueRun` 548 (`onField`) | `PrepFlow::on_field` (para.rs) + `Engine::fields`, `Call::take_fields` (engine/mod.rs) |
| universal-write.ts `refreshFieldCache` 6457, `nextManualHyphenation` 17337 (layout half) | `oracle.rs` |
| viewer views.ts `webDocument` 27 | `view_transform.rs` |

### Parity (P4: the editor- and viewer-facing facets; TS capture regenerated in the same run, LQ21)

Harness: `tools/lib/facets-f8.mjs` (owned) runs from `capture-layout.mjs --f8` after the P1 stream
of each document. The exported TS functions are imported from the frozen sources (Node 24 strips
the types of `engine-doc.ts`, `notes.ts`, `views.ts`); the `<universal-write>` methods, which are
class-private on a DOM element, are **extracted from the frozen `universal-write.ts` at capture
time** (their text, types stripped by `module.stripTypeScriptTypes`) and run on a plain object
standing for the element — only the find list's `pageOf` closure (4 lines) is transcribed. Zoom 15
makes `PX_PER_TWIP * zoom === 1`, so client pixels are page twips; one canvas per page. Facets:
`p4.locate` (every page line: kind + paragraph), `p4.caret` (`caretXIn`/`offsetAtX` over every page
line, sampled offsets and x), `p4.edit` (per scope — body, the first six cells depth first, the
first four header/footer parts on the first and the last page, three notes of each kind, the first
four model textboxes — the edit lines, `hitTestRaw` over sample points, `caretRectAt` per sampled
offset, `paintEngineSelection` per sampled range and page), `p4.lineat`, `p4.cellat`, `p4.pageof`,
`p4.runboxes` (links and plugin inlines), `p4.inline` (`paintCustomInlines`' `run0` +
`measureText` through a recording renderer), `p4.nav`, `p4.fields` (`onField` of the main layout),
`p4.hyph` (two `hyphenation: 'force'` layouts: nothing refused, then the first two points the engine
asked about refused — `p4.hyphskip`, plain, the input of the replay), `p4.web` (the P1 stream of
`webDocument(full, 9000)`). The sampling rules are mirrored in `tests/f8_parity.rs`, which lays
every document out through the real engine (reader → derive → engine, transcript policy) and
computes the same streams with the Rust queries; P1 and `pages` are compared in the same run.
`UO_F8_TRACE=<dir>` writes both item traces of `p4.edit`. `--gen f8` = an F7c (odd index: F7b)
document of the seed with hyperlinks (to bookmarks, external, broken) and plugin inlines
(`wordnext:inline:` tags) appended to some paragraphs.

| Corpus | Documents | Equal (every P4 facet + P1) | Facets compared | Mismatches |
|---|---|---|---|---|
| Word oracle, all documents, installed fonts | 2,742 | **2,742** | 41,130 | **0** |
| `--gen f8`, seed 1 | 300 | **300** | 4,500 | **0** |
| committed fixture `tests/captures/f8` (`--gen f8` seed 7, open fonts; runs in `cargo test`) | 12 | **12** | 180 | 0 |

**Rounds** (4; the first oracle run: 2,670 of 2,742 equal, 72 mismatches, P1 equal everywhere):
1. *harness*: the facet turned page twips into client pixels as `x * PX_PER_TWIP * 15`, not exactly
   `x`: points on a character's middle went to the other side (65 documents of `p4.edit`,
   `p4.lineat`, `p4.cellat`). Fixed in the facet.
2. *port*: `onField` of a field inside a field result (a PAGEREF in a TOC's cached result) reaches
   `PrepFlow::on_field` as `FieldRun { run: parent, sub: [j] }`; the first version resolved only
   `sub == []` (4 documents of `p4.fields`). Fixed: `[j]` resolves through
   `LDoc::field_result_key` (F8-6).
3. *port*: the header/footer and textbox scopes took the top-level paragraphs; the editor's model
   there is the part's / box's **flat** paragraph array (cells included), so the lines of a table in
   a textbox were missing (`txbxlink/lk-table`, `lk-table2`). Fixed in `EditScope` and in the facet,
   which now hands the part's own array (F8-8).
4. *port* (the generated documents, seed 1: 3 of 300, `p4.edit`): `installEngineResult` collapses
   plugin-inline characters **in place**, once per line, and the lines the engine shifts from one
   source line (`shiftLine`: a repeated table header row) share one `chars` array, which is
   therefore collapsed once per page it repeats on — a second collapse swallows the character
   after the inline. Reproduced (F8-15).

Between rounds 3 and 4 the capture's names were widened to the runs of textbox contents and group
members (`<run>/tx`, `<run>/g<j>`): field values in textboxes (8 values in 6 oracle documents,
`hfgroup/*`, `hftpl/pageMargin-*`) were left out of `p4.fields` by both sides; now they are compared
(`p4.fields.unnamed` = 0 on every document).

Timing: oracle TS capture with the F8 facets 2 min 27 s (four layouts per document); Rust replay of
P1 + P4 (four layouts per document) 6–9 min (release, contended machine).

### Memory and robustness (`tests/f8.rs`)

| Check | Result |
|---|---|
| M1: the line index, the edit lines of every scope of a fixture document (hostile scopes included) with a selection, both run-box filters, the navigation; outputs dropped | 151,317 B allocated, 52,136 B peak, **0 B left** |
| M3: the hot queries — `EditLines::hit`/`caret`, `LineIndex::line_at`, `cell_at` (200 points per page), `caret_x_in`/`offset_at_x` (20 probes per line) | **0 bytes allocated** (asserted) |
| M3: building them (the same document, 3 pages) | `LineIndex` 2,376 B; `EditLines` (body) 98,728 B allocated, 48,280 B peak, 47,680 B kept (lines and characters copied, F8-4) |
| no panic (12 documents): every scope (plus a cell path past the body, part `u32::MAX`, note id NaN, an empty chain) × 200 hostile probes (pages `u32::MAX`, coordinates NaN/±∞/±1e300/−0/subnormal, `EditPos` past every paragraph, reversed and `u32::MAX` selections), 300 random hostile `LineRef`s (paths of `u32::MAX`, note ids −1/0.5/NaN/1e22, unknown parts), `CharsView`s with NaN and unequal columns, a face id out of range, `block_line(NaN)`, `web_document(NaN)` | no panic |
| the oracle over a uo-docx `Session` (4 fixture documents): `field_values(session.view(), All)` = the values of the direct layout in order, run by run on the same field instructions (the session's run indices include its mark runs); `Runs(subset)` keeps the order and only those; `next_hyphenation` walked from the start = `first_hyphenation` of a forced layout of the same view | equal |
| `missing_faces` (a provider without Liberation Sans: noted, once each); `"font budget"` (a provider reporting refusals: the last warning); `Web`/`Draft` sizes, no header/footer line in Web; `Outline` = error | pass |

### Declarations (the nine kinds of uo-docx A7; LA3 keys; limits)

1. **Numbers**: `Math.min`/`max`/`abs`/`round`/`floor` through uo-jsnum; offsets are `u32` (the
   engine's `chars.o`, P1-proven non-negative) compared as `f64` where the TS compares numbers
   (`caret_x_in` takes an `f64` offset: `start − 1` is −1).
2. **Identity (LA3)**: a line's TS `Paragraph` object → `ParaLoc` (story + model path), mapped once
   to the scope index (`FxHashMap`); `l.para === p` → `ParaLoc` equality; the `Run` of `onField` →
   `RunRef` (the `RunKey` of the line's `LDoc`, relocated copies included); a chain's
   `DrawingTextbox` → `DrawingId`; `o.run.textbox` → the model run's `textbox.drawing` (a group's
   run has none, as in the TS).
3. **Order**: `eng.lines` = pages then lines; `allLines()` = scope paragraphs then lines;
   `onField`'s map = first set over every pass of the call (balancing probes and `pure()`
   included), last value (`IndexMap` insert); `missing_faces` first miss first.

| # | Decision | Why |
|---|---|---|
| F8-1 | The editor queries take a page index and page twips; choosing the canvas under the pointer (rectangles, zoom, `hitTestRaw`'s horizontal-distance order) stays in the shell; `hit` returns the raw position — `canonPos` (hidden text before the caret) is the session's | the TS mixes DOM geometry with the line logic; P4 drives the TS with identity pixels |
| F8-2 | The plan's `Affinity` is dropped: the TS has none (`lineOf` puts an offset at a wrap on the next line; the last line keeps its end) | parity |
| F8-3 | `offset_at_x` over characters not in x order finds the least `(x, index)` candidate instead of sorting a copy (V8's stable sort by `a.x − b.x`): the same element for every non-NaN x, no allocation; with NaN x V8's order is implementation-defined and the result may differ (never produced on the corpus: P1 proves the characters) | M3: a hit test allocates nothing |
| F8-4 | `EditLines`/`LineIndex` copy what they need (lines; characters collapsed or shifted as the TS mutates them) and borrow nothing; built once per layout. The TS `installEngineResult` collapses the layout's own `chars` in place; here the `Layout` stays immutable | plain data across the binding |
| F8-5 | `RunBox.value` is the link (UTF-16) or the plugin inline's `type`; the inline's `data`/`fallbackText` are read from the model through `RunBox.run` | the TS hands the `CustomInline` object; the binding serialises it on demand |
| F8-6 | `onField` reports the `RunRef` of a model run and of a field result one level down (`RunSub::FieldResult(j)`); a field nested deeper in results, a chain slice and a synthetic (frame) run have none and are not reported (the TS reports the object). Counted by the capture: `p4.fields.unnamed` = 0 on the oracle and the generated documents (the names cover textbox contents and group members) | `RunRef` names one level of results (uo-docx A4); the trait doc says "sliced and frame runs absent" |
| F8-7 | `RunFilter` replaces the `pick(run)` callback (its two TS callers) | no callbacks across the binding |
| F8-8 | Edit scopes: body and cell = their top-level paragraphs; header/footer and textbox = the flat paragraph array (cells included); note = `flattenBlocks`; a textbox scope names its chain by `DrawingId`s, head first | the editor's `this.model.paragraphs` in each mode |
| F8-9 | `next_hyphenation` answers the first body point at or after `from` (body block, then offset) in page order; the session calls again after its `b − a < 2` skip (the TS goes on in the same layout: the same answer, one layout more) | the approved two-method trait |
| F8-10 | `EngineOracle` lives in uo-docx-layout behind the feature `oracle` (default on): `uo-wasm`/`uo-ffi` carry no layout yet; a binding wraps it | brief |
| F8-11 | `"font budget"`: a new `FontProvider::budget_refusals()` (default 0) over a refusal counter of `FaceLedger` (implemented by the `viewer`, `server-substitute`, `oracle` policies and the embedded wrapper); the layout warns once, last, when the provider of the call reports refusals (the TS has no such warning; 0 on the corpus) | LA6: the minimal registry signal (uo-fonts) |
| F8-12 | `ViewTransform::Draft` = `webDocument` at the first section's text width plus the two Web margins (no TS function: the TS draft view was the legacy layout, LQ2); `Outline` = `Err(Unported(F8, …))` until milestone 2 designs it; a transformed view drops the `DocView`'s editing context | LQ2 |
| F8-13 | `missing_faces` = every `provider.face` request answered `None` during the call (the substitution chain's fallbacks included), unique, first miss first, capped at 4,096 (M5) | the shell loads them and lays out again, as the viewer does |
| F8-14 | P4 harness: the private editor methods extracted from the frozen source at capture time (above), the `--gen f8` decorator, `p4.*` skipped by `layout_parity` | the element cannot run in Node without its DOM; the extraction keeps the frozen text as the reference |
| F8-15 | The edit lines of a paragraph with plugin inlines repeat the TS's cumulative in-place collapse: a page line is collapsed once per page line sharing its TS `chars` array, found as the page lines of that paragraph with the same ref, offsets and character bits (the layout keeps no array identity; `shiftLine`/`shiftLineXY` with `dx = 0` are the sharing sites). A TS defect (a repeated header's caret skips the character after the inline) reproduced for parity — a stage-5 candidate (collapse once), to be decided with the oracle | parity; only lines with plugin inlines, so the extra key costs nothing elsewhere |

**Limits**: none new; every query indexes with `get` (a hostile page, paragraph or offset answers
nothing, or `{ para: 0, offset: 0 }` where the TS does). `flatten` over cells recurses as deep as
the model nests tables (uo-docx G20: 64).

### Requests to other modules

- **uo-docx `LayoutOracle`, D2a-C2 (open — not added)**: "which header/footer part does page N show"
  would be a third method `hf_part_on_page(&mut self, doc, page: u32, which: HfWhich) ->
  Result<Option<(HfPartId, HfSlot)>, LayoutError>` — the TS `hfPartOfPage` (universal-write.ts:18648):
  the slot of the page's first header/footer line, else Word's rule (the section's first page with
  `titlePg` → `first`; an even page with even/odd headers → `even`; else `default`), then
  `hfBlocksFor`. Everything it reads is in `Layout` (`LineStory::HeaderFooter { slot, part }`,
  `LayoutPage::{section, number}`) plus the settings; it needs the layout of the session's current
  state (a `Layout` cached by the binding, or a layout per call).
- **uo-docx session (D2b-F10)**: `field_values` keys runs of `session.view()`, whose paragraphs hold
  the editor's mark runs — the run indices are the session's (tested). The `b − a < 2` skip and the
  `to` bound of Manual Hyphenation are the session's (F8-9).
- **uo-fonts** (done, F8-11): `FaceLedger::refused`, `FontProvider::budget_refusals`.

### Shared files edited (additive)

`para.rs` (`PrepFlow::on_field` gains the resolved `RunRef`; `field_value_run` resolves it when
collecting), `engine/mod.rs` (`Engine::fields`, `Call::{fields, take_fields}`, the `Layout`'s
`fields`, `missing_faces` and the budget warning), `engine/flow.rs` (the engine's `on_field`),
`fonts.rs` (the resolver's misses, `MAX_MISSES`, `take_misses`; the embedded wrapper keeps its
ledger and answers `budget_refusals`), `lib.rs` (modules; `LayoutEngine::layout` applies the view),
`Cargo.toml` (feature `oracle`; the dev-dependency `uo-docx` with `session`),
`tests/layout_parity.rs` (skips `p4.*`), `tools/capture-layout.mjs` (`--f8`, `--gen f8`, `onField`
in the main run with `--f8`), `.github/workflows/rust-core.yml` (the F8 capture + replay step),
uo-fonts `registry/ledger.rs`, `registry/answer.rs`, `policy/{viewer,server,oracle}.rs`, this
record. Owned: `refs.rs`, `hit.rs`, `run_boxes.rs`, `navigation.rs`, `oracle.rs`,
`view_transform.rs`, `query.rs` (rewritten), `tests/f8.rs`, `tests/f8_parity.rs`,
`tests/captures/f8/`, `tools/lib/facets-f8.mjs`.

### Not done (handed on)

- `encode_page`: done by G8 (section "Stage 3 — G8").
- The editor's object hit test (`hitTextbox`, `enumFloats`: rotation tolerance in screen pixels, z
  order by `relativeHeight`) and the table-selection boxes (`tableBoxes`): their inputs are in
  `PlacedObject`/`PlacedCell`; they depend on the editor's context sets (`contextTextboxRuns`) and
  the zoom — milestone 2, with the session.
- `ViewTransform::Outline` (milestone 2); the tiling of the editor's tall Web page (TS shell).
- D2a-C2 (above).

## Stage 3 — G8 (the Write page section)

Opus WIP (commit 6471786d, 2026-11-05: schema, section/annotations code, facets, wasm export) finished
by Sonnet 5.5 on 2026-11-07 (parity, memory and robustness tests, size, CI, records). **SKILL.md assigns every role of
uo-docx-layout to Opus; the maintainer asked for a Sonnet implementation in this run.** Behaviour source: the frozen TS
d0f45bc5 — `universal-document-viewer.ts` `para(ref)` (the text layer's paragraph), `text-layer.ts`, `annotations.ts`
(`collectAnnotations`, `levelResolver`).

Record anchor: **"Write page section (G8)"** (the code comments cite it).

### Scope

What the viewer reads from the *model* after the switch, per page: each body line's paragraph (its text — the lines'
`start`/`end` and the chars' `o` are offsets in it — and its heading level) for the text layer, find and screen readers; the
run name of each placed object (`runRefs`); and, per document, the annotations (the comments with their rectangles, the
headings). Out of scope (still model reads: frontier D and the switch): bookmarks (`goToBookmark`), `getLinks`,
`getCustomInlines`.

### API shape

| Rust | TS |
|---|---|
| `section::SectionModel::{new(view, &Layout), empty(), paras(), heap_bytes()}`: gathered once per layout, plain data (it outlives the model; LA1.5: no model borrow) | the viewer's `para(ref)` over `lines`, `runRefs` |
| `Layout::{write_page(i, &SectionModel) -> Option<WritePage>, write_section(i, &m, generation) -> Result<WriteSection, SectionError>, encode_page(i, &m, &mut DeltaMark, &mut Vec<u8>)}`; `WriteSection: AppSection` | the page message with the Write section as its `app` |
| `section::RunNames::{new(view), name(&RunRef)}` (the `runRefs` names of tools/lib/canon.mjs; `MAX_NAME_DEPTH` 64) | `tree.ts` names |
| `annotations::annotations(view, &Layout) -> Annotations { comments, headings }` (`Layout::annotations`) | `collectAnnotations` |
| `SectionError::{NoPage, Codec, Render}` | — |

G8 sits on the F8 queries, not on a parallel walker: `section::text_layer_paragraph` is `refs::Locator::body_path` +
`DocView::paragraph`; the heading level is `word::StyleResolver`'s outline level (`outline_level`, then the style name
`/^(?:heading|t[íi]?tulo)\s*(\d)$/i`, unit-tested against V8 in `section.rs`).

`query::PENDING` is empty: `encode_page` landed with G8.

`write.uos` changes (the SCHEMA hash changed; `cargo run -p uo-schema --release -- generate` regenerated
`uo-docx-layout/src/codec/generated*.rs` and `uo-wasm/js/gen/write*.ts`; `check`: 12 files up to date; `codec_real` 10/10):
`WritePage.{hf_items, objects, cells}` optional (absent differs from empty in the TS `LayoutPage`), `PlacedCell.span` f64,
`WritePage.paras: list<TextPara { text, level? }>` and `LaidOutLine.para: u32?`.

### Ported

| TS | Rust |
|---|---|
| viewer `para(ref)` → `paragraphAtPath` + `runs.map(r => r.text).join('')` | `section::{text_layer_paragraph, paragraph_text}` |
| `levelResolver(full)` | `section::outline_level`, `heading_name_level` |
| `runRefs(full)` for the placed objects | `section::RunNames` |
| `collectAnnotations` (comment rectangles merged per line, reading order by the first anchor: `ka[0]-kb[0] \|\| …`, headings with the collapsed text) | `annotations::annotations`; the sort is `uo_jsnum::sort::js_sort_by` (V8 replay, also for NaN comparators), the map key `NumKey` (SameValueZero) |
| `LayoutPage` → the page message | `section::wire_page`, `Layout::{write_page, write_section, encode_page}` |
| wasm: `UoLayout` owns the `SectionModel` and the `Annotations` (computed at layout time, as the TS viewer does at `setLayout`) | `uo-wasm/src/write.rs` `uo_layout_encode_page`, `uo_layout_annotations`; `js/write.mjs` `encodePage` (message with the section), `paint.encodePage` (the bare render message), `annotations()` |

### Parity (against the frozen TS: `tools/lib/facets-g8.mjs`, capture `--g8`)

The facet `g8.section` is a stream written from the TS `LayoutPage` + `para(ref)`; the Rust side lays the document out,
encodes every page with `Layout::encode_page`, decodes the render message and the `app` with the generated decoders and
writes the same stream from the DECODED values; `g8.annotations` is the `collectAnnotations` stream. P1 and the page count
are compared too. Round trip per page: the decoded section re-encodes to the same bytes and equals `write_page`; the render
part equals uo-render's own encoding without a section.

| Corpus | Docs | Equal | Facets | Mismatches | Pages | Lines | Section B | Render B |
|---|---|---|---|---|---|---|---|---|
| committed f7a | 16 | 16 | 64 | 0 | 151 | 5,415 | 4,177,729 | 2,244,076 |
| committed f7b | 12 | 12 | 48 | 0 | 138 | 10,615 | 4,681,868 | 2,127,637 |
| committed f7d | 12 | 12 | 48 | 0 | 32 | 1,704 | 1,180,777 | 536,392 |
| committed f8 | 12 | 12 | 48 | 0 | 139 | 9,207 | 4,681,631 | 2,244,784 |
| ci seed 1 (open fonts) | 100 | 100 | 400 | 0 | 515 | 30,278 | 22,187,955 | 10,863,849 |
| `--gen f8` seed 1 (links, plugin inlines) | 100 | 100 | 400 | 0 | 971 | 56,969 | 32,923,085 | 15,526,574 |
| oracle (installed fonts) | 2,742 | 2,742 | 10,968 | 0 | 4,629 | 109,651 | 70,299,188 | 29,261,876 |
| real documents with comments (E:\UniversalWriteTest, installed fonts) | 10 | 10 | 40 | 0 | 118 | 5,748 | 5,901,525 | 2,977,232 |

0 not replayed everywhere. **The comment-rectangle path** is exercised only by the last row: none of the generated, committed
or oracle documents has `comments.xml` (0 comments in 2,742 + 200 + 52 documents). The 10 real documents (not versionable)
carry 35 comments and 55 rectangles, all equal: they prove the merging and the reading order.

Rounds: **0** G8 mismatches (every comparison equal on its first run). The finish found two harness issues instead (G8-7,
G8-8).

### Memory and robustness

- M1: `section_and_annotations_leak_nothing` — `SectionModel::new`, every page through `encode_page`, `write_section`,
  `write_page`, `annotations`, all dropped: 0 B left (13-page document, 7.7 MB allocated over the run).
- M3 (13 pages, 36 paragraphs): `SectionModel::new` allocates 88,419 B (peak 32,361 B) and **retains 27,974 B**
  (`heap_bytes`); one `encode_page` allocates 241,433 B (peak 127,480 B) for a 24,100-byte message; `annotations` allocates
  32,487 B and retains 6,096 B. wasm: 44 pages, Write sections 1,581,080 B, render parts 647,093 B.
- No panic: `hostile_section_inputs_do_not_panic` — an empty model, the model of another document's layout, another document's
  view for the model and the annotations, page `len` and `usize::MAX` (`NoPage`, nothing written, the mark unchanged),
  generations 0 and `u32::MAX`, `RunNames::name` over `u32::MAX` path steps, unknown parts and drawings, field-result and group
  subs.
- Native over the wasm exports: `write_native` 6/6 (sums `sections=8cea474e/0f5959f7/188af57d`,
  `annotations=4ceffb56/9be17165/e37051ed`, equal to the values pinned in `write_api.test.mjs`); Node on the built module:
  `write_section.test.mjs` + `write_api.test.mjs` 8/8 under `--expose-gc` (0 skipped); text layer: 1,198 lines with a paragraph.

### Size

- Shipped module (`-p uo-wasm --features write --profile wasm`): **3,948,899 B raw, 1,401,464 B gzip -9** (HANDOFF: 3.96 MB /
  1.405 MB). G8 is already inside it; its share by name (`tools/size/wasm-share.mjs`, names kept): `section` 12,921 B raw /
  5,184 gzip, `annotations` 7,929 / 2,877, `codec` 2,906 / 917, the two exports 2,595 / 371; **all 26 functions: 26,351 B raw,
  9,364 B gzip (brotli 6,475)**.
- JS: `js/gen/write.ts` 19,110 B (5,061 gzip) vs 16,601 (4,304) before 6471786d; `js/write.mjs` 18,347 (6,080) vs 13,449 (4,519).
- **Wire size (maintainer decision G8-W):** the section is 1.9–2.2× the render message (f7a: 4.18 MB vs 2.24 MB over 151 pages;
  oracle: 70.3 MB vs 29.3 MB over 4,629 pages; ≈ 28–34 KB per page on the fixtures). The chars are o/x/w columns with f64 x
  and w, and the paragraph texts are repeated per page. Options, each a schema change with its own TS-equality check: the
  paragraph texts once per layout, positions in f32 or integer twips, the chars only on demand. Kept as is: the contract is
  the TS value, equal to the last bit.

### Declarations

- **G8-1** `SectionModel` is gathered once per layout and kept beside the `Layout` (LA1.5: no model borrow).
- **G8-2** The wasm layout computes the `SectionModel` and the annotations eagerly at layout time (as the TS viewer at
  `setLayout`); every relayout pays for them. Lazy needs the document handle alive and borrowed at query time (maintainer
  decision).
- **G8-3** `uo_layout_encode_page` does not call `Layout::encode_page`: the wasm layout moves its display lists into the
  `RenderView` (`pipeline::take_pages`), so it calls `write_section` + `RenderView::set_app`/`encode_page`.
- **G8-4** `section::paragraph_text` stays local: `uo_docx::session::text::para_text` is behind uo-docx's `session` feature,
  which the viewer build does not enable. `section::RunNames` and the test-only `run_names` of `layout_parity.rs` /
  `f8_parity.rs` stay two implementations (those suites pass; refactoring them is out of scope).
- **G8-5** `MAX_NAME_DEPTH` 64: deeper nesting is not named (no `run`); the walk is recursive and the depth is the reader's.
- **G8-6** `annotations` sorts through `js_sort_by` (da31758d's one sort core) and keys by `NumKey`; the TS `!(x2 > x1)` carries
  `#[expect(clippy::neg_cmp_op_on_partial_ord)]`, as in `run_boxes.rs`.
- **G8-7** The adapter test failed on any fresh `--dump` capture since 5cfa71d9 (not since 6471786d): it skipped a fixed list of
  facets (`p2`, `p2first`, `p3.prepare`, `p3.lines`, `pages`), so the P3 facets that 5cfa71d9 added (14 per document) were
  "not replayed" (840 for 60 documents; 96 on a 6-document `--g8` capture = 14 + the 2 g8 facets per document). It now skips
  every facet but `p1` (fresh ci capture: 100 equal, 0 mismatches, 0 not replayed). Other replays with fixed skip lists may
  have the same latent issue on fresh captures: audit before the first CI run.
- **G8-8** `f8_parity` skips `g8.*` (it reported 200 "not replayed" on an `--f8 --g8` capture). CI captures G8 in a separate
  step (`--gen f8 --f8 --g8`, 100 documents) replayed by `tests/g8.rs`.

### Shared files edited

`schema/write.uos` + generated codecs (WIP), `lib.rs`, `query.rs` (`PENDING` empty), `tools/capture-layout.mjs` (`--g8`),
`tests/adapter.rs`, `tests/f8_parity.rs`, `.github/workflows/rust-core.yml` (the G8 capture + replay,
`write_section.test.mjs`), uo-wasm `src/write.rs`, `src/render.rs`, `js/write.mjs`, `js/write.d.ts`, the three records.
Owned: `section.rs`, `annotations.rs`, `tests/g8.rs`, `tests/captures/g8/`, `tools/lib/facets-g8.mjs`,
`uo-wasm/tests/write_section.test.mjs`.

### Not done

- Bookmarks, `getLinks`, `getCustomInlines` as exports (frontier D and the switch).
- A versioned fixture with comments (the real corpus is not versionable).
- Wire-size reductions (G8-W) and a lazy section/annotations (G8-2): maintainer decisions.

### Process metrics

| Stage | Agent (model) | Notes |
|---|---|---|
| schema, section, annotations, facets, wasm export | Opus (WIP 6471786d) | stopped mid-frontier, nothing run |
| finish: clippy fix, `NumKey`/`js_sort_by`, adapter and `f8_parity` fixes, memory and no-panic tests, heading unit test, parity (8 corpora), wasm and Node, size, CI, records | Sonnet 5.5 (maintainer's request; SKILL.md says Opus) | one session, 2026-11-07 |

Parity rounds: 0 G8 mismatches. Rework: `--gen f8` needs `--f8` too (the first capture crashed in the harness);
`f8_parity` did not tolerate the g8 facets (fixed, G8-8).

## Stage 3 — G4 (cancellation inside the layout)

### Scope

Bindings gate item G4 (uo-bindings.md): a caller can stop a running layout. Before, the flag was read before and after the
layout only; the work budget bounded the time. No TS source: this is an addition outside the frozen semantics; with both
new fields `None` the output is unchanged (parity runs and the wasm digest below).

### API shape

- `LayoutOptions.cancel: Option<Arc<AtomicBool>>`: raised from any thread (the shape of uo-pdf `PdfOptions.cancel` and uo-ffi
  `UoCancel`); a Relaxed load at every `Budget::charge`.
- `LayoutOptions.cancel_poll: Option<fn() -> bool>`: for hosts that cannot share a flag (wasm); called at the first charge and
  then every `CANCEL_POLL_UNITS` (4,096) work units; `true` cancels.
- `LayoutError::Cancelled` (Display "layout cancelled"); `Budget::with_cancel(cancel, poll)`, set in `layout_document`
  (`engine/mod.rs`, the one place a `Budget` is built). The state is latched in the budget.
- Every work charge goes through `Budget::charge` (38 call sites), so no engine code changed; no layout code swallows the
  `Err`, so `Cancelled` propagates like `Limit(Work)`.

### Tests

- `options.rs` unit tests (4): raised flag latches, `Cancelled` wins over an exhausted budget, the poll cadence, no cancel = no error.
- `tests/g4_cancel.rs` (5 + 1 opt-in; 1.8 s release, 16.6 s debug: debug uses a 150-paragraph document and a small warm-up): flag raised before the call; raised mid-layout at 10/40/120 ms on a 650-paragraph
  document returns in 0.16-1.2 ms (bound 250 ms release, 2 s debug); read and derive run outside the timed and the leak-checked regions (`prepared` / `layout_prepared`); the poll cancels at exactly the k-th poll (201 polls in a
  120-paragraph layout); M1 after a cancelled layout 0 B left (poll: 180 MB allocated, 4.4 MB peak; flag at 5 ms: 8.1 MB
  allocated, 0.68 MB peak, asserted > 100 kB so the flag cannot have gone up before the work); a layout after a cancelled one, same provider and registry, equals a fresh one (pages/items/lines per page and the whole P1 canonical stream); `bench_cancel_check_cost` (`UO_G4_BENCH`).
- Through the bindings (numbers of the fix round; they vary run to run, e.g. C ABI 0.7-1.4 ms, .NET 0.6-1.5 ms): uo-ffi `tests/cancel_layout.rs` (code 10 / stage none, 1.4 and 1.2 ms, bound 50 ms release, M1 66 MB allocated, 2.7 MB peak,
  0 B left), `tests/write_cancel.test.mjs` (Node, 7; 53 polls in the largest f8 fixture; the state-after-cancel and M1 cases cancel at the k = n/2-th poll, M1 with the counting build; a nested `layout()` from `cancelCheck` throws instead of re-entering), uo-wasm `write_native` `the_host_poll_cancels_the_layout_with_code_10` (bit 9: yes = code 10, no = same bytes, no bit = never asked), .NET
  `A_token_cancelled_during_the_layout_stops_it_promptly` (0.6-1.5 ms) and 5 mid-layout cancellations in
  `Conversions_leave_nothing_ok_failed_or_cancelled` (0 B left; whole project 14/14).
- Unchanged and green: layout_parity 5, f8_parity 2, g8 4, f7a 7, uo-ffi server_parity 3 and ffi_memory 4, uo-wasm write_native 6,
  write_api and write_section (Node).

### Cost (the check on a big document; interleaved A none / B flag / C poll, release, a noisy machine)

| document | A min / median | B flag (min / median) | C poll (min / median) | polls, ms per poll |
|---|---|---|---|---|
| pages100 (100 pages, 15 runs) | 527.1 / 881.4 ms | -2.9% / -0.3% | +17.5% (one slow run) / -0.3% | 395, 1.3 ms |
| pages500 (500 pages, 5 runs) | 4,450.2 / 4,483.5 ms | +0.4% / -0.1% | -0.1% / +1.5% | 1,980, 2.3 ms |

The differences are inside the run-to-run noise of this machine (other agents compiling): the check costs nothing measurable,
so `CANCEL_POLL_UNITS` = 4,096 stays (1.3-2.3 ms per poll native; the +17.5% min of C on pages100 is one outlier, its median is -0.3% and pages500 shows -0.1%/+1.5%). wasm (`tools/size/wasm-ab.mjs --runs 21`, base = HEAD module):
digest `cca8d8794ec4bfe0` equal on both, layout min 366.1 -> 367.9 ms (+0.5%), total 498.7 -> 500.1 ms (+0.3%). Module size 3,948,899 ->
3,966,599 B raw (+17,700), 1,401,461 -> 1,409,093 B gzip -9 (+7,632); the base is HEAD and the working tree also holds the
other frontiers' uncommitted changes (uo-render, uo-pdf, wasm render), so this delta is not attributable to G4 alone.

### Declarations

- **G4-1** `cancel` is checked at every charge (a Relaxed load plus an `Option` test), not on a stride; the cost above is noise.
- **G4-2** Cancellation is checked before the work limit (`Cancelled` wins over `Limit(Work)`), it latches, and `left` is not zeroed.
- **G4-3** uo-ffi maps `LayoutError::Cancelled` to `JobError::Cancelled`: code 10, stage none, as a PDF cancel (also an explicit arm in `code()`).
- **G4-4** The `.doc` conversion, the docx reader and the wasm PDF are not cancellable inside (bounded by their limits; the wasm PDF stops
  through `pdf_chunk` answering non-zero, code 13).
- **G4-5** uo-docx's `LayoutOracle` error kind is not extended: `to_oracle_error` maps `Cancelled` to Internal (the oracle never passes a cancel).
- **G4-6** `CANCEL_POLL_UNITS` = 4,096; the latency bound in the tests is 250 ms release (the measured values are about 1 ms).
- **G4-W** wasm: opt-in host poll. `layout_flags` bit 9 makes the layout call `uo_host.layout_poll() -> i32` (non-zero cancels, code 10);
  JS `LayoutOptions.cancelCheck` (typically a deadline); a throwing callback counts as cancel (an exception must not unwind through the
  wasm frames). Not chosen: a Worker with `terminate()` (host side, still possible), a clock import, a `SharedArrayBuffer` flag
  (needs cross-origin isolation, outside the Safari 15 baseline). **New import**: every loader must build its imports with
  `writeImports()`; a module built before G4 ignores the extra import.
- Native stand-in: `uo_wasm::write::host::set_layout_poll` (thread-local, as `set_pdf_sink`); `write_native` tests it. JS: `layout()` refuses a nested call (`host.busy`).
- The first poll happens at the first charge, so even a tiny layout polls once.

### Shared files edited

uo-ffi `src/pipeline.rs`, `src/server.rs`; uo-wasm `src/write.rs`, `js/write.mjs`, `js/write.d.ts` (minimal: the import, the host
slot, the flag bit, the option; the files also carry the other frontiers' uncommitted edits); `.github/workflows/rust-core.yml`
(`write_cancel.test.mjs`); `dotnet/UniversalOffice.Docx/DocxLayout/DocxPdfConverter.cs` (doc comment),
`dotnet/UniversalOffice.Docx.Tests/FacadeTests.cs`; uo-bindings.md. Owned: `tests/g4_cancel.rs`, uo-ffi `tests/cancel_layout.rs`,
uo-wasm `tests/write_cancel.test.mjs`.

### Not done

- `LayoutErrorKind::Cancelled` in the uo-docx oracle (milestone 2); cancellation of the wasm PDF and the reader.
- The cross-binary bench (`bench_memory` HEAD vs new); the in-process A/B above is the measure.
- clippy `-D warnings` is clean natively and for wasm32 on uo-docx-layout, uo-ffi and uo-wasm (`--all-targets --all-features`).
  `rustfmt --check` shows diffs that were there before (`engine/mod.rs` line 1121, an import order in `write.rs`), none in the G4 lines.

### Process metrics

SKILL.md assigns every role of uo-docx-layout to Opus; the maintainer asked for a Sonnet implementation in this run.

| Stage | Agent (model) | Notes |
|---|---|---|
| plan (measured baselines, steps) | Opus | one round |
| implementation: layout, ffi, wasm/JS, .NET, tests, measurements, records | Sonnet 5.5 (maintainer's request) | two sessions on 2026-11-09: the first attempt, then a re-verification of the same working tree; wall time not recorded by the agents (the first release build alone 2.5 min, the tests seconds) |
| review | reviewer | one round: 1 major (FacadeTests.cs mixed encodings, now UTF-8 again), 6 minor |
| fix round 1 | Sonnet 5.5 | one session: UTF-8 restore, docs, test split (read/derive outside), debug 45 s -> 16.6 s, canon-stream comparison, ffi bound 250 -> 50 ms, wasm k-th-poll cases, native host-poll test, JS re-entry guard |

## Stage M2 — incremental relayout (LA7)

Frontier R of milestone 2, wave 2 (queue item 7, 2026-11-11). Opus planner → Opus implementer. Not committed (the
orchestrator commits the wave).

### Scope

`LayoutEngine::relayout(view, changes, provider, o)` (signature unchanged; the WS frontier calls it) gives **the same
`Layout` bits as a full layout of the same view** (LA7 criterion, maintainer decision 1 "EXACT" as recommended: pages by
`Debug`, resource tables entry by entry, `image_origins`, `warnings`, `missing_faces`, `fields`, `stamp`, lineBox ids
included), computed on the previous relayout's run when only body blocks changed in place. **Level 2 (page checkpoints)
is implemented, single- and multi-pass. Level 1 (the paragraph memo of `prepare_paragraph`, with the recorded reads and
the replayed effect log) is implemented in fix round 2**; its line memo is not (D-LA7-26, measured). Structure edits
(Enter, a merge, a paste) stay full layouts for level 2 and are served by the memo: half the time of a full layout.
`layout()`, `EngineOracle` and the viewer/server path (`engine::layout_document`) use neither level and keep their output
(re-proved below).

### Design (code: `src/incr/`, hooks in `src/engine/`)

- **Recording** (`incr/record.rs`, `Recorder`, a `BlockHook`). A recorded full call (`layout_document_with(…, Some(rec))`)
  takes, per pass, a `Checkpoint` before the first body block of every new page (`run_from` calls the hook before each
  body block of the main flow, never in cells, static flows or the endnote flow): the engine cloned without its pages and
  trace, the open pages `pages[cur..]`, the lengths of the call's tables (`CallLens`: faces, resolver entries and misses,
  symbols, images, text effects, page fills, custom paths, image origins, pending metafile warnings, `line_seq`). Per
  pass it keeps the input, the final pages, the final engine (its warnings as at the pass end, before `finish`), the
  pass's `onField` map and the end lengths; per call the final `FaceTable`, `FaceResolverCache` (with the provider
  request log) and `ImageTable`. The `Layout` keeps the resources. A call that ran column-balancing probes keeps nothing
  (`Balancing`, decision 4).
- **Trace** (`incr/trace.rs`, `Engine::trace`, a new field class "trace": never compared, never in a checkpoint). Per
  interval between checkpoints: the body blocks read besides the one being placed (`flow_block`, `flow_table`, i.e.
  lookahead, keep-with-next chains, REF/NOTEREF/STYLEREF/bookmark paragraphs, `paraByRef`; `note_read(i)` directly in
  `pin_floats_over_table` — the paragraph after a table, read for its floats, space before and style — and in
  `place_late_tables` — the late floating table at block `bi`, laid out at its page's start before its turn; both
  added in fix round 1 after the review found them untraced; `note_read_all` for the list markers cache `list_texts`
  and for late floats), and the lowest page written (`ps_mut`, `make_page`). Every direct read of
  `ldoc.stories[…].blocks` under `src/engine` was checked again: the others are the block being placed (`run_from`,
  `story_table` from it), the block count (`story_len`, keyed by the shape), static, note, endnote and header/footer
  stories (not the body flow) and the prologue's cover page (keyed by the prologue). The reads use
  `Cell<u32>` counters because the read sites are `&self` methods (no `RefCell`, no sharing: D-LA7-7).
- **Keys** (`incr/keys.rs`): the options by `Debug` without `cancel`/`cancel_poll` (none with hyphenation skips);
  every recorded provider request asked again with the same answer identity, and the budget-refusal flag; every document
  part but the body the same object (`sections`, `notes`, `comments`, `styles`, `bookmarks`, `formatting`,
  `background`, each header/footer part; `core_props` and `formatting_version` equal; `media` needs no key: `MediaStore`
  is append-only, with no replace API, so an id never changes bytes); no context story; no textbox chains;
  the same derived-document shape (`shape()`: every LPara's source, id, flags, runs, overrides, path; every run's source
  and `RunRef`; stories, textboxes, revision order); the same prologue (cover page, first page number, section
  geometries). The changed body blocks `[b0, b1]` come from structural sharing (`PVec::unshared_chunk_elements`, then
  `Arc::ptr_eq` per paragraph and table anchor, mapped to `Story::blocks` indices). The previous call's body and parts
  are kept as shallow clones, so a changed paragraph always has a new pointer (copy-on-write sees them shared).
- **Resume** (`incr/mod.rs`, `incremental`). Per pass of the previous call, in order: the input must equal the previous
  one; the latest checkpoint `c` with `c.b ≤ b0`, nothing read at or past `b0` before it, and no page below `c.cur`
  written after it ("sealed"); the tables restored as prefixes of the final ones (`restore`: the symbol table rebuilt by
  interning its first strings again, the others truncated copies); the engine cloned from `c`, the previous pages
  `[0, c.cur)` moved in, `c.open` cloned; `run_from(cx, Some((gi, b)), &mut Converge)`.
- **Convergence** (`Converge::meets`, `incr/bits.rs`). At a later checkpoint `k` of the previous run (`k.b > b1`, same
  section/block): sealed after `k` and nothing after `k` reads a block `≤ b1`; same `cur` and page count; the same table
  lengths (the line counter aside) and the same entries since `c`; the same engine state — `state_string`, the `Debug` of
  every field but the pages, the trace, the style-resolver memo and the cell-neighbour memo (exhaustive destructuring:
  a new `Engine` field does not compile until classed) — and the same open pages by `Debug`; both compared after moving
  the previous run's line ids into the new numbering (`relabel_debug`). Then `Stop`.
- **Line ids** (`incr/relabel.rs`). An edit that adds or removes lines moves every later `lineBox.line` of a full
  layout. `LineMap { from, by }` (ids at or past `from` move by `by`); `Relabel` (exhaustive destructuring over
  `PassItems`, `CellContent`, `NestRow`, `RowLayout`, `MergeText`, `RowSplit`, `TableLayout`, `ZObj`, `PageNote`,
  `PageState`, `FloatCarry`, `FloatFlow`, `FlowMerge`, `NoteCarry`, `NoteCut`, `LastRowEmit`, `Engine`, `LayoutPage`)
  moves the kept pages, the final engine and the kept checkpoints after the meeting point, and the reused `LayoutPage`s;
  a shift carried from an earlier pass (`shift`) moves the resumed pass's prefix too. Nothing is moved when no line was
  added (the typing case).
- **Splice.** Pages `[0, k.cur)` from the new run, `[k.cur, n)` from the previous one; the pass's final engine, end
  tables and `onField` map are the previous run's (the state at `k` is equal and the rest is deterministic and reads no
  changed block). The pass record is spliced the same way (checkpoints and trace intervals before `c`, the new ones, the
  previous ones from `k`). Without a meeting point the pass runs to its end (still exact); then the next pass resumes only
  if the tables equal the previous call's at that pass end (`Tables` otherwise).
- **Pass loop.** `Call::next_input` (factored out of `layout_document`, shared by both paths) on the spliced engine; the
  incremental call stops where the previous one stopped (else `Unstable`), continues while each next input equals the
  previous call's (else `PassInput`); balancing found → `Balancing`.
- **Page assembly.** `Engine::finish` split into `finish_prep` + `finish_page(reuse)`: the pages before the resume point
  (and not written by the new run) and the pages from the meeting point on reuse the previous `LayoutPage` (relabelled),
  after running their resource-touching steps (page borders, revision-bar colour, background) in page order; then the
  final tables must equal the previous call's (`Tables` otherwise), so a reused page's ids mean the same strings.
- **Caps (M5).** `IncrConfig { enabled, checkpoint_bytes }` (defaults: native 512 MiB, wasm32 96 MiB) covers the
  estimated heap of the checkpoints plus the kept pages of every recorded pass. Past it the checkpoints are spaced every
  k pages (`fit_cap`, distance-based so re-applying keeps them; applied after every recorded or incremental relayout);
  when the kept pages alone do not fit nothing is kept (`Memory`).
- **Level 1 — the paragraph memo** (`incr/memo.rs`, fix round 2). Every `relayout` (the recording full layout, a resumed
  level-2 run, the context-view full layout) prepares body paragraphs through `Engine::prepare` → `memo::prepare` (the memo
  rides in `Cx::memo`, taken out of the `Cx` while one paragraph is prepared, so nothing a field lays out inside it uses
  it). Scope: a derived paragraph that is the model paragraph itself (`ParaSrc::Model`), in the body story, without
  overrides, flags, frame carry or revision joins, and a top-level body paragraph (its `Arc` is in `body.paras`); the
  rest is prepared as before (`skipped`). An entry per `ParaId` and environment (at most two: the main flow's and the
  keep-with-next lookahead's, which now also goes through `Engine::prepare`, identical without a memo) holds:
  - the **key**: the paragraph node's `Arc` (kept alive, compared with `ptr::eq`), the address of each run's model run and
    its field-result count, the `ParaEnv` by `Debug`, `cx.depth`; per call, the options by `Debug` (as level 2, the
    effective options) and the `formatting` and `styles` objects; every face key the prepare asked the resolver for
    (`FaceResolverCache::tap`), its `FaceId` and the provider's answer identity — all must be cached alike in the running
    call (`cached`), so a hit adds nothing to the face tables;
  - the **effect log**: every `PrepFlow` call in order — `field` (PAGE, REF, STYLEREF, NOTEREF…: the other paragraphs
    they read are read again by the live call), `number_next` (the numbering marker), `note_mark` (with the text cap
    before and after), `on_field`, `warn` (a new `PrepFlow` method, default `ctx().warn`, so the one warning site of
    `prepare_paragraph` is seen) — with its answer, and the paragraph's own work units (the budget spent minus what the
    live flow calls spent);
  - the **result** with its run keys made relative to the paragraph (`run` index, field result `sub`), its glyphs packed
    (`Core`, 48 bytes for a glyph with no optional field, ascent/descent/size in a per-paragraph table; the others kept
    whole).
  A hit replays the log on the live pass state in order; when every answer is the recorded one it charges the work
  units once and returns the result with the running call's keys. When answer `i` differs (a list item renumbered, a
  PAGE field on another page, NUMPAGES of the next pass) the paragraph is prepared again with calls `0..=i` served from
  what the replay already did (`RecFlow`: the same calls with the same arguments — a deterministic prefix — so no effect
  runs twice), and the new run replaces the entry. Not kept: a prepare that added an entry to any call table
  (`CallLens` before/after: a new face, symbol, text effect, image, or the line counter — then a hit would have to add
  it too; in practice the first paragraph using a face), a result with a text effect or a picture-bullet image (call-local
  ids), a key that is not one of the paragraph's runs or their field results, a face in the result not asked by the
  prepare. Caps (M5): `IncrConfig::memo = MemoCaps { enabled, entries, bytes }` (defaults on, 2^20 entries, 384 MiB native,
  64 MiB wasm32); past the bytes new paragraphs are refused (`over_cap`) and the kept ones stay (a full layout walks in
  document order: evicting the oldest would evict exactly what the next full layout needs first); after each full layout
  the entries it did not use go (removed paragraphs). The memo is dropped with `clear()`/`clear_caches()`, on any error
  of a relayout, and when the call key changes.

### API added (for the WS frontier)

- `LayoutEngine::with_incremental(IncrConfig) -> Self`, `last_relayout() -> Option<&incr::RelayoutReport>`,
  `clear_incremental()`; `clear_caches()` also drops the incremental state; `last()` returns the newer of `layout()`'s
  and `relayout()`'s result.
- `incr::RelayoutReport { path: Full | Incremental, reason: Option<FallbackReason>, passes, changed_blocks,
  resumed_page, converged_page, relaid_pages, checkpoints, spacing, checkpoint_bytes, retained_page_bytes, line_shift,
  layout_bytes }`. The shell repaints `relaid_pages` when `path == Incremental`, every page otherwise. The pages outside
  `relaid_pages` encode the same section bytes as before even when `line_shift ≠ 0` (lineBox is not in the page encoding,
  F0-18), but their `LayoutPage.item_meta` ids moved.
- `incr::FallbackReason`: `NoBase, Disabled, View, Options, Provider, Document, Context, Chains, Shape, Prologue,
  NoCheckpoint, PassInput, Unstable, Tables, Balancing, Memory, Failed` (`Failed` added in fix round 1, D-LA7-15).
- Unchanged: `relayout`'s signature and the layout-local `model::StoryChange`; `layout()`, `layout_document`.
- Fix round 2: `IncrConfig::memo: MemoCaps` (a new field: a struct literal of `IncrConfig` needs `..IncrConfig::default()`,
  only this crate builds one today); `incr::{MemoCaps, MemoReport, ParaMemo}`; `RelayoutReport::memo` (hits, replayed,
  misses, skipped, unmemoizable, over_cap, entries, bytes). `relaid_pages` now starts at the lowest page the resumed run
  wrote (`lowest.min(resume page)`), not the resume page (review finding: a page below it re-assembled after a float
  receipt was outside the range); the tests now check the contract on every incremental relayout. Crate-internal:
  `Cx::memo` (a new field of a public struct with public fields: struct literals of `Cx` outside the crate need it; only
  this crate's tests build one), `Call::memo`, `layout_document_with(…, rec, memo)`, `FaceResolverCache::{tap, cached}`,
  `PrepFlow::warn` (defaulted), `para::field_model_ref` (factored out of `field_value_run`, same logic).
- `last()` (review finding checked): milestone 1's `layout()` already set `last = None` before laying out, so a failed
  `layout()` or `relayout()` left `last()` `None` then too; the semantics are unchanged and the doc comment, which said
  "the last successful layout", now says what it does (`None` after a failed call; a failed relayout also drops the
  incremental state). For the WS frontier: keep the shell's own copy of the pages if it must show something after a
  failed relayout.

### Declarations (outside the TS semantics)

- **D-LA7-1** Exact criterion (decision 1): `Layout` equal field by field, compared in the tests by the P1 digest, the
  `Debug` of every page, the symbol strings in order, the faces by key/family/flags/bytes identity, the other resources
  and the rest by `Debug`.
- **D-LA7-2** `changes` is advisory (decision 2): what changed is found by structural sharing.
- **D-LA7-3** Level-2 scope (decision 3): in-place edits only. Enter, Backspace across a paragraph mark, paste, table
  structure, list toggles, section and style changes replace `sections`, bump `formatting_version` or change the shape:
  full layout (`Document`/`Shape`). Measured probe (`which_arcs_change_on_typing`): typing replaces 1 body paragraph and
  no document part; Enter and Backspace (merge) replace `sections` and bump `formatting_version`.
- **D-LA7-4** `FaceResolverCache.resolved` is an `IndexMap` with `FxBuildHasher` (was `FxHashMap`) so a prefix can be
  restored; its order is never observed. The resolver gains a provider request log (`Some` only while recording).
- **D-LA7-5** A call that ran balancing probes keeps no checkpoints (decision 4).
- **D-LA7-6** The incremental call's work budget is fresh and counts only the work it does (decision 6); cancellation
  polls at its own charges. On any `Err` the incremental state is dropped (decision 7).
- **D-LA7-15** (fix round 1) An `Err` of the incremental attempt itself (an internal check of the splice or the
  restore, `Limit` of its fresh budget, `NoFace`…) does not fail the call: the state is dropped and the same call lays
  out in full (`FallbackReason::Failed`); `relayout` returns `Err` only when the full layout fails too. Exception:
  `Cancelled` is returned at once (the caller asked to stop; a full layout would be cancelled again). New variant
  `FallbackReason::Failed` (no other crate matches on the enum yet).
- **D-LA7-16** (fix round 1) A relayout of a view with a context story (a cell, note, header/footer or textbox being
  edited) is a plain full layout that keeps no base (`no_base = Context`): the next relayout outside the context is a
  full recording one. Keeping a base recorded under a context view would make the next non-context relayout exact
  only if the session replaced the body or part `Arc`s whenever the context's content is written back — a dependency
  on the session not worth its one saved full layout. Tested: `lookahead_sites_are_traced` enters a cell, types,
  leaves, types in the body (exact; the relayout after leaving is `Full(Context)`, the next keystroke incremental).
- **D-LA7-7** The read trace uses `Cell<u32>` inside `Engine::trace` (the read sites take `&self`).
- **D-LA7-8** Options with hyphenation skips (a `HashSet`, unstable `Debug`) are never incremental (the editor's
  relayouts pass none; `EngineOracle` uses `layout()`).
- **D-LA7-9** State equality by `Debug` strings: two NaNs of different payloads print alike (no carried NaN reaches the
  output with its payload); `FxHashMap`/`IndexMap` order differences only miss a convergence (slower, exact).
- **D-LA7-10** Line ids after an added/removed line are moved by `Relabel` (the plan's `shift_line_seq`, extended to every
  type holding items because checkpoints hold engines with notes, chains and float carries).
- **D-LA7-11** Copies (frame anchors, revision views, chain slices) need no special rule in level 2: the shape key
  includes them. Documents with textbox chains are never incremental (`Chains`): a chain member's content is read from
  other blocks.
- **D-LA7-12** Conservative reads: a read at or before `b1` after the meeting point (e.g. a STYLEREF in a header looking
  back) prevents convergence there, even when the block read is unchanged (min/max per interval, not a set).
- **D-LA7-13** The final page assembly requires the call's final tables equal to the previous call's whenever pages are
  reused: an edit that resolves a new face or interns a new symbol is a full layout (`Tables`).
- **D-LA7-14** Debug-based `ImageTable`/resource comparisons compare image bytes by content, not `Debug`.
- **D-LA7-17** (decided by fix round 2, option (a)): level 1 is implemented by this frontier as the brief asks (the review
  of fix round 1 rejected descoping it inside the frontier); the level-2 key remap stays a follow-up for the gate.
- **D-LA7-18** Memo scope: plain body model paragraphs only (above). Cells, headers/footers (PAGE differs per page: they
  would mostly replay), notes, textboxes and copies are prepared as before. Measured on pages50: 245 of 579 prepares of a
  full layout are header/footer paragraphs (`skipped`).
- **D-LA7-19** A hit needs no table growth: a prepare that grew any call table is not kept, and a hit needs every face key
  it asked cached with the same `FaceId` and answer. Exact because the tables only grow, and an existing entry's id never
  changes. Faces are the only call-local ids a kept result may hold (checked: every `FaceId` of the result, the marker's
  and the equations' included, was asked by the prepare).
- **D-LA7-20** Answers are compared by value: field texts and note marks as UTF-16, the text cap by value, numbering
  markers by `Debug` (a JS number: `-0`/`+0` apart), `f64` arguments by bits, `JsNumber` arguments by `Debug`. A
  different answer only costs a new prepare.
- **D-LA7-21** A served call that is not the one the prepare makes, or a key that does not map, is an internal error
  (`incr: memo check`; impossible for a deterministic prepare, never seen in the 110,000 hits and 1,900 replays of the fix-round-2 test run). In a
  level-2 attempt it falls back as D-LA7-15; in a full layout the call is laid out again without the memo (and the memo
  is dropped). Not tested by injection (no hook).
- **D-LA7-22** The work budget: a hit charges the paragraph's own units in one charge after the replay (the same total;
  a limit error has the same kind; cancellation polls there once). A live field call that fails during a replay returns
  its error at once (the prepare would fail with the same first error; the formula path's later field calls are not
  made — the relayout fails either way).
- **D-LA7-23** Not compared or replayed (pure caches, never observed): the pass's style resolver memo, the hyphenation
  sessions (created lazily, persistent per `LayoutEngine`), the face table's equation info. `PreparedPara.ppr` and the
  styles' `rpr` of a hit are the previous call's `Arc`s (equal content; no `ptr_eq` on them anywhere in the engine,
  checked by grep).
- **D-LA7-24** The memo's call key is the options' `Debug` like level 2's: a host passing a new `clock` every relayout
  would clear both levels every time (the editor shell should pass a fixed clock between explicit refreshes — WS
  frontier).
- **D-LA7-25** The keep-with-next lookahead (`next_block_lead_step`, 2 sites) calls `self.prepare(cx, &PSrc::Doc(k), …)`
  instead of `prepare_paragraph(cx, self, k, …)`: the same function when no memo is given (the full/viewer path), and
  memoized with its own environment entry when one is.
- **D-LA7-26** The line memo of level 1 is not implemented. Measured (temporary counters, removed; release, pages50 /
  pages500 full layout): `prepare_paragraph` through `Engine::prepare` 105 / 1,085 ms of 204 / 2,113 ms (51 %),
  `LineBreaker::next` 12.3 / 130 ms (6 %). A line memo would need the identity of the `PreparedPara` at the 23
  `LineBreaker` call sites (the engine changes prepared paragraphs after `prepare`: markers attached and detached, spacing)
  and would save at most 6 %. Proposed for the gate: drop it.
- **D-LA7-27** Memo cap policy: keep-first, no eviction (above); the per-paragraph environments evict the one used longest
  ago. Glyph packing (`Core`) is a storage choice: `plain()` destructures `Glyph` exhaustively, so a new optional field
  does not compile until it is classed, and an unpacked glyph is `Glyph::new` plus the stored fields (exact).

### Engine field-by-field equality (`incr/bits.rs::state_string`)

Compared by `Debug`: `ctx.{flow, rev, warnings, note_numbering, note_mark_override}`, `story, cur, col, y, col_top, at_top,
top_gap_prev_after, prev, last_box, first_in_doc, ln_count, ln_page, ln_sect, last_row_emit, tbl_x0, in_hf, float_carry,
float_flow, next_table_layout, only_text_floats, pre_placed, z_seq, col_used, balance_saved, balance_last_free,
placed_tables, late_split, col_bottoms, in_cell, hf_frame_after, hf_frame_tbl, static_dx, cell_mar, col_max_y,
sect_gap_after, pending_join, pending_del, float_table_gap, carry_anchor_gap, note_carry, row_note_cut, flow_fills,
en_flow, cur_endnote, endnotes_done, pinned_floats, chain_slices, field_at, cell_ps, cell_collect, late_marks, scratch,
static_seq, field_page, en_blocks, sect_geoms, geoms, cover_page, first_page_number, list_texts, note_cache, sep_cache,
found, fields`. Not compared: `pages` (the open ones are compared apart, the earlier ones are sealed), `trace`,
`ctx.resolver` (pure memo of `Arc`s of resolved styles), `cell_nb` (pure memo of a table's cell neighbours; the changed
blocks are not read after the meeting point). `note_cache`, `list_texts`, `sep_cache` and `chain_slices` are compared
although caches: a miss lays out content (tables, line ids), so presence matters.

### Verification

**Differential "incremental == full"** (`tests/la7.rs`, three tests sharing the 70 documents so `--test-threads=3`
runs them in parallel: the 68 committed fixtures f7a–f8 with their transcript providers + the generated 58-page PAGE
and PAGE+NUMPAGES documents; per step 70 % `d2a_support::next_step` (type, Enter, Backspace, Delete, selection,
tracking, contexts, undo, redo) and 30 % own layout steps (bold, space after, line spacing, centre, list toggle,
200-character insertion); after every step `relayout` vs a fresh `LayoutEngine::layout`, errors compared by kind):

| run | documents × steps | relayouts | incremental | errors (equal kinds) | mismatches |
|---|---|---|---|---|---|
| default (`UO_LA7_STEPS=8`) | 70 × 8 | 625 | 296 (47.4 %) | 5 | **0** |
| big (`UO_LA7_STEPS=60`) | 70 × 60 | 4,143 | 1,837 (44.3 %) | 127 | **0** |

Fallback reasons in the default run: Chains 69, Document 78, NoBase/Balancing 71/56 (balanced fixtures keep nothing),
Context 9, Shape 15, NoCheckpoint 9, Tables 18, PassInput 4. `incremental_path_is_taken`: on the generated 58-page
documents, 20/20 keystrokes in the middle are incremental with one page laid out again (PAGE only and PAGE+NUMPAGES), and
a 60-character insertion that adds a line is incremental with `line_shift ≠ 0` and equal bits.

**Fix round 1 (review of 2026-11-11).** Rerun of the default differential after the fixes: the same 625 relayouts,
296 incremental, 5 equal errors, **0 mismatches**, the same fallback counts. `incremental_path_is_taken` now fills its
timings: 20 keystrokes on the 58-page documents cost 255 ms incremental vs 13,625 ms full (PAGE only) and 492 ms vs
25,727 ms (PAGE+NUMPAGES), each including a fresh engine. New `lookahead_sites_are_traced`: (1) a page-anchored
floating table over the lines laid out before it (late in the next pass: `place_late_tables`), grown by Replace All and
restored by undo/redo, then a keystroke; (2) a 70-row page-spanning table and the paragraph after it with a
wrapping shape at −36 pt (`pin_floats_over_table`), its space before set to 24 pt (pinned) then 60 pt (not pinned) and
undone, then a cell entered, typed in, left, and two body keystrokes. Every relayout exact; the Replace All and both
space-before relayouts are incremental. **Detection checked**: with the `note_read` of `place_late_tables` removed the
test fails at the Replace All step ("late table step 4: canonical digest"); with the one of `pin_floats_over_table`
removed it fails at the 60 pt step ("pinned float step 4"); both restored, green.

**Fix round 2 (review of 2026-11-11: level 1).** `la7` release, 13 tests, `--test-threads=3`, 188.3 s (log
`target/m2-relayout/fix2-la7-release-nocap.log`; the run without `--nocapture` before it: 13 passed, 189.2 s):

- The three differentials (memo on, the default): the same 625 relayouts, 296 incremental, 5 equal errors, **0
  mismatches**; memo 69,897 hits, 909 replayed, 6,707 misses, 1,846 not memoizable, 0 over the cap. The relaid-pages
  contract (new, every incremental relayout): 2,808 pages outside `relaid_pages` checked equal to the previous
  relayout's (`Debug`; `item_meta` aside when `line_shift ≠ 0`).
- New `memo_only_equals_full` (level 2 off: `checkpoint_bytes: 0`, so every relayout is a full layout through the
  memo): every 2nd of the 70 documents × 8 steps, 315 relayouts, 0 errors, **0 mismatches**; memo 40,089 hits, 998
  replayed, 4,328 misses, 1,615 not memoizable; time 28.5 s against 40.0 s of fresh full layouts.
- New `memo_serves_structure_edits` (310 ¶, PAGE only and PAGE + NUMPAGES): Enter in the middle (Full, `Document`) 371
  hits of 375 prepares (743 of 748 with NUMPAGES: pass 2 hits pass 1's entries); a numbered list toggled (a new
  `formatting`: the memo starts again, 0 hits); Enter inside the list: 8 replayed (the items after it renumbered),
  364 hits; Backspace at an item start (the number goes, in place: level 2) 182 hits, 8 replayed; all exact.
- New `m5_memo_caps`: the 310-¶ document needs 14.2 MB; capped at a third (4.7 MB) the memo keeps 126 entries (4.72 MB),
  refuses 247 (`over_cap`), still serves 125 hits on the next Enter, exact; the memo off keeps and serves nothing.
- `incremental_path_is_taken` now checks the full criterion (`assert_same_layout`) on every keystroke (review finding):
  20/20 incremental, exact, PAGE only and PAGE + NUMPAGES.
- M1 `m1_relayouts_leave_nothing`: **0 B** left on the 5 documents (the memo is dropped by `clear_caches`). M2
  `m2_soak_is_flat`: 200/200 incremental, samples 20,341,993–20,344,153 B after the warm-up (flat; 2.9 MB more than
  without the memo: the 60-¶ document's entries), **0 B** after `clear_caches` and drop. No-panic: 140 relayouts, 0
  errors, 55.0 s.
- Debug size (CI's workspace job): `la7` 13 tests, 0 failures, 77.4 s (1 min 32 s with the build).
- Crate suite, release, without `--nocapture` (`cargo test -j 4 --release -p uo-docx-layout --no-fail-fast --
  --test-threads=3`, 13 min 5 s wall with the build): **25 binaries, 0 failures** — lib 8, adapter 1, f0 4, f1 19,
  f1_parity 4, f2_parity 3, f3 9, f3_facets 2, f4 4, f4_parity 2, f5 14, f5_parity 2, f6 4, f6_parity 2, f7a 7, f7b 7,
  f7c 6, f7d 6, f8 6, f8_parity 2, g4_cancel 6, g8 4, la7 13, layout_parity 5, doc-tests 0. The viewer/server path changed
  only by D-LA7-25 (the same function without a memo); the generated and oracle captures (b)–(d) were not rerun, nor the
  uo-wasm `viewer_switch` harness (uo-wasm is the WS frontier's this wave; the viewer calls `layout_document`, which
  passes no memo) — for the orchestrator at merge.
- **Merge (orchestrator, 2026-11-11).** Fix round 2 also touched the full path through hooks that are no-ops without a
  memo (`PrepFlow::warn`, a defaulted method; the `field_model_ref` extraction; the resolver `tap`/`log`). Rerun on the
  merged tree: same-run captures f7b 100 / 1,295 pages, f7c 100 / 588, f7d 100 / 306 — 300 facets equal each, strict;
  f8 1,500 equal (100/100); Word oracle P1 2,742/2,742 documents, 8,226 facets equal (the reviewer reran f7a: 300
  equal); the crate suite in release without `--nocapture`, 25 binaries green (la7 13); uo-wasm `viewer_switch --mode
  all` at stride 1: 74 documents / 579 pages, 0 differences in sizes, text, find, pixels (tolerance 0) and PDF, memory
  flat over 200 cycles. `last()` was fixed at merge (gate item 7).

**Test sizes (fix round 1).** CI's `cargo test --workspace --all-features` is a debug build: `la7` scales down under
`cfg!(debug_assertions)`; every bound has an env var (`UO_LA7_STEPS`, `UO_LA7_STRIDE`, `UO_LA7_BIG`, `UO_LA7_KEYS`,
`UO_LA7_SOAK`, `UO_LA7_NOPANIC_STEPS`).

| bound | release default | debug default |
|---|---|---|
| differential: documents × steps | 70 × 8 (stride 1, generated 310 ¶ ≈ 58 pages) | every 8th of 70 → 9 × 2 (generated 40 ¶ ≈ 8 pages) |
| `incremental_path_is_taken` | 310 ¶, 20 keystrokes | 40 ¶, 3 keystrokes |
| `lookahead_sites_are_traced` | 20 + 30 ¶ around the table, 70 rows | 8 + 8 ¶, 40 rows |
| M1 | f7a/f7c/f8 every 12th + big60 NUMPAGES | every 24th + big30 |
| M2 soak | 20 iterations on big60 | 4 iterations on big30 |
| M5 | big310, 6 steps per cap | big60, 2 steps |
| no-panic | 28 documents × 4 steps, bound 300 s | every 7th (4) × 2 steps, bound 600 s |

Measured (`--test-threads=3`, other agents compiling): release `la7` 10 tests in **159.7 s** (the three differential
tests 57.5 / 103.1 / 106.5 s in parallel; no-panic 49.9 s), wall 4 min 3 s with the build; debug `la7` **76.3 s**
(wall 2 min 4 s with `g4_cancel`). The plan's ≈ 60 s for the release run is not met: each differential step runs a
fresh full layout besides the relayout (the comparison is the test); `UO_LA7_STEPS=4` halves it. No-panic default
went from 8 to 4 steps per document (`UO_LA7_NOPANIC_STEPS=8` = the 252 relayouts of the first round).

**Parity of the full (viewer/server) path after the `run`/`finish` refactor** (STEP 11):

- (a) crate suite: the first round's claim "identical to the baseline, every binary green" was **wrong**: that run
  passed `--nocapture`, and without it `m2_soak_is_flat` failed every time (`left: 304`, an `eprintln!` inside the
  measured window grew libtest's capture buffer). Fix round 1 prints only after the measurement. Rerun without
  `--nocapture` (`cargo test -j 4 --release -p uo-docx-layout -- --test-threads=3`, 11 min 7 s wall with the build):
  **25 binaries, 0 failures** — lib 8, adapter 1, f0 4, f1 19, f1_parity 4, f2_parity 3, f3 9, f3_facets 2, f4 4,
  f4_parity 2, f5 14, f5_parity 2, f6 4, f6_parity 2, f7a 7, f7b 7, f7c 6, f7d 6, f8 6, f8_parity 2, g4_cancel 6,
  g8 4, la7 10, layout_parity 5, doc-tests 0. (`g4_cancel`, `la7` and the bench now share
  `tests/common/{open_fonts,docgen,hyph}.rs`; `hyph.rs` = `hyph_registry` + `hyph_opts`, fix round 1.)
- (b) same-run generated captures (`capture-layout.mjs --mode ci --count 100 --seed 11`, frozen TS `d0f45bc5`),
  strict replay: f7a 100 documents / 994 pages, f7b 100 / 1,295, f7c 100 / 588, f7d 100 / 306 — each **300 facets
  equal, 0 mismatches, 0 pending**.
- (c) f8 (`--gen f8 --f8`, `f8_parity`): **1,500 facets equal**, 100/100 documents, 0 mismatches (and the committed
  fixture 180/180).
- (d) Word oracle P1 (`--mode oracle`, installed fonts, docs from the frozen root): **2,742/2,742 documents equal**,
  8,226 facets, 0 mismatches, 0 pending (4,629 pages).
- The viewer harness (`uo-wasm` `viewer_switch.mjs`) was not run: uo-wasm is being edited by the WS frontier. The
  viewer calls `engine::layout_document`, whose output (b)–(d) prove unchanged; the orchestrator runs it at merge.
- Fix round 1 changed the engine only by two `note_read` calls, no-ops without a trace (the full path never has one),
  so (b)–(d) were not rerun. Their captures, dumps and documents (15 GB under `target/m2-relayout/cap`) were deleted
  to free the disk; the capture and replay logs are kept there and the data is regenerated by the commands above.

**Memory.**

- M1 (`m1_relayouts_leave_nothing`): a session, a relayout, five edits with relayouts (one adds a line),
  `clear_caches`, drop → **0 B** left on 5 documents (f7a ×2, f7c, f8, a 60-paragraph NUMPAGES document); allocated
  57–488 MB, peak 2.6–12.4 MB above start.
- M2 (`m2_soak_is_flat`, default 20 iterations; `UO_LA7_SOAK` for more): 10 typed edits with a relayout each (200/200
  incremental), every 5th iteration digest-compared with a full layout, all undone and a relayout; samples 17.42 MB
  ± 1 KB after the warm-up (flat), **0 B** after `clear_caches` and drop. Big run (`UO_LA7_SOAK=200`): 2,000/2,000 incremental relayouts, samples 17.419–17.421 MB after the warm-up, 0 B after `clear_caches`.
  Fix round 1, with libtest's output capture on (no `--nocapture`): release 17,417,822–17,419,982 B after the warm-up,
  0 B left; debug (4 iterations, big30) 14,344,201–14,345,095 B, 0 B left.
- M5 (`m5_caps`): a cap of `pages + checkpoints/3` keeps a checkpoint every 4 pages (15 checkpoints, estimate under the
  cap) and is still exact and incremental; a cap below the pages keeps nothing (`Memory` every relayout); `enabled:
  false` → `Disabled` every relayout. All exact.
- No-panic (`no_panic_seeded_steps`): 28 documents (f7c notes/headers, f7d chains/endnote tables) × 8 steps = 252
  relayouts, 0 panics, error kinds equal, 78.5 s alone, 133 s next to the 60-step run (a 300 s hang bound; the plan's 30 s is not met on these
  fixtures: every step also runs a full layout). Fix round 1 default (4 steps): 140 relayouts, 0 errors, 49.9 s
  release; debug 4 documents × 2 steps, 39.8 s.

### Speed (release, native, this machine; `examples/bench_relayout.rs`, caret in the middle paragraph)

| document | pages | full layout | recording relayout | type 1 char (median / p90) | ratio | pages re-laid | 60-char insertion | Enter |
|---|---|---|---|---|---|---|---|---|
| pages50 (gen_docs seed 16, 310 ¶) | 49 | 262 ms | 231 ms | **5.3 / 5.7 ms** | 2.0 % | 1 | 5.8 ms (incremental, line added) | 233 ms (full, `Document`) |
| pages500 | 500 | 2,459 ms | 2,569 ms | **7.9 / 11.0 ms** | 0.32 % | 1 | 8.3 ms | 2,409 ms (full) |
| gen310 PAGE+NUMPAGES (2 passes) | 58 | 591 ms | 586 ms | **11.4 / 12.0 ms** | 1.9 % | 1 | 738 ms (full, `PassInput`: the page count changed) | 586 ms |
| gen3130 PAGE+NUMPAGES | 581 | 6,152 ms | 6,951 ms | **13.1 / 19.0 ms** | 0.21 % | 1 | (before the line-shift support: 8.5 s, `Tables`) | 6,812 ms |

The target (one character ≤ 5 % of the full layout on 500 pages, ≤ 3 pages re-laid) is met: 0.32 % and 1 page. The
keystroke after an Enter or an insertion is incremental again (4.7–13 ms). Second run (nothing else of this frontier running): pages50 type 1 char 5.19 / 5.64 ms vs 232 ms (2.2 %), insertion 5.6 ms, Enter 232 ms; pages500 8.02 / 8.59 ms vs 2,399 ms (0.33 %), insertion 8.7 ms, Enter 2,449 ms; recording 230 / 2,387 ms vs full 230 / 2,401 ms (the recording costs nothing measurable).

**Fix round 2 (level 1)** (`examples/bench_relayout.rs`, same machine, log `target/m2-relayout/fix2-bench.jsonl`):

| document | full layout | Enter before (level 2 only) | **Enter with the memo** | memo hits of the Enter | type 1 char (median / p90) | recording relayout |
|---|---|---|---|---|---|---|
| pages50 (49 pages) | 232 ms | 233 ms | **115 ms** (49.6 %) | 329 (6 misses) | 3.13 / 3.51 ms (was 5.3) | 243 ms |
| pages500 (500 pages) | 2,427 ms | 2,409 ms | **1,208 ms** (49.8 %) | 3,363 (5 misses) | 5.74 / 6.32 ms (was 7.9) | 2,506 ms |
| gen310 PAGE+NUMPAGES (2 passes, 58 pages) | 599 ms | 586 ms | **262 ms** (43.7 %) | 743 | 6.39 / 6.58 ms | 443 ms (pass 2 from pass 1's entries) |
| gen3130 PAGE+NUMPAGES (581 pages) | 6,126 ms | 6,812 ms | **2,787 ms** (45.5 %) | 7,511 | 8.65 / 9.34 ms | 4,541 ms |

The rest of a full layout (page flow, lines, paint, derive, header/footer paragraphs) is what an Enter still costs; the
level-2 key remap (gate) would make it a resumed run. Keystrokes got faster too (the paragraphs of the re-laid page are
hits). The 60-character insertion on gen310 NUMPAGES changes the page count: `PassInput`, 333 ms (was 738).

### Memory kept (measured with `uo-alloc` in the bench; estimates in the report)

| document | `Layout` (measured) | kept besides the `Layout` | × `Layout` | checkpoints (est.) | kept pages (est.) |
|---|---|---|---|---|---|
| pages50 | 8.8 MB | 10.0 MB | 1.14 | 1.0 MB (49) | 8.6 MB |
| pages500 | 90.5 MB | 102.8 MB | 1.14 | 11.1 MB (500) | 87.9 MB |
| gen310 NUMPAGES | 12.5 MB | 23.8 MB | 1.90 | 2.8 MB (116) | 20.4 MB (2 passes) |
| gen3130 NUMPAGES | 128.0 MB | 240.7 MB | 1.88 | 29.0 MB (1,162) | 205.6 MB (2 passes) |

The kept `PageState`s of every recorded pass dominate (≈ 1× the `Layout` per pass); checkpoints are ≈ 12 % of one pass.

The paragraph memo (fix round 2; measured = kept bytes with the memo − without it, same document and relayout):

| document | `Layout` | memo measured | memo estimate (report) | × `Layout` | entries | all kept (both levels) × `Layout` |
|---|---|---|---|---|---|---|
| pages50 | 8.8 MB | 11.9 MB | 10.2 MB | 1.35 | 331 | 30.6 MB, 3.49 |
| pages500 | 90.5 MB | 122.4 MB | 105.6 MB | 1.35 | 3,365 | 315.6 MB, 3.49 |
| gen310 NUMPAGES | 12.5 MB | 15.3 MB | 14.2 MB | 1.23 | 374 | 51.6 MB, 4.13 |
| gen3130 NUMPAGES | 128.0 MB | 154.8 MB | 143.8 MB | 1.21 | 3,758 | 523.4 MB, 4.09 |

Before the glyph packing the memo of pages50 measured 49 MB (5.6× the `Layout`; a `Glyph` is ≈ 200 bytes). The estimate
is 14 % under the measure (allocator rounding, the node maps). With the wasm32 default (64 MiB) pages500 keeps the first
≈ 60 % of its paragraphs; the native default (384 MiB) keeps every document measured.
With the wasm32 default cap (96 MiB) pages500 still fits (88 MB of pages + 11 MB of checkpoints, every page); gen3130 keeps nothing
(`Memory`: 206 MB of pages) and every relayout is full — decision 5 below.

### Not done (handed on)

- **Level 1's line memo** (D-LA7-26: 6 % of a full layout; proposed to drop) and the memo for cells, headers/footers,
  notes and textboxes (D-LA7-18). An Enter costs half a full layout with the memo; the level-2 key remap (LParaId/RunKey/
  block-index shift of the kept checkpoints and pages, including the `LineRef` paths `b<i>` of the output) would make it
  a resumed run — a follow-up for the gate.
- No injection test of the memo-check fallback (D-LA7-21).
- A changed page count in a NUMPAGES/SECTIONPAGES/PAGEREF document changes the next pass's input: full (`PassInput`).
  Running only the remaining passes fully would halve it.
- Edits in context stories (cells entered as a context, notes, headers, textboxes) are full (`Context`); a header or note
  edit would need the hf/note stories in the change detection.
- Copies of `OpenFonts` (5 left in tests), `hyph_registry`/`opts` (in f4, f4_parity, f7a–f7d, f8, f8_parity, g8,
  layout_parity) and `docx()` builders not consolidated beyond g4_cancel, la7 and the bench (declared follow-up).
- The plan's no-panic bound of 30 s (49.9 s at the new 4-step default, release).

### For the milestone-2 gate

1. Decision 5 — memory budget of the incremental state: measured 1.14× the `Layout` for one-pass documents, 1.9× for
   two-pass ones (every pass's pages are kept). Options: keep only the final pass's pages and replay non-final passes from
   their checkpoints ("lite" retention), lower the wasm32/phone cap, or accept.
2. D-LA7-17 is closed (level 1 implemented). Still open: the level-2 key remap for structure edits (Enter now costs
   ≈ 50 % of a full layout through the memo), and dropping the line memo (D-LA7-26).
3. The memo's memory (1.2–1.35× the `Layout`, measured) and its wasm32 default cap (64 MiB: pages500 keeps ≈ 60 % of its
   paragraphs); with level 2 the incremental state keeps 3.5–4.1× the `Layout` in all.
4. A fixed `clock` between relayouts in the editor shell (D-LA7-24, WS frontier).
5. Rust full == TS full on edited documents (LA7 last bullet): a bounded `capture-layout.mjs --mode files` run over the
   saved states of the differential scripts with CI font isolation.
6. A corpus-wide differential (oracle documents + edit scripts) — the reads traced are the ones the 70 test documents
   exercise; a missed read site would show as a mismatch only on a document that has it.
7. The WS frontier's use: repaint `relaid_pages`. (`last()` with `layout()` interleaved was fixed at merge: the
   engine remembers whether the newest attempt was a relayout, so a failed `layout()` after a relayout gives `None`, as
   in milestone 1.)
8. The memo's scope (D-LA7-18): header/footer, cell, note and textbox paragraphs are not memoized; header/footer
   paragraphs are 245 of 579 prepares (42 %) on pages50 — the main reason an Enter still costs ≈ 50 % of a full layout.

### Requests to other modules

- None required. (`uo_docx::model::{Document, TableAnchor}` are re-exported by `src/model.rs`, additive.)
- For the orchestrator (fix round 1, not done here: `.github/workflows/rust-core.yml` is not this frontier's file): the
  workspace debug job now runs `la7` at its debug size (76 s); a release line in the layout-parity job, e.g.
  `cargo test --release -p uo-docx-layout --test la7 -- --test-threads=3`, would run the full-size differential
  (160 s) on every push, if wanted.

### Process metrics

| Step | Wall time | Notes |
|---|---|---|
| baseline suite | 8 min 15 s | all green |
| design + instrumentation + level 2 (single pass) | — | first differential run: 0 mismatches |
| line-id relabel (d ≠ 0) | — | pages50 60-char insertion 128 ms → 5.8 ms |
| caps (M5) | — | one rework: the cap was applied only after recorded calls (an incremental relayout recorded a checkpoint per page and passed the cap); now `fit_cap` after every relayout, distance-based thinning |
| crate suite after the refactor | 4 min 22 s | reported "identical to the baseline": wrong, it ran with `--nocapture` (fix round 1) |
| parity reruns (b)–(d) | ≈ 45 min | two script reruns (below) |
| differential 60 steps + soak 200 | 10 min | 0 mismatches |
| fix round 1: fixes + new test | — | two untraced lookahead sites, M2 capture artefact, debug sizes, `Failed` fallback, context base, shared helpers, bench JSON |
| fix round 1: detection check | ≈ 6 min | each `note_read` removed in turn: the new test fails; restored |
| fix round 1: crate suite (release, no `--nocapture`) | 11 min 7 s | 25 binaries, 0 failures |
| fix round 1: `la7` release / debug | 4 min 3 s / 2 min 4 s | 159.7 s / 76.3 s of tests |
| fix round 1: `check-changed.mjs` | ≈ 3 min | one rework (clippy `large_enum_variant` on the test's `Step`); then all steps passed |
| fix round 2: profile (temporary counters) | ≈ 3 min | prepare 51 %, line breaking 6 % of a full layout (D-LA7-26) |
| fix round 2: level 1 (memo, effect log, replay, caps) + tests | — | one rework: unpacked glyphs made the memo 5.6× the `Layout`; packed `Core` glyphs, 1.35× |
| fix round 2: `la7` release | 3 min 9 s / 3 min 8 s | 13 tests, 0 failures (with and without `--nocapture`) |
| fix round 2: bench (4 documents) | 2 min 15 s | |
| fix round 2: `check-changed.mjs` | 59 s | all steps passed |
| fix round 2: crate suite (release) / `la7` debug | 13 min 5 s / 1 min 32 s | 25 binaries, 0 failures / 13 tests, 0 failures |

Rework: two parity-script bugs of this frontier (the bash builtin `GROUPS` shadowed the script variable, so the first
capture ran a default generator; relative capture paths failed in `cargo test`, whose working directory is the crate);
both reruns are the numbers above. A test-file import (`Mutex` in `g4_cancel.rs` after the helpers moved) failed the first
suite rerun. Level 1 was not started in the first round (it was implemented in fix round 2). Fix round 1 (review of 2026-11-11): the review found the M2 soak failing under
output capture (the first suite run used `--nocapture`), the release-sized `la7` in the debug CI job, two lookahead sites
missing from the trace (`pin_floats_over_table`, `place_late_tables`; found by reading, then reproduced by the new
test), the level-1 scope gap (now the open decision D-LA7-17), `Err` of an attempt failing the call (D-LA7-15), the
context base (D-LA7-16), duplicated test helpers, invalid bench JSON (`None`/`Some(…)`; now `null`/numbers/strings,
validated with `JSON.parse` on a `--gen 60` run) and 15 GB of captures left in the target dir (deleted).

## Wave 7c — W: Word-measured rules over the frozen TS (2026-11-13)

Word 16 pt-BR is the authority where it differs from the frozen TS ("Word manda sobre o TS"). Each rule below was measured
with a Word oracle series (`write/tools/layout-oracle/cases/<group>.mjs`, Liberation Sans, Word under invisible COM through
`tools/parallel/word-lock.mjs`; the expectations in `core/crates/uo-docx-layout/tests/oracle_w7c/<group>/*.word.json`, kept out of the frozen TS's 2,742-document corpus that the uo-docx parity tests replay) and is
checked by `tests/word_w7c.rs` (a port of the TS `oracle-compare.ts`: pages, line texts, the page of each line, baseline
±0.3 pt, x ±0.6 pt where the group measures it). The frozen TS behaviour stays behind a `LayoutOptions` switch;
`LayoutOptions::ts_strict()` turns every switch on and is what every replay of TS captures uses (`layout_parity`,
`f8_parity`, `f4_parity`, `f6_parity`, `g8`, uo-ffi `server_parity`, the wasm `wasmOptions` with flag bits 12–17).
`word_w7c` also asserts, case by case, where `ts_strict()` differs from Word (`ts_diverges`), so each divergence stays
documented.

| Rule (switch) | Word | Frozen TS | Where |
|---|---|---|---|
| keepLines taller than what is left (`ts_keep_lines_within_page`) | the paragraph goes to the next page even when taller than a page, then splits; in mode 15 in a section of several columns a keepLines paragraph taller than the column goes to the next PAGE and each of its columns ends with a page break (at the top of a page too; mode 14: the next column); in a table cell (mode 15) a keepLines paragraph crossed by the row's split goes down whole (not at the top of a page; mode 14 ignores it) | moves only when `need <= full`; cells and columns: no rule | `engine/flow.rs` (place_paragraph), `engine/page.rs` `break_column` (`Engine::kl_col_to_page`, reset at the paragraph's end), `table.rs` `keep_cut` + `ParaSplitInfo::keep`, `RowSplit::split_keep` |
| no `word/settings.xml` (`ts_missing_settings_mode15`) | compatibility mode 12 (COM `CompatibilityMode` = 12); a missing `w:defaultTabStop` = 708 tw (COM `DefaultTabStop` 35.4 pt) | mode 15; 720 tw | uo-docx `ReadFacts` (outside the model's JS shape and equality), `LDoc::compat_mode()`/`default_tab_stop()` at every read site (flow, floats, tables, static flow, lines, hyphenation, para, table layout) |
| linked text boxes (`ts_linked_box_cell_chain`) | a box of a chain is never left empty: when widows/orphans (or its height) would send everything on, it keeps the lines that fit without the widow rule, at least the first line; mode 15 wraps a cell's text around a `layoutInCell="0"` box too | box 1 empty, the chain starts in box 2; no wrap | `engine/chain.rs` `keep_first_lines`, `engine/floats.rs` |
| REF/PAGEREF/NOTEREF to an undefined bookmark (`ts_ref_bookmarks`) | missing, without `w:bookmarkEnd` or ending before it starts: REF (every switch) "Erro! Fonte de referência não encontrada.", PAGEREF/NOTEREF "Erro! Indicador não definido." — in BOLD (also STYLEREF's error); a range over several paragraphs = all of them (line breaks); a bookmark in a cell, a text box or a header = its range's text (with the bookmark marks of a lossless read) | the start paragraph's text, the page of its paragraph, the cached result | `engine/field_refs.rs` (`bookmark_where`, `ref_error`, `bookmark_error`, `is_ref_error_text`), `engine/fields.rs` PAGEREF, `para.rs` `DRun::bold` |

Open (not applied): the theme tint/shade rounding (`ts_theme_tint` reserved: no model reproduces Word's 3,356 samples in
`tests/oracle_w7c/tint/word-samples.tsv` — the TS rule hits 2,323, pinned by `word_w7c` `tint_shade_samples_against_the_ts_rule`)
and the hyphenation patterns (`ts_hyphenation_patterns` reserved; the `hyphword` measurement: line splits equal to Word in
pt 877/900, en 809/900, es 898/900 paragraphs) — uo-bindings.md "Wave 7c — W". The .NET port
(`UniversalOffice.Docx/DocxLayout`) has none of these rules: to be ported with the parity dumps regenerated.

## WordCraft study — tables

Implementer of the plan `core/target/wordcraft-study/PLAN-tables.md` (2026-11-13; items B5, B6, B12, B15 rules A and B,
the TCond aside; the placement half of B10/B11 is the page area's, reviewed below). Word 365 pt-BR (16.0) decides where it
differs from the frozen TS. Every rule was measured with Word (invisible COM through `tools/parallel/word-lock.mjs`, case
generators `write/tools/layout-oracle/cases/wctab*.mjs` and the B study's `wcb*.mjs`, Liberation Sans only) and is checked by
`tests/word_wc_tables.rs` against the fixtures in `tests/oracle_wc/<group>` (pages, line texts, the page of each line,
baseline ±0.3 pt, x ±0.6 pt where the group measures it, and for the border groups a RULES comparer: Word's vertical and
horizontal paths of the PDF against the layout's border lines, collinear pieces joined, centre ±0.3 pt, thickness
±0.15 pt). Each rule's frozen-TS behaviour stays behind a `LayoutOptions` switch, set by `ts_strict()` (therefore by every
strict replay: `layout_parity`, `f8_parity`, `f4_parity`, `f6_parity`, `g8`, uo-ffi `server_parity`) and, in wasm, by the
ONE area bit 20 `LAYOUT_TS_WC_TABLES` (`write.rs`; JS `tsWcTables`, set by the harnesses' `wasmOptions`;
`uo-wasm/tests/wc_tables_native.rs`). `word_wc_tables` asserts, per group, the cases where `ts_strict()` differs from Word.

| Rule (switch) | Word (measured) | Frozen TS | Where | Group |
|---|---|---|---|---|
| B5 — a table row kept with the next (`ts_row_keep_any_paragraph`) | only the FIRST paragraph, in document order, of the row's first cell (smallest logical column) decides, its keepNext direct or from its style (an empty paragraph too); 2nd/3rd paragraphs and other cells do not; gridBefore: the first `w:tc`; a vMerge continuation by its own paragraph (the restart's keepNext does not carry); a nested table first in the cell: the first paragraph inside it; modes 15 and 14 alike | keepNext in any paragraph of any cell | `table/layout.rs` row loop (`row_keep_next`, `cell_first_keep_next` for a cell without content), `CellContent::first_keep_next` from `engine/static_layout.rs` | `rowkeep` 26 (12 TS-divergent) |
| B6 — conditional table-style borders (`ts_table_style_cell_borders`) | a part's `w:tcBorders` are its REGION's: a cell takes left/right/top/bottom on the region's edges and insideV/insideH inside it (firstRow = row 0 + the contiguous `tblHeader` rows × all columns; lastRow; firstCol/lastCol × all rows; band1/2Horz = each band of `tblStyleRowBandSize` rows; band1/2Vert = each band of `tblStyleColBandSize` columns; corner parts = the cell); wholeTable `w:tcBorders` are ignored; the conflict rules are unchanged (heavier wins between cells, the conditional cell border beats the table's insideV). Found while implementing (`bb-*`, measured): the look's firstRow (firstColumn) bit shifts the row (column) bands only when the style has a NON-EMPTY firstRow (firstCol) part, and lastRow/lastColumn keep the last row/column out of the bands only with a lastRow/lastCol part | each part's four sides to every cell of the part, insideH/insideV never read, wholeTable's applied; bands shifted by the look bits alone | `table/layout.rs` (`region_tc`, `row_has`/`col_has`, `has_part`, `cond_for`'s band offsets) | `condbd` 37 (31) |
| B12 — a floating table's wrap edges (`ts_float_table_wrap_edge`) | mode 15: right = grid right + the right border's width (15 tw without one) + max(rightFromText, 10 tw); left = the table's x − max(leftFromText, 10 tw); a table aligned `right`/`outside` is placed by that same right edge (grid + right border, or + 15 tw). Mode ≤ 14: from the painted outer left edge (half the left border before the grid, 15 tw without one) − max(lft, 10 tw) to the painted outer right edge (half the right border past the grid, 15 tw without one) + max(rft, 10 tw). Bottom unchanged; the 359.5-tw minimum strip counts from the new edges | the grid ± fromText (mode ≤ 14: ± half the left border) | `engine/tables.rs` `float_table_wrap_box`, `right_edge_w`, `emit_float_rows`, `static_table` (a floating table in a cell), `place_floating_table` (right alignment); `TableLayout::border_right` | `ftedge` 102 (48) |
| B15 rule A — wrapping objects in a cell (`ts_cell_float_no_clamp`) | a wrapping anchored object (square, tight, through, topAndBottom; behindDoc with wrapping) laid out in a cell is clamped horizontally into the cell's INTERIOR — between the inner edges of its painted left and right borders (the painter's: the heavier border of a boundary, centred on it), left first (a wider object overflows on the right); wrapNone is not clamped; no vertical clamp | where the offset puts it | `engine/floats.rs` `place_float` (cell branch), `Engine::cell_interior` from `CellEnv::cell_interior` (`table/layout.rs`) | `cellclamp` 110 (73; 8 open) |
| B15 rule B — a word that does not fit the strip beside a float (`ts_gap_word_any_mode`) | mode 15: the line goes below the float, in the body and in table cells; mode ≤ 14: the word is split by characters into the strip, in cells and in the body | body: below in every mode; cells: split in every mode | `engine/flow.rs` (`gapind` block, mode 15 only), `engine/static_layout.rs` (the same block for cell floats), `engine/floats.rs` `gap_word_cut` | `cellclamp` `fw-*` |
| the TCond aside — `w:tblStyle` order (`ts_tbl_style_order`) | Word reads `w:tblPr` in order: a `w:tblStyle` naming an EXISTING table style after other children discards every direct property before it (tblW, jc, tblBorders, tblCellMar, tblpPr…); those after it are kept; a missing style discards nothing; modes 14 and 15 | every child read | uo-docx `read::props::read_tbl_pr_word` → the hidden `Table::word_tbl_pr` (outside the JS shape, never written, dropped by every table-property edit of the session: `table_ops::tbl_pr_of`; a split keeps it on both parts: `split_table`); uo-docx-layout `table::table_props` at every read of the direct table properties (`layout_table`, `measure_cell`, the floating-table reads of `engine/tables.rs`) | `tblorder` 21 (8); uo-docx `tests/tblpr_word_unit.rs` (3 tests, a module of the one test binary `tests/all.rs` since the fix round) |

Also checked: `light_grid_keeps_its_borders_under_the_region_rule` (a built-in style with firstRow insideV: the region rule
paints what the per-cell rule painted). Opt-in: `UO_WC_ORACLE=<frozen TS oracle dir>` runs
`tables_rules_agreement_on_the_oracle` (installed fonts; `ts_strict()` against `ts_strict()` with the six tables switches
at Word's rules — the area's own effect, whatever the other areas do); `UO_WC_ATTR=<dir>` runs
`attribute_documents_to_the_tables_rules` (which switch changes which pages of a document); `UO_WC_ADHOC=<dir of .docx +
.word.json>` runs `adhoc_documents_against_word_by_switch` (each document against Word's PDF with `ts_strict()` and with
each switch that changes it at Word's rule; `UO_WC_ADHOC_PRODUCT=1` also the product and the product with that switch at
the TS's; per switch the "Δlines": the line texts only one layout has, and how many of them Word's PDF has);
`UO_WC_FACETS=<dir>` runs `predict_viewer_facets_of_the_tables_rules` (the viewer harness facets and pages a switch
changes). Word agreement on the oracle, re-run on the fix round's tree (2026-11-14, `core/target/wc-tables/agree2.log`):
2,741 cases with a Word PDF, 46 change under the tables rules, Word-equal among them 46 with the TS rules and 46 with
Word's (none worse).

#### Harness documents (fix round 1)

Attribution over every committed capture (`captures/*/docs`), the uo-docx d3 fixtures and `tests/fidelity`, with the
viewer's read and the editor's (`UO_WC_ATTR_SOURCES=1`) alike: 13 documents, each changed by ONE switch — row keep:
`f7b__7__11` (page index 2), `f7b__7__5` (15 → 14 pages), `f7b__7__6` (6–8), `f8__7__1` (14–15), `f8__7__5`
(16 → 15 pages); cell clamp: `f7b__7__2` (3); floating-table edges: `f7b__7__3` (3), `f8__7__3` (3); gap word:
`f7b__7__4` (1–2), `f7b__7__7` (8–15), `f7b__7__9` (21–22), `f8__7__7` (33 → 34 pages), `f8__7__9` (22–23). Each was
exported by Word as it is (`write/tools/layout-oracle/cases/wctab-attr.mjs`, `--fixdir core/target/wc-tables/adhoc`;
five `.word.json` are the page area's exports of the same bytes) and compared with the installed fonts. These fuzz
documents agree with Word on 52–75 % of their lines under either rule set, so whole-document counts move both ways;
Word-matched lines / same-page lines, `ts_strict()` → the switch, and the Δlines (switch-only lines, of them Word's;
TS-only lines, of them Word's):

| Document (switch) | Pages (Word) | Lines matched of Word's | Same page | Δlines switch / TS |
|---|---|---|---|---|
| `f7b__7__11` (row keep) | 7 → 7 (6) | 254 → 264 of 345 | 250 → 260 | 12 (10) / 0 |
| `f7b__7__5` (row keep) | 15 → 14 (15) | 785 → 898 of 1,307 | 529 → 842 | 25 (15) / 2 (2) |
| `f7b__7__6` (row keep) | 13 → 13 (14) | 540 → 540 of 870 | 208 → 206 | 0 / 0 |
| `f8__7__1` (row keep) | 16 → 16 (16) | 754 → 754 of 1,364 | 671 → 656 | 0 / 0 |
| `f8__7__5` (row keep) | 16 → 15 (16) | 866 → 869 of 1,403 | 814 → 814 | 0 / 0 |
| `f7b__7__2` (cell clamp) | 9 → 9 (11) | 587 → 587 of 803 | 404 → 404 | 0 / 0 |
| `f7b__7__3` (table edges) | 4 → 4 (5) | 143 → 143 of 179 | 79 → 79 | 0 / 0 |
| `f8__7__3` (table edges) | 4 → 4 (5) | 159 → 159 of 192 | 87 → 87 | 0 / 0 |
| `f7b__7__4` (gap word) | 7 → 7 (8) | 355 → 356 of 534 | 308 → 309 | 1 (1) / 2 (0) |
| `f7b__7__7` (gap word) | 32 → 32 (30) | 1,999 → 1,999 of 2,728 | 159 → 154 | 3 (1) / 5 (2) |
| `f7b__7__9` (gap word) | 23 → 23 (26) | 930 → 934 of 2,194 | 285 → 285 | 4 (4) / 9 (4) |
| `f8__7__7` (gap word) | 33 → 34 (33) | 2,062 → 2,030 of 2,936 | 790 → 675 | 81 (66) / 6 (2) |
| `f8__7__9` (gap word) | 24 → 24 (27) | 848 → 863 of 2,319 | 277 → 277 | 7 (7) / 16 (9) |

(The product — every area at Word's rules — gives the same numbers as the single switch on all 13, except `f7b__7__3`
and `f8__7__3`, where the other areas' rules raise the lines; the product with the one switch back at the TS's equals
`ts_strict()` there.) Where a document got worse, the rule was measured again IN THE DOCUMENT:
- Row keep (`f7b__7__6`, `f8__7__1` same page; `f7b__7__5`, `f8__7__5` page count): Word exported copies with the keepNext
  the rule ignores removed (`core/target/wc-tables/tools/b5_variant.py`, case module `wctab-b5.mjs`, `--fixdir
  core/target/wc-tables/b5`). Removing every cell paragraph's keepNext except the first paragraph's of each row's first
  cell: Word's PDF is IDENTICAL to the original's (every line, baseline and x) on `f7b__7__11`, `f7b__7__5`, `f7b__7__6`,
  `f8__7__1`; on `f8__7__5` two lines move (pages 5–6) — a keepNext between two paragraphs of ONE cell keeps them
  together when Word splits the row (follow-up T-2 in `QUEUE.md`, not B5): the copy that removes only each cell's LAST
  paragraph's keepNext is identical to the original on all five. So in these documents Word ignores exactly the keepNext
  the rule ignores; the losses are where the rows land after other divergences (the documents' line agreement).
- Gap word (`f7b__7__7` same page, `f8__7__7` pages/lines): at the place the switch acts, the TS splits "jumps"/"objeto"
  by characters into the strip beside the float (`jum`/`ps`/`obj`, x 282.4 pt); Word's PDF has no such lines and sets
  the words at the cell's left edge below the float (`f7b__7__7` p8: `jumpsobjetoflutuante`, `nadebrown` at x 225.4;
  `f8__7__7` p9: `jumpsobjeto`, `flutuantenadebrown`, `informacaojumps` at x 223.8) — the switch's lines (`jumpsobjeto`
  at x 219.3). The extra page of `f8__7__7` follows from the rows below, which already differ from Word.

Harness declarations (`id` "Word manda sobre o TS (WordCraft tables …)"): `viewer_switch.mjs` `WC.tables` (until
wordcraft-adopt `WC_TABLES`) — the 13 documents, facets and pages as `predict_viewer_facets_of_the_tables_rules`
predicts them (scoped `textLayer`/`pixels`;
joined with an earlier declaration of the same document, if any); `editor_switch.mjs` DECLARED — the 13 (the scripted
clicks land on any paragraph: every facet from step 0; `pagesAfterLoad` on `f7b__7__5`, `f8__7__5`, `f8__7__7`, whose
page count the rule changes although Word's count is the TS's — see above; `f8__7__5` keeps ED-108 in the same entry);
`editor_switch_stories.mjs` — `T2 f7b__7__4/5/6/7/9`, `X1 f8__7__3`, `X1 f8__7__5`, `X2 f8__7__5` (with ED-108), and the
harness now accepts `pagesAfterLoad` like `editor_switch.mjs`; `editor_switch_objects.mjs` — `f8__7__5|objects` (`pages`
at every step, `pages after load`, end pages/thumbnails/screen: the facets left in the page area's run with its own
switches on, `core/target/wc-page/diffs-attr-objects.json`); `editor_switch_completion.mjs` — `f7b__7__3|objects` end
thumbnails. The fix round did not run the browser harnesses; the wordcraft-adopt leva ran them and narrowed the viewer
facets to the measured ones ("WordCraft study — harness declarations" below).

Open (not applied, asserted to still differ in `word_wc_tables` `cellclamp.open`; a NEW divergence outside the plan,
not a cell rule): the `lic-tight-*`/`lic-through-*` box has no fill and no outline, and Word's tight/through wrap of
such an INVISIBLE shape blocks only the lines beside the box's own text (its first line), in the body as in a cell, with
`wrapPolygon edited` 0 or 1 — lines lower down, fully inside the box's height, run past its left edge (measured
2026-11-14: `write/tools/layout-oracle/cases/wctabtt.mjs`, box offset swept 6–20 pt, square × tight × through, cell ×
body; `wctabtt2.mjs`: the same box filled, outlined, or filled without outline wraps like a square box). Both engines
wrap around the whole polygon; the rule needs its own measurement (which outline Word derives from the visible content)
and its own switch — queued as follow-up T-1 (`core/target/wordcraft-study/QUEUE.md`, "Follow-ups — tables": measure
the outline Word derives from the visible content, `wctabtt*`; a `ts_*` switch; then `cellclamp.open` empties). The
clamp of tight/through boxes is Word's: the box x is equal in `lic-*`, and the filled copies
`licf-*` (`wctabtt3.mjs`, modes 15/14, left/right) and the `t2-cell-*` cases (filled / outlined / filled without
outline) are equal by whole lines (`cellclamp` now 110 cases, 73 TS-divergent, 8 open). Unmeasured
and kept as implemented: a hidden first paragraph or a vertical-text first cell (B5: like any paragraph); a nested table
first in a vMerge continuation (B5: its first cell without the nested style's pPr); diagonal borders in parts and regions
broken by gridSpan (B6: per logical grid); the band rule for `lastRow` (inferred from the measured `lastCol`); a
`center`-aligned floating table (B12: by the grid, as before); mixed border widths per row (B12: the widest right
border); rule B in text boxes and headers/footers (unchanged); `w:tblPrEx` order (unchanged).

B10/B11 placement half (implemented by the page area: `ts_float_anchor_first_line`, `ts_float_no_reflow`), reviewed
against the plan's item 3: a line-relative float measures from its anchor line's top in the body only (`float_line_top`);
the cell branch of `place_float` keeps the paragraph top and ignores the pins (relativeFrom="line" in cells is not
measured), so the B15 clamp and the B10/B11 pins never meet. The tables planner's own seven B10/B11 cases
(`wctabfl.mjs`: `fl-b10-cols`, `fl-b11-push-38/44`, `fl-line-same-l1/l5/l5-20`, `fl-para-same-l5`) are the native
group `fltanchor` of `word_wc_tables.rs` (fix round 1): all seven equal Word under Word's rules, five differ under
`ts_strict()` (`fl-b10-cols`, `fl-b11-push-38`, `fl-b11-push-44`, `fl-line-same-l5`, `fl-line-same-l5-20`).

.NET port need (`UniversalOffice.Docx/DocxLayout`, reader in `UniversalOffice.Docx`; NOT changed here): the six rules
above — the row keep by the first cell, `region_tc` + the band offsets, the floating-table edges and right alignment, the
cell clamp, the mode-dependent gap word, `read_tbl_pr_word` + the layout accessor — then the parity dumps regenerated
(`tools/gen-parity-layout.mjs`).

## WordCraft study — fields

Fields, lists, font names and the English hyphenation data (plan `core/target/wordcraft-study/PLAN-fields.md`, verdicts
`VERDICTS.json` B/CD, measurements under `wordcraft-study/B`, `fh`, `fields`). Word 365 for Windows (pt-BR UI) decides:
every rule below follows Word; the frozen TS's behaviour stays behind one `LayoutOptions::ts_*` switch per item (default
`false` = Word), which `ts_strict()` turns on for the strict replays (`layout_parity`, `f4/f6/f8_parity`, `g8`,
`fidelity_w7`, `toc_pages_w7`, uo-ffi `server_parity`). WASM: bit 21 `LAYOUT_TS_WC_FIELDS` (`tsWcFields` in
`js/write.mjs`, `paint-common.mjs` `wasmOptions`) turns every switch of this area on; D1 is bit 16's
`ts_hyphenation_patterns`. Session switches: `OpenOptions::ts_formula_errors`, `OpenOptions::ts_numbering` (kept in
`SessionOptions`). uo-wasm `tests/wc_fields_native.rs`: bit 21 changes the painted pages of one case per
document-level switch (`styleref`, `fields22`, `sections23`, `refs24`, `formula25`, `lists26`, `lists27`, C1
`c1-symbul-108`, C1b `c1y-courier-150`) and not those of `b27-deep-first`; its environment answers Courier New with
Liberation Mono (source 5, the page resolver's open substitute), since C1b needs two faces of different
ascent/descent proportions on one line (with Liberation Sans alone `c1y-courier-150` laid out the same).

Tests: `tests/word_wc_fields.rs` (22 tests; `oracle_wc/<group>` `.docx` + Word's `.word.json`, open fonts only; the data
fixtures `oracle_wc/numwords`, `fields`, `lists` are Word's `Fields.Update`/`ListString` results; `fonts_sys` — the
installed-font cases — runs only with `UO_WC_INSTALLED=1`). Results with Word's rules: every case equal to Word (pages,
line texts, the page of each line, baseline ±0.3 pt); `ts_strict()` differs in the cases listed per group. `fonts_sys`
(113 cases, this machine's fonts, oracle policy): Word's rules 109 equal to Word, `ts_strict` 36; the other 4 are the
open cases below, asserted to still differ (`c2x-segoe_ui_baltic`, `c2y-segoe_ui_baltic-notable`, `c2y-segoe_ui_ce`,
`c5x-calibrilight`). `c4-angsana` was re-measured (2026-11-14): its first measurement predates Word's download of Angsana
New into the Office cloud-font cache (Word then split the run by script); now Word draws it in Angsana New, as ours.

| Group | Cases | `ts_strict` differs | Item |
|---|---|---|---|
| `c1` (+ `c1v`) | 11 + 1 | 7 + 1 | C1, C1b |
| `c2` | 17 | 17 | C2 |
| `c6` (baselines only) | 2 | 2 | C6 |
| `styleref` | 12 | 12 | B21 |
| `fields22` | 16 | 15 | B22 |
| `sections23` | 5 | 2 | B23 |
| `refs24` | 4 | 3 | B24 |
| `formula25` | 2 | 2 | B25 |
| `lists26` | 7 | 7 | B26 |
| `lists27` | 10 | 3 | B27 |
| `numwords` | 46,469 values | — | B22/B26 words: 0 differ |
| `fields` | 336 results (268 formulas on the evaluator) | — | B22/B24/B25: 336 equal |
| `lists` | 354 list strings | 203 | B26/B27: all equal |
| `hyphen` | 6,000 paragraphs | — | D1: Word's rules 5,748 (95.80 %), `ts_strict` 5,296 (88.27 %) |

### C1 — Symbol/Wingdings line metrics; C1b — multiple spacing over mixed faces

- Word (fh `c1-*`, `wcfonts2/c1x-*`): a line holding a Symbol run or bullet takes Symbol's ascent 2059/2048 em (a run
  also its descent 450/2048); Wingdings 1841/2048 and 432/2048 — when the face that draws it is a stand-in (Symbol is
  missing: the open-font viewer and server). `fonts::symbol_stand_in_metrics`, applied in `para.rs` (`symbol_metrics`) to
  the text glyphs and to the marker (the marker keeps "ascent only"). A face that is itself symbol-encoded keeps its own.
  Switch `ts_symbol_font_metrics`.
- C1b (`wcfonts2/c1x-*`, `wcfonts3/c1y-*`): "multiple" spacing adds (factor − 1) × the LARGEST OWN height (ascent +
  descent) of the line's text runs, not (max ascent + max descent) (Calibri 11 + Tahoma 11 at 1.5: Word 20.67, the TS
  rule 20.94). `lines.rs` `t_own` → `line_box(…, text_nat)`; the objects-only, empty and rule lines keep the single
  face's height. `WordLineRules::multiple_own_height`, switch `ts_multiple_text_height`.

### C5 — PostScript, camel-case and full names

- Word: "Family-Style" (PostScript) and "FamilyStyle" (camel case) are the FAMILY at the caller's weight (the style
  ignored) — the shortest installed family the name starts with, followed only by style tokens (MT, PS, PSMT, Bold,
  Italic, BoldItalic, Oblique, Light, Semibold, Narrow, Regular, Roman): "ArialNarrow-Bold" → Arial,
  "TimesNewRomanPS-BoldMT" → Times New Roman, "SegoeUISemibold" → Segoe UI. A FULL name (name ID 4) in the UI language
  — or en-US when the font has no record in it — is the face it names ("Arial Negrito" → Arial Bold in pt-BR; "Arial
  Bold" stays regular; "Calibri Bold" is bold: English names only). A "Family Style" name that is not such a full name
  is the FAMILY at the caller's weight: the trailing style words (Bold, Italic measured; Oblique, Regular, Light,
  Semibold, Semilight, Medium, Black, Thin inferred) are stripped, the longest remainder first ("Arial Bold", "Arial
  Italic", "Times New Roman Bold", "Georgia/Verdana/Tahoma/Segoe UI/Courier New Bold" → the regular face; the TS gave the
  missing-font chain, Cambria). A named instance of a variable font ("Bahnschrift Light") is the family.
- `fonts.rs` `lookup` (`LookupRules::postscript`, `spaced_style_candidates`, `camel_candidates`, the provider's
  `full_name_face`; all among the mapped tries, so a host stand-in gives way to them); uo-fonts
  `FontProvider::full_name_face` / `FaceSource::full_name` (default `None`), implemented by `SystemFontIndex` (name ID 4
  per language and the `fvar` instances indexed at the scan) and passed through by the oracle, server and substitute
  policies and the embedded-font wrapper. The UI LCID comes from `LayoutOptions::lang` (pt 0x0416, es 0x0C0A, else
  0x0409). Switch `ts_postscript_names`.
- Browser and viewer (fix round 2): the `viewer` policy indexes the name ID 4 records (platform 3, every language) of the
  faces the host pushed — what Local Font Access, the host fonts and the page's files hand over, the same bytes the
  fullName comes from — through `FontRegistry::full_name_face` (`uo-fonts/src/info.rs` `platform3_names`; the index is
  built once per registry generation; the pushed families in key order, then the hosted assets; `Substitute`/`Default`
  answers and faces whose own name ID 1/16 is not the family they were pushed for do not answer, so a stand-in's names
  never stand for another family). "Liberation Sans Bold" is Liberation Sans bold in the viewer as in the oracle and
  server policies. Only pushed faces are known: a full name whose family the document names nowhere else resolves once
  the host has pushed that family (the missing-family round trip). Tests: uo-fonts `registry_viewer`
  `the_full_name_of_a_pushed_face_names_its_family_and_variant` (pt-BR/en-US records, substitute and foreign faces,
  generation), `word_wc_fields::full_names_resolve_in_the_viewer_policy` (the lookup over the viewer provider, Word's
  rules against the TS chain).
- Name ID 6 (the PostScript name) is deliberately NOT indexed: Word maps a PostScript name to the FAMILY at the caller's
  weight (the first rule above: "Arial-BoldMT" is Arial regular), never to the face whose ID 6 it is, so the token rule
  is what matches Word.
- Open (asserted to still differ by `installed_fonts_against_word`, not special-cased): Word leaves "CalibriLight"
  unresolved (Cambria) while it resolves "SegoeUILight"; our shortest-family rule gives Calibri. Only pt-BR Word could be
  measured.

### C2 — charset-suffix names

- Word: the Windows `FontSubstitutes` names (Arial/Courier New/Times New Roman × Baltic/CE/CYR/Greek/TUR, Arabic
  Transparent, Tahoma Armenian) and a trailing parenthesized script (" (Hebrew)", " (Arabic)", " (Vietnamese)" measured;
  Greek, Cyrillic, Baltic, Turkish, Thai inferred) resolve to the base family; other " CE"-style suffixes keep the
  missing-font chain ("Calibri CE" → Calibri by swiss, "Georgia TUR" → Cambria). `fonts.rs` `charset_base` among the
  mapped tries. Switch `ts_charset_suffix_names`. Open: "Segoe UI CE/Baltic" resolve to Segoe UI in Word (no rule found).

### C6 — a missing symbol-charset font

- Word: a missing font with charset 02 is Symbol when its `w:family` is roman (whatever the PANOSE) or its PANOSE is
  Symbol's (`0505…`) with no family, Wingdings otherwise (`fonts::symbol_charset_stand_in`, before `generic_for`). Its
  text is drawn and measured with that face. When Symbol itself is missing (open fonts), the run shows the Unicode
  equivalents of the Symbol encoding (`symbol_map.rs`, written by hand from the public Adobe Symbol encoding) and takes
  the C1 vertical constants. Switch `ts_symbol_charset_fallback`. The `c6` group checks the baselines only (the stand-in
  draws other glyphs).

### C4 — a missing Thai font; A2 aside — the East Asian document default language (the Segoe UI CJK line height)

- C4 (fh `c4*`): a missing charset-DE font is one Thai face for the whole run: roman → Angsana New, swiss → Browallia New,
  else or unavailable Leelawadee UI; Thai characters of other runs fall back to Leelawadee UI first. Switch
  `ts_thai_fallback`.
- A2 aside (`wcfonts2/a2x-*`, `wcfonts3/a2y-*`, `wcfonts4/a2z-*`): with a DOCUMENT DEFAULT `w:lang w:val` ja/zh/ko every
  character takes the eastAsia slot (ASCII too, a run's en-US does not undo it); an eastAsia font without CJK (Segoe UI,
  Calibri, Arial) gives way to MS Mincho — the measured Segoe UI paragraph with 日本語 has Word's pitch 15.60 pt at 12 pt
  (the TS rule 16.42); Hangul falls back to Malgun Gothic; without the trigger a CJK character of a Latin-font run falls
  back to MS Mincho (not MS Gothic). `para.rs` `face_for`/`fallback_order`. Switch `ts_east_asian_default_lang`.
- Both need installed fonts (proprietary): `fonts_sys` (opt-in); the resolution rows are unit-tested
  (`font_names_resolve_as_word`).

### B21 — STYLEREF per page, by style name

- Word (B `wcbfld/b21-*`, `fields/wcfld4/sr-*`): in a header/footer, per page — the first paragraph with the style on the
  page (`\l`: the last), else the nearest before the page, else the nearest after it; a character style matches its runs
  (the text: that run group). Names: the built-in style's UI name, a custom name, any `w:aliases`, case-insensitively —
  not the English built-in name in a pt-BR UI, the styleId or an unaccented spelling (`uo_docx::word::style_names`). A
  name no style answers to: "Erro! Use a guia Página Inicial para aplicar <name> ao texto que deverá aparecer aqui."
  (bold, like Word's other reference errors); a style no text has: "Erro! Nenhum texto com o estilo especificado…". `\n
  \r \w \s` give the number without its final period. Error results never take `\*` case switches.
- `engine/field_refs.rs` `style_ref_at` (the TS `style_ref_value` under the switch), `PassInput::page_ranges_guess` (the
  body blocks of each page's lines, built in `next_input`; `PassFindings::used_styleref_hf` joins the stability test),
  `para.rs` lets STYLEREF be dynamic in headers/footers, the reader keeps `w:aliases` (`StyleDef::aliases`, a hidden
  field outside the TS shape and equality). Switch `ts_styleref_per_page`.

### B22 — `\*` switches, number words, the field's language

- Word (B `wcbfld*`, `fields/wcfld3*`): PAGE, NUMPAGES, SECTIONPAGES, SECTION, PAGEREF, SEQ and formula results take the
  format; the case switches apply to REF/STYLEREF text too. The last format wins; case switches after it apply in order,
  those before it are dropped; MERGEFORMAT/CHARFORMAT ignored; an unknown keyword is "Erro! Argumento de opção
  desconhecido."; roman/alphabetic in lower case give lower case; half away from zero; the limits and zero forms; the
  number words of pt-BR, en-US and es-ES reproduced exactly (46,469 values; `uo_docx::word::numwords`,
  `uo_docx::word::field_switch`); pt-BR DollarText keeps the cached result (Word 365 pt-BR crashes on it). The words
  follow the language of the field's INSTRUCTION run (its rPr, character style, paragraph style, defaults); the error
  texts the UI language.
- The reader keeps the first `w:instrText` run's `w:rPr` (`RunExt::instr_rpr`, hidden); `para.rs` resolves the language
  (`field_language`) and passes it through `PrepFlow::field_in` (the paragraph memo records it: `FlowOp::Field::lang`);
  `engine/fields.rs` applies the switches; PAGEREF reads the bookmark's page NUMBER (`PassInput::marks_num_guess`).
  Switch `ts_field_format_switches`.
- Ordinal past the exact integers (fix round 3, not measured): |n| ≥ 9e15 is the Arabic text (as `\* Arabic` writes
  it) with the suffix of its last two digits (`numwords::ordinal_text`: 1e16 → "10000000000000000th"/"º", not "0th");
  a value JS writes with an exponent (|n| ≥ 1e21) is "not representable", as the other word formats past their range.

### B23 — SECTIONPAGES on shared pages; a continuous restart

- Word (B `wcbfld2/b23-*`): a page shared through a continuous break counts in SECTIONPAGES for every section with
  content on it; a continuous section's `pgNumType start` numbers its first (shared) page, so the next page is start + 1.
  `engine/page.rs` `PageState::{sections, cont_first, num_sect_page}` (the numbering counter apart from `sect_page`, which
  the first-page header keeps), `section_page_counts(ts_shared)`. Switch `ts_section_pages_shared`.

### B24 — REF `\r` relative; `\p` per language

- Word (B `wcbfld2/b24-*`): `\r` is the full number less the leading levels the field's own list position shares with the
  target (same list), final period stripped; an UNNUMBERED field paragraph takes the list position of the nearest
  numbered paragraph of the target's list before it (the corpus oracle `refs/ref-num`: after "2." "b)", `\r` to "2.b)"
  is "b)"; b24 `fim`, after "2.", the full "1.a)i"; before the list, the full number). `\p` writes above/below in the
  language of the field paragraph's STYLE (acima/abaixo, above/below, más atrás/más adelante). `field_refs.rs`
  `ref_value`, `position_word`, `para_style_lang`. Switch `ts_ref_relative`. Tests: `oracle_wc/refs24` (the three b24
  cases and `ref-num`, copied from the frozen corpus: the first implementation regressed it in the Word-oracle replay).
- Under `ts_ref_relative` (fix round 2) `\p` keeps the frozen TS's mapping exactly: a pt UI gives acima/abaixo, EVERY
  other UI language above/below (engine.ts:450 knows no Spanish; the switch had written "más atrás" for an es UI).
  `word_wc_fields::b24_ref_position_words_ts_strict_per_ui_language` (es, en-US, pt-BR under `ts_strict()`; Word's
  rule on the es-ES style).

### B25 — header/footer formulas (and the session's table formula texts)

- Word (B `wcbfld2/b25-*`, the 104 `hf-*` rows): comparisons, `^`, postfix `%`, the 18 functions (case-insensitive, `;`
  between arguments, a function short of its arguments is a syntax error at the token where the separator was due),
  Word's error texts with the offending token ("!Erro de sintaxe, ,", "!Indicador não definido, NADA", "!Está faltando
  operador", "!Fim imprevisto da fórmula", "!Divisão por zero"), the thousands of a grouped literal kept, numbers read in
  the UI notation (pt-BR `,` decimal, `.` thousands). `uo_docx::word::field_formula::evaluate_field_formula_word` (and
  `word_function`); `para.rs` calls it unless `ts_hf_formula`.
- Bounds (fix round 1): a run of operators of one precedence is ONE flat node (`WNode::Chain`, evaluated left to right
  as the nested `((a o b) o c)`, same order of evaluation and errors) and a run of `%` one node with its count, so the
  tree's height grows only through the nesting `max_parse_depth` bounds (parentheses, unary signs, arguments). A
  left-deep tree aborted the process on a 10,000-term `= 1+1+…` (stack overflow in the evaluation and the drop);
  `nopanic_word::word_formula_operator_runs_on_a_small_stack` evaluates 100,000- and 400,000-term runs on a 1 MiB stack,
  and `hostile_field_codes` runs the evaluator over the 20,000-code soup.
- Session (`session/ops/tables/formula.rs`): under Word's rules (`FormulaEnv::word`, i.e. `!OpenOptions::
  ts_formula_errors`) the table formula is parsed by the header/footer evaluator's grammar
  (`word::field_formula::evaluate_word_expression`, fix round 3): the parser is shared, the table answers its names
  through the `FormulaNames` hooks (`TableNames`: `A1`, `A1:B2`, `ABOVE`/`BELOW`/`LEFT`/`RIGHT` with the format their
  numbers pass to the result — currency, suffix, decimals, thousands —, the bookmarks with a number; `DEFINED(name)` asks
  the bookmarks). So the session gives Word's texts and results: `= FOO(1)` "!Erro de sintaxe, 1", `= MAX()` "!Erro de
  sintaxe, )", `= 1.5` "!Erro de sintaxe, .", `= SUM(1,2)` "!Erro de sintaxe, 1,2", `= 1;2` "!Erro de sintaxe, ;", `= 2
  3` "!Está faltando operador", `= nada` "!Indicador não definido, NADA", `= 1.000,5` "1.000,5" (a grouped literal's
  thousands and a literal's decimals join the cells' format), ROUND half away at the decimal digits. Not measured in a
  table (the `hf-*` rows have none), kept from the TS where Word's rows say nothing: a table list alone is its sum, a
  list function takes every number of a list, another operand needs a list of one (else the syntax error at the list's
  text), MAX/MIN of an empty list 0, a single empty cell 0. `ts_formula_errors` keeps the TS's parser and texts
  byte-identical (the parity replays). Correction: `ROUND(2,345;2)` is 2,35 under BOTH rules (2,345 × 100 is 234,5 in
  binary); the rules part where the binary product falls below the half (`ROUND(1,005;2)`: Word's rule 1,01, the TS 1 —
  inferred from the decimal rule, not measured). Tests: uo-docx `word_wc_fields_session` (in `tests/all.rs`):
  `b25_table_formula_texts_and_round_behind_the_ts_switch` (11 formulas and 51 of the 52 `hf-pt-BR` rows, both rules;
  out: `= 5 \* CardText`, Word "Cinco" — the table formula's instruction takes a `\#` picture only and the session does
  not know the field run's language the words follow, so it stays "!Erro de sintaxe" under both rules),
  `b25_table_names_under_word_rules` (12 formulas over a 3 × 2 table: sums, ranges, currency, `DEFINED` of a cell, a
  list as an operand).
- Strict session replays (fix round 1): the uo-docx `parity_d2a_session` and `parity_d2b_f1…f10_session` open with
  `ts_formula_errors: true, ts_numbering: true`; the wasm `session_replay.test.mjs` sets both on each handle through
  the self-test export `uo_session_test_ts_wc_fields(h, 3)` (`Session::set_ts_wc_fields`; the production open has no
  wire field for them).

### B26 — list words per paragraph-mark language; B27 — isLgl

- B26 (B `wcblst/b26-*`, `fields/wcflst2/lw-*`, 189/189): cardinalText, ordinalText and ordinal list numbers use the
  language of the paragraph MARK's resolved run properties; the words are the field words with a capital first letter;
  ordinal pt/es "º". `uo_docx::word::numbering::format_number_word`, `NumRules::word_lang`, `NumberingState::next_in`;
  `para.rs` passes the mark's language (`PrepFlow::number_next_in`, recorded by the memo), `field_refs.rs` builds with the
  same rules (`numbering_state_for`) and gives each paragraph's marker its mark's language too (`list_next`: the mark's
  direct rPr, the paragraph style's, the defaults — REF/STYLEREF `\n \r \w` read those texts). Switch
  `ts_list_number_language`.
- B27 (B `wcblst/b27-*`, `fields/wcflst2/lgl-*`): `w:isLgl` turns EVERY placeholder Arabic, the level's own included;
  decimalZero keeps its zero; a bullet placeholder is empty. `NumRules::word_lgl`. Switch `ts_is_lgl`.
- The session's outline, TOC sources and cross-reference items use the same rules (`outline::*_with`,
  `Session::num_rules`) unless `OpenOptions::ts_numbering`. Test (fix round 2): uo-docx
  `word_wc_fields_session::b26_b27_session_items_follow_word_unless_ts_numbering` — the numbered cross-reference items
  of `b26-en-US`, `b27-lgl-own` and `b27-lgl-letter` equal Word's list strings (the `.word.json` lines) under Word's
  rules; `ts_numbering` gives the TS's back ("Um", "I)", "1.a.").

### D1 — English hyphenation: CC0 patterns + Moby exceptions (option H)

- Word's English points follow a dictionary: on `hyphword1k` (1,000 words × 6 widths) the paragraphs equal to Word go
  from 88.27 % (hyph-utf8 `en-us`) to 95.80 % (5,748/6,000) with DesignCraft's CC0 patterns plus the Moby Hyphenator II
  entries those patterns get wrong (19,548 patterns, 35,675 exceptions; minimums 2/3). pt and es are unchanged.
- Data: `core/tools/hyphen/gen_en_word.mjs` (inputs by path, `git show 3110dcf0^:assets/hyphenation/en-us.pat|.dic` of the
  WordCraft repository, sha256 checked) → `uo-fonts/data/hyphen/en-us-word.txt` (embedded under `hyphen-data`) and
  `uo-wasm/js/hyphen/en-us-word.mjs` (fetched by `hyphen-patterns.mjs`; its header is a legal comment, so the npm
  bundles keep it in `*.LEGAL.txt`). Notices: `uo-fonts/data/hyphen/THIRD-PARTY-NOTICES.md` (CC0-1.0 patterns,
  public-domain Moby), regenerated into `core/licenses` (row "Hyphenation patterns (hyph-utf8, CC0, Moby) compiled into
  uo-fonts").
- Sizes: `en-us-word.txt` 593,877 B raw, 220,531 B gzip-9, 192,697 B brotli (`en-us.txt`: 32,476 B, 17,750 B gzip); the
  JS module 593,679 B raw, 215,164 B gzip-9. The wasm binaries do not change (data is not compiled into wasm); native and
  .NET embeds grow by ~594 KB. The exception map holds 35,675 words.
- `HYPHENATION_PATTERN_IDS` gains `en-us-word` (8 ids); `HyphenSessions::session_with(…, word)` takes it for English when
  `!ts_hyphenation_patterns` and it is loaded, else `en-us`. The bindings ask for ONE English id by the layout flags
  (fix round 2, as the plan): `uo_write_hyphenation_patterns(doc, env, langs, flags)` and
  `uo_shell_hyphenation_patterns(h, env, langs, flags)` take a trailing `flags` word (`pattern_ids_to_load_with(…,
  flags & LAYOUT_TS_HYPHENATION == 0)`): `en-us-word` under Word's rules, `en-us` under bit 16 only; JS
  `hyphenationPatterns(env, langs, { tsHyphenation })` in `write.mjs`/`session.mjs` (`.d.ts` too); a caller that passes
  nothing passes 0 (wasm reads a missing argument as 0), so the product (`write/packages` viewer and editor engines,
  the benches) fetches `en-us-word` only. The harnesses that set `tsHyphenation` (`paint_parity`, `paint_memory`)
  register every pattern file through `openEnv` and do not ask. Tests: `registry` unit test
  `word_data_loads_en_us_word_or_en_us`, uo-wasm `wc_fields_native::d1_english_pattern_ids_follow_the_ts_hyphenation_bit`
  (an en-US document: `en-us-word` without bit 16, `en-us` with it). `TS_HYPHENATION_PATTERN_IDS` keeps the
  TS's 7 ids: the TS wrapper `pattern_ids_to_load` (which `fonts_parity_small` replays) and the uo-fonts parity replays
  (`fonts_parity_small`, `frontier_d_parity`) use it. Tests: uo-fonts `hyphen_data` (the file parses: 19,548 patterns,
  35,675 exceptions; Moby's `shep-herd-ess`, `im-pon-der-a-ble`, `hy-phen-a-tion`), `registry` unit tests, and
  `word_wc_fields::d1_english_hyphenation_agreement` (≥ 95.8 % asserted).

### B8 — README

`write/packages/docx-layout/README.md`: in compat ≤ 14 header/footer floats do not push the header/footer's own text;
square and topAndBottom ones DO push the body (re-measured `hff14wrap`; the code was right).

### Harness pins and documents (fix round 2)

- wasm `write_api.test.mjs` pins (printed by `write_native`): `f8__7__0` (page 2: C1b and B22 — header2's
  `PAGE \* upperLetter` is Word's unknown-switch error, `upperLetter` being no field keyword) and `f8__7__10` (pages 2–5:
  C1b on page 2, the header's `PAGE \* upperLetter` from page 2 on) take the new checksums, the old ones in the comments;
  the wasm layout with bit 21 gives the old page messages back (checked natively: `ea592122`, `06bed89e`), and bits
  18–20 change neither document. `f8__7__3` is the page area's.
- Which documents of the TS-capture harness corpora this area's rules move (native attribution,
  `attribute_documents_to_the_fields_rules` over `captures/<g>/docs`, each switch alone at Word's rule over `ts_strict()`;
  pages 0-based):
  - C1b `multiple_text_height` moves almost every document (multiple spacing with a mixed-face line): f7a 0–5, 7–13, 15
    (all but 14); f7b 0–11 (all); f7c 0–3, 5–8, 10–15; f7d 0, 2–8, 11; f8 0–2, 4–11 (all but 3).
  - B22 `field_format_switches` (`\*` keywords Word refuses, e.g. `upperLetter` — a `w:numFmt` name, no field
    keyword —, and the format/case order): f7a 0, 1, 2, 6, 9, 12, 15; f7c 0, 2–15 (all but 1); f8 0, 2, 4, 6, 8, 10.
  - B23 `section_pages_shared`: f7a 10 (page 2). B24 `ref_relative`: f7c 3 (page 14), f7c 5 (page 2).
  - None of C1, C2, C5, C6, C4, A2, B21, B26, B27 moves a harness document (wordcraft-adopt, over `captures/f2`, `f4`,
    `f6` too: C1b moves `gen__1__4` 0–1 and `gen__1__7` 0, 2–3; B25 `f2-3-1` 0; D1 `f2-3-1` 0 and `f2-3-3` 0).
- The editor/viewer switch harness declarations for these documents: "WordCraft study — harness declarations" below.

### Not measurable here / open

- pt-BR DollarText (Word crashes), every en/es UI text (only pt-BR Office is installed: the en/es strings are Word's UI
  strings, unverified), "CalibriLight" and "Segoe UI CE/Baltic" (no rule explains Word), Symbol truly missing in Word
  (always installed on Windows), Bahnschrift Light geometry (Word's PDF swaps the glyphs).
- The navigation pane of a layout (`navigation.rs`) keeps the TS list words (no options reach it).

### .NET port needs (not changed here)

`DocxLayout`: the C1 constants and the C1b line box; `Core/Fonts`: the C5 name rules and the full-name/`fvar` index,
the C2 aliases, the C6 Symbol/Wingdings choice and the Symbol map, the C4 Thai and A2 East Asian rules; `Docx/Fields`:
the number words, the `\*` switches, the field language (the reader's instrText rPr), STYLEREF per page and by name, REF
`\r`/`\p`, the Word formula evaluator; numbering: `NumRules` (B26, B27); the B23 page counters; `Core/Fonts/Hyphen`:
`patterns/en-us-word.json` and the English id mapping. Then regenerate the parity dumps (`tools/gen-parity-layout.mjs`).

## WordCraft study — page

WordCraft study, area "page" (plan `core/target/wordcraft-study/PLAN-page.md`, 2026-11-13). Every rule was measured in
Microsoft Word 16.0.20527.20150 (pt-BR, invisible COM through `tools/parallel/word-lock.mjs`; generators
`write/tools/layout-oracle/cases/wcpage{,2,3,4,5}.mjs`, untracked like the other study generators, plus the study's A and
B cases) and is checked natively by `tests/word_wc_page.rs` against Word's PDFs (`tests/oracle_wc/<group>`: 224 `.docx`
+ `.word.json`, Liberation Sans 11 on A4; the Arial/Calibri corpus bytes through `OpenFonts`; no embedded font —
checked). The comparer is `word_w7c.rs`'s (pages, line texts, the page of each line, baseline ±0.3 pt, x ±0.6 pt) plus
the page sizes, Word's empty pages (no line, no header/footer item) and each body float's box against a path rectangle
of Word's page (±0.6 pt after the outline's 0/0.25/0.5 pt). Word decides every rule; the frozen TS's behaviour stays
behind a `LayoutOptions` switch, all eight set by `ts_strict()` and, in wasm, by ONE flag bit (bit 19,
`LAYOUT_TS_WC_PAGE`, JS `tsWcPage`, set by `tests/support/paint-common.mjs` `wasmOptions`).

| Rule (switch) | Word (measured) | Frozen TS | Where |
|---|---|---|---|
| A15: a paragraph whose first content is a page break (`ts_page_break_keeps_space_before`) | mode 15: its text starts the new page with NO space before (before 6/12/24 pt, previous after 0/3/10 pt, numbered, bookmarked, the document's first paragraph: `p15-*` 81.12 pt, `p5-first-br-c15`); modes ≤ 14: `max(0, before − previous after)` (`p2-a15-c11/c12` 95.2), the document's first paragraph its whole before | `before − previous after` in every mode | `engine/flow.rs` (`place_paragraph_inner`, the page-break-at-start block; `first_in_doc` cleared) |
| A16, section parity (`ts_section_parity`) | at a nextPage/oddPage/evenPage section start (not the first): (a) odd/even with the wrong parity — a restarted number goes up by one (no page), else ONE blank page and the number after it; (b) with `w:evenAndOddHeaders` or `w:mirrorMargins` the number's parity must equal the page's document parity D (the first section's start + the physical index), else one blank page (nextPage too; modes 15/14/12); the blank page is empty (no header, footer or page border), has the NEW section's paper, counts in NUMPAGES and in no SECTIONPAGES; the text page after it is the section's first page (titlePg's first header) | a blank page whenever the number has the wrong parity, restarted or not; none for eo/mm; the blank page is the section's first page with its header and counts in SECTIONPAGES | `engine/page.rs` `new_page`/`make_page` (`PageState::blank`), `engine/hf.rs` (no hf on a blank page), `page_borders`, `engine/mod.rs` `section_page_counts` |
| gutter, rtlGutter, gutterAtTop, mirrorMargins (`ts_no_gutter_mirror`) | the gutter on the left (rtlGutter: right) of every page; mirrorMargins: odd-D pages left = left + G, even-D pages left = right (+G with rtlGutter), right = left + G; gutterAtTop: top + G, mirror and rtl ignored, the bottom and the header/footer y unchanged; equal columns recomputed, explicit `w:col` widths shifted; the header/footer, tables (pct), page borders offsetFrom text, margin-relative floats and the margin areas follow the page's margins; insideMargin = the left area on odd-D pages, the right one on even; modes 15/14/12 alike | ignores all four | `engine/page.rs` `word_margins`, `geom_of(…, even)`, `SectGeom::{base, even, hf_mar}` (the even-page geometry appended after the section geometries in `prologue`; the endnote copies get theirs in `notes.rs`), `geom_variant`, `make_page`/`continuous_section`/`next_column_section`; uo-docx `ReadFacts::gutter_at_top` |
| inside/outside of floats (`ts_page_parity_by_number`) | by the page's document parity D (`f5-inside-r1`: page 2 shown as 1 puts `inside` on the right) | by the displayed number | `engine/floats.rs` (`place_float`: `PageState::doc_even`) |
| B10: floats on their anchor line (`ts_float_anchor_first_line`) | a float belongs to the page and COLUMN of the line holding its anchor run: paragraph-relative = the paragraph's part top there (the column/body top on a later page), line-relative = that LINE's top (also on the same page: `b10x-line-same` 197.09), margin/page = that page; the text of that page/column (the paragraph's own earlier lines too) wraps around it; modes 14/12 alike | the page of the paragraph's first line; line-relative = the paragraph top | `engine/flow.rs` `place_floats_at_lines` (after the lines: the anchor line from the paragraph's `PassLine`s, the column from the part tops), `FloatPin` (a first detection is provisional — that pass had the float on the first line's page; the next pass measures the line again without it and settles the pin), placed at the page/column start in the following pass (`engine/page.rs` `place_late_float`, `place_late_col`), `Engine::float_line_top` read by `place_float` |
| B11: re-layout around a pinned float (`ts_float_no_reflow`) | a paragraph/line-relative wrapping float placed before its paragraph's lines that covers text laid before it on the page: Word lays the page out again around it, the float KEEPS the position computed before that re-layout; floats are handled one after the other (`b11x-two`); if the re-layout pushes the anchor paragraph to the next page the float goes with it (clamped to the body top) and the earlier page is laid without it (`b11x-push2`) | only absolute floats are laid out again | `engine/floats.rs` (the late recording: the first such float per page per pass, `Engine::body_early`), `engine/flow.rs` `check_pinned_anchor` + the `anchor_pushed` break, `PassInput::{late_floats: LateFloat, anchor_pushed, late_dropped}`, `merge_late` (the first settled pin wins), the pass loop: 5 + one pass per late float (≤ 20), `WorkInput::extra_passes` in the budget |
| mirrorIndents (`ts_no_mirror_indents`) | mode 15: mirrored on even-D pages, modes ≤ 14 on odd-D pages; `right' = left + firstLine` (signed), `left' = right − min(0, firstLine)`, the first-line indent unchanged (`left 400, right 2000, firstLine 720` breaks at right' 1120, mi-forms p2; `left 1440, hanging 720` at 720); per page within a paragraph; in the body, headers/footers and footnotes, never in cells or text boxes; independent of mirrorMargins; tab stops unchanged | ignored | `engine/flow.rs` `mirror_here`/`mirror_indents` (re-applied on every page change, `pfmt` rebuilt), `engine/static_layout.rs` (hf/footnote paragraphs, `Engine::mirror_static` set by `hf.rs` and `notes.rs`; the footnote cache key carries the page's side) |
| bookFoldPrinting, bookFoldRevPrinting, printTwoOnOne (`ts_no_print_fold`) | Word's PDF: landscape fold — odd D: left = right + G, right = left; even: left = left, right = right + G; the header/footer by the normal gutter/mirror rules; reverse fold = mirror margins; portrait fold = plain margins; printTwoOnOne portrait — odd: top = bottom + G, bottom = top; even: top = top, bottom = bottom + G; the sides (header/footer too) plain; landscape — the sides as the fold, the header/footer with the body; `bookFoldPrintingSheets` no effect; precedence printTwoOnOne > bookFold > gutterAtTop > mirrorMargins (inferred) | ignored | `engine/page.rs` `word_margins` (`SectGeom::hf_mar` read by `hf.rs`), uo-docx `ReadFacts::{book_fold, print_two_on_one}` |

The mirrorIndents formula differs from the plan's (`right' = left + min(0, firstLine)`): with a positive first-line indent
Word's mirrored lines break at `left + firstLine` (mi-forms, mi2-forms-c11…c15, mi2-mirror-c14, mi2-restart-c14,
mi2-sect-restart: 6–12 lines differed with the plan's formula, every case is equal with this one).

**Word build drift (A15).** Word 16.0.20430 (2026-10-11) kept the space before after an opening page break; 16.0.20527
drops it. The frozen corpus's `tocref/tocref-pbstart-c15`, `tocref/tocref-sectgap` and `tocref/tocref-sectgap-soma` were
measured with the old build: `word_w7c` `word_rules_agreement_on_the_oracle` now reads their re-measures from
`tests/oracle_wc/drift/tocref/*.word.json` before the corpus file (the frozen `.word.json` files are not edited; the
frozen TS's own oracle test reads them). A later Word update may move the rule again (memory `word-update-oracle-drift`).

**Tests** (`cargo test -j 3 --release -p uo-docx-layout --test word_wc_page -- --test-threads=3`): groups `pgbreak` 26,
`sectparity` 88, `gutter` 35, `floatflow` 27, `mirrorind` 31, `printfold` 17 — every case equal to Word under Word's
rules; `ts_diverges` lists where `ts_strict()` differs (14, 76, 32, 24, 29, 16). Also:
`the_blank_page_is_empty_with_the_new_paper` (`p16-size` 16838 × 11906 tw, no hf item; the `p16-numpages`
NUMPAGES/SECTIONPAGES texts), `each_switch_changes_its_own_cases` (each switch alone over `ts_strict()`: page_break_space
14, section_parity 50, gutter_mirror 31, parity_by_number 1, anchor_first_line 15, float_reflow 9, mirror_indents 29,
print_fold 14 cases), opt-in `attribute_documents_to_the_page_rules` (`UO_WC_ATTR`) and
`groups_with_the_installed_fonts` (`UO_WC_INSTALLED=1`). The line comparer moved from `word_w7c.rs` to
`tests/common/word_cmp.rs` (lines, Word's page sizes and path rectangles, `compare`, `compare_pages`), shared by
`word_w7c.rs` and `word_wc_page.rs`; the other areas' `word_wc_*.rs` still hold their own copies (they can switch to it).

**Not measured** (applied as written, listed so nobody reads them as measured): a continuous section that restarts its
numbering followed by a nextPage section under eo/mm; mirrorIndents in headers/footers and notes in modes ≤ 14 (the body
rule); mirrorIndents in a column section (the page's side); gutterAtTop combined with a fold or two-on-one;
bookFoldRevPrinting with a gutter; floating tables for B10/B11 (they keep the TS's placement; `engine/tables.rs` still
takes inside/outside by the displayed number — the tables area's call); the session commands that size things from the
model's margins under a gutter (table insert width, the TOC's right tab, watermark, cover: a follow-up for the session
owner); the editor's ruler shows the canonical margins on mirrored pages; the print options are the PDF's (the screen
was not captured).

**.NET port** (`UniversalOffice.Docx/DocxLayout`; nothing changed in `dotnet/`): the A15 top rule; the section parity and
the blank page (no header/footer/border, SECTIONPAGES); the per-page margin variants (gutter, rtlGutter, gutterAtTop,
mirrorMargins, bookFold*, printTwoOnOne, the header/footer margins) with document-parity floats; the late-float pins
(B10 provisional/settled, B11 first-wins, the pushed anchor, the pass count); mirrorIndents (body, hf, footnotes). Then
regenerate the parity dumps (`tools/gen-parity-layout.mjs`).

## WordCraft study — lines

Implementer of the plan `core/target/wordcraft-study/PLAN-L.md` (2026-11-13/14; items L1–L13). Word 365 pt-BR (16.0)
decides where it differs from the frozen TS. Every rule was measured with Word (invisible COM through
`tools/parallel/word-lock.mjs`; case generators `write/tools/layout-oracle/cases/wcl*.mjs`, 129 documents, data in
`core/target/wordcraft-study/L/`) and is checked by `tests/word_wc_lines.rs` against the fixtures in
`tests/oracle_wc/l-<group>` (open fonts only — Liberation, Carlito, or the family NAMES Arial/Calibri/Times New
Roman/Cambria that Word draws with its own fonts and `OpenFonts` maps to the metric twins; generated by
`write/tools/layout-oracle/cases/wcfix-lines.mjs` and `wcfix-l14.mjs`; pages, line texts, the page of each line, baselines
±0.3 pt and, where a group measures positions, every glyph origin of Word's PDF (`.wordx.json`, `extract_xs.py`) ±0.6 pt).
Each case is laid out with Word's rules (must equal Word, or match its recorded residual) and with `ts_strict()` (the
group's `ts_diverges` list is asserted exactly). The frozen TS's behaviour stays behind one `LayoutOptions` switch per
item, set by `ts_strict()` (so by every strict replay: `layout_parity`, `f8_parity`, `f4_parity`, `f6_parity`, `g8`,
uo-ffi `server_parity`) and, in wasm, by the ONE area bit 18 `LAYOUT_TS_WC_LINES` (`uo-wasm/src/write.rs`; JS
`tsWcLines` in `js/write.mjs`/`write.d.ts`, set by the harnesses' `wasmOptions` in `tests/support/paint-common.mjs`).
Inside the breaker the switches arrive as `BreakSettings::word` (`lines::WordLineRules`, filled by `Cx::settings`; the
facet replays and the breaker's unit tests build `BreakSettings::default()` = all TS).

| Item (switch) | Word (measured) | Frozen TS | Where | Group (cases; TS-divergent) |
|---|---|---|---|---|
| L1 — space height (`ts_space_like_height`) | none of the SPACE_LIKE set (U+0020, U+2002, U+2003, U+2005, U+200B, U+3000) gives a line height, in the middle, at the end or alone; NBSP, U+2000/01/04/06–0A, U+202F, U+205F do (`wcl/l-spaces-*`, Calibri/Arial/Times, 17 code points) | only U+0020 and the tab | `lines.rs` vertical metrics and the tabs-only line (`space_like_cp`) | `l-spaceh` 3 (3) |
| L2 — the SPACE suffix of a list number (`ts_number_suffix_font_space`) | Arial's space, 569/2048 em at the NUMBER's size (1/4096 pt floor), whatever the number's font, text or mark size (8 pt 2.18 … 100 pt 27.83); a level without sz takes the mark's size; lvlJc right/center align the number AND the suffix as one unit (`wcl/l-numsp`, `-c14`) | the number face's space; the number alone aligned | `lines.rs` marker block | `l-numsp` 2 (2) |
| L3 — jc=both compression (`ts_justify_tolerance`) | the two-outcome rule with slacks +0.035 tw on the 25 % cap and +0.025 tw per space (the plan's MODEL: 1,006/1,008 `wcl/l-jboth-*` + 210/210 jsweep; the 2 residual Calibri cases sit at c−½e ∈ (0.0115, 0.025) tw. This ENGINE with the installed fonts, `UO_WC_INSTALLED` 2026-11-14: about 1,003/1,008 — `l-jboth-arial` misfits 1 paragraph (p24, 1,330/1,332 lines) and `l-jboth-calibri` 4 (p22, p24, p25 ×2, 1,332/1,340 lines), all compressed where Word does not; Arial is not among the model's 2 residuals — not analysed further) | 0.0175 / 0.0075 tw | `lines.rs` word fit, `hyphenate.rs` `choose_hyphenation` (the same test) | `l-jboth` 2 (2) |
| L4 — kinsoku (`ts_kinsoku_lists`) | by the run's RESOLVED East Asian language (styles/defaults: `m-kinsoku-inherit-*`): ja, zh-CN (every zh but TW/HK/MO), zh-TW/HK/MO; ko none; every mode, whatever the document defaults. No-start lists (the character before goes down with it, over spaces), no-end lists (any next character), a break allowed between a non-East-Asian character and an East Asian one; hang lists per language for ja and zh-CN (mode 15, jc both, overflowPunct, East Asian document) and the hanging mark may end the line when an East Asian character that may start a line follows it — never between two non-East-Asian characters (review 2026-11-14: opening a break after EVERY hang character, the Latin `,` `.` between digits too, broke `R$12.|345,67` where Word keeps `R$12.345,67fim` whole and turned 8 frozen-corpus cases from equal to different — `kinsoku/ks-ja-jp-msmincho-c15`, `ks-zh-cn-msmincho-c11`, `-c11-noearules`, `hang15/h15-{ja-jp,zh-cn}-cjk-c11-left`, `-c15-both`, `h15-zh-cn-cjk-c15-left`; with the condition `word_rules_agreement_on_the_oracle` has none: 79 cases change, Word-equal 74 → 79); `w:kinsoku w:val="0"` turns the hang off in mode 15 (mode 14 still hangs). Found while implementing (`m-kinsoku-hang-*` against `l-kinsoku-sw-latdef`): in an East Asian document a justified jc=both line of East Asian text WITHOUT spaces (the nine ideographs a push leaves) spreads its slack equally over the gaps between its characters (`GlyphFlags::EA_JUSTIFY`); with Latin document defaults it does not | no-end only before a space, one hang list | `para.rs` `KINSOKU_NO_START`/`KINSOKU_NO_END`/`KINSOKU_HANG`, `kinsoku_lang`, the pass after the break flags, `word_kinsoku_off_15`; `lines.rs` justification (`cjk_spread`) | engine rule tests `l4_kinsoku_no_start_and_no_end` (181 characters; the TS differs on 122), `l4_kinsoku_hang_lists` (90 characters: the ja and zh-CN no-start lists in an East Asian document, mode 15 jc both: hang or push; overflowPunct 0 pushes; mode 14 hangs 、。」） with kinsoku 0 and jc left), `l4_kinsoku_inherits_the_language` (docDefaults ja-JP / en-US), `l4_kinsoku_may_start_ja` (36 characters); wasm bit 18: a generated ja document |
| L5 — character grid (`ts_no_char_grid`) | linesAndChars: every character + charSpace/4096 pt (East Asian exact: 11.5/12.0/16.0 pt at 11 pt, `wcl2/l-cgrid-lc-*-11`; other characters 0.27 tw less per character for a positive pitch, fitted on `wcfix-lines` cgrid-lc-4096/20480); snapToChars: cell = W / ⌊W / (Normal's size + charSpace/4096 pt)⌋, each East Asian character on ⌈adv/cell⌉ cells centred in them, each run of other characters between them on the fewest cells that hold it, centred (not when it ends at an automatic break), the alignment ignored, the cells fill the line exactly (no internal-unit rounding of the fit edge, `wcl4/n-charsind-sc-4096`), and snapToChars is a LINE grid too (a hostile charSpace with Normal's size + charSpace/4096 pt ≤ 0 gives no grid); `w:snapToGrid w:val="0"` on a run takes it out | both grids ignored | `para.rs` `section_char_grid`, `CharGridSpec`, the advance pass; `lines.rs` `snap_to_chars`, `fit_right`; `engine/page.rs` line pitch | `l-cgrid` 9 (8) |
| L6 — *Chars indents (`ts_no_chars_indent`) | the *Chars value (hundredths of a character) wins over the twips of the same side (0 = the twips); unit of firstLineChars/hangingChars = the paragraph's FIRST run size, of leftChars/rightChars = the paragraph style's size; linesAndChars adds charSpace/4096 pt to the unit, snapToChars rounds up to whole cells; hangingChars ignores w:left (first line at leftChars, the others at leftChars + hangingChars). Style inheritance (review, `l-charsind/charsind-style`, generator `write/tools/layout-oracle/cases/wcl6_charsind_style.py`, Word 365 2026-11-14, 24 paragraphs): the plan's note "a direct twips value cancels an inherited *Chars" is NOT Word's rule — a paragraph's direct w:left/w:firstLine/w:hanging/w:right leaves the style's leftChars/firstLineChars/hangingChars/rightChars in force (as the merge already did); a direct *Chars of 0 over an inherited nonzero *Chars on a side with no twips anywhere takes the inherited *Chars VALUE as twips, whatever the size (leftChars 300/150/777 → 300/150/777 tw, firstLineChars 200 → 200 tw, rightChars 1000 → 1000 tw; hangingChars 200 with leftChars 300 → the first line at leftChars, the others 200 tw further) — implemented (`para.rs`, the `fb_*` values) | ignored | `para.rs` `prepare_paragraph` indents | `l-charsind` 4 (4) |
| L7 — doNotExpandShiftReturn (`ts_shift_return_expands`) | with the compat option a jc=both line ending in a `w:br` is not expanded (modes 14 and 15); jc=distribute still is | expanded | `lines.rs` `justified` | `l-shiftret` 4 (4) |
| L8 — `w:ptab` (`ts_ptab_indent_frame`) | left = the left margin (relativeTo margin) or w:left (indent); center = centre of the margins, or of [left indent, right MARGIN]; right = the right indent (at most the margin); a left/centre target left of the pen ends the line; a right ptab whose segment overflows right-aligns the part that fits | targets in the line frame, no break | `lines.rs` ptab branch, `PtabSpec::relative_to_margin`, the break path (`close_pending`) | `l-ptab` 2 (2) |
| L9 — jc=distribute (`ts_distribute_spaces_only`) | spread (last line too, after the last tab): up to ½ g per space the slack goes to the spaces; past it each space takes 0.46 g and every gap between the characters an equal share of the rest (the plan's model: 16/2,004 measured lines over 0.6 pt, 14 of them `l-jspread-calibri22`; its 2/2,004 needs the gap share floored to 0.025 pt with the remainder lost, which is NOT implemented — in this engine, against Word with the installed fonts at 0.6 pt, the floor gives 14 of 16 `l-jspread-*`/`m-jspread48-*` cases equal against 15 without, 5 of 12 `l-jdist-*`/`m-jdist-*` against 6, and `m-jdist-liberationsans` 0.63 pt against 0.48; open: `l-jspread-calibri22` 0.84 pt. Why model and engine pull opposite ways (review, `core/target/wc-lines/l9_floor_sets.py` + the `UO_WC_DUMP` of `l-jspread-calibri22`): the misfit is the same in both — Word's line ends short by a shortfall that grows with the glyph count on long spread lines (calibri22, 16–36 glyphs: the model without the floor has 14 lines over 0.6 pt, this engine 13 of 1,063, every one with the last glyph +0.57…+0.79 pt right of Word's; the model's end error is positive even in phase 1, e.g. +0.36…+0.56 pt on 35-glyph lines where only the spaces stretch, so the floor is a proxy for a per-glyph shortfall, not a quantum of the gap share). Per CASE the model without the floor fails 3 of 15 `jspread` sets (calibri22, m-jspread48-timesnewroman 0.65, -georgia 0.61) and with it still 1 (calibri22, 2 lines); the engine without it fails only calibri22 (timesnewroman 0.60, georgia 0.56 — its positions differ from the model's by a few hundredths), and the model never scored the `jdist` sets, where the floor's lost remainder (up to 0.025 pt × the gap count) is what pushes `m-jdist-liberationsans` to 0.63. Floor kept off; the per-glyph shortfall itself is not modelled (open); modes ≤ 14 (`wcfix-l14`, 80 lines per font, all within 0.6 pt): 1.17 g for both constants; compression: as "both" (cap +0.035 tw) with the expansion side × f = Σg / (Σg + 0.1271·ΣxAvg) and no per-space slack (9 misfits over 3,985 paragraphs, against 1,017) — the same test in the hyphenation choice | spaces only, as "both" | `lines.rs` word fit (`spaces_xavg`, `LineEnv::x_avg`) and justification (`gaps`); `hyphenate.rs` `choose_hyphenation` (`HyphenUnit::distribute`/`spaces_xavg`) | `l-jdist` 6 (6) |
| L10 — default tab left of the margin (`ts_default_tab_after_negative`) | x < 0: the first default multiple AT or after the pen; x ≥ 0 strictly after; a hanging stop still wins | strictly after | `lines.rs` `next_tab` | `l-tabs` `tabs-neg` |
| L11 — bar tabs and the default grid (`ts_bar_tab_counts`) | default stops count as if the bar tabs did not exist (modes 14 and 15) | a bar tab is the last custom stop | `lines.rs` `next_tab` | `l-tabs` `tabs-bar-c14`/`-c15` (group: 3 cases, 3) |
| L12 — emphasis marks (`ts_no_emphasis_marks`) | a line with a `w:em` dot/comma/circle run gets E = ¼ of its natural height above (underDot: below); single and multiple < 1.25 add E to the height, multiple ≥ 1.25 keeps it and moves the baseline E down (marks above), at least: the value when it holds natural + E, exact unchanged; the marks: one per non-space character at the run's size and colour, dot U+FF0E and comma U+3001 "MS Mincho" at baseline − 1.0·size, circle U+02DA "Batang" at − 0.294·size, underDot U+FF0E "SimSun" at + 0.382·size, centred (width 0.337/0.316/0.40 of the size); the families resolve through the provider's fallback, nothing shipped; the marks are decoration — Word's selectable text has none: their glyph runs carry an EMPTY run text (the glyphs keep their code points, which Word's PDF text also holds; `l12_emphasis_marks` asserts both) | `w:em` ignored | `lines.rs` `emphasis_mark`/line box, `paint.rs` `paint_emphasis_marks` (`PaintEnv::emphasis_face`, `None` in the facet replays) | `l-emph` 2 (1) |
| L13 — superscript/subscript above 24 pt (`ts_supsub_formula_above_24`) | per-font raise, drop and superscript SIZE in half points, every half point 24–200 pt, the measured points 250–1000 pt and a ratio per font beyond, for Arial, Calibri, Courier New, Segoe UI, Times New Roman, Tahoma, Verdana, Cambria, Georgia, Century Gothic (`L/suptable-word.json`); beyond 200 pt the superscript size is 0.6 × the size for Cambria (its measured row), 1331/2048 for the others; the table is read by the run's REQUESTED family (its rFonts slot) when the provider draws it or its open metric twin (Arial → Liberation Sans, Calibri → Carlito, Cambria → Caladea, Times New Roman → Liberation Serif, Courier New → Liberation Mono), else by the drawn family — so the viewer/editor with open fonts gets Word's values for a document that asks for Arial/Calibri/…; Calibri Light reads Calibri's row (an assumption, not measured); other fonts and ≤ 24 pt unchanged | the ≤ 24 pt formula extended | `supsub_word.rs`, `para.rs` `resolve_run_style` | `l-supbig` 4 (4) |

Recorded residuals (asserted in `word_wc_lines`; a residual that closes must leave the list): `cgrid-lc-4096` (Latin
glyphs ±0.82 pt: Word's Latin advances on a linesAndChars grid are not the font's plus the pitch — 0.27 tw is the best
single fit), `charsind-grid` (±1.09 pt), `m-jdist-carlito` (2 lines of the 9 compression misfits), `supbig-cambria`
(±0.4 pt: Cambria's measured jumps at even sizes ≥ 58 pt).

Opt-ins: `UO_WC_INSTALLED=core/target/wordcraft-study/L` lays out the 127 measured cases with the INSTALLED fonts (oracle
policy) under Word's rules and `ts_strict()` and fails when a case equal under the TS's rules is not under Word's —
2026-11-14: Word-equal 81 of 127 with Word's rules against 4 with the TS's, none worse (the previous implementer's
probe: 70). Kinsoku: every `l-kinsoku-*`
and `m-kinsoku-*` case breaks its lines as Word (ja-JP, sw, sw-c14, sw-latdef, hang-ja, inherit-ja/-en equal; zh-CN,
zh-TW, ko-KR and hang-zh-CN differ only by the line height or advance of a few punctuation marks the CJK fonts lack —
‼ ⁇ ⁈ ⁉ 〗 ‐ … — i.e. Word's FONT FALLBACK for them, not a kinsoku rule; the page break of `l-kinsoku-zh-cn` follows
from those heights). Every case that still differs under Word's rules (46 of 127, re-run 2026-11-14 after the review's
fixes; same list before them), with its cause:
- `l-cgrid-sc-*` (10): the snapToChars Latin advances (Word's are ~0.5 % wider than the font's on a character grid: one
  word of the long Latin line wraps differently, or glyphs up to 1.09 pt off);
- `l-cgrid-lc-2048-*`, `-4096-*` (4; glyphs up to 1.02 pt) and `l-cgrid-lc-20480-*` (2; 2 lines break differently):
  Word's Latin advances on a linesAndChars grid (the 0.27 tw single fit of the residual `cgrid-lc-4096`); `l-charsind-grid`
  (0.89 pt) the same;
- `m-cgrid-lc-normal14-0` / `-4096` (gx 7.2 / 7.7 pt): a PRE-EXISTING autospace gap, outside this plan — 'あabcあ' gets no
  East Asian–Latin space where 'あabc' does (already in the previous implementer's probe, also under the TS rules);
- `l-emph` (gx 2.91), `m-emph-circle` (dy 0.30), `m-emph-underDot` (1 line): not analysed;
- `l-jboth-arial`, `l-jboth-calibri`: the L3 misfits above; `l-jdist-arial`, `l-jdist-calibri`, `m-jdist-cambria`,
  `-carlito`, `-verdana` (line breaks) and `m-jdist-georgia` (gx 0.71): the L9 compression misfits (κ, the plan's 9 over
  3,985 — the engine's count with installed fonts not analysed); `l-jspread-calibri22` (0.84 pt): L9 above;
- `l-kinsoku-zh-cn`, `m-kinsoku-zh-tw`, `-ko-kr`, `-hang-zh-cn`: the CJK font fallback above;
- `l-spaces-arial` (U+202F/U+205F are missing from Arial: Word's fallback face is 0.25 pt taller);
- `m-supfill-*` (11; sizes 250–1000 pt: lines taller than the page).
`UO_WC_ATTR=<dir of .docx>` (`attribute_documents_to_the_lines_rules`) prints which of the 13 switches changes which pages
of each document.

Paragraph memo (review): the section's character grid (`para::prep_char_grid`, the `CharGridSpec` of the paragraph's
section) is part of the memo's per-paragraph key (`incr/memo.rs` `prepare`): a section edit — margins, orientation,
`w:docGrid`, a break that moves paragraphs to another section — replaces `doc.sections` but neither the formatting nor the
styles, so without it the unchanged paragraphs kept the old cell/pitch. `la7::memo_keys_the_character_grid` (snapToChars:
custom margins, landscape, then a typed character served by the memo again; linesAndChars: charSpace 4096 / −2048 / 2048)
asserts every relayout equal to a fresh full layout and no memo hit after a grid change; without the key it fails at the
margins step (canonical digest).

**.NET port** (`UniversalOffice.Docx/DocxLayout`; nothing changed in `dotnet/`): none of the 13 rules is ported — the
.NET engine follows the frozen TS, so the JS↔.NET parity dumps are unaffected. Port needs: `Lines.cs` (L1 the SPACE_LIKE
set in the vertical metrics; L2 the marker suffix and the unit alignment; L3 the compression slacks, also in the
hyphenation choice; L7; L8 the ptab targets and break; L9 the two-phase spread and the xAvgCharWidth factor; L10/L11
`NextTab`; L12 the line box; L5 the snapToChars placement and fit edge; L4 the jc=both East Asian spread), `Para.cs` (L4
the per-language kinsoku lists and the break pass; L5 the grid advances; L6 the *Chars indents; L13 the per-font size
table), `Page.cs` (L5 the snapToChars line pitch), `Paint.cs` (L12 the marks), `Fonts.cs` (L9 xAvgCharWidth; L13 the
table).

## WordCraft study — harness declarations

Leva "wordcraft-adopt" (2026-11-14). The browser switch harnesses compare the frozen TS editor/viewer with the Rust one
running the product rules (Word's), so every WordCraft rule that moves a harness document shows there. Each difference
is declared only where a run PROVES it comes from a WordCraft switch, in the harness's own shape, scoped as narrowly as
the harness allows, with the id "Word manda sobre o TS (WordCraft <area> <item>, …)" (F-1: "WordCraft page F-1 …").

**Proofs.** (1) Everything at the TS's rules: the viewer with `UO_SW_TS_WC=lines,page,tables,fields,d1` (a harness-only
knob: an esbuild `onLoad` gives the BUNDLED `js/write.mjs` the `tsWc*` layout options and D1's `tsHyphenation`, nothing
shipped changes; the WordCraft declarations are then left out) equals the TS on every document but for the
non-WordCraft declarations (gate-O3, ED-713, wave 7c items 3/4/6); the editor harnesses with session wasm builds that
lay the session out with every area's `ts_*` switches on (and the fields area's session switches, D1's pattern ids):
`editor_switch` parity 0 undeclared with every WordCraft entry "declared but equal now", the other harnesses likewise
(completion: only the wave 7c entries and the F-1 proxies below). Those builds used a compile-time env knob in
`uo-wasm/src/session` (`option_env!`), removed afterwards: the product wasm rebuilt from the tree is byte-identical to
the one every product run used. (2) One area at Word's rules, the other three (and D1) at the TS's — viewer: the dumps
of `UO_SW_MISMATCHES` (every mismatch as JSON) per area less the all-TS dump; editor: per-area builds, every difference
listed (`UO_EO_MEASURE`/`UO_EC_MEASURE` + `COVERED`) — gives what each declaration lists. (3) Inside an area: the native
attribution (`attribute_documents_to_the_<area>_rules`, the pages each switch alone changes over `ts_strict()`; the
fields test now takes `UO_WC_ATTR_SOURCES=1` for the editor's read, as the tables test) and, for the editor's own
scenarios, builds with ONE switch at Word's rule: tables B6 for stories T4, completion `blank|tables`, `f2-3-0|tables`
and the table-style previews; B15 A for fidelity `cell-nested`; B5 for `f8__7__11|objects`'s end pages; page A15 for the
editor's `f7a__7__11/13/15/6`; fields C1b for `f7a__7__9`'s page count. (4) F-1 (page, UI): completion and objects run
with the `ResizeObserver` of `write/packages/editor/src/rust-engine.ts` disabled (local, reverted): the IME proxies of
completion `blank|objects` @41, `blank|inline` and `f2-3-0|inline` @5/@7, `f7b__7__3|objects` @41 and objects
`f7b__7__3|objects`, `f8__7__11|objects` @2 are equal then. The lines area moves no capture document of any harness
(its runs equal the all-TS ones); its one declaration is stories `H2` (L8).

**What moves.** Fields C1b (a multiple-spaced line over runs of several faces gets Word's text height) moves almost
every line of the f7–f8 fuzz documents by a few hundredths of a point — the viewer's text layer, pixels and PDF, and in
the editors the caret of the first click, the end thumbnails and screens; B22 (`\*` switches: an unknown keyword is
Word's error text, the case/format order — f7a__7__0's two `PAGE \* ROMAN` are Word's `VI`, the TS's `vi`), B23, B24
and B25 change field results; D1 (Word's English hyphenation data) breaks English lines elsewhere. Page A15 (no space
before after an opening page break; in the editor also after the scripts' Ctrl+Enter), A16 (section parity: f7c__7__6
and f8__7__6 get a blank page, 7 pages — Word's own count, the TS 6) and B10/B11 (floats on their anchor line, the page
laid out again). Tables: B5, B6, B12, B15 A/B as in "WordCraft study — tables".

| Harness | Rule | Documents / keys (facets) |
|---|---|---|
| `viewer_switch.mjs` `WC` (per area, per switch; `textLayer`/`pixels` scoped to pages, one list or `{ textLayer, pixels }`) | fields C1b | 57 documents: `gen__1__4/7`, f7a 0–13 and 15, f7b 0–11, f7c 0–3, 6–8, 12–14, f7d 0, 2–5, 7, 8, 11, f8 0–2, 4–9, 11 (`f7d__7__5`: the PDF only — its moves are below a pixel; on the other f7c documents the C1b pages are already a wave 7c declaration's) |
| | fields B22 / B23 / B24 / B25 / D1 | B22 27 documents (f7a 0, 1, 2, 6, 9, 12, 15; f7c 0, 2–10, 12–15; f8 0, 2, 4, 6, 8, 10); B23 `f7a__7__10` p2; B24 `f7c__7__3` p14; B25 and D1 `f2-3-1` text layer p0; D1 `f2-3-3` p0 |
| | page A15 / A16 / B10 | A15 `f7a__7__13` p1, `f7a__7__15` p3, `f7a__7__6` p11; A16 `f7a__7__2` p10, `f7a__7__5` p8, `f7c__7__13` p3, `f7c__7__6`, `f8__7__6` (7 pages); B10 `f7b__7__10` p0, `f7b__7__3` p1, `f7b__7__5` p0, `f7b__7__6` p5, `f8__7__3` p1, `f8__7__5` p0 |
| | tables | the 13 documents of "Harness documents", the facets measured: `f7b__7__2` pixels/pdf (no text layer), `f7b__7__6` no find, `f7b__7__11`, `f7b__7__9`, `f8__7__1` + find, `f7b__7__5` + linkBoxes |
| `editor_switch.mjs` `WC_DECLARED` (merged with an existing key as an array; `declaredFor` takes arrays) | fields (+ page) | 50 documents: from the first differing step the caret (`selection`/`state` where the first click lands elsewhere) and the end thumbnails/screens; `f2-3-3` D1; `f7a__7__11/13/15/6` + A15, `f7a__7__2/5` + A16, `f7b__7__10` + B10; `pagesAfterLoad` on `f7c__7__6`/`f8__7__6` (A16, Word's 7 pages) and `f7a__7__9` (C1b: 11 pages, the TS 12, Word's count not measured — the viewer's read of the same bytes keeps 12; open) |
| `editor_switch_stories.mjs` `WC_S` | fields C1b; lines L8; tables B6 | `N2 f7c__7__13`, `T2 f7b__7__1`, `T2 f7d__7__8`, `X3`/`X4 f7b__7__10`; `H2 two sections` steps 7–8 (the alignment tab of the header: its target left of the pen ends the line in Word); `T4` from step 22 (its Grid Table 4 Accent 1: rows, the distributed sizes it saves) |
| `editor_switch_objects.mjs` | fields C1b (+ B23, B10, tables B5) | `f7a__7__5/10|page`, `f7c__7__1|page`, `f7b__7__8|objects`, `f7b__7__0|menu`, `f7a__7__1|views` end thumbnails/screens; `f7b__7__0|objects`, `f7b__7__1|objects`, `f8__7__11|objects` the frames of both drags (`at`, steps 2, 4, 6, 8) and the end (f8__7__11 also its saved body; tables B5 its end pages); `f7b__7__10|objects` (+ B10). The F-1 entries kept; the existing page/tables entries hold the fields run's thumbnails too |
| `editor_switch_review.mjs` | fields C1b, B22 | `f7a__7__0|revisions` (the revision anchors from step 2, the end screens and mask), `f8__7__0|spell` (the screen under the squiggles, the mask) |
| `editor_switch_format.mjs` | fields C1b, B22 | `f7a__7__0|font/para/ruler/view/bar`, `f8__7__0|file`, `f7b__7__0|file` (printed pages); the TOC extras: `wcRoman` (the TS's two `PAGE \* ROMAN` results upper-cased, the only text change allowed besides ED-712, and required) and ED-713's printed pages `WC_ED713_PRINTED` [0, 1, 2, 3, 5, 6] (exactly those) |
| `editor_switch_completion.mjs` `WC_C` | fields D1, C1b, B22; tables B6; page B10/B11, F-1 | `f2-3-1|lists` D1; `f7c__7__2|lists` screen; `f7a__7__1|page`; `f7b__7__0|previews` answer 3 (C1b), answers 4–5 (B6); `blank|tables`, `f2-3-0|tables`, `f2-3-0|previews`, `blank|previews` B6; `f7b__7__3|objects` page counts and frames (B10/B11) and F-1 @41; F-1 `blank|objects` @41, `blank|inline`/`f2-3-0|inline` @5/@7 |
| `editor_switch_fidelity.mjs` | tables B15 A | `cell-nested` (step 17's screen, the end) |

The viewer's five text-layer-only pages (`f7a__7__3` p3, `f7b__7__4` p1/p4, `f7a__7__12` p4, `f7a__7__2` p6,
`f7a__7__6` p5: the markup differs, the pixels do not) are the fields area's (its run with and without D1) on pages
whose PAINT no fields switch changes natively — a line box height the paint does not show; declared under C1b.

**Final runs** (the product wasm of the current tree, `UO_SW_STRIDE=1`, `UO_ES_STRIDE=1 UO_ES_STEPS=12`; every one 0
undeclared, 0 "declared but equal now"): viewer 78 documents / 592 pages, 5 equal, 73 with declared differences (the
all-areas-at-the-TS's run: 0 undeclared, only the non-WordCraft declarations met); `editor_switch` all 13 fixtures
(264 facet differences: 246 declared, 18 informative), parity 74 fixtures (1,532: 1,410 declared, 122 informative);
stories 21 scenarios (273: 266 declared, 7 informative); objects page 94, objects 239, menu 4, views 2 differences, all
declared; review revisions 27, styles 36, fields 5 and the spell screen, declared; format font 7, para 28, ruler 59,
text 7, view 438, bar 25 declared, every extra as declared; completion lists 4, tables 4, objects 141, inline 4, page
2, previews 9 declared; fidelity 148 declared. The wasm API tests (`session_api`, `session_hostile` with
`UO_WS_FUZZ_MS=10000`, `session_replay`, `shell_api`, `write_api`) pass; `write_api`'s pins `f8__7__0`/`f8__7__10` (the
fields area's) hold in wasm.

**Open.** `f7a__7__9` in the editor: under C1b the session's layout of the bytes has 11 pages, the write API's (the
viewer's read) 12, both Rust (the TS's rules: 12 and 12; a single-switch build proves C1b) — the two models lay the
document out differently; not analysed further (Word's page count not measured). Objects `f7b__7__1|objects` F-1 (@2)
still differs with the observer disabled: the pre-existing declaration is kept as it was (its cause not re-measured
here). Completion `paste`: an image-state difference after `pic:change:clip` (steps 14–15) showed twice in full runs
(`f2-3-0|paste` in an all-TS run, `blank|paste` in a product run) and never in nine runs of the family alone —
intermittent and not WordCraft's (the all-TS build shows it): not declared.
