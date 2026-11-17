# uo-pptx — the Slides presentation model (architecture, plan §4.0)

Record of the rust-port workflow (hybrid mode, `SKILL.md` "Architecture pass (Opus), only for the big shapes").
Status: architecture pass done 2026-11-17 (Opus); **B1b (read path) ported 2026-11-17** (Sonnet): package session, deck caches,
`readPresentation`/`readSlide`/notes, embedded fonts, parity 100% (see "B1b result"). **B6 (timing, animation, playback, show,
media, slot evaluation) ported 2026-11-17** (see "B6 — timing and playback"). The editor and the exporters are still skeleton types.
The milestone-4 gate (plan §11) approves it together with `uo-dml-render.md` and the "Slides extensions"
section of `uo-render.md`.

Behaviour source (frozen, never edited): `D:\Projects\OfficeNext\.claude\worktrees\ts-ref-092` at `b92b60fa`
(0.9.2, `dist` built), called **TS** below. Line counts are **raw** lines of that checkout (the plan's 19,250
for slides-core is non-blank non-comment; raw is 24,595). Line numbers are of
`slides/packages/slides-core/src` unless a path is given.

## Decisions already applied (recorded; the maintainer may revisit them at the M4 gate)

| Id | Decision | Effect here |
|---|---|---|
| T1 | Text parity = advance tables + `TextRun` items | the Rust text layout takes measurements as input (`uo-dml-render.md` "Measurement protocol"); the canvas painter draws `TextRun` exactly as the TS `fillText` path; the PDF draws glyphs through `uo-fonts` |
| T2 | Font policy = the TS Slides rule (a font installed by name is drawn by the browser) | the display list carries the family, never a resolved file; the shell's `fontStack` builds the CSS string, the advance tables are measured with it |
| P1 | PDF scope = plan §9 M4 | `Effect` items are listed missing in the PDF until measured |
| C0 | Corpus = generated decks + committed samples (`slides/samples`, 53 files) | no downloads |
| W0 | Write's behaviour and its twips unit unchanged | the Slides list is a separate message (`uo-render.md` "Slides extensions") |
| A1 (B1a, 2026-11-17) | The slide model is owned trees, not arenas (`ShapeArena` and its ids are gone; see "B1b result" and `uo-dml-render.md` A1) | P1 point 2 and P4 below describe the arena design of the architecture pass; read them with that change |

## Scope

What `uo-pptx` owns: the **presentation** — the byte-part package session, the slide/master/layout/placeholder
graph and the slide model read over it, the deck-level text-style context, notes, embedded fonts, the timing
and animation model with the slot evaluation, playback and the slideshow state machine, media state, the
editing session (operations, snapshot undo, save), the `.ppt` converter (feature module) and the exporters.
What it does **not** own: anything Sheet also needs (shape reader, theme with the HSL colour profile, text
cascade mechanics, table styles, text layout, shape → display list, SmartArt, metafiles, PICT) — that is
`uo-dml-render` (plan §4.5, `uo-dml-render.md`).

### TS covered by uo-pptx

| TS file | Raw lines | Rust module (`uo_pptx::…`) | Frontier |
|---|---|---|---|
| `package.ts` | 73 | `package` (`PartStore`, generations, snapshots) | B1b |
| `xml.ts` | 132 | — reused: `uo_opc::xml` lenient parser, slides preset (`ContentPi::Skip`, `CharRefPolicy::Replace`, no depth limit; the lone-surrogate join is uo-opc's declared decision) | B1b |
| `read.ts` deck half: `readRels` callers, `slideNumberOf`, `readPresentation`, `readShowSettings`, `themeFor`, `findRelTarget`, `readSlideNotes`, `readSlide`, `hasBackground`, `countTextUnits`, `parseTransition`, `parseTiming`, `readCondDelay`, `deepFindAttr`, `phKey`, `placeholderXfrmMap`, `placeholderBodyPrMap`, `masterTypeOf`, `buildTextStyleContext`, `collectPhStyles`, `chartFileFormat` (1–731, 1134–1153) | ≈ 750 | `deck::read` | B1b |
| `model.ts` deck types: `Presentation`, `ShowSettings`, `SlideRef`, `Slide`, `SlideAnimEntry`, `SlideTransition` | ≈ 110 of 558 | `model` (the shape types are `uo_dml_render::model`) | skeleton |
| `fonts.ts` | 133 | `fonts` | B1b |
| `eot.ts` | 542 | **moves to `uo-fonts`** (`eot::read` + MicroType Express, next to the existing writer `uo-fonts/src/eot.rs`) | B5b |
| `timing.ts` | 830 | `timing` | B6 |
| `anim-engine.ts` | 686 | `anim::engine` | B6 |
| `anim-formula.ts` | 176 | `anim::formula` | B6 |
| `playback.ts` | 283 | `playback` | B6 |
| `slideshow.ts` | 434 | `show` | B6 |
| `media.ts` | 162 | `media` | B6 |
| `ppt/*` (16 files) | 5,259 | `ppt` (feature `ppt`) | B2 |
| `export/model.ts`, `export/images.ts`, `export/markdown.ts`, `export/html.ts`, `export/pdf.ts` | ≈ 370 | `export` (feature `export`; the converter wrappers become Rust exporters, `export/raster.ts` 145 is `uo-image`) | after B7 |
| editor `slides/packages/slides/src/deck-edit.ts`, `xml-edit.ts`, `history.ts`, `blank-deck.ts` | 1,304 + 192 + 86 + 231 | `edit::{ops, xml, history, blank}` (feature `edit`) | B8 |
| C# `UniversalOffice.Pptx` (`PptxManipulate.SetRunText`, `PptxPdf`) | 2,610 C# | `edit::ops` / `export`; the C# becomes a facade (plan §9) | B8, M6 |

Total ported into uo-pptx ≈ **9,100 raw TS lines** (+ 1,813 editor, + 542 EOT to uo-fonts).

### Stays in the TS shell (the viewer/editor packages)

`reveal.ts` (245: `applyReveal`, `applySoftReveal`, canvas clip/mask code driven by slot values),
`transition.ts` (570: `paintTransition`, compositing of two slide bitmaps; the transition *state* is `show`),
the viewer `slides/packages/viewer/src/*` (2,016: element, controls, a11y, media player — over the wasm), the
editor element/chrome/inline text overlay. The canvas effect code of `render.ts` that reads pixels moves to
the painter (see `uo-dml-render.md`). These are canvas code; their parity is "same code, same arguments".

## Data model (ownership: arenas and indices, no `Rc<RefCell>`)

### P1. Principles

1. **Three memory scopes.** *Deck* (lives with the open file): `PartStore`, the parse caches, the media
   table, the theme table, the master/layout read cache. *Slide* (rebuilt when stale): `SlideModel`, flat
   arenas. *Emit* (per slide × scale × origin): the display list and its scratch arenas (`uo-dml-render`).
   No object of an inner scope is referenced from an outer one except through ids checked against a
   generation.
2. **The TS slide model is a resolved, flattened copy** (`readSlide` returns shapes with layout/master
   inheritance applied, inherited decorations prepended with `inherited: true`, styles cascaded into every
   run). Rust keeps that contract: `SlideModel` is plain data with the same fields; the graph
   (slide → layout → master → theme) is *navigated* at read time through the part store, never stored as
   object links.
3. **Every TS `?:` field is an `Option`** (the model-JSON capture distinguishes absent from `false`/`0`;
   `readSlide` copies with `!== undefined` tests). Booleans the TS always sets are `bool`.
4. **Bytes are shared, never copied**: part bytes are `Arc<[u8]>`; an `ImageRef` points at the media table
   entry, which holds the same `Arc` as the part store (the TS `ImageRef.bytes` *is* the package's
   `Uint8Array`).

### P2. The package session (`package`)

```rust
pub struct PartStore {                 // the TS `Map<string, Uint8Array>` with JS Map semantics
    entries: Vec<PartEntry>,           // insertion order = ZIP order on save (package.ts:46)
    index: HashMap<Box<str>, u32>,     // name -> slot (rustc-hash), rebuilt on removal
    next_gen: u64,
}
pub struct PartEntry { pub name: Box<str>, pub bytes: Arc<[u8]>, pub generation: PartGen }  // `gen` is reserved in edition 2024
```

- `set_part` on an existing name keeps its position and bumps `gen`; on a new name appends; `remove_part`
  shifts the rest (O(n), parts are few). This is exactly `Map.set`/`Map.delete`, which `history.ts restore`
  depends on (a removed part restored by undo is appended at the end, in snapshot order).
- `part_text` = `TextDecoder` (UTF-8, BOM stripped, U+FFFD replacement); `set_part_text` = `TextEncoder`.
  A part edited by text loses its BOM — TS behaviour, kept. Helper: `uo_jsnum::text` decode.
- `PartGen` is a per-store monotone counter: identity of a part version. Caches and read sets key on
  `(slot name, gen)`; the TS's `Uint8Array` identity tests (`changedSince`, `restore`) become `gen`
  comparisons (`Arc::ptr_eq` would also work but would keep old bytes alive inside read sets).
- `load`: `.ppt` (CFB magic) → `ppt::convert` (feature) → parts directly (the TS zips and unzips again,
  `package.ts:25`; skipping that round trip is behaviour-neutral: same entries, same order — declared). ZIP
  through `uo-opc` with the TS bomb filter (`originalSize <= max(1 MiB, size × 1100)`) and the
  `ppt/presentation.xml` check, as typed errors.
- `save`: deflate level 6 in store order (`uo-opc` writer; parity is on decompressed bytes, as Write).

**Parse caches** (behaviour-neutral: the TS re-parses on every call): `XmlCache` maps `(name, gen)` →
`Arc<uo_opc::xml::Document>`, LRU with a byte cap (M5). `RelsCache` maps `(owner, gen of the .rels)` →
`Arc<Rels>` (ordered `Vec<(Id, Target, Type, Mode)>` + index; the TS `readRels` `Map` keeps first-set order
and `findRelTarget` scans in document order — both kept).

**`PartSource`** (defined in `uo-dml-render`, implemented here and by the future `uo-xlsx` package): what
the shared shape reader needs from a package — `part`, `xml` (through the cache), `resolve_rel_target`.

### P3. The deck (`deck`)

```rust
pub struct Deck {
    pub(crate) parts: PartStore,
    pub(crate) xml: XmlCache, pub(crate) rels: RelsCache,
    pub(crate) media: MediaTable,           // part name -> MediaId: Arc<[u8]>, mime, header size/dpi
    pub(crate) themes: ThemeTable,          // (theme part gen, effective clrMap) -> ThemeId, hash-consed
    pub(crate) masters: MasterCache,        // (layout part gen, master part gen, ThemeId) -> MasterRead
    pub(crate) slides: SlideCache,          // index -> (Arc<SlideModel>, ReadSet), LRU
    pub(crate) presentation: Option<(Presentation, ReadSet)>,
    pub(crate) limits: Limits,
}
```

- **The master/layout/placeholder graph.** `readSlide` reads the master and layout `spTree`s up to five
  times per slide (decorations, `placeholderXfrmMap` with *empty* rels, the placeholder `spPr` map,
  `placeholderBodyPrMap`, `collectPhStyles`), each time with the *slide's* theme (the slide's
  `clrMapOvr` changes the colours of master shapes). `MasterRead` caches, per `(layout gen, master gen,
  ThemeId)`, everything those passes produce: the decoration shapes (owned arenas), the three placeholder
  maps (`PhMap<Xfrm>`, `PhMap<PhShapeProps>`, `PhMap<BodyPrProps>`) and the master/layout halves of the
  `TextStyleContext`. Keys are the TS **strings** (`phKey`: `title#3`, `body`, `idx:3`), not a struct:
  collisions such as a `type="body#3"` are part of the behaviour. `PhMap` is an ordered small map with the
  TS write semantics of each site (`map.set` overwrites in place; `collectPhStyles` sets the type and
  `idx:` keys only if absent; the `spMap` merge is `{...prev, ...stripUndef(psh)}` = field-wise "`Some`
  overrides"). The cache is behaviour-neutral because the read is deterministic per key.
- **Per-slide copy, not sharing.** `readSlide` mutates inherited shapes afterwards (slide numbers in fields,
  the text cascade, `tx1`, `useBgFill` with the slide's background), so decorations are **cloned** into the
  slide arena (the TS also builds them fresh per slide); the cache saves the XML walk, not the copy.
- **Aliasing in the TS** (`sh.shadow = inh.shadow`, `sh.fill = slide.bg`, the `spMap` merge sharing
  objects): no later in-place mutation of the aliased sub-objects exists in `readSlide` (checked:
  `shadeToTitle` mutates `slide.bg.fillTo` *before* the `useBgFill` copy), so value copies are equal. B1b
  must re-check every new mutation site against this list.
- **Theme per part.** `themeFor(pkg, part)` = theme of the master + clrMap (slide override → layout
  override → master). `ThemeTable` hash-conses `(theme part, gen, clrMap entries)` → `ThemeId` →
  `uo_dml_render::Theme`. Chart themes (`chartThemeFor`, `themeOverride`) are entries too.
- **Deck-level memo replacements**: `slideNumCache` (WeakMap by package, keyed by the presentation bytes)
  and `chartFormatCache` (AppVersion of `docProps/app.xml`) become `Deck` fields invalidated by `gen`.

### P4. The slide model (`model::SlideModel`)

```rust
pub struct SlideModel {
    pub shapes: Vec<Shape>,               // z-order; inherited decorations first (uo_dml_render::model::Shape)
    pub arena: ShapeArena,                // bodies, paragraphs, runs, tables, rows, cells, images, cust geoms
    pub bg: Option<Fill>, pub bg_image: Option<ImageId>, pub bg_tile: Option<bool>,
    pub animations: Option<Vec<SlideAnimEntry>>,  // legacy "cascade fade" list (parseTiming)
    pub transition: Option<SlideTransition>,      // legacy (parseTransition)
    pub timing: Option<TimingModel>,              // parseSlideTiming: always set by readSlide
    pub theme: ThemeId,                           // the slide's effective theme (not in the TS model: kept for emit)
    pub read_set: ReadSet,                        // parts read, with their gens (stale check)
}
```

- `ShapeArena` (in `uo-dml-render`) is flat: `bodies: Vec<TextBody>` (paragraph `Span`), `paras`
  (run `Span`), `runs`, `tables`/`rows`/`cells`, `images: Vec<ImageRef>`, `cust: Vec<CustGeom>`,
  `strings` for long free text. `Shape.tx_body: Option<BodyId>`, `TableCell.tx_body: Option<BodyId>`. The
  read builds once and then only mutates fields in place (cascade, numbering), never splices, so spans are
  stable. Render-time variants (`cellBody`, the stacked WordArt body, `{...tb, wrap: 'none'}`) live in the
  emitter's scratch arena, never in the model.
- **`ReadSet`**: every `(part name, gen | Absent)` the read touched, including parts it looked for and did
  not find (a `.rels` added later must invalidate). A slide is stale when any entry differs from the store.
  This is the **per-slide re-read after edits**: the editor replaces part bytes; the next `slide(i)` sees a
  stale read set and calls `read_slide` again (exactly what the TS does after every edit — it re-reads the
  edited slide; here an edit to a layout also invalidates every slide that read it, which the TS gets by
  re-reading everything on demand).

### P5. Timing and animation (`timing`, `anim`, `playback`, `show`, `media`)

```rust
pub struct TimingModel {                  // SlideTimingModel
    pub transition: Option<TransitionInfo>,
    pub steps: Vec<AnimStep>, pub interactive: Vec<EffectTiming>, pub hidden: bool,
    pub anim_bg: Option<Vec<Box<str>>>, pub media: Option<Vec<SlideMediaInfo>>,
    pub bookmark_triggers: Option<Vec<BookmarkTrigger>>,
    pub nodes: TimeArena,                 // every TimeNode of the slide; EffectTiming.node is a NodeId
}
pub struct TimeArena { pub nodes: Vec<TimeNode>, pub children: Vec<NodeId> }   // node.children: Span into `children`
```

- `TimeNode` mirrors `timing.ts:76` field by field (`dur: Option<f64>` with `f64::INFINITY` for
  "indefinite", `repeat`, `tav` keyframes, `byHsl`, …); recursion (`parseTimeNode`) becomes an explicit
  stack with a depth limit (`Limits::max_time_depth`).
- `EffectTiming.node` (an object reference in the TS) is a `NodeId`; `PlacedEffect` extends it with the
  playback offsets. `AnimTarget`, `TimeCond`, `AnimStep`, `BookmarkTrigger`, `SlideMediaInfo` are plain.
- **The animation engine** (`anim::engine`, `shapeAnimHooks`) is lazy in the TS: the painter calls
  `all(base)`, `bg(base)`, `para(i, base)`, `unit.state(u, base, word)`, `chartParts[j].state(base)` with
  the geometry it just computed. In Rust the emitter computes the same bases (in px, normalized exactly as
  `boxBase`) and stores them in the static list's **slot table template**; per frame
  `anim::evaluate(frame_ms, &template) -> SlotTable` runs the hooks over the stored bases. Slots, the
  re-emit rule and the per-scale dependency are specified in `uo-render.md` "Slides extensions".
- `PartState` (`anim-engine.ts:52`) is the slot value; `AnimBase`, `UnitRef`, `AnimFilter` mirror the TS.
- **Caches**: `pathCache` (motion paths by string) and the formula cache of `anim-formula.ts` become capped
  maps owned by the deck view (M5).
- `playback` (timeline, `playbackStateAt`, `shapeOpacityAt`), `show` (`SlideShow` state machine: input
  `now`, actions, frames) and `media` (`mediaStateAt`, `gifFrameAt`, fades) are pure functions of time and
  the timing models; the shell drives the clock and the media elements.

### P6. The editing session and snapshot undo (`edit`, feature `edit`)

```rust
pub struct Editor { deck: Deck, history: EditHistory }
pub struct PartsSnapshot(Vec<(Box<str>, Arc<[u8]>, PartGen)>);   // shallow: one Arc per part, store order
pub struct EditHistory { steps: VecDeque<Step>, pos: usize, saved_pos: Option<usize>, limit: usize /* 100 */,
                         byte_cap: usize }
pub struct Step { label: Box<str>, before: PartsSnapshot, after: PartsSnapshot,
                  state_before: EditState, state_after: EditState }
pub struct EditState { pub index: u32, pub shape_id: Option<Box<str>> }
```

- Port of `history.ts` 1:1 (push truncates the redo tail, `savedPos` bookkeeping, `limit` 100,
  `changedSince` by size + identity, `restore` removing missing parts then setting differing ones *in
  snapshot order*). The **byte cap** is new (M5): the bytes retained by distinct part versions across the
  history are counted once per `PartGen`; past the cap the oldest steps go (as when `limit` is exceeded).
  Declared: the TS has only the 100-step limit.
- **Operations** (`deck-edit.ts`, 46 exports) are string surgery over part text (`xml-edit.ts`: `tagEnd`,
  `findElements` with a depth counter, `setAttr`, `splice`, `insertChild`…). Rust works on `String`
  (UTF-8) with **byte** offsets: every offset is produced by a scan for ASCII tokens and consumed by
  slice/splice, so byte and UTF-16 positions select the same text. Trap for B8: any arithmetic with the
  `.length` of non-ASCII text must go through `uo_text::utf16`; each such site is listed in the B8 record.
  The `RegExp` of `findElements` (`<(/?)name(?=[\s/>])`, `\s` = JS white space) is a hand matcher.
- Strings from JS (inserted text) cross the boundary with U+FFFD replacement of lone surrogates — the same
  as `TextEncoder` in `setPartText`.
- Every operation is `fn op(&mut Deck, …) -> Result<Outcome, EditError>`; the editor wraps it in the step
  protocol: snapshot before → op → if `changed_since` then push. An `Err` restores the snapshot (no partial
  change), as Write's D2a-T1.

### P7. The `.ppt` converter (`ppt`, feature `ppt`)

*(Ported: see "B2 — .ppt converter" below. The part maps are returned as parts; the lazily loaded `uo-ppt.wasm` wrapper that
zips them is B7's.)*

Bytes → parts, built on `uo_opc::cfb` (Slides' `cfb.ts` differs from Write's in 4 lines: a `Flavor` there if
needed) and `uo_drawingml::odraw` (`Flavor::Slides`, `msospt::PresetVariant`). Output = the **same
in-memory XML bytes** as the TS (`model-regress.mjs` hashes), so every number written into XML goes through
`uo_jsnum::format` (`String(n)`, `toFixed`), and `roundtrip.ts`'s `partTextCache`/`metroCache` are per
conversion scratch maps (dropped at the end). In wasm it is a **separate lazily loaded module**
(`uo-ppt.wasm`, bytes → stored ZIP bytes; a second instance cannot share memory, so it returns a container
the main module opens).

## Public API (first shape; the binding codec is designed with B7)

```rust
// uo_pptx
pub struct Deck;            impl Deck {
    pub fn open(bytes: &[u8], limits: Limits) -> Result<Deck, PptxError>;      // .ppt via feature `ppt`
    pub fn from_parts(parts: Vec<(Box<str>, Arc<[u8]>)>, limits: Limits) -> Result<Deck, PptxError>;
    pub fn presentation(&mut self) -> Result<&Presentation, PptxError>;
    pub fn slide(&mut self, index: u32) -> Result<Arc<SlideModel>, PptxError>;  // cached; re-read when stale
    pub fn slide_notes(&mut self, index: u32) -> Result<String, PptxError>;
    pub fn timing_models(&mut self) -> Vec<Option<Arc<TimingModel>>>;
    pub fn embedded_fonts(&self) -> Vec<EmbeddedFont>;  pub fn deck_typefaces(&self) -> Vec<Box<str>>;
    pub fn save(&self) -> Result<Vec<u8>, PptxError>;
    pub fn clear_caches(&mut self);
}
pub fn emit_slide(deck: &mut Deck, index: u32, view: &ViewParams, measures: &MeasureTable)
    -> Result<Emitted, NeedMeasures>;           // two-phase (uo-dml-render "Measurement protocol")
pub fn evaluate_slots(template: &SlotTemplate, t_ms: f64, view: &ViewParams) -> SlotTable;
pub struct SlideShow;  pub struct PlaybackTimeline;      // B6
pub struct Editor;                                        // feature `edit`, B8
```

Changed from 0.9 for the better (CHANGELOG at the switch): the shell never sees a `Slide` object graph; it
gets display lists, slot tables, hit/link/find data and the a11y text; `PaintResources.charts`,
`paintSlideTo`'s callbacks (`shapeOpacity`, `shapeAnim`, `mediaFrame`) become slot/media tables.

## Frontiers B1–B8 (closed scope per frontier)

Sizes are raw TS lines moved into Rust. Models per `SKILL.md`. Plan §6 order, with the corrections of
"Plan corrections" below.

| # | Crate · files | API delivered | Capture points (scripts owned by the capture agent under `core/crates/uo-pptx/js/`) | Lines | Model |
|---|---|---|---|---|---|
| **B1a** | `uo-dml-render` · `read.ts` shape half (732–2320 minus 1134–1153), `theme.ts`, `text-style.ts`, `table-style.ts`, `table-styles-builtin.ts`, `symbol-fonts.ts` | `read_shapes`, `read_drawing_elements` (Sheet), `read_theme`, `color_from_dml` (HSL profile), `apply_shape_text_cascade`, `apply_text_styles`, `merge_style_levels`, table styles | per-shape model JSON (canonicalised, see Parity), `colorFromDml` facet over generated colour elements | ≈ 2,680 | Sonnet |
| **B1b** | `uo-pptx` · `package.ts`, `read.ts` deck half, `fonts.ts`, deck types | `PartStore`, caches, `Deck::{open, presentation, slide, slide_notes, embedded_fonts}` | `Slide` model JSON per slide; presentation JSON; notes text | ≈ 960 + new session code | Sonnet |
| **B2** | `uo-pptx::ppt` · `ppt/*` | `ppt::convert(bytes) -> parts` | `.ppt` → part bytes (byte-exact per part), `model-regress` hashes | 5,259 | Sonnet |
| **B3** | `uo-dml-render::text` · `text-layout.ts`, `pptMeasure` and the plain measurer (render.ts 2085–2150, 1889), `diagram-metrics.ts`; new `uo_text::grapheme` | `layout_text_body`, `MeasureTable`, `need_measures`, `format_auto_num` | `layoutTextBody` inputs/outputs with the measurement log of the real TS run | ≈ 840 + grapheme | **Opus** |
| **B4** | `uo-dml-render::emit` · `render.ts` minus the shell half, `raster.ts`, `chart-engine.ts` bridge; the uo-render Slides list (its own sub-frontier, `uo-render.md`) | `emit_shapes`, `emit_slide` | draw-op stream (primary), Chromium pixels (second) | ≈ 2,400 | Opus architecture (done), **Sonnet** port |
| **B5a** | `uo-dml-render::smartart` · `diagram-layout.ts`, `diagram-fallback.ts` | SmartArt regeneration and fallback inside the reader | SmartArt `dsp:spTree` text + shapes JSON | 2,926 | Sonnet |
| **B5b** | `uo-dml-render::metafile` · `metafile.ts`, `emfplus.ts`, `pict.ts`; `uo-image` TIFF variant (`tiff.ts`); `uo-fonts` EOT reader (`eot.ts`) | metafile/PICT → display sub-list at display size; `eot::read` | op stream of the metafile layer; TIFF RGBA; EOT → sfnt bytes | 3,844 | Sonnet |
| **B6** | `uo-pptx::{timing, anim, playback, show, media}` | `parse_slide_timing`, `evaluate_slots`, timeline, `SlideShow` | timing model JSON; playback timelines; slot tables per sampled frame (from a recording of the hooks' inputs/outputs in the TS paint) | 2,571 | Sonnet; slot template reviewed by Opus |
| **B7** | `uo-wasm` + viewer shell | `<universal-slides-viewer engine="rust">`, `uo-slides.wasm`, hyperlink scheme filter (Write's rule, behind a switch) | viewer harness: op stream, pixels, links, find, a11y text, memory, faults, Safari | shell ≈ 600 touched | Sonnet |
| **B8** | `uo-pptx::edit` | `Editor`: the 46 operations, history, save with font embedding, per-slide re-read, routing behind `engine="rust"` | editor scripts: part bytes after every op and every undo/redo | 1,813 + C# `SetRunText` | Sonnet; session core reviewed by Opus |

Order: B1a → B1b → (B2 ∥ B3) → B4 → (B5a ∥ B5b) → B6 → B7 → B8, at most three agents in total (plan §3.1).
`uo-charts` (plan §6) plugs into B4 (`uo-dml-render.md` "Charts and number formats").

## Parity (capture points)

| Capture | Where in the TS | Compared |
|---|---|---|
| **Model JSON** | `readPresentation`, `readSlide` per slide, `readSlideNotes`, `readDrawingElements` (Sheet path, generated drawings) | canonical JSON: keys **sorted** (key creation order is not behaviour, except `Record` iteration sites, which get their own facets), `undefined` dropped, numbers by `uo_jsnum` `String(n)`, `ImageRef.bytes` as part name + length + hash, `chartXml` as the chart part's text hash, `Theme` XML lists as element paths |
| **`.ppt` → parts** | `convertPptToPptx` (before the ZIP) | every part's bytes, exact |
| **Text layout** | `layoutTextBody` with the measurement log | `PlacedLine`/`PlacedRun` fields by f64 bits; the log replays as the `MeasureTable` |
| **Timing** | `parseSlideTiming`, `buildPlaybackTimeline`, `playbackStateAt` | JSON by bits |
| **Slots** | recording of every hook call during `paintSlideTo` (`all`/`bg`/`para`/`unit`/`chartParts`: base in, `PartState` out) at sampled frames | `PartState` by bits |
| **Draw ops** | recording context over `paintSlideTo`/`paintShapesTo` (plan §4.3) | canonical op stream; Chromium pixels second |
| **Editor** | the 46 operations + undo/redo over generated decks | part bytes after each step |

Corpus (C0): seeded generated decks (plan §4.3 deck generator) + `slides/samples` (53 files incl. `legacy/`
`.ppt` and `playback/`), installed-font groups opt-in.

## JS semantics that are traps (helper to use)

| TS construct (site) | Trap | Helper |
|---|---|---|
| `String(n)` (slide numbers, CSS `${size}px`, `.ppt` XML, `toFixed(2)` in `blur()` strings) | shortest round-trip digits, exponent forms, `-0` → `"0"` | `uo_jsnum::format` |
| `parseInt(v, 10)`, `parseInt(h, 16)`, `parseFloat`, `Number(v)` on attributes | leading white space, trailing garbage, `NaN` flowing on | `uo_jsnum::parse`, `uo_jsnum::conv` |
| `Map` insertion order (`PartStore`, rels, `PhMap`s, `pbox`, `boxes`) | iteration order is behaviour; overwrite keeps position | `Vec` + index (own), as `PartStore` |
| `Record` key order (`Theme.colors`, `clrMap`, transition `attrs`, Sheet's `Object.entries(byName)`) | integer-like keys first, then creation order | `uo_jsnum::object` |
| `/\s/`, `/\S/`, `/^\s+$/`, `/\s+$/`, `trim()` | JS white space ≠ Unicode White_Space | `uo_jsnum::text::{is_js_space, trim}` |
| `PER_CHAR` regex (render.ts:2092) over UTF-16 | an astral character is two surrogates, which are outside every range: astral text is measured *whole* | `uo_text::utf16` (test code units, not chars) |
| `\p{Ll}`, `\p{M}` (`text-layout.ts:172`, `text-warp.ts`) | Unicode version of the reference | `uo_text::props` |
| `Intl.Segmenter('grapheme')` (`text-layout.ts:217`), fallback `[...t]` when absent | UAX #29 of the reference's ICU; a capability | **new** `uo_text::grapheme` (tables pinned to Node 24.15's Unicode), capability bit in the measurement table |
| `[...text].length` vs `.length` vs `slice` | code points vs UTF-16 units | `uo_text::utf16` |
| `toUpperCase` (alpha numbering, `cap="all"`) | full case mapping (`ß` → `SS`) | `uo_text::case` |
| `String.fromCharCode(96 + ((n-1) % 26) + 1)` (`formatAutoNum`) | `%` keeps the dividend's sign; `n = 0` gives `` ` `` | `uo_jsnum::math::rem` |
| `.sort(cmp)` (anim-engine 582/606, diagram-layout 143/1364/1621, diagram-fallback 92 with a `NaN`-returning comparator, emfplus 206/208), `.sort()` default (eot 516, fonts 132) | stability; `NaN` comparator results; default sort = UTF-16 code-unit order (≠ Rust `str` order for astral vs U+E000–FFFF) | `uo_jsnum::sort` (stable, f64 comparator), `uo_text::utf16::cmp` |
| `Math.round`, `Math.max/min` incl. `Math.min(...arr)` | half towards +∞; NaN wins; `Math.min()` = +∞; a spread over > ~10⁵ elements throws `RangeError` (caught by `guarded`: the shape is skipped) | `uo_jsnum::math`; the RangeError case is a declared difference on hostile input (Rust paints the shape) |
| `TextDecoder`/`TextEncoder` | BOM stripping, U+FFFD | `uo_jsnum::text` |
| `{...prev, ...stripUndef(x)}`, `Object.assign` | field-wise override by present fields | explicit `if let Some` per field |
| `String(Math.round(x))` → `parseInt` round trip (Sheet `placeElement`) | `x ≥ 1e21` prints `1e+21`, read back as `1` | reproduced in `FrameOverride` (`uo-dml-render`) |

## Memory plan (M1–M5)

**M5 — every cache of plan §10 gets a cap (or disappears):**

| TS cache | Where it goes | Cap / bound |
|---|---|---|
| `advCache` (render.ts:2090, cleared per paint) | `MeasureTable` held by the deck view, invalidated by the shell's font generation | 65,536 entries or 8 MiB per table, then cleared (the TS clears per paint, so values never depend on it) |
| `layerPool` (WeakMap of canvas layers) | shell painter | ≤ 4 free layers per canvas (the TS evicts other sizes); shell test |
| `pattCache` (8×8 pattern tiles) | `uo-dml-render` generated-bitmap cache | 256 entries |
| `pathGradCache` (≤ 192² rasters) | `uo-dml-render` | 64 entries / 16 MiB |
| `fxCache` (recoloured images) | shell (`Effect::ImageFx` results) | 64 images × 8 variants; shell test |
| `dpiCache` | `MediaTable` field | bounded by the package's media |
| emfplus `decoded` (embedded PNG/JPEG) | shell decode results | 128 entries |
| metafile `fontAvail` | `MeasureTable` `FontAvailable` entries | with the table |
| metafile `exPaths` (WeakMap) | per-rasterization scratch | dropped after the layer |
| anim `pathCache` | deck view | 256 paths |
| anim-formula `cache` | deck view | 1,024 formulas |
| diagram-metrics `cache` | static width tables | fixed (faces × styles) |
| `read.ts` WeakMaps `slideNumCache`, `chartFormatCache` | `Deck` fields | 1 each, invalidated by `gen` |
| ppt `partTextCache`, `metroCache` | per conversion | dropped at the end |
| *(new)* `XmlCache`, `RelsCache` | `Deck` | 64 MiB wasm32 / 256 MiB native, LRU |
| *(new)* `MasterCache` | `Deck` | 32 entries |
| *(new)* `SlideCache` | `Deck` | 16 slides, LRU (viewer neighbours) |
| *(new)* display lists per (slide, scale, origin) | deck view | 4 lists |
| *(new)* `ChartModel` cache (was `PaintResources.charts`, unbounded) | deck view | 32 models |
| `EditHistory` | `Editor` | 100 steps (TS) + byte cap 64 MiB wasm32 / 256 MiB native |

**M1**: `Deck::open` (ok and each error), `slide`, `slide_notes`, `save`, `emit_slide` (both phases),
`evaluate_slots`, every `SlideShow` transition, every editor op + undo/redo; live bytes back to the start
after `clear_caches`. **M2**: (a) slideshow 1,000 frames over a generated animated deck (per-letter
effects, motion paths, media) sampled every 50, flat; (b) editor: open → 100 edits → 50 undo → 50 redo →
save → close, 1,000×, 0 B after drop; (c) wasm `memory.buffer.byteLength` flat after warm-up. **M3**: first
budgets = TS heap deltas of `readSlide` and one paint, replaced by measurements; **retained `SlideModel` ≤
0.5× the TS `Slide` heap**; per frame ≤ 64 KiB allocated for a slot evaluation without re-emit. **M4**:
every handle (`Deck`, `Editor`, `SlideShow`, encoded list buffers, slot tables) 10,000 × create/drop, explicit
and finalizer paths, both bindings.

## Security and fuzz targets

Limits (`uo_pptx::Limits`, on top of `uo_opc::Limits`): slides, shapes per slide, group depth (`readGroup`
recursion → explicit stack), runs per body, table rows × columns (the border grids `hLines`/`vLines` are
`nR × nC`), custom-geometry commands, time-tree depth and nodes, `<p:iterate>` units, SmartArt depth/points,
metafile records and the generated-bitmap pixels, media bytes, history bytes. Errors are typed, never a
panic; the TS `guarded` per shape (a throwing shape is skipped) is the emitter's per-shape `Result`.

Fuzz targets (`core/fuzz`): `pptx_open` (bytes → presentation + every slide), `pptx_slide_xml`
(structure-aware mutation of slide/layout/master XML in a fixed package), `ppt_convert` (CFB bytes),
`timing_tree` (deep/wide `p:timing`), `anim_formula` (formula strings), `slideshow_drive` (random action
sequences), `deck_edit` (random op sequences on generated decks; the saved package must reopen), plus the
`uo-dml-render` targets (`uo-dml-render.md`).

## Wasm module split (plan §9; budgets fixed there)

| Module | Contents | Budget (gzip) |
|---|---|---|
| `uo-slides.wasm` | uo-pptx read + timing/anim/playback/show + uo-dml-render (reader, text layout, emit, SmartArt, metafiles) + uo-charts + uo-numfmt + uo-render Slides encoder | ≤ 1.6 MB |
| `uo-slides-editor.wasm` | the above + `edit` + save with font embedding (uo-fonts subset/EOT) | ≤ 3.5 MB |
| `uo-ppt.wasm` | `.ppt` → ZIP bytes (lazily loaded, separate instance) | not in the viewer budget |

## Plan corrections found in this pass

Listed in the plan's §14 as well; each with the evidence.

1. **Advance-table format** (plan §4.3 "advance at 1000 px rounded to 1/8 pt, kerning deltas"): the TS
   stores the *unrounded* em (`measureText(ch).width / 1000` at 1000 px, kerning `none`) and rounds per use
   (`Math.round(em × sizePt × 8) / 8`, render.ts:2134); kerning is a **whole-string** delta (`w1 − w0` at
   1000 px), not per pair; non-`PER_CHAR` strings are measured whole at the run's size; WordArt layout uses a
   second, plain measurer; unit animations measure at device scale. Full list in `uo-dml-render.md`.
2. **"Laid out once per slide" (B3)** is not bit-exact: the layout box is `(cx × scale) / scale`
   (render.ts:1621 `layoutTextBody(tb, w / scale, h / scale, …)`, `w = cx × scale`) and text-rect boxes are
   derived in px. Layout runs once per (slide, scale) — the display list is per scale anyway.
3. **Text warp** (`text-warp.ts`, 594) measures with `actualBoundingBox` at 10 px/pt and rasterizes each line
   into a canvas: in the viewer it is an `Effect` executed by the shell with the TS code (the plan's §9
   already lists warp missing in the PDF); its Rust port moves to the PDF effects wave. B3 shrinks by 594.
4. **"Own parser semantics" (B1)**: already ported — the `uo-opc` lenient parser has the slides-core options.
5. **Not "all" of slides-core moves**: `reveal.ts`, `transition.ts`, the pixel-reading effects of `render.ts`
   and the warp raster stay in the shell painter (≈ 1,900 raw lines of canvas code), driven by Rust values.
6. **B5 is 6.8k raw lines** (SmartArt 2.9k, metafiles/PICT/TIFF/EOT 3.8k), split into B5a ∥ B5b; at the
   Sonnet pace measured on Write ≈ 2 days, not 1.5.
7. **Slots depend on the viewport**: `AnimBase` is computed from px geometry normalized by the slide's px size
   (`boxBase`, render.ts:106), so a slot table is per (frame, scale, origin); unit bases need device-scale
   measurements (two-phase, see `uo-render.md`).
8. **Colour animation re-emits** (v1): `ov.fill` replaces the paint kind in `paintBody` (a gradient becomes a
   solid colour), so a colour slot cannot patch a precomputed item; colour, font and size changes re-emit the
   shape's sub-list for that frame (only that shape crosses).
9. **EOT reader**: confirmed missing; `uo-fonts` has only the writer (`eot.rs`). The reader + MTX goes there
   (B5b), not into uo-pptx.
10. **Grapheme segmentation** is missing in `uo-text` (`Intl.Segmenter` in `text-layout.ts:217`): a new
    shared piece, needed by B3.
11. **Image natural sizes come from the shell** in the viewer: `imgW/imgH` read the *decoded* image
    (Chromium applies EXIF orientation; SVG sizes are the browser's), used by tiles, crops and picture
    bullets; the server path reads headers through `uo-image`.

## Questions for the M4 gate (batched)

- SP1: the history byte cap (new) — accept the numbers above?
- SP2: canonical model JSON with sorted keys (key creation order not compared) — accept?
- SP3: the `Math.min(...hugeArray)` `RangeError` difference on hostile input — accept (Rust paints the shape)?
- SP4: `uo-ppt.wasm` as a separate lazily loaded instance — accept, or keep `.ppt` in the editor module?

## Analysis skeptic / Verification / Evolution / Final review

*(Not yet: hybrid mode, one skeptic and security review for milestone 4.)*

## B1b result (2026-11-17, Sonnet)

Ported 1:1 into `core/crates/uo-pptx`: `package.rs` (`PartStore` with `Map` semantics, `load` through `uo_opc::Package` with the
TS bomb filter, `save` deflate 6 in store order), `deck.rs` (`Deck::{open, open_with, from_parts, presentation, slide,
slide_notes, embedded_fonts, deck_typefaces, save, clear_caches, set_part, remove_part}`, the reading view `DeckSource`),
`read.rs` (deck half of `read.ts`: `readPresentation`, `readShowSettings`, `slideNumberOf`, `themeFor`, `readSlideNotes`,
`readSlide` with decorations, placeholder xfrm/spPr/bodyPr inheritance, the text cascade, table-cell cascade, background,
`parseTiming`, `parseTransition`, `phKey`, `masterTypeOf`, `buildTextStyleContext`, `collectPhStyles`, `hasBackground`,
`countTextUnits`), `fonts.rs` (`readEmbeddedFonts`, `collectDeckTypefaces`, odttf, sfnt style), `canon.rs` (feature `canon`: the
canonical JSON), `ppt.rs` (placeholder for B2). `xml.ts` is NOT ported: the `uo_opc::xml` lenient parser with the slides-core
options (`ContentPi::Skip`, `CharRefPolicy::Replace`, no depth limit) gives identical trees (the replay proves it over 253 decks).

### Parity (the gate)

Replay `tests/replay.rs` (runs in every `cargo test -p uo-pptx`: a self dev-dependency enables the feature `canon`):

| Capture | Decks | Facets compared | Equal | Mismatches |
|---|---|---|---|---|
| `model-b1a.capture` (200 generated decks, 500 slides, 5,360 shapes) | 200 | `input read pres slides n<N> nmodel` | 1,500 | **0** |
| `model-samples.capture` (53 samples; the 25 `.ppt` records wait for B2) | 28 | same | 472 | **0** |

`s<N>` and `model` include `slide.timing`, which is `parseSlideTiming` (B6): not replayed. The capture got the facets `n<N>` (the
slide without `timing`) and `nmodel` (`js/capture-model.mjs`); the existing facets were not changed (diffed before/after: 0
differences). Coverage beyond the generated decks comes from the samples: 18 slides with charts, 8 with SmartArt drawings, 11
with media, 4 with tables. Not covered by any capture, only by the unit tests (`tests/units.rs` of both crates): OLE preview from
VML, `chartImages`, `fillOverlay`, embedded fonts, `firstSlideNum`, the `xdr:` drawing path.

### Memory (M1–M5), measured by `uo-alloc`

- **M1** (`tests/memory.rs`, all green): `Deck::open` (ok, garbage, CFB, truncated, no presentation, `from_parts` error), every read
  operation + `clear_caches`, error paths of the reader (malformed slide, missing part), `PartStore` set/remove/snapshot/save:
  live bytes return **exactly** to the start. `MediaId`s of a slide model are invalid after `clear_caches` (the table is emptied).
- **M2**: 120 iterations of open → presentation → every slide + notes → save → drop: the series is constant at 37,380 B (tolerance 0);
  a second soak edits a part between reads (stale re-read) and is flat too. Wasm `memory.buffer` flatness comes with B7.
- **M3 budgets** (deck `0008`, 4 slides; slide 0 has 9 shapes): `open` allocates 472,866 B (peak 302,695, retained 131,608);
  `presentation` 21,313 B; first `slide(0)` allocates 859,898 B (peak 362,100) and keeps 278,928 B (model + parse cache); a cached
  `slide(0)` allocates 0; `save` 446,896 B. **Retained `SlideModel` alone: 135,955 B against 142,524 B for the TS `Slide` heap
  (measured in Node with `--expose-gc`, 20 slides): 0.95×, the target of 0.5× is NOT met.** Cause: `TextStyleLevel` is 512 B inline
  and an `lstStyle` holds nine of them (4.6 KB per body, cloned into every inherited decoration), `Shape` is 408 B and `Paragraph`
  368 B. Follow-up (does not change behaviour): compact levels (sparse or boxed bullet/spacing groups), `Arc` for the master's
  `lstStyle` bodies, move the picture-only fields of `Shape` into the boxed extras.
- **M5 caps**: `XmlCache` 64 MiB wasm32 / 256 MiB native LRU by `Document::heap_bytes` (`Limits::max_xml_cache_bytes`), slide cache
  16 slides LRU (`max_cached_slides`), media table bounded by the package's media. Tested past the cap: caps of 1 KiB, a third, a
  half and the whole XML size of a deck hold (`bytes() <= cap`); a part bigger than the cap is read but not kept; one-slot and
  two-slot slide caches evict in LRU order. `MasterCache` and the deck-level `slideNumCache`/`chartFormatCache` are NOT built
  (decisions 4 and 5): nothing to cap.

### Security

No panic over 11,880 mutated decks in the long run (`UO_PPTX_NOPANIC_ROUNDS=60`, 23 s release) and 792 in the default run (1.6 s
release, 15 s debug): truncations, cuts, duplications, byte flips, hostile tokens, 50–450-deep group insertions, plus 120 mutated
ZIPs. A 10,000-deep group is a typed error (`ReadError::Limit`) at `Limits::max_group_depth` (64), never a stack overflow. Caps
added (the TS has none): shapes per list, group depth, `rows × columns` of a table. Fuzz targets `pptx_open` and `dml_shapes`
(`core/fuzz/src/pptx.rs`; invariants: the same deck/tree read twice gives the same canonical text, a saved package reopens, the
shape cap holds): 1.27 M and 1.73 M executions in 25 s each, 0 crashes (the first run found only a harness bug: `==` on a model
holding NaN and `Debug` of a parsed tree that lists a hash map; both comparisons now use the canonical text).

### Decisions outside the source semantics (declared)

1. **Owned trees, not arenas** (`uo-dml-render.md` A1): `SlideModel.shapes` is `Vec<Shape>`; images, bodies and tables are owned;
   `SlideModel.bg_image` is an `ImageRef`. The skeleton's `ReadSet`/`PartGen` design is kept.
2. **`PartSource` takes `&self`** and the deck keeps its caches in `RefCell`s; `xml()` returns `R<Option<Arc<Document>>>`;
   `resolve_rel_target` is a free function (pure, the TS rule), not a trait method. The TS rule is kept on purpose: `uo-opc`'s D6
   `Package::resolve_target` is a different rule (R3 of the opc record) and parity needs the old one while `engine="ts"` exists.
3. **Messages are English**, XML error text is `uo-opc`'s: the replay compares errors by class (none occurs in the corpora).
4. **Per-slide memo of master/layout shape trees**: `readSlide` reads them up to five times; here each (part, rels) is read once
   per call (deterministic, behaviour-neutral) and decorations are cloned. No cross-slide `MasterRead` cache (M3 first).
5. `slideNumCache`/`chartFormatCache` are recomputed per call (O(slides) per slide read; one small part read per chart shape).
6. `SlideModel.timing` is `None` (B6); `readSlide`'s bookmark-trigger fix-up belongs to that frontier.
7. `.ppt` through `Deck::open` returns `PptxError::PptFormat` (feature `ppt` routes to `ppt::convert`, a placeholder for B2).
8. `chartXml` is not stored in the shape: `ChartRef.part` names it and the text is read through the part source (printed in full
   at every occurrence, as the TS).
9. EOT-embedded fonts: read since B5b (`eot.ts` → `uo_fonts::eot::read`, with MicroType Express, called by `fonts.rs` through `uo_dml_render::eot_to_sfnt`); a part that is no EOT is skipped as the TS does.
   B5b (2026-11-17, `uo-dml-render.md` "B5b — metafiles"): metafile / PICT / TIFF pictures are recipes the shell replays (not a display sub-list); `uo-image::decode_tiff_slides`; the draw-op comparison covers them (`drawops-b5b`, 162 / 162).

### Gaps left for later frontiers

B2 `.ppt` converter; B3 text layout (`layout.rs`, `measure.rs` skeletons untouched; they still name `ImageId`/`Span` of the arena
design: B3 replaces them by owned `ImageRef` clones); B4 emission and the media header size/dpi of `MediaEntry`; B5a SmartArt
regeneration/fallback (`regenerateDiagram`, `diagramFallbackShapes`, `diagramStyler`: a SmartArt without a cached drawing yields no
shapes here); B5b EOT; B6 `parseSlideTiming`; B8 the editor (`edit.rs` untouched). CI: `rust-core.yml` should run `cargo test -p
uo-dml-render -p uo-pptx` (the replay needs no flag) and list the fuzz targets `pptx_open`, `dml_shapes`.

## B2 — .ppt converter (2026-11-17, Sonnet)

Ported 1:1 into `core/crates/uo-pptx/src/ppt.rs` (module root, the skeleton's placeholder: `convert(bytes) -> RawParts` is kept
and now works, so `Deck::open` of a `.ppt` converts under feature `ppt`) and `src/ppt/*.rs`: `rd` (records.ts), `text` (text.ts),
`textconv`, `reader` (ppt-reader.ts), `oa` (officeart.ts: the shape arena, `readDgContainer`, `resolveShapeMasters`),
`shapeconv`, `roundtrip` (roundtrip.ts + the `xml.ts` tree it reads with), `anim`, `notes`, `transition`, `to_pptx`, `context`,
`xmlw`, `pict` (`fitPictFrame` of pict.ts) and `metafile` (`recolorMetafile` of metafile.ts). The CFB reader is `uo_opc::cfb` (the
three TS copies are identical: only the comments differ, checked by `diff`); the FOPT, BStore, BLIP and msospt → preset layers are
`uo_drawingml::odraw` with `Flavor::Slides` / `PresetVariant::WordSlides` (the two functions the odraw record named "to add at the
Slides port", `read_dg_container` and `resolve_shape_masters`, are in `ppt/oa.rs` over odraw's `Shape`, which keeps the records).
API: `ppt::convert_ppt_to_pptx_parts(bytes, &ConvertOptions) -> Result<Vec<(String, Vec<u8>)>, PptError>` (the parts in archive
order), `ppt::{read_ppt_document, ppt_to_pptx_parts}` (the reader and the writer apart), `ppt::{is_cfb, transition_xml}`;
`PptError::message()` is the text `convertPptToPptx` throws (the Portuguese `PptFormatError` messages are identical; the prefix
"Arquivo .ppt corrompido ou não suportado: " wraps what the TS wraps).

### Parity (the gate)

`tests/ppt_parity.rs` replays `tests/captures/ppt-samples.capture` (25 `.ppt`, one fresh TS module per deck):

| Facet | Compared | Equal | Mismatches |
|---|---|---|---|
| `input`, `convert`, `parts` (count + names + order) | 25 decks | 75 | **0** |
| `p<N>` (length + SHA-256 of every part) | **1,082 parts** | **1,082** | **0** |
| `b<N>` (the bytes of the 901 parts of at most 8 KiB, inflated from the capture) | 901 | 901 | **0** |

First run: 1,082/1,082 (0 parity rounds). Replay 0.2 s in release. What the 25 decks exercise (parts containing the tag):
`prstTxWarp` 209 (WordArt), `p:pic` 194, `p:transition` 294, `p:timing` 77, `p:bldLst` 79, `gradFill` 24, `a:fld` 32 (30 with the
automatic date), `p:graphicFrame` 7 (the round-trip SmartArt rebuild), `outerShdw` 30, `buAutoNum` 10, `blipFill` 4, `grpSp` 3,
`custGeom` 1, `cxnSp` 1, `hlinkClick` 1, 111 media/diagram/notes parts. **Not exercised by any deck** (ported with the TS beside
them, checked by reading only): `pattFill` (`patternPrst`), `duotone`, `scene3d` (`threeDXml`), `buBlip` (picture bullets),
`clrChange`/`lum` (`blipAdjustXml`), `recolorRel`/`recolorMetafile`, `complexImageRel`, PICT frames (`fitPictFrame`), the arc
geometry. A capture of `.ppt` decks saved by PowerPoint COM from generated decks (the "Next wave" of `uo-pptx-captures.md`) is the
way to cover them.

### Memory (M1–M3), measured by `uo-alloc` (`tests/ppt_memory.rs`)

- **M1**: `convert_ppt_to_pptx_parts` over each of the 25 samples, the reader alone, and the error classes (not a CFB, empty, cut in
  half, cut at 512 B, zero-filled body, 0xFF-filled body, a bare CFB header): live bytes return **exactly** to the start.
- **M2**: 60 conversions of the 8 smallest decks: the series is constant (11,694,865 B, tolerance 0). Nothing is global: the GUID
  counter, the paragraph-start memo, `partTextCache`, `metroCache`, the blip cache and the media maps are per conversion.
- **M3 budgets** (asserted: peak ≤ 10 × the input, allocated ≤ 50 × the input; worst case `pb-transicoes-variantes-3d.ppt`, 650 KB):
  allocated 4.1× (`pb-midia-2`, 846 KB of pictures that pass through) to 36.9× the input, peak above the start 1.3× to 8.0× (the
  peak holds the output, up to 0.56 MB, the three streams read out of the CFB and the part maps). The big ratios are the
  decks with many shapes and text: every `el()` builds its string (the TS does the same with template strings).

### Security

No panic or stack overflow: default run 5.6 s in debug (overflow checks on): 100 mutated files + 300 mutated streams
(`PowerPoint Document`, `Pictures`, `Current User` apart: random edits, 4-byte writes of `0`/`0x7FFFFFFF`/`0xFFFFFFFF`/`0x80000000`
at aligned offsets, cuts, zero/0xFF fills, block copies, bit flips); long run `UO_PPT_NOPANIC_ROUNDS=25`: 2,500 files (45.7 s) and
7,500 streams (142.8 s), 0 findings. Synthetic streams with containers nested 100,000 deep (tag lists, `SpgrContainer`,
`ExtTimeNodeContainer`) and a 4,095-entry FOPT of complex properties on 2,000 shapes: typed errors or bounded results.
Fuzz target `ppt_convert` (`core/fuzz/src/ppt.rs`, in `TARGETS`, 25 seeds + 5 with the round-trip layer off in `seed`): 9.6 M
executions in 60 s (fallback driver, release, overflow checks on), 0 crashes, slowest execution 49 ms; invariants beyond "no
panic": a conversion that works starts with `[Content_Types].xml`, has `ppt/presentation.xml`, has unique part names and gives the
same parts when run again; the reader and writer run apart from the container too.

### Decisions outside the source semantics (declared; none changes a byte of the 1,082 parts)

1. **The field counter starts at 1 per conversion** (the finding of `uo-pptx-captures.md`): `fieldSeq` of `textconv.ts` is
   module-global and never reset; here it is an `Rc<Cell<u32>>` of the conversion's context (`(n + 1) % 0xFFFF`, the first GUID is
   `…DAAF0001`), shared by the slides, the masters and the notes as in the TS.
2. **Today's date is an option.** The automatic date field writes `new Date()` (local time, `dd/mm/yyyy`) into the text of the
   field: 30 parts of the capture carry **2026-11-17**, the day it was taken, so the capture is date-bound. `ConvertOptions.today`
   (year, month, day) carries it: `ppt_parity.rs` pins 2026-11-17 (`UO_PPT_TODAY=2026-11-16` makes 30 parts differ, which proves
   the dependence); `None` takes the UTC date of the system clock on native targets and 1970-01-01 on wasm (no clock: the host must
   pass it). `capture-ppt.mjs` should freeze `Date` the next time it is re-run.
3. **Parts, not a ZIP.** The TS zips the parts and `PptxPackage.load` unzips them again; `convert` returns the parts in the same
   order and skips the round trip. One consequence: the load-time bomb filter (declared size ≤ max(1 MiB, 1,100 × compressed)) is
   not applied to the converter's own output (a part over 1 MiB that deflates 1,100:1 would be dropped by the TS).
4. **Text as UTF-16, the rest as strings.** Text blocks are `Rc<Vec<u16>>` (the TS indexes them by code unit: runs, links, fields,
   animation ranges, `metroTextBody`); shape names, hyperlink targets, footer texts and XML attribute values are strings in which a
   lone surrogate is U+FFFD, which is what `TextEncoder` makes of it when the part is written. `escText`/`escAttr` do the same for
   units (Write's `.doc` `xmlw` drops lone surrogates instead: not the TS bytes). Two relations whose targets differ only by lone
   surrogates would be one here (never seen).
5. **The round-trip XML tree is a port of `xml.ts`, not `uo_opc`'s lenient parser**: `metroTextBody`/`serializeXml` need the TS's
   attribute-record semantics and its error cases (`parseXml` throws where the output depends on it). The tree stops at depth 512
   (a JS engine throws `RangeError` at some thousands; real parts nest a few dozens; the bound keeps the recursion inside a 1 MiB
   wasm stack). Not reproduced: `&constructor;`-style entities (`ENTITIES[name]` finds the prototype's methods in JS).
6. **Bounds the TS does not have** (hostile input only): odraw's `max_shapes` (65,536 per tree) and `max_master_props` (1,048,576
   inherited properties in all, `resolve_shape_masters`); `ProgTags`/`List` nesting beyond 1,000 is `PptError::Internal("Maximum call
   stack size exceeded")` (what the TS's stack overflow becomes after the wrap); the text budget of 64 Mi characters is the TS's.
   A `DataView` read outside the buffer in `recolorMetafile` (a truncated record at the very end) is an error in the same places.
7. **Shapes live in one arena** (`Shapes`, indices) because `resolveShapeMasters` works on object identity across all drawings
   (`bySpid` keeps the first shape of a spid over every drawing, `done` is a set of objects); a text block copied with a spread is an
   `Rc` clone (the spread shares the big arrays; a block named by thousands of shapes must not be copied).
8. **Round-trip data keyed by the blob's address** (`partTextCache`/`metroCache` are `WeakMap`s keyed by object): a map keyed by
   (pointer, length) of the property's bytes; same results, per conversion.
9. **CFB error messages are `uo-opc`'s**, not the TS's ("Arquivo .ppt inválido ou corrompido: …" keeps its prefix); the
   `PptFormatError` texts of the converter are identical. None occurs in the 25 samples.
10. **Edits outside `src/ppt/**`** (agreed in the brief): `uo-pptx/Cargo.toml` (`ppt = ["dep:uo-drawingml", "uo-drawingml/legacy"]`,
    `uo-drawingml` as an optional dependency, `uo-testkit` as a dev-dependency), the `#[cfg(feature = "ppt")] pub mod ppt;` line of
    `lib.rs`, `src/ppt.rs` (the placeholder is now the module root); `core/fuzz` (`Cargo.toml`: feature `ppt` on `uo-pptx` and the
    `[[bin]]`; `src/lib.rs`, `src/bin/seed.rs`, new `src/ppt.rs` and `fuzz_targets/ppt_convert.rs`). `uo-opc` needed no CFB variant.

### Process metrics

| Stage | Agent | Time | Notes |
|---|---|---|---|
| B2 port: reading the TS (5,259 lines + pict/metafile/xml) and writing 17 Rust files (≈ 8,400 lines after rustfmt) | Sonnet | ≈ 50 min wall | parity 1,082/1,082 on the first run: **0 parity rounds** |
| Verification: parity, M1–M3, no-panic (default + long), fuzz, clippy, wasm | Sonnet | ≈ 15 min wall | replay 0.2 s, memory 0.4 s (release), no-panic 5.6 s (debug); long no-panic 189 s; fuzz 60 s |
| Rework | | | 1 compile round (5 errors: `PptDocument<'a>` is invariant through its `RefCell` cache, so the references are `&'a PptDocument<'a>`), 6 clippy lints, the capture's date and `b<N>` facets in the replay |

## Process metrics

| Stage | Agent | Time | Notes |
|---|---|---|---|
| Architecture pass (plan §4.0 Slides, §4.2 design, §4.5 boundary) | Opus | one session | records `uo-pptx.md`, `uo-dml-render.md`, `uo-render.md` "Slides extensions"; skeletons `uo-pptx`, `uo-dml-render` |
| B1b port (with B1a, one session) | Sonnet | one session; replay 0.4 s, memory 0.6 s, no-panic 1.6 s (release); long no-panic 23 s; capture re-run 2.5 s per file | the first replay of 253 decks was 100% equal (0 parity rounds); rework: 2 harness fixes (fuzz comparison on NaN / hash-map Debug, M1 tests that forgot to clear a cache), 3 lint fixes |
| B2 port (the `.ppt` converter, see "B2 — .ppt converter") | Sonnet | ≈ 65 min wall; replay 0.2 s, memory 0.4 s (release), no-panic 5.6 s (debug), long no-panic 189 s, fuzz 60 s | 1,082/1,082 parts byte-equal on the first run (0 parity rounds); rework: 1 compile round (invariant lifetime), 6 lints, the capture's date/`b<N>` facets in the replay |

## B6 — timing and playback (2026-11-17, Sonnet)

Ported 1:1 into `core/crates/uo-pptx/src`: `timing_parse.rs` (`parseSlideTransition`, `parseTimeNode` over an explicit stack,
`parseSlideTiming` with the main sequence steps, interactive and media-bookmark sequences, media nodes, `animBg`),
`timing_eval.rs` (`beginOffset`, `simpleDur`/`blockDur`/`activeDur`, `collectEffects`/`collectGroup`/`stepFrom`),
`anim_formula.rs` (`evalFormula` + the capped parse cache), `anim_engine.rs` (accel/decel, `tmFilter`, progress, colours in RGB and
HSL, motion paths + cache, every behaviour, `shapeAnimHooks` as `ShapeHooks`: `all`/`bg`/`para`/`unit`/`chartParts`),
`slots.rs` (`evaluate_slots` over a `SlotTemplate`), `playback.rs` (`placeSlideEffects`, `clickWait`,
`buildPlaybackTimeline`, `playbackStateAt`, `shapeOpacityAt`), `show.rs` (`SlideShow`, `mirrorTransition`), `media.rs`
(`mediaCommands`, `mediaStateAt`, `mediaFadeAlpha`, `gifFrameAt`, plus the viewer's `showWhenStopped` opacity). `transition.ts` has no
state or timing code (only `paintTransition`, which stays in the shell, driven by `SlideShowFrame`/`PlaybackState`); the timing facts it
needs (`instantTransition`, `mirrorTransition`) live in `playback`/`show` as in the TS. Touched outside the new files (additive):
`lib.rs` (modules), `read.rs` (`read_slide_with` takes the presentation `Limits`; `readSlide`'s `parseSlideTiming` call and the bookmark
fix-up `timeMs`/`trimStartMs`), `deck.rs` (the `read_slide_with` call, `Deck::timing_models`), `model.rs`
(`SlideModel.timing: Option<Arc<TimingModel>>`, shared with the timeline and the show), `canon.rs` (`timing_json`,
`slide_record_with_timing`), `anim.rs` (the skeleton `SlotKind`/`SlotValue` moved to `slots.rs`), `tests/replay.rs` (the `s<N>`/`model`
facets enabled), the harness (`js/capture-timing.mjs`, `js/lib/gen/b6.mjs`, `groups.mjs`, `PartCtx.addRel` in `b1.mjs`), `core/fuzz`
(`src/timing.rs`, two `[[bin]]`, one line each in `lib.rs`/`seed.rs`).

### API

```rust
parse_slide_timing(root: Element, &TimingInputs { units, color, has_background }, &Limits) -> TimingModel   // read_slide calls it
build_playback_timeline(&[Option<Arc<TimingModel>>], &PlaybackOptions) -> PlaybackTimeline; playback_state_at(&tl, t) -> PlaybackState
SlideShow::new(models, from, now, opts); next(now, mouse) prev go_to first last update(now) -> SlideShowAction; frame(now) due_at wake_at
shape_anim_hooks(&[PlacedEffect], &TimeArena, ids, anim_ms, &ShapeAnimContext) -> Option<ShapeHooks>
evaluate_slots(&SlotTemplate, &PlaybackSlide, anim_ms, &MediaClock, &mut AnimCaches) -> SlotTable     // one SlotVal per template slot
```

No call reads a clock: every time is an argument (`now`, `t`, `anim_ms`). A `PlaybackSlide` carries `Arc<TimingModel>` (the node arena the
placed effects point into), so a frame of the show is self-contained. The two caches (`pathCache` 256, formulas 1,024) are an
`AnimCaches` the caller owns (the deck view in B7) and empties with `clear()`.

### Parity (the gate)

`tests/timing_replay.rs` over `js/capture-timing.mjs`; `tests/replay.rs` for the model facets (`s<N>`/`model` now include `timing`).
Generated group `b6a` (100 decks, 202 slides; seeds 600000+, `js/lib/gen/b6.mjs`): every effect class (entrance 15 kinds, emphasis 16,
exit 6, motion paths 7 forms, media commands), click/with/after, repeat, auto-reverse, accel/decel, `spd`, `tmFilter`, fill, "until next
click", iterate by element/word/letter (forward, backwards, `tmPct`/`tmAbs`), build by paragraph, `animBg`, chart and SmartArt targets,
trigger and media-bookmark sequences, video/audio with trim/fade/bookmarks, every transition kind (21 `p:`, 19 `p14:`,
`p15:prstTrans`, `p159:morph`, a duration outside fast/med/slow, `sndAc`), hidden slides.

| Capture | Decks | Facets compared | Equal | Mismatches |
|---|---|---|---|---|
| `model-b6a.capture` (model facets with `timing`) | 100 | `input read pres slides n<N> s<N> nmodel model` | 1,004 | **0** |
| existing model captures, `s<N>`/`model` newly replayed | b1a 200, samples 28, b5a 150 | same | 2,200 / 807 / 1,340 (were 1,500 / 472 / 970) | **0** |
| `timing-samples.capture` + `.frames.gz` (28 `.pptx` samples incl. `playback/`) | 28 | `t<N> tl.a-d st.a-d hk sh.a-d fr` | 783 (+ 8,060 hook calls) | **0** |
| `timing-b1a.capture` | 200 | same | 3,900 (+ 9,758 calls) | **0** |
| `timing-b6a.capture` | 100 | same | 1,902 (+ 76,609 calls) | **0** |
| `timing-formula.capture` (1,740 formulas x 3 variable sets) | - | every result by bits | 5,211 of 5,220 | 9, declared (below) |
| `fn/media` (`accelDecel`, `gifFrameAt`, `mediaFadeAlpha` grids) | - | digests | 3 of 3 | **0** |

Facets: `t<N>` the model of slide N (`SlideTimingModel` canonical JSON); `tl.<v>` the timeline of four option sets (default; video export
tick 33 ms + 30 fps transitions; no timings; 60 Hz tick) with every placed effect; `st.<v>` `playbackStateAt` at up to 80 sampled times
with `shapeOpacityAt` of every target and `mediaStateAt` on two media clocks; `hk` every effect through `shapeAnimHooks` at six times
over a fixed base (all/bg/para/unit/chart); `sh.<k>` four random `SlideShow` scripts of 40 commands (default, no timings, no animations,
loop) with `index/step/ended/dueAt/wakeAt/frame` and the placed effects after each; `fr` the **slot recording**: `paintSlideTo` of the
frozen painter over the recording canvas at sampled frames of timelines `a`/`b` and of show script `a` (6,400 frames), every hook call
(`all`/`bg`/`para`/`unit`/`chartParts`: base in, `PartState` out, every field by bits) and the media `shapeOpacity`, as the viewer wires
them; Rust re-runs each call from the recorded base and compares the output tokens (the `fr` digest is of the Rust-produced lines).
**Mismatches left: 0 in every capture.** The 9 formula differences are the declared bound cases (three formulas x three variable sets): a
150-level parenthesis nest, 200 unary minus signs and `sin(` nested 130 times: the TS evaluates them, Rust returns NaN (bounds below).
Mutation check: changing `AFTER_LINK_MS` to 2 made 1,093 of 1,902 b6a facets fail, so the replay does discriminate.

### Memory (M1-M5), measured by `uo-alloc` (`tests/timing_limits.rs`)

- **M1**: the parse (document + model), timelines of two option sets, states, `shapeOpacityAt`, `mediaStateAt`, `evaluate_slots` (with
  `AnimCaches::clear`) and 80 `SlideShow` commands return live bytes **exactly** to the start. (The first run found 180 B: `clear()` kept
  the maps' capacity; it now replaces them.)
- **M2**: a slideshow of 1,000 frames (`UO_PPTX_TIMING_SOAK` to lengthen) over a b6a deck with per-letter/word effects, motion paths and
  media, a full slot evaluation (one shape slot + background + 3 paragraphs + 6 units + chart part per animated shape, 72,228 slots)
  every frame, restarted every 400: live bytes sampled every 50 frames are flat (within 64 KiB; in practice constant after the 3rd
  sample, the caches being capped); 41 ms in debug.
- **M3**: a slot evaluation allocates at most **10,232 B** per frame (budget 64 KiB) over the busiest slides.
- **M5**: the formula cache holds 1,024 entries and the path cache 256, tested past their caps (3,000 formulas, 600 paths) with other
  caps (16 / 8 / 1) and always the same bits as the uncapped result; both hold nothing after `clear()`.

### Security

Bounded, no panic (`hostile_*` tests): a 60,000-deep `<p:par>` nest (dropped below depth 256), 100,000 siblings with a 20,000-node cap,
48 combinations of `repeatCount` (`1e30`, `indefinite`, `NaN`, negative...) x `dur` (`indefinite`, `1e308`, `NaN`, `1e-320`...) with
`autoRev`, `spd`, `accel=99999999` and `<p:iterate><p:tmAbs val="1e999">` driven through timelines, states, hooks with a `u32::MAX` unit
and a slideshow on NaN/infinite clocks, a hand-built model with NaN durations/keyframes/ranges everywhere, 13 formula bombs (200,000
`(`, 1 M `-`, a 10 MB number, 300,000 arguments...) and six motion-path bombs (100,000 `C` commands, 400,000 `Z`...), and random command
sequences on 24 shows with `now` going backwards, `1e6` and NaN. Fuzz targets `slide_timing` (structure-aware: limits + wrapping byte,
hostile `<p:sld>` content, then timelines, hooks, slot table and a driven show; invariants: caps hold, ids resolve, the same canonical
text twice) and `anim_formula` (NaN or finite, an infinity only for a numeric literal like `1e999`, which the TS also returns; the same
through three caches): 2.84 M executions in 45 s and 0.94 M in 40 s on the stable driver, 0 crashes (the first formula run flagged
`1e999`: the invariant was too strong, not the evaluator).

### Decisions outside the source semantics (declared)

1. **Time-tree bounds** (`Limits::max_time_depth` 256, `max_time_nodes` 1,000,000): a container that would pass either bound, with its
   subtree, is dropped as if it were not a time node (the TS recursion overflows its stack). `read_slide_with` passes the deck's limits;
   `read_slide` uses the defaults.
2. **Formula bounds**: text over 4,096 characters, more than 1,024 nodes, parenthesis/sign/argument nesting over 128 or a tree deeper
   than 512 is "not a formula" (NaN). The TS has none; about 130 nested `sin(` overflows its stack. Names of `Object.prototype` used as
   variables (`constructor`, `toString`...) are NaN here; in JS they are functions whose arithmetic is NaN except `^0` = 1.
3. **Motion paths**: text over 65,536 characters is the empty path; at most 262,144 sampled points (the TS has no cap).
4. **Transition speed** `spd` outside `fast/med/slow` is 500 ms (a `spd` named like an `Object.prototype` member gives a function in
   the TS).
5. **Caches are capped FIFO** (formulas 1,024, paths 256) and owned by the caller; the TS caches grow without bound. Neutral: a miss
   re-parses.
6. **Composition, not spread**: `PlacedEffect { fx: EffectTiming, t0, step, end_at }`; `EffectTiming.node` is a `NodeId` in the model's
   arena (the timing capture prints `[tag, id]`; the model JSON prints the whole subtree, as the TS does).
7. `evaluate_slots` has no `view` argument: the bases are already normalized in the template (per viewport, `uo-render.md` S6);
   `shapeOpacity` (media `showWhenStopped`) is part of the shape slot's value (`SlotVal::State { op, .. }`, `Hidden` when `op <= 0`).
8. `SlideShow::new(from: f64)` keeps `Math.trunc(from) || 0`; `go_to` takes an `i64`; a frame owns `Arc`s (clones are pointer copies).
9. Hand-built models (not reachable from the parser) get guards where the TS would throw a `TypeError` (empty `tmFilter`/`tav` lists).

### Slot schema and what B7 needs

- **No `slots.uos` change is required.** (B4b then added `Replacement.values` and the slot kinds `UnitGate`/`BoundsPass`: `uo-render.md`
  "B4b schema changes".) The wire `SlotDef`/`PartState` carry what the painter reads. What the evaluation needs beyond
  it (`AnimBase.fill/line/text` for `animClr` without `from`, the shape's id + group ids, `dgmId`, `has_media`, `inherited`, the five
  integers of a `UnitRef` and the word's) stays in the Rust-side `SlotTemplate` (`slots.rs`) that the emitter fills next to the encoded
  list. If B4 wants them on the wire: `SlotDef.index: u32` cannot hold a `UnitRef`, and `base_rot_rad` is not `AnimBase.rot` (the TS
  passes degrees, `xfrm.rot / 60000`; keep the degree value).
- B4 fills `SlotTemplate { shapes: [SlotShape { ids: [id, ..group ids], dgm_id, has_media, inherited, has_xfrm }], slots: [SlotSpec {
  kind, shape, base, word }] }` at emission (bases exactly as `boxBase`, text bases at device scale) and re-emits the shapes whose
  `PartState::needs_reemit()`.
- B7: the wasm handles are `Deck::timing_models()` -> `build_playback_timeline`/`SlideShow` (state in Rust, `now` from the shell), the
  frame's `PlaybackSlide` + `anim_ms` -> `evaluate_slots` -> `SlotTable` -> `SlotFrame`; the shell keeps `paintTransition` (reads
  `PlaybackSlide.transition` and `transition_progress`), the media elements (`media_state_at`, `gif_frame_at`, `media_fade_alpha`) and
  the decode of GIF delays; `AnimCaches` lives in the deck view (`clear()` with `Deck::clear_caches`).

### Process metrics

| Stage | Time | Notes |
|---|---|---|
| Port (about 2,700 TS lines into `timing_parse`, `timing_eval`, `anim_formula`, `anim_engine`, `slots`, `playback`, `show`, `media`) | one session | every capture equal on the first run (**0 parity rounds**) |
| Harness: `b6.mjs` generator, `capture-timing.mjs` | one session | captures: samples 1.6 s, b6a 10.3 s, b1a 21.9 s, formulas 1.5 s; the 100 decks regenerate in under 2 s, byte-equal (`--check`) |
| Verification | - | `timing_replay` 4.2 s, `timing_limits` 1.2 s, `replay` 5 s (debug); whole `uo-pptx` suite 74 s (debug, 4 threads); fuzz 45 + 40 s stable driver; clippy native + wasm32 clean |
| Rework | - | M1: `clear()` kept map capacity (180 B); the formula parser's nest counter counted parentheses twice (64 levels instead of 128) and shared its bound with the tree depth (a 256-term sum was NaN): split into `MAX_DEPTH`/`MAX_TREE_DEPTH`; the fuzz invariant on `1e999` |

## B4b — playback emission: slots, frames, re-emission (2026-11-17, Opus)

The animated slide as a display list with slots, replacing the TS path of `paintPlaybackSlide` (`shapeAnim`, `shapeOpacity`,
`mediaFrame` over `paintSlideTo`). New: `src/stage.rs` (this crate), `uo-dml-render/src/emit/anim.rs` (the emitter side, record
`uo-dml-render.md` "B4b"), the wasm exports `uo_slides_stage_show` / `uo_slides_stage_timeline` / `uo_slides_stage_clear`
(`uo-bindings.md` "Slides viewer (B7)", "B4b"), the painter rules in `paint-slides.ts`, and the viewer wiring.

### Design

- **The structure is static per slide, the values per frame.** Which TS hooks exist for a shape (`hooks.bg`, the paragraph ranges of
  `hooks.para`, `hooks.unit` letters/words and its ranges, `hooks.chartParts`) depends only on the placed effects, not on the time, and
  it changes what the TS draws (a paragraph with a state draws its runs without the fit spacing, a body entering by unit is drawn piece
  by piece). `ShapeHooks::plan()` returns that structure (`uo_dml_render::emit::ShapeAnim`); `stage::anim_plan(ps, shapes)` builds it for
  every shape of a `PlaybackSlide` as the viewer's `shapeAnim` does (none for an inherited shape, a shape without `xfrm`, a slide
  without effects). `emit_stage` emits the slide once with it (`emit_slide_anim`) and keeps the `SlotTemplate` (`template_of`: every
  emitted slot's base exactly as `boxBase`, the shape facts the hooks need) next to the list: a `Stage`.
- **A frame** (`stage_frame`) is `evaluate_slots` over the stage's template. A state with a field `applyPart` does not apply
  (`PartState::needs_reemit`) re-emits that shape alone (`emit_shape_anim`) with a `SlotStates` that evaluates each slot the emitter
  writes as `evaluate_slots` would (`SlotEval`, the per-slot evaluator split out of `evaluate_slots`), so `fillOv`/`lineOv`, `fontOf`,
  `colorOf`, `underOf` and the font scale of the layout are the TS ones; the replacement has its own slots and values (a font scale
  moves the paragraph and unit boxes).
- **Still lists are unchanged**: no slots, a plain `save`/`restore` per shape (the PDF and the thumbnails read them). Only a playback list
  puts every shape in its shape slot.
- The stage is reused while the slide, the viewport, the media flag and the hooks' structure are the same; a newer placement (steps
  fired) only replaces the `PlaybackSlide` the values are evaluated over.
- `media` is the viewer's `hasMedia`: without it (a frozen transition origin) no `MediaFrame` items and no `shapeOpacity`
  (`template_of(.., media)` clears `has_media`; found by the Chromium harness: the show's origin hid a `showWhenStopped` picture).
- The `chartPartBounds` probe of the TS (a recording proxy that forwards `save`/`restore`, `clip`, `roundRect`, `drawImage` and the
  state sets to the real canvas on a bounds-cache miss) is a `BoundsPass` gate: emitted when the emitter's bounds cache misses (the
  boxes computed in a pass are committed only by a complete pass, so the decision is the same in every measurement round), drawn only in
  the first frame of the stage (`stage_frame(first)`), as the TS cache holds the boxes afterwards.

### Parity (the gate)

TS side: `js/capture-anim.mjs` paints the slides of the default playback timeline at sampled animation times with the frozen
`paintSlideTo` and the viewer's hooks (`shapeAnim`, `shapeOpacity`, and a deterministic `mediaFrame` stub: nothing, an opaque frame
or a half-faded one by `(shape + frame) % 3`) over the recording context. Rust side: `uo-wasm/tests/slides_anim.test.mjs` builds the same
timeline in `uo-slides.wasm`, calls `deck.stage` at the same `t`, decodes the stage list, the `SlotFrame` and its replacement lists,
paints with `paint-slides.ts` (same stub) and compares the draw form line by line, exact; then the same frames through ONE view (the
stage reused) must give the same streams (except the first-frame chart probes).

| Capture | Decks | Frames | Equal | Reused stage equal | Frames with re-emitted shapes | Slots |
|---|---|---|---|---|---|---|
| `anim-b6a` (committed, 3 times per slide) | 96 | 472 | **472** | 472 | 125 | 16,359 |
| `anim-samples` (committed) | 16 | 551 | **551** | 527 + 24 without the first frame's chart probes | 27 | 5,197 |
| `anim-b1a` (committed, 60 decks, 2 times per slide) | 60 | 150 | **150** | 150 | 8 | 1,604 |
| `anim-b6a` full (local, 6 times per slide) | 96 | 1,015 | **1,015** | 1,015 | 328 | 36,471 |
| `anim-b1a` full (local, 160 decks) | 160 | 1,462 | **1,462** | 1,462 | 133 | 15,667 |

The static gates are unchanged: `drawops-samples` 1,202/1,202, `drawops-b1a` 246/246, `drawops-c1d` 182/182 (wasm), `drawops-b5b`
162/162 (native `emit-uolf` + `compare-drawops`). No committed capture was regenerated; the three `anim-*` captures are new.
Chromium (`slides_switch.mjs --mode anim`): 121 animated decks (the playback samples, 40 `b6a`, 40 `b1a`), `play({ media: 'seek' })` at 13
instants and `present()` driven by `next()` with the clock stepping (0 / 180 / 420 / 900 ms) through 12 steps: **5,089 frames, 5,089
pixel-equal** between `engine="ts"` and `engine="rust"` (5,254 stage paints by the Rust list), 0 errors; the TS-against-TS control of the
same harness is 1,993/1,993 on the playback samples (the harness first had `play()`'s real-clock start in it: fixed, `t0` on the virtual clock).
The CI form (`--mode all`, `UO_SS_DECKS=100`, final tree): phase 0 clean; still parity 213 decks / 959 slides pixel-equal (981 M pixels, main canvas
617, thumbnails, notes, a11y: 0 different); anim **181 decks, 7,221 frames, 7,221 pixel-equal** (7,246 stage paints by the Rust list); memory and
faults green.

Rounds found by the harness and fixed in this session: the soft-reveal layer collided with a shape's own effect layer right after its
slot (the painter now knows the soft layers by reserved ids 15/14 and composites the reveals only on them); a slot frame symbol was written
as the 1-based id; the list of a stage that needed measurements was encoded and not delivered (now encoded only with a complete frame);
the chart probes (above); a letter of an astral character is half a surrogate pair in the TS (pieces now carry UTF-16 units); the media
flag of the transition origin (above).

### Memory (M1-M3), `tests/stage.rs` (uo-alloc)

- **M1**: stages and frames (with re-emission) of three `b6a` decks, then `AnimCaches::clear`, `EmitCaches::clear`, `Deck::clear_caches`:
  live bytes back exactly.
- **M2**: 400 frames over the stages of a deck (one evaluation per frame, stages reused): flat within 64 KiB after warm-up.
- **M3**: the busiest frame WITHOUT re-emission over six `b6a` decks allocates **28,276 B** (peak 23,461 B above the start); budget
  64 KiB. (B6's synthetic template measured 10,232 B; these are real slides with up to 300 slots.)
- In the module (Chromium): 300 playback frames of a `b6a` deck, live bytes 251,720 at every sample (spread 0), 0 handles and 0 live bytes after
  the element left.

### Security and fuzz

`tests/stage.rs` `hostile_frames_do_not_panic` (NaN / infinite / huge animation times, absurd viewports). Fuzz: `slides_emit` now also emits
every input as a playback list with a hostile `AnimPlan` and re-emits each shape with hostile states (NaN, negative and 1e9 font scales,
invalid colours, empty font style), invariants in `core/fuzz/src/slides_emit.rs`; `slide_timing` evaluates the new slot kinds
(`UnitGate`, `BoundsPass`). Stable driver, 40 s each: `slides_emit` 16,886 executions (422/s: two playback emissions plus one
re-emission per shape for each input), slowest 126 ms; `slide_timing` 864,150 executions; **0 crashes**. The wasm exports refuse a bad
`which`, a NaN/negative scale and null handles (`slides_api.test.mjs`, the Rust unit test of `src/slides.rs`).

### Decisions outside the source semantics (declared)

1. A unit piece that is half of a surrogate pair is measured as U+FFFD (the measure key is UTF-8, as B3-3); its drawn text keeps the
   UTF-16 unit. A browser draws and measures a lone surrogate as U+FFFD, so only the deterministic stub can tell.
2. `MediaFrame` items carry the poster's draw; a media picture whose poster is not decodable draws nothing (the TS would draw an opaque
   video frame there with the picture's shadow and clip). The poster is part of every `p:pic` with media.
3. A soft-revealing shape (or chart part) is written twice (the soft layer and its else block), and so are its slots; the values are
   the same.
4. `globalAlpha` absolute restores inside a slot (`ctx.globalAlpha = prevAlpha` of the outline) are relative to the alpha the slot's value
   multiplied in (`paint-slides.ts`): exact with one static factor, as every case in the captures.

### Process metrics

One session (Opus). 0 parity rounds after the first complete run except the six harness findings above (each a code fix, none a
comparison change). Builds `-j 4`: wasm `count,slides` 20-40 s incremental; captures: anim-b6a full 25 s, b1a full 55 s; `slides_anim` 2 min;
Chromium anim 190 s; `tests/stage.rs` 0.5 s.

## B8 — editor session (2026-11-17, Sonnet)

Ported 1:1 into `core/crates/uo-pptx/src/edit.rs` and `src/edit/*.rs` (feature `edit`): `xml` (`xml-edit.ts`: string scans, byte offsets,
`RegExp`s as hand matchers), `pkg` (`deck-edit.ts` parts, relations, content types, slide list, layouts, `addSlide`, `duplicateSlide`,
`deleteSlide`, `moveSlide`, background, transition), `shapes` (insert, frame, fill, outline, order, delete with its animations, text, character
and paragraph formatting, bullets, link, replace, and the C# `PptxManipulate.SetRunText`), `doc` (notes, properties, "Set up show", the shape
clipboard), `blank` (`blank-deck.ts`, parts byte-equal), `fonts` (`deckFontUsage` and `readPptxEmbedding`/`writePptxEmbedding` of `shared/fonts`
over the part store; the plan is `uo_fonts::plan_font_embedding`), `random` (`Math.random` of `freshCreationId`), `session` (`Editor`: the
`edit()`/`undo()`/`redo()` of `universal-slides.ts` without the DOM) and, in `edit.rs`, `history.ts` with the byte cap. The 46 exported functions of
`deck-edit.ts` are all here, plus `set_run_text`.
Additive edits outside `edit*`: `Deck::parts_mut` (deck.rs); `Cargo.toml` (`edit = [dep:uo-fonts, dep:uo-text]`, and the self dev-dependency enables
`edit`, so `cargo test` needs no flag); `package.rs` and `lib.rs` unchanged (`PartStore::snapshot`/`PartsSnapshot` and `pub mod edit` were there).

### API

`Editor::{new, open, blank, apply(Op) -> Outcome, undo, redo, select, set_index, copy_shape, find_text, default_new_layout, font_requests,
save_with_fonts(faces) -> SavedPackage, save, is_dirty, mark_clean, set_random_seed, set_text_lang}`. `Op` has one variant per mutating method of the
element (the arguments the element resolves from its UI state: slide index, shape id), with the guards of the wrappers (a missing slide is a no-op).
The editor owns the UI state the history saves (current slide, selected shape id); the shell mirrors it after each call.

### Parity (the gate)

`js/capture-edit.mjs` runs seeded scripts on the FROZEN TS operations (`deck-edit.js`, `history.js`, `blank-deck.js`, `writePptxEmbedding` from the
built `dist/` of the 0.9.2 checkout) under a `Session` that repeats the `edit()` sequence of `universal-slides.ts` (snapshot, operation, `changedSince`,
re-read, `selectById`, `history.push`; undo/redo with `restore`) and the wrappers' guards and defaults (the DOM is out: this is not the real element in
Chromium). `Math.random` is the xorshift32 of `Xorshift32` (seed 1 per script), `Date` is fixed. After EVERY step (operation, undo, redo, selection) the
capture keeps the digest of all part bytes in order, the UI state with the history flags and labels, the presentation model, the model of the current
slide, and what the operation returned. The scripts are generated in the harness and written to a sidecar (`.ops.gz`, one line per step), so the replay
has no generator. A second sidecar (`.parts.gz`) lists the parts each step changed (`UO_EDIT_DUMP=<key>#<step>` prints Rust next to TS).

| Capture | Scripts | Steps | Equal |
|---|---|---|---|
| `edit-samples` (the 28 tracked `.pptx`) | 28 | 1,680 | 100% |
| `edit-b1a+b6a` (35 + 35 generated decks: animations, tables, groups, pictures) | 70 | 4,200 | 100% |
| `edit-blank` (4 blank-deck variants byte-equal part by part, then 12 scripts of 120 steps from them) | 12 | 1,440 | 100% |
| `edit-fonts` (save with the font option: plan, subsets, EOT, `embeddedFontLst`; saves compared by the parts of the reopened file) | 25 | 1,376 | 100% |

Mismatches left: none. The captures total about 2 MB (limit 10). Every operation, undo, redo, copy/paste inside a deck and from another deck
(`xpaste`), replace, notes (creating the notes master), properties, show settings, transitions and the slide-list operations occur many times.
`tests/edit_units.rs` pins the primitives (`tag_end` with quotes, nested elements, the `$` replacement patterns, entities, part names) and the C# test of
`SetRunText`.

### Memory (M1, M2, M5), `uo-alloc` (`tests/edit_memory.rs`)

- M1: each of 28 operations (two of them error paths), undo, redo and `save` return live bytes exactly to the start once the editor is dropped; so do the
  blank deck and the save with fonts.
- M2: soak of 24 sessions (open, 100 random steps with undo/redo, undo everything, redo everything, save, close): flat within 64 KiB.
- M5: the history keeps the 100-step limit of the TS and a byte cap (`Limits::max_history_bytes`, 64 MiB): the bytes held BEYOND the live package
  (distinct `Arc`s counted once; parts shared with the live package cost nothing) are bounded by the cap; past it the oldest steps go as when the limit is
  exceeded; the newest step is always kept (one step larger than the cap stays undoable). Tested with 40 KiB and 100 KiB part versions.

### Security

`tests/edit_nopanic.rs`: random scripts (hostile arguments: NaN and huge frames, markup in texts and ids, unknown layouts) over decks whose XML parts are
truncated, cut, duplicated, flipped and spliced with hostile tokens; a failed operation leaves the package as it was. Fuzz target `slides_edit`
(`core/fuzz/src/slides_edit.rs`; invariants: a failed operation changes nothing, undo then redo restores the bytes, the saved package reopens)): 19,343 executions in 90 s with the stable driver, 0 crashes (the first run found a harness assumption: an insert on a deck without slides adds one first, as its own step).

### Decisions outside the source semantics (declared)

1. **A failed operation restores the snapshot** (the TS lets the exception escape with what it had written and no history step). Errors are `EditError`
   (a `!` on a missing element is `EditError::Js`).
2. **Byte offsets** where the TS has UTF-16 offsets; every offset is produced by a scan for ASCII tokens, so both select the same text. The UTF-16 sites:
   `hex()` (`slice(0, 6)`/`padStart` by units), the 79-character cap of a name in `elementName`/`childRanges` (the 80-unit window), `nextPartName`
   (the `i` flag compares canonical UTF-16 units: `uo_text::regexp_canonicalize`), `replaceInShape` (matching and case folding per unit), `copyPartDeep`
   without a `/` (`slice(0, -1)` drops a character). Known divergence: a close tag written with a NON-ASCII white space, where `end - '</name>'.length`
   lands differently (floored to a character boundary, never a panic).
3. **`unescapeXml`**: a code point above U+10FFFF (the TS throws `RangeError`) or a lone surrogate becomes U+FFFD.
4. **`Math.random`** is a seeded xorshift32 (`set_random_seed`); only `freshCreationId` consumes it, and only when the id is found.
5. **Regexps built from user strings**: `nextPartName`'s `base` and `dropAnimationsOf`'s shape id are taken literally (the TS interpolates them
   unescaped); `replaceInShape`'s query is escaped by the TS, so it is literal there too. The `$` patterns of `String.replace` ARE kept in `setAttrInTag`
   (the TS bug is behaviour); they are not kept for the font entries of `writePptxEmbedding` (a family name containing `$&`).
6. **`copyPartDeep` has a budget** (depth 64, 20,000 parts): the TS recurses without a bound (a cycle of non-shared relations overflows its stack).
7. **The save with fonts runs over a clone of the part store**, not over `PptxPackage.load(this.pkg.save())`: the zip round trip is lossless except for the
   decompression-bomb filter of `load`.
8. **Selection is the shape id** (the TS keeps an index into `slides[index].shapes`): `select` accepts only a non-inherited shape of the current slide, the
   rule of `selectById`. `Typing` finds the shape by id among the non-inherited ones.
9. **Model-dependent wrappers**: the default frames of inserts, the image natural-size fit, `growFont`'s steps, `defaultNewLayout` and `findText` are in
   `Editor` (pure over the model); `getTextFormat` (the base size of `growFont`) and the image decode stay in the shell, which passes `base` and `size_px`.
10. The `.ppt` samples are not scripted (opening converts first; B2 has its own gate).

### Routing behind `engine="rust"` (not done in this part): what the shell needs

- **wasm exports** (`uo-wasm`, one `Editor` per element, in the SAME instance as the viewer's `Deck`: the emission reads `editor.deck_mut()`):
  `editor_open(bytes)`, `editor_blank(opts)`, `editor_apply(op)` returning the `Outcome` (the `Op` needs a binary codec like the display list: a schema
  `edit.uos` with the 28 variants, `Frame`, `RunFormat`, `TransitionSpec`, `DocProperties`, `ShowSettingsSpec`; images travel as bytes), `undo`/`redo`
  returning the `EditState`, getters (`index`, `selected`, `can_undo/redo`, `undo_label/redo_label`, `is_dirty`), `set_index`, `select`, `slide_paths`,
  `copy_shape` (a clip serialised to bytes: XML, relations, image bytes, `source_id`), `find_text`, `default_new_layout`, `font_requests` and
  `save_with_fonts(faces)` (the two-phase protocol of Write: the host answers each `FaceRequest` with the font file; the `SavedPackage` report is the
  `us-font-embedding` event), `set_font_embedding`, `save`, `mark_clean`, `set_random_seed` (from `crypto.getRandomValues`), `set_text_lang`.
- **Inline text overlay**: the contenteditable stays in the shell. `commitEditShape` sends `Op::Typing { slide, shape, text }` with the text normalised as
  now (`\r\n` to `\n`, one trailing `\n` removed); the comparison with the current text and the shape lookup are inside `apply`. The shell must commit before
  every other operation, undo, redo and save (the TS `edit()` does it for every label but `typing`); a cancel sends nothing. After each call the shell reads
  the state (`index`, `selected`); the changed slides are re-read lazily by the deck (a part generation changed).
- Events (`us-change`, `us-slide`, `us-dirty`, `us-selection`) stay in the shell, derived from the `Outcome` and the state before and after.

### Process metrics

| Stage | Time | Notes |
|---|---|---|
| Port (about 1,800 TS lines + the C# `SetRunText`) | one session | the four captures were equal on the first complete run after one harness bug (the language token of the script header): 0 parity rounds on the operations |
| Capture generation (TS side) | samples 157 s; see the session log for the others | 7,300 steps |
| Verification (debug, 4 threads) | replay: samples 70 s, generated 47 s, blank 11 s, fonts 118 s; memory 8 s; no-panic 18 s; units 0.1 s | clippy native (all targets, all features) and wasm32 (lib) clean, `cargo fmt`, `grep-bans.sh` |
| Rework | - | the history byte cap first counted the live package (it left one step); `Typing` found inherited shapes by id; 3 lints |
