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

Pending.

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

## Milestone-1 gate — approved 2026-11-03

Maintainer, verbatim: "Approve all" — every final recommendation of the milestone-1 questions accepted (G15, G19, G21, LQ1–LQ22). Cross-record changes from the uo-render/uo-pdf plans (RQ1–RQ3, RQ13, PQ1, PQ8) apply to this record where listed there.
