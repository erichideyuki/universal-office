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
| (M2) | incremental relayout; uo-docx session `fontUsage()` port (LQ15, uo-docx D2) | LA7 levels 1 and 2, differential tests | new | F7, D2a |

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

Pending. **For the security review** (collected as they are found):

- F2-21: the TS `documentHyphenationLanguages` is exponential in nested text boxes and tables
  (2^depth; 30 levels ≈ 25 min); the Rust walk visits each paragraph once (same output). The TS bug
  remains in the shell until the switch (not edited by the port).

## Evolution

Pending. Candidates: f32 coordinates (LQ5), level-2 incremental relayout tuning, the `pure()` flow
clones as persistent structures, explicit-stack nesting (LQ13), the format-2 kerning class tables
(uo-fonts D5).

## Final review and benchmarks (gate 3)

Pending. `tools/bench`: layout of the oracle corpus and of the 500-page document, main × rust-core,
Node and WebKit, plus the editor keystroke budget in milestone 2.

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
| F0-3 | `OptF64`: optional numbers in 8 bytes, "absent" = a signalling-NaN payload JS cannot produce (values are NaN-canonicalized on entry, as the codec does) | `Option<f64>` is 16 B; RA1.2 needs absent ≠ NaN |
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
| F2-20 | **The TS checkout of the captures**: `.claude/worktrees/clickcompliance-docx` at `d0f45bc5` (= this branch's frozen `write/`; its dist built after `field-formula.ts`). The main checkout (`b92b60fa`) has a **stale dist** (built 2026-10-23, before `field-formula.ts`/`numpages.ts` were added on 10-30): no `dist/field-formula.js`, `numpages.js`, `run-boxes.js`, `pdf-navigation.js`, `pdf-word-spaces.js`, `inline-renderer.js`, `docx-core/dist/inspect.js`, `style-names-es.js`, and different `engine.js`, `para.js`, `pdf.js`, `document.js`, `fulldoc.js`, `xml.js`. The captures naming `@ts-commit b92b60fa` (F0, F1, F5 `layout-ci`, F1 `resolver`) come from that older engine | finding for the orchestrator: recapture with a dist built from the frozen TS before the gate |
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

## Stage 3 — F6 (tables): brief, not started (2026-11-05)

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

## Stop point (2026-11-05, after F4)

Done: F0; wave 1 — F1 (0 parity rounds), F2 (0), F3 (1), F5 (0); wave 2 — **F4 (1 round:
the `.notdef` metrics face)**. F6 not started (brief above). Next: F6 tables, then wave 3 (F7a
engine core, F7b ∥ F7c, F8). Open items carried: the F2-20 recapture of the older fixtures (now
also their `p3.prepare` encoding), the `layout-parity` CI steps for `f4_parity`, the F4 hand-ons
to F7a (derived paragraphs, the engine `PrepFlow`), F7b (picture-bullet painting) and F7c.
