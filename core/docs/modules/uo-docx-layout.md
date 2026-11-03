# uo-docx-layout — the Write layout engine (pages, lines, tables, floats, notes, fields, display list)

Record of the rust-port workflow. Status: **architecture plan, milestone-1 gate pending** (hybrid
mode, `SKILL.md` "Hybrid mode"; every role of this module runs on Opus, implementation and verifiers
included — "Models"). Planner and reuse analysis together (one Opus agent, 2026-11-03). The
analysis skeptic has not run yet. Under the hybrid mode there is no per-frontier brief: the
architecture below fixes the shapes (inputs, identity keys, pass state, output, caches, the painter
and PDF boundaries, the editor API), the Opus frontiers port 1:1 inside it, and every decision
outside the source semantics is declared in this record (section "Stage 3", per frontier, with the
nine-item list of uo-docx A7).

Line counts are non-test source lines on `main` 0.9.x (frozen), counted in this worktree.
Milestone 1 is the read path: uo-docx D1a → **uo-docx-layout** → uo-render → uo-pdf (viewer and
server PDF). This module is its long pole (≈ 12.3k lines of behaviour, all Opus).

## Scope

### TS covered (behaviour source, `write/packages/docx-layout/src`)

| TS files | Lines | Rust module (crate `uo-docx-layout` unless noted) |
|---|---|---|
| `engine.ts` (4,183: `layoutDocument` pass loop + column balancing search, `class Engine` with ≈ 90 methods), `chain.ts` (91), `frames.ts` (130) | 4,404 | `engine/` (`mod.rs` pass loop and balancing; `page.rs` geometry, sections, columns, page borders, line numbers; `flow.rs` `placeParagraph`, `layoutStatic`, keep rules, contextual spacing, border groups; `tables.rs` `placeTable`, floating and late tables, row emission; `floats.rs` `placeFloats`, `wrapFrame`, `polyBandIntervals`, objects, textbox chains and frames; `notes.rs` footnotes/endnotes, separators, note columns; `hf.rs` headers/footers; `fields.rs` SEQ/REF/NOTEREF/STYLEREF/PAGEREF/SECTIONPAGES/chapter numbers); `derive.rs` (frames lifted, chain slices) |
| `para.ts` (1,178), `hyphenate.ts` (254), `math.ts` (327) | 1,759 | `para.rs` (`prepareParagraph`, `resolveRunStyle`, glyph fallback order, the 600-DPI width model `advanceTwips`/`wordWidth`/`scaledUnits`/`toWordUnit`), `hyphenate.rs` (Word's choice over `uo_fonts::Hyphenator`), `math.rs` (OMML layout + its own font reader `parseFont`, uo-fonts S14) |
| `lines.ts` (689), `underline.ts` (184), `paint.ts` (504) | 1,377 | `lines.rs` (`LineBreaker`; seed: `experiments/rust-spike/src/linebreak.rs`, 0 mismatches on 13,353 lines), `underline.rs`, `paint.rs` (lines, boxes and borders → display-list items) |
| `table.ts` | 1,406 | `table.rs` (grid widths, row layout, row split, vertical merges, nested rows) |
| `fonts.ts` (355), `context.ts` (374) | 729 | `fonts.rs` (`FaceResolver` Word substitution chain, `faceMetrics` DirectWrite vertical metrics + the East Asian 1.3× rule, `VERT_TABLE`, superscript/subscript, `parsePostScriptName`, `withEmbeddedFonts` precedence), `context.rs` (`DocContext`, flow state `saveFlow`/`restoreFlow`/`pure`, note marks) |
| `resolve.ts` (414), `paraformat.ts` (31), `numbering.ts` (236), `datefmt.ts` (103), `field-formula.ts` (129), `outline.ts` (185), `numpages.ts` (30), `revisions.ts` (178); `context.ts` `directRPr`/`directPPr` and `para.ts` `resolvePPr` | 1,306 | **Word semantics shared with the session** — placement is question **LQ3** (recommended: `uo_docx::word`, ported by these frontiers) |
| `geometry.ts` (layout half ≈ 380: `presetPathLocal`, `presetPath`, `mapPath`, `pathBounds`, `custPaths`, `withThemeStyles`, `shapeExtras`, `groupChild`), `picture.ts` `pictureLook` (≈ 75), `page-fill.ts` `resolvePageFill` + `patternRowsFromImage` (≈ 80), `textfx.ts` `resolveTextFx`/`fxColor`/`gradOf`/`fxMarginPt` (≈ 80) | ≈ 615 | `drawing.rs` (shapes, groups, pictures, page fill, text effects resolved into display-list values) |
| `emf.ts` (633), `wmf.ts` (148) | 781 | `metafile.rs` (EMF/WMF records → display-list items; needs only a face lookup — LQ7) |
| `refs.ts` (41), `run-boxes.ts` (85); editor `engine-doc.ts` `caretXIn`, `offsetAtX`, `locateRef`, `isSortedByX` (≈ 60) | ≈ 190 | `query.rs` (refs, run boxes, hit testing, caret geometry — the editor-facing API) |
| `tree.ts` (275) | 275 | the item vocabulary goes to **uo-render** (types + encoding); the Write-only geometry (`LaidOutLine`, `PlacedObject`, `PlacedCell`, `ObjectFrame`, `LineParaFormat`) stays here (`output.rs`) |
| `typography/lineheight.ts` `deviceEmPt` | 5 | `para.rs` |

Total ported here ≈ **12,340 lines** (≈ 11,030 if LQ3 places the Word-semantics files in uo-docx;
they are ported by the same frontiers either way).

### C# (type guide only)

`dotnet/src/UniversalOffice.Docx/DocxLayout/**` (18,797 lines; ≈ 15.9k excluding `Pdf.cs`,
`PdfPicture.cs`, `PdfNavigation.cs`, `PdfWordSpaces.cs`, `Images.cs`): a port of the TS, useful
where the TS has unions, `any` and optional fields (e.g. `Tree.cs` item records, `Para.cs` `Glyph`).
Nothing exists only in C#: `SystemFontProvider.cs`/`FontCatalog.cs` are host font discovery (LQ8),
`LayoutDump.cs` is the 0.01-rounded dump of `tools/gen-parity-layout.mjs` (JS × .NET parity) — the
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
| the model, `DocView`, `DocRef`/`StoryRef`/`BlockPath`/`RunRef`, embedded-font reading | **uo-docx** (A2, A4, D1a) | |

### Stays in the TS shell

`canvas.ts` (601) becomes a decoder/painter of the uo-render display list; `canvas-picture.ts`
(391) stays as the canvas side of the picture/shape effect pipelines (raster work through
`uo-render::paint` exports, LQ6); `inline-renderer.ts` (89, `paintCustomInlines`: plugin renderers
draw over the fallback-text box from `run_boxes`); `FontFace`/Local Font Access/`fetch` of font
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
   layout never mutates the model; field values, page numbers, hyphenation points and font usage
   go back through `LayoutOracle` values (G21).
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

```rust
/// One per layout call; borrows the view, owns only what the TS synthesizes.
pub(crate) struct LDoc<'a> {
    view: DocView<'a>,
    paras: Vec<LPara<'a>>,          // LParaId(u32): every paragraph the passes touch, model or synthetic
    runs: RunTable<'a>,             // RunKey(u32): every run, incl. synthetic frame runs and sliced runs
    stories: Vec<LStory>,           // body, each hf part, each note, each textbox content: block lists of LBlock
    textboxes: Vec<LTextbox<'a>>,   // TbKey(u32): model textboxes (DrawingId) + synthetic frame boxes (FrameInfo inside)
    chains: Vec<Chain>,             // TbKey members in seq order; TbKey -> chain
    revision_order: Vec<&'a str>,
}
pub(crate) enum ParaSrc<'a> { Model { para: &'a Paragraph, id: ParaId }, Synth(Box<Paragraph>) }
pub(crate) struct LPara<'a> { src: ParaSrc<'a>, flags: ParaFlags /* CONTINUATION, FRAME_SKIP, INNER */,
                              frame_carry: Option<RunKey>, story: StoryRef, path: BlockPath }
```

- `ParaSrc::Model` only when the TS hands the **model object itself** to the engine. Every TS copy
  (`{ ...p, runs }` of an anchor paragraph, a slice, a join, a frame's inner copy) is `Synth` and has
  **no `ParaId`**, so what the TS keys by paragraph object behaves the same: e.g. a manual
  hyphenation skip (`hyphenationSkip(para, offset)`, keyed by the editor's paragraph objects) never
  matches a continuation slice or a frame anchor copy in the TS, and must not match in Rust either.
  The same rule for runs: a sliced run (`{ ...r, text }`) gets a `RunKey` with no `RunRef`, so it is
  absent from `field_values` exactly as `onField` reported an object the editor could not find.
- The TS frame caches (`runCache`, `anchorCache`: `WeakMap` by paragraph, so the synthetic run is
  the same object in every pass of one call and across calls) become: built once per `LDoc`, reused
  by all passes of the call. Across calls the object identity is not observable (late floats are
  per call), so nothing is carried.
- Cost: O(paragraphs + runs) per call, `Cow`-free for model paragraphs (borrowed), owned only for
  synthetic ones (rare). The `Story::blocks()` order of uo-docx gives the TS block indices, so
  `BlockPath` equals the TS ref path.

#### LA3. TS identity → Rust key (the port's rule list; each frontier declares its sites)

| TS identity (file:line) | Use | Rust key |
|---|---|---|
| `Run` objects: `lateFloats`/`lateFound` `Map<number, Run[]>` + `mergeLate` `Set<Run>` (engine.ts:203, 304), `prePlaced: Set<Run>` (371), `PlacedObject.run`, `GLUED_FIELD: WeakSet<Run>` (para.ts:176), `FRAME_CARRY` (frames.ts:25), `Glyph.obj` | late floats per pass; editor finds an object by its run | `RunKey` (LDoc); `PlacedObject` carries `RunRef` (uo-docx) when the run is a model run and its `DrawingId` |
| `Paragraph` objects: `CONTINUATION`, `FRAME_SKIP`, `INNER` (`WeakSet`), `anchorCache`, `listItemValues` `Map<Paragraph, …>`, `hyphenationSkip(para, …)` | flags, caches, editor lookups | `LParaId` + `ParaSrc::Model { id }` (LA2 rule) |
| `DrawingTextbox`: `FRAME_INFO` (`WeakMap`), `indexChains` `Map<DrawingTextbox, TextboxChain>`, editor `members.has(t)` | frame data, chains | `TbKey`; model ones map to `DrawingId` |
| `TableLayout`: `floatFlow` `WeakMap` (engine.ts:363) | split floating tables | `TableLayoutId(u32)` in a per-pass arena |
| `PreparedPara`: `PFMT` `WeakMap` (engine.ts:3931) — one `LineParaFormat` object shared by every line of a paragraph | ruler | per-page `pfmts: Vec<LineParaFormat>` + index in `LineGeom`; the canonical stream records the sharing as "same object" groups |
| items: `LATE_ITEMS` `WeakMap<object>` (table.ts:1383), `moved: Set<object>`, `idx: Map<object, number>`, `seen` | moving items between arrays during row splits and floats | `ItemMeta(u32)` side-table index carried by each pass-scope item; a `shiftItem` copy keeps or drops it exactly as the TS keeps or drops the map entry (declared per site) |
| `row.split` closure (table.ts `RowLayout.split`), `CellLayouter` callback, `ParaEnv.field` callback | deferred work with captured state | `RowSplit` data (the captured contents) + `fn split(&self, avail, cx)`; `trait CellLayouter` implemented by `Engine`; `FieldSource` enum + engine method |
| `PageState` used as identity (`boxPage?: object`, `lnPage`) | border groups, line numbering | `PageIx(u32)` |
| module counters `lineSeq` (paint.ts:16, never reset), `zSeq` | `lineBox.line`, z order | per call; the canonical stream relabels by first appearance (the TS value depends on every layout run before in the process) |
| `metricsCache` (fonts.ts:55), `infoCache` (math.ts:43), `CTX` (paraformat.ts:7) | memo of pure functions | values on the face table / per call; no global |

#### LA4. Pass state

```rust
pub struct LayoutRun<'a> {          // one layoutDocument call
    ldoc: LDoc<'a>, opts: &'a LayoutOptions, fonts: &'a mut FaceTable, budget: Budget,
    // inter-pass inputs, exactly the TS loop variables (engine.ts:197–251):
    total: f64, marks: IndexMap<Box<str>, Utf16>, late: IndexMap<u32, Vec<RunKey>>,
    late_tables: IndexMap<u32, LateTable>, note_marks: Option<IndexMap<Box<str>, Utf16>>,
    note_refs: IndexMap<Box<str>, Utf16>, chaps: ChapterIndex, sect_pages: IndexMap<u32, f64>,
    balance: IndexMap<i64, BalanceSpec>,
}
#[derive(Clone)]
pub(crate) struct Engine<'r> {       // one pass (TS `class Engine`)
    ctx: DocContext,                 // resolver, flow state (numbering, SEQ, notes), revision display, warnings (IndexSet)
    pages: Vec<PageState>, cur: PageIx, col: u32, y: f64, col_top: f64, at_top: bool, prev: Option<PrevPara>,
    tables: Arena<TableLayout>, items_meta: Vec<ItemMetaData>, float_carry: Vec<FloatCarry>, …
    found: PassFindings,             // lateFound, lateTablesFound, balanceFound, used* flags, heading marks
    input: &'r PassInput,            // the read-only inter-pass values above
}
```

- **Pass loop and balancing** are ported literally: up to 5 passes until stable (engine.ts:224),
  then per balanced section a binary search where each probe is a full pass (263–290; guard 20).
  Every pass is a fresh `Engine`; only `PassFindings` survive it. The final result is the last pass.
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
                                              //   stand_in, face: uo_fonts::Face }, ImageId -> ImageRef { media: MediaId, … }
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
pub struct LineRef { pub story: StoryRef, pub path: BlockPath }   // "b3/r0/c1/b0", "hdr:default/b0", "txbx/b3" in the canonical stream
pub struct PlacedObject { pub run: Option<RunRef>, pub drawing: Option<DrawingId>, pub x: f64, pub y: f64, pub w: f64, pub h: f64,
    pub layer: Layer, pub hf: bool, pub frame: Option<ObjectFrame>, pub content: Option<(Range<u32> /* lines */, [f64; 6])> }
```

- **uo-render page** (owned by uo-render, sketched for the boundary): `items: Vec<Item>` with
  structure-of-arrays side buffers (glyph `cp: u32`, `x: f64`, optional `gid`; UTF-16 `text`; path
  commands; dash arrays), interned strings per layout (`Sym(u32)`: colours, preset names, dash
  names), so a glyph costs ≈ 12–16 bytes instead of a JS object. **Colours stay the TS strings**
  (interned), because the TS writes what the document had (`'ffffff'`, `'auto'`…) and the painters
  test `HEX6` (canvas.ts) — an `Rgb(u32)` would change which items paint.
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
  `bytes: Arc<[u8]>`, `ttc index`, `stand_in`) and asks for the misses after a layout. Synchronous,
  no callbacks into the shell (the TS `FontProvider.face` is synchronous over loaded fonts too).
  Embedded document fonts (uo-docx `read_embedded_fonts`, deobfuscated) are registered per document
  with the `withEmbeddedFonts` precedence (fonts.ts:316–352, incl. "variant not embedded but the
  family exists on the system → prefer the system").
- `FaceCache` (uo-fonts `registry`): parsed `uo_fonts::Face` deduplicated by bytes (length +
  SHA-256, then the `Arc` is shared), **one per wasm instance / native process**, LRU over
  `bytes.len() + face.heap_bytes()` (needs a new `Face::heap_bytes()` in uo-fonts — LQ8), faces
  pinned while a layout or a live `Layout` references them.
- **Per-document face budget (uo-fonts D4)**: a document may pin at most `face_budget` bytes
  (proposal: wasm32 128 MiB, native 1 GiB); the instance cache keeps at most `cache_cap` unpinned
  (wasm32 64 MiB, native 512 MiB). Past the document budget, a new face resolves as a miss of the
  provider (the `FaceResolver` chain continues to the fallbacks; warning `"font budget"`), which
  differs from the TS only on documents that embed tens of fonts — accepted by name, counted by the
  capture (0 expected on the corpus). First numbers; measured at gate 2 (M3).
- `FaceTable` (per `Layout`): `FaceId` in first-use order → resolved face (`FaceResolver` result:
  key, effective family, synthetic flags) + the `FaceMetrics` computed once. `FaceResolver`'s cache
  (fonts.ts:229, keyed `family|bi`) lives on the engine (per document), cleared with the registry
  generation.
- Hyphenation: `uo_fonts::HyphenationRegistry` per engine, one `HyphenSession` per language
  (word cache capped, M5).

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
  (Rust only) plus "Rust full == TS full" on the edited documents.

### 3. Public API

```rust
// uo_docx_layout
pub struct LayoutOptions {
    pub lang: Box<str>,                       // 'pt-BR'
    pub now: LocalDateTime, pub tz: TimeZoneRule,   // DATE/TIME, CREATEDATE…: local parts and offset at an instant (LQ10)
    pub hyphenation: Hyphenation,             // On | Off | Force
    pub hyphenation_languages: Option<Vec<Box<str>>>,
    pub hyphenation_skips: HyphenSkips,       // FxHashSet<(ParaId, u32)> (TS callback keyed by paragraph object)
    pub revisions: Option<RevisionView>,
    pub view: ViewTransform,                  // Print | Web { width } | Draft { … } | Outline { … } (LQ2)
    pub collect_fields: Option<FieldScope>,   // onField
    pub limits: LayoutLimits,
}
pub struct LayoutEngine { /* per document: FaceResolver cache, hyphen sessions, para memo (M2), last Layout */ }
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
    pub fn run_boxes(&self, view: DocView<'_>, pick: RunFilter) -> Vec<RunBox>;   // links, plugin inlines (run-boxes.ts)
    pub fn navigation(&self, view: DocView<'_>) -> uo_render::Navigation;          // pdf-navigation.ts → uo-pdf
    pub fn lines_of(&self, story: StoryRef, path: &BlockPath) -> impl Iterator<Item = (u32, &LineGeom)>;
    pub fn hit_test(&self, page: u32, x: f64, y: f64) -> Option<Hit>;            // DocRef + object + cell
    pub fn caret(&self, at: &DocRef, affinity: Affinity) -> Option<CaretRect>;    // caretXIn
    pub fn offset_at_x(&self, line: LineHandle, x: f64) -> u32;                   // offsetAtX
    pub fn selection_rects(&self, from: &DocRef, to: &DocRef) -> Vec<(u32, Rect)>;
    pub fn font_usage(&self) -> Vec<FontUsage>;
}
// the LayoutOracle of uo-docx (G21), implemented in the binding on top of LayoutEngine
impl LayoutOracle for BindingLayout { field_values, pages_of, next_hyphenation, font_usage }
```

- **No measuring callback** into the shell: `InlineRenderer.measure` is never called by the engine
  or the editor (only `plugins.test.ts:11`); custom inlines are laid out by their fallback text and
  the renderer draws over the box from `run_boxes`. uo-docx A4's `InlineMeasurer` is dropped (LQ1).
- **What crosses to JS** (milestone 1): one handle per opened document for the viewer
  (`UoDocumentView`: `Document` + `LayoutEngine`) and the instance-wide `FontRegistry`/`FaceCache`;
  `layout()` returns page count, warnings, misses; pages are pulled encoded (`encode_page`, Rust-owned
  buffer, JS copies and frees) only for the pages being painted; text layer and link boxes come with
  the page encoding; PDF export is one call returning bytes. In milestone 2 the same `LayoutEngine`
  lives inside `UoWriteSession`, and the hot paths (hit test, caret, selection rectangles, line
  motion) are dedicated exports.
- **.NET** (server PDF): `DocxPdf.Convert(bytes, options) -> bytes` through uo-ffi, faces from the
  native system index (LQ8); no layout handle crosses.
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
| Regex for field instructions (≈ 80 sites) | `regex`, `regress` | **own matchers** from a spec table (uo-docx G9 policy) |

`#![forbid(unsafe_code)]`, `#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]`,
`#![deny(clippy::indexing_slicing)]` (LQ9).

### 5. Parity strategy

#### Captures (Node 24.15 x64, uo-jsnum G2; inputs and outputs, exact)

| Capture | Where in the TS | Compared |
|---|---|---|
| **P1 layout** (the gate) | `layoutDocument(full, withEmbeddedFonts(provider, …), opts)` → `LayoutResult` | every page: `widthTw`, `heightTw`, `section`, `number`, `hfItems`, background fields, every item (all fields, **f64 bits**, `-0` kept, NaN canonical), every line (all fields incl. `chars`), `objects`, `cells`; `warnings` in order |
| P1 painter inputs | the same items, through the TS functions the painters call on them: `presetPath`/`custPaths`/`shapePath` for shapes, `imagePixelSize` for blips, symbol remap and outlines for `gid`/`asPath`/COLR glyphs | the resolved geometry Rust stores (LA5), exact |
| **P2 provider transcript** | every `provider.face(family, bold, italic)` call in order and its answer (`null`, or file path + SHA-256 + face index + `standIn`) | the Rust replay provider answers from the transcript; the call sequence itself is compared (it is the `FaceResolver` chain) |
| P3 facets (localization, not the gate) | `prepareParagraph` (glyph arrays), `LineBreaker` calls (the spike's `lines/capture.mjs`), `layoutTable` per table, `NumberingState` markers, `StyleResolver` outputs, `markHyphenation`, `resolveTextFx`, `pictureLook`, `renderEmf`/`renderWmf` | per facet, exact |
| P4 editor queries | `onField` values (`collect_fields`), `bookmarkPages()` (pages_of), the hyphenation search with `'force'` + skips (universal-write.ts:17337–17353), `runBoxes`, `documentOutline`/`listItemValues`/`numberedItems`/`outlineParagraphs`, `pdfNavigation`, `caretXIn`/`offsetAtX` over every line | exact |

- **Canonical stream** (uo-testkit `Stream`, `.cap`): a TS walker extending
  `tools/gen-parity-layout.mjs` `normalizeLayout` (same field coverage) with exact bits instead of
  0.01 rounding; object references mapped to stable values — `FontFace` → (key, family, flags,
  file digest), `InlineImage` → (media part name, SHA-256), `PlacedObject.run` → the TS ref of the
  run (story path + run index) or `synthetic`, `lineBox.line` and `zSeq` relabelled by first
  appearance, shared `pfmt` objects as identity groups, refs as the TS strings. The Rust `Canon`
  emits the same stream from `Layout` (refs printed as TS strings).
- **Determinism**: `opts.now` fixed, `TZ=UTC` (plus one facet under `TZ=America/Sao_Paulo` for the
  date fields), all 7 hyphenation pattern sets loaded (as `oracle.test.ts`), `mk-*` cases with
  `revisions: { mode: 'all' }` (as the oracle and the .NET parity), a fresh process per shard
  (`lineSeq` is process-global; relabelled anyway).
- **Fonts**: the Word oracle runs on the machine's installed fonts (never versioned, CLAUDE.md); the
  capture records the face digests, and a replay on a machine whose file differs reports
  `font drift` and skips the case instead of a mismatch. CI runs the open-font subset.

#### Corpus

| Set | Size | Where it runs |
|---|---|---|
| Word oracle (`test/oracle`, 259 groups) | 2,742 docx | local Windows with Office fonts (as `oracle.test.ts` and `LayoutParityTests`) |
| .NET synthetic inputs (`dotnet/tests/fixtures/input`: pictures, page, page-pattern, page-frame, page-tile, textboxes, texteffects) | 7 docx | local |
| option sweeps: the four revision modes over `mk-*`, `hyphenation: 'force'` with seeded skips, `lang: en-US`, Web/Draft transforms (LQ2, Rust-only after the switch) | ≈ 600 runs | local |
| **differential generated documents** (uo-testkit generator: random sections, columns, tables incl. nested and merged, floats, textboxes and chains, notes, fields, lists, revisions) with **open fonts only** (Liberation, Carlito, Caladea, Cambria Math substitute) | 2,000 per CI run, seeded | CI |
| stress: a 500-page document, nesting 64, 10,000-row table, 1,000 continuous column sections, one 1 MB word | 10 | CI (time and memory, and parity where the TS finishes) |

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
  `hit_test`, `caret`, `selection_rects`, `font_usage`, each oracle call, registry add/remove,
  face-budget eviction.
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
| viewer, 100-page typical document, wasm high-water incl. faces | today's TS tab heap | ≤ 160 MiB (phone baseline, iOS 15) |
| 500-page stress, wasm high-water | — | ≤ 384 MiB (phone risk, reported, not a gate) |
| per-document faces pinned | uo-fonts D4 | ≤ `face_budget` (LA6) |

- **M4**: `UoDocumentView` (10,000 create/free and the finalizer path, with a laid-out document
  inside), encoded page buffers, registry handles; .NET: the PDF call returns owned bytes only.
- **M5** (each filled past its cap): `FaceCache` (bytes), per-document face pins (budget),
  `FaceResolver` cache (entries), `HyphenSession` word caches (100,000 words per language),
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
| work explosion: 5 passes × balancing probes (≤ 20 sections × log₂ h each a full pass), `pure()` re-layouts of cells per nesting level (kᵈ), late-float/late-table ping-pong | `max_work` units (glyphs prepared + lines placed + cells laid out + flow clones), per call | `Err(Limit(Work))` (TS: hangs) |
| page explosion: zero/negative page or column size, 1 glyph per line over 1M lines, `pgSz` 1e9 | `max_pages` (e.g. 100,000), `min_content_width` guard where the TS would loop without progress | `Err(Limit(Pages))` / the TS's own progress rule where it has one |
| huge tables (100k rows × 63 columns, `gridSpan` 1e9) | model limits + `max_table_cells` per table | `Err(Limit(Table))` |
| deep recursion (cells, textboxes, groups, OMML) | model `max_nesting` 64 + layout depth guard + measured wasm stack (LQ13) | text fallback from the reader; `Err(Limit(Depth))` past the guard |
| metafiles: record counts, nested save/restore, huge polypolygons, text records with hostile fonts | `max_metafile_records`, `max_metafile_items`, path points (uo-drawingml `max_path_points`) | truncated drawing + warning (TS draws until memory runs out) |
| number formatting: roman/alphabetic/ordinal text of 1e15, `\#` pictures, date pictures of 10k characters | output length caps per field | truncated + warning |
| pathological line breaking (one 1 MB word, all-NBSP, thousands of tabs) | `max_work` | `Err(Limit(Work))` |
| faces (many embedded fonts) | per-document face budget (LA6) | fallback face + warning |
| panics (wasm `panic = "abort"` kills every document in the instance) | `deny(indexing_slicing)`, checked integer arithmetic on counters, no `unwrap` (workspace) | a TS `x[i]!` that would throw → `Err(Internal)`; a TS `x[i]` read as `undefined` → `get()` with the TS fallback (declared per site) |

Each limit sits above everything the corpus reaches (the capture records the per-counter maxima);
results differ from the TS only where the TS hangs or exhausts memory. **Fuzz targets**:
`layout_docx` (bytes → read with small limits → layout with open fonts: no panic, terminates within
`max_work`, every index in the output valid, encode → decode round trip), `layout_model`
(structure-aware, uo-testkit generator), `emf_records`, `wmf_records`, `field_instr` (field formula,
date and number pictures, numbering formats), `hit_test` (random queries on laid-out pages),
`incremental` (milestone 2: random edits, incremental == full), `omml`.

### 8. JS semantics and risks (each becomes a declared item per frontier)

1. **Floats and `Math`**: 350 `Math.max`, 155 `Math.min`, 88 `Math.round`, 43 `floor`, 36 `ceil`,
   58 `sin`/`cos`, 8 `hypot`, 5 `tan`, 1 `pow`, 1 `exp`, 2 `clz32` in the ported files — all through
   uo-jsnum; `Math.max(...[])` = −∞ (lines.ts `nextTab`); `x | 0`, `>>> 0` as `uo_jsnum::conv`.
2. **Iteration order**: `Map`/`Set` insertion order (`IndexMap`/`IndexSet`); plain-object records
   iterate integer-like keys first (e.g. `Object.values(styles).find(...)`, engine.ts:1672, over
   styles keyed by `styleId`) — the uo-docx `StyleSheet` order must be the TS object order.
3. **Identity** (LA3) and **module state** (LA3 last rows).
4. **Closures** stored in data (`RowLayout.split`, `CellLayouter`, `ParaEnv.field`) → data + traits.
5. **Text**: UTF-16 offsets (`text.length`, `codePointAt` with lone surrogates, `Array.from` by code
   point, `String.fromCodePoint`), case mapping (`toUpperCase` for caps, `doNotHyphenateCaps`) via
   `uo_jsnum::text`, NFC of hyphenation clusters and `\p{M}` via uo-text (LQ11).
6. **Regex** (≈ 80 sites, mostly field instructions): JS `\s` (Unicode white space + line
   terminators + U+FEFF), ASCII `\b`, `/i` without `u` (simple case folding), `\p{M}` with `u` —
   hand matchers from a spec table, differential against JS `RegExp`.
7. **Exceptions**: a TS throw inside `layoutDocument` aborts the layout → `Err`; the TS `try/finally`
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

All frontiers are Opus (Models rule). Each compiles with clippy clean against the F0 types, runs its
facet of the capture continuously, and declares its sites (uo-docx A7 items 1–9, plus LA3 keys).

| # | Frontier | TS → Rust | Lines | Depends on |
|---|---|---|---|---|
| **F0** | foundation and harness | crate skeleton; `LDoc`, keys (LA2–LA3), `Engine` struct shell and `PassInput`/`PassFindings`, `Glyph`/`PreparedPara`/`Line`/`PlacedGlyph`/`CellContent`/`RowLayout`/`TableLayout` types (the spike's `linedata.rs` as seed), `LayoutOptions`, `LayoutError`, `LayoutLimits`/`Budget`, `Layout`/`LayoutPage`/`LineGeom` + the uo-render item types (with the uo-render planner), `FaceTable`; the TS canonical walker, the provider transcript, the `.cap` capture over the corpus, the Rust replay test and `Canon` | — (new) | uo-docx A2/A4 types, uo-render item types |
| F1 | fonts and measuring | `fonts.ts`, `context.ts`, `deviceEmPt`, the 600-DPI width model of `para.ts` (`advanceTwips`, `wordWidth`, `scaledUnits`, `toWordUnit`, `spacingTwips`), `math.ts` `parseFont`; uo-fonts `registry` (Sonnet-portable, designed in LA6) | ≈ 900 | F0 |
| F2 | Word semantics | `resolve.ts`, `paraformat.ts`, `directRPr`/`directPPr`, `resolvePPr`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `numpages.ts`, `outline.ts`, `revisions.ts` (placement LQ3) | ≈ 1,350 | F0 |
| F3 | drawing values and metafiles | `geometry.ts` (layout half), `pictureLook`, `resolvePageFill` + `patternRowsFromImage`, `resolveTextFx`, `emf.ts`, `wmf.ts` | ≈ 1,400 | F0, F1 (faces), uo-drawingml |
| F4 | paragraph preparation | `para.ts` (minus F1's width model), `hyphenate.ts`, `math.ts` layout | ≈ 1,650 | F1, F2, F3 (`resolveTextFx`) |
| F5 | lines and paint | `lines.ts` (spike seed), `underline.ts`, `paint.ts` | ≈ 1,380 | F0 types (can start with F4, on captured glyph arrays) |
| F6 | tables | `table.ts` | ≈ 1,410 | F4, F5 (through `CellLayouter`) |
| F7a | engine core | `engine.ts`: pass loop, balancing, `run`, page/section/column geometry, `placeParagraph`, `layoutStatic`, keep rules, border groups, line numbers; `frames.ts`, `chain.ts` (`derive.rs`) | ≈ 1,900 | F4, F5 |
| F7b | engine: tables and floats | `placeTable`, floating/late tables, row emission, `placeFloats`, `wrapFrame`, objects, textboxes and chains | ≈ 1,500 | F6, F7a |
| F7c | engine: notes, headers/footers, fields | notes, separators, note columns, endnotes, `layoutHeaderFooter`, SEQ/REF/NOTEREF/STYLEREF/PAGEREF/chapters | ≈ 1,000 | F7a |
| F8 | editor-facing API and oracle | `refs.ts`, `run-boxes.ts`, `caretXIn`/`offsetAtX`/`locateRef`, hit tests, selection rectangles, `navigation`, `font_usage`, `collect_fields`, `pages_of`, `next_hyphenation`; the `LayoutOracle` impl in the bindings; `ViewTransform` (LQ2) | ≈ 400 + new | F7a–c |
| (M2) | incremental relayout | LA7 levels 1 and 2, differential tests | new | F7, D2a |

**Parallelism.** Wave 0: F0 alone (it fixes every shared type; ≈ 1–1.5 days of agent time). Wave 1:
F1, F2, F3 and F5 in parallel. Wave 2: F4, then F6 (F5 is done). Wave 3: F7a, then F7b and F7c in
parallel (separate `impl Engine` modules on one struct), then F8. Parity, memory and fuzzing run
continuously from wave 1 (facets) and on P1 from F7a. uo-render's codec and uo-pdf can proceed in
parallel with waves 1–3 against the F0 item types. uo-drawingml phase 2 is untouched by this work
(its raster/effects are used, not modified).

### 10. Estimate

| Part | Hours (agent time, through gate 2, before maintainer latency) | Basis |
|---|---|---|
| F0 foundation + harness (walker, transcript, capture, replay, `Canon`) | 10–14 | assumed; uo-docx A6/A7 harness work is comparable |
| F1–F8 port, ≈ 12.3k lines on Opus | 15–22 | uo-opc's Opus port ran at ≈ 1,300 lines/h port-only; layout is denser: 550–800 lines/h assumed |
| parity rounds (rule-dense module, 2,742 + generated documents) | 15–25 | assumed; the spike's line breaker had 0 mismatches on the first run, the engine's pass state is the risk |
| editor API + oracle + view transforms (F8) | 5–8 | assumed |
| memory and security verification, fuzzing | 8–12 | uo-fonts stage 4 ≈ 160 min for 2.7k lines, scaled and Opus |
| **total** | **≈ 55–80 (midpoint ≈ 68)** | the uo-docx record's hybrid figure gave layout ≈ 30–35 h of its Opus share; this plan adds the harness, the painter boundary and the editor API it left out |

Critical path (waves): ≈ 35–45 h. Not included: the M2 incremental relayout (≈ 10–15 h, milestone
2), uo-render and uo-pdf, the TS painter rewrite, real-Safari/BrowserStack runs.

## Milestone-1 gate — questions (LQ1–LQ14)

Batched into the milestone-1 gate with uo-docx G1–G27. Answers to the three questions uo-docx left
for this plan first.

| # | Question | Recommendation |
|---|---|---|
| **G21** (uo-docx) | `LayoutOracle` shape | **A4 as written, with four changes**: (1) `field_values` returns `(RunRef, Utf16)` with the TS semantics (last value per run, first-set order; runs the TS copied — slices, frame copies — are absent); (2) `pages_of` returns `Option<PageInfo { index: u32, displayed: Utf16 }>` (PAGEREF and TOC need the number as displayed in the section's format, `bookmarkPages()`); (3) **no `InlineMeasurer`** (LQ1); (4) the implementation reuses the binding's last `Layout` when the view's `formatting_version` and story roots are unchanged (`LayoutStamp`). `next_hyphenation` and `font_usage` as written. |
| **G19** (uo-docx) | legacy canvas views | **LQ2** |
| **G15** (uo-docx) | codec shared with uo-render | **Own schema-driven generator** (one schema → Rust enums/structs and the TS encoder/decoder; little-endian, varints for counts and ids, f64 for coordinates (LQ5), UTF-16 strings, interned symbols per layout), shared by the session `exec`/`query` and the display list. No third-party format (flatbuffers/capnp/serde formats add runtime code on both sides and do not give exact JS-number round trips without care). Pages are encoded on demand, one page per buffer, with a resource-table delta (faces, images, symbols first seen). Decided in the uo-render plan; this plan needs only: plain-data item types, per-page encoding, stable ids. |
| LQ1 | Drop uo-docx A4's `InlineMeasurer` | **Yes** — the TS engine never calls `InlineRenderer.measure` (only `plugins.test.ts:11`); custom inlines lay out their fallback text and renderers draw over the run box. A future measuring plugin API would be a new feature. |
| LQ2 | G19: legacy canvas layout (web/draft/outline views, XPS export, `layout="legacy"`) | **Drop it; port nothing of `breakRich`/`paginate`/`lineHeightPt`.** Web view = the engine over a view transform equal to the viewer's `webDocument` (views.ts:28: page width = viewer width, no headers/footers, no columns, continuous sections) — the viewer already works this way; Draft = the same transform at the section's text width; Outline = an engine transform (paragraphs indented by outline level, collapsed levels hidden) — designed with milestone 2; XPS export from the display list (milestone 2, with uo-pdf's painter structure). Header/footer and textbox editing already run on the engine. The views look like the viewer's web view instead of the legacy renderer — **accepted by name**, CHANGELOG. |
| LQ3 | Where the Word-semantics files go (`resolve.ts`, `paraformat.ts`, `directRPr`/`directPPr`/`resolvePPr`, `numbering.ts`, `datefmt.ts`, `field-formula.ts`, `outline.ts`, `numpages.ts`, revision colours) | **`uo_docx::word`**, ported by frontier F2 (Opus). The editor uses them without layout (`StyleResolver`, `cascadeRPr`, `mergePPr`, `effectivePPr`, `inheritedPPr`, `fontOfSlot`, `resolveColor`, `formatNumber`, `formatWordDate`, `documentOutline`, `listItemValues`, `numberedItems`, `outlineParagraphs`, `hasCoverPage`, `numPagesShown`, `authorColor` are imported by universal-write.ts and the viewer) — through the oracle they would make the session and its tests depend on layout. uo-docx-layout depends on uo-docx anyway. |
| LQ4 | Incremental relayout: equality with the full layout as the criterion; the TS fast path's transient output (and its 1.5 s idle relayout) not reproduced; milestone 1 ships the API with a full relayout behind it; levels 1–2 (LA7) in milestone 2; `Engine` designed checkpointable now | **Yes** |
| LQ5 | `f64` in the display list and in the encoding (exact parity; the canvas takes doubles); `f32` only as a stage-5 candidate measured by a pixel compare | **Yes** |
| LQ6 | Painter boundary: the TS canvas painter becomes a decoder; every value the painters derive from model objects or fonts (shape paths from `ShapeStyle`, `InlineImage` look-ups, glyph outlines/COLR for `gid`/`asPath` runs, symbol remaps, batching hints) is resolved in Rust; raster helpers used by both painters (gradient/pattern tiles, page-fill tiles, text-fx layers, picture/shape effect rasters) live in `uo-render::paint` and are exported to wasm; painter parity = equality of the canvas draw-call log (a recording 2D context) between the TS painter on TS items and the new painter on Rust pages, over the corpus | **Yes** (details in the uo-render plan) |
| LQ7 | EMF/WMF renderer placement | **`uo-docx-layout::metafile` now** (it needs the face resolver and emits display-list items), with an app-neutral signature (`trait FaceLookup`); moves to a shared crate when Slides ports its EMF path (`slides-core/emfplus.ts`). This corrects the uo-docx estimate table, which put `emf.ts`/`wmf.ts` (781) in uo-image; uo-image keeps the codecs only. |
| LQ8 | Fonts: (a) `FontRegistry` + `FaceCache` + the per-document face budget as a new **uo-fonts `registry`** module (app-neutral, one cache per wasm instance for every app), with a new `Face::heap_bytes()`; (b) the numbers of LA6 (wasm32: 128 MiB per document, 64 MiB unpinned cache; native: 1 GiB / 512 MiB) as first values, measured at gate 2; (c) for the .NET server PDF, port the deferred **`uo_fonts::system`** index (C# `SystemFonts.cs`, `write/tools/corpus-lab/node-fonts.mjs`) in milestone 1 (Sonnet, closed scope) instead of feeding the registry from C# | **Yes to all three**; (c) alternative: the C# facade enumerates and feeds bytes (keeps `SystemFontProvider.cs` alive one more release) |
| LQ9 | No-panic policy: `#![deny(clippy::indexing_slicing)]` in this crate; a TS `x[i]!` that would throw → `Err(Internal)`; a TS read of `undefined` → `get()` with the TS fallback, declared per site | **Yes** (wasm `panic = "abort"` would kill every open document) |
| LQ10 | Dates: `LayoutOptions { now: LocalDateTime, tz: TimeZoneRule }` from the binding's `Env` (offset at any instant, for `new Date(iso)` of CREATEDATE/SAVEDATE/PRINTDATE); captures under `TZ=UTC` plus one Sao Paulo facet | **Yes** |
| LQ11 | Pull the Unicode part of uo-docx G10 (`uo-text`) into milestone 1: NFC composition for hyphenation clusters (hyphenate.ts:126), `\p{M}`, JS `\s`, generated from Node 24.15 with a regeneration diff test; case mapping stays in `uo_jsnum::text` | **Yes** (collation stays milestone 2) |
| LQ12 | Drop the lab-only options (`onWordFit`, `advanceEm: 'device'`, `advanceRound` ≠ `'word'`, `justifyShrink` override) from the 0.10 API; the calibration labs (`write/tools/layout-oracle/analyze-*`) keep running on frozen `main` | **Yes**, CHANGELOG |
| LQ13 | Recursion: keep the TS recursion (faithful), bounded by `max_nesting` 64 and a layout depth guard; set the wasm stack by measurement (link arg `-z stack-size`, target ≤ 8 MiB) with a test at depth 64 through read → layout → encode; an explicit-stack rewrite only if 8 MiB is not enough (stage 5) | **Yes** (joins uo-docx G20's wasm test) |
| LQ14 | `LayoutLimits` outcomes (section 7): a limit returns `Err(Limit(kind))` (the viewer shows an error; the TS would hang or run out of memory), metafiles and field texts truncate with a warning, faces past the budget fall back | **Yes**, accepted by name; each limit above every corpus maximum (recorded) |

**Requirements on uo-docx** (for its D1a types; not questions): `DocView` always presents the
reconciled block order (never a stale `CellView`); `RunRef { story, path, run: u32 }` defined next to
`DocRef`; `read_embedded_fonts` returns `Arc<[u8]>` per face; `Paragraph`/`Run` constructible by
this crate (synthetic paragraphs, LA2); the `StyleSheet` iterates in TS object order (integer-like
`styleId`s first); `formatting_version` also bumps on font-table, theme and section changes.

## Analysis skeptic

Pending (Opus).

| # | Finding | Blocking | Decision |
|---|---|---|---|

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
